//! Writing what Core asks and records: a question as a provider-role operation, signed with the operator key, and a
//! decision record in its envelope, with its digest.

use std::collections::BTreeMap;

use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{
    Conversion, Deposit, Direction, JudgementRequest, NodeRef, OPERATION_SHAPE, Operation,
    Signature, SignatureScheme, Statement, Transfer, Withdrawal, canonicalize, signing_input,
};
use rutent_contracts::router_api::{
    CatalogueSnapshot, DECISION_RECORD_SHAPE, DecisionRecord, Endpoint, FigureValue, Id,
    OperationKind, PolicyBounds, PositiveAmount, Timestamp, TupleKey,
};
use serde::Serialize;

use super::ports::OperatorKey;

/// One question to one provider about one of its tuples, at one amount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Question {
    /// The tuple, by its provider and ends.
    pub tuple: TupleKey,
    /// The operation the tuple performs.
    pub kind: OperationKind,
    /// What enters it, in its input unit.
    pub amount_in: PositiveAmount,
}

/// Why Core couldn't sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unsigned;

/// The operation a question asks, as a dry run under `transaction`: a withdrawal in `session`, which reveals
/// whether funds suffice (`REQ-CAT-4`), and every other kind with no session. It carries only what the step needs
/// (`REQ-CAT-5`): its tuple's ends and amount, and of the order's inputs only those its tuple requires.
pub(super) fn operation(
    question: &Question,
    transaction: &Id,
    session: Option<&Id>,
    inputs: BTreeMap<String, String>,
) -> Option<Operation> {
    let (from, to) = (&question.tuple.from, &question.tuple.to);
    let provider = question.tuple.provider.as_str().to_owned();
    let transaction = transaction.as_str().to_owned();
    let amount = question.amount_in.amount().as_str().to_owned();
    let inner = |e: &Endpoint| match &e.node {
        NodeRef::Inner(n) => Some(n.clone()),
        NodeRef::Outer(_) => None,
    };
    let outer = |e: &Endpoint| match &e.node {
        NodeRef::Outer(n) => Some(n.clone()),
        NodeRef::Inner(_) => None,
    };
    Some(match question.kind {
        OperationKind::Withdrawal => Operation::Withdrawal(Withdrawal {
            transaction,
            provider,
            session: Some(session?.as_str().to_owned()),
            origin: outer(from)?,
            destination: inner(to)?,
            asset: from.asset.id.clone(),
            amount,
            dry_run: true,
            inputs,
        }),
        OperationKind::Deposit => Operation::Deposit(Deposit {
            transaction,
            provider,
            origin: inner(from)?,
            destination: outer(to)?,
            asset: from.asset.id.clone(),
            amount,
            dry_run: true,
            inputs,
        }),
        OperationKind::Route => Operation::Transfer(Transfer {
            transaction,
            provider,
            origin: inner(from)?,
            destination: inner(to)?,
            asset: from.asset.id.clone(),
            direction: Direction::ExactIn,
            amount,
            dry_run: true,
            inputs,
        }),
        OperationKind::Exchange => Operation::Conversion(Conversion {
            transaction,
            provider,
            node: inner(from)?,
            from: from.asset.id.clone(),
            to: to.asset.id.clone(),
            direction: Direction::ExactIn,
            amount,
            dry_run: true,
            inputs,
        }),
    })
}

/// A question to a verifier: a person, by the relying provider's subject reference, and a class only
/// (`REQ-CPL-4`, ADR-0012).
pub(super) fn judgement(verifier: &Id, subject: &str, class: &str) -> Operation {
    Operation::Judgement(JudgementRequest {
        provider: verifier.as_str().to_owned(),
        subject: subject.to_owned(),
        class: class.to_owned(),
    })
}

