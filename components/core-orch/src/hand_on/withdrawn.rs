//! A hand-on's withdrawal and its linked order's accepted route, as the log holds them (`T-020-46`, moved into Core by
//! `T-020-61`): what a Core that reopened, holding no mover's record of the withdrawal, hands the part on from.

use rutent_contracts::core_internal::{
    Action, Attribution, Entry, Outcome, Receipt, Resolution, VerifiedEntry, What,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{
    NodeRef, RESULT_SHAPE, ReserveMovementOutcome, ResultBody, Statement,
};
use rutent_contracts::router_api::Id;
use rutent_contracts::state_machine::{self as stm, PartPlan, SegmentPlan};
use rutent_prov::holding::wallet::{HandedOnStart, WithdrawalRecord, proof_of_withdrawal};

use crate::intake::held_record;
use crate::stuck::AcceptedRoute;

/// The identifier of the segment at `place` of a linked order's part `part`: its part's and its place, `{part}-s{n}`.
/// The log's acceptance names the part, so a Core that reopens derives the plan again from the log and the record the
/// route was chosen by (`T-020-46`). Unique within the order, as a plan's ids are.
pub fn linked_segment_id(part: &stm::Id, place: usize) -> Option<stm::Id> {
    stm::Id::new(format!("{}-s{}", part.as_str(), place + 1))
}

/// The route the linked order `linked` was accepted on, from the log: its acceptance (its id and its part) and the
/// decision record its route was chosen by, read back from the evidence store through `kept` and held to the log's
/// digest, each segment under the identifier [`linked_segment_id`] derives. `None` where any of it isn't there.
pub fn accepted_in_the_log(
    history: &[(Receipt, Entry)],
    kept: &mut dyn FnMut(&Digest) -> Option<Vec<u8>>,
    linked: &Id,
) -> Option<AcceptedRoute> {
    let (acceptance, part) = history.iter().find_map(|(_, e)| match e {
        Entry::Verified(v) => match &v.what {
            What::Acceptance {
                acceptance,
                order: o,
                yeses,
                ..
            } if o == linked => Some((acceptance.clone(), yeses.first()?.part.clone())),
            _ => None,
        },
        Entry::Stated(_) => None,
    })?;
    let decision_record = history.iter().rev().find_map(|(_, e)| match e {
        Entry::Verified(v) => match &v.what {
            What::DecisionRecord {
                decision_record,
                order: o,
                ..
            } if o == linked => Some(decision_record.clone()),
            _ => None,
        },
        Entry::Stated(_) => None,
    })?;
    let record = held_record(history, kept, &decision_record)?;
    let [only] = record.chosen()?.parts.as_slice() else {
        return None;
    };
    let part_id = stm::Id::new(part.as_str())?;
    let mut segments = Vec::new();
    for (place, operation) in only.operations.iter().enumerate() {
        segments.push(SegmentPlan {
            segment_id: linked_segment_id(&part_id, place)?,
            provider: stm::Id::new(operation.provider.as_str())?,
            kind: operation.kind,
            from: operation.from.clone(),
            to: operation.to.clone(),
        });
    }
    Some(AcceptedRoute {
        order: linked.clone(),
        decision_record,
        acceptance,
        plan: vec![PartPlan { part_id, segments }],
    })
}

/// The holding account's withdrawal for the hand-on `effect` carried out, as the log holds it: the effect's
/// `resolve_stuck` with its `done` result, handing the part's value on to its linked order; the linked order's accepted
/// route ([`accepted_in_the_log`]); and the holding account's own answer, verified, about the linked order, its record
/// read from the bytes the evidence store keeps (`proof_of_withdrawal`). The linked order, its route, the start the
/// answer proves and the receipt of the answer's stated entry; `None` where any of it isn't there.
pub fn withdrawn_in_the_log(
    history: &[(Receipt, Entry)],
    kept: &mut dyn FnMut(&Digest) -> Option<Vec<u8>>,
    effect: &Receipt,
) -> Option<(Id, AcceptedRoute, HandedOnStart, Receipt)> {
    let (order, holder, linked) = history.iter().find_map(|(r, e)| match e {
        Entry::Verified(VerifiedEntry {
            what:
                What::Effect {
                    action:
                        Action::ResolveStuck {
                            order,
                            resolution:
                                Resolution::HandOn {
                                    by, linked_order, ..
                                },
                            ..
                        },
                    ..
                },
            ..
        }) if r == effect => Some((order.clone(), by.clone(), linked_order.clone())),
        _ => None,
    })?;
    let done = history.iter().any(|(_, e)| {
        matches!(e, Entry::Verified(v) if matches!(&v.what, What::Result { effect: of } if of == effect)
            && matches!(v.outcome, Outcome::Done {}))
    });
    if !done {
        return None;
    }
    let route = accepted_in_the_log(history, kept, &linked)?;
    let first = route.plan.first()?.segments.first()?.clone();
    let NodeRef::Inner(node) = &first.from.node else {
        return None;
    };
    let (statement, commitment) = history.iter().find_map(|(r, e)| match e {
        Entry::Stated(s)
            if s.what.statement.as_str() == RESULT_SHAPE
                && matches!(&s.attribution, Attribution::Verified { who } if who.acts == holder)
                && s.about
                    .as_ref()
                    .is_some_and(|a| a.order.as_ref() == Some(&linked)) =>
        {
            Some((r.clone(), s.commitment.clone()))
        }
        _ => None,
    })?;
    let answer = kept(&commitment)?;
    let result: Statement<ResultBody> = serde_json::from_slice(&answer).ok()?;
    let ResultBody::Withdrawal(withdrawal) = result.statement else {
        return None;
    };
    let ReserveMovementOutcome::Succeeded(settled) = withdrawal.outcome else {
        return None;
    };
    let reference =
        proof_of_withdrawal(holder.as_str(), &withdrawal.transaction, &settled.evidence).ok()?;
    let start = HandedOnStart {
        reserve: holder.as_str().to_owned(),
        node: node.clone(),
        first_segment: first.segment_id.clone(),
        record: WithdrawalRecord {
            by: holder.as_str().to_owned(),
            node: node.clone(),
            asset: first.from.asset.id.clone(),
            order: order.as_str().to_owned(),
            amount: settled.amount_out.parse().ok()?,
            reference,
        },
        transaction: withdrawal.transaction,
        answer,
    };
    Some((linked, route, start, statement))
}
