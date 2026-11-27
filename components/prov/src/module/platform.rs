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
use std::sync::{Arc, Mutex};
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
    Operation, RESULT_SHAPE, ResultBody, Signature, SignatureScheme, Statement, StatusQuery,
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
use crate::module::record::{
    AcceptedEntry, CarriedEntry, DecisionEntry, Entry, MarkEntry, MemoryRecord, Record,
    ReleasesEntry,
};
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

/// The time the platform's operation deadlines and a callee's remaining budget run on (`T-013-08`). Whether a
/// callee is started, and when an operation's process is given up on, are judged on this clock and not on how
/// long a caller took to come up.
///
/// In production it is [`Monotonic`], `Instant`, as it always was. A test hands the platform a [`SteppedClock`],
/// which moves only when the test steps it.
pub trait OperationClock: Send + Sync {
    /// The time now, on this clock.
    fn now(&self) -> Instant;

    /// Whether this clock runs with the operating system's, so that waiting until `due` is waiting that long. A
    /// clock that does not is polled by the platform while it waits, so that a step is seen.
    fn follows_wall_time(&self) -> bool;

    /// The operating system's time a wait that ends at `due`, a time on this clock, and that the platform hands
    /// outward (a name lookup, a connection) is to end at: `due` itself on a clock that follows it.
    fn wall_due(&self, due: Instant) -> Instant {
        due
    }
}

/// The operating system's monotonic clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct Monotonic;

impl OperationClock for Monotonic {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn follows_wall_time(&self) -> bool {
        true
    }
}

/// A clock that stands still until [`SteppedClock::step`] moves it: for tests of the deadlines, which then never
/// depend on how long a process took to start.
#[derive(Debug, Clone)]
pub struct SteppedClock {
    start: Instant,
    stepped: Arc<Mutex<Duration>>,
    /// Whether what the platform hands outward waits on the operating system's time for what is left on this
    /// clock, or is not tied to it.
    outward_on_wall_time: bool,
}

impl SteppedClock {
    /// A clock that reads the current time and stays there.
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            stepped: Arc::new(Mutex::new(Duration::ZERO)),
            outward_on_wall_time: false,
        }
    }

    /// A clock that stands still, whose deadlines the platform hands outward as the operating system's time left on
    /// them when it does (a name lookup given up, a peer cut off): for a test of those, which ends by itself at
    /// the deadline and takes as long as the deadline says, whatever else the machine is doing.
    pub fn with_outward_waits_on_wall_time() -> Self {
        Self {
            outward_on_wall_time: true,
            ..Self::new()
        }
    }

    /// Moves the clock on by `by`; every clone of it moves.
    pub fn step(&self, by: Duration) {
        if let Ok(mut stepped) = self.stepped.lock() {
            *stepped += by;
        }
    }
}

impl Default for SteppedClock {
    fn default() -> Self {
        Self::new()
    }
}

impl OperationClock for SteppedClock {
    fn now(&self) -> Instant {
        self.start + self.stepped.lock().map_or(Duration::ZERO, |s| *s)
    }

    fn follows_wall_time(&self) -> bool {
        false
    }

    fn wall_due(&self, due: Instant) -> Instant {
        let wait = if self.outward_on_wall_time {
            due.saturating_duration_since(self.now())
        } else {
            STEPPED_WAIT_CAP
        };
        Instant::now() + wait
    }
}

/// How often the platform looks at a clock that doesn't follow wall time while it waits for a process.
const STEPPED_POLL: Duration = Duration::from_millis(2);

/// The longest an outbound call waits on the operating system's time under a clock that doesn't follow it: the
/// deadline there ends the operation by a step of the clock, and this only stops a test that never steps from
/// waiting for ever.
const STEPPED_WAIT_CAP: Duration = Duration::from_secs(60);

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
    /// Whether the provider's advertisement declares a repeat safe (`REQ-CAT-10`), read from the judge when a
    /// release was admitted or accepted. `None`, declared or not known, is no safe retry (`T-013-09`).
    safe_retry: Option<bool>,
    /// How long, in seconds, evidence of the provider's outcomes stays available, as its advertisement declares
    /// (`REQ-CAT-10`), read with `safe_retry`. `None`, unbounded, undeclared or not known, keeps the record of an
    /// operation it carried out with no segment for as long as the platform's record lasts (`T-013-10`).
    evidence_window: Option<u64>,
}

/// The in-place form of a decision that only restricts what runs, taken where the platform's record can't hold the
/// decision itself (`T-013-10`).
type Restrict<'a> = Box<dyn FnOnce(&mut Lifecycle) -> Result<(), LifecycleRefusal> + 'a>;

/// The scope of an operation the platform was handed, as against one a module called a callee with.
const TOP_LEVEL: &str = "";

/// An operation of a transaction the platform carried out: whose it was, and its answer if it gave one, which a
/// provider that declares a repeat safe is answered from (`T-013-09`). It is kept in the platform's record too, so a
/// restarted platform holds it (`T-013-10`).
struct Carried {
    provider: String,
    /// The operation as the platform was handed it. Only an identical one is a repeat, answered from the record; one
    /// of other terms or another envelope (deployment, protocol version or signing key) is a second operation under
    /// the transaction's identifier, and refused whatever the provider declares (`REQ-MDL-14`, `T-013-10`).
    operation: Statement<Operation>,
    answer: Option<Box<Statement<ResultBody>>>,
    /// When, in seconds of the world's clock, the record may be freed: its provider's evidence window after it was
    /// carried out, where no segment held its transaction. `None` is never; a segment's record is freed as the
    /// segment ends (`REQ-CAT-10`).
    free_at: Option<u64>,
    /// The version of the provider that carried it out: a question about it later runs on that one (`T-013-11`).
    version: Option<String>,
}

impl Carried {
    fn entry(&self, transaction: &str, scope: &str) -> CarriedEntry {
        CarriedEntry {
            transaction: transaction.to_owned(),
            scope: scope.to_owned(),
            provider: self.provider.clone(),
            operation: self.operation.clone(),
            answer: self.answer.as_deref().cloned(),
            free_at: self.free_at,
            version: self.version.clone(),
        }
    }
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
    /// Whether the module passed anything out: an outbound call a destination was sent (a hop of its record
    /// connected), or a cross-module call handed to its callee. From then on what it did may have taken effect, so
    /// a failure is never given as a reason that says nothing took effect (`PlatformRefusal::OutcomeUnknown`,
    /// `REQ-ORD-1`).
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
    /// The most a process may take to start, on the operating system's time: [`START_DEADLINE`] by default.
    start_deadline: Duration,
    clock: Arc<dyn OperationClock>,
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
    /// The operation carried out under each transaction, by the scope it was asked in, until its segment ends or,
    /// for one no segment held, its evidence window passes: [`TOP_LEVEL`] for the platform's own, and the callee's
    /// name for a call a module made into it. A second one in a scope is a repeat, whatever its kind (`INV-8`,
    /// `REQ-CAT-10`, `T-013-09`). Kept in the platform's record too (`T-013-10`).
    carried: BTreeMap<String, BTreeMap<String, Carried>>,
    /// The callees' versions whose process ended during a top-level operation, started again at the platform's
    /// next call, after that operation's answer was handed back ([`Platform::restart_ended`]).
    restarts: Vec<(String, String)>,
    /// Where what the platform carried out is kept, beyond its memory (`T-013-10`).
    record: Box<dyn Record>,
    /// A write to the record failed: nothing more is written to it by this platform (`Record`'s contract, which the
    /// platform holds itself to whatever the record does).
    record_failed: bool,
    /// Who accepted each provider's accepted releases, and when, in the order they were accepted: written to the
    /// record with the releases' state, and what a release admitted again after a restart is accepted as
    /// (`T-013-11`).
    acceptances: BTreeMap<String, Vec<AcceptedEntry>>,
    /// The providers whose restored releases wait for the operator's confirmation: the record the platform opened on
    /// didn't end with the platform's clean close, so a restrictive decision it took and couldn't write isn't known
    /// (`T-013-10`'s `TakenUnrecorded`). Nothing of such a provider runs until [`Platform::confirm`] (`T-013-11`).
    unconfirmed: BTreeSet<String>,
    /// The question [`Platform::status`] is putting to a module, while it does.
    status_run: Option<StatusRun>,
}

