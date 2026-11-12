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
//! - **Policy.** `CMP-CORE-AUTHZ`'s engine, its fixed rules first, each input answered from a real record
//!   ([`policy::Records`]); the calls it doesn't build yet answered beside it on its own handle
//!   ([`policy::PolicySide`]). Core's own actor is listed as carrying the user's authorisation onward, so it can move
//!   value, and the operator's login holds each catalogue change the assembly makes.
//! - **One clock.** Core's snapshots and segments, policy's session bound, the log's skew and the simulated world
//!   read one [`SharedClock`].
//! - **Sessions.** Each session the engine opens is handed to the catalogue's register (`Register::open_in`).
//! - **Intake and the segments.** An order's segments follow only its start as intake answers it
//!   (`Intake::start`), never an acceptance, and the rounds' transactions (`Issued`) are kept across routings so no
//!   segment runs under one.
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

pub mod durable;
mod grading;
pub mod machine;
pub mod policy;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;

use rutent_conformance::role::SuiteContext;
use rutent_conformance::sim::Spec;
use rutent_contracts::core_internal::calls::{AppendFailure, GeneralEntry, Log};
use rutent_contracts::core_internal::{
    Action, Actor, ActorKind, Authorisation, CatalogueChange, DecisionRequest, Entry, Outcome,
    Receipt, RefusalClass, Role, RouteEnd, SessionOutcome, SessionRequest, TimeSource,
    VerifiedEntry, VerifiedKind, What, When,
};
use rutent_contracts::provider_role::{
    ADVERTISEMENT_SHAPE, Operation, ReserveDeclaration, Statement,
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
    OrderRequest, Quote, Resolution, SessionRegister, Status as IntakeStatus, Told,
};
use rutent_core_orch::rounds::{Fresh, Issued, OperatorKey, Parts as Rounds, Providers};
use rutent_core_orch::segments::{
    Live, Modules, NoProxy, Parts as SegmentParts, Ran, Segments, SegmentsError, Status,
    StatusError,
};
use rutent_core_orch::snapshot::{Exposures, Price, Releases};
use rutent_prov::catalogue::Catalogue;
use rutent_prov::module::lifecycle::Staging;
use rutent_prov::module::platform::Platform;
use rutent_prov::module::types::Handled;
use rutent_router::RouteFinder;
use serde_json::json;

use crate::kit::{self, RealKey, Seeded, SharedClock};
use crate::sim::{Fleet, Installed, SimWorld};
use crate::{GeneralHandle, PolicyHandle};
use durable::{Disk, LogDisk};
use machine::{Delivery, Environment, Feed};
use policy::{Deployment, PolicyEngine, PolicySide, Records, Registers};

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

/// What an order needs to run once it started.
#[derive(Debug, Clone)]
struct Started {
    record: DecisionRecord,
    plan: Vec<PartPlan>,
    authorisation: Id,
    origin: Id,
    route: RouteEnd,
    session: Option<Id>,
    inputs: BTreeMap<String, String>,
    /// The least the customer takes at the destination, as the authorisation in the log names it (`T-020-10`).
    floor: Option<rutent_contracts::router_api::PositiveAmount>,
}

/// The operator's key, watched: every decision record Core signs is kept by its id, for the segments to run the
/// route it chose (`Live::record`). Intake hands no record back; the log names it by digest, which Core holds the
/// kept record to before it runs the route.
pub struct Signer {
    key: RealKey,
    /// Each decision record Core signed, by its id: the envelope as signed, its bytes, and nothing read from them
    /// until they are held to the log's digest.
    records: BTreeMap<String, Vec<u8>>,
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
            self.records
                .insert(record.record_id().as_str().to_owned(), recorded);
        }
        Some(signature)
    }
}

/// The platform, as the rounds ask it: each question handed to its provider's module at Core's clock.
struct Asking<'a> {
    platform: &'a mut Platform<SimWorld>,
    clock: &'a SharedClock,
}

