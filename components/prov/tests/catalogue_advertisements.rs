//! Advertisements and the key history the catalogue keeps (`T-012-02`; `REQ-CAT-2`, `REQ-PRT-1`, `THR-4`,
//! `THR-8`).

mod common;

use common::*;
use ed25519_dalek::SigningKey;
use rutent_contracts::provider_role::{
    Advertisement, KeyPurpose, PublicKey, RESULT_SHAPE, RuleViolation, SignatureScheme,
    SigningKey as ListedKey, VERSION,
};
use rutent_prov::catalogue::{
    Catalogue, DEFAULT_CLOCK_SKEW_SECONDS, Instant, JoinRefusal, SignatureError, StatementRefusal,
};

const NOW: Instant = Instant(2_000);

/// A catalogue serving the tests' deployment, with a grant of `bridge-pay`.
fn catalogue() -> Catalogue {
    let mut catalogue = Catalogue::new();
    catalogue.set_context(deployment(), VERSION);
    catalogue
        .grant(grant(
            "g-1",
            "bridge-pay",
            rutent_prov::catalogue::tuples_of(&ramp_advertisement()),
            100_000,
        ))
        .unwrap();
    catalogue
}

/// A catalogue where the operator trusts `bridge-pay`'s advertising key.
fn trusting() -> Catalogue {
    let mut catalogue = catalogue();
    catalogue
        .trust_advertising_key("g-1", public_key(&advertising_key("bridge-pay")))
        .unwrap();
    catalogue
}

fn listed(key: &SigningKey, purpose: KeyPurpose) -> ListedKey {
    ListedKey {
        scheme: SignatureScheme::Ed25519,
        public_key: public_key(key),
        purpose,
    }
}

/// The ramp advertisement, signed with `signer`, listing `keys`, dated `made_at`.
fn bytes(keys: Vec<ListedKey>, made_at: u64, signer: &SigningKey) -> Vec<u8> {
    advertisement_bytes(&ramp_advertisement(), Some(keys), Instant(made_at), signer)
}

fn adv(n: u8) -> SigningKey {
    signing_key("bridge-pay-advertising", n)
}

fn stm(n: u8) -> SigningKey {
    signing_key("bridge-pay-statements", n)
}

fn keys(advertising: &[&SigningKey], statements: &[&SigningKey]) -> Vec<ListedKey> {
    advertising
        .iter()
        .map(|k| listed(k, KeyPurpose::Advertising))
        .chain(statements.iter().map(|k| listed(k, KeyPurpose::Statements)))
        .collect()
}

/// A catalogue that trusts `adv(1)` for `bridge-pay`, and has accepted its first advertisement, {adv(1), stm(1)},
/// made at 2 000.
fn first_accepted() -> Catalogue {
    let mut catalogue = catalogue();
    catalogue
        .trust_advertising_key("g-1", public_key(&adv(1)))
        .unwrap();
    catalogue
        .join(&bytes(keys(&[&adv(1)], &[&stm(1)]), 2_000, &adv(1)), NOW)
        .expect("a first advertisement signed with a granted key");
    catalogue
}

fn made_by(maker: &str, made_at: u64, signer: &SigningKey, shape: &str) -> Vec<u8> {
    sign_json(
        serde_json::json!({
            "deployment": {"name": "rutent-test", "operator_key": deployment().operator_key.as_str()},
            "protocol_version": VERSION,
            "shape": shape,
            "maker": maker,
            "made_at": Instant(made_at).rfc3339().unwrap(),
            "signature": {"scheme": "ed25519", "key": "", "value": ""},
            "statement": {}
        }),
        signer,
    )
}

