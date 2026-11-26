//! What the simulated providers never exercised, over the assembled Core (`T-024-23`, finding 13 of `M3`'s milestone
//! review of 2026-11-24): the fault a failed result shows and the attribution it carries (`REQ-ORD-11`), reversals the
//! provider refuses, leaves unknown or states short (`NFR-5`, `INV-4`), the success branch of `AC-4` with a party
//! other than the provider and its publisher showing the value back, and a late answer the platform receives
//! (`AC-5`) in place of a test driving it. Each drives Core's own `request`, `quote`, `accept` and `run`, and its
//! reversals, over [`execution_support`]'s world; every entry Core writes is admitted by the log on its way in.

mod execution_support;
mod failure_support;

use execution_support::*;
use failure_support::*;
use rutent_conformance::sim::{Act, Behaviour};
use rutent_contracts::core_internal::{Action, Carrier as Recorded, What};
use rutent_contracts::provider_role::Fault;
use rutent_contracts::router_api::Id;
use rutent_contracts::state_machine::{OrderState, SegmentState};
use rutent_core_orch::recalc::Recalculation;
use rutent_core_orch::reversal::{Carrier, Confirmation, NotCounted, ReversalError, Word};
use rutent_core_orch::segments::SegmentOutcome;
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::assembly::{Core, StepError};
use rutent_scenarios::harness::Scenario;
use rutent_scenarios::kit::{self, AMOUNT, RESERVE, id};

const IN: &str = "sim-join-in";
const EXCHANGE: &str = "sim-join-exchange";
const QUICK: &str = "sim-join-ramp-quick";
const OUT: &str = "sim-join-out";

/// The order that carries `AMOUNT` of COP from the reserve `sim-join-in`'s bank to USD at the bank of `sim-join-out`.
fn across(name: &str, session: &Id) -> rutent_core_orch::intake::OrderRequest {
    request(
        name,
        session,
        AMOUNT,
        IN,
        outer("sim-join-in-bank", "COP"),
        outer("sim-join-out-bank", "USD"),
        OUT,
        OTHER,
        "lowest_total_cost",
    )
}

/// What a world of one order whose ramp failed holds: Core, the customer's session, the order and the ramp's segment,
/// the withdrawal and the conversion settled and the ramp's reversal owed.
struct Failed {
    core: Core,
    session: Id,
    order: Id,
    ramp: Id,
}

/// The order across the joined providers, run with its ramp failing as `behaviour` says. The ramp is the segment
/// that fails because it is a provider that declares a repeat safe (`REQ-CAT-10`): asked again, the platform answers
/// from what it holds, the first answer, so its signed failed result can be shown to Core as it was given.
fn ramp_failing(accepts: &'static str, behaviour: Behaviour) -> Failed {
    let (mut core, session) = core_with(accepts, &[IN, EXCHANGE, QUICK, OUT]);
    core.world().script_for(QUICK, "transfer", behaviour);
    let (order, ran) = run(&mut core, across("req-fails", &session));
    let segments = segments_of(&ran);
    assert!(
        segments[..2]
            .iter()
            .all(|s| matches!(s.outcome, SegmentOutcome::Settled { .. })),
        "{segments:?}"
    );
    assert!(
        matches!(segments[2].outcome, SegmentOutcome::Failed),
        "the ramp failed: {segments:?}"
    );
    assert_eq!(segments.len(), 3, "nothing is handed over after it");
    let ramp = segments[2].segment.clone();
    Failed {
        core,
        session,
        order,
        ramp,
    }
}

/// The state of `segment` as Core follows it.
fn segment_state(core: &Core, order: &Id, segment: &Id) -> SegmentState {
    core.followed(order).expect("Core follows the order").parts[0]
        .segments
        .iter()
        .find(|s| s.segment_id.as_str() == segment.as_str())
        .expect("the segment is followed")
        .state
}

/// How many moves of value the log holds: what Core handed to a provider.
fn effects(core: &Core) -> usize {
    entries(core)
        .iter()
        .filter(|v| {
            matches!(
                v.what,
                What::Effect {
                    action: Action::MoveValue { .. },
                    ..
                }
            )
        })
        .count()
}

/// The ramp's failed result for its segment, as it gave it: signed under the segment's own transaction.
fn showing(core: &mut Core, order: &Id, segment: &Id) -> Vec<u8> {
    core.kept_result(order, segment)
        .expect("Core kept the provider's result for the segment")
}

/// Replays the log's history through `admit_entry`, each entry against what came before it, by the handle it came by:
/// policy's decisions through policy's, everything else through the general one.
fn admitted(core: &Core) {
    use rutent_contracts::core_internal::{DeploymentOperator, Through, admit_entry, calls};
    let history: Vec<_> = core.log().entries().to_vec();
    let operator = DeploymentOperator {
        maker: kit::operator().acts,
        party: kit::operator().party,
    };
    for (i, (_, entry)) in history.iter().enumerate() {
        let through = match entry {
            rutent_contracts::core_internal::Entry::Verified(v) if calls::is_decision(&v.what) => {
                Through::Policy
            }
            _ => Through::General,
        };
        if let Err(violation) = admit_entry(&history[..i], entry, through, &operator) {
            panic!("entry {i} isn't admitted: {violation:?}");
        }
    }
}

/// The carrier recorded in the log for `segment`, if the log holds one.
fn recorded_carrier(core: &Core, segment: &Id) -> Option<Recorded> {
    entries(core).iter().find_map(|v| match &v.what {
        What::ReversalCarrier {
            segment: s,
            carrier,
            ..
        } if s == segment => Some(*carrier),
        _ => None,
    })
}

