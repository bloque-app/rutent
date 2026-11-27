//! The assembly (`T-024-02`): Core built from its three crates over `core-internal`, with the platform and the
//! simulated providers, the evidence checker and the state machine on its host, as one [`Core`].
//!
//! # What is wired, once
//!
//! - **The evidence store and the log.** The store is opened on its shelf, its log's read taken
//!   ([`EvidenceStore::for_the_log`]) and the log opened with it ([`AuditLog::open`]), each once; the store hands
//!   orchestration its [`Keeper`] once, whose reads policy checks, and is then bound to that same log
//!   ([`EvidenceStore::bind`]), once, so only that log's records erase.
//! - **The log's two handles.** The policy engine holds the policy handle ([`PolicyHandle`]), and nothing else
//!   does; orchestration holds the general handle ([`GeneralHandle`]) and no other (`DEC-2`, `T-020-01`).
//! - **Policy.** `CMP-CORE-AUTHZ`'s engine, its fixed rules first, each input answered by Core's own sources from its
//!   log and evidence store, and the deployment's registers for what is the deployment's alone ([`policy::Deployed`],
//!   `T-020-55`); every call reaches it through [`policy::PolicySide`]. Core's own actor is listed as carrying the user's authorisation onward, so it can move
//!   value, and the operator's login holds each catalogue change the assembly makes.
//! - **One clock.** Core's snapshots and segments, policy's session bound, the log's skew and the simulated world
//!   read one [`SharedClock`].
//! - **Sessions.** Each session the engine opens is handed to the catalogue's register (`Register::open_in`).
//! - **Intake and the segments.** An order's segments follow only its start as intake answers it
//!   (`Intake::start`), never an acceptance, and the rounds' transactions (`Issued`) are kept across routings so no
//!   segment runs under one.
//! - **Arrivals** (`T-020-04`, `arrivals.rs`). `Arrivals` is held beside intake, on the same desk; the orders open at a
//!   reserve are intake's started orders whose first segment hasn't settled; each order a filing opens is put into
//!   Core's sources' record of it (`OpenedOrders`) before policy is asked about it, and dropped where none opens
//!   (`FiledOrders`); an order
//!   started on an automatic deposit is followed from its start as arrivals answers it (`Arrivals::start`); and the
//!   operator's login holds `ActForUser`, to file for a reserve that declared it.
//! - **The providers in the log.** Each simulated provider is granted by a decided catalogue change, its effect
//!   and its result, and its advertisement appended as its stated entry, before any order: the log verifies their
//!   statements by the keys it holds that way, and by no setting.
//! - **The state machine.** Every input the log takes is fed to the host, in the log's order ([`machine::Feed`]).
//! - **Staging**, set explicitly ([`Staged`]): there is no default (`PLAN.md` §4.4, `F-030` sets the deployment's).
//! - **Grading** (`T-020-09`): each part runs under `closing::Evidenced`, over the simulated networks and the reserves'
//!   books as read before every grading (at the start of each run and after each operation carried out), with one
//!   `Closing` kept beside the segments, rebuilt from the log whenever Core is put together on a disk
//!   (`Closing::recorded`).
//!
//! # What isn't, and why
//!
//! - The credential proxy: the simulated world resolves nothing through one, so the segments take uses from
//!   [`NoProxy`], and the proxy's `take_uses` runs only in `F-030`'s production assembly.
//! - What survives a restart: Core's own records (intake's, the segments', `Issued`) are in memory. A Core opened
//!   again on a [`durable::Disk`] reads back the log and the evidence store, and nothing of what it had acted on
//!   (`F-030`).

mod arrivals;
pub mod audit;
pub mod charges;
pub mod checking;
pub mod durable;
mod erasure;
mod grading;
pub use grading::Tamper;
pub mod holding;
pub mod lifecycle;
pub mod linked;
pub mod machine;
pub mod measures;
pub mod policy;
pub mod price;
pub mod releases;
mod reopen;
pub mod schedule;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;

use rutent_conformance::role::SuiteContext;
use rutent_conformance::sim::Spec;
use rutent_contracts::core_internal::calls::{AppendFailure, GeneralEntry, Log, Reads};
use rutent_contracts::core_internal::{
    Action, Actor, ActorKind, Authorisation, CatalogueChange, Decision, DecisionRequest, Entry,
    OperatorSetting, Outcome, PermissionChange, ReadAnswer, ReadCheck, Receipt, RefusalClass, Role,
    RouteEnd, SessionOutcome, SessionRequest, SettingKind, TimeSource, VerifiedEntry, VerifiedKind,
    What, When,
};
use rutent_contracts::provider_role::{
    ADVERTISEMENT_SHAPE, JudgementOutcome, JudgementResult, Operation, ResultBody, Statement,
};
use rutent_contracts::router_api::{DECISION_RECORD_SHAPE, DecisionRecord, Id, PolicyBounds};
use rutent_contracts::state_machine::{Observed, PartPlan, RecordedInput};
use rutent_core_audit::log::{AuditLog, Settings};
use rutent_core_audit::receipts::store::{BoundStore, EvidenceStore, Keeper, LogsRead};
use rutent_core_authz::engine::Engine;
use rutent_core_authz::roles::Permission;
use rutent_core_authz::session::LONGEST_SESSION_SECS;
use rutent_core_orch::intake::{
    Actors, DESTINATION_INPUT, Desk, Destinations, HeldSession, Intake, IntakeError, OrderRecord,
    OrderRequest, Quote, Resolution, SessionRegister, Status as IntakeStatus, StatusReads, Told,
};
use rutent_core_orch::rounds::{Fresh, Issued, OperatorKey, Parts as Rounds, Providers};
use rutent_core_orch::segments::{
    Live, Modules, NoProxy, Parts as SegmentParts, Ran, Segments, SegmentsError, Status,
    StatusError,
};
use rutent_core_orch::snapshot::{Exposures, Price, Releases};
use rutent_core_orch::stuck::{
    Deciding, Moved, Mover, Moving, NoRouteLeft, Parts as StuckParts, Ports, ResolveError, Stuck,
    StuckError,
};
use rutent_prov::catalogue::Catalogue;
use rutent_prov::module::lifecycle::Staging;
use rutent_prov::module::platform::Platform;
use rutent_prov::module::types::Handled;
use rutent_router::RouteFinder;
use serde_json::json;

use crate::kit::{self, RealKey, Seeded, SharedClock};
use crate::sim::{Fleet, Installed, SimWorld};
use rutent_core_orch::schedule::Started;

/// Who hands a statement to Core in a scenario: the maker its envelope names, acting for its publisher. A scenario
/// hands over its providers' own statements, so none is a relay's copy (`T-020-28`).
fn handed_over_by(catalogue: &Catalogue, bytes: &[u8]) -> Actor {
    let acts = serde_json::from_slice::<serde_json::Value>(bytes)
        .ok()
        .and_then(|v| v["maker"].as_str().and_then(Id::new))
        .unwrap_or_else(|| Id::new("unknown").expect("an id"));
    let party = match catalogue.publishers().publisher(acts.as_str()) {
        Some(rutent_prov::catalogue::Publisher::Operator) => Id::new("operator"),
        Some(rutent_prov::catalogue::Publisher::Party(publisher)) => Id::new(publisher),
        None => None,
    }
    .unwrap_or_else(|| acts.clone());
    Actor {
        party,
        acts,
        role: Role::Provider,
        kind: ActorKind::Component,
    }
}
use crate::{GeneralHandle, PolicyHandle};
use durable::{Disk, LogDisk};
use machine::{Delivery, Environment, Feed};
use policy::{Deployed, Deployment, PolicyEngine, PolicySide, Registers};

/// The log as the assembly opens it: on its disk, reading the evidence store's bytes through the store's own read.
pub type CoreLog = AuditLog<LogDisk, LogsRead>;

/// How the platform stages an upgrade, set explicitly by every scenario: there is no default, since staging is the
/// deployment's to set (`F-030`), and `Lifecycle::default()` is never what a scenario runs under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Staged {
    /// An upgrade takes effect at once, staged for no one.
    AtOnce,
    /// An upgrade is staged under this policy.
    Under(Staging),
}

impl Staged {
    fn policy(self) -> Option<Staging> {
        match self {
            Staged::AtOnce => None,
            Staged::Under(staging) => Some(staging),
        }
    }
}

/// What a scenario sets before Core is put together.
#[derive(Debug, Clone)]
pub struct Config {
    /// The run's seed: every identifier Core draws, and every choice a fault makes, follow from it (`DEC-18`).
    pub seed: u64,
    /// How the platform stages an upgrade.
    pub staging: Staged,
    /// How long the platform waits on a module's answer before it counts it silent.
    pub deadline: Duration,
    /// The simulated providers, admitted to the platform and listed in the catalogue and the log.
    pub providers: Vec<Spec>,
    /// How the log's inputs reach the state machine.
    pub delivery: Delivery,
    /// A holdout fleet: the version the simulated providers still speak, and the cutover date the operator sets for it
    /// before they are listed. The providers sign everything under that version, and take Core's operations under the
    /// current one; Core's catalogue, log and router read the old version until the date (`REQ-PRT-4`, `T-020-54`).
    pub holdout: Option<(String, String)>,
}

/// How long before the earliest advertisement was made its provider is granted, in seconds.
pub const GRANTED_BEFORE: u64 = 3_600;

/// How many balance checks of one account count in a day, the deployment's bound (`REQ-CAT-4`).
pub const CHECKS_PER_WINDOW: u32 = 10;

/// How long a reserve has to answer a balance check, in seconds.
pub const CHECK_ANSWERED_WITHIN: u64 = 60;

/// The log's name in the assembly.
pub const LOG_NAME: &str = "rutent-scenarios-log";

/// The context every statement of the assembly is in: the conformance deployment, whose operator key is the
/// assembly's real key ([`RealKey`]).
pub fn context() -> SuiteContext {
    let mut context = SuiteContext::default();
    context.deployment.operator_key = RealKey::assembly().public();
    context
}

/// The log's settings over `clock`: its operator is the one Core signs as, of the operator's party.
pub fn settings(clock: &SharedClock) -> Settings {
    let context = context();
    Settings::new(
        kit::id(LOG_NAME),
        context.deployment.clone(),
        kit::operator().acts,
        kit::operator().party,
    )
    .on(Rc::new(clock.clone()))
}

/// The log and the evidence store as they come back from `disk`: the store opened on its shelf, the log opened
/// with its read, and nothing else made of the store yet.
pub(crate) fn read_back(
    disk: &Disk,
    clock: &SharedClock,
) -> Result<(EvidenceStore, Rc<RefCell<CoreLog>>), AppendFailure> {
    let store = EvidenceStore::on(Box::new(disk.shelf()))?;
    let log = AuditLog::open(settings(clock), disk.log(), store.for_the_log())?;
    Ok((store, Rc::new(RefCell::new(log))))
}

/// Why Core couldn't be put together.
#[derive(Debug)]
pub enum AssemblyError {
    /// The log or the evidence store wouldn't open or bind.
    Log(AppendFailure),
    /// A provider couldn't be listed in the log: which step, and why.
    Listing(String),
    /// The state machine's host failed.
    Host(rutent_contracts::state_machine::host::HostError),
}

/// What an order's start takes from its intake or its filing: its origin reserve, the session its withdrawal goes in,
/// the inputs its tuples need, and what its route delivers before the operator's fee.
struct Begun {
    origin: Id,
    session: Option<Id>,
    inputs: BTreeMap<String, String>,
    delivers: rutent_contracts::router_api::Amount,
}

/// The operator's key, watched: every decision record Core signs is kept by its id, for the segments to run the
/// route it chose (`Live::record`). Intake hands no record back; the log names it by digest, which Core holds the
/// kept record to before it runs the route.
pub struct Signer {
    key: RealKey,
    /// Each decision record Core signed, by its id: the envelope as signed, its bytes, and nothing read from them
    /// until they are held to the log's digest.
    records: BTreeMap<String, Vec<u8>>,
    /// The ids of `records`, in the order they were signed: what a scenario that takes the first or the last record
    /// means, since the ids are drawn and order by chance of what else Core drew.
    signed: Vec<String>,
}

impl Signer {
    /// The records, in the order they were signed; any the order doesn't name (carried over a restart) after, by id.
    fn in_order(&self) -> Vec<(&String, &Vec<u8>)> {
        let mut named: Vec<(&String, &Vec<u8>)> = self
            .signed
            .iter()
            .filter_map(|id| self.records.get_key_value(id))
            .collect();
        for (id, bytes) in &self.records {
            if !self.signed.contains(id) {
                named.push((id, bytes));
            }
        }
        named
    }

    /// The first record signed, to change.
    fn first_mut(&mut self) -> Option<&mut Vec<u8>> {
        let first = self.in_order().first().map(|(id, _)| (*id).clone())?;
        self.records.get_mut(&first)
    }
}

impl OperatorKey for Signer {
    fn key(&self) -> rutent_contracts::provider_role::PublicKey {
        self.key.key()
    }

    fn sign(&mut self, input: &[u8]) -> Option<[u8; 64]> {
        let signature = self.key.sign(input)?;
        if let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(input)
            && value["shape"] == DECISION_RECORD_SHAPE
            && let Ok(record) = serde_json::from_value::<DecisionRecord>(value["statement"].clone())
        {
            value["signature"]["value"] = rutent_conformance::signing::base64url(&signature).into();
            let recorded = serde_json::to_vec(&value).unwrap_or_default();
            let id = record.record_id().as_str().to_owned();
            if self.records.insert(id.clone(), recorded).is_none() {
                self.signed.push(id);
            }
        }
        Some(signature)
    }
}

/// The platform, as the rounds ask it: each question handed to its provider's module at Core's clock.
struct Asking<'a> {
    fleet: &'a mut Fleet,
    clock: &'a SharedClock,
}

impl Providers for Asking<'_> {
    fn ask(&mut self, provider: &Id, question: &[u8]) -> Option<Vec<u8>> {
        let operation: Statement<Operation> = serde_json::from_slice(question).ok()?;
        self.fleet.platform.world.set_clock(self.clock.instant());
        // A verifier's module answers like any other (`T-024-18`), and the judgement its result carries inside, its own
        // signed statement, is what the rounds read. A refused judgement stays the result.
        match self.fleet.platform.handle(provider.as_str(), &operation) {
            Handled::Answered(result) => match result.statement {
                ResultBody::Judgement(JudgementResult {
                    outcome: JudgementOutcome::Judged(judged),
                }) => serde_json::to_vec(&*judged.judgement).ok(),
                _ => serde_json::to_vec(&*result).ok(),
            },
            Handled::Refused(_) => None,
        }
    }
}

/// Faults planted at the platform's seam, counted by the effects the segments carry out, from 0 (`T-024-02`).
#[derive(Debug, Clone, Default)]
pub struct Faults {
    /// Right before the platform carries out this effect, the process crashes: the effect's record is in the log,
    /// and answered, and nothing was carried out (`INV-7`).
    pub crash_before_effect: Option<usize>,
    /// Right before the platform carries out this effect, the state machine moves to [`Core::swap_to`]: halted,
    /// exported and imported there, mid-order (`REQ-OPS-3`).
    pub swap_before_effect: Option<usize>,
    /// Each operation the segments carry out is delivered to its provider twice, and Core is handed the answer to
    /// the second: a repeated delivery between Core and a provider, under one transaction.
    pub deliver_twice: bool,
    /// How many operations reached their provider a second time and were answered again.
    pub delivered_twice: usize,
    /// How many effects the platform was handed so far.
    pub carried: usize,
    /// Why a planted swap failed, if it did.
    pub swap_failed: Option<rutent_contracts::state_machine::host::HostError>,
    /// The platform refuses to begin the segment at this place in the calls to begin, once: nothing passes out, so
    /// the effect it was to carry out failed (`T-020-16`).
    pub refuse_begin: Option<usize>,
    /// How many segments the platform was asked to begin so far.
    pub begun: usize,
    /// How many seconds Core's clock moves on while each operation is carried out (`T-024-06`): what a provider's
    /// segment takes, so the settlements Core learns its p99 from have a duration.
    pub operation_takes: u64,
}

/// The platform, as the segments carry an operation out: at Core's clock, with the faults planted at it.
struct Running<'a> {
    platform: &'a mut Platform<SimWorld>,
    clock: &'a SharedClock,
    faults: &'a mut Faults,
    feed: &'a mut Feed,
    log: &'a Rc<RefCell<CoreLog>>,
    swap_to: Option<&'a mut (dyn Environment + 'static)>,
    readings: &'a grading::Readings,
}

