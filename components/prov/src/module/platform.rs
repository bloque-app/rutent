//! The platform: the format binding the module-boundary suite drives (`ModuleBinding`, `BoundaryHarness`). It runs
//! in its own process and holds everything a module must not reach on its own: the credential proxy, each
//! provider's state, the egress layer and the endpoints behind it, the cross-module router, admission and every
//! artefact. Each provider's module runs in a process of its own (`children::ProviderProcess`), and each admission
//! compiles in a fresh compiler process (`children::compile`).
//!
//! Every check is made here, against the provider's **running** declaration, and every request is attributed to
//! the provider whose process's pipe it came on, never to anything the request says (`DEC-9`). A module escaping
//! Wasm reaches its own process, which holds no check and nothing of another provider's.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use std::num::NonZeroU16;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rutent_contracts::module_interface::{
    Acceptance, AcceptanceMode, Accepted, AdmissionArtefact, Build, CallOutcome, CallRefusalReason,
    CallRefused, Capabilities, CrossModuleCall, CrossModuleOutcome, Declaration, Digest,
    DigestAlgorithm, DigestValue, Hop, Host, Identifier, Name, Nothing, OperationKind,
    OutboundCallRecord, OutsideOutcome, OutsideRequest, OutsideResponse, PartName, Parts,
    RefusalReason, Refused, RuleViolation, Scheme, SecretLookup, SecretRefusalReason, SecretValue,
    Time, validate_artefact, validate_release, widening,
};
use rutent_contracts::provider_role::{
    Operation, RESULT_SHAPE, ResultBody, Signature, SignatureScheme, Statement,
};
use serde_json::Value;

use crate::catalogue::{Instant as Clock, Stamp};
use crate::egress::{
    self, Call, ConnectError, Connector, Dial, Egress, Method, NameResolver, Outcome,
};
use crate::module::admission::{self, ReleaseJudge, Unverified};
pub use crate::module::children::COMPILE_DEADLINE;
use crate::module::children::{
    CompileError, Machine, Paths, ProviderProcess, Received, Stderr, Stderrs, compile,
};
use crate::module::engine::{self, FORMAT, MAX_BINARY};
use crate::module::lifecycle::{self, Lifecycle, LifecycleRefusal, SegmentRecord, Staging};
use crate::module::limits::{
    Bounds, LimitHit, LimitKind, MAX_BODY, MAX_HEADERS, MAX_HOST, MAX_KEY, MAX_MEASUREMENTS,
    MAX_NAME, MAX_OUTSIDE_MESSAGE, Metered, Usage, charge, depth_allows, keep_last,
    within as within_bound,
};
use crate::module::output;
use crate::module::types::{
    AdmissionRefusal, Admitted, Endpoint, Envelope, Events, Handled, LimitEvent, LookupEvent,
    LookupOutcome, PlatformRefusal, ProcessId, StateEvent, UnknownOutcome,
};
use crate::module::wire::{
    FromProvider, OUTPUT_TOO_LARGE, Reply, ToProvider, WireHeader, sha256, unhex,
};
use crate::proxy::{self, CallAsked, Caller, InFlight, RouteCalls};

/// The longest one operation may take, from the platform handing it to a provider's process to that process's
/// answer, cross-module calls it makes included, before the platform kills the process (`REQ-MDL-11`). Fuel
/// bounds the instructions a guest runs; this bounds the wall time, which fuel doesn't: a process that stops
/// answering, or keeps the platform busy with requests, is stopped by it. Far above what a guest that runs out of
/// fuel takes.
pub const OPERATION_DEADLINE: Duration = Duration::from_secs(10);

/// The most a provider's process may take to start, both parts started, before the platform kills it.
pub const START_DEADLINE: Duration = Duration::from_secs(10);

/// The least time a callee is given to answer a cross-module call (`REQ-MDL-11`, `REQ-MDL-21`). The callee's
/// deadline ends a reserve of half this budget before its caller's, so that a callee stopped at its deadline leaves
/// its caller time to hear that the call went unanswered, and to answer its own operation (`REQ-ORD-1`). A call is
/// made only when the budget and the reserve are both left of the caller's deadline, one and a half times this,
/// and is refused as `deadline_too_near` otherwise: a callee started with less would be stopped before it could
/// answer, or take its caller's answer with it.
pub const MIN_CALL_BUDGET: Duration = Duration::from_secs(1);

/// One provider's releases and its running processes.
#[derive(Default)]
struct Slot {
    accepted: Vec<AdmissionArtefact>,
    /// The release waiting for a fresh acceptance, its machine code, and its assertion as received, judged again
    /// when it is accepted.
    pending: Option<(AdmissionArtefact, Rc<Machine>, Vec<u8>)>,
    /// The versions of releases withdrawn while they waited: a version names one release, so none is admitted
    /// again.
    withdrawn: BTreeSet<String>,
    machines: BTreeMap<String, Rc<Machine>>,
    /// The version in force, and its process.
    active: Option<String>,
    process: Option<ProviderProcess>,
    /// The other versions that run beside it, each in its own process: the staged release, and versions segments
    /// in flight are finishing on (`T-013-06`). Still the one provider's code, so no other provider's process is
    /// shared.
    others: BTreeMap<String, ProviderProcess>,
}

/// The operation one provider's module is handling, and what it obtained while handling it.
struct Context {
    provider: String,
    version: String,
    declaration: Declaration,
    operation: Statement<Operation>,
    transaction: String,
    serving: Option<OutsideRequest>,
    resolved: BTreeMap<String, Vec<Name>>,
    /// When the operation's deadline passes: its own, or the operation that called it's, whichever is first.
    due: Instant,
    /// What this module's operation has used of its budget.
    usage: Usage,
    /// The first bound the operation hit. From then on every request of it is refused, and the operation is
    /// refused as a limit whatever its process says.
    limited: Option<LimitHit>,
    /// Whether the module passed anything out: an outbound call handed to the egress layer, or a cross-module call
    /// handed to its callee. From then on what it did may have taken effect, so a failure is never given as a
    /// reason that says nothing took effect (`PlatformRefusal::OutcomeUnknown`, `REQ-ORD-1`).
    passed_out: bool,
}

/// What the binding measured, for `B-06`.
#[derive(Debug, Clone, Default)]
pub struct Measurements {
    /// Each admission's compilation, start of the compiler's process to its exit, with both parts' portable and
    /// machine-code sizes in bytes.
    pub compiles: Vec<(Duration, [usize; 2], [usize; 2])>,
    /// Each provider process's start, spawn to both parts started. Like `compiles`, only the newest are kept
    /// ([`Platform::set_measurement_cap`]).
    pub starts: Vec<Duration>,
    /// Why the last operation a sandboxed part didn't finish stopped, as its process reported it, or
    /// `deadline: ...` if the platform killed the process at the operation's deadline.
    pub last_trap: Option<String>,
    /// Operations whose provider's process missed the deadline and was killed.
    pub overdue: usize,
    /// The process ids of the admitting compilers that passed their wall-time deadline, and were killed.
    pub overdue_compilers: Vec<u32>,
}

/// What the platform reaches beyond the provider's process: the credential proxy's secrets, each provider's
/// state, and the network behind the egress layer. The platform makes every check before it asks, and tells the
/// world what each provider's module is handling, so a world that judges for itself, the credential proxy
/// (`proxy::ProductionWorld`, `T-014-05`), judges against the same state.
///
/// The four required hooks are a test double's: the module-boundary suite's binding seeds secrets, state and
/// endpoints behind them. Every other hook has a default built on them, which the production world replaces:
///
/// - the lifecycle hooks ([`World::running`], [`World::begin_operation`], [`World::begin_request`] and their
///   ends) do nothing by default;
/// - [`World::lookup`] answers from [`World::secret`], and only when the platform's own judgement passed;
/// - [`World::statement_key`] refuses, so a world with no key has no operation handed to its modules;
/// - [`World::now`] reads the system clock;
/// - [`World::send`] runs the egress layer ([`Egress`]) over [`World::route`] and [`World::deliver`], so every
///   world's outbound calls pass the same checks and leave the same records.
pub trait World {
    /// The value of the secret `name` of `provider`, or `None` if the world holds none. The default
    /// [`World::lookup`] asks it only after the platform judged the lookup declared and for the request in flight.
    fn secret(&mut self, provider: &str, name: &str) -> Option<String>;

    /// The value of the provider's own state entry `key`, if any. Asked only while an outside request is in
    /// flight.
    fn state(&mut self, provider: &str, key: &str) -> Option<String>;

    /// Where `endpoint` is served: the address connected to, and where it redirects, or `None` if nothing answers.
    /// A lookup only; nothing is sent. Only the default [`World::send`] asks it.
    fn route(&mut self, endpoint: &Endpoint) -> Option<(IpAddr, Option<Endpoint>)>;

    /// Receives `body` at `endpoint`, which the egress layer has judged allowed and dialled at the address
    /// [`World::route`] gave. Only the default [`World::send`] calls it.
    fn deliver(&mut self, endpoint: &Endpoint, body: &str);

    /// Receives `request` at `endpoint`, which the egress layer has judged allowed and dialled at the address
    /// [`World::route`] gave: its method, target, headers and body, as the connector would send them. The default
    /// hands the body to [`World::deliver`]. Only the default [`World::send`] calls it.
    fn receive(&mut self, endpoint: &Endpoint, request: &egress::Request) {
        self.deliver(endpoint, &String::from_utf8_lossy(request.body()));
    }

    /// The release `artefact` now runs for its provider: its process started.
    fn running(&mut self, artefact: &AdmissionArtefact) {
        let _ = artefact;
    }

    /// The caller's module starts handling an operation for `transaction`, which must end by `due`: whatever
    /// the world asks a store or a peer for it is given up then (`REQ-MDL-11`).
    fn begin_operation(&mut self, caller: &Caller, transaction: &Identifier, due: Instant) {
        let _ = (caller, transaction, due);
    }

    /// The caller's module ended the operation it was handling, however it ended.
    fn end_operation(&mut self, caller: &Caller) {
        let _ = caller;
    }

    /// The platform passed an outside request for `transaction` and `operation` to the caller's outside part: it is
    /// in flight until [`World::end_request`].
    fn begin_request(&mut self, caller: &Caller, transaction: &Identifier, operation: &Name) {
        let _ = (caller, transaction, operation);
    }

    /// The caller's outside request in flight was answered.
    fn end_request(&mut self, caller: &Caller) {
        let _ = caller;
    }

    /// Answers the caller's `lookup`, for the outside request in flight, given the platform's own `judged`
    /// verdict on it. A refused verdict is never answered with a value; the world may record it. The default
    /// asks [`World::secret`] only if the verdict passed, and a secret it doesn't hold is `not_found`.
    fn lookup(
        &mut self,
        caller: &Caller,
        lookup: &SecretLookup,
        judged: Result<(), proxy::Refusal>,
    ) -> Result<SecretValue, SecretRefusalReason> {
        judged.map_err(|refusal| refusal.wire_reason())?;
        self.secret(caller.provider().as_str(), lookup.name.as_str())
            .and_then(SecretValue::new)
            .ok_or(SecretRefusalReason::NotFound)
    }

