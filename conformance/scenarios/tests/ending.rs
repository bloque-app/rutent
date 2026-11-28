//! Ending scenarios over the assembled Core against the simulated providers (`T-024-06`): an order that ends without
//! completing only until its first segment settles (`AC-7`), a complete order whose start is proven undone (`AC-8`),
//! a payment returned after complete (`AC-9`), and every change of an order notified to its user and the operator
//! (`AC-41`). Each drives Core's own `request`, `quote`, `accept` and `run`, its reversals and endings, and its
//! notices, over [`execution_support`]'s world.

mod execution_support;
mod failure_support;

use execution_support::*;
use failure_support::*;
use rutent_conformance::sim::{Act, Behaviour};
use rutent_contracts::core_internal::{Afterwards, Entry, Role, What};
use rutent_contracts::provider_role::{EvidenceGrade, RESULT_SHAPE};
use rutent_contracts::router_api::Id;
use rutent_contracts::state_machine::{self as stm, Observed, OrderState, StateMachine};
use rutent_core_orch::notice::Receiver;
use rutent_core_orch::reversal::{Confirmation, NotCounted, ReversalError, Word};
use rutent_core_orch::segments::SegmentOutcome;
use rutent_scenarios::assembly::{Core, StepError};
use rutent_scenarios::kit::{self, AMOUNT, RESERVE, id};
use rutent_scenarios::on_tick::OnTick;
use rutent_stm::Stm;
use serde_json::json;

/// What the customer is told, in order, of `order`, as `(change, state after it)`.
fn told_to_the_user(core: &mut Core, order: &Id) -> Vec<(String, OrderState)> {
    core.notify().expect("the notices read the log");
    changes(&core.notices_to(&Core::user_receiver(), order))
}

/// What the operator is told, in order, of `order`.
fn told_to_the_operator(core: &mut Core, order: &Id) -> Vec<(String, OrderState)> {
    core.notify().expect("the notices read the log");
    changes(&core.notices_to(&core.operator_receiver(), order))
}

/// How many moves of value the log holds: what Core handed to a provider.
fn effects(core: &Core) -> usize {
    entries(core)
        .iter()
        .filter(|v| {
            matches!(
                v.what,
                What::Effect {
                    action: rutent_contracts::core_internal::Action::MoveValue { .. },
                    ..
                }
            )
        })
        .count()
}

/// How many `end_requested` inputs the log holds for `order`.
fn end_requests(core: &Core, order: &Id) -> usize {
    count_inputs(core, order, |o| matches!(o, Observed::EndRequested {}))
}

