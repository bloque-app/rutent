//! `T-024-02`'s four checks over the assembled Core, each a scenario run on the state machine's host in the
//! environment it is handed, so the reference host's run (`tests/assembly.rs`) and the run on three VOS replicas
//! (`rutent-scenarios-vos`) are one code.
//!
//! Each scenario runs the base order: [`crate::kit::RESERVE`]'s withdrawal from its bank to its ledger and its deposit back,
//! two segments of one route, which is `AC-28`'s shape, under staging set at once. Each runs under every seed
//! [`seeds`] gives, so CI's seeded step varies them.

use std::collections::BTreeMap;

use rutent_contracts::core_internal::{Action, Entry, What};
use rutent_contracts::state_machine::Order;

use crate::assembly::Staged;
use crate::assembly::machine::{Delivery, Environment};
use crate::harness::{Fault, Given, Run, SEEDS, Scenario, Step, seeded_steps, seeds};
use crate::kit::AMOUNT;

/// The acceptance criterion the base order's scenarios accept: one reserve's withdrawal and deposit make a route
/// of two segments.
pub const BASE: &str = "AC-28";

/// The seed a check runs under where CI sets none.
pub const DEFAULT_SEEDS: [u64; 1] = [1];

/// Makes a fresh environment for a swap to move the state machine to.
pub type SwapTo<'a> = &'a dyn Fn() -> Box<dyn Environment>;

fn order(n: usize) -> Step {
    Step::Order {
        request: format!("req-{n}"),
        amount: AMOUNT.to_owned(),
    }
}

