//! The catalogue (`F-012`): who may be routed through, under what bounds, and what the catalogue must keep.
//!
//! - `grant`: an operator's grant, attributable, scoped to a named legal party, categories and tuples, with an
//!   expiry (`REQ-CAT-2`); standing lost stops routing.
//! - `bounds`: the operator's bound per tuple (`REQ-CAT-1`).
//! - `reversal`: each grant's reversal-capacity check, the ceiling that follows the bond (`DEC-11`), and the
//!   reversals a provider owes (`REQ-CAT-7`).
//! - `declaration`: reserve declarations, recorded once and shown before acceptance (`REQ-CAT-6`).
//! - `publisher`: the publisher behind each provider and node (`REQ-CAT-20`), and the modules the operator
//!   publishes itself, such as a public node's holding account (`REQ-CAT-23`).
//! - `operator`: the operator's own signed statements, verified against the operator key of the deployment the
//!   catalogue serves (`T-016-04`; `REQ-MDL-1`).
//! - `nodes`: the operator's public nodes, and every node's holding account (`REQ-CAT-22`, `REQ-CAT-23`).
//! - `balance`: balance checks, bounded and logged per account, and a silent reserve's ineligibility
//!   (`REQ-CAT-3`, `REQ-CAT-4`, `REQ-CAT-19`).
//! - `segments`: dry runs, refusal classes, a true undo and where value is after a failure (`REQ-CAT-14`,
//!   `REQ-CAT-15`, `REQ-CAT-18`, `INV-2`).
//! - [`check_tuple`]: the shape of a tuple (`REQ-CAT-13`, `REQ-CAT-21`).
//! - [`Catalogue::snapshot`]: what the router reads (`T-012-03`).
//! - `advertisements`: advertisements by their received bytes, first trust by the operator's grant, the key history
//!   and the clock rule (`T-012-02`; `REQ-CAT-2`, `THR-4`).
//! - `signature`: the Ed25519 verifier over the contract's canonical bytes (`REQ-PRT-1`, `THR-8`), the one admission
//!   uses.
//! - [`Catalogue`]: the whole, with the join rule of `REQ-CAT-16`.
//!
//! The catalogue is a plain value with no clock and no provider names in it: every time is passed in, and a
//! provider the project builds is held to the same rules as any stand-in (`REQ-MOD-4`).
//!
//! The caller (the router or Core) must pass its own clock as `at`: a stale `at` would honour an expired grant.
//! The check on how far ahead an advertisement is dated uses that same `at`, and a figure's age counts from its
//! advertisement's `made_at`.

mod advertisements;
mod balance;
mod bounds;
mod declaration;
mod grant;
mod nodes;
mod operator;
mod publisher;
mod reversal;
mod segments;
mod signature;
mod snapshot;
mod time;
mod tuples;

pub(crate) use time::Stamp;

pub use advertisements::{DEFAULT_CLOCK_SKEW_SECONDS, JoinRefusal, StatementRefusal};
pub use balance::{
    Alert, CheckBound, CheckKind, CheckRecord, CheckRefusal, CheckTicket, Eligibility, Verdict,
    reveals_sufficiency,
};
pub use bounds::{OperatorBounds, TupleBound};
pub use declaration::{
    AcceptRefusal, DeclarationError, DeclarationId, DeclarationLog, DeclarationRecord,
    RecordOutcome,
};
pub use grant::{Endpoint, Grant, GrantError, GrantTerms, Instant, Standing, TupleKey, tuples_of};
pub use nodes::{HoldingAccount, HoldingSource, NodeBook, NodeError};
pub use publisher::{Publisher, PublisherError, PublisherIndex};
pub use reversal::{
    Bond, Observation, Owed, OwedError, OwedReversals, ReversalCapacity, ReversalError,
};
pub use rutent_contracts::provider_role::Category;
pub use segments::{
    DryRunAssessment, DryRunError, EffectError, RefusalClass, SegmentDecline, SegmentEffect,
    UndoVerdict, judge_reversal, segment_effect,
};
pub use signature::{
    SignatureError, check_public_key, decode_signature, verify_ed25519, verify_envelope,
    verify_text,
};
pub use snapshot::{LeftOut, LeftOutReason, SnapshotBuild, SnapshotError};
pub use tuples::{TupleRefusal, check_tuple};

use std::collections::{BTreeMap, HashMap, HashSet};

use rutent_contracts::provider_role::{
    Advertisement, Declarations, Deployment, EvidenceGrade, PublicKey,
};
use rutent_contracts::router_api::Version;

/// Why a provider's tuple can't be used in a new route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unusable {
    /// The provider has no grant at all.
    NoGrant,
    /// It has grants, but none is in force; the standing of the most recent one.
    NotInForce(Standing),
    /// A grant is in force, but none covers this tuple (or category).
    OutsideGrantScope,
    /// The operator restricted the tuple (`REQ-CAT-1`).
    Restricted,
    /// The amount is over the operator's bound for the tuple.
    OverBound {
        /// The largest amount the operator allows.
        max: u128,
    },
    /// A reserve that didn't answer a check in time is out until it answers one (`REQ-CAT-19`).
    TemporarilyUnavailable {
        /// The moment its answer was due.
        since: Instant,
    },
}

