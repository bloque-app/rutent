//! [`PvmBinding`]: the module-boundary suite's `ModuleBinding` and `BoundaryHarness` for PolkaVM programs.
//!
//! This process is the platform. It admits releases, through the admitting compiler in its own process; keeps
//! each provider's history; runs each provider's module in that provider's own worker process; and is the
//! credential proxy, the state store, the egress layer and the cross-module router every worker reaches through
//! its pipe. Everything the suite reads as evidence, lookups, state reads, outbound call records and cross-module
//! call records, is written here, from what crossed a pipe, never from what a guest says about itself.

use std::collections::BTreeMap;
use std::io::BufReader;
use std::net::IpAddr;
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout};
use std::time::Instant;

use rutent_conformance::boundary::{
    AdmissionRefusal, Admitted, BoundaryHarness, Endpoint, Events, Handled, LookupEvent,
    LookupOutcome, ModuleBinding, ModuleSpec, PlatformRefusal, ProcessId, StateEvent,
};
use rutent_conformance::role::placeholder_signature;
use rutent_contracts::module_interface::{
    Acceptance, AcceptanceMode, Accepted, Address, AdmissionArtefact, Build, CallOutcome,
    CallRefusalReason, CallRefused, Capabilities, CrossModuleCall, CrossModuleOutcome, Declaration,
    Destination, Digest, DigestAlgorithm, DigestValue, Hop, Host, Identifier, Name, Nothing,
    OperationKind, OutboundCallRecord, PartName, Parts, RefusalReason, Refused, ReleaseAssertion,
    RouteReference, Scheme, SecretLookup, SecretRefusalReason, Time, validate_lookup, widening,
};
use rutent_contracts::provider_role::{
    Operation, RESULT_SHAPE, ResultBody, Signature, SignatureScheme, Statement,
};
use serde_json::Value;

use crate::egress::allows;
use crate::engine;
use crate::isolation::{self, Role};
use crate::protocol::{
    AdmitRequest, AdmitResponse, FromWorker, HostCall, Reply, ToWorker, hex, receive, send, unhex,
};
use crate::sha256::hex_digest;
use crate::spec;

/// The format release assertions name (`EXP-2` item `exp2-format-names`).
pub const FORMAT: &str = "polkavm";

/// The stand-ins' provider categories (`DEC-7`).
pub const CATEGORIES: [&str; 4] = ["reserve", "exchange", "ramp", "verifier"];

/// The languages the stand-ins are written in (`EXP-2` item `exp2-languages`).
pub const LANGUAGES: [&str; 2] = ["rs", "c"];

/// Where the binding's other programs and the compiled stand-ins are.
#[derive(Debug, Clone)]
pub struct Setup {
    /// The worker program, `rutent-pvm-worker`.
    pub worker: PathBuf,
    /// The admitting compiler, `rutent-pvm-admit`.
    pub admit: PathBuf,
    /// The compiled stand-ins: `<language>/<part>-<category>.polkavm`.
    pub guests: PathBuf,
}

/// Measurements the run note records (scoring sheet `B-06`): never a target.
#[derive(Debug, Clone, Default)]
pub struct Stats {
    /// Each admission's compile time, inside the admitting compiler, in microseconds.
    pub compile_micros: Vec<u128>,
    /// Each admission's whole round trip, process start included, in microseconds.
    pub admission_micros: Vec<u128>,
    /// Each load into a worker: compile, instantiate and start both parts, in microseconds.
    pub load_micros: Vec<u128>,
    /// Workers started.
    pub workers_started: usize,
    /// The most workers alive at once.
    pub most_workers: usize,
    /// Releases that didn't start, with why.
    pub not_started: Vec<String>,
    /// Admissions refused by the admitting compiler, with why.
    pub refused: Vec<String>,
    /// Loads whose digests differed from the admitted ones.
    pub digest_mismatches: usize,
    /// PolkaVM sandbox worker processes seen, under the Linux sandbox.
    pub guest_pids: usize,
}