// verifies: AC-7
#[test]
fn order_ends_without_completing_before_its_first_segment_settles_and_not_after() {
    // Accepted, and nothing handed over yet: no segment has settled, so the customer's request ends it, without
    // completing. Nothing is handed to the reserve, now or if the order is run afterwards.
    let (mut core, session) = core_with("AC-7", &[RESERVE]);
    let order = accepted(
        &mut core,
        within("req-ends", &session, AMOUNT, RESERVE, OTHER),
    );
    // Ending is policy's decision of `end_order` (`T-020-61`): another customer's request is refused, and ends nothing.
    let mut stranger = kit::customer();
    stranger.party = id("someone-else");
    stranger.acts = id("someone-else");
    assert!(
        matches!(
            core.end(&order, &stranger),
            Err(StepError::Reversal(ReversalError::NotPermitted(_)))
        ),
        "only the order's requester, or the operator holding the permission, ends it"
    );
    assert_eq!(end_requests(&core, &order), 0);
    core.end(&order, &kit::customer())
        .expect("an order ends before its first segment settles");
    assert_eq!(state(&mut core, &order), OrderState::EndedWithoutCompleting);
    assert_eq!(end_requests(&core, &order), 1);
    let part = core.part_of(&order).expect("a part");
    assert!(
        matches!(core.ran_on_tick(&order), Ok(ran) if ran.is_empty())
            && !core.is_stuck(&order, &part),
        "no segment of an ended part is handed over"
    );
    assert_eq!(effects(&core), 0, "no value moved");
    assert_eq!(
        core.world().journal(RESERVE).len(),
        0,
        "the reserve was handed nothing"
    );
    // Told to the customer and the operator: it ended without completing.
    let ended = (
        "end_requested".to_owned(),
        OrderState::EndedWithoutCompleting,
    );
    assert_eq!(told_to_the_user(&mut core, &order).last(), Some(&ended));
    assert_eq!(told_to_the_operator(&mut core, &order).last(), Some(&ended));

    // A segment whose outcome is unknown is value in motion: even before anything settled, no request ends the order
    // while it moves.
    let (mut core, session) = core_with("AC-7", &[RESERVE]);
    core.world()
        .script_for(RESERVE, "withdrawal", Behaviour::act(Act::Unknown));
    let (order, ran) = run(
        &mut core,
        within("req-moving", &session, AMOUNT, RESERVE, OTHER),
    );
    assert!(matches!(
        segments_of(&ran)[0].outcome,
        SegmentOutcome::Unknown(_)
    ));
    let (before, log) = (state(&mut core, &order), core.log().entries().len());
    assert!(matches!(
        core.end(&order, &kit::customer()),
        Err(StepError::Reversal(ReversalError::InMotion(_)))
    ));
    assert_eq!(
        end_requests(&core, &order),
        0,
        "no end is recorded while value moves"
    );
    assert_eq!(state(&mut core, &order), before);
    let _ = log;

    // After the first segment settles, no request can end it: the withdrawal settled and the deposit was refused, so the
    // order is past its first segment and goes on. The request is refused and recorded nowhere; the order stays as it was.
    let (mut core, session) = core_with("AC-7", &[RESERVE]);
    core.world()
        .script_for(RESERVE, "deposit", Behaviour::act(Act::Refuse));
    let (open, _) = run(
        &mut core,
        within("req-open", &session, AMOUNT, RESERVE, OTHER),
    );
    assert_eq!(state(&mut core, &open), OrderState::FirstSegmentSettled);
    let log = core.log().entries().len();
    assert!(
        matches!(
            core.end(&open, &kit::customer()),
            Err(StepError::Reversal(ReversalError::NotApplicable(_)))
        ),
        "no request ends an order past its first segment"
    );
    let _ = log;
    assert_eq!(state(&mut core, &open), OrderState::FirstSegmentSettled);
    assert!(
        !told_to_the_user(&mut core, &open)
            .iter()
            .any(|(kind, _)| kind == "end_requested"),
        "no end is notified: the machine took none"
    );
    // Nor does it end a complete one.
    let (done, _) = run(
        &mut core,
        within("req-done", &session, AMOUNT, RESERVE, OTHER),
    );
    assert_eq!(state(&mut core, &done), OrderState::Complete);
    assert!(core.end(&done, &kit::customer()).is_err());
    assert_eq!(state(&mut core, &done), OrderState::Complete);
}

// verifies: AC-8
#[test]
fn complete_order_whose_start_is_proven_undone_is_marked_failed_and_notified() {
    let (mut core, session) = core_with("AC-8", &[RESERVE]);
    let (order, ran) = run(
        &mut core,
        within("req-undone", &session, AMOUNT, RESERVE, OTHER),
    );
    assert_eq!(state(&mut core, &order), OrderState::Complete);
    let withdrawal = segments_of(&ran)[0].segment.clone();

    // The origin reserve says no credit arrived: nothing is proven, the order stays complete and nothing is recorded
    // as undone.
    let done = core
        .confirm_undone(&order, &withdrawal)
        .expect("the reserve was asked");
    assert!(
        matches!(
            done,
            Confirmation::NotCompleted {
                why: NotCounted::NotConfirmed,
                ..
            }
        ),
        "{done:?}"
    );
    assert_eq!(state(&mut core, &order), OrderState::Complete);
    assert_eq!(
        count_inputs(&core, &order, |o| matches!(
            o,
            Observed::ReversalCompleted { .. }
        )),
        0
    );

    // The reserve says the credit arrived, in full and in the asset it left in: the start is proven undone, by the
    // origin reserve's own word, said so; the part, and so the order, is marked failed.
    core.world()
        .script(RESERVE, Behaviour::crediting_a_reversal());
    // Asked again a retry cadence after the last question, as every reversal confirmation is (the review of #355's
    // round 2, H1).
    core.clock
        .advance(rutent_core_orch::reversal::DEFAULT_REVERSAL_RETRY_SECONDS);
    let done = core
        .confirm_undone(&order, &withdrawal)
        .expect("the reserve was asked");
    let Confirmation::Completed(completed) = done else {
        panic!("the start is proven undone: {done:?}");
    };
    assert_eq!(completed.whose.as_str(), RESERVE);
    assert_eq!(completed.grade, EvidenceGrade::Statement);
    assert_eq!(completed.word, Word::Reserves);
    assert_eq!(state(&mut core, &order), OrderState::Failed);
    assert_eq!(
        core.followed(&order).expect("followed").parts[0].state,
        OrderState::Failed
    );
    assert!(
        entries(&core).iter().any(|v| matches!(
            &v.what,
            What::Evidence { whose, grade: EvidenceGrade::Statement, segment, .. }
                if whose.as_str() == RESERVE && segment == &withdrawal
        )),
        "recorded as the reserve's own word"
    );

    // The user and the operator are each notified that the order failed, and each is told so as its own.
    let failed = ("reversal_completed".to_owned(), OrderState::Failed);
    assert_eq!(told_to_the_user(&mut core, &order).last(), Some(&failed));
    assert_eq!(
        told_to_the_operator(&mut core, &order).last(),
        Some(&failed)
    );
    // A stranger's inbox is sent nothing of it.
    let stranger = Receiver {
        party: id("another-customer"),
        role: Role::Customer,
    };
    assert!(core.notices_to(&stranger, &order).is_empty());
}