/// What a status question needs beyond the question itself while the module answers it.
struct StatusRun {
    /// The version that carried out the transaction asked about, where no segment holds it.
    version: Option<String>,
    /// The operation the platform carried out under that transaction.
    about: Operation,
    /// What the platform holds of the provider's answer to it.
    held: Option<ResultBody>,
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
            start_deadline: START_DEADLINE,
            clock: Arc::new(Monotonic),
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
            carried: BTreeMap::new(),
            restarts: Vec::new(),
            record: Box::new(MemoryRecord::new()),
            record_failed: false,
            acceptances: BTreeMap::new(),
            unconfirmed: BTreeSet::new(),
            status_run: None,
        })
    }

    /// A platform as [`Platform::new`] makes it, on `record`, which holds what an earlier platform carried out
    /// (`T-013-10`; `INV-8`, `NFR-6`). It reads every entry back: what was carried out under each transaction, and
    /// its answer if it gave one, an operation written as handed over and never answered being carried out with no
    /// answer; the segments in flight, on the versions they ran on; the transactions that ended; the operator's
    /// marks on versions and the decisions recorded; and how many operations each provider's version ran. A version
    /// marked before the restart stays marked when its release is admitted again: one declared exploited runs
    /// nothing, and no marked version is put back in force by starting. Then it frees what its evidence window let go of, and replaces the
    /// record with what it holds. From then on it writes each to the record before it acts on it, so a repeat
    /// after the platform restarts is answered as one in the same run is: refused, or from the first answer.
    ///
    /// It holds the state of every release it held (`T-013-11`): which were accepted, by whom and when, which version
    /// is in force and which is staged, the staging's counts and the versions withdrawn while they waited. It holds
    /// no process: each release is admitted again, as at any start, in the order they were accepted, and is then
    /// accepted as the record says and put where it stood, in force, staged or under a segment in flight, never
    /// staged as a new upgrade nor given its acceptance again; one the record never held is admitted as a new
    /// release is. A release not yet admitted again has no process, so an operation for it is `Unavailable`. The
    /// staging policy ([`Platform::set_staging`]) is the deployment's setting, not the record's. Refused where the
    /// record can't be read, holds an entry the platform doesn't read, or can't be replaced.
    pub fn open(
        paths: Paths,
        world: W,
        envelope: Envelope,
        record: Box<dyn Record>,
    ) -> Result<Self, String> {
        let mut platform = Self::new(paths, world, envelope)?;
        platform.record = record;
        let lines = platform
            .record
            .load()
            .map_err(|e| format!("the platform's record can't be read: {e}"))?;
        for (n, line) in lines.iter().enumerate() {
            Entry::read(line)
                .and_then(|entry| platform.read_back(entry))
                .map_err(|e| format!("the platform's record, entry {}: {e}", n + 1))?;
        }
        // A record that doesn't end with one of the platform's whole closes can't vouch for a decision the platform took
        // and couldn't write: every provider with a release in force or staged waits for the operator (`T-013-11`).
        // One that ends with a whole close while some providers still waited holds those again, and only those: the
        // operator's confirmations of the others stand, since the platform wrote every decision it took after them
        // (`T-020-49`). A record with no entry holds nothing.
        let held_again: Option<BTreeSet<String>> = match lines.last().map(|line| Entry::read(line))
        {
            None | Some(Ok(Entry::Closed)) => Some(BTreeSet::new()),
            Some(Ok(Entry::ClosedHolding { held })) => Some(held.into_iter().collect()),
            Some(_) => None,
        };
        platform.unconfirmed = platform
            .lifecycle
            .providers()
            .filter(|p| {
                platform
                    .lifecycle
                    .in_force(p)
                    .or_else(|| platform.lifecycle.staged(p))
                    .is_some()
                    && held_again.as_ref().is_none_or(|held| held.contains(*p))
            })
            .map(str::to_owned)
            .collect();
        platform.free_spent();
        let lines = platform
            .held()
            .map_err(|e| format!("the platform's record can't be written: {e}"))?;
        platform
            .record
            .replace(&lines)
            .map_err(|e| format!("the platform's record can't be replaced: {e}"))?;
        Ok(platform)
    }

    /// Takes one entry of the record back into what the platform holds.
    fn read_back(&mut self, entry: Entry) -> std::io::Result<()> {
        match entry {
            Entry::Carrying {
                transaction,
                scope,
                provider,
                operation,
            } => {
                if !self.lifecycle.has_ended(&transaction) {
                    // Handed over and never answered: carried out, with no answer.
                    self.carried
                        .entry(transaction)
                        .or_default()
                        .entry(scope)
                        .or_insert(Carried {
                            provider,
                            operation: *operation,
                            answer: None,
                            free_at: None,
                            version: None,
                        });
                }
            }
            Entry::Ran {
                provider,
                version,
                carried,
                answered,
            } => {
                // What it counted towards a staging, as it was counted: the releases the entries before it left.
                if let Some(answered) = answered {
                    self.lifecycle.observe(&provider, &version, answered);
                }
                *self.runs.entry((provider, version)).or_default() += 1;
                if let Some(carried) = carried {
                    self.carried_again(*carried);
                }
            }
            Entry::Withdrawn { transaction, scope } => {
                if let Some(scopes) = self.carried.get_mut(&transaction) {
                    if scopes.get(&scope).is_some_and(|c| c.answer.is_none()) {
                        scopes.remove(&scope);
                    }
                    if scopes.is_empty() {
                        self.carried.remove(&transaction);
                    }
                }
            }
            Entry::Carried(carried) => self.carried_again(*carried),
            Entry::Runs {
                provider,
                version,
                count,
            } => {
                self.runs.insert((provider, version), count);
            }
            Entry::Segment(segment) => self.lifecycle.restore_segment(segment.into()),
            Entry::Began { segment, release } => {
                self.lifecycle.restore_segment(segment.into());
                if let Some(release) = release {
                    self.restore_releases(&release)?;
                }
            }
            Entry::Releases(release) => self.restore_releases(&release)?,
            Entry::Closed | Entry::ClosedHolding { .. } => {}
            Entry::Ended { transaction } => {
                self.carried.remove(&transaction);
                self.lifecycle.spend(&transaction);
            }
            Entry::Decided {
                marks,
                segments,
                decisions,
                releases,
            } => {
                for release in &releases {
                    self.restore_releases(release)?;
                }
                for mark in marks {
                    self.lifecycle
                        .restore_mark(&mark.provider, &mark.version, mark.mark.into());
                }
                for segment in segments {
                    self.lifecycle.restore_segment(segment.into());
                }
                for decision in decisions {
                    let decision: lifecycle::Decision = decision.try_into()?;
                    // A release withdrawn while it waited is never admitted again: a version names one release.
                    if decision.kind == lifecycle::DecisionKind::Withdrawn {
                        self.providers
                            .entry(decision.provider.clone())
                            .or_default()
                            .withdrawn
                            .insert(decision.version.clone());
                    }
                    self.lifecycle.restore_decision(decision);
                }
            }
        }
        Ok(())
    }

    /// Holds a provider's releases as the record says they stood (`T-013-11`): which were accepted and by whom, the
    /// version in force, the staged one and its counts. No process runs until each release is admitted again.
    fn restore_releases(&mut self, release: &ReleasesEntry) -> std::io::Result<()> {
        self.lifecycle
            .restore_release_state(&release.provider, release.state()?);
        self.acceptances
            .insert(release.provider.clone(), release.accepted.clone());
        Ok(())
    }

    /// The provider's releases as an entry of the record: their state in `lifecycle`, accepted as `accepted`.
    fn releases_entry(
        lifecycle: &Lifecycle,
        provider: &str,
        accepted: &[AcceptedEntry],
    ) -> Option<ReleasesEntry> {
        let state = lifecycle.release_state(provider)?;
        (!state.is_empty()).then(|| ReleasesEntry::of(provider, &state, accepted))
    }

    /// Holds `carried` as the record of its transaction in its scope, unless the transaction ended.
    fn carried_again(&mut self, carried: CarriedEntry) {
        if self.lifecycle.has_ended(&carried.transaction) {
            return;
        }
        self.carried.entry(carried.transaction).or_default().insert(
            carried.scope,
            Carried {
                provider: carried.provider,
                operation: carried.operation,
                answer: carried.answer.map(Box::new),
                free_at: carried.free_at,
                version: carried.version,
            },
        );
    }

    /// What the platform holds, as the entries of a record: the transactions that ended, the segments in flight,
    /// what was carried out, and the runs counted.
    fn held(&self) -> std::io::Result<Vec<String>> {
        let ended = self.lifecycle.ended().map(|transaction| Entry::Ended {
            transaction: transaction.to_owned(),
        });
        let decided = std::iter::once(Entry::Decided {
            marks: self
                .lifecycle
                .marks()
                .map(|(provider, version, mark)| MarkEntry {
                    provider: provider.to_owned(),
                    version: version.to_owned(),
                    mark: mark.into(),
                })
                .collect(),
            segments: Vec::new(),
            decisions: self
                .lifecycle
                .decisions()
                .iter()
                .map(DecisionEntry::from)
                .collect(),
            releases: Vec::new(),
        });
        let releases = self
            .lifecycle
            .providers()
            .filter_map(|provider| {
                let accepted = self
                    .acceptances
                    .get(provider)
                    .map_or(&[][..], Vec::as_slice);
                Self::releases_entry(&self.lifecycle, provider, accepted)
            })
            .map(|release| Entry::Releases(Box::new(release)));
        let segments = self
            .lifecycle
            .segments()
            .map(|segment| Entry::Segment(segment.into()));
        let carried = self.carried.iter().flat_map(|(transaction, scopes)| {
            scopes.iter().map(move |(scope, carried)| {
                Entry::Carried(Box::new(carried.entry(transaction, scope)))
            })
        });
        let runs = self
            .runs
            .iter()
            .map(|((provider, version), count)| Entry::Runs {
                provider: provider.clone(),
                version: version.clone(),
                count: *count,
            });
        ended
            .chain(decided)
            .chain(releases)
            .chain(segments)
            .chain(carried)
            .chain(runs)
            .map(|entry| entry.line())
            .collect()
    }

    /// Writes `entry` to the record, durably once this returns. Refused, without asking the record, once a write
    /// to it failed: a record that recovered in place would otherwise take later entries, none of which holds a
    /// decision taken while it couldn't be written (`Record`'s contract).
    fn write(&mut self, entry: &Entry) -> std::io::Result<()> {
        if self.record_failed {
            return Err(std::io::Error::other(
                "a write to the platform's record failed: it takes no more until the platform opens on it again",
            ));
        }
        let written = entry.line().and_then(|line| self.record.append(&line));
        self.record_failed |= written.is_err();
        written
    }

    /// Takes an operator's decision, `decide`, on the lifecycle, once what it changes is in the platform's record:
    /// the marks it sets, the segments it moves and the decisions it records, in one entry written before the
    /// platform holds any of it (`T-013-10`). A decision the lifecycle refuses changes nothing.
    ///
    /// Where the record can't hold it, what happens is the decision's kind. One that widens what runs (`restrict`
    /// `None`: promoting, rolling back, withdrawing) is refused as `Unrecorded`, nothing changed. One that only
    /// restricts it is taken all the same, in memory, by `restrict`, its in-place form, which puts no version back
    /// in force, and reported as `TakenUnrecorded`: a failing disk never keeps an exploited version running
    /// (`REQ-MDL-20`).
    fn decide<T>(
        &mut self,
        decide: impl FnOnce(&mut Lifecycle) -> Result<T, LifecycleRefusal>,
        restrict: Option<Restrict<'_>>,
    ) -> Result<T, LifecycleRefusal> {
        let mut next = self.lifecycle.clone();
        let made = decide(&mut next)?;
        let marks: Vec<MarkEntry> = next
            .marks()
            .filter(|mark| !self.lifecycle.marks().any(|held| held == *mark))
            .map(|(provider, version, mark)| MarkEntry {
                provider: provider.to_owned(),
                version: version.to_owned(),
                mark: mark.into(),
            })
            .collect();
        let segments: Vec<_> = next
            .segments()
            .filter(|segment| self.lifecycle.segment(&segment.transaction) != Some(*segment))
            .map(Into::into)
            .collect();
        let decisions: Vec<DecisionEntry> = next
            .decisions()
            .get(self.lifecycle.decisions().len()..)
            .unwrap_or_default()
            .iter()
            .map(DecisionEntry::from)
            .collect();
        // The releases whose state the decision changed: written with it, so a promotion, a rollback or a revocation of
        // the version in force is held or not at all (`T-013-11`).
        let releases: Vec<ReleasesEntry> = next
            .providers()
            .filter(|provider| {
                self.lifecycle.release_state(provider) != next.release_state(provider)
            })
            .filter_map(|provider| {
                let accepted = self
                    .acceptances
                    .get(provider)
                    .map_or(&[][..], Vec::as_slice);
                Self::releases_entry(&next, provider, accepted)
            })
            .collect();
        if !(marks.is_empty() && segments.is_empty() && decisions.is_empty() && releases.is_empty())
        {
            let written = self.write(&Entry::Decided {
                marks,
                segments,
                decisions,
                releases,
            });
            if written.is_err() {
                return Err(match restrict {
                    None => LifecycleRefusal::Unrecorded,
                    Some(restrict) => match restrict(&mut self.lifecycle) {
                        Ok(()) => LifecycleRefusal::TakenUnrecorded,
                        Err(refusal) => refusal,
                    },
                });
            }
        }
        self.lifecycle = next;
        Ok(made)
    }

    /// The world's clock, in whole seconds, or `None` where it gives no time the platform can read.
    fn world_secs(&mut self) -> Option<u64> {
        let now = self.world.now().ok()?;
        Stamp::parse(now.as_str()).map(|stamp| stamp.secs)
    }

    /// When the record of an operation the provider just carried out under `transaction` may be freed: its
    /// evidence window from now, where no segment holds the transaction, the provider's evidence is bounded and
    /// the world's clock can be read; never otherwise (`REQ-CAT-10`).
    fn free_at(&mut self, provider: &str, transaction: &str) -> Option<u64> {
        if self.lifecycle.segment(transaction).is_some() {
            return None;
        }
        let window = self.providers.get(provider)?.evidence_window?;
        self.world_secs()?.checked_add(window)
    }

    /// Frees the record of each transaction no segment holds whose every operation's evidence window has passed,
    /// by the world's clock (`REQ-CAT-10`): its operations and answers are dropped, and the transaction is spent,
    /// so a repeat of it is refused and never run (`INV-8`). Nothing is freed where the clock can't be read.
    fn free_spent(&mut self) {
        let Some(now) = self.world_secs() else {
            return;
        };
        let due: Vec<String> = self
            .carried
            .iter()
            .filter(|(transaction, scopes)| {
                self.lifecycle.segment(transaction).is_none()
                    && !scopes.is_empty()
                    && scopes
                        .values()
                        .all(|carried| carried.free_at.is_some_and(|at| at <= now))
            })
            .map(|(transaction, _)| transaction.clone())
            .collect();
        for transaction in due {
            self.carried.remove(&transaction);
            self.lifecycle.spend(&transaction);
            // A record that can't be written still holds the transaction as carried out: a restarted platform
            // refuses its repeat all the same.
            let _ = self.write(&Entry::Ended { transaction });
        }
    }

    /// Whether the platform keeps a record of an operation it carried out under `transaction`: until its segment
    /// ends, or, for one no segment held, until its evidence window passes (`T-013-10`).
    pub fn carries(&self, transaction: &str) -> bool {
        self.carried.contains_key(transaction)
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

    /// Sets the most a provider's process may take to start, [`START_DEADLINE`] by default. A start is the operating
    /// system's work and cannot be stepped, so a test of something other than start-up on a loaded machine gives
    /// it room.
    pub fn set_start_deadline(&mut self, deadline: Duration) {
        self.start_deadline = deadline;
    }

    /// Sets the clock the deadlines and a callee's remaining budget run on, [`Monotonic`] by default
    /// (`T-013-08`).
    pub fn set_operation_clock(&mut self, clock: Arc<dyn OperationClock>) {
        self.clock = clock;
    }

    /// The operating system's time a wait for `due` on the clock ends at: `due` itself on a clock that follows it.
    fn wall(&self, due: Instant) -> Instant {
        self.clock.wall_due(due)
    }

    /// The process's next message, waiting at most until `due` on the clock.
    fn receive(&self, process: &mut ProviderProcess, due: Instant) -> Received {
        if self.clock.follows_wall_time() {
            return process.receive_until(due);
        }
        // The clock moves only when stepped: a deadline it has reached has passed, whatever is waiting.
        loop {
            if self.clock.now() >= due {
                return Received::Overdue;
            }
            match process.receive_until(Instant::now() + STEPPED_POLL) {
                Received::Overdue => {}
                received => return received,
            }
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

    /// Every running provider and its process id. A callee whose process ended during a top-level operation isn't
    /// listed until the platform's next call starts it again.
    pub fn processes(&self) -> Vec<(String, u32)> {
        self.providers
            .iter()
            .filter_map(|(name, slot)| slot.process.as_ref().map(|p| (name.clone(), p.pid())))
            .collect()
    }

    /// Kills a provider's process from outside, as a crash would: the platform sees it gone.
    pub fn kill(&mut self, provider: &str) -> bool {
        self.restart_ended();
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
            self.start_deadline,
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

    /// Accepts and starts a release, at the caller's clock `at`, `by` being who accepted it (none for an automatic
    /// acceptance): staged beside the version in force where a staging policy is set and a version is in force, and in
    /// force otherwise (`T-013-06`). If its process doesn't start, the version in force stays in force, and nothing
    /// runs the release (`REQ-MDL-4`, `AC-19`). What the acceptance changes, the version accepted and where it went,
    /// is written to the platform's record before it takes effect, and the release isn't accepted, its process
    /// stopped, where the record can't hold it: `false`, nothing changed (`T-013-11`). Nor is it accepted while the
    /// provider's releases wait for the operator's confirmation: `false` (`T-020-66`).
    fn accept_and_start(
        &mut self,
        provider: &str,
        artefact: &AdmissionArtefact,
        machine: Rc<Machine>,
        by: Option<&Identifier>,
        at: Clock,
    ) -> bool {
        // A provider whose releases wait for the operator's confirmation accepts nothing new: what its record holds
        // is what the operator confirms (`T-013-11`, `T-020-66`).
        if self.unconfirmed.contains(provider) {
            return false;
        }
        let version = artefact.declaration().version.as_str().to_owned();
        let slot = self.providers.entry(provider.to_owned()).or_default();
        slot.accepted.push(artefact.clone());
        slot.machines.insert(version.clone(), machine);
        let placement = self.lifecycle.placement(provider);
        let started = self.spawn(provider, &version);
        let mut next = self.lifecycle.clone();
        next.accept(provider, &version);
        if started.is_some() {
            next.started(provider, &version, placement, at);
        }
        let mut acceptances = self.acceptances.get(provider).cloned().unwrap_or_default();
        acceptances.push(AcceptedEntry {
            version: version.clone(),
            assertion_digest: artefact.assertion_digest.value.as_str().to_owned(),
            by: by.map(|by| by.as_str().to_owned()),
            at: at.0,
        });
        let written: std::io::Result<()> = match Self::releases_entry(&next, provider, &acceptances)
        {
            Some(entry) => self.write(&Entry::Releases(Box::new(entry))),
            None => Ok(()),
        };
        if written.is_err() {
            // The process, if one started, is dropped, which stops it.
            if let Some(slot) = self.providers.get_mut(provider) {
                slot.accepted.pop();
                slot.machines.remove(&version);
            }
            return false;
        }
        self.acceptances.insert(provider.to_owned(), acceptances);
        self.lifecycle = next;
        if let Some((process, artefact)) = started {
            self.providers
                .entry(provider.to_owned())
                .or_default()
                .others
                .insert(version, process);
            self.told_running(provider, &artefact);
            self.reconcile(provider);
        }
        true
    }

    /// Starts the process of a release admitted again after a restart (`T-013-11`), if the record holds its version
    /// in force, staged or under a segment in flight: in its place, and nothing about the version in force, the
    /// staged release or the staging's counts changed. A version that doesn't start is out of force and out of
    /// staging, as one that failed to start at its acceptance is.
    fn start_restored(&mut self, provider: &str, version: &str) {
        if !self.lifecycle.wanted(provider).contains(version) || self.unconfirmed.contains(provider)
        {
            return;
        }
        let Some((process, artefact)) = self.spawn(provider, version) else {
            self.lifecycle.not_running(provider, version);
            return;
        };
        let in_force = self.lifecycle.in_force(provider) == Some(version);
        let slot = self.providers.entry(provider.to_owned()).or_default();
        if in_force {
            slot.active = Some(version.to_owned());
            slot.process = Some(process);
        } else {
            slot.others.insert(version.to_owned(), process);
        }
        self.told_running(provider, &artefact);
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
                let context = self.stack.last().expect("a context");
                let (provider, transaction) =
                    (context.provider.clone(), context.transaction.clone());
                self.events.state_reads.push(StateEvent {
                    provider,
                    part: PartName::Sandboxed,
                    transaction,
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
                let (provider, transaction, serving) = (
                    context.provider.clone(),
                    context.transaction.clone(),
                    context.serving.is_some(),
                );
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
                    transaction,
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
        let due = self.wall(context.due);
        let secrets = context
            .resolved
            .get(in_flight.as_str())
            .cloned()
            .unwrap_or_default();
        // A call the platform couldn't date isn't made, and isn't recorded: a record's time would be made up
        // (`REQ-AUD-1`). Nothing of it leaves the platform, so no call goes unrecorded (`REQ-MDL-8`), as for an
        // endpoint no record could name. The module is told `refused`, which is true: nothing was sent.
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
        let written = self.events.records.len();
        let outcome = self.world.send(
            &call,
            egress::Endpoint { scheme, host, port },
            request,
            &mut self.events.records,
        );
        // The call passed something out if a hop of its record connected, so a destination was sent the request.
        // A call refused, or one whose name never resolved or whose dial connected to nothing, sent nothing. A world
        // that wrote no record is judged by the outcome alone.
        let records = &self.events.records[written.min(self.events.records.len())..];
        let sent = if records.is_empty() {
            !matches!(outcome, Outcome::Refused(_) | Outcome::Unreachable)
        } else {
            records
                .iter()
                .any(|record| record.hops.iter().any(|hop| hop.address.is_some()))
        };
        if sent && let Some(context) = self.stack.last_mut() {
            context.passed_out = true;
        }
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
    ///
    /// A world whose clock gives no time leaves the call unrecorded rather than dated with a made-up time
    /// (`REQ-AUD-1`): the call was refused before anything of it was sent, so no call goes unrecorded (`REQ-MDL-8`).
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
                c.due.saturating_duration_since(self.clock.now())
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
            answered = matches!(
                self.handle_in(callee, &asked, None, true),
                Handled::Answered(_)
            );
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
        let own = self.clock.now() + self.deadline;
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
        // statement key only for the operation it was told is being handled. It is also what the operation's own
        // outside requests name (`Context::transaction`): an outside request names a transaction, so an operation
        // that names none is asked about by this one, which its module computes from the bytes it was handed
        // (`untransacted`), and its state, egress and limit records name it too (`T-024-18`).
        let handled_as = Identifier::new(transaction.clone()).or_else(|| untransacted(&json));
        // The world judges against the release that handles it, which the last one told may not be (`T-013-06`).
        self.tell_running(provider, &version);
        if let (Some(caller), Some(handled_as)) = (&caller, &handled_as) {
            self.world
                .begin_operation(caller, handled_as, self.wall(due));
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
            transaction: handled_as
                .as_ref()
                .map_or(transaction, |id| id.as_str().to_owned()),
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
            let received = self.receive(&mut process, due);
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
            // callee's is started only after the top-level operation's answer was handed back, at the platform's
            // next call, so its start, up to `START_DEADLINE`, delays neither its caller nor that answer.
            drop(process);
            if let Some(context) = context {
                if self.stack.is_empty() {
                    self.respawn(provider, &context.version);
                } else {
                    self.restarts.push((provider.to_owned(), context.version));
                }
            }
        }
        (outcome.map(|bytes| (bytes, key)), passed_out)
    }

    /// Starts again each callee's version whose process ended during an earlier top-level operation, before the
    /// platform does anything else, so every later call finds what an immediate restart would have left.
    fn restart_ended(&mut self) {
        for (provider, version) in std::mem::take(&mut self.restarts) {
            self.respawn(&provider, &version);
        }
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

/// What an operation that names no transaction (a verifier's judgement) is known by: `rutent:untransacted:` and the
/// SHA-256 of the operation's JSON, as the platform hands it to the module. A module computes it from the bytes it
/// was handed, to name it in an outside request.
fn untransacted_name(operation_json: &str) -> String {
    format!("rutent:untransacted:{}", sha256(operation_json.as_bytes()))
}

fn untransacted(operation_json: &str) -> Option<Identifier> {
    Identifier::new(untransacted_name(operation_json))
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

/// Whether what the platform handed back counts as an answer of the module's towards a staging's minimum
/// (`T-013-11`, `REQ-MDL-4`): a result it passed on whose outcome is not a refusal of the module's own and not
/// "unknown" or "not known", which say nothing of what became of the operation. A platform refusal is none.
fn is_an_answer(handled: &Handled) -> bool {
    let Handled::Answered(result) = handled else {
        return false;
    };
    serde_json::to_value(&result.statement)
        .ok()
        .and_then(|v| v["outcome"]["status"].as_str().map(str::to_owned))
        .is_some_and(|status| !matches!(status.as_str(), "refused" | "unknown" | "not_known"))
}

/// The kinds that only ask: they move nothing, so asking one again, under the same transaction, is no repeat. A hold
/// request among them (`T-020-56`'s review, finding 2): a held quote binds no provider and moves nothing, so the
/// platform keeps no carried record of it, which no evidence window would ever free.
const READ_ONLY_KINDS: [&str; 5] = [
    "balance_verification",
    "judgement",
    "holding_balance",
    "status_query",
    "hold",
];

/// The transaction an operation is a repeat under, if the platform carries out one operation of it only once: one
/// that names a transaction, is not a dry run and not a kind that only asks. A transaction's identifier is one
/// operation's (`INV-8`), so the kind doesn't matter; any kind, one added later too, counts (`T-013-09`).
fn repeat_key(operation: &Operation) -> Option<String> {
    let value = serde_json::to_value(operation).ok()?;
    if value["dry_run"].as_bool() == Some(true) {
        return None;
    }
    let kind = value["kind"].as_str()?;
    if READ_ONLY_KINDS.contains(&kind) {
        return None;
    }
    Some(value["transaction"].as_str()?.to_owned())
}

fn result_matches(result: &ResultBody, operation: &Operation) -> bool {
    let (Ok(result), Ok(operation)) = (
        serde_json::to_value(result),
        serde_json::to_value(operation),
    ) else {
        return false;
    };
    // A status answer is to the transaction it was asked about too, as to the one it was asked under.
    result["kind"] == operation["kind"]
        && result["transaction"] == operation["transaction"]
        && result["about"] == operation["about"]
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
        // A record this platform could write to the end ends whole: the next opening knows every decision this one took
        // (`T-013-11`), and holds again only the providers that still wait for the operator's word (`T-020-49`). One
        // whose write failed takes no more, so it ends with neither, and the next opening holds every provider.
        if !self.record_failed {
            let close = if self.unconfirmed.is_empty() {
                Entry::Closed
            } else {
                Entry::ClosedHolding {
                    held: self.unconfirmed.iter().cloned().collect(),
                }
            };
            let _ = self.write(&close);
        }
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
    /// and a release of the provider admitted under that version or waiting for an acceptance, under any, or for
    /// the operator's confirmation (`AdmissionPending`, unless the record holds this release), are judged before the
    /// admitting compiler runs. After it, only the compiler's refusal and the judge's refusal to
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
        self.restart_ended();
        // An automatic acceptance is recorded at the caller's clock: one no record's time can hold admits nothing.
        let Some(admitted_at) = time_of(at) else {
            return Admitted::ClockOutOfRange;
        };
        // Judged at the platform's clock, never the assertion's own `made_at` (provider-role 4.0.0).
        let versions = rutent_contracts::provider_role::ProtocolVersions::only(
            self.envelope.protocol_version.as_str(),
        );
        let assertion = match admission::read(
            received_assertion,
            &self.envelope.deployment,
            &versions,
            admitted_at.as_str(),
        ) {
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
        // A release the platform's record holds as accepted, which a restart now admits again: accepted as it was,
        // by whom and when, and put where the record says it stood, never staged as a new upgrade (`T-013-11`).
        let held = self
            .lifecycle
            .is_accepted(&provider, &version)
            .then(|| {
                self.acceptances
                    .get(&provider)
                    .and_then(|accepted| accepted.iter().find(|a| a.version == version))
                    .cloned()
            })
            .flatten();
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
        // A version the record holds as accepted names the one release that was accepted: only an assertion of the
        // digest it recorded is that release, accepted as it was; any other under the version is refused, since a
        // version names one release and the acceptance a person gave isn't theirs to lend (`REQ-MDL-1`, `REQ-MDL-3`,
        // `REQ-AUD-1`, `T-013-11`).
        let restored = if self.lifecycle.is_accepted(&provider, &version) {
            match held {
                Some(held) if held.assertion_digest == artefact.assertion_digest.value.as_str() => {
                    Some(held)
                }
                _ => return Admitted::Refused(AdmissionRefusal::VersionAlreadyAdmitted),
            }
        } else {
            None
        };
        // While a release is staged, no other is admitted: its staging ends first, promoted or rolled back
        // (ADR-0014). And a release asserted before the provider's last accepted one is an older signed release,
        // which publishing never reinstates (`REQ-MDL-4`, `THR-17`).
        if restored.is_none() && self.lifecycle.staged(&provider).is_some() {
            return Admitted::Refused(AdmissionRefusal::ReleaseStaged);
        }
        // While the provider's releases wait for the operator's confirmation, no new one is admitted: what the
        // record holds is what the operator confirms, and a release accepted or staged now would read as one placed
        // before a restrictive decision the platform took and couldn't write (`T-013-11`, `T-020-66`). Only a
        // release the record holds is admitted again. The release waiting is the provider's confirmation, so the
        // refusal is the one for a waiting acceptance.
        if restored.is_none() && self.unconfirmed.contains(&provider) {
            return Admitted::Refused(AdmissionRefusal::AdmissionPending);
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
        if let Some(held) = &restored {
            let Some(at) = time_of(Clock(held.at)) else {
                return Admitted::ClockOutOfRange;
            };
            artefact.acceptance.accepted = Some(Accepted {
                by: held.by.as_deref().and_then(Identifier::new),
                at,
            });
        } else if !fresh {
            artefact.acceptance.accepted = Some(Accepted {
                by: None,
                at: admitted_at,
            });
        }
        let slot = self.providers.entry(provider.clone()).or_default();
        slot.safe_retry = judge.safe_retry(&provider);
        slot.evidence_window = judge.evidence_window(&provider);
        if restored.is_some() {
            slot.accepted.push(artefact.clone());
            slot.machines.insert(version.clone(), machine);
            self.start_restored(&provider, &version);
        } else if fresh {
            slot.pending = Some((artefact.clone(), machine, received_assertion.to_vec()));
        } else if !self.accept_and_start(&provider, &artefact, Rc::clone(&machine), None, at) {
            // An acceptance the platform's record can't hold isn't given: the release waits for one, which the
            // operator gives once the record can be written (`T-013-11`).
            artefact.acceptance.accepted = None;
            if let Some(slot) = self.providers.get_mut(&provider) {
                slot.pending = Some((artefact.clone(), machine, received_assertion.to_vec()));
            }
        }
        Admitted::Artefact(Box::new(artefact))
    }

    /// Gives a fresh acceptance, by `by`, to the provider's release that waits for one, and starts it. The
    /// acceptance is of one assertion: `assertion_digest` must be the waiting artefact's, the digest of its
    /// assertion as received (`REQ-MDL-1`). That assertion is judged again by `judge` at the caller's clock `at`,
    /// so a release signed with a key retired since its admission is no longer accepted (`THR-4`), and the
    /// acceptance is recorded at `at`. The accepted artefact, or `None` if no release waits under that digest, `by`
    /// names no one, the assertion no longer passes, `at` is past what a record's time can hold (the year 9999), or
    /// the provider's releases wait for the operator's confirmation (`T-020-66`); the waiting release then stays as
    /// it was.
    pub fn accept(
        &mut self,
        provider: &str,
        assertion_digest: &Digest,
        by: &str,
        judge: &impl ReleaseJudge,
        at: Clock,
    ) -> Option<AdmissionArtefact> {
        self.restart_ended();
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
        slot.safe_retry = judge.safe_retry(provider);
        slot.evidence_window = judge.evidence_window(provider);
        let (waiting, machine, _) = slot.pending.as_ref()?;
        let (mut artefact, machine) = (waiting.clone(), Rc::clone(machine));
        artefact.acceptance.accepted = Some(Accepted {
            by: Some(by.clone()),
            at: accepted_at,
        });
        // Written to the platform's record before it takes effect: a release the record can't hold isn't accepted,
        // and stays waiting (`T-013-11`).
        if !self.accept_and_start(provider, &artefact, machine, Some(&by), at) {
            return None;
        }
        self.providers.get_mut(provider)?.pending = None;
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

    /// The operating-system process the provider's module runs in, if it runs. A callee whose process ended during
    /// a top-level operation has none until the platform's next call starts it again.
    pub fn process(&self, provider: &str) -> Option<ProcessId> {
        let slot = self.providers.get(provider)?;
        slot.process.as_ref().map(|p| ProcessId::from(p.pid()))
    }

    /// Restarts the provider's process, and no other. False if it has none.
    pub fn restart(&mut self, provider: &str) -> bool {
        self.restart_ended();
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
    /// segment's version was declared exploited (`T-013-06`). A second operation of a kind under a transaction it
    /// carried out is refused as `DuplicateTransaction`, the module not run, unless the provider declares a repeat
    /// safe and it is the same operation, and then it is answered from the platform's record: the first answer,
    /// not a second run. Where the platform can't tell whether the provider declared it, it is refused (`T-013-09`;
    /// `INV-8`, `REQ-CAT-10`).
    ///
    /// The operation is written to the platform's record before the module is handed it, and refused as
    /// `Unavailable`, nothing of it run, where it can't be; its answer is written once the module ran, so a platform
    /// restarted on the record answers a repeat as this one would (`T-013-10`). An operation carried out with no
    /// segment is held until its provider's evidence window passes, and its transaction is spent from then on.
    pub fn handle(&mut self, provider: &str, operation: &Statement<Operation>) -> Handled {
        self.restart_ended();
        self.free_spent();
        // A status question is put through [`Platform::status`] alone, which holds it to the transaction it asks about
        // (`T-013-11`): handed here, it would run on the version in force and be signed for anything the module nests.
        if matches!(operation.statement, Operation::StatusQuery(_)) {
            return Handled::Refused(PlatformRefusal::NotHandled);
        }
        let transaction = transaction_of(&operation.statement);
        self.handle_in(provider, operation, transaction.as_deref(), false)
    }

    /// Asks the provider's module where the transaction `about` is of stands (`REQ-ORD-1`, `T-013-11`; provider-role
    /// 4.0.0), under `asking`, the question's own transaction, and hands back the provider's answer signed as any
    /// result is. `about` is an operation the platform carried out at this provider: one it holds a record of, or
    /// whose segment is in flight there. The question is made from it: the same deployment, protocol version, maker
    /// and time, naming `asking`, the provider and the transaction asked about. The platform signs and verifies no
    /// operation, so the maker's own signature is left as `about` carries it, and a caller that signs its own
    /// questions hands the platform the one it signed through [`Platform::handle`].
    ///
    /// The question moves nothing: it is no operation carried out (it is no repeat of itself nor of `about`), no
    /// record is kept of it, and the transaction asked about is left as it was, its segment in flight on the version
    /// it runs on, which answers; the provider's module is asked only if its release lists `status_query` in
    /// `handles`, and is `NotHandled` otherwise, never run. Refused as `DuplicateTransaction` where `asking` is the
    /// transaction asked about or one the platform holds a record of, a segment of or ended: a question has a
    /// transaction of its own. Refused as `Unavailable`, nothing run, where the platform carried nothing out under
    /// `about` at this provider. As for any operation, a version declared exploited answers nothing.
    pub fn status(
        &mut self,
        provider: &str,
        asking: &Identifier,
        about: &Statement<Operation>,
    ) -> Handled {
        self.restart_ended();
        self.free_spent();
        let Some(asked_about) = transaction_of(&about.statement) else {
            return Handled::Refused(PlatformRefusal::Unavailable);
        };
        let record = self
            .carried
            .get(&asked_about)
            .and_then(|scopes| scopes.get(TOP_LEVEL))
            .filter(|carried| carried.provider == provider);
        let in_a_segment = self
            .lifecycle
            .segment(&asked_about)
            .is_some_and(|segment| segment.provider == provider);
        // The kind of the operation asked about is the platform's own record's, never the caller's: a segment begun with
        // nothing carried out under it in the record has no operation to ask about.
        let Some(record) = record else {
            return Handled::Refused(PlatformRefusal::Unavailable);
        };
        // What the platform holds is what it asks about: the operation it carried out, and the answer it holds, not
        // what the caller says of them. On the version the transaction's segment runs on, or, where none holds it, the
        // one that carried it out; where the record doesn't keep which, nothing is asked.
        let operation = record.operation.statement.clone();
        let held = record.answer.as_deref().map(|a| a.statement.clone());
        let version = if in_a_segment {
            None
        } else {
            match record.version.clone() {
                Some(version) => Some(version),
                None => return Handled::Refused(PlatformRefusal::Unavailable),
            }
        };
        let own = asking.as_str();
        if own == asked_about
            || self.carried.contains_key(own)
            || self.lifecycle.has_ended(own)
            || self.lifecycle.segment(own).is_some()
        {
            return Handled::Refused(PlatformRefusal::DuplicateTransaction);
        }
        let mut question = about.clone();
        question.statement = Operation::StatusQuery(StatusQuery {
            transaction: own.to_owned(),
            provider: provider.to_owned(),
            about: asked_about.clone(),
        });
        self.status_run = Some(StatusRun {
            version,
            about: operation,
            held,
        });
        let handled = self.handle_in(provider, &question, Some(&asked_about), false);
        self.status_run = None;
        handled
    }

    /// The operator's confirmation, by `by` at the caller's clock `at`, of the provider's releases as the platform's
    /// record holds them, after a restart whose record didn't end with the platform's clean close (`T-013-11`). Until
    /// then nothing of the provider runs: a restrictive decision the platform took and couldn't write before it stopped
    /// (`LifecycleRefusal::TakenUnrecorded`) isn't in the record, and a release it declared exploited would otherwise
    /// run again. The operator takes such a decision again first, then confirms; the versions the record holds in force,
    /// staged or under a segment in flight then run, unless marked. Written to the record before it is taken, and
    /// refused as `Unrecorded` where it can't be. Refused as `NoSuchVersion` where nothing of the provider waits.
    pub fn confirm(&mut self, provider: &str, by: &str, at: Clock) -> Result<(), LifecycleRefusal> {
        self.restart_ended();
        if !self.unconfirmed.contains(provider) {
            return Err(LifecycleRefusal::NoSuchVersion);
        }
        self.decide(|lifecycle| lifecycle.confirmed(provider, by, at), None)?;
        self.unconfirmed.remove(provider);
        let admitted: Vec<String> = self
            .lifecycle
            .wanted(provider)
            .into_iter()
            .filter(|version| {
                self.providers.get(provider).is_some_and(|slot| {
                    slot.accepted
                        .iter()
                        .any(|a| a.declaration().version.as_str() == version.as_str())
                        && slot.active.as_deref() != Some(version.as_str())
                        && !slot.others.contains_key(version)
                })
            })
            .collect();
        for version in admitted {
            self.start_restored(provider, &version);
        }
        Ok(())
    }

    /// The versions of the provider's releases held out of force until the operator confirms them (`Platform::confirm`),
    /// as the record holds them: the one in force, the staged one and every one a segment in flight runs on. `None` where
    /// nothing of the provider waits (`T-013-11`, `T-020-49`).
    pub fn awaiting_confirmation(&self, provider: &str) -> Option<BTreeSet<String>> {
        self.unconfirmed
            .contains(provider)
            .then(|| self.lifecycle.wanted(provider))
    }

    /// [`Platform::handle`], in the segment of `transaction` if one holds it, and on the version in force for
    /// `None`.
    fn handle_in(
        &mut self,
        provider: &str,
        operation: &Statement<Operation>,
        transaction: Option<&str>,
        called: bool,
    ) -> Handled {
        *self
            .operations_received
            .entry(provider.to_owned())
            .or_default() += 1;
        if self.unconfirmed.contains(provider) {
            return Handled::Refused(PlatformRefusal::Unavailable);
        }
        // A status question about a transaction no segment holds runs on the version that carried it out.
        let carried_on = match (&operation.statement, called) {
            (Operation::StatusQuery(_), false) => {
                self.status_run.as_ref().and_then(|run| run.version.clone())
            }
            _ => None,
        };
        let version = match carried_on {
            Some(version) => match self.lifecycle.mark(provider, &version) {
                Some(lifecycle::Mark::Exploited) => {
                    return Handled::Refused(PlatformRefusal::VersionExploited);
                }
                _ => version,
            },
            None => match self.lifecycle.version_for(provider, transaction) {
                Ok(version) => version,
                Err(refusal) => return Handled::Refused(refusal),
            },
        };
        // A callee's record has the callee's own scope, so a transaction a module asks another provider about is no
        // collision with the transaction's own (`T-013-09`).
        let scope = if called { provider } else { TOP_LEVEL };
        let repeat = repeat_key(&operation.statement).map(|t| (t, scope.to_owned()));
        if let Some((transaction, scope)) = &repeat
            && let Some(answered_or_refused) =
                self.repeated(provider, transaction, scope, operation)
        {
            return answered_or_refused;
        }
        let Some(declaration) = self.declaration_of(provider, &version) else {
            return Handled::Refused(PlatformRefusal::Unavailable);
        };
        // Written before the module is handed it, so a platform stopped while the module runs reopens with it carried
        // out (`T-013-10`). Where the record holds it already, a first run that gave no answer, nothing is written
        // again. An operation the record can't hold is never handed over: a restart could lose it.
        let written_ahead = match &repeat {
            Some((transaction, scope))
                if !self
                    .carried
                    .get(transaction)
                    .is_some_and(|scopes| scopes.contains_key(scope)) =>
            {
                let entry = Entry::Carrying {
                    transaction: transaction.clone(),
                    scope: scope.clone(),
                    provider: provider.to_owned(),
                    operation: Box::new(operation.clone()),
                };
                if self.write(&entry).is_err() {
                    return Handled::Refused(PlatformRefusal::Unavailable);
                }
                true
            }
            _ => false,
        };
        // Only an operation the version's process ran counts towards a staging: not one refused as not handled,
        // nor one that found its process gone.
        let key = (provider.to_owned(), version.clone());
        let before = self.runs.get(&key).copied().unwrap_or_default();
        let handled = self.handle_on(provider, operation, declaration, version.clone());
        if self.runs.get(&key).copied().unwrap_or_default() > before {
            // A status question asks about an operation that was counted when it ran, so it counts for nothing: a version
            // that answers every operation as unknown would otherwise reach its minimum on the questions about them
            // (`T-013-11`).
            let asks_status = matches!(operation.statement, Operation::StatusQuery(_));
            let answered = is_an_answer(&handled);
            let counted =
                (!asks_status && self.lifecycle.staged(provider).is_some()).then_some(answered);
            if !asks_status {
                self.lifecycle.observe(provider, &version, answered);
            }
            // The module ran: whatever it came to, the operation was carried out (`T-013-09`). A first outcome
            // unknown is the case a repeat is least safe in.
            let mut kept = None;
            if let Some((transaction, scope)) = repeat {
                let answer = match &handled {
                    Handled::Answered(result) => Some(result.clone()),
                    Handled::Refused(_) => None,
                };
                let free_at = self.free_at(provider, &transaction);
                let carried = self.carried.entry(transaction.clone()).or_default();
                // The first answer stays the record, whoever asks again.
                if carried
                    .get(&scope)
                    .is_none_or(|first| first.answer.is_none())
                {
                    let first = Carried {
                        provider: provider.to_owned(),
                        operation: operation.clone(),
                        answer,
                        free_at,
                        version: Some(version.clone()),
                    };
                    kept = Some(Box::new(first.entry(&transaction, &scope)));
                    carried.insert(scope, first);
                }
            }
            // A record that can't be written holds the operation as handed over with no answer, if it was one carried
            // out once: a restarted platform refuses its repeat, or runs it again where that's declared safe.
            let _ = self.write(&Entry::Ran {
                provider: provider.to_owned(),
                version,
                carried: kept,
                answered: counted,
            });
        } else if written_ahead && let Some((transaction, scope)) = repeat {
            // Nothing of it ran: the record lets it go, and the transaction is free, as it was. One that can't be
            // written keeps it carried out with no answer, which fails closed.
            let _ = self.write(&Entry::Withdrawn { transaction, scope });
        }
        handled
    }

    /// What a repeat of `operation`, an operation of `transaction` asked in `scope`, gets, if it is one: nothing
    /// where the platform carried none out under that transaction in that scope. Otherwise the platform answers
    /// it, the module never run: from its record where the provider declares a repeat safe and the operation is the
    /// first one, and refused as `DuplicateTransaction` where it doesn't declare it, where it declares nothing,
    /// where the platform can't tell, where the first was another provider's, and whatever the kind or the
    /// envelope: the platform verifies no signature, so a changed key or protocol version is a retry (`INV-8`,
    /// `REQ-CAT-10`). Where the provider declares a repeat safe, an operation of other terms or another envelope is
    /// no repeat of the first but a second operation under one identifier, refused as `DuplicateTransaction` too,
    /// at the top level and in a callee's scope alike (`REQ-MDL-14`, `T-013-10`): a declared safe retry covers the
    /// same operation asked again, nothing else. Only there does the module run again: the same operation, where
    /// its first run gave no answer, and the same call into a callee.
    fn repeated(
        &self,
        provider: &str,
        transaction: &str,
        scope: &str,
        operation: &Statement<Operation>,
    ) -> Option<Handled> {
        let first = self.carried.get(transaction)?.get(scope)?;
        let safe = self.providers.get(provider).and_then(|s| s.safe_retry) == Some(true);
        if first.provider != provider || !safe {
            return Some(Handled::Refused(PlatformRefusal::DuplicateTransaction));
        }
        // Only the same operation is a repeat, in any scope: its deployment, protocol version, shape, maker,
        // signature scheme and key, and its whole statement. Its `made_at` and signature value are left out, since a
        // retry is the same operation made again later and signed again, and the platform verifies no signature
        // (`REQ-MDL-14`, `T-013-10`).
        let (before, now) = (&first.operation, operation);
        let same = before.deployment == now.deployment
            && before.protocol_version == now.protocol_version
            && before.shape == now.shape
            && before.maker == now.maker
            && before.signature.scheme == now.signature.scheme
            && before.signature.key == now.signature.key
            && before.statement == now.statement;
        if !same {
            return Some(Handled::Refused(PlatformRefusal::DuplicateTransaction));
        }
        // A callee that declares a repeat safe runs the same call again: each call is an operation of its own,
        // within its budgets, and the declaration says it may.
        if scope != TOP_LEVEL {
            return None;
        }
        first.answer.clone().map(Handled::Answered)
    }

    /// How many operations the provider's module ran, on any of its versions: its process took the operation and
    /// said something of it. An operation the platform refused or answered from its record is not one.
    pub fn operations_run(&self, provider: &str) -> u64 {
        self.runs
            .iter()
            .filter(|((p, _), _)| p == provider)
            .map(|(_, n)| n)
            .sum()
    }

    /// How many operations the provider's `version` ran: its process took the operation and said something of it.
    pub fn operations_run_on(&self, provider: &str, version: &str) -> u64 {
        self.runs
            .get(&(provider.to_owned(), version.to_owned()))
            .copied()
            .unwrap_or_default()
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
                about: match (&operation.statement, &self.status_run) {
                    (Operation::StatusQuery(_), Some(run)) => Some(proxy::AboutAsked {
                        operation: &run.about,
                        held: run.held.as_ref(),
                    }),
                    _ => None,
                },
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
        self.restart_ended();
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
    /// marks that version stale. A transaction a segment holds or held, or one the platform carried out an
    /// operation of with no segment, is refused as `DuplicateTransaction` (`INV-8`, `REQ-MDL-14`, `T-013-10`), and a
    /// provider with no version in force as `Unavailable`. The segment is written to the platform's record before
    /// it is begun; one the record can't hold is refused as `Unavailable`, and nothing of it began.
    pub fn begin_segment(
        &mut self,
        provider: &str,
        transaction: &Identifier,
    ) -> Result<String, PlatformRefusal> {
        self.restart_ended();
        self.free_spent();
        if self.unconfirmed.contains(provider) {
            return Err(PlatformRefusal::Unavailable);
        }
        if self.carried.contains_key(transaction.as_str()) {
            return Err(PlatformRefusal::DuplicateTransaction);
        }
        let before = self.lifecycle.release_state(provider);
        let version = self.lifecycle.begin_segment(provider, transaction)?;
        // The segment, and the staging's counts it changed (the segments begun and the share taken), in one entry
        // (`T-013-11`).
        let release = (self.lifecycle.release_state(provider) != before)
            .then(|| {
                let accepted = self
                    .acceptances
                    .get(provider)
                    .map_or(&[][..], Vec::as_slice);
                Self::releases_entry(&self.lifecycle, provider, accepted).map(Box::new)
            })
            .flatten();
        let written = match self.lifecycle.segment(transaction.as_str()) {
            Some(segment) => {
                let entry = Entry::Began {
                    segment: segment.into(),
                    release,
                };
                self.write(&entry)
            }
            None => Ok(()),
        };
        if written.is_err() {
            self.lifecycle.forget_segment(transaction.as_str());
            return Err(PlatformRefusal::Unavailable);
        }
        Ok(version)
    }

    /// Ends the segment of `transaction` and hands over its record: the version it began on, the one it ran on and
    /// each move between them. Its transaction never begins another, and a version nothing runs on any more stops.
    pub fn end_segment(&mut self, transaction: &str) -> Option<SegmentRecord> {
        self.restart_ended();
        let record = self.lifecycle.end_segment(transaction)?;
        self.carried.remove(transaction);
        // A record that can't be written holds the segment in flight and its operations carried out: a restarted
        // platform refuses their repeats all the same, and the segment can be ended again.
        let _ = self.write(&Entry::Ended {
            transaction: transaction.to_owned(),
        });
        self.reconcile(&record.provider);
        Some(record)
    }

    /// Withdraws, by `by` at the caller's clock `at`, the provider's release that waits for its fresh acceptance
    /// under `assertion_digest`, accepted or not since: one refused on being judged again stays waiting until it
    /// is withdrawn. The provider's next release may then be admitted, under another version. The withdrawn
    /// artefact, or `None` if no release waits under that digest or `by` names no one.
    ///
    /// Written to the platform's record before it is taken; `None`, nothing changed, where it can't be
    /// (`T-013-10`).
    pub fn withdraw(
        &mut self,
        provider: &str,
        assertion_digest: &Digest,
        by: &str,
        at: Clock,
    ) -> Option<AdmissionArtefact> {
        self.restart_ended();
        Identifier::new(by)?;
        let (waiting, _, _) = self.providers.get(provider)?.pending.as_ref()?;
        if &waiting.assertion_digest != assertion_digest {
            return None;
        }
        let version = waiting.declaration().version.as_str().to_owned();
        self.decide(
            |lifecycle| lifecycle.withdrawn(provider, &version, by, at),
            None,
        )
        .ok()?;
        let slot = self.providers.get_mut(provider)?;
        let (artefact, _, _) = slot.pending.take()?;
        slot.withdrawn.insert(version);
        Some(artefact)
    }

    /// Puts the provider's staged release in force, by `by` at the caller's clock `at`, once its time window has
    /// passed and it has answered its minimum, refusals not counted (`REQ-MDL-4`, ADR-0014, `T-013-11`). Segments in flight stay where they run.
    /// Refused as `StagedNotRunning` while the staged version has no live process, so a promotion never leaves the
    /// provider with no version in force.
    ///
    /// Written to the platform's record before it is taken, and refused as `Unrecorded`, nothing changed, where it
    /// can't be (`T-013-10`).
    pub fn promote(
        &mut self,
        provider: &str,
        by: &str,
        at: Clock,
    ) -> Result<String, LifecycleRefusal> {
        self.restart_ended();
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
        let promoted = self.decide(|lifecycle| lifecycle.promote(provider, by, at), None)?;
        self.reconcile(provider);
        Ok(promoted)
    }

    /// Rolls the provider's staged release back, by `by`: the version in force stays, and segments on the staged
    /// one finish on it.
    ///
    /// Written to the platform's record before it is taken, and refused as `Unrecorded`, nothing changed, where it
    /// can't be (`T-013-10`).
    pub fn roll_back(
        &mut self,
        provider: &str,
        by: &str,
        at: Clock,
    ) -> Result<String, LifecycleRefusal> {
        self.restart_ended();
        let rolled_back = self.decide(|lifecycle| lifecycle.roll_back(provider, by, at), None)?;
        self.reconcile(provider);
        Ok(rolled_back)
    }

    /// Revokes the provider's `version`, by `by`: it takes no new segment, and those in flight on it finish on it
    /// (`REQ-MDL-20`). Revoking the version in force puts the one before it back in force, a recorded decision; the
    /// version put back, if any.
    ///
    /// Written to the platform's record before it is taken. Where it can't be, it only restricts what runs, so it
    /// is taken all the same, in memory, putting no version back in force, and answered `TakenUnrecorded`: not
    /// durable, to be taken again after a restart (`T-013-10`).
    pub fn revoke(
        &mut self,
        provider: &str,
        version: &str,
        by: &str,
        at: Clock,
    ) -> Result<Option<String>, LifecycleRefusal> {
        self.restart_ended();
        let back = self.decide(
            |lifecycle| lifecycle.revoke(provider, version, by, at),
            Some(Box::new(|lifecycle| {
                lifecycle.revoke_in_place(provider, version, by, at)
            })),
        );
        self.reconcile(provider);
        back
    }

    /// Declares the provider's `version` actively exploited, by `by`: its process is stopped and its segments are
    /// refused as `VersionExploited`, not moved (`REQ-MDL-20`). The version put back in force, if any.
    ///
    /// Written to the platform's record before it is taken. Where it can't be, it only restricts what runs, so it
    /// is taken all the same, in memory, putting no version back in force, and answered `TakenUnrecorded`: not
    /// durable, to be taken again after a restart (`T-013-10`).
    pub fn declare_exploited(
        &mut self,
        provider: &str,
        version: &str,
        by: &str,
        at: Clock,
    ) -> Result<Option<String>, LifecycleRefusal> {
        self.restart_ended();
        let back = self.decide(
            |lifecycle| lifecycle.declare_exploited(provider, version, by, at),
            Some(Box::new(|lifecycle| {
                lifecycle.declare_exploited_in_place(provider, version, by, at)
            })),
        );
        self.reconcile(provider);
        back
    }

    /// Marks the provider's `version` stale, by `by`, the operator's decision and never the publisher's: its
    /// segments in flight move to the version in force, each move recorded in its segment's record (`REQ-MDL-5`,
    /// `REQ-MDL-19`). Refused while a release is staged (ADR-0014). How many segments moved.
    ///
    /// Written to the platform's record before it is taken. Where it can't be, it only restricts what runs, so it
    /// is taken all the same, in memory, putting no version back in force, and answered `TakenUnrecorded`: not
    /// durable, to be taken again after a restart (`T-013-10`).
    pub fn mark_stale(
        &mut self,
        provider: &str,
        version: &str,
        by: &str,
        at: Clock,
    ) -> Result<usize, LifecycleRefusal> {
        self.restart_ended();
        let moved = self.decide(
            |lifecycle| lifecycle.mark_stale(provider, version, by, at),
            Some(Box::new(|lifecycle| {
                lifecycle.mark_stale(provider, version, by, at).map(|_| ())
            })),
        );
        self.reconcile(provider);
        moved
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

    /// A world with nothing in it: `repeated` asks it nothing.
    struct Empty;

    impl super::World for Empty {
        fn secret(&mut self, _: &str, _: &str) -> Option<String> {
            None
        }

        fn state(&mut self, _: &str, _: &str) -> Option<String> {
            None
        }

        fn route(
            &mut self,
            _: &super::Endpoint,
        ) -> Option<(std::net::IpAddr, Option<super::Endpoint>)> {
            None
        }

        fn deliver(&mut self, _: &super::Endpoint, _: &str) {}
    }

    /// A call into a callee that declares a repeat safe runs again only where it is the same call: one under the
    /// same transaction with other terms, or of another maker or shape, is a second operation under one
    /// identifier, refused before the callee runs, as at the top level (`REQ-MDL-14`, `INV-8`, `T-013-10`). The
    /// boundary suite's stand-ins make each call from the caller's own operation, so this is judged here, on the
    /// record the platform keeps.
    // verifies: REQ-MDL-14, INV-8
    #[test]
    fn a_call_with_other_terms_into_a_safe_retry_callee_under_one_transaction_is_refused() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
            "../../contracts/provider-role/fixtures/valid/operation-withdrawal-from-a-bank-reserve.json",
        );
        let first: rutent_contracts::provider_role::Statement<
            rutent_contracts::provider_role::Operation,
        > = serde_json::from_str(&std::fs::read_to_string(path).expect("the fixture reads"))
            .expect("an operation");
        let mut platform = super::Platform::new(
            super::Paths {
                compiler: std::path::PathBuf::new(),
                provider: std::path::PathBuf::new(),
            },
            Empty,
            super::Envelope {
                deployment: first.deployment.clone(),
                protocol_version: first.protocol_version.clone(),
            },
        )
        .expect("a platform");
        let callee = "callee";
        platform
            .providers
            .entry(callee.to_owned())
            .or_default()
            .safe_retry = Some(true);
        for scope in [callee, super::TOP_LEVEL] {
            platform.carried.entry("tx".to_owned()).or_default().insert(
                scope.to_owned(),
                super::Carried {
                    provider: callee.to_owned(),
                    operation: first.clone(),
                    answer: None,
                    free_at: None,
                    version: None,
                },
            );
        }
        let changed = |change: fn(&mut serde_json::Value)| {
            let mut v = serde_json::to_value(&first).expect("JSON");
            change(&mut v);
            serde_json::from_value(v).expect("an operation")
        };
        let others: [(&str, rutent_contracts::provider_role::Statement<_>); 3] = [
            (
                "amount",
                changed(|v| v["statement"]["amount"] = "999999".into()),
            ),
            ("maker", changed(|v| v["maker"] = "someone-else".into())),
            (
                "shape",
                changed(|v| v["shape"] = "urn:rutent:provider-role:other".into()),
            ),
        ];
        for scope in [callee, super::TOP_LEVEL] {
            for (what, other) in &others {
                assert_eq!(
                    platform.repeated(callee, "tx", scope, other),
                    Some(super::Handled::Refused(
                        PlatformRefusal::DuplicateTransaction
                    )),
                    "{scope:?}: {what}"
                );
            }
        }
        // The same call runs again, as the callee's declaration allows, and so does the top-level operation whose
        // first run gave no answer.
        assert_eq!(platform.repeated(callee, "tx", callee, &first), None);
        assert_eq!(
            platform.repeated(callee, "tx", super::TOP_LEVEL, &first),
            None
        );
    }

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

    /// A status answer is a result of the question only if it is about the transaction asked about as well as under
    /// the question's own (`T-013-11`).
    // verifies: REQ-ORD-1
    #[test]
    fn a_status_answer_about_another_transaction_is_no_result_of_the_question() {
        let fixture = |name: &str| -> serde_json::Value {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../contracts/provider-role/fixtures/valid")
                .join(format!("{name}.json"));
            serde_json::from_slice(&std::fs::read(path).expect("the fixture")).expect("JSON")
        };
        let question: rutent_contracts::provider_role::Statement<
            rutent_contracts::provider_role::Operation,
        > = serde_json::from_value(fixture("operation-status-query")).expect("a question");
        let answer = fixture("result-status-query-not-known");
        let body = |value: &serde_json::Value| -> rutent_contracts::provider_role::ResultBody {
            serde_json::from_value(value["statement"].clone()).expect("a result")
        };
        assert!(super::result_matches(&body(&answer), &question.statement));
        let mut other = answer.clone();
        other["statement"]["about"] = "tx-another".into();
        assert!(!super::result_matches(&body(&other), &question.statement));
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
            AnswerOfAnotherQuestion,
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
