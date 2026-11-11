//! Shared helpers for the routing rounds' tests (`T-020-06`): simulated providers that answer what they are asked
//! and keep it, the operator key, fresh identifiers, policy's charges and questions about people, a catalogue with a
//! ramp, and one environment that runs a routing. Built on `snapshot_support`, which `T-020-05` owns.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use ed25519_dalek::{Signer, SigningKey};
use rutent_contracts::core_internal::calls::{
    AppendFailure, Charges, Evidence, Fetched, Kept, KeptStatements, PersonQuestions, Signatures,
    StatementKeys,
};
use rutent_contracts::core_internal::{
    Actor, ActorKind, Charge, ChargeAnswer, ChargeOutcome, Entry, PersonAnswer, PersonOutcome,
    PersonQuestion, ReadCheck, Receipt, Role, RoutingRound, What,
};
use rutent_contracts::core_internal::{KeptStatement, SignedWith, Through, admit_entry, attribute};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::Deployment;
use rutent_contracts::provider_role::{
    Advertisement, EvidenceGrade, JUDGEMENT_SHAPE, PublicKey, RESULT_SHAPE, SafeInteger, Statement,
};
use rutent_contracts::router_api::{
    FeeFormula, Id, PolicyBounds, Request, Router, Timestamp, TupleKey, Version,
};
use rutent_core_orch::rounds::{
    self, Fresh, History, Issued, OperatorKey, Order, Parts, Presence, Providers, Routed, Routing,
    RoutingError,
};
use rutent_core_orch::snapshot::{Clock, Exposures, Price, Releases};
use rutent_prov::catalogue::Publisher;
use rutent_prov::catalogue::{
    Bond, Catalogue, Category, CheckBound, Grant, GrantTerms, Instant, ReversalCapacity, tuples_of,
};
use rutent_router::RouteFinder;
use serde_json::{Value, json};

use crate::snapshot_support::*;

/// The order every test routes.
pub const ORDER: &str = "ord-7";
/// The user's session, for the account `acct-1`.
pub const SESSION: &str = "session-1";
/// When the tests' clock starts.
pub const START: u64 = 2_000;

// The operator key, fresh identifiers ----------------------------------------------------------------------------

/// The deployment's operator key, as the assembly holds it.
pub struct TestKey(pub SigningKey);

impl OperatorKey for TestKey {
    fn key(&self) -> PublicKey {
        public_key(&self.0)
    }

    fn sign(&mut self, input: &[u8]) -> Option<[u8; 64]> {
        Some(self.0.sign(input).to_bytes())
    }
}

/// Identifiers drawn from a fixed-seed generator: unrelated to one another and to any order.
#[derive(Debug, Default)]
pub struct Ids(pub u64);

impl Fresh for Ids {
    fn id(&mut self) -> Id {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        id(&format!("q{:016x}", z ^ (z >> 31)))
    }
}

/// A source that gives the same identifier every time.
#[derive(Debug)]
pub struct SameId;

impl Fresh for SameId {
    fn id(&mut self) -> Id {
        id("q0000000000000001")
    }
}

// Policy -----------------------------------------------------------------------------------------------------------

fn receipt(n: usize) -> Receipt {
    Receipt {
        log: id("policy-log"),
        index: SafeInteger::try_from(n as u64).unwrap(),
        entry: serde_json::from_value(json!({ "algorithm": "sha-256", "value": "ab".repeat(32) }))
            .unwrap(),
    }
}

/// Policy's charges: one bound on fanned-out work per accountable party, every charge kept.
#[derive(Debug)]
pub struct Bound {
    pub bound: u64,
    pub used: BTreeMap<String, u64>,
    pub charges: Vec<Charge>,
}

impl Bound {
    pub fn of(bound: u64) -> Self {
        Self {
            bound,
            used: BTreeMap::new(),
            charges: Vec::new(),
        }
    }
}

