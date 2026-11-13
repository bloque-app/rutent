//! A reserve's held quote, with its validity (`REQ-QTE-15`).

use std::collections::BTreeMap;

use rutent_contracts::core_internal::{Attribution, Receipt, ShownAs, StatedEntry};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{
    HOLD_SHAPE, HeldFor, Hold, RedemptionRefused, Statement, check_redemption,
};
use rutent_contracts::router_api::{
    Candidate, DecisionRecord, HeldQuote, Id, Timestamp, Violation,
};
use rutent_prov::catalogue::Instant;
use rutent_router::quote::{self, HeldQuoteRefused, HoldRefused, Label};

use super::shown::{NotShown, Shown, show};

/// One quote a reserve holds.
///
/// The quote itself is private: a hold is redeemed only through [`Holds::redeem`], which checks that only the
/// holding reserve, for the order its signed hold names, redeems it, once. Nothing else hands out the [`HeldQuote`],
/// so it can't be passed to the router's `quote::redeem` around that check.
///
/// ```compile_fail,E0616
/// use rutent_core_orch::quote::Held;
///
/// fn around_the_check(held: &Held) {
///     let _ = &held.quote;
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    /// The reserve that holds it: the provider of each part's first segment, and the hold's maker.
    pub reserve: Id,
    /// The hold's own transaction, its id: the hold is kept once under it, however many times it is stated.
    pub transaction: Id,
    /// The receipt of the hold's stated entry, which an acceptance redeeming it cites (`What::Acceptance.held`).
    pub hold: Receipt,
    /// The reserve's signed hold, as it made it: it names the order, or the session, that may redeem it.
    signed: Statement<Hold>,
    /// The quote held, with its validity: the one stated, capped at the earliest expiry of the figures its route
    /// cites (router-api's type).
    quote: HeldQuote,
    /// What it shows: always a quote, never the price (`REQ-QTE-7`).
    pub shown: Shown,
    /// The order that redeemed it, once one has: no other does (`hold-is-redeemed-by-one-order`).
    redeemed_by: Option<Id>,
}

impl Held {
    /// Until when the quote is valid: the one stated, capped at the earliest expiry of the figures its route cites.
    pub fn valid_until(&self) -> &Timestamp {
        &self.quote.valid_until
    }

    /// What the hold is for: the order, or the session, that may redeem it.
    pub fn held_for(&self) -> &HeldFor {
        &self.signed.statement.held_for
    }
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
    /// The entry isn't the stated entry of a hold: another shape, a signature that didn't verify, another maker than
    /// the statement's, or not about its own transaction and no order.
    NotAHold,
    /// The signed hold isn't a hold of this quote: it names another record, another candidate, or another maker than
    /// the reserve each part starts at, or a validity past its figures' or short of the quote's
    /// (router-api's `HeldQuote::check_hold`).
    NotThisQuotesHold(Violation),
    /// A hold under that transaction is kept already.
    AlreadyHeld,
    /// No hold under that transaction is kept.
    NoSuchHold,
    /// It is redeemed by another reserve than it was held by.
    NotTheHolder,
    /// It is for another order, or for a session the redeeming order wasn't accepted in (provider-role's
    /// `check_redemption`).
    NotForThisOrder(RedemptionRefused),
    /// Another order redeemed it already: a hold is redeemed for one order (`hold-is-redeemed-by-one-order`).
    RedeemedByAnother,
    /// It is past its validity, or holds nothing chosen: it starts nothing.
    Redeem(HeldQuoteRefused),
}

/// A hold redeemed: the route it starts, what it showed, and the receipt of the hold's stated entry, which the
/// acceptance of the order that redeems it cites as `held`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Redeemed<'a> {
    /// The route held.
    pub candidate: &'a Candidate,
    /// What it showed.
    pub shown: &'a Shown,
    /// The receipt of the hold's stated entry.
    pub hold: &'a Receipt,
}

/// A reserve's signed hold, and the stated entry that holds it in the log.
#[derive(Debug, Clone, Copy)]
pub struct Signed<'a> {
    /// The hold as the reserve made it.
    pub hold: &'a Statement<Hold>,
    /// Its stated entry, as the log attributed it.
    pub stated: &'a StatedEntry,
    /// The receipt of that entry.
    pub receipt: &'a Receipt,
}

/// The decision record a hold is of, as Core's log names it: its id and its digest (core-internal's `decision_record`
/// entry), which the hold names, and Core computes.
#[derive(Debug, Clone, Copy)]
pub struct RecordedAs<'a> {
    /// The record's id.
    pub id: &'a Id,
    /// Its digest.
    pub digest: &'a Digest,
}

/// Core's record of the quotes reserves hold (`REQ-QTE-15`), in memory, keeping it across a restart the
/// deployment's (`F-030`). A held quote stays a quote until each of its segments starts: it binds no provider, so
/// dropping it, or its route failing to start, has no effect beyond the hold.
///
/// A hold is the reserve's own signed statement (provider-role's `Hold`), kept and appended as a stated entry about
/// its own transaction and no order, which is what the hold is kept under here as in the log. A hold is valid no
/// longer than the figures its route stands on: the quote's stated validity is capped at the earliest expiry of any
/// figure the chosen candidate cites ([`DecisionRecord::earliest_expiry`]), and a hold stating more than that is
/// refused ([`HeldQuote::check_hold`]). It is redeemed only by the reserve that holds it, for the order, or an
/// order accepted in the session, its signed hold names, and by one order.
///
/// What isn't built is the caller that accepts an order on a held quote: intake accepts a quote it offered and
/// redeems no hold, so `What::Acceptance.held` is `None` until it does, and the receipt [`Redeemed::hold`] is what it
/// will cite.
#[derive(Debug, Clone, Default)]
pub struct Holds {
    held: BTreeMap<String, Held>,
}