struct Worker {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Worker {
    fn spawn(exe: &Path) -> std::io::Result<Self> {
        let mut child = isolation::spawn(Role::Worker, exe)?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("no stdout"))?;
        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    fn pid(&self) -> ProcessId {
        ProcessId::from(self.child.id())
    }

    fn send(&mut self, message: &ToWorker) -> bool {
        send(&mut self.stdin, message).is_ok()
    }

    fn receive(&mut self) -> Option<FromWorker> {
        receive(&mut self.stdout).ok().flatten()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Default)]
struct Slot {
    accepted: Vec<AdmissionArtefact>,
    pending: Option<AdmissionArtefact>,
    binaries: BTreeMap<String, Parts<Vec<u8>>>,
    active: Option<String>,
    worker: Option<Worker>,
}

/// The PolkaVM binding, for one provider category's stand-ins.
pub struct PvmBinding {
    setup: Setup,
    category: String,
    languages: Vec<String>,
    templates: BTreeMap<(String, &'static str), Vec<u8>>,
    output_limit: usize,
    secrets: BTreeMap<(String, String), String>,
    delivered: BTreeMap<String, Vec<String>>,
    state: BTreeMap<(String, String), String>,
    endpoints: BTreeMap<Endpoint, (IpAddr, Option<Endpoint>)>,
    received: BTreeMap<Endpoint, Vec<String>>,
    operations_received: BTreeMap<String, usize>,
    providers: BTreeMap<String, Slot>,
    serving: BTreeMap<String, String>,
    resolved: BTreeMap<(String, String, String), Vec<Name>>,
    clock: u32,
    events: Events,
    /// What the run measured.
    pub stats: Stats,
}

fn digest(bytes: &[u8]) -> Digest {
    Digest {
        algorithm: DigestAlgorithm::Sha256,
        value: DigestValue::new(hex_digest(bytes)).expect("a digest"),
    }
}

fn digest_of_hex(text: &str) -> Option<Digest> {
    Some(Digest {
        algorithm: DigestAlgorithm::Sha256,
        value: DigestValue::new(text)?,
    })
}

fn parse_url(url: &str) -> Option<Endpoint> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = match scheme {
        "https" => Scheme::Https,
        "wss" => Scheme::Wss,
        _ => return None,
    };
    let (host, port) = rest.rsplit_once(':')?;
    Some(Endpoint {
        scheme,
        host: host.to_owned(),
        port: port.parse().ok()?,
    })
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

impl PvmBinding {
    /// A binding for `category`'s stand-ins, in each language whose two compiled parts are in `setup.guests`.
    pub fn new(setup: Setup, category: &str) -> Self {
        let mut templates = BTreeMap::new();
        let mut languages = Vec::new();
        for language in LANGUAGES {
            let mut both = true;
            for part in ["sandboxed", "outside"] {
                let path = setup
                    .guests
                    .join(language)
                    .join(format!("{part}-{category}.polkavm"));
                match std::fs::read(&path) {
                    Ok(bytes) => {
                        templates.insert((language.to_owned(), part), bytes);
                    }
                    Err(_) => both = false,
                }
            }
            if both {
                languages.push(language.to_owned());
            }
        }
        Self {
            setup,
            category: category.to_owned(),
            languages,
            templates,
            output_limit: 0,
            secrets: BTreeMap::new(),
            delivered: BTreeMap::new(),
            state: BTreeMap::new(),
            endpoints: BTreeMap::new(),
            received: BTreeMap::new(),
            operations_received: BTreeMap::new(),
            providers: BTreeMap::new(),
            serving: BTreeMap::new(),
            resolved: BTreeMap::new(),
            clock: 0,
            events: Events::default(),
            stats: Stats::default(),
        }
    }

