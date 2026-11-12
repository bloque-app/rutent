//! The kit for the segments' tests (`T-020-08`): simulated providers admitted to the platform, a catalogue of their
//! advertisements, a route the router chose over them, and Core's parts around one log that the tests replay as the
//! log itself would admit it.

#![allow(dead_code)]

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;
use std::time::Duration;

use ed25519_dalek::{Signer, Verifier};
use rutent_conformance::role::SuiteContext;
use rutent_conformance::signing::real;
use rutent_conformance::sim::{Role as SimRole, Spec};
use rutent_contracts::core_internal::CatalogueChange;
use rutent_contracts::core_internal::Effect;
use rutent_contracts::core_internal::calls::{
    AppendFailure, Authorisations, DecisionEntry, Decisions, Evidence, Fetched, GeneralEntry, Kept,
    KeptStatements, Log, PolicyLog, Reads, Signatures, StatementKeys,
};
use rutent_contracts::core_internal::{
    AcceptanceBasis, Actor, ActorKind, Authorisation, Decision, DecisionRequest, Destination,
    Entry, KeptStatement, Outcome, Parties, PermissionChange, PublicMovement, ReadAnswer,
    ReadCheck, Receipt, RefusalClass, Role, RouteEnd, RoutingRound, SignedWith, Through,
    TimeSource, VerifiedEntry, VerifiedKind, What, When, admit_entry, attribute,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::module_interface::{Identifier, Name, PlatformRefusal};
use rutent_contracts::provider_role::{
    Denomination, Deployment, EvidenceGrade, NodeRef, OuterNode, PublicKey, SafeInteger,
    canonicalize, signing_input,
};
use rutent_contracts::provider_role::{Operation as ProviderOperation, Statement};
use rutent_contracts::router_api::{
    CatalogueSnapshot, DecisionRecord, Endpoint, Id, OperationKind, PositiveAmount, Request,
    Router, Timestamp,
};
use rutent_contracts::state_machine::{
    self as stm, Observed, PartPlan, RecordedInput, SegmentPlan, Source, SourceKind, Standing,
    Subject,
};
use rutent_core_authz::engine::{self as authz, Engine, Input, Need, Reply, Ruling, Said};
use rutent_core_authz::roles::Permission;
use rutent_core_orch::rounds::Issued;
use rutent_core_orch::segments::{
    Answered, Graded, Grading, Live, Modules, NoProxy, Parts, ProvidersWord, Ran, Segments,
    SegmentsError, ToGrade, Uses,
};
use rutent_core_orch::snapshot::{
    Ask, Asked, Clock, Exposures, Reading, Received, Releases, Snapshots,
};
use rutent_prov::catalogue::{Catalogue, Instant, Publisher};
use rutent_prov::module::lifecycle::{LifecycleRefusal, SegmentRecord, Staging};
use rutent_prov::module::platform::Platform;
use rutent_prov::module::types::Events;
use rutent_prov::module::types::Handled;
use rutent_prov::module::wire::sha256;
use rutent_prov::proxy::RouteNotNamed;
use rutent_prov::proxy::Use;
use rutent_router::RouteFinder;
use rutent_scenarios::sim::SimWorld;
use rutent_scenarios::sim::{Fleet, Installed};
use serde_json::{Value, json};

// What every scenario kit shares is the assembly's kit (`T-024-02`): the route, who acts, the operator's key, fresh
// identifiers, the bounds and the prices, and the listing of an advertisement in Core's catalogue.
// Each test crate that mounts this kit uses its own share of these.
use rutent_scenarios::kit::list;
pub use rutent_scenarios::kit::price;
#[allow(unused_imports)]
pub use rutent_scenarios::kit::{
    AMOUNT, Counted as Ids, DECLARED, PlaceholderKey as TestKey, RESERVE, START, bank, bounds,
    core, customer, id, ledger, operator, tuple,
};
/// The order.
pub const ORDER: &str = "ord-1";
/// The part.
pub const PART: &str = "p1";
/// The withdrawal's segment.
pub const WITHDRAWAL: &str = "p1-s1";
/// The deposit's segment.
pub const DEPOSIT: &str = "p1-s2";

// Core's clock, keys and identifiers --------------------------------------------------------------------------------

/// Core's clock, which a test moves.
pub struct TestClock(pub Cell<Reading>);

impl TestClock {
    pub fn at(seconds: u64) -> Self {
        Self(Cell::new(Reading::at(seconds)))
    }

    pub fn set(&self, seconds: u64) {
        self.0.set(Reading::at(seconds));
    }

    pub fn advance(&self, seconds: u64) {
        let now = self.0.get();
        self.0.set(Reading::at(now.seconds + seconds));
    }
}

impl Clock for TestClock {
    fn now(&self) -> Reading {
        self.0.get()
    }
}

// The log -----------------------------------------------------------------------------------------------------------

/// What the log does with the next append that matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// It can't take it now: nothing is appended.
    Unavailable,
    /// It takes it, and its answer is lost: the caller sees `Unavailable`.
    LostAnswer,
    /// It answers with the receipt of the first effect it holds, as a log that mixed up its answers would: the
    /// entry is not appended.
    Replay,
    /// It refuses it: nothing is appended (`T-020-11`).
    Refuse,
}

