//! A reserve's held quote, with its validity (`REQ-QTE-15`).

use std::collections::BTreeMap;

use rutent_contracts::core_internal::calls::{AppendFailure, Evidence, KeptStatements, Log};
use rutent_contracts::core_internal::{
    About, Actor, Attribution, Entry, Receipt, ShownAs, StatedEntry, What,
};
use rutent_contracts::module_interface::{Digest, DigestAlgorithm};
use rutent_contracts::provider_role::{
    HOLD_SHAPE, HeldFor, Hold, RedemptionRefused, Statement, check_redemption,
};
use rutent_contracts::router_api::{
    Candidate, DecisionRecord, HeldQuote, Id, Timestamp, Violation,
};
use rutent_prov::catalogue::{Catalogue, Instant};
use rutent_router::quote::{self, HeldQuoteRefused, HoldRefused, Label};

use super::journal;
use super::shown::{NotShown, Shown, show};
use super::terms::{Recorded, from_base64url};
use crate::segments::{self, Unappended};
use crate::snapshot::Clock;

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
    /// The commitment the hold's stated entry names its bytes by: what the statement kept behind it is held to
    /// before the hold is redeemed.
    commitment: Digest,
    /// The reserve's signed hold, as it made it: it names the order, or the session, that may redeem it.
    signed: Statement<Hold>,
    /// The quote held, with its validity: the one stated, capped at the earliest expiry of the figures its route
    /// cites (router-api's type).
    quote: HeldQuote,
    /// What it shows: always a quote, never the price (`REQ-QTE-7`).
    pub shown: Shown,
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

    /// The decision record whose chosen candidate is held, by its id.
    pub fn record(&self) -> &Id {
        self.quote.record.record_id()
    }

    /// The candidate held: the record's chosen one.
    pub fn candidate(&self) -> Option<&Id> {
        self.quote.record.chosen().map(|c| &c.candidate_id)
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
    /// It is not a hold of the quote the acceptance is of: another decision record, or another candidate of it.
    NotThisQuote,
    /// The evidence store keeps nothing behind the hold's entry's commitment, or what it keeps does not hash to that
    /// commitment, or is not the signed hold Core holds: the entry rests on other bytes, so the hold rests on
    /// nothing the log holds (`REQ-AUD-2`).
    NotWhatIsKept,
    /// The evidence store could not be read now: nothing is redeemed on a hold that could not be held to its bytes.
    KeptUnavailable(AppendFailure),
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

/// The quote an acceptance is of, by the decision record and the candidate the customer was shown: a hold is
/// redeemed only for a quote it holds.
#[derive(Debug, Clone, Copy)]
pub struct AcceptedQuote<'a> {
    /// The decision record that chose the route.
    pub record: &'a Id,
    /// The candidate chosen.
    pub candidate: &'a Id,
}

/// A redemption asked: every field is what the caller verified, never a word a customer gave.
#[derive(Debug, Clone, Copy)]
pub struct Redemption<'a> {
    /// The hold's transaction, which the acceptance names it by.
    pub transaction: &'a Id,
    /// The reserve redeeming it: the order's origin reserve.
    pub reserve: &'a Id,
    /// The order the acceptance is of.
    pub order: &'a Id,
    /// The session the acceptance is given in, the one the caller verified for it.
    pub session: Option<&'a Id>,
    /// The quote the acceptance is of.
    pub quote: AcceptedQuote<'a>,
    /// When it is redeemed, by Core's clock.
    pub now: &'a Timestamp,
}

/// What taking a reserve's signed hold reaches.
pub struct HoldDesk<'a> {
    /// The evidence store, which keeps the bytes before their entry is appended.
    pub evidence: &'a mut dyn Evidence,
    /// The log, through orchestration's handle.
    pub log: &'a mut dyn Log,
    /// The log's recorded entries.
    pub recorded: &'a dyn Recorded,
    /// Core's clock, which stamps when the hold was received.
    pub clock: &'a dyn Clock,
    /// The catalogue, which names the party a provider states for.
    pub catalogue: &'a Catalogue,
    /// Who handed the hold to Core, as external authentication established it: named in the entry as its relay where
    /// it isn't the hold's maker (`REQ-AUD-2`).
    pub presented_by: &'a Actor,
}

