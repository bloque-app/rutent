//! Admission of a signed release (`T-013-04`; `REQ-MDL-1`, `REQ-MDL-3`, `AC-21`, `THR-4`): the assertion verified
//! with the catalogue's verifier against the advertisement the catalogue accepted, the acceptance bound to the
//! assertion's digest, no new admission while one waits, and a widened release waiting for a fresh acceptance.

#[path = "module_support/mod.rs"]
mod common;
#[path = "module_support/signed.rs"]
mod signed;

use rutent_contracts::module_interface::{
    AcceptanceMode, AdmissionRefusal, RuleViolation, validate_release,
};
use rutent_contracts::provider_role::ContextMismatch;
use rutent_prov::catalogue::{SignatureError, StatementRefusal};
use rutent_prov::module::admission::Unverified;
use rutent_prov::module::types::Admitted;
use rutent_prov::module::wire::sha256;
use signed::{RAMP, RESERVE, Signed, release};

// verifies: REQ-MDL-1, THR-4
#[test]
fn release_with_an_unverified_assertion_is_refused() {
    let mut s = Signed::new();
    let (declaration, binaries) = release(RESERVE, "1.0.0", &["withdrawal"]);
    let advertising = signed::catalogue_support::advertising_key(RESERVE);
    let statements = signed::catalogue_support::statements_key(RESERVE);

    // The signature doesn't verify: a byte of what was signed changed after signing.
    let mut altered = signed::assertion(&declaration, &binaries, &advertising);
    let at = signed::find(&altered, b"\"version\":\"1.0.0\"").expect("the version is in the bytes");
    altered[at + 11] = b'2';
    // Signed with the provider's statements key, which the outside part can resolve.
    let by_statements_key = signed::assertion(&declaration, &binaries, &statements);
    // Of another deployment than the platform's.
    let other_deployment =
        signed::assertion_in(&declaration, &binaries, &advertising, "another-deployment");
    // Made by a provider the catalogue holds no advertisement of.
    let (stranger, stranger_binaries) = release("stranger", "1.0.0", &["withdrawal"]);
    let unknown = signed::assertion(
        &stranger,
        &stranger_binaries,
        &signed::catalogue_support::advertising_key("stranger"),
    );
    // Made and signed by another provider, for this one's module.
    let by_another = signed::assertion_by(
        &declaration,
        &binaries,
        RAMP,
        &signed::catalogue_support::advertising_key(RAMP),
    );

    // Unsigned: the signature's value is no signature, or there is no signature at all.
    let mut value: serde_json::Value =
        serde_json::from_slice(&signed::assertion(&declaration, &binaries, &advertising))
            .expect("JSON");
    value["signature"]["value"] = signed::catalogue_support::base64url(&[0u8; 64]).into();
    let unsigned = serde_json::to_vec(&value).expect("JSON");
    value
        .as_object_mut()
        .expect("an object")
        .remove("signature");
    let no_signature = serde_json::to_vec(&value).expect("JSON");
    let mut another = signed::catalogue_support::deployment();
    another.name = "another-deployment".to_owned();

    let cases = [
        (
            "an altered assertion",
            altered,
            Unverified::Signature(StatementRefusal::BadSignature(
                SignatureError::DoesNotVerify,
            )),
        ),
        (
            "an unsigned assertion",
            unsigned,
            Unverified::Signature(StatementRefusal::BadSignature(
                SignatureError::DoesNotVerify,
            )),
        ),
        (
            "an assertion with no signature",
            no_signature,
            Unverified::Unreadable,
        ),
        (
            "an assertion wrongly signed, with the statements key",
            by_statements_key,
            Unverified::Signature(StatementRefusal::KeyNotInForce),
        ),
        (
            "an assertion of another deployment",
            other_deployment,
            Unverified::WrongContext(ContextMismatch::Deployment {
                expected: signed::catalogue_support::deployment(),
                found: another,
            }),
        ),
        (
            "an assertion by an unknown provider",
            unknown,
            Unverified::Signature(StatementRefusal::NoAdvertisementThen),
        ),
        (
            "an assertion by another provider",
            by_another,
            Unverified::AssertionKey(vec![
                RuleViolation::AssertionOfAnotherMaker,
                RuleViolation::AssertionKeyNotAdvertised,
            ]),
        ),
    ];
    for (what, bytes, why) in cases {
        let compiles = s.platform.measurements.compiles.len();
        let admitted = s.admit(&bytes, &binaries);
        assert_eq!(
            admitted,
            Admitted::Unverified(why),
            "{what} wasn't refused for its reason"
        );
        assert_eq!(
            s.platform.measurements.compiles.len(),
            compiles,
            "{what} was compiled before it was refused"
        );
        assert!(s.platform.pending(RESERVE).is_none(), "{what} was kept");
        assert!(s.platform.pending("stranger").is_none(), "{what} was kept");
        assert_eq!(s.platform.active_version(RESERVE), None);
    }

    // A key the advertisement in force no longer lists can't sign a release, though it signed one before
    // (`THR-4`): the provider retires its advertising key for another.
    let retired = advertising.clone();
    s.rotate_advertising_key(RESERVE);
    let by_retired = signed::assertion(&declaration, &binaries, &retired);
    match s.admit(&by_retired, &binaries) {
        Admitted::Unverified(Unverified::Signature(StatementRefusal::KeyNotInForce)) => {}
        other => panic!("a retired key's assertion wasn't refused as such: {other:?}"),
    }
    // The same release, signed with the key now in force, is admitted.
    let by_current = signed::assertion(&declaration, &binaries, &signed::rotated_key(RESERVE));
    assert!(
        matches!(s.admit(&by_current, &binaries), Admitted::Artefact(_)),
        "the release signed with the key in force wasn't admitted"
    );
}

