//! Exposures from Core's records, so the router's ceilings in flight hold (`T-020-05`; `REQ-ORD-8`), and the
//! `calls` figure from each accepted release (`F-011`'s notes).

mod snapshot_support;

use rutent_contracts::provider_role::Denomination;
use rutent_contracts::router_api::{DropReason, FigureValue, Origin};
use rutent_core_orch::snapshot::{Exposures, Releases};
use serde_json::json;
use snapshot_support::*;

fn usdc_unit() -> Denomination {
    Denomination {
        id: "USDC".into(),
        decimals: 6,
    }
}

fn exposure_of(formed: &rutent_core_orch::snapshot::Formed, provider: &str) -> Option<String> {
    formed.snapshot.figures.iter().find_map(|f| match &f.value {
        FigureValue::Exposure {
            provider: p,
            in_flight,
            ..
        } if p.as_str() == provider => Some(in_flight.as_str().to_owned()),
        _ => None,
    })
}

// verifies: REQ-ORD-8
#[test]
fn route_over_a_providers_ceiling_in_flight_is_not_offered() {
    let catalogue = catalogue(&[(RESERVE, "acme")], 100_000);
    let clock = TestClock::at(2_000);
    let releases = Releases::new();
    let prices = prices(&[RESERVE]);
    // The route withdraws 1 000 and deposits 1 000 at the reserve: 2 000 in flight there. The ceiling is 2 500.
    let bounds = bounds(
        json!([{ "provider": RESERVE, "asset": usdc(), "amount": "2500" }]),
        json!([]),
    );
    let request = shortlist_request("1000");

    let idle = Exposures::new();
    let formed = form(&catalogue, &clock, &idle, &releases, &bounds, &prices, &[]);
    assert!(offered_through(
        &route(&request, &formed.snapshot, &bounds),
        RESERVE
    ));

    // One order split in two parts, each holding 300 at the reserve: together 600, so the route would put 2 600
    // in flight there.
    let mut exposures = Exposures::new();
    exposures
        .hold(&id("order-1"), &id("seg-1a"), &withdrawal(RESERVE, "300"))
        .unwrap();
    exposures
        .hold(&id("order-1"), &id("seg-1b"), &withdrawal(RESERVE, "300"))
        .unwrap();
    assert_eq!(
        exposures.in_flight(&id(RESERVE), &usdc_unit()).as_str(),
        "600"
    );

    let formed = form(
        &catalogue,
        &clock,
        &exposures,
        &releases,
        &bounds,
        &prices,
        &[],
    );
    assert_eq!(exposure_of(&formed, RESERVE).as_deref(), Some("600"));
    let record = route(&request, &formed.snapshot, &bounds);
    assert!(!offered_through(&record, RESERVE));
    assert!(
        record
            .candidates()
            .iter()
            .all(|c| matches!(c.dropped, Some(DropReason::OverCeiling { .. })))
    );
}

