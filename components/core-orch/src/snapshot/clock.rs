//! Core's own clock (`F-012`'s notes): the one time Core passes as `at` to the catalogue, and to every check and
//! record that reads a time.
//!
//! The catalogue holds no clock, so whoever calls it chooses the time, and a stale one would honour an expired
//! grant (`REQ-CAT-2`). Core's guard is that nothing in this module takes a time from its caller: each function
//! that needs one reads the [`Clock`] it is handed, and [`super::Snapshots`] reads its clock once per snapshot and
//! uses that one reading throughout. The assembly hands Core [`SystemClock`]; tests hand it a clock they set.
//!
//! The catalogue counts whole seconds, and a reading may fall inside one. A [`Reading`] keeps both whole seconds
//! around the moment, so a grant is honoured only when it is in force at both: never before its `granted_at`, and
//! never at or after its `expires_at`.

use std::time::Duration;

use rutent_contracts::router_api::Timestamp;
use rutent_prov::catalogue::Instant;

/// One reading of Core's clock: the moment, to the nanosecond, since the Unix epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    /// Whole seconds.
    pub seconds: u64,
    /// The part of a second, in nanoseconds, below 1 000 000 000.
    pub nanos: u32,
}

impl Reading {
    /// A reading at a whole second.
    pub fn at(seconds: u64) -> Self {
        Self { seconds, nanos: 0 }
    }

    /// A reading of a time since the epoch.
    pub fn of(since_epoch: Duration) -> Self {
        Self {
            seconds: since_epoch.as_secs(),
            nanos: since_epoch.subsec_nanos(),
        }
    }

    /// The whole second at or before the moment.
    pub fn floor(self) -> Instant {
        Instant(self.seconds)
    }

    /// The whole second at or after the moment: a part of a second counts as the next second.
    pub fn ceil(self) -> Instant {
        Instant(self.seconds.saturating_add(u64::from(self.nanos > 0)))
    }

    /// The reading as a timestamp, at its [`Self::ceil`], so a record is never stamped before its moment; `None`
    /// past the year 9999.
    pub fn timestamp(self) -> Option<Timestamp> {
        Timestamp::new(self.ceil().rfc3339()?)
    }
}

/// Core's clock.
pub trait Clock {
    /// The moment now.
    fn now(&self) -> Reading;
}

/// The host's clock, read through the standard library.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Reading {
        Reading::of(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default(),
        )
    }
}
