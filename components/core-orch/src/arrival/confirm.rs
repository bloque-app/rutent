//! Core's question about an arrival, for the order it is held against: whether the account holds what arrived,
//! asked of the reserve under a fresh transaction for that order, signed with the operator's key and appended about
//! the order and the transaction before it is sent, its answer appended with that transaction in its `about`, so the
//! log binds the answer to the order before the answer exists (`REQ-ORD-32`, `REQ-ORD-38`). The answer is the
//! reserve's report an `arrival` naming the order rests on (`arrival-rests-on-the-reserves-report`).
//!
//! provider-role has no question asking a reserve what arrived for an order, nor a result naming the node, asset and
//! amount of a credit; the nearest is a balance verification at the amount that arrived (`INV-9`: yes or no). It is
//! a balance check (`REQ-CAT-4`), so it goes only once the catalogue's `begin_check` lets it, in the session the
//! order's reserve is asked in, for the order's own account, and the catalogue reads its answer (`answer_check`).

use std::collections::BTreeSet;

use rutent_contracts::core_internal::{About, Actor, ActorKind, Entry, Receipt, Role};
use rutent_contracts::provider_role::{
    BalanceVerification, Denomination, OPERATION_SHAPE, Operation, ResultBody, Signature,
    SignatureScheme, Statement, signing_input,
};
use rutent_contracts::router_api::{Id, PositiveAmount};
use rutent_prov::catalogue::{Catalogue, CheckRefusal, CheckTicket, Publisher, Verdict};

use super::{ArrivalError, journal};
use crate::intake::Desk;

/// What Core asks about: an arrival's account, asset and amount at its reserve, for one order, in one session.
pub(super) struct Asking<'a> {
    pub order: &'a Id,
    pub reserve: &'a Id,
    pub account: &'a Id,
    pub session: &'a Id,
    pub asset: &'a Denomination,
    pub amount: &'a PositiveAmount,
}

/// One question about an arrival, kept from when it is built until its answer is read, so a call that stopped is
/// carried on from where it stopped: the question appended once, sent once, its answer appended once and read once.
#[derive(Debug, Clone)]
pub(super) struct Confirming {
    order: Id,
    reserve: Id,
    transaction: Id,
    ticket: CheckTicket,
    /// The question in its envelope, as signed: what is logged and what is sent.
    bytes: Vec<u8>,
    question: Entry,
    question_receipt: Option<Receipt>,
    sent: bool,
    answer: Option<Vec<u8>>,
    answer_entry: Option<Entry>,
    answer_receipt: Option<Receipt>,
    verdict: Option<Verdict>,
}

/// How a question came out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Confirmation {
    /// The reserve answered Core's question, under its transaction, that the account holds what arrived: the receipt
    /// of its answer's stated entry.
    Confirmed(Receipt),
    /// It answered no, refused, answered something else, or nothing.
    NotConfirmed,
}

/// The actor a reserve's statement claims made it: the reserve, stating for its publisher.
pub(super) fn reserve_maker(catalogue: &Catalogue, operator: &Actor, reserve: &Id) -> Actor {
    let party = match catalogue.publishers().publisher(reserve.as_str()) {
        Some(Publisher::Operator) => Some(operator.party.clone()),
        Some(Publisher::Party(publisher)) => Id::new(publisher),
        None => None,
    };
    Actor {
        party: party.unwrap_or_else(|| reserve.clone()),
        acts: reserve.clone(),
        role: Role::Provider,
        kind: ActorKind::Component,
    }
}

/// A fresh transaction for `order`: never one the rounds or arrivals asked under, never naming the order.
fn fresh(
    desk: &mut Desk<'_, '_>,
    used: &mut BTreeSet<String>,
    order: &Id,
) -> Result<Id, ArrivalError> {
    let id = desk.parts.fresh.id();
    if desk.parts.issued.contains(id.as_str())
        || used.contains(id.as_str())
        || id.as_str().contains(order.as_str())
    {
        return Err(ArrivalError::Fresh);
    }
    used.insert(id.as_str().to_owned());
    Ok(id)
}

