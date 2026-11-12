//! `T-024-02`'s four checks on three VOS replicas: the reference host's run (`conformance/scenarios/tests/assembly.rs`)
//! with the state machine on VOS. The swap moves it from one cluster of three to another, mid-order, so the import
//! into VOS runs; the other checks' planted swaps move it out to the reference host.
//! These need `../build.sh`'s binaries and `../test.sh`'s paths; without the `vos` feature this file is empty.

#![cfg(feature = "vos")]

use rutent_scenarios::assembly::machine::Environment;
use rutent_scenarios::checks;
use rutent_scenarios::harness::reference;
use rutent_scenarios_vos::{Base, Vos};

fn reference_target() -> Box<dyn Environment> {
    reference()
}

// verifies: NFR-5, INV-7
#[test]
fn crash_between_record_and_effect_leaves_no_effect_in_the_assembled_core_on_three_vos_replicas() {
    let base = Base::new("crash");
    checks::crash_between_record_and_effect(&mut Vos::three(&base, "c"), &reference_target);
}

// verifies: NFR-6, REQ-OPS-3
#[test]
fn execution_environment_swapped_mid_order_between_two_clusters_of_three_vos_replicas_loses_no_order()
 {
    let base = Base::new("swap");
    let into = Base::new("swap-into");
    let into_vos = || -> Box<dyn Environment> { Box::new(Vos::three(&into, "i")) };
    checks::swapped_mid_order(&mut Vos::three(&base, "s"), &into_vos);
}

// verifies: NFR-4
#[test]
fn replaying_a_scenarios_recorded_inputs_reaches_the_same_states_on_three_vos_replicas() {
    let base = Base::new("replay");
    checks::replaying_recorded_inputs(&mut Vos::three(&base, "r"), &reference_target);
}

// verifies: DEC-18
#[test]
fn scenario_run_twice_with_one_seed_gives_one_run_on_three_vos_replicas() {
    let base = Base::new("seed");
    checks::one_seed_one_run(&mut Vos::three(&base, "d"), &reference_target);
}
