//! A reserve's held quote, with its validity (`REQ-QTE-15`).

use std::collections::BTreeMap;

use rutent_contracts::core_internal::ShownAs;
use rutent_contracts::router_api::{Candidate, DecisionRecord, HeldQuote, Id, Timestamp};
use rutent_prov::catalogue::Instant;
use rutent_router::quote::{self, HeldQuoteRefused, HoldRefused, Label};

use super::shown::{NotShown, Shown, show};

/// One quote a reserve holds, for one order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    /// The reserve that holds it: the provider of each part's first segment.
    pub reserve: Id,
    /// The order it is held for.
    pub order: Id,
    /// The quote held, with its validity: the one stated, capped at the earliest expiry of the figures its route
    /// cites (router-api's type).
    pub quote: HeldQuote,
    /// What it shows: always a quote, never the price (`REQ-QTE-7`).
    pub shown: Shown,
}

/// Why a quote isn't held, or a held quote starts nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoldError {
    /// The router's rules refuse the hold: nothing chosen, a validity before the quote, or a record that breaks
    /// router-api's rules.
    Refused(HoldRefused),
    /// Its quote can't be shown.
    NotShown(NotShown),
    /// The reserve isn't the one each part of the route starts at.
    NotTheReserve,
    /// A hold under that identifier is kept already.
    AlreadyHeld,
    /// No hold under that identifier is kept.
    NoSuchHold,
    /// It is redeemed by another reserve, or for another order, than it was held by and for.
    NotTheHolder,
    /// It is past its validity, or holds nothing chosen: it starts nothing.
    Redeem(HeldQuoteRefused),
}

/// Core's record of the quotes reserves hold (`REQ-QTE-15`), in memory, keeping it across a restart the
/// deployment's (`F-030`). A held quote stays a quote until each of its segments starts: it binds no provider, so
/// dropping it, or its route failing to start, has no effect beyond the hold.
///
/// A hold is valid no longer than the figures its route stands on: its stated validity is capped at the earliest
/// expiry of any figure the chosen candidate cites, so a hold can't outlive the dry runs and quotes that confirmed
/// it. It is redeemed only by the reserve that holds it, for the order it is held for.
///
/// provider-role names no statement by which a reserve asks for a hold, so a hold rests on no stated entry (a gap,
/// in `F-020`'s notes, for contract round 3); whoever starts a route on a held quote redeems it here first.
#[derive(Debug, Clone, Default)]
pub struct Holds {
    held: BTreeMap<String, Held>,
}

/// When the earliest figure the candidate cites stops being usable, in whole seconds, rounded down: the record's
/// snapshot time plus what is left of that figure's validity (`REQ-QTE-4`). `None` where no figure it cites states a
/// validity.
fn earliest_expiry(record: &DecisionRecord, candidate: &Candidate) -> Option<i128> {
    let taken_at = record
        .snapshot
        .taken_at
        .unix_nanos()
        .div_euclid(1_000_000_000);
    candidate
        .parts
        .iter()
        .flat_map(|part| part.operations.iter())
        .flat_map(|operation| operation.figures.iter())
        .filter_map(|id| record.snapshot.figures.iter().find(|f| &f.figure_id == id))
        .filter_map(|figure| {
            let valid = i128::from(figure.valid_for_seconds?.get());
            let age = i128::from(figure.age_seconds.get());
            Some(taken_at + (valid - age).max(0))
        })
        .min()
}

/// `valid_until`, or the earliest expiry where that is sooner.
fn capped(valid_until: Timestamp, expiry: Option<i128>) -> Option<Timestamp> {
    let Some(expiry) = expiry else {
        return Some(valid_until);
    };
    if valid_until.unix_nanos() <= expiry * 1_000_000_000 {
        return Some(valid_until);
    }
    Timestamp::new(Instant(u64::try_from(expiry).ok()?).rfc3339()?)
}

impl Holds {
    /// No quote held.
    pub fn new() -> Self {
        Self::default()
    }

    /// Holds, under `hold`, the quote `record` chose, for `reserve` and `order`, until `valid_until` or the earliest
    /// expiry of the figures its route cites, whichever is sooner.
    pub fn hold(
        &mut self,
        hold: &Id,
        reserve: &Id,
        order: &Id,
        record: DecisionRecord,
        valid_until: Timestamp,
    ) -> Result<&Held, HoldError> {
        if self.held.contains_key(hold.as_str()) {
            return Err(HoldError::AlreadyHeld);
        }
        let candidate = record
            .chosen()
            .ok_or(HoldError::Refused(HoldRefused::NothingChosen))?;
        // Only the reserve each part of the route starts at holds its quote.
        if candidate
            .parts
            .iter()
            .any(|part| &part.operations.first().provider != reserve)
        {
            return Err(HoldError::NotTheReserve);
        }
        let valid_until = capped(valid_until, earliest_expiry(&record, candidate))
            .ok_or(HoldError::Refused(HoldRefused::ValidityBeforeQuote))?;
        let held = quote::hold(record, valid_until).map_err(HoldError::Refused)?;
        let candidate = held
            .record
            .chosen()
            .ok_or(HoldError::Refused(HoldRefused::NothingChosen))?;
        let floor = held.record.request.floor.clone();
        let shown = show(&held.record, candidate, floor.as_ref()).map_err(HoldError::NotShown)?;
        // Held, it stays a quote (`REQ-QTE-7`): `show` labels nothing the price, and the router's label agrees.
        if shown.shown_as == ShownAs::Price || quote::held_label(&held) != Label::Quote {
            return Err(HoldError::NotShown(NotShown::Unexplained));
        }
        let entry = self.held.entry(hold.as_str().to_owned()).or_insert(Held {
            reserve: reserve.clone(),
            order: order.clone(),
            quote: held,
            shown,
        });
        Ok(entry)
    }

    /// The quote held under `hold`.
    pub fn held(&self, hold: &Id) -> Option<&Held> {
        self.held.get(hold.as_str())
    }

    /// The route of the quote held under `hold`, to start at `now` by `reserve` for `order`, and what it showed:
    /// refused to anyone but the reserve that holds it, for the order it is held for, and past its validity; at its
    /// validity still usable (`REQ-QTE-15`). Redeeming changes nothing: the hold stays until dropped.
    pub fn redeem(
        &self,
        hold: &Id,
        reserve: &Id,
        order: &Id,
        now: &Timestamp,
    ) -> Result<(&Candidate, &Shown), HoldError> {
        let held = self.held(hold).ok_or(HoldError::NoSuchHold)?;
        if &held.reserve != reserve || &held.order != order {
            return Err(HoldError::NotTheHolder);
        }
        let candidate = quote::redeem(&held.quote, now).map_err(HoldError::Redeem)?;
        Ok((candidate, &held.shown))
    }

    /// Drops the hold under `hold`, with no effect beyond it; false if none was kept.
    pub fn drop_hold(&mut self, hold: &Id) -> bool {
        self.held.remove(hold.as_str()).is_some()
    }
}