    /// The compiled stand-in a language's part is built from.
    pub fn template(&self, language: &str, part: PartName) -> Option<&[u8]> {
        let part = match part {
            PartName::Sandboxed => "sandboxed",
            PartName::Outside => "outside",
        };
        self.templates
            .get(&(language.to_owned(), part))
            .map(Vec::as_slice)
    }

    /// The worker processes alive now, by provider.
    pub fn workers(&self) -> BTreeMap<String, ProcessId> {
        self.providers
            .iter()
            .filter_map(|(name, slot)| slot.worker.as_ref().map(|w| (name.clone(), w.pid())))
            .collect()
    }

    fn now(&mut self) -> Time {
        self.clock += 1;
        let (h, m, s) = (self.clock / 3600, (self.clock / 60) % 60, self.clock % 60);
        Time::new(format!("2026-11-07T{:02}:{m:02}:{s:02}Z", h % 24)).expect("a time")
    }

    /// Runs the admitting compiler, in its own isolated process, on a release's two parts.
    pub fn compile(&mut self, binaries: &Parts<Vec<u8>>) -> Result<(Digest, Digest), String> {
        let started = Instant::now();
        let mut child =
            isolation::spawn(Role::Admit, &self.setup.admit).map_err(|e| e.to_string())?;
        let request = AdmitRequest {
            sandboxed: hex(&binaries.sandboxed),
            outside: hex(&binaries.outside),
        };
        let mut stdin = child.stdin.take().ok_or("no stdin")?;
        send(&mut stdin, &request).map_err(|e| e.to_string())?;
        drop(stdin);
        let mut stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        let response: Option<AdmitResponse> = receive(&mut stdout).map_err(|e| e.to_string())?;
        let _ = child.wait();
        self.stats
            .admission_micros
            .push(started.elapsed().as_micros());
        match response {
            Some(AdmitResponse::Compiled {
                sandboxed,
                outside,
                micros,
                ..
            }) => {
                self.stats.compile_micros.push(micros);
                Ok((
                    digest_of_hex(&sandboxed).ok_or("a malformed digest")?,
                    digest_of_hex(&outside).ok_or("a malformed digest")?,
                ))
            }
            Some(AdmitResponse::Refused { reason }) => {
                self.stats.refused.push(reason.clone());
                Err(reason)
            }
            None => Err("the admitting compiler gave no answer".to_owned()),
        }
    }

    fn artefact(&self, provider: &str, version: &str) -> Option<&AdmissionArtefact> {
        self.providers
            .get(provider)?
            .accepted
            .iter()
            .rev()
            .find(|a| a.declaration().version.as_str() == version)
    }

    /// The declaration and version now running for a provider.
    fn running(&self, provider: &str) -> Option<(Declaration, String)> {
        let slot = self.providers.get(provider)?;
        let version = slot.active.clone()?;
        slot.worker.as_ref()?;
        let declaration = self.artefact(provider, &version)?.declaration().clone();
        Some((declaration, version))
    }