impl Charges for Bound {
    fn charge(&mut self, charge: &Charge) -> Result<ChargeAnswer, AppendFailure> {
        charge.check().map_err(AppendFailure::Refused)?;
        self.charges.push(charge.clone());
        let used = self
            .used
            .entry(charge.party.as_str().to_owned())
            .or_default();
        let outcome = if *used + charge.count.get() > self.bound {
            ChargeOutcome::Over {
                bound: SafeInteger::try_from(self.bound).unwrap(),
            }
        } else {
            *used += charge.count.get();
            ChargeOutcome::Within {
                remaining: SafeInteger::try_from(self.bound - *used).unwrap(),
            }
        };
        Ok(ChargeAnswer {
            request: charge.request.clone(),
            outcome,
            receipt: receipt(self.charges.len()),
        })
    }
}

/// Policy's questions about people: each allowed, and kept.
#[derive(Debug, Default)]
pub struct People {
    pub questions: Vec<PersonQuestion>,
}

impl PersonQuestions for People {
    fn ask_about_person(
        &mut self,
        question: &PersonQuestion,
    ) -> Result<PersonAnswer, AppendFailure> {
        question.check().map_err(AppendFailure::Refused)?;
        self.questions.push(question.clone());
        Ok(PersonAnswer {
            request: question.request.clone(),
            outcome: PersonOutcome::Allowed {},
            receipt: receipt(self.questions.len()),
        })
    }
}

// The evidence store -------------------------------------------------------------------------------------------------

/// An evidence store that keeps every statement it is handed, but fails to keep any made by `refuse`.
#[derive(Debug, Default)]
pub struct Store {
    pub kept: Vec<Vec<u8>>,
    pub refuse: Option<String>,
}

impl Evidence for Store {
    fn keep(&mut self, received: &[u8]) -> Result<Kept, AppendFailure> {
        let maker = serde_json::from_slice::<Value>(received)
            .ok()
            .and_then(|v| v["maker"].as_str().map(str::to_owned));
        if self.refuse.is_some() && maker == self.refuse {
            return Err(AppendFailure::Unavailable);
        }
        self.kept.push(received.to_vec());
        Ok(Kept {
            commitment: commitment(self.kept.len() - 1),
            restates: None,
        })
    }

    fn fetch(&mut self, _check: &ReadCheck) -> Result<Fetched, AppendFailure> {
        Ok(Fetched::Erased)
    }
}

/// The commitment the store answers for the statement it kept at `place`.
fn commitment(place: usize) -> rutent_contracts::module_interface::Digest {
    serde_json::from_value(json!({ "algorithm": "sha-256", "value": format!("{place:064x}") }))
        .unwrap()
}

// Simulated providers ------------------------------------------------------------------------------------------------

/// Providers that answer each dry run they are asked, signed with their first statements key, at Core's clock, and
/// move the clock on a second each time; every question they receive is kept.
pub struct Sim {
    pub clock: Rc<TestClock>,
    /// Each question received: the provider asked, and the operation, as the envelope carried it.
    pub received: Vec<(String, Value)>,
    /// Each provider's fee, flat, in the asset it takes in; none is 0.
    pub fees: BTreeMap<String, u64>,
    /// Providers that refuse, finally.
    pub refuse: BTreeSet<String>,
    /// Providers that answer nothing.
    pub silent: BTreeSet<String>,
    /// Providers that sign with a key their advertisement doesn't list.
    pub wrong_key: BTreeSet<String>,
    /// Providers that answer with these bytes instead.
    pub relay: BTreeMap<String, Vec<u8>>,
    /// Verifiers that judge no.
    pub deny: BTreeSet<String>,
}

impl Sim {
    pub fn new(clock: Rc<TestClock>) -> Self {
        Self {
            clock,
            received: Vec::new(),
            fees: BTreeMap::new(),
            refuse: BTreeSet::new(),
            silent: BTreeSet::new(),
            wrong_key: BTreeSet::new(),
            relay: BTreeMap::new(),
            deny: BTreeSet::new(),
        }
    }

    /// The operations `provider` received.
    pub fn asked(&self, provider: &str) -> Vec<&Value> {
        self.received
            .iter()
            .filter(|(p, _)| p == provider)
            .map(|(_, op)| op)
            .collect()
    }
}

