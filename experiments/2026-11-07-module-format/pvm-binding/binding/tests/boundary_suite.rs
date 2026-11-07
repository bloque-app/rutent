//! The module-boundary suite (`T-003-03`) on the PolkaVM binding: every category's stand-ins, in Rust and in C.
//! It needs the stand-ins `build.sh` makes, so it is ignored by default; `run.sh` runs it with
//! `RUTENT_PVM_GUESTS` set, before the suite's full report.

use std::path::PathBuf;

use rutent_conformance::boundary::{RunConfig, run_boundary_suite};
use rutent_pvm_binding::binding::{CATEGORIES, PvmBinding, Setup};

fn setup() -> Setup {
    Setup {
        worker: PathBuf::from(env!("CARGO_BIN_EXE_rutent-pvm-worker")),
        admit: PathBuf::from(env!("CARGO_BIN_EXE_rutent-pvm-admit")),
        guests: PathBuf::from(
            std::env::var("RUTENT_PVM_GUESTS")
                .expect("RUTENT_PVM_GUESTS: the stand-ins build.sh makes"),
        ),
    }
}

// verifies: AC-20, REQ-MDL-15, REQ-MDL-18, REQ-MDL-13, REQ-MDL-9
#[test]
#[ignore = "needs the stand-ins build.sh makes; run.sh runs it with RUTENT_PVM_GUESTS set"]
fn every_categorys_stand_ins_pass_the_module_boundary_suite_in_rust_and_in_c() {
    for category in CATEGORIES {
        let mut binding = PvmBinding::new(setup(), category);
        let report = run_boundary_suite(&mut binding, &RunConfig::default());
        assert_eq!(report.languages, ["rs", "c"], "{category}");
        assert!(
            report.passed(),
            "{category}: findings {:?}, uncovered {:?}",
            report.findings,
            report.uncovered
        );
    }
}