// verifies: REQ-CAT-2, THR-4
#[test]
fn advertisement_dated_too_far_ahead_of_the_catalogues_clock_is_refused() {
    let skew = DEFAULT_CLOCK_SKEW_SECONDS;
    let ahead = |made_at: &str| {
        let mut catalogue = trusting();
        let ad = advertisement_bytes_at(
            &ramp_advertisement(),
            None,
            made_at,
            &advertising_key("bridge-pay"),
        );
        let refusal = catalogue.join(&ad, NOW);
        (refusal, catalogue.advertisement_history("bridge-pay").len())
    };
    let exactly = Instant(NOW.0 + skew).rfc3339().unwrap();
    assert_eq!(ahead(&exactly), (Ok(()), 1));

    let one_more = Instant(NOW.0 + skew + 1).rfc3339().unwrap();
    assert_eq!(
        ahead(&one_more),
        (
            Err(JoinRefusal::DatedTooFarAhead {
                latest_allowed: Instant(NOW.0 + skew)
            }),
            0
        )
    );
    // A fraction of a second past the bound counts as the next second.
    let fraction = exactly.replace('Z', ".001Z");
    assert_eq!(ahead(&fraction).1, 0);
    // The year 9999: no later advertisement could ever pass it.
    assert_eq!(ahead("9999-12-31T23:59:59Z").1, 0);
}

// verifies: REQ-CAT-2
#[test]
fn skew_is_the_operators_to_set_and_counts_from_the_callers_clock() {
    let mut catalogue = trusting();
    catalogue.set_clock_skew(0);
    assert_eq!(catalogue.clock_skew(), 0);
    let ad = |made_at| {
        bytes(
            keys_of_bridge_pay(),
            made_at,
            &advertising_key("bridge-pay"),
        )
    };
    assert!(matches!(
        catalogue.join(&ad(NOW.0 + 1), NOW),
        Err(JoinRefusal::DatedTooFarAhead { .. })
    ));
    // The same advertisement is fine on a clock that has caught up, and a stale clock refuses it.
    assert_eq!(catalogue.join(&ad(NOW.0 + 1), Instant(NOW.0 + 1)), Ok(()));
}

fn keys_of_bridge_pay() -> Vec<ListedKey> {
    keys_of("bridge-pay")
}

// verifies: REQ-CAT-2
#[test]
fn advertisement_no_newer_than_the_last_accepted_is_refused() {
    let mut catalogue = first_accepted();
    let same_time = bytes(keys(&[&adv(1)], &[&stm(1)]), 2_000, &adv(1));
    let older = bytes(keys(&[&adv(1)], &[&stm(1)]), 1_999, &adv(1));
    for refused in [same_time, older] {
        let Err(JoinRefusal::NotASuccessor(violations)) = catalogue.join(&refused, NOW) else {
            panic!("a replay is refused");
        };
        assert!(violations.contains(&RuleViolation::SuccessorNotNewer));
    }
    assert_eq!(catalogue.advertisement_history("bridge-pay").len(), 1);

    let newer = bytes(keys(&[&adv(1)], &[&stm(1)]), 2_001, &adv(1));
    assert_eq!(catalogue.join(&newer, NOW), Ok(()));
}

// verifies: REQ-CAT-2, THR-4
#[test]
fn replayed_advertisement_does_not_bring_back_a_retired_statements_key() {
    let mut catalogue = first_accepted();
    let v1 = bytes(keys(&[&adv(1)], &[&stm(1)]), 2_000, &adv(1));
    let v2 = bytes(keys(&[&adv(1)], &[&stm(2)]), 2_001, &adv(1));
    assert_eq!(catalogue.join(&v2, NOW), Ok(()));
    // v1 is public and validly signed; it is not newer, so it is not accepted again.
    assert!(catalogue.join(&v1, NOW).is_err());
    let held = catalogue.accepted_advertisement("bridge-pay").unwrap();
    assert!(
        held.statement
            .keys
            .iter()
            .all(|k| k.public_key != public_key(&stm(1)))
    );
}

