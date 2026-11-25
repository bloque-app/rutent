//! Core rebuilds its state from the log when it reopens (`T-020-46`, finding 3 of `M3`'s milestone review): a Core
//! opened again on its disk agrees with the Core that ran on. The order it was running carries on from where it
//! stopped, its inputs read back from the operator's statement of them; no value moves twice; what is in flight is held
//! again, so new orders respect the ceilings; a stuck part's kept decision and what moved are read back.

use std::cell::RefCell;

use rutent_contracts::core_internal::{
    Action, Actor, ActorKind, Answer, Decided, Destination, Entry, Resolution, Role, What,
};
use rutent_contracts::provider_role::Denomination;
use rutent_contracts::router_api::{Amount, PositiveAmount, ProviderCeiling};
use rutent_contracts::state_machine::{Observed, ResolvedTo};
use rutent_core_authz::roles::Permission;
use rutent_core_orch::intake::{Status, Told};
use rutent_core_orch::stuck::{Moved, Resolved};
use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::assembly::{Core, Staged};
use rutent_scenarios::harness::{Fault, Given, Run, Scenario, Step, base_request, reference};
use rutent_scenarios::kit::{self, AMOUNT, RESERVE, id};

const SEEDS: [u64; 2] = [1, 7];

fn order(n: usize) -> Step {
    Step::Order {
        request: format!("req-{n}"),
        amount: AMOUNT.to_owned(),
    }
}

fn scenario(name: &'static str) -> Scenario {
    Scenario::new(name, "AC-28", Given::one_reserve(Staged::AtOnce))
}

/// Every `move_value` effect the log holds, in its order: its segment.
fn moves(run: &Run) -> Vec<String> {
    run.log
        .iter()
        .filter_map(|e| match e {
            Entry::Verified(v) => match &v.what {
                What::Effect {
                    action: Action::MoveValue { segment, .. },
                    ..
                } => Some(segment.as_str().to_owned()),
                _ => None,
            },
            Entry::Stated(_) => None,
        })
        .collect()
}

/// Which of the log's `decide` entries after the order started, from 0, decides its `nth` move, from 1: a crash
/// planted once the order started counts the records from there.
fn decide_of_move(run: &Run, nth: usize) -> usize {
    let started = run
        .log
        .iter()
        .position(|e| {
            matches!(e, Entry::Verified(v)
                if matches!(&v.what, What::Input { input } if matches!(input.observed, Observed::RouteChosen { .. })))
        })
        .expect("the order started");
    let mut decides = 0;
    let mut seen = 0;
    for entry in &run.log[started..] {
        if let Entry::Verified(v) = entry
            && let What::Decide { request, .. } = &v.what
        {
            if matches!(request.action, Action::MoveValue { .. }) {
                seen += 1;
                if seen == nth {
                    return decides;
                }
            }
            decides += 1;
        }
    }
    panic!("the log decides no move {nth}")
}

/// A Core stopped mid-order, between its withdrawal and its deposit (right after policy's decision of the deposit's
/// move reached the log, before anything of the deposit was), opened again on its disk and run again, carries the
/// order on as the Core that ran on: the order completes, each of its segments moved once under a transaction of its
/// own, by one `move_value` each, and nothing stopped.
// verifies: NFR-6
#[test]
fn core_reopened_mid_order_carries_on_as_the_core_that_ran_on() {
    for seed in SEEDS {
        let ran_on = scenario("the Core that ran on")
            .when(order(1))
            .when(Step::Run)
            .play(seed, &mut Reference, reference());
        let nth = decide_of_move(&ran_on, 2);
        let reopened = scenario("a Core reopened mid-order")
            .when(order(1))
            .when(Step::Plant(Fault::CrashAfterRecord {
                call: "decide",
                nth,
            }))
            .when(Step::Run)
            .when(Step::Restart)
            .when(Step::Rerun)
            .play(seed, &mut Reference, reference());
        assert!(reopened.crashed, "seed {seed}: the crash stopped Core");
        assert_eq!(reopened.reopened, Ok(0), "seed {seed}");
        assert!(
            reopened.stopped.is_empty(),
            "seed {seed}: {:?}",
            reopened.stopped
        );
        assert_eq!(
            reopened.state_names(),
            ran_on.state_names(),
            "seed {seed}: the order stands as the one that ran on"
        );
        assert!(
            ran_on
                .state_names()
                .values()
                .all(|s| s.as_deref() == Some("complete")),
            "seed {seed}: {:?}",
            ran_on.state_names()
        );
        assert_eq!(
            moves(&reopened),
            moves(&ran_on),
            "seed {seed}: one move each"
        );
        let moved = reopened.movements();
        assert_eq!(
            moved.len(),
            ran_on.movements().len(),
            "seed {seed}: {moved:?}"
        );
        assert!(moved.values().all(|n| *n == 1), "seed {seed}: {moved:?}");
    }
}

