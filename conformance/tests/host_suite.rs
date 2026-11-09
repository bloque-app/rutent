//! The host-contract suite's own tests (`T-005-03`): it passes a correct in-memory double, and each broken double
//! fails the cell for the fault it breaks, with a detail that names the cause.

mod support;

use rutent_conformance::host::probe::{self, Probe};
use rutent_conformance::host::{
    CELLS, CellOutcome, Fault, Report, RunConfig, Verdict, run_black_box, run_with_fault_seam,
};
use rutent_contracts::state_machine::host::GUARANTEES;
use rutent_contracts::state_machine::{OrderState, Outcome, StateMachine};
use support::{DoubleHarness, Flaw};

const SEED: u64 = 20_261_106;

fn config(candidate: &str) -> RunConfig {
    RunConfig {
        run_id: format!("test-{candidate}"),
        candidate: candidate.into(),
        target: candidate.into(),
        seed: SEED,
    }
}

/// A run of the double with the flaw, swapped onto another of the same kind: the flaw is on both sides of a swap.
fn run(flaw: Flaw) -> Report {
    run_with_fault_seam(
        config(&format!("{flaw:?}")),
        &mut DoubleHarness::<Probe>::new(flaw),
        &mut DoubleHarness::<Probe>::new(flaw),
    )
}

/// The cell fails on the broken double with a detail containing `cause`, and passes on the correct one.
fn fails_only_when_broken(flaw: Flaw, cell: &str, cause: &str) {
    match run(flaw).outcome(cell) {
        Some(CellOutcome::Fail(detail)) => assert!(
            detail.contains(cause),
            "{cell} failed on {flaw:?} for another reason: {detail}"
        ),
        other => panic!("{cell} didn't fail on {flaw:?}: {other:?}"),
    }
    assert_eq!(
        run(Flaw::None).outcome(cell),
        Some(&CellOutcome::Pass),
        "{cell} fails on a correct host"
    );
}

/// The cell fails on the broken double, naming `cause`, and every other cell passes on it.
fn fails_only(flaw: Flaw, cell: &str, cause: &str) {
    fails_only_when_broken(flaw, cell, cause);
    let report = run(flaw);
    let others: Vec<&str> = report
        .cells
        .iter()
        .filter(|(c, o)| c.id != cell && o != &CellOutcome::Pass)
        .map(|(c, _)| c.id)
        .collect();
    assert!(others.is_empty(), "{flaw:?} also fails {others:?}");
}

// verifies: INV-7, NFR-5
#[test]
fn crash_between_record_and_effect_leaves_no_effect() {
    fails_only_when_broken(
        Flaw::AcknowledgesBeforeDurable,
        "released-records-survive-a-kill",
        "an effect could act on it with no record",
    );
    let correct = run(Flaw::None);
    for cell in [
        "stop-after-received",
        "stop-after-computed",
        "stop-after-durable",
    ] {
        assert_eq!(correct.outcome(cell), Some(&CellOutcome::Pass), "{cell}");
    }
    assert_eq!(
        correct.fault(Fault::CrashBetweenRecordAndEffect),
        Verdict::Pass
    );
}

// verifies: REQ-STM-4, INV-8
#[test]
fn repeated_input_moves_nothing_twice() {
    fails_only_when_broken(
        Flaw::RecordsARepeatTwice,
        "repeat-returns-the-recorded-record",
        "made a second record",
    );
    assert_eq!(
        run(Flaw::RecordsARepeatTwice).guarantee("exactly-once"),
        Verdict::Fail
    );
    assert_eq!(
        run(Flaw::None).fault(Fault::ReorderedDelivery),
        Verdict::Pass
    );
}

// verifies: NFR-6, REQ-OPS-3
#[test]
fn swap_mid_order_loses_no_order() {
    fails_only_when_broken(
        Flaw::LosesAnOrderOnSwap,
        "swap-mid-order-loses-no-order",
        "order ord-2 is missing after the swap",
    );
}

// verifies: NFR-4, REQ-STM-3
#[test]
fn replay_reaches_the_same_states() {
    fails_only_when_broken(
        Flaw::ReplayDiverges,
        "replay-reaches-the-same-states",
        "the recorded state differs from the replay",
    );
}

// verifies: DEC-5
#[test]
fn a_correct_host_passes_every_cell() {
    let report = run(Flaw::None);
    let failed: Vec<_> = report
        .cells
        .iter()
        .filter(|(_, o)| o != &CellOutcome::Pass)
        .map(|(c, o)| format!("{}: {o:?}", c.id))
        .collect();
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert_eq!(report.verdict(), Verdict::Pass);
}