/// Which entries a fault is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum On {
    /// Any effect.
    Effect,
    /// The effect of the segment named.
    EffectOf(&'static str),
    /// Any item handed over.
    HandOver,
    /// A segment's record, handed over.
    Record,
    /// A provider's result, as its stated entry.
    StatedResult,
    /// The result of the segment named, as its stated entry (`T-020-11`).
    StatedResultOf(&'static str),
    /// The input that binds a segment's provider.
    Bound,
    /// A result, once the log holds one already.
    LaterResult,
    /// An order's refusal.
    Refusal,
    /// A part's route recalculated (`T-020-10`).
    Recalculated,
    /// A part waiting at its floor (`T-020-10b`).
    FloorWait,
    /// A part declared stuck, or left no route (`T-020-11`).
    Stuck,
    /// A raise to the operator (`T-020-11`).
    Raise,
    /// A segment's graded evidence (`T-020-09`).
    Evidence,
    /// A segment's fee record (`T-020-09`).
    Fee,
}

impl On {
    fn matches(self, entry: &Entry) -> bool {
        match (self, entry) {
            (On::Effect, Entry::Verified(v)) => matches!(v.what, What::Effect { .. }),
            (On::EffectOf(segment), Entry::Verified(v)) => matches!(&v.what,
                What::Effect { action: rutent_contracts::core_internal::Action::MoveValue { segment: s, .. }, .. }
                    if s.as_str() == segment),
            (On::HandOver, Entry::Verified(v)) => matches!(v.what, What::HandOver { .. }),
            (On::Record, Entry::Verified(v)) => matches!(
                v.what,
                What::HandOver {
                    item: rutent_contracts::core_internal::HandedOver::SegmentRecord { .. },
                    ..
                }
            ),
            (On::StatedResult, Entry::Stated(s)) => {
                s.what.statement.as_str() == rutent_contracts::provider_role::RESULT_SHAPE
            }
            (On::StatedResultOf(segment), Entry::Stated(s)) => {
                s.what.statement.as_str() == rutent_contracts::provider_role::RESULT_SHAPE
                    && s.about
                        .as_ref()
                        .and_then(|a| a.segment.as_ref())
                        .is_some_and(|g| g.as_str() == segment)
            }
            (On::Refusal, Entry::Verified(v)) => matches!(v.what, What::OrderRefusal { .. }),
            (On::Recalculated, Entry::Verified(v)) => matches!(&v.what,
                What::Input { input } if matches!(input.observed, Observed::RouteRecalculated { .. })),
            (On::FloorWait, Entry::Verified(v)) => matches!(&v.what,
                What::Input { input } if input.observed == Observed::WaitingAtFloor {}),
            (On::Bound, Entry::Verified(v)) => matches!(&v.what,
                What::Input { input } if input.observed == Observed::ProviderBound {}),
            (On::Raise, Entry::Verified(v)) => matches!(v.what, What::Raise { .. }),
            (On::Stuck, Entry::Verified(v)) => matches!(&v.what,
                What::Input { input } if matches!(input.observed,
                    Observed::StuckDeclared { .. } | Observed::NoRoute { .. })),
            (On::Evidence, Entry::Verified(v)) => matches!(v.what, What::Evidence { .. }),
            (On::Fee, Entry::Verified(v)) => matches!(v.what, What::Fee { .. }),
            _ => false,
        }
    }
}

/// The log: every entry with the handle it came through. An equal entry appended again is one entry, with its first
/// receipt.
#[derive(Default)]
pub struct TestLog {
    pub entries: Vec<(Entry, Through)>,
    pub faults: VecDeque<(On, Fault)>,
}

impl TestLog {
    fn take(&mut self, entry: &Entry, through: Through) -> Result<Receipt, AppendFailure> {
        entry.check().map_err(AppendFailure::Refused)?;
        let results = self
            .entries
            .iter()
            .filter(
                |(e, _)| matches!(e, Entry::Verified(v) if matches!(v.what, What::Result { .. })),
            )
            .count();
        let later_result = matches!(entry, Entry::Verified(v) if matches!(v.what, What::Result { .. }))
            && results > 0;
        let fault = match self.faults.front() {
            Some((On::LaterResult, fault)) if later_result => {
                let fault = *fault;
                self.faults.pop_front();
                Some(fault)
            }
            Some((on, fault)) if on.matches(entry) => {
                let fault = *fault;
                self.faults.pop_front();
                Some(fault)
            }
            _ => None,
        };
        if fault == Some(Fault::Unavailable) {
            return Err(AppendFailure::Unavailable);
        }
        if fault == Some(Fault::Refuse) {
            return Err(AppendFailure::Refused(
                rutent_contracts::core_internal::Violation("the test log refuses it"),
            ));
        }
        if fault == Some(Fault::Replay) {
            let (index, first) = self
                .entries
                .iter()
                .enumerate()
                .find(|(_, (e, _))| On::Effect.matches(e))
                .expect("an effect to replay");
            return Ok(receipt(index, &first.0));
        }
        let index = match self.entries.iter().position(|(e, _)| e == entry) {
            Some(index) => index,
            None => {
                self.entries.push((entry.clone(), through));
                self.entries.len() - 1
            }
        };
        if fault == Some(Fault::LostAnswer) {
            return Err(AppendFailure::Unavailable);
        }
        Ok(receipt(index, entry))
    }

    /// The receipt this log gave the entry at `index` (`T-020-10`: an order's history resumed from the log).
    pub fn receipt_at(&self, index: usize) -> Receipt {
        receipt(index, &self.entries[index].0)
    }

    /// Every verified entry's `what`, with its place.
    pub fn records(&self) -> Vec<(usize, &What)> {
        self.entries
            .iter()
            .enumerate()
            .filter_map(|(i, (e, _))| match e {
                Entry::Verified(v) => Some((i, &v.what)),
                Entry::Stated(_) => None,
            })
            .collect()
    }

    /// Every verified entry's `what` and outcome.
    pub fn verified(&self) -> Vec<&VerifiedEntry> {
        self.entries
            .iter()
            .filter_map(|(e, _)| match e {
                Entry::Verified(v) => Some(v),
                Entry::Stated(_) => None,
            })
            .collect()
    }

    /// Every input the segments recorded, as the state machine reads it.
    pub fn inputs(&self) -> Vec<&RecordedInput> {
        self.verified()
            .into_iter()
            .filter_map(|v| match &v.what {
                What::Input { input } => Some(input),
                _ => None,
            })
            .collect()
    }

    pub fn count(&self, pick: impl Fn(&What) -> bool) -> usize {
        self.verified().iter().filter(|v| pick(&v.what)).count()
    }
}

fn receipt(index: usize, entry: &Entry) -> Receipt {
    let bytes = canonicalize(&serde_json::to_vec(entry).unwrap()).unwrap();
    Receipt {
        log: id("log-1"),
        index: SafeInteger::try_from(index as u64).unwrap(),
        entry: digest(&sha256(&bytes)),
    }
}

pub fn digest(hex: &str) -> Digest {
    serde_json::from_value(json!({ "algorithm": "sha-256", "value": hex })).unwrap()
}

/// Orchestration's handle on the shared log.
pub struct General(pub Rc<RefCell<TestLog>>);

impl Log for General {
    fn append(&mut self, entry: &GeneralEntry) -> Result<Receipt, AppendFailure> {
        self.0.borrow_mut().take(entry.entry(), Through::General)
    }
}

// The evidence store ------------------------------------------------------------------------------------------------

#[derive(Default)]
pub struct Store {
    pub kept: Vec<Vec<u8>>,
}

impl Evidence for Store {
    fn keep(&mut self, received: &[u8]) -> Result<Kept, AppendFailure> {
        self.kept.push(received.to_vec());
        Ok(Kept {
            commitment: commitment(self.kept.len() - 1),
            restates: None,
        })
    }

    fn fetch(&mut self, _: &ReadCheck) -> Result<Fetched, AppendFailure> {
        Ok(Fetched::Erased)
    }
}

fn commitment(place: usize) -> Digest {
    digest(&format!("{place:064x}"))
}

// Policy ------------------------------------------------------------------------------------------------------------

/// Policy's handle on the shared log.
pub struct PolicyPath(pub Rc<RefCell<TestLog>>);

impl PolicyLog for PolicyPath {
    fn append_decision(&mut self, entry: &DecisionEntry) -> Result<Receipt, AppendFailure> {
        self.0
            .borrow_mut()
            .take(&Entry::Verified(entry.entry().clone()), Through::Policy)
    }
}

/// Policy's read of the log, on its own handle: the authorisations the log holds as permitted, so the engine's register
/// is the log's (`T-021-04`).
impl rutent_core_authz::funds::Recorded for PolicyPath {
    fn permitted(&mut self) -> Result<Vec<Authorisation>, rutent_core_authz::funds::ReadFailure> {
        let log = self.0.borrow();
        Ok(rutent_core_authz::funds::permitted_in(
            log.entries.iter().map(|(entry, _)| entry),
        ))
    }

    fn charged(
        &mut self,
    ) -> Result<Vec<rutent_core_authz::parties::Counted>, rutent_core_authz::funds::ReadFailure>
    {
        let log = self.0.borrow();
        Ok(rutent_core_authz::parties::charged_in(
            log.entries.iter().map(|(entry, _)| entry),
        ))
    }

    fn part_of(
        &mut self,
        order: &Id,
        segment: &Id,
    ) -> Result<Option<Id>, rutent_core_authz::funds::ReadFailure> {
        let log = self.0.borrow();
        Ok(rutent_core_authz::funds::part_in(
            log.entries.iter().map(|(entry, _)| entry),
            order,
            segment,
        ))
    }
}

/// Policy's read of a stuck order's steps, each entry with the receipt the log gave it (`T-021-06`).
impl rutent_core_authz::stuck::Receipted for PolicyPath {
    fn receipted(
        &mut self,
    ) -> Result<Vec<(Receipt, Entry)>, rutent_core_authz::funds::ReadFailure> {
        let log = self.0.borrow();
        Ok((0..log.entries.len())
            .map(|i| (log.receipt_at(i), log.entries[i].0.clone()))
            .collect())
    }
}

/// The deployment's policy: permits whatever the fixed rules let through, or refuses everything with `refuse`.
pub struct Deployment_ {
    pub refuse: Rc<Cell<Option<RefusalClass>>>,
}

impl authz::Policy for Deployment_ {
    fn needs(&mut self, _: authz::Asked<'_>) -> Reply<Vec<Need>> {
        Reply::Given(Vec::new())
    }

    fn rule(&mut self, _: authz::Asked<'_>, _: &[Input]) -> Reply<Ruling> {
        Reply::Given(match self.refuse.get() {
            Some(class) => Ruling::Refuse(class),
            None => Ruling::Permit,
        })
    }
}

/// The deployment's registers, as the policy engine asks them: which actor acts for which party
/// (`Source::ActsFor`), which actor holds which permission (`Source::Permissions`: Core's actor carries the user's
/// authorisation onward, the operator's login promotes a staged release and reads an order's state and age), whose
/// order this is (the customer is the requester of every order the kit seeds, and its party of [`ORDER`]), and which
/// provider a segment ran at: every segment the kit's routes run is the reserve's (`T-020-11`).
pub struct Register;

impl authz::Sources for Register {
    fn supply(&mut self, need: &Need) -> Reply<Said> {
        let yes = |b: bool| Reply::Given(if b { Said::Yes } else { Said::No });
        match &need.from {
            authz::Source::ActsFor { acts, party, kind } => {
                let listed = [
                    core(),
                    customer(),
                    operator(),
                    provider_desk(),
                    other_provider_desk(),
                ];
                match listed.iter().find(|a| &a.acts == acts) {
                    Some(actor) => yes(&actor.party == party && &actor.kind == kind),
                    None => Reply::Missing,
                }
            }
            authz::Source::Permissions { permission, .. } => yes(match permission {
                Permission::CarryOnward => need.input == core().acts,
                Permission::Change(CatalogueChange::Promote) | Permission::ReadOrderState => {
                    need.input == operator().acts
                }
                _ => false,
            }),
            authz::Source::OrderParties(order) => {
                yes(order.as_str() == ORDER && need.input == customer().party)
            }
            authz::Source::SegmentProvider { .. } => yes(need.input.as_str() == RESERVE),
            // The customer authorises each order the kit seeds through the engine (`T-021-04`).
            authz::Source::Requester(_) => yes(need.input == customer().party),
            _ => Reply::Missing,
        }
    }
}

/// Policy: the policy engine itself, its fixed rules first, over the deployment's policy and registers, appending
/// each decision through policy's handle before it answers. What it was asked is kept.
pub struct Policy {
    engine: Engine<PolicyPath, Deployment_, Register>,
    refuse: Rc<Cell<Option<RefusalClass>>>,
    pub asked: Vec<DecisionRequest>,
}

impl Policy {
    pub fn new(log: Rc<RefCell<TestLog>>) -> Self {
        let refuse = Rc::new(Cell::new(None));
        let engine = Engine::with_policy(
            PolicyPath(log),
            Deployment_ {
                refuse: Rc::clone(&refuse),
            },
            Register,
        );
        Self {
            engine,
            refuse,
            asked: Vec::new(),
        }
    }

    /// From now on, the deployment's policy refuses with `class`.
    pub fn refuse(&self, class: RefusalClass) {
        self.refuse.set(Some(class));
    }

    /// The deployment's switch, to refuse from within a run and permit again after it (`T-020-11`).
    pub fn refuser(&self) -> Rc<Cell<Option<RefusalClass>>> {
        Rc::clone(&self.refuse)
    }
}

impl Decisions for Policy {
    fn decide(&mut self, request: &DecisionRequest) -> Result<Decision, AppendFailure> {
        self.asked.push(request.clone());
        self.engine.decide(request)
    }
}

impl Authorisations for Policy {
    fn authorise(&mut self, authorisation: &Authorisation) -> Result<Decision, AppendFailure> {
        self.engine.authorise(authorisation)
    }
}

impl Reads for Policy {
    fn check_read(&mut self, check: &ReadCheck) -> Result<ReadAnswer, AppendFailure> {
        self.engine.check_read(check)
    }

    fn grant_permission(&mut self, change: &PermissionChange) -> Result<Decision, AppendFailure> {
        self.engine.grant_permission(change)
    }

    fn revoke_permission(&mut self, change: &PermissionChange) -> Result<Decision, AppendFailure> {
        self.engine.revoke_permission(change)
    }
}

// Who acts, besides the shared kit's -----------------------------------------------------------------------------

/// The reserve's own desk, a person acting for the reserve: the provider of every segment the kit's routes run
/// (`T-020-11`).
pub fn provider_desk() -> Actor {
    Actor {
        party: id(RESERVE),
        acts: id("sim-reserve-a-desk"),
        role: Role::Provider,
        kind: ActorKind::Person,
    }
}

/// Another provider's desk, which ran no segment of the kit's routes (`T-020-11`).
pub fn other_provider_desk() -> Actor {
    Actor {
        party: id("sim-reserve-b"),
        acts: id("sim-reserve-b-desk"),
        role: Role::Provider,
        kind: ActorKind::Person,
    }
}

// The fleet and its catalogue ---------------------------------------------------------------------------------------

/// A deadline short enough that silence is quick.
pub const DEADLINE: Duration = Duration::from_secs(2);

/// The simulated providers, and a catalogue of their advertisements as the operator lists them.
pub struct World {
    pub fleet: Fleet,
    pub catalogue: Catalogue,
    pub installed: Vec<Installed>,
}

impl World {
    pub fn of(reserves: &[&str]) -> Self {
        let mut fleet = Fleet::new(DEADLINE);
        let context = fleet.context().clone();
        let mut catalogue = Catalogue::new();
        catalogue.set_context(context.deployment.clone(), &context.protocol_version);
        let mut installed = Vec::new();
        for reserve in reserves {
            let spec = Spec::of(SimRole::Reserve, reserve);
            let done = fleet.install(spec);
            list(&mut catalogue, &done);
            installed.push(done);
        }
        // What the routing's questions left behind is not the segments'.
        let _ = fleet.platform.take_events();
        World {
            fleet,
            catalogue,
            installed,
        }
    }

    pub fn context(&self) -> SuiteContext {
        self.fleet.context().clone()
    }
}

// The route --------------------------------------------------------------------------------------------------------

/// The route the router chooses for [`AMOUNT`] from `reserve`'s bank network back to it, through its ledger: its
/// withdrawal and its deposit, confirmed by the reserve's own dry runs.
pub fn chosen_route(world: &mut World, clock: &TestClock) -> DecisionRecord {
    let context = world.context();
    let bounds = bounds(&context);
    let withdrawal = tuple(RESERVE, bank(RESERVE), ledger(RESERVE));
    let deposit = tuple(RESERVE, ledger(RESERVE), bank(RESERVE));
    let prices = [price(withdrawal.clone()), price(deposit.clone())];
    let exposures = Exposures::new();
    let releases = Releases::new();
    let snapshots = Snapshots::new(&world.catalogue, clock, &exposures, &releases);
    let request = |round: Value, request_id: &str| -> Request {
        serde_json::from_value(json!({
            "request_id": request_id,
            "round": round,
            "from": bank(RESERVE),
            "to": bank(RESERVE),
            "amount": AMOUNT,
            "objective": { "kind": "lowest_total_cost" },
        }))
        .unwrap()
    };
    // The shortlist is taken before the reserve's dry runs are made, and the choice after.
    let started = clock.now().seconds;
    clock.set(started - 7_200);
    let shortlist = snapshots
        .form(&Ask {
            snapshot_id: "snap-1",
            bounds: &bounds,
            prices: &prices,
            statements: &[],
        })
        .expect("a shortlist snapshot");
    let shortlist = RouteFinder::route(
        &request(json!({ "kind": "shortlist" }), "req-shortlist"),
        &shortlist.snapshot,
        &bounds,
    );
    shortlist.check().expect("a shortlist record");
    // The reserve's dry runs, as the routing asks them.
    let dry = |world: &mut World, kind: &str, transaction: &str, from: Value, to: Value| {
        let mut statement = json!({
            "kind": kind,
            "transaction": transaction,
            "provider": RESERVE,
            "asset": "COP",
            "amount": AMOUNT,
            "dry_run": true,
        });
        if kind == "withdrawal" {
            statement["inputs"] = json!({ "payer.bank_account": "acct-1" });
            statement["session"] = context.session.clone().into();
            statement["origin"] = from["node"].clone();
            statement["destination"] = to["node"].clone();
        } else {
            statement["inputs"] = json!({ "beneficiary.bank_account": "acct-2" });
            statement["origin"] = from["node"].clone();
            statement["destination"] = to["node"].clone();
        }
        let operation = rutent_conformance::sim::operation(&context, statement);
        match world.fleet.handle(RESERVE, &operation) {
            Handled::Answered(result) => serde_json::to_vec(&*result).unwrap(),
            Handled::Refused(why) => panic!("the dry run was refused: {why:?}"),
        }
    };
    let statements = vec![
        Received {
            tuple: withdrawal.clone(),
            asked: Asked::DryRun {
                amount_in: PositiveAmount::new(
                    rutent_contracts::router_api::Amount::new(AMOUNT).unwrap(),
                )
                .unwrap(),
            },
            bytes: dry(
                world,
                "withdrawal",
                "tx-dry-w",
                bank(RESERVE),
                ledger(RESERVE),
            ),
        },
        Received {
            tuple: deposit.clone(),
            asked: Asked::DryRun {
                amount_in: PositiveAmount::new(
                    rutent_contracts::router_api::Amount::new(AMOUNT).unwrap(),
                )
                .unwrap(),
            },
            bytes: dry(world, "deposit", "tx-dry-d", ledger(RESERVE), bank(RESERVE)),
        },
    ];
    let _ = world.fleet.platform.take_events();
    clock.set(started);
    let snapshots = Snapshots::new(&world.catalogue, clock, &exposures, &releases);
    let choice = snapshots
        .form(&Ask {
            snapshot_id: "snap-2",
            bounds: &bounds,
            prices: &prices,
            statements: &statements,
        })
        .expect("a choice snapshot");
    assert!(
        choice.refused.is_empty(),
        "{:?}",
        choice
            .refused
            .iter()
            .map(|r| (&r.reason, String::from_utf8_lossy(&r.bytes).into_owned()))
            .collect::<Vec<_>>()
    );
    let record = RouteFinder::route(
        &request(
            json!({ "kind": "choice", "refines": {
                "request_id": "req-shortlist",
                "taken_at": shortlist.snapshot.taken_at,
            }}),
            "req-choice",
        ),
        &choice.snapshot,
        &bounds,
    );
    record.check().expect("a choice record");
    assert!(
        record.chosen().is_some(),
        "the router chose a route: {:?}",
        record
            .candidates()
            .iter()
            .map(|c| &c.dropped)
            .collect::<Vec<_>>()
    );
    record
}

// The order, as the log holds it before its segments run ------------------------------------------------------------

/// What the route ends at.
pub fn route_end() -> RouteEnd {
    RouteEnd {
        destination: Destination {
            reserve: id(RESERVE),
            node: OuterNode {
                network: format!("{RESERVE}-bank"),
            },
            account: id("acct-2"),
        },
        to_asset: Denomination {
            id: "COP".into(),
            decimals: 2,
        },
        delivers: rutent_contracts::router_api::Amount::new(AMOUNT).unwrap(),
    }
}

/// An order, as the tests name it and its segments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    pub order: String,
    pub withdrawal: String,
    pub deposit: String,
}