impl Providers for Asking<'_> {
    fn ask(&mut self, provider: &Id, question: &[u8]) -> Option<Vec<u8>> {
        let operation: Statement<Operation> = serde_json::from_slice(question).ok()?;
        self.platform.world.set_clock(self.clock.instant());
        match self.platform.handle(provider.as_str(), &operation) {
            Handled::Answered(result) => serde_json::to_vec(&*result).ok(),
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

/// The register of actors, as intake reads it: the deployment's, the one policy reads.
struct Acting(Rc<RefCell<Registers>>);

impl Actors for Acting {
    fn acts_for(&mut self, acts: &Id, party: &Id) -> bool {
        self.0.borrow().acts_for(acts, party) == Some(true)
    }
}

/// Policy's session register, as intake reads it: the sessions the engine opened.
struct Held(Rc<RefCell<PolicyEngine>>);

impl SessionRegister for Held {
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
}

/// Core, put together.
pub struct Core {
    /// Core's one clock.
    pub clock: SharedClock,
    /// What survives a crash: the log's store and the evidence store's shelf.
    pub(crate) disk: Disk,
    log: Rc<RefCell<CoreLog>>,
    general: GeneralHandle<CoreLog>,
    bound: BoundStore<LogDisk, LogsRead>,
    keeper: Keeper,
    policy: PolicySide,
    records: Records,
    /// The platform with the simulated providers admitted to it.
    pub(crate) fleet: Fleet,
    /// Each simulated provider, as installed.
    pub installed: Vec<Installed>,
    /// Core's catalogue.
    pub(crate) catalogue: Catalogue,
    intake: Intake,
    segments: Segments,
    /// What the grading took across orders, kept as long as the segments (`T-020-09`).
    closing: rutent_core_orch::closing::Closing,
    readings: grading::Readings,
    issued: Issued,
    exposures: Exposures,
    releases: Releases,
    signer: Signer,
    fresh: Seeded,
    bounds: PolicyBounds,
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
    /// The order is in no state that step can take.
    NotNow(&'static str),
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
    /// platform with the providers and their networks, the state machine's host, the clock), and Core's own parts are
    /// put together again over what the disk holds: the log and the evidence store read back, and, from the log,
    /// what the grading took (`Closing::recorded`). The rest of Core's own record (intake's, the segments', `Issued`)
    /// is in memory and starts empty (`F-030`); the providers are granted again on the reopened log.
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
        };
        Self::assemble_with(config, environment, self.disk, Some(carried))
    }

    fn assemble_with(
        config: &Config,
        environment: &mut dyn Environment,
        disk: Disk,
        carried: Option<Carried>,
    ) -> Result<Self, AssemblyError> {
        let context = context();
        let reopened = carried.is_some();
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
                let mut fleet = Fleet::with_context(config.deadline, context.clone());
                let installed: Vec<Installed> = config
                    .providers
                    .iter()
                    .map(|spec| fleet.install(spec.clone()))
                    .collect();
                fleet.platform.set_staging(config.staging.policy());

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
            for change in [CatalogueChange::Grant, CatalogueChange::Promote] {
                r.permit(&kit::operator_login(), Permission::Change(change));
            }
            r.permit(&kit::operator_login(), Permission::ReadOrderState);
            r.accounts.insert(
                context.account.clone(),
                kit::customer().party.as_str().to_owned(),
            );
            r.accounts.insert(
                DESTINATION_ACCOUNT.to_owned(),
                kit::customer().party.as_str().to_owned(),
            );
        }
        let records = Records {
            log: Rc::clone(&log),
            orders: Rc::default(),
            declarations: Rc::default(),
            registers,
        };

        // Policy: the engine holds the log's policy handle, the only one, and reads Core's clock.
        let deployment = Deployment::default();
        let mut engine: PolicyEngine = Engine::with_policy(
            PolicyHandle(Rc::clone(&log)),
            deployment.clone(),
            records.clone(),
        );
        engine.set_clock(Box::new(clock.clone()));
        let policy = PolicySide::new(
            Rc::new(RefCell::new(engine)),
            records.clone(),
            deployment.clone(),
        );

        // Orchestration's handle on the evidence store, its reads checked by policy, and then the store bound to the
        // one log it serves.
        let keeper = store.keeper(Box::new(policy.clone()));
        let bound = store.bind(Rc::clone(&log)).map_err(AssemblyError::Log)?;

        let mut catalogue = Catalogue::new();
        catalogue.set_context(context.deployment.clone(), &context.protocol_version);
        // The deployment's bound on balance checks and how long a reserve has to answer one (`REQ-CAT-4`).
        catalogue.set_check_bound(rutent_prov::catalogue::CheckBound {
            max_counted: CHECKS_PER_WINDOW,
            window_secs: 86_400,
        });
        catalogue.set_answer_within(CHECK_ANSWERED_WITHIN);
        let prices = config
            .providers
            .iter()
            .filter(|s| s.role == rutent_conformance::sim::Role::Reserve)
            .flat_map(|s| kit::reserve_prices(&s.provider))
            .collect();

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
            bound,
            keeper,
            policy,
            records,
            fleet,
            installed,
            catalogue,
            intake: Intake::new(),
            segments: Segments::new(),
            closing: rutent_core_orch::closing::Closing::new(),
            readings: grading::Readings::default(),
            issued: Issued::new(),
            exposures: Exposures::new(),
            releases: Releases::new(),
            signer: Signer {
                key: RealKey::assembly(),
                records: BTreeMap::new(),
            },
            fresh: carried_fresh.unwrap_or_else(|| Seeded::new(config.seed)),
            bounds: kit::bounds(&context),
            prices,
            quotes: BTreeMap::new(),
            requests: BTreeMap::new(),
            started: BTreeMap::new(),
            feed,
            deployment,
            faults: Faults::default(),
            swap_to: None,
        };
        // Each provider is granted before the advertisement it sends was made; then the clock moves to the
        // scenario's start, and each advertisement is recorded as received then.
        // What the grading reads, and what it took, rebuilt from the log the disk holds (`T-020-09`).
        core.readings = grading::Readings::of(&core.installed);
        if ties {
            core.readings.tie_by_destination();
        }
        core.closing = rutent_core_orch::closing::Closing::recorded(core.log.borrow().entries());
        if reopened {
            // The providers were listed before Core stopped: its catalogue is rebuilt from what its log holds.
            core.relist()?;
            return Ok(core);
        }
        let installed = core.installed.clone();
        for one in &installed {
            core.grant(one)?;
        }
        core.clock.set_nanos(u128::from(kit::START) * 1_000_000_000);
        for one in &installed {
            core.advertise(one)?;
        }
        Ok(core)
    }

    /// Every decision record Core signed, by its id.
    pub fn decision_records(&self) -> Vec<DecisionRecord> {
        self.signer
            .records
            .values()
            .filter_map(|bytes| record_in(bytes))
            .collect()
    }

    /// From now on, the destination's network ties each deposit to the first movement to its account of its amount, as
    /// a reserve paying two orders with one payment would have it: a fault the scenarios plant (`T-020-09`).
    pub fn tie_payments_by_destination(&mut self) {
        self.readings.tie_by_destination();
    }

    /// The log.
    pub fn log(&self) -> std::cell::Ref<'_, CoreLog> {
        self.log.borrow()
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

    /// The platform with the simulated providers, to read.
    pub fn fleet(&self) -> &Fleet {
        &self.fleet
    }

    /// Policy, as the other parts reach it: the engine's calls, and the ones answered beside it.
    pub fn policy(&self) -> PolicySide {
        self.policy.clone()
    }

    /// What policy's sources read.
    pub fn sources(&self) -> Records {
        self.records.clone()
    }

    /// The deployment's registers.
    pub fn registers(&self) -> std::cell::RefMut<'_, Registers> {
        self.records.registers.borrow_mut()
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
        for tuple in rutent_prov::catalogue::tuples_of(&statement.statement) {
            let action = Action::ChangeCatalogue {
                change: CatalogueChange::Grant,
                provider: provider.clone(),
                version: None,
                tuple: Some(kit::tuple_id(provider.as_str(), &tuple)),
                advertising_keys: vec![key.clone()],
                statement_key: None,
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
            terms.tuples = vec![tuple];
            kit::grant(&mut self.catalogue, installed, terms);
            self.verified(What::Result { effect }, Outcome::Done {})
                .map_err(|e| listing("the grant's result", e))?;
        }
        Ok(())
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
        for (receipt, entry) in &entries {
            let Entry::Verified(v) = entry else {
                if let Entry::Stated(s) = entry
                    && s.what.statement.as_str() == ADVERTISEMENT_SHAPE
                    && let Some(one) = self.installed.iter().find(|one| {
                        one.advertisement.statement().maker == s.attribution.maker().acts.as_str()
                    })
                {
                    let one = one.clone();
                    let bytes = kept
                        .iter()
                        .find(|k| k.kept.commitment == s.commitment)
                        .and_then(|k| rutent_core_audit::receipts::store::bearing_out(&k.kept));
                    if bytes.as_deref() != Some(one.advertisement.bytes()) {
                        return Err(AssemblyError::Listing(format!(
                            "the advertisement the log holds for {} isn't the installed one: its commitment's bytes differ",
                            s.attribution.maker().acts.as_str()
                        )));
                    }
                    let at = rutent_core_authz::session::instant(&s.received_at);
                    kit::join(&mut self.catalogue, &one, at);
                    let statement = one.advertisement.statement();
                    if let Some(reserve) = &statement.statement.offers.reserve {
                        self.records.declarations.borrow_mut().insert(
                            statement.statement.provider.clone(),
                            ReserveDeclaration::clone(&reserve.declaration),
                        );
                    }
                }
                continue;
            };
            let What::Effect {
                action:
                    Action::ChangeCatalogue {
                        change,
                        provider,
                        tuple,
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
        let provider = kit::id(&statement.statement.provider);
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
        // Into Core's catalogue once the log holds it, at the time the log received it.
        let at = self.time_of_stated(&receipt);
        kit::join(&mut self.catalogue, installed, at);
        if let Some(reserve) = &statement.statement.offers.reserve {
            self.records.declarations.borrow_mut().insert(
                provider.as_str().to_owned(),
                ReserveDeclaration::clone(&reserve.declaration),
            );
        }
        Ok(())
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
    pub fn open_session(&mut self, holder: &Actor) -> Result<Id, StepError> {
        let context = context();
        let now = self.clock.peek();
        let until = rutent_contracts::router_api::Timestamp::new(
            rutent_prov::catalogue::Instant(
                u64::try_from(now / 1_000_000_000).unwrap_or(0) + LONGEST_SESSION_SECS - 60,
            )
            .rfc3339()
            .ok_or(StepError::NotNow("a session's end"))?,
        )
        .ok_or(StepError::NotNow("a session's end"))?;
        let request = SessionRequest {
            request: kit::id(&context.session),
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
            platform: &mut self.fleet.platform,
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
        let mut owners = Owners(Rc::clone(&self.records.registers));
        let mut acting = Acting(Rc::clone(&self.records.registers));
        let mut held = Held(Rc::clone(self.policy.engine()));
        let mut authorisations = self.policy.clone();
        let mut desk = Desk {
            parts: &mut parts,
            destinations: &mut owners,
            authorisations: &mut authorisations,
            actors: &mut acting,
            sessions: &mut held,
            bounds: &self.bounds,
            prices: &self.prices,
            core: &core,
            operator: &operator,
        };
        call(&mut self.intake, &mut desk)
    }

    /// Takes `request`, and keeps intake's record of the order it opens for policy's sources.
    pub fn request(&mut self, request: OrderRequest) -> Result<Told, StepError> {
        let told = self.with_desk(|intake, desk| intake.open(desk, request.clone()))?;
        if let Told::Exists { order, .. } = &told {
            if let Some(record) = self.intake.record(order) {
                self.records
                    .orders
                    .borrow_mut()
                    .insert(order.as_str().to_owned(), record);
            }
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
        let told = self.with_desk(|intake, desk| intake.accept(desk, order, by, digest, None))?;
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
        let started = Started {
            record,
            plan: parts.clone(),
            authorisation,
            origin: request.origin_reserve.clone(),
            route: RouteEnd {
                destination: authorised.destination.clone(),
                to_asset: authorised.to_asset.clone(),
                delivers: quote.delivers.clone(),
            },
            session: Some(request.session.clone()),
            inputs,
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

    /// Runs every part of `order`'s route, each segment after the one before settles.
    pub fn run(&mut self, order: &Id) -> Result<Vec<Ran>, StepError> {
        let started = self
            .started
            .get(order.as_str())
            .cloned()
            .ok_or(StepError::NotNow("the order started"))?;
        let core = kit::core();
        let operator = kit::operator();
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
            };
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
            };
            let result = self.segments.run_part(&mut parts, &live);
            self.feed()?;
            ran.push(result?);
        }
        Ok(ran)
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
/// the same id for another order, or one replaced, is no authorisation this order starts under. The engine holds each
/// of the order's moves to the same view.
fn authorised(entries: &[Entry], order: &Id, authorisation: &Id) -> Option<Authorisation> {
    rutent_core_authz::funds::Authorised::of(rutent_core_authz::funds::permitted_in(entries))
        .in_force_as(order, authorisation)
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
            status: Status::Quoted { quote, digest },
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
            advertising_keys: Vec::new(),
            statement_key: None,
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

    /// A Core opened again joins an advertisement only where the bytes the evidence store kept under the log's
    /// commitment are the installed provider's: the same statement in other bytes refuses the reopening.
    // verifies: REQ-AUD-2, REQ-CAT-6
    #[test]
    fn reopened_core_joins_only_the_advertisement_its_log_committed_to() {
        let (mut core, config) = alone();
        let mut bytes = core.installed[0].advertisement.bytes().to_vec();
        bytes.push(b' ');
        core.installed[0].advertisement =
            rutent_conformance::role::Received::from_bytes(bytes).expect("the same statement");
        let reopened = core.reopened(&config, &mut machine::Reference);
        assert!(
            matches!(&reopened, Err(AssemblyError::Listing(why)) if why.contains("commitment")),
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
        let waits = core
            .segments
            .waiting(&order, &part)
            .expect("the part waits at its floor");
        assert_eq!(waits.evidenced.amount().as_str(), "990000");
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