    /// Starts an accepted release in the provider's worker, starting the worker if there is none. The running
    /// release, if any, stays in force unless the new one starts and its digests are the admitted ones.
    fn start(&mut self, provider: &str, version: &str) {
        let Some(artefact) = self.artefact(provider, version).cloned() else {
            return;
        };
        let operations = artefact
            .declaration()
            .outside
            .operations
            .iter()
            .map(|o| o.name.as_str().to_owned())
            .collect();
        let Some(binaries) = self
            .providers
            .get(provider)
            .and_then(|s| s.binaries.get(version))
            .cloned()
        else {
            return;
        };
        let worker_exe = self.setup.worker.clone();
        let slot = self.providers.entry(provider.to_owned()).or_default();
        if slot.worker.is_none() {
            match Worker::spawn(&worker_exe) {
                Ok(worker) => {
                    slot.worker = Some(worker);
                    self.stats.workers_started += 1;
                }
                Err(e) => {
                    self.stats
                        .not_started
                        .push(format!("{provider} {version}: no worker: {e}"));
                    return;
                }
            }
        }
        let started = Instant::now();
        let worker = slot.worker.as_mut().expect("a worker");
        let answer = worker
            .send(&ToWorker::Load {
                version: version.to_owned(),
                sandboxed: hex(&binaries.sandboxed),
                outside: hex(&binaries.outside),
                operations,
            })
            .then(|| worker.receive())
            .flatten();
        let micros = started.elapsed().as_micros();
        let mut restore = None;
        match answer {
            Some(FromWorker::Loaded {
                sandboxed,
                outside,
                guest_pids,
            }) => {
                let build = &artefact.build.machine_code;
                if build.sandboxed.value.as_str() == sandboxed
                    && build.outside.value.as_str() == outside
                {
                    slot.active = Some(version.to_owned());
                    self.stats.load_micros.push(micros);
                    self.stats.guest_pids += guest_pids.len();
                } else {
                    // The worker now runs code other than what was admitted: stop it, and bring the running
                    // release back up in a new one.
                    self.stats.digest_mismatches += 1;
                    self.stats.not_started.push(format!(
                        "{provider} {version}: the worker's digests differ from the admitted ones"
                    ));
                    restore = slot.active.take();
                    slot.worker = None;
                }
            }
            Some(FromWorker::NotStarted { reason }) => self
                .stats
                .not_started
                .push(format!("{provider} {version}: {reason}")),
            _ => {
                slot.worker = None;
                slot.active = None;
                self.stats
                    .not_started
                    .push(format!("{provider} {version}: the worker died"));
            }
        }
        let alive = self
            .providers
            .values()
            .filter(|s| s.worker.is_some())
            .count();
        self.stats.most_workers = self.stats.most_workers.max(alive);
        if let Some(previous) = restore {
            self.start(provider, &previous);
        }
    }

    fn proxy(
        &mut self,
        provider: &str,
        declaration: &Declaration,
        lookup: &SecretLookup,
    ) -> (LookupOutcome, Option<String>) {
        if validate_lookup(declaration, lookup).is_err()
            || self.serving.get(provider).map(String::as_str) != Some(lookup.transaction.as_str())
        {
            return (
                LookupOutcome::Refused(SecretRefusalReason::UndeclaredSecret),
                None,
            );
        }
        match self
            .secrets
            .get(&(provider.to_owned(), lookup.name.as_str().to_owned()))
            .cloned()
        {
            Some(value) => {
                self.delivered
                    .entry(provider.to_owned())
                    .or_default()
                    .push(value.clone());
                (LookupOutcome::Resolved, Some(value))
            }
            None => (LookupOutcome::Refused(SecretRefusalReason::NotFound), None),
        }
    }

