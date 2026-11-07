//! The module-boundary suite against its in-process reference double (`T-003-03`).

use rutent_conformance::boundary::reference::ReferenceDouble;
use std::collections::BTreeSet;

use rutent_conformance::boundary::{Check, Exercised, Report, RunConfig, run_boundary_suite};

fn run(mut binding: ReferenceDouble) -> Report {
    run_boundary_suite(&mut binding, &RunConfig::default())
}

fn exercised_in_every_language(report: &Report, answer: Exercised) -> bool {
    report.languages.len() >= 2
        && report
            .languages
            .iter()
            .all(|l| report.exercised.contains(&(l.clone(), answer)))
}

// verifies: REQ-MDL-9, AC-20
#[test]
fn hostile_module_cannot_read_another_providers_secrets() {
    let report = run(ReferenceDouble::new());
    for answer in [
        Exercised::ForeignSecretRefusedToOutside,
        Exercised::ForeignSecretRefusedToSandboxed,
        Exercised::ForeignStateRefusedToOutside,
        Exercised::ForeignStateRefusedToSandboxed,
    ] {
        assert!(
            exercised_in_every_language(&report, answer),
            "{answer:?} not exercised: {report:?}"
        );
    }
    assert!(!report.flagged().contains(&Check::OtherProvidersSecrets));
    let leaking = run(ReferenceDouble::new().answering_from_any_providers_secrets());
    assert_eq!(
        leaking.flagged(),
        [Check::OtherProvidersSecrets].into(),
        "a proxy answering from any provider's secrets: {:?}",
        leaking.findings
    );
}

// verifies: REQ-MDL-7
#[test]
fn redirect_to_an_undeclared_destination_is_refused() {
    let report = run(ReferenceDouble::new());
    assert!(exercised_in_every_language(
        &report,
        Exercised::UndeclaredRedirectRefused
    ));
    assert!(exercised_in_every_language(
        &report,
        Exercised::DeclaredRedirectFollowed
    ));
    let following = run(ReferenceDouble::new().following_any_redirect());
    assert_eq!(
        following.flagged(),
        [Check::RedirectChecked].into(),
        "a binding following any redirect: {:?}",
        following.findings
    );
}

// verifies: REQ-MDL-12
#[test]
fn oversized_output_is_refused() {
    let report = run(ReferenceDouble::new());
    assert!(exercised_in_every_language(
        &report,
        Exercised::OversizedOutputRefused
    ));
    assert!(exercised_in_every_language(
        &report,
        Exercised::OutputAtTheLimitPassed
    ));
    assert_eq!(report.output_limit, RunConfig::default().output_limit);
    for (name, binding) in [
        (
            "without a limit",
            ReferenceDouble::new().without_an_output_limit(),
        ),
        (
            "refusing at the limit",
            ReferenceDouble::new().refusing_output_at_the_limit(),
        ),
    ] {
        let broken = run(binding);
        assert_eq!(
            broken.flagged(),
            [Check::OutputSizeBounded].into(),
            "a binding {name}: {:?}",
            broken.findings
        );
    }
}

// verifies: AC-21, REQ-MDL-3
#[test]
fn widened_release_needs_fresh_acceptance() {
    let report = run(ReferenceDouble::new());
    assert!(exercised_in_every_language(
        &report,
        Exercised::WideningReleaseHeld
    ));
    assert!(exercised_in_every_language(
        &report,
        Exercised::NarrowingReleaseRan
    ));
    let automatic = run(ReferenceDouble::new().accepting_widening_releases_automatically());
    assert_eq!(
        automatic.flagged(),
        [Check::WideningNeedsFreshAcceptance].into(),
        "a binding accepting widening releases automatically: {:?}",
        automatic.findings
    );
}

// verifies: DEC-5, AC-19, AC-20, AC-21, REQ-MDL-13
#[test]
fn reference_double_passes_the_suite_in_two_languages() {
    let report = run(ReferenceDouble::new());
    assert!(
        report.passed(),
        "the reference double should pass: findings {:?}; uncovered {:?}",
        report.findings,
        report.uncovered
    );
    assert_eq!(report.format, "reference");
    assert_eq!(report.languages.len(), 2);
    assert_eq!(report.run_id, RunConfig::default().run_id);
    for language in &report.languages {
        for answer in Exercised::ALL {
            assert!(
                report.exercised.contains(&(language.clone(), answer)),
                "{answer:?} not exercised in {language}"
            );
        }
    }
}