// verifies: REQ-MDL-1
#[test]
fn acceptance_of_another_assertion_digest_does_not_admit_the_release() {
    let mut s = Signed::new();
    let key = signed::catalogue_support::advertising_key(RESERVE);
    let (declaration, binaries) = release(RESERVE, "1.0.0", &["withdrawal"]);
    let received = signed::assertion(&declaration, &binaries, &key);
    let Admitted::Artefact(artefact) = s.admit(&received, &binaries) else {
        panic!("the first release wasn't admitted");
    };
    assert_eq!(
        artefact.assertion_digest.value.as_str(),
        sha256(&received),
        "the assertion's digest isn't of the assertion as received"
    );
    assert_eq!(artefact.acceptance.required, AcceptanceMode::Fresh);

    // The same release asserted again, a second later: another assertion, so another digest.
    let other = signed::assertion_at(&declaration, &binaries, &key, "2026-11-07T00:00:01Z");
    let others = [
        signed::digest_of(&other),
        signed::digest_of(&binaries.sandboxed),
        signed::digest_of(&[]),
    ];
    for wrong in &others {
        assert!(
            s.accept(RESERVE, wrong, "test-reviewer").is_none(),
            "an acceptance of another digest admitted the release"
        );
        assert_eq!(s.platform.active_version(RESERVE), None, "the release ran");
        assert_eq!(
            s.platform.pending(RESERVE).map(|a| &a.assertion_digest),
            Some(&artefact.assertion_digest),
            "the release stopped waiting"
        );
    }

    let accepted = s
        .accept(RESERVE, &artefact.assertion_digest, "test-reviewer")
        .expect("the acceptance of its own digest");
    assert_eq!(accepted.assertion_digest, artefact.assertion_digest);
    assert_eq!(s.platform.active_version(RESERVE).as_deref(), Some("1.0.0"));
    assert!(
        s.answers_as(RESERVE, "1.0.0", 1),
        "the accepted release didn't run"
    );

    // The next release's history names the accepted one by that digest.
    let (next, next_binaries) = release(RESERVE, "1.0.1", &["withdrawal"]);
    let Admitted::Artefact(following) = s.admit(
        &signed::assertion(&next, &next_binaries, &key),
        &next_binaries,
    ) else {
        panic!("the next release wasn't admitted");
    };
    assert_eq!(
        following.history.last().map(|e| &e.assertion_digest),
        Some(&artefact.assertion_digest)
    );
}

// verifies: REQ-MDL-1
#[test]
fn admission_while_one_is_pending_is_refused() {
    let mut s = Signed::new();
    let key = signed::catalogue_support::advertising_key(RESERVE);
    let (first, first_binaries) = release(RESERVE, "1.0.0", &["withdrawal"]);
    let Admitted::Artefact(waiting) = s.admit(
        &signed::assertion(&first, &first_binaries, &key),
        &first_binaries,
    ) else {
        panic!("the first release wasn't admitted");
    };
    let compiles = s.platform.measurements.compiles.len();
    for version in ["1.0.0", "1.0.1", "2.0.0", "0.9.0"] {
        let (declaration, binaries) = release(RESERVE, version, &["withdrawal"]);
        match s.admit(&signed::assertion(&declaration, &binaries, &key), &binaries) {
            Admitted::Refused(AdmissionRefusal::VersionAlreadyAdmitted) => {}
            other => panic!("release {version} was admitted while 1.0.0 waits: {other:?}"),
        }
    }
    assert_eq!(
        s.platform.measurements.compiles.len(),
        compiles,
        "a release refused while one waits was compiled"
    );
    assert_eq!(
        s.platform.pending(RESERVE).map(|a| &a.assertion_digest),
        Some(&waiting.assertion_digest),
        "the waiting release changed"
    );

    // Accepted, it runs, and the provider's next release is admitted again.
    s.accept(RESERVE, &waiting.assertion_digest, "test-reviewer")
        .expect("the waiting release");
    // 1.1.0 widens (it handles deposits too), so it waits in turn, and 1.2.0 is refused while it does, the
    // running version staying in force.
    let (widened, widened_binaries) = release(RESERVE, "1.1.0", &["withdrawal", "deposit"]);
    let Admitted::Artefact(second) = s.admit(
        &signed::assertion(&widened, &widened_binaries, &key),
        &widened_binaries,
    ) else {
        panic!("the widening release wasn't admitted");
    };
    assert_eq!(second.acceptance.accepted, None);
    let (third, third_binaries) = release(RESERVE, "1.2.0", &["withdrawal", "deposit"]);
    assert!(matches!(
        s.admit(
            &signed::assertion(&third, &third_binaries, &key),
            &third_binaries
        ),
        Admitted::Refused(AdmissionRefusal::VersionAlreadyAdmitted)
    ));
    assert_eq!(s.platform.active_version(RESERVE).as_deref(), Some("1.0.0"));
    assert!(
        s.answers_as(RESERVE, "1.0.0", 1),
        "the running version stopped answering"
    );
}

