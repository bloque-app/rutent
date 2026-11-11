//! Core's own clock (`F-012`'s notes): the one time Core passes as `at` to the catalogue, and to every check that
//! reads a time.
//!
//! The catalogue holds no clock, so whoever calls it chooses the time, and a stale one would honour an expired
//! grant (`REQ-CAT-2`). Core's guard is that no formation takes a time from its caller: [`super::Snapshots`] reads
//! the [`Clock`] it was built with, once per snapshot, and uses that one reading for the catalogue's snapshot, for
//! every statement's key check and for the snapshot's `taken_at`.

use rutent_prov::catalogue::Instant;

/// Core's clock: the moment now, in whole seconds since the Unix epoch.
pub trait Clock {
    /// The moment now. A clock that would round must round up, so that a grant's expiry is never read late.
    fn now(&self) -> Instant;
}

/// The host's clock, read through the standard library: a part of a second counts as the next second, so the
/// reading is never earlier than the moment.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        let since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let part = u64::from(since.subsec_nanos() > 0);
        Instant(since.as_secs().saturating_add(part))
    }
}