/// A deposit's result by the reserve, signed with its own statements key, that says the payment came back.
fn returned_by(core: &Core, reserve: &str) -> Vec<u8> {
    let mut statement = json!({
        "deployment": rutent_scenarios::assembly::context().deployment,
        "protocol_version": rutent_contracts::provider_role::VERSION,
        "shape": RESULT_SHAPE,
        "maker": reserve,
        "made_at": core.clock.timestamp().as_str(),
        "signature": { "scheme": "ed25519", "key": "", "value": "" },
        "statement": {
            "kind": "deposit",
            "transaction": "tx-returned",
            "outcome": {
                "status": "failed",
                "evidence": [{ "grade": 3, "reference": format!("{reserve}:returned-1"), "by": reserve }],
            },
        },
    });
    rutent_conformance::signing::sign_value(&mut statement, &statements_key(reserve));
    serde_json::to_vec(&statement).expect("bytes")
}

// verifies: AC-9
#[test]
fn payment_returned_after_complete_is_recorded_and_notified() {
    let other = "sim-reserve-a-quick";
    let (mut core, session) = core_with("AC-9", &[RESERVE, other]);
    let (order, _) = run(
        &mut core,
        within("req-returned", &session, AMOUNT, RESERVE, OTHER),
    );
    assert_eq!(state(&mut core, &order), OrderState::Complete);
    let before = (effects(&core), core.inputs().len());

    // Only the destination's report is one: another provider's isn't, and nothing is recorded for it.
    let log = core.log().entries().len();
    let refused = core.returned(&order, &returned_by(&core, other));
    assert!(
        matches!(
            refused,
            Err(StepError::Reversal(ReversalError::NotTheDestination))
        ),
        "{refused:?}"
    );
    assert_eq!(core.log().entries().len(), log, "nothing is recorded");

    // The destination's: kept as its statement about the order, and recorded as returned. The order stays complete.
    let report = returned_by(&core, RESERVE);
    let after = core.returned(&order, &report).expect("recorded");
    assert_eq!(
        (after.event, after.maker.as_str()),
        (Afterwards::Returned, RESERVE)
    );
    assert!(
        entries(&core).iter().any(|v| matches!(
            &v.what,
            What::Afterwards { event: Afterwards::Returned, statement, .. } if *statement == after.statement
        )),
        "the log holds it as returned, resting on the reserve's statement"
    );
    assert!(
        entries(&core)
            .iter()
            .any(|v| matches!(&v.what, What::Evidence { .. })),
        "the order's evidence is still there"
    );
    assert_eq!(
        state(&mut core, &order),
        OrderState::Complete,
        "it stays complete"
    );
    assert_eq!(
        (effects(&core), core.inputs().len()),
        before,
        "nothing moved and no input came: no new route starts"
    );
    assert_eq!(
        core.returned(&order, &report).expect("kept once"),
        after,
        "recorded once"
    );

    // The customer reads it, and is told of it, as the operator is, the order still complete.
    let seen = core
        .after_the_end(&kit::customer(), &order)
        .expect("the customer reads what came after");
    assert_eq!(seen, vec![after]);
    let told = ("afterwards:Returned".to_owned(), OrderState::Complete);
    assert_eq!(told_to_the_user(&mut core, &order).last(), Some(&told));
    assert_eq!(told_to_the_operator(&mut core, &order).last(), Some(&told));

    // An order that isn't complete has no return to record.
    let (mut core, session) = core_with("AC-9", &[RESERVE]);
    core.world()
        .script_for(RESERVE, "deposit", Behaviour::act(Act::Refuse));
    let (open, _) = run(
        &mut core,
        within("req-open", &session, AMOUNT, RESERVE, OTHER),
    );
    let report = returned_by(&core, RESERVE);
    assert!(matches!(
        core.returned(&open, &report),
        Err(StepError::Reversal(ReversalError::NotComplete))
    ));
}