// verifies: REQ-CAT-2, THR-4
#[test]
fn dropped_statements_key_is_never_listed_again() {
    let mut catalogue = first_accepted();
    let v2 = bytes(keys(&[&adv(1)], &[&stm(2)]), 2_001, &adv(1));
    assert_eq!(catalogue.join(&v2, NOW), Ok(()));
    // A fresh advertisement, newer than both, signed with the right key, lists the dropped key again.
    let v3 = bytes(keys(&[&adv(1)], &[&stm(1), &stm(2)]), 2_002, &adv(1));
    assert_eq!(
        catalogue.join(&v3, NOW),
        Err(JoinRefusal::DroppedKeyRelisted(public_key(&stm(1))))
    );
    // Under the other purpose too.
    let v3 = bytes(keys(&[&adv(1), &stm(1)], &[&stm(2)]), 2_002, &adv(1));
    assert!(catalogue.join(&v3, NOW).is_err());
    assert_eq!(catalogue.advertisement_history("bridge-pay").len(), 2);
    // A key that was never dropped can be kept.
    let v3 = bytes(keys(&[&adv(1)], &[&stm(2), &stm(3)]), 2_002, &adv(1));
    assert_eq!(catalogue.join(&v3, NOW), Ok(()));
}

// verifies: REQ-CAT-2, THR-4
#[test]
fn key_once_used_for_statements_is_never_accepted_for_advertising_in_any_deployment() {
    // dex-pool's accepted advertisement lists stm_dex for statements.
    let mut catalogue = first_accepted();
    catalogue
        .grant(grant(
            "g-2",
            "dex-pool",
            rutent_prov::catalogue::tuples_of(&exchange_advertisement()),
            100_000,
        ))
        .unwrap();
    let dex_key = advertising_key("dex-pool");
    let stm_dex = statements_key("dex-pool");
    catalogue
        .trust_advertising_key("g-2", public_key(&dex_key))
        .unwrap();
    let ad = advertisement_bytes(
        &exchange_advertisement(),
        Some(keys(&[&dex_key], &[&stm_dex])),
        Instant(2_000),
        &dex_key,
    );
    assert_eq!(catalogue.join(&ad, NOW), Ok(()));

    // The operator's grant can't vouch for it.
    assert_eq!(
        catalogue.trust_advertising_key("g-1", public_key(&stm_dex)),
        Err(rutent_prov::catalogue::CatalogueError::KeyIsAStatementsKey)
    );
    // bridge-pay's successor, signed with the key it has, adds that key for advertising: refused, whoever
    // vouched for what.
    let adds =
        |key: &SigningKey, made_at: u64| bytes(keys(&[&adv(1), key], &[&stm(1)]), made_at, &adv(1));
    assert_eq!(
        catalogue.join(&adds(&stm_dex, 2_001), NOW),
        Err(JoinRefusal::AdvertisingKeyIsAStatementsKey(public_key(
            &stm_dex
        )))
    );
    assert_eq!(catalogue.advertisement_history("bridge-pay").len(), 1);

    // A key a release declared as its statement key, in any deployment, is a statements key too.
    let declared = signing_key("a release's statement key", 1);
    assert_eq!(catalogue.join(&adds(&declared, 2_001), NOW), Ok(()));
    let declared_later = signing_key("a release's statement key", 2);
    catalogue
        .record_statement_key(&public_key(&declared_later))
        .unwrap();
    assert_eq!(
        catalogue.join(&adds(&declared_later, 2_002), NOW),
        Err(JoinRefusal::AdvertisingKeyIsAStatementsKey(public_key(
            &declared_later
        )))
    );
    // And a key that is already an advertising key can't be recorded as a release's statement key.
    assert_eq!(
        catalogue.record_statement_key(&public_key(&declared)),
        Err(rutent_prov::catalogue::CatalogueError::KeyIsAnAdvertisingKey)
    );
}

// verifies: REQ-CAT-2, THR-4
#[test]
fn advertising_key_is_never_listed_as_a_statements_key_by_another_provider() {
    let mut catalogue = first_accepted();
    catalogue
        .grant(grant(
            "g-2",
            "dex-pool",
            rutent_prov::catalogue::tuples_of(&exchange_advertisement()),
            100_000,
        ))
        .unwrap();
    let dex_key = advertising_key("dex-pool");
    catalogue
        .trust_advertising_key("g-2", public_key(&dex_key))
        .unwrap();
    // dex-pool lists bridge-pay's advertising key for statements, which would lock bridge-pay out of its own key.
    let ad = advertisement_bytes(
        &exchange_advertisement(),
        Some(keys(&[&dex_key], &[&adv(1)])),
        Instant(2_000),
        &dex_key,
    );
    assert_eq!(
        catalogue.join(&ad, NOW),
        Err(JoinRefusal::StatementsKeyIsAnAdvertisingKey(public_key(
            &adv(1)
        )))
    );
    let successor = bytes(keys(&[&adv(1)], &[&stm(1), &stm(2)]), 2_001, &adv(1));
    assert_eq!(catalogue.join(&successor, NOW), Ok(()));
}

