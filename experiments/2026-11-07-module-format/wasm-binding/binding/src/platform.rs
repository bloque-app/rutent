//! The platform: the format binding the module-boundary suite drives (`ModuleBinding`, `BoundaryHarness`). It runs
//! in its own process and holds everything a module must not reach on its own: the credential proxy, each
//! provider's state, the egress layer and the endpoints behind it, the cross-module router, admission and every
//! artefact. Each provider's module runs in a process of its own (`children::ProviderProcess`), and each admission
//! compiles in a fresh compiler process (`children::compile`).
//!
//! Every check is made here, against the provider's **running** declaration, and every request is attributed to
//! the provider whose process's pipe it came on, never to anything the request says (`DEC-9`). A module escaping
//! Wasm reaches its own process, which holds no check and nothing of another provider's.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use rutent_conformance::boundary::{
    AdmissionRefusal, Admitted, BoundaryHarness, Endpoint, Events, Handled, LookupEvent,
    LookupOutcome, ModuleBinding, ModuleSpec, PlatformRefusal, ProcessId, StateEvent,
};
use rutent_contracts::module_interface::{
    Acceptance, AcceptanceMode, Accepted, Address, Addresses, AdmissionArtefact, Build,
    CallOutcome, CallRefusalReason, CallRefused, Capabilities, CrossModuleCall, CrossModuleOutcome,
    Declaration, Destination, Digest, DigestAlgorithm, DigestValue, Hop, Host, Identifier, Name,
    Nothing, OperationKind, OutboundCallRecord, OutsideOutcome, OutsideRequest, OutsideResponse,
    PartName, Parts, RefusalReason, Refused, ReleaseAssertion, RouteReference, SPECIAL_PURPOSE,
    Scheme, SecretLookup, SecretRefusalReason, Time, validate_lookup, widening,
};
use rutent_contracts::provider_role::{
    Operation, RESULT_SHAPE, ResultBody, Signature, SignatureScheme, Statement,
};
use serde_json::Value;

use crate::children::{Machine, Paths, ProviderProcess, compile};
use crate::engine::{self, FORMAT, MAX_BINARY};
use crate::program::{self, Probe};
use crate::wire::{FromProvider, Reply, ToProvider, sha256, unhex};

/// The most hops an outbound call follows, redirects included.
const MAX_HOPS: usize = 5;

/// One provider's releases and its running process.
#[derive(Default)]
struct Slot {
    accepted: Vec<AdmissionArtefact>,
    pending: Option<(AdmissionArtefact, Rc<Machine>)>,
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
}

/// What the binding measured, for `B-06`.
#[derive(Debug, Clone, Default)]
pub struct Measurements {
    /// Each admission's compilation, start of the compiler's process to its exit, with both parts' portable and
    /// machine-code sizes in bytes.
    pub compiles: Vec<(Duration, [usize; 2], [usize; 2])>,
    /// Each provider process's start, spawn to both parts started.
    pub starts: Vec<Duration>,
    /// Why the last operation a sandboxed part didn't finish stopped, as its process reported it.
    pub last_trap: Option<String>,
}

/// The Wasm-component binding.
pub struct WasmBinding {
    paths: Paths,
    workdir: PathBuf,
    languages: Vec<String>,
    category: String,
    output_limit: usize,
    secrets: BTreeMap<(String, String), String>,
    delivered: BTreeMap<String, Vec<String>>,
    state: BTreeMap<(String, String), String>,
    endpoints: BTreeMap<Endpoint, (IpAddr, Option<Endpoint>)>,
    received: BTreeMap<Endpoint, Vec<String>>,
    operations_received: BTreeMap<String, usize>,
    providers: BTreeMap<String, Slot>,
    stack: Vec<Context>,
    clock: u32,
    events: Events,
    /// What it measured.
    pub measurements: Measurements,
}