/// How many inputs of `order` in `entries` the state machine applies, with each one's kind and the state after it.
fn applied(
    entries: &[(rutent_contracts::core_internal::Receipt, Entry)],
    order: &Id,
) -> Vec<(String, OrderState)> {
    let mut followed: Option<stm::Order> = None;
    let mut applied = Vec::new();
    for (_, entry) in entries {
        let Entry::Verified(v) = entry else { continue };
        let What::Input { input } = &v.what else {
            continue;
        };
        if input.subject.order.as_str() != order.as_str() {
            continue;
        }
        let kind = serde_json::to_value(&input.observed).expect("json")["kind"]
            .as_str()
            .expect("a kind")
            .to_owned();
        followed = match followed {
            None => Stm::open(input).inspect(|o| applied.push((kind, o.state))),
            Some(o) => {
                let next = Stm::transition(&o, input);
                if next.last_input.outcome == (stm::Outcome::Applied {}) {
                    applied.push((kind, next.state));
                    Some(next)
                } else {
                    Some(o)
                }
            }
        };
    }
    applied
}

// verifies: AC-41
#[test]
fn each_state_change_is_notified_to_the_user_and_the_operator() {
    // An order through several states: its route chosen, its withdrawal settled, its deposit refused and its value
    // recalculated, its second deposit settled and the order complete.
    let (mut core, session) = core_with("AC-41", &[RESERVE]);
    core.world()
        .script_for(RESERVE, "deposit", Behaviour::act(Act::Refuse));
    let (order, _) = run(
        &mut core,
        within("req-states", &session, AMOUNT, RESERVE, OTHER),
    );
    let Ok(recalculated) = core.recalculated_on_tick(&order) else {
        panic!("recalculated")
    };
    let rutent_core_orch::recalc::Recalculation::Recalculated(_) = recalculated else {
        panic!("a route to the same destination: {recalculated:?}");
    };
    core.ran_once_on_tick(&order)
        .expect("the recalculated part runs");
    assert_eq!(state(&mut core, &order), OrderState::Complete);

    // Every change the state machine applies to the order, as the log's own inputs give it, in order.
    let log = core.log().entries().to_vec();
    let expected = applied(&log, &order);
    assert!(
        expected.len() >= 5,
        "the order moved through several states: {expected:?}"
    );
    let mut states: Vec<OrderState> = Vec::new();
    for (_, s) in &expected {
        if !states.contains(s) {
            states.push(*s);
        }
    }
    assert!(
        states.len() >= 3,
        "and through several states: {expected:?}"
    );

    // The user is notified of each, and so is the operator: the same changes, in the order they happened, with the
    // order's state after each, counted from 1 in the log's order, none twice.
    // Core's ticks sent what the run's changes called for so far; what is left goes now.
    core.notify().expect("the notices read the log");
    let to_user = core.notices_to(&Core::user_receiver(), &order);
    let to_operator = core.notices_to(&core.operator_receiver(), &order);
    assert_eq!(changes(&to_user), expected, "each change, to the user");
    assert_eq!(
        changes(&to_operator),
        expected,
        "each change, to the operator"
    );
    for notices in [&to_user, &to_operator] {
        let sequences: Vec<u64> = notices
            .iter()
            .filter_map(|n| match &n.about {
                rutent_core_orch::notice::About::Order { sequence, .. } => Some(*sequence),
                _ => None,
            })
            .collect();
        assert_eq!(
            sequences,
            (1..=u64::try_from(expected.len()).expect("a count")).collect::<Vec<_>>(),
            "in the log's order"
        );
    }
    // Asked again, the log has nothing new: nothing is sent twice.
    assert_eq!(core.notify().expect("read again"), 0);
    // Nobody else is told: a stranger's inbox holds nothing of the order.
    let stranger = Receiver {
        party: id("another-customer"),
        role: Role::Customer,
    };
    assert!(core.notices_to(&stranger, &order).is_empty());
    // The customer's notices name the order's state and each part's, and nothing of a provider or an amount.
    let text = String::from_utf8(
        serde_json::to_vec(
            &to_user
                .iter()
                .map(|n| serde_json::to_value(n).unwrap())
                .collect::<Vec<_>>(),
        )
        .expect("json"),
    )
    .expect("text");
    assert!(!text.contains(RESERVE), "no provider in a notice");
    assert!(!text.contains(AMOUNT), "no amount in a notice");
    let _ = kit::AMOUNT;
}