// verifies: DEC-5, AC-19, AC-20, REQ-MDL-6, REQ-MDL-7, REQ-MDL-8, REQ-MDL-9, REQ-MDL-12, REQ-MDL-13
#[test]
fn every_broken_boundary_is_flagged_by_its_own_check_only() {
    let cases: Vec<(&str, ReferenceDouble, Check)> = vec![
        (
            "refusing every redirect",
            ReferenceDouble::new().refusing_every_redirect(),
            Check::DeclaredReachWorks,
        ),
        (
            "refusing declared secrets",
            ReferenceDouble::new().refusing_declared_secrets(),
            Check::DeclaredReachWorks,
        ),
        (
            "answering from any provider's secrets",
            ReferenceDouble::new().answering_from_any_providers_secrets(),
            Check::OtherProvidersSecrets,
        ),
        (
            "sharing state between providers",
            ReferenceDouble::new().sharing_state_between_providers(),
            Check::OtherProvidersState,
        ),
        (
            "letting the sandboxed part reach the proxy",
            ReferenceDouble::new().letting_the_sandboxed_part_reach_the_proxy(),
            Check::SandboxedPartHoldsNoSecret,
        ),
        (
            "connecting to undeclared destinations",
            ReferenceDouble::new().connecting_to_undeclared_destinations(),
            Check::UndeclaredDestinationRefused,
        ),
        (
            "following any redirect",
            ReferenceDouble::new().following_any_redirect(),
            Check::RedirectChecked,
        ),
        (
            "trusting resolution",
            ReferenceDouble::new().trusting_resolution(),
            Check::ConnectedAddressChecked,
        ),
        (
            "recording another module version",
            ReferenceDouble::new().recording_another_module_version(),
            Check::RecordsTellTheTruth,
        ),
        (
            "delivering undeclared calls",
            ReferenceDouble::new().delivering_undeclared_calls(),
            Check::UndeclaredCallRefused,
        ),
        (
            "misstating call refusals",
            ReferenceDouble::new().misstating_call_refusals(),
            Check::UndeclaredCallRefused,
        ),
        (
            "trusting the output's kind and transaction",
            ReferenceDouble::new().trusting_the_outputs_kind_and_transaction(),
            Check::OutputShapeChecked,
        ),
        (
            "without an output limit",
            ReferenceDouble::new().without_an_output_limit(),
            Check::OutputSizeBounded,
        ),
        (
            "refusing output at the limit",
            ReferenceDouble::new().refusing_output_at_the_limit(),
            Check::OutputSizeBounded,
        ),
        (
            "skipping the digest check",
            ReferenceDouble::new().skipping_the_digest_check(),
            Check::AdmissionRecordsWhatItAdmits,
        ),
        (
            "accepting widening releases automatically",
            ReferenceDouble::new().accepting_widening_releases_automatically(),
            Check::WideningNeedsFreshAcceptance,
        ),
        (
            "restarting every provider",
            ReferenceDouble::new().restarting_every_provider(),
            Check::ProvidersAreIsolated,
        ),
        (
            "dropping the previous version on a failed start",
            ReferenceDouble::new().dropping_the_previous_version_on_a_failed_start(),
            Check::ProvidersAreIsolated,
        ),
        (
            "running every provider in one process",
            ReferenceDouble::new().running_every_provider_in_one_process(),
            Check::ProvidersAreIsolated,
        ),
        (
            "holding non-starting releases as waiting",
            ReferenceDouble::new().holding_non_starting_releases_as_waiting(),
            Check::ProvidersAreIsolated,
        ),
        (
            "honouring the pending declaration in its proxy and calls",
            ReferenceDouble::new().honouring_the_pending_declaration_in_proxy_and_calls(),
            Check::WideningNeedsFreshAcceptance,
        ),
        (
            "refusing every cross-module call",
            ReferenceDouble::new().refusing_every_cross_module_call(),
            Check::DeclaredReachWorks,
        ),
        (
            "recording every declared secret",
            ReferenceDouble::new().recording_every_declared_secret(),
            Check::RecordsTellTheTruth,
        ),
        (
            "misstating lookup refusals",
            ReferenceDouble::new().misstating_lookup_refusals(),
            Check::OtherProvidersSecrets,
        ),
        (
            "resolving undeclared secrets",
            ReferenceDouble::new().resolving_undeclared_secrets(),
            Check::UndeclaredSecretRefused,
        ),
        (
            "sharing one egress allowlist",
            ReferenceDouble::new().sharing_one_egress_allowlist(),
            Check::UndeclaredDestinationRefused,
        ),
        (
            "checking only the first redirect",
            ReferenceDouble::new().checking_only_the_first_redirect(),
            Check::RedirectChecked,
        ),
        (
            "checking the address on the first hop only",
            ReferenceDouble::new().checking_the_address_on_the_first_hop_only(),
            Check::ConnectedAddressChecked,
        ),
        (
            "checking IPv4 addresses only",
            ReferenceDouble::new().checking_ipv4_addresses_only(),
            Check::ConnectedAddressChecked,
        ),
        (
            "recording no secrets",
            ReferenceDouble::new().recording_no_secrets(),
            Check::RecordsTellTheTruth,
        ),
        (
            "checking only the callee",
            ReferenceDouble::new().checking_only_the_callee(),
            Check::UndeclaredCallRefused,
        ),
        (
            "padding output short",
            ReferenceDouble::new().padding_output_short(),
            Check::OutputSizeBounded,
        ),
        (
            "checking only the outside digest",
            ReferenceDouble::new().checking_only_the_outside_digest(),
            Check::AdmissionRecordsWhatItAdmits,
        ),
        (
            "ignoring secret widenings",
            ReferenceDouble::new().ignoring_secret_widenings(),
            Check::WideningNeedsFreshAcceptance,
        ),
        (
            "ignoring callee widenings",
            ReferenceDouble::new().ignoring_callee_widenings(),
            Check::WideningNeedsFreshAcceptance,
        ),
        (
            "ignoring address widenings",
            ReferenceDouble::new().ignoring_address_widenings(),
            Check::WideningNeedsFreshAcceptance,
        ),
        (
            "enforcing the pending declaration",
            ReferenceDouble::new().enforcing_the_pending_declaration(),
            Check::WideningNeedsFreshAcceptance,
        ),
        (
            "keeping every declaration's reach",
            ReferenceDouble::new().keeping_every_declarations_reach(),
            Check::WideningNeedsFreshAcceptance,
        ),
    ];
    let mut flagged = BTreeSet::new();
    for (name, binding, check) in cases {
        let report = run(binding);
        assert_eq!(
            report.flagged(),
            [check].into(),
            "a binding {name} should be flagged by exactly {check:?}: {:?}",
            report.findings
        );
        assert!(!report.passed(), "a binding {name} must not pass");
        flagged.insert(check);
    }
    assert_eq!(flagged.len(), 15, "every check has a breaker");
}