// verifies: REQ-ORD-11
#[test]
fn failed_segments_provider_carries_the_reversal_unless_it_shows_the_fault_was_the_customers_or_the_operators()
 {
    // A provider that shows no fault carries the reversal: its failed result names none, so there is nothing to
    // accept, and the log holds no carrier but the provider's own, which is the default.
    let mut failed = ramp_failing("AC-4", Behaviour::act(Act::Fail));
    let Failed {
        core, order, ramp, ..
    } = &mut failed;
    core.owe_reversal(order, ramp).expect("owed");
    assert_eq!(
        core.reversal_carrier(order, ramp),
        Some(Carrier::Provider(id(QUICK)))
    );
    let bare = showing(core, order, ramp);
    assert!(
        matches!(
            core.show_fault(order, ramp, &bare),
            Err(StepError::Reversal(ReversalError::NoFaultShown))
        ),
        "a failure that names no fault shows none"
    );
    assert_eq!(
        core.reversal_carrier(order, ramp),
        Some(Carrier::Provider(id(QUICK)))
    );
    assert_eq!(recorded_carrier(core, ramp), None);

    // The customer's fault, shown: the provider still carries the reversal until the customer accepts it. The
    // operator can't accept for the customer; the customer's acceptance is policy's permitted decision, read from the
    // log, and from it the customer carries the reversal, recorded with the showing and who accepted it.
    let mut failed = ramp_failing("AC-4", Behaviour::failing_by(Fault::Customer));
    let Failed {
        core,
        session,
        order,
        ramp,
    } = &mut failed;
    core.owe_reversal(order, ramp).expect("owed");
    let shown = showing(core, order, ramp);
    core.show_fault(order, ramp, &shown).expect("shown");
    assert_eq!(
        core.reversal_carrier(order, ramp),
        Some(Carrier::Provider(id(QUICK))),
        "the provider carries it until the party accepts"
    );
    assert!(
        matches!(
            core.accept_fault(order, ramp, &kit::operator(), session),
            Err(StepError::Reversal(ReversalError::NotTheParty))
        ),
        "the signed result names the customer's fault, not the operator's"
    );
    assert_eq!(
        core.accept_fault(order, ramp, &kit::customer(), session)
            .expect("the customer accepts its fault"),
        Carrier::Customer
    );
    assert_eq!(core.reversal_carrier(order, ramp), Some(Carrier::Customer));
    assert_eq!(recorded_carrier(core, ramp), Some(Recorded::Customer));
    admitted(core);
    assert!(
        matches!(
            core.show_fault(order, ramp, &shown),
            Err(StepError::Reversal(ReversalError::FaultSettled))
        ),
        "an accepted showing can't be replaced"
    );

    // The operator's fault, shown: the operator accepts it, the customer can't.
    let mut failed = ramp_failing("AC-4", Behaviour::failing_by(Fault::Operator));
    let Failed {
        core,
        session,
        order,
        ramp,
    } = &mut failed;
    core.owe_reversal(order, ramp).expect("owed");
    let shown = showing(core, order, ramp);
    core.show_fault(order, ramp, &shown).expect("shown");
    assert!(
        matches!(
            core.accept_fault(order, ramp, &kit::customer(), session),
            Err(StepError::Reversal(ReversalError::NotTheParty))
        ),
        "the signed result names the operator's fault, not the customer's"
    );
    assert_eq!(
        core.accept_fault(order, ramp, &kit::operator(), session)
            .expect("the operator accepts its fault"),
        Carrier::Operator
    );
    assert_eq!(recorded_carrier(core, ramp), Some(Recorded::Operator));
    admitted(core);
}

// verifies: REQ-ORD-11
#[test]
fn dispute_over_a_fault_makes_the_order_stuck() {
    for fault in [Fault::Customer, Fault::Operator] {
        let mut failed = ramp_failing("AC-4", Behaviour::failing_by(fault));
        let Failed {
            core, order, ramp, ..
        } = &mut failed;
        core.owe_reversal(order, ramp).expect("owed");
        let shown = showing(core, order, ramp);
        core.show_fault(order, ramp, &shown).expect("shown");
        let (party, other) = match fault {
            Fault::Customer => (kit::customer(), kit::operator()),
            Fault::Operator => (kit::operator(), kit::customer()),
        };
        let part = core.part_of(order).expect("a part");
        assert_eq!(state(core, order), OrderState::FirstSegmentSettled);
        assert!(!core.is_stuck(order, &part));
        assert!(
            matches!(
                core.dispute_fault(order, ramp, &other),
                Err(StepError::Reversal(ReversalError::NotTheParty))
            ),
            "only the party the showing names disputes it"
        );
        assert!(!core.is_stuck(order, &part));
        core.dispute_fault(order, ramp, &party).expect("disputed");
        assert_eq!(state(core, order), OrderState::Stuck, "{fault:?}");
        assert!(core.is_stuck(order, &part));
        assert_eq!(core.stuck_values().len(), 1, "the stuck value is held");
        assert_eq!(
            core.reversal_carrier(order, ramp),
            Some(Carrier::Provider(id(QUICK))),
            "the provider carries it while it is disputed"
        );
        core.notify().expect("notices");
        assert_eq!(
            changes(&core.notices_to(&core.operator_receiver(), order)).last(),
            Some(&("stuck_declared".to_owned(), OrderState::Stuck)),
            "the operator is warned"
        );
        assert!(
            matches!(
                core.accept_fault(order, ramp, &party, &id("unused")),
                Err(StepError::Reversal(
                    ReversalError::NoShowing | ReversalError::NotAnAcceptance
                ))
            ),
            "a disputed fault isn't accepted"
        );
        // Its reversal still completes on the stuck part, and the order stays stuck.
        let asked = core.ask_reversal(order, ramp).expect("asked");
        assert!(asked.completed.is_some(), "{:?}", asked.why);
        assert_eq!(state(core, order), OrderState::Stuck);
        admitted(core);
    }
}