/// The inputs a tuple of the snapshot requires, by name: those of the order's inputs its terms list, and no other.
pub(super) fn inputs_for(
    snapshot: &CatalogueSnapshot,
    tuple: &TupleKey,
    order_inputs: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let required = snapshot.figures.iter().find_map(|f| {
        if f.origin.provider() != Some(&tuple.provider) {
            return None;
        }
        let (terms, from, to) = match &f.value {
            FigureValue::ReserveTuple { tuple: t } => {
                (&t.terms, (&t.origin, &t.asset), (&t.destination, &t.asset))
            }
            FigureValue::RampTuple { tuple: t } => {
                let (o, d) = (
                    NodeRef::Inner(t.origin.clone()),
                    NodeRef::Inner(t.destination.clone()),
                );
                return (same_node(&tuple.from, &o)
                    && same_node(&tuple.to, &d)
                    && tuple.from.asset.id == t.asset)
                    .then_some(&t.terms);
            }
            FigureValue::ExchangeTuple { tuple: t } => {
                let node = NodeRef::Inner(t.node.clone());
                return (same_node(&tuple.from, &node)
                    && same_node(&tuple.to, &node)
                    && tuple.from.asset == t.from
                    && tuple.to.asset == t.to)
                    .then_some(&t.terms);
            }
            _ => return None,
        };
        (same_node(&tuple.from, from.0)
            && same_node(&tuple.to, to.0)
            && tuple.from.asset.id == *from.1)
            .then_some(terms)
    });
    let Some(terms) = required else {
        return BTreeMap::new();
    };
    terms
        .required_inputs
        .iter()
        .filter_map(|name| Some((name.clone(), order_inputs.get(name)?.clone())))
        .collect()
}

fn same_node(end: &Endpoint, node: &NodeRef) -> bool {
    &end.node == node
}

/// `body` in its envelope, made by `maker` at `made_at` for the bounds' deployment and protocol version, signed with
/// the operator key: the bytes as signed, which are what is logged and what is sent.
pub(super) fn sign<T: Serialize>(
    key: &mut dyn OperatorKey,
    bounds: &PolicyBounds,
    shape: &str,
    maker: &Id,
    made_at: &Timestamp,
    body: T,
) -> Result<Vec<u8>, Unsigned> {
    let mut statement = Statement {
        deployment: bounds.deployment.clone(),
        protocol_version: bounds.protocol_version.as_str().to_owned(),
        shape: shape.to_owned(),
        maker: maker.as_str().to_owned(),
        made_at: made_at.as_str().to_owned(),
        signature: Signature {
            scheme: SignatureScheme::Ed25519,
            key: key.key(),
            value: base64url(&[0u8; 64]),
        },
        statement: body,
    };
    let unsigned = serde_json::to_vec(&statement).map_err(|_| Unsigned)?;
    let input = signing_input(&unsigned).map_err(|_| Unsigned)?;
    statement.signature.value = base64url(&key.sign(&input).ok_or(Unsigned)?);
    serde_json::to_vec(&statement).map_err(|_| Unsigned)
}

/// A question's operation, signed.
pub(super) fn sign_question(
    key: &mut dyn OperatorKey,
    bounds: &PolicyBounds,
    maker: &Id,
    made_at: &Timestamp,
    operation: Operation,
) -> Result<Vec<u8>, Unsigned> {
    sign(key, bounds, OPERATION_SHAPE, maker, made_at, operation)
}

/// A round's decision record, in its envelope, signed: router-api's `RecordedDecision`.
pub(super) fn sign_record(
    key: &mut dyn OperatorKey,
    record: &DecisionRecord,
    maker: &Id,
    made_at: &Timestamp,
) -> Result<Vec<u8>, Unsigned> {
    sign(
        key,
        &record.bounds,
        DECISION_RECORD_SHAPE,
        maker,
        made_at,
        record,
    )
}

/// The digest of a recorded decision: SHA-256 of its RFC 8785 canonical JSON, as signed. The `decision_record`
/// entry and a question's shortlisted authority name it by this one function.
pub(super) fn digest(recorded: &[u8]) -> Option<Digest> {
    let canonical = canonicalize(recorded).ok()?;
    let value = rutent_prov::module::wire::sha256(&canonical);
    serde_json::from_value(serde_json::json!({ "algorithm": "sha-256", "value": value })).ok()
}

/// The transaction a provider's answer names, read without trusting anything else in it.
pub(super) fn transaction_of(answer: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(answer).ok()?;
    Some(
        value
            .get("statement")?
            .get("transaction")?
            .as_str()?
            .to_owned(),
    )
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

    // verifies: REQ-AUD-4
    #[test]
    fn bytes_are_written_in_base64url_without_padding() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
    }
}
