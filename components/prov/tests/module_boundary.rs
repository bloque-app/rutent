//! The module-boundary suite (`T-003-03`) run against the Wasm binding, once per provider category's stand-ins,
//! in Rust and C.

#[path = "module_support/mod.rs"]
mod common;

use rutent_conformance::boundary::{
    AdmissionRefusal, Admitted, Exercised, ModuleBinding, RunConfig, run_boundary_suite,
};
use rutent_conformance::role::SuiteContext;
use rutent_conformance::signing::{advertisement, another_key};

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
    // Every release was judged by the catalogue, a forged one among them.
    for language in &report.languages {
        assert!(
            report
                .exercised
                .contains(&(language.clone(), Exercised::ForgedReleaseRefused)),
            "{category}: no forged release was refused in {language}"
        );
    }
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

// verifies: REQ-MDL-12, DEC-4
#[test]
fn boundary_suite_refuses_a_release_whose_assertion_is_signed_by_another_key() {
    let mut binding = common::binding("reserve");
    let provider = "forged-provider-rs";
    let spec = common::spec(provider, "1.0.0", Vec::new(), true);
    let binaries = binding
        .build_with(&spec, "rs", "reserve", &[])
        .expect("the stand-in builds");
    assert!(binding.list(&advertisement(provider, &SuiteContext::default())));
    // A key the provider's advertisement doesn't list signs the assertion: refused before any binary is judged.
    let forged = common::assertion_signed_by(&spec.declaration, &binaries, &another_key());
    assert_eq!(
        binding.admit(&forged, &binaries),
        Admitted::Refused(AdmissionRefusal::AssertionUnverified)
    );
    assert_eq!(binding.active_version(provider), None);
    // The same release, signed with the provider's advertising key, is admitted.
    assert!(matches!(
        binding.admit(&common::assertion(&spec.declaration, &binaries), &binaries),
        Admitted::Artefact(_)
    ));
}