/// `valid_until`, or the earliest expiry where that is sooner, to the whole second below it.
fn capped(valid_until: Timestamp, expiry: Option<i128>) -> Option<Timestamp> {
    let Some(expiry) = expiry else {
        return Some(valid_until);
    };
    if valid_until.unix_nanos() <= expiry {
        return Some(valid_until);
    }
    Timestamp::new(Instant(u64::try_from(expiry.div_euclid(1_000_000_000)).ok()?).rfc3339()?)
}

impl Holds {
    /// No quote held.
    pub fn new() -> Self {
        Self::default()
    }

    /// Holds the quote `record`, recorded as `recorded`, chose, on the reserve's `signed` hold, until `valid_until`
    /// or the earliest expiry of the figures its route cites, whichever is sooner. The hold must be the stated
    /// entry's own, of this record's chosen candidate, made by the reserve each part starts at, valid no longer than
    /// those figures and no shorter than the quote.
    pub fn hold(
        &mut self,
        signed: Signed<'_>,
        recorded: RecordedAs<'_>,
        record: DecisionRecord,
        valid_until: Timestamp,
    ) -> Result<&Held, HoldError> {
        let statement = signed.hold;
        let Attribution::Verified { who } = &signed.stated.attribution else {
            return Err(HoldError::NotAHold);
        };
        let transaction =
            Id::new(statement.statement.transaction.as_str()).ok_or(HoldError::NotAHold)?;
        let about = signed.stated.about.as_ref();
        if signed.stated.what.statement.as_str() != HOLD_SHAPE
            || who.acts.as_str() != statement.maker
            || about.and_then(|a| a.transaction.as_ref()) != Some(&transaction)
            || about.is_some_and(|a| a.order.is_some())
        {
            return Err(HoldError::NotAHold);
        }
        if self.held.contains_key(transaction.as_str()) {
            return Err(HoldError::AlreadyHeld);
        }
        let reserve = Id::new(statement.maker.as_str()).ok_or(HoldError::NotAHold)?;
        let candidate = record
            .chosen()
            .ok_or(HoldError::Refused(HoldRefused::NothingChosen))?;
        // Only the reserve each part of the route starts at holds its quote.
        if candidate
            .parts
            .iter()
            .any(|part| part.operations.first().provider != reserve)
        {
            return Err(HoldError::NotTheReserve);
        }
        let valid_until = capped(valid_until, record.earliest_expiry(candidate))
            .ok_or(HoldError::Refused(HoldRefused::ValidityBeforeQuote))?;
        let held = quote::hold(record, valid_until).map_err(HoldError::Refused)?;
        held.check_hold(statement, recorded.id, recorded.digest)
            .map_err(HoldError::NotThisQuotesHold)?;
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
        let entry = self
            .held
            .entry(transaction.as_str().to_owned())
            .or_insert(Held {
                reserve,
                transaction,
                hold: signed.receipt.clone(),
                signed: statement.clone(),
                quote: held,
                shown,
                redeemed_by: None,
            });
        Ok(entry)
    }

    /// The quote held under the hold's `transaction`.
    pub fn held(&self, transaction: &Id) -> Option<&Held> {
        self.held.get(transaction.as_str())
    }

    /// The route of the quote held under the hold's `transaction`, to start at `now` by `reserve` for `order`,
    /// accepted in `session` where it was, and what it showed: refused to anyone but the reserve that holds it, for
    /// an order its signed hold doesn't name (or a session the order wasn't accepted in), after another order
    /// redeemed it, and past its validity; at its validity still usable (`REQ-QTE-15`). The same order redeems it
    /// again. The hold stays until dropped.
    pub fn redeem(
        &mut self,
        transaction: &Id,
        reserve: &Id,
        order: &Id,
        session: Option<&Id>,
        now: &Timestamp,
    ) -> Result<Redeemed<'_>, HoldError> {
        let held = self
            .held
            .get_mut(transaction.as_str())
            .ok_or(HoldError::NoSuchHold)?;
        if &held.reserve != reserve {
            return Err(HoldError::NotTheHolder);
        }
        check_redemption(&held.signed, order.as_str(), session.map(Id::as_str))
            .map_err(HoldError::NotForThisOrder)?;
        if held
            .redeemed_by
            .as_ref()
            .is_some_and(|first| first != order)
        {
            return Err(HoldError::RedeemedByAnother);
        }
        quote::redeem(&held.quote, now).map_err(HoldError::Redeem)?;
        held.redeemed_by = Some(order.clone());
        let held = &*held;
        let candidate = quote::redeem(&held.quote, now).map_err(HoldError::Redeem)?;
        Ok(Redeemed {
            candidate,
            shown: &held.shown,
            hold: &held.hold,
        })
    }

    /// Drops the hold under `transaction`, with no effect beyond it; false if none was kept.
    pub fn drop_hold(&mut self, transaction: &Id) -> bool {
        self.held.remove(transaction.as_str()).is_some()
    }
}
