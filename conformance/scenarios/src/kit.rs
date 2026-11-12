//! What every scenario kit shares (`T-024-02`): identifiers, who acts, Core's clock, fresh identifiers drawn from a
//! seed, the operator's key, the route the simulated reserve offers, the bounds the router runs under and the
//! prices Core holds.
//!
//! `T-020-08`'s segments kit (`tests/segments_support/`) and the assembly ([`crate::assembly`]) both build on these,
//! so a scenario and a segments test name one customer, one operator and one route.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;
use std::time::Duration;

use ed25519_dalek::Signer;
use rutent_conformance::role::SuiteContext;
use rutent_conformance::signing::{public_key, real};
use rutent_contracts::core_internal::{Actor, ActorKind, Role};
use rutent_contracts::provider_role::{PublicKey, SafeInteger};
use rutent_contracts::router_api::{FeeFormula, Id, PolicyBounds, Timestamp, TupleKey};
use rutent_core_orch::rounds::{Fresh, OperatorKey};
use rutent_core_orch::snapshot::{Price, Reading};
use rutent_prov::catalogue::{
    Bond, Catalogue, Category, Grant, GrantTerms, Instant, ReversalCapacity, tuples_of,
};
use serde_json::{Value, json};

use crate::sim::Installed;
use crate::sim::world::WORLD_AT;

/// The simulated reserve every route of the segments' tests and the assembly's base scenario runs through.
pub const RESERVE: &str = "sim-reserve-a";

/// The amount the base route moves, in the reserve's asset's unit.
pub const AMOUNT: &str = "1000000";

/// The settlement time each simulated tuple states, in seconds.
pub const DECLARED: u64 = 60;

/// An identifier, which `text` must be.
///
/// # Panics
///
/// If `text` isn't one.
pub fn id(text: &str) -> Id {
    Id::new(text).expect("a valid identifier")
}

// Who acts ----------------------------------------------------------------------------------------------------------

/// The customer: a person of the requester's party.
pub fn customer() -> Actor {
    Actor {
        party: id("acme-customer"),
        acts: id("user-1"),
        role: Role::Customer,
        kind: ActorKind::Person,
    }
}

/// Core's own accountable party, apart from the deployment operator's (`T-021-06`): only Core's own components act
/// for it, so Core is never the operator's, and never decides a stuck order or moves its value.
pub const CORE_PARTY: &str = "rutent";

/// Core, acting on its own account, and carrying the user's authorisation onward.
pub fn core() -> Actor {
    Actor {
        party: id(CORE_PARTY),
        acts: id("rutent-core"),
        role: Role::Core,
        kind: ActorKind::Component,
    }
}

/// The operator, in whose name Core signs: the operations' maker, the simulated providers' operator.
pub fn operator() -> Actor {
    Actor {
        party: id("operator"),
        acts: id(&SuiteContext::default().operator),
        role: Role::Operator,
        kind: ActorKind::Component,
    }
}

/// The operator's login a person uses to change the catalogue: a login of its own, held to the permissions the
/// register lists for it (`REQ-AUZ-6`).
pub fn operator_login() -> Actor {
    Actor {
        party: id("operator"),
        acts: id("op-login-1"),
        role: Role::Operator,
        kind: ActorKind::Person,
    }
}

// Core's clock ------------------------------------------------------------------------------------------------------

/// Core's one clock, shared by everything that reads a time in a scenario: Core's snapshots and segments, policy's
/// session bound, the log's skew and the simulated world (`F-021`'s notes: the assembly hands policy and Core the
/// one clock). It reads in nanoseconds since the epoch and moves only when a scenario moves it, or by
/// [`SharedClock::TICK`] on each reading, so two runs of a scenario read the same times in the same order.
#[derive(Debug, Clone)]
pub struct SharedClock(Rc<Cell<u128>>);

impl SharedClock {
    /// How far each reading moves the clock: a millisecond, so no two readings are equal and a time read later is
    /// later.
    pub const TICK: u128 = 1_000_000;

    /// A clock at `seconds` since the epoch.
    pub fn at(seconds: u64) -> Self {
        Self(Rc::new(Cell::new(u128::from(seconds) * 1_000_000_000)))
    }

