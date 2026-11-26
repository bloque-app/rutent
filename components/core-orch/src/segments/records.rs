//! Reading a private node's record while a segment is graded (`REQ-ORD-41`, `REQ-AUZ-1`, `T-020-58`).
//!
//! The tick's job reads them the same way ([`Segments::read_node_record`]), so no read of a private node's record is
//! made undecided.
//!
//! The grading says which records it reads ([`Grading::records_wanted`]); for each, the segments do what they do for any
//! action policy decides: draw a fresh transaction, have policy decide `read_node_record` (the operator's permission,
//! `core-authz`'s roles), record the effect before anything is sent, act on it once, put the question to the platform as
//! that effect lets ([`Modules::ask_for_record`]) and record how it came out. What came back goes to the grading, which
//! takes it only as `closing::publishers::taken` allows. A refusal by policy asks nothing and the grading reads no record
//! of that provider: it fails closed.

use rutent_contracts::core_internal::{Action, DecisionRequest, Effect, Outcome, What};
use rutent_contracts::provider_role::{Operation, Statement};
use rutent_contracts::router_api::Id;

use super::journal;
use super::ports::{Answered, ToGrade};
use super::{Parts, Segments, now, sign};
use crate::closing::publishers;

/// One read of a private node's record: whose it is for (an order's segment that passes the node, which the action
/// names), the node and the provider asked, the bounds the question is made in, the operator who asks policy and Core,
/// who records it.
#[derive(Debug, Clone, Copy)]
pub struct RecordRead<'a> {
    /// The order.
    pub order: &'a Id,
    /// The segment that passes the node.
    pub segment: &'a Id,
    /// The private node.
    pub node: &'a str,
    /// The provider of the node's publisher that is asked.
    pub provider: &'a str,
    /// The bounds the question is made in: the deployment and protocol version of the order's route record.
    pub bounds: &'a rutent_contracts::router_api::PolicyBounds,
    /// The operator, who asks policy and in whose name the question is signed.
    pub operator: &'a rutent_contracts::core_internal::Actor,
    /// Core, who records the effect and its result.
    pub core: &'a rutent_contracts::core_internal::Actor,
}

impl Segments {
    /// Asks for each record the grading of `settled` reads, and hands the grading what came back.
    pub(super) fn read_records(
        &mut self,
        parts: &mut Parts<'_>,
        live: &super::Live<'_>,
        settled: &ToGrade<'_>,
    ) {
        for wanted in parts.grading.records_wanted(settled) {
            // Once for each segment, node and provider: graded again while it waits, a segment appends nothing more, and
            // the books keep what was read.
            let asked = (
                live.order.as_str().to_owned(),
                settled.segment.as_str().to_owned(),
                wanted.node.clone(),
                wanted.provider.clone(),
            );
            if !self.records_read.insert(asked) {
                continue;
            }
            let read = RecordRead {
                order: live.order,
                segment: settled.segment,
                node: &wanted.node,
                provider: &wanted.provider,
                bounds: &live.record.bounds,
                operator: live.operator,
                core: live.core,
            };
            let answer = self.read_node_record(parts, &read);
            parts.grading.record_read(
                &wanted,
                answer.as_ref().map(|(q, bytes)| (q, bytes.as_slice())),
            );
        }
    }

    /// One record: decided, recorded as an effect, asked, and its result recorded. `None` where policy refused, a
    /// record couldn't be written, or nothing came back; nothing is asked before its effect is in the log (`INV-7`).
    pub fn read_node_record(
        &mut self,
        parts: &mut Parts<'_>,
        read: &RecordRead<'_>,
    ) -> Option<(Statement<Operation>, Vec<u8>)> {
        let transaction = self.claim_transaction(parts.fresh, parts.issued).ok()?;
        let action = Action::ReadNodeRecord {
            order: read.order.clone(),
            segment: read.segment.clone(),
            node: Id::new(read.node)?,
            provider: Id::new(read.provider)?,
            transaction: transaction.clone(),
        };
        let request = DecisionRequest {
            request: parts.fresh.id(),
            caller: read.operator.clone(),
            action: action.clone(),
            at: now(parts.clock).ok()?,
        };
        let decision = parts.decisions.decide(&request).ok()?;
        let permit = decision.permit()?;
        let receipt = journal::append(
            parts.log,
            journal::verified(
                read.core,
                What::Effect {
                    action: action.clone(),
                    decision: permit.clone(),
                },
                &now(parts.clock).ok()?,
                Outcome::ToTakeEffect {},
            ),
        )
        .ok()?;
        let effect = Effect {
            action,
            decision: permit,
            receipt: receipt.clone(),
        };
        let asked = self.act_on(&receipt).then(|| {
            let signed = sign::sign(
                parts.operator_key,
                read.bounds,
                &read.operator.acts,
                &now(parts.clock).ok()?,
                publishers::question(read.provider, read.node, transaction.as_str()),
            )
            .ok()?;
            serde_json::from_slice::<Statement<Operation>>(&signed).ok()
        });
        let question = asked.flatten();
        let answered = match &question {
            Some(question) => {
                parts
                    .modules
                    .ask_for_record(&effect, &Id::new(read.provider)?, question)
            }
            None => {
                Answered::Refused(rutent_contracts::module_interface::PlatformRefusal::Unavailable)
            }
        };
        let outcome = match &answered {
            Answered::Bytes(_) => Outcome::Done {},
            Answered::Refused(_) => Outcome::Unanswered {},
        };
        let _ = journal::append(
            parts.log,
            journal::verified(
                read.core,
                What::Result { effect: receipt },
                &now(parts.clock).ok()?,
                outcome,
            ),
        );
        match (question, answered) {
            (Some(question), Answered::Bytes(bytes)) => Some((question, bytes)),
            _ => None,
        }
    }
}
