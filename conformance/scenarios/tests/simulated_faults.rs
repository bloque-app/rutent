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
use rutent_scenarios::assembly::{Core, StepError};
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
        assert_eq!(state(core, order), OrderState::FirstSegmentSettled, "{name}");
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
    assert_eq!(effects(&core), 3, "the withdrawal, the conversion and the transfer, and no more");
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
    // Past its time the order is stuck.
    core.clock.advance(2 * kit::DECLARED + 5);
    assert!(matches!(
        core.watch(&order).expect("watched"),
        rutent_core_orch::stuck::Watch::Stuck(_)
    ));
    let held = core.stuck_values();
    // The provider's answer comes late and the platform hands it on: kept once, as its maker's statement.
    let (log_before, inputs_before, effects_before) = (
        core.log().entries().len(),
        core.inputs().len(),
        effects(&core),
    );
    let kept = core.receive_late(&order).expect("received");
    assert_eq!(kept.len(), 1, "one late answer");
    assert_eq!(core.log().entries().len(), log_before + 1, "one statement");
    assert!(
        core.receive_late(&order).expect("received").is_empty(),
        "the platform takes it once"
    );
    assert_eq!(core.log().entries().len(), log_before + 1);
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
    assert_eq!(core.log().entries().len(), log_before + 1, "nothing new");
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
