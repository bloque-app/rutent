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
use rutent_scenarios::on_tick::OnTick;

const IN: &str = "sim-join-in";
const EXCHANGE: &str = "sim-join-exchange";
const QUICK: &str = "sim-join-ramp-quick";
const OUT: &str = "sim-join-out";
/// A reserve like [`OUT`] that declares a safe repeat and no status query (`T-020-63`).
const OUT_UNTRACKED: &str = "sim-join-out-untracked";

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

/// The order across the joined providers to the reserve that declares no status query.
fn across_untracked(name: &str, session: &Id) -> rutent_core_orch::intake::OrderRequest {
    request(
        name,
        session,
        AMOUNT,
        IN,
        outer("sim-join-in-bank", "COP"),
        outer("sim-join-out-untracked-bank", "USD"),
        OUT_UNTRACKED,
        OTHER,
        "lowest_total_cost",
    )
}

/// How many segment moves repeated, as the log holds their effects.
fn repeats(core: &Core) -> usize {
    entries(core)
        .iter()
        .filter(|v| {
            matches!(
                v.what,
                What::Effect {
                    action: Action::RepeatOperation { .. },
                    ..
                }
            )
        })
        .count()
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

/// What Core's own tick found timing `order`'s part, where it was more than running: Core's stuck watch as its
/// schedule runs it, over every order on one `tick`, never asked per order (`T-020-60` item (8)). `None` where the tick
/// found it running, or stuck already.
fn watched_on_tick(core: &mut Core, order: &Id) -> Option<rutent_core_orch::stuck::Watch> {
    core.tick()
        .watched
        .into_iter()
        .find(|w| &w.order == order)
        .map(|w| w.found.expect("the tick timed the part"))
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
    core.owed_reversal(order, ramp).expect("owed");
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
    core.owed_reversal(order, ramp).expect("owed");
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
    core.owed_reversal(order, ramp).expect("owed");
    let shown = showing(core, order, ramp);
    core.show_fault(order, ramp, &shown).expect("shown");
    assert!(
        matches!(
            core.accept_fault(order, ramp, &kit::customer(), session),
            Err(StepError::Reversal(ReversalError::NotTheParty))
        ),
        "the signed result names the operator's fault, not the customer's"
    );
    // A login of the operator's the register lists, holding no permission to accept a fault, is refused by policy, and
    // nothing is carried (`T-020-68`, the third security pass's L8).
    let unpermitted = rutent_contracts::core_internal::Actor {
        acts: id("op-desk-unpermitted"),
        ..kit::operator()
    };
    core.registers().list(&unpermitted);
    assert!(
        matches!(
            core.accept_fault(order, ramp, &unpermitted, session),
            Err(StepError::Reversal(ReversalError::NotPermitted(_)))
        ),
        "policy decides the operator's acceptance"
    );
    assert_eq!(
        core.reversal_carrier(order, ramp),
        Some(Carrier::Provider(id(QUICK)))
    );
    assert_eq!(
        core.accept_fault(order, ramp, &kit::operator(), session)
            .expect("the operator accepts its fault"),
        Carrier::Operator
    );
    assert_eq!(recorded_carrier(core, ramp), Some(Recorded::Operator));
    admitted(core);
}

/// A provider's showing of fault, kept before Core restarts, is rebuilt from the log as Core reopens (the coordinator's
/// ruling on round 3 of #355, under #143's second blocker): the party it names accepts it after the restart with no new
/// showing, and carries the reversal from then, recorded with the showing the log took before the restart.
// verifies: REQ-ORD-11, NFR-6
#[test]
fn fault_shown_before_a_restart_is_accepted_after_it_without_a_new_showing() {
    let mut failed = ramp_failing("AC-4", Behaviour::failing_by(Fault::Customer));
    let Failed {
        core,
        session,
        order,
        ramp,
    } = &mut failed;
    core.owed_reversal(order, ramp).expect("owed");
    let shown = showing(core, order, ramp);
    let showing_receipt = core.show_fault(order, ramp, &shown).expect("shown");
    let (order, ramp, session) = (order.clone(), ramp.clone(), session.clone());
    let Failed { core, .. } = failed;
    let given = execution_support::given(&[IN, EXCHANGE, QUICK, OUT]);
    let mut core = Scenario::new("execution", "AC-4", given).reopen(core, 1, &mut Reference);
    assert_eq!(
        core.accept_fault(&order, &ramp, &kit::customer(), &session)
            .expect("the customer accepts the showing kept before the restart"),
        Carrier::Customer
    );
    assert_eq!(
        core.reversal_carrier(&order, &ramp),
        Some(Carrier::Customer)
    );
    assert!(
        entries(&core).iter().any(|v| matches!(
            &v.what,
            What::ReversalCarrier { carrier: Recorded::Customer, showing: Some(s), .. } if *s == showing_receipt
        )),
        "the carrier rests on the showing the log took before the restart"
    );
    admitted(&core);
}

// verifies: REQ-ORD-11
#[test]
fn dispute_over_a_fault_makes_the_order_stuck() {
    for fault in [Fault::Customer, Fault::Operator] {
        let mut failed = ramp_failing("AC-4", Behaviour::failing_by(fault));
        let Failed {
            core, order, ramp, ..
        } = &mut failed;
        core.owed_reversal(order, ramp).expect("owed");
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
                    ReversalError::NoShowing
                        | ReversalError::NotAnAcceptance
                        | ReversalError::NotPermitted(_)
                ))
            ),
            "a disputed fault isn't accepted: Core's entry point asks policy, which refuses it in a session not open"
        );
        // Its reversal still completes on the stuck part, and the order stays stuck.
        let asked = core.asked_on_tick(order, ramp).expect("asked");
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
        core.owed_reversal(order, ramp).expect("owed");
        core.world().script_for(QUICK, "reversal", behaviour);
        let asked = core.asked_on_tick(order, ramp).expect("asked");
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
            !matches!(
                core.recalculated_on_tick(order),
                Ok(Recalculation::Recalculated(_))
            ),
            "{name}: no route starts from value in motion, nor from a reversal owed"
        );
        core.clock.advance(7 * 86_400);
        core.tick();
        assert_eq!(
            state(core, order),
            OrderState::Stuck,
            "{name}: the owed reversal is over its window"
        );
        let _ = core.ran_on_tick(order);
        // Nothing moved twice: no move of value beyond the route's three, the withdrawal and the conversion were
        // asked once each, and no reserve was asked to pay out.
        assert_eq!(effects(core), moves, "{name}");
        assert_eq!(core.world().journal(IN).len(), 1, "{name}");
        assert_eq!(core.world().journal(EXCHANGE).len(), 1, "{name}");
        assert_eq!(core.world().journal(OUT).len(), 0, "{name}");
        // An unknown or short reversal may have moved value: it is never asked again under a fresh transaction (the
        // review of #355's round 2, H1), however long it waits.
        if name != "refused" {
            core.clock
                .advance(rutent_core_orch::reversal::DEFAULT_REVERSAL_RETRY_SECONDS);
            assert!(
                matches!(core.asked_on_tick(order, ramp), Err(StepError::NotNow(_))),
                "{name}: never asked again"
            );
        } else {
            // A refusal moved nothing, so the same undo asked again, past the cadence, is the one that happens, once.
            let again = core.asked_on_tick(order, ramp).expect("asked again");
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
        core.owed_reversal(order, ramp).expect("owed");
        core.world().script_for(QUICK, "reversal", behaviour);
        let asked = core.asked_on_tick(order, ramp).expect("asked");
        assert!(asked.completed.is_none(), "{name}: {asked:?}");
        // The operator asks a reversal again only after the window here, so each tick reads the watch alone: Core's run on
        // the same tick asks nothing again of the refused one meanwhile.
        core.set_reversal_retry_cadence(30 * 86_400);
        // Inside its window nothing is declared: Core's own tick times it, over every order.
        let early = watched_on_tick(core, order);
        assert!(early.is_none(), "{name}: {early:?}");
        assert_eq!(
            state(core, order),
            OrderState::FirstSegmentSettled,
            "{name}"
        );
        core.clock.advance(7 * 86_400);
        let late = watched_on_tick(core, order);
        assert!(matches!(late, Some(Watch::Stuck(_))), "{name}: {late:?}");
        assert_eq!(state(core, order), OrderState::Stuck, "{name}");
        assert_eq!(
            segment_state(core, order, ramp),
            SegmentState::FailedReversalOwed,
            "{name}: still owed, and now to the stuck order's resolution"
        );
        admitted(core);
    }
}