    /// The egress layer: checks each hop by name and by connected address against the running declaration's
    /// destinations for the operation, delivers to the simulated endpoint only what passes, and records the call.
    #[allow(clippy::too_many_arguments)]
    fn egress(
        &mut self,
        provider: &str,
        declaration: &Declaration,
        version: &str,
        transaction: &str,
        operation: &str,
        endpoint: &Endpoint,
        body: &str,
    ) -> CallOutcome {
        let listed: Vec<Name> = declaration
            .outside
            .operations
            .iter()
            .filter(|o| o.name.as_str() == operation)
            .flat_map(|o| o.destinations.clone())
            .collect();
        let find = |current: &Endpoint| -> Option<Destination> {
            declaration
                .outside
                .destinations
                .iter()
                .find(|d| {
                    listed.contains(&d.name)
                        && d.scheme == current.scheme
                        && d.host.as_str() == current.host
                        && d.port.get() == current.port
                })
                .cloned()
        };
        let mut hops = Vec::new();
        let mut current = endpoint.clone();
        let mut outcome = CallOutcome::Completed(Nothing {});
        for hop in 0..5 {
            let matched = find(&current);
            let mut refusal = match (hop, &matched) {
                (0, None) => Some(RefusalReason::UndeclaredDestination),
                (_, None) => Some(RefusalReason::RedirectToUndeclaredDestination),
                _ => None,
            };
            let served = self.endpoints.get(&current).cloned();
            if let (None, Some(destination), Some((address, _))) = (&refusal, &matched, &served)
                && !allows(&destination.addresses, *address)
            {
                refusal = Some(RefusalReason::AddressNotAllowed);
            }
            let (Some(host), Some(port)) = (
                Host::new(current.host.clone()),
                NonZeroU16::new(current.port),
            ) else {
                outcome = CallOutcome::Refused(Refused {
                    reason: RefusalReason::UndeclaredDestination,
                });
                break;
            };
            let destination = matched.as_ref().map(|d| d.name.clone());
            if let Some(reason) = refusal {
                hops.push(Hop {
                    scheme: current.scheme,
                    host,
                    port,
                    destination,
                    address: None,
                });
                outcome = CallOutcome::Refused(Refused { reason });
                break;
            }
            let Some((address, redirect)) = served else {
                hops.push(Hop {
                    scheme: current.scheme,
                    host,
                    port,
                    destination,
                    address: None,
                });
                outcome = CallOutcome::Failed(Nothing {});
                break;
            };
            hops.push(Hop {
                scheme: current.scheme,
                host,
                port,
                destination,
                address: Address::new(address.to_string()),
            });
            self.received
                .entry(current.clone())
                .or_default()
                .push(body.to_owned());
            match redirect {
                Some(next) => current = next,
                None => break,
            }
        }
        let secrets = self
            .resolved
            .get(&(
                provider.to_owned(),
                transaction.to_owned(),
                operation.to_owned(),
            ))
            .cloned()
            .unwrap_or_default();
        let started_at = self.now();
        if let (Some(t), Some(p), Some(v), Some(o)) = (
            Identifier::new(transaction),
            Identifier::new(provider),
            Identifier::new(version),
            Name::new(operation),
        ) {
            self.events.records.push(OutboundCallRecord {
                transaction: t,
                provider: p,
                module_version: v,
                operation: o,
                secrets,
                started_at,
                hops,
                outcome: outcome.clone(),
            });
        }
        outcome
    }

    /// A cross-module call: made only where the caller's running declaration names the callee and the kind, and
    /// the callee runs; recorded either way.
    fn cross_module(
        &mut self,
        provider: &str,
        declaration: &Declaration,
        version: &str,
        operation: &Statement<Operation>,
        callee: &str,
        kind: OperationKind,
    ) -> bool {
        let refusal = match declaration
            .outside
            .calls
            .iter()
            .find(|c| c.callee.as_str() == callee)
        {
            None => Some(CallRefusalReason::UndeclaredCallee),
            Some(c) if !c.kinds.contains(&kind) => Some(CallRefusalReason::UndeclaredKind),
            Some(_) if self.running(callee).is_none() => Some(CallRefusalReason::CalleeUnavailable),
            Some(_) => None,
        };
        let Ok(mut asked) = serde_json::to_value(operation) else {
            return false;
        };
        asked["statement"]["provider"] = Value::from(callee);
        let Ok(asked) = serde_json::from_value::<Statement<Operation>>(asked) else {
            return false;
        };
        let transaction = serde_json::to_value(&operation.statement)
            .ok()
            .and_then(|v| v["transaction"].as_str().map(str::to_owned))
            .unwrap_or_default();
        let outcome = match refusal {
            Some(reason) => CrossModuleOutcome::Refused(CallRefused {
                callee: Identifier::new(callee).expect("a callee"),
                kind,
                reason,
            }),
            None => {
                self.handle(callee, &asked);
                CrossModuleOutcome::Delivered(Nothing {})
            }
        };
        let delivered = matches!(outcome, CrossModuleOutcome::Delivered(_));
        if let (Some(t), Some(c), Some(v)) = (
            Identifier::new(&transaction),
            Identifier::new(provider),
            Identifier::new(version),
        ) {
            self.events.calls.push(CrossModuleCall {
                transaction: t,
                caller: c,
                caller_version: v,
                route: RouteReference {
                    decision_record: Identifier::new("conformance-route").expect("an id"),
                    segment: Identifier::new("segment-1").expect("an id"),
                },
                operation: asked.statement,
                outcome,
            });
        }
        delivered
    }