// verifies: AC-21, REQ-MDL-3
#[test]
fn widened_release_needs_fresh_acceptance() {
    let mut s = Signed::new();
    let key = signed::catalogue_support::advertising_key(RESERVE);
    let (first, first_binaries) = release(RESERVE, "1.0.0", &["withdrawal"]);
    let Admitted::Artefact(first) = s.admit(
        &signed::assertion(&first, &first_binaries, &key),
        &first_binaries,
    ) else {
        panic!("the first release wasn't admitted");
    };
    let first = s
        .accept(RESERVE, &first.assertion_digest, "test-reviewer")
        .expect("the first release");

    // A release handling deposits too reaches money the first couldn't: it waits for a fresh acceptance, and
    // the first stays in force.
    let (widened, widened_binaries) = release(RESERVE, "1.1.0", &["withdrawal", "deposit"]);
    let Admitted::Artefact(waiting) = s.admit(
        &signed::assertion(&widened, &widened_binaries, &key),
        &widened_binaries,
    ) else {
        panic!("the widening release wasn't admitted");
    };
    assert_eq!(waiting.acceptance.required, AcceptanceMode::Fresh);
    assert_eq!(waiting.acceptance.accepted, None);
    assert!(!waiting.capabilities.widenings.is_empty());
    assert_eq!(s.platform.active_version(RESERVE).as_deref(), Some("1.0.0"));
    assert!(
        s.answers_as(RESERVE, "1.0.0", 1),
        "the widening release ran before its acceptance"
    );

    let accepted = s
        .accept(RESERVE, &waiting.assertion_digest, "test-reviewer")
        .expect("the widening release");
    assert_eq!(
        accepted
            .acceptance
            .accepted
            .as_ref()
            .and_then(|a| a.by.as_ref())
            .map(|b| b.as_str()),
        Some("test-reviewer")
    );
    assert_eq!(validate_release(&first, &accepted), Ok(()));
    assert!(
        s.answers_as(RESERVE, "1.1.0", 2),
        "the accepted release didn't run"
    );

    // A release that widens nothing is accepted automatically and runs at once.
    let (same, same_binaries) = release(RESERVE, "1.1.1", &["withdrawal", "deposit"]);
    let Admitted::Artefact(automatic) = s.admit(
        &signed::assertion(&same, &same_binaries, &key),
        &same_binaries,
    ) else {
        panic!("the release that widens nothing wasn't admitted");
    };
    assert_eq!(automatic.acceptance.required, AcceptanceMode::Automatic);
    assert!(automatic.acceptance.accepted.is_some());
    assert_eq!(validate_release(&accepted, &automatic), Ok(()));
    assert!(
        s.answers_as(RESERVE, "1.1.1", 3),
        "the automatic release didn't run"
    );
}

// verifies: REQ-MDL-1, THR-4
#[test]
fn acceptance_after_the_signing_keys_rotation_is_refused() {
    let mut s = Signed::new();
    let key = signed::catalogue_support::advertising_key(RESERVE);
    let (declaration, binaries) = release(RESERVE, "1.0.0", &["withdrawal"]);
    let Admitted::Artefact(waiting) =
        s.admit(&signed::assertion(&declaration, &binaries, &key), &binaries)
    else {
        panic!("the release wasn't admitted");
    };
    // The key that signed it is retired, as a provider does with a key it believes stolen: what it signed is
    // judged again when it is accepted, and is no longer vouched for.
    s.rotate_advertising_key(RESERVE);
    assert!(
        s.accept(RESERVE, &waiting.assertion_digest, "test-reviewer")
            .is_none(),
        "a release signed with a retired key was accepted"
    );
    assert_eq!(s.platform.active_version(RESERVE), None, "the release ran");
    assert_eq!(
        s.platform.pending(RESERVE).map(|a| &a.assertion_digest),
        Some(&waiting.assertion_digest),
        "the refused release stopped waiting"
    );
}
