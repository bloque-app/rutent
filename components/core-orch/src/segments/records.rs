//! Reading a private node's record while a segment is graded (`REQ-ORD-41`, `REQ-AUZ-1`, `T-020-58`).
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
use super::ports::{Answered, RecordWanted, ToGrade};
use super::{Parts, Segments, now, sign};
use crate::closing::publishers;

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
            let answer = self.read_record(parts, live, settled, &wanted);
            parts.grading.record_read(
                &wanted,
                answer.as_ref().map(|(q, bytes)| (q, bytes.as_slice())),
            );
        }
    }

    /// One record: decided, recorded as an effect, asked, and its result recorded. `None` where policy refused, a
    /// record couldn't be written, or nothing came back; nothing is asked before its effect is in the log (`INV-7`).
    fn read_record(
        &mut self,
        parts: &mut Parts<'_>,
        live: &super::Live<'_>,
        settled: &ToGrade<'_>,
        wanted: &RecordWanted,
    ) -> Option<(Statement<Operation>, Vec<u8>)> {
        let transaction = self.claim_transaction(parts.fresh, parts.issued).ok()?;
        let action = Action::ReadNodeRecord {
            order: live.order.clone(),
            segment: settled.segment.clone(),
            node: Id::new(&wanted.node)?,
            provider: Id::new(&wanted.provider)?,
            transaction: transaction.clone(),
        };
        let request = DecisionRequest {
            request: parts.fresh.id(),
            caller: live.operator.clone(),
            action: action.clone(),
            at: now(parts.clock).ok()?,
        };
        let decision = parts.decisions.decide(&request).ok()?;
        let permit = decision.permit()?;
        let receipt = journal::append(
            parts.log,
            journal::verified(
                live.core,
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
                &live.record.bounds,
                &live.operator.acts,
                &now(parts.clock).ok()?,
                publishers::question(&wanted.provider, &wanted.node, transaction.as_str()),
            )
            .ok()?;
            serde_json::from_slice::<Statement<Operation>>(&signed).ok()
        });
        let question = asked.flatten();
        let answered = match &question {
            Some(question) => {
                parts
                    .modules
                    .ask_for_record(&effect, &Id::new(&wanted.provider)?, question)
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
                live.core,
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