// verifies: NFR-5, INV-4
#[test]
fn refused_unknown_and_short_reversals_each_leave_the_order_stuck_with_nothing_moved_twice() {
    // A reversal that never completes leaves the segment owed, no route starts from the value, nothing moves again,
    // and the order, timed from the failure, is declared stuck once its window passes (`T-020-47`).
    for (name, behaviour, why) in [
        ("refused", Behaviour::act(Act::Refuse), NotCounted::Refused),
        (
            "unknown",
            Behaviour::act(Act::Unknown),
            NotCounted::NotConfirmed,
        ),
        (
            "short",
            Behaviour::short_by(100),
            NotCounted::NotInFull {
                stated: "990000".to_owned(),
            },
        ),
    ] {
        let mut failed = ramp_failing("AC-4", Behaviour::act(Act::Fail));
        let Failed {
            core, order, ramp, ..
        } = &mut failed;
        let moves = effects(core);
        core.owe_reversal(order, ramp).expect("owed");
        core.world().script_for(QUICK, "reversal", behaviour);
        let asked = core.ask_reversal(order, ramp).expect("asked");
        assert!(asked.completed.is_none(), "{name}: {asked:?}");
        assert_eq!(asked.why, Some(why), "{name}");
        assert_eq!(
            segment_state(core, order, ramp),
            SegmentState::FailedReversalOwed,
            "{name}: still owed"
        );
        assert!(core.completed_reversal(order, ramp).is_none(), "{name}");
        assert_eq!(state(core, order), OrderState::FirstSegmentSettled);
        assert!(
            matches!(
                core.recalculate(order).expect("tried"),
                Recalculation::NotNow(_)
            ),
            "{name}: no route starts from value in motion"
        );
        core.clock.advance(7 * 86_400);
        let _ = core.watch(order);
        assert_eq!(
            state(core, order),
            OrderState::Stuck,
            "{name}: the owed reversal is over its window"
        );
        let _ = core.run(order);
        // Nothing moved twice: no move of value beyond the route's three, the withdrawal and the conversion were
        // asked once each, and no reserve was asked to pay out.
        assert_eq!(effects(core), moves, "{name}");
        assert_eq!(core.world().journal(IN).len(), 1, "{name}");
        assert_eq!(core.world().journal(EXCHANGE).len(), 1, "{name}");
        assert_eq!(core.world().journal(OUT).len(), 0, "{name}");
        // Asked again, an unknown or short reversal is not completed by a second undo.
        if name != "refused" {
            let again = core.ask_reversal(order, ramp).expect("asked again");
            assert!(again.completed.is_none(), "{name}: {again:?}");
        } else {
            // A refusal moved nothing, so the same undo asked again is the one that happens, once.
            let again = core.ask_reversal(order, ramp).expect("asked again");
            assert!(again.completed.is_some(), "{name}: {again:?}");
        }
        assert_eq!(effects(core), moves, "{name}");
        admitted(core);
    }
}

/// A reversal the provider refuses, leaves unknown or states short stays owed, and the order it holds the value of is
/// declared stuck once the failed segment's window passes, never before (`INV-3`'s last branch, `REQ-ORD-3`): such an
/// order doesn't rest at `FailedReversalOwed` for ever.
// verifies: INV-3, REQ-ORD-3
#[test]
fn owed_reversal_refused_short_or_unknown_past_its_window_makes_its_order_stuck() {
    use rutent_core_orch::stuck::Watch;
    for (name, behaviour) in [
        ("refused", Behaviour::act(Act::Refuse)),
        ("unknown", Behaviour::act(Act::Unknown)),
        ("short", Behaviour::short_by(100)),
    ] {
        let mut failed = ramp_failing("AC-4", Behaviour::act(Act::Fail));
        let Failed {
            core, order, ramp, ..
        } = &mut failed;
        core.owe_reversal(order, ramp).expect("owed");
        core.world().script_for(QUICK, "reversal", behaviour);
        let asked = core.ask_reversal(order, ramp).expect("asked");
        assert!(asked.completed.is_none(), "{name}: {asked:?}");
        // Inside its window nothing is declared.
        let early = core.watch(order).expect("watched");
        assert!(matches!(early, Watch::Running), "{name}: {early:?}");
        assert_eq!(
            state(core, order),
            OrderState::FirstSegmentSettled,
            "{name}"
        );
        core.clock.advance(7 * 86_400);
        let late = core.watch(order).expect("watched");
        assert!(!matches!(late, Watch::Running), "{name}: {late:?}");
        assert_eq!(state(core, order), OrderState::Stuck, "{name}");
        assert_eq!(
            segment_state(core, order, ramp),
            SegmentState::FailedReversalOwed,
            "{name}: still owed, and now to the stuck order's resolution"
        );
        admitted(core);
    }
}

/// What the ramp's transfer, left unknown by a ramp that says later that it failed, comes to: Core's run asks the ramp,
/// which declares a status query, where it stands, and the answer, a failed result of the transfer's own transaction,
/// is kept as the ramp's statement.
fn ramp_left_unknown() -> Failed {
    let (mut core, session) = core_with("AC-5", &[IN, EXCHANGE, QUICK, OUT]);
    core.world()
        .script_for(QUICK, "transfer", Behaviour::unknown_that_failed());
    let (order, ran) = run(&mut core, across("req-unknown", &session));
    let segments = segments_of(&ran);
    assert!(
        matches!(segments[2].outcome, SegmentOutcome::Unknown(_)),
        "the ramp's outcome is not known: {segments:?}"
    );
    let ramp = segments[2].segment.clone();
    Failed {
        core,
        session,
        order,
        ramp,
    }
}

/// Another provider's statement about the ramp's transfer, as the exchange of the route would make it: of another
/// maker than the ramp, so it states nothing of the move itself and only backs a confirmation.
fn by_another_party(core: &mut Core, order: &Id, ramp: &Id) -> Vec<u8> {
    let kept = core.kept_result(order, ramp).expect("kept");
    let mut word: serde_json::Value = serde_json::from_slice(&kept).expect("JSON");
    word["maker"] = OUT.into();
    word["made_at"] = core.clock.timestamp().as_str().into();
    rutent_conformance::signing::sign_value(&mut word, &statements_key(OUT));
    serde_json::to_vec(&word).unwrap()
}