/// A Core stopped after a segment's `move_value` effect reached the log, before or after its provider carried it out,
/// opened again and asked twice to run the order on, moves nothing again for that segment: no second effect, no
/// transaction moved twice, and no more movement than the Core that stopped made. The segment's value may be in
/// motion, so the reopened Core never carries it out again (`INV-8`).
// verifies: INV-8
#[test]
fn no_value_moves_twice_after_core_reopens() {
    for seed in SEEDS {
        for (crash, moved_before) in [
            (Fault::CrashBeforeEffect(0), 0),
            (
                Fault::CrashAfterRecord {
                    call: "effect",
                    nth: 0,
                },
                0,
            ),
            (
                Fault::CrashAfterRecord {
                    call: "evidence",
                    nth: 0,
                },
                1,
            ),
        ] {
            let run = scenario("a Core reopened after a move took effect")
                .when(order(1))
                .when(Step::Plant(crash.clone()))
                .when(Step::Run)
                .when(Step::Restart)
                .when(Step::Rerun)
                .when(Step::Rerun)
                .play(seed, &mut Reference, reference());
            assert!(
                run.crashed,
                "seed {seed}, {crash:?}: the crash stopped Core"
            );
            assert_eq!(
                moves(&run).len(),
                1,
                "seed {seed}, {crash:?}: no second move: {:?}",
                moves(&run)
            );
            let moved = run.movements();
            assert!(
                moved.values().all(|n| *n == 1),
                "seed {seed}, {crash:?}: {moved:?}"
            );
            assert_eq!(
                moved.values().sum::<usize>(),
                moved_before,
                "seed {seed}, {crash:?}: {moved:?}"
            );
            assert!(
                run.state_names()
                    .values()
                    .all(|s| s.as_deref() != Some("complete")),
                "seed {seed}, {crash:?}"
            );
        }
    }
}

fn cop() -> Denomination {
    Denomination {
        id: "COP".into(),
        decimals: 2,
    }
}

/// Twice the base amount at [`RESERVE`], in COP: an order moves its amount out of the reserve and back into it, two
/// operations there, so it fits only with nothing else in flight.
fn ceiling() -> ProviderCeiling {
    ProviderCeiling {
        provider: id(RESERVE),
        asset: cop(),
        amount: Amount::new((2 * AMOUNT.parse::<u128>().unwrap()).to_string()).unwrap(),
    }
}

/// A second order of the base amount under the ceiling: told whether it can't proceed.
fn second_order_refused(core: &mut Core) -> bool {
    core.set_provider_ceiling(ceiling());
    let customer = kit::customer();
    let session = core.open_session(&customer).expect("a session");
    let told = core
        .request(base_request("req-2", AMOUNT, &session))
        .expect("the request");
    let order = Core::order_of(&told).expect("an order");
    matches!(
        core.quote(&customer, &order).expect("asked"),
        Told::Exists {
            status: Status::CantProceed { .. },
            ..
        }
    )
}

