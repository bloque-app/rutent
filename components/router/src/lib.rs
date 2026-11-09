//! The route finder, component CMP-ROUTER of the SAD: `route(request, snapshot, policy bounds)` as router-api
//! defines it (`contracts/src/router_api`), a pure hop-limited exhaustive search over a catalogue snapshot
//! (ADR-0006) that returns one route as a split of one part (`DEC-8`).
//!
//! [`RouteFinder`] implements the contract's [`Router`]. It reads nothing but its three arguments: no clock, no
//! randomness, no provider, and no hash map whose order could leak into the record. Time enters only as the
//! snapshot's `taken_at` and each figure's age at it, so a figure past its validity is judged from the record
//! alone and never used (`REQ-QTE-4`). The same arguments always give the same record, so routing a record's own
//! request, snapshot and bounds gives it again (`AC-2`, `REQ-AUD-4`).
//!
//! # The search
//!
//! From the request's origin, every path of at most `max_hops` segments, each tuple at most once in a path, is
//! evaluated at the actual amount and direction: each segment on what the one before delivered, after its fees
//! and in its own unit, and its band and its price's levels judged on that amount (`REQ-QTE-12`). A segment is any
//! advertised tuple with its price, a reserve, ramp or exchange, so any origin reaches any destination the
//! catalogue connects, with no local money or exchange needed (`REQ-QTE-14`). An exchange's price may be by
//! levels, an order book's depth, filled at the actual amount (`REQ-EXT-2`). Each operation's terms are its
//! provider's usable answer at that amount where the snapshot holds one, and else its advertised price, so a
//! route's price is its segments' prices composed, with nothing added (`REQ-QTE-8`).
//!
//! # The record
//!
//! Every path found is a candidate, named `c001`, `c002`, … in order of the tuple ids it takes, so the names don't
//! depend on the snapshot's order. Each is dropped for the first reason that holds (see the contract's
//! [`DropReason`]); the rest are ranked by the request's [`Objective`], ties going to the lowest `candidate_id`.
//! A shortlist round keeps them all, its shortlist; a choice round chooses the first and drops the others as
//! outranked by it. Every candidate is one part (`DEC-8`).

mod judge;
mod network;
mod price;
pub mod quote;
mod search;

use std::cmp::Ordering;

use num_bigint::BigUint;
use rutent_contracts::router_api::{
    Candidate, CatalogueSnapshot, DecisionRecord, DropReason, FigureValue, Id, NonEmpty, Objective,
    Part, PolicyBounds, Request, Router, Version,
};

use search::{Path, Search, answer_of};

/// The version every record this router makes names in its `router_version`.
pub const VERSION: &str = "rutent-router-0.1.0";

/// The route finder: the router API's [`Router`], as ADR-0006 and `DEC-8` decide it.
#[derive(Debug, Clone, Copy, Default)]
pub struct RouteFinder;

impl Router for RouteFinder {
    fn route(
        request: &Request,
        snapshot: &CatalogueSnapshot,
        bounds: &PolicyBounds,
    ) -> DecisionRecord {
        let search = Search::new(request, snapshot, bounds);
        let mut paths = search.paths();
        paths.sort_by(|a, b| search.tuple_ids(a).cmp(&search.tuple_ids(b)));
        let width = paths.len().to_string().len().max(3);
        let id = |i: usize| Id::new(format!("c{:0width$}", i + 1)).expect("c and digits");
        let mut dropped: Vec<Option<DropReason>> = paths.iter().map(|p| search.fault(p)).collect();
        let mut ranked: Vec<usize> = (0..paths.len()).filter(|&i| dropped[i].is_none()).collect();
        // Ids are zero-padded to one width, so comparing indices compares their bytes.
        ranked.sort_by(|&a, &b| search.rank(&paths[a], &paths[b]).then(a.cmp(&b)));
        let mut chosen = None;
        if search.is_choice()
            && let Some(&winner) = ranked.first()
        {
            for &i in &ranked[1..] {
                dropped[i] = Some(DropReason::Outranked { by: id(winner) });
            }
            chosen = Some(id(winner));
        }
        let candidates = paths
            .into_iter()
            .zip(dropped)
            .enumerate()
            .map(|(i, (path, dropped))| {
                let operations = path.steps.into_iter().map(|s| s.operation).collect();
                let part = Part {
                    operations: NonEmpty::new(operations).expect("a path takes a segment"),
                };
                Candidate {
                    candidate_id: id(i),
                    parts: NonEmpty::new(vec![part]).expect("one part"),
                    dropped,
                }
            })
            .collect();
        DecisionRecord {
            router_version: Version::new(VERSION).expect("a version"),
            request: request.clone(),
            snapshot: snapshot.clone(),
            bounds: bounds.clone(),
            candidates,
            chosen,
        }
    }
}

impl Search<'_> {
    /// The tuple figure ids a path takes, in order: its name in the catalogue.
    fn tuple_ids(&self, path: &Path) -> Vec<&str> {
        path.segments
            .iter()
            .map(|&s| self.segments[s].tuple.figure_id.as_str())
            .collect()
    }

    /// The order of two paths by the request's objective, better first (`REQ-QTE-11`).
    fn rank(&self, a: &Path, b: &Path) -> Ordering {
        match self.request.objective {
            Objective::FastestSettlement {} => self.settles(a).cmp(&self.settles(b)),
            // Lowest total cost, the default: the most value at the destination.
            _ => arrives(b).cmp(&arrives(a)),
        }
    }

    /// A path's declared settlement time: the sum of its operations' seconds, each from the usable fresh quote it
    /// cites for its amount in, the slowest if several, or else from its price (`REQ-QTE-3`). Summed in 128 bits,
    /// so no number of operations of at most 2^53 − 1 seconds each overflows.
    fn settles(&self, path: &Path) -> u128 {
        path.segments
            .iter()
            .zip(&path.steps)
            .map(|(&s, step)| {
                let quoted = step
                    .operation
                    .figures
                    .iter()
                    .filter_map(|id| self.figure(id.as_str()))
                    .filter(|f| matches!(f.value, FigureValue::FreshQuote { .. }))
                    .filter(|f| self.is_usable(f))
                    .filter_map(|f| answer_of(f).map(|a| a.settlement_seconds))
                    .max();
                u128::from(quoted.unwrap_or(self.segments[s].settlement_seconds))
            })
            .sum()
    }
}

/// What a path delivers at its destination.
fn arrives(path: &Path) -> BigUint {
    path.steps
        .last()
        .and_then(|s| price::whole(s.operation.amount_out.as_str()))
        .unwrap_or_default()
}
