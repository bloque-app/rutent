//! Grants (`REQ-CAT-2`) and the tuples they are scoped to.

use rutent_contracts::provider_role::{Advertisement, Category, NodeRef};

use super::reversal::ReversalCapacity;

/// A moment, in whole seconds since the Unix epoch. The catalogue holds no clock: the caller passes the time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant(pub u64);

/// One end of a tuple: an inner node by identifier, or an outer node by network (`REQ-CAT-17`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Endpoint {
    /// An inner node.
    Inner(String),
    /// An outer node, named for its network.
    Outer(String),
}

impl From<&NodeRef> for Endpoint {
    fn from(node: &NodeRef) -> Self {
        match node {
            NodeRef::Inner(id) => Self::Inner(id.clone()),
            NodeRef::Outer(outer) => Self::Outer(outer.network.clone()),
        }
    }
}

/// What a grant and an operator's bound name: one tuple of one category, as an advertisement offers it
/// (`REQ-CAT-13`). A reserve or ramp tuple is its origin, destination and asset; an exchange tuple is its node
/// at both ends, its `from` asset and its `to` asset.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TupleKey {
    /// The offer the tuple is in.
    pub category: Category,
    /// Where the value starts.
    pub origin: Endpoint,
    /// Where it ends.
    pub destination: Endpoint,
    /// The asset, or an exchange's `from` asset.
    pub asset: String,
    /// An exchange's `to` asset; `None` for every other category.
    pub to_asset: Option<String>,
}

/// Every tuple an advertisement offers, in the order reserve, exchange, ramp.
pub fn tuples_of(ad: &Advertisement) -> Vec<TupleKey> {
    let mut keys = Vec::new();
    if let Some(reserve) = &ad.offers.reserve {
        keys.extend(reserve.tuples.iter().map(|t| TupleKey {
            category: Category::Reserve,
            origin: (&t.origin).into(),
            destination: (&t.destination).into(),
            asset: t.asset.clone(),
            to_asset: None,
        }));
    }
    if let Some(exchange) = &ad.offers.exchange {
        keys.extend(exchange.tuples.iter().map(|t| TupleKey {
            category: Category::Exchange,
            origin: Endpoint::Inner(t.node.clone()),
            destination: Endpoint::Inner(t.node.clone()),
            asset: t.from.id.clone(),
            to_asset: Some(t.to.id.clone()),
        }));
    }
    if let Some(ramp) = &ad.offers.ramp {
        keys.extend(ramp.tuples.iter().map(|t| TupleKey {
            category: Category::Ramp,
            origin: Endpoint::Inner(t.origin.clone()),
            destination: Endpoint::Inner(t.destination.clone()),
            asset: t.asset.clone(),
            to_asset: None,
        }));
    }
    keys
}

/// Where a grant stands at a moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// Granted, and not yet expired or revoked.
    InForce,
    /// Its start is later than the moment asked about.
    NotYetInForce,
    /// Past its stated expiry (`REQ-CAT-2`).
    Expired,
    /// Revoked at or before the moment.
    Revoked,
}

/// What an operator writes to grant entry (`REQ-CAT-2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantTerms {
    /// The grant's identifier, never reused.
    pub id: String,
    /// The provider it admits.
    pub provider: String,
    /// The named legal party it is scoped to.
    pub party: String,
    /// The operator's person or role it is attributable to.
    pub granted_by: String,
    /// When it starts.
    pub granted_at: Instant,
    /// When it expires: a grant always has a stated expiry.
    pub expires_at: Instant,
    /// The categories it admits.
    pub categories: Vec<Category>,
    /// The tuples it admits; every value-moving category needs at least one.
    pub tuples: Vec<TupleKey>,
    /// The check that the provider can pay for its reversals, with the bond its ceiling follows (`REQ-CAT-7`,
    /// `DEC-11`). A grant can't be made without one.
    pub reversal_capacity: ReversalCapacity,
}

