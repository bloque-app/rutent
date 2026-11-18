//! A second operation under a transaction the platform already carried out (`T-013-09`; `INV-8`, `REQ-CAT-10`,
//! `NFR-5`): refused for a provider whose advertisement declares no safe retry, with no module run and nothing
//! moved, and answered from the platform's record for one that declares it. The providers are the catalogue's
//! reserve (`safe_retry` false) and ramp (`safe_retry` true), both running the boundary suite's stand-in.

#[path = "module_support/mod.rs"]
mod common;
#[path = "module_support/signed.rs"]
mod signed;
#[path = "holding_support/mod.rs"]
mod support;

use common::withdrawal;
use rutent_contracts::module_interface::{Identifier, PlatformRefusal};
use rutent_prov::catalogue::{Category, Grant, GrantTerms, Instant};
use rutent_prov::module::types::{Admitted, Handled};
use signed::{AT, RAMP, RESERVE, Signed, release};

/// Admits `provider`'s first release, handling withdrawals, and gives it its acceptance: it runs.
fn admit(s: &mut Signed, provider: &str) {
    let key = signed::catalogue_support::advertising_key(provider);
    let (declaration, binaries) = release(provider, "1.0.0", &["withdrawal"]);
    let Admitted::Artefact(artefact) =
        s.admit(&signed::assertion(&declaration, &binaries, &key), &binaries)
    else {
        panic!("the release wasn't admitted");
    };
    if artefact.acceptance.accepted.is_none() {
        s.accept(provider, &artefact.assertion_digest, "test-reviewer")
            .expect("the release is accepted");
    }
    assert_eq!(
        s.platform.active_version(provider).as_deref(),
        Some("1.0.0")
    );
}

/// A platform with `provider`'s first release in force.
fn running(provider: &str) -> Signed {
    let mut s = Signed::new();
    admit(&mut s, provider);
    s
}

fn tx(provider: &str, n: usize) -> Identifier {
    Identifier::new(format!("tx-{provider}-{n}")).expect("an identifier")
}

// verifies: INV-8, REQ-CAT-10
#[test]
fn second_operation_under_a_carried_out_transaction_is_refused_for_a_provider_without_safe_retry() {
    let mut s = running(RESERVE);
    // Held in a segment, as Core holds it, and not.
    s.platform
        .begin_segment(RESERVE, &tx(RESERVE, 1))
        .expect("a segment");
    for n in [1, 2] {
        let first = s.platform.handle(RESERVE, &withdrawal(RESERVE, n));
        assert!(matches!(first, Handled::Answered(_)), "{first:?}");
        assert_eq!(
            s.platform.operations_run(RESERVE),
            if n == 1 { 1 } else { 2 }
        );
        let before = s.platform.operations_run(RESERVE);

        let repeat = s.platform.handle(RESERVE, &withdrawal(RESERVE, n));
        assert_eq!(
            repeat,
            Handled::Refused(PlatformRefusal::DuplicateTransaction)
        );
        assert_eq!(
            s.platform.operations_run(RESERVE),
            before,
            "the module ran the repeat of transaction {n}"
        );
    }
    // The refusal is of the repeat only: another transaction is carried out.
    let other = s.platform.handle(RESERVE, &withdrawal(RESERVE, 3));
    assert!(matches!(other, Handled::Answered(_)), "{other:?}");
    // A dry run moves nothing, so it is no repeat: asked twice, it is answered twice.
    let mut dry = serde_json::to_value(withdrawal(RESERVE, 4)).expect("JSON");
    dry["statement"]["dry_run"] = true.into();
    let dry = serde_json::from_value(dry).expect("an operation");
    for _ in 0..2 {
        let answered = s.platform.handle(RESERVE, &dry);
        assert!(matches!(answered, Handled::Answered(_)), "{answered:?}");
    }
    // Once its segment ends the transaction is spent, as it was.
    s.platform
        .end_segment(tx(RESERVE, 1).as_str())
        .expect("its record");
    assert_eq!(
        s.platform.handle(RESERVE, &withdrawal(RESERVE, 1)),
        Handled::Refused(PlatformRefusal::DuplicateTransaction)
    );
}

// verifies: INV-8
#[test]
fn second_operation_under_a_carried_out_transaction_is_answered_from_the_record_for_a_provider_with_safe_retry()
 {
    let mut s = running(RAMP);
    s.platform
        .begin_segment(RAMP, &tx(RAMP, 1))
        .expect("a segment");
    for n in [1, 2] {
        let first = s.platform.handle(RAMP, &withdrawal(RAMP, n));
        assert!(matches!(first, Handled::Answered(_)), "{first:?}");
        let before = s.platform.operations_run(RAMP);

        // The first answer, byte for byte, and not a second run of the module.
        let repeat = s.platform.handle(RAMP, &withdrawal(RAMP, n));
        assert_eq!(repeat, first);
        assert_eq!(
            s.platform.operations_run(RAMP),
            before,
            "the module ran the repeat of transaction {n}"
        );
    }
}