/// A provider's signed answer to `operation`, made at `made_at`: accepted with a flat `fee`, or refused.
pub fn answer(provider: &str, operation: &Value, made_at: u64, fee: u64, refuse: bool) -> Value {
    let kind = operation["kind"].as_str().unwrap();
    let amount: u64 = operation["amount"].as_str().unwrap().parse().unwrap();
    let asset = operation
        .get("asset")
        .or_else(|| operation.get("from"))
        .unwrap()
        .clone();
    let mut outcome = json!({
        "status": "dry_run_accepted",
        "amount_in": amount.to_string(),
        "amount_out": (amount - fee).to_string(),
        "fee": { "asset": asset, "amount": fee.to_string() },
        "valid_until": Instant(made_at + 3_600).rfc3339().unwrap(),
        "voided_by": ["validity_passes", "amount_changes"],
        "settlement_seconds": 60,
    });
    if kind == "transfer" {
        outcome["direction"] = "exact_in".into();
    }
    if refuse {
        outcome = json!({ "status": "refused", "refusal": { "class": "final" } });
    }
    envelope(
        &deployment(),
        RESULT_SHAPE,
        provider,
        made_at,
        json!({ "kind": kind, "transaction": operation["transaction"], "outcome": outcome }),
    )
}

impl Providers for Sim {
    fn ask(&mut self, provider: &Id, question: &[u8]) -> Option<Vec<u8>> {
        let question: Value = serde_json::from_slice(question).unwrap();
        let operation = question["statement"].clone();
        let provider = provider.as_str().to_owned();
        self.received.push((provider.clone(), operation.clone()));
        if self.silent.contains(&provider) {
            return None;
        }
        if let Some(bytes) = self.relay.get(&provider) {
            return Some(bytes.clone());
        }
        let now = self.clock.now().seconds;
        self.clock.set(now + 1);
        if operation["kind"] == "judgement" {
            let body = json!({
                "subject": operation["subject"],
                "class": operation["class"],
                "answer": if self.deny.contains(&provider) { "no" } else { "yes" },
                "valid_until": Instant(now + 86_400).rfc3339().unwrap(),
            });
            let value = envelope(&deployment(), JUDGEMENT_SHAPE, &provider, now, body);
            return Some(sign(value, &statements_key(&provider, 1)));
        }
        let fee = self.fees.get(&provider).copied().unwrap_or(0);
        let value = answer(
            &provider,
            &operation,
            now,
            fee,
            self.refuse.contains(&provider),
        );
        let n = if self.wrong_key.contains(&provider) {
            2
        } else {
            1
        };
        Some(sign(value, &statements_key(&provider, n)))
    }
}

// The environment ----------------------------------------------------------------------------------------------------

/// Everything a routing reaches but the providers.
pub struct Env {
    pub catalogue: Catalogue,
    pub clock: Rc<TestClock>,
    pub exposures: Exposures,
    pub releases: Releases,
    pub log: KeptLog,
    pub evidence: Store,
    pub bound: Bound,
    pub people: People,
    pub key: TestKey,
    pub ids: Box<dyn Fresh>,
    pub issued: Issued,
}

impl Env {
    /// An environment over `catalogue`, with balance checks bounded at ten per account in a day, answered within
    /// a minute, the user's session open until 50 000 if `session`, and fanned-out work bounded at `bound`.
    pub fn new(mut catalogue: Catalogue, session: bool, bound: u64) -> Self {
        catalogue.set_check_bound(CheckBound {
            max_counted: 10,
            window_secs: 86_400,
        });
        catalogue.set_answer_within(60);
        if session {
            catalogue.open_session(SESSION, "acct-1", Instant(50_000));
        }
        Self {
            catalogue,
            clock: Rc::new(TestClock::at(START)),
            exposures: Exposures::new(),
            releases: Releases::new(),
            log: KeptLog::default(),
            evidence: Store::default(),
            bound: Bound::of(bound),
            people: People::default(),
            key: TestKey(operator_key()),
            ids: Box::new(Ids::default()),
            issued: Issued::new(),
        }
    }

    /// Simulated providers on this environment's clock.
    pub fn sim(&self) -> Sim {
        Sim::new(self.clock.clone())
    }

    /// Runs one routing with `RouteFinder`.
    pub fn route(
        &mut self,
        sim: &mut Sim,
        routing: Routing<'_>,
        history: &mut History,
    ) -> Result<Routed, RoutingError> {
        self.route_with::<RouteFinder>(sim, routing, history)
    }