/// Core's schedule declares the order stuck past its time and reports the failure the provider stated, its own answer
/// to Core's status question as Core kept it, to the stuck mover; another actor confirms it with another maker's
/// statement: the segment ends failed. No test watches the order or hands an answer in (`T-020-48`, the review's S4).
fn ended_failed(core: &mut Core, order: &Id, ramp: &Id) {
    core.clock.advance(2 * kit::DECLARED + 5);
    let round = core.tick();
    assert!(
        round.watched.iter().any(|w| &w.order == order
            && matches!(w.found, Ok(rutent_core_orch::stuck::Watch::Stuck(_)))),
        "the tick declared it stuck: {:?}",
        round.watched
    );
    let report = round
        .reported
        .iter()
        .find_map(|r| r.taken.as_ref().ok().map(|v| (*v, r.statement.clone())))
        .unwrap_or_else(|| panic!("the mover's report was taken: {:?}", round.reported));
    assert!(core.is_unknown(order, ramp), "reported, not yet confirmed");
    let other = by_another_party(core, order, ramp);
    // A person at another desk than the reporter's, whom policy lets confirm an outcome.
    let confirmer = rutent_contracts::core_internal::Actor {
        party: id("desk-b"),
        acts: id("bob"),
        role: rutent_contracts::core_internal::Role::Operator,
        kind: rutent_contracts::core_internal::ActorKind::Person,
    };
    {
        let mut registers = core.registers();
        registers.list(&rutent_contracts::core_internal::Actor {
            party: kit::operator().party,
            acts: id("desk-b"),
            role: rutent_contracts::core_internal::Role::Operator,
            kind: rutent_contracts::core_internal::ActorKind::Component,
        });
        registers.list(&confirmer);
        registers.permit(
            &confirmer,
            rutent_core_authz::roles::Permission::ConfirmOutcome,
        );
    }
    let moved = core
        .confirm_reported_failure(order, ramp, &confirmer, (report.0, &report.1), &other)
        .expect("another actor confirms it");
    assert!(
        matches!(moved, rutent_core_orch::stuck::Moved::CarriedOut { .. }),
        "{moved:?}"
    );
}

/// How many questions of the status of a segment Core's log holds an effect of.
fn status_asks(core: &Core) -> usize {
    entries(core)
        .iter()
        .filter(|v| {
            matches!(
                v.what,
                What::Effect {
                    action: Action::AskStatus { .. },
                    ..
                }
            )
        })
        .count()
}

/// A segment the run left unknown, at a provider that declares a status query, is asked where it stands by Core's own
/// run, under a transaction of Core's own; the provider answers that its transfer failed, and the answer is kept as
/// the provider's statement of the segment's transaction (`REQ-ORD-1`, `T-020-47`).
// verifies: REQ-ORD-1
#[test]
fn segment_left_unknown_is_asked_its_status_and_ends_failed_on_the_providers_answer() {
    let Failed {
        mut core,
        order,
        ramp,
        ..
    } = ramp_left_unknown();
    let asked = core
        .status_asked(&order, &ramp)
        .cloned()
        .expect("Core asked")
        .expect("the question was put");
    let receipt = asked.answer.clone().expect("the answer was kept");
    assert_eq!(status_asks(&core), 1);
    // The answer is the ramp's own result of the transfer's transaction, and it says the transfer failed.
    let bytes = core.kept_result(&order, &ramp).expect("kept");
    let kept: serde_json::Value = serde_json::from_slice(&bytes).expect("JSON");
    assert_eq!(kept["maker"], QUICK);
    assert_eq!(kept["statement"]["kind"], "transfer");
    assert_eq!(kept["statement"]["outcome"]["status"], "failed", "{kept}");
    assert_ne!(
        kept["statement"]["transaction"].as_str(),
        Some(asked.transaction.as_str()),
        "it is about the transfer's transaction, not the question's own"
    );
    // The entry is the log's, about the order and the segment.
    assert!(core.log().entries().iter().any(|(r, e)| {
        *r == receipt
            && matches!(e, rutent_contracts::core_internal::Entry::Stated(s)
                if s.about.as_ref().and_then(|a| a.segment.as_ref()) == Some(&ramp))
    }));
    // Asked once: a run again asks no more.
    core.run(&order).expect("runs again");
    assert_eq!(status_asks(&core), 1);
    // Core only keeps the answer, and moves nothing: the segment is unknown still. It ends failed on the provider's
    // answer through the stuck mover: the schedule reports it, another actor confirms it (`AC-6`, `T-020-48`).
    assert!(core.is_unknown(&order, &ramp));
    ended_failed(&mut core, &order, &ramp);
    assert_eq!(
        segment_state(&core, &order, &ramp),
        SegmentState::FailedReversalOwed,
        "the segment ended failed, its reversal owed"
    );
    admitted(&core);
}

/// Core's answer to a status question moves nothing: the segment is as it was, unknown, no input or evidence is
/// recorded on the answer, no move of value follows it, and the order stays where it was (`INV-4`).
// verifies: INV-4
#[test]
fn status_answer_moves_nothing() {
    let Failed {
        mut core,
        order,
        ramp,
        ..
    } = ramp_left_unknown();
    let asked = core
        .status_asked(&order, &ramp)
        .cloned()
        .expect("Core asked")
        .expect("the question was put");
    assert!(asked.answer.is_some());
    assert_eq!(
        effects(&core),
        3,
        "the withdrawal, the conversion and the transfer, and no more"
    );
    assert_eq!(
        segment_state(&core, &order, &ramp),
        SegmentState::OutcomeUnknown
    );
    assert!(matches!(
        core.followed(&order).expect("followed").parts[0].segments[2].state,
        SegmentState::OutcomeUnknown
    ));
    assert_eq!(state(&mut core, &order), OrderState::FirstSegmentSettled);
    // No input follows the question's effect but its own result: nothing the state machine is fed.
    let log = entries(&core);
    let asking = log
        .iter()
        .position(|v| {
            matches!(
                v.what,
                What::Effect {
                    action: Action::AskStatus { .. },
                    ..
                }
            )
        })
        .expect("the question's effect");
    assert!(
        log[asking..]
            .iter()
            .all(|v| !matches!(v.what, What::Input { .. } | What::Evidence { .. })),
        "nothing moved on the answer: {:?}",
        &log[asking..]
    );
    admitted(&core);
}