impl WasmBinding {
    /// A binding whose stand-ins are `category`'s, in `languages` (`rs`, `c`).
    pub fn new(paths: Paths, languages: &[&str], category: &str) -> Result<Self, String> {
        let workdir = std::env::temp_dir().join(format!(
            "rutent-wasm-binding-{}-{}",
            std::process::id(),
            unique()
        ));
        std::fs::create_dir_all(&workdir).map_err(|e| format!("{}: {e}", workdir.display()))?;
        Ok(Self {
            paths,
            workdir,
            languages: languages.iter().map(|l| (*l).to_owned()).collect(),
            category: category.to_owned(),
            output_limit: 0,
            secrets: BTreeMap::new(),
            delivered: BTreeMap::new(),
            state: BTreeMap::new(),
            endpoints: BTreeMap::new(),
            received: BTreeMap::new(),
            operations_received: BTreeMap::new(),
            providers: BTreeMap::new(),
            stack: Vec::new(),
            clock: 0,
            events: Events::default(),
            measurements: Measurements::default(),
        })
    }

    /// The directory the stand-ins are read from.
    pub fn standins(&self) -> &Path {
        &self.paths.standins
    }

    /// The template of one stand-in part: `{language}-{category}-{part}.wasm`.
    pub fn template(&self, language: &str, category: &str, part: &str) -> Option<Vec<u8>> {
        std::fs::read(
            self.paths
                .standins
                .join(format!("{language}-{category}-{part}.wasm")),
        )
        .ok()
    }

    /// Builds the module `spec` describes from `category`'s stand-in in `language`, with the binding's own
    /// `probes` after its steps.
    pub fn build_with(
        &self,
        spec: &ModuleSpec,
        language: &str,
        category: &str,
        probes: &[Probe],
    ) -> Option<Parts<Vec<u8>>> {
        if !self.languages.iter().any(|l| l == language) {
            return None;
        }
        let text = program::encode(spec, probes);
        Some(Parts {
            sandboxed: program::patch(&self.template(language, category, "sandboxed")?, &text)?,
            outside: program::patch(&self.template(language, category, "outside")?, &text)?,
        })
    }