    /// The private half of the statement key the caller's running release declares, while its module handles an
    /// operation for `transaction`. The platform asks it once the operation began ([`World::begin_operation`])
    /// and before it hands the operation to the module, so a key it can't have refuses the operation before
    /// anything of it runs: `store_unavailable` as `PlatformRefusal::StatementKeyUnavailable`, which may pass, and
    /// any other refusal, or a key that isn't the declared one's, as `PlatformRefusal::StatementKeyMisconfigured`.
    /// The default holds none.
    fn statement_key(
        &mut self,
        caller: &Caller,
        transaction: &Identifier,
    ) -> Result<SecretValue, SecretRefusalReason> {
        let _ = (caller, transaction);
        Err(SecretRefusalReason::UndeclaredSecret)
    }

    /// The time now, by the world's clock: when an outbound call the platform records started, refused ones
    /// included (`REQ-AUD-1`). The platform holds no clock of its own, and makes no call it couldn't date: with no
    /// time, the call is refused before it is made, and nothing is recorded. The default reads the system clock.
    fn now(&mut self) -> Result<Time, ClockError> {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ClockError::BeforeEpoch)?
            .as_secs();
        time_of(Clock(secs)).ok_or(ClockError::PastYear9999)
    }

    /// Makes one outbound call through the egress layer and writes its record to `log`. The default runs
    /// [`Egress`] over [`World::route`] and [`World::deliver`].
    fn send(
        &mut self,
        call: &Call<'_>,
        endpoint: egress::Endpoint,
        request: egress::Request,
        log: &mut Vec<OutboundCallRecord>,
    ) -> Outcome {
        let served = Served(RefCell::new(self));
        Egress::new(&served, &served).send(call, endpoint, request, log)
    }
}

/// Why a world's clock gives no time a record can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockError {
    /// It reads before 1970-01-01T00:00:00Z.
    BeforeEpoch,
    /// It reads past the year 9999.
    PastYear9999,
}

/// An outbound call's request as the outside part wrote it, before the platform checks it.
struct WireRequest {
    method: String,
    path: String,
    headers: Vec<WireHeader>,
    body: String,
}

/// A world's [`World::route`] and [`World::deliver`] as the egress layer's resolver and connector, for the
/// default [`World::send`]. A name resolves to the address its endpoint is served at; a dial to that address
/// delivers the body and answers `200`, or `307` to the endpoint it redirects to, which keeps the method and the
/// body. A dial to any other address, or to an endpoint nothing serves, connects to nothing.
struct Served<'a, W: World + ?Sized>(RefCell<&'a mut W>);

impl<W: World + ?Sized> NameResolver for Served<'_, W> {
    fn resolve(&self, host: &Host, port: NonZeroU16) -> Vec<IpAddr> {
        let mut world = self.0.borrow_mut();
        [Scheme::Https, Scheme::Wss]
            .into_iter()
            .find_map(|scheme| {
                world.route(&Endpoint {
                    scheme,
                    host: host.as_str().to_owned(),
                    port: port.get(),
                })
            })
            .map(|(address, _)| address)
            .into_iter()
            .collect()
    }
}

impl<W: World + ?Sized> Connector for Served<'_, W> {
    fn exchange(
        &self,
        dial: &Dial<'_>,
        request: &egress::Request,
    ) -> Result<egress::Response, ConnectError> {
        let mut world = self.0.borrow_mut();
        let endpoint = Endpoint {
            scheme: dial.scheme,
            host: dial.name.as_str().to_owned(),
            port: dial.to.port(),
        };
        let (address, redirect) = world.route(&endpoint).ok_or(ConnectError::Unreachable)?;
        if address != dial.to.ip() {
            return Err(ConnectError::Unreachable);
        }
        world.receive(&endpoint, request);
        Ok(match redirect {
            None => egress::Response {
                status: 200,
                headers: Vec::new(),
                body: Vec::new(),
            },
            Some(next) => egress::Response {
                status: 307,
                headers: vec![(
                    "location".to_owned(),
                    format!(
                        "{}://{}:{}/",
                        match next.scheme {
                            Scheme::Wss => "wss",
                            _ => "https",
                        },
                        next.host,
                        next.port
                    ),
                )],
                body: Vec::new(),
            },
        })
    }
}

/// The Wasm-component platform: admission, one OS process per provider, and the checks on everything a provider's
/// module asks for.
pub struct Platform<W: World> {
    paths: Paths,
    workdir: PathBuf,
    output_limit: usize,
    envelope: Envelope,
    /// What the platform reaches beyond its providers' processes.
    pub world: W,
    /// The calls the routes in flight name, by transaction: a cross-module call is made only where one does
    /// (`REQ-MDL-21`). Whoever hands the platform a route segment's operation names its calls here, and ends them.
    pub routes: RouteCalls,
    operations_received: BTreeMap<String, usize>,
    providers: BTreeMap<String, Slot>,
    stack: Vec<Context>,
    events: Events,
    deadline: Duration,
    stderrs: Stderrs,
    bounds: Bounds,
    /// What each provider has used inside the top-level operation in flight, which ends with it.
    tree: BTreeMap<String, Usage>,
    call_budget: Duration,
    compile_deadline: Duration,
    measurement_cap: usize,
    /// What it measured.
    pub measurements: Measurements,
    /// Each provider's accepted releases: in force, staged, the segments in flight and the decisions taken.
    lifecycle: Lifecycle,
    /// The version of each provider the world was last told runs ([`World::running`]): with two versions running,
    /// it is told again whenever the other one handles an operation, so the proxy judges against the release that
    /// handles it (`T-013-06`).
    told: BTreeMap<String, String>,
    /// How many operations each provider's version ran: its process took the operation and said something of it,
    /// so an operation refused before reaching a live process doesn't count towards a staging.
    runs: BTreeMap<(String, String), u64>,
    /// The versions whose process ended during the top-level operation in flight, started again once it ends.
    restarts: Vec<(String, String)>,
}

impl<W: World> Platform<W> {
    /// A platform whose children are the executables at `paths`, over `world`, serving the deployment and
    /// protocol version `envelope` names.
    pub fn new(paths: Paths, world: W, envelope: Envelope) -> Result<Self, String> {
        let workdir = std::env::temp_dir().join(format!(
            "rutent-wasm-binding-{}-{}",
            std::process::id(),
            unique()
        ));
        std::fs::create_dir_all(&workdir).map_err(|e| format!("{}: {e}", workdir.display()))?;
        Ok(Self {
            paths,
            workdir,
            output_limit: 0,
            envelope,
            world,
            routes: RouteCalls::default(),
            operations_received: BTreeMap::new(),
            providers: BTreeMap::new(),
            stack: Vec::new(),
            events: Events::default(),
            deadline: OPERATION_DEADLINE,
            stderrs: Stderrs::default(),
            bounds: Bounds::DEFAULT,
            tree: BTreeMap::new(),
            call_budget: MIN_CALL_BUDGET,
            compile_deadline: COMPILE_DEADLINE,
            measurement_cap: MAX_MEASUREMENTS,
            measurements: Measurements::default(),
            lifecycle: Lifecycle::default(),
            told: BTreeMap::new(),
            runs: BTreeMap::new(),
            restarts: Vec::new(),
        })
    }

    /// The directory the platform keeps its children's working directories in: one for each child running, none
    /// for one that ended.
    pub fn workdir(&self) -> &std::path::Path {
        &self.workdir
    }

    /// Sets the bounds on what a module may ask of the platform, [`Bounds::DEFAULT`] by default.
    pub fn set_bounds(&mut self, bounds: Bounds) {
        self.bounds = bounds;
    }

    /// Sets the least time a callee is given to answer a cross-module call, [`MIN_CALL_BUDGET`] by default. A call
    /// needs one and a half times it left of its caller's deadline: the budget, and the caller's reserve after it.
    pub fn set_call_budget(&mut self, least: Duration) {
        self.call_budget = least;
    }

    /// Sets how long the admitting compiler has, wall time, [`COMPILE_DEADLINE`] by default.
    pub fn set_compile_deadline(&mut self, deadline: Duration) {
        self.compile_deadline = deadline;
    }

    /// Sets how many of each measurement the platform keeps, the newest, [`MAX_MEASUREMENTS`] by default.
    pub fn set_measurement_cap(&mut self, cap: usize) {
        self.measurement_cap = cap;
    }

    /// Sets how many children's standard errors the platform keeps, the newest.
    pub fn set_stderr_capacity(&mut self, capacity: usize) {
        if let Ok(mut log) = self.stderrs.lock() {
            log.set_capacity(capacity);
        }
    }

    /// Sets the operation deadline, [`OPERATION_DEADLINE`] by default.
    pub fn set_deadline(&mut self, deadline: Duration) {
        self.deadline = deadline;
    }

    /// What every child, compiler or provider, wrote to its standard error, each capped and labelled.
    pub fn stderrs(&self) -> Vec<Stderr> {
        self.stderrs.lock().map(|s| s.entries()).unwrap_or_default()
    }

    /// The machine code admitted for a provider's version.
    pub fn machine(&self, provider: &str, version: &str) -> Option<Rc<Machine>> {
        let slot = self.providers.get(provider)?;
        slot.machines.get(version).cloned().or_else(|| {
            slot.pending
                .as_ref()
                .filter(|(a, _, _)| a.declaration().version.as_str() == version)
                .map(|(_, m, _)| m.clone())
        })
    }

    /// Every running provider and its process id.
    pub fn processes(&self) -> Vec<(String, u32)> {
        self.providers
            .iter()
            .filter_map(|(name, slot)| slot.process.as_ref().map(|p| (name.clone(), p.pid())))
            .collect()
    }

    /// Kills a provider's process from outside, as a crash would: the platform sees it gone.
    pub fn kill(&mut self, provider: &str) -> bool {
        match self
            .providers
            .get_mut(provider)
            .and_then(|s| s.process.as_mut())
        {
            Some(process) => {
                process.kill();
                true
            }
            None => false,
        }
    }

    /// Starts `version` of the provider in a new process, from its admitted machine code: the process and the
    /// release it runs. `None` if the version has no accepted release or its process didn't start; nothing of the
    /// provider changes.
    fn spawn(
        &mut self,
        provider: &str,
        version: &str,
    ) -> Option<(ProviderProcess, AdmissionArtefact)> {
        let slot = self.providers.get(provider)?;
        let machine = slot.machines.get(version).cloned()?;
        let artefact = slot
            .accepted
            .iter()
            .rev()
            .find(|a| a.declaration().version.as_str() == version)?
            .clone();
        let code = &artefact.build.machine_code;
        let admitted = [
            code.sandboxed.value.as_str().to_owned(),
            code.outside.value.as_str().to_owned(),
        ];
        // A process that fails to start is dropped, which kills it.
        let process = ProviderProcess::spawn(
            &self.paths,
            &self.workdir,
            provider,
            &machine,
            [&admitted[0], &admitted[1]],
            &self.stderrs,
            START_DEADLINE,
        )
        .ok()?;
        keep_last(
            &mut self.measurements.starts,
            process.took,
            self.measurement_cap,
        );
        Some((process, artefact))
    }

    /// Starts `version` of the provider again, in its place: the version in force's process, or another version's
    /// beside it. If it doesn't start, the provider keeps what it had.
    fn respawn(&mut self, provider: &str, version: &str) -> bool {
        let Some((process, artefact)) = self.spawn(provider, version) else {
            return false;
        };
        let slot = self.providers.entry(provider.to_owned()).or_default();
        if slot.active.as_deref() == Some(version) {
            slot.process = Some(process);
        } else {
            slot.others.insert(version.to_owned(), process);
        }
        self.told_running(provider, &artefact);
        true
    }

