//! The snapshot read at Core's own clock, passed as `at` (`T-020-05`; `REQ-CAT-2`, `F-012`'s notes).

mod snapshot_support;

use rutent_contracts::router_api::Origin;
use rutent_core_orch::snapshot::{Exposures, Releases};
use rutent_prov::catalogue::{Instant, LeftOutReason, Standing, Unusable};
use snapshot_support::*;

// verifies: REQ-CAT-2
#[test]
fn snapshot_read_at_cores_clock_honours_no_expired_grant() {
    // The reserve's grant runs to 5 000.
    let catalogue = catalogue(&[(RESERVE, "acme"), (SIBLING, "acme")], 5_000);
    let clock = TestClock::at(4_000);
    let (exposures, releases, bounds) = (Exposures::new(), Releases::new(), no_ceilings());
    let prices = prices(&[RESERVE, SIBLING]);
    let routed_by = |formed: &rutent_core_orch::snapshot::Formed, provider: &str| {
        formed.snapshot.figures.iter().any(
            |f| matches!(&f.origin, Origin::Provider { provider: p, .. } if p.as_str() == provider),
        )
    };

    let before = form(
        &catalogue,
        &clock,
        &exposures,
        &releases,
        &bounds,
        &prices,
        &[],
    );
    assert_eq!(before.at, Instant(4_000));
    assert_eq!(
        before.snapshot.taken_at.as_str(),
        Instant(4_000).rfc3339().unwrap()
    );
    assert!(routed_by(&before, RESERVE));
    assert!(offered_through(
        &route(&shortlist_request("1000"), &before.snapshot, &bounds),
        RESERVE
    ));

    // Core's clock passes the expiry. No caller hands the snapshot a time: the next one is read at the clock.
    clock.set(5_001);
    let after = form(
        &catalogue,
        &clock,
        &exposures,
        &releases,
        &bounds,
        &prices,
        &[],
    );
    assert_eq!(after.at, Instant(5_001));
    assert!(!routed_by(&after, RESERVE) && !routed_by(&after, SIBLING));
    assert!(
        after
            .left_out
            .iter()
            .all(|l| l.reason == LeftOutReason::Unusable(Unusable::NotInForce(Standing::Expired)))
    );
    assert_eq!(after.left_out.len(), 4);
    // Every price stays out with its tuple, and no route is offered.
    assert_eq!(after.prices_left_out.len(), 4);
    let record = route(&shortlist_request("1000"), &after.snapshot, &bounds);
    assert!(record.candidates().is_empty());
}
