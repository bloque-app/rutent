//! Reading a private node's record (`REQ-ORD-41`, `REQ-AUZ-1`, `T-020-58`).
//!
//! Closing says which records it reads ([`Grading::records_wanted`]), and the tick's job which orders are still unsettled at
//! a node; for each read ([`Segments::read_node_record`]) the segments do what they do for any action policy decides: draw
//! a fresh transaction, have policy decide `read_node_record` as Core, which carries the operator's standing permission
//! onward (`core-authz`'s roles, as `ask_status` is decided), record the effect before anything is sent, act on it once,
//! put the question, scoped to the transactions asked about, to the platform as that effect lets
//! ([`Modules::ask_for_record`]) and record how it came out. What came back goes to the grading, which takes it only as
//! `closing::publishers::taken` allows. A refusal by policy asks nothing and the grading reads no record of that
//! provider: it fails closed. A read is always recorded to its result: where the result can't be written the answer is
//! not used.

use rutent_contracts::core_internal::{Action, DecisionRequest, Effect, Outcome, What};
use rutent_contracts::provider_role::{Operation, Statement};
use rutent_contracts::router_api::Id;

use super::journal;
use super::ports::{Answered, ToGrade};
use super::{Parts, Segments, now, sign};
use crate::closing::publishers;

/// One read of a private node's record: whose it is for (an order's segment that passes the node, which the action
/// names), the node and the provider asked, the transactions asked about, the bounds the question is made in, the
/// operator in whose name it is signed and Core, who asks policy and records it.
#[derive(Debug, Clone, Copy)]
pub struct RecordRead<'a> {
    /// The order.
    pub order: &'a Id,
    /// A segment of the order that passes the node.
    pub segment: &'a Id,
    /// The private node.
    pub node: &'a str,
    /// The provider of the node's publisher that is asked.
    pub provider: &'a str,
    /// The transactions the question is about: the segments whose record is asked for, at least one.
    pub about: &'a [Id],
    /// The bounds the question is made in: the deployment and protocol version of the order's route record.
    pub bounds: &'a rutent_contracts::router_api::PolicyBounds,
    /// The operator, in whose name the question is signed.
    pub operator: &'a rutent_contracts::core_internal::Actor,
    /// Core, who asks policy and records the effect and its result.
    pub core: &'a rutent_contracts::core_internal::Actor,
}

/// How a read came out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Read {
    /// Policy refused it: nothing was asked and nothing recorded but policy's own refusal.
    Denied,
    /// It couldn't be done now (an entry the log couldn't take, no answer from the platform): worth trying again.
    Failed,
    /// The question as put and the answer as the platform passed it on.
    Answered(Box<Statement<Operation>>, Vec<u8>),
}

impl Segments {
    /// Asks for each record the grading of `settled` reads, and hands the grading what came back. A read is not asked
    /// again for the segment once policy refused it or its record was taken; a failed one, or an answer that was no record
    /// yet, is tried again the next time the segment is graded.
    pub(super) fn read_records(
        &mut self,
        parts: &mut Parts<'_>,
        live: &super::Live<'_>,
        settled: &ToGrade<'_>,
    ) {
        for wanted in parts.grading.records_wanted(settled) {
            let asked = (
                live.order.as_str().to_owned(),
                settled.segment.as_str().to_owned(),
                wanted.node.clone(),
                wanted.provider.clone(),
            );
            if self.records_read.contains(&asked) {
                continue;
            }
            let about: Vec<Id> = wanted.about.iter().filter_map(Id::new).collect();
            let read = RecordRead {
                order: live.order,
                segment: settled.segment,
                node: &wanted.node,
                provider: &wanted.provider,
                about: &about,
                bounds: &live.record.bounds,
                operator: live.operator,
                core: live.core,
            };
            let done = match self.read_node_record(parts, &read) {
                Read::Denied => true,
                Read::Failed => parts.grading.record_read(live.order, &wanted, None) && false,
                Read::Answered(question, bytes) => parts.grading.record_read(
                    live.order,
                    &wanted,
                    Some((&question, bytes.as_slice())),
                ),
            };
            if done {
                self.records_read.insert(asked);
            }
        }
    }

    /// One read: decided, recorded as an effect, asked, and its result recorded. Nothing is asked before its effect is in
    /// the log (`INV-7`), and an answer is handed back only once its result is.
    pub fn read_node_record(&mut self, parts: &mut Parts<'_>, read: &RecordRead<'_>) -> Read {
        let (Some(node), Some(provider)) = (Id::new(read.node), Id::new(read.provider)) else {
            return Read::Failed;
        };
        if read.about.is_empty() {
            return Read::Failed;
        }
        let Ok(transaction) = self.claim_transaction(parts.fresh, parts.issued) else {
            return Read::Failed;
        };
        let action = Action::ReadNodeRecord {
            order: read.order.clone(),
            segment: read.segment.clone(),
            node,
            provider: provider.clone(),
            transaction: transaction.clone(),
            about: read.about.to_vec(),
        };
        let Ok(at) = now(parts.clock) else {
            return Read::Failed;
        };
        let request = DecisionRequest {
            request: parts.fresh.id(),
            caller: read.core.clone(),
            action: action.clone(),
            at,
        };
        let Ok(decision) = parts.decisions.decide(&request) else {
            return Read::Failed;
        };
        let Some(permit) = decision.permit() else {
            return Read::Denied;
        };
        let Ok(effect_at) = now(parts.clock) else {
            return Read::Failed;
        };
        let Ok(receipt) = journal::append(
            parts.log,
            journal::verified(
                read.core,
                What::Effect {
                    action: action.clone(),
                    decision: permit.clone(),
                },
                &effect_at,
                Outcome::ToTakeEffect {},
            ),
        ) else {
            return Read::Failed;
        };
        let effect = Effect {
            action,
            decision: permit,
            receipt: receipt.clone(),
        };
        // The result's time is read now, before anything is asked, so no early return can follow the question.
        let result_at = now(parts.clock);
        let question = self
            .act_on(&receipt)
            .then(|| {
                let signed = sign::sign(
                    parts.operator_key,
                    read.bounds,
                    &read.operator.acts,
                    result_at.as_ref().ok()?,
                    publishers::question(
                        read.provider,
                        read.node,
                        transaction.as_str(),
                        &read
                            .about
                            .iter()
                            .map(|t| t.as_str().to_owned())
                            .collect::<Vec<_>>(),
                    ),
                )
                .ok()?;
                serde_json::from_slice::<Statement<Operation>>(&signed).ok()
            })
            .flatten();
        let answered = match &question {
            Some(question) => parts.modules.ask_for_record(&effect, &provider, question),
            None => {
                Answered::Refused(rutent_contracts::module_interface::PlatformRefusal::Unavailable)
            }
        };
        let outcome = match (&question, &answered) {
            (_, Answered::Bytes(_)) => Outcome::Done {},
            (Some(_), Answered::Refused(_)) => Outcome::Unanswered {},
            (None, _) => Outcome::Failed {},
        };
        let wrote = result_at.ok().and_then(|at| {
            journal::append(
                parts.log,
                journal::verified(read.core, What::Result { effect: receipt }, &at, outcome),
            )
            .ok()
        });
        // An answer is used only once the entry that records how the read came out is in the log.
        match (question, answered, wrote) {
            (Some(question), Answered::Bytes(bytes), Some(_)) => {
                Read::Answered(Box::new(question), bytes)
            }
            _ => Read::Failed,
        }
    }
}