    fn host_call(
        &mut self,
        provider: &str,
        declaration: &Declaration,
        version: &str,
        operation: &Statement<Operation>,
        call: HostCall,
    ) -> Reply {
        let transaction = self.serving.get(provider).cloned().unwrap_or_default();
        match call {
            HostCall::Secret {
                transaction: asked,
                operation: op,
                name,
            } => {
                let (Some(t), Some(o), Some(n)) =
                    (Identifier::new(&asked), Name::new(&op), Name::new(&name))
                else {
                    return Reply::Nothing;
                };
                let lookup = SecretLookup {
                    transaction: t,
                    operation: o,
                    name: n.clone(),
                };
                let (outcome, value) = self.proxy(provider, declaration, &lookup);
                self.events.lookups.push(LookupEvent {
                    provider: provider.to_owned(),
                    part: PartName::Outside,
                    lookup,
                    outcome: Some(outcome),
                });
                match value {
                    Some(value) => {
                        self.resolved
                            .entry((provider.to_owned(), transaction, op))
                            .or_default()
                            .push(n);
                        Reply::Value(value)
                    }
                    None => Reply::Nothing,
                }
            }
            HostCall::State { key } => {
                let value = self.state.get(&(provider.to_owned(), key.clone())).cloned();
                self.events.state_reads.push(StateEvent {
                    provider: provider.to_owned(),
                    part: PartName::Outside,
                    key,
                    found: value.is_some(),
                });
                value.map_or(Reply::Nothing, Reply::Value)
            }
            HostCall::Egress {
                operation: op,
                url,
                body,
                ..
            } => {
                let Some(endpoint) = parse_url(&url) else {
                    return Reply::Status(2);
                };
                match self.egress(
                    provider,
                    declaration,
                    version,
                    &transaction,
                    &op,
                    &endpoint,
                    &body,
                ) {
                    CallOutcome::Completed(_) => Reply::Status(0),
                    CallOutcome::Refused(_) => Reply::Status(2),
                    _ => Reply::Status(1),
                }
            }
            HostCall::Call { callee, kind } => {
                let Ok(kind) = serde_json::from_value::<OperationKind>(Value::from(kind)) else {
                    return Reply::Status(1);
                };
                let delivered =
                    self.cross_module(provider, declaration, version, operation, &callee, kind);
                Reply::Status(u64::from(!delivered))
            }
        }
    }

    /// Hands one operation to the provider's worker and serves its host calls until it answers. The provider's
    /// output bytes, or `None` if its guest failed.
    fn run(
        &mut self,
        provider: &str,
        declaration: &Declaration,
        version: &str,
        operation: &Statement<Operation>,
        transaction: &str,
    ) -> Option<Vec<u8>> {
        let mut worker = self.providers.get_mut(provider)?.worker.take()?;
        self.serving
            .insert(provider.to_owned(), transaction.to_owned());
        let text = serde_json::to_string(operation).ok()?;
        let mut output = None;
        let mut alive = worker.send(&ToWorker::Handle {
            operation: text,
            transaction: transaction.to_owned(),
        });
        while alive {
            match worker.receive() {
                Some(FromWorker::Host(call)) => {
                    let reply = self.host_call(provider, declaration, version, operation, call);
                    alive = worker.send(&ToWorker::Reply(reply));
                }
                Some(FromWorker::SandboxedSecretRefused {
                    operation: op,
                    name,
                }) => {
                    if let (Some(t), Some(o), Some(n)) = (
                        Identifier::new(transaction),
                        Name::new(&op),
                        Name::new(&name),
                    ) {
                        self.events.lookups.push(LookupEvent {
                            provider: provider.to_owned(),
                            part: PartName::Sandboxed,
                            lookup: SecretLookup {
                                transaction: t,
                                operation: o,
                                name: n,
                            },
                            outcome: None,
                        });
                    }
                }
                Some(FromWorker::Handled { output: bytes }) => {
                    output = unhex(&bytes);
                    break;
                }
                Some(FromWorker::Failed { .. }) => break,
                _ => alive = false,
            }
        }
        self.serving.remove(provider);
        self.resolved
            .retain(|(p, t, _), _| !(p == provider && t == transaction));
        if let Some(slot) = self.providers.get_mut(provider) {
            if alive {
                slot.worker = Some(worker);
            } else {
                slot.active = None;
            }
        }
        output
    }
}

impl ModuleBinding for PvmBinding {
    fn format(&self) -> String {
        FORMAT.to_owned()
    }