/// An owed reversal at a tuple that never settles, with no history of its own and no time its record declares, is held
/// to the floor window on Core's own tick, and its order goes stuck past twice it: the schedule's watch, not a test's,
/// times it (`INV-3`, `REQ-ORD-29`, `T-020-63`; the milestone reviewer's check of `T-020-48`). The call is on record with
/// a basis of its own, the floor, not an advertised time nothing advertised.
// verifies: INV-3, REQ-ORD-29
#[test]
fn owed_reversal_at_a_never_settling_tuple_goes_stuck_on_cores_tick() {
    use rutent_contracts::core_internal::StuckBasis;
    use rutent_core_orch::stuck::{FLOOR_SECONDS, Watch};
    let mut failed = ramp_failing("AC-4", Behaviour::act(Act::Fail));
    let Failed {
        core, order, ramp, ..
    } = &mut failed;
    core.owed_reversal(order, ramp).expect("owed");
    core.world()
        .script_for(QUICK, "reversal", Behaviour::act(Act::Refuse));
    let asked = core.asked_on_tick(order, ramp).expect("asked");
    assert!(asked.completed.is_none(), "the reversal stays owed");
    assert!(
        core.plant_unpriced_route(order),
        "the order's record declares no settlement time"
    );
    // Inside twice the floor nothing is declared, however many ticks; it takes the schedule, not a call, to time it.
    core.clock.advance(2 * FLOOR_SECONDS - 120);
    // Core's run asks the ramp again in turn with its confirmation (`T-020-61`, item 10): it keeps refusing.
    core.world()
        .script_for(QUICK, "reversal", Behaviour::act(Act::Refuse));
    let round = core.tick();
    assert!(
        round.watched.iter().all(|w| &w.order != order),
        "still running: {:?}",
        round.watched
    );
    assert_eq!(state(core, order), OrderState::FirstSegmentSettled);
    core.clock.advance(240);
    core.world()
        .script_for(QUICK, "reversal", Behaviour::act(Act::Refuse));
    let round = core.tick();
    let called = round
        .watched
        .iter()
        .find_map(|w| match &w.found {
            Ok(Watch::Stuck(called)) if &w.order == order => Some(called.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the tick declared it stuck: {:?}", round.watched));
    assert_eq!(
        called.segment, *ramp,
        "the owed reversal's segment is the one timed"
    );
    assert_eq!(called.basis, StuckBasis::Floor);
    assert_eq!(called.figure_seconds, FLOOR_SECONDS);
    assert!(called.held_seconds > 2 * FLOOR_SECONDS, "{called:?}");
    assert_eq!(state(core, order), OrderState::Stuck);
    assert_eq!(
        segment_state(core, order, ramp),
        SegmentState::FailedReversalOwed,
        "still owed, and now to the stuck order's resolution"
    );
    // The figures behind the call are on record, with the floor as their basis.
    assert!(
        entries(core).iter().any(|v| matches!(
            &v.what,
            What::StuckCall { basis: StuckBasis::Floor, figure_seconds, .. }
                if u64::from(*figure_seconds) == FLOOR_SECONDS
        )),
        "the stuck call's figures name the floor"
    );
    admitted(core);
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
    core.ran_on_tick(&order).expect("runs again");
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
    let ran = core.ran_on_tick(&order).expect("the order ran");
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
        mut core,
        order,
        ramp,
        ..
    } = ramp_left_unknown();
    assert_eq!(status_asks(&core), 1);
    // The schedule's own question again is held off: what this shows is the run's own step after a reopen.
    core.set_status_cadence(QUICK, u64::MAX);
    let given = execution_support::given(&[IN, EXCHANGE, QUICK, OUT]);
    let mut core = Scenario::new("execution", "AC-5", given).reopen(core, 1, &mut Reference);
    assert!(
        core.is_unknown(&order, &ramp),
        "the reopened Core still holds the move unknown"
    );
    // The reopened Core takes it up on its tick.
    let round = core.tick();
    assert!(
        round
            .runs
            .parts
            .iter()
            .all(|p| p.asked.is_empty() && p.carried_on.is_empty()),
        "{:?}",
        round.runs
    );
    admitted(&core);
}

/// The schedule's own state survives a restart: a provider asked where a segment stands is not asked again by the first
/// tick of a Core that reopened, until its cadence has passed from the question the log holds (`NFR-6`, `REQ-ORD-1`,
/// `T-020-63`; the milestone reviewer's check of `T-020-48`). A schedule that forgot it would press every provider again
/// at each restart.
// verifies: NFR-6
#[test]
fn first_tick_after_a_restart_keeps_each_providers_ask_cadence() {
    use rutent_core_orch::schedule::Job;
    let Failed {
        mut core,
        order,
        ramp,
        ..
    } = ramp_left_unknown();
    // The step that left the ramp unknown asked its provider once, now; its cadence is ten minutes.
    core.set_status_cadence(QUICK, 600);
    assert_eq!(status_asks(&core), 1);
    core.clock.advance(100);
    let given = execution_support::given(&[IN, EXCHANGE, QUICK, OUT]);
    let mut core = Scenario::new("execution", "AC-5", given).reopen(core, 1, &mut Reference);
    assert!(
        core.is_unknown(&order, &ramp),
        "still unknown after the restart"
    );
    // The reopened Core's first tick runs every job, the status question's among them, and presses nobody.
    let round = core.tick();
    assert!(round.did(Job::StatusAsks));
    assert!(
        round.statuses.is_empty(),
        "asked a hundred seconds ago: {:?}",
        round.statuses
    );
    assert_eq!(status_asks(&core), 1);
    // Ten minutes from the question the log holds, it is asked again, once.
    core.clock.advance(500);
    assert!(core.tick().statuses.is_empty(), "599 seconds on, not yet");
    core.clock.advance(1);
    // Due again from the question the log holds, as the first tick would have asked it had the schedule forgotten. The
    // question is put: a reopened Core holds the unknown segment answered as the Core that ran on did (`T-020-61`), so it
    // is asked once and then waits its cadence again, never pressed each tick.
    let round = core.tick();
    assert_eq!(round.statuses.len(), 1, "{:?}", round.statuses);
    assert_eq!(round.statuses[0].unknown.segment, ramp);
    assert!(round.statuses[0].asked.is_ok(), "{:?}", round.statuses[0]);
    assert!(
        core.tick().statuses.is_empty(),
        "not again before its cadence"
    );
    assert_eq!(status_asks(&core), 2);
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
    core.owed_reversal(order, ramp).expect("owed");
    core.asked_on_tick(order, ramp).expect("asked");
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
    core.owed_reversal(&order, &exchange).expect("owed");

    // The exchange's own word that it undid the segment completes nothing: it is the failed provider's.
    let asked = core.asked_on_tick(&order, &exchange).expect("asked");
    assert!(asked.completed.is_none());
    assert_eq!(asked.why, Some(NotCounted::FailedProvidersOwn));
    // The origin reserve says no credit arrived: nothing completes. Then it says one arrived short: nor that.
    let done = core.confirmed_on_tick(&order, &exchange).expect("asked");
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
    // Asked again on the retry cadence, never on every tick (the review of #355's round 2, H1).
    core.clock
        .advance(rutent_core_orch::reversal::DEFAULT_REVERSAL_RETRY_SECONDS);
    let done = core.confirmed_on_tick(&order, &exchange).expect("asked");
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
    core.clock
        .advance(rutent_core_orch::reversal::DEFAULT_REVERSAL_RETRY_SECONDS);
    let Confirmation::Completed(completed) =
        core.confirmed_on_tick(&order, &exchange).expect("asked")
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
    let Recalculation::Recalculated(recalculated) =
        core.recalculated_on_tick(&order).expect("tried")
    else {
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
    let (mut core, session) = core_with("AC-5", &[IN, EXCHANGE, QUICK, OUT_UNTRACKED]);
    core.world()
        .script_for(OUT_UNTRACKED, "deposit", Behaviour::late());
    let (order, ran) = run(&mut core, across_untracked("req-late", &session));
    let segments = segments_of(&ran);
    assert!(
        matches!(segments[3].outcome, SegmentOutcome::Unknown(_)),
        "the deposit's outcome is not known: {segments:?}"
    );
    assert_eq!(
        status_asks(&core),
        0,
        "its provider declares no status query"
    );
    // The provider's answer comes late: Core's own schedule carries the operation out again, as a decided effect on its
    // provider's cadence, and takes the answer the platform holds, with no call of a test's (`T-020-48`, `T-020-63`).
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
    core.set_cadence(rutent_core_orch::schedule::Job::StuckReports, u64::MAX);
    let (results_before, effects_before) = (results(&core), effects(&core));
    let round = core.tick();
    assert_eq!(repeats(&core), 1, "the repeat is a decided effect");
    assert_eq!(round.late.len(), 1, "one late answer: {:?}", round.late);
    assert!(round.late[0].kept.is_ok(), "{:?}", round.late[0]);
    assert_eq!(results(&core), results_before + 1, "kept once");
    assert!(core.tick().late.is_empty(), "the platform takes it once");
    assert_eq!(results(&core), results_before + 1);
    // Past its time the order is stuck.
    core.clock.advance(2 * kit::DECLARED + 5);
    assert!(matches!(
        watched_on_tick(&mut core, &order),
        Some(rutent_core_orch::stuck::Watch::Stuck(_))
    ));
    let held = core.stuck_values();
    // The part's being declared stuck is an input of its own: what follows is held to it.
    let inputs_before = core.inputs().len();
    let log_before = core.log().entries().len();
    // The same answer handed to Core directly, past the platform's once: Core itself moves nothing and records nothing new.
    let segment = id(
        core.followed(&order).expect("followed").parts[0].segments[3]
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
    assert_eq!(
        effects(&core),
        effects_before,
        "no value moves, by the answer or by the repeat"
    );
    assert_eq!(state(&mut core, &order), OrderState::Stuck);
    assert_eq!(core.stuck_values(), held);
    admitted(&core);
}

/// A segment left unknown at a provider that declares a safe repeat and no status query is carried out again by Core's
/// schedule as a decided effect, on its provider's cadence and no oftener, and nothing is handed over where policy
/// refuses (`REQ-AUZ-1`, `REQ-ORD-1`, `AC-5`, `T-020-63`; review M2).
// verifies: REQ-AUZ-1, REQ-ORD-1
#[test]
fn unknown_segment_is_carried_out_again_on_a_decided_effect_at_its_providers_cadence() {
    let (mut core, session) = core_with("AC-5", &[IN, EXCHANGE, QUICK, OUT_UNTRACKED]);
    core.world()
        .script_for(OUT_UNTRACKED, "deposit", Behaviour::late());
    let (_, ran) = run(&mut core, across_untracked("req-repeat", &session));
    assert!(matches!(
        segments_of(&ran)[3].outcome,
        SegmentOutcome::Unknown(_)
    ));
    core.set_status_cadence(OUT_UNTRACKED, 600);
    let before = core.operations_received(OUT_UNTRACKED);
    assert_eq!(repeats(&core), 0, "nothing is carried out again by a run");
    // The first tick carries it out again, once.
    core.tick();
    assert_eq!(repeats(&core), 1);
    assert_eq!(core.operations_received(OUT_UNTRACKED), before + 1);
    // A tick every ten seconds for ten minutes less ten: nothing is handed over, nothing recorded.
    for _ in 0..59 {
        core.clock.advance(10);
        core.tick();
    }
    assert_eq!(repeats(&core), 1, "held to its provider's cadence");
    assert_eq!(core.operations_received(OUT_UNTRACKED), before + 1);
    // Ten minutes from it, due again: policy decides again, and the platform, holding the answer, runs nothing.
    core.clock.advance(10);
    core.tick();
    assert_eq!(repeats(&core), 2);
    assert_eq!(core.operations_received(OUT_UNTRACKED), before + 1);
    // Policy no longer lets Core: nothing is handed over, and no effect is recorded.
    core.registers().permissions.retain(|(acts, p)| {
        !(acts == kit::core().acts.as_str()
            && *p == rutent_core_authz::roles::Permission::RepeatOperation)
    });
    core.clock.advance(600);
    core.tick();
    assert_eq!(repeats(&core), 2, "refused, so no effect");
    assert_eq!(core.operations_received(OUT_UNTRACKED), before + 1);
    admitted(&core);
}

/// A late answer Core couldn't record now (its log can't take the entry) is not dropped: the platform holds it until Core
/// says it kept it, and hands it over again the next tick, when it is kept once (`NFR-6`, `AC-5`, `T-020-63`; the
/// milestone reviewer's check of `T-020-48`). The platform's one delivery of it was the first build's only.
// verifies: NFR-6
#[test]
fn late_answer_the_log_could_not_record_is_kept_for_the_next_tick() {
    use rutent_core_orch::schedule::Job;
    // The deposit's outcome is left unknown at a reserve that declares no status query, so Core's schedule carries it out
    // again, and what its late answer says is the first statement of it Core keeps.
    let (mut core, session) = core_with("AC-5", &[IN, EXCHANGE, QUICK, OUT_UNTRACKED]);
    core.world()
        .script_for(OUT_UNTRACKED, "deposit", Behaviour::late());
    let (order, ran) = run(&mut core, across_untracked("req-late-log", &session));
    assert!(
        matches!(segments_of(&ran)[3].outcome, SegmentOutcome::Unknown(_)),
        "the deposit's outcome is not known: {ran:?}"
    );
    core.set_cadence(Job::StuckReports, u64::MAX);
    let statements = |core: &Core| {
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
    let before = statements(&core);
    let ran_before = core.operations_received(OUT_UNTRACKED);
    // The log can take no result statement now: three tries are three refusals.
    core.refuse_log_appends(
        rutent_contracts::provider_role::RESULT_SHAPE,
        rutent_core_orch::segments::APPEND_TRIES,
    );
    let round = core.tick();
    assert!(round.did(Job::LateAnswers));
    assert_eq!(round.late.len(), 1, "one late answer: {:?}", round.late);
    assert!(
        round.late[0].kept.is_err(),
        "the log couldn't take it: {:?}",
        round.late[0]
    );
    assert_eq!(statements(&core), before, "nothing was recorded");
    // The platform hands it over again, and this time it is kept, once; the module wasn't run for it again.
    let round = core.tick();
    assert_eq!(
        core.operations_received(OUT_UNTRACKED),
        ran_before + 1,
        "run once, for the repeat"
    );
    assert_eq!(
        round.late.len(),
        1,
        "the same answer again: {:?}",
        round.late
    );
    assert!(round.late[0].kept.is_ok(), "{:?}", round.late[0]);
    assert!(
        core.tick().late.is_empty(),
        "and the platform holds it no more"
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
    // The notices the run's changes call for went with the tick the run was on, and this one: none is left for a caller.
    assert!(
        matches!(round.notified, Some(Ok(_))),
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

/// The nodes of `provider`'s operation on the route Core last recorded, each by the name an unlist gives it.
fn nodes_of(core: &Core, provider: &str) -> Vec<String> {
    let record = core
        .decision_records()
        .last()
        .cloned()
        .expect("the route's record");
    let operation = record.chosen().expect("a route").parts[0]
        .operations
        .iter()
        .find(|o| o.provider.as_str() == provider)
        .cloned()
        .expect("the provider's operation");
    [operation.from.node, operation.to.node]
        .iter()
        .map(|end| match end {
            rutent_contracts::provider_role::NodeRef::Inner(id) => id.as_str().to_owned(),
            rutent_contracts::provider_role::NodeRef::Outer(outer) => outer.network.clone(),
        })
        .collect()
}

/// The operator's unlist of `node` is refused as the unlist rule says: policy is asked (its decision is in the log, the
/// attempt recorded), no `set_setting` effect is recorded, and the node's standing is unchanged.
fn unlist_is_refused(core: &mut Core, node: &str) {
    use rutent_contracts::core_internal::{Action, CatalogueChange, Entry};
    use rutent_core_orch::settings::SettingRefused;
    let listed = core.catalogue().nodes().is_public(node);
    let counted = |core: &Core| {
        let (mut decided, mut effects) = (0, 0);
        for (_, entry) in core.log().entries() {
            let Entry::Verified(v) = entry else { continue };
            match &v.what {
                What::Decide { request, .. }
                    if matches!(
                        request.action,
                        Action::ChangeCatalogue {
                            change: CatalogueChange::SetSetting,
                            ..
                        }
                    ) =>
                {
                    decided += 1;
                }
                What::Effect {
                    action:
                        Action::ChangeCatalogue {
                            change: CatalogueChange::SetSetting,
                            ..
                        },
                    ..
                } => effects += 1,
                _ => {}
            }
        }
        (decided, effects)
    };
    let (decided, effects) = counted(core);
    let unlisted: rutent_contracts::core_internal::OperatorSetting = serde_json::from_value(
        serde_json::json!({"setting": "public_node", "node": {"network": node}, "listed": false}),
    )
    .expect("a setting");
    let refused = core.set_setting(&unlisted);
    assert!(
        matches!(&refused, Err(SettingRefused::Rests { node: n }) if n == node),
        "{node}: {refused:?}"
    );
    assert_eq!(
        counted(core),
        (decided + 1, effects),
        "{node}: policy asked, no effect recorded"
    );
    assert_eq!(
        core.catalogue().nodes().is_public(node),
        listed,
        "{node}: unchanged"
    );
}

/// The unlist rule holds for a segment in flight (`T-020-65`, `REQ-CAT-22`): the ramp's transfer is left unknown, so its
/// segment is in flight at both its nodes; the operator's unlist of either is refused after policy is asked, no effect
/// is recorded, and a node on the public list stays on it, so the segment is graded as it began, against its public
/// record.
// verifies: REQ-CAT-22
#[test]
fn public_node_cannot_be_unlisted_while_a_segment_is_in_flight_there() {
    let Failed { mut core, .. } = ramp_left_unknown();
    for node in nodes_of(&core, QUICK) {
        unlist_is_refused(&mut core, &node);
    }
}

/// A public node can't be unlisted while a failed segment's reversal is owed there (`T-020-66`, `REQ-CAT-22`): the ramp's
/// transfer failed, its reversal owed (`FailedReversalOwed`), so the value it took is still held for the order; the
/// operator's unlist of either of its nodes is refused after policy is asked, with no effect recorded. A failed or
/// refused segment counts until its reversal completes, as one waiting on a person or with no result does.
// verifies: REQ-CAT-22
#[test]
fn public_node_cannot_be_unlisted_while_a_failed_segments_reversal_is_owed_there() {
    let Failed {
        mut core,
        order,
        ramp,
        ..
    } = ramp_failing("AC-4", Behaviour::act(Act::Fail));
    assert_eq!(
        segment_state(&core, &order, &ramp),
        SegmentState::FailedReversalOwed
    );
    for node in nodes_of(&core, QUICK) {
        unlist_is_refused(&mut core, &node);
    }
}

/// The operator's setting that takes `node` off the public list.
fn unlisting(node: &str) -> rutent_contracts::core_internal::OperatorSetting {
    serde_json::from_value(
        serde_json::json!({"setting": "public_node", "node": {"network": node}, "listed": false}),
    )
    .expect("a setting")
}

/// An order quoted on a route whose public node the operator unlisted before the customer accepted isn't accepted on
/// it (`T-020-66`, #346's review, L2): the route was chosen on a standing that no longer holds, and a segment begun on it
/// would be graded on a node that is neither public nor its publisher's. The quote is unlisted while nothing of Core's
/// is at the node, so the unlist is permitted; the acceptance then reads each inner node of the route as the catalogue's
/// snapshot does and finds the node off the list, so it is refused as no route right now, nothing started, and the
/// order is quoted again on the standing that holds now.
// verifies: REQ-CAT-22
#[test]
fn quote_whose_route_names_a_node_unlisted_since_is_not_accepted() {
    use rutent_contracts::core_internal::OrderRefusalClass;
    use rutent_core_orch::intake::{Status, Told};
    let (mut core, session) = core_with("AC-1", &[IN, EXCHANGE, QUICK, OUT]);
    let (order, told) = quoted(&mut core, across("req-quoted", &session));
    let Told::Exists {
        status: Status::Quoted { digest, .. },
        ..
    } = told
    else {
        panic!("quoted: {told:?}");
    };
    let chain = "sim-join-chain";
    assert!(core.catalogue().nodes().is_public(chain));
    assert_eq!(core.set_setting(&unlisting(chain)).map(|_| ()), Ok(()));
    let told = core
        .accept(&kit::customer(), &order, &digest)
        .expect("told");
    assert!(
        matches!(
            told,
            Told::Exists {
                status: Status::CantProceed {
                    class: OrderRefusalClass::NoRouteRightNow
                },
                ..
            }
        ),
        "{told:?}"
    );
    assert!(core.followed(&order).is_none(), "nothing started");
    assert!(
        core.accept(&kit::customer(), &order, &digest).is_err(),
        "the quote shown is gone"
    );
    let told = core.quote(&kit::customer(), &order).expect("quoted again");
    assert!(
        !matches!(
            told,
            Told::Exists {
                status: Status::Quoted { .. },
                ..
            }
        ),
        "no route through the unlisted node: {told:?}"
    );
}

/// A public node can't be unlisted while an order Core follows that hasn't ended names it on its route (`T-020-66`, #342's
/// L2): accepted and not yet run, nothing of it is at the ramp's nodes, and its segments there are still to begin; the
/// operator's unlist of either is refused, so the segments are graded on the standing they begin under.
// verifies: REQ-CAT-22
#[test]
fn public_node_cannot_be_unlisted_while_an_open_orders_route_names_it() {
    let (mut core, session) = core_with("AC-1", &[IN, EXCHANGE, QUICK, OUT]);
    let _ = accepted(&mut core, across("req-planned", &session));
    for node in nodes_of(&core, QUICK) {
        unlist_is_refused(&mut core, &node);
    }
}

/// A provider's answer to Core's status question, kept before Core reopens, saying the segment's move failed: the reopened
/// Core rebuilds what it kept from the log and the evidence store (`Reversals::reopened`, `T-020-61` item 6), and once the
/// part is stuck its tick reports that same kept answer to the stuck mover, citing the entry the log took before the
/// restart, with no second stated entry of it.
// verifies: NFR-6, REQ-ORD-29
#[test]
fn late_answer_kept_before_a_restart_is_reported_after_it() {
    let Failed {
        core, order, ramp, ..
    } = ramp_left_unknown();
    let answer = core
        .status_asked(&order, &ramp)
        .cloned()
        .expect("Core asked")
        .expect("the question was put")
        .answer
        .expect("the ramp answered");
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
    let kept_before = kept_of_the_ramp(&core);
    let given = execution_support::given(&[IN, EXCHANGE, QUICK, OUT]);
    let mut core = Scenario::new("execution", "AC-5", given).reopen(core, 1, &mut Reference);
    core.clock.advance(2 * kit::DECLARED + 5);
    let round = core.tick();
    assert_eq!(state(&mut core, &order), OrderState::Stuck);
    let taken: Vec<_> = round.reported.iter().filter(|r| r.taken.is_ok()).collect();
    assert_eq!(taken.len(), 1, "{:?}", round.reported);
    assert_eq!(
        taken[0].statement, answer,
        "the report rests on the answer kept before the restart"
    );
    assert_eq!(
        kept_of_the_ramp(&core),
        kept_before,
        "no second entry of it"
    );
    admitted(&core);
}

/// A deposit taken and never answered, its effect's result `unanswered` before Core and its platform restart: the
/// restarted Core holds it answered, as the Core that ran on did, so its provider's late answer is taken after the restart,
/// kept once and moving nothing, and never refused as not answered (`T-020-61`: #350's review, seed 117).
// verifies: NFR-6, REQ-ORD-29
#[test]
fn late_answer_to_a_segment_left_unanswered_before_core_restarts_is_taken_after_it() {
    let (mut core, session) = core_with("AC-5", &[IN, EXCHANGE, QUICK, OUT_UNTRACKED]);
    core.world()
        .script_for(OUT_UNTRACKED, "deposit", Behaviour::late());
    // No repeat before the restart: the answer is the platform's to hand over after it.
    core.set_status_cadence(OUT_UNTRACKED, u64::MAX);
    let (order, ran) = run(&mut core, across_untracked("req-late-restart", &session));
    let segments = segments_of(&ran);
    assert!(
        matches!(segments[3].outcome, SegmentOutcome::Unknown(_)),
        "the deposit's outcome is not known: {segments:?}"
    );
    let segment = segments[3].segment.clone();
    let given = execution_support::given(&[IN, EXCHANGE, QUICK, OUT_UNTRACKED]);
    let mut core = Scenario::new("execution", "AC-5", given).restart(core, 1, &mut Reference);
    assert!(
        core.is_unknown(&order, &segment),
        "held unknown after the restart"
    );
    let effects_before = effects(&core);
    let late = core
        .provider_again(&order, &segment)
        .expect("the provider's answer, as it gave it late");
    let kept = core
        .late_delivery(&order, &segment, &late)
        .expect("taken after the restart, not refused as not answered");
    let again = core
        .late_delivery(&order, &segment, &late)
        .expect("the same answer again is the same entry");
    assert_eq!(kept, again, "kept once");
    assert_eq!(effects(&core), effects_before, "no value moves");
    assert!(
        core.is_unknown(&order, &segment),
        "a late answer settles nothing"
    );
    admitted(&core);
}