    /// Now, in nanoseconds, moving the clock by a tick.
    pub fn read(&self) -> u128 {
        let now = self.0.get();
        self.0.set(now + Self::TICK);
        now
    }

    /// Now, in nanoseconds, without moving it.
    pub fn peek(&self) -> u128 {
        self.0.get()
    }

    /// Sets the clock to `nanos` since the epoch, never back: a later time is kept.
    pub fn set_nanos(&self, nanos: u128) {
        self.0.set(self.0.get().max(nanos));
    }

    /// Moves the clock on by `seconds`.
    pub fn advance(&self, seconds: u64) {
        self.0
            .set(self.0.get() + u128::from(seconds) * 1_000_000_000);
    }

    /// Now, on the catalogue's clock: whole seconds, rounded down.
    pub fn instant(&self) -> Instant {
        Instant(u64::try_from(self.peek() / 1_000_000_000).unwrap_or(u64::MAX))
    }

    /// Now, as a timestamp, rounded up to the second, as Core's snapshots stamp it.
    pub fn timestamp(&self) -> Timestamp {
        self.reading()
            .timestamp()
            .expect("a scenario's clock reads a time a timestamp holds")
    }

    fn reading(&self) -> Reading {
        let nanos = self.read();
        Reading::of(Duration::from_nanos(
            u64::try_from(nanos).unwrap_or(u64::MAX),
        ))
    }
}

impl rutent_core_orch::snapshot::Clock for SharedClock {
    fn now(&self) -> Reading {
        self.reading()
    }
}

impl rutent_core_authz::session::Clock for SharedClock {
    fn now(&self) -> Duration {
        Duration::from_nanos(u64::try_from(self.read()).unwrap_or(u64::MAX))
    }
}

impl rutent_core_audit::log::Clock for SharedClock {
    fn unix_nanos(&self) -> i128 {
        i128::try_from(self.read()).unwrap_or(i128::MAX)
    }
}

/// When the base scenario starts: two days and an hour after the simulated world's clock, so the providers'
/// grants and advertisements are in force.
pub const START: u64 = WORLD_AT.0 + 2 * 86_400 + 3_600;

// Fresh identifiers ------------------------------------------------------------------------------------------------

/// Fresh identifiers drawn from a seed (`DEC-18`): SplitMix64 over the seed, so the same seed draws the same
/// identifiers in the same order, and no identifier is drawn twice.
#[derive(Debug, Clone)]
pub struct Seeded {
    state: u64,
    drawn: BTreeSet<u64>,
}

impl Seeded {
    /// Identifiers drawn from `seed`.
    pub fn new(seed: u64) -> Self {
        Self {
            state: seed,
            drawn: BTreeSet::new(),
        }
    }

    /// The next number of the sequence.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number below `bound`, which must be above 0.
    pub fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound.max(1)
    }
}

impl Fresh for Seeded {
    fn id(&mut self) -> Id {
        loop {
            let n = self.next_u64();
            if self.drawn.insert(n) {
                return id(&format!("id-{n:016x}"));
            }
        }
    }
}

/// Fresh identifiers, counted from 1: `id-0001`, `id-0002`, and so on.
#[derive(Debug, Default)]
pub struct Counted(pub u64);

impl Fresh for Counted {
    fn id(&mut self) -> Id {
        self.0 += 1;
        id(&format!("id-{:04}", self.0))
    }
}

// The operator's key -----------------------------------------------------------------------------------------------

/// The deployment's operator key, as the simulated providers know it: its placeholder is the deployment's
/// `operator_key`, and the real key it stands for signs (the segments kit's convention).
pub struct PlaceholderKey(pub PublicKey);

impl OperatorKey for PlaceholderKey {
    fn key(&self) -> PublicKey {
        self.0.clone()
    }

    fn sign(&mut self, input: &[u8]) -> Option<[u8; 64]> {
        Some(real(self.0.as_str()).sign(input).to_bytes())
    }
}

/// The deployment's operator key in the assembly: a real key, the one the deployment names, so the log verifies
/// Core's statements by it (`REQ-AUD-2`).
pub struct RealKey(pub ed25519_dalek::SigningKey);