impl Named {
    /// The order every test runs: [`ORDER`], [`WITHDRAWAL`] and [`DEPOSIT`].
    pub fn first() -> Self {
        Named {
            order: ORDER.into(),
            withdrawal: WITHDRAWAL.into(),
            deposit: DEPOSIT.into(),
        }
    }

    /// Another order, `n`, over the same route.
    pub fn another(n: u8) -> Self {
        Named {
            order: format!("ord-{n}"),
            withdrawal: format!("p1-s1-of-{n}"),
            deposit: format!("p1-s2-of-{n}"),
        }
    }

    pub fn authorisation(&self) -> String {
        format!("auth-{}", self.order)
    }
}

/// The plan of the order's one part.
pub fn plan(record: &DecisionRecord) -> PartPlan {
    plan_of(record, &Named::first())
}

/// The plan of `named`'s one part.
pub fn plan_of(record: &DecisionRecord, named: &Named) -> PartPlan {
    let operations = &record.chosen().unwrap().parts[0].operations;
    let ids = [named.withdrawal.as_str(), named.deposit.as_str()];
    PartPlan {
        part_id: stm::Id::new(PART).unwrap(),
        segments: operations
            .iter()
            .zip(ids)
            .map(|(o, s)| SegmentPlan {
                segment_id: stm::Id::new(s).unwrap(),
                provider: stm::Id::new(o.provider.as_str()).unwrap(),
                kind: o.kind,
                from: o.from.clone(),
                to: o.to.clone(),
            })
            .collect(),
    }
}

