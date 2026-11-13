//! Each tuple's history of stated terms, with their times and the stated entries that hold them (`REQ-QTE-6`).

use std::collections::BTreeMap;

use rutent_contracts::core_internal::calls::{AppendFailure, Evidence, KeptStatements, Log};
use rutent_contracts::core_internal::{Attribution, Entry, Receipt, StatedEntry};
use rutent_contracts::provider_role::{Statement, TERMS_SHAPE, TupleTerms, check_terms};
use rutent_contracts::router_api::{StatedTerms, Timestamp, TupleKey};
use rutent_prov::catalogue::Catalogue;
use rutent_router::quote::{Binding, binding};

use super::journal;
use crate::segments::{self, Unappended};
use crate::snapshot::Clock;

/// Core's record of what each provider stated of its terms for each tuple, and when: the history the binding terms
/// of a segment are read from (`REQ-QTE-6`).
///
/// Each is a provider's signed terms (provider-role's [`TupleTerms`], shape [`TERMS_SHAPE`]), kept and appended as a
/// stated entry, and the history holds that entry's receipt beside the terms, so a charge raised against them can
/// name them (`charge-raise-rests-on-terms-in-force`). Terms bind a provider from [`StatedEntry::binds_from`], the
/// later of the statement's `made_at` and Core's `received_at`, never `made_at` alone: terms dated before a segment
/// started but received after it never bind it.
///
/// A provider's terms bind it for a segment from the segment's start: the last stated at or before it, never one
/// stated later. Before the segment starts nothing binds, and the route's total is a quote.
///
/// It is kept in memory, keyed by the tuple's provider and ends; keeping it across a restart is the deployment's
/// (`F-030`), as with the rounds' `Issued`. Which module hands a provider's signed terms to [`Terms::state`] is not
/// built: no part of Core receives them yet.
#[derive(Debug, Clone, Default)]
pub struct Terms {
    history: BTreeMap<String, Vec<StatedTerms>>,
    /// The receipt of the stated entry of each terms of `history`, in the same order.
    receipts: BTreeMap<String, Vec<Receipt>>,
}

/// Why a stated entry isn't kept as a provider's terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotTerms {
    /// The entry isn't of the terms shape.
    NotTheTermsShape,
    /// The log didn't attribute it: its signature didn't verify, or couldn't be checked.
    Unattributed,
    /// The entry's maker isn't the statement's: another statement was handed with it.
    NotItsStatement,
    /// The statement breaks the contract's rules for terms: it isn't of the terms shape, its maker isn't its tuple's
    /// provider, or it states a slippage regime where its tuple sets no rate, or none where it does.
    Refused,
    /// The entry doesn't state the statement's own times: its `made_at`, or the time they bind from.
    NotItsTimes,
    /// The evidence store keeps nothing behind the entry's commitment, or what it keeps is not the statement handed
    /// in: the entry rests on other bytes, so the terms rest on nothing the log holds (`REQ-AUD-2`).
    NotWhatIsKept,
}

/// The log's recorded entries, read by receipt: what a call reads back to get a statement as the log attributed it.
pub trait Recorded {
    /// The entry the log recorded at `receipt`, or `None` where it holds none.
    fn entry(&self, receipt: &Receipt) -> Option<Entry>;
}

/// What taking a provider's signed terms reaches.
pub struct TermsDesk<'a> {
    /// The evidence store, which keeps the bytes before their entry is appended.
    pub evidence: &'a mut dyn Evidence,
    /// The log, through orchestration's handle.
    pub log: &'a mut dyn Log,
    /// The log's own read of what the evidence store kept under a commitment.
    pub kept: &'a mut dyn KeptStatements,
    /// The log's recorded entries.
    pub recorded: &'a dyn Recorded,
    /// Core's clock, which stamps when the terms were received.
    pub clock: &'a dyn Clock,
    /// The catalogue, which names the party a provider states for.
    pub catalogue: &'a Catalogue,
}

/// Why a provider's terms weren't taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TakeError {
    /// The bytes aren't a provider's signed terms in an envelope.
    Unreadable,
    /// The evidence store or the log refused or failed.
    Append(AppendFailure),
    /// The log holds no entry at the receipt it answered.
    NotRecorded,
    /// The entry isn't kept as terms: why.
    NotKept(NotTerms),
}

/// What taking terms came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Taken {
    /// The receipt of their stated entry.
    pub receipt: Receipt,
    /// Whether they were new: the same terms taken again are kept once.
    pub new: bool,
}

/// What binds a tuple's provider for a segment, with the stated entry that holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InForce<'a> {
    /// The segment hasn't started: its terms are a quote and bind nobody.
    Quote,
    /// The terms stated last at or before the segment started bind it, in the entry at `receipt`.
    Binds {
        /// The terms.
        terms: &'a StatedTerms,
        /// The receipt of their stated entry.
        receipt: &'a Receipt,
    },
    /// The segment started before any terms were stated: the price its route was priced on bound it.
    Unstated,
}

/// A tuple as a key across snapshots: its provider and ends, as their canonical JSON.
fn key(tuple: &TupleKey) -> String {
    serde_json::to_string(tuple).unwrap_or_default()
}