/// The deposit's provider waits on the person, so the deposit stays in flight, held at the reserve. A Core opened again
/// holds what the Core that stopped held, at its provider and in its asset, and a second order that fits under the
/// reserve's ceiling only with nothing in flight is refused, as the Core that ran on refuses it (`REQ-ORD-8`,
/// `THR-74`).
// verifies: REQ-ORD-8, THR-74
#[test]
fn exposures_are_rebuilt_from_the_log_so_routes_respect_the_ceiling_after_a_restart() {
    let waits = || {
        Step::Plant(Fault::WaitsOnPerson {
            provider: RESERVE.to_owned(),
            kind: "deposit".to_owned(),
        })
    };
    for seed in SEEDS {
        let held_before = RefCell::new(Vec::new());
        let ran_on_refused = RefCell::new(None);
        scenario("the Core that ran on")
            .when(order(1))
            .when(waits())
            .when(Step::Run)
            .play_probed(
                seed,
                &mut Reference,
                reference(),
                &mut |_| {},
                &mut |core| {
                    *held_before.borrow_mut() = core.exposures().totals();
                    *ran_on_refused.borrow_mut() = Some(second_order_refused(core));
                },
            );
        let held_after = RefCell::new(Vec::new());
        let reopened_refused = RefCell::new(None);
        scenario("a Core reopened with a deposit in flight")
            .when(order(1))
            .when(waits())
            .when(Step::Run)
            .when(Step::Restart)
            .play_probed(
                seed,
                &mut Reference,
                reference(),
                &mut |_| {},
                &mut |core| {
                    *held_after.borrow_mut() = core.exposures().totals();
                    *reopened_refused.borrow_mut() = Some(second_order_refused(core));
                },
            );
        assert!(
            !held_before.borrow().is_empty(),
            "seed {seed}: the deposit is held in flight"
        );
        assert_eq!(*held_after.borrow(), *held_before.borrow(), "seed {seed}");
        assert_eq!(*ran_on_refused.borrow(), Some(true), "seed {seed}");
        assert_eq!(*reopened_refused.borrow(), Some(true), "seed {seed}");
    }
}

/// A Core that reopens and can't read back the decision record a segment in flight ran on holds that segment at the
/// value its signed operation states, at its provider and in its asset, never at nothing: the ceiling still binds
/// (`REQ-ORD-8`, `THR-74`).
// verifies: REQ-ORD-8, THR-74
#[test]
fn in_flight_segment_whose_decision_record_is_unreadable_is_held_at_its_stated_value() {
    let (mut core, _) = scenario("a deposit in flight, its record unread")
        .when(order(1))
        .when(Step::Plant(Fault::WaitsOnPerson {
            provider: RESERVE.to_owned(),
            kind: "deposit".to_owned(),
        }))
        .when(Step::Run)
        .core(1, &mut Reference);
    let held = core.exposures().totals();
    assert!(!held.is_empty(), "the deposit is held in flight");
    assert_eq!(core.exposures_rebuilt_without_records(), held);
}

/// An operator login: a person of its own desk, which acts for the operator.
fn staff(core: &Core, desk: &str, login: &str) -> Actor {
    let actor = Actor {
        party: id(desk),
        acts: id(login),
        role: Role::Operator,
        kind: ActorKind::Person,
    };
    let mut registers = core.registers();
    registers.list(&Actor {
        party: kit::operator().party,
        acts: id(desk),
        role: Role::Operator,
        kind: ActorKind::Component,
    });
    registers.list(&actor);
    actor
}

/// Alice's desk decides, bob's moves: the deployment's registers, set again on each Core put together.
fn desks(core: &Core) -> (Actor, Actor) {
    let alice = staff(core, "desk-a", "alice");
    let bob = staff(core, "desk-b", "bob");
    let mut registers = core.registers();
    registers.permit(&alice, Permission::DecideStuck);
    registers.permit(&bob, Permission::MoveStuck);
    (alice, bob)
}

fn destination(core: &Core) -> Destination {
    core.verified_whats()
        .into_iter()
        .find_map(|w| match w {
            What::Authorise { authorisation, .. } => Some(authorisation.destination),
            _ => None,
        })
        .expect("the order's authorisation")
}

