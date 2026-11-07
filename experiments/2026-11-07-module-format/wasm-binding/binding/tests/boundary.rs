//! The module-boundary suite (`T-003-03`) run against the Wasm binding, once per provider category's stand-ins,
//! in Rust and C.

mod common;

use rutent_conformance::boundary::{RunConfig, run_boundary_suite};

fn run(category: &str) {
    let mut binding = common::binding(category);
    let config = RunConfig {
        run_id: format!("test-{category}"),
        ..RunConfig::default()
    };
    let report = run_boundary_suite(&mut binding, &config);
    assert!(
        report.findings.is_empty() && report.uncovered.is_empty(),
        "findings: {:#?}\nuncovered: {:#?}",
        report.findings,
        report.uncovered
    );
    assert_eq!(report.format, "wasm-component");
    assert!(report.passed(), "{report:#?}");
}

// verifies: AC-20, REQ-MDL-15, REQ-MDL-9, REQ-MDL-6, REQ-MDL-7, REQ-MDL-12, AC-19, AC-21
#[test]
fn the_reserve_stand_ins_pass_the_module_boundary_suite() {
    run("reserve");
}

// verifies: AC-20, REQ-MDL-15, REQ-MDL-9, REQ-MDL-6, REQ-MDL-7, REQ-MDL-12, AC-19, AC-21
#[test]
fn the_exchange_stand_ins_pass_the_module_boundary_suite() {
    run("exchange");
}

// verifies: AC-20, REQ-MDL-15, REQ-MDL-9, REQ-MDL-6, REQ-MDL-7, REQ-MDL-12, AC-19, AC-21
#[test]
fn the_ramp_stand_ins_pass_the_module_boundary_suite() {
    run("ramp");
}

// verifies: AC-20, REQ-MDL-15, REQ-MDL-9, REQ-MDL-6, REQ-MDL-7, REQ-MDL-12, AC-19, AC-21
#[test]
fn the_verifier_stand_ins_pass_the_module_boundary_suite() {
    run("verifier");
}