    /// Runs one routing with router `R`.
    pub fn route_with<R: Router>(
        &mut self,
        sim: &mut Sim,
        routing: Routing<'_>,
        history: &mut History,
    ) -> Result<Routed, RoutingError> {
        let mut parts = Parts {
            catalogue: &mut self.catalogue,
            clock: &*self.clock,
            exposures: &self.exposures,
            releases: &self.releases,
            log: &mut self.log,
            evidence: &mut self.evidence,
            charges: &mut self.bound,
            persons: &mut self.people,
            providers: sim,
            operator_key: &mut self.key,
            fresh: &mut *self.ids,
            issued: &mut self.issued,
        };
        rounds::route::<R>(&mut parts, routing, history)
    }

    /// Every `decision_record` entry, with its place in the log.
    pub fn records(&self) -> Vec<(usize, &What)> {
        self.log
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| match e {
                Entry::Verified(v) if matches!(v.what, What::DecisionRecord { .. }) => {
                    Some((i, &v.what))
                }
                _ => None,
            })
            .collect()
    }

    /// The place in the log of the stated entry holding `bytes`.
    pub fn stated_at(&self, bytes: &[u8]) -> Option<usize> {
        let kept = self.evidence.kept.iter().position(|b| b == bytes)?;
        let commitment = commitment(kept);
        self.log.entries.iter().position(|e| match e {
            Entry::Stated(s) => s.commitment == commitment,
            _ => false,
        })
    }
}

/// Whether `what` is a decision record of `round`.
pub fn is_round(what: &What, round: RoutingRound) -> bool {
    matches!(what, What::DecisionRecord { round: r, .. } if *r == round)
}

// The log's own reading ---------------------------------------------------------------------------------------------

/// The receipt the tests' log hands out for the entry at `index`.
fn log_receipt(index: usize) -> Receipt {
    Receipt {
        log: id("log-1"),
        index: SafeInteger::try_from(index as u64).unwrap(),
        entry: serde_json::from_value(json!({ "algorithm": "sha-256", "value": "72".repeat(32) }))
            .unwrap(),
    }
}

/// What the log reads to attribute a statement: the bytes the store kept, the catalogue's publishers and keys, and
/// its verification at Core's clock.
struct Sources<'a> {
    catalogue: &'a Catalogue,
    store: &'a Store,
    at: Instant,
}

impl KeptStatements for Sources<'_> {
    fn kept(&mut self, wanted: &Digest) -> Result<Option<KeptStatement>, AppendFailure> {
        let place = (0..self.store.kept.len()).find(|i| &commitment(*i) == wanted);
        Ok(place.map(|i| {
            serde_json::from_value(json!({
                "commitment": wanted,
                "salt": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                "bytes": base64url(&self.store.kept[i]),
            }))
            .unwrap()
        }))
    }
}

impl StatementKeys for Sources<'_> {
    fn deployment(&mut self) -> Result<Deployment, AppendFailure> {
        Ok(deployment())
    }

    fn operator(&mut self) -> Result<Id, AppendFailure> {
        Ok(operator().acts)
    }

    fn publisher(&mut self, maker: &Id) -> Result<Option<Id>, AppendFailure> {
        if maker == &operator().acts {
            return Ok(Some(operator().party));
        }
        Ok(
            match self.catalogue.publishers().publisher(maker.as_str()) {
                Some(Publisher::Operator) => Some(operator().party),
                Some(Publisher::Party(party)) => Id::new(party),
                None => None,
            },
        )
    }

    fn signs_for(&mut self, _: &Id, _: &SignedWith, _: &Timestamp) -> Result<bool, AppendFailure> {
        // The catalogue's verification below ties the key to its maker at its `made_at`.
        Ok(true)
    }
}

impl Signatures for Sources<'_> {
    fn verifies(&self, received: &[u8]) -> bool {
        let maker = serde_json::from_slice::<Value>(received)
            .ok()
            .and_then(|v| v["maker"].as_str().map(str::to_owned));
        if maker.as_deref() == Some(operator().acts.as_str()) {
            self.catalogue
                .verify_operator_statement(received, self.at)
                .is_ok()
        } else {
            self.catalogue.verify_statement(received, self.at).is_ok()
        }
    }
}

