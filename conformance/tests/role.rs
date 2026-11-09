//! The role conformance suite against one in-memory stand-in per category (`T-002-03`).

use std::collections::BTreeSet;

use rutent_conformance::role::stand_in::{Exchange, Ramp, Reserve, Verifier};
use rutent_conformance::role::{
    Check, DeferredVerifier, Exercised, Report, RoleHarness, SignatureVerifier, SuiteContext,
    Verification, WithoutHooks, run_role_suite,
};
use rutent_contracts::provider_role::Signature;

fn run(provider: &mut dyn RoleHarness) -> Report {
    run_role_suite(provider, &SuiteContext::default(), &DeferredVerifier)
}

/// Signatures are deferred until `M2`, so a stand-in passes but for that one check, and only a verifier that
/// concludes lets it pass whole.
fn assert_passes(name: &str, provider: &dyn Fn() -> Box<dyn RoleHarness>) {
    let report = run(provider().as_mut());
    assert!(
        report.passed_but_for_deferred(),
        "{name} should pass its role suite: findings {:?}; uncovered {:?}",
        report.findings,
        report.uncovered
    );
    assert!(
        report.signatures_deferred > 0,
        "{name}: no signature was handed to the verifier"
    );
    assert_eq!(
        report.deferred,
        BTreeSet::from([Check::SignatureVerifies]),
        "{name}: only signatures are deferred"
    );
    assert!(!report.passed(), "{name}: a deferred check must not pass");
    let concluded = run_role_suite(provider().as_mut(), &SuiteContext::default(), &Accepting);
    assert!(
        concluded.passed(),
        "{name} should pass whole once signatures verify: {:?}",
        concluded.findings
    );
}

// verifies: REQ-CAT-8, REQ-MOD-4
#[test]
fn stand_in_of_each_category_passes_its_role_suite() {
    let context = SuiteContext::default();
    assert_passes("the reserve", &|| Box::new(Reserve::new(&context)));
    assert_passes("the exchange", &|| Box::new(Exchange::new(&context)));
    assert_passes("the ramp", &|| Box::new(Ramp::new(&context)));
    assert_passes("the verifier", &|| Box::new(Verifier::new(&context)));
}

// verifies: REQ-CAT-18
#[test]
fn refusal_lifting_at_a_known_time_is_accepted_where_allowed() {
    let context = SuiteContext::default();
    let report = run(&mut Exchange::new(&context));
    assert!(
        report
            .exercised
            .contains(&Exercised::RefusalLiftingAtKnownTime),
        "the exchange's lifting refusal of a settled reversal was not seen"
    );
    assert!(report.findings.is_empty(), "{:?}", report.findings);
}

// verifies: REQ-CAT-8, ADR-0008
#[test]
fn provider_without_hooks_breaks_nothing_but_is_uncovered() {
    let context = SuiteContext::default();
    let report = run(&mut WithoutHooks(Reserve::new(&context)));
    assert!(report.findings.is_empty(), "{:?}", report.findings);
    assert!(!report.hooks_used, "no hook took effect without hooks");
    assert!(
        run(&mut Reserve::new(&context)).hooks_used,
        "the reserve's hooks took effect when present"
    );
    let uncovered: BTreeSet<Exercised> = report.uncovered.iter().map(|u| u.answer).collect();
    assert_eq!(
        uncovered,
        BTreeSet::from([
            Exercised::OutcomeUnknown,
            Exercised::StatusQuery,
            Exercised::ReversalConfirmed
        ]),
        "a reserve run without hooks leaves only the provoked answers uncovered"
    );
}

// verifies: REQ-CAT-8, REQ-MOD-4
#[test]
fn certification_takes_a_run_without_hooks_and_a_hooked_one() {
    let context = SuiteContext::default();
    let certify = |provider: &mut dyn RoleHarness| run_role_suite(provider, &context, &Accepting);
    for (name, without, hooked) in [
        (
            "the reserve",
            certify(&mut WithoutHooks(Reserve::new(&context))),
            certify(&mut Reserve::new(&context)),
        ),
        (
            "the exchange",
            certify(&mut WithoutHooks(Exchange::new(&context))),
            certify(&mut Exchange::new(&context)),
        ),
        (
            "the ramp",
            certify(&mut WithoutHooks(Ramp::new(&context))),
            certify(&mut Ramp::new(&context)),
        ),
        (
            "the verifier",
            certify(&mut WithoutHooks(Verifier::new(&context))),
            certify(&mut Verifier::new(&context)),
        ),
    ] {
        assert!(
            without.passed_without_hooks(),
            "{name} without hooks: findings {:?}; uncovered {:?}",
            without.findings,
            without.uncovered
        );
        assert!(hooked.passed(), "{name} with hooks: {:?}", hooked.findings);
    }
    // A hooked run is never the first report, even when it passes.
    assert!(!certify(&mut Reserve::new(&context)).passed_without_hooks());
    // Nor is a run with a deferred check.
    assert!(!run(&mut WithoutHooks(Reserve::new(&context))).passed_without_hooks());
    // A provider that refuses everything leaves uncovered what needs no hook.
    let silent = certify(&mut WithoutHooks(
        Exchange::new(&context).refusing_everything(),
    ));
    assert!(!silent.passed_without_hooks(), "{:?}", silent.uncovered);
    // A broken rule fails it too.
    let broken = certify(&mut WithoutHooks(
        Ramp::new(&context).ignoring_missing_inputs(),
    ));
    assert!(!broken.passed_without_hooks());
}

