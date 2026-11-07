//! A guest's CPU and memory limits (`REQ-MDL-11`, `B-03`, `B-04`): a guest past either is stopped, its operation
//! is refused, and its provider keeps answering.

mod common;

use std::time::Instant;

use common::{answers_as, binding, install, languages, spec};
use rutent_conformance::boundary::{Handled, ModuleBinding, PlatformRefusal};
use rutent_wasm_binding::engine::{FUEL_PER_CALL, MEMORY_PER_PART};
use rutent_wasm_binding::program::Probe;

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
        eprintln!(
            "B-03 [{language}]: {FUEL_PER_CALL} units of fuel; stopped after {took:?}: {why}"
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
