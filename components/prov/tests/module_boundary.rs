//! The module-boundary suite (`T-003-03`) run against the Wasm binding, once per provider category's stand-ins,
//! in Rust and C.

#[path = "module_support/mod.rs"]
mod common;

use rutent_conformance::boundary::{
    AdmissionRefusal, Admitted, Check, Exercised, Handled, ModuleBinding, PlatformRefusal,
    RunConfig, STEP_LIMIT, Step, run_boundary_suite,
};
use rutent_conformance::role::SuiteContext;
use rutent_conformance::signing::{advertisement, another_key};
use rutent_contracts::module_interface::PartName;

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

// verifies: REQ-MDL-1
#[test]
fn re_admitted_version_is_refused_with_its_own_reason() {
    let mut binding = common::binding("reserve");
    let report = run_boundary_suite(&mut binding, &RunConfig::default());
    for language in &report.languages {
        for probe in [
            Exercised::ReadmittedVersionRefused,
            Exercised::ReadmittedWaitingVersionRefused,
        ] {
            assert!(
                report.exercised.contains(&(language.clone(), probe)),
                "{language}: {probe:?} wasn't seen: {:#?}",
                report.findings
            );
        }
    }
    assert!(
        !report.flagged().contains(&Check::ReadmittedVersionRefused),
        "{:#?}",
        report.findings
    );
    // The reason itself, from the platform: a version admitted is named, whichever the binaries.
    let provider = "readmitted-provider-rs";
    let spec = common::spec(provider, "1.0.0", Vec::new(), true);
    let binaries = binding
        .build_with(&spec, "rs", "reserve", &[])
        .expect("the stand-in builds");
    let assertion = common::assertion(&spec.declaration, &binaries);
    assert!(matches!(
        binding.admit(&assertion, &binaries),
        Admitted::Artefact(_)
    ));
    binding.accept(provider, "1.0.0", "test-reviewer");
    assert_eq!(
        binding.admit(&assertion, &binaries),
        Admitted::Refused(AdmissionRefusal::VersionAlreadyAdmitted)
    );
    assert_eq!(binding.active_version(provider).as_deref(), Some("1.0.0"));
}

// verifies: REQ-MDL-11
#[test]
fn module_past_its_step_limit_is_refused() {
    let mut binding = common::binding("reserve");
    let config = RunConfig::default();
    let report = run_boundary_suite(&mut binding, &config);
    for language in &report.languages {
        for probe in [
            Exercised::StepLimitRefused,
            Exercised::StepsWithinTheLimitAnswered,
        ] {
            assert!(
                report.exercised.contains(&(language.clone(), probe)),
                "{language}: {probe:?} wasn't seen: {:#?}",
                report.findings
            );
        }
    }
    assert!(
        !report.flagged().contains(&Check::StepLimitEnforced),
        "{:#?}",
        report.findings
    );
    // The reason itself, from the platform: past the limit, a module that passed nothing out is refused as a
    // limit, and the same module within it answers.
    assert!(binding.set_step_limit(STEP_LIMIT));
    let step = |n: usize| -> Vec<Step> {
        (0..n)
            .map(|i| Step::ReadState {
                part: PartName::Sandboxed,
                key: format!("step.{i}"),
            })
            .collect()
    };
    for (provider, steps, answers) in [
        ("within-the-limit-rs", STEP_LIMIT, true),
        ("past-the-limit-rs", STEP_LIMIT + 1, false),
    ] {
        let spec = common::spec(provider, "1.0.0", step(steps), true);
        let installed = common::install(&mut binding, &spec, "rs", "reserve", &[]);
        assert!(matches!(installed, Admitted::Artefact(_)));
        let handled = binding.handle(provider, &common::withdrawal(provider, 1));
        if answers {
            assert!(matches!(handled, Handled::Answered(_)), "{handled:?}");
        } else {
            assert_eq!(handled, Handled::Refused(PlatformRefusal::LimitReached));
            assert_eq!(binding.active_version(provider).as_deref(), Some("1.0.0"));
        }
    }
    assert!(binding.set_step_limit(usize::MAX));
}