/// The `route_chosen` input the order was opened by.
pub fn route_chosen(record: &DecisionRecord) -> RecordedInput {
    route_chosen_of(record, &Named::first())
}

/// The `route_chosen` input `named` was opened by.
pub fn route_chosen_of(record: &DecisionRecord, named: &Named) -> RecordedInput {
    RecordedInput {
        record_id: stm::RecordId::new(format!("rec-chosen-{}", named.order)).unwrap(),
        recorded_at: stm::Timestamp::new(Instant(START).rfc3339().unwrap()).unwrap(),
        time_source: stm::Id::new("core_clock").unwrap(),
        source: Source {
            kind: SourceKind::Customer,
            id: stm::Id::new("user-1").unwrap(),
        },
        standing: Standing::Stated,
        subject: Subject {
            order: stm::Id::new(named.order.as_str()).unwrap(),
            part: None,
            segment: None,
        },
        observed: Observed::RouteChosen {
            decision_record: stm::Id::new(record.record_id().as_str()).unwrap(),
            parts: vec![plan_of(record, named)],
            handed_on_from: None,
        },
    }
}

/// The second part of a split order (`T-020-10`): the same route as [`PART`], under its own segment ids.
pub const SECOND_PART: &str = "p2";
/// The second part's withdrawal.
pub const SECOND_WITHDRAWAL: &str = "p2-s1";
/// The second part's deposit.
pub const SECOND_DEPOSIT: &str = "p2-s2";

/// The `route_chosen` input of `named` split in two parts over the same route, [`PART`] and [`SECOND_PART`]
/// (`T-020-10`). The router never splits, so the kit does.
pub fn route_chosen_split(record: &DecisionRecord, named: &Named) -> RecordedInput {
    let mut chosen = route_chosen_of(record, named);
    let Observed::RouteChosen { parts, .. } = &mut chosen.observed else {
        unreachable!("a route chosen");
    };
    let mut second = parts[0].clone();
    second.part_id = stm::Id::new(SECOND_PART).unwrap();
    for (segment, id) in second
        .segments
        .iter_mut()
        .zip([SECOND_WITHDRAWAL, SECOND_DEPOSIT])
    {
        segment.segment_id = stm::Id::new(id).unwrap();
    }
    parts.push(second);
    chosen
}

/// Appends what the log holds of the order before its segments run: the authorisation, the deposit's arrival at the
/// origin reserve, on the reserve's report to the operator's question, the acceptance (an automatic deposit's, which
/// needs no yes and rests on that arrival), the router's decision record and the route chosen. The authorisation is
/// the customer's, given through `policy`, so the engine holds the order's moves to it (`T-021-04`).
pub fn seed(
    log: &Rc<RefCell<TestLog>>,
    policy: &mut Policy,
    store: &mut Store,
    record: &DecisionRecord,
    operator: &Actor,
) {
    seed_order(log, policy, store, record, &Named::first(), operator);
}

/// A stated entry of `bytes`, kept in `store`, claimed by `maker`, unverified, about `order` and `transaction`.
fn stated_seed(
    store: &mut Store,
    bytes: Vec<u8>,
    maker: Actor,
    order: &Id,
    transaction: &str,
) -> Entry {
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    let kept = store.keep(&bytes).unwrap();
    serde_json::from_value(json!({
        "kind": "stated",
        "attribution": { "signature": "unverified", "claimed_by": maker },
        "what": { "statement": value["shape"] },
        "when": { "at": value["made_at"], "source": "party_stated" },
        "received_at": value["made_at"],
        "outcome": { "status": "stated" },
        "signed_with": { "scheme": "ed25519", "key": value["signature"]["key"] },
        "commitment": kept.commitment,
        "about": { "order": order, "transaction": transaction },
    }))
    .unwrap()
}

/// An envelope of `shape` made by `maker` at the start, around `body`, unsigned.
fn seed_envelope(shape: &str, maker: &str, body: Value) -> Value {
    json!({
        "deployment": SuiteContext::default().deployment,
        "protocol_version": rutent_contracts::provider_role::VERSION,
        "shape": shape,
        "maker": maker,
        "made_at": Instant(START).rfc3339().unwrap(),
        "signature": { "scheme": "ed25519", "key": "", "value": "" },
        "statement": body,
    })
}

/// A second simulated reserve, of another party: installed beside [`RESERVE`] where a move's outcome is reported
/// and confirmed, so its two results are by two makers the catalogue knows, neither the operator.
pub const OTHER_RESERVE: &str = "sim-reserve-b";