// verifies: REQ-ORD-8
#[test]
fn orders_at_one_publishers_providers_count_together_against_its_ceiling() {
    let catalogue = catalogue(&[(RESERVE, "acme"), (SIBLING, "acme")], 100_000);
    let clock = TestClock::at(2_000);
    let mut releases = Releases::new();
    // The reserve's release may ask its sibling's module for a deposit: a route through both names the call.
    releases
        .record(&accepted_release(
            RESERVE,
            json!([{ "callee": SIBLING, "kinds": ["deposit"] }]),
        ))
        .unwrap();
    let prices = prices(&[RESERVE, SIBLING]);
    // Every route puts 2 000 in flight at acme's providers. The publisher's ceiling is 2 500.
    let bounds = bounds(
        json!([]),
        json!([{ "publisher": "acme", "asset": usdc(), "amount": "2500" }]),
    );
    let request = shortlist_request("1000");

    // One order holds 300 at the reserve: 2 300 is within the ceiling.
    let mut exposures = Exposures::new();
    exposures
        .hold(&id("order-1"), &id("seg-1"), &withdrawal(RESERVE, "300"))
        .unwrap();
    let formed = form(
        &catalogue,
        &clock,
        &exposures,
        &releases,
        &bounds,
        &prices,
        &[],
    );
    assert!(formed.snapshot.figures.iter().any(|f| matches!(
        (&f.value, &f.origin),
        (FigureValue::Calls { .. }, Origin::Provider { provider, .. }) if provider.as_str() == RESERVE
    )));
    let record = route(&request, &formed.snapshot, &bounds);
    assert!(offered_through(&record, RESERVE) && offered_through(&record, SIBLING));
    assert!(record.shortlist().iter().any(|c| {
        c.parts
            .iter()
            .any(|p| p.operations.iter().any(|o| !o.calls.is_empty()))
    }));

    // Another order holds 300 at the sibling. Neither provider is near a ceiling of its own, but together the
    // publisher's providers would have 2 600 in flight.
    exposures
        .hold(&id("order-2"), &id("seg-2"), &withdrawal(SIBLING, "300"))
        .unwrap();
    let formed = form(
        &catalogue,
        &clock,
        &exposures,
        &releases,
        &bounds,
        &prices,
        &[],
    );
    assert_eq!(exposure_of(&formed, RESERVE).as_deref(), Some("300"));
    assert_eq!(exposure_of(&formed, SIBLING).as_deref(), Some("300"));
    let record = route(&request, &formed.snapshot, &bounds);
    assert!(record.shortlist().is_empty());
    assert!(
        record
            .candidates()
            .iter()
            .all(|c| matches!(c.dropped, Some(DropReason::OverCeiling { .. })))
    );
}

// verifies: REQ-ORD-8
#[test]
fn exposure_on_stuck_orders_is_readable_at_any_time() {
    let mut catalogue = catalogue(&[(RESERVE, "acme"), (SIBLING, "acme")], 100_000);
    let clock = TestClock::at(2_000);
    let releases = Releases::new();
    let mut exposures = Exposures::new();
    exposures
        .hold(&id("order-9"), &id("seg-9"), &withdrawal(RESERVE, "300"))
        .unwrap();
    exposures
        .hold(&id("order-8"), &id("seg-8"), &withdrawal(SIBLING, "100"))
        .unwrap();
    exposures.mark_stuck(&id("order-9"));

    // Read with no snapshot formed and no clock read.
    let stuck = exposures.stuck();
    assert_eq!(stuck.len(), 1);
    assert_eq!(
        (
            stuck[0].order.as_str(),
            stuck[0].provider.as_str(),
            stuck[0].amount.as_str()
        ),
        ("order-9", RESERVE, "300")
    );

    // The reserve's grant is revoked and time passes: its tuples are out, but its exposure is still read, and
    // still in every snapshot, where it counts against its publisher's ceiling.
    catalogue
        .revoke("g-0", rutent_prov::catalogue::Instant(2_500))
        .unwrap();
    clock.set(50_000);
    assert_eq!(exposures.stuck().len(), 1);
    let bounds = bounds(
        json!([]),
        json!([{ "publisher": "acme", "asset": usdc(), "amount": "2350" }]),
    );
    let prices = prices(&[RESERVE, SIBLING]);
    let formed = form(
        &catalogue,
        &clock,
        &exposures,
        &releases,
        &bounds,
        &prices,
        &[],
    );
    assert!(!formed.snapshot.figures.iter().any(|f| matches!(
        &f.origin, Origin::Provider { provider, .. } if provider.as_str() == RESERVE
    )));
    assert_eq!(exposure_of(&formed, RESERVE).as_deref(), Some("300"));
    // The sibling's own route puts 2 000 in flight; with the stuck 300 and its own 100, acme is past 2 350.
    let record = route(&shortlist_request("1000"), &formed.snapshot, &bounds);
    assert!(!offered_through(&record, SIBLING));

    // Once the stuck order is resolved, nothing of it is held.
    exposures.release_order(&id("order-9"));
    assert!(exposures.stuck().is_empty());
}