// verifies: INV-8, REQ-CAT-10
#[test]
fn a_repeat_where_the_platform_cannot_tell_whether_a_repeat_is_safe_is_refused() {
    // A provider whose advertisement declares nothing of safe retry (a verifier's): the platform can't tell, and
    // a repeat is refused as it is for one that declares none.
    let provider = "id-check";
    let mut s = Signed::new();
    let ad = signed::catalogue_support::advertisement(
        "advertisement-verifier-without-evidence-or-retry-declarations",
    );
    s.catalogue
        .grant(
            Grant::new(GrantTerms {
                id: "g-verifier".into(),
                provider: provider.into(),
                party: "Acme Payments Ltd".into(),
                granted_by: "operator-admin".into(),
                granted_at: Instant(1_000),
                expires_at: Instant(4_000_000_000),
                categories: vec![Category::Verifier],
                tuples: Vec::new(),
                reversal_capacity: signed::catalogue_support::capacity(1_000, 500),
            })
            .expect("a grant"),
        )
        .expect("the grant is held");
    signed::catalogue_support::join_made_at(&mut s.catalogue, &ad, Instant(AT.0 - 86_400), AT)
        .expect("the advertisement joins");
    assert_eq!(
        s.catalogue.declared_by(provider).map(|d| d.safe_retry),
        Some(None)
    );
    admit(&mut s, provider);

    let first = s.platform.handle(provider, &withdrawal(provider, 1));
    assert!(matches!(first, Handled::Answered(_)), "{first:?}");
    assert_eq!(
        s.platform.handle(provider, &withdrawal(provider, 1)),
        Handled::Refused(PlatformRefusal::DuplicateTransaction)
    );
    assert_eq!(s.platform.operations_run(provider), 1);
}

// verifies: INV-8
#[test]
fn operation_under_another_providers_carried_out_transaction_is_refused_even_where_it_declares_safe_retry()
 {
    let mut s = running(RESERVE);
    admit(&mut s, RAMP);
    let first = s.platform.handle(RESERVE, &withdrawal(RESERVE, 1));
    assert!(matches!(first, Handled::Answered(_)), "{first:?}");

    // The ramp, which declares a repeat safe, is asked the reserve's transaction: not a repeat of its own.
    let mut elsewhere = serde_json::to_value(withdrawal(RAMP, 1)).expect("JSON");
    elsewhere["statement"]["transaction"] = tx(RESERVE, 1).as_str().into();
    let elsewhere = serde_json::from_value(elsewhere).expect("an operation");
    assert_eq!(
        s.platform.handle(RAMP, &elsewhere),
        Handled::Refused(PlatformRefusal::DuplicateTransaction)
    );
    assert_eq!(s.platform.operations_run(RAMP), 0);
}

// verifies: INV-8, REQ-CAT-10
#[test]
fn a_provider_that_withdraws_its_safe_retry_declaration_is_refused_from_its_next_release_on() {
    let mut s = running(RAMP);
    let first = s.platform.handle(RAMP, &withdrawal(RAMP, 1));
    assert_eq!(s.platform.handle(RAMP, &withdrawal(RAMP, 1)), first);

    // The provider advertises again, declaring a repeat unsafe, and releases again.
    let mut ad = s
        .catalogue
        .accepted_advertisement(RAMP)
        .expect("an accepted advertisement")
        .statement
        .clone();
    ad.declarations.safe_retry = Some(false);
    let key = signed::catalogue_support::advertising_key(RAMP);
    let bytes = signed::catalogue_support::advertisement_bytes(
        &ad,
        None,
        Instant(AT.0 - 86_400 + 10),
        &key,
    );
    s.catalogue.join(&bytes, AT).expect("the successor joins");
    assert_eq!(
        s.catalogue.declared_by(RAMP).map(|d| d.safe_retry),
        Some(Some(false))
    );
    let (declaration, binaries) = release(RAMP, "1.0.1", &["withdrawal"]);
    let admitted = s.admit(&signed::assertion(&declaration, &binaries, &key), &binaries);
    assert!(signed::is_artefact(&admitted), "{admitted:?}");

    // What the platform reads is the declaration now in force: the first answer is no longer repeated.
    assert_eq!(
        s.platform.handle(RAMP, &withdrawal(RAMP, 1)),
        Handled::Refused(PlatformRefusal::DuplicateTransaction)
    );
}
