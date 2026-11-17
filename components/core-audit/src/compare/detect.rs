//! The detectors: [`Compare`] records each comparison and raises a difference to the operator, and does nothing
//! else (`REQ-ORD-9`, `REQ-AUD-15`, `INV-1`, ADR-0009).
//!
//! What it can reach is what its type parameters give it, and no more:
//!
//! - a [`General`] handle on the log, which implements [`Log`] alone: it writes no decision entry and, since its
//!   entries reach the log unverified, attributes no statement (`T-022-01`);
//! - an [`Operator`], whose `raise` takes a [`Raise`], a difference and the
//!   entries it rests on, and answers only whether it was delivered;
//! - a [`Clock`], which reads the time.
//!
//! It holds no handle on the state machine, the provider system, the ledger or `core-orch`, which this crate doesn't
//! import (`DEC-2`), so no call it makes can move value or change an order. The comparisons themselves are the pure
//! functions of [`super::report`], [`super::holding`] and [`super::evidenced`].

use rutent_contracts::core_internal::calls::{
    AppendFailure, Comparisons, Delivery, GeneralEntry, Log, Operator,
};
use rutent_contracts::core_internal::{
    Actor, ActorKind, Difference, Entry, HoldingReading, Outcome, Raise, Receipt, ReportPair, Role,
    TimeSource, VerifiedEntry, VerifiedKind, Violation, What, When,
};
use rutent_contracts::router_api::{Id, Timestamp};

use super::evidenced::{Evidenced, value_evidenced_against_balances};
use super::holding::{HoldingVerdict, check_holding};
use super::report::{ReportsVerdict, compare_reports};
use crate::log::merkle::sha256;
use crate::log::{Clock, General};

/// The refusal to build an entry the contract's handles refuse.
pub const NOT_A_GENERAL_ENTRY: &str = "a comparison's record is an entry of the general handle";

/// The checks of `CMP-CORE-AUDIT` that only detect. See the [module](self).
pub struct Compare<L: Log, O: Operator, C: Clock> {
    actor: Actor,
    log: General<L>,
    operator: O,
    clock: C,
}

impl<L: Log, O: Operator, C: Clock> Compare<L, O, C> {
    /// The checks, writing as `actor` to `log` and raising to `operator`, on `clock`.
    pub fn new(actor: Actor, log: General<L>, operator: O, clock: C) -> Self {
        Compare {
            actor,
            log,
            operator,
            clock,
        }
    }

    /// The actor the checks write as: `CMP-CORE-AUDIT`, one of Core's own parts, for the deployment's `party`.
    pub fn core_audit(party: Id) -> Actor {
        Actor {
            party,
            acts: Id::new("core-audit").expect("a valid identifier"),
            role: Role::Core,
            kind: ActorKind::Component,
        }
    }

    /// Core's clock, now.
    fn now(&self) -> Result<Timestamp, AppendFailure> {
        timestamp(self.clock.unix_nanos()).ok_or(AppendFailure::Unavailable)
    }

    /// Appends a verified entry of the checks, `what` with `outcome`, at `at` by Core's clock.
    fn record(
        &mut self,
        what: What,
        outcome: Outcome,
        at: &Timestamp,
    ) -> Result<Receipt, AppendFailure> {
        let entry = Entry::Verified(VerifiedEntry {
            kind: VerifiedKind::Verified,
            who: self.actor.clone(),
            what,
            when: When {
                at: at.clone(),
                source: TimeSource::CoreClock,
            },
            outcome,
        });
        let entry = GeneralEntry::try_from(entry)
            .map_err(|_| AppendFailure::Refused(Violation(NOT_A_GENERAL_ENTRY)))?;
        self.log.append(&entry)
    }