/// Two results about the current order through `transaction`, and about `segment` where one is named, made at Core's
/// clock now and appended to the log now: [`RESERVE`]'s and [`OTHER_RESERVE`]'s, each signed with its maker's
/// statements key, so the log verifies each. A move's outcome is reported and confirmed on two such results, which
/// follow the move, by two providers' parties (`outcome-confirmation-cites-verified-evidence`,
/// `outcome-confirmation-rests-on-its-report`). The world must have [`OTHER_RESERVE`] installed.
pub fn results_after_the_move(
    core: &mut Core,
    segment: Option<&str>,
    transaction: &str,
) -> (Receipt, Receipt) {
    let order = id(&core.named.order);
    let made_at = core.clock.now().timestamp().unwrap();
    let body = json!({ "kind": "balance_verification", "transaction": transaction });
    let mut result = |maker: &str| {
        let mut e = seed_envelope(
            rutent_contracts::provider_role::RESULT_SHAPE,
            maker,
            body.clone(),
        );
        e["made_at"] = made_at.as_str().into();
        rutent_conformance::signing::sign_value(&mut e, &statements_key_of(maker));
        let publisher = Spec::of(SimRole::Reserve, maker)
            .make(&SuiteContext::default())
            .advertisement()
            .into_statement()
            .statement
            .publisher;
        let actor = Actor {
            party: id(&publisher),
            acts: id(maker),
            role: Role::Provider,
            kind: ActorKind::Component,
        };
        match stated_seed(
            &mut core.store,
            serde_json::to_vec(&e).unwrap(),
            actor,
            &order,
            transaction,
        ) {
            Entry::Stated(mut stated) => {
                stated.about = Some(rutent_contracts::core_internal::About {
                    order: Some(order.clone()),
                    segment: segment.map(id),
                    transaction: Some(id(transaction)),
                });
                Entry::Stated(stated)
            }
            verified => verified,
        }
    };
    let (first, second) = (result(RESERVE), result(OTHER_RESERVE));
    // A stuck resolution's transaction has no question under it yet: the move under it, which the production
    // resolutions would hand the provider (unbuilt), is asked here as the operator's operation about the order,
    // naming no segment, so the providers' results answer it (`answer-follows-its-question`).
    let question = segment.is_none().then(|| {
        let placeholder = SuiteContext::default().deployment.operator_key;
        let mut question = seed_envelope(
            rutent_contracts::provider_role::OPERATION_SHAPE,
            core.operator.acts.as_str(),
            body.clone(),
        );
        question["made_at"] = made_at.as_str().into();
        question["signature"]["key"] = placeholder.as_str().into();
        question["signature"]["value"] = rutent_conformance::signing::base64url(&[0u8; 64]).into();
        let input = signing_input(&serde_json::to_vec(&question).unwrap()).unwrap();
        question["signature"]["value"] = rutent_conformance::signing::base64url(
            &real(placeholder.as_str()).sign(&input).to_bytes(),
        )
        .into();
        stated_seed(
            &mut core.store,
            serde_json::to_vec(&question).unwrap(),
            core.operator.clone(),
            &order,
            transaction,
        )
    });
    let mut log = core.log.borrow_mut();
    if let Some(question) = question {
        log.take(&question, Through::General).unwrap();
    }
    let first = log.take(&first, Through::General).unwrap();
    let second = log.take(&second, Through::General).unwrap();
    (first, second)
}

/// The deposit's arrival at the origin reserve, on the reserve's report to the operator's question: one deposit, and
/// one arrival, for each part of `chosen`'s route, each naming its part where the route has more than one
/// (`split-deposit-arrival-names-its-part`).
fn seed_arrival(
    log: &mut TestLog,
    store: &mut Store,
    record: &DecisionRecord,
    named: &Named,
    operator: &Actor,
    chosen: &RecordedInput,
    entry: &dyn Fn(What, Outcome) -> Entry,
) {
    let order = id(&named.order);
    let Observed::RouteChosen { parts, .. } = &chosen.observed else {
        panic!("a route chosen");
    };
    let split = parts.len() > 1;
    for plan in parts {
        let transaction = if split {
            format!("tx-deposit-{}-{}", named.order, plan.part_id.as_str())
        } else {
            format!("tx-deposit-{}", named.order)
        };
        let body = json!({ "kind": "balance_verification", "transaction": transaction });
        // The question, signed as the operator, under the placeholder key the log reads it by.
        let placeholder = SuiteContext::default().deployment.operator_key;
        let mut question = seed_envelope(
            rutent_contracts::provider_role::OPERATION_SHAPE,
            operator.acts.as_str(),
            body.clone(),
        );
        question["signature"]["key"] = placeholder.as_str().into();
        question["signature"]["value"] = rutent_conformance::signing::base64url(&[0u8; 64]).into();
        let input = signing_input(&serde_json::to_vec(&question).unwrap()).unwrap();
        question["signature"]["value"] = rutent_conformance::signing::base64url(
            &real(placeholder.as_str()).sign(&input).to_bytes(),
        )
        .into();
        let question = stated_seed(
            store,
            serde_json::to_vec(&question).unwrap(),
            operator.clone(),
            &order,
            &transaction,
        );
        log.take(&question, Through::General).unwrap();
        // The reserve's report, signed with its statements key.
        let mut report =
            seed_envelope(rutent_contracts::provider_role::RESULT_SHAPE, RESERVE, body);
        rutent_conformance::signing::sign_value(&mut report, &statements_key_of(RESERVE));
        let publisher = Spec::of(SimRole::Reserve, RESERVE)
            .make(&SuiteContext::default())
            .advertisement()
            .into_statement()
            .statement
            .publisher;
        let reserve = Actor {
            party: id(&publisher),
            acts: id(RESERVE),
            role: Role::Provider,
            kind: ActorKind::Component,
        };
        let report = stated_seed(
            store,
            serde_json::to_vec(&report).unwrap(),
            reserve,
            &order,
            &transaction,
        );
        // The deposit's notice is this report: it met the order at once.
        let notice = match &report {
            Entry::Stated(stated) => stated.commitment.clone(),
            Entry::Verified(_) => unreachable!("a stated report"),
        };
        let report = log.take(&report, Through::General).unwrap();
        let chosen = record.chosen().unwrap();
        let first = chosen.parts.first().operations.first();
        let NodeRef::Outer(node) = &first.from.node else {
            panic!("the route starts at an outer node");
        };
        log.take(
            &entry(
                What::Arrival {
                    reserve: id(RESERVE),
                    node: node.clone(),
                    asset: first.from.asset.clone(),
                    amount: first.amount_in.amount().clone(),
                    order: Some(order.clone()),
                    matched: rutent_contracts::core_internal::ArrivalMatch::Matches,
                    report,
                    notice: Some(notice),
                    part: split.then(|| id(plan.part_id.as_str())),
                },
                Outcome::Observed {},
            ),
            Through::General,
        )
        .unwrap();
    }
}

/// The authorisation the kit seeds for `named`, under `floor` (`T-020-10`).
pub fn authorisation_of(named: &Named, floor: Option<PositiveAmount>) -> Authorisation {
    let end = route_end();
    Authorisation {
        authorisation: id(&named.authorisation()),
        order: id(&named.order),
        origin_reserve: id(RESERVE),
        // The payer's account at the reserve, the one the kit's withdrawal names (`payer.bank_account`).
        origin_account: Some(id("acct-1")),
        destination: end.destination,
        to_asset: end.to_asset,
        floor,
        parts: None,
        given_by: customer(),
        at: Timestamp::new(Instant(START).rfc3339().unwrap()).unwrap(),
    }
}

/// [`seed`], for `named`.
pub fn seed_order(
    log: &Rc<RefCell<TestLog>>,
    policy: &mut Policy,
    store: &mut Store,
    record: &DecisionRecord,
    named: &Named,
    operator: &Actor,
) {
    seed_order_under(log, policy, store, record, named, operator, None);
}

/// [`seed_order`], its authorisation naming `floor` (`T-020-10`).
pub fn seed_order_under(
    log: &Rc<RefCell<TestLog>>,
    policy: &mut Policy,
    store: &mut Store,
    record: &DecisionRecord,
    named: &Named,
    operator: &Actor,
    floor: Option<PositiveAmount>,
) {
    seed_chosen(
        log,
        policy,
        store,
        record,
        named,
        operator,
        floor,
        route_chosen_of(record, named),
    );
}

/// [`seed_order_under`], the order opened by `chosen`.
#[allow(clippy::too_many_arguments)]
fn seed_chosen(
    log: &Rc<RefCell<TestLog>>,
    policy: &mut Policy,
    store: &mut Store,
    record: &DecisionRecord,
    named: &Named,
    operator: &Actor,
    floor: Option<PositiveAmount>,
    chosen: RecordedInput,
) {
    let (order, authorisation) = (id(&named.order), id(&named.authorisation()));
    let at = Timestamp::new(Instant(START).rfc3339().unwrap()).unwrap();
    let entry = |what: What, outcome: Outcome| {
        Entry::Verified(VerifiedEntry {
            kind: VerifiedKind::Verified,
            who: core(),
            what,
            when: When {
                at: at.clone(),
                source: TimeSource::CoreClock,
            },
            outcome,
        })
    };
    let authorised = policy.authorise(&authorisation_of(named, floor)).unwrap();
    assert!(
        authorised.permit().is_some(),
        "the customer authorises its order through policy"
    );
    let mut log = log.borrow_mut();
    seed_arrival(&mut log, store, record, named, operator, &chosen, &entry);
    log.take(
        &entry(
            What::Acceptance {
                acceptance: id(&format!("acc-{}", named.order)),
                order: order.clone(),
                request: id(&format!("req-{}", named.order)),
                quote: digest(&"ab".repeat(32)),
                authorisation: authorisation.clone(),
                basis: AcceptanceBasis::AutomaticDeposit,
                parties: Parties {
                    requester: customer().party,
                    beneficiary: customer().party,
                    settlement_account_at: None,
                    payer: customer().party,
                },
                origin_reserve: id(RESERVE),
                yeses: Vec::new(),
                session: None,
            },
            Outcome::Observed {},
        ),
        Through::General,
    )
    .unwrap();
    log.take(
        &entry(
            What::DecisionRecord {
                decision_record: record.record_id().clone(),
                order: order.clone(),
                round: RoutingRound::Choice,
                record: digest(&"cd".repeat(32)),
                previous: None,
                shortlisted: None,
                verifiers: Vec::new(),
            },
            Outcome::Observed {},
        ),
        Through::General,
    )
    .unwrap();
    log.take(
        &entry(What::Input { input: chosen }, Outcome::Observed {}),
        Through::General,
    )
    .unwrap();
}

