//! A verifier's judgement in the snapshot: tied to the verifier's key, with the verifier's origin, and only while its
//! grant is in force at Core's clock (`T-020-05`; `REQ-CAT-2`, `REQ-AUD-4`).

mod snapshot_support;

use std::path::Path;

use rutent_contracts::provider_role::{Advertisement, JUDGEMENT_SHAPE, Statement};
use rutent_contracts::router_api::{FigureValue, Origin};
use rutent_core_orch::snapshot::{Asked, Exposures, Received, Refusal, Releases};
use rutent_prov::catalogue::{
    Bond, Category, Grant, GrantTerms, Instant, ReversalCapacity, Standing, Unusable,
};
use serde_json::json;
use snapshot_support::*;

const VERIFIER: &str = "id-check";

fn verifier_advertisement() -> Advertisement {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../contracts/provider-role/fixtures/valid/advertisement-verifier-without-evidence-or-retry-declarations.json",
    );
    let ad: Statement<Advertisement> =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    ad.statement
}

/// The verifier's judgement of `subj-1`, made at `made_at`, signed with its first statements key.
fn judgement(made_at: u64) -> Received {
    let body = json!({
        "subject": "subj-1",
        "class": "kyc.natural_person",
        "answer": "yes",
        "valid_until": Instant(made_at + 86_400).rfc3339().unwrap(),
    });
    Received {
        tuple: withdrawal_tuple(RESERVE),
        asked: Asked::Eligibility,
        bytes: sign(
            envelope(&deployment(), JUDGEMENT_SHAPE, VERIFIER, made_at, body),
            &statements_key(VERIFIER, 1),
        ),
    }
}

// verifies: REQ-CAT-2, REQ-AUD-4
#[test]
fn judgement_is_carried_only_while_its_verifiers_grant_is_in_force_at_cores_clock() {
    let mut catalogue = catalogue(&[(RESERVE, "acme")], 100_000);
    // The verifier is granted judgements, no tuple, until 5 000.
    let ad = verifier_advertisement();
    catalogue
        .grant(
            Grant::new(GrantTerms {
                id: "g-verifier".into(),
                provider: VERIFIER.into(),
                party: "Id Check Ltd".into(),
                granted_by: "operator-admin".into(),
                granted_at: Instant(1_000),
                expires_at: Instant(5_000),
                categories: vec![Category::Verifier],
                tuples: vec![],
                reversal_capacity: ReversalCapacity::check(
                    Bond {
                        asset: "USDC".into(),
                        amount: 0,
                        held_by: "escrow-co".into(),
                    },
                    0,
                    "ops-lead",
                    Instant(1_000),
                )
                .unwrap(),
            })
            .unwrap(),
        )
        .unwrap();
    catalogue
        .trust_advertising_key("g-verifier", public_key(&advertising_key(VERIFIER)))
        .unwrap();
    let keys = keys_of(VERIFIER, &[&statements_key(VERIFIER, 1)]);
    catalogue
        .join(
            &advertisement_bytes(&ad, keys, JOINED_AT),
            Instant(JOINED_AT),
        )
        .unwrap();
    let mut releases = Releases::new();
    releases
        .record(&accepted_release(VERIFIER, json!([])))
        .unwrap();
    let clock = TestClock::at(4_000);
    let bounds = no_ceilings();
    let prices = prices(&[RESERVE]);

    let formed = form(
        &catalogue,
        &clock,
        &Exposures::new(),
        &releases,
        &bounds,
        &prices,
        &[judgement(3_500)],
    );
    assert!(formed.refused.is_empty(), "{:?}", formed.refused);
    let carried = formed
        .snapshot
        .figures
        .iter()
        .find(|f| matches!(f.value, FigureValue::Eligibility { .. }))
        .expect("the judgement is carried");
    let Origin::Provider {
        provider,
        publisher,
        provider_version,
        module_version,
    } = &carried.origin
    else {
        panic!("a judgement is its verifier's");
    };
    assert_eq!(
        (
            provider.as_str(),
            publisher.as_str(),
            provider_version.as_str(),
            module_version.as_str()
        ),
        (VERIFIER, "acme", "0.1.0", "1.0.0")
    );

    // Past the verifier's expiry by Core's clock, its judgement is kept out.
    clock.set(5_001);
    let formed = form(
        &catalogue,
        &clock,
        &Exposures::new(),
        &releases,
        &bounds,
        &prices,
        &[judgement(3_500)],
    );
    assert!(
        !formed
            .snapshot
            .figures
            .iter()
            .any(|f| matches!(f.value, FigureValue::Eligibility { .. }))
    );
    assert_eq!(formed.refused.len(), 1);
    assert_eq!(
        formed.refused[0].reason,
        Refusal::MakerNotUsable(Unusable::NotInForce(Standing::Expired))
    );
}