    /// Tells the world that `artefact` runs for `provider`, and records it.
    fn told_running(&mut self, provider: &str, artefact: &AdmissionArtefact) {
        self.world.running(artefact);
        self.told.insert(
            provider.to_owned(),
            artefact.declaration().version.as_str().to_owned(),
        );
    }

    /// Tells the world that the provider's `version` runs, unless it was the last told: the version about to handle
    /// an operation, so the proxy judges its lookups and resolves its statement key against its own release.
    fn tell_running(&mut self, provider: &str, version: &str) {
        if self.told.get(provider).map(String::as_str) == Some(version) {
            return;
        }
        let artefact = self.providers.get(provider).and_then(|slot| {
            slot.accepted
                .iter()
                .rev()
                .find(|a| a.declaration().version.as_str() == version)
                .cloned()
        });
        if let Some(artefact) = artefact {
            self.told_running(provider, &artefact);
        }
    }

    /// Starts a release just accepted, at the caller's clock `at`: staged beside the version in force where a
    /// staging policy is set and a version is in force, and in force otherwise (`T-013-06`). If its process
    /// doesn't start, the version in force stays in force, and nothing runs the release (`REQ-MDL-4`, `AC-19`).
    fn launch(&mut self, provider: &str, version: &str, at: Clock) {
        let placement = self.lifecycle.placement(provider);
        let Some((process, artefact)) = self.spawn(provider, version) else {
            return;
        };
        self.lifecycle.started(provider, version, placement, at);
        self.providers
            .entry(provider.to_owned())
            .or_default()
            .others
            .insert(version.to_owned(), process);
        self.told_running(provider, &artefact);
        self.reconcile(provider);
    }

    /// Makes the provider's processes what its lifecycle wants: the version in force in its place, every other
    /// version wanted beside it, and no process of a version nothing runs on, which is stopped as it drops. A
    /// version put back in force with no process is started; if it doesn't start, no version is in force. Never
    /// called while the provider handles an operation, whose process is out of its place.
    fn reconcile(&mut self, provider: &str) {
        let wanted = self.lifecycle.wanted(provider);
        let in_force = self.lifecycle.in_force(provider).map(str::to_owned);
        let Some(slot) = self.providers.get_mut(provider) else {
            return;
        };
        let mut running = std::mem::take(&mut slot.others);
        if let (Some(active), Some(process)) = (slot.active.clone(), slot.process.take()) {
            running.insert(active, process);
        }
        running.retain(|version, _| wanted.contains(version));
        slot.process = in_force.as_ref().and_then(|v| running.remove(v));
        slot.active.clone_from(&in_force);
        slot.others = running;
        if let Some(version) = in_force
            && slot.process.is_none()
            && !self.respawn(provider, &version)
        {
            self.lifecycle.not_running(provider, &version);
            if let Some(slot) = self.providers.get_mut(provider) {
                slot.active = None;
            }
        }
    }

    /// The release a new one of the provider follows: the version in force, or else the newest accepted that is
    /// neither revoked nor declared exploited.
    fn release_followed(&self, provider: &str) -> Option<AdmissionArtefact> {
        let slot = self.providers.get(provider)?;
        let in_force = self.lifecycle.in_force(provider);
        slot.accepted
            .iter()
            .rev()
            .find(|a| in_force == Some(a.declaration().version.as_str()))
            .or_else(|| {
                slot.accepted.iter().rev().find(|a| {
                    !matches!(
                        self.lifecycle
                            .mark(provider, a.declaration().version.as_str()),
                        Some(lifecycle::Mark::Revoked | lifecycle::Mark::Exploited)
                    )
                })
            })
            .cloned()
    }

    /// The declaration of the provider's accepted release of `version`.
    fn declaration_of(&self, provider: &str, version: &str) -> Option<Declaration> {
        self.providers
            .get(provider)?
            .accepted
            .iter()
            .rev()
            .find(|a| a.declaration().version.as_str() == version)
            .map(|a| a.declaration().clone())
    }

    /// The operation on top of the stack hit `hit`: it is refused as a limit, and so is every later request of it
    /// (`REQ-MDL-11`). Recorded once, for the first bound it hit.
    fn limit(&mut self, hit: LimitHit) -> Reply {
        let context = self.stack.last_mut().expect("a context");
        if context.limited.is_none() {
            context.limited = Some(hit);
            self.events.limits.push(LimitEvent {
                provider: context.provider.clone(),
                transaction: context.transaction.clone(),
                hit,
            });
        }
        Reply::Limit
    }

    /// Charges `amount` of `what` to the operation on top of the stack and to its provider, or hits the bound.
    fn charge(&mut self, what: Metered, amount: u64) -> Result<(), LimitHit> {
        let context = self.stack.last_mut().expect("a context");
        let provider = self.tree.entry(context.provider.clone()).or_default();
        charge(&self.bounds, &mut context.usage, provider, what, amount)
    }

    /// Judges what a request carries before anything of it is copied, recorded or looked up: each length within
    /// its bound, in order, and then the metered counts. Hitting one ends the operation (`REQ-MDL-11`, `THR-57`).
    fn admit_request(
        &mut self,
        lengths: &[(LimitKind, usize, usize)],
        metered: &[(Metered, u64)],
    ) -> Result<(), Reply> {
        for (kind, length, max) in lengths {
            within_bound(*kind, *length, *max).map_err(|hit| self.limit(hit))?;
        }
        for (what, amount) in metered {
            self.charge(*what, *amount).map_err(|hit| self.limit(hit))?;
        }
        Ok(())
    }

    /// Answers one request a provider's process made while handling the operation on top of the stack.
    fn serve(&mut self, request: FromProvider) -> Reply {
        // Once an operation hit a bound nothing more of it is served: no world is asked, nothing is recorded.
        if self
            .stack
            .last()
            .is_some_and(|context| context.limited.is_some())
        {
            return Reply::Limit;
        }
        match request {
            FromProvider::OutsideRequest { json } => {
                match self.admit_request(
                    &[(LimitKind::MessageSize, json.len(), MAX_OUTSIDE_MESSAGE)],
                    &[(Metered::OutsideRequests, 1)],
                ) {
                    Ok(()) => self.outside_request(&json),
                    Err(reply) => reply,
                }
            }
            FromProvider::OutsideResponse { json } => {
                match self.admit_request(
                    &[(LimitKind::MessageSize, json.len(), MAX_OUTSIDE_MESSAGE)],
                    &[],
                ) {
                    Ok(()) => self.outside_response(&json),
                    Err(reply) => reply,
                }
            }
            FromProvider::DeniedSecret { name } => {
                if let Err(reply) = self.admit_request(
                    &[(LimitKind::NameLength, name.len(), MAX_NAME)],
                    &[(Metered::SecretLookups, 1)],
                ) {
                    return reply;
                }
                // The attempt names no operation; it is recorded under the running declaration's first outside
                // operation, which the platform knows itself, and the transaction being handled.
                let context = self.stack.last().expect("a context");
                let operation = context
                    .declaration
                    .outside
                    .operations
                    .first()
                    .map(|o| o.name.clone());
                if let (Some(transaction), Some(operation), Some(name)) = (
                    Identifier::new(context.transaction.clone()),
                    operation,
                    Name::new(name),
                ) {
                    let provider = context.provider.clone();
                    self.events.lookups.push(LookupEvent {
                        provider,
                        part: PartName::Sandboxed,
                        lookup: SecretLookup {
                            transaction,
                            operation,
                            name,
                        },
                        outcome: None,
                    });
                }
                Reply::Noted
            }
            FromProvider::DeniedState { key } => {
                if let Err(reply) = self.admit_request(
                    &[(LimitKind::KeyLength, key.len(), MAX_KEY)],
                    &[(Metered::StateReads, 1)],
                ) {
                    return reply;
                }
                let provider = self.stack.last().expect("a context").provider.clone();
                self.events.state_reads.push(StateEvent {
                    provider,
                    part: PartName::Sandboxed,
                    key,
                    found: false,
                });
                Reply::Noted
            }
            FromProvider::Lookup {
                transaction,
                operation,
                name,
            } => {
                let lengths = [transaction.len(), operation.len(), name.len()]
                    .map(|length| (LimitKind::NameLength, length, MAX_NAME));
                match self.admit_request(&lengths, &[(Metered::SecretLookups, 1)]) {
                    Ok(()) => self.lookup(transaction, operation, name),
                    Err(reply) => reply,
                }
            }
            FromProvider::State { key } => {
                // The key is judged where it arrived, before it is copied into an event or handed to the world.
                if let Err(reply) = self.admit_request(
                    &[(LimitKind::KeyLength, key.len(), MAX_KEY)],
                    &[(Metered::StateReads, 1)],
                ) {
                    return reply;
                }
                // Only while an outside request is in flight: outside one, the outside part has nothing to serve.
                // Only the shared entries and the handled transaction's own: another transaction's entry is never
                // asked of the world (`REQ-MDL-10`).
                let context = self.stack.last().expect("a context");
                let (provider, serving) = (context.provider.clone(), context.serving.is_some());
                let readable = lifecycle::state_readable(&key, &context.transaction);
                let value = if serving && readable {
                    self.world.state(&provider, &key)
                } else {
                    None
                };
                // What the read returns counts against the budget too: a value past it is not handed over.
                if let Some(value) = &value
                    && let Err(reply) =
                        self.admit_request(&[], &[(Metered::StateBytes, value.len() as u64)])
                {
                    return reply;
                }
                self.events.state_reads.push(StateEvent {
                    provider,
                    part: PartName::Outside,
                    key,
                    found: value.is_some(),
                });
                Reply::State { value }
            }
            FromProvider::Egress {
                operation,
                scheme,
                host,
                port,
                method,
                path,
                headers,
                body,
            } => self.egress(
                &operation,
                &scheme,
                host,
                port,
                WireRequest {
                    method,
                    path,
                    headers,
                    body,
                },
            ),
            FromProvider::Module { callee, kind } => self.cross_module(&callee, &kind),
            FromProvider::Ready
            | FromProvider::Failed { .. }
            | FromProvider::Done { .. }
            | FromProvider::Trapped { .. } => Reply::Noted,
        }
    }

    /// An outside request from the sandboxed part: passed to the outside part only if it is for the operation's
    /// transaction and names an outside operation the running declaration declares.
    fn outside_request(&mut self, json: &str) -> Reply {
        let context = self.stack.last_mut().expect("a context");
        let request: Option<OutsideRequest> = serde_json::from_str(json).ok();
        let refusal = match &request {
            None => None,
            Some(r) if r.transaction.as_str() != context.transaction => None,
            Some(r)
                if !context
                    .declaration
                    .outside
                    .operations
                    .iter()
                    .any(|o| o.name == r.operation) =>
            {
                Some(RefusalReason::UndeclaredOperation)
            }
            Some(r) => {
                context.serving = request.clone();
                // The request is in flight for the world too: the credential proxy answers lookups only for it.
                if let Some(caller) = Identifier::new(context.provider.clone()) {
                    self.world.begin_request(
                        &Caller::process_of(caller),
                        &r.transaction,
                        &r.operation,
                    );
                }
                return Reply::Perform;
            }
        };
        let (Some(transaction), Some(operation)) = (
            Identifier::new(context.transaction.clone()),
            request
                .map(|r| r.operation)
                .or_else(|| Name::new("unknown")),
        ) else {
            return Reply::Noted;
        };
        let response = OutsideResponse {
            transaction,
            operation,
            outcome: match refusal {
                Some(reason) => OutsideOutcome::Refused(Refused { reason }),
                None => OutsideOutcome::Refused(Refused {
                    reason: RefusalReason::UndeclaredOperation,
                }),
            },
        };
        Reply::Answer {
            json: serde_json::to_string(&response).unwrap_or_default(),
        }
    }

