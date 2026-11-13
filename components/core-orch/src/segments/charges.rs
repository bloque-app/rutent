//! A settled segment's charge, held to the formula that bound it (`REQ-QTE-5`, `REQ-QTE-6`, `AC-14`; `T-020-14`).
//!
//! The segments are the one place that knows a settled segment's charge, its amount in, when it started and the
//! stated entry of the result that counted, so they make the check themselves: [`Segments::note_charge`] keeps what
//! the check needs when a segment settles, [`Segments::check_charge`] hands it to `quote::Charges::check` at once,
//! and every run of a part checks its settled segments again ([`Segments::check_charges`]), so a check or a raise the
//! log couldn't take is made when it can, and a segment is never raised twice. The check is made whatever the
//! provider states of billing the charge separately: it is the charge, not the billing, that the formula bounds.
//!
//! What binds a charge is read as `quote` reads it: the terms the provider stated last at or before the segment's
//! first `move_value` effect, which [`Segments::take_terms`] keeps from the provider's signed bytes; or else the fee
//! formula of the price the segment's operation cites.

use rutent_contracts::core_internal::{Entry, Receipt, VerifiedEntry};
use rutent_contracts::router_api::{Amount, FeeFormula, Id, Operation, TupleKey};

use super::{Live, Parts, Segments, SegmentsError};
use crate::quote::{
    ChargeDesk, ChargeError, Charged, Charges, Checked, TakeError, Taken, Terms, TermsDesk,
};

impl Segments {
    /// What the charge check needs of the settled `segment` of `order`: its provider's charge as its result that
    /// counted states it, what it took in, when it started, its result's stated entry and the price its route was
    /// priced on. `None` until the segment settles, and where its result states its charge in another asset than the
    /// one it takes in, which no formula of its tuple bounds.
    pub fn charged(&self, order: &Id, segment: &Id) -> Option<&Charged> {
        self.flights
            .get(&(order.as_str().to_owned(), segment.as_str().to_owned()))?
            .charged
            .as_ref()
    }

    /// The charges checked so far.
    pub fn charges(&self) -> &Charges {
        &self.charges
    }

    /// The terms the providers stated.
    pub fn terms(&self) -> &Terms {
        &self.terms
    }

    /// The register of checked charges, rebuilt from the log's `entries` after a restart (`Charges::recorded`): a
    /// charge the log holds a raise of is not raised again.
    pub fn recorded_charges(&mut self, entries: &[(Receipt, Entry)]) {
        self.charges = Charges::recorded(entries);
    }

    /// Takes a provider's signed terms, `bytes` as they arrived (`Terms::take`): kept and appended as a stated entry,
    /// the statement kept behind its commitment compared with the one handed in, then held for the segments that
    /// start after them (`REQ-QTE-6`). Nothing in Core delivers a provider's terms to this call yet: no module
    /// hands them over (`T-020-14`'s notes).
    ///
    /// # Errors
    ///
    /// If the bytes aren't terms, the log refused or failed, or the entry isn't kept as terms.
    pub fn take_terms(&mut self, desk: TermsDesk<'_>, bytes: &[u8]) -> Result<Taken, TakeError> {
        self.terms.take(desk, bytes)
    }

    /// Keeps what the check of `segment`'s charge needs, once its result counted. `stated` is the receipt of the
    /// result's stated entry; `taken` is what the result says it took in and charged.
    pub(super) fn note_charge(
        &mut self,
        live: &Live<'_>,
        at: &(String, String),
        operation: &Operation,
        price: Option<&FeeFormula>,
        stated: &Receipt,
        taken: (&str, &rutent_contracts::provider_role::Fee),
    ) -> Result<(), SegmentsError> {
        let (amount_in, fee) = taken;
        // A charge in another asset than the one the segment takes in is held to no formula of its tuple: it is noted
        // with the asset it is stated in, and the check raises it. One in an asset the operation names at neither
        // end can't be stated in the contract's raise: the part stops rather than let it pass (`REQ-QTE-5`).
        let charged_in = if fee.asset == operation.from.asset.id {
            None
        } else if fee.asset == operation.to.asset.id {
            Some(operation.to.asset.clone())
        } else {
            return Err(SegmentsError::Plan(
                "a counted result states its charge in an asset of its segment",
            ));
        };
        let advertised = price
            .ok_or(SegmentsError::Plan("an operation cites a price"))?
            .clone();
        let flight = self.flight(at)?;
        // A segment starts at its first `move_value` effect: the terms stated last at or before it bind it.
        let started_at = match flight.steps.get("effect").map(|s| s.entry.entry()) {
            Some(Entry::Verified(VerifiedEntry { when, .. })) => when.at.clone(),
            _ => return Err(SegmentsError::Plan("an effect is a verified entry")),
        };
        let (Some(amount_in), Some(charged)) = (
            Amount::new(amount_in.to_owned()),
            Amount::new(fee.amount.clone()),
        ) else {
            return Err(SegmentsError::Plan("a counted result states its amounts"));
        };
        let charged = Charged {
            order: live.order.clone(),
            segment: Id::new(&at.1).ok_or(SegmentsError::Plan("a segment is an id"))?,
            provider: operation.provider.clone(),
            tuple: TupleKey {
                provider: operation.provider.clone(),
                from: operation.from.clone(),
                to: operation.to.clone(),
            },
            started_at,
            amount_in,
            charged,
            charged_in,
            advertised,
            result: stated.clone(),
        };
        self.flight_mut(at)?.charged = Some(charged);
        Ok(())
    }

    /// Checks the charge of `at` once it is noted: raised to the operator where it is above its formula, and the
    /// raise recorded. Nothing where none is noted; a charge checked already isn't raised again.
    pub(super) fn check_charge(
        &mut self,
        parts: &mut Parts<'_>,
        live: &Live<'_>,
        at: &(String, String),
    ) -> Result<(), SegmentsError> {
        let Some(charged) = self.flights.get(at).and_then(|f| f.charged.clone()) else {
            return Ok(());
        };
        let desk = ChargeDesk {
            terms: &self.terms,
            log: &mut *parts.log,
            clock: parts.clock,
            fresh: &mut *parts.fresh,
            operator: &mut *parts.channel,
            core: live.core,
        };
        match self.charges.check(desk, &charged) {
            Ok(Checked::Within | Checked::Flagged(_) | Checked::AlreadyRaised) => Ok(()),
            Err(ChargeError::Append(failure)) => Err(SegmentsError::Append(failure)),
            Err(other) => Err(SegmentsError::Charge(other)),
        }
    }

    /// Checks every settled segment of the part (`Segments::check_charge`), each one even where an earlier one
    /// couldn't be checked, and answers the first failure.
    pub(super) fn check_charges(
        &mut self,
        parts: &mut Parts<'_>,
        live: &Live<'_>,
    ) -> Result<(), SegmentsError> {
        let mut first = Ok(());
        for segment in live.segments {
            let at = (live.order.as_str().to_owned(), segment.as_str().to_owned());
            if let Err(failure) = self.check_charge(parts, live, &at)
                && first.is_ok()
            {
                first = Err(failure);
            }
        }
        first
    }
}
