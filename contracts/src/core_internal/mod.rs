//! The core-internal contract: the calls between Core's three parts, `CMP-CORE-ORCH`, `CMP-CORE-AUTHZ` and
//! `CMP-CORE-AUDIT` (ADR-0001, `DEC-2`, `DEC-4`, `T-020-01`).
//!
//! `C1` doesn't freeze these interfaces (`PLAN.md` §3). They are settled here, as a sixth contract, before the three
//! parts are built side by side, so no part's builders edit another's interface: each change after 1.0.0 is a
//! version bump with a changelog entry naming a `DEC-*`, and waits for the owner's click (`DEC-20`). Core's three
//! crates import none of one another. Each reaches the others only through the traits in [`calls`], and one assembly
//! package puts them together.
//!
//! This is a library API (`DEC-4`): the traits and types here are the API, and every value that crosses between the
//! parts serialises to a schema under `contracts/core-internal/schemas/`. `contracts/core-internal/README.md` lists
//! every call, every shape and every rule, and where each is enforced.
//!
//! # The calls
//!
//! From orchestration to policy ([`calls::Decisions`], [`calls::Authorisations`], [`calls::Sessions`],
//! [`calls::Reads`], [`calls::Charges`], [`calls::StuckOrders`], [`calls::PersonQuestions`]):
//!
//! - **One decision call** for every action that moves value, posts a route, changes the catalogue or moves a stuck
//!   order's value ([`DecisionRequest`] to [`Decision`]), answering permit, refuse with its class, or not known
//!   (`REQ-AUZ-1`, `REQ-AUZ-5`). Nothing takes effect on any answer but a permit: an [`Effect`] cites a [`Permit`],
//!   which carries its decision entry's receipt. A permit can be built anywhere, so the log is the guard: it admits
//!   an effect only on an unused permit for a permitting entry that decided the same action, and holds every other
//!   entry to the earlier entries it cites ([`admit_entry`]).
//! - **The authorisation** that names one order, its origin reserve, its destination, its to asset and any floor
//!   the customer set ([`Authorisation`], `REQ-AUZ-3`, `REQ-AUZ-9`). A value-moving action names the segment's
//!   provider, the origin reserve, the authorisation it acts under and the route's own destination, to asset and
//!   amount, so policy can check that a recalculated route stays inside it.
//! - **A stuck order's decide and move steps**, as two calls ([`StuckDecide`], [`StuckMove`], ADR-0011): the move
//!   names the decision it carries out, policy compares the two actors at their accountable party, and the value
//!   moves as an [`Action::ResolveStuck`].
//! - **Sessions** ([`SessionRequest`] to [`SessionAnswer`]): the session register the catalogue reads.
//! - **A charge** of work to the caller's accountable party, answered within or over its bound ([`Charge`] to
//!   [`ChargeAnswer`], `REQ-OPS-1`, `REQ-AUZ-8`).
//! - **A question about a person** ([`PersonQuestion`] to [`PersonAnswer`], `REQ-CPL-2`, `REQ-CPL-5`), naming the
//!   asker, the person's reference, its authority (with the shortlisted providers, for a shortlisted route) and the
//!   providers already asked about that order across its recalculations, answered allowed, refused or past the
//!   bound.
//! - **A read check** ([`ReadCheck`] to [`ReadAnswer`], `REQ-AUZ-7`, `REQ-AUD-12`), naming the reader in its role and
//!   what it reads (an order, a segment with its provider, the whole trail, or a named resource), answered allowed
//!   or refused and recorded either way.
//!
//! From orchestration to the log's checks ([`calls::Comparisons`], [`calls::Measures`]): each segment's
//! [`ReportPair`] (`REQ-ORD-9`), each [`HoldingReading`] (`REQ-AUD-15`) and the [`MeasureRecord`]s the bulk
//! measures read (`REQ-AUD-7`). They answer with a receipt and nothing else: what the comparison finds goes to the
//! operator, never back to orchestration, so a check can't move an order (ADR-0009).
//!
//! From orchestration and policy to the log ([`calls::Log`], [`calls::Evidence`]): an append that answers with a
//! [`Receipt`] before the effect it records takes place (`INV-7`). An [`Entry`] is a [`StatedEntry`], what a party
//! stated, with the key it says signed it and a salted commitment to its bytes as received, which the evidence store
//! keeps apart from the log; or a [`VerifiedEntry`], what rutent itself did or saw (`REQ-AUD-2`). Both say who, what,
//! when, the source of when and the outcome (`REQ-AUD-1`). A verified entry's [`What`] covers every record Core's
//! parts write: each policy call with its answer's ids, each effect and its result, the state machine's recorded
//! inputs, orchestration's own records (acceptances, refusals, decision records, settlement times, graded evidence,
//! fees, stuck calls, returns and late approvals, reliance on a judgement, operator commands, erasures), and the
//! platform's drained events, the proxy's uses and each segment record ([`HandedOver`], `REQ-MDL-8`, `REQ-MDL-5`).
//!
//! From the checks to the operator ([`calls::Operator`]): a [`Raise`], which carries a difference and nothing that
//! can move value, answered only by whether it was delivered (`REQ-ORD-9`, `REQ-AUD-15`, ADR-0009).
//!
//! From orchestration to notices: none here. Notices are `F-023`'s module of `CMP-CORE-ORCH`, inside one crate.
//!
//! # Rust is as strict as the schemas
//!
//! Every identifier, amount, count and timestamp is a checked newtype, every list the schemas require non-empty or
//! distinct is checked, optional fields refuse `null`, and every struct refuses unknown fields. Every shape's
//! `check` enforces the rules JSON Schema can't state and recurses into the shapes it holds, so a value built in
//! memory is held to the same rules as one read from the wire; deserialising runs it too. The shapes of other
//! contracts it carries (outbound call records, cross-module calls, secret lookups, recorded inputs, nodes and
//! denominations) are those contracts' types, as strict as those contracts make them.
//!
//! # Visible to every crate, named by Core alone
//!
//! The contract lives in `rutent-contracts`, the one crate all three parts already reach, so it adds no edge between
//! them, and the contract guard keeps every change to it a reviewed bump. The outer libraries reach the same crate,
//! so they could name these types; a source check in `tools/boundary-check` fails if any of them names
//! `core_internal` at all (`DEC-2`).

mod attribution;
pub mod calls;
mod check;
mod relations;
mod standing;
mod types;

pub use attribution::{
    STATED_WITHOUT_ITS_KEPT_BYTES, attribute, granted_advertising_keys, release_statement_keys,
};
pub use relations::{
    Answers, CLOSING_RELATIONS, Cite, Cites, DeploymentOperator, History, Match, Quantifier,
    RELATIONS, Relation, STATED_RELATIONS, StatedCiting, Through, VerifiedCiting, admit_entry,
    covers,
};
pub use standing::state_machine_takes;
pub use types::*;

/// The contract's version: the first `## ` heading of `contracts/core-internal/CHANGELOG.md`.
pub const VERSION: &str = "1.0.0";

/// A rule of the contract that a value breaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation(pub &'static str);

impl core::fmt::Display for Violation {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.0)
    }
}

impl core::error::Error for Violation {}