// verifies: REQ-CAT-2, THR-4
#[test]
fn refused_advertisement_leaves_no_trace_in_the_key_history() {
    let mut catalogue = first_accepted();
    // A refused advertisement that lists a key for statements ...
    let bad = bytes(keys(&[&adv(1)], &[&stm(1), &stm(5)]), 2_001, &adv(2));
    assert!(catalogue.join(&bad, NOW).is_err());
    // ... does not make that key a statements key anywhere.
    let advertising = signing_key("later", 1);
    assert_eq!(catalogue.record_statement_key(&public_key(&stm(5))), Ok(()));
    assert!(
        catalogue
            .record_statement_key(&public_key(&advertising))
            .is_ok()
    );
    assert_eq!(catalogue.advertisement_history("bridge-pay").len(), 1);
}

// verifies: REQ-CAT-2, THR-4
#[test]
fn first_advertisement_vouches_for_nothing_by_listing_its_own_key() {
    // The operator granted no key: an advertisement signed by the key it lists is not trusted.
    let mut catalogue = catalogue();
    let own = bytes(keys_of_bridge_pay(), 2_000, &advertising_key("bridge-pay"));
    assert_eq!(
        catalogue.join(&own, NOW),
        Err(JoinRefusal::NotSignedWithAGrantedKey)
    );
    // The operator granted another key, and the advertisement is signed with a key it lists that isn't it.
    catalogue
        .trust_advertising_key("g-1", public_key(&adv(7)))
        .unwrap();
    assert_eq!(
        catalogue.join(&own, NOW),
        Err(JoinRefusal::NotSignedWithAGrantedKey)
    );
    // Signed with the granted key, it lists one the operator never named: refused.
    let extra = bytes(keys(&[&adv(7), &adv(8)], &[&stm(1)]), 2_000, &adv(7));
    assert_eq!(
        catalogue.join(&extra, NOW),
        Err(JoinRefusal::AdvertisingKeyNotGranted(public_key(&adv(8))))
    );
    assert!(catalogue.accepted_advertisement("bridge-pay").is_none());
    let right = bytes(keys(&[&adv(7)], &[&stm(1)]), 2_000, &adv(7));
    assert_eq!(catalogue.join(&right, NOW), Ok(()));
}

// verifies: REQ-CAT-2
#[test]
fn trust_in_a_key_needs_a_grant_in_force() {
    let mut catalogue = trusting();
    assert_eq!(
        catalogue.trust_advertising_key("no-such-grant", public_key(&adv(1))),
        Err(rutent_prov::catalogue::CatalogueError::UnknownGrant(
            "no-such-grant".into()
        ))
    );
    // The grant lapses: its keys are trusted no longer.
    let ad = bytes(
        keys_of_bridge_pay(),
        200_000,
        &advertising_key("bridge-pay"),
    );
    assert_eq!(
        catalogue.join(&ad, Instant(200_000)),
        Err(JoinRefusal::NoGrantInForce)
    );
}