// Core around the platform ------------------------------------------------------------------------------------------

/// Uses of the proxy, as a world with one would report them: what a test hands it, drained once.
#[derive(Default)]
pub struct Reported(pub Vec<Use>);

impl Uses for Reported {
    fn take_uses(&mut self) -> Vec<Use> {
        std::mem::take(&mut self.0)
    }
}

/// Core's parts and state around one log, the world, the clock and policy.
pub struct Core {
    pub log: Rc<RefCell<TestLog>>,
    pub store: Store,
    pub policy: Policy,
    pub clock: Rc<TestClock>,
    pub ids: Ids,
    pub issued: Issued,
    pub exposures: Exposures,
    pub segments: Segments,
    pub record: DecisionRecord,
    pub inputs: BTreeMap<String, String>,
    pub key: TestKey,
    /// The operator, as Core signs in its name and the log reads it: its party is the one a module the operator
    /// publishes states for.
    pub operator: Actor,
    /// The order the runs are of.
    pub named: Named,
    /// The floor its authorisation names (`T-020-10`).
    pub floor: Option<PositiveAmount>,
}

impl Core {
    /// Core with the order's route chosen over `world` and its log seeded.
    pub fn over(world: &mut World) -> Self {
        Self::over_with(world, operator(), |_| {})
    }

    /// [`Core::over`], the order's authorisation naming `floor` (`T-020-10`).
    pub fn under(world: &mut World, floor: Option<PositiveAmount>) -> Self {
        Self::over_under(world, operator(), floor, |_| {})
    }

    /// [`Core::over`], with `operator` as the operator Core signs as, and `before` appended to the log first.
    pub fn over_with(
        world: &mut World,
        operator: Actor,
        before: impl FnOnce(&Rc<RefCell<TestLog>>),
    ) -> Self {
        Self::over_under(world, operator, None, before)
    }

    /// [`Core::over_with`], the order's authorisation naming `floor`.
    fn over_under(
        world: &mut World,
        operator: Actor,
        floor: Option<PositiveAmount>,
        before: impl FnOnce(&Rc<RefCell<TestLog>>),
    ) -> Self {
        Self::opened(world, operator, floor, before, route_chosen_of)
    }

    /// [`Core::over`], the order split in two parts over the same route ([`route_chosen_split`], `T-020-10`).
    pub fn split(world: &mut World) -> Self {
        Self::opened(world, operator(), None, |_| {}, route_chosen_split)
    }

    fn opened(
        world: &mut World,
        operator: Actor,
        floor: Option<PositiveAmount>,
        before: impl FnOnce(&Rc<RefCell<TestLog>>),
        open: fn(&DecisionRecord, &Named) -> RecordedInput,
    ) -> Self {
        let clock = Rc::new(TestClock::at(START));
        let record = chosen_route(world, &clock);
        let log = Rc::new(RefCell::new(TestLog::default()));
        before(&log);
        let mut store = Store::default();
        let mut policy = Policy::new(Rc::clone(&log));
        let chosen = open(&record, &Named::first());
        seed_chosen(
            &log,
            &mut policy,
            &mut store,
            &record,
            &Named::first(),
            &operator,
            floor.clone(),
            chosen.clone(),
        );
        let mut segments = Segments::new();
        assert!(segments.follow(&*clock, &chosen));
        let context = world.context();
        Core {
            policy,
            log,
            store,
            clock,
            ids: Ids::default(),
            issued: Issued::new(),
            exposures: Exposures::new(),
            segments,
            record,
            inputs: BTreeMap::from([
                ("payer.bank_account".to_owned(), "acct-1".to_owned()),
                ("beneficiary.bank_account".to_owned(), "acct-2".to_owned()),
            ]),
            key: TestKey(context.deployment.operator_key.clone()),
            operator,
            named: Named::first(),
            floor,
        }
    }

    /// Seeds another order over the same route, follows it, and runs it from now on.
    pub fn take_up(&mut self, named: Named) {
        let operator = self.operator.clone();
        seed_order(
            &self.log,
            &mut self.policy,
            &mut self.store,
            &self.record,
            &named,
            &operator,
        );
        assert!(
            self.segments
                .follow(&*self.clock, &route_chosen_of(&self.record, &named))
        );
        self.named = named;
    }

    /// Runs the order's part over `modules`, with `uses` reported and `grading` grading.
    pub fn run_with(
        &mut self,
        catalogue: &Catalogue,
        modules: &mut dyn Modules,
        uses: &mut dyn Uses,
        grading: &mut dyn Grading,
        session: &str,
    ) -> Ran {
        self.try_run_with(catalogue, modules, uses, grading, session)
            .expect("the part runs")
    }

    /// [`Core::run_with`], with why it couldn't run.
    pub fn try_run_with(
        &mut self,
        catalogue: &Catalogue,
        modules: &mut dyn Modules,
        uses: &mut dyn Uses,
        grading: &mut dyn Grading,
        session: &str,
    ) -> Result<Ran, SegmentsError> {
        let (order, part, auth, origin, end) = (
            id(&self.named.order),
            id(PART),
            id(&self.named.authorisation()),
            id(RESERVE),
            route_end(),
        );
        let segments = [id(&self.named.withdrawal), id(&self.named.deposit)];
        let (caller, core_actor, operator_actor) = (core(), core(), self.operator.clone());
        let session = id(session);
        let live = Live {
            order: &order,
            part: &part,
            segments: &segments,
            record: &self.record,
            at: 0,
            authorisation: &auth,
            origin_reserve: &origin,
            route: &end,
            caller: &caller,
            core: &core_actor,
            operator: &operator_actor,
            session: Some(&session),
            inputs: &self.inputs,
            floor: self.floor.as_ref(),
        };
        let mut general = General(Rc::clone(&self.log));
        let mut parts = Parts {
            clock: &*self.clock,
            catalogue,
            log: &mut general,
            evidence: &mut self.store,
            decisions: &mut self.policy,
            modules,
            uses,
            grading,
            operator_key: &mut self.key,
            fresh: &mut self.ids,
            issued: &self.issued,
            exposures: &mut self.exposures,
        };
        self.segments.run_part(&mut parts, &live)
    }

    /// Runs the order's part over `modules`, as on the platform, with no proxy.
    pub fn run_over(&mut self, world_catalogue: &Catalogue, modules: &mut dyn Modules) -> Ran {
        let session = SuiteContext::default().session;
        self.run_with(
            world_catalogue,
            modules,
            &mut NoProxy,
            &mut Recorded,
            &session,
        )
    }

    /// Runs the order's part on the world's platform.
    pub fn run(&mut self, world: &mut World) -> Ran {
        let session = world.context().session;
        self.run_with(
            &world.catalogue,
            &mut world.fleet.platform,
            &mut NoProxy,
            &mut Recorded,
            &session,
        )
    }

    /// The log as the log itself would keep it: each stated entry attributed from its kept bytes, and every entry
    /// admitted over the ones before it on the path it came by. Refused at the first entry the log wouldn't admit.
    ///
    /// What it leaves out: a receipt is of the entry as handed in, since this log answers before it attributes, where
    /// the real log's is of the entry it recorded; `signs_for` always answers true, the key's tie to its maker resting on the catalogue's
    /// verification in `Signatures`; the operator's statements are verified under the real key the deployment's
    /// placeholder `operator_key` stands for, the simulated providers' convention.
    pub fn admitted(&self, catalogue: &Catalogue) -> Result<Vec<Entry>, (usize, String)> {
        let log = self.log.borrow();
        let mut history: Vec<(Receipt, Entry)> = Vec::new();
        for (i, (appended, through)) in log.entries.iter().enumerate() {
            let entry = match appended {
                Entry::Stated(claim) => {
                    let mut sources = Sources {
                        catalogue,
                        store: &self.store,
                        at: self.clock.now().floor(),
                        operator: &self.operator,
                    };
                    let mut keys = Sources { ..sources };
                    let signatures = Sources { ..sources };
                    Entry::Stated(
                        attribute(claim, &history, &mut sources, &mut keys, &signatures)
                            .map_err(|e| (i, format!("{e:?}")))?,
                    )
                }
                verified => verified.clone(),
            };
            admit_entry(
                &history,
                &entry,
                *through,
                &rutent_contracts::core_internal::DeploymentOperator {
                    maker: operator().acts,
                    party: operator().party,
                },
            )
            .map_err(|v| (i, v.0.to_owned()))?;
            // The receipt the writer was handed: this log answers before it attributes.
            history.push((receipt(i, appended), entry));
        }
        Ok(history.into_iter().map(|(_, e)| e).collect())
    }
}