    /// The outside part's response, passed back to the sandboxed part only if it is a valid outside response to
    /// the request being served; otherwise the sandboxed part is told no answer came.
    fn outside_response(&mut self, json: &str) -> Reply {
        let context = self.stack.last_mut().expect("a context");
        let Some(serving) = context.serving.take() else {
            return Reply::Noted;
        };
        if let Some(caller) = Identifier::new(context.provider.clone()) {
            self.world.end_request(&Caller::process_of(caller));
        }
        let response = serde_json::from_str::<OutsideResponse>(json)
            .ok()
            .filter(|r| r.transaction == serving.transaction && r.operation == serving.operation)
            .unwrap_or(OutsideResponse {
                transaction: serving.transaction,
                operation: serving.operation,
                outcome: OutsideOutcome::Unknown(Nothing {}),
            });
        Reply::Answer {
            json: serde_json::to_string(&response).unwrap_or_default(),
        }
    }

    /// The credential proxy: it answers only from the calling provider's own secrets, only a secret the running
    /// declaration's operation declares, and only for the outside request in flight. The lookup is judged as one
    /// for that request's transaction and operation, which the platform knows from its own state; a lookup naming
    /// another is refused, whatever it names.
    fn lookup(&mut self, transaction: String, operation: String, name: String) -> Reply {
        let context = self.stack.last_mut().expect("a context");
        let provider = context.provider.clone();
        let (Some(serving), Some(name)) = (context.serving.clone(), Name::new(name)) else {
            return Reply::SecretRefused {
                reason: secret_reason(SecretRefusalReason::UndeclaredSecret).to_owned(),
            };
        };
        // The credential proxy's rule (`F-014`), from the platform's own state: the caller is the provider whose
        // process asked, and the request in flight the one passed to its outside part.
        let judged = match (
            Identifier::new(provider.clone()),
            Identifier::new(transaction),
            Name::new(operation),
        ) {
            (Some(caller), Some(transaction), Some(operation)) => proxy::judge(
                &Caller::process_of(caller.clone()),
                &context.declaration,
                Some(&InFlight {
                    provider: caller,
                    transaction: serving.transaction.clone(),
                    operation: serving.operation.clone(),
                }),
                &SecretLookup {
                    transaction,
                    operation,
                    name: name.clone(),
                },
            ),
            _ => Err(proxy::Refusal::NotTheRequestInFlight),
        };
        let lookup = SecretLookup {
            transaction: serving.transaction,
            operation: serving.operation,
            name,
        };
        // The world answers with the platform's verdict in hand: the credential proxy judges again from what the
        // platform told it (`begin_request`), and records the use, refused or not; a refused verdict is never
        // answered with a value.
        let refused = judged.is_err();
        let answered = match Identifier::new(provider.clone()) {
            Some(caller) => self
                .world
                .lookup(&Caller::process_of(caller), &lookup, judged),
            None => Err(SecretRefusalReason::UndeclaredSecret),
        };
        // Whatever a world answers, a lookup the platform refused is handed no value.
        let answered = if refused {
            answered.and(Err(SecretRefusalReason::UndeclaredSecret))
        } else {
            answered
        };
        let mut value = None;
        let outcome = match answered {
            Ok(found) => {
                context
                    .resolved
                    .entry(lookup.operation.as_str().to_owned())
                    .or_default()
                    .push(lookup.name.clone());
                value = Some(found);
                LookupOutcome::Resolved
            }
            Err(reason) => LookupOutcome::Refused(reason),
        };
        self.events.lookups.push(LookupEvent {
            provider: provider.clone(),
            part: PartName::Outside,
            lookup,
            outcome: Some(outcome),
        });
        match (outcome, value) {
            (LookupOutcome::Resolved, Some(value)) => Reply::Secret {
                value: value.as_str().to_owned(),
            },
            (LookupOutcome::Refused(reason), _) => Reply::SecretRefused {
                reason: secret_reason(reason).to_owned(),
            },
            _ => Reply::SecretRefused {
                reason: secret_reason(SecretRefusalReason::UndeclaredSecret).to_owned(),
            },
        }
    }

    /// The egress layer (`F-014`): the call goes through [`World::send`], so through [`Egress`], which checks every
    /// hop's scheme, host and port against the destinations the operation in flight lists, resolves each host once
    /// and dials only the address it checked (`B-17`), refuses a peer not verified over TLS (`B-18`), and writes the
    /// outbound call record, refused calls included (`REQ-MDL-7`, `REQ-MDL-8`). The operation is the outside
    /// request in flight, from the platform's own state: a call naming another is refused as
    /// `undeclared_operation`, and its record names the operation in flight. The call carries a whole request (a
    /// method, a path, headers and a body, `T-013-07`), which the platform builds with [`egress::Request::new`]: that
    /// is the one place that checks the target is an origin-form path and no header is one the connector sets. A
    /// request it refuses has no reason in the contract to be recorded under, so it is refused unrecorded, as an
    /// unknown scheme is. Its bytes count against the call's bound and budget like the body's.
    fn egress(
        &mut self,
        operation: &str,
        scheme: &str,
        host: String,
        port: u16,
        wire: WireRequest,
    ) -> Reply {
        let body = wire.body.as_str();
        let size = body.len()
            + wire.path.len()
            + wire
                .headers
                .iter()
                .map(|h| h.name.len() + h.value.len())
                .sum::<usize>();
        let refused = || Reply::Egress {
            outcome: "refused".to_owned(),
        };
        // The call, its size and its names are judged before anything else, so that a module past its bound is
        // refused and its call recorded as refused for it, never delivered (`REQ-MDL-11`).
        if let Err(reply) = self.admit_request(
            &[
                (LimitKind::NameLength, operation.len(), MAX_NAME),
                (LimitKind::NameLength, host.len(), MAX_HOST),
                (LimitKind::NameLength, scheme.len(), 8),
                (LimitKind::MessageSize, size, MAX_BODY),
                (LimitKind::MessageSize, wire.headers.len(), MAX_HEADERS),
            ],
            &[
                (Metered::OutboundCalls, 1),
                (Metered::OutboundBytes, size as u64),
            ],
        ) {
            self.record_refused_call(scheme, &host, port, RefusalReason::LimitReached);
            return reply;
        }
        let context = self.stack.last().expect("a context");
        // Nothing is in flight, so there is no operation to make the call under or to record it for.
        let Some(in_flight) = context.serving.as_ref().map(|s| s.operation.clone()) else {
            return refused();
        };
        let scheme_name = scheme;
        let scheme = match scheme {
            "https" => Scheme::Https,
            "wss" => Scheme::Wss,
            _ => return refused(),
        };
        // An endpoint no record could name is refused unrecorded, as before.
        let (Some(transaction), true, true) = (
            Identifier::new(context.transaction.clone()),
            Host::new(host.clone()).is_some(),
            port != 0,
        ) else {
            return refused();
        };
        let declaration = context.declaration.clone();
        let due = context.due;
        let secrets = context
            .resolved
            .get(in_flight.as_str())
            .cloned()
            .unwrap_or_default();
        // A call the platform couldn't date isn't made.
        let Ok(started_at) = self.world.now() else {
            return refused();
        };
        let call = Call {
            declaration: &declaration,
            in_flight: &in_flight,
            asked: operation,
            transaction: &transaction,
            secrets: &secrets,
            started_at: &started_at,
            due: Some(due),
        };
        let method = match wire.method.as_str() {
            "get" => Method::Get,
            "post" => Method::Post,
            "put" => Method::Put,
            "patch" => Method::Patch,
            "delete" => Method::Delete,
            _ => {
                self.record_refused_call(
                    scheme_name,
                    &host,
                    port,
                    RefusalReason::RequestNotAllowed,
                );
                return refused();
            }
        };
        let headers = wire
            .headers
            .iter()
            .map(|h| {
                if h.sensitive {
                    egress::Header::sensitive(h.name.as_str(), h.value.as_str())
                } else {
                    egress::Header::plain(h.name.as_str(), h.value.as_str())
                }
            })
            .collect();
        let Ok(request) =
            egress::Request::new(method, wire.path, headers, body.as_bytes().to_vec())
        else {
            self.record_refused_call(scheme_name, &host, port, RefusalReason::RequestNotAllowed);
            return refused();
        };
        // From here the call may reach its destination.
        if let Some(context) = self.stack.last_mut() {
            context.passed_out = true;
        }
        let outcome = self.world.send(
            &call,
            egress::Endpoint { scheme, host, port },
            request,
            &mut self.events.records,
        );
        Reply::Egress {
            outcome: match outcome {
                Outcome::Completed(_) => "completed",
                Outcome::Failed(_) => "failed",
                // A refusal after a hop that delivered the request is told as unknown: that destination may have
                // acted on it (`REQ-ORD-1`).
                Outcome::Unreachable | Outcome::NoAnswer | Outcome::RefusedAfterDelivery(_) => {
                    "unknown"
                }
                Outcome::Refused(_) => "refused",
            }
            .to_owned(),
        }
    }

    /// Writes the outbound call record of a call refused before the egress layer saw it, because its module hit a
    /// bound (`limit_reached`) or its request isn't one the platform sends (`request_not_allowed`): one hop, the
    /// endpoint it asked for, and the refusal; nothing of the request (`REQ-MDL-8`). Nothing is written where no outside request is in
    /// flight, the endpoint isn't one a record can name, or the world's clock gives no time.
    fn record_refused_call(&mut self, scheme: &str, host: &str, port: u16, reason: RefusalReason) {
        let context = self.stack.last().expect("a context");
        let Some(operation) = context.serving.as_ref().map(|s| s.operation.clone()) else {
            return;
        };
        let (Some(host), Some(port), Some(transaction), Some(provider), Some(module_version)) = (
            Host::new(host.to_owned()),
            NonZeroU16::new(port),
            Identifier::new(context.transaction.clone()),
            Identifier::new(context.provider.clone()),
            Identifier::new(context.version.clone()),
        ) else {
            return;
        };
        let secrets = context
            .resolved
            .get(operation.as_str())
            .cloned()
            .unwrap_or_default();
        let scheme = match scheme {
            "wss" => Scheme::Wss,
            _ => Scheme::Https,
        };
        let Ok(started_at) = self.world.now() else {
            return;
        };
        self.events.records.push(OutboundCallRecord {
            transaction,
            provider,
            module_version,
            operation,
            secrets,
            started_at,
            hops: vec![Hop {
                scheme,
                host,
                port,
                destination: None,
                address: None,
            }],
            outcome: CallOutcome::Refused(Refused { reason }),
        });
    }