/// The commitment of a provider's result the log holds about `order`: the evidence a decision names.
fn evidence_about(
    core: &Core,
    order: &rutent_contracts::router_api::Id,
) -> Vec<rutent_contracts::module_interface::Digest> {
    let log = core.log();
    let found = log
        .entries()
        .iter()
        .find_map(|(_, e)| match e {
            Entry::Stated(s)
                if s.what.statement.as_str() == rutent_contracts::provider_role::RESULT_SHAPE
                    && s.about
                        .as_ref()
                        .is_some_and(|a| a.order.as_ref() == Some(order)) =>
            {
                Some(s.commitment.clone())
            }
            _ => None,
        })
        .expect("a statement about the order");
    vec![found]
}

fn resolve_effects(core: &Core) -> usize {
    core.verified_whats()
        .iter()
        .filter(|w| {
            matches!(
                w,
                What::Effect {
                    action: Action::ResolveStuck { .. },
                    ..
                }
            )
        })
        .count()
}

fn resolved(core: &Core) -> Vec<ResolvedTo> {
    core.inputs()
        .iter()
        .filter_map(|i| match &i.observed {
            Observed::StuckResolved { to, .. } => Some(*to),
            _ => None,
        })
        .collect()
}

/// A stuck part's decision, kept for its move, and the move once made, are read back from the log by a Core opened
/// again. Decided before the restart, the part is moved after it by the decision kept, once, and resolved at its
/// destination; opened again after the move, the Core holds the part resolved and the decision spent, and nothing
/// moves a second time (`INV-4`, `INV-8`).
// verifies: INV-4
#[test]
fn stuck_parts_kept_decision_and_move_are_rebuilt_from_the_log() {
    let given = scenario("a stuck order, decided, then reopened").when(order(1));
    let (mut core, orders) = given.core(1, &mut Reference);
    let order = orders[0].clone();
    let part = core.first_part(&order).expect("a started order");
    // The withdrawal settles; the platform won't begin the deposit: its value rests at the reserve's ledger.
    core.faults.refuse_begin = Some(1);
    core.run(&order).expect("the order runs");
    core.leave_no_route(&order).expect("the part is stuck");
    let (alice, _) = desks(&core);
    let resolution = Resolution::Deliver {
        by: id(RESERVE),
        to: destination(&core),
        asset: cop(),
        amount: PositiveAmount::new(AMOUNT).unwrap(),
    };
    let evidence = evidence_about(&core, &order);
    let decision = core
        .decide_stuck(&order, &alice, &resolution, &evidence)
        .expect("asked");
    assert_eq!(decision.outcome, Answer::Permit {});
    let carries_out = Decided {
        request: decision.request.clone(),
        decision: decision.decision.clone(),
    };
    let stuck_value = core.exposures().stuck().len();
    assert!(stuck_value > 0, "the part's value rests held");

    // Opened again: the part is stuck, its value held again, and the decision kept for its move.
    let mut core = given.reopen(core, 1, &mut Reference);
    let (_, bob) = desks(&core);
    assert!(core.is_stuck(&order, &part), "stuck, as before");
    assert_eq!(core.exposures().stuck().len(), stuck_value, "held again");
    let handled = core.operations_received(RESERVE);
    let moved = core
        .move_stuck(&order, &carries_out, &bob)
        .expect("moved on the decision kept");
    assert!(
        matches!(
            moved,
            Moved::CarriedOut {
                resolved: Resolved::Done,
                ..
            }
        ),
        "{moved:?}"
    );
    assert_eq!(core.operations_received(RESERVE), handled + 1);
    assert_eq!(resolve_effects(&core), 1);
    assert_eq!(resolved(&core), vec![ResolvedTo::Destination]);

    // Opened again after the move: the part is resolved, the decision spent, and nothing moves a second time.
    let mut core = given.reopen(core, 1, &mut Reference);
    let (_, bob) = desks(&core);
    assert!(!core.is_stuck(&order, &part), "resolved");
    assert!(core.move_stuck(&order, &carries_out, &bob).is_err());
    assert_eq!(core.operations_received(RESERVE), handled + 1);
    assert_eq!(resolve_effects(&core), 1);
    assert!(core.exposures().stuck().is_empty(), "nothing held for it");
}