    /// Raises `difference`, resting on `cites`, to the operator, and records the raise and whether it was delivered.
    /// The raise's id is a digest of what it rests on, the time and the difference, so no two raises share one.
    fn raise(
        &mut self,
        difference: Difference,
        mut cites: Vec<Receipt>,
        at: &Timestamp,
    ) -> Result<(), AppendFailure> {
        let mut seen = Vec::new();
        cites.retain(|c| {
            let new = !seen.contains(c);
            seen.push(c.clone());
            new
        });
        let basis = serde_json::to_vec(&(&cites, at, &difference))
            .map_err(|_| AppendFailure::Refused(Violation("a raise's basis has a JSON form")))?;
        let digest = sha256(&basis);
        let id: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        let raise = Raise {
            raise: Id::new(format!("raise-{id}")).expect("a valid identifier"),
            difference,
            cites,
            at: at.clone(),
        };
        raise.check().map_err(AppendFailure::Refused)?;
        let outcome = match self.operator.raise(&raise) {
            Delivery::Delivered => Outcome::Done {},
            Delivery::Undelivered => Outcome::Failed {},
        };
        // A raise that couldn't be recorded is answered unavailable, so its writer appends the comparison again.
        self.record(What::Raise { raise }, outcome, at)
            .map(|_| ())
            .map_err(|_| AppendFailure::Unavailable)
    }

    /// Records the check of `reading`, without its balance (`REQ-AUD-6`), and answers its receipt.
    fn record_check(
        &mut self,
        reading: &HoldingReading,
        at: &Timestamp,
    ) -> Result<Receipt, AppendFailure> {
        self.record(
            What::HoldingCheck {
                account: reading.account.clone(),
                provider: reading.provider.clone(),
                node: reading.node.clone(),
                word: reading.word,
                orders: reading.held_for.iter().map(|h| h.order.clone()).collect(),
            },
            Outcome::Observed {},
            at,
        )
    }

    /// Across orders, compares the value evidenced at each node with its holding account's balance (`INV-1`). For
    /// each account that differs it records the check and raises the difference, as a single reading's check does,
    /// and changes nothing else; an account that can't be compared raises nothing. It answers the receipts of the
    /// checks it recorded.
    pub fn evidenced_against_balances(
        &mut self,
        readings: &[HoldingReading],
        evidenced: &[Evidenced],
    ) -> Result<Vec<Receipt>, AppendFailure> {
        let at = self.now()?;
        let findings = value_evidenced_against_balances(readings, evidenced);
        let mut receipts = Vec::new();
        // The findings of the readings come first, in the readings' order.
        for (finding, reading) in findings.into_iter().zip(readings) {
            if let HoldingVerdict::Differs(difference) = finding.verdict {
                let check = self.record_check(reading, &at)?;
                self.raise(difference, vec![check.clone()], &at)?;
                receipts.push(check);
            }
        }
        Ok(receipts)
    }
}

impl<L: Log, O: Operator, C: Clock> Comparisons for Compare<L, O, C> {
    fn report_pair(&mut self, pair: &ReportPair) -> Result<Receipt, AppendFailure> {
        let at = self.now()?;
        let receipt = self.record(
            What::ReportPair { pair: pair.clone() },
            Outcome::Observed {},
            &at,
        )?;
        if let ReportsVerdict::Differs(difference) = compare_reports(pair) {
            let cites = vec![
                receipt.clone(),
                pair.sent.statement.clone(),
                pair.received.statement.clone(),
            ];
            self.raise(difference, cites, &at)?;
        }
        Ok(receipt)
    }

    fn holding_reading(&mut self, reading: &HoldingReading) -> Result<Receipt, AppendFailure> {
        let at = self.now()?;
        let receipt = self.record_check(reading, &at)?;
        if let HoldingVerdict::Differs(difference) = check_holding(reading) {
            self.raise(difference, vec![receipt.clone()], &at)?;
        }
        Ok(receipt)
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

#[cfg(test)]
mod tests {
    use super::timestamp;

    /// A time round-trips through the contract's own parse of a timestamp.
    // verifies: REQ-AUD-1
    #[test]
    fn a_clock_reading_is_a_timestamp_of_the_same_second() {
        for text in [
            "1970-01-01T00:00:00Z",
            "2000-02-29T23:59:59Z",
            "2026-11-10T09:00:00Z",
            "2100-03-01T00:00:01Z",
            "1969-12-31T23:59:59Z",
        ] {
            let nanos = rutent_contracts::router_api::Timestamp::new(text)
                .unwrap()
                .unix_nanos();
            assert_eq!(timestamp(nanos + 999_999_999).unwrap().as_str(), text);
        }
    }
}