impl Env {
    /// The log as the log itself would keep it: each stated entry attributed from its kept bytes (core-internal's
    /// `attribute`), and every entry admitted over the ones before it on the general path (`admit_entry`), with its
    /// rows `answer-follows-its-question`, `transaction-belongs-to-one-order` and the decision records' chain.
    /// Refused at the first entry the log wouldn't admit, with its place.
    ///
    /// What it leaves out:
    /// - policy's question and charge entries go to the fakes ([`People`], [`Bound`]), not this log, so the rows over
    ///   them (`provider-asked-before-was-asked`, `asked-before-is-complete`, `shortlisted-authority-cites-its-shortlist`)
    ///   aren't exercised;
    /// - every receipt this log hands out shares one digest, so a row matching a receipt by its digest alone can't
    ///   tell two entries apart;
    /// - `signs_for` always answers true: whether a key signs for its maker at its `made_at` rests on the
    ///   catalogue's verification in `Signatures`, not on keys the log recorded itself.
    pub fn admitted(&self) -> Result<Vec<Entry>, (usize, String)> {
        let mut history: Vec<(Receipt, Entry)> = Vec::new();
        for (i, entry) in self.log.entries.iter().enumerate() {
            let entry = match entry {
                Entry::Stated(claim) => {
                    let mut sources = Sources {
                        catalogue: &self.catalogue,
                        store: &self.evidence,
                        at: self.clock.now().floor(),
                    };
                    let mut keys = Sources { ..sources };
                    let signatures = Sources { ..sources };
                    Entry::Stated(
                        attribute(claim, &mut sources, &mut keys, &signatures)
                            .map_err(|e| (i, format!("{e:?}")))?,
                    )
                }
                verified => verified.clone(),
            };
            admit_entry(&history, &entry, Through::General, &operator().acts)
                .map_err(|v| (i, v.0.to_owned()))?;
            history.push((log_receipt(i), entry));
        }
        Ok(history.into_iter().map(|(_, e)| e).collect())
    }
}

// Who acts ----------------------------------------------------------------------------------------------------------

/// The customer whose order it is.
pub fn customer() -> Actor {
    Actor {
        party: id("acme-customer"),
        acts: id("user-1"),
        role: Role::Customer,
        kind: ActorKind::Person,
    }
}

/// The operator, in whose name Core signs: the maker the log's `StatementKeys::operator` names.
pub fn operator() -> Actor {
    Actor {
        party: id("operator"),
        acts: id("operator"),
        role: Role::Operator,
        kind: ActorKind::Component,
    }
}

/// The order, from `origin`, with the user present in [`SESSION`].
pub fn order(origin: &str) -> Order {
    Order {
        order: id(ORDER),
        caller: customer(),
        origin_reserve: id(origin),
        presence: Presence::User {
            session: id(SESSION),
        },
        inputs: BTreeMap::from([("beneficiary.address".to_owned(), "0xbeef".to_owned())]),
        needs: Vec::new(),
    }
}

/// What one routing needs besides the environment.
pub struct Inputs {
    pub request: Request,
    pub bounds: PolicyBounds,
    pub prices: Vec<Price>,
    pub order: Order,
    pub core: Actor,
    pub operator: Actor,
}

impl Inputs {
    pub fn routing(&self) -> Routing<'_> {
        Routing {
            request: &self.request,
            bounds: &self.bounds,
            prices: &self.prices,
            order: &self.order,
            core: &self.core,
            operator: &self.operator,
        }
    }
}

/// Moving 1 000 USDC units on `polygon` through the given reserves, from `origin`, with free prices.
pub fn on_polygon(reserves: &[&str], origin: &str) -> Inputs {
    Inputs {
        request: shortlist_request("1000"),
        bounds: no_ceilings(),
        prices: prices(reserves),
        order: order(origin),
        core: core_actor(),
        operator: operator(),
    }
}

/// `bounds` excluding `providers`.
pub fn excluding(bounds: &PolicyBounds, providers: &[&str]) -> PolicyBounds {
    let mut value = serde_json::to_value(bounds).unwrap();
    value["excluded_providers"] = json!(providers);
    serde_json::from_value(value).unwrap()
}

// Ramps between two networks --------------------------------------------------------------------------------------