// verifies: DEC-5
#[test]
fn every_guarantee_and_every_planned_fault_has_a_cell() {
    for g in GUARANTEES {
        assert!(
            CELLS.iter().any(|c| c.guarantees.contains(&g.id)),
            "no cell checks `{}`",
            g.id
        );
    }
    for c in CELLS {
        for g in c.guarantees {
            assert!(
                GUARANTEES.iter().any(|known| &known.id == g),
                "{} names an unknown guarantee {g}",
                c.id
            );
        }
    }
    let plan = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../PLAN.md")).unwrap();
    let row = plan
        .lines()
        .find(|l| l.starts_with("| `T-005-03` |"))
        .expect("PLAN's T-005-03 row");
    let work = row.split('|').nth(2).unwrap();
    for fault in [
        Fault::CrashBetweenRecordAndEffect,
        Fault::RepeatedDelivery,
        Fault::ReorderedDelivery,
        Fault::SwapMidOrder,
        Fault::ReplayOfARecordedRun,
    ] {
        let words = fault.name().replace(" delivery", "");
        assert!(
            work.contains(&words),
            "PLAN's T-005-03 work names no `{words}`"
        );
        assert!(
            CELLS.iter().any(|c| c.fault == fault),
            "no cell injects {}",
            fault.name()
        );
    }
    let ids: std::collections::BTreeSet<_> = CELLS.iter().map(|c| c.id).collect();
    assert_eq!(ids.len(), CELLS.len(), "cell ids are not unique");
}

// verifies: DEC-5
#[test]
fn a_run_reports_per_fault_and_per_guarantee_with_its_provenance() {
    let report = run(Flaw::ReplayDiverges);
    let text = report.to_markdown();
    assert!(text.contains("### Per fault") && text.contains("### Per guarantee"));
    let provenance = format!(
        "| test-ReplayDiverges | ReplayDiverges | ReplayDiverges | {} | {} | {SEED} |",
        rutent_contracts::state_machine::host::VERSION,
        rutent_contracts::state_machine::VERSION
    );
    let rows: Vec<&str> = text.lines().filter(|l| l.starts_with("| test-")).collect();
    assert_eq!(rows.len(), CELLS.len() + GUARANTEES.len());
    assert!(
        rows.iter().all(|r| r.starts_with(&provenance)),
        "a row lacks its provenance"
    );
    for fault in Fault::ALL {
        assert!(
            text.contains(&format!("| {} |", fault.name())),
            "no row for {}",
            fault.name()
        );
    }
    assert!(text.contains("| `replay` | 1 | fail |"));
    assert_eq!(report.verdict(), Verdict::Fail);
}

// verifies: DEC-5, NFR-6
#[test]
fn a_black_box_run_reports_the_named_stops_as_not_run() {
    let report = run_black_box(
        config("black-box"),
        &mut DoubleHarness::<Probe>::new(Flaw::None),
        &mut DoubleHarness::<Probe>::new(Flaw::None),
    );
    for (cell, outcome) in &report.cells {
        match (cell.needs_fault_seam, outcome) {
            (true, CellOutcome::NotRun(why)) => assert!(why.contains("FaultSeam")),
            (false, CellOutcome::Pass) => {}
            (_, other) => panic!("{}: {other:?}", cell.id),
        }
    }
    assert_eq!(report.guarantee("durable"), Verdict::Incomplete);
    assert_eq!(report.guarantee("swap"), Verdict::Pass);
    assert_eq!(report.verdict(), Verdict::Incomplete);
}

// verifies: REQ-STM-3, DEC-5
#[test]
fn the_probe_opens_moves_and_names_every_outcome() {
    let inputs = probe::order_inputs("ord-1", 0);
    for input in inputs.iter().chain([&probe::orphan(9)]) {
        assert!(
            input.check().is_ok(),
            "{} is not a valid input",
            input.record_id.as_str()
        );
    }
    assert!(
        Probe::open(&inputs[1]).is_none(),
        "a binding opened an order"
    );
    let states = probe::fold(&inputs);
    let last = states.last().unwrap().as_ref().unwrap();
    assert_eq!(last.state, OrderState::Complete);
    assert!(
        states.iter().flatten().all(|o| o.check().is_ok()),
        "the probe returned an invalid order"
    );

    let repeat = Probe::transition(last, &inputs[6]);
    assert_eq!(repeat.state, last.state);
    assert_eq!(repeat.last_input.outcome, Outcome::Repeat {});

    let opened = Probe::open(&inputs[0]).unwrap();
    let early = Probe::transition(&opened, &inputs[2]);
    assert!(matches!(
        early.last_input.outcome,
        Outcome::NotApplicable { .. }
    ));
    assert_eq!(
        early.parts, opened.parts,
        "an out-of-order settle moved a segment"
    );
    assert!(early.check().is_ok());
}