    /// The cross-module router: a call is made only if the caller's running declaration declares the callee and
    /// the kind of the operation actually forwarded, and the callee's module runs; it is recorded either way
    /// (`REQ-MDL-21`). The kind is judged by `OperationKind::of` the forwarded operation, the one the caller is
    /// handling, never by the kind the guest names: a guest naming another kind, or none the contract knows, is
    /// refused as `undeclared_kind`, and the record names the forwarded kind. The route is judged against
    /// [`Platform::routes`] (`proxy::judge_call`), and the record names the segment that names the call.
    fn cross_module(&mut self, callee: &str, claimed: &str) -> Reply {
        if let Err(reply) = self.admit_request(
            &[
                (LimitKind::NameLength, callee.len(), MAX_NAME),
                (LimitKind::NameLength, claimed.len(), MAX_NAME),
            ],
            &[],
        ) {
            return reply;
        }
        // Counted first, so that no way out below leaves a call uncounted.
        let counted = self.charge(Metered::ModuleCalls, 1);
        let context = self.stack.last().expect("a context");
        let (caller, version, transaction) = (
            context.provider.clone(),
            context.version.clone(),
            context.transaction.clone(),
        );
        let envelope = context.operation.clone();
        let declaration = context.declaration.clone();
        let in_flight = context.serving.is_some();
        let Some(asked) = with_provider(&envelope, callee) else {
            return match counted {
                Err(hit) => self.limit(hit),
                Ok(()) => Reply::Module {
                    outcome: "refused".to_owned(),
                },
            };
        };
        let kind = OperationKind::of(&asked.statement);
        let claimed = serde_json::from_value::<OperationKind>(Value::from(claimed)).ok();
        // The declaration's and the route's checks, the contract's first three, are the proxy's rule (`F-014`).
        let judged = proxy::judge_call(
            &declaration,
            &self.routes,
            &CallAsked {
                caller: &caller,
                transaction: &transaction,
                callee,
                kind,
                claimed,
                in_flight,
            },
        );
        let mut refusal = judged.refusal;
        // Every call is counted, refused ones too, and a call that passed the declaration's checks is bounded in depth.
        // A call over a bound keeps the reason it already has, the first check it failed, or is refused as
        // `limit_reached`; the caller's operation ends as a limit, which is where the bound is recorded
        // (`LimitEvent`).
        let mut limited = false;
        let hit = counted.and_then(|()| {
            if refusal.is_none() {
                depth_allows(&self.bounds, self.stack.len())
            } else {
                Ok(())
            }
        });
        if let Err(hit) = hit {
            self.limit(hit);
            limited = true;
            refusal = refusal.or(Some(CallRefusalReason::LimitReached));
        }
        if refusal.is_none() {
            let running = self
                .providers
                .get(callee)
                .is_some_and(|s| s.active.is_some() && s.process.is_some());
            // The callee runs inside the caller's deadline: with less than it needs left, it is not started
            // (`REQ-MDL-11`). It is refused as `deadline_too_near`, and the caller's operation goes on.
            let left = self.stack.last().map_or(Duration::ZERO, |c| {
                c.due.saturating_duration_since(Instant::now())
            });
            let handling = self.stack.iter().map(|context| context.provider.as_str());
            if reenters(handling, callee) || !running {
                refusal = Some(CallRefusalReason::CalleeUnavailable);
            } else if left < self.call_budget + self.call_reserve() {
                // The callee keeps its whole budget, and the caller its reserve after it.
                refusal = Some(CallRefusalReason::DeadlineTooNear);
            }
        }
        // Only an answer makes a call delivered (`REQ-MDL-21`): the callee answers on its own credentials, in its
        // own process, and a callee that stopped, hit a limit or gave no readable answer leaves the call unanswered.
        let mut answered = false;
        if refusal.is_none() {
            // Passed on: what the callee does from here may take effect.
            if let Some(context) = self.stack.last_mut() {
                context.passed_out = true;
            }
            // On the callee's version in force: the caller's segment is the caller's, and names no version of the
            // callee (`T-013-06`).
            answered = matches!(self.handle_in(callee, &asked, None), Handled::Answered(_));
        }
        // What the caller is told is what is recorded: a call passed on and not answered is unanswered, never
        // refused, since the callee may have acted on it (`REQ-ORD-1`).
        let told = Reply::Module {
            outcome: match (refusal, answered) {
                (Some(_), _) => "refused",
                (None, true) => "delivered",
                (None, false) => "unanswered",
            }
            .to_owned(),
        };
        let (Some(transaction), Some(caller), Some(caller_version), Some(callee_id)) = (
            Identifier::new(transaction),
            Identifier::new(caller),
            Identifier::new(version),
            Identifier::new(callee),
        ) else {
            // Already counted above; over the bound, the operation ends as a limit.
            return if limited { Reply::Limit } else { told };
        };
        let outcome = match refusal {
            Some(reason) => CrossModuleOutcome::Refused(CallRefused {
                callee: callee_id,
                kind,
                reason,
            }),
            None if answered => CrossModuleOutcome::Delivered(Nothing {}),
            None => CrossModuleOutcome::Unanswered(Nothing {}),
        };
        self.events.calls.push(CrossModuleCall {
            transaction,
            caller,
            caller_version,
            route: judged.route,
            operation: asked.statement,
            outcome,
        });
        if limited {
            return Reply::Limit;
        }
        told
    }

    /// Hands the operation to the provider's process and serves its requests until it answers, or until the
    /// operation's deadline, when the process is killed and the operation refused.
    ///
    /// The statement key is had first, once the world knows the operation is being handled and before the process
    /// is handed it: a key the world can't give, or not the one the release declares, refuses the operation as
    /// `StatementKeyUnavailable` or `StatementKeyMisconfigured` with nothing of it run (`REQ-MDL-6`, `THR-4`). It
    /// gives the module's output and the key to sign it with, or why there is none, and whether the module passed
    /// anything out, which [`Platform::refused_after`] judges the refusal by.
    fn run_in_process(
        &mut self,
        provider: &str,
        operation: &Statement<Operation>,
        declaration: Declaration,
        version: String,
    ) -> (Result<(Vec<u8>, SecretValue), PlatformRefusal>, bool) {
        // The process of the version handling it: the one in force, or one beside it (`T-013-06`).
        let Some(mut process) = self.providers.get_mut(provider).and_then(|s| {
            if s.active.as_deref() == Some(version.as_str()) {
                s.process.take()
            } else {
                s.others.remove(&version)
            }
        }) else {
            return (Err(PlatformRefusal::Unavailable), false);
        };
        let transaction = serde_json::to_value(&operation.statement)
            .ok()
            .and_then(|v| v["transaction"].as_str().map(str::to_owned))
            .unwrap_or_default();
        // A cross-module call's operation ends no later than the caller's reserve before the one that made it, so
        // the caller outlives a callee stopped at its deadline and hears that its call went unanswered.
        let own = Instant::now() + self.deadline;
        let reserve = self.call_reserve();
        let due = self.stack.last().map_or(own, |caller| {
            caller
                .due
                .checked_sub(reserve)
                .unwrap_or(caller.due)
                .min(own)
        });
        let caller = Identifier::new(provider.to_owned()).map(Caller::process_of);
        let json = serde_json::to_string(operation).unwrap_or_default();
        // What the world knows the operation by: its transaction, or, for one that names none (a verifier's
        // judgement), an identifier of the platform's own, from the operation's digest. The proxy resolves the
        // statement key only for the operation it was told is being handled.
        let handled_as = Identifier::new(transaction.clone()).or_else(|| {
            Identifier::new(format!("rutent:untransacted:{}", sha256(json.as_bytes())))
        });
        // The world judges against the release that handles it, which the last one told may not be (`T-013-06`).
        self.tell_running(provider, &version);
        if let (Some(caller), Some(handled_as)) = (&caller, &handled_as) {
            self.world.begin_operation(caller, handled_as, due);
        }
        // The statement key, resolved while the module handles the operation, the proxy's condition for it, and
        // before the module is handed it: a store that can't answer, or a key that isn't the declared one's, ends
        // the operation here, before anything of it runs. The module never sees the key.
        // A store that couldn't answer may later; a key that can't sign as declared won't until it is fixed.
        let key = match (&caller, &handled_as) {
            (Some(caller), Some(handled_as)) => {
                match self.world.statement_key(caller, handled_as) {
                    Ok(key)
                        if proxy::check_key(&key, &declaration.outside.statement_key).is_ok() =>
                    {
                        Ok(key)
                    }
                    Err(SecretRefusalReason::StoreUnavailable) => {
                        Err(PlatformRefusal::StatementKeyUnavailable)
                    }
                    _ => Err(PlatformRefusal::StatementKeyMisconfigured),
                }
            }
            _ => Err(PlatformRefusal::StatementKeyMisconfigured),
        };
        let key = match key {
            Ok(key) => key,
            Err(refusal) => {
                if let Some(caller) = &caller {
                    self.world.end_operation(caller);
                }
                self.put_back(provider, &version, process);
                return (Err(refusal), false);
            }
        };
        let ran_as = (provider.to_owned(), version.clone());
        let mut ran = false;
        self.stack.push(Context {
            provider: provider.to_owned(),
            version,
            declaration,
            operation: operation.clone(),
            transaction,
            serving: None,
            resolved: BTreeMap::new(),
            due,
            usage: Usage::default(),
            limited: None,
            passed_out: false,
        });
        let mut outcome = Err(PlatformRefusal::Unavailable);
        let mut alive = process
            .send(&ToProvider::Handle { operation: json })
            .is_ok();
        while alive {
            let received = process.receive_until(due);
            // Anything but the process gone before a word means it took the operation and ran it.
            ran |= !matches!(received, Received::Gone);
            match received {
                Received::Gone => alive = false,
                Received::Overdue => {
                    // Whatever it would still say is dropped with it.
                    self.record_hit(LimitKind::Deadline);
                    self.measurements.overdue += 1;
                    self.measurements.last_trap = Some(format!(
                        "deadline: no answer within {:?}; the process was killed",
                        self.deadline
                    ));
                    alive = false;
                    outcome = Err(PlatformRefusal::LimitReached);
                }
                Received::Message(FromProvider::Done { output }) => {
                    outcome = unhex(&output).ok_or(PlatformRefusal::MalformedOutput);
                    break;
                }
                Received::Message(FromProvider::Trapped { message }) => {
                    // Fuel running out, and a part trapping after the process refused it growth, are limits; any other
                    // trap is the guest's own failure. A bound the platform enforced is judged below, from its own
                    // state.
                    if message == OUTPUT_TOO_LARGE {
                        outcome = Err(PlatformRefusal::OversizedOutput);
                    } else if message.contains("all fuel consumed") {
                        self.record_hit(LimitKind::Fuel);
                        outcome = Err(PlatformRefusal::LimitReached);
                    } else if message.starts_with("limit reached") {
                        // A bound the platform judged is recorded already; otherwise the process refused growth.
                        self.record_hit(LimitKind::Memory);
                        outcome = Err(PlatformRefusal::LimitReached);
                    }
                    self.measurements.last_trap = Some(message);
                    break;
                }
                Received::Message(request) => {
                    let reply = self.serve(request);
                    alive = process.reply(&reply).is_ok();
                }
            }
        }
        if ran {
            *self.runs.entry(ran_as).or_default() += 1;
        }
        let context = self.stack.pop();
        // However the operation ended, nothing of it stays in flight in the world.
        if let Some(caller) = &caller {
            self.world.end_operation(caller);
        }
        if self.stack.is_empty() {
            // The top-level operation ended, and with it every provider's budget inside it.
            self.tree.clear();
        }
        // A bound the platform enforced decides the operation, whatever the process went on to say: the module's
        // answer, if any, is dropped (`REQ-MDL-11`).
        if let Some(hit) = context.as_ref().and_then(|c| c.limited) {
            self.measurements.last_trap = Some(format!("limit reached: {:?}", hit.kind));
            outcome = Err(PlatformRefusal::LimitReached);
        }
        let passed_out = context.as_ref().is_some_and(|c| c.passed_out);
        if alive && process.alive() {
            if let Some(context) = &context {
                self.put_back(provider, &context.version, process);
            }
        } else {
            // The process is gone: this operation is refused, and the platform starts the version again in a new
            // process, in its place, as a supervisor would, touching no other provider and no other version. A
            // callee's is started once the top-level operation ended, never inside its caller's deadline.
            drop(process);
            if let Some(context) = context {
                self.restarts.push((provider.to_owned(), context.version));
            }
        }
        if self.stack.is_empty() {
            for (provider, version) in std::mem::take(&mut self.restarts) {
                self.respawn(&provider, &version);
            }
        }
        (outcome.map(|bytes| (bytes, key)), passed_out)
    }