/// A segment still unknown is asked its status again by Core's schedule, at its provider's cadence and no sooner: the
/// run asked once, the schedule asks again under a fresh transaction once the cadence has passed, and what the provider
/// answers is the result it gave, held, so nothing new is kept and nothing moves (`REQ-ORD-1`, `T-020-48`).
// verifies: REQ-ORD-1
#[test]
fn unknown_segment_is_asked_again_at_its_providers_cadence() {
    let Failed {
        mut core,
        order,
        ramp,
        ..
    } = ramp_left_unknown();
    assert_eq!(status_asks(&core), 1, "the run asked once");
    let first = core
        .status_asked(&order, &ramp)
        .cloned()
        .expect("Core asked")
        .expect("the question was put");
    core.set_status_cadence(QUICK, 600);
    let effects_before = effects(&core);
    let results_before = core.log().entries().len();
    // Not due: its provider's cadence counts from the question the run put.
    assert!(core.tick().statuses.is_empty(), "not due yet");
    core.clock.advance(599);
    assert!(core.tick().statuses.is_empty(), "599 seconds is not 600");
    assert_eq!(status_asks(&core), 1);
    // Due: asked again, under a fresh transaction, and what comes of it is held.
    core.clock.advance(1);
    let round = core.tick();
    assert_eq!(round.statuses.len(), 1, "{:?}", round.statuses);
    let again = &round.statuses[0];
    assert_eq!(
        (&again.unknown.order, &again.unknown.segment),
        (&order, &ramp)
    );
    assert!(again.asked.is_ok(), "{again:?}");
    assert_eq!(status_asks(&core), 2);
    assert_ne!(
        core.status_asked(&order, &ramp)
            .and_then(|a| a.as_ref().ok())
            .map(|a| a.transaction.clone()),
        Some(first.transaction.clone()),
        "a fresh transaction of Core's own"
    );
    assert_eq!(
        again.kept, first.answer,
        "the provider's answer is the result it gave, held: the one entry"
    );
    // Nothing moved on it, and the segment is as it was.
    assert_eq!(effects(&core), effects_before, "no value moves");
    assert!(core.is_unknown(&order, &ramp));
    assert!(core.log().entries().len() > results_before);
    // And not again before its cadence has passed once more.
    core.clock.advance(599);
    assert!(core.tick().statuses.is_empty());
    core.clock.advance(1);
    assert_eq!(core.tick().statuses.len(), 1);
    assert_eq!(status_asks(&core), 3);
    admitted(&core);
}

/// What a provider says of a stuck move, kept from Core's status question, reaches the move's mover: the schedule
/// hands the kept statement to the stuck mover's report, so a segment its provider says failed is reported, not left
/// unknown until its window. Nothing is settled by it until another actor confirms (`REQ-ORD-1`, `AC-6`, `T-020-48`).
// verifies: REQ-ORD-1, AC-6
#[test]
fn segment_its_provider_says_failed_is_reported_to_the_stuck_mover_through_the_schedule() {
    let Failed {
        mut core,
        order,
        ramp,
        ..
    } = ramp_left_unknown();
    let reports = |core: &Core| {
        entries(core)
            .iter()
            .filter(|v| matches!(v.what, What::ReportOutcome { .. }))
            .count()
    };
    let kept_of_the_ramp = |core: &Core| {
        core.log()
            .entries()
            .iter()
            .filter(|(_, e)| {
                matches!(e, rutent_contracts::core_internal::Entry::Stated(s)
                    if s.about.as_ref().and_then(|a| a.segment.as_ref()) == Some(&ramp))
            })
            .count()
    };
    // The provider's own answer to the question Core put when the run left the segment unknown, as Core kept it.
    let answer = core
        .status_asked(&order, &ramp)
        .cloned()
        .expect("Core asked")
        .expect("the question was put")
        .answer
        .expect("the ramp answered");
    // Not stuck yet: nothing is reported, whatever the provider said.
    assert!(core.tick().reported.is_empty());
    let kept_before = kept_of_the_ramp(&core);
    // Past its time the tick declares it stuck and hands the provider's own answer to the mover's report.
    core.clock.advance(2 * kit::DECLARED + 5);
    let round = core.tick();
    assert_eq!(state(&mut core, &order), OrderState::Stuck);
    let taken: Vec<_> = round.reported.iter().filter(|r| r.taken.is_ok()).collect();
    assert_eq!(taken.len(), 1, "{:?}", round.reported);
    assert_eq!(taken[0].unknown.segment, ramp);
    assert_eq!(
        taken[0].statement, answer,
        "the report rests on the provider's own answer, the entry Core kept"
    );
    assert_eq!(
        kept_of_the_ramp(&core),
        kept_before,
        "handed on, it is kept once: no second entry of the same statement"
    );
    assert_eq!(reports(&core), 1, "the log holds the mover's report");
    // The report settles nothing: the move is unknown until another actor confirms.
    assert!(core.is_unknown(&order, &ramp));
    // Told once: a later tick takes none again.
    core.clock.advance(60);
    assert!(core.tick().reported.iter().all(|r| r.taken.is_err()));
    assert_eq!(reports(&core), 1);
    admitted(&core);
}