impl Terms {
    /// No terms stated yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Keeps a provider's signed `terms`, whose stated entry is `stated`, appended at `receipt`, in its tuple's
    /// history. They bind from [`StatedEntry::binds_from`]. The same terms stated again are kept once, under the
    /// receipt they were first kept under; false then.
    pub fn state(
        &mut self,
        stated: &StatedEntry,
        receipt: &Receipt,
        terms: &Statement<TupleTerms>,
    ) -> Result<bool, NotTerms> {
        if stated.what.statement.as_str() != TERMS_SHAPE {
            return Err(NotTerms::NotTheTermsShape);
        }
        let Attribution::Verified { who } = &stated.attribution else {
            return Err(NotTerms::Unattributed);
        };
        if who.acts.as_str() != terms.maker {
            return Err(NotTerms::NotItsStatement);
        }
        terms.check_shape().map_err(|_| NotTerms::Refused)?;
        check_terms(terms).map_err(|_| NotTerms::Refused)?;
        let kept = StatedTerms::from_statement(terms, &stated.received_at)
            .map_err(|_| NotTerms::Refused)?;
        if stated.when.at.as_str() != terms.made_at
            || kept.stated_at.unix_nanos() != stated.binds_from().unix_nanos()
        {
            return Err(NotTerms::NotItsTimes);
        }
        let k = key(&kept.tuple);
        let history = self.history.entry(k.clone()).or_default();
        if history.contains(&kept) {
            return Ok(false);
        }
        history.push(kept);
        self.receipts.entry(k).or_default().push(receipt.clone());
        Ok(true)
    }

    /// Takes a provider's signed terms, `bytes` as they arrived: keeps them in the evidence store, appends them as a
    /// stated entry about no order (the log attributes it), reads the entry back as the log recorded it, fetches the
    /// statement kept behind its commitment and compares it with the statement the bytes read as, and only then
    /// keeps the terms ([`Terms::state`] checks the entry's shape, attribution, maker and times, never the
    /// commitment, which only the kept bytes answer for). The same terms taken again are kept once
    /// (`Terms::state`).
    ///
    /// Nothing in Core hands a provider's terms to this call yet: no module delivers them (`T-020-14`'s notes).
    pub fn take(&mut self, desk: TermsDesk<'_>, bytes: &[u8]) -> Result<Taken, TakeError> {
        let statement: Statement<TupleTerms> =
            serde_json::from_slice(bytes).map_err(|_| TakeError::Unreadable)?;
        let received_at = journal::now(desk.clock).map_err(TakeError::Append)?;
        let entry = segments::journal::stated_entry(
            desk.evidence,
            &received_at,
            bytes,
            segments::maker(desk.catalogue, bytes),
            None,
        )
        .map_err(|why| match why {
            Unappended::Unrecordable => TakeError::Unreadable,
            Unappended::Append(failure) => TakeError::Append(failure),
        })?;
        let receipt = journal::built_stated(desk.log, entry).map_err(TakeError::Append)?;
        let Some(Entry::Stated(stated)) = desk.recorded.entry(&receipt) else {
            return Err(TakeError::NotRecorded);
        };
        let kept = desk
            .kept
            .kept(&stated.commitment)
            .map_err(TakeError::Append)?
            .ok_or(TakeError::NotKept(NotTerms::NotWhatIsKept))?;
        let same = kept.commitment == stated.commitment
            && from_base64url(kept.bytes.as_str())
                .and_then(|b| serde_json::from_slice::<Statement<TupleTerms>>(&b).ok())
                .is_some_and(|k| k == statement);
        if !same {
            return Err(TakeError::NotKept(NotTerms::NotWhatIsKept));
        }
        let new = self
            .state(&stated, &receipt, &statement)
            .map_err(TakeError::NotKept)?;
        Ok(Taken { receipt, new })
    }

    /// How many terms are kept, across tuples.
    pub fn len(&self) -> usize {
        self.history.values().map(Vec::len).sum()
    }

    /// Whether no terms are kept.
    pub fn is_empty(&self) -> bool {
        self.history.is_empty()
    }

    /// Every terms stated for `tuple`, in the order they were kept.
    pub fn history(&self, tuple: &TupleKey) -> &[StatedTerms] {
        self.history.get(&key(tuple)).map_or(&[], Vec::as_slice)
    }

    /// The terms that bind `tuple`'s provider for a segment that started at `started_at`, or a quote if it hasn't
    /// started (`REQ-QTE-6`): the router's `binding` over this tuple's history.
    pub fn binding(&self, tuple: &TupleKey, started_at: Option<&Timestamp>) -> Binding<'_> {
        binding(self.history(tuple), started_at)
    }

    /// What binds `tuple`'s provider for a segment that started at `started_at`, with the receipt of the stated
    /// entry that holds the terms that do (`REQ-QTE-6`): [`Terms::binding`], and where it binds, its entry.
    pub fn in_force(&self, tuple: &TupleKey, started_at: Option<&Timestamp>) -> InForce<'_> {
        match self.binding(tuple, started_at) {
            Binding::Quote => InForce::Quote,
            Binding::Unstated => InForce::Unstated,
            Binding::Binds(terms) => {
                let receipt = self
                    .history(tuple)
                    .iter()
                    .position(|t| std::ptr::eq(t, terms))
                    .and_then(|i| self.receipts.get(&key(tuple))?.get(i));
                // Every terms kept has its receipt: kept together.
                match receipt {
                    Some(receipt) => InForce::Binds { terms, receipt },
                    None => InForce::Unstated,
                }
            }
        }
    }
}

/// The bytes base64url text without padding writes, or `None` where it isn't such text.
fn from_base64url(text: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0u32);
    for c in text.bytes() {
        let v = u32::try_from(ALPHABET.iter().position(|a| *a == c)?).ok()?;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from(acc >> bits).ok()?);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}