    /// Records a bound the provider's process hit on its own, its deadline, its fuel or its memory, for the
    /// operation on top of the stack, unless a bound the platform judged was recorded for it already
    /// (`REQ-MDL-11`).
    fn record_hit(&mut self, kind: LimitKind) {
        if let Some(context) = self.stack.last()
            && context.limited.is_none()
        {
            self.events.limits.push(LimitEvent {
                provider: context.provider.clone(),
                transaction: context.transaction.clone(),
                hit: LimitHit { kind, scope: None },
            });
        }
    }

    /// How an operation that gave no answer is refused: for `refusal`, where its module passed nothing out, and
    /// as `OutcomeUnknown` once it passed something out, an outbound or a cross-module call, with what ended it
    /// recorded (`REQ-ORD-1`).
    fn refused_after(
        &mut self,
        provider: &str,
        transaction: Option<&str>,
        refusal: PlatformRefusal,
        passed_out: bool,
    ) -> Handled {
        let told = told_after(refusal, passed_out);
        if told != refusal {
            self.events.unknown.push(UnknownOutcome {
                provider: provider.to_owned(),
                transaction: transaction.unwrap_or_default().to_owned(),
                ended_by: refusal,
            });
        }
        Handled::Refused(told)
    }

    /// The least of the caller's deadline kept back from a callee, so that a callee stopped at its own deadline
    /// leaves its caller time to hear that the call went unanswered and to answer: half the call budget.
    fn call_reserve(&self) -> Duration {
        self.call_budget / 2
    }

    /// Puts the process of the provider's `version` back in its place: the version in force's, or beside it.
    fn put_back(&mut self, provider: &str, version: &str, process: ProviderProcess) {
        if let Some(slot) = self.providers.get_mut(provider) {
            if slot.active.as_deref() == Some(version) {
                slot.process = Some(process);
            } else {
                slot.others.insert(version.to_owned(), process);
            }
        }
    }
}

/// Whether a call to `callee` would re-enter a provider already `handling` an operation: the platform's stack, from
/// the top-level operation's provider to the caller. That is the caller itself, or one further down the chain (A
/// calls B, B calls A). Such a call is refused as `callee_unavailable` on purpose, before the callee's process is
/// looked at, not only because that process is busy (`T-014-04`, `T-014-05`): one provider handles one operation
/// at a time, and the credential proxy keeps one operation and one request in flight per provider, so a
/// re-entrant operation would end the outer one's request in the proxy.
fn reenters<'a>(handling: impl IntoIterator<Item = &'a str>, callee: &str) -> bool {
    handling.into_iter().any(|provider| provider == callee)
}

/// `envelope` asked of `callee`: the same operation, its `provider` the callee.
fn with_provider(envelope: &Statement<Operation>, callee: &str) -> Option<Statement<Operation>> {
    let mut value = serde_json::to_value(envelope).ok()?;
    value["statement"]["provider"] = Value::from(callee);
    serde_json::from_value(value).ok()
}

/// The wire reason of a secret refusal, each the contract's own: `store_unavailable` is a store that couldn't
/// answer, never `not_found` (module-interface 4.0.0, `T-010-09`), and a reason this platform doesn't know is
/// `undeclared_secret`.
fn secret_reason(reason: SecretRefusalReason) -> &'static str {
    match reason {
        SecretRefusalReason::NotFound => "not_found",
        SecretRefusalReason::StoreUnavailable => "store_unavailable",
        SecretRefusalReason::UndeclaredSecret => "undeclared_secret",
        _ => "undeclared_secret",
    }
}

fn digest(hex: &str) -> Digest {
    Digest {
        algorithm: DigestAlgorithm::Sha256,
        value: DigestValue::new(hex.to_owned()).expect("a digest"),
    }
}

/// What an operation that gave no answer is refused as: `refusal` where its module passed nothing out, so nothing of
/// it took effect, and `OutcomeUnknown` once it passed something out (`REQ-ORD-1`).
fn told_after(refusal: PlatformRefusal, passed_out: bool) -> PlatformRefusal {
    if passed_out {
        PlatformRefusal::OutcomeUnknown
    } else {
        refusal
    }
}

/// Why a module's answer that couldn't be signed is refused. The key was checked before the module ran, so a key
/// that doesn't sign now is the key's; anything else is the module's output nesting a statement the operation
/// didn't ask for (`REQ-MDL-12`).
fn unsigned_refusal(why: proxy::SigningError) -> PlatformRefusal {
    match why {
        proxy::SigningError::MalformedKey | proxy::SigningError::NotTheDeclaredKey => {
            PlatformRefusal::StatementKeyMisconfigured
        }
        _ => PlatformRefusal::MalformedOutput,
    }
}

/// `at` as a record's time, or `None` past the year 9999, which no record's time can hold. Every time the platform
/// records is the caller's clock or the world's ([`World::now`]), never one of its own.
fn time_of(at: Clock) -> Option<Time> {
    at.rfc3339().and_then(Time::new)
}

/// The transaction an operation names, if it names one.
fn transaction_of(operation: &Operation) -> Option<String> {
    serde_json::to_value(operation)
        .ok()
        .and_then(|v| v["transaction"].as_str().map(str::to_owned))
}

/// Whether `next`'s assertion was made before `previous`'s, or either time can't be read: an older signed release
/// (`THR-17`).
fn asserted_before(next: &AdmissionArtefact, previous: &AdmissionArtefact) -> bool {
    match (
        Stamp::parse(&next.assertion.made_at),
        Stamp::parse(&previous.assertion.made_at),
    ) {
        (Some(next), Some(previous)) => next < previous,
        _ => true,
    }
}

fn result_matches(result: &ResultBody, operation: &Operation) -> bool {
    let (Ok(result), Ok(operation)) = (
        serde_json::to_value(result),
        serde_json::to_value(operation),
    ) else {
        return false;
    };
    result["kind"] == operation["kind"] && result["transaction"] == operation["transaction"]
}

fn unique() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// A well-formed placeholder for a 64-byte signature value, replaced when the result is signed.
fn placeholder_signature() -> String {
    format!("{}A", "s".repeat(85))
}

impl<W: World> Drop for Platform<W> {
    fn drop(&mut self) {
        // Every provider's process is killed and reaped as its slot drops.
        self.providers.clear();
        let _ = std::fs::remove_dir_all(&self.workdir);
    }
}

impl<W: World> Platform<W> {
    /// The portable binary format this platform admits, as release assertions name it.
    pub fn format(&self) -> String {
        FORMAT.to_owned()
    }

    /// Sets the largest output, in bytes as a module emits them, the platform passes on.
    pub fn set_output_limit(&mut self, bytes: usize) {
        self.output_limit = bytes;
    }

