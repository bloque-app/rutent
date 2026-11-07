//! The router API contract: `route(request, catalogueSnapshot, policyBounds) → {candidates, chosen,
//! decisionRecord}` (SAD §3, `DEC-4`, `DEC-8`, ADR-0006).
//!
//! This is a library API (`DEC-4`): these types are the API, and their records serialise to the schemas under
//! `contracts/router-api/schemas/`. `CMP-ROUTER` implements [`Router`] in M2; this module holds no routing rule of
//! its own.
//!
//! # A pure function, recomputable from its record
//!
//! [`Router::route`] reads nothing but its three arguments, and the [`DecisionRecord`] it returns holds all three
//! verbatim, with the router's version, every candidate and why each was dropped, and the chosen one. Calling
//! `route` again on the record's own request, snapshot and bounds gives the same record, with no provider call
//! (`AC-2`, `REQ-AUD-4`). Time enters only as the snapshot's `taken_at` and each figure's age at it, so validity
//! is judged from the record alone (`REQ-QTE-4`). The deployment and protocol version that decide whether a
//! statement is void are in the [`PolicyBounds`] (`REQ-PRT-1`), the ceilings there and the exposure in the snapshot
//! (`REQ-ORD-8`), and ties go to the lowest `candidate_id` (see [`Objective`]).
//!
//! # Two rounds
//!
//! Core calls `route` twice per routing (ruled by the orchestrator, 2026-11-06): once on the advertised snapshot,
//! a [`Round::Shortlist`] whose kept candidates are the shortlist, then on a refreshed snapshot that adds fresh
//! quotes, dry-run and eligibility answers for the shortlisted providers only, a [`Round::Choice`] that names the
//! shortlist round it refines (`REQ-CAT-5`, `REQ-AUD-4`).
//!
//! # Split-shaped from the start
//!
//! A candidate is a split of one or more parts, each an ordered list of typed operations (`REQ-ORD-37`,
//! `REQ-QTE-16`). v0's router always chooses one part; v1 adds the splitting rule without changing this contract
//! (`DEC-8`).
//!
//! # Rust is as strict as the schemas
//!
//! Every identifier, version, amount, decimal and timestamp is a checked newtype, every list the schemas require non-empty or
//! distinct is a [`NonEmpty`] or [`Distinct`], optional fields refuse `null`, and every struct refuses unknown
//! fields. Deserialising an [`Operation`], [`Part`], [`Candidate`], [`CatalogueSnapshot`] or [`DecisionRecord`]
//! runs its `check`, which also enforces the rules JSON Schema can't state. `contracts/router-api/README.md` lists
//! every rule and where it is enforced. The provider-role shapes embedded here (tuples, nodes, denominations) are
//! that contract's types, as strict as that contract makes them.
//!
//! # The statement envelope
//!
//! Every party statement the router reads or makes travels in the provider-role contract's envelope
//! (`urn:rutent:provider-role:statement-envelope`, `REQ-PRT-1`), as [`crate::provider_role::Statement`]. Fresh
//! quotes and dry runs are the providers' own signed results (`urn:rutent:provider-role:result`), and eligibility is
//! a verifier's signed judgement (`urn:rutent:provider-role:judgement`); each is carried whole and unchanged, so its
//! signature still verifies. A decision record is the deployment's statement, [`RecordedDecision`], signed when it
//! is recorded, since a pure router can neither sign nor read the time.
//!
//! # The typed operation
//!
//! The router contract owns the typed operation of `REQ-ORD-37` (ruled by the orchestrator, 2026-11-06):
//! `urn:rutent:router-api:operation#/$defs/operation`, with its kind at `#/$defs/kind` ([`OperationKind`]). The
//! state machine's segment plan refers to it.

mod atoms;
mod check;
mod types;

pub use atoms::{
    Amount, Decimal, Distinct, Id, NonEmpty, PositiveAmount, ProtocolVersion, Timestamp, Version,
};
pub use types::*;

/// The contract's version: the first `## ` heading of `contracts/router-api/CHANGELOG.md`.
pub const VERSION: &str = "0.5.0";

/// A rule of the contract that a value breaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation(pub &'static str);

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Violation {}

/// The router's API: `route(request, catalogueSnapshot, policyBounds) → {candidates, chosen, decisionRecord}`
/// (SAD §3, ADR-0006).
///
/// `route` is pure and deterministic: it takes no `self`, reads no clock, draws no randomness and calls no
/// provider, so no hidden input can enter. It returns the [`DecisionRecord`], which is the SAD's triple in one
/// value: [`DecisionRecord::candidates`] and [`DecisionRecord::chosen`] are views into it, so the three can't
/// disagree. A record with no chosen candidate means no route.
///
/// Its inputs must pass their checks: [`CatalogueSnapshot::check`] and [`PolicyBounds::check`]. Then every record
/// it returns passes [`DecisionRecord::check`], holds `request`, `snapshot` and `bounds` unchanged,
/// and names the implementation's version in [`DecisionRecord::router_version`]. The deployment puts it in its
/// envelope ([`RecordedDecision`]) when it records it. The same arguments always give
/// the same record, so the choice can be recomputed from the record alone (`AC-2`, `REQ-AUD-4`).
pub trait Router {
    /// Routes `request` over `snapshot`, within `bounds`.
    fn route(
        request: &Request,
        snapshot: &CatalogueSnapshot,
        bounds: &PolicyBounds,
    ) -> DecisionRecord;
}
