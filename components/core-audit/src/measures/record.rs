//! The writer of the measure records: [`Record`] appends each [`MeasureRecord`] orchestration hands over to the log,
//! through a general handle, and does nothing else (`REQ-AUD-7`, `T-022-06`).
//!
//! What it can reach is what its type parameters give it: a [`General`] handle on the log, which implements [`Log`]
//! alone (no decision entry, no statement attributed), and a [`Clock`]. It holds no handle on the state machine, the
//! provider system, the ledger or `core-orch`, so nothing it does can move value or change an order. A measure is
//! computed later, from what the log holds ([`super::measure`]), never from what a caller says at the time.

use rutent_contracts::core_internal::calls::{AppendFailure, GeneralEntry, Log, Measures};
use rutent_contracts::core_internal::{
    Actor, ActorKind, Entry, MeasureRecord, Outcome, Receipt, Role, TimeSource, VerifiedEntry,
    VerifiedKind, Violation, What, When,
};
use rutent_contracts::router_api::{Id, Timestamp};

use crate::log::{Clock, General};

/// The refusal to build an entry the contract's general handle refuses.
pub const NOT_A_GENERAL_ENTRY: &str = "a measure record is an entry of the general handle";

/// `CMP-CORE-AUDIT`'s writer of measure records.
pub struct Record<L: Log, C: Clock> {
    actor: Actor,
    log: General<L>,
    clock: C,
}

impl<L: Log, C: Clock> Record<L, C> {
    /// The writer, recording as `actor` to `log`, on `clock`.
    pub fn new(actor: Actor, log: General<L>, clock: C) -> Self {
        Record { actor, log, clock }
    }

    /// The actor the measures write as: `CMP-CORE-AUDIT`, one of Core's own parts, for the deployment's `party`.
    pub fn core_audit(party: Id) -> Actor {
        Actor {
            party,
            acts: Id::new("core-audit").expect("a valid identifier"),
            role: Role::Core,
            kind: ActorKind::Component,
        }
    }
}

impl<L: Log, C: Clock> Measures for Record<L, C> {
    /// Records `record`, which must cite what it rests on (at least one entry, each once), at Core's clock, and
    /// answers the entry's receipt. The log holds each cited entry to exist (`measure-cites-entries-of-the-log`).
    fn measure(&mut self, record: &MeasureRecord) -> Result<Receipt, AppendFailure> {
        record.check().map_err(AppendFailure::Refused)?;
        let at = timestamp(self.clock.unix_nanos()).ok_or(AppendFailure::Unavailable)?;
        let entry = Entry::Verified(VerifiedEntry {
            kind: VerifiedKind::Verified,
            who: self.actor.clone(),
            what: What::Measure {
                record: record.clone(),
            },
            when: When {
                at,
                source: TimeSource::CoreClock,
            },
            outcome: Outcome::Observed {},
        });
        let entry = GeneralEntry::try_from(entry)
            .map_err(|_| AppendFailure::Refused(Violation(NOT_A_GENERAL_ENTRY)))?;
        self.log.append(&entry)
    }
}

/// The RFC 3339 UTC time `nanos` after the epoch, to the second, or `None` where it isn't a time a timestamp states.
fn timestamp(nanos: i128) -> Option<Timestamp> {
    let seconds = nanos.div_euclid(1_000_000_000);
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    // Howard Hinnant's civil-from-days, on days since 1970-01-01.
    let z = i64::try_from(days).ok()?.checked_add(719_468)?;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    if !(0..=9999).contains(&year) {
        return None;
    }
    Timestamp::new(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest / 60 % 60,
        rest % 60
    ))
}