    /// Admits a release: the provider's signed assertion, as the bytes it arrived in, and the binaries it received
    /// (`REQ-MDL-1`). The assertion is read from those bytes and judged by `judge` at the caller's clock `at`; its
    /// digest is of those bytes. The assertion, the binaries against the digests it states, their format and size,
    /// and a release of the provider admitted under that version or waiting for an acceptance, under any, are
    /// judged before the admitting compiler runs. After it, only the compiler's refusal and the judge's refusal to
    /// record the declared statement key remain; the catalogue's can't happen once its judgement passed, since the
    /// accepted advertisement lists that key for statements. Each provider's history is
    /// kept, and a release that needs only an automatic acceptance is started, its acceptance recorded at `at`. An
    /// `at` past what a record's time can hold (the year 9999) admits nothing (`Admitted::ClockOutOfRange`).
    pub fn admit(
        &mut self,
        received_assertion: &[u8],
        binaries: &Parts<Vec<u8>>,
        judge: &mut impl ReleaseJudge,
        at: Clock,
    ) -> Admitted {
        // An automatic acceptance is recorded at the caller's clock: one no record's time can hold admits nothing.
        let Some(admitted_at) = time_of(at) else {
            return Admitted::ClockOutOfRange;
        };
        let assertion = match admission::read(received_assertion, &self.envelope) {
            Ok(assertion) => assertion,
            Err(why) => return Admitted::Unverified(why),
        };
        let received = Parts {
            sandboxed: digest(&sha256(&binaries.sandboxed)),
            outside: digest(&sha256(&binaries.outside)),
        };
        let declaration = assertion.statement.declaration.clone();
        let provider = declaration.provider.as_str().to_owned();
        let version = declaration.version.as_str().to_owned();
        // The release it follows is the version in force, or else the newest neither revoked nor declared exploited:
        // never one rolled back or exploited, so a release isn't spared a fresh acceptance by widening what was
        // withdrawn from use (`REQ-MDL-3`, `T-013-06`). The newest accepted, whatever it became, still bounds how old
        // an assertion may be, and every statement key the provider's releases declared stays unusable for signing.
        let latest = self
            .providers
            .get(&provider)
            .and_then(|slot| slot.accepted.last().cloned());
        let previous = self.release_followed(&provider);
        let widenings = previous
            .as_ref()
            .map(|p| widening(p.declaration(), &declaration))
            .unwrap_or_default();
        let fresh = previous.is_none() || !widenings.is_empty();
        let history = previous
            .as_ref()
            .map(|p| {
                let mut history = p.history.clone();
                history.extend(p.as_history_entry());
                history
            })
            .unwrap_or_default();
        let Some(platform_version) = Identifier::new(engine::platform_version()) else {
            return Admitted::Refused(AdmissionRefusal::UnreadableBinary);
        };
        // The artefact as it will be recorded, its machine code filled in once compiled.
        let mut artefact = AdmissionArtefact {
            assertion,
            assertion_digest: digest(&sha256(received_assertion)),
            received: received.clone(),
            build: Build {
                platform_version,
                machine_code: received.clone(),
            },
            history,
            capabilities: Capabilities {
                compared_with: previous.as_ref().map(|p| p.declaration().version.clone()),
                widenings,
            },
            acceptance: Acceptance {
                required: if fresh {
                    AcceptanceMode::Fresh
                } else {
                    AcceptanceMode::Automatic
                },
                accepted: None,
            },
        };
        // Who vouches for it: the signature and the keys, against the advertisement the judge accepted.
        if let Err(why) = judge.judge(received_assertion, &artefact, at) {
            return Admitted::Unverified(why);
        }
        let asserted = &artefact.assertion.statement.parts;
        if received.sandboxed != asserted.sandboxed.digest {
            return Admitted::Refused(AdmissionRefusal::PartAltered(PartName::Sandboxed));
        }
        if received.outside != asserted.outside.digest {
            return Admitted::Refused(AdmissionRefusal::PartAltered(PartName::Outside));
        }
        let format_ok = [&asserted.sandboxed.format, &asserted.outside.format]
            .iter()
            .all(|f| f.as_str() == FORMAT);
        if !format_ok
            || binaries.sandboxed.len() > MAX_BINARY
            || binaries.outside.len() > MAX_BINARY
        {
            return Admitted::Refused(AdmissionRefusal::UnreadableBinary);
        }
        // A version already admitted, accepted, waiting or withdrawn, is never admitted again: its machine code and
        // artefact stay what they are (`VersionAlreadyAdmitted`). While a release waits for its acceptance, no other
        // is admitted, under any version: the one waiting is what its acceptance is given to (`AdmissionPending`).
        if let Some(slot) = self.providers.get(&provider) {
            let named = slot
                .accepted
                .iter()
                .chain(slot.pending.as_ref().map(|(a, _, _)| a))
                .any(|a| a.declaration().version.as_str() == version)
                || slot.withdrawn.contains(&version);
            if named {
                return Admitted::Refused(AdmissionRefusal::VersionAlreadyAdmitted);
            }
            if slot.pending.is_some() {
                return Admitted::Refused(AdmissionRefusal::AdmissionPending);
            }
        }
        // While a release is staged, no other is admitted: its staging ends first, promoted or rolled back
        // (ADR-0014). And a release asserted before the provider's last accepted one is an older signed release,
        // which publishing never reinstates (`REQ-MDL-4`, `THR-17`).
        if self.lifecycle.staged(&provider).is_some() {
            return Admitted::Refused(AdmissionRefusal::ReleaseStaged);
        }
        if let Some(latest) = &latest
            && asserted_before(&artefact, latest)
        {
            return Admitted::Refused(AdmissionRefusal::OlderRelease);
        }
        let signed_with = &artefact.assertion.signature.key;
        if self.providers.get(&provider).is_some_and(|slot| {
            slot.accepted
                .iter()
                .any(|a| &a.declaration().outside.statement_key == signed_with)
        }) {
            return Admitted::Unverified(Unverified::Release(vec![
                RuleViolation::AssertionSignedWithPreviousStatementKey,
            ]));
        }
        // The contract's rules on the release, and against the release it follows: an assertion signed with a
        // statement key an earlier release declared is refused (`THR-4`).
        if let Err(violations) = validate_artefact(&artefact) {
            return Admitted::Unverified(Unverified::Release(violations));
        }
        if let Some(previous) = &previous
            && let Err(violations) = validate_release(previous, &artefact)
        {
            return Admitted::Unverified(Unverified::Release(violations));
        }
        let machine = match compile(
            &self.paths,
            &self.workdir,
            &binaries.sandboxed,
            &binaries.outside,
            &self.stderrs,
            self.compile_deadline,
        ) {
            Ok(machine) => Rc::new(machine),
            Err(CompileError::Overdue(pid)) => {
                // Killed and reaped: nothing of this admission is kept, and the same release may be admitted again.
                keep_last(
                    &mut self.measurements.overdue_compilers,
                    pid,
                    self.measurement_cap,
                );
                return Admitted::Refused(AdmissionRefusal::UnreadableBinary);
            }
            Err(CompileError::Refused {
                part,
                disallowed_import: true,
                ..
            }) => {
                let part = if part == "sandboxed" {
                    PartName::Sandboxed
                } else {
                    PartName::Outside
                };
                return Admitted::Refused(AdmissionRefusal::DisallowedImport(part));
            }
            Err(_) => return Admitted::Refused(AdmissionRefusal::UnreadableBinary),
        };
        keep_last(
            &mut self.measurements.compiles,
            (
                machine.took,
                [binaries.sandboxed.len(), binaries.outside.len()],
                [machine.sandboxed.len(), machine.outside.len()],
            ),
            self.measurement_cap,
        );
        artefact.build.machine_code = Parts {
            sandboxed: digest(&sha256(&machine.sandboxed)),
            outside: digest(&sha256(&machine.outside)),
        };
        // From now on the declared statement key is a statements key everywhere (`THR-4`).
        if let Err(why) = judge.record_statement_key(&declaration.outside.statement_key) {
            return Admitted::Unverified(why);
        }
        if !fresh {
            artefact.acceptance.accepted = Some(Accepted {
                by: None,
                at: admitted_at,
            });
        }
        let slot = self.providers.entry(provider.clone()).or_default();
        if fresh {
            slot.pending = Some((artefact.clone(), machine, received_assertion.to_vec()));
        } else {
            slot.accepted.push(artefact.clone());
            slot.machines.insert(version.clone(), machine);
            self.lifecycle.accept(&provider, &version);
            self.launch(&provider, &version, at);
        }
        Admitted::Artefact(Box::new(artefact))
    }

    /// Gives a fresh acceptance, by `by`, to the provider's release that waits for one, and starts it. The
    /// acceptance is of one assertion: `assertion_digest` must be the waiting artefact's, the digest of its
    /// assertion as received (`REQ-MDL-1`). That assertion is judged again by `judge` at the caller's clock `at`,
    /// so a release signed with a key retired since its admission is no longer accepted (`THR-4`), and the
    /// acceptance is recorded at `at`. The accepted artefact, or `None` if no release waits under that digest, `by`
    /// names no one, the assertion no longer passes, or `at` is past what a record's time can hold (the year 9999);
    /// the waiting release then stays as it was.
    pub fn accept(
        &mut self,
        provider: &str,
        assertion_digest: &Digest,
        by: &str,
        judge: &impl ReleaseJudge,
        at: Clock,
    ) -> Option<AdmissionArtefact> {
        let by = Identifier::new(by)?;
        // Recorded at the caller's clock, the time the acceptance was given (`REQ-AUD-1`).
        let accepted_at = time_of(at)?;
        let (artefact, _, received) = self.providers.get(provider)?.pending.as_ref()?;
        if &artefact.assertion_digest != assertion_digest
            || judge.judge(received, artefact, at).is_err()
        {
            return None;
        }
        let slot = self.providers.get_mut(provider)?;
        let (mut artefact, machine, _) = slot.pending.take()?;
        let version = artefact.declaration().version.as_str().to_owned();
        artefact.acceptance.accepted = Some(Accepted {
            by: Some(by),
            at: accepted_at,
        });
        slot.accepted.push(artefact.clone());
        slot.machines.insert(version.clone(), machine);
        self.lifecycle.accept(provider, &version);
        self.launch(provider, &version, at);
        Some(artefact)
    }

    /// The provider's release waiting for a fresh acceptance, if one waits.
    pub fn pending(&self, provider: &str) -> Option<&AdmissionArtefact> {
        self.providers
            .get(provider)?
            .pending
            .as_ref()
            .map(|(a, _, _)| a)
    }

    /// The version of the provider's module now running, if any.
    pub fn active_version(&self, provider: &str) -> Option<String> {
        self.providers.get(provider)?.active.clone()
    }

    /// The operating-system process the provider's module runs in, if it runs.
    pub fn process(&self, provider: &str) -> Option<ProcessId> {
        let slot = self.providers.get(provider)?;
        slot.process.as_ref().map(|p| ProcessId::from(p.pid()))
    }

    /// Restarts the provider's process, and no other. False if it has none.
    pub fn restart(&mut self, provider: &str) -> bool {
        let Some(version) = self
            .providers
            .get(provider)
            .filter(|s| s.process.is_some())
            .and_then(|s| s.active.clone())
        else {
            return false;
        };
        // Stop this provider's process only, then start its running version again from its admitted machine code.
        if let Some(slot) = self.providers.get_mut(provider) {
            slot.process = None;
        }
        self.respawn(provider, &version);
        self.process(provider).is_some()
    }

    /// How many operations the provider's module received, from the platform or from another module.
    pub fn operations_received(&self, provider: &str) -> usize {
        self.operations_received
            .get(provider)
            .copied()
            .unwrap_or_default()
    }

    /// Hands one provider-role operation to the provider's running module. Its answer is passed on only checked
    /// against the result schema, and signed with the provider's statement key, which the world resolves once the
    /// operation began and before the module is handed it (`T-014-05`): a store that can't answer refuses the
    /// operation as `StatementKeyUnavailable`, and a key that can't sign as declared as `StatementKeyMisconfigured`,
    /// with nothing of it run. Once the module passed something out, an outbound or a cross-module call, whatever
    /// then ends the operation without an answer, a limit, a bad output, a stop or a result that can't be signed,
    /// is `OutcomeUnknown`, and what ended it is in [`Events::unknown`] (`REQ-ORD-1`).
    ///
    /// It runs on the version its segment runs on, if a segment began for its transaction
    /// ([`Platform::begin_segment`]), and on the version in force otherwise. It is refused as `DuplicateTransaction`
    /// where its transaction's segment is another provider's or has ended, and as `VersionExploited` where its
    /// segment's version was declared exploited (`T-013-06`).
    pub fn handle(&mut self, provider: &str, operation: &Statement<Operation>) -> Handled {
        let transaction = transaction_of(&operation.statement);
        self.handle_in(provider, operation, transaction.as_deref())
    }

    /// [`Platform::handle`], in the segment of `transaction` if one holds it, and on the version in force for
    /// `None`.
    fn handle_in(
        &mut self,
        provider: &str,
        operation: &Statement<Operation>,
        transaction: Option<&str>,
    ) -> Handled {
        *self
            .operations_received
            .entry(provider.to_owned())
            .or_default() += 1;
        let version = match self.lifecycle.version_for(provider, transaction) {
            Ok(version) => version,
            Err(refusal) => return Handled::Refused(refusal),
        };
        let Some(declaration) = self.declaration_of(provider, &version) else {
            return Handled::Refused(PlatformRefusal::Unavailable);
        };
        // Only an operation the version's process ran counts towards a staging: not one refused as not handled,
        // nor one that found its process gone.
        let key = (provider.to_owned(), version.clone());
        let before = self.runs.get(&key).copied().unwrap_or_default();
        let handled = self.handle_on(provider, operation, declaration, version.clone());
        if self.runs.get(&key).copied().unwrap_or_default() > before {
            self.lifecycle
                .observe(provider, &version, matches!(handled, Handled::Answered(_)));
        }
        handled
    }