#[derive(Clone, Copy)]
struct Sources<'a> {
    catalogue: &'a Catalogue,
    store: &'a Store,
    at: Instant,
    operator: &'a Actor,
}

impl KeptStatements for Sources<'_> {
    fn kept(&mut self, wanted: &Digest) -> Result<Option<KeptStatement>, AppendFailure> {
        let place = (0..self.store.kept.len()).find(|i| &commitment(*i) == wanted);
        Ok(place.map(|i| {
            serde_json::from_value(json!({
                "commitment": wanted,
                "salt": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
                "bytes": rutent_conformance::signing::base64url(&self.store.kept[i]),
            }))
            .unwrap()
        }))
    }
}

impl StatementKeys for Sources<'_> {
    fn deployment(&mut self) -> Result<Deployment, AppendFailure> {
        Ok(SuiteContext::default().deployment)
    }

    fn operator(&mut self) -> Result<Id, AppendFailure> {
        Ok(operator().acts)
    }

    fn publisher(&mut self, maker: &Id) -> Result<Option<Id>, AppendFailure> {
        if maker == &self.operator.acts {
            return Ok(Some(self.operator.party.clone()));
        }
        Ok(
            match self.catalogue.publishers().publisher(maker.as_str()) {
                Some(Publisher::Operator) => Some(self.operator.party.clone()),
                Some(Publisher::Party(party)) => Id::new(party),
                None => None,
            },
        )
    }

    fn signs_for(&mut self, _: &Id, _: &SignedWith, _: &Timestamp) -> Result<bool, AppendFailure> {
        Ok(true)
    }
}

impl Signatures for Sources<'_> {
    fn verifies(&self, received: &[u8]) -> bool {
        let value: Value = match serde_json::from_slice(received) {
            Ok(value) => value,
            Err(_) => return false,
        };
        if value["maker"].as_str() == Some(operator().acts.as_str()) {
            let operator_key = SuiteContext::default().deployment.operator_key;
            if value["signature"]["key"].as_str() != Some(operator_key.as_str()) {
                return false;
            }
            let Some(signature) = value["signature"]["value"]
                .as_str()
                .and_then(decode_signature)
            else {
                return false;
            };
            let Ok(input) = signing_input(received) else {
                return false;
            };
            return real(operator_key.as_str())
                .verifying_key()
                .verify(&input, &ed25519_dalek::Signature::from_bytes(&signature))
                .is_ok();
        }
        self.catalogue.verify_statement(received, self.at).is_ok()
    }
}

fn decode_signature(text: &str) -> Option<[u8; 64]> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut bits = 0u32;
    let mut count = 0;
    let mut out = Vec::new();
    for c in text.bytes() {
        let v = ALPHABET.iter().position(|a| *a == c)? as u32;
        bits = (bits << 6) | v;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    out.try_into().ok()
}

/// The kinds of `what`, for a test to find entries by.
pub fn is_effect(what: &What) -> bool {
    matches!(what, What::Effect { .. })
}

pub fn is_result(what: &What) -> bool {
    matches!(what, What::Result { .. })
}

pub fn is_hand_over(what: &What) -> bool {
    matches!(what, What::HandOver { .. })
}

/// The node an endpoint is at, as text.
pub fn node_of(endpoint: &Endpoint) -> String {
    match &endpoint.node {
        NodeRef::Inner(n) => n.clone(),
        NodeRef::Outer(o) => o.network.clone(),
    }
}

pub fn kind_of(record: &DecisionRecord, place: usize) -> OperationKind {
    record.chosen().unwrap().parts[0].operations[place].kind
}

pub fn snapshot_of(record: &DecisionRecord) -> &CatalogueSnapshot {
    &record.snapshot
}

// The platform, watched ---------------------------------------------------------------------------------------------

/// What a test does while an operation is handed over, with the platform as it stands then.
pub type During<'a> = Box<dyn FnMut(&mut Platform<SimWorld>, &Statement<ProviderOperation>) + 'a>;

/// What a test makes of the result a module gave for one operation.
pub type Alter<'a> = Box<dyn FnMut(Vec<u8>) -> Vec<u8> + 'a>;

/// The platform with simulated providers, as Core uses it, with what a test watches or changes: each operation
/// handed over, each drain, what happens during an operation, a result changed on its way back, and Core's clock
/// moved on while an operation runs.
pub struct Watched<'a> {
    pub platform: &'a mut Platform<SimWorld>,
    pub carried: Vec<(Id, Statement<ProviderOperation>)>,
    pub drained: Vec<Events>,
    pub during: Option<During<'a>>,
    pub alter: Option<(usize, Alter<'a>)>,
    pub takes: Option<(Rc<TestClock>, u64)>,
    pub transactions: Rc<RefCell<Vec<String>>>,
    /// Each drain of events holds, besides, a state read the contract can't carry: one of no transaction.
    pub spoil: bool,
    /// The platform refuses to begin the segment at this place in the calls to begin, once.
    pub refuse_begin: Option<usize>,
    begun: usize,
}

impl<'a> Watched<'a> {
    pub fn new(platform: &'a mut Platform<SimWorld>) -> Self {
        Self {
            platform,
            carried: Vec::new(),
            drained: Vec::new(),
            during: None,
            alter: None,
            takes: None,
            transactions: Rc::new(RefCell::new(Vec::new())),
            spoil: false,
            refuse_begin: None,
            begun: 0,
        }
    }