impl Modules for Running<'_> {
    fn begin_segment(
        &mut self,
        provider: &Id,
        transaction: &rutent_contracts::module_interface::Identifier,
    ) -> Result<String, rutent_contracts::module_interface::PlatformRefusal> {
        let place = self.faults.begun;
        self.faults.begun += 1;
        if self.faults.refuse_begin == Some(place) {
            self.faults.refuse_begin = None;
            return Err(rutent_contracts::module_interface::PlatformRefusal::Unavailable);
        }
        self.platform.world.set_clock(self.clock.instant());
        Modules::begin_segment(self.platform, provider, transaction)
    }

    fn name_calls(
        &mut self,
        transaction: &rutent_contracts::module_interface::Identifier,
        record: &DecisionRecord,
        at: (usize, usize),
        segment: &rutent_contracts::module_interface::Identifier,
    ) -> Result<usize, rutent_prov::proxy::RouteNotNamed> {
        Modules::name_calls(self.platform, transaction, record, at, segment)
    }

    fn carry_out(
        &mut self,
        effect: &rutent_contracts::core_internal::Effect,
        provider: &Id,
        operation: &Statement<Operation>,
    ) -> rutent_core_orch::segments::Answered {
        let n = self.faults.carried;
        self.faults.carried += 1;
        if self.faults.crash_before_effect == Some(n) {
            panic!("{}", durable::CRASHED);
        }
        if self.faults.swap_before_effect == Some(n)
            && let Some(to) = self.swap_to.as_deref_mut()
        {
            let inputs = inputs_of(&self.log.borrow());
            if let Err(e) = self.feed.flush(&inputs).and_then(|()| self.feed.swap(to)) {
                self.faults.swap_failed = Some(e);
            }
        }
        self.platform.world.set_clock(self.clock.instant());
        let first = Modules::carry_out(self.platform, effect, provider, operation);
        self.readings.refresh(&self.platform.world);
        if self.faults.operation_takes > 0 {
            self.clock.advance(self.faults.operation_takes);
        }
        if !self.faults.deliver_twice {
            return first;
        }
        // The same operation again, under its transaction: the provider answers it as it did, and Core takes the
        // answer that came second, where one came.
        let again = Modules::carry_out(self.platform, effect, provider, operation);
        self.readings.refresh(&self.platform.world);
        match again {
            again @ rutent_core_orch::segments::Answered::Bytes(_) => {
                self.faults.delivered_twice += 1;
                again
            }
            _ => first,
        }
    }

    fn status(
        &mut self,
        effect: &rutent_contracts::core_internal::Effect,
        provider: &Id,
        asking: &Id,
        about: &Statement<Operation>,
    ) -> rutent_core_orch::segments::Answered {
        self.platform.world.set_clock(self.clock.instant());
        let answered = Modules::status(self.platform, effect, provider, asking, about);
        self.readings.refresh(&self.platform.world);
        answered
    }

    fn ask_for_record(
        &mut self,
        effect: &rutent_contracts::core_internal::Effect,
        provider: &Id,
        question: &Statement<Operation>,
    ) -> rutent_core_orch::segments::Answered {
        use rutent_core_orch::segments::Answered;
        self.platform.world.set_clock(self.clock.instant());
        if let Operation::InnerSegmentsQuery(q) = &question.statement {
            self.readings
                .note_asked(provider.as_str(), &q.node, &q.transaction);
        }
        // The production platform carries it; only the fault a scenario planted on the answer's way to Core is added.
        match Modules::ask_for_record(self.platform, effect, provider, question) {
            Answered::Bytes(bytes) => {
                Answered::Bytes(self.readings.tampered(provider.as_str(), bytes))
            }
            refused => refused,
        }
    }

    fn ask_for_balance(
        &mut self,
        effect: &rutent_contracts::core_internal::Effect,
        provider: &Id,
        question: &Statement<Operation>,
    ) -> rutent_core_orch::segments::Answered {
        // The production platform carries it, at Core's clock (`T-020-63`).
        self.platform.world.set_clock(self.clock.instant());
        Modules::ask_for_balance(self.platform, effect, provider, question)
    }

    fn repeat_operation(
        &mut self,
        effect: &rutent_contracts::core_internal::Effect,
        provider: &Id,
        transaction: &Id,
    ) -> rutent_core_orch::segments::Answered {
        // The production platform carries it, at Core's clock (`T-020-63`).
        self.platform.world.set_clock(self.clock.instant());
        let answered = Modules::repeat_operation(self.platform, effect, provider, transaction);
        self.readings.refresh(&self.platform.world);
        answered
    }

    fn take_events(&mut self) -> rutent_prov::module::types::Events {
        Modules::take_events(self.platform)
    }

    fn end_segment(
        &mut self,
        transaction: &rutent_contracts::module_interface::Identifier,
    ) -> Option<rutent_prov::module::lifecycle::SegmentRecord> {
        Modules::end_segment(self.platform, transaction)
    }

    fn end_calls(&mut self, transaction: &rutent_contracts::module_interface::Identifier) {
        Modules::end_calls(self.platform, transaction);
    }

    fn staged(&self, provider: &Id) -> Option<String> {
        Modules::staged(&*self.platform, provider)
    }

    fn promote(
        &mut self,
        effect: &rutent_contracts::core_internal::Effect,
        provider: &Id,
        by: &Id,
        at: rutent_prov::catalogue::Instant,
    ) -> Result<String, rutent_prov::module::lifecycle::LifecycleRefusal> {
        Modules::promote(self.platform, effect, provider, by, at)
    }
}

/// How the evidence store's read is made to answer otherwise than it kept: given what it kept under a commitment, what
/// it answers instead ([`Core::read_kept_through`]).
pub type KeptThrough = dyn FnMut(
    Option<rutent_contracts::core_internal::KeptStatement>,
) -> Option<rutent_contracts::core_internal::KeptStatement>;

/// The log's own read of the evidence store, as intake reads it: what it kept, or what a scenario has it answer.
struct Reading<'a> {
    kept: &'a mut LogsRead,
    through: Option<&'a mut KeptThrough>,
}

impl rutent_contracts::core_internal::calls::KeptStatements for Reading<'_> {
    fn kept(
        &mut self,
        commitment: &rutent_contracts::module_interface::Digest,
    ) -> Result<Option<rutent_contracts::core_internal::KeptStatement>, AppendFailure> {
        let found = self.kept.kept(commitment)?;
        Ok(match self.through.as_deref_mut() {
            Some(through) => through(found),
            None => found,
        })
    }
}

/// The log's own record of an entry a call appended, read back from the log as it stands after the call.
pub(crate) struct LogNow<'a>(pub(crate) &'a RefCell<CoreLog>);

impl rutent_core_orch::quote::Recorded for LogNow<'_> {
    fn entry(&self, receipt: &Receipt) -> Option<Entry> {
        rutent_core_orch::quote::Recorded::entry(&charges::Log(self.0.borrow().entries()), receipt)
    }
}

/// The register of actors, as intake reads it: the deployment's, the one policy reads, which binds the party, the kind
/// and the role the actor states (`T-021-14`, `T-020-52`).
struct Acting(Rc<RefCell<Registers>>);

impl Actors for Acting {
    fn acts_for(&mut self, acts: &Id, party: &Id, kind: ActorKind, role: Role) -> bool {
        self.0.borrow().acts_in(acts, party, kind, role) == Some(true)
    }
}

/// Policy's read check, as intake asks it before it tells an order's status, and where intake hands over its own
/// record of the filed request first, for policy's `Requester` source to answer from (`T-020-52`).
struct StatusReading {
    policy: PolicySide,
    orders: rutent_core_authz::core_sources::OpenedOrders,
}

impl Reads for StatusReading {
    fn check_read(&mut self, check: &ReadCheck) -> Result<ReadAnswer, AppendFailure> {
        self.policy.check_read(check)
    }

    fn grant_permission(&mut self, change: &PermissionChange) -> Result<Decision, AppendFailure> {
        self.policy.grant_permission(change)
    }

    fn revoke_permission(&mut self, change: &PermissionChange) -> Result<Decision, AppendFailure> {
        self.policy.revoke_permission(change)
    }
}

impl StatusReads for StatusReading {
    fn filed(&mut self, record: &OrderRecord) {
        self.orders
            .opened(&record.order, &record.parties, &record.origin_reserve);
    }

    fn withdrawn(&mut self, order: &Id) {
        self.orders.withdrawn(order);
    }
}

/// Policy's session register, as intake reads it: the sessions the engine opened.
struct Held(Rc<RefCell<PolicyEngine>>);

impl SessionRegister for Held {
    fn unread(&mut self) -> bool {
        self.0.borrow().register().is_unread()
    }

    fn held(&mut self, session: &Id) -> Option<HeldSession> {
        self.0
            .borrow()
            .register()
            .sessions()
            .iter()
            .rev()
            .find(|s| &s.session == session)
            .map(|s| HeldSession {
                holder: s.holder.clone(),
                expires_at: s.expires_at.clone(),
            })
    }
}

/// Where a destination resolves: the destination reserve's register of accounts, which the assembly keeps as the
/// deployment's (no provider-role operation resolves one, `F-020`'s notes).
struct Owners(Rc<RefCell<Registers>>);

impl Destinations for Owners {
    fn resolve(
        &mut self,
        destination: &rutent_contracts::core_internal::Destination,
    ) -> Resolution {
        match self.0.borrow().accounts.get(destination.account.as_str()) {
            Some(owner) => Resolution::Owner(kit::id(owner)),
            None => Resolution::Refused,
        }
    }
}

/// What goes on across a restart of Core: what is outside it, and the source of its fresh identifiers.
struct Carried {
    fleet: Fleet,
    installed: Vec<Installed>,
    clock: SharedClock,
    feed: Feed,
    fresh: Seeded,
    ties: bool,
    /// The host's holding accounts, beside the platform that runs their wallets: the host's attribution of each
    /// order and its own transactions' count go on with it (`T-020-46`).
    holdings: rutent_prov::holding::wallet::Holdings,
    /// The deployment's register of holding accounts, its own configuration, which goes on as the deployment keeps it
    /// (`T-020-55`): policy reads a node's holder from it, never from the host's holdings above.
    holding_accounts: BTreeMap<String, String>,
    /// The decimals the register gives each public node's asset, which go on with it (`T-020-63`).
    holding_decimals: BTreeMap<String, u8>,
    /// The same register's private nodes' accounts, which go on with it (`T-020-48`, the review's S1).
    publisher_accounts: BTreeMap<String, rutent_core_orch::schedule::PublisherAccount>,
    /// The same register's assets of the holding accounts, which go on with it (`T-024-21`).
    holding_assets: BTreeMap<String, String>,
    /// How many times Core was restarted with its platform before this one ([`Core::restarted`]), and, where the
    /// platform restarted with it, whether the operator confirms the releases it holds as Core opens (`T-024-21`).
    restarts: usize,
    confirm: Option<bool>,
}

/// Core, put together.
pub struct Core {
    /// Core's one clock.
    pub clock: SharedClock,
    /// What survives a crash: the log's store and the evidence store's shelf.
    pub(crate) disk: Disk,
    /// How many times Core and its platform were restarted after a crash in this run ([`Core::restarted`]).
    restarts: usize,
    log: Rc<RefCell<CoreLog>>,
    general: GeneralHandle<CoreLog>,
    bound: BoundStore<LogDisk, LogsRead>,
    keeper: Keeper,
    policy: PolicySide,
    /// Core's own sources, which policy's engine asks (`T-020-55`), and the deployment's registers beside them.
    sources: policy::CoreSources,
    deployed: Deployed,
    /// The platform with the simulated providers admitted to it.
    pub(crate) fleet: Fleet,
    /// Each simulated provider, as installed.
    pub installed: Vec<Installed>,
    /// Core's catalogue.
    pub(crate) catalogue: Catalogue,
    intake: Intake,
    /// Arrivals and the orders filed on automatic deposits (`T-020-04`), held beside intake.
    arrivals: rutent_core_orch::arrival::Arrivals,
    /// Each filing on an automatic deposit Core was handed, by the order it opened: what its start takes.
    filings: BTreeMap<String, rutent_core_orch::arrival::Filing>,
    segments: Segments,
    /// The parts Core declared stuck, and the mover that carries their resolutions out (`T-020-16`).
    stuck: Stuck,
    mover: Mover,
    /// The operator's holding accounts, the routes the linked orders were accepted on and what Core handed on
    /// (`T-020-26`).
    holdings: Rc<RefCell<rutent_prov::holding::wallet::Holdings>>,
    linked: BTreeMap<String, rutent_core_orch::stuck::AcceptedRoute>,
    hand_on: rutent_core_orch::hand_on::HandOn,
    /// The notices Core sends, whose operator channel a charge above its formula is raised through (`T-020-14`),
    /// the key they are signed with and the transport that takes them.
    notices: rutent_core_orch::notice::Notices,
    notice_signer: charges::NoticeSigner,
    wire: charges::Wire,
    /// The log's own read of the evidence store: what the terms a provider stated are compared with.
    kept: LogsRead,
    /// How a scenario has the evidence store's read answer otherwise than it kept ([`Core::read_kept_through`]): what
    /// a statement is held to before Core keeps it, shown failing closed.
    kept_through: Option<Box<KeptThrough>>,
    /// What the grading took across orders, kept as long as the segments (`T-020-09`).
    closing: rutent_core_orch::closing::Closing,
    /// The quotes reserves hold, which an acceptance may redeem (`T-020-15`); which order redeemed which is read from
    /// the log whenever Core is put together on a disk (`Holds::recorded`), and a reopened Core keeps each hold again
    /// from the log (`Holds::reopened`, `T-020-57`).
    holds: rutent_core_orch::quote::Holds,
    readings: grading::Readings,
    issued: Issued,
    /// What goes wrong with an order and how it ends (`T-024-06`).
    pub(crate) life: lifecycle::Lifecycle,
    exposures: Exposures,
    /// The prices Core's closing reported, once per order (`T-020-47`).
    reported: rutent_core_orch::quote::Prices,
    /// Each reported price as it was built, and why a completed order's wasn't, by order.
    closed: price::Priced,
    releases: Releases,
    signer: Signer,
    fresh: Seeded,
    /// The router's bounds as the operator's settings leave them (`T-020-49`): the deployment's own, `base_bounds`, with
    /// the decided fee, slippage limits and ceilings in place, recomputed whenever a setting is set or Core reopens.
    bounds: PolicyBounds,
    /// The deployment's own bounds: hops, parts, policy version and exclusions, which no operator setting names.
    base_bounds: PolicyBounds,
    /// The operator's settings, each decided through policy and rebuilt from the log on reopening (`T-020-49`).
    settings: rutent_core_orch::settings::Settings,
    prices: Vec<Price>,
    quotes: BTreeMap<String, Quote>,
    requests: BTreeMap<String, OrderRequest>,
    started: BTreeMap<String, Started>,
    /// The state machine, fed from the log.
    pub feed: Feed,
    /// The deployment's policy, which a scenario may set to refuse.
    pub deployment: Deployment,
    /// The faults planted at the platform's seam.
    pub faults: Faults,
    /// Where a planted swap moves the state machine.
    pub swap_to: Option<Box<dyn Environment>>,
    /// Core's schedule: what each tick runs and when it last did (`T-020-48`). A restart forgets it, so a reopened
    /// Core runs every job on its first tick.
    schedule: rutent_core_orch::schedule::Schedule,
    /// The checkpointer Core's schedule runs (`REQ-AUD-14`, `T-020-48`), on the disk's checkpoint store, signing with
    /// the log's own key on the schedule fixed in advance; `None` where it refused the log or its store, which each
    /// tick then answers.
    checkpoints: Option<checking::ScheduledCheckpointer>,
    /// What the checkpointer published, as the deployment's publication channel received it, in order.
    published: Vec<Vec<u8>>,
}

/// Why a step of Core's failed.
#[derive(Debug)]
pub enum StepError {
    /// The log couldn't take an entry.
    Log(AppendFailure),
    /// Intake refused or stopped.
    Intake(IntakeError),
    /// The segments stopped.
    Segments(SegmentsError),
    /// A state read failed.
    Status(StatusError),
    /// Policy didn't open the session.
    Session(SessionOutcome),
    /// The state machine's host failed.
    Host(rutent_contracts::state_machine::host::HostError),
    /// Arrivals refused or stopped (`T-020-04`).
    Arrival(rutent_core_orch::arrival::ArrivalError),
    /// The order is in no state that step can take.
    NotNow(&'static str),
    /// Declaring a part stuck, or following its ends, refused or stopped (`T-020-16`).
    Stuck(StuckError),
    /// A step of a stuck order's resolution was refused or stopped (`T-020-16`).
    Resolve(ResolveError),
    /// A recalculation couldn't be tried (`T-024-06`).
    Recalc(rutent_core_orch::recalc::RecalcError),
    /// A reversal or an ending was refused or stopped (`T-024-06`).
    Reversal(rutent_core_orch::reversal::ReversalError),
}

impl From<StuckError> for StepError {
    fn from(e: StuckError) -> Self {
        StepError::Stuck(e)
    }
}

impl From<ResolveError> for StepError {
    fn from(e: ResolveError) -> Self {
        StepError::Resolve(e)
    }
}

impl From<rutent_core_orch::arrival::ArrivalError> for StepError {
    fn from(e: rutent_core_orch::arrival::ArrivalError) -> Self {
        StepError::Arrival(e)
    }
}

impl From<AppendFailure> for StepError {
    fn from(e: AppendFailure) -> Self {
        StepError::Log(e)
    }
}

impl From<IntakeError> for StepError {
    fn from(e: IntakeError) -> Self {
        StepError::Intake(e)
    }
}

impl From<SegmentsError> for StepError {
    fn from(e: SegmentsError) -> Self {
        StepError::Segments(e)
    }
}

impl From<rutent_contracts::state_machine::host::HostError> for StepError {
    fn from(e: rutent_contracts::state_machine::host::HostError) -> Self {
        StepError::Host(e)
    }
}

impl Core {
    /// Core put together under `config`, its state machine in `environment`, over an empty disk.
    pub fn assemble(
        config: &Config,
        environment: &mut dyn Environment,
    ) -> Result<Self, AssemblyError> {
        Self::assemble_on(config, environment, Disk::new())
    }