impl RealKey {
    /// The assembly's operator key.
    pub fn assembly() -> Self {
        Self(rutent_conformance::signing::signing_key(
            "the operator of rutent-scenarios",
            0,
        ))
    }

    /// Its public half.
    pub fn public(&self) -> PublicKey {
        public_key(&self.0)
    }
}

impl OperatorKey for RealKey {
    fn key(&self) -> PublicKey {
        self.public()
    }

    fn sign(&mut self, input: &[u8]) -> Option<[u8; 64]> {
        Some(self.0.sign(input).to_bytes())
    }
}

// The route --------------------------------------------------------------------------------------------------------

/// The simulated reserves' asset.
pub fn cop() -> Value {
    json!({ "id": "COP", "decimals": 2 })
}

/// `reserve`'s bank network, as an endpoint.
pub fn bank(reserve: &str) -> Value {
    json!({ "node": { "network": format!("{reserve}-bank") }, "asset": cop() })
}

/// `reserve`'s ledger, its private node, as an endpoint.
pub fn ledger(reserve: &str) -> Value {
    json!({ "node": format!("{reserve}-ledger"), "asset": cop() })
}

/// `reserve`'s tuple from `from` to `to`.
pub fn tuple(reserve: &str, from: Value, to: Value) -> TupleKey {
    serde_json::from_value(json!({ "provider": reserve, "from": from, "to": to })).expect("a tuple")
}

/// The bounds the router runs under in `context`: four hops, one part, no ceiling, no fee.
pub fn bounds(context: &SuiteContext) -> PolicyBounds {
    serde_json::from_value(json!({
        "deployment": context.deployment,
        "protocol_version": context.protocol_version,
        "policy_version": "policy-1",
        "max_hops": 4,
        "max_parts": 1,
        "excluded_providers": [],
        "excluded_tuples": [],
        "provider_ceilings": [],
        "publisher_ceilings": [],
        "operator_fee": { "flat": "0", "proportional": { "numerator": "0", "denominator": "1" } },
        "slippage_limits": [],
    }))
    .expect("bounds")
}

/// Core's price of `tuple`: no fee, no rate, settling in [`DECLARED`] seconds, stated at the world's clock.
pub fn price(tuple: TupleKey) -> Price {
    Price {
        tuple,
        stated_at: Timestamp::new(WORLD_AT.rfc3339().expect("a time")).expect("a timestamp"),
        fee: serde_json::from_value::<FeeFormula>(json!({
            "flat": "0", "proportional": { "numerator": "0", "denominator": "1" }
        }))
        .expect("a fee formula"),
        rate: None,
        settlement_seconds: SafeInteger::try_from(DECLARED).expect("a safe integer"),
    }
}

/// The prices of `reserve`'s withdrawal from its bank to its ledger and its deposit back.
pub fn reserve_prices(reserve: &str) -> Vec<Price> {
    vec![
        price(tuple(reserve, bank(reserve), ledger(reserve))),
        price(tuple(reserve, ledger(reserve), bank(reserve))),
    ]
}

// The catalogue -----------------------------------------------------------------------------------------------------

/// The categories `installed`'s advertisement offers.
fn categories_of(installed: &Installed) -> Vec<Category> {
    let ad = &installed.advertisement.statement().statement;
    [
        (ad.offers.reserve.is_some(), Category::Reserve),
        (ad.offers.exchange.is_some(), Category::Exchange),
        (ad.offers.ramp.is_some(), Category::Ramp),
        (ad.offers.verifier.is_some(), Category::Verifier),
    ]
    .into_iter()
    .filter_map(|(offered, category)| offered.then_some(category))
    .collect()
}