/// Why a reserve's hold wasn't taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TakeHoldError {
    /// The bytes aren't a reserve's signed hold in an envelope.
    Unreadable,
    /// The evidence store or the log refused or failed.
    Append(AppendFailure),
    /// The log holds no entry at the receipt it answered.
    NotRecorded,
    /// The hold isn't kept: why.
    NotHeld(HoldError),
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
/// Intake redeems the hold an acceptance names (`Intake::accept_holding`), and cites the receipt [`Redeemed::hold`] as
/// the acceptance's `held`. The holds themselves stay in memory, the deployment's to keep across a restart (`F-030`);
/// which order redeemed which is rebuilt from the log ([`Holds::recorded`]).
#[derive(Debug, Clone, Default)]
pub struct Holds {
    held: BTreeMap<String, Held>,
    /// The order that redeemed each hold, by the hold's transaction, once one has: no other does
    /// (`hold-is-redeemed-by-one-order`). Kept apart from the holds so it is rebuilt from the log's acceptances alone
    /// ([`Holds::recorded`]): after a restart a hold kept again meets the order that redeemed it.
    redeemed: BTreeMap<String, Id>,
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
                commitment: signed.stated.commitment.clone(),
                signed: statement.clone(),
                quote: held,
                shown,
            });
        Ok(entry)
    }

    /// Takes a reserve's signed hold, `bytes` as they arrived: keeps them in the evidence store, appends them as a stated
    /// entry about the hold's own transaction and no order (the log attributes it), reads the entry back as the log
    /// recorded it, and holds the quote `record`, recorded as `recorded`, chose on it ([`Holds::hold`]). A hold of
    /// which the stated entry is not about its own transaction is refused there. The statement kept behind the
    /// entry's commitment is held to it when the hold is redeemed ([`Holds::redeem`]), never taken on the caller's
    /// word.
    ///
    /// Nothing in Core hands a reserve's hold to this call yet: no module delivers them (`T-020-15`'s notes).
    ///
    /// # Errors
    ///
    /// If the bytes aren't a hold, the log or the store refused or failed, or the hold isn't kept.
    pub fn take(
        &mut self,
        desk: HoldDesk<'_>,
        bytes: &[u8],
        recorded: RecordedAs<'_>,
        record: DecisionRecord,
        valid_until: Timestamp,
    ) -> Result<&Held, TakeHoldError> {
        let statement: Statement<Hold> =
            serde_json::from_slice(bytes).map_err(|_| TakeHoldError::Unreadable)?;
        let transaction =
            Id::new(statement.statement.transaction.as_str()).ok_or(TakeHoldError::Unreadable)?;
        let received_at = journal::now(desk.clock).map_err(TakeHoldError::Append)?;
        let entry = segments::journal::stated_entry(
            desk.evidence,
            &received_at,
            bytes,
            segments::maker(desk.catalogue, bytes),
            desk.presented_by,
            Some(About {
                order: None,
                segment: None,
                transaction: Some(transaction),
            }),
        )
        .map_err(|why| match why {
            Unappended::Unrecordable => TakeHoldError::Unreadable,
            Unappended::Append(failure) => TakeHoldError::Append(failure),
        })?;
        let receipt = journal::built_stated(desk.log, entry).map_err(TakeHoldError::Append)?;
        let Some(Entry::Stated(stated)) = desk.recorded.entry(&receipt) else {
            return Err(TakeHoldError::NotRecorded);
        };
        self.hold(
            Signed {
                hold: &statement,
                stated: &stated,
                receipt: &receipt,
            },
            recorded,
            record,
            valid_until,
        )
        .map_err(TakeHoldError::NotHeld)
    }

    /// The quote held under the hold's `transaction`.
    pub fn held(&self, transaction: &Id) -> Option<&Held> {
        self.held.get(transaction.as_str())
    }

    /// The order that redeemed the hold under `transaction`, if one did.
    pub fn redeemed_by(&self, transaction: &Id) -> Option<&Id> {
        self.redeemed.get(transaction.as_str())
    }

    /// Which order redeemed which hold, rebuilt from the log's `history`, as after a restart: each acceptance that
    /// cites a hold's stated entry binds that hold's transaction, the one its entry is about, to the acceptance's order,
    /// the first acceptance to do so. The holds themselves are kept again by the deployment (`F-030`), and meet
    /// the order that redeemed them, so a restarted Core does not leave the refusal of a second order to the log.
    pub fn recorded(history: &[(Receipt, Entry)]) -> Self {
        let mut holds = Self::new();
        for (_, entry) in history {
            let Entry::Verified(verified) = entry else {
                continue;
            };
            let What::Acceptance {
                order,
                held: Some(held),
                ..
            } = &verified.what
            else {
                continue;
            };
            let transaction = history.iter().find_map(|(receipt, e)| match e {
                Entry::Stated(s) if receipt == held => s.about.as_ref()?.transaction.clone(),
                _ => None,
            });
            if let Some(transaction) = transaction {
                holds
                    .redeemed
                    .entry(transaction.as_str().to_owned())
                    .or_insert_with(|| order.clone());
            }
        }
        holds
    }

    /// The route of the quote held under the hold's transaction, to start at `redemption.now` by the reserve for the
    /// order, accepted in the session where it was, and what it showed. Refused, in this order: for no such hold;
    /// to anyone but the reserve that holds it; for an order its signed hold doesn't name, or a session the order
    /// wasn't accepted in; after another order redeemed it; for a quote it is not a hold of; when the statement
    /// kept behind its entry's commitment is not the hold Core holds (`kept` reads it); and past its validity, at
    /// which it is still usable (`REQ-QTE-15`). The same order redeems it again. The hold stays until dropped.
    ///
    /// Nothing is redeemed unless every check passes.
    pub fn redeem(
        &mut self,
        redemption: Redemption<'_>,
        kept: &mut dyn KeptStatements,
    ) -> Result<Redeemed<'_>, HoldError> {
        let Redemption {
            transaction,
            reserve,
            order,
            session,
            quote,
            now,
        } = redemption;
        let held = self
            .held
            .get(transaction.as_str())
            .ok_or(HoldError::NoSuchHold)?;
        if &held.reserve != reserve {
            return Err(HoldError::NotTheHolder);
        }
        check_redemption(&held.signed, order.as_str(), session.map(Id::as_str))
            .map_err(HoldError::NotForThisOrder)?;
        if self
            .redeemed
            .get(transaction.as_str())
            .is_some_and(|first| first != order)
        {
            return Err(HoldError::RedeemedByAnother);
        }
        if held.record() != quote.record || held.candidate() != Some(quote.candidate) {
            return Err(HoldError::NotThisQuote);
        }
        is_what_is_kept(held, kept)?;
        quote::redeem(&held.quote, now).map_err(HoldError::Redeem)?;
        self.redeemed
            .insert(transaction.as_str().to_owned(), order.clone());
        let held = self
            .held
            .get(transaction.as_str())
            .ok_or(HoldError::NoSuchHold)?;
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

