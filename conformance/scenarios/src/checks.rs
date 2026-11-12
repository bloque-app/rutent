//! `T-024-02`'s four checks over the assembled Core, each a scenario run on the state machine's host in the
//! environment it is handed, so the reference host's run (`tests/assembly.rs`) and the run on three VOS replicas
//! (`rutent-scenarios-vos`) are one code.
//!
//! Each scenario runs the base order: [`RESERVE`]'s withdrawal from its bank to its ledger and its deposit back,
//! two segments of one route, which is `AC-28`'s shape, under staging set at once.

use std::collections::BTreeMap;

use rutent_contracts::core_internal::{Action, Entry, What};

use crate::assembly::Staged;
use crate::assembly::machine::{Delivery, Environment};
use crate::harness::{Fault, Given, Run, Scenario, Step, seeded_steps, seeds};
use crate::kit::{AMOUNT, RESERVE};

/// The acceptance criterion the base order's scenarios accept: one reserve's withdrawal and deposit make a route
/// of two segments.
pub const BASE: &str = "AC-28";

/// Makes a fresh environment for a swap to move the state machine to.
pub type SwapTo<'a> = &'a dyn Fn() -> Box<dyn Environment>;

fn order(n: usize) -> Step {
    Step::Order {
        request: format!("req-{n}"),
        amount: AMOUNT.to_owned(),
    }
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

/// How many entries of the reserve's book moved value for real.
fn movements(run: &Run) -> usize {
    run.books.get(RESERVE).map_or(0, |book| {
        book.iter()
            .filter(|e| {
                let op = &e["operation"]["statement"];
                matches!(op["kind"].as_str(), Some("withdrawal" | "deposit"))
                    && op["dry_run"].as_bool() != Some(true)
            })
            .count()
    })
}

fn check(holds: bool, why: impl FnOnce() -> String) -> Result<(), String> {
    if holds { Ok(()) } else { Err(why()) }
}

/// A crash between a record and its effect leaves no effect (`NFR-5`, `INV-7`): wherever the crash falls between
/// the log's record of the `nth` effect and the platform carrying it out, at the log's store before it answers or
/// at the platform's seam after it did, the record reads back from the disk, the provider's book holds no movement
/// for it, and every movement the book does hold has its effect in the log.
pub fn crash_between_record_and_effect(environment: &mut dyn Environment, swap_to: SwapTo<'_>) {
    for nth in [0, 1] {
        for crash in [
            Fault::CrashAfterRecord {
                call: "effect",
                nth,
            },
            Fault::CrashBeforeEffect(nth),
        ] {
            Scenario::new(
                "a crash between the record and the effect",
                BASE,
                Given::one_reserve(Staged::AtOnce),
            )
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
            .run(1, environment, swap_to());
        }
    }
}

/// The execution environment swapped mid-order loses no order (`NFR-6`, `REQ-OPS-3`): two orders, the state
/// machine moved to another environment after the first order's withdrawal and before its deposit, and both
/// complete there, every input the log took for each recorded once in the new host, its state the one replaying
/// them reaches, and the one Core follows.
pub fn swapped_mid_order(environment: &mut dyn Environment, swap_to: SwapTo<'_>) {
    Scenario::new(
        "the execution environment swapped mid-order",
        BASE,
        Given::one_reserve(Staged::AtOnce),
    )
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
    .then("each order's every input is recorded once", |run| {
        let short: Vec<_> = run
            .orders
            .iter()
            .map(|o| (o.as_str().to_owned(), run.recorded(o)))
            .filter(|(_, (inputs, journal))| inputs != journal || *inputs == 0)
            .collect();
        check(short.is_empty(), || format!("(inputs, records): {short:?}"))
    })
    .then(
        "each order completed, as the host, a replay and Core all say",
        |run| {
            let complete: BTreeMap<String, Option<String>> = run
                .orders
                .iter()
                .map(|o| (o.as_str().to_owned(), Some("complete".to_owned())))
                .collect();
            let followed: BTreeMap<String, Option<String>> = run
                .followed
                .iter()
                .map(|(k, v)| (k.clone(), Some(v.clone())))
                .collect();
            check(
                run.states == complete && run.replayed() == complete && followed == complete,
                || {
                    format!(
                        "host {:?}, replay {:?}, Core {:?}",
                        run.states,
                        run.replayed(),
                        run.followed
                    )
                },
            )
        },
    )
    .run(1, environment, swap_to());
}

/// Replaying a scenario's recorded inputs reaches the same states (`NFR-4`): over two orders, with each input
/// delivered twice and the orders' inputs interleaved, the inputs the log took, folded through the state machine
/// alone and fed afresh to a new host, reach the states the host recorded and the ones Core follows.
pub fn replaying_recorded_inputs(environment: &mut dyn Environment, swap_to: SwapTo<'_>) {
    for delivery in [Delivery::Repeated, Delivery::Reordered] {
        let run = Scenario::new(
            "the recorded inputs replayed",
            BASE,
            Given::one_reserve(Staged::AtOnce),
        )
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
        .then("folding the inputs reaches the host's states", |run| {
            check(run.replayed() == run.states, || {
                format!("replay {:?}, host {:?}", run.replayed(), run.states)
            })
        })
        .then("and the ones Core follows", |run| {
            let followed: BTreeMap<String, Option<String>> = run
                .followed
                .iter()
                .map(|(k, v)| (k.clone(), Some(v.clone())))
                .collect();
            check(followed == run.states && !followed.is_empty(), || {
                format!("Core {:?}, host {:?}", run.followed, run.states)
            })
        })
        .then(
            "a reordered delivery reached the host out of the log's order",
            move |run| {
                let delivered: Vec<&String> = run.deliveries.iter().map(|(id, _)| id).collect();
                let logged: Vec<String> = run
                    .inputs
                    .iter()
                    .map(|i| i.record_id.as_str().to_owned())
                    .collect();
                check(
                    delivery != Delivery::Reordered || delivered.iter().copied().ne(logged.iter()),
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
        .run(1, environment, swap_to());
        // The recorded inputs, fed to a fresh host, give the same journals.
        let mut fresh = environment.open().expect("a fresh host");
        for input in &run.inputs {
            fresh
                .submit(input.clone())
                .expect("the host takes a recorded input");
        }
        for (order, journal) in &run.journals {
            let at = rutent_contracts::state_machine::Id::new(order.as_str()).expect("an id");
            let again: Vec<(u64, String)> = fresh
                .journal(&at)
                .expect("a journal")
                .iter()
                .map(|r| (r.seq, r.input.record_id.as_str().to_owned()))
                .collect();
            let was: Vec<(u64, String)> = journal.iter().map(|(s, r, _)| (*s, r.clone())).collect();
            assert_eq!(again, was, "{order}'s journal, replayed into a fresh host");
        }
    }
}

/// A scenario run twice with one seed gives one run (`DEC-18`): for each seed, the seeded scenario, which draws its
/// amounts, its delivery fault, a swap and a provider's silence from the seed, run twice, projects to one run.
pub fn one_seed_one_run(environment: &mut dyn Environment, swap_to: SwapTo<'_>) {
    for seed in seeds(&[1, 2, 3]) {
        let scenario = || {
            let mut s = Scenario::new("a seeded run", BASE, Given::one_reserve(Staged::AtOnce));
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
            "seed {seed}: two runs from one seed differ; replay with {}={seed}",
            crate::harness::SEEDS
        );
    }
}