/// The resolution's move is done, and Core stops right after its `done` result reached the log, before the part's
/// `stuck_resolved`. Opened again, Core counts what the done move moved and finishes the part as a Core that ran on
/// would have: its `stuck_resolved` appended once, at its destination, the part no longer stuck and holding nothing,
/// and a second decision to deliver its value refused: the value moves once (`INV-8`, `INV-4`).
// verifies: INV-8, INV-4
#[test]
fn done_move_is_counted_as_moved_after_a_restart() {
    let given = scenario("a stuck order, moved, then reopened").when(order(1));
    let (mut core, orders) = given.core(1, &mut Reference);
    let order = orders[0].clone();
    let part = core.first_part(&order).expect("a started order");
    core.faults.refuse_begin = Some(1);
    core.run(&order).expect("the order runs");
    core.leave_no_route(&order).expect("the part is stuck");
    let (alice, bob) = desks(&core);
    let resolution = Resolution::Deliver {
        by: id(RESERVE),
        to: destination(&core),
        asset: cop(),
        amount: PositiveAmount::new(AMOUNT).unwrap(),
    };
    let evidence = evidence_about(&core, &order);
    let decision = core
        .decide_stuck(&order, &alice, &resolution, &evidence)
        .expect("asked");
    let carries_out = Decided {
        request: decision.request.clone(),
        decision: decision.decision.clone(),
    };
    rutent_scenarios::harness::plant(
        &mut core,
        &Fault::CrashAfterRecord {
            call: "result",
            nth: 0,
        },
    );
    let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        core.move_stuck(&order, &carries_out, &bob)
    }));
    assert!(crashed.is_err(), "the crash stopped Core");
    let mut core = given.reopen(core, 1, &mut Reference);
    let (alice, _) = desks(&core);
    assert_eq!(resolve_effects(&core), 1, "the move's effect is in the log");
    assert_eq!(
        resolved(&core),
        vec![ResolvedTo::Destination],
        "the reopened Core appended its stuck_resolved, once"
    );
    assert!(!core.is_stuck(&order, &part), "the part is resolved");
    assert!(core.exposures().stuck().is_empty(), "nothing held for it");
    let handled = core.operations_received(RESERVE);
    assert!(
        core.decide_stuck(&order, &alice, &resolution, &evidence)
            .is_err(),
        "nothing more is decided for it"
    );
    assert_eq!(core.operations_received(RESERVE), handled);
    let mut core = given.reopen(core, 1, &mut Reference);
    assert_eq!(
        resolved(&core),
        vec![ResolvedTo::Destination],
        "once, across a second reopening"
    );
    let (alice, _) = desks(&core);
    assert!(
        core.decide_stuck(&order, &alice, &resolution, &evidence)
            .is_err()
    );
}

/// Every transaction the log names, a round's question, an answer, a move's or a hand-over's, is one a Core opened
/// again holds as asked under (`Issued::recorded`), so no fresh transaction it draws is one of them (`INV-8`).
// verifies: INV-8
#[test]
fn transactions_the_log_names_are_held_as_used_after_a_restart() {
    let given = scenario("a Core reopened after an order ran")
        .when(order(1))
        .when(Step::Run);
    let (core, _) = given.core(1, &mut Reference);
    let named: Vec<String> = core
        .log()
        .entries()
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Stated(s) => s
                .about
                .as_ref()
                .and_then(|a| a.transaction.as_ref())
                .map(|t| t.as_str().to_owned()),
            Entry::Verified(_) => None,
        })
        .collect();
    assert!(!named.is_empty(), "the log names transactions");
    let core = given.reopen(core, 1, &mut Reference);
    for transaction in &named {
        assert!(
            core.issued().contains(transaction),
            "{transaction} is held as used"
        );
    }
}