fn base(name: &'static str) -> Scenario {
    Scenario::new(name, BASE, Given::one_reserve(Staged::AtOnce))
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

/// How many entries of the reserve's books moved value for real.
fn movements(run: &Run) -> usize {
    run.movements().values().sum()
}

fn check(holds: bool, why: impl FnOnce() -> String) -> Result<(), String> {
    if holds { Ok(()) } else { Err(why()) }
}

/// The host's states, step by step, against the replay's.
fn replay_holds(run: &Run) -> Result<(), String> {
    let replayed: BTreeMap<String, Vec<Option<Order>>> = run
        .replayed()
        .into_iter()
        .map(|(k, steps)| (k, steps.into_iter().map(Some).collect()))
        .collect();
    check(
        run.hosted_steps() == replayed
            && run.replayed_states() == run.states
            && !replayed.is_empty(),
        || "the host's states differ from the replay's".to_owned(),
    )
}

/// Core's state read of each order, against the host's.
fn followed_holds(run: &Run) -> Result<(), String> {
    let followed: BTreeMap<String, Option<String>> = run
        .followed
        .iter()
        .map(|(k, v)| (k.clone(), Some(v.clone())))
        .collect();
    check(
        followed == run.state_names() && !followed.is_empty(),
        || format!("Core {:?}, host {:?}", run.followed, run.state_names()),
    )
}

/// A crash between a record and its effect leaves no effect (`NFR-5`, `INV-7`): wherever the crash falls between
/// the log's record of the `nth` effect and the platform carrying it out, at the log's store before it answers or
/// at the platform's seam after it did, the record reads back from the disk, the provider's book holds no movement
/// for it, and every movement the book does hold has its effect in the log.
pub fn crash_between_record_and_effect(environment: &mut dyn Environment, swap_to: SwapTo<'_>) {
    for seed in seeds(&DEFAULT_SEEDS) {
        for nth in [0, 1] {
            for crash in [
                Fault::CrashAfterRecord {
                    call: "effect",
                    nth,
                },
                Fault::CrashBeforeEffect(nth),
            ] {
                base("a crash between the record and the effect")
                    .when(order(1))
                    .when(Step::Plant(crash.clone()))
                    .when(Step::Run)
                    .then("the crash stopped Core", |run| {
                        check(run.crashed, || "no crash".to_owned())
                    })
                    .then(
                        "the log and the evidence store read back, nothing missing",
                        |run| check(run.reopened == Ok(0), || format!("{:?}", run.reopened)),
                    )
                    .then(
                        "the record of the effect it crashed on is in the log",
                        move |run| {
                            let moves = moves(run);
                            check(moves.len() == nth + 1, || {
                                format!("effects recorded: {moves:?}")
                            })
                        },
                    )
                    .then("the provider moved nothing for it", move |run| {
                        let moved = movements(run);
                        check(moved == nth, || {
                            format!("{moved} movements for {} effects", nth + 1)
                        })
                    })
                    .then("every movement has its effect in the log", |run| {
                        let missing = run.effects_without_record();
                        check(missing.is_empty(), || format!("{missing:?}"))
                    })
                    .run(seed, environment, swap_to());
            }
        }
    }
}

/// The execution environment swapped mid-order loses no order (`NFR-6`, `REQ-OPS-3`): two orders, the state
/// machine moved to another environment after the first order's withdrawal and before its deposit. The new host
/// holds, right after the import, every journal the old one did; each input reaches the state machine once; each
/// order's every state is the one replaying its inputs reaches; and both complete there, as Core follows them.
pub fn swapped_mid_order(environment: &mut dyn Environment, swap_to: SwapTo<'_>) {
    for seed in seeds(&DEFAULT_SEEDS) {
        base("the execution environment swapped mid-order")
            .when(order(1))
            .when(order(2))
            .when(Step::Plant(Fault::SwapBeforeEffect(1)))
            .when(Step::Run)
            .then("the state machine moved once, and the move held", |run| {
                check(
                    run.environments.len() == 2 && run.swap_failed.is_none(),
                    || format!("{:?}, {:?}", run.environments, run.swap_failed),
                )
            })
            .then(
                "the new host held what the old one did, right after the import",
                |run| {
                    check(
                        run.swaps.len() == 1
                            && run
                                .swaps
                                .iter()
                                .all(|(before, after)| before == after && !before.is_empty()),
                        || {
                            format!(
                                "{:?}",
                                run.swaps
                                    .iter()
                                    .map(|(b, a)| (b.len(), a.len()))
                                    .collect::<Vec<_>>()
                            )
                        },
                    )
                },
            )
            .then("the new host holds every order", |run| {
                let missing: Vec<_> = run
                    .orders
                    .iter()
                    .filter(|o| !run.hosted.contains(&o.as_str().to_owned()))
                    .collect();
                check(missing.is_empty() && run.orders.len() == 2, || {
                    format!("{missing:?} of {:?}", run.orders)
                })
            })
            .then("each input reached the state machine once", |run| {
                check(run.deliveries.len() == run.inputs.len(), || {
                    format!(
                        "{} deliveries of {} inputs",
                        run.deliveries.len(),
                        run.inputs.len()
                    )
                })
            })
            .then(
                "each order's every state is the one replaying its inputs reaches",
                replay_holds,
            )
            .then(
                "each order completed, as the host and Core both say",
                |run| {
                    let complete: BTreeMap<String, Option<String>> = run
                        .orders
                        .iter()
                        .map(|o| (o.as_str().to_owned(), Some("complete".to_owned())))
                        .collect();
                    check(run.state_names() == complete, || {
                        format!("{:?}", run.state_names())
                    })
                    .and_then(|()| followed_holds(run))
                },
            )
            .run(seed, environment, swap_to());
    }
}

/// Replaying a scenario's recorded inputs reaches the same states (`NFR-4`), however they and the providers'
/// answers were delivered:
///
/// - each input delivered to the state machine twice, or those of two orders interleaved: the host's every state is
///   the one folding the log's inputs through the state machine alone reaches, each order's journal is in the log's
///   order, and feeding them afresh to a new host gives the same journals;
/// - each operation delivered to its provider twice, and the whole route run again: nothing moves twice, and every
///   order ends in the state it ends in with each delivered once.
pub fn replaying_recorded_inputs(environment: &mut dyn Environment, swap_to: SwapTo<'_>) {
    for seed in seeds(&DEFAULT_SEEDS) {
        for delivery in [Delivery::Repeated, Delivery::Reordered] {
            let run = base("the recorded inputs replayed")
                .when(Step::Plant(Fault::Deliver(delivery)))
                .when(order(1))
                .when(order(2))
                .when(Step::Run)
                .then("the log took inputs for every order", |run| {
                    check(
                        run.orders.len() == 2 && run.orders.iter().all(|o| run.started(o)),
                        || format!("{:?}", run.orders),
                    )
                })
                .then(
                    "folding the inputs reaches the host's every state",
                    replay_holds,
                )
                .then("and Core follows the state they reach", followed_holds)
                .then("each order's journal is in the log's order", |run| {
                    let mut logged: BTreeMap<String, Vec<String>> = BTreeMap::new();
                    for input in &run.inputs {
                        logged
                            .entry(input.subject.order.as_str().to_owned())
                            .or_default()
                            .push(input.record_id.as_str().to_owned());
                    }
                    let hosted: BTreeMap<String, Vec<String>> = run
                        .journals
                        .iter()
                        .map(|(k, j)| (k.clone(), j.iter().map(|(_, r, _)| r.clone()).collect()))
                        .collect();
                    check(hosted == logged, || {
                        "a journal out of the log's order".to_owned()
                    })
                })
                .then(
                    "a reordered delivery reached the host out of the log's order",
                    move |run| {
                        let delivered: Vec<&String> =
                            run.deliveries.iter().map(|(id, _)| id).collect();
                        let logged: Vec<String> = run
                            .inputs
                            .iter()
                            .map(|i| i.record_id.as_str().to_owned())
                            .collect();
                        check(
                            delivery != Delivery::Reordered
                                || delivered.iter().copied().ne(logged.iter()),
                            || "the reordered delivery kept the log's order".to_owned(),
                        )
                    },
                )
                .then("a repeated delivery is recorded once", move |run| {
                    let new = run.deliveries.iter().filter(|(_, new)| *new).count();
                    let times = if delivery == Delivery::Repeated { 2 } else { 1 };
                    check(
                        new == run.inputs.len() && run.deliveries.len() == times * new,
                        || {
                            format!(
                                "{new} recorded of {} inputs, {} delivered",
                                run.inputs.len(),
                                run.deliveries.len()
                            )
                        },
                    )
                })
                .run(seed, environment, swap_to());
            // The recorded inputs, fed to a fresh host, give the same journals, record for record.
            let mut fresh = environment.open().expect("a fresh host");
            for input in &run.inputs {
                fresh
                    .submit(input.clone())
                    .expect("the host takes a recorded input");
            }
            for (order, journal) in &run.journals {
                let at = rutent_contracts::state_machine::Id::new(order.as_str()).expect("an id");
                let again: Vec<(u64, String, Option<Order>)> = fresh
                    .journal(&at)
                    .expect("a journal")
                    .iter()
                    .map(|r| {
                        (
                            r.seq,
                            r.input.record_id.as_str().to_owned(),
                            r.state_after.clone(),
                        )
                    })
                    .collect();
                assert_eq!(
                    &again, journal,
                    "{order}'s journal, replayed into a fresh host"
                );
            }
        }
        provider_deliveries_repeated(seed, environment, swap_to);
    }
}

/// Each operation delivered to its provider twice, and the route run again: nothing moves twice, and the orders end
/// where they end with each delivered once.
fn provider_deliveries_repeated(seed: u64, environment: &mut dyn Environment, swap_to: SwapTo<'_>) {
    let once = base("each provider delivery once")
        .when(order(1))
        .when(order(2))
        .when(Step::Run)
        .run(seed, environment, swap_to());
    let ended: BTreeMap<String, Option<String>> = once
        .state_names()
        .into_values()
        .enumerate()
        .map(|(i, s)| (i.to_string(), s))
        .collect();
    let moved = movements(&once);
    base("each provider delivery twice, and the route run again")
        .when(Step::Plant(Fault::DeliverTwice))
        .when(order(1))
        .when(order(2))
        .when(Step::Run)
        .when(Step::Rerun)
        .then("every operation reached its provider twice", |run| {
            check(run.delivered_twice == 4, || {
                format!("{} delivered twice", run.delivered_twice)
            })
        })
        .then("each transaction moved value once", |run| {
            let twice: Vec<_> = run
                .movements()
                .into_iter()
                .filter(|(_, n)| *n > 1)
                .collect();
            check(twice.is_empty() && !run.movements().is_empty(), || {
                format!("{twice:?}")
            })
        })
        .then(
            "as many movements as with each delivered once",
            move |run| {
                check(movements(run) == moved, || {
                    format!("{} movements, {moved} once", movements(run))
                })
            },
        )
        .then(
            "every order ends where it ends with each delivered once",
            move |run| {
                let now: BTreeMap<String, Option<String>> = run
                    .state_names()
                    .into_values()
                    .enumerate()
                    .map(|(i, s)| (i.to_string(), s))
                    .collect();
                check(now == ended, || format!("{now:?}, once {ended:?}"))
            },
        )
        .then(
            "and the host holds what replaying the log's inputs reaches",
            replay_holds,
        )
        .then("every movement has its effect in the log", |run| {
            let missing = run.effects_without_record();
            check(missing.is_empty(), || format!("{missing:?}"))
        })
        .run(seed, environment, swap_to());
}

/// A scenario run twice with one seed gives one run (`DEC-18`): for each seed, the seeded scenario, which draws its
/// amounts, its delivery fault, a swap and a provider's silence from the seed, run twice, projects to one run; and
/// the same steps under two seeds project to two, so the seed reaches what Core draws.
pub fn one_seed_one_run(environment: &mut dyn Environment, swap_to: SwapTo<'_>) {
    for seed in seeds(&[1, 2, 3]) {
        let scenario = || {
            let mut s = base("a seeded run");
            for step in seeded_steps(seed) {
                s = s.when(step);
            }
            s
        };
        let first = scenario().run(seed, environment, swap_to());
        let second = scenario().run(seed, environment, swap_to());
        assert!(!first.log.is_empty(), "seed {seed}: the run left a log");
        assert_eq!(
            first.projection(),
            second.projection(),
            "seed {seed}: two runs from one seed differ; replay with {SEEDS}={seed}"
        );
        // The same steps under the next seed: only what Core draws from the seed differs.
        let one = base("one order, two seeds").when(order(1)).when(Step::Run);
        let here = one.play(seed, environment, swap_to());
        let next = one.play(seed.wrapping_add(1), environment, swap_to());
        let strip = |run: &Run| {
            let mut p = run.projection();
            p["seed"] = serde_json::Value::Null;
            p
        };
        assert_ne!(
            strip(&here),
            strip(&next),
            "seeds {seed} and {}: one run, so the seed reaches nothing Core draws",
            seed.wrapping_add(1)
        );
    }
}

/// A Core stopped mid-order and opened again on its disk takes what its grading took before it stopped
/// (`Closing::recorded`, `T-020-09`, `REQ-ORD-38`, `REQ-ORD-33`): the first order closes on its deposit's public movement;
/// Core crashes before the second order's withdrawal; opened again on the same disk, with the reserve now paying two
/// orders with one payment, a third order to the same account for the same amount is tied to the first's movement, and
/// its deposit waits rather than close on it a second time. Nothing stops on a refusal of the log, and no movement
/// evidences two orders.
pub fn restart_keeps_what_closing_took(environment: &mut dyn Environment, swap_to: SwapTo<'_>) {
    for seed in seeds(&DEFAULT_SEEDS) {
        base("a Core opened again keeps what its grading took")
            .when(order(1))
            .when(Step::Run)
            .when(order(2))
            .when(Step::Plant(Fault::CrashBeforeEffect(2)))
            .when(Step::Run)
            .when(Step::Restart)
            .when(Step::Plant(Fault::OnePaymentForTwo))
            .when(order(3))
            .when(Step::Run)
            .then("Core was stopped mid-order", |run| {
                check(run.crashed && run.orders.len() == 3, || {
                    format!("crashed {}, orders {:?}", run.crashed, run.orders)
                })
            })
            .then("the first order completed", |run| {
                let first = run.orders[0].as_str();
                check(
                    run.state_names().get(first) == Some(&Some("complete".to_owned())),
                    || format!("{:?}", run.state_names()),
                )
            })
            .then("nothing stopped on the log's refusal", |run| {
                check(run.stopped.is_empty(), || format!("{:?}", run.stopped))
            })
            .then(
                "the third order's deposit waits: its only payment closed the first",
                |run| {
                    let third = run.orders[2].as_str();
                    let settled = run
                        .inputs
                        .iter()
                        .filter(|i| {
                            i.subject.order.as_str() == third
                                && matches!(
                                    i.observed,
                                    rutent_contracts::state_machine::Observed::SegmentSettled { .. }
                                )
                        })
                        .count();
                    check(
                        settled == 1
                            && run.state_names().get(third) != Some(&Some("complete".to_owned())),
                        || format!("{settled} settled, {:?}", run.state_names().get(third)),
                    )
                },
            )
            .then("no movement evidences two orders", |run| {
                let mut by: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
                for e in &run.log {
                    if let Entry::Verified(v) = e
                        && let What::Evidence {
                            order,
                            movement: Some(m),
                            ..
                        } = &v.what
                    {
                        let orders = by
                            .entry((m.network.as_str().to_owned(), m.id.as_str().to_owned()))
                            .or_default();
                        if !orders.contains(&order.as_str().to_owned()) {
                            orders.push(order.as_str().to_owned());
                        }
                    }
                }
                check(!by.is_empty() && by.values().all(|o| o.len() == 1), || {
                    format!("{by:?}")
                })
            })
            .run(seed, environment, swap_to());
    }
}