/// Why a grant's terms are refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantError {
    /// No identifier, provider or grantor named.
    NotAttributable,
    /// No legal party named.
    NoNamedParty,
    /// Its expiry is not after its start.
    ExpiresBeforeItStarts,
    /// It admits no category.
    NoCategories,
    /// A value-moving category has no tuple.
    CategoryWithoutTuples(Category),
    /// A tuple is of a category the grant doesn't admit.
    TupleOutsideCategories(Category),
    /// A tuple's shape breaks its role's contract: an exchange spanning two nodes, a ramp within one
    /// (`REQ-CAT-13`).
    MalformedTuple(super::tuples::TupleRefusal),
}

/// An operator's grant of entry (`REQ-CAT-2`). Built only from well-formed terms, and never deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    terms: GrantTerms,
    revoked_at: Option<Instant>,
}

impl Grant {
    /// A grant from its terms, or why they are refused.
    pub fn new(terms: GrantTerms) -> Result<Self, GrantError> {
        let blank = |s: &str| s.trim().is_empty();
        if blank(&terms.id) || blank(&terms.provider) || blank(&terms.granted_by) {
            return Err(GrantError::NotAttributable);
        }
        if blank(&terms.party) {
            return Err(GrantError::NoNamedParty);
        }
        if terms.expires_at <= terms.granted_at {
            return Err(GrantError::ExpiresBeforeItStarts);
        }
        if terms.categories.is_empty() {
            return Err(GrantError::NoCategories);
        }
        if let Some(t) = terms
            .tuples
            .iter()
            .find(|t| !terms.categories.contains(&t.category))
        {
            return Err(GrantError::TupleOutsideCategories(t.category));
        }
        for t in &terms.tuples {
            super::tuples::check_tuple(t).map_err(GrantError::MalformedTuple)?;
        }
        for &category in &terms.categories {
            if category.moves_value() && !terms.tuples.iter().any(|t| t.category == category) {
                return Err(GrantError::CategoryWithoutTuples(category));
            }
        }
        Ok(Self {
            terms,
            revoked_at: None,
        })
    }

    /// The terms it was made with.
    pub fn terms(&self) -> GrantTerms {
        self.terms.clone()
    }

    /// The grant's identifier.
    pub fn id(&self) -> &str {
        &self.terms.id
    }

    /// The provider it admits.
    pub fn provider(&self) -> &str {
        &self.terms.provider
    }

    /// The legal party it is scoped to.
    pub fn party(&self) -> &str {
        &self.terms.party
    }

    /// The operator's person or role it is attributable to.
    pub fn granted_by(&self) -> &str {
        &self.terms.granted_by
    }

    /// When it starts.
    pub fn granted_at(&self) -> Instant {
        self.terms.granted_at
    }

    /// When it expires.
    pub fn expires_at(&self) -> Instant {
        self.terms.expires_at
    }

    /// Its reversal-capacity check.
    pub fn reversal_capacity(&self) -> &ReversalCapacity {
        &self.terms.reversal_capacity
    }

    pub(super) fn reversal_capacity_mut(&mut self) -> &mut ReversalCapacity {
        &mut self.terms.reversal_capacity
    }

    /// Where it stands at `at`. Revocation wins over expiry.
    pub fn standing(&self, at: Instant) -> Standing {
        if self.revoked_at.is_some_and(|r| at >= r) {
            Standing::Revoked
        } else if at < self.terms.granted_at {
            Standing::NotYetInForce
        } else if at >= self.terms.expires_at {
            Standing::Expired
        } else {
            Standing::InForce
        }
    }

    pub(super) fn revoke(&mut self, at: Instant) {
        self.revoked_at.get_or_insert(at);
    }

    /// Whether the grant admits this tuple.
    pub fn covers(&self, tuple: &TupleKey) -> bool {
        self.terms.tuples.contains(tuple)
    }

    /// Whether the grant admits this category.
    pub fn covers_category(&self, category: Category) -> bool {
        self.terms.categories.contains(&category)
    }
}