impl Confirming {
    /// Builds the question, once the catalogue lets the check out: `Ok(Err(refusal))` where it doesn't, with
    /// nothing asked or recorded.
    pub(super) fn ask(
        desk: &mut Desk<'_, '_>,
        used: &mut BTreeSet<String>,
        asking: Asking<'_>,
    ) -> Result<Result<Self, CheckRefusal>, ArrivalError> {
        let transaction = fresh(desk, used, asking.order)?;
        let operation = Operation::BalanceVerification(BalanceVerification {
            transaction: transaction.as_str().to_owned(),
            provider: asking.reserve.as_str().to_owned(),
            session: asking.session.as_str().to_owned(),
            account: asking.account.as_str().to_owned(),
            asset: asking.asset.id.clone(),
            amount: asking.amount.amount().as_str().to_owned(),
        });
        // The holder's own order: not counted against the account's bound (`REQ-CAT-3`).
        let ticket = match desk.parts.catalogue.begin_check(
            &operation,
            Some(asking.order.as_str()),
            desk.parts.clock.now().floor(),
        ) {
            Ok(ticket) => ticket,
            Err(refusal) => return Ok(Err(refusal)),
        };
        let bytes = sign(desk, operation)?;
        let about = About {
            order: Some(asking.order.clone()),
            segment: None,
            transaction: Some(transaction.clone()),
        };
        let question = journal::stated(
            &mut *desk.parts.evidence,
            desk.parts.clock,
            &bytes,
            desk.operator.clone(),
            Some(about),
        )?;
        Ok(Ok(Confirming {
            order: asking.order.clone(),
            reserve: asking.reserve.clone(),
            transaction,
            ticket,
            bytes,
            question,
            question_receipt: None,
            sent: false,
            answer: None,
            answer_entry: None,
            answer_receipt: None,
            verdict: None,
        }))
    }

    /// Carries the question on: appended, sent, its answer appended and read, each once.
    pub(super) fn go_on(&mut self, desk: &mut Desk<'_, '_>) -> Result<Confirmation, ArrivalError> {
        // Appended before it is sent, so the log binds its answer to the order before the answer exists.
        journal::once(
            &mut *desk.parts.log,
            &self.question,
            &mut self.question_receipt,
        )?;
        if !self.sent {
            self.answer = desk.parts.providers.ask(&self.reserve, &self.bytes);
            self.sent = true;
        }
        let Some(answer) = &self.answer else {
            return Ok(Confirmation::NotConfirmed);
        };
        if self.answer_entry.is_none() {
            let maker = reserve_maker(desk.parts.catalogue, desk.operator, &self.reserve);
            let about = About {
                order: Some(self.order.clone()),
                segment: None,
                transaction: Some(self.transaction.clone()),
            };
            self.answer_entry = Some(journal::stated(
                &mut *desk.parts.evidence,
                desk.parts.clock,
                answer,
                maker,
                Some(about),
            )?);
        }
        let Some(entry) = &self.answer_entry else {
            return Ok(Confirmation::NotConfirmed);
        };
        let receipt = journal::once(&mut *desk.parts.log, entry, &mut self.answer_receipt)?;
        let verdict = match &self.verdict {
            Some(verdict) => verdict.clone(),
            None => {
                let verdict = serde_json::from_slice::<Statement<ResultBody>>(answer)
                    .ok()
                    .filter(|s| s.maker == self.reserve.as_str())
                    .map_or(Verdict::NotKnown, |result| {
                        desk.parts.catalogue.answer_check(
                            &self.ticket,
                            &result.statement,
                            desk.parts.clock.now().floor(),
                        )
                    });
                self.verdict = Some(verdict.clone());
                verdict
            }
        };
        Ok(if verdict == Verdict::Yes {
            Confirmation::Confirmed(receipt)
        } else {
            Confirmation::NotConfirmed
        })
    }
}

/// The question in its envelope, made by the operator at Core's clock for the bounds' deployment and protocol
/// version, signed with the operator key: the bytes as signed.
fn sign(desk: &mut Desk<'_, '_>, operation: Operation) -> Result<Vec<u8>, ArrivalError> {
    let made_at = journal::now(desk.parts.clock)?;
    let mut statement = Statement {
        deployment: desk.bounds.deployment.clone(),
        protocol_version: desk.bounds.protocol_version.as_str().to_owned(),
        shape: OPERATION_SHAPE.to_owned(),
        maker: desk.operator.acts.as_str().to_owned(),
        made_at: made_at.as_str().to_owned(),
        signature: Signature {
            scheme: SignatureScheme::Ed25519,
            key: desk.parts.operator_key.key(),
            value: base64url(&[0u8; 64]),
        },
        statement: operation,
    };
    let unsigned = serde_json::to_vec(&statement).map_err(|_| ArrivalError::Unsigned)?;
    let input = signing_input(&unsigned).map_err(|_| ArrivalError::Unsigned)?;
    let signature = desk
        .parts
        .operator_key
        .sign(&input)
        .ok_or(ArrivalError::Unsigned)?;
    statement.signature.value = base64url(&signature);
    serde_json::to_vec(&statement).map_err(|_| ArrivalError::Unsigned)
}

/// Bytes as base64url without padding.
fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(char::from(ALPHABET[(n >> (18 - 6 * i) & 63) as usize]));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::base64url;

    // verifies: REQ-ORD-32
    #[test]
    fn bytes_are_written_in_base64url_without_padding() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
    }
}
