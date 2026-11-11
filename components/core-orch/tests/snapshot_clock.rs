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

// verifies: REQ-CAT-2
#[test]
fn grant_is_honoured_neither_before_its_start_nor_at_its_expiry() {
    // Granted from 1 000 until 5 000.
    let catalogue = catalogue(&[(RESERVE, "acme")], 5_000);
    let (exposures, releases, bounds) = (Exposures::new(), Releases::new(), no_ceilings());
    let prices = prices(&[RESERVE]);
    let clock = TestClock::at(0);
    let routed_at = |seconds: u64, nanos: u32| {
        clock.set_within(seconds, nanos);
        let formed = form(
            &catalogue,
            &clock,
            &exposures,
            &releases,
            &bounds,
            &prices,
            &[],
        );
        formed.snapshot.figures.iter().any(
            |f| matches!(&f.origin, Origin::Provider { provider, .. } if provider.as_str() == RESERVE),
        )
    };
    // Half a second before it starts: the next whole second is in force, the moment isn't.
    assert!(!routed_at(999, 500_000_000));
    assert!(routed_at(1_000, 0));
    assert!(routed_at(4_999, 0));
    // Inside its last second the moment is in force, the next whole second isn't: out.
    assert!(!routed_at(4_999, 500_000_000));
    // At exactly its expiry: out.
    assert!(!routed_at(5_000, 0));
}

// verifies: REQ-CAT-2
#[test]
fn a_reading_inside_a_second_counts_as_both_whole_seconds_around_it() {
    use rutent_core_orch::snapshot::{Clock, Reading, SystemClock};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    let within = Reading::of(Duration::new(5, 1));
    assert_eq!((within.floor(), within.ceil()), (Instant(5), Instant(6)));
    let whole = Reading::of(Duration::new(5, 0));
    assert_eq!((whole.floor(), whole.ceil()), (Instant(5), Instant(5)));
    assert_eq!(within.timestamp().unwrap().as_str(), "1970-01-01T00:00:06Z");

    let before = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let read = SystemClock.now();
    let after = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    assert!(before.as_secs() <= read.floor().0 && read.floor().0 <= after.as_secs());
    assert!(read.ceil().0 <= after.as_secs() + 1);
    assert_eq!(read.ceil().0 - read.floor().0, u64::from(read.nanos > 0));
}