    /// [`Platform::handle`] on `version`, whose declaration is `declaration`.
    fn handle_on(
        &mut self,
        provider: &str,
        operation: &Statement<Operation>,
        declaration: Declaration,
        version: String,
    ) -> Handled {
        if !declaration
            .sandboxed
            .handles
            .contains(&OperationKind::of(&operation.statement))
        {
            return Handled::Refused(PlatformRefusal::NotHandled);
        }
        let statement_key = declaration.outside.statement_key.clone();
        let transaction = transaction_of(&operation.statement);
        let (ran, passed_out) = self.run_in_process(provider, operation, declaration, version);
        let (bytes, private) = match ran {
            Ok(answered) => answered,
            Err(refusal) => {
                return self.refused_after(provider, transaction.as_deref(), refusal, passed_out);
            }
        };
        // The output is untrusted: judged against the result schema, patterns and lengths included (`REQ-MDL-12`).
        let result = match output::check(&bytes, self.output_limit) {
            Ok(result) => result,
            Err(refusal) => {
                return self.refused_after(provider, transaction.as_deref(), refusal, passed_out);
            }
        };
        if !result_matches(&result, &operation.statement) {
            return self.refused_after(
                provider,
                transaction.as_deref(),
                PlatformRefusal::MalformedOutput,
                passed_out,
            );
        }
        let unsigned = Statement {
            // The deployment and protocol version are the platform's own: a result never echoes an operation's
            // envelope, which may be another deployment's or another version's (`REQ-PRT-1`).
            deployment: self.envelope.deployment.clone(),
            protocol_version: self.envelope.protocol_version.clone(),
            shape: RESULT_SHAPE.to_owned(),
            maker: provider.to_owned(),
            made_at: operation.made_at.clone(),
            signature: Signature {
                scheme: SignatureScheme::Ed25519,
                key: statement_key.clone(),
                value: placeholder_signature(),
            },
            statement: result,
        };
        // Signed with the provider's statement key, held through the credential proxy and never by the module
        // (`F-014`): the result and what the module nested in it in the provider's name. The key was judged before
        // the module ran; a result that still can't be signed, because a nested statement isn't what the operation
        // asked, in this deployment and protocol version, is malformed output, and isn't passed on.
        match proxy::sign_result(
            &unsigned,
            &proxy::Answering {
                provider,
                deployment: &self.envelope.deployment,
                protocol_version: &self.envelope.protocol_version,
                operation: &operation.statement,
            },
            &private,
            &statement_key,
        ) {
            Ok(signed) => Handled::Answered(Box::new(signed)),
            Err(why) => self.refused_after(
                provider,
                transaction.as_deref(),
                unsigned_refusal(why),
                passed_out,
            ),
        }
    }

    /// Sets how upgrades are staged (ADR-0014): a policy, [`lifecycle::DEFAULT_STAGING`] at first, stages an
    /// accepted release beside the version in force, on a share of new segments, until the operator promotes it or
    /// rolls it back; `None` puts an accepted release in force once it starts.
    pub fn set_staging(&mut self, staging: Option<Staging>) {
        self.lifecycle.set_staging(staging);
    }

    /// Each provider's lifecycle: the version in force, the staged one and what was observed of it, the operator's
    /// marks, the segments in flight and the decisions recorded.
    pub fn lifecycle(&self) -> &Lifecycle {
        &self.lifecycle
    }

    /// The process the provider's `version` runs in, in force or beside it.
    pub fn version_process(&self, provider: &str, version: &str) -> Option<ProcessId> {
        let slot = self.providers.get(provider)?;
        if slot.active.as_deref() == Some(version) {
            slot.process.as_ref()
        } else {
            slot.others.get(version)
        }
        .map(|p| ProcessId::from(p.pid()))
    }

    /// Begins the segment of `transaction` at the provider (`REQ-MDL-5`, `REQ-MDL-14`): it runs on the version this
    /// returns, the staged one for its share and the one in force otherwise, until it ends, unless the operator
    /// marks that version stale. A transaction a segment holds or held is refused as `DuplicateTransaction`
    /// (`INV-8`), and a provider with no version in force as `Unavailable`.
    pub fn begin_segment(
        &mut self,
        provider: &str,
        transaction: &Identifier,
    ) -> Result<String, PlatformRefusal> {
        self.lifecycle.begin_segment(provider, transaction)
    }

    /// Ends the segment of `transaction` and hands over its record: the version it began on, the one it ran on and
    /// each move between them. Its transaction never begins another, and a version nothing runs on any more stops.
    pub fn end_segment(&mut self, transaction: &str) -> Option<SegmentRecord> {
        let record = self.lifecycle.end_segment(transaction)?;
        self.reconcile(&record.provider);
        Some(record)
    }

    /// Withdraws, by `by` at the caller's clock `at`, the provider's release that waits for its fresh acceptance
    /// under `assertion_digest`, accepted or not since: one refused on being judged again stays waiting until it
    /// is withdrawn. The provider's next release may then be admitted, under another version. The withdrawn
    /// artefact, or `None` if no release waits under that digest or `by` names no one.
    pub fn withdraw(
        &mut self,
        provider: &str,
        assertion_digest: &Digest,
        by: &str,
        at: Clock,
    ) -> Option<AdmissionArtefact> {
        Identifier::new(by)?;
        let slot = self.providers.get_mut(provider)?;
        if &slot.pending.as_ref()?.0.assertion_digest != assertion_digest {
            return None;
        }
        let (artefact, _, _) = slot.pending.take()?;
        let version = artefact.declaration().version.as_str().to_owned();
        slot.withdrawn.insert(version.clone());
        self.lifecycle.withdrawn(provider, &version, by, at).ok()?;
        Some(artefact)
    }

    /// Puts the provider's staged release in force, by `by` at the caller's clock `at`, once its time window has
    /// passed and it has handled its minimum (`REQ-MDL-4`, ADR-0014). Segments in flight stay where they run.
    /// Refused as `StagedNotRunning` while the staged version has no live process, so a promotion never leaves the
    /// provider with no version in force.
    pub fn promote(
        &mut self,
        provider: &str,
        by: &str,
        at: Clock,
    ) -> Result<String, LifecycleRefusal> {
        if let Some(staged) = self.lifecycle.staged(provider).map(str::to_owned) {
            let live = self
                .providers
                .get_mut(provider)
                .and_then(|slot| slot.others.get_mut(&staged))
                .is_some_and(ProviderProcess::alive);
            if !live {
                return Err(LifecycleRefusal::StagedNotRunning);
            }
        }
        let promoted = self.lifecycle.promote(provider, by, at)?;
        self.reconcile(provider);
        Ok(promoted)
    }

    /// Rolls the provider's staged release back, by `by`: the version in force stays, and segments on the staged
    /// one finish on it.
    pub fn roll_back(
        &mut self,
        provider: &str,
        by: &str,
        at: Clock,
    ) -> Result<String, LifecycleRefusal> {
        let rolled_back = self.lifecycle.roll_back(provider, by, at)?;
        self.reconcile(provider);
        Ok(rolled_back)
    }

    /// Revokes the provider's `version`, by `by`: it takes no new segment, and those in flight on it finish on it
    /// (`REQ-MDL-20`). Revoking the version in force puts the one before it back in force, a recorded decision; the
    /// version put back, if any.
    pub fn revoke(
        &mut self,
        provider: &str,
        version: &str,
        by: &str,
        at: Clock,
    ) -> Result<Option<String>, LifecycleRefusal> {
        let back = self.lifecycle.revoke(provider, version, by, at)?;
        self.reconcile(provider);
        Ok(back)
    }

    /// Declares the provider's `version` actively exploited, by `by`: its process is stopped and its segments are
    /// refused as `VersionExploited`, not moved (`REQ-MDL-20`). The version put back in force, if any.
    pub fn declare_exploited(
        &mut self,
        provider: &str,
        version: &str,
        by: &str,
        at: Clock,
    ) -> Result<Option<String>, LifecycleRefusal> {
        let back = self
            .lifecycle
            .declare_exploited(provider, version, by, at)?;
        self.reconcile(provider);
        Ok(back)
    }

    /// Marks the provider's `version` stale, by `by`, the operator's decision and never the publisher's: its
    /// segments in flight move to the version in force, each move recorded in its segment's record (`REQ-MDL-5`,
    /// `REQ-MDL-19`). Refused while a release is staged (ADR-0014). How many segments moved.
    pub fn mark_stale(
        &mut self,
        provider: &str,
        version: &str,
        by: &str,
        at: Clock,
    ) -> Result<usize, LifecycleRefusal> {
        let moved = self.lifecycle.mark_stale(provider, version, by, at)?;
        self.reconcile(provider);
        Ok(moved)
    }

    /// What the platform saw since it was last asked: outbound call records, secret lookups, state reads and
    /// cross-module calls.
    pub fn take_events(&mut self) -> Events {
        std::mem::take(&mut self.events)
    }
}

#[cfg(test)]
mod tests {
    use super::{PlatformRefusal, proxy, reenters, told_after, unsigned_refusal};

    /// Every reason an operation with no answer could end for is told as itself only where its module passed
    /// nothing out; once it passed something out, every one is an outcome unknown (`REQ-ORD-1`).
    // verifies: REQ-ORD-1
    #[test]
    fn every_refusal_after_the_module_passed_something_out_is_an_outcome_unknown() {
        let reasons = [
            PlatformRefusal::MalformedOutput,
            PlatformRefusal::OversizedOutput,
            PlatformRefusal::LimitReached,
            PlatformRefusal::Unavailable,
            PlatformRefusal::StatementKeyMisconfigured,
        ];
        for reason in reasons {
            assert_eq!(told_after(reason, false), reason, "{reason:?}");
            assert_eq!(
                told_after(reason, true),
                PlatformRefusal::OutcomeUnknown,
                "{reason:?}"
            );
        }
    }

    /// A result that can't be signed although its key was checked before the module ran nests a statement the
    /// operation didn't ask for: malformed output, never `Unavailable`, which would say no module runs. A key that
    /// doesn't sign is the key's misconfiguration (`REQ-MDL-12`, `THR-4`).
    // verifies: REQ-MDL-12, THR-4, REQ-ORD-1
    #[test]
    fn a_result_that_cant_be_signed_is_refused_for_its_output_or_its_key_never_as_unavailable() {
        use proxy::SigningError::*;
        for why in [
            NestedForAnotherDeployment,
            NestedOfAnotherVersion,
            NestedOfAnotherShape,
            NestedNotAsked,
            NoSigningInput,
        ] {
            assert_eq!(
                unsigned_refusal(why),
                PlatformRefusal::MalformedOutput,
                "{why:?}"
            );
        }
        for why in [MalformedKey, NotTheDeclaredKey] {
            assert_eq!(
                unsigned_refusal(why),
                PlatformRefusal::StatementKeyMisconfigured,
                "{why:?}"
            );
        }
        // After the module passed something out, either is an outcome unknown.
        assert_eq!(
            told_after(unsigned_refusal(NestedNotAsked), true),
            PlatformRefusal::OutcomeUnknown
        );
    }

    /// A call back into the caller itself, or into a provider further down the chain of operations, re-enters it,
    /// and is refused as `callee_unavailable` whatever its process's state; a provider not on the stack doesn't.
    // verifies: REQ-MDL-21, REQ-MDL-9
    #[test]
    fn a_call_into_any_provider_already_handling_an_operation_re_enters_it() {
        let stack = ["alpha", "beta", "gamma"];
        assert!(reenters(stack, "gamma"), "the caller itself");
        assert!(reenters(stack, "beta"), "the caller's caller");
        assert!(
            reenters(stack, "alpha"),
            "the top-level operation's provider"
        );
        assert!(!reenters(stack, "delta"), "a provider not on the stack");
        assert!(!reenters([], "alpha"), "no operation in flight");
    }
}