// verifies: REQ-STM-4
#[test]
fn the_fixed_seed_reorders_the_delivery() {
    let recorded = probe::order_inputs("ord-1", 0);
    let mut submitted = recorded.clone();
    probe::shuffle(&mut submitted, SEED);
    assert_ne!(
        submitted, recorded,
        "the seed leaves the delivery in its recorded order"
    );
    let mut again = recorded.clone();
    probe::shuffle(&mut again, SEED);
    assert_eq!(again, submitted, "the shuffle isn't deterministic");
}

// verifies: INV-7, NFR-5
#[test]
fn a_state_written_before_its_record_fails_the_stop_cells() {
    fails_only_when_broken(
        Flaw::CachesStateBeforeRecord,
        "stop-after-computed",
        "differs from the probe's",
    );
}

// verifies: REQ-AUD-1, INV-8
#[test]
fn a_refusal_log_kept_in_memory_fails_across_a_kill() {
    fails_only_when_broken(
        Flaw::KeepsRefusalsInMemory,
        "reused-record-id-is-refused-and-logged",
        "after a kill",
    );
}

// verifies: REQ-STM-4, INV-8
#[test]
fn an_input_equality_without_recorded_at_fails_the_reuse_cell() {
    fails_only_when_broken(
        Flaw::IgnoresRecordedAt,
        "reused-record-id-is-refused-and-logged",
        "differs only in recorded_at",
    );
}

// verifies: REQ-STM-4, INV-8
#[test]
fn a_record_id_looked_up_per_order_fails_the_reuse_cell() {
    fails_only_when_broken(
        Flaw::DedupsPerOrder,
        "reused-record-id-is-refused-and-logged",
        "differs only in its order",
    );
}

// verifies: REQ-STM-3, REQ-STM-4
#[test]
fn one_sequence_across_orders_fails_the_reorder_cell() {
    fails_only_when_broken(
        Flaw::OneSequenceAcrossOrders,
        "reordered-delivery-is-applied-in-submit-order",
        "gap",
    );
}

// verifies: NFR-6, REQ-OPS-3
#[test]
fn an_import_that_checks_nothing_fails_the_import_refusal_cell() {
    fails_only_when_broken(
        Flaw::AcceptsAnyImport,
        "import-refuses-a-stream-it-cannot-take",
        "was imported",
    );
}

// verifies: NFR-6, REQ-OPS-3
#[test]
fn a_submit_kept_while_halted_fails_the_halt_cell() {
    fails_only_when_broken(
        Flaw::QueuesSubmitsWhileHalted,
        "halted-host-refuses-and-only-a-halted-host-exports",
        "holding an input the host refused while halted",
    );
}

// verifies: NFR-6, REQ-AUD-1
#[test]
fn an_input_that_opened_nothing_must_cross_the_swap() {
    fails_only_when_broken(
        Flaw::DropsStatelessRecordsOnExport,
        "swap-mid-order-loses-no-order",
        "order ord-orphan is missing after the swap",
    );
}

// verifies: NFR-6, REQ-OPS-3
#[test]
fn a_refused_submit_applied_after_resume_fails_only_the_halt_cell() {
    fails_only(
        Flaw::AppliesRefusedSubmitLater,
        "halted-host-refuses-and-only-a-halted-host-exports",
        "came back as already recorded",
    );
}

// verifies: INV-7, NFR-5
#[test]
fn a_record_holding_another_input_after_a_stop_fails_only_the_stop_after_durable_cell() {
    fails_only(
        Flaw::RewritesTheInputAtAStop,
        "stop-after-durable",
        "holds another input",
    );
}

// verifies: NFR-6, REQ-AUD-1
#[test]
fn a_redone_import_that_loses_refusals_fails_only_the_import_stop_cell() {
    fails_only(
        Flaw::DropsRefusalsAfterAStoppedImport,
        "stop-during-import-is-refused-at-open",
        "the refusal log differs after the import was redone",
    );
}

// verifies: REQ-ORD-43, NFR-4
#[test]
fn the_probe_records_where_a_final_deposit_failed() {
    let states = probe::fold(&probe::order_inputs_with_a_failed_final_deposit("ord-4", 0));
    let last = states.last().unwrap().as_ref().unwrap();
    let part = &last.parts[0];
    assert_eq!(
        part.segments[2].state,
        rutent_contracts::state_machine::SegmentState::FailedReversalOwed
    );
    assert_eq!(
        part.deposit_failed_at.as_ref().map(|n| n.as_str()),
        Some("ledger")
    );
    assert!(
        states.iter().flatten().all(|o| o.check().is_ok()),
        "the probe returned an invalid order"
    );
}
