//! The assembled Core, on the reference host (`T-024-02`): a crash between a record and its effect, the execution
//! environment swapped mid-order, the recorded inputs replayed, and one seed's two runs, run on three VOS replicas
//! too in `rutent-scenarios-vos`; and a Core opened again on its disk keeping what its grading took (`T-020-09`).

use rutent_scenarios::assembly::machine::Reference;
use rutent_scenarios::checks;
use rutent_scenarios::harness::reference;

// verifies: NFR-5, INV-7
#[test]
fn crash_between_record_and_effect_leaves_no_effect_in_the_assembled_core() {
    checks::crash_between_record_and_effect(&mut Reference, &reference);
}

// verifies: NFR-6, REQ-OPS-3
#[test]
fn execution_environment_swapped_mid_order_loses_no_order() {
    checks::swapped_mid_order(&mut Reference, &reference);
}

// verifies: NFR-4
#[test]
fn replaying_a_scenarios_recorded_inputs_reaches_the_same_states() {
    checks::replaying_recorded_inputs(&mut Reference, &reference);
}

// verifies: DEC-18
#[test]
fn scenario_run_twice_with_one_seed_gives_one_run() {
    checks::one_seed_one_run(&mut Reference, &reference);
}

// verifies: REQ-ORD-38, REQ-ORD-33
#[test]
fn reopened_core_keeps_what_its_grading_took() {
    checks::restart_keeps_what_closing_took(&mut Reference, &reference);
}