// verifies: REQ-CAT-2, THR-4
#[test]
fn successor_is_accepted_only_through_the_last_accepted_advertisements_advertising_key() {
    let mut catalogue = first_accepted();
    // Signed with a key it adds itself.
    let own = bytes(keys(&[&adv(1), &adv(2)], &[&stm(1)]), 2_001, &adv(2));
    let Err(JoinRefusal::NotASuccessor(violations)) = catalogue.join(&own, NOW) else {
        panic!("a key the successor adds itself can't sign it");
    };
    assert!(violations.contains(&RuleViolation::SuccessorNotSignedWithPreviousAdvertisingKey));
    // Signed with the accepted advertisement's statements key.
    let with_statements = bytes(keys(&[&adv(1)], &[&stm(1)]), 2_001, &stm(1));
    assert!(catalogue.join(&with_statements, NOW).is_err());
    assert_eq!(catalogue.advertisement_history("bridge-pay").len(), 1);

    // Adding a new advertising key, signed with the old, is accepted; then the old one can be retired, signed
    // with either.
    let add = bytes(keys(&[&adv(1), &adv(2)], &[&stm(1)]), 2_001, &adv(1));
    assert_eq!(catalogue.join(&add, NOW), Ok(()));
    let retire = bytes(keys(&[&adv(2)], &[&stm(1)]), 2_002, &adv(2));
    assert_eq!(catalogue.join(&retire, NOW), Ok(()));
    // The retired key signs no more.
    let after = bytes(keys(&[&adv(2)], &[&stm(1)]), 2_003, &adv(1));
    assert!(catalogue.join(&after, NOW).is_err());
}

// verifies: REQ-CAT-2, THR-4
#[test]
fn advertisement_signed_by_its_own_statements_key_is_refused() {
    let mut catalogue = trusting();
    let ad = bytes(keys(&[&adv(1)], &[&stm(1)]), 2_000, &stm(1));
    assert!(matches!(
        catalogue.join(&ad, NOW),
        Err(JoinRefusal::Invalid(v)) if v.contains(&RuleViolation::SignedWithStatementsKey)
    ));
}

// verifies: REQ-CAT-2
#[test]
fn fresh_grant_recovers_a_provider_that_lost_every_advertising_key() {
    let mut catalogue = first_accepted();
    // adv(1) is lost, and an advertisement signed with adv(2) isn't a successor.
    let recovery = bytes(keys(&[&adv(2)], &[&stm(1)]), 2_100, &adv(2));
    assert!(matches!(
        catalogue.join(&recovery, Instant(2_100)),
        Err(JoinRefusal::NotSignedWithAGrantedKey | JoinRefusal::NotASuccessor(_))
    ));
    // The operator grants again, naming the new key.
    let mut fresh = grant(
        "g-2",
        "bridge-pay",
        rutent_prov::catalogue::tuples_of(&ramp_advertisement()),
        100_000,
    )
    .terms();
    fresh.granted_at = Instant(2_050);
    catalogue
        .grant(rutent_prov::catalogue::Grant::new(fresh).unwrap())
        .unwrap();
    catalogue
        .trust_advertising_key("g-2", public_key(&adv(2)))
        .unwrap();
    assert_eq!(catalogue.join(&recovery, Instant(2_100)), Ok(()));
    // The old key no longer signs successors.
    let old = bytes(keys(&[&adv(2)], &[&stm(1)]), 2_101, &adv(1));
    assert!(catalogue.join(&old, Instant(2_101)).is_err());
    // A recovery is no replay either: it is dated after the last accepted.
    let replay = bytes(keys(&[&adv(2)], &[&stm(1)]), 2_000, &adv(2));
    assert!(catalogue.join(&replay, Instant(2_101)).is_err());
}

// verifies: REQ-PRT-1, THR-8
#[test]
fn advertisement_of_another_deployment_or_protocol_version_is_refused() {
    let ad = bytes(keys_of_bridge_pay(), 2_000, &advertising_key("bridge-pay"));
    let mut catalogue = trusting();
    let other = rutent_contracts::provider_role::Deployment {
        name: "another-deployment".into(),
        operator_key: deployment().operator_key,
    };
    catalogue.set_context(other, VERSION);
    assert!(matches!(
        catalogue.join(&ad, NOW),
        Err(JoinRefusal::WrongContext(_))
    ));
    catalogue.set_context(deployment(), "1.0.0");
    assert!(matches!(
        catalogue.join(&ad, NOW),
        Err(JoinRefusal::WrongContext(_))
    ));
    catalogue.set_context(deployment(), VERSION);
    assert_eq!(catalogue.join(&ad, NOW), Ok(()));
}