/// What a provider says of a stuck move, kept, reaches the move's mover whichever way it says it went: a kept answer that
/// says the move settled is reported as done, as one that says it failed is reported as failed (`REQ-ORD-1`, `AC-6`,
/// `T-020-48`). Nothing is settled by it until another actor confirms it.
// verifies: REQ-ORD-1, AC-6
#[test]
fn segment_its_provider_says_settled_is_reported_to_the_stuck_mover_through_the_schedule() {
    let (mut core, session) = core_with("AC-5", &[RESERVE]);
    let order = accepted(
        &mut core,
        within("req-says-settled", &session, AMOUNT, RESERVE, OTHER),
    );
    core.world()
        .script_for(RESERVE, "deposit", Behaviour::late());
    let ran = core.run(&order).expect("the order ran");
    let deposit = segments_of(&ran)[1].segment.clone();
    assert!(core.is_unknown(&order, &deposit));
    // The reserve's own word that the deposit settled, its answer to Core's status question, as Core kept it.
    let kept = core.kept_result(&order, &deposit).expect("kept");
    let word: serde_json::Value = serde_json::from_slice(&kept).expect("JSON");
    assert_eq!(
        word["statement"]["outcome"]["status"], "succeeded",
        "{word}"
    );
    let outcomes = |core: &Core| -> Vec<rutent_contracts::core_internal::MoveOutcome> {
        entries(core)
            .iter()
            .filter_map(|v| match &v.what {
                What::ReportOutcome { request, .. } => Some(request.outcome),
                _ => None,
            })
            .collect()
    };
    assert!(outcomes(&core).is_empty());
    // Past its time the tick declares it stuck and reports what the reserve said.
    core.clock.advance(2 * kit::DECLARED + 5);
    let round = core.tick();
    assert!(
        round.reported.iter().any(|r| r.taken.is_ok()),
        "{:?}",
        round.reported
    );
    assert_eq!(
        outcomes(&core),
        [rutent_contracts::core_internal::MoveOutcome::Done],
        "reported as done"
    );
    assert!(core.is_unknown(&order, &deposit), "settled by nothing yet");
    admitted(&core);
}

/// A segment Core asked the status of while it was unknown is not asked again by a Core that reopened on the log: the
/// log holds the question's effect and its result (`T-020-47`).
// verifies: REQ-ORD-1
#[test]
fn unknown_segment_already_asked_is_not_asked_again_after_core_reopens() {
    let Failed {
        core, order, ramp, ..
    } = ramp_left_unknown();
    assert_eq!(status_asks(&core), 1);
    let given = execution_support::given(&[IN, EXCHANGE, QUICK, OUT]);
    let mut core = Scenario::new("execution", "AC-5", given).reopen(core, 1, &mut Reference);
    assert!(
        core.is_unknown(&order, &ramp),
        "the reopened Core still holds the move unknown"
    );
    core.ask_unknown_statuses(&order)
        .expect("the reopened Core takes it up");
    assert!(
        core.status_asked(&order, &ramp).is_none(),
        "no question was put again, nor refused: {:?}",
        core.status_asked(&order, &ramp)
    );
    assert_eq!(
        status_asks(&core),
        1,
        "asked once, not again by the Core that reopened"
    );
    admitted(&core);
}

/// What Core keeps of a segment's result after its status question is that one answer: another statement of the segment's
/// result handed to it later, made at another time, is the same result held, never a second result statement for the
/// segment (`INV-4`, `T-020-47`).
// verifies: INV-4
#[test]
fn status_answer_is_never_a_second_result_statement_for_its_segment() {
    let Failed {
        mut core,
        order,
        ramp,
        ..
    } = ramp_left_unknown();
    let asked = core
        .status_asked(&order, &ramp)
        .cloned()
        .expect("Core asked")
        .expect("the question was put");
    let receipt = asked.answer.expect("the answer was kept");
    let results = |core: &Core| {
        core.log()
            .entries()
            .iter()
            .filter(|(_, e)| {
                matches!(e, rutent_contracts::core_internal::Entry::Stated(s)
                    if s.what.statement.as_str().ends_with("result")
                        && s.about.as_ref().and_then(|a| a.segment.as_ref()) == Some(&ramp)
                        && s.about.as_ref().and_then(|a| a.order.as_ref()) == Some(&order))
            })
            .count()
    };
    let before = results(&core);
    // The provider's result as it would come again, made at another time.
    let kept = core.kept_result(&order, &ramp).expect("kept");
    let mut value: serde_json::Value = serde_json::from_slice(&kept).expect("JSON");
    value["made_at"] = "2026-11-06T15:04:06Z".into();
    let again = serde_json::to_vec(&value).expect("JSON");
    assert_ne!(again, kept);
    let second = core
        .late_delivery(&order, &ramp, &again)
        .expect("taken as what is held");
    assert_eq!(second, receipt, "the one result, the one entry");
    assert_eq!(results(&core), before, "no second result statement");
    admitted(&core);
}

/// A later statement of another body than the result Core's status question brought is kept beside it, a further
/// statement that moves nothing: the log holds what the provider said twice (`REQ-AUD-2`, `REQ-AUD-5`, `INV-4`,
/// `T-020-47`).
// verifies: INV-4, REQ-AUD-5
#[test]
fn status_answer_and_a_later_statement_of_another_body_are_both_kept_and_move_nothing() {
    let Failed {
        mut core,
        order,
        ramp,
        ..
    } = ramp_left_unknown();
    let asked = core
        .status_asked(&order, &ramp)
        .cloned()
        .expect("Core asked")
        .expect("the question was put");
    let receipt = asked.answer.expect("the answer was kept");
    let kept = core.kept_result(&order, &ramp).expect("kept");
    let mut value: serde_json::Value = serde_json::from_slice(&kept).expect("JSON");
    value["statement"]["outcome"] = serde_json::json!({ "status": "unknown" });
    let other = serde_json::to_vec(&value).expect("JSON");
    let (log_before, inputs_before, effects_before) = (
        core.log().entries().len(),
        core.inputs().len(),
        effects(&core),
    );
    let second = core
        .late_delivery(&order, &ramp, &other)
        .expect("kept as a further statement");
    assert_ne!(second, receipt, "not the result held");
    assert_eq!(
        core.log().entries().len(),
        log_before + 1,
        "one more statement"
    );
    assert_eq!(core.inputs().len(), inputs_before, "no input");
    assert_eq!(effects(&core), effects_before, "no value moves");
    assert!(matches!(
        core.followed(&order).expect("followed").parts[0].segments[2].state,
        SegmentState::OutcomeUnknown
    ));
    admitted(&core);
}