/// Why the catalogue refused to change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogueError {
    /// A grant with this identifier exists already.
    DuplicateGrant(String),
    /// No grant has this identifier.
    UnknownGrant(String),
    /// The reversal-capacity rule refused it (`DEC-11`).
    Reversal(ReversalError),
    /// The key is a statements key somewhere, so it can't be trusted for advertising (`THR-4`).
    KeyIsAStatementsKey,
    /// The key is an advertising key somewhere, so it can't be a statements key (`THR-4`).
    KeyIsAnAdvertisingKey,
}

/// An advertisement as joined, and when.
#[derive(Debug, Clone)]
struct Joined {
    advertisement: Advertisement,
    /// When it was made, to the whole second: its figures' age counts from here (`REQ-QTE-4`).
    made_at: Instant,
}

/// The catalogue.
#[derive(Debug, Clone, Default)]
pub struct Catalogue {
    grants: Vec<Grant>,
    bounds: OperatorBounds,
    declarations: DeclarationLog,
    publishers: PublisherIndex,
    owed: OwedReversals,
    joined: BTreeMap<String, Joined>,
    ads: BTreeMap<String, Vec<advertisements::Accepted>>,
    advertising_seen: HashSet<PublicKey>,
    statements_seen: HashSet<PublicKey>,
    context: Option<(Deployment, String)>,
    skew_secs: Option<u64>,
    nodes: NodeBook,
    floors: HashMap<TupleKey, EvidenceGrade>,
    modules: BTreeMap<String, Version>,
    checks: balance::BalanceChecks,
}

impl Catalogue {
    /// An empty catalogue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a grant. Grants are never removed or replaced: re-entry adds a new one and the old stays visible
    /// (`REQ-CAT-2`).
    pub fn grant(&mut self, grant: Grant) -> Result<(), CatalogueError> {
        if self.grants.iter().any(|g| g.id() == grant.id()) {
            return Err(CatalogueError::DuplicateGrant(grant.id().to_owned()));
        }
        self.grants.push(grant);
        Ok(())
    }

    /// Revokes a grant from `at`. A second revocation keeps the first time.
    pub fn revoke(&mut self, id: &str, at: Instant) -> Result<(), CatalogueError> {
        self.grant_mut(id)?.revoke(at);
        Ok(())
    }

    /// Every grant made to a legal party, oldest first, expired and revoked ones included.
    pub fn grants_of_party(&self, party: &str) -> Vec<&Grant> {
        self.grants.iter().filter(|g| g.party() == party).collect()
    }

    /// Every grant of a provider, oldest first.
    pub fn grants_of_provider(&self, provider: &str) -> Vec<&Grant> {
        self.grants
            .iter()
            .filter(|g| g.provider() == provider)
            .collect()
    }

    /// Sets the operator's bound on one of a provider's tuples.
    pub fn bound(&mut self, provider: &str, tuple: TupleKey, bound: TupleBound) {
        self.bounds.set(provider, tuple, bound);
    }

    /// Whether `tuple` of `provider` may be in a new route at `at`: a grant in force covers it and the operator
    /// hasn't restricted it.
    pub fn usable(&self, provider: &str, tuple: &TupleKey, at: Instant) -> Result<(), Unusable> {
        self.check_standing(provider, at, |g| g.covers(tuple))?;
        self.check_answering(provider, tuple.category, at)?;
        match self.bounds.get(provider, tuple) {
            Some(TupleBound::Restricted) => Err(Unusable::Restricted),
            _ => Ok(()),
        }
    }

    /// [`Self::usable`], and the amount (in the tuple's input unit) within the operator's bound.
    pub fn usable_for(
        &self,
        provider: &str,
        tuple: &TupleKey,
        amount: u128,
        at: Instant,
    ) -> Result<(), Unusable> {
        self.usable(provider, tuple, at)?;
        match self.bounds.get(provider, tuple) {
            Some(TupleBound::Max(max)) if amount > *max => Err(Unusable::OverBound { max: *max }),
            _ => Ok(()),
        }
    }

    /// Whether a provider's category with no tuples (a verifier) is covered by a grant in force at `at`.
    pub fn category_usable(
        &self,
        provider: &str,
        category: Category,
        at: Instant,
    ) -> Result<(), Unusable> {
        self.check_standing(provider, at, |g| g.covers_category(category))?;
        self.check_answering(provider, category, at)
    }

    /// A reserve marked silent is out until it answers a check in time.
    fn check_answering(
        &self,
        provider: &str,
        category: Category,
        at: Instant,
    ) -> Result<(), Unusable> {
        match self.checks.unavailable_since(provider, at) {
            Some(since) if category == Category::Reserve => {
                Err(Unusable::TemporarilyUnavailable { since })
            }
            _ => Ok(()),
        }
    }