    /// The operations handed to `kind`'s segments, in order.
    pub fn amounts(&self) -> Vec<String> {
        self.carried
            .iter()
            .map(|(_, op)| {
                serde_json::to_value(&op.statement).unwrap()["amount"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect()
    }
}

impl Modules for Watched<'_> {
    fn begin_segment(
        &mut self,
        provider: &Id,
        transaction: &Identifier,
    ) -> Result<String, PlatformRefusal> {
        let place = self.begun;
        self.begun += 1;
        if self.refuse_begin == Some(place) {
            self.refuse_begin = None;
            return Err(PlatformRefusal::Unavailable);
        }
        Modules::begin_segment(&mut *self.platform, provider, transaction)
    }

    fn name_calls(
        &mut self,
        transaction: &Identifier,
        record: &DecisionRecord,
        at: (usize, usize),
        segment: &Identifier,
    ) -> Result<usize, RouteNotNamed> {
        Modules::name_calls(&mut *self.platform, transaction, record, at, segment)
    }

    fn carry_out(
        &mut self,
        effect: &Effect,
        provider: &Id,
        operation: &Statement<ProviderOperation>,
    ) -> Answered {
        let place = self.carried.len();
        self.carried.push((provider.clone(), operation.clone()));
        let transaction = serde_json::to_value(&operation.statement).unwrap()["transaction"]
            .as_str()
            .unwrap()
            .to_owned();
        self.transactions.borrow_mut().push(transaction);
        if let Some(during) = self.during.as_mut() {
            during(&mut *self.platform, operation);
        }
        let answered = Modules::carry_out(&mut *self.platform, effect, provider, operation);
        if let Some((clock, seconds)) = &self.takes {
            clock.advance(*seconds);
        }
        match (answered, self.alter.as_mut()) {
            (Answered::Bytes(bytes), Some((at, alter))) if *at == place => {
                Answered::Bytes(alter(bytes))
            }
            (answered, _) => answered,
        }
    }

    fn take_events(&mut self) -> Events {
        let mut events = Modules::take_events(&mut *self.platform);
        if self.spoil {
            events
                .state_reads
                .push(rutent_prov::module::types::StateEvent {
                    provider: RESERVE.into(),
                    part: rutent_contracts::module_interface::PartName::Outside,
                    transaction: String::new(),
                    key: "book".into(),
                    found: true,
                });
        }
        self.drained.push(events.clone());
        events
    }

    fn end_segment(&mut self, transaction: &Identifier) -> Option<SegmentRecord> {
        Modules::end_segment(&mut *self.platform, transaction)
    }

    fn end_calls(&mut self, transaction: &Identifier) {
        Modules::end_calls(&mut *self.platform, transaction)
    }

    fn staged(&self, provider: &Id) -> Option<String> {
        Modules::staged(&*self.platform, provider)
    }

    fn promote(
        &mut self,
        effect: &Effect,
        provider: &Id,
        by: &Id,
        at: Instant,
    ) -> Result<String, LifecycleRefusal> {
        Modules::promote(&mut *self.platform, effect, provider, by, at)
    }
}

/// The grading the platform tests that aren't about closing run under (`T-020-09`): a segment on its provider's
/// word, as [`ProvidersWord`] grades it, and a part's final deposit on the movement the simulated reserve records on
/// its destination's network, which it names by the segment's transaction (`rutent_conformance::sim::movements_of`),
/// taken as found rather than read, plus the reserve's statement. The tests of closing grade through
/// `closing::Evidenced`, which reads the networks (`tests/closing.rs`).
pub struct Recorded;

impl Grading for Recorded {
    fn grade(&mut self, settled: &ToGrade<'_>) -> Graded {
        if !rutent_core_orch::closing::is_final_deposit(settled) {
            return ProvidersWord.grade(settled);
        }
        let network = settled.end.destination.node.network.clone();
        let evidence = |evidence: Id, grade, whose: Id, movement| {
            Entry::Verified(VerifiedEntry {
                kind: VerifiedKind::Verified,
                who: settled.core.clone(),
                what: What::Evidence {
                    evidence,
                    order: settled.order.clone(),
                    segment: settled.segment.clone(),
                    grade,
                    whose,
                    rests_on: vec![settled.result.clone()],
                    movement,
                },
                when: When {
                    at: settled.at.clone(),
                    source: TimeSource::CoreClock,
                },
                outcome: Outcome::Observed {},
            })
        };
        let movement = PublicMovement {
            network: id(&network),
            id: settled.transaction.clone(),
            node: None,
        };
        Graded::Settles {
            cites: settled.evidence.clone(),
            entries: vec![
                evidence(
                    settled.evidence.clone(),
                    EvidenceGrade::PublicRecord,
                    id(&network),
                    Some(movement),
                ),
                evidence(
                    id(&format!("{}.reserve", settled.evidence.as_str())),
                    EvidenceGrade::Statement,
                    settled.provider.clone(),
                    None,
                ),
            ],
        }
    }
}

/// The proxy's uses as a world with one would report them for each operation handed over: its statement key,
/// resolved, and a secret its outside part asked for and was refused.
pub struct UsesOf(pub Rc<RefCell<Vec<String>>>, pub usize);

impl Uses for UsesOf {
    fn take_uses(&mut self) -> Vec<Use> {
        let transactions = self.0.borrow();
        let fresh = &transactions[self.1..];
        self.1 = transactions.len();
        fresh
            .iter()
            .flat_map(|t| {
                let identifier = |text: &str| Identifier::new(text).unwrap();
                [
                    Use {
                        provider: identifier(RESERVE),
                        module_version: Some(identifier("1.0.0")),
                        transaction: identifier(t),
                        operation: None,
                        of: rutent_prov::proxy::UseOf::StatementKey(None),
                        outcome: Ok(()),
                    },
                    Use {
                        provider: identifier(RESERVE),
                        module_version: Some(identifier("1.0.0")),
                        transaction: identifier(t),
                        operation: Some(Name::new("settle").unwrap()),
                        of: rutent_prov::proxy::UseOf::Secret(Name::new("rail-token").unwrap()),
                        outcome: Err(rutent_prov::proxy::Refusal::UndeclaredSecret),
                    },
                ]
            })
            .collect()
    }
}

/// The private half of the statements key `provider`'s module signs its results with.
pub fn statements_key_of(provider: &str) -> ed25519_dalek::SigningKey {
    let context = SuiteContext::default();
    let keys = Spec::of(SimRole::Reserve, provider)
        .make(&context)
        .advertisement()
        .into_statement()
        .statement
        .keys;
    let placeholder = keys
        .iter()
        .find(|k| k.purpose == rutent_contracts::provider_role::KeyPurpose::Statements)
        .unwrap()
        .public_key
        .clone();
    real(placeholder.as_str())
}

/// `bytes`, a result, changed by `change` and signed anew with `key`.
pub fn resigned(
    bytes: &[u8],
    key: &ed25519_dalek::SigningKey,
    change: impl FnOnce(&mut Value),
) -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(bytes).unwrap();
    change(&mut value);
    rutent_conformance::signing::sign_value(&mut value, key);
    serde_json::to_vec(&value).unwrap()
}

// A second release, catalogue changes, the log's own record of them ------------------------------------------------

/// Admits `version` of [`RESERVE`]'s release, the same module under another version, as its provider asserts it, with
/// `staging` set first: `None` puts it in force once accepted, a staging stages it beside the version in force.
pub fn admit_release(world: &mut World, version: &str, staging: Option<Staging>) {
    world.fleet.platform.set_staging(staging);
    let installed = world.installed[0].clone();
    let mut declaration = installed.declaration.clone();
    declaration.version = Identifier::new(version).unwrap();
    let guests = rutent_scenarios::sim::disk::guests();
    let binaries = rutent_contracts::module_interface::Parts {
        sandboxed: std::fs::read(guests.join("sim-reserve-sandboxed.wasm")).unwrap(),
        outside: std::fs::read(guests.join("sim-outside.wasm")).unwrap(),
    };
    let context = SuiteContext::default();
    let keys = Spec::of(SimRole::Reserve, RESERVE)
        .make(&context)
        .advertisement()
        .into_statement()
        .statement
        .keys;
    let advertising = keys
        .iter()
        .find(|k| k.purpose == rutent_contracts::provider_role::KeyPurpose::Advertising)
        .unwrap()
        .public_key
        .clone();
    let assertion = rutent_scenarios::sim::fleet::assertion_signed_by(
        &declaration,
        &binaries,
        &real(advertising.as_str()),
    );
    let admitted = world.fleet.admit(&assertion, &binaries);
    let rutent_prov::module::types::Admitted::Artefact(artefact) = admitted else {
        panic!("the release is admitted: {admitted:?}");
    };
    if artefact.acceptance.accepted.is_none() {
        world
            .fleet
            .accept(RESERVE, &artefact)
            .expect("the release is accepted");
    }
}

/// A catalogue change as the log holds one carried out: policy's permitting decision, its effect and a `done` result.
pub fn seed_change(log: &Rc<RefCell<TestLog>>, action: rutent_contracts::core_internal::Action) {
    let at = Timestamp::new(Instant(START).rfc3339().unwrap()).unwrap();
    let entry = |who: Actor, what: What, outcome: Outcome| {
        Entry::Verified(VerifiedEntry {
            kind: VerifiedKind::Verified,
            who,
            what,
            when: When {
                at: at.clone(),
                source: TimeSource::CoreClock,
            },
            outcome,
        })
    };
    let mut log = log.borrow_mut();
    let n = log.entries.len();
    let request = DecisionRequest {
        request: id(&format!("req-change-{n}")),
        caller: operator(),
        action: action.clone(),
        at: at.clone(),
    };
    let decision = id(&format!("dec-change-{n}"));
    let decided = log
        .take(
            &entry(
                operator(),
                What::Decide {
                    request: request.clone(),
                    decision: decision.clone(),
                },
                Outcome::Permitted {},
            ),
            Through::Policy,
        )
        .unwrap();
    let permit = Decision {
        request: request.request.clone(),
        decision,
        outcome: rutent_contracts::core_internal::Answer::Permit {},
        receipt: decided,
    }
    .permit()
    .unwrap();
    let effect = log
        .take(
            &entry(
                core(),
                What::Effect {
                    action,
                    decision: permit,
                },
                Outcome::ToTakeEffect {},
            ),
            Through::General,
        )
        .unwrap();
    log.take(
        &entry(core(), What::Result { effect }, Outcome::Done {}),
        Through::General,
    )
    .unwrap();
}

/// A change about [`RESERVE`]'s release `version`.
pub fn release_change(
    change: CatalogueChange,
    version: &str,
    statement_key: Option<PublicKey>,
) -> rutent_contracts::core_internal::Action {
    rutent_contracts::core_internal::Action::ChangeCatalogue {
        change,
        provider: id(RESERVE),
        version: Some(id(version)),
        tuple: None,
        advertising_keys: Vec::new(),
        statement_key,
    }
}

// Stuck orders and recalculation, for `T-020-11` --------------------------------------------------------------------

pub mod rerouting;
pub mod resolving;