    /// [`Core::assemble`], over `disk`.
    pub(crate) fn assemble_on(
        config: &Config,
        environment: &mut dyn Environment,
        disk: Disk,
    ) -> Result<Self, AssemblyError> {
        Self::assemble_with(config, environment, disk, None)
    }

    /// Core opened again on its disk after it stopped (`T-020-09`): what is outside Core goes on as it stands (the
    /// platform with the providers and their networks, the state machine's host, the clock, and the operator's key with
    /// the decision records it signed), and Core's own parts are put together again over what the disk holds: the log
    /// and the evidence store read back, and, from the log, what the grading took (`Closing::recorded`), the held
    /// quotes, and Core's own state, rebuilt (`T-020-46`, the `reopen` module): the transactions asked under, intake's orders,
    /// the segments and the value in flight, the stuck orders, and what each started order runs on. The providers are
    /// granted again on the reopened log.
    ///
    /// # Errors
    ///
    /// If Core can't be put together again.
    pub fn reopened(
        self,
        config: &Config,
        environment: &mut dyn Environment,
    ) -> Result<Self, AssemblyError> {
        let carried = Carried {
            fleet: self.fleet,
            installed: self.installed,
            clock: self.clock,
            feed: self.feed,
            fresh: self.fresh,
            ties: self.readings.ties(),
            // The host's holdings are shared within Core (`Rc`), so they are taken out of the share, not moved.
            holdings: std::mem::take(&mut *self.holdings.borrow_mut()),
            holding_accounts: self.deployed.registers.borrow().holding_accounts.clone(),
            holding_decimals: self.deployed.registers.borrow().holding_decimals.clone(),
            publisher_accounts: self.deployed.registers.borrow().publisher_accounts.clone(),
            holding_assets: self.deployed.registers.borrow().holding_assets.clone(),
            restarts: self.restarts,
            confirm: None,
        };
        Self::assemble_with(config, environment, self.disk, Some(carried))
    }

    /// Core and its platform restarted after a crash mid-order (`T-024-21`; `NFR-6`, `INV-8`): every value either held
    /// in memory dropped, and both opened again from their disks. The platform is opened on its carried-out record, its
    /// releases admitted again ([`Fleet::restart_platform`]); the state machine's host on its journal
    /// ([`Feed::restarted`]); Core on its log and its evidence store, its own state rebuilt from the log as
    /// [`Core::reopened`] rebuilds it, with fresh identifiers drawn from a source of its own, never the one the Core
    /// that stopped drew from. What goes on is what is outside both: the providers' world, the clock, the operator's
    /// key, the deployment's registers and the host's holding accounts, whose wallets' records went with their
    /// modules' processes.
    ///
    /// A platform whose record didn't end whole holds every provider's releases until the operator confirms them; the
    /// operator's login confirms each provider's, through Core's decided path ([`Core::confirm_releases`]), as Core
    /// opens and before anything of an order is asked of a provider.
    ///
    /// # Errors
    ///
    /// If Core can't be put together again, the host can't be opened on its journal, or a confirmation is refused.
    pub fn restarted(
        self,
        config: &Config,
        environment: &mut dyn Environment,
    ) -> Result<Self, AssemblyError> {
        self.restart(config, environment, true)
    }

    /// [`Core::restarted`], with the platform's releases left held: nothing of a provider runs until the operator
    /// confirms it ([`Core::confirm_releases`]), for a test that reads what the platform does before then.
    ///
    /// # Errors
    ///
    /// As [`Core::restarted`].
    pub fn restarted_holding(
        self,
        config: &Config,
        environment: &mut dyn Environment,
    ) -> Result<Self, AssemblyError> {
        self.restart(config, environment, false)
    }

    fn restart(
        mut self,
        config: &Config,
        environment: &mut dyn Environment,
        confirm: bool,
    ) -> Result<Self, AssemblyError> {
        let restarts = self.restarts + 1;
        // What the scenario counted of the platform's seam, which is the scenario's, not Core's.
        let (delivered_twice, swap_failed) =
            (self.faults.delivered_twice, self.faults.swap_failed.take());
        // The platform's crash, and its restart on its record (`Fleet::restart_platform`): unclean, so it holds every
        // provider's releases until the operator confirms them. Its staging is the deployment's, set again.
        self.fleet.restart_platform(true);
        let fleet = self.fleet;
        self.feed
            .restarted(environment)
            .map_err(AssemblyError::Host)?;
        let carried = Carried {
            fleet,
            installed: self.installed,
            clock: self.clock,
            feed: self.feed,
            // Core's fresh identifiers come from a source of the restarted Core's own: a Core that drew the stopped one's
            // sequence again would draw identifiers the log already holds.
            fresh: Seeded::new(config.seed ^ 0x7e57_a27e_d000_0000 ^ (restarts as u64)),
            ties: self.readings.ties(),
            // The host's holding accounts go with the restart: what they held in memory, each order's attribution, the
            // transactions they asked under and the start they last saw, is rebuilt from the deployment's register, the
            // platform's record and the log as Core opens (`Core::reopen_holdings`), never carried.
            holdings: rutent_prov::holding::wallet::Holdings::default(),
            holding_accounts: self.deployed.registers.borrow().holding_accounts.clone(),
            publisher_accounts: self.deployed.registers.borrow().publisher_accounts.clone(),
            holding_assets: self.deployed.registers.borrow().holding_assets.clone(),
            holding_decimals: self.deployed.registers.borrow().holding_decimals.clone(),
            restarts,
            confirm: Some(confirm),
        };
        let mut core = Self::assemble_with(config, environment, self.disk, Some(carried))?;
        core.faults.delivered_twice = delivered_twice;
        core.faults.swap_failed = swap_failed;
        Ok(core)
    }

    /// The operator's login confirms the releases the platform holds after a restart whose record didn't end whole,
    /// provider by provider, at the versions the platform holds them at, through Core's decided path
    /// (`Core::change_release`, `ReleaseChange::Confirm`; `T-013-11`, `T-020-49`): policy decides, the effect is
    /// recorded before the platform is asked and its result after. Then what the segments owe the holding accounts is
    /// asked of them again, now that their wallets run. Answers the providers confirmed.
    ///
    /// # Errors
    ///
    /// If a confirmation is refused.
    pub fn confirm_releases(&mut self) -> Result<Vec<String>, AssemblyError> {
        let mut confirmed = Vec::new();
        for (provider, versions) in self.fleet.held() {
            let versions: Vec<Id> = versions.iter().filter_map(Id::new).collect();
            self.change_release(
                &provider,
                rutent_core_orch::segments::lifecycle::ReleaseChange::Confirm {
                    versions: &versions,
                },
            )
            .map_err(|e| {
                AssemblyError::Listing(format!("{provider}'s releases weren't confirmed: {e:?}"))
            })?;
            confirmed.push(provider);
        }
        self.reconcile_landings();
        Ok(confirmed)
    }

    /// How many times Core and its platform were restarted in this run ([`Core::restarted`]).
    pub fn restarts(&self) -> usize {
        self.restarts
    }

    fn assemble_with(
        config: &Config,
        environment: &mut dyn Environment,
        disk: Disk,
        carried: Option<Carried>,
    ) -> Result<Self, AssemblyError> {
        let context = context();
        let reopened = carried.is_some();
        let mut carried = carried;
        let carried_holdings = carried
            .as_mut()
            .map(|c| std::mem::take(&mut c.holdings))
            .unwrap_or_default();
        let holding_accounts = carried
            .as_mut()
            .map(|c| std::mem::take(&mut c.holding_accounts))
            .unwrap_or_default();
        let holding_decimals = carried
            .as_mut()
            .map(|c| std::mem::take(&mut c.holding_decimals))
            .unwrap_or_default();
        let publisher_accounts = carried
            .as_mut()
            .map(|c| std::mem::take(&mut c.publisher_accounts))
            .unwrap_or_default();
        let holding_assets = carried
            .as_mut()
            .map(|c| std::mem::take(&mut c.holding_assets))
            .unwrap_or_default();
        let restarts = carried.as_ref().map_or(0, |c| c.restarts);
        let confirm = carried.as_ref().and_then(|c| c.confirm);
        let (fleet, installed, clock, carried_feed, carried_fresh, ties) = match carried {
            Some(c) => (
                c.fleet,
                c.installed,
                c.clock,
                Some(c.feed),
                Some(c.fresh),
                c.ties,
            ),
            None => {
                // The platform and the simulated providers, staging set explicitly.
                // A holdout fleet speaks the old version and takes the current one's operations (`SuiteContext::older_versions`).
                let mut fleet_context = context.clone();
                if let Some((old, _)) = &config.holdout {
                    fleet_context.older_versions = vec![(
                        context.protocol_version.clone(),
                        "9999-12-31T23:59:59Z".to_owned(),
                    )];
                    fleet_context.protocol_version = old.clone();
                }
                let mut fleet = Fleet::with_context(config.deadline, fleet_context);
                let installed: Vec<Installed> = config
                    .providers
                    .iter()
                    .map(|spec| fleet.install(spec.clone()))
                    .collect();
                fleet.set_staging(config.staging.policy());

                // First trust in a provider is its grant's, and an advertisement made before its grant rides on
                // none: Core's clock starts before the earliest advertisement was made, for the grants.
                let earliest = installed
                    .iter()
                    .filter_map(|one| {
                        rutent_contracts::router_api::Timestamp::new(
                            &one.advertisement.statement().made_at,
                        )
                    })
                    .map(|t| u64::try_from(t.unix_nanos() / 1_000_000_000).unwrap_or(0))
                    .min()
                    .unwrap_or(kit::START);
                let clock =
                    SharedClock::at(earliest.saturating_sub(GRANTED_BEFORE).min(kit::START));
                (fleet, installed, clock, None, None, false)
            }
        };

        // The evidence store and the log: the store's read taken once, the log opened with it.
        let (store, log) = read_back(&disk, &clock).map_err(AssemblyError::Log)?;

        // The deployment's registers: who acts for whom, and who holds which permission.
        let registers = Rc::new(RefCell::new(Registers::default()));
        {
            let mut r = registers.borrow_mut();
            for actor in [
                kit::customer(),
                kit::core(),
                kit::operator(),
                kit::operator_login(),
            ] {
                r.list(&actor);
            }
            r.permit(&kit::core(), Permission::CarryOnward);
            // Core reads a private node's record from its publisher's provider where policy decides it, carrying the
            // operator's standing permission onward, beside `CarryOnward` (`REQ-ORD-41`, `REQ-AUZ-1`, `T-020-58`).
            r.permit(&kit::core(), Permission::ReadNodeRecord);
            // Core reads each holding account's balance where policy decides it, carrying the operator's standing
            // permission onward, beside `CarryOnward` (`REQ-AUD-15`, `REQ-AUZ-1`, `T-020-63`).
            r.permit(&kit::core(), Permission::ReadHoldingBalance);
            // And carries a segment's own operation out again where policy decides it (`REQ-ORD-1`, `T-020-63`).
            r.permit(&kit::core(), Permission::RepeatOperation);
            for change in [
                CatalogueChange::Grant,
                CatalogueChange::Promote,
                CatalogueChange::RollBack,
                // A release's lifecycle actions and the confirmation of a provider's releases (`T-020-49`).
                CatalogueChange::AcceptModule,
                CatalogueChange::WithdrawRelease,
                CatalogueChange::RevokeRelease,
                CatalogueChange::DeclareExploited,
                CatalogueChange::MarkStale,
                CatalogueChange::ConfirmReleases,
                // Each simulated network listed with its reader service (`T-024-25`, `DEC-28`).
                CatalogueChange::ListPublicNetwork,
            ] {
                r.permit(&kit::operator_login(), Permission::Change(change));
            }
            // Each of the operator's settings, a permission of its own apiece (`T-021-15`, `T-020-49`).
            for kind in [
                SettingKind::OperatorFee,
                SettingKind::SlippageLimit,
                SettingKind::EvidenceFloor,
                SettingKind::PublicNode,
                SettingKind::ProviderCeiling,
                SettingKind::PublisherCeiling,
                SettingKind::Cadence,
                SettingKind::Cutover,
                SettingKind::DepositWait,
            ] {
                r.permit(&kit::operator_login(), Permission::Setting(kind));
            }
            r.permit(&kit::operator_login(), Permission::ReadOrderState);
            // The operator's login erases a kept statement's bytes where policy decides it (`REQ-AUD-13`, `T-020-37`).
            r.permit(&kit::operator_login(), Permission::EraseStatement);
            // Not the override's permission (`EraseOverridingCitations`, `T-021-13`): the scenarios that need it grant it
            // through `Core::registers`, so the login erases a cited statement only where a scenario says it may
            // (`T-020-42`).
            // The operator's login files automatic deposits for the reserves that declared them (`REQ-ORD-13`,
            // `T-020-04`).
            r.permit(&kit::operator_login(), Permission::ActForUser);
            // The operator's login files an arrival anew where policy decides it (`REQ-ORD-23`, `T-020-52`).
            r.permit(&kit::operator_login(), Permission::RefileArrival);
            r.accounts.insert(
                context.account.clone(),
                kit::customer().party.as_str().to_owned(),
            );
            r.accounts.insert(
                DESTINATION_ACCOUNT.to_owned(),
                kit::customer().party.as_str().to_owned(),
            );
            // The payer's account at the origin reserve, which every order's inputs name (`payer.bank_account`):
            // policy holds an authorisation's origin account to the payer's own (`T-021-04`, the payer gap's ruling).
            r.accounts.insert(
                ORIGIN_ACCOUNT.to_owned(),
                kit::customer().party.as_str().to_owned(),
            );
            // The register of holding accounts the deployment kept, on a reopen (`T-020-55`).
            r.holding_accounts = holding_accounts;
            r.holding_decimals = holding_decimals;
            r.publisher_accounts = publisher_accounts;
            r.holding_assets = holding_assets;
        }
        // The operator's holding accounts, the host's: on a reopen they are the ones carried across it (`T-020-46`).
        let holdings: Rc<RefCell<rutent_prov::holding::wallet::Holdings>> =
            Rc::new(RefCell::new(carried_holdings));
        // Core's own sources, over its log and evidence store, and the deployment's registers beside them (`T-020-55`).
        let deployed = Deployed {
            registers,
            verifier: Rc::default(),
        };
        let sources = policy::CoreSources::new(
            policy::LogRead(Rc::clone(&log)),
            policy::LogRead(Rc::clone(&log)),
            deployed.clone(),
        );

        // Policy: the engine holds the log's policy handle, the only one, and reads Core's clock.
        let deployment = Deployment::default();
        let mut engine: PolicyEngine = Engine::with_policy(
            PolicyHandle(Rc::clone(&log)),
            deployment.clone(),
            sources.clone(),
        );
        engine.set_clock(Box::new(clock.clone()));
        // Policy's register of sessions and read permissions, rebuilt from the decisions the log holds, the
        // register's only durable record: empty on a fresh disk, the register the engine that ran on held on a
        // reopened one (`T-021-17`).
        engine.reopen();
        let policy = PolicySide::new(
            Rc::new(RefCell::new(engine)),
            sources.clone(),
            deployment.clone(),
        );

        // Orchestration's handle on the evidence store, its reads checked by policy, and then the store bound to the
        // one log it serves.
        let keeper = store.keeper(Box::new(policy.clone()));
        // The log's own read of the store, a second handle on the same shelf, for comparing the terms a provider
        // stated with the bytes kept behind their commitment.
        let kept = store.for_the_log();
        let bound = store.bind(Rc::clone(&log)).map_err(AssemblyError::Log)?;

        let mut catalogue = Catalogue::new();
        catalogue.set_context(context.deployment.clone(), &context.protocol_version);
        // The deployment's bound on balance checks and how long a reserve has to answer one (`REQ-CAT-4`).
        catalogue.set_check_bound(rutent_prov::catalogue::CheckBound {
            max_counted: CHECKS_PER_WINDOW,
            window_secs: 86_400,
        });
        catalogue.set_answer_within(CHECK_ANSWERED_WITHIN);
        // The sessions policy's register holds, handed to the catalogue's again, as each was when it opened.
        policy.engine().borrow().register().open_in(&mut catalogue);
        let prices = config.providers.iter().flat_map(kit::prices_of).collect();

        let feed = match carried_feed {
            Some(feed) => feed,
            None => {
                Feed::new(environment, config.delivery, config.seed).map_err(AssemblyError::Host)?
            }
        };
        let mut core = Core {
            general: GeneralHandle(Rc::clone(&log)),
            log,
            clock,
            disk,
            restarts,
            bound,
            keeper,
            policy,
            sources,
            deployed,
            fleet,
            installed,
            catalogue,
            intake: Intake::new(),
            arrivals: rutent_core_orch::arrival::Arrivals::new(),
            filings: BTreeMap::new(),
            segments: Segments::new(),
            stuck: Stuck::new(),
            mover: Mover::new(),
            holdings,
            linked: BTreeMap::new(),
            hand_on: rutent_core_orch::hand_on::HandOn::new(),
            notices: rutent_core_orch::notice::Notices::new(
                kit::id("rutent-scenarios"),
                kit::operator().party,
            ),
            notice_signer: charges::NoticeSigner::assembly(),
            wire: charges::Wire::default(),
            kept,
            kept_through: None,
            closing: rutent_core_orch::closing::Closing::new(),
            holds: rutent_core_orch::quote::Holds::new(),
            readings: grading::Readings::default(),
            issued: Issued::new(),
            life: lifecycle::Lifecycle::new(),
            exposures: Exposures::new(),
            reported: rutent_core_orch::quote::Prices::new(),
            closed: price::Priced::default(),
            releases: Releases::new(),
            signer: Signer {
                key: RealKey::assembly(),
                records: BTreeMap::new(),
                signed: Vec::new(),
            },
            fresh: carried_fresh.unwrap_or_else(|| Seeded::new(config.seed)),
            bounds: kit::bounds(&context),
            base_bounds: kit::bounds(&context),
            settings: rutent_core_orch::settings::Settings::new(),
            prices,
            quotes: BTreeMap::new(),
            requests: BTreeMap::new(),
            started: BTreeMap::new(),
            feed,
            deployment,
            faults: Faults::default(),
            swap_to: None,
            schedule: rutent_core_orch::schedule::Schedule::new(),
            checkpoints: None,
            published: Vec::new(),
        };
        // Core's checkpointer, over its log and the disk's checkpoint store, which a reopened Core reads back.
        core.checkpoints = checking::ScheduledCheckpointer::new(
            Rc::clone(&core.log),
            core.disk.checkpoint_store(),
            checking::log_key(),
            checking::schedule(),
        )
        .ok();
        // Each provider is granted before the advertisement it sends was made; then the clock moves to the
        // scenario's start, and each advertisement is recorded as received then.
        // What the grading reads, and what it took, rebuilt from the log the disk holds (`T-020-09`).
        core.readings = grading::Readings::of(&core.installed);
        if ties {
            core.readings.tie_by_destination();
        }
        core.closing = rutent_core_orch::closing::Closing::recorded(core.log.borrow().entries());
        core.holds = rutent_core_orch::quote::Holds::recorded(core.log.borrow().entries());
        if reopened {
            // A host restarted with its platform opens its holding accounts again from what is kept beyond its memory
            // (`T-024-21`); one reopened alone carried them.
            if confirm.is_some() {
                core.reopen_holdings();
            }
            // The providers were listed before Core stopped: its catalogue is rebuilt from what its log holds, the host's
            // holding accounts listed again first.
            core.relist_holdings();
            core.relist_publisher_accounts();
            core.relist()?;
            // A platform restarted beside Core holds its releases until the operator confirms them, which the operator's
            // login does through Core's decided path before anything is asked of a provider (`T-024-21`).
            if confirm == Some(true) {
                core.confirm_releases()?;
            }
            // Core's own state, rebuilt from the log (`T-020-46`), the proven starts of `T-020-26` among it.
            core.rebuild_from_log();
            // The schedule's own state: when each segment was last asked its status, so the first tick after a restart
            // keeps each provider's cadence (`T-020-63`).
            core.schedule =
                rutent_core_orch::schedule::Schedule::recorded(core.log.borrow().entries());
            // The operator's settings, rebuilt from the log, a setting's missing result completed, and put back where they
            // are read (`T-020-49`).
            let (settings, completed) = rutent_core_orch::settings::Settings::reopen(
                &mut core.general,
                &core.clock,
                &kit::core(),
                &mut rutent_core_orch::settings::Targets {
                    catalogue: Some(&mut core.catalogue),
                    ..rutent_core_orch::settings::Targets::default()
                },
            );
            completed.map_err(|e| {
                AssemblyError::Listing(format!("a setting's result wasn't completed: {e:?}"))
            })?;
            core.settings = settings;
            core.bounds = core.settings.bounds(&core.base_bounds);
            core.settings
                .apply(&mut rutent_core_orch::settings::Targets {
                    catalogue: Some(&mut core.catalogue),
                    cadences: Some(core.schedule.cadences_mut()),
                    arrivals: Some(&mut core.arrivals),
                    platform: Some(&mut core.fleet.platform),
                    segments: None,
                });
            // A restrictive lifecycle effect whose result the log never took is completed from what the platform holds
            // (`T-020-65`), as a setting's is.
            {
                let releases = core.fleet.releases();
                Segments::reopen_lifecycle(&mut core.general, &releases, &core.clock, &kit::core())
                    .map_err(|e| {
                        AssemblyError::Listing(format!(
                            "a lifecycle effect's result wasn't completed: {e:?}"
                        ))
                    })?;
            }
            return Ok(core);
        }
        let installed = core.installed.clone();
        for one in &installed {
            core.grant(one)?;
        }
        core.list_public_networks()?;
        core.clock.set_nanos(u128::from(kit::START) * 1_000_000_000);
        // The holdout's version is put in force until its cutover before its providers are listed, through the
        // operator's command as any setting is (`REQ-PRT-4`).
        if let Some((old, cutover)) = &config.holdout {
            let setting = OperatorSetting::Cutover {
                version: rutent_contracts::router_api::ProtocolVersion::new(old.as_str())
                    .ok_or_else(|| AssemblyError::Listing("a holdout version".to_owned()))?,
                cutover: rutent_contracts::router_api::Timestamp::new(cutover.as_str())
                    .ok_or_else(|| AssemblyError::Listing("a holdout cutover".to_owned()))?,
            };
            core.set_setting(&setting)
                .map_err(|e| AssemblyError::Listing(format!("the holdout's cutover: {e:?}")))?;
        }
        for one in &installed {
            core.advertise(one)?;
        }
        // The operator's slippage limit on each exchange tuple the providers serve, the deployment's starting settings,
        // set through the operator's command as any setting is (`T-020-49`).
        for limit in kit::bounds_for(&context, &config.providers).slippage_limits {
            core.set_setting(&OperatorSetting::SlippageLimit { limit })
                .map_err(|e| AssemblyError::Listing(format!("the slippage limit: {e:?}")))?;
        }
        Ok(core)
    }