/// The reserve on `polygon` the order leaves from.
pub const FREE_RESERVE: &str = "polygon-reserve-a";
/// The reserve on `base` it arrives at.
pub const BASE_RESERVE: &str = "base-reserve";
/// Two ramps from `polygon` to `mid`; the second charges 10.
pub const RAMP_A: &str = "bridge-a";
pub const RAMP_B: &str = "bridge-b";
/// The ramp from `mid` to `base`, which both paths reach.
pub const RAMP_C: &str = "bridge-c";

fn polygon_to_base(value: &mut Value) {
    match value {
        Value::String(s) if s == "polygon" => *s = "base".into(),
        Value::Object(m) => m.values_mut().for_each(polygon_to_base),
        Value::Array(a) => a.iter_mut().for_each(polygon_to_base),
        _ => {}
    }
}

/// A ramp's advertisement, from `origin` to `destination`, in USDC.
pub fn ramp_ad(provider: &str, origin: &str, destination: &str) -> Advertisement {
    serde_json::from_value(json!({
        "provider": provider,
        "publisher": "acme",
        "version": "1.0.0",
        "keys": [],
        "offers": { "ramp": { "tuples": [{
            "origin": origin,
            "destination": destination,
            "asset": "USDC",
            "decimals": { "origin": 6, "destination": 6 },
            "terms": {
                "bands": [{ "min": "1", "max": "1000000000000" }],
                "reporting": "gross",
                "required_inputs": [],
            },
        }]}},
        "declarations": {
            "tracking": ["status_query"],
            "evidence": { "grade": 2, "availability": { "seconds": 2592000 } },
            "safe_retry": true,
        },
        "private_nodes": [],
    }))
    .unwrap()
}

fn bond() -> ReversalCapacity {
    ReversalCapacity::check(
        Bond {
            asset: "USDC".into(),
            amount: 1_000,
            held_by: "escrow-co".into(),
        },
        500,
        "ops-lead",
        Instant(1_000),
    )
    .unwrap()
}

fn grant_of(id: &str, ad: &Advertisement, category: Category) -> Grant {
    Grant::new(GrantTerms {
        id: id.into(),
        provider: ad.provider.clone(),
        party: "Acme Payments Ltd".into(),
        granted_by: "operator-admin".into(),
        granted_at: Instant(1_000),
        expires_at: Instant(100_000),
        categories: vec![category],
        tuples: if category == Category::Verifier {
            Vec::new()
        } else {
            tuples_of(ad)
        },
        reversal_capacity: bond(),
    })
    .unwrap()
}

/// Joins `ad` to `catalogue` under a grant of `category`, listing its first statements key.
fn join(catalogue: &mut Catalogue, grant: &str, ad: &Advertisement, category: Category) {
    catalogue.grant(grant_of(grant, ad, category)).unwrap();
    catalogue
        .trust_advertising_key(grant, public_key(&advertising_key(&ad.provider)))
        .unwrap();
    let keys = keys_of(&ad.provider, &[&statements_key(&ad.provider, 1)]);
    catalogue
        .join(
            &advertisement_bytes(ad, keys, JOINED_AT),
            Instant(JOINED_AT),
        )
        .unwrap();
    catalogue.set_module_version(&ad.provider, Version::new("1.0.0").unwrap());
    for key in tuples_of(ad) {
        catalogue.set_evidence_floor(key, EvidenceGrade::Statement);
    }
}

/// A verifier's advertisement, from provider-role's fixture, as `provider`: it judges `kyc.natural_person` and
/// `aml.sanctions_screening`.
pub fn verifier_ad(provider: &str) -> Advertisement {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../contracts/provider-role/fixtures/valid/advertisement-verifier-without-evidence-or-retry-declarations.json",
    );
    let ad: Statement<Advertisement> =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut ad = ad.statement;
    ad.provider = provider.into();
    ad
}

impl Env {
    /// Joins a verifier to the catalogue, granted judgements until 100 000, with an accepted release.
    pub fn add_verifier(&mut self, provider: &str) {
        let ad = verifier_ad(provider);
        join(
            &mut self.catalogue,
            &format!("g-{provider}"),
            &ad,
            Category::Verifier,
        );
        self.releases
            .record(&accepted_release(provider, json!([])))
            .unwrap();
    }
}