/// The transactions of the reversals Core carried out are held again by a Core that reopened: a private node's record
/// that lists one is still the order's own (`REQ-ORD-41`, `T-020-47`).
// verifies: REQ-ORD-41
#[test]
fn reversals_core_carried_out_are_known_again_after_core_reopens() {
    let mut failed = ramp_failing("AC-4", Behaviour::act(Act::Fail));
    let Failed {
        core, order, ramp, ..
    } = &mut failed;
    core.owe_reversal(order, ramp).expect("owed");
    core.ask_reversal(order, ramp).expect("asked");
    let before = core.reversal_transactions(order);
    assert_eq!(before.len(), 1, "one reversal Core carried out");
    let order = order.clone();
    let Failed { core, .. } = failed;
    let given = execution_support::given(&[IN, EXCHANGE, QUICK, OUT]);
    let core = Scenario::new("execution", "AC-4", given).reopen(core, 1, &mut Reference);
    assert_eq!(core.reversal_transactions(&order), before);
}

/// The duo world's route: `AMOUNT` of COP from `sim-duo-in`'s bank, converted by an exchange of another publisher at
/// the public ledger `sim-duo-in`'s node is, and deposited at a reserve of a third in USD.
const DUO_IN: &str = "sim-duo-in";
const DUO_EXCHANGE: &str = "sim-duo-exchange";
const DUO_OUT: &str = "sim-duo-out-usd";

// verifies: AC-4
#[test]
fn value_undone_at_its_providers_cost_is_shown_back_by_another_party() {
    let (mut core, session) = core_with("AC-4", &[DUO_IN, DUO_EXCHANGE, DUO_OUT]);
    core.world()
        .script_for(DUO_EXCHANGE, "conversion", Behaviour::act(Act::Fail));
    let (order, ran) = run(
        &mut core,
        request(
            "req-duo",
            &session,
            AMOUNT,
            DUO_IN,
            outer("sim-duo-in-bank", "COP"),
            outer("sim-duo-out-usd-bank", "USD"),
            DUO_OUT,
            OTHER,
            "lowest_total_cost",
        ),
    );
    let segments = segments_of(&ran);
    assert!(
        matches!(segments[1].outcome, SegmentOutcome::Failed),
        "{segments:?}"
    );
    let exchange = segments[1].segment.clone();
    // The exchange and the origin reserve are of two publishers.
    let publishers = core.catalogue().publishers();
    assert_ne!(
        publishers.publisher(DUO_IN),
        publishers.publisher(DUO_EXCHANGE)
    );
    core.owe_reversal(&order, &exchange).expect("owed");

    // The exchange's own word that it undid the segment completes nothing: it is the failed provider's.
    let asked = core.ask_reversal(&order, &exchange).expect("asked");
    assert!(asked.completed.is_none());
    assert_eq!(asked.why, Some(NotCounted::FailedProvidersOwn));
    // The origin reserve says no credit arrived: nothing completes. Then it says one arrived short: nor that.
    let done = core.confirm_reversal(&order, &exchange).expect("asked");
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
    core.world().script(
        DUO_IN,
        Behaviour {
            reversal_credited: true,
            ..Behaviour::short_by(100)
        },
    );
    let done = core.confirm_reversal(&order, &exchange).expect("asked");
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
    assert_eq!(
        segment_state(&core, &order, &exchange),
        SegmentState::FailedReversalOwed
    );
    // The credit arrived in full: the origin reserve, another party than the exchange and its publisher, shows it
    // back at the node, as its word.
    core.world()
        .script(DUO_IN, Behaviour::crediting_a_reversal());
    let Confirmation::Completed(completed) =
        core.confirm_reversal(&order, &exchange).expect("asked")
    else {
        panic!("the origin reserve's word completes the reversal");
    };
    assert_eq!(completed.whose, id(DUO_IN));
    assert_eq!(completed.word, Word::Reserves);
    assert_eq!(
        segment_state(&core, &order, &exchange),
        SegmentState::ReversalComplete
    );
    // The route is recalculated from there, to the same destination and asset, and runs to the end.
    let Recalculation::Recalculated(recalculated) = core.recalculate(&order).expect("tried") else {
        panic!("a route from where the value is shown back");
    };
    assert_eq!(recalculated.route.to_asset.id, "USD");
    assert_eq!(
        Some(recalculated.route.destination.clone()),
        core.requested_destination(&order)
    );
    admitted(&core);
}

