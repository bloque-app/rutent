//! A guest's CPU, time and memory limits (`REQ-MDL-11`, `B-03`, `B-04`): a guest past any is stopped, its operation
//! is refused, and its provider keeps answering. CPU is bounded per operation: the outside part's calls for one
//! operation share one budget of fuel, and the platform kills a process that hasn't answered by the deadline.

#[path = "module_support/mod.rs"]
mod common;

use std::time::{Duration, Instant};

use common::Probe;
use common::{answers_as, binding, install, languages, spec};
use rutent_conformance::boundary::{Handled, ModuleBinding, PlatformRefusal};
use rutent_prov::module::engine::{FUEL_PER_PART, MEMORY_PER_PART};
use rutent_prov::module::platform::OPERATION_DEADLINE;

// verifies: REQ-MDL-11
#[test]
fn a_guest_that_loops_is_stopped_when_its_fuel_runs_out() {
    for language in languages() {
        let mut binding = binding("reserve");
        let (looping, steady) = (format!("looping-{language}"), format!("steady-{language}"));
        install(
            &mut binding,
            &spec(&steady, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[],
        );
        install(
            &mut binding,
            &spec(&looping, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[Probe::Spin],
        );
        let pid = binding.process(&looping);
        let started = Instant::now();
        let handled = binding.handle(&looping, &common::withdrawal(&looping, 1));
        let took = started.elapsed();
        assert_eq!(
            handled,
            Handled::Refused(PlatformRefusal::Unavailable),
            "a looping guest answered"
        );
        assert_eq!(
            binding.process(&looping),
            pid,
            "running out of fuel ended the process"
        );
        assert!(
            answers_as(&mut binding, &steady, "1.0.0", 2),
            "another provider stopped answering"
        );
        let why = binding.measurements.last_trap.clone().unwrap_or_default();
        assert!(why.contains("fuel"), "it stopped for another reason: {why}");
        assert!(
            took < OPERATION_DEADLINE / 4,
            "running out of fuel took {took:?}, too near the deadline"
        );
        eprintln!(
            "B-03 [{language}]: {FUEL_PER_PART} units of fuel; stopped after {took:?}: {why}"
        );
    }
}

// verifies: REQ-MDL-11
#[test]
fn a_guest_growing_its_memory_past_the_bound_is_refused_the_memory() {
    let bound_pages = (MEMORY_PER_PART / 65536) as u32;
    for language in languages() {
        let mut binding = binding("reserve");
        let (small, large) = (format!("small-{language}"), format!("large-{language}"));
        install(
            &mut binding,
            &spec(&small, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[Probe::Grow(16)],
        );
        install(
            &mut binding,
            &spec(&large, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[Probe::Grow(bound_pages)],
        );
        assert!(
            answers_as(&mut binding, &small, "1.0.0", 1),
            "growing within the bound was refused"
        );
        let pid = binding.process(&large);
        let handled = binding.handle(&large, &common::withdrawal(&large, 2));
        assert_eq!(
            handled,
            Handled::Refused(PlatformRefusal::Unavailable),
            "growing past the bound gave an answer"
        );
        assert_eq!(
            binding.process(&large),
            pid,
            "the refusal ended the process"
        );
        assert!(
            answers_as(&mut binding, &small, "1.0.0", 3),
            "another provider stopped answering"
        );
        let why = binding.measurements.last_trap.clone().unwrap_or_default();
        eprintln!(
            "B-04 [{language}]: bound {MEMORY_PER_PART} bytes; growing by {bound_pages} pages refused: {why}"
        );
    }
}

/// How many `burned` values the provider's answer to a withdrawal carries, or `None` if it gave no answer.
fn burned(binding: &mut common::WasmBinding, provider: &str, n: usize) -> Option<usize> {
    let Handled::Answered(result) = binding.handle(provider, &common::withdrawal(provider, n))
    else {
        return None;
    };
    let value = serde_json::to_value(&result.statement).ok()?;
    let reference = value["outcome"]["evidence"][0]["reference"]
        .as_str()?
        .to_owned();
    Some(reference.matches("-burned").count())
}

// verifies: REQ-MDL-11
#[test]
fn the_outside_parts_calls_for_one_operation_share_one_budget_of_fuel() {
    // Each outside request costs the outside part well under its fuel: 2,000,000 iterations, about 14 million
    // units in `c` and 32 million in `rs` on arm64 macOS, the parsing of its request included. Thirty of them cost
    // more than one budget, so a fresh budget per call would let all thirty finish, and one per operation can't.
    const TIMES: usize = 30;
    for language in languages() {
        let mut binding = binding("reserve");
        let provider = format!("burning-{language}");
        install(
            &mut binding,
            &spec(&provider, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[Probe::Burn {
                iterations: 2_000_000,
                times: TIMES,
            }],
        );
        let pid = binding.process(&provider);
        let first = burned(&mut binding, &provider, 1).expect("the operation was answered");
        assert!(
            first >= 1,
            "not one outside request finished within the budget"
        );
        assert!(
            first < TIMES,
            "all {TIMES} outside requests finished: the outside part's fuel was refilled per call"
        );
        // The next operation has a budget of its own, and fuel is deterministic.
        let second = burned(&mut binding, &provider, 2).expect("the next operation was answered");
        assert_eq!(second, first, "the next operation had another budget");
        assert_eq!(
            binding.process(&provider),
            pid,
            "running out of fuel ended the process"
        );
        eprintln!(
            "B-03 [{language}]: {first} of {TIMES} outside requests finished on one operation's {FUEL_PER_PART} units"
        );
    }
}

// verifies: REQ-MDL-11, REQ-MDL-13
#[test]
fn a_provider_process_that_misses_the_operation_deadline_is_killed_and_started_again() {
    for language in languages() {
        let mut binding = binding("reserve");
        let (late, steady) = (format!("late-{language}"), format!("steady-{language}"));
        install(
            &mut binding,
            &spec(&steady, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[],
        );
        install(
            &mut binding,
            &spec(&late, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[Probe::Spin],
        );
        let (late_pid, steady_pid) = (binding.process(&late), binding.process(&steady));
        // Far below what the spinning guest takes to run out of fuel, so the deadline, not fuel, stops it.
        let deadline = Duration::from_millis(1);
        binding.set_deadline(deadline);
        let started = Instant::now();
        let handled = binding.handle(&late, &common::withdrawal(&late, 1));
        let took = started.elapsed();
        binding.set_deadline(OPERATION_DEADLINE);
        assert_eq!(
            handled,
            Handled::Refused(PlatformRefusal::Unavailable),
            "an operation past its deadline was answered"
        );
        let why = binding.measurements.last_trap.clone().unwrap_or_default();
        assert!(
            why.starts_with("deadline"),
            "it stopped for another reason: {why}"
        );
        assert_eq!(
            binding.measurements.overdue, 1,
            "the deadline wasn't counted"
        );
        assert!(
            binding.process(&late).is_some() && binding.process(&late) != late_pid,
            "the late process wasn't replaced"
        );
        if let Some(old) = late_pid {
            assert!(!common::alive(old), "the late process is still alive");
        }
        assert_eq!(
            binding.process(&steady),
            steady_pid,
            "another provider's process changed"
        );
        assert!(
            answers_as(&mut binding, &steady, "1.0.0", 2),
            "another provider stopped answering"
        );
        eprintln!("deadline [{language}]: {deadline:?}; refused after {took:?}: {why}");
    }
}
