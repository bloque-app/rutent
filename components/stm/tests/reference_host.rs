//! The reference host against the host-contract suite (`T-005-04`): it must pass every cell, the baseline both
//! `EXP-1` candidates are compared with (`DEC-6`).

use rutent_conformance::host::probe::Probe;
use rutent_conformance::host::{
    CellOutcome, RunConfig, Verdict, run_black_box, run_with_fault_seam,
};
use rutent_stm::reference_host::ReferenceHarness;

fn config(run_id: &str) -> RunConfig {
    RunConfig {
        run_id: run_id.into(),
        candidate: "reference-host".into(),
        target: "reference-host".into(),
        seed: 20_261_106,
    }
}

fn failures(report: &rutent_conformance::host::Report) -> Vec<String> {
    report
        .cells
        .iter()
        .filter(|(_, o)| !matches!(o, CellOutcome::Pass | CellOutcome::NotRun(_)))
        .map(|(c, o)| format!("{}: {o:?}", c.id))
        .collect()
}

// verifies: REQ-OPS-3, DEC-6, DEC-5
#[test]
fn the_reference_host_passes_the_host_contract_suite() {
    let report = run_with_fault_seam(
        config("t-005-04-reference-with-fault-seam"),
        &mut ReferenceHarness::<Probe>::new(),
        &mut ReferenceHarness::<Probe>::new(),
    );
    let failed = failures(&report);
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert_eq!(report.verdict(), Verdict::Pass, "{}", report.to_markdown());
}

// verifies: REQ-OPS-3, DEC-6
#[test]
fn the_reference_host_driven_black_box_passes_every_cell_it_can_run() {
    let report = run_black_box(
        config("t-005-04-reference-black-box"),
        &mut ReferenceHarness::<Probe>::new(),
        &mut ReferenceHarness::<Probe>::new(),
    );
    let failed = failures(&report);
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert_eq!(report.verdict(), Verdict::Incomplete);
}