    fn check_standing(
        &self,
        provider: &str,
        at: Instant,
        covers: impl Fn(&Grant) -> bool,
    ) -> Result<(), Unusable> {
        let grants = self.grants_of_provider(provider);
        let Some(latest) = grants.iter().max_by_key(|g| g.granted_at()) else {
            return Err(Unusable::NoGrant);
        };
        let in_force: Vec<&&Grant> = grants
            .iter()
            .filter(|g| g.standing(at) == Standing::InForce)
            .collect();
        if in_force.is_empty() {
            return Err(Unusable::NotInForce(latest.standing(at)));
        }
        if in_force.iter().any(|g| covers(g)) {
            Ok(())
        } else {
            Err(Unusable::OutsideGrantScope)
        }
    }

    /// The reversal ceiling in force for a provider at `at`: the largest of its grants' ceilings in force.
    pub fn ceiling_of(&self, provider: &str, at: Instant) -> Option<u64> {
        self.grants_of_provider(provider)
            .into_iter()
            .filter(|g| g.standing(at) == Standing::InForce)
            .map(|g| g.reversal_capacity().ceiling())
            .max()
    }

    /// Sets a grant's ceiling, which can't pass its bond (`DEC-11`).
    pub fn set_ceiling(&mut self, id: &str, ceiling: u64) -> Result<(), CatalogueError> {
        self.grant_mut(id)?
            .reversal_capacity_mut()
            .set_ceiling(ceiling)
            .map_err(CatalogueError::Reversal)
    }

    /// Records a new check of a grant's bond. The ceiling follows it down, and never up (`DEC-11`).
    pub fn rebond(
        &mut self,
        id: &str,
        bond: Bond,
        checked_by: &str,
        at: Instant,
    ) -> Result<(), CatalogueError> {
        self.grant_mut(id)?
            .reversal_capacity_mut()
            .rebond(bond, checked_by, at)
            .map_err(CatalogueError::Reversal)
    }

    fn grant_mut(&mut self, id: &str) -> Result<&mut Grant, CatalogueError> {
        self.grants
            .iter_mut()
            .find(|g| g.id() == id)
            .ok_or_else(|| CatalogueError::UnknownGrant(id.to_owned()))
    }

    /// What a joined provider declared: how it is followed, its evidence and whether a repeat is safe
    /// (`REQ-CAT-10`).
    pub fn declared_by(&self, provider: &str) -> Option<&Declarations> {
        self.joined
            .get(provider)
            .map(|j| &j.advertisement.declarations)
    }

    /// Adds a node to the operator's public list. A node a provider declares private can't be public.
    pub fn add_public_node(&mut self, node: &str) -> Result<(), NodeError> {
        let private = self.publishers.publisher_of_node(node).is_some();
        self.nodes.add_public(node, private)
    }

    /// Registers a public node's holding account, by default a plain wallet, as a module outside the core
    /// (`REQ-CAT-23`). `reference` must be a module the operator publishes ([`Self::publish_operator_module`]):
    /// a provider's module is never a node's holding account (`T-016-04`).
    pub fn register_public_holding(
        &mut self,
        node: &str,
        reference: &str,
    ) -> Result<(), NodeError> {
        let operators = self.publishers.published_by_operator(reference);
        self.nodes.hold_public(node, reference, operators)
    }

    /// Records the way a private node's publisher gave the operator to its holding account (`REQ-CAT-23`).
    pub fn register_private_holding(
        &mut self,
        node: &str,
        publisher: &str,
        interface: &str,
    ) -> Result<(), NodeError> {
        let recorded = self.publishers.publisher_of_node(node);
        self.nodes
            .hold_private(node, recorded, publisher, interface)
    }

    /// The public nodes and every node's holding account.
    pub fn nodes(&self) -> &NodeBook {
        &self.nodes
    }

    /// Sets the weakest evidence grade the operator accepts on a tuple (`REQ-ORD-39`): grade 1 is the strongest.
    /// A provider declaring a weaker grade is left out of the snapshot for it.
    pub fn set_evidence_floor(&mut self, tuple: TupleKey, floor: EvidenceGrade) {
        self.floors.insert(tuple, floor);
    }

    /// The floor set on a tuple, if any.
    pub fn evidence_floor(&self, tuple: &TupleKey) -> Option<EvidenceGrade> {
        self.floors.get(tuple).copied()
    }

    /// Records the version of the admitted module behind a provider, which its figures' origin names.
    pub fn set_module_version(&mut self, provider: &str, version: Version) {
        self.modules.insert(provider.to_owned(), version);
    }

    /// The reserve declarations.
    pub fn declarations(&self) -> &DeclarationLog {
        &self.declarations
    }

    /// The reserve declarations, to record and show.
    pub fn declarations_mut(&mut self) -> &mut DeclarationLog {
        &mut self.declarations
    }

    /// The publisher behind each provider and node.
    pub fn publishers(&self) -> &PublisherIndex {
        &self.publishers
    }

    /// The reversals providers owe.
    pub fn owed(&self) -> &OwedReversals {
        &self.owed
    }

    /// The reversals providers owe, to record and discharge.
    pub fn owed_mut(&mut self) -> &mut OwedReversals {
        &mut self.owed
    }
}