// verifies: AC-5, INV-4
#[test]
fn late_answer_the_platform_receives_moves_nothing_twice() {
    let (mut core, session) = core_with("AC-5", &[RESERVE]);
    let order = accepted(
        &mut core,
        within("req-late", &session, AMOUNT, RESERVE, OTHER),
    );
    core.world()
        .script_for(RESERVE, "deposit", Behaviour::late());
    let ran = core.run(&order).expect("the order ran");
    let segments = segments_of(&ran);
    assert!(
        matches!(segments[1].outcome, SegmentOutcome::Unknown(_)),
        "the deposit's outcome is not known: {segments:?}"
    );
    // The provider's answer comes late and the platform hands it on: Core's own schedule takes it, with no call of a
    // test's (`T-020-48`). Core's own status question, asked when the segment was left unknown, already brought the
    // provider's result of it, kept once as its statement: the late answer is that result held, never a second result
    // statement for the segment (`T-020-47`).
    let results = |core: &Core| {
        core.log()
            .entries()
            .iter()
            .filter(|(_, e)| {
                matches!(e, rutent_contracts::core_internal::Entry::Stated(s)
                    if s.what.statement.as_str().ends_with("result")
                        && s.about.as_ref().and_then(|a| a.order.as_ref()) == Some(&order))
            })
            .count()
    };
    core.set_cadence(rutent_core_orch::schedule::Job::StatusAsks, u64::MAX);
    core.set_cadence(rutent_core_orch::schedule::Job::StuckReports, u64::MAX);
    let (results_before, effects_before) = (results(&core), effects(&core));
    let round = core.tick();
    assert_eq!(round.late.len(), 1, "one late answer: {:?}", round.late);
    assert!(round.late[0].kept.is_ok(), "{:?}", round.late[0]);
    assert_eq!(results(&core), results_before, "no second statement");
    assert!(core.tick().late.is_empty(), "the platform takes it once");
    assert_eq!(results(&core), results_before);
    // Past its time the order is stuck.
    core.clock.advance(2 * kit::DECLARED + 5);
    assert!(matches!(
        core.watch(&order).expect("watched"),
        rutent_core_orch::stuck::Watch::Stuck(_)
    ));
    let held = core.stuck_values();
    // The part's being declared stuck is an input of its own: what follows is held to it.
    let inputs_before = core.inputs().len();
    let log_before = core.log().entries().len();
    // The same answer handed to Core directly, past the fleet's once: Core itself moves nothing and records nothing new.
    let segment = id(
        core.followed(&order).expect("followed").parts[0].segments[1]
            .segment_id
            .as_str(),
    );
    let late = core
        .provider_again(&order, &segment)
        .expect("the provider's answer, as it gave it late");
    core.late_delivery(&order, &segment, &late)
        .expect("kept once more");
    assert_eq!(core.log().entries().len(), log_before, "nothing new");
    assert_eq!(core.inputs().len(), inputs_before, "no input");
    assert_eq!(effects(&core), effects_before, "no value moves");
    assert_eq!(state(&mut core, &order), OrderState::Stuck);
    assert_eq!(core.stuck_values(), held);
    assert_eq!(
        core.world().journal(RESERVE).len(),
        2,
        "the provider's books grew only by its own withdrawal and deposit"
    );
    admitted(&core);
}

/// Core's tick alone times each followed order's part and declares it stuck past its time, sends the notices the log's
/// changes call for, compares the pairs of adjacent reports the segments' ends gave, and issues and publishes the
/// checkpoints due on the schedule fixed in advance: no test calls `watch`, `notify`, `write_pairs` or a checkpointer
/// (`REQ-ORD-29`, `REQ-ORD-20`, `REQ-ORD-9`, `REQ-AUD-14`, `T-020-48`; the review of its first build, its B3).
// verifies: REQ-ORD-3, REQ-AUD-14
#[test]
fn stuck_watch_notices_pairs_and_checkpoints_run_on_cores_schedule() {
    use rutent_core_orch::schedule::Job;
    use rutent_scenarios::assembly::checking::{CHECKPOINT_EVERY, checker, history_of};
    let Failed {
        mut core, order, ..
    } = ramp_left_unknown();
    let round = core.tick();
    for job in [Job::Watch, Job::Notify, Job::WritePairs, Job::Checkpoints] {
        assert!(round.did(job), "{job:?} runs on Core's first tick");
    }
    // The notices the run's changes call for went with the tick: none is left for a caller.
    assert!(
        matches!(round.notified, Some(Ok(sent)) if sent > 0),
        "{:?}",
        round.notified
    );
    assert_eq!(core.notify().expect("notified"), 0, "the tick sent them");
    // The pairs of reports the run's settled segments gave were compared by the tick.
    assert!(
        matches!(round.pairs, Some(Ok(written)) if written > 0),
        "{:?}",
        round.pairs
    );
    assert!(
        entries(&core)
            .iter()
            .any(|v| matches!(v.what, What::ReportPair { .. }))
    );
    assert_eq!(core.write_pairs(), Ok(0), "none left for a caller");

    // Inside its time the part runs; past it, the tick declares it stuck, with no test's watch.
    assert_eq!(state(&mut core, &order), OrderState::FirstSegmentSettled);
    core.clock.advance(2 * kit::DECLARED + 5);
    let round = core.tick();
    assert_eq!(
        state(&mut core, &order),
        OrderState::Stuck,
        "the tick timed it"
    );
    assert!(
        round
            .watched
            .iter()
            .any(|w| w.order == order
                && matches!(w.found, Ok(rutent_core_orch::stuck::Watch::Stuck(_)))),
        "{:?}",
        round.watched
    );
    assert!(
        matches!(round.notified, Some(Ok(sent)) if sent > 0),
        "the stuck order's notice went with the same tick: {:?}",
        round.notified
    );
    // A later tick declares it no more.
    core.clock.advance(60);
    assert!(core.tick().watched.is_empty());

    // The checkpoints fall on the schedule fixed in advance, an hour apart from the scenario's start, however busy the
    // log: each due is issued and published once, and an outside checker holding the history finds nothing wrong.
    core.clock.advance(3 * CHECKPOINT_EVERY);
    core.tick();
    let published = core.published_checkpoints().to_vec();
    assert!(published.len() >= 3, "{} published", published.len());
    assert_eq!(published.len(), core.scheduled_checkpoints().len());
    core.tick();
    assert_eq!(
        core.published_checkpoints().len(),
        published.len(),
        "none twice"
    );
    let openings = rutent_scenarios::assembly::checking::openings_of(core.scheduled_checkpoints());
    let through = core
        .scheduled_checkpoints()
        .last()
        .and_then(|k| rutent_core_audit::checkpoint::tick_of(k.opening.tick.as_str()));
    let checked = checker().check(&history_of(&core.log()), &published, &openings, through);
    assert!(checked.findings.is_empty(), "{:?}", checked.findings);
    admitted(&core);
}
