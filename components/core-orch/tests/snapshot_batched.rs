//! A reserve's `payouts` declaration reaches the router through Core's snapshot, so a route ending on a reserve that
//! batches payouts is dropped with its reason and one ending on a reserve that pays per order is kept (`T-020-43`;
//! `REQ-ORD-33`, `REQ-AUD-4`).

mod snapshot_support;

use rutent_contracts::provider_role::{Advertisement, EvidenceGrade, Payouts};
use rutent_contracts::router_api::{DropReason, FigureValue, Version};
use rutent_core_orch::snapshot::{Exposures, Releases};
use rutent_prov::catalogue::{Catalogue, Instant, tuples_of};
use serde_json::json;
use snapshot_support::*;

const BATCHING: &str = "reserve-batching";
const PER_ORDER: &str = "reserve-per-order";

/// Two reserves of one publisher: `BATCHING` declares `batched` on its deposit tuple, `PER_ORDER` nothing.
fn two_reserves() -> Catalogue {
    let mut catalogue = catalogue(&[], 100_000);
    for (i, provider) in [BATCHING, PER_ORDER].into_iter().enumerate() {
        let mut value = serde_json::to_value(reserve(provider, "acme")).unwrap();
        if provider == BATCHING {
            for tuple in value["offers"]["reserve"]["tuples"].as_array_mut().unwrap() {
                if tuple["origin"].is_string() {
                    tuple["payouts"] = json!("batched");
                }
            }
        }
        let ad: Advertisement = serde_json::from_value(value).unwrap();
        let id = format!("g-{i}");
        catalogue.grant(grant(&id, &ad, 100_000)).unwrap();
        catalogue
            .trust_advertising_key(&id, public_key(&advertising_key(provider)))
            .unwrap();
        let keys = keys_of(provider, &[&statements_key(provider, 1)]);
        catalogue
            .join(
                &advertisement_bytes(&ad, keys, JOINED_AT),
                Instant(JOINED_AT),
            )
            .unwrap();
        catalogue.set_module_version(provider, Version::new("1.0.0").unwrap());
        for key in tuples_of(&ad) {
            catalogue.set_evidence_floor(key, EvidenceGrade::Statement);
        }
    }
    catalogue
}

// verifies: REQ-ORD-33, REQ-AUD-4
#[test]
fn snapshot_hands_each_deposit_tuples_payouts_to_the_router() {
    let catalogue = two_reserves();
    let clock = TestClock::at(2_000);
    let bounds = no_ceilings();
    let formed = form(
        &catalogue,
        &clock,
        &Exposures::new(),
        &Releases::new(),
        &bounds,
        &prices(&[BATCHING, PER_ORDER]),
        &[],
    );
    let declared: Vec<(String, Payouts)> = formed
        .snapshot
        .figures
        .iter()
        .filter_map(|f| match &f.value {
            FigureValue::ReserveTuple { tuple } if tuple.direction().is_some() => Some((
                f.origin.provider().unwrap().as_str().to_owned(),
                tuple.payouts(),
            )),
            _ => None,
        })
        .filter(|(_, p)| *p == Payouts::Batched)
        .collect();
    assert_eq!(declared.len(), 1);
    assert_eq!(declared[0].0, BATCHING);
    // The router drops what ends there, and keeps what ends at the other reserve.
    let record = route(&shortlist_request("1000"), &formed.snapshot, &bounds);
    let ends_at = |c: &rutent_contracts::router_api::Candidate, provider: &str| {
        c.parts[0].operations.last().provider.as_str() == provider
    };
    let (mut dropped, mut kept) = (0, 0);
    for candidate in &record.candidates {
        if ends_at(candidate, BATCHING) {
            assert!(
                matches!(candidate.dropped, Some(DropReason::BatchedPayout { .. })),
                "{:?}",
                candidate.dropped
            );
            dropped += 1;
        } else if ends_at(candidate, PER_ORDER) && candidate.dropped.is_none() {
            kept += 1;
        }
    }
    assert!(dropped > 0 && kept > 0, "dropped {dropped}, kept {kept}");
}