// verifies: REQ-PRT-1
#[test]
fn catalogue_told_no_deployment_joins_nothing() {
    let mut catalogue = Catalogue::new();
    let ad = bytes(keys_of_bridge_pay(), 2_000, &advertising_key("bridge-pay"));
    assert_eq!(catalogue.join(&ad, NOW), Err(JoinRefusal::NoContext));
}

// verifies: REQ-PRT-1, THR-8
#[test]
fn advertisement_with_a_bad_signature_is_refused() {
    let mut catalogue = trusting();
    let ad = bytes(keys_of_bridge_pay(), 2_000, &advertising_key("bridge-pay"));
    let text = String::from_utf8(ad.clone()).unwrap();
    // Another version string: the signature no longer covers the statement.
    let tampered = text.replacen("\"version\":\"2.0.0\"", "\"version\":\"2.0.1\"", 1);
    assert_ne!(tampered, text);
    assert_eq!(
        catalogue.join(tampered.as_bytes(), NOW),
        Err(JoinRefusal::BadSignature(SignatureError::DoesNotVerify))
    );
    // A signature in a non-canonical encoding: its last character changed to another that writes the same bytes.
    let value: serde_json::Value = serde_json::from_slice(&ad).unwrap();
    let signature = value["signature"]["value"].as_str().unwrap();
    let last = signature.chars().last().unwrap();
    assert!("AQgw".contains(last));
    let alias = 'B';
    let aliased = text.replacen(signature, &format!("{}{alias}", &signature[..85]), 1);
    assert_eq!(
        catalogue.join(aliased.as_bytes(), NOW),
        Err(JoinRefusal::BadSignature(SignatureError::SignatureEncoding))
    );
    // A member named twice has no signing input.
    let doubled = text.replacen(
        "\"maker\":\"bridge-pay\"",
        "\"maker\":\"bridge-pay\",\"maker\":\"x\"",
        1,
    );
    assert!(matches!(
        catalogue.join(doubled.as_bytes(), NOW),
        Err(JoinRefusal::NoSigningInput(_) | JoinRefusal::Unreadable(_))
    ));
    assert!(catalogue.accepted_advertisement("bridge-pay").is_none());
    assert_eq!(catalogue.join(&ad, NOW), Ok(()));
}

// verifies: REQ-PRT-1
#[test]
fn small_order_public_key_in_an_advertisement_is_refused() {
    let mut catalogue = trusting();
    let identity = PublicKey::new(base64url(&{
        let mut b = [0u8; 32];
        b[0] = 1;
        b
    }))
    .unwrap();
    let mut listed_keys = keys_of_bridge_pay();
    listed_keys.push(ListedKey {
        scheme: SignatureScheme::Ed25519,
        public_key: identity.clone(),
        purpose: KeyPurpose::Statements,
    });
    let ad = bytes(listed_keys, 2_000, &advertising_key("bridge-pay"));
    assert_eq!(
        catalogue.join(&ad, NOW),
        Err(JoinRefusal::ListedKeyRefused {
            key: identity,
            why: SignatureError::KeySmallOrder
        })
    );
}

// verifies: REQ-PRT-1, THR-8
#[test]
fn statement_is_checked_against_the_key_valid_at_its_time() {
    let mut catalogue = first_accepted();
    let v2 = bytes(keys(&[&adv(1)], &[&stm(2)]), 3_000, &adv(1));
    assert_eq!(catalogue.join(&v2, Instant(3_000)), Ok(()));
    let at = Instant(4_000);
    let result = |made_at: u64, key: &SigningKey| made_by("bridge-pay", made_at, key, RESULT_SHAPE);

    // Before the second advertisement, stm(1) was the statements key.
    assert_eq!(
        catalogue.verify_statement(&result(2_500, &stm(1)), at),
        Ok(())
    );
    assert_eq!(
        catalogue.verify_statement(&result(2_500, &stm(2)), at),
        Err(StatementRefusal::KeyNotValidThen)
    );
    // From it on, stm(2) is.
    assert_eq!(
        catalogue.verify_statement(&result(3_500, &stm(2)), at),
        Ok(())
    );
    assert_eq!(
        catalogue.verify_statement(&result(3_500, &stm(1)), at),
        Err(StatementRefusal::KeyNotValidThen)
    );
    // Before any advertisement there was no key at all.
    assert_eq!(
        catalogue.verify_statement(&result(1_000, &stm(1)), at),
        Err(StatementRefusal::NoAdvertisementThen)
    );
    // A statements key signs no advertisement, and an advertising key signs no result.
    assert_eq!(
        catalogue.verify_statement(&result(3_500, &adv(1)), at),
        Err(StatementRefusal::KeyNotValidThen)
    );
    let advertisement = made_by(
        "bridge-pay",
        3_500,
        &stm(2),
        rutent_contracts::provider_role::ADVERTISEMENT_SHAPE,
    );
    assert_eq!(
        catalogue.verify_statement(&advertisement, at),
        Err(StatementRefusal::KeyNotValidThen)
    );
    // Another provider's statement is checked against its own history.
    assert_eq!(
        catalogue.verify_statement(&made_by("dex-pool", 3_500, &stm(2), RESULT_SHAPE), at),
        Err(StatementRefusal::NoAdvertisementThen)
    );
}

