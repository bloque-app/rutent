//! Each tuple's history of stated terms, with their times (`REQ-QTE-6`).

use std::collections::BTreeMap;

use rutent_contracts::router_api::{StatedTerms, Timestamp, TupleKey};
use rutent_router::quote::{Binding, binding};

/// Core's record of what each provider stated of its terms for each tuple, and when: the history the binding terms
/// of a segment are read from (`REQ-QTE-6`).
///
/// A provider's terms bind it for a segment from the segment's start: the last stated at or before it, never one
/// stated later. Before the segment starts nothing binds, and the route's total is a quote.
///
/// It is kept in memory, keyed by the tuple's provider and ends; keeping it across a restart is the deployment's
/// (`F-030`), as with the rounds' `Issued`. provider-role names no signed statement of a provider's terms, so the
/// history holds no stated entry's receipt (a gap, in `F-020`'s notes).
#[derive(Debug, Clone, Default)]
pub struct Terms {
    history: BTreeMap<String, Vec<StatedTerms>>,
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

    /// Keeps `terms` in its tuple's history. The same terms stated again are kept once; false then.
    pub fn state(&mut self, terms: StatedTerms) -> bool {
        let history = self.history.entry(key(&terms.tuple)).or_default();
        if history.contains(&terms) {
            return false;
        }
        history.push(terms);
        true
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
}