    /// The machine code admitted for a provider's version.
    pub fn machine(&self, provider: &str, version: &str) -> Option<Rc<Machine>> {
        let slot = self.providers.get(provider)?;
        slot.machines.get(version).cloned().or_else(|| {
            slot.pending
                .as_ref()
                .filter(|(a, _)| a.declaration().version.as_str() == version)
                .map(|(_, m)| m.clone())
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
        ) {
            self.measurements.starts.push(process.took);
            let slot = self.providers.entry(provider.to_owned()).or_default();
            slot.active = Some(version.to_owned());
            slot.process = Some(process);
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

    /// Answers one request a provider's process made while handling the operation on top of the stack.
    fn serve(&mut self, request: FromProvider) -> Reply {
        match request {
            FromProvider::OutsideRequest { json } => self.outside_request(&json),
            FromProvider::OutsideResponse { json } => self.outside_response(&json),
            FromProvider::DeniedSecret { name } => {
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
            } => self.lookup(transaction, operation, name),
            FromProvider::State { key } => {
                // Only while an outside request is in flight: outside one, the outside part has nothing to serve.
                let context = self.stack.last().expect("a context");
                let (provider, serving) = (context.provider.clone(), context.serving.is_some());
                let value = serving
                    .then(|| self.state.get(&(provider.clone(), key.clone())).cloned())
                    .flatten();
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
                body,
            } => self.egress(&operation, &scheme, host, port, &body),
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
            Some(_) => {
                context.serving = request.clone();
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
                reason: "undeclared_secret".to_owned(),
            };
        };
        let as_claimed =
            transaction == serving.transaction.as_str() && operation == serving.operation.as_str();
        let lookup = SecretLookup {
            transaction: serving.transaction,
            operation: serving.operation,
            name,
        };
        let outcome = if !as_claimed || validate_lookup(&context.declaration, &lookup).is_err() {
            LookupOutcome::Refused(SecretRefusalReason::UndeclaredSecret)
        } else {
            match self
                .secrets
                .get(&(provider.clone(), lookup.name.as_str().to_owned()))
            {
                Some(_) => {
                    context
                        .resolved
                        .entry(lookup.operation.as_str().to_owned())
                        .or_default()
                        .push(lookup.name.clone());
                    LookupOutcome::Resolved
                }
                None => LookupOutcome::Refused(SecretRefusalReason::NotFound),
            }
        };
        let value = self
            .secrets
            .get(&(provider.clone(), lookup.name.as_str().to_owned()))
            .cloned();
        self.events.lookups.push(LookupEvent {
            provider: provider.clone(),
            part: PartName::Outside,
            lookup,
            outcome: Some(outcome),
        });
        match (outcome, value) {
            (LookupOutcome::Resolved, Some(value)) => {
                self.delivered
                    .entry(provider)
                    .or_default()
                    .push(value.clone());
                Reply::Secret { value }
            }
            (LookupOutcome::Refused(SecretRefusalReason::NotFound), _) => Reply::SecretRefused {
                reason: "not_found".to_owned(),
            },
            _ => Reply::SecretRefused {
                reason: "undeclared_secret".to_owned(),
            },
        }
    }

    /// The egress layer: every hop's scheme, host and port must be a destination the running declaration's
    /// operation lists, and the address connected to within that destination's addresses (ADR-0004). The operation
    /// is the outside request in flight, from the platform's own state: a call naming another is refused as
    /// `undeclared_operation`, and its record names the operation in flight. It writes the outbound call record,
    /// refused calls included (`REQ-MDL-8`).
    fn egress(
        &mut self,
        operation: &str,
        scheme: &str,
        host: String,
        port: u16,
        body: &str,
    ) -> Reply {
        let context = self.stack.last().expect("a context");
        let (provider, version, transaction) = (
            context.provider.clone(),
            context.version.clone(),
            context.transaction.clone(),
        );
        let declaration = context.declaration.clone();
        let Some(op_name) = context.serving.as_ref().map(|s| s.operation.clone()) else {
            // Nothing is in flight, so there is no operation to make the call under or to record it for.
            return Reply::Egress {
                outcome: "refused".to_owned(),
            };
        };
        let as_claimed = operation == op_name.as_str();
        let operation = op_name.as_str();
        let secrets = context.resolved.get(operation).cloned().unwrap_or_default();
        let scheme = match scheme {
            "wss" => Scheme::Wss,
            _ => Scheme::Https,
        };
        let listed: Vec<Name> = declaration
            .outside
            .operations
            .iter()
            .filter(|o| as_claimed && o.name.as_str() == operation)
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
        if Host::new(host.clone()).is_none() {
            return Reply::Egress {
                outcome: "refused".to_owned(),
            };
        }
        let mut hops = Vec::new();
        let mut current = Endpoint { scheme, host, port };
        let mut outcome = CallOutcome::Completed(Nothing {});
        for hop in 0..MAX_HOPS {
            let matched = find(&current);
            let served = self.endpoints.get(&current).cloned();
            let refusal = match (&matched, &served) {
                _ if !as_claimed => Some(RefusalReason::UndeclaredOperation),
                (None, _) if hop == 0 => Some(RefusalReason::UndeclaredDestination),
                (None, _) => Some(RefusalReason::RedirectToUndeclaredDestination),
                (Some(d), Some((address, _))) if !allows(&d.addresses, *address) => {
                    Some(RefusalReason::AddressNotAllowed)
                }
                _ => None,
            };
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
                Some(next) if hop + 1 < MAX_HOPS => current = next,
                Some(_) => {
                    outcome = CallOutcome::Failed(Nothing {});
                    break;
                }
                None => break,
            }
        }
        let reply = match &outcome {
            CallOutcome::Completed(_) => "completed",
            CallOutcome::Failed(_) => "failed",
            CallOutcome::Refused(_) => "refused",
            _ => "unknown",
        };
        let started_at = self.now();
        if let (Some(transaction), Some(provider), Some(module_version)) = (
            Identifier::new(transaction),
            Identifier::new(provider),
            Identifier::new(version),
        ) {
            self.events.records.push(OutboundCallRecord {
                transaction,
                provider,
                module_version,
                operation: op_name.clone(),
                secrets,
                started_at,
                hops,
                outcome,
            });
        }
        Reply::Egress {
            outcome: reply.to_owned(),
        }
    }

    /// The cross-module router: a call is made only if the caller's running declaration declares the callee and
    /// the kind, and the callee's module runs; it is recorded either way (`REQ-MDL-21`). The route is the suite's
    /// placeholder, `conformance-route` and `segment-1`: the suite has no routes, so this binding checks none.
    fn cross_module(&mut self, callee: &str, kind: &str) -> Reply {
        let context = self.stack.last().expect("a context");
        let (caller, version, transaction) = (
            context.provider.clone(),
            context.version.clone(),
            context.transaction.clone(),
        );
        let envelope = context.operation.clone();
        let declaration = context.declaration.clone();
        let in_flight = context.serving.is_some();
        let Ok(kind) = serde_json::from_value::<OperationKind>(Value::from(kind)) else {
            return Reply::Module {
                outcome: "refused".to_owned(),
            };
        };
        let mut refusal = match declaration
            .outside
            .calls
            .iter()
            .find(|c| c.callee.as_str() == callee)
        {
            None => Some(CallRefusalReason::UndeclaredCallee),
            Some(c) if !c.kinds.contains(&kind) => Some(CallRefusalReason::UndeclaredKind),
            // Outside an outside request, no route segment can name the call: the contract's third check.
            Some(_) if !in_flight => Some(CallRefusalReason::NotNamedByRoute),
            Some(_) => None,
        };
        let Some(asked) = with_provider(&envelope, callee) else {
            return Reply::Module {
                outcome: "refused".to_owned(),
            };
        };
        if refusal.is_none() {
            let running = self
                .providers
                .get(callee)
                .is_some_and(|s| s.active.is_some() && s.process.is_some());
            if !running || callee == caller {
                refusal = Some(CallRefusalReason::CalleeUnavailable);
            }
        }
        if refusal.is_none() {
            // The callee answers on its own credentials, in its own process.
            let _ = self.handle(callee, &asked);
        }
        let (Some(transaction), Some(caller), Some(caller_version), Some(callee_id)) = (
            Identifier::new(transaction),
            Identifier::new(caller),
            Identifier::new(version),
            Identifier::new(callee),
        ) else {
            return Reply::Module {
                outcome: "refused".to_owned(),
            };
        };
        let outcome = match refusal {
            Some(reason) => CrossModuleOutcome::Refused(CallRefused {
                callee: callee_id,
                kind,
                reason,
            }),
            None => CrossModuleOutcome::Delivered(Nothing {}),
        };
        let delivered = matches!(outcome, CrossModuleOutcome::Delivered(_));
        self.events.calls.push(CrossModuleCall {
            transaction,
            caller,
            caller_version,
            route: RouteReference {
                decision_record: Identifier::new("conformance-route").expect("an id"),
                segment: Identifier::new("segment-1").expect("an id"),
            },
            operation: asked.statement,
            outcome,
        });
        Reply::Module {
            outcome: if delivered { "delivered" } else { "refused" }.to_owned(),
        }
    }

    /// Hands the operation to the provider's process and serves its requests until it answers.
    fn run_in_process(
        &mut self,
        provider: &str,
        operation: &Statement<Operation>,
        declaration: Declaration,
        version: String,
    ) -> Result<Vec<u8>, PlatformRefusal> {
        let mut process = self
            .providers
            .get_mut(provider)
            .and_then(|s| s.process.take())
            .ok_or(PlatformRefusal::Unavailable)?;
        let transaction = serde_json::to_value(&operation.statement)
            .ok()
            .and_then(|v| v["transaction"].as_str().map(str::to_owned))
            .unwrap_or_default();
        self.stack.push(Context {
            provider: provider.to_owned(),
            version,
            declaration,
            operation: operation.clone(),
            transaction,
            serving: None,
            resolved: BTreeMap::new(),
        });
        let json = serde_json::to_string(operation).unwrap_or_default();
        let mut outcome = Err(PlatformRefusal::Unavailable);
        let mut alive = process
            .send(&ToProvider::Handle { operation: json })
            .is_ok();
        while alive {
            match process.receive() {
                None => alive = false,
                Some(FromProvider::Done { output }) => {
                    outcome = unhex(&output).ok_or(PlatformRefusal::MalformedOutput);
                    break;
                }
                Some(FromProvider::Trapped { message }) => {
                    self.measurements.last_trap = Some(message);
                    break;
                }
                Some(request) => {
                    let reply = self.serve(request);
                    alive = process.reply(&reply).is_ok();
                }
            }
        }
        let context = self.stack.pop();
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
        outcome
    }
}

/// `envelope` asked of `callee`: the same operation, its `provider` the callee.
fn with_provider(envelope: &Statement<Operation>, callee: &str) -> Option<Statement<Operation>> {
    let mut value = serde_json::to_value(envelope).ok()?;
    value["statement"]["provider"] = Value::from(callee);
    serde_json::from_value(value).ok()
}

/// Whether `address` is within `addresses`: for `public`, globally routable, in no special-purpose range.
fn allows(addresses: &Addresses, address: IpAddr) -> bool {
    match addresses {
        Addresses::Public(_) => !SPECIAL_PURPOSE.iter().any(|(range, len)| {
            range
                .parse::<IpAddr>()
                .is_ok_and(|range| within(address, range, *len))
        }),
        Addresses::Blocks(blocks) => blocks.iter().any(|block| {
            let (ip, len) = block.parts();
            within(address, ip, len)
        }),
    }
}

/// Whether `ip` lies within the block `outer/outer_len`. An IPv4 address and an IPv6 block never match, so an
/// IPv4-mapped IPv6 address is judged as IPv6, where `::ffff:0:0/96` is special-purpose.
fn within(ip: IpAddr, outer: IpAddr, outer_len: u8) -> bool {
    let bits = |ip: IpAddr| match ip {
        IpAddr::V4(v4) => (u128::from(u32::from(v4)) << 96, true),
        IpAddr::V6(v6) => (u128::from(v6), false),
    };
    let ((a, a4), (b, b4)) = (bits(ip), bits(outer));
    if a4 != b4 {
        return false;
    }
    let mask = if outer_len == 0 {
        0
    } else {
        u128::MAX << (128 - u32::from(outer_len))
    };
    a & mask == b & mask
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

impl Drop for WasmBinding {
    fn drop(&mut self) {
        // Every provider's process is killed and reaped as its slot drops.
        self.providers.clear();
        let _ = std::fs::remove_dir_all(&self.workdir);
    }
}

impl ModuleBinding for WasmBinding {
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
            sandboxed: digest(&sha256(&binaries.sandboxed)),
            outside: digest(&sha256(&binaries.outside)),
        };
        let asserted = &assertion.statement.parts;
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
        // stay what they are. The suite's refusals have no reason for it; it is refused as unreadable.
        let (provider, version) = (
            assertion.statement.declaration.provider.as_str(),
            assertion.statement.declaration.version.as_str(),
        );
        if let Some(slot) = self.providers.get(provider) {
            let accepted = slot
                .accepted
                .iter()
                .any(|a| a.declaration().version.as_str() == version);
            let pending = slot
                .pending
                .as_ref()
                .is_some_and(|(a, _)| a.declaration().version.as_str() == version);
            if accepted || pending {
                return Admitted::Refused(AdmissionRefusal::UnreadableBinary);
            }
        }
        let machine = match compile(
            &self.paths,
            &self.workdir,
            &binaries.sandboxed,
            &binaries.outside,
        ) {
            Ok(machine) => Rc::new(machine),
            Err(_) => return Admitted::Refused(AdmissionRefusal::UnreadableBinary),
        };
        self.measurements.compiles.push((
            machine.took,
            [binaries.sandboxed.len(), binaries.outside.len()],
            [machine.sandboxed.len(), machine.outside.len()],
        ));
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
        let assertion_bytes = serde_json::to_vec(assertion).unwrap_or_default();
        let accepted = (!fresh).then(|| Accepted {
            by: None,
            at: self.now(),
        });
        let Some(platform_version) = Identifier::new(engine::platform_version()) else {
            return Admitted::Refused(AdmissionRefusal::UnreadableBinary);
        };
        let artefact = AdmissionArtefact {
            assertion: assertion.clone(),
            assertion_digest: digest(&sha256(&assertion_bytes)),
            received,
            build: Build {
                platform_version,
                machine_code: Parts {
                    sandboxed: digest(&sha256(&machine.sandboxed)),
                    outside: digest(&sha256(&machine.outside)),
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
        if fresh {
            slot.pending = Some((artefact.clone(), machine));
        } else {
            slot.accepted.push(artefact.clone());
            slot.machines.insert(version.clone(), machine);
            self.start(&provider, &version);
        }
        Admitted::Artefact(Box::new(artefact))
    }

    fn accept(&mut self, provider: &str, version: &str, by: &str) -> Option<AdmissionArtefact> {
        let at = self.now();
        let slot = self.providers.get_mut(provider)?;
        let (mut artefact, machine) = slot.pending.take()?;
        if artefact.declaration().version.as_str() != version {
            slot.pending = Some((artefact, machine));
            return None;
        }
        artefact.acceptance.accepted = Some(Accepted {
            by: Identifier::new(by),
            at,
        });
        slot.accepted.push(artefact.clone());
        slot.machines.insert(version.to_owned(), machine);
        self.start(provider, version);
        Some(artefact)
    }

    fn active_version(&self, provider: &str) -> Option<String> {
        self.providers.get(provider)?.active.clone()
    }

    fn process(&self, provider: &str) -> Option<ProcessId> {
        let slot = self.providers.get(provider)?;
        slot.process.as_ref().map(|p| ProcessId::from(p.pid()))
    }

    fn restart(&mut self, provider: &str) -> bool {
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

    fn handle(&mut self, provider: &str, operation: &Statement<Operation>) -> Handled {
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
        let statement_key = declaration.outside.statement_key.as_str().to_owned();
        let bytes = match self.run_in_process(provider, operation, declaration, version) {
            Ok(bytes) => bytes,
            Err(refusal) => return Handled::Refused(refusal),
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
        // The signature is the suite's placeholder: signing is `M2` and `M3` work, here as in the suite.
        Handled::Answered(Box::new(Statement {
            deployment: operation.deployment.clone(),
            protocol_version: operation.protocol_version.clone(),
            shape: RESULT_SHAPE.to_owned(),
            maker: provider.to_owned(),
            made_at: operation.made_at.clone(),
            signature: Signature {
                scheme: SignatureScheme::Ed25519,
                key: statement_key,
                value: rutent_conformance::role::placeholder_signature(),
            },
            statement: result,
        }))
    }

    fn take_events(&mut self) -> Events {
        std::mem::take(&mut self.events)
    }
}

impl BoundaryHarness for WasmBinding {
    fn build(&mut self, spec: &ModuleSpec, language: &str) -> Option<Parts<Vec<u8>>> {
        let category = self.category.clone();
        self.build_with(spec, language, &category, &[])
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