    fn languages(&self) -> Vec<String> {
        self.languages.clone()
    }

    fn set_output_limit(&mut self, bytes: usize) {
        self.output_limit = bytes;
    }

    fn admit(
        &mut self,
        assertion: &Statement<ReleaseAssertion>,
        binaries: &Parts<Vec<u8>>,
    ) -> Admitted {
        let received = Parts {
            sandboxed: digest(&binaries.sandboxed),
            outside: digest(&binaries.outside),
        };
        let asserted = &assertion.statement.parts;
        if received.sandboxed != asserted.sandboxed.digest {
            return Admitted::Refused(AdmissionRefusal::PartAltered(PartName::Sandboxed));
        }
        if received.outside != asserted.outside.digest {
            return Admitted::Refused(AdmissionRefusal::PartAltered(PartName::Outside));
        }
        if asserted.sandboxed.format.as_str() != FORMAT
            || asserted.outside.format.as_str() != FORMAT
        {
            return Admitted::Refused(AdmissionRefusal::UnreadableBinary);
        }
        let Ok((sandboxed_code, outside_code)) = self.compile(binaries) else {
            return Admitted::Refused(AdmissionRefusal::UnreadableBinary);
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
        let assertion_bytes = serde_json::to_vec(assertion).expect("json");
        let accepted = (!fresh).then(|| Accepted {
            by: None,
            at: self.now(),
        });
        let artefact = AdmissionArtefact {
            assertion: assertion.clone(),
            assertion_digest: digest(&assertion_bytes),
            received,
            build: Build {
                platform_version: Identifier::new(engine::platform_version()).expect("a version"),
                machine_code: Parts {
                    sandboxed: sandboxed_code,
                    outside: outside_code,
                },
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
                accepted,
            },
        };
        let slot = self.providers.entry(provider.clone()).or_default();
        slot.binaries.insert(version.clone(), binaries.clone());
        if fresh {
            slot.pending = Some(artefact.clone());
        } else {
            slot.accepted.push(artefact.clone());
            self.start(&provider, &version);
        }
        Admitted::Artefact(Box::new(artefact))
    }

    fn accept(&mut self, provider: &str, version: &str, by: &str) -> Option<AdmissionArtefact> {
        let at = self.now();
        let slot = self.providers.get_mut(provider)?;
        let mut artefact = slot.pending.take()?;
        if artefact.declaration().version.as_str() != version {
            slot.pending = Some(artefact);
            return None;
        }
        artefact.acceptance.accepted = Some(Accepted {
            by: Identifier::new(by),
            at,
        });
        slot.accepted.push(artefact.clone());
        self.start(provider, version);
        Some(artefact)
    }

    fn active_version(&self, provider: &str) -> Option<String> {
        self.running(provider).map(|(_, version)| version)
    }

    fn process(&self, provider: &str) -> Option<ProcessId> {
        let slot = self.providers.get(provider)?;
        slot.active.as_ref()?;
        slot.worker.as_ref().map(Worker::pid)
    }

    fn restart(&mut self, provider: &str) -> bool {
        let Some(slot) = self.providers.get_mut(provider) else {
            return false;
        };
        let (Some(version), Some(_)) = (slot.active.clone(), slot.worker.as_ref()) else {
            return false;
        };
        // Only this provider's worker is stopped; every other worker keeps running.
        slot.worker = None;
        slot.active = None;
        self.start(provider, &version);
        true
    }

    fn handle(&mut self, provider: &str, operation: &Statement<Operation>) -> Handled {
        *self
            .operations_received
            .entry(provider.to_owned())
            .or_default() += 1;
        let Some((declaration, version)) = self.running(provider) else {
            return Handled::Refused(PlatformRefusal::Unavailable);
        };
        if !declaration
            .sandboxed
            .handles
            .contains(&OperationKind::of(&operation.statement))
        {
            return Handled::Refused(PlatformRefusal::NotHandled);
        }
        let transaction = serde_json::to_value(&operation.statement)
            .ok()
            .and_then(|v| v["transaction"].as_str().map(str::to_owned))
            .unwrap_or_default();
        let Some(bytes) = self.run(provider, &declaration, &version, operation, &transaction)
        else {
            return Handled::Refused(PlatformRefusal::Unavailable);
        };
        if bytes.len() > self.output_limit {
            return Handled::Refused(PlatformRefusal::OversizedOutput);
        }
        let Ok(result) = serde_json::from_slice::<ResultBody>(&bytes) else {
            return Handled::Refused(PlatformRefusal::MalformedOutput);
        };
        if !result_matches(&result, &operation.statement) {
            return Handled::Refused(PlatformRefusal::MalformedOutput);
        }
        Handled::Answered(Box::new(Statement {
            deployment: operation.deployment.clone(),
            protocol_version: operation.protocol_version.clone(),
            shape: RESULT_SHAPE.to_owned(),
            maker: provider.to_owned(),
            made_at: operation.made_at.clone(),
            signature: Signature {
                scheme: SignatureScheme::Ed25519,
                key: declaration.outside.statement_key.as_str().to_owned(),
                value: placeholder_signature(),
            },
            statement: result,
        }))
    }

    fn take_events(&mut self) -> Events {
        std::mem::take(&mut self.events)
    }
}

impl BoundaryHarness for PvmBinding {
    fn build(&mut self, spec: &ModuleSpec, language: &str) -> Option<Parts<Vec<u8>>> {
        let text = spec::encode(spec, &self.category);
        let part = |part: PartName| -> Option<Vec<u8>> {
            let template = self.template(language, part)?;
            match spec::patch(template, &text) {
                Ok(blob) => Some(blob),
                Err(e) => panic!("the {language} {part:?} stand-in can't take this spec: {e:?}"),
            }
        };
        Some(Parts {
            sandboxed: part(PartName::Sandboxed)?,
            outside: part(PartName::Outside)?,
        })
    }

    fn delivered_secrets(&self, provider: &str) -> Vec<String> {
        self.delivered.get(provider).cloned().unwrap_or_default()
    }

    fn seed_secret(&mut self, provider: &str, name: &str, value: &str) {
        self.secrets
            .insert((provider.to_owned(), name.to_owned()), value.to_owned());
    }

    fn seed_state(&mut self, provider: &str, key: &str, value: &str) {
        self.state
            .insert((provider.to_owned(), key.to_owned()), value.to_owned());
    }

    fn serve(&mut self, endpoint: &Endpoint, address: IpAddr, redirect: Option<&Endpoint>) {
        self.endpoints
            .insert(endpoint.clone(), (address, redirect.cloned()));
    }

    fn received(&self, endpoint: &Endpoint) -> Vec<String> {
        self.received.get(endpoint).cloned().unwrap_or_default()
    }

    fn operations_received(&self, provider: &str) -> usize {
        self.operations_received
            .get(provider)
            .copied()
            .unwrap_or_default()
    }
}
