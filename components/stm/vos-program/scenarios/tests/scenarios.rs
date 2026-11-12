//! `T-024-02`'s four checks on three VOS replicas: the reference host's run (`conformance/scenarios/tests/assembly.rs`)
//! with the state machine on VOS. The swap moves it out of VOS to the reference host, mid-order, and loses nothing.
//! These need `../build.sh`'s binaries and `../test.sh`'s paths; without the `vos` feature this file is empty.

#![cfg(feature = "vos")]

use rutent_scenarios::assembly::machine::Environment;
use rutent_scenarios::checks;
use rutent_scenarios::harness::reference;
use rutent_scenarios_vos::{Base, Vos};

fn reference_target() -> Box<dyn Environment> {
    reference()
}

// verifies: NFR-5, INV-7, DEC-26
#[test]
fn crash_between_record_and_effect_leaves_no_effect_in_the_assembled_core_on_three_vos_replicas() {
    let base = Base::new("crash");
    checks::crash_between_record_and_effect(&mut Vos::three(&base, "c"), &reference_target);
}

// verifies: NFR-6, REQ-OPS-3, DEC-26
#[test]
fn execution_environment_swapped_mid_order_out_of_three_vos_replicas_loses_no_order() {
    let base = Base::new("swap");
    checks::swapped_mid_order(&mut Vos::three(&base, "s"), &reference_target);
}

// verifies: NFR-4, DEC-26
#[test]
fn replaying_a_scenarios_recorded_inputs_reaches_the_same_states_on_three_vos_replicas() {
    let base = Base::new("replay");
    checks::replaying_recorded_inputs(&mut Vos::three(&base, "r"), &reference_target);
}

// verifies: DEC-18, DEC-26
#[test]
fn scenario_run_twice_with_one_seed_gives_one_run_on_three_vos_replicas() {
    let base = Base::new("seed");
    checks::one_seed_one_run(&mut Vos::three(&base, "d"), &reference_target);
}