// verifies: DEC-5, REQ-MDL-9
#[test]
fn modules_that_attempt_nothing_leave_the_suite_uncovered() {
    let report = run(ReferenceDouble::new().ignoring_the_modules_steps());
    assert!(
        !report.passed(),
        "a binding whose modules attempt nothing must not pass"
    );
    for answer in [
        Exercised::ForeignSecretRefusedToOutside,
        Exercised::ForeignSecretRefusedToSandboxed,
        Exercised::SandboxedLookupRefused,
        Exercised::UndeclaredDestinationRefused,
        Exercised::UndeclaredRedirectRefused,
        Exercised::AddressRefused,
        Exercised::UndeclaredCallRefused,
    ] {
        assert!(
            report.uncovered.iter().any(|u| u.answer == answer),
            "{answer:?} should be uncovered when nothing is attempted"
        );
    }
}

// verifies: AC-20
#[test]
fn a_binding_with_one_language_does_not_pass() {
    let report = run(ReferenceDouble::new().with_one_language());
    assert_eq!(report.missing_languages, 1);
    assert!(report.findings.is_empty(), "{:?}", report.findings);
    assert!(report.uncovered.is_empty(), "{:?}", report.uncovered);
    assert!(!report.passed(), "one language is short of AC-20's two");
}

// verifies: REQ-MDL-9
#[test]
fn findings_name_the_channel_never_the_secret() {
    let report = run(ReferenceDouble::new().answering_from_any_providers_secrets());
    assert!(!report.findings.is_empty());
    for finding in &report.findings {
        assert!(
            !finding.detail.contains("canary-"),
            "a finding carries a secret's value: {}",
            finding.detail
        );
    }
}

// verifies: AC-19, REQ-MDL-13
#[test]
fn a_binding_that_never_fails_an_upgrade_leaves_it_uncovered() {
    let report = run(ReferenceDouble::new().refusing_releases_that_cannot_start());
    assert!(report.findings.is_empty(), "{:?}", report.findings);
    assert!(
        report
            .uncovered
            .iter()
            .any(|u| u.answer == Exercised::FailedUpgradeKeptThePrevious),
        "a failed upgrade refused at admission must leave its fixture uncovered"
    );
    assert!(!report.passed());
}

// verifies: REQ-MDL-7
#[test]
fn each_egress_probe_catches_its_own_breaker() {
    for (binding, labels) in [
        (
            ReferenceDouble::new().checking_ipv4_addresses_only(),
            vec!["ipv6-loopback", "ipv6-unique-local", "ipv4-mapped"],
        ),
        (
            ReferenceDouble::new().checking_only_the_first_redirect(),
            vec!["redirect-chain"],
        ),
        (
            ReferenceDouble::new().checking_the_address_on_the_first_hop_only(),
            vec!["redirect-to-private"],
        ),
    ] {
        let report = run(binding);
        for label in labels {
            assert!(
                report
                    .findings
                    .iter()
                    .any(|f| f.detail.ends_with(&format!("in {label}"))),
                "the {label} probe should catch its breaker: {:?}",
                report.findings
            );
        }
    }
}

// verifies: DEC-5
#[test]
fn every_fixture_and_control_is_listed_once() {
    for (position, answer) in Exercised::ALL.iter().enumerate() {
        assert_eq!(
            answer.ordinal(),
            position,
            "{answer:?} is out of place in Exercised::ALL"
        );
    }
}