/// A catalogue of a reserve on `polygon`, one on `base`, two ramps from `polygon` to `mid` and one from `mid` to
/// `base`, each joined at [`JOINED_AT`].
pub fn ramp_catalogue() -> Catalogue {
    let mut catalogue = Catalogue::new();
    catalogue.set_context(deployment(), rutent_contracts::provider_role::VERSION);
    catalogue.publish_operator_module("wallet-module").unwrap();
    for node in ["polygon", "mid", "base"] {
        catalogue.add_public_node(node).unwrap();
        catalogue
            .register_public_holding(node, "wallet-module")
            .unwrap();
    }
    let mut base = reserve(BASE_RESERVE, "acme");
    let mut offers = serde_json::to_value(&base.offers).unwrap();
    polygon_to_base(&mut offers["reserve"]["tuples"]);
    base.offers = serde_json::from_value(offers).unwrap();
    let ads = [
        (reserve(FREE_RESERVE, "acme"), Category::Reserve),
        (base, Category::Reserve),
        (ramp_ad(RAMP_A, "polygon", "mid"), Category::Ramp),
        (ramp_ad(RAMP_B, "polygon", "mid"), Category::Ramp),
        (ramp_ad(RAMP_C, "mid", "base"), Category::Ramp),
    ];
    for (i, (ad, category)) in ads.iter().enumerate() {
        join(&mut catalogue, &format!("g-{i}"), ad, *category);
    }
    catalogue
}

fn endpoint(node: Value) -> Value {
    json!({ "node": node, "asset": usdc() })
}

fn tuple(provider: &str, from: Value, to: Value) -> TupleKey {
    serde_json::from_value(
        json!({ "provider": provider, "from": endpoint(from), "to": endpoint(to) }),
    )
    .unwrap()
}

/// The ramp both paths reach.
pub fn ramp_tuple() -> TupleKey {
    tuple(RAMP_C, json!("mid"), json!("base"))
}

/// A price of a flat `fee` and a minute's settlement for `tuple`, stated at [`JOINED_AT`].
pub fn priced(tuple: TupleKey, fee: u64) -> Price {
    Price {
        tuple,
        stated_at: Timestamp::new(Instant(JOINED_AT).rfc3339().unwrap()).unwrap(),
        fee: serde_json::from_value::<FeeFormula>(json!({
            "flat": fee.to_string(), "proportional": { "numerator": "0", "denominator": "1" }
        }))
        .unwrap(),
        rate: None,
        settlement_seconds: SafeInteger::try_from(60).unwrap(),
    }
}

/// Moving 1 000 USDC units from `polygon` to `base` over the ramp catalogue, from [`FREE_RESERVE`]: through
/// [`RAMP_A`] free, or [`RAMP_B`] for 10, then [`RAMP_C`].
pub fn polygon_to_base_inputs() -> Inputs {
    let outer = |network: &str| json!({ "network": network });
    let request = serde_json::from_value(json!({
        "request_id": "req-1",
        "round": { "kind": "shortlist" },
        "from": endpoint(outer("polygon")),
        "to": endpoint(outer("base")),
        "amount": "1000",
        "objective": { "kind": "lowest_total_cost" },
    }))
    .unwrap();
    Inputs {
        request,
        bounds: no_ceilings(),
        prices: vec![
            priced(tuple(FREE_RESERVE, outer("polygon"), json!("polygon")), 0),
            priced(tuple(RAMP_A, json!("polygon"), json!("mid")), 0),
            priced(tuple(RAMP_B, json!("polygon"), json!("mid")), 10),
            priced(ramp_tuple(), 0),
            priced(tuple(BASE_RESERVE, json!("base"), outer("base")), 0),
        ],
        order: order(FREE_RESERVE),
        core: core_actor(),
        operator: operator(),
    }
}

/// The ramp scenario: [`RAMP_A`] refuses and [`RAMP_B`] charges 10, so the choice round has only the dear path,
/// which reaches [`RAMP_C`] with 990 after it was asked at 1 000.
pub fn ramp_sim(env: &Env) -> Sim {
    let mut sim = env.sim();
    sim.refuse.insert(RAMP_A.into());
    sim.fees.insert(RAMP_B.into(), 10);
    sim
}

/// A statement's bytes, as a JSON value.
pub fn value_of(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap()
}

/// Statement bytes as a provider-role statement of any body.
pub fn statement_of(bytes: &[u8]) -> Statement<Value> {
    serde_json::from_slice(bytes).unwrap()
}