    /// Every decision record Core signed, in the order it signed them.
    pub fn decision_records(&self) -> Vec<DecisionRecord> {
        self.signer
            .in_order()
            .into_iter()
            .filter_map(|(_, bytes)| record_in(bytes))
            .collect()
    }

    /// Every decision record Core signed, as signed: its id and the envelope's bytes, whether or not they read back as
    /// a record (`T-024-22`: a scan that read only the records that parse would miss one that doesn't).
    pub fn decision_record_bytes(&self) -> Vec<(String, Vec<u8>)> {
        self.signer
            .in_order()
            .into_iter()
            .map(|(id, bytes)| (id.clone(), bytes.clone()))
            .collect()
    }

    /// A control for the check that every record Core signed is read (`T-024-22`): rewrites the first record kept with
    /// its `candidates` set to a string, so it no longer reads back as a record. False if Core kept none.
    pub fn corrupt_decision_record(&mut self) -> bool {
        let Some(bytes) = self.signer.first_mut() else {
            return false;
        };
        let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
            return false;
        };
        value["statement"]["candidates"] = "not a list".into();
        match serde_json::to_vec(&value) {
            Ok(written) => {
                *bytes = written;
                true
            }
            Err(_) => false,
        }
    }

    /// A control for the scan of the records Core signs (`T-024-22`): writes `leak` into the first decision record
    /// Core kept, in the text field of the record `at` names (a JSON pointer under the statement, such as
    /// `/request/request_id`), as a record that carried a figure would. Core sets the request's identifier and the
    /// rest itself, so a customer's figure can't be put there from outside; this puts it there from inside. False if
    /// Core kept no record, or the record has no text there, or reads back as no record with the figure in it.
    pub fn leak_into_decision_record(&mut self, leak: &str, at: &str) -> bool {
        // The first record signed that has text at `at`: a refining round's record has fields the first round's hasn't.
        let Some(id) = self
            .signer
            .in_order()
            .into_iter()
            .find(|(_, bytes)| {
                serde_json::from_slice::<serde_json::Value>(bytes)
                    .ok()
                    .is_some_and(|v| v["statement"].pointer(at).is_some_and(|f| f.is_string()))
            })
            .map(|(id, _)| id.clone())
        else {
            return false;
        };
        let Some(bytes) = self.signer.records.get_mut(&id) else {
            return false;
        };
        let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
            return false;
        };
        let Some(field) = value["statement"].pointer_mut(at).filter(|f| f.is_string()) else {
            return false;
        };
        // An amount stays a number, which is how a figure would sit there; any other text takes the figure after a word.
        let numeric = field
            .as_str()
            .is_some_and(|text| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()));
        *field = if numeric {
            leak.to_owned()
        } else {
            format!("req-{leak}")
        }
        .into();
        match serde_json::to_vec(&value)
            .ok()
            .filter(|w| record_in(w).is_some())
        {
            Some(written) => {
                *bytes = written;
                true
            }
            None => false,
        }
    }

    /// From now on, the destination's network ties each deposit to the first movement to its account of its amount, as
    /// a reserve paying two orders with one payment would have it: a fault the scenarios plant (`T-020-09`).
    pub fn tie_payments_by_destination(&mut self) {
        self.readings.tie_by_destination();
    }

    /// From now on, `provider`'s record of the segments inside its node is altered after it was signed, as a forger
    /// would: a fault the scenarios plant (`T-024-14`). Core's books refuse it, so the replay finds nothing recorded.
    pub fn forge_record_of(&mut self, provider: &str) {
        self.readings.forge(provider, Tamper::Figure);
    }

    /// From now on, `provider`'s answer to the question of its record is altered as `how` says (`T-020-58`): Core takes
    /// it only as a record of the node asked about, so the replay finds nothing recorded.
    pub fn tamper_with_record_of(&mut self, provider: &str, how: Tamper) {
        self.readings.forge(provider, how);
    }

    /// How many records closing's books hold now, whatever the node: the orders still in flight through a private node
    /// hold theirs, and an order whose segments there have settled holds none (`T-020-58`).
    pub fn records_held(&self) -> usize {
        self.readings.held()
    }

    /// The records of the private node `node` Core took of its providers, as closing's books hold them (`T-020-58`).
    pub fn records_taken(
        &self,
        node: &str,
    ) -> Vec<Statement<rutent_contracts::provider_role::InnerSegments>> {
        self.readings.taken(node)
    }

    /// Every question of a private node's record Core put to a provider, as the platform carried it: the provider, the
    /// node and the transaction, in the order put (`T-020-58`). The assembly chooses none: Core does, from the routes
    /// it runs, when it closes a segment and on its tick.
    pub fn publishers_asked(&self) -> Vec<(String, String, String)> {
        self.readings.asked()
    }

    /// The log.
    pub fn log(&self) -> std::cell::Ref<'_, CoreLog> {
        self.log.borrow()
    }

    /// The bytes the evidence store keeps behind `commitment`, as the log's own read finds them: what a stated entry
    /// rests on, read back by a scenario.
    pub fn kept_bytes(
        &mut self,
        commitment: &rutent_contracts::module_interface::Digest,
    ) -> Option<Vec<u8>> {
        let kept = rutent_contracts::core_internal::calls::KeptStatements::kept(
            &mut self.kept,
            commitment,
        )
        .ok()??;
        rutent_core_audit::log::kept::from_base64url(kept.bytes.as_str())
    }

    /// The evidence store, bound to the log.
    pub fn bound(&self) -> &BoundStore<LogDisk, LogsRead> {
        &self.bound
    }

    /// The policy engine.
    pub fn engine(&self) -> std::cell::Ref<'_, PolicyEngine> {
        self.policy.engine().borrow()
    }

    /// Every transaction the routing rounds asked under, kept across routings.
    pub fn issued(&self) -> &Issued {
        &self.issued
    }

    /// Core's catalogue, to read.
    pub fn catalogue(&self) -> &Catalogue {
        &self.catalogue
    }

    /// Core's record of value in flight and of the orders declared stuck, to read (`T-024-04`).
    pub fn exposures(&self) -> &Exposures {
        &self.exposures
    }

    /// What Core's books give for private node `node` and the segments run under `transactions`, as the closing reads
    /// them (`T-024-14`).
    pub fn books_for(
        &self,
        node: &str,
        transactions: &[Id],
    ) -> Option<Vec<rutent_evidence::replay::RecordedSegment>> {
        // The books are what Core took of its providers' answers when it last asked them (`T-020-58`).
        self.readings.refresh(&self.fleet.platform.world);
        self.readings
            .recorded_ever(node, transactions)
            .map(|r| r.segments)
    }

    /// Restarts the module of `provider` on the platform, and no other, as its process ending and starting again does:
    /// a module's state in its instance, a plain wallet's record among it, is gone (`REQ-ORD-43`, `T-020-46`). False
    /// where it runs no process.
    pub fn restart_module(&mut self, provider: &str) -> bool {
        self.fleet.platform.restart(provider)
    }

    /// The platform with the simulated providers, to read.
    pub fn fleet(&self) -> &Fleet {
        &self.fleet
    }

    /// The platform with the simulated providers, for a test that hands it an operation itself or reads what it reports
    /// under a hook of its own (`T-024-21`): nothing of Core's is asked.
    pub fn fleet_mut(&mut self) -> &mut Fleet {
        &mut self.fleet
    }

    /// The simulated world, to script what a provider does next or to read what it did: its scripts, its books and
    /// what its rails and networks received. Core's platform stays a view ([`Core::fleet`]).
    pub fn world(&mut self) -> &mut SimWorld {
        self.fleet.world()
    }

    /// Sets `setting` by the operator's command, as the operator's login asks and policy decides (`T-020-49`): Core's
    /// `Settings::set` records the effect before it applies the setting and its result after, and a reopened Core
    /// rebuilds it from the log. The router's bounds of every route decided from now on carry it.
    ///
    /// # Errors
    ///
    /// If policy doesn't permit it, the log takes no step of it, or it is a deposit wait above its ceiling.
    pub fn set_setting(
        &mut self,
        setting: &OperatorSetting,
    ) -> Result<Receipt, rutent_core_orch::settings::SettingRefused> {
        self.set_setting_as(&kit::operator_login(), setting)
    }

    /// [`Core::set_setting`], asked by `caller`.
    ///
    /// # Errors
    ///
    /// As [`Core::set_setting`].
    pub fn set_setting_as(
        &mut self,
        caller: &Actor,
        setting: &OperatorSetting,
    ) -> Result<Receipt, rutent_core_orch::settings::SettingRefused> {
        let mut decisions = self.policy.clone();
        let core = kit::core();
        let set = self.settings.set(
            &mut self.general,
            &mut decisions,
            &self.clock,
            &mut self.fresh,
            &mut rutent_core_orch::settings::Targets {
                catalogue: Some(&mut self.catalogue),
                cadences: Some(self.schedule.cadences_mut()),
                arrivals: Some(&mut self.arrivals),
                platform: Some(&mut self.fleet.platform),
                segments: Some(&self.segments),
            },
            &rutent_core_orch::settings::SettingAsked {
                setting,
                caller,
                core: &core,
            },
        );
        self.bounds = self.settings.bounds(&self.base_bounds);
        set
    }

    /// The operator's settings, as Core holds them (`T-020-49`).
    pub fn settings(&self) -> &rutent_core_orch::settings::Settings {
        &self.settings
    }

    /// A fault the scenarios plant (`T-020-66`): `action`, by the operator's login, decided by policy and its effect
    /// appended, and nothing after it: what Core's log holds when Core stops between an effect and its result, the
    /// platform never asked. The effect's receipt.
    ///
    /// # Errors
    ///
    /// If policy doesn't permit it, or the log takes no step of it.
    pub fn effect_without_its_result(&mut self, action: Action) -> Result<Receipt, AssemblyError> {
        let listing =
            |step: &str, e: AppendFailure| AssemblyError::Listing(format!("{step}: {e:?}"));
        let request = DecisionRequest {
            request: self.fresh.id(),
            caller: kit::operator_login(),
            action: action.clone(),
            at: self.clock.timestamp(),
        };
        let decision =
            rutent_contracts::core_internal::calls::Decisions::decide(&mut self.policy, &request)
                .map_err(|e| listing("the decision", e))?;
        let permit = decision.permit().ok_or_else(|| {
            AssemblyError::Listing(format!("not permitted: {:?}", decision.outcome))
        })?;
        self.verified(
            What::Effect {
                action,
                decision: permit,
            },
            Outcome::ToTakeEffect {},
        )
        .map_err(|e| listing("the effect", e))
    }

    /// Sets the weakest evidence the operator accepts on `tuple` (`REQ-ORD-39`) through the operator's command
    /// ([`Core::set_setting`], `T-020-49`): a provider declaring weaker is left out of the snapshot for it.
    ///
    /// # Panics
    ///
    /// If no price Core holds names the tuple, or policy doesn't permit the setting.
    pub fn set_evidence_floor(
        &mut self,
        tuple: rutent_prov::catalogue::TupleKey,
        floor: rutent_contracts::provider_role::EvidenceGrade,
    ) {
        let routed = self
            .prices
            .iter()
            .map(|p| p.tuple.clone())
            .find(|t| rutent_core_orch::settings::catalogue_tuple(t) == tuple)
            .expect("a tuple Core prices");
        self.set_setting(&OperatorSetting::EvidenceFloor {
            tuple: routed,
            grade: floor,
        })
        .expect("the operator's evidence floor is set");
    }

    /// Sets the operator's own fee (`REQ-QTE-2`) through the operator's command ([`Core::set_setting`], `T-020-49`):
    /// the formula the bounds of every route decided from now on carry. A route already decided keeps the formula its
    /// record ran under.
    ///
    /// # Panics
    ///
    /// If policy doesn't permit the setting.
    pub fn set_operator_fee(&mut self, fee: rutent_contracts::router_api::FeeFormula) {
        self.set_setting(&OperatorSetting::OperatorFee { formula: fee })
            .expect("the operator's fee is set");
    }

    /// Sets the most in flight at a provider in one asset (`REQ-ORD-8`) through the operator's command
    /// ([`Core::set_setting`], `T-020-49`): the bounds of every route decided from now on carry it, in place of any the
    /// provider had in that asset, and a Core opened again rebuilds it from the log.
    ///
    /// # Panics
    ///
    /// If policy doesn't permit the setting.
    pub fn set_provider_ceiling(&mut self, ceiling: rutent_contracts::router_api::ProviderCeiling) {
        self.set_setting(&OperatorSetting::ProviderCeiling { ceiling })
            .expect("the provider's ceiling is set");
    }

    /// The holder's delegate asks `reserve` whether the account's balance covers `amount`, in `session`: a balance
    /// check outside any order, so it counts against the account's bound (`REQ-CAT-3`, `REQ-CAT-4`). It goes through
    /// Core's catalogue, which logs it and counts it, then to the reserve through the platform, and the answer is
    /// read through the catalogue, so what the asker is told is yes or no and nothing more (`INV-9`).
    ///
    /// # Errors
    ///
    /// The refusal the catalogue gives where the check can't be sent: no session, over the bound, no grant.
    ///
    /// # Panics
    ///
    /// If the platform doesn't answer.
    pub fn check_balance(
        &mut self,
        reserve: &str,
        session: &Id,
        amount: &str,
    ) -> Result<rutent_prov::catalogue::Verdict, rutent_prov::catalogue::CheckRefusal> {
        let context = context();
        let statement = json!({
            "kind": "balance_verification",
            "transaction": self.fresh.id().as_str(),
            "provider": reserve,
            "session": session.as_str(),
            "account": context.account,
            "asset": "COP",
            "amount": amount,
        });
        self.check(reserve, statement)
    }

    /// [`Core::check_balance`] by a dry-run withdrawal of `amount` from `reserve`'s bank, which reveals whether the
    /// funds suffice as a verification does, and counts as one (`REQ-CAT-4`).
    ///
    /// # Errors
    ///
    /// As [`Core::check_balance`].
    ///
    /// # Panics
    ///
    /// If the platform doesn't answer.
    pub fn check_by_dry_run(
        &mut self,
        reserve: &str,
        session: &Id,
        amount: &str,
    ) -> Result<rutent_prov::catalogue::Verdict, rutent_prov::catalogue::CheckRefusal> {
        let statement = json!({
            "kind": "withdrawal",
            "transaction": self.fresh.id().as_str(),
            "provider": reserve,
            "session": session.as_str(),
            "origin": { "network": format!("{reserve}-bank") },
            "destination": format!("{reserve}-ledger"),
            "asset": "COP",
            "amount": amount,
            "dry_run": true,
            "inputs": { "payer.bank_account": ORIGIN_ACCOUNT },
        });
        self.check(reserve, statement)
    }

    fn check(
        &mut self,
        reserve: &str,
        statement: serde_json::Value,
    ) -> Result<rutent_prov::catalogue::Verdict, rutent_prov::catalogue::CheckRefusal> {
        let operation = rutent_conformance::sim::operation(&context(), statement);
        let ticket =
            self.catalogue
                .begin_check(&operation.statement, None, self.clock.instant())?;
        self.fleet.platform.world.set_clock(self.clock.instant());
        let Handled::Answered(result) = self.fleet.platform.handle(reserve, &operation) else {
            panic!("the platform refused the check");
        };
        Ok(self
            .catalogue
            .answer_check(&ticket, &result.statement, self.clock.instant()))
    }

    /// Policy, as the other parts reach it: the engine's calls, and the ones answered beside it.
    pub fn policy(&self) -> PolicySide {
        self.policy.clone()
    }

    /// Policy's sources: Core's own, over its log and evidence store, and the deployment's registers beside them
    /// (`T-020-55`).
    pub fn sources(&self) -> policy::CoreSources {
        self.sources.clone()
    }

    /// Sets what the deployment's verifier stand-in does from now on (`T-024-07`).
    pub fn verifier_is(&self, verifier: policy::Verifier) {
        self.deployed.verifier_is(verifier);
    }

    /// `reader`'s read of the audit trail, by the identified `permission` it names (`REQ-AUD-12`, `T-020-55`): Core's
    /// entry point, at Core's clock under a fresh request id. Policy decides and records the read; the trail is handed
    /// over only on a read it allowed and the log shows recorded, and every other reader is handed nothing of it
    /// (`rutent_core_audit::measures::hand_trail`). The transport in front of it is the deployment's (`F-030`).
    pub fn read_trail(
        &mut self,
        reader: &Actor,
        permission: Option<Id>,
    ) -> rutent_core_audit::measures::Handed {
        let asked = rutent_core_audit::measures::TrailRequest {
            request: self.fresh.id(),
            reader: reader.clone(),
            permission,
            at: self.clock.timestamp(),
        };
        let mut policy = self.policy.clone();
        let log = Rc::clone(&self.log);
        rutent_core_audit::measures::hand_trail(&mut policy, &asked, || {
            log.borrow().entries().to_vec()
        })
    }

    /// The deployment's registers.
    pub fn registers(&self) -> std::cell::RefMut<'_, Registers> {
        self.deployed.registers.borrow_mut()
    }

    /// Grants `installed`, tuple by tuple: each decided by the operator's login as a catalogue change naming the
    /// tuple and the advertising key, and its effect recorded; then that tuple granted in Core's catalogue, at its
    /// effect's time, by the decider, under the key; then the effect's `done` result. So nothing is granted before its
    /// record.
    ///
    /// The catalogue's grant equals the log's on what the log's grant holds: the tuple, the advertising key, the
    /// decider and the effect's time. Its party, expiry, categories and reversal capacity are the deployment's
    /// settings ([`kit::grant_terms`]), which core-internal's `change_catalogue` doesn't carry.
    ///
    /// # Errors
    ///
    /// If policy doesn't permit a tuple's grant, or the log takes no step of it.
    pub fn grant(&mut self, installed: &Installed) -> Result<(), AssemblyError> {
        let listing =
            |step: &str, e: AppendFailure| AssemblyError::Listing(format!("{step}: {e:?}"));
        let statement = installed.advertisement.statement();
        let provider = kit::id(&statement.statement.provider);
        let key = statement.signature.key.clone();
        let decider = kit::operator_login();
        // A verifier advertises classes, not tuples: its grant is logged naming a class, one grant for each (`T-020-30`),
        // and the keys that grant trusts attribute its judgements of that class (`T-024-18`).
        let verifier_classes = statement
            .statement
            .offers
            .verifier
            .as_ref()
            .filter(|_| statement.statement.offers.reserve.is_none())
            .map(|v| v.classes.clone())
            .unwrap_or_default();
        let tuples: Vec<(
            Option<Id>,
            Option<Id>,
            Option<rutent_prov::catalogue::TupleKey>,
        )> = rutent_prov::catalogue::tuples_of(&statement.statement)
            .into_iter()
            .map(|t| (Some(kit::tuple_id(provider.as_str(), &t)), None, Some(t)))
            .chain(
                verifier_classes
                    .iter()
                    .map(|class| (None, Some(kit::id(class)), None)),
            )
            .collect();
        for (tuple_id, class, tuple) in tuples {
            let action = Action::ChangeCatalogue {
                change: CatalogueChange::Grant,
                provider: provider.clone(),
                version: None,
                tuple: tuple_id,
                class,
                advertising_keys: vec![key.clone()],
                statement_key: None,
                setting: None,
                versions: Vec::new(),
                public_network: None,
            };
            let request = DecisionRequest {
                request: self.fresh.id(),
                caller: decider.clone(),
                action: action.clone(),
                at: self.clock.timestamp(),
            };
            let decision = rutent_contracts::core_internal::calls::Decisions::decide(
                &mut self.policy,
                &request,
            )
            .map_err(|e| listing("the grant's decision", e))?;
            let permit = decision.permit().ok_or_else(|| {
                AssemblyError::Listing(format!(
                    "the grant wasn't permitted: {:?}",
                    decision.outcome
                ))
            })?;
            let effect = self
                .verified(
                    What::Effect {
                        action,
                        decision: permit,
                    },
                    Outcome::ToTakeEffect {},
                )
                .map_err(|e| listing("the grant's effect", e))?;
            // Carried out on the terms the log holds, under an id of its own, the effect's place in the log, so a
            // tuple granted again is granted anew.
            let mut terms =
                kit::grant_terms(installed, decider.acts.as_str(), self.time_of(&effect));
            terms.id = format!("grant-{}-{}", provider.as_str(), u64::from(effect.index));
            terms.tuples = tuple.into_iter().collect();
            kit::grant(&mut self.catalogue, installed, terms);
            self.verified(What::Result { effect }, Outcome::Done {})
                .map_err(|e| listing("the grant's result", e))?;
        }
        self.record_release(installed);
        Ok(())
    }

    /// Lists each simulated network a provider's tuples are on with its reader service, as the operator does: a
    /// `list_public_network` the operator's login asks for and policy decides, its effect and its result, one for each
    /// network, before any order (`T-024-25`, `DEC-28`; [`Fleet::public_networks`]). Each decision is asked under an
    /// id of the network's own (`list-{network}`), never one drawn from Core's source, so the listings leave every
    /// identifier a scenario draws as it was.
    fn list_public_networks(&mut self) -> Result<(), AssemblyError> {
        let listing = |step: &str, e: AppendFailure| {
            AssemblyError::Listing(format!("a public network's {step}: {e:?}"))
        };
        let decider = kit::operator_login();
        for network in self.fleet.public_networks() {
            let request = DecisionRequest {
                request: kit::id(&format!("list-{}", network.network.as_str())),
                caller: decider.clone(),
                action: Action::ChangeCatalogue {
                    change: CatalogueChange::ListPublicNetwork,
                    provider: decider.party.clone(),
                    version: None,
                    tuple: None,
                    class: None,
                    advertising_keys: Vec::new(),
                    statement_key: None,
                    setting: None,
                    versions: Vec::new(),
                    public_network: Some(Box::new(network)),
                },
                at: self.clock.timestamp(),
            };
            let decision = rutent_contracts::core_internal::calls::Decisions::decide(
                &mut self.policy,
                &request,
            )
            .map_err(|e| listing("decision", e))?;
            let permit = decision.permit().ok_or_else(|| {
                AssemblyError::Listing(format!(
                    "a public network's listing wasn't permitted: {:?}",
                    decision.outcome
                ))
            })?;
            let effect = self
                .verified(
                    What::Effect {
                        action: request.action,
                        decision: permit,
                    },
                    Outcome::ToTakeEffect {},
                )
                .map_err(|e| listing("effect", e))?;
            self.verified(What::Result { effect }, Outcome::Done {})
                .map_err(|e| listing("result", e))?;
        }
        Ok(())
    }

    /// Every public network the log lists, by a decided `list_public_network` carried out, in the order listed: what
    /// names each network's reader service (`T-024-25`, `DEC-28`).
    pub fn public_networks(&self) -> Vec<rutent_contracts::core_internal::PublicNetwork> {
        let log = self.log.borrow();
        let entries = log.entries();
        let done: Vec<Receipt> = entries
            .iter()
            .filter_map(|(_, e)| match e {
                Entry::Verified(VerifiedEntry {
                    what: What::Result { effect },
                    outcome: Outcome::Done {},
                    ..
                }) => Some(effect.clone()),
                _ => None,
            })
            .collect();
        entries
            .iter()
            .filter(|(receipt, _)| done.contains(receipt))
            .filter_map(|(_, e)| match e {
                Entry::Verified(VerifiedEntry {
                    what:
                        What::Effect {
                            action:
                                Action::ChangeCatalogue {
                                    change: CatalogueChange::ListPublicNetwork,
                                    public_network: Some(network),
                                    ..
                                },
                            ..
                        },
                    ..
                }) => Some((**network).clone()),
                _ => None,
            })
            .collect()
    }

    /// Whether the planted fault ties a network's movements to segments by destination and amount
    /// ([`Core::tie_payments_by_destination`]): what a simulated reader planted the same way matches by (`T-024-25`).
    pub fn ties_by_destination(&self) -> bool {
        self.readings.ties()
    }

    /// The public records Core reads now: the simulated networks as they stand, read again (`T-020-09`), through the
    /// assembly's `impl PublicRecords` that `T-020-60` replaces with Core's reader adapter. What a simulated reader is
    /// held to answer as (`T-024-25`).
    pub fn public_records(&self) -> &dyn rutent_core_orch::closing::PublicRecords {
        self.readings.refresh(&self.fleet.platform.world);
        &self.readings
    }

    /// A judgement's origin names its maker's release, so a verifier's accepted release is recorded; a reserve's is
    /// not, since recording one would add its calls figure to every snapshot (`T-024-12`).
    fn record_release(&mut self, installed: &Installed) {
        if installed.spec.role == rutent_conformance::sim::Role::Verifier
            && let Some(released) = &installed.released
        {
            let _ = self.releases.record(released);
        }
    }

    /// Grants a verifier its category in the catalogue where the log's grants are replayed (`Core::relist`), which
    /// finds no tuple behind a class grant. Nothing for a provider that offers a tuple.
    fn grant_verifier(&mut self, installed: &Installed) {
        self.record_release(installed);
        let ad = &installed.advertisement.statement().statement;
        if ad.offers.verifier.is_some() && rutent_prov::catalogue::tuples_of(ad).is_empty() {
            let terms = kit::grant_terms(
                installed,
                kit::operator_login().acts.as_str(),
                rutent_prov::catalogue::Instant(1_000),
            );
            kit::grant(&mut self.catalogue, installed, terms);
        }
    }

    /// Core's catalogue rebuilt from its log, as after a restart: each tuple granted by a `done` grant, at its effect's
    /// time and under its id, as [`Core::grant`] granted it, and each advertisement the log holds joined at the time
    /// the log received it, as [`Core::advertise`] joined it, once the bytes the evidence store kept under the log's
    /// commitment are the installed provider's advertisement. A `done` catalogue change of any other kind (a revoke, a
    /// release accepted, revoked or promoted) refuses the reopening: it isn't replayed, and a catalogue rebuilt
    /// without it would grant what the log revoked.
    ///
    /// # Errors
    ///
    /// On a catalogue change it doesn't replay, or an advertisement whose kept bytes aren't the installed one's.
    fn relist(&mut self) -> Result<(), AssemblyError> {
        let entries: Vec<(Receipt, Entry)> = self.log.borrow().entries().to_vec();
        let kept = rutent_core_audit::receipts::store::Shelf::load(&mut self.disk.shelf())
            .map_err(|e| AssemblyError::Listing(format!("the evidence store's shelf: {e:?}")))?
            .kept;
        let done: Vec<&Receipt> = entries
            .iter()
            .filter_map(|(_, e)| match e {
                Entry::Verified(v) if v.outcome == (Outcome::Done {}) => match &v.what {
                    What::Result { effect } => Some(effect),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        let decider = kit::operator_login();
        // Each installed provider the log holds an advertisement of, and whether the log committed to the installed
        // advertisement's own bytes in one of its stated entries (`REQ-AUD-2`).
        let mut committed: BTreeMap<String, bool> = BTreeMap::new();
        for (place, (receipt, entry)) in entries.iter().enumerate() {
            let Entry::Verified(v) = entry else {
                if let Entry::Stated(s) = entry
                    && s.what.statement.as_str() == ADVERTISEMENT_SHAPE
                    && let Some(one) = self.installed.iter().find(|one| {
                        one.advertisement.statement().maker == s.attribution.maker().acts.as_str()
                    })
                {
                    let these = kept
                        .iter()
                        .find(|k| k.kept.commitment == s.commitment)
                        .and_then(|k| rutent_core_audit::receipts::store::bearing_out(&k.kept));
                    *committed
                        .entry(s.attribution.maker().acts.as_str().to_owned())
                        .or_default() |= these.as_deref() == Some(one.advertisement.bytes());
                    // The advertisement the log's key register accepted last for the provider is what Core lists again,
                    // as the register took it in (`T-024-21`): a re-advertisement it accepted after the installed one
                    // is listed as such, and a stated entry it refused, or one it accepted and then saw replaced, is
                    // not listed at all. Only the entry that brought the accepted one is replayed.
                    let maker = s.attribution.maker().acts.as_str().to_owned();
                    let Some(accepted) = self.log.borrow().accepted_advertisement(&maker) else {
                        continue;
                    };
                    if accepted.commitment != s.commitment {
                        continue;
                    }
                    let Some(bytes) = kept
                        .iter()
                        .find(|k| k.kept.commitment == s.commitment)
                        .and_then(|k| rutent_core_audit::receipts::store::bearing_out(&k.kept))
                    else {
                        return Err(AssemblyError::Listing(format!(
                            "the advertisement the log's register accepted for {maker} isn't kept: its bytes can't be read"
                        )));
                    };
                    let mut one = one.clone();
                    one.advertisement = rutent_conformance::role::Received::from_bytes(bytes).map_err(|e| {
                        AssemblyError::Listing(format!(
                            "the advertisement the log's register accepted for {maker} doesn't read: {e}"
                        ))
                    })?;
                    let at = rutent_core_authz::session::instant(&s.received_at);
                    self.grant_verifier(&one);
                    // The advertisement is judged by the cutovers the log held when it was received, not by those it
                    // holds now: a cutover moved earlier since doesn't void what was taken before (`REQ-PRT-4`,
                    // `T-020-54`, #143's blocker b).
                    let held = rutent_core_orch::settings::Settings::reopened(&entries[..place]);
                    held.apply_listings(&mut rutent_core_orch::settings::Targets {
                        catalogue: Some(&mut self.catalogue),
                        ..rutent_core_orch::settings::Targets::default()
                    });
                    held.apply_cutovers(&mut rutent_core_orch::settings::Targets {
                        catalogue: Some(&mut self.catalogue),
                        ..rutent_core_orch::settings::Targets::default()
                    });
                    kit::join(&mut self.catalogue, &one, at);
                }
                continue;
            };
            let What::Effect {
                action:
                    Action::ChangeCatalogue {
                        change,
                        provider,
                        tuple,
                        class,
                        ..
                    },
                ..
            } = &v.what
            else {
                continue;
            };
            if !done.contains(&receipt) {
                continue;
            }
            // An operator's setting is Core's to rebuild, from the log, apart (`Settings::reopened`, `T-020-49`).
            if *change == CatalogueChange::SetSetting {
                continue;
            }
            // The operator's confirmation of a provider's releases held after a platform's restart is the platform's to
            // hold, in its own record (`T-013-11`): Core's catalogue lists nothing for it (`T-024-21`).
            if *change == CatalogueChange::ConfirmReleases {
                continue;
            }
            // A public network listed with its reader service (`T-024-25`): until `T-020-60` puts Core on its reader
            // and the catalogue's public list on the listing, the log holds it and Core's catalogue takes nothing from
            // it.
            if change.is_about_a_public_network() {
                continue;
            }
            // A verifier's class grant has no tuple to rebuild: the category grant is made where its advertisement is
            // replayed (`grant_verifier`).
            if *change == CatalogueChange::Grant && tuple.is_none() && class.is_some() {
                continue;
            }
            // A holding account's release the operator accepted: its wallet is one of the host's accounts, which go on
            // beside the platform and are listed again first (`Core::relist_holdings`, `T-020-46`).
            if *change == CatalogueChange::AcceptModule
                && self
                    .catalogue
                    .publishers()
                    .published_by_operator(provider.as_str())
            {
                continue;
            }
            let (CatalogueChange::Grant, Some(tuple_id)) = (change, tuple) else {
                return Err(AssemblyError::Listing(format!(
                    "the log holds a {change:?} of {} that Core::relist doesn't replay",
                    provider.as_str()
                )));
            };
            let Some(one) = self
                .installed
                .iter()
                .find(|one| one.advertisement.statement().statement.provider == provider.as_str())
                .cloned()
            else {
                continue;
            };
            let statement = one.advertisement.statement();
            let Some(tuple) = rutent_prov::catalogue::tuples_of(&statement.statement)
                .into_iter()
                .find(|t| &kit::tuple_id(provider.as_str(), t) == tuple_id)
            else {
                continue;
            };
            let mut terms = kit::grant_terms(
                &one,
                decider.acts.as_str(),
                rutent_core_authz::session::instant(&v.when.at),
            );
            terms.id = format!("grant-{}-{}", provider.as_str(), u64::from(receipt.index));
            terms.tuples = vec![tuple];
            kit::grant(&mut self.catalogue, &one, terms);
        }
        // An installed advertisement the log committed to in none of its stated entries is no advertisement this Core
        // ever recorded: tampered with, or never received. The reopening is refused (`REQ-AUD-2`), whatever the register
        // accepted since; a re-advertisement the register accepted after the installed one leaves the installed one
        // committed to.
        if let Some((maker, _)) = committed.iter().find(|(_, committed)| !**committed) {
            return Err(AssemblyError::Listing(format!(
                "the advertisement installed for {maker} isn't one the log committed to: no commitment's bytes are its"
            )));
        }
        // Every cutover the log holds now stands in the catalogue and the platform, before the log's results are judged
        // again (`REQ-PRT-4`, `T-020-54`).
        rutent_core_orch::settings::Settings::reopened(&entries).apply_cutovers(
            &mut rutent_core_orch::settings::Targets {
                catalogue: Some(&mut self.catalogue),
                platform: Some(&mut self.fleet.platform),
                ..rutent_core_orch::settings::Targets::default()
            },
        );
        Ok(())
    }

    /// When the log received the stated entry at `receipt`, on the catalogue's clock.
    fn time_of_stated(&self, receipt: &Receipt) -> rutent_prov::catalogue::Instant {
        let index = usize::try_from(u64::from(receipt.index)).unwrap_or(usize::MAX);
        match self.log.borrow().entries().get(index) {
            Some((_, Entry::Stated(s))) => rutent_core_authz::session::instant(&s.received_at),
            _ => self.clock.instant(),
        }
    }

    /// The time of the verified entry at `receipt`, on the catalogue's clock.
    fn time_of(&self, receipt: &Receipt) -> rutent_prov::catalogue::Instant {
        let index = usize::try_from(u64::from(receipt.index)).unwrap_or(usize::MAX);
        match self.log.borrow().entries().get(index) {
            Some((_, Entry::Verified(v))) => rutent_core_authz::session::instant(&v.when.at),
            _ => self.clock.instant(),
        }
    }

    /// Records `installed`'s advertisement in the log, received now: its bytes kept, and its stated entry
    /// appended, claimed by its maker for its publisher.
    ///
    /// # Errors
    ///
    /// If the evidence store doesn't keep its bytes, or the log doesn't take its entry.
    pub fn advertise(&mut self, installed: &Installed) -> Result<(), AssemblyError> {
        let listing =
            |step: &str, e: AppendFailure| AssemblyError::Listing(format!("{step}: {e:?}"));
        let statement = installed.advertisement.statement();
        let publisher = &statement.statement.publisher;
        let bytes = installed.advertisement.bytes();
        let kept = rutent_contracts::core_internal::calls::Evidence::keep(&mut self.keeper, bytes)
            .map_err(|e| listing("the advertisement's bytes", e))?;
        let maker = &statement.maker;
        let entry: GeneralEntry = serde_json::from_value(json!({
            "kind": "stated",
            "attribution": {"signature": "unverified", "claimed_by": {
                "party": publisher, "acts": maker, "role": "provider", "kind": "component"
            }},
            "what": {"statement": ADVERTISEMENT_SHAPE},
            "when": {"at": statement.made_at, "source": "party_stated"},
            "received_at": self.clock.timestamp(),
            "outcome": {"status": "stated"},
            "signed_with": {"scheme": "ed25519", "key": statement.signature.key},
            "commitment": kept.commitment,
        }))
        .map_err(|e| AssemblyError::Listing(format!("the advertisement's entry: {e}")))?;
        let receipt = self
            .general
            .append(&entry)
            .map_err(|e| listing("the advertisement", e))?;
        // Into Core's catalogue once the log holds it, at the time the log received it; the deployment's defaults the join
        // sets (a floor on each tuple, the public nodes listed) give way to the operator's settings (L2 of #337's review).
        let at = self.time_of_stated(&receipt);
        kit::join(&mut self.catalogue, installed, at);
        self.reapply_catalogue_settings();
        Ok(())
    }

    /// Puts the operator's catalogue settings back after the deployment's own listing touched the catalogue: each
    /// evidence floor, and each public node listed or taken off, as `Settings` holds them (`T-020-49`).
    pub(crate) fn reapply_catalogue_settings(&mut self) {
        self.settings
            .apply(&mut rutent_core_orch::settings::Targets {
                catalogue: Some(&mut self.catalogue),
                platform: Some(&mut self.fleet.platform),
                ..rutent_core_orch::settings::Targets::default()
            });
    }

    /// A test hook (`T-020-55`, the review of `8f7ddefd`): `bytes` appended as an advertisement's stated entry, kept and
    /// claimed by the maker it names, signed (it says) with the key it names, made when it says, as [`Core::advertise`]
    /// appends one, but with nothing joined into Core's catalogue. The log takes the entry whatever the bytes hold; its
    /// key register accepts the advertisement, or not, by its own rules.
    ///
    /// # Errors
    ///
    /// If the bytes name no maker, key or time, or the log takes no entry.
    pub fn append_advertisement(&mut self, bytes: &[u8]) -> Result<Receipt, AssemblyError> {
        let listing =
            |step: &str, e: AppendFailure| AssemblyError::Listing(format!("{step}: {e:?}"));
        let value: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(|e| AssemblyError::Listing(format!("the advertisement's bytes: {e}")))?;
        let kept = rutent_contracts::core_internal::calls::Evidence::keep(&mut self.keeper, bytes)
            .map_err(|e| listing("the advertisement's bytes", e))?;
        let entry: GeneralEntry = serde_json::from_value(json!({
            "kind": "stated",
            "attribution": {"signature": "unverified", "claimed_by": {
                "party": value["statement"]["publisher"], "acts": value["maker"], "role": "provider",
                "kind": "component"
            }},
            "what": {"statement": ADVERTISEMENT_SHAPE},
            "when": {"at": value["made_at"], "source": "party_stated"},
            "received_at": self.clock.timestamp(),
            "outcome": {"status": "stated"},
            "signed_with": {"scheme": "ed25519", "key": value["signature"]["key"]},
            "commitment": kept.commitment,
        }))
        .map_err(|e| AssemblyError::Listing(format!("the advertisement's entry: {e}")))?;
        self.general
            .append(&entry)
            .map_err(|e| listing("the advertisement", e))
    }

    /// Appends a verified entry of Core's own, at its clock, on orchestration's handle.
    fn verified(&mut self, what: What, outcome: Outcome) -> Result<Receipt, AppendFailure> {
        let entry = Entry::Verified(VerifiedEntry {
            kind: VerifiedKind::Verified,
            who: kit::core(),
            what,
            when: When {
                at: self.clock.timestamp(),
                source: TimeSource::CoreClock,
            },
            outcome,
        });
        let entry = GeneralEntry::try_from(entry).map_err(AppendFailure::Refused)?;
        self.general.append(&entry)
    }

    /// Opens `holder`'s session on its account through policy, and hands it to the catalogue's register.
    ///
    /// A session policy's register holds open for the holder on the deployment's account is the one answered, a Core
    /// restarted since included, since its register is rebuilt from the log (`T-021-17`); none is asked for again. A
    /// new one is asked under an id of its own: the deployment's session id for the first, then that id with the count
    /// of sessions the register issued, so a run longer than a session's life opens another and never asks again under
    /// an id the register issued, which it refuses (`T-024-21`).
    pub fn open_session(&mut self, holder: &Actor) -> Result<Id, StepError> {
        let context = context();
        let now = self.clock.peek();
        let (open, issued) = {
            let engine = self.policy.engine().borrow();
            let sessions = engine.register().sessions();
            let open = sessions
                .iter()
                .find(|s| {
                    s.holder == holder.party
                        && s.account.as_str() == context.account
                        && s.expires_at.unix_nanos() > i128::try_from(now).unwrap_or(i128::MAX)
                })
                .map(|s| s.session.clone());
            (open, sessions.len())
        };
        if let Some(session) = open {
            return Ok(session);
        }
        let id = if issued == 0 {
            context.session.clone()
        } else {
            format!("{}-{issued}", context.session)
        };
        let until = rutent_contracts::router_api::Timestamp::new(
            rutent_prov::catalogue::Instant(
                u64::try_from(now / 1_000_000_000).unwrap_or(0) + LONGEST_SESSION_SECS - 60,
            )
            .rfc3339()
            .ok_or(StepError::NotNow("a session's end"))?,
        )
        .ok_or(StepError::NotNow("a session's end"))?;
        let request = SessionRequest {
            request: kit::id(&id),
            account: kit::id(&context.account),
            holder: holder.party.clone(),
            requester: holder.clone(),
            until,
            at: self.clock.timestamp(),
        };
        let answer = rutent_contracts::core_internal::calls::Sessions::open_session(
            &mut self.policy,
            &request,
        )?;
        match answer.outcome {
            SessionOutcome::Opened { session, .. } => {
                self.policy
                    .engine()
                    .borrow()
                    .register()
                    .open_in(&mut self.catalogue);
                // Each reserve is told the sessions the register issued for the account it holds, to that account's own
                // holder, and no other, so it answers a check in its holder's sessions only (`REQ-CAT-3`, `T-024-21`):
                // the world holds them where each simulated reserve reads them.
                let account = context.account.clone();
                let holder = self
                    .deployed
                    .registers
                    .borrow()
                    .accounts
                    .get(&account)
                    .cloned();
                let issued: Vec<String> = self
                    .policy
                    .engine()
                    .borrow()
                    .register()
                    .sessions()
                    .iter()
                    .filter(|s| {
                        s.account.as_str() == account
                            && holder.as_deref() == Some(s.holder.as_str())
                    })
                    .map(|s| s.session.as_str().to_owned())
                    .collect();
                let issued = serde_json::to_string(&issued).unwrap_or_default();
                let reserves: Vec<String> = self
                    .installed
                    .iter()
                    .filter(|one| one.spec.role == rutent_conformance::sim::Role::Reserve)
                    .map(|one| one.spec.provider.clone())
                    .collect();
                for provider in reserves {
                    self.fleet.platform.world.seed_state(
                        &provider,
                        rutent_conformance::sim::SESSIONS_KEY,
                        &issued,
                    );
                }
                Ok(session)
            }
            other => Err(StepError::Session(other)),
        }
    }

    /// Runs `call` on intake with the desk it reaches.
    fn with_desk<T>(&mut self, call: impl FnOnce(&mut Intake, &mut Desk<'_, '_>) -> T) -> T {
        let core = kit::core();
        let operator = kit::operator();
        let mut asking = Asking {
            fleet: &mut self.fleet,
            clock: &self.clock,
        };
        let mut charges = self.policy.clone();
        let mut persons = self.policy.clone();
        let mut parts = Rounds {
            catalogue: &mut self.catalogue,
            clock: &self.clock,
            exposures: &self.exposures,
            releases: &self.releases,
            log: &mut self.general,
            evidence: &mut self.keeper,
            charges: &mut charges,
            persons: &mut persons,
            providers: &mut asking,
            operator_key: &mut self.signer,
            fresh: &mut self.fresh,
            issued: &mut self.issued,
        };
        let mut owners = Owners(Rc::clone(&self.deployed.registers));
        let mut acting = Acting(Rc::clone(&self.deployed.registers));
        let mut held = Held(Rc::clone(self.policy.engine()));
        let mut authorisations = self.policy.clone();
        let mut decisions = self.policy.clone();
        let mut reads = StatusReading {
            policy: self.policy.clone(),
            orders: self.sources.opened_orders(),
        };
        let log = Rc::clone(&self.log);
        let recorded = LogNow(&log);
        let mut kept = Reading {
            kept: &mut self.kept,
            through: self.kept_through.as_deref_mut(),
        };
        let mut desk = Desk {
            parts: &mut parts,
            destinations: &mut owners,
            authorisations: &mut authorisations,
            decisions: &mut decisions,
            actors: &mut acting,
            sessions: &mut held,
            reads: &mut reads,
            bounds: &self.bounds,
            prices: &self.prices,
            core: &core,
            operator: &operator,
            holds: &mut self.holds,
            kept: &mut kept,
            recorded: &recorded,
            terms: &mut self.segments,
        };
        call(&mut self.intake, &mut desk)
    }

    /// Takes `request`. Policy's sources read the order it opens from the log's `order_opened` (`T-020-55`).
    pub fn request(&mut self, request: OrderRequest) -> Result<Told, StepError> {
        let told = self.with_desk(|intake, desk| intake.open(desk, request.clone()))?;
        if let Told::Exists { order, .. } = &told {
            self.requests.insert(order.as_str().to_owned(), request);
        }
        self.feed()?;
        Ok(told)
    }

    /// The customer `caller` quotes `order`.
    pub fn quote(&mut self, caller: &Actor, order: &Id) -> Result<Told, StepError> {
        let told =
            self.with_desk(|intake, desk| intake.quote::<RouteFinder>(desk, caller, order))?;
        if let Told::Exists {
            status: IntakeStatus::Quoted { quote, .. },
            ..
        } = &told
        {
            self.quotes
                .insert(order.as_str().to_owned(), (**quote).clone());
        }
        self.feed()?;
        Ok(told)
    }

    /// `by` accepts the quote of `order` shown with `digest`; once the order starts, its segments follow it.
    pub fn accept(
        &mut self,
        by: &Actor,
        order: &Id,
        digest: &rutent_contracts::module_interface::Digest,
    ) -> Result<Told, StepError> {
        self.accept_holding(by, order, digest, None)
    }

    /// The quotes reserves hold: where a scenario keeps a reserve's signed hold (`Holds::hold`) for an acceptance
    /// to name.
    pub fn holds(&mut self) -> &mut rutent_core_orch::quote::Holds {
        &mut self.holds
    }

    /// From now on, the evidence store's read answers what `through` makes of what it kept under each commitment,
    /// for every statement Core holds to what the store keeps (a provider's terms, a reserve's hold): how a scenario
    /// shows Core keeping nothing that rests on other bytes than its entry names (`REQ-AUD-2`). `None` reads it as kept
    /// again.
    pub fn read_kept_through(&mut self, through: Option<Box<KeptThrough>>) {
        self.kept_through = through;
    }

    /// The holds Core took of the quotes it showed, by the transaction of each hold's stated entry, in the log's order
    /// (`T-020-56`).
    pub fn holds_taken(&self) -> Vec<(Id, Receipt)> {
        self.log
            .borrow()
            .entries()
            .iter()
            .filter_map(|(receipt, entry)| match entry {
                Entry::Stated(stated)
                    if stated.what.statement.as_str()
                        == rutent_contracts::provider_role::HOLD_SHAPE =>
                {
                    Some((stated.about.as_ref()?.transaction.clone()?, receipt.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// [`Core::accept`], naming `hold`, the transaction of a reserve's hold of the quote shown (`T-020-15`).
    pub fn accept_holding(
        &mut self,
        by: &Actor,
        order: &Id,
        digest: &rutent_contracts::module_interface::Digest,
        hold: Option<&Id>,
    ) -> Result<Told, StepError> {
        let told = self
            .with_desk(|intake, desk| intake.accept_holding(desk, order, by, digest, None, hold))?;
        if let Some(start) = self.intake.start(order).cloned()
            && !self.started.contains_key(order.as_str())
        {
            self.follow(order, &start)?;
        }
        self.feed()?;
        Ok(told)
    }

    /// Follows `order` from its start as intake answered it.
    fn follow(&mut self, order: &Id, start: &RecordedInput) -> Result<(), StepError> {
        let Observed::RouteChosen {
            decision_record,
            parts,
            ..
        } = &start.observed
        else {
            return Err(StepError::NotNow("an order starts on its route chosen"));
        };
        let record = self.record_of(order, &kit::id(decision_record.as_str()))?;
        let entries: Vec<What> = self.verified_whats();
        let authorisation = entries
            .iter()
            .find_map(|w| match w {
                What::Acceptance {
                    order: o,
                    authorisation,
                    ..
                } if o == order => Some(authorisation.clone()),
                _ => None,
            })
            .ok_or(StepError::NotNow("an order starts on its acceptance"))?;
        let logged: Vec<Entry> = self
            .log
            .borrow()
            .entries()
            .iter()
            .map(|(_, e)| e.clone())
            .collect();
        let authorised = authorised(&logged, order, &authorisation).ok_or(StepError::NotNow(
            "an acceptance rests on its authorisation",
        ))?;
        let quote = self
            .quotes
            .get(order.as_str())
            .ok_or(StepError::NotNow("an order starts on the quote shown"))?;
        let request = self
            .requests
            .get(order.as_str())
            .ok_or(StepError::NotNow("an order starts on its request"))?;
        let mut inputs = request.inputs.clone();
        inputs.insert(
            DESTINATION_INPUT.to_owned(),
            request.destination.account.as_str().to_owned(),
        );
        let begun = Begun {
            origin: request.origin_reserve.clone(),
            session: Some(request.session.clone()),
            inputs,
            delivers: quote.delivers.clone(),
        };
        self.follow_on(
            order,
            start,
            record,
            parts.clone(),
            authorisation,
            authorised,
            begun,
        )
    }

    /// Follows `order` from `start`, its route `record` and `plan`, under `authorisation` as the log holds it
    /// (`authorised`), with what its intake or its filing gave (`begun`).
    #[allow(clippy::too_many_arguments)]
    fn follow_on(
        &mut self,
        order: &Id,
        start: &RecordedInput,
        record: DecisionRecord,
        plan: Vec<PartPlan>,
        authorisation: Id,
        authorised: rutent_contracts::core_internal::Authorisation,
        begun: Begun,
    ) -> Result<(), StepError> {
        let started = Started {
            record,
            plan,
            authorisation,
            origin: begun.origin,
            route: RouteEnd {
                destination: authorised.destination.clone(),
                to_asset: authorised.to_asset.clone(),
                delivers: begun.delivers,
            },
            session: begun.session,
            inputs: begun.inputs,
            floor: authorised.floor.clone(),
        };
        if !self.segments.follow(&self.clock, start) {
            return Err(StepError::NotNow("the state machine opens the order"));
        }
        self.started.insert(order.as_str().to_owned(), started);
        Ok(())
    }

    /// The decision record `id` of `order`, as Core signed it, held to the digest the log names it by.
    fn record_of(&self, order: &Id, id: &Id) -> Result<DecisionRecord, StepError> {
        let recorded = self
            .signer
            .records
            .get(id.as_str())
            .ok_or(StepError::NotNow("Core signed the decision record"))?;
        let named = self
            .verified_whats()
            .into_iter()
            .find_map(|w| match w {
                What::DecisionRecord {
                    decision_record,
                    order: o,
                    record,
                    ..
                } if &decision_record == id && &o == order => Some(record),
                _ => None,
            })
            .ok_or(StepError::NotNow("the log names the decision record"))?;
        let canonical = rutent_contracts::provider_role::to_canonical(
            &serde_json::from_slice::<serde_json::Value>(recorded)
                .map_err(|_| StepError::NotNow("a decision record is JSON"))?,
        )
        .map_err(|_| StepError::NotNow("a decision record has a canonical form"))?;
        if named.value.as_str() != rutent_prov::module::wire::sha256(&canonical) {
            return Err(StepError::NotNow(
                "the decision record is the one the log names by digest",
            ));
        }
        // Read only from the bytes the digest holds.
        record_in(recorded).ok_or(StepError::NotNow("a decision record in its envelope"))
    }

    /// The receipt of the log's `decision_record` entry for `record`, which holds the answers the route was
    /// evaluated with (`T-020-31`).
    pub(crate) fn record_entry_of(&self, record: &DecisionRecord) -> Option<Receipt> {
        self.log
            .borrow()
            .entries()
            .iter()
            .find_map(|(receipt, e)| match e {
                Entry::Verified(v) => match &v.what {
                    What::DecisionRecord {
                        decision_record, ..
                    } if decision_record == record.record_id() => Some(receipt.clone()),
                    _ => None,
                },
                Entry::Stated(_) => None,
            })
    }

    /// Runs every part of `order`'s route, each segment after the one before settles.
    pub fn run(&mut self, order: &Id) -> Result<Vec<Ran>, StepError> {
        let started = self
            .started
            .get(order.as_str())
            .cloned()
            .ok_or(StepError::NotNow("the order started"))?;
        let core = kit::core();
        let operator = kit::operator();
        let record_entry = self.record_entry_of(&started.record);
        let mut ran = Vec::new();
        for (at, part) in started.plan.iter().enumerate() {
            let segments: Vec<Id> = part
                .segments
                .iter()
                .map(|s| kit::id(s.segment_id.as_str()))
                .collect();
            let part_id = kit::id(part.part_id.as_str());
            let live = Live {
                order,
                part: &part_id,
                segments: &segments,
                record: &started.record,
                at,
                authorisation: &started.authorisation,
                origin_reserve: &started.origin,
                route: &started.route,
                caller: &core,
                core: &core,
                operator: &operator,
                session: started.session.as_ref(),
                inputs: &started.inputs,
                floor: started.floor.as_ref(),
                record_entry: record_entry.as_ref(),
            };
            ran.push(self.run_live(&live)?);
        }
        Ok(ran)
    }

    /// Declares the part of `order` stuck for want of a route (`REQ-ORD-28`): its recalculations at their bound, the
    /// value left resting where the last segment that settled put it. Core's record of the order's ends is taken
    /// from the log's own entries, as the assembly follows an order (`T-020-11`, `T-020-16`).
    ///
    /// # Errors
    ///
    /// If the order hasn't started, or the part can't be declared.
    pub fn leave_no_route(&mut self, order: &Id) -> Result<(), StepError> {
        let started = self
            .started
            .get(order.as_str())
            .cloned()
            .ok_or(StepError::NotNow("the order started"))?;
        let part = kit::id(
            started
                .plan
                .first()
                .ok_or(StepError::NotNow("the order has a part"))?
                .part_id
                .as_str(),
        );
        let core = kit::core();
        let outcome = rutent_core_orch::recalc::Recalculation::NoRouteLeft { recalculated: 0 };
        let last = started.record.record_id().clone();
        let mut parts = StuckParts {
            clock: &self.clock,
            log: &mut self.general,
            fresh: &mut self.fresh,
            issued: &self.issued,
            segments: &mut self.segments,
            exposures: &mut self.exposures,
        };
        self.stuck.no_route(
            &mut parts,
            NoRouteLeft {
                order,
                part: &part,
                outcome: &outcome,
                last_record: &last,
                core: &core,
            },
        )?;
        self.feed()?;
        let entries: Vec<Entry> = self
            .log
            .borrow()
            .entries()
            .iter()
            .map(|(_, e)| e.clone())
            .collect();
        self.stuck.follow(&self.segments, &entries, order)?;
        Ok(())
    }

    /// The decide step of `order`'s stuck part, asked of the engine through the assembly's policy
    /// (`REQ-AUZ-4`).
    ///
    /// # Errors
    ///
    /// If Core refuses it before policy is asked, or the log can't take it.
    pub fn decide_stuck(
        &mut self,
        order: &Id,
        decider: &Actor,
        resolution: &rutent_contracts::core_internal::Resolution,
        evidence: &[rutent_contracts::module_interface::Digest],
    ) -> Result<rutent_contracts::core_internal::Decision, StepError> {
        let started = self
            .started
            .get(order.as_str())
            .cloned()
            .ok_or(StepError::NotNow("the order started"))?;
        let part = kit::id(
            started
                .plan
                .first()
                .ok_or(StepError::NotNow("the order has a part"))?
                .part_id
                .as_str(),
        );
        let mut policy = self.policy.clone();
        let mut parts = StuckParts {
            clock: &self.clock,
            log: &mut self.general,
            fresh: &mut self.fresh,
            issued: &self.issued,
            segments: &mut self.segments,
            exposures: &mut self.exposures,
        };
        let history: Vec<_> = self.log.borrow().entries().to_vec();
        Ok(self.stuck.decide(
            &mut parts,
            &mut policy,
            Deciding {
                order,
                part: &part,
                decider,
                resolution,
                evidence,
            },
            &history,
        )?)
    }

    /// The move step of `order`'s stuck part, carrying out `carries_out`: policy decides it through the assembly's
    /// engine, and the production mover hands the platform the effect it records (`REQ-AUZ-4`, `T-020-16`).
    ///
    /// # Errors
    ///
    /// If Core refuses it before policy is asked, or the log can't take a step of it.
    pub fn move_stuck(
        &mut self,
        order: &Id,
        carries_out: &rutent_contracts::core_internal::Decided,
        mover: &Actor,
    ) -> Result<Moved, StepError> {
        let core = kit::core();
        let operator = kit::operator();
        let mut policy = self.policy.clone();
        let resting = self.segments.resting_totals();
        let readings = self.readings.clone();
        readings.refresh(&self.fleet.platform.world);
        let running = RefCell::new(Running {
            platform: &mut self.fleet.platform,
            clock: &self.clock,
            faults: &mut self.faults,
            feed: &mut self.feed,
            log: &self.log,
            swap_to: self.swap_to.as_deref_mut(),
            readings: &readings,
        });
        let mut parts = StuckParts {
            clock: &self.clock,
            log: &mut self.general,
            fresh: &mut self.fresh,
            issued: &self.issued,
            segments: &mut self.segments,
            exposures: &mut self.exposures,
        };
        let mut no_proxy = NoProxy;
        // The hand-on's reach (`T-020-26`): the operator's holding accounts, run on the platform the move's other
        // calls go to; the linked orders' accepted routes; and the log's own handle, read as it stands each time.
        let mut modules = holding::Shared(&running);
        let mut wallets = holding::HeldWallets {
            holdings: &self.holdings,
            running: &running,
            bounds: &self.bounds,
            resting,
        };
        let accepted = holding::Accepted(&self.linked);
        let log = holding::LogHandle(Rc::clone(&self.log));
        let mut delivering = self
            .mover
            .on(Ports {
                modules: &mut modules,
                uses: &mut no_proxy,
                evidence: &mut self.keeper,
                catalogue: &self.catalogue,
                operator_key: &mut self.signer,
                bounds: &self.bounds,
                operator: &operator,
            })
            .handing_on(rutent_core_orch::stuck::HandingOn {
                withdrawals: &mut wallets,
                routes: &accepted,
                log: &log,
            });
        let moved = self.stuck.carry_out(
            &mut parts,
            &mut policy,
            &mut delivering,
            Moving {
                order,
                carries_out,
                mover,
                core: &core,
            },
        );
        self.feed()?;
        Ok(moved?)
    }

    /// Declares that `order`'s input `input` rests on `judgement`, before the segment that carries it is handed over,
    /// so Core writes the `reliance` entry before it sends (`T-020-23`).
    ///
    /// # Errors
    ///
    /// As [`Segments::rests_on`](rutent_core_orch::segments::Segments::rests_on).
    pub fn rests_on(
        &mut self,
        order: &Id,
        input: &str,
        judgement: rutent_core_orch::reliance::Judgement,
    ) -> Result<(), rutent_core_orch::reliance::Rests> {
        self.segments.rests_on(order, input, judgement)
    }

    /// The judgement the log holds of `verifier`: the receipt of its stated entry of the judgement's shape, which
    /// the routing doesn't hand back (`T-020-23`, gap 1), and its maker. The latest, if it made several.
    pub fn judgement_of(&self, verifier: &str) -> Option<rutent_core_orch::reliance::Judgement> {
        self.log
            .borrow()
            .entries()
            .iter()
            .rev()
            .find_map(|(receipt, entry)| match entry {
                Entry::Stated(s)
                    if s.what.statement.as_str()
                        == rutent_contracts::provider_role::JUDGEMENT_SHAPE
                        && s.attribution.maker().acts.as_str() == verifier =>
                {
                    Some(rutent_core_orch::reliance::Judgement {
                        receipt: receipt.clone(),
                        judge: kit::id(verifier),
                    })
                }
                _ => None,
            })
    }

    /// The first part of a started `order`'s plan.
    pub fn first_part(&self, order: &Id) -> Option<Id> {
        self.started
            .get(order.as_str())
            .and_then(|s| s.plan.first())
            .map(|p| kit::id(p.part_id.as_str()))
    }

    /// Whether Core holds the stuck part of `order` unresolved (`T-020-11`).
    pub fn is_stuck(&self, order: &Id, part: &Id) -> bool {
        self.stuck.is_stuck(order, part)
    }

    /// The platform's count of operations handed to `provider`.
    pub fn operations_received(&self, provider: &str) -> usize {
        self.fleet.platform.operations_received(provider)
    }

    /// Runs one part as `live` says, each segment after the one before settles.
    pub(crate) fn run_live(&mut self, live: &Live<'_>) -> Result<Ran, StepError> {
        let readings = self.readings.clone();
        readings.refresh(&self.fleet.platform.world);
        let mut running = Running {
            platform: &mut self.fleet.platform,
            clock: &self.clock,
            faults: &mut self.faults,
            feed: &mut self.feed,
            log: &self.log,
            swap_to: self.swap_to.as_deref_mut(),
            readings: &readings,
        };
        let mut grading = rutent_core_orch::closing::Evidenced {
            catalogue: &self.catalogue,
            records: &readings,
            books: &readings,
            closing: &mut self.closing,
        };
        let mut decisions = self.policy.clone();
        let mut channel = rutent_core_orch::notice::Channel {
            notices: &mut self.notices,
            key: &mut self.notice_signer,
            transport: &mut self.wire,
        };
        let mut parts = SegmentParts {
            clock: &self.clock,
            catalogue: &self.catalogue,
            log: &mut self.general,
            evidence: &mut self.keeper,
            decisions: &mut decisions,
            modules: &mut running,
            uses: &mut NoProxy,
            grading: &mut grading,
            operator_key: &mut self.signer,
            fresh: &mut self.fresh,
            issued: &self.issued,
            exposures: &mut self.exposures,
            channel: &mut channel,
        };
        let result = self.segments.run_part(&mut parts, live);
        // What settled at a public node a holding account holds is landed in it by Core's schedule, on its next tick
        // (`T-020-33`, `T-020-48`): a run lands nothing itself.
        // Core's closing is its schedule's (`REQ-QTE-7`, `T-020-48`): a run closes nothing.
        self.feed()?;
        // A segment the run left unknown is asked where it stands, once, at a provider that declares a status query
        // (`REQ-ORD-1`, `T-020-47`).
        self.ask_unknown_status(live);
        self.feed()?;
        Ok(result?)
    }

    /// `reader`'s read of `order`'s state and age, checked by policy.
    pub fn status(&mut self, reader: &Actor, order: &Id) -> Result<Status, StepError> {
        let mut reads = self.policy.clone();
        self.segments
            .status(
                &mut reads,
                &self.clock,
                &mut self.fresh,
                reader,
                None,
                order,
            )
            .map_err(StepError::Status)
    }

    /// Every verified entry's `what`, oldest first.
    pub fn verified_whats(&self) -> Vec<What> {
        self.log
            .borrow()
            .entries()
            .iter()
            .filter_map(|(_, e)| match e {
                Entry::Verified(v) => Some(v.what.clone()),
                Entry::Stated(_) => None,
            })
            .collect()
    }

    /// Every input the log took, in its order.
    pub fn inputs(&self) -> Vec<RecordedInput> {
        inputs_of(&self.log.borrow())
    }

    /// Feeds the state machine what the log took since it was last fed, unless its delivery holds it back.
    pub fn feed(&mut self) -> Result<(), StepError> {
        let inputs = self.inputs();
        self.feed.feed(&inputs)?;
        Ok(())
    }

    /// Feeds the state machine everything the log took that it hasn't had.
    pub fn flush(&mut self) -> Result<(), StepError> {
        let inputs = self.inputs();
        self.feed.flush(&inputs)?;
        Ok(())
    }

    /// The order a told answer names.
    pub fn order_of(told: &Told) -> Option<Id> {
        match told {
            Told::Exists { order, .. } => Some(order.clone()),
            Told::Refused { .. } => None,
        }
    }

    /// Intake's record of `order`.
    pub fn record(&self, order: &Id) -> Option<OrderRecord> {
        self.intake.record(order)
    }

    /// The operator's actor of the deployment, and its login: who the assembly lists, apart from the customer.
    pub fn actors() -> [Actor; 4] {
        [
            kit::customer(),
            kit::core(),
            kit::operator(),
            kit::operator_login(),
        ]
    }

    /// Sets the deployment's policy to refuse every call with `class`, or to permit what the fixed rules let
    /// through.
    pub fn refuse(&self, class: Option<RefusalClass>) {
        self.deployment.refuse.set(class);
    }
}

/// The decision record a signed envelope carries.
fn record_in(recorded: &[u8]) -> Option<DecisionRecord> {
    let value: serde_json::Value = serde_json::from_slice(recorded).ok()?;
    serde_json::from_value(value["statement"].clone()).ok()
}

/// The terms the authorisation `authorisation` of `order` acts under, as the policy engine's register reads the log
/// (`T-021-04`): an `authorise` of that authorisation and that order whose outcome is permitted, as
/// `move-rests-on-its-authorisation` holds a move to, and in force, not replaced since. A refusal under the same id,
/// the same id for another order, or one replaced, is no authorisation this order starts under. An order starts as a
/// whole, before any part's own authorisation, so it is read for the order as a whole. The engine holds each of the
/// order's moves to the same view, for the move's own part.
fn authorised(entries: &[Entry], order: &Id, authorisation: &Id) -> Option<Authorisation> {
    // The order's customer, as its acceptance names it, is the one whose new terms replace those in force.
    let requester = rutent_core_authz::funds::parties_in(entries, order).map(|p| p.requester);
    rutent_core_authz::funds::Authorised::of(rutent_core_authz::funds::permitted_in(entries))
        .in_force_as(order, None, requester.as_ref(), authorisation)
        .cloned()
}

/// Every input `log` took, in its order.
pub fn inputs_of(log: &CoreLog) -> Vec<RecordedInput> {
    log.entries()
        .iter()
        .filter_map(|(_, e)| match e {
            Entry::Verified(v) => match &v.what {
                What::Input { input } => Some(input.clone()),
                _ => None,
            },
            Entry::Stated(_) => None,
        })
        .collect()
}

/// The account the base order delivers to, held by the customer at the destination reserve.
pub const DESTINATION_ACCOUNT: &str = "acct-2";

/// The payer's account at the origin reserve, held by the customer, the base order's payer: the one its inputs name
/// (`payer.bank_account`) and its authorisation names as its origin account.
pub const ORIGIN_ACCOUNT: &str = "acct-1";

/// An actor that is not a person: a component of `party`'s, acting as `acts`, in `role`.
pub fn component(party: &str, acts: &str, role: Role) -> Actor {
    Actor {
        party: kit::id(party),
        acts: kit::id(acts),
        role,
        kind: ActorKind::Component,
    }
}

#[cfg(test)]
mod tests {
    use rutent_core_orch::intake::{Status, Told};

    use super::*;
    use crate::harness::{Given, Scenario, base_request};
    use crate::kit::AMOUNT;

    // verifies: DEC-18
    #[test]
    fn route_whose_signed_record_differs_from_the_logs_digest_is_not_run() {
        let (mut core, _) =
            Scenario::new("Core alone", "AC-28", Given::one_reserve(Staged::AtOnce))
                .core(1, &mut machine::Reference);
        let customer = kit::customer();
        let session = core.open_session(&customer).unwrap();
        let told = core
            .request(base_request("req-1", AMOUNT, &session))
            .unwrap();
        let order = Core::order_of(&told).unwrap();
        let Told::Exists {
            status: Status::Quoted { quote, digest, .. },
            ..
        } = core.quote(&customer, &order).unwrap()
        else {
            panic!("no quote")
        };
        // The record Core kept, changed after the log took its digest.
        let kept = core
            .signer
            .records
            .get_mut(quote.decision_record.as_str())
            .expect("the chosen record is kept");
        let mut value: serde_json::Value = serde_json::from_slice(kept).unwrap();
        value["made_at"] = "2026-11-09T02:00:00Z".into();
        *kept = serde_json::to_vec(&value).unwrap();
        assert!(matches!(
            core.accept(&customer, &order, &digest),
            Err(StepError::NotNow(
                "the decision record is the one the log names by digest"
            ))
        ));
        assert!(!core.started.contains_key(order.as_str()));
    }

    /// Core alone, put together, and the config it was put together under.
    fn alone() -> (Core, Config) {
        let scenario = Scenario::new("Core alone", "AC-28", Given::one_reserve(Staged::AtOnce));
        let (core, _) = scenario.core(1, &mut machine::Reference);
        (core, scenario.config(1))
    }

    /// A Core opened again rebuilds its catalogue from the grants and advertisements its log holds, and from nothing
    /// else: a catalogue change it doesn't replay, here a promotion, refuses the reopening rather than leave the
    /// catalogue short of it (a revoked tuple granted again, a release not in force).
    // verifies: REQ-MDL-4, REQ-AUZ-6
    #[test]
    fn reopened_core_refuses_a_catalogue_change_it_does_not_replay() {
        let (mut core, config) = alone();
        let installed = core.installed[0].clone();
        let provider = kit::id(&installed.advertisement.statement().statement.provider);
        let action = Action::ChangeCatalogue {
            change: CatalogueChange::Promote,
            provider: provider.clone(),
            version: Some(kit::id("1.0.0")),
            tuple: None,
            class: None,
            advertising_keys: Vec::new(),
            statement_key: None,
            setting: None,
            versions: Vec::new(),
            public_network: None,
        };
        let request = DecisionRequest {
            request: core.fresh.id(),
            caller: kit::operator_login(),
            action: action.clone(),
            at: core.clock.timestamp(),
        };
        let decision =
            rutent_contracts::core_internal::calls::Decisions::decide(&mut core.policy, &request)
                .unwrap();
        let permit = decision.permit().expect("the operator's login may promote");
        let effect = core
            .verified(
                What::Effect {
                    action,
                    decision: permit,
                },
                Outcome::ToTakeEffect {},
            )
            .unwrap();
        core.verified(What::Result { effect }, Outcome::Done {})
            .unwrap();
        let reopened = core.reopened(&config, &mut machine::Reference);
        assert!(
            matches!(&reopened, Err(AssemblyError::Listing(why)) if why.contains("doesn't replay")),
            "{:?}",
            reopened.err()
        );
    }

    /// A Core opened again joins only an advertisement its log committed to (`REQ-AUD-2`, `T-024-21`): the one the
    /// log's key register accepted, read from the bytes the evidence store keeps under that entry's commitment, never
    /// the installed bytes. Opened with the installed advertisement intact, the catalogue holds the committed statement;
    /// the same statement in other bytes, one the log committed to in no entry, refuses the reopening.
    // verifies: REQ-AUD-2, REQ-CAT-6
    #[test]
    fn reopened_core_joins_only_the_advertisement_its_log_committed_to() {
        let (mut core, config) = alone();
        let provider = core.installed[0].spec.provider.clone();
        let committed = core
            .log()
            .accepted_advertisement(&provider)
            .expect("the register accepted the advertisement")
            .commitment;
        let kept = core
            .kept_bytes(&committed)
            .expect("the evidence store keeps what the log committed to");
        let reopened = core
            .reopened(&config, &mut machine::Reference)
            .expect("reopened on the advertisement the log committed to");
        let joined = reopened
            .catalogue
            .accepted_advertisement(&provider)
            .expect("joined again")
            .clone();
        let kept: rutent_contracts::provider_role::Statement<
            rutent_contracts::provider_role::Advertisement,
        > = serde_json::from_slice(&kept).expect("an advertisement");
        assert_eq!(
            joined, kept,
            "the catalogue holds the statement the log committed to"
        );

        let (mut core, config) = alone();
        let mut bytes = core.installed[0].advertisement.bytes().to_vec();
        bytes.push(b' ');
        core.installed[0].advertisement =
            rutent_conformance::role::Received::from_bytes(bytes).expect("the same statement");
        let reopened = core.reopened(&config, &mut machine::Reference);
        assert!(
            matches!(&reopened, Err(AssemblyError::Listing(why)) if why.contains("committed to")),
            "{:?}",
            reopened.err()
        );
    }

    /// An order of 1 000 000 whose customer takes no less than 995 000, accepted, and its withdrawal scripted to
    /// settle 1 % short.
    fn floored() -> (Core, Id) {
        let (mut core, _) =
            Scenario::new("Core alone", "AC-42", Given::one_reserve(Staged::AtOnce))
                .core(1, &mut machine::Reference);
        let customer = kit::customer();
        let session = core.open_session(&customer).unwrap();
        let mut request = base_request("req-floor", AMOUNT, &session);
        request.route.floor = Some(rutent_contracts::router_api::Amount::new("995000").unwrap());
        let order = Core::order_of(&core.request(request).unwrap()).unwrap();
        let Told::Exists {
            status: Status::Quoted { digest, .. },
            ..
        } = core.quote(&customer, &order).unwrap()
        else {
            panic!("no quote")
        };
        core.accept(&customer, &order, &digest).unwrap();
        core.fleet.world().script_for(
            kit::RESERVE,
            "withdrawal",
            rutent_conformance::sim::Behaviour::delivering(
                rutent_conformance::sim::Delivery::Short { bps: 100 },
            ),
        );
        (core, order)
    }

    // verifies: DEC-18
    #[test]
    fn floor_the_authorisation_names_holds_the_part_at_the_segment_that_crosses_it() {
        let (mut core, order) = floored();
        core.run(&order).unwrap();
        let moves = core
            .verified_whats()
            .into_iter()
            .filter(|w| {
                matches!(
                    w,
                    What::Effect {
                        action: Action::MoveValue { .. },
                        ..
                    }
                )
            })
            .count();
        assert_eq!(moves, 1, "the deposit is not handed over");
        let started = &core.started[order.as_str()];
        let part = kit::id(started.plan[0].part_id.as_str());
        let since = core
            .segments
            .waiting(&order, &part)
            .cloned()
            .expect("the part waits at its floor");
        // The wait is a recorded state-machine input, the part waiting since it was recorded (`T-020-10b`).
        let waits = core
            .verified_whats()
            .into_iter()
            .filter_map(|w| match w {
                What::Input { input } if input.observed == Observed::WaitingAtFloor {} => {
                    Some(input.recorded_at.clone())
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(waits, vec![since]);
    }

    // verifies: DEC-18
    #[test]
    fn order_runs_under_its_permitted_authorisation_not_an_earlier_refusal_of_its_id() {
        let (core, order) = floored();
        let acceptance = core
            .verified_whats()
            .into_iter()
            .find_map(|w| match w {
                What::Acceptance {
                    order: o,
                    authorisation,
                    ..
                } if o == order => Some(authorisation),
                _ => None,
            })
            .unwrap();
        let mut entries: Vec<Entry> = core
            .log()
            .entries()
            .iter()
            .map(|(_, e)| e.clone())
            .collect();
        let permitted = entries
            .iter()
            .find_map(|e| match e {
                Entry::Verified(v) if matches!(&v.what, What::Authorise { authorisation: a, .. } if a.authorisation == acceptance) => Some(v.clone()),
                _ => None,
            })
            .unwrap();
        // The same id refused earlier, for another destination, and the same id permitted for another order.
        let mut refused = permitted.clone();
        refused.outcome = Outcome::Refused {
            class: RefusalClass::NotPermitted,
        };
        let mut other = permitted.clone();
        if let What::Authorise { authorisation, .. } = &mut refused.what {
            authorisation.destination.account = kit::id("acct-elsewhere");
        }
        if let What::Authorise { authorisation, .. } = &mut other.what {
            authorisation.order = kit::id("another-order");
            authorisation.destination.account = kit::id("acct-of-another");
        }
        entries.insert(0, Entry::Verified(refused));
        entries.insert(0, Entry::Verified(other));
        let found = authorised(&entries, &order, &acceptance).expect("the permitted authorisation");
        let What::Authorise {
            authorisation: want,
            ..
        } = permitted.what
        else {
            unreachable!()
        };
        assert_eq!(found, want);
    }

    /// The assembly starts an order under the authorisation its acceptance names only while the policy engine's
    /// register holds it in force (`T-021-04`): once the customer's new authorisation of other terms replaces it, the
    /// order starts under it no more, and the new one is found; another's authorisation of other terms, which only
    /// engines racing over one log could leave, replaces nothing.
    // verifies: REQ-AUZ-3, INV-10
    #[test]
    fn order_starts_under_no_authorisation_the_customer_replaced() {
        let (core, order) = floored();
        let acceptance = core
            .verified_whats()
            .into_iter()
            .find_map(|w| match w {
                What::Acceptance {
                    order: o,
                    authorisation,
                    ..
                } if o == order => Some(authorisation),
                _ => None,
            })
            .unwrap();
        let entries: Vec<Entry> = core
            .log()
            .entries()
            .iter()
            .map(|(_, e)| e.clone())
            .collect();
        let permitted = entries
            .iter()
            .find_map(|e| match e {
                Entry::Verified(v) if matches!(&v.what, What::Authorise { authorisation: a, .. } if a.authorisation == acceptance) => Some(v.clone()),
                _ => None,
            })
            .unwrap();
        let anew = |id: &str, by: Actor| {
            let mut entry = permitted.clone();
            if let What::Authorise { authorisation, .. } = &mut entry.what {
                authorisation.authorisation = kit::id(id);
                authorisation.destination.account = kit::id("acct-anew");
                authorisation.given_by = by;
            }
            Entry::Verified(entry)
        };
        let operator = Actor {
            party: kit::id("operator"),
            acts: kit::id("op-login-2"),
            role: rutent_contracts::core_internal::Role::Operator,
            kind: rutent_contracts::core_internal::ActorKind::Person,
        };
        let mut raced = entries.clone();
        raced.push(anew("auth-raced", operator));
        assert!(
            authorised(&raced, &order, &acceptance).is_some(),
            "another's authorisation replaces nothing"
        );
        assert!(authorised(&raced, &order, &kit::id("auth-raced")).is_none());

        let mut replaced = entries;
        replaced.push(anew("auth-anew", kit::customer()));
        assert!(authorised(&replaced, &order, &acceptance).is_none());
        let found = authorised(&replaced, &order, &kit::id("auth-anew")).expect("the new one");
        assert_eq!(found.destination.account.as_str(), "acct-anew");
    }
    /// The assembly's authorisations are the policy engine's (`T-021-04`), held to the register it reads from the
    /// log: the customer giving the order's authorisation again under its id, with other terms, is refused, as no
    /// answer beside the engine would refuse it, and the log records the refusal as the engine's.
    // verifies: REQ-AUZ-3
    #[test]
    fn authorisation_given_again_with_other_terms_is_refused_by_the_engine() {
        let (mut core, order) = floored();
        let given = core
            .log()
            .entries()
            .iter()
            .find_map(|(_, e)| match e {
                Entry::Verified(v) => match &v.what {
                    What::Authorise { authorisation, .. }
                        if authorisation.order == order && v.outcome == Outcome::Permitted {} =>
                    {
                        Some(authorisation.clone())
                    }
                    _ => None,
                },
                Entry::Stated(_) => None,
            })
            .expect("the order's authorisation");
        let mut again = given.clone();
        again.destination.account = kit::id("acct-anew");
        let decision = rutent_contracts::core_internal::calls::Authorisations::authorise(
            &mut core.policy,
            &again,
        )
        .unwrap();
        assert_eq!(
            decision.outcome,
            rutent_contracts::core_internal::Answer::Refuse {
                class: RefusalClass::NotPermitted
            }
        );
        assert!(
            authorised(
                &core
                    .log()
                    .entries()
                    .iter()
                    .map(|(_, e)| e.clone())
                    .collect::<Vec<_>>(),
                &order,
                &given.authorisation
            )
            .is_some()
        );
    }
}