/// An erasure decided and done when Core stops, its `erasure` entry not yet in the log: the reopened Core finds it in
/// the log (`Unsettled::reopened`) and settles it, the store erasing (one erased already is `Ok`) and the `erasure`
/// entry appended once, so nothing is left for the store to report as carried out with no record (`REQ-AUD-13`,
/// `INV-8`).
// verifies: REQ-AUD-13, INV-8
#[test]
fn erasure_done_before_core_reopens_is_settled_once() {
    let given = scenario("an erasure cut short")
        .when(order(1))
        .when(Step::Run);
    let (mut core, _) = given.core(1, &mut Reference);
    let commitment = core
        .log()
        .entries()
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Stated(s) => Some(s.commitment.clone()),
            Entry::Verified(_) => None,
        })
        .find(|c| core.bound().erasable(c).is_ok())
        .expect("a statement nothing rests on");
    let erasures = |core: &Core| {
        core.verified_whats()
            .iter()
            .filter(|w| matches!(w, What::Erasure { evidence } if *evidence == commitment))
            .count()
    };
    rutent_scenarios::harness::plant(
        &mut core,
        &Fault::CrashAfterRecord {
            call: "result",
            nth: 0,
        },
    );
    let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        core.erase_statement(&commitment, "retention")
    }));
    assert!(
        crashed.is_err(),
        "the crash stopped Core after the done result"
    );
    assert_eq!(erasures(&core), 0, "its erasure entry is not in the log");
    let core = given.reopen(core, 1, &mut Reference);
    assert_eq!(erasures(&core), 1, "settled, once");
    assert!(
        core.rebound_store()
            .expect("the store binds again")
            .unsettled_erasures()
            .is_empty(),
        "the store reports nothing left unsettled"
    );
    let core = given.reopen(core, 1, &mut Reference);
    assert_eq!(erasures(&core), 1, "and once only");
}

/// What a reopened intake reads its orders' inputs from, in place of the evidence store: a statement for any
/// commitment, or none.
struct Inputs(Option<Vec<u8>>);

impl rutent_core_orch::segments::Reopening for Inputs {
    fn record(
        &self,
        _: &rutent_contracts::router_api::Id,
    ) -> Option<rutent_contracts::router_api::DecisionRecord> {
        None
    }
    fn kept(&mut self, _: &rutent_contracts::module_interface::Digest) -> Option<Vec<u8>> {
        self.0.clone()
    }
}

/// A reopened intake that can't read an order's inputs back holds the order rather than run it with inputs missing:
/// the statement gone, unreadable, or naming another order. The order's own statement is read and the order isn't
/// held (`REQ-AUD-6`, `NFR-6`; the review's low 6).
// verifies: NFR-6, REQ-AUD-6
#[test]
fn order_whose_inputs_statement_names_another_order_is_held_on_reopening() {
    let history = RefCell::new(Vec::new());
    let orders = RefCell::new(Vec::new());
    let run = scenario("an order opened").when(order(1)).play_probed(
        1,
        &mut Reference,
        reference(),
        &mut |_| {},
        &mut |core| {
            *history.borrow_mut() = core.log().entries().to_vec();
        },
    );
    orders.borrow_mut().extend(run.orders.iter().cloned());
    let order = orders.borrow()[0].clone();
    let own = run
        .statements
        .iter()
        .find(|v| v["shape"] == rutent_contracts::core_internal::ORDER_INPUTS_SHAPE)
        .cloned()
        .expect("the order's inputs statement is kept");
    let mut another = own.clone();
    another["statement"]["order"] = "ord-another".into();
    let history = history.into_inner();
    let read = |bytes: Option<Vec<u8>>| {
        rutent_core_orch::intake::Intake::reopened(&history, &mut Inputs(bytes))
    };
    let intake = read(Some(serde_json::to_vec(&own).unwrap()));
    assert!(!intake.held(&order), "its own statement is read");
    assert!(intake.request_of(&order).is_some());
    for (case, bytes) in [
        (
            "another order's",
            Some(serde_json::to_vec(&another).unwrap()),
        ),
        ("gone", None),
        ("unreadable", Some(b"not json".to_vec())),
    ] {
        let intake = read(bytes);
        assert!(intake.held(&order), "{case}: held");
        assert!(intake.request_of(&order).is_none(), "{case}: not run");
    }
}