// verifies: REQ-PRT-1, THR-8
#[test]
fn statement_dated_too_far_ahead_of_the_callers_clock_is_refused() {
    let catalogue = first_accepted();
    let ahead = made_by("bridge-pay", 5_000, &stm(1), RESULT_SHAPE);
    assert_eq!(
        catalogue.verify_statement(&ahead, Instant(5_000 - DEFAULT_CLOCK_SKEW_SECONDS - 1)),
        Err(StatementRefusal::DatedTooFarAhead)
    );
    assert_eq!(
        catalogue.verify_statement(&ahead, Instant(5_000 - DEFAULT_CLOCK_SKEW_SECONDS)),
        Ok(())
    );
}

// verifies: REQ-PRT-1
#[test]
fn statement_with_a_bad_signature_does_not_verify() {
    let catalogue = first_accepted();
    let sent = made_by("bridge-pay", 2_500, &stm(1), RESULT_SHAPE);
    let text = String::from_utf8(sent).unwrap();
    let tampered = text.replacen("\"statement\":{}", "\"statement\":{\"x\":1}", 1);
    assert_eq!(
        catalogue.verify_statement(tampered.as_bytes(), Instant(3_000)),
        Err(StatementRefusal::BadSignature(
            SignatureError::DoesNotVerify
        ))
    );
    assert!(matches!(
        catalogue.verify_statement(b"{}", Instant(3_000)),
        Err(StatementRefusal::Unreadable(_))
    ));
}

// verifies: REQ-QTE-4, REQ-CAT-2
#[test]
fn figures_age_counts_from_the_advertisements_made_at() {
    let made_at = Instant(JOINED_AT.0 - 100);
    let catalogue = acme_catalogue_made_at(&[ramp_advertisement()], made_at);
    let built = catalogue.snapshot("snap-1", JOINED_AT).unwrap();
    assert!(!built.snapshot.figures.is_empty());
    for figure in &built.snapshot.figures {
        assert_eq!(figure.age_seconds.get(), 100);
    }
    // Dated a little ahead of the clock, within the skew: no age yet.
    let catalogue = acme_catalogue_made_at(&[ramp_advertisement()], Instant(JOINED_AT.0 + 30));
    let built = catalogue.snapshot("snap-1", JOINED_AT).unwrap();
    assert!(
        built
            .snapshot
            .figures
            .iter()
            .all(|f| f.age_seconds.get() == 0)
    );
}

// verifies: REQ-CAT-8, REQ-CAT-2
#[test]
fn accepted_advertisements_are_kept_for_the_whole_history() {
    let mut catalogue = first_accepted();
    let v2 = bytes(keys(&[&adv(1)], &[&stm(2)]), 2_001, &adv(1));
    assert_eq!(catalogue.join(&v2, NOW), Ok(()));
    let history = catalogue.advertisement_history("bridge-pay");
    assert_eq!(history.len(), 2);
    assert!(history[0].made_at < history[1].made_at);
    let _: &Advertisement = &history[0].statement;
    assert!(catalogue.advertisement_history("nobody").is_empty());
}