// verifies: REQ-CAT-8, REQ-CAT-14
#[test]
fn stand_in_that_refuses_everything_fails_on_coverage_alone() {
    let context = SuiteContext::default();
    let report = run(&mut Exchange::new(&context).refusing_everything());
    assert!(
        report.findings.is_empty(),
        "a provider that only refuses breaks no rule: {:?}",
        report.findings
    );
    assert!(
        !report.passed() && !report.uncovered.is_empty(),
        "a provider that only refuses must not pass"
    );
}

/// A verifier that accepts every signature, standing in for one that concludes.
struct Accepting;

impl SignatureVerifier for Accepting {
    fn verify(&self, _maker: &str, _signature: &Signature) -> Verification {
        Verification::Verified
    }
}

/// A verifier that rejects every signature.
struct Rejecting;

impl SignatureVerifier for Rejecting {
    fn verify(&self, _maker: &str, _signature: &Signature) -> Verification {
        Verification::Failed("rejected".to_owned())
    }
}

// verifies: REQ-CAT-21
#[test]
fn stand_in_that_breaks_its_role_is_flagged() {
    let context = SuiteContext::default();
    let cases: Vec<(&str, Box<dyn RoleHarness>, Check)> = vec![
        (
            "an exchange failing after moving",
            Box::new(Exchange::new(&context).failing_after_moving()),
            Check::ExecutionReported,
        ),
        (
            "a ramp refusing to undo failures",
            Box::new(Ramp::new(&context).refusing_to_undo_failures()),
            Check::ReversalIsTrueUndo,
        ),
        (
            "a reserve confirming without the segment",
            Box::new(Reserve::new(&context).confirming_without_the_segment()),
            Check::ReversalConfirmationMatches,
        ),
        (
            "an exchange ignoring the operation envelope",
            Box::new(Exchange::new(&context).ignoring_the_operation_envelope()),
            Check::RefusesForeignOperations,
        ),
        (
            "a ramp checking the envelope only on dry runs",
            Box::new(Ramp::new(&context).checking_the_envelope_only_on_dry_runs()),
            Check::RefusesForeignOperations,
        ),
        (
            "a reserve skipping the envelope on confirmations",
            Box::new(Reserve::new(&context).skipping_the_envelope_on_confirmations()),
            Check::RefusesForeignOperations,
        ),
        (
            "a ramp checking its repeats before the envelope",
            Box::new(Ramp::new(&context).checking_its_repeats_before_the_envelope()),
            Check::RefusesForeignOperations,
        ),
        (
            "a ramp comparing only the major version",
            Box::new(Ramp::new(&context).comparing_only_the_major_version()),
            Check::RefusesForeignOperations,
        ),
        (
            "a ramp checking the name and its own operator key",
            Box::new(Ramp::new(&context).checking_the_name_and_its_own_operator_key()),
            Check::RefusesForeignOperations,
        ),
        (
            "an exchange failing only once",
            Box::new(Exchange::new(&context).failing_only_once()),
            Check::ExecutionReported,
        ),
        (
            "a verifier checking the envelope on its first class only",
            Box::new(Verifier::new(&context).checking_the_envelope_on_its_first_class_only()),
            Check::RefusesForeignOperations,
        ),
        (
            "a ramp paying less through its status",
            Box::new(Ramp::new(&context).paying_less_through_its_status()),
            Check::QuoteHonoured,
        ),
        (
            "a reserve whose declaration names another maker",
            Box::new(Reserve::new(&context).with_a_declaration_of_another_maker()),
            Check::AdvertisementValid,
        ),
        (
            "a verifier dating its judgement badly",
            Box::new(Verifier::new(&context).dating_its_judgement_badly()),
            Check::EnvelopeWellFormed,
        ),
        (
            "an exchange signing with an unpublished key",
            Box::new(Exchange::new(&context).signing_with_an_unpublished_key()),
            Check::MakerAndKey,
        ),
        (
            "an exchange quoting another amount",
            Box::new(Exchange::new(&context).quoting_another_amount()),
            Check::AnswersItsOperation,
        ),
        (
            "an exchange charging its fee in another asset",
            Box::new(Exchange::new(&context).charging_its_fee_in_another_asset()),
            Check::AnswersItsOperation,
        ),
        (
            "a ramp taking its fee on top of the amount",
            Box::new(Ramp::new(&context).taking_its_fee_on_top_of_the_amount()),
            Check::AnswersItsOperation,
        ),
        (
            "an exchange answering dry runs with outcome unknown",
            Box::new(Exchange::new(&context).answering_dry_runs_with_outcome_unknown()),
            Check::DryRunAnswered,
        ),
        (
            "an exchange whose dry run moves",
            Box::new(Exchange::new(&context).with_a_dry_run_that_moves()),
            Check::DryRunHasNoEffect,
        ),
        (
            "an exchange succeeding without moving",
            Box::new(Exchange::new(&context).succeeding_without_moving()),
            Check::ExecutionReported,
        ),
        (
            "an exchange paying less than it quoted",
            Box::new(Exchange::new(&context).paying_less_than_it_quoted()),
            Check::QuoteHonoured,
        ),
        (
            "a ramp moving again on a repeat",
            Box::new(Ramp::new(&context).moving_again_on_a_repeat()),
            Check::SafeRetry,
        ),
        (
            "a ramp serving outside its tuples and bands",
            Box::new(Ramp::new(&context).serving_outside_its_tuples_and_bands()),
            Check::StaysWithinTuplesAndBands,
        ),
        (
            "a ramp ignoring missing inputs",
            Box::new(Ramp::new(&context).ignoring_missing_inputs()),
            Check::RequiresDeclaredInputs,
        ),
        (
            "a reserve that moves value outside its role",
            Box::new(Reserve::new(&context).moving_value_outside_its_role()),
            Check::StaysWithinRole,
        ),
        (
            "an exchange refusing with a lifting time",
            Box::new(Exchange::new(&context).refusing_with_a_lifting_time()),
            Check::RefusalClass,
        ),
        (
            "a ramp leaving its status unknown",
            Box::new(Ramp::new(&context).leaving_its_status_unknown()),
            Check::OutcomeUnknownDistinct,
        ),
        (
            "a ramp with weaker evidence",
            Box::new(Ramp::new(&context).with_weaker_evidence()),
            Check::EvidenceAtDeclaredGrade,
        ),
        (
            "a reserve ignoring the session",
            Box::new(Reserve::new(&context).ignoring_the_session()),
            Check::BalanceRevealsOnlySufficiency,
        ),
        (
            "a verifier judging another class",
            Box::new(Verifier::new(&context).judging_another_class()),
            Check::JudgementMatchesQuestion,
        ),
        (
            "a ramp reversing without undoing",
            Box::new(Ramp::new(&context).reversing_without_undoing()),
            Check::ReversalIsTrueUndo,
        ),
        (
            "a reserve confirming any credit",
            Box::new(Reserve::new(&context).confirming_any_credit()),
            Check::ReversalConfirmationMatches,
        ),
    ];
    for (name, mut provider, check) in cases {
        let report = run(provider.as_mut());
        assert_eq!(
            report.flagged(),
            BTreeSet::from([check]),
            "{name} should be flagged by exactly {check:?}: {:?}",
            report.findings
        );
    }
    let report = run_role_suite(&mut Exchange::new(&context), &context, &Rejecting);
    assert_eq!(
        report.flagged(),
        BTreeSet::from([Check::SignatureVerifies]),
        "a rejected signature should be flagged by exactly SignatureVerifies: {:?}",
        report.findings
    );
}