/// Whether the statement kept behind `held`'s entry's commitment is the hold Core holds: the evidence store answers
/// under that commitment, SHA-256 of its salt followed by its bytes is that commitment, and the bytes read as the
/// signed hold Core was handed. The log holds only the commitment, so only whoever holds the kept bytes can tell
/// (`REQ-AUD-2`): `Holds::hold` checks the entry's shape, attribution, maker and times, never this.
fn is_what_is_kept(held: &Held, kept: &mut dyn KeptStatements) -> Result<(), HoldError> {
    let found = kept
        .kept(&held.commitment)
        .map_err(HoldError::KeptUnavailable)?
        .ok_or(HoldError::NotWhatIsKept)?;
    let (Some(salt), Some(bytes)) = (
        from_base64url(found.salt.as_str()),
        from_base64url(found.bytes.as_str()),
    ) else {
        return Err(HoldError::NotWhatIsKept);
    };
    let committed = rutent_prov::module::wire::sha256(&[salt, bytes.clone()].concat());
    let same = found.commitment == held.commitment
        && held.commitment.algorithm == DigestAlgorithm::Sha256
        && held.commitment.value.as_str() == committed
        && serde_json::from_slice::<Statement<Hold>>(&bytes).is_ok_and(|k| k == held.signed);
    if same {
        Ok(())
    } else {
        Err(HoldError::NotWhatIsKept)
    }
}
