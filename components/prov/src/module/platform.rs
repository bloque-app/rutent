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
use std::collections::BTreeMap;
use std::net::IpAddr;
use std::num::NonZeroU16;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use rutent_contracts::module_interface::{
    Acceptance, AcceptanceMode, Accepted, AdmissionArtefact, Build, CallOutcome, CallRefusalReason,
    CallRefused, Capabilities, CrossModuleCall, CrossModuleOutcome, Declaration, Digest,
    DigestAlgorithm, DigestValue, Hop, Host, Identifier, Name, Nothing, OperationKind,
    OutboundCallRecord, OutsideOutcome, OutsideRequest, OutsideResponse, PartName, Parts,
    RefusalReason, Refused, Scheme, SecretLookup, SecretRefusalReason, SecretValue, Time,
    validate_artefact, validate_release, widening,
};
use rutent_contracts::provider_role::{
    Operation, RESULT_SHAPE, ResultBody, Signature, SignatureScheme, Statement,
};
use serde_json::Value;

use crate::catalogue::Instant as Clock;
use crate::egress::{
    self, Call, ConnectError, Connector, Dial, Egress, Method, NameResolver, Outcome,
};
use crate::module::admission::{self, ReleaseJudge, Unverified};
pub use crate::module::children::COMPILE_DEADLINE;
use crate::module::children::{
    CompileError, Machine, Paths, ProviderProcess, Received, Stderr, Stderrs, compile,
};
use crate::module::engine::{self, FORMAT, MAX_BINARY};
use crate::module::limits::{
    Bounds, LimitHit, LimitKind, MAX_BODY, MAX_HEADERS, MAX_HOST, MAX_KEY, MAX_MEASUREMENTS,
    MAX_NAME, MAX_OUTSIDE_MESSAGE, Metered, Usage, charge, depth_allows, keep_last,
    within as within_bound,
};
use crate::module::output;
use crate::module::types::{
    AdmissionRefusal, Admitted, Endpoint, Envelope, Events, Handled, LimitEvent, LookupEvent,
    LookupOutcome, PlatformRefusal, ProcessId, StateEvent,
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

/// The least of the calling operation's deadline that must be left for the platform to hand an operation to
/// another provider's module (`REQ-MDL-11`, `REQ-MDL-21`). A callee started with less would be killed at the
/// caller's deadline before it could answer, and the caller's own answer would be lost with it.
pub const MIN_CALL_BUDGET: Duration = Duration::from_secs(1);

/// One provider's releases and its running process.
#[derive(Default)]
struct Slot {
    accepted: Vec<AdmissionArtefact>,
    /// The release waiting for a fresh acceptance, its machine code, and its assertion as received, judged again
    /// when it is accepted.
    pending: Option<(AdmissionArtefact, Rc<Machine>, Vec<u8>)>,
    machines: BTreeMap<String, Rc<Machine>>,
    active: Option<String>,
    process: Option<ProviderProcess>,
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
/// - [`World::statement_key`] refuses, so a world with no key has none of its results passed on;
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
    /// operation for `transaction`. The default holds none.
    fn statement_key(
        &mut self,
        caller: &Caller,
        transaction: &Identifier,
    ) -> Result<SecretValue, SecretRefusalReason> {
        let _ = (caller, transaction);
        Err(SecretRefusalReason::UndeclaredSecret)
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
    clock: u32,
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
            clock: 0,
            events: Events::default(),
            deadline: OPERATION_DEADLINE,
            stderrs: Stderrs::default(),
            bounds: Bounds::DEFAULT,
            tree: BTreeMap::new(),
            call_budget: MIN_CALL_BUDGET,
            compile_deadline: COMPILE_DEADLINE,
            measurement_cap: MAX_MEASUREMENTS,
            measurements: Measurements::default(),
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

    /// Sets the least of a caller's deadline that must be left for it to call another module,
    /// [`MIN_CALL_BUDGET`] by default.
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

    fn now(&mut self) -> Time {
        self.clock += 1;
        let (h, m, s) = (self.clock / 3600, (self.clock / 60) % 60, self.clock % 60);
        Time::new(format!("2026-11-07T{:02}:{m:02}:{s:02}Z", h % 24)).expect("a time")
    }

    /// Starts `version` of the provider in a new process. If it starts, its process replaces the old one; if not,
    /// the old one keeps running and the previous version stays in force.
    fn start(&mut self, provider: &str, version: &str) {
        let Some(slot) = self.providers.get(provider) else {
            return;
        };
        let (Some(machine), Some(artefact)) = (
            slot.machines.get(version).cloned(),
            slot.accepted
                .iter()
                .rev()
                .find(|a| a.declaration().version.as_str() == version),
        ) else {
            return;
        };
        let code = &artefact.build.machine_code;
        let admitted = [
            code.sandboxed.value.as_str().to_owned(),
            code.outside.value.as_str().to_owned(),
        ];
        // A process that fails to start is dropped, which kills it; the slot keeps what it had.
        if let Ok(process) = ProviderProcess::spawn(
            &self.paths,
            &self.workdir,
            provider,
            &machine,
            [&admitted[0], &admitted[1]],
            &self.stderrs,
            START_DEADLINE,
        ) {
            keep_last(
                &mut self.measurements.starts,
                process.took,
                self.measurement_cap,
            );
            let artefact = artefact.clone();
            let slot = self.providers.entry(provider.to_owned()).or_default();
            slot.active = Some(version.to_owned());
            slot.process = Some(process);
            self.world.running(&artefact);
        }
    }

    fn running_declaration(&self, provider: &str) -> Option<(Declaration, String)> {
        let slot = self.providers.get(provider)?;
        let version = slot.active.clone()?;
        let declaration = slot
            .accepted
            .iter()
            .rev()
            .find(|a| a.declaration().version.as_str() == version)?
            .declaration()
            .clone();
        Some((declaration, version))
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
                let context = self.stack.last().expect("a context");
                let (provider, serving) = (context.provider.clone(), context.serving.is_some());
                let value = if serving {
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
        let started_at = self.now();
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
                Outcome::Unreachable | Outcome::NoAnswer => "unknown",
                Outcome::Refused(_) => "refused",
            }
            .to_owned(),
        }
    }

    /// Writes the outbound call record of a call refused before the egress layer saw it, because its module hit a
    /// bound (`limit_reached`) or its request isn't one the platform sends (`request_not_allowed`): one hop, the
    /// endpoint it asked for, and the refusal; nothing of the request (`REQ-MDL-8`). Nothing is written where no outside request is in
    /// flight, or the endpoint isn't one a record can name.
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
        let started_at = self.now();
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
            } else if left < self.call_budget {
                refusal = Some(CallRefusalReason::DeadlineTooNear);
            }
        }
        // Only an answer makes a call delivered (`REQ-MDL-21`): the callee answers on its own credentials, in its
        // own process, and a callee that stopped, hit a limit or gave no readable answer leaves the call unanswered.
        let mut answered = false;
        if refusal.is_none() {
            answered = matches!(self.handle(callee, &asked), Handled::Answered(_));
        }
        let (Some(transaction), Some(caller), Some(caller_version), Some(callee_id)) = (
            Identifier::new(transaction),
            Identifier::new(caller),
            Identifier::new(version),
            Identifier::new(callee),
        ) else {
            // Already counted above; over the bound, the operation ends as a limit.
            return if limited {
                Reply::Limit
            } else {
                Reply::Module {
                    outcome: "refused".to_owned(),
                }
            };
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
        let delivered = matches!(outcome, CrossModuleOutcome::Delivered(_));
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
        Reply::Module {
            outcome: if delivered { "delivered" } else { "refused" }.to_owned(),
        }
    }

    /// Hands the operation to the provider's process and serves its requests until it answers, or until the
    /// operation's deadline, when the process is killed and the operation refused.
    fn run_in_process(
        &mut self,
        provider: &str,
        operation: &Statement<Operation>,
        declaration: Declaration,
        version: String,
    ) -> Result<(Vec<u8>, Result<SecretValue, SecretRefusalReason>), PlatformRefusal> {
        let mut process = self
            .providers
            .get_mut(provider)
            .and_then(|s| s.process.take())
            .ok_or(PlatformRefusal::Unavailable)?;
        let transaction = serde_json::to_value(&operation.statement)
            .ok()
            .and_then(|v| v["transaction"].as_str().map(str::to_owned))
            .unwrap_or_default();
        // A cross-module call's operation ends no later than the one that made it.
        let own = Instant::now() + self.deadline;
        let due = self.stack.last().map_or(own, |caller| caller.due.min(own));
        let caller = Identifier::new(provider.to_owned()).map(Caller::process_of);
        let json = serde_json::to_string(operation).unwrap_or_default();
        // What the world knows the operation by: its transaction, or, for one that names none (a verifier's
        // judgement), an identifier of the platform's own, from the operation's digest. The proxy resolves the
        // statement key only for the operation it was told is being handled.
        let handled_as = Identifier::new(transaction.clone()).or_else(|| {
            Identifier::new(format!("rutent:untransacted:{}", sha256(json.as_bytes())))
        });
        if let (Some(caller), Some(handled_as)) = (&caller, &handled_as) {
            self.world.begin_operation(caller, handled_as, due);
        }
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
        });
        let mut outcome = Err(PlatformRefusal::Unavailable);
        let mut alive = process
            .send(&ToProvider::Handle { operation: json })
            .is_ok();
        while alive {
            match process.receive_until(due) {
                Received::Gone => alive = false,
                Received::Overdue => {
                    // Whatever it would still say is dropped with it.
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
                    } else if message.contains("all fuel consumed")
                        || message.starts_with("limit reached")
                    {
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
        // The statement key is resolved while the module still handles the operation, the proxy's condition for
        // it, and only for an answer to sign; the module never sees it.
        let limited = self.stack.last().is_none_or(|c| c.limited.is_some());
        let statement_key = match (&outcome, &caller, &handled_as) {
            (Ok(_), Some(caller), Some(handled_as)) if !limited => {
                Some(self.world.statement_key(caller, handled_as))
            }
            _ => None,
        };
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
        if alive && process.alive() {
            if let Some(slot) = self.providers.get_mut(provider) {
                slot.process = Some(process);
            }
        } else {
            // The process is gone: this operation is refused, and the platform starts the running version again
            // in a new process, as a supervisor would, touching no other provider.
            drop(process);
            if let Some(context) = context {
                self.start(provider, &context.version);
            }
        }
        outcome.map(|bytes| {
            (
                bytes,
                statement_key.unwrap_or(Err(SecretRefusalReason::UndeclaredSecret)),
            )
        })
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
    /// kept, and a release that needs only an automatic acceptance is started.
    pub fn admit(
        &mut self,
        received_assertion: &[u8],
        binaries: &Parts<Vec<u8>>,
        judge: &mut impl ReleaseJudge,
        at: Clock,
    ) -> Admitted {
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
        let previous = self
            .providers
            .get(&provider)
            .and_then(|slot| slot.accepted.last().cloned());
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
        // A version already admitted, accepted or waiting, is never admitted again: its machine code and artefact
        // stay what they are. While a release waits for its acceptance, no other is admitted, under any version:
        // the one waiting is what its acceptance is given to (`AdmissionRefusal::VersionAlreadyAdmitted`).
        if let Some(slot) = self.providers.get(&provider) {
            let accepted = slot
                .accepted
                .iter()
                .any(|a| a.declaration().version.as_str() == version);
            if accepted || slot.pending.is_some() {
                return Admitted::Refused(AdmissionRefusal::VersionAlreadyAdmitted);
            }
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
                at: self.now(),
            });
        }
        let slot = self.providers.entry(provider.clone()).or_default();
        if fresh {
            slot.pending = Some((artefact.clone(), machine, received_assertion.to_vec()));
        } else {
            slot.accepted.push(artefact.clone());
            slot.machines.insert(version.clone(), machine);
            self.start(&provider, &version);
        }
        Admitted::Artefact(Box::new(artefact))
    }

    /// Gives a fresh acceptance, by `by`, to the provider's release that waits for one, and starts it. The
    /// acceptance is of one assertion: `assertion_digest` must be the waiting artefact's, the digest of its
    /// assertion as received (`REQ-MDL-1`). That assertion is judged again by `judge` at the caller's clock `at`,
    /// so a release signed with a key retired since its admission is no longer accepted (`THR-4`). The accepted
    /// artefact, or `None` if no release waits under that digest, `by` names no one, or the assertion no longer
    /// passes; the waiting release then stays as it was.
    pub fn accept(
        &mut self,
        provider: &str,
        assertion_digest: &Digest,
        by: &str,
        judge: &impl ReleaseJudge,
        at: Clock,
    ) -> Option<AdmissionArtefact> {
        let by = Identifier::new(by)?;
        let (artefact, _, received) = self.providers.get(provider)?.pending.as_ref()?;
        if &artefact.assertion_digest != assertion_digest
            || judge.judge(received, artefact, at).is_err()
        {
            return None;
        }
        let at = self.now();
        let slot = self.providers.get_mut(provider)?;
        let (mut artefact, machine, _) = slot.pending.take()?;
        let version = artefact.declaration().version.as_str().to_owned();
        artefact.acceptance.accepted = Some(Accepted { by: Some(by), at });
        slot.accepted.push(artefact.clone());
        slot.machines.insert(version.clone(), machine);
        self.start(provider, &version);
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
        self.start(provider, &version);
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
    /// against the result schema, and signed with the provider's statement key, which the world resolves while
    /// the module still handles the operation (`T-014-05`); a result that can't be signed is refused as
    /// `Unavailable`.
    pub fn handle(&mut self, provider: &str, operation: &Statement<Operation>) -> Handled {
        *self
            .operations_received
            .entry(provider.to_owned())
            .or_default() += 1;
        let Some((declaration, version)) = self.running_declaration(provider) else {
            return Handled::Refused(PlatformRefusal::Unavailable);
        };
        if !declaration
            .sandboxed
            .handles
            .contains(&OperationKind::of(&operation.statement))
        {
            return Handled::Refused(PlatformRefusal::NotHandled);
        }
        let statement_key = declaration.outside.statement_key.clone();
        let (bytes, private) = match self.run_in_process(provider, operation, declaration, version)
        {
            Ok(answered) => answered,
            Err(refusal) => return Handled::Refused(refusal),
        };
        // The output is untrusted: judged against the result schema, patterns and lengths included (`REQ-MDL-12`).
        let result = match output::check(&bytes, self.output_limit) {
            Ok(result) => result,
            Err(refusal) => return Handled::Refused(refusal),
        };
        if !result_matches(&result, &operation.statement) {
            return Handled::Refused(PlatformRefusal::MalformedOutput);
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
        // (`F-014`): the result and what the module nested in it in the provider's name. A result that can't be
        // signed, because the world holds no key, the key isn't the one declared, or a nested statement isn't
        // what the operation asked, in this deployment and protocol version, isn't passed on.
        match private.ok().and_then(|key| {
            proxy::sign_result(
                &unsigned,
                &proxy::Answering {
                    provider,
                    deployment: &self.envelope.deployment,
                    protocol_version: &self.envelope.protocol_version,
                    operation: &operation.statement,
                },
                &key,
                &statement_key,
            )
            .ok()
        }) {
            Some(signed) => Handled::Answered(Box::new(signed)),
            None => Handled::Refused(PlatformRefusal::Unavailable),
        }
    }

    /// What the platform saw since it was last asked: outbound call records, secret lookups, state reads and
    /// cross-module calls.
    pub fn take_events(&mut self) -> Events {
        std::mem::take(&mut self.events)
    }
}

#[cfg(test)]
mod tests {
    use super::reenters;

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