// verifies: THR-108, REQ-QTE-5, REQ-CAT-11
#[test]
fn a_fee_on_top_of_the_amount_or_in_another_asset_is_flagged_on_the_quote_and_on_the_execution() {
    let context = SuiteContext::default();
    let cases: Vec<(&str, Box<dyn RoleHarness>, &str)> = vec![
        (
            "a ramp taking its fee on top of the amount",
            Box::new(Ramp::new(&context).taking_its_fee_on_top_of_the_amount()),
            "TookInMoreThanStated",
        ),
        (
            "an exchange charging its fee in another asset",
            Box::new(Exchange::new(&context).charging_its_fee_in_another_asset()),
            "FeeInAnotherAsset",
        ),
    ];
    for (name, mut provider, rule) in cases {
        let report = run(provider.as_mut());
        for answer in [", dry run: ", ", execution: "] {
            assert!(
                report
                    .findings
                    .iter()
                    .any(|f| f.check == Check::AnswersItsOperation
                        && f.detail.contains(answer)
                        && f.detail.ends_with(rule)),
                "{name}: no {rule} finding on the{answer}{:?}",
                report.findings
            );
        }
    }
}

// verifies: REQ-CAT-21, AC-30
#[test]
fn stand_in_with_internal_steps_passes() {
    let context = SuiteContext::default();
    assert_passes("the ramp with internal steps", &|| {
        Box::new(Ramp::new(&context).with_internal_steps())
    });
    let mut ramp = Ramp::new(&context).with_internal_steps();
    run(&mut ramp);
    assert!(
        !ramp.internal_steps().is_empty(),
        "the ramp took no internal steps"
    );
}
