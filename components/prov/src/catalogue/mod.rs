//! The catalogue (`F-012`): who may be routed through, under what bounds, and what the catalogue must keep.
//!
//! - `grant`: an operator's grant, attributable, scoped to a named legal party, categories and tuples, with an
//!   expiry (`REQ-CAT-2`); standing lost stops routing.
//! - `bounds`: the operator's bound per tuple (`REQ-CAT-1`).
//! - `reversal`: each grant's reversal-capacity check, the ceiling that follows the bond (`DEC-11`), and the
//!   reversals a provider owes (`REQ-CAT-7`).
//! - `declaration`: reserve declarations, recorded once and shown before acceptance (`REQ-CAT-6`).
//! - `publisher`: the publisher behind each provider and node (`REQ-CAT-20`).
//! - `nodes`: the operator's public nodes, and every node's holding account (`REQ-CAT-22`, `REQ-CAT-23`).
//! - [`check_tuple`]: the shape of a tuple (`REQ-CAT-13`, `REQ-CAT-21`).
//! - [`Catalogue::snapshot`]: what the router reads (`T-012-03`).
//! - [`Catalogue`]: the whole, with the join rule of `REQ-CAT-16`.
//!
//! The catalogue is a plain value with no clock and no provider names in it: every time is passed in, and a
//! provider the project builds is held to the same rules as any stand-in (`REQ-MOD-4`).
//!
//! The caller (the router or Core) must pass its own clock as `at`: a stale `at` would honour an expired grant.
//! `T-012-02`'s check on how far ahead an advertisement is dated uses that same `at`.

mod bounds;
mod declaration;
mod grant;
mod nodes;
mod publisher;
mod reversal;
mod snapshot;
mod tuples;

pub use bounds::{OperatorBounds, TupleBound};
pub use declaration::{
    AcceptRefusal, DeclarationError, DeclarationId, DeclarationLog, DeclarationRecord,
    RecordOutcome,
};
pub use grant::{Endpoint, Grant, GrantError, GrantTerms, Instant, Standing, TupleKey, tuples_of};
pub use nodes::{HoldingAccount, HoldingSource, NodeBook, NodeError};
pub use publisher::{PublisherError, PublisherIndex};
pub use reversal::{
    Bond, Observation, Owed, OwedError, OwedReversals, ReversalCapacity, ReversalError,
};
pub use rutent_contracts::provider_role::Category;
pub use snapshot::{LeftOut, LeftOutReason, SnapshotBuild, SnapshotError};
pub use tuples::{TupleRefusal, check_tuple};

use std::collections::{BTreeMap, HashMap};

use rutent_contracts::provider_role::{Advertisement, Declarations, EvidenceGrade};
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
}

/// Why a provider can't join (`REQ-CAT-16`, `REQ-CAT-2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinRefusal {
    /// It offers neither an identifier nor a status query to follow an outcome with.
    NoTracking,
    /// The advertisement breaks one of the contract's rules.
    Invalid(Vec<rutent_contracts::provider_role::RuleViolation>),
    /// The provider has no grant in force: entry is by the operator's grant.
    NoGrantInForce,
    /// A publisher rule refused it.
    Publisher(PublisherError),
    /// A declaration rule refused it.
    Declaration(DeclarationError),
    /// It declares a private node the operator lists as public (`REQ-CAT-22`).
    PrivateNodeIsPublic(String),
}

/// An advertisement as joined, and when.
#[derive(Debug, Clone)]
struct Joined {
    advertisement: Advertisement,
    /// When the catalogue took it: its figures' age counts from here.
    stated_at: Instant,
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
    nodes: NodeBook,
    floors: HashMap<TupleKey, EvidenceGrade>,
    modules: BTreeMap<String, Version>,
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
        self.check_standing(provider, at, |g| g.covers_category(category))
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

    /// Admits an advertisement: it follows the contract's rules, offers a way to follow an outcome
    /// (`REQ-CAT-16`), comes from a provider with a grant in force (`REQ-CAT-2`), and its publisher and reserve
    /// declaration are recorded. A refusal changes nothing.
    pub fn join(&mut self, ad: &Advertisement, at: Instant) -> Result<(), JoinRefusal> {
        if ad.declarations.tracking.is_empty() {
            return Err(JoinRefusal::NoTracking);
        }
        rutent_contracts::provider_role::validate(ad).map_err(JoinRefusal::Invalid)?;
        let in_force = self
            .grants_of_provider(&ad.provider)
            .iter()
            .any(|g| g.standing(at) == Standing::InForce);
        if !in_force {
            return Err(JoinRefusal::NoGrantInForce);
        }
        if let Some(node) = ad.private_nodes.iter().find(|n| self.nodes.is_public(n)) {
            return Err(JoinRefusal::PrivateNodeIsPublic(node.clone()));
        }
        let backup = (self.publishers.clone(), self.declarations.clone());
        let done = self
            .publishers
            .register(ad)
            .map_err(JoinRefusal::Publisher)
            .and_then(|()| {
                self.declarations
                    .record(ad, at)
                    .map(|_| ())
                    .map_err(JoinRefusal::Declaration)
            });
        if done.is_err() {
            (self.publishers, self.declarations) = backup;
        } else {
            self.joined.insert(
                ad.provider.clone(),
                Joined {
                    advertisement: ad.clone(),
                    stated_at: at,
                },
            );
        }
        done
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
    /// (`REQ-CAT-23`).
    pub fn register_public_holding(
        &mut self,
        node: &str,
        reference: &str,
    ) -> Result<(), NodeError> {
        self.nodes.hold_public(node, reference)
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