/// The terms of `installed`'s grant: every tuple it advertises, for every category it offers, granted by
/// `granted_by` at `granted_at`, as `grant-<provider>`, with a reversal capacity of 500 against a bond of 1 000. The
/// party, the expiry, the categories and the reversal capacity are the deployment's settings: core-internal's grant
/// carries none of them.
///
/// # Panics
///
/// If the reversal capacity doesn't check.
pub fn grant_terms(installed: &Installed, granted_by: &str, granted_at: Instant) -> GrantTerms {
    let ad = &installed.advertisement.statement().statement;
    let capacity = ReversalCapacity::check(
        Bond {
            asset: "USDC".into(),
            amount: 1_000,
            held_by: "escrow-co".into(),
        },
        500,
        "ops-lead",
        Instant(1_000),
    )
    .expect("a capacity");
    GrantTerms {
        id: format!("grant-{}", ad.provider),
        provider: ad.provider.clone(),
        party: format!("{} Ltd", ad.publisher),
        granted_by: granted_by.to_owned(),
        granted_at,
        expires_at: Instant(5_000_000_000),
        categories: categories_of(installed),
        tuples: tuples_of(ad),
        reversal_capacity: capacity,
    }
}

/// Grants `terms` in Core's catalogue and trusts `installed`'s advertising key under it.
///
/// # Panics
///
/// If the catalogue refuses either.
pub fn grant(catalogue: &mut Catalogue, installed: &Installed, terms: GrantTerms) {
    let id = terms.id.clone();
    catalogue
        .grant(Grant::new(terms).expect("a grant"))
        .expect("granted");
    catalogue
        .trust_advertising_key(
            &id,
            installed.advertisement.statement().signature.key.clone(),
        )
        .expect("trusted");
}

/// Joins `installed`'s advertisement to Core's catalogue at `at`, once it is granted: its module's version, a
/// statement's evidence floor on each tuple, and each private node it declares registered as its publisher's holding.
///
/// # Panics
///
/// If the catalogue refuses the advertisement.
pub fn join(catalogue: &mut Catalogue, installed: &Installed, at: Instant) {
    let ad = &installed.advertisement.statement().statement;
    catalogue
        .join(installed.advertisement.bytes(), at)
        .expect("joined");
    catalogue.set_module_version(
        &ad.provider,
        rutent_contracts::router_api::Version::new("1.0.0").expect("a version"),
    );
    for key in tuples_of(ad) {
        catalogue.set_evidence_floor(
            key,
            rutent_contracts::provider_role::EvidenceGrade::Statement,
        );
    }
    for node in &ad.private_nodes {
        let _ = catalogue.register_private_holding(node, &ad.publisher, "holding");
    }
}

/// Lists `installed`'s advertisement in Core's catalogue as a test kit does, with no log: granted by
/// `operator-admin` at the epoch's 1 000th second, and joined at the simulated world's clock. The assembly grants
/// through a decided catalogue change instead ([`crate::assembly::Core::grant`]).
pub fn list(catalogue: &mut Catalogue, installed: &Installed) {
    grant(
        catalogue,
        installed,
        grant_terms(installed, "operator-admin", Instant(1_000)),
    );
    join(catalogue, installed, WORLD_AT);
}

/// The identifier the log names `provider`'s catalogue tuple `tuple` by in a grant (`change_catalogue`'s `tuple`),
/// which an outside checker recomputes from the advertisement: `tuple-` and the first 32 hex digits of SHA-256 over
/// the RFC 8785 canonical JSON of `{"provider", "category", "origin", "destination", "asset", "to_asset"}`, each in
/// provider-role's own form (a category by its snake-case name, an inner node by its identifier, an outer node as
/// `{"network": ...}`), `to_asset` only where the tuple has one.
///
/// # Panics
///
/// If the JSON has no canonical form, which a tuple's never lacks.
pub fn tuple_id(provider: &str, tuple: &rutent_prov::catalogue::TupleKey) -> Id {
    let node = |end: &rutent_prov::catalogue::Endpoint| match end {
        rutent_prov::catalogue::Endpoint::Inner(n) => json!(n),
        rutent_prov::catalogue::Endpoint::Outer(n) => json!({ "network": n }),
    };
    let mut value = json!({
        "provider": provider,
        "category": tuple.category,
        "origin": node(&tuple.origin),
        "destination": node(&tuple.destination),
        "asset": tuple.asset,
    });
    if let Some(to) = &tuple.to_asset {
        value["to_asset"] = to.clone().into();
    }
    let canonical =
        rutent_contracts::provider_role::to_canonical(&value).expect("a tuple's canonical JSON");
    let digest = rutent_prov::module::wire::sha256(&canonical);
    id(&format!("tuple-{}", &digest[..32]))
}
