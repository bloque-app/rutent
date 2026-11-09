//! The module-boundary suite's run: per language, a world of endpoints, secrets and state and another provider
//! running in it, then one module per fixture or control, then the releases and the isolation probes.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use rutent_contracts::module_interface::{
    AcceptanceMode, AdmissionArtefact, BinaryFormat, BinaryPart, CallOutcome, CallRefusalReason,
    CrossModuleOutcome, Declaration, Digest, DigestAlgorithm, DigestValue, OperationKind,
    OutboundCallRecord, PartName, Parts, RELEASE_ASSERTION_SHAPE, RefusalReason, Refused,
    ReleaseAssertion, SecretRefusalReason, validate_artefact, validate_call, validate_call_against,
    validate_record, validate_record_secrets, validate_release, widening,
};
use rutent_contracts::provider_role::{
    OPERATION_SHAPE, Operation, Signature, SignatureScheme, Statement,
};
use serde_json::{Value, json};

use super::sha256::hex_digest;
use super::{
    AdmissionRefusal, Admitted, BoundaryHarness, Check, Endpoint, Events, Exercised, Finding,
    Handled, LookupOutcome, ModuleSpec, Output, PlatformRefusal, Report, RunConfig, Step,
    Uncovered,
};
use crate::role::placeholder_signature;
use crate::signing::{
    advertisement, advertising_key, another_key, public_key, sign_statement, statements_key,
};

const OPERATION: &str = "api.call";
const OWN_SECRET: &str = "alpha.api_key";
const SHARED_SECRET: &str = "shared.token";
const UNLISTED_SECRET: &str = "alpha.unlisted";
const RELEASE_SECRET: &str = "release.token";
const RELEASE_UNLISTED: &str = "release.unlisted";
const OWN_STATE: &str = "own.note";
const SHARED_STATE: &str = "shared.balance";
const REVIEWER: &str = "conformance-reviewer";

/// The labels of the endpoints the base declaration lists, each `{label}.alpha-{language}.example`.
const DECLARED: [&str; 10] = [
    "api",
    "files",
    "moved",
    "internal",
    "chain",
    "relay",
    "bounce",
    "loopback6",
    "local6",
    "mapped",
];

/// Every endpoint label of the world, the declared ones and the ones only releases declare.
const LABELS: [&str; 13] = [
    "api",
    "files",
    "moved",
    "internal",
    "chain",
    "relay",
    "bounce",
    "loopback6",
    "local6",
    "mapped",
    "extra",
    "archive",
    "spread",
];

/// Runs the module-boundary suite against `binding`, in each language it offers, and reports every broken rule
/// and every fixture or control not seen to pass.
pub fn run_boundary_suite(binding: &mut dyn BoundaryHarness, config: &RunConfig) -> Report {
    binding.set_output_limit(config.output_limit);
    let languages = binding.languages();
    let mut report = Report {
        run_id: config.run_id.clone(),
        format: binding.format(),
        languages: languages.clone(),
        output_limit: config.output_limit,
        missing_languages: 2usize.saturating_sub(languages.len()),
        ..Report::default()
    };
    for language in &languages {
        let mut run = Run {
            binding: &mut *binding,
            config,
            language,
            report: &mut report,
            transactions: 0,
            advertised: BTreeSet::new(),
        };
        run.world();
        run.other_provider();
        run.reach();
        run.output();
        run.releases();
        run.isolation();
        run.no_canary_anywhere();
    }
    for language in &languages {
        for answer in Exercised::ALL {
            if !report.exercised.contains(&(language.clone(), answer)) {
                report.uncovered.insert(Uncovered {
                    language: language.clone(),
                    answer,
                });
            }
        }
    }
    report
}

struct Run<'a> {
    binding: &'a mut dyn BoundaryHarness,
    config: &'a RunConfig,
    language: &'a str,
    report: &'a mut Report,
    transactions: usize,
    /// The providers whose signed advertisement the binding was given: a release is vouched for by the key its
    /// provider's advertisement lists.
    advertised: BTreeSet<String>,
}

/// What one module did on one operation.
struct Seen {
    provider: String,
    handled: Handled,
    events: Events,
    declaration: Declaration,
    version: String,
    transaction: String,
}

impl Seen {
    fn answered(&self) -> bool {
        matches!(self.handled, Handled::Answered(_))
    }

    fn refused(&self, refusal: PlatformRefusal) -> bool {
        self.handled == Handled::Refused(refusal)
    }

    fn reference(&self) -> Option<String> {
        let Handled::Answered(result) = &self.handled else {
            return None;
        };
        let value = serde_json::to_value(&result.statement).ok()?;
        value["outcome"]["evidence"][0]["reference"]
            .as_str()
            .map(str::to_owned)
    }

    fn record(&self) -> Option<&OutboundCallRecord> {
        self.events.records.first()
    }
}

/// One destination of a declaration the suite writes.
struct Dest {
    name: String,
    endpoint: Endpoint,
    addresses: Value,
}

/// What a declaration the suite writes holds besides its provider and version.
#[derive(Default)]
struct Shape {
    destinations: Vec<Dest>,
    secrets: Vec<&'static str>,
    calls: Vec<(String, Vec<&'static str>)>,
}

/// A widening release's fixture and the capability it adds; `None` for a narrowing release.
type Held = Option<(Exercised, Widened)>;

/// The capability a widening release adds, which the suite probes before and after its acceptance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Widened {
    /// A new destination, `extra`.
    Destination,
    /// A new secret, `release.token`.
    Secret,
    /// A new callee, the other provider.
    Callee,
    /// Grown addresses: `spread` opened from a pin it is served outside to `public`.
    Address,
}

/// What came of a call to `endpoint` among `events`, given whether the endpoint received anything, where the
/// expected refusal is `reason` at the first hop.
fn reach_of(events: &Events, endpoint: &Endpoint, received: bool, reason: RefusalReason) -> Reach {
    let Some(record) = events.records.iter().find(|r| {
        r.hops
            .first()
            .is_some_and(|h| h.host.as_str() == endpoint.host)
    }) else {
        return Reach::Unclear;
    };
    match (refused_call(record, reason, 0), completed(record), received) {
        (true, _, false) => Reach::Refused,
        (_, true, true) => Reach::Reached,
        (true, _, true) => Reach::RefusedButDelivered,
        _ => Reach::Unclear,
    }
}

/// What came of a call to one endpoint, judged on the record and on what the endpoint received.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// Recorded as refused as undeclared, and nothing received.
    Refused,
    /// Recorded as completed, and received: the binding allowed it.
    Reached,
    /// Recorded as refused, but received: the egress check itself is broken, which the egress probes judge, so a
    /// reach probe neither passes nor fails on it.
    RefusedButDelivered,
    /// Anything else.
    Unclear,
}

fn refused_call(record: &OutboundCallRecord, reason: RefusalReason, at_hop: usize) -> bool {
    record.outcome == CallOutcome::Refused(Refused { reason }) && record.hops.len() == at_hop + 1
}

fn completed(record: &OutboundCallRecord) -> bool {
    matches!(record.outcome, CallOutcome::Completed(_))
}

impl Run<'_> {
    fn name(&self, what: &str) -> String {
        format!("{what}-{}", self.language)
    }

    fn victim(&self) -> String {
        self.name("bravo")
    }

    fn endpoint(&self, label: &str) -> Endpoint {
        Endpoint::https(&format!("{label}.alpha-{}.example", self.language))
    }

    fn victims_endpoint(&self) -> Endpoint {
        Endpoint::https(&format!("api.bravo-{}.example", self.language))
    }

    fn undeclared_endpoint(&self) -> Endpoint {
        Endpoint::https(&format!("evil-{}.example", self.language))
    }

    fn every_endpoint(&self) -> Vec<Endpoint> {
        LABELS
            .iter()
            .map(|label| self.endpoint(label))
            .chain([self.undeclared_endpoint(), self.victims_endpoint()])
            .collect()
    }

    fn canary(&self, what: &str) -> String {
        format!("canary-{what}-{}", self.language)
    }

    fn own_canary(&self, provider: &str) -> String {
        format!("canary-own-{provider}")
    }

    fn find(&mut self, check: Check, detail: impl Into<String>) {
        self.report.findings.push(Finding {
            check,
            language: self.language.to_owned(),
            detail: detail.into(),
        });
    }

    fn saw(&mut self, answer: Exercised) {
        self.report
            .exercised
            .insert((self.language.to_owned(), answer));
    }

    /// The endpoints, and the other provider's secret and state, every probe in this language shares.
    fn world(&mut self) {
        let v4 = |last: u8| IpAddr::V4(Ipv4Addr::new(185, 12, 64, last));
        let (api, internal, relay, undeclared) = (
            self.endpoint("api"),
            self.endpoint("internal"),
            self.endpoint("relay"),
            self.undeclared_endpoint(),
        );
        let served: [(&str, IpAddr, Option<&Endpoint>); 13] = [
            ("api", v4(10), None),
            ("files", v4(11), Some(&api)),
            ("moved", v4(12), Some(&undeclared)),
            ("internal", IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5)), None),
            ("chain", v4(16), Some(&relay)),
            ("relay", v4(17), Some(&undeclared)),
            ("bounce", v4(18), Some(&internal)),
            ("loopback6", IpAddr::V6(Ipv6Addr::LOCALHOST), None),
            (
                "local6",
                IpAddr::V6(Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 5)),
                None,
            ),
            (
                "mapped",
                IpAddr::V6(Ipv4Addr::new(10, 0, 0, 5).to_ipv6_mapped()),
                None,
            ),
            ("extra", v4(13), None),
            ("archive", v4(15), None),
            ("spread", v4(19), None),
        ];
        for (label, address, redirect) in served {
            let endpoint = self.endpoint(label);
            self.binding.serve(&endpoint, address, redirect);
        }
        self.binding.serve(&undeclared, v4(20), None);
        self.binding.serve(&self.victims_endpoint(), v4(30), None);
        let victim = self.victim();
        let (secret, state) = (self.canary("secret"), self.canary("state"));
        self.binding.seed_secret(&victim, SHARED_SECRET, &secret);
        self.binding.seed_state(&victim, SHARED_STATE, &state);
    }

    fn dest(&self, label: &str, addresses: Value) -> Dest {
        Dest {
            name: label.to_owned(),
            endpoint: self.endpoint(label),
            addresses,
        }
    }

    fn declaration(&self, provider: &str, version: &str, shape: &Shape) -> Declaration {
        let destinations: Vec<Value> = shape
            .destinations
            .iter()
            .map(|d| {
                json!({
                    "name": d.name,
                    "scheme": "https",
                    "host": d.endpoint.host,
                    "port": d.endpoint.port,
                    "addresses": d.addresses,
                })
            })
            .collect();
        let names: Vec<&str> = shape.destinations.iter().map(|d| d.name.as_str()).collect();
        let calls: Vec<Value> = shape
            .calls
            .iter()
            .map(|(callee, kinds)| json!({ "callee": callee, "kinds": kinds }))
            .collect();
        serde_json::from_value(json!({
            "provider": provider,
            "version": version,
            "interface": rutent_contracts::module_interface::VERSION,
            "sandboxed": { "handles": ["withdrawal"] },
            "outside": {
                "operations": [{ "name": OPERATION, "destinations": names, "secrets": shape.secrets }],
                "destinations": destinations,
                "secrets": shape.secrets,
                "calls": calls,
                "statement_key": public_key(&statements_key(provider)),
            },
        }))
        .expect("the suite's declaration is valid")
    }

    fn base_shape(&self) -> Shape {
        Shape {
            destinations: DECLARED
                .iter()
                .map(|l| self.dest(l, json!("public")))
                .collect(),
            secrets: vec![OWN_SECRET, SHARED_SECRET],
            calls: Vec::new(),
        }
    }

    fn operation(&mut self, provider: &str) -> Statement<Operation> {
        self.transactions += 1;
        let context = &self.config.context;
        let value = json!({
            "deployment": context.deployment,
            "protocol_version": context.protocol_version,
            "shape": OPERATION_SHAPE,
            "maker": context.operator,
            "made_at": "2026-11-07T00:00:00Z",
            "signature": {
                "scheme": "ed25519",
                "key": context.deployment.operator_key,
                "value": placeholder_signature(),
            },
            "statement": {
                "kind": "withdrawal",
                "transaction": format!("tx-{}-{}", self.language, self.transactions),
                "provider": provider,
                "origin": { "network": "conformance-network" },
                "destination": "conformance-ledger",
                "asset": "COP",
                "amount": "1000",
                "dry_run": false,
                "inputs": {},
            },
        });
        serde_json::from_value(value).expect("the suite's operation is valid")
    }

    /// Gives the binding `provider`'s signed advertisement, once, before its first release.
    fn vouch(&mut self, provider: &str) {
        if !self.advertised.insert(provider.to_owned()) {
            return;
        }
        let bytes = advertisement(provider, &self.config.context);
        if !self.binding.list_provider(&bytes) {
            self.find(
                Check::AdmissionRecordsWhatItAdmits,
                "the binding wouldn't take a provider's signed advertisement",
            );
        }
    }

    /// The provider's release assertion of `binaries` under `declaration`, signed with its advertising key over
    /// the contract's signing input.
    fn assertion(
        &mut self,
        declaration: &Declaration,
        binaries: &Parts<Vec<u8>>,
    ) -> Option<Statement<ReleaseAssertion>> {
        let provider = declaration.provider.as_str();
        self.vouch(provider);
        let format = BinaryFormat::new(self.binding.format())?;
        let part = |bytes: &[u8]| -> Option<BinaryPart> {
            Some(BinaryPart {
                format: format.clone(),
                digest: Digest {
                    algorithm: DigestAlgorithm::Sha256,
                    value: DigestValue::new(hex_digest(bytes))?,
                },
            })
        };
        let context = &self.config.context;
        let unsigned = Statement {
            deployment: context.deployment.clone(),
            protocol_version: context.protocol_version.clone(),
            shape: RELEASE_ASSERTION_SHAPE.to_owned(),
            maker: provider.to_owned(),
            made_at: "2026-11-07T00:00:00Z".to_owned(),
            signature: Signature {
                scheme: SignatureScheme::Ed25519,
                key: public_key(&advertising_key(provider)),
                value: placeholder_signature(),
            },
            statement: ReleaseAssertion {
                declaration: declaration.clone(),
                parts: Parts {
                    sandboxed: part(&binaries.sandboxed)?,
                    outside: part(&binaries.outside)?,
                },
            },
        };
        Some(sign_statement(&unsigned, &advertising_key(provider)))
    }

    /// Builds and admits a release. The binaries submitted, the assertion and what admission gave.
    fn release(
        &mut self,
        spec: &ModuleSpec,
    ) -> Option<(Parts<Vec<u8>>, Statement<ReleaseAssertion>, Admitted)> {
        let binaries = self.binding.build(spec, self.language)?;
        let assertion = self.assertion(&spec.declaration, &binaries)?;
        let admitted = self.binding.admit(&assertion, &binaries);
        Some((binaries, assertion, admitted))
    }

    /// Admits `spec` as its provider's first release and accepts it.
    fn install(&mut self, spec: &ModuleSpec) -> bool {
        let provider = spec.declaration.provider.as_str().to_owned();
        let version = spec.declaration.version.as_str().to_owned();
        self.binding
            .seed_secret(&provider, OWN_SECRET, &self.own_canary(&provider));
        self.binding
            .seed_state(&provider, OWN_STATE, &self.own_canary(&provider));
        match self.release(spec) {
            Some((_, _, Admitted::Artefact(_))) => {
                self.binding.accept(&provider, &version, REVIEWER).is_some()
            }
            _ => false,
        }
    }

    /// Hands the provider one operation; what it gave and what the binding saw.
    fn handle(&mut self, provider: &str) -> (Handled, Events, String) {
        self.binding.take_events();
        let operation = self.operation(provider);
        let handled = self.binding.handle(provider, &operation);
        let events = self.binding.take_events();
        let transaction = serde_json::to_value(&operation.statement)
            .ok()
            .and_then(|v| v["transaction"].as_str().map(str::to_owned))
            .unwrap_or_default();
        (handled, events, transaction)
    }

    fn probe_with(
        &mut self,
        label: &str,
        shape: &Shape,
        steps: Vec<Step>,
        output: Output,
    ) -> Option<Seen> {
        let provider = self.name(&format!("alpha-{label}"));
        let spec = ModuleSpec {
            declaration: self.declaration(&provider, "1.0.0", shape),
            steps,
            output,
            starts: true,
        };
        if !self.install(&spec) {
            return None;
        }
        let (handled, events, transaction) = self.handle(&provider);
        let seen = Seen {
            provider,
            handled,
            events,
            declaration: spec.declaration,
            version: "1.0.0".to_owned(),
            transaction,
        };
        self.records_tell_the_truth(&seen);
        Some(seen)
    }

    fn probe(&mut self, label: &str, steps: Vec<Step>, output: Output) -> Option<Seen> {
        let shape = self.base_shape();
        self.probe_with(label, &shape, steps, output)
    }

    fn records_tell_the_truth(&mut self, seen: &Seen) {
        for record in &seen.events.records {
            // The secrets a call used are the ones its outside part resolved for its operation, no more, no fewer.
            let used: std::collections::BTreeSet<&str> = seen
                .events
                .lookups
                .iter()
                .filter(|e| {
                    e.outcome == Some(LookupOutcome::Resolved)
                        && e.lookup.operation == record.operation
                })
                .map(|e| e.lookup.name.as_str())
                .collect();
            let listed: std::collections::BTreeSet<&str> =
                record.secrets.iter().map(|s| s.as_str()).collect();
            let problems = [
                (listed != used)
                    .then(|| "it doesn't name exactly the secrets its call used".to_owned()),
                validate_record(record).err().map(|v| format!("{v:?}")),
                validate_record_secrets(&seen.declaration, record)
                    .err()
                    .map(|v| format!("{v:?}")),
                (record.transaction.as_str() != seen.transaction)
                    .then(|| "another transaction".to_owned()),
                (record.provider.as_str() != seen.provider).then(|| "another provider".to_owned()),
                (record.module_version.as_str() != seen.version)
                    .then(|| "another module version".to_owned()),
                (record.operation.as_str() != OPERATION).then(|| "another operation".to_owned()),
            ];
            for problem in problems.into_iter().flatten() {
                self.find(
                    Check::RecordsTellTheTruth,
                    format!("{}'s outbound call record: {problem}", seen.provider),
                );
            }
        }
    }

    /// Whether `canary` appears in what the module could write to or was handed: its result, its records, the
    /// secret values the proxy delivered to it, its calls, or anything any endpoint received. The channel, if so.
    fn leak(&self, seen: &Seen, canary: &str) -> Option<&'static str> {
        let text = |value: Result<String, serde_json::Error>| value.unwrap_or_default();
        if let Handled::Answered(result) = &seen.handled
            && text(serde_json::to_string(result)).contains(canary)
        {
            return Some("the module's result");
        }
        if text(serde_json::to_string(&seen.events.records)).contains(canary) {
            return Some("an outbound call record");
        }
        if self
            .binding
            .delivered_secrets(&seen.provider)
            .iter()
            .any(|v| v == canary)
        {
            return Some("a secret the proxy delivered");
        }
        if seen
            .events
            .calls
            .iter()
            .any(|c| text(serde_json::to_string(c)).contains(canary))
        {
            return Some("a cross-module call");
        }
        let reached = self.every_endpoint().iter().any(|e| {
            self.binding
                .received(e)
                .iter()
                .any(|body| body.contains(canary))
        });
        reached.then_some("a request an endpoint received")
    }

    /// Installs the other provider, running, with a declaration naming its own endpoint, so that a binding
    /// sharing one egress allowlist, or delivering to it, is seen.
    fn other_provider(&mut self) {
        let victim = self.victim();
        let shape = Shape {
            destinations: vec![Dest {
                name: "bravo_api".to_owned(),
                endpoint: self.victims_endpoint(),
                addresses: json!("public"),
            }],
            secrets: vec![SHARED_SECRET],
            calls: Vec::new(),
        };
        let spec = ModuleSpec {
            declaration: self.declaration(&victim, "1.0.0", &shape),
            steps: Vec::new(),
            output: Output::Honest,
            starts: true,
        };
        if !self.install(&spec) || self.binding.active_version(&victim).is_none() {
            self.find(
                Check::DeclaredReachWorks,
                "the other provider's module didn't run",
            );
        }
    }

    /// The fixtures and controls of what a module may reach.
    fn reach(&mut self) {
        self.controls();
        self.other_providers_secrets_and_state();
        self.lookups_the_module_has_no_right_to();
        self.egress();
        self.cross_module_calls();
    }

    fn controls(&mut self) {
        let api = self.endpoint("api");
        if let Some(seen) = self.probe(
            "own-secret",
            vec![lookup(PartName::Outside, OWN_SECRET), call(api.clone())],
            Output::Honest,
        ) {
            if seen
                .events
                .lookups
                .iter()
                .any(|e| e.outcome == Some(LookupOutcome::Resolved))
            {
                self.saw(Exercised::OwnSecretResolved);
            } else {
                self.find(
                    Check::DeclaredReachWorks,
                    "its own declared secret didn't resolve",
                );
            }
            if seen
                .record()
                .is_some_and(|r| completed(r) && r.hops.len() == 1)
                && !self.binding.received(&api).is_empty()
            {
                self.saw(Exercised::DeclaredCallCompleted);
            } else {
                self.find(
                    Check::DeclaredReachWorks,
                    "a call to its declared endpoint didn't complete",
                );
            }
            if seen.answered() {
                self.saw(Exercised::HonestOutputPassed);
            } else {
                self.find(
                    Check::DeclaredReachWorks,
                    "its honest output wasn't passed on",
                );
            }
        }
        if let Some(seen) = self.probe(
            "own-state",
            vec![read(PartName::Outside, OWN_STATE)],
            Output::Honest,
        ) {
            if seen.events.state_reads.iter().any(|e| e.found) {
                self.saw(Exercised::OwnStateRead);
            } else {
                self.find(Check::DeclaredReachWorks, "its own state didn't read back");
            }
        }
        if let Some(seen) = self.probe(
            "declared-redirect",
            vec![call(self.endpoint("files"))],
            Output::Honest,
        ) {
            if seen
                .record()
                .is_some_and(|r| completed(r) && r.hops.len() == 2)
            {
                self.saw(Exercised::DeclaredRedirectFollowed);
            } else {
                self.find(
                    Check::DeclaredReachWorks,
                    "a redirect to a listed destination wasn't followed",
                );
            }
        }
        let victim = self.victim();
        let mut shape = self.base_shape();
        shape.calls = vec![(victim.clone(), vec!["withdrawal"])];
        let before = self.binding.operations_received(&victim);
        if let Some(seen) = self.probe_with(
            "declared-call",
            &shape,
            vec![call_module(&victim)],
            Output::Honest,
        ) {
            let record_ok = seen.events.calls.iter().any(|c| {
                matches!(c.outcome, CrossModuleOutcome::Delivered(_))
                    && validate_call(c).is_ok()
                    && validate_call_against(&seen.declaration, c).is_ok()
            });
            if record_ok && self.binding.operations_received(&victim) > before {
                self.saw(Exercised::DeclaredCallDelivered);
            } else {
                self.find(
                    Check::DeclaredReachWorks,
                    "a declared cross-module call wasn't delivered",
                );
            }
        }
    }

    fn other_providers_secrets_and_state(&mut self) {
        let api = self.endpoint("api");
        let secret = self.canary("secret");
        let state = self.canary("state");
        for (label, steps, fixture, check, canary) in [
            (
                "foreign-secret-outside",
                vec![lookup(PartName::Outside, SHARED_SECRET), call(api.clone())],
                Exercised::ForeignSecretRefusedToOutside,
                Check::OtherProvidersSecrets,
                secret.clone(),
            ),
            (
                "foreign-secret-sandboxed",
                vec![lookup(PartName::Sandboxed, SHARED_SECRET)],
                Exercised::ForeignSecretRefusedToSandboxed,
                Check::OtherProvidersSecrets,
                secret.clone(),
            ),
            (
                "foreign-state-outside",
                vec![read(PartName::Outside, SHARED_STATE), call(api.clone())],
                Exercised::ForeignStateRefusedToOutside,
                Check::OtherProvidersState,
                state.clone(),
            ),
            (
                "foreign-state-sandboxed",
                vec![read(PartName::Sandboxed, SHARED_STATE)],
                Exercised::ForeignStateRefusedToSandboxed,
                Check::OtherProvidersState,
                state.clone(),
            ),
        ] {
            let Some(seen) = self.probe(label, steps, Output::Honest) else {
                continue;
            };
            let sandboxed = label.ends_with("sandboxed");
            let refused = match check {
                Check::OtherProvidersSecrets => {
                    seen.events.lookups.iter().any(|e| match e.outcome {
                        None => sandboxed,
                        Some(outcome) => !sandboxed && matches!(outcome, LookupOutcome::Refused(_)),
                    })
                }
                _ => seen.events.state_reads.iter().any(|e| !e.found),
            };
            if sandboxed
                && check == Check::OtherProvidersSecrets
                && seen.events.lookups.iter().any(|e| e.outcome.is_some())
            {
                self.find(
                    Check::SandboxedPartHoldsNoSecret,
                    "a sandboxed part's lookup reached the proxy",
                );
            }
            // The module declares the shared name and holds no secret by it: the proxy must say not found.
            let wrong_reason = !sandboxed
                && check == Check::OtherProvidersSecrets
                && seen.events.lookups.iter().any(|e| {
                    matches!(e.outcome, Some(LookupOutcome::Refused(r)) if r != SecretRefusalReason::NotFound)
                });
            match self.leak(&seen, &canary) {
                Some(channel) => self.find(
                    check,
                    format!("another provider's canary appeared in {channel}"),
                ),
                None if wrong_reason => self.find(
                    check,
                    "another provider's secret was refused for another reason than not_found",
                ),
                None if refused => self.saw(fixture),
                None => {}
            }
        }
    }

    fn lookups_the_module_has_no_right_to(&mut self) {
        if let Some(seen) = self.probe(
            "sandboxed-own-secret",
            vec![lookup(PartName::Sandboxed, OWN_SECRET)],
            Output::Honest,
        ) {
            let reached = seen.events.lookups.iter().any(|e| e.outcome.is_some());
            let leaked = seen
                .reference()
                .is_some_and(|r| r.contains(&self.own_canary(&seen.provider)));
            if reached || leaked {
                self.find(
                    Check::SandboxedPartHoldsNoSecret,
                    if leaked {
                        "its own secret appeared in the sandboxed part's output"
                    } else {
                        "a sandboxed part's lookup reached the proxy"
                    },
                );
            } else if !seen.events.lookups.is_empty() {
                self.saw(Exercised::SandboxedLookupRefused);
            }
        }
        // A name the module doesn't declare, though its provider holds a secret by it.
        let provider = self.name("alpha-undeclared-secret");
        let unlisted = self.canary("unlisted");
        self.binding
            .seed_secret(&provider, UNLISTED_SECRET, &unlisted);
        if let Some(seen) = self.probe(
            "undeclared-secret",
            vec![lookup(PartName::Outside, UNLISTED_SECRET)],
            Output::Honest,
        ) {
            let refused = seen.events.lookups.iter().any(|e| {
                e.outcome
                    == Some(LookupOutcome::Refused(
                        SecretRefusalReason::UndeclaredSecret,
                    ))
            });
            if self.leak(&seen, &unlisted).is_some() || !refused && !seen.events.lookups.is_empty()
            {
                self.find(
                    Check::UndeclaredSecretRefused,
                    "a secret the module doesn't declare wasn't refused as undeclared",
                );
            } else if refused {
                self.saw(Exercised::UndeclaredSecretNameRefused);
            }
        }
    }

    fn egress(&mut self) {
        let (undeclared, internal) = (self.undeclared_endpoint(), self.endpoint("internal"));
        let cases = [
            (
                "undeclared-destination",
                undeclared.clone(),
                undeclared.clone(),
                RefusalReason::UndeclaredDestination,
                0,
                Exercised::UndeclaredDestinationRefused,
                Check::UndeclaredDestinationRefused,
            ),
            (
                "other-providers-endpoint",
                self.victims_endpoint(),
                self.victims_endpoint(),
                RefusalReason::UndeclaredDestination,
                0,
                Exercised::OtherProvidersEndpointRefused,
                Check::UndeclaredDestinationRefused,
            ),
            (
                "undeclared-redirect",
                self.endpoint("moved"),
                undeclared.clone(),
                RefusalReason::RedirectToUndeclaredDestination,
                1,
                Exercised::UndeclaredRedirectRefused,
                Check::RedirectChecked,
            ),
            (
                "redirect-chain",
                self.endpoint("chain"),
                undeclared.clone(),
                RefusalReason::RedirectToUndeclaredDestination,
                2,
                Exercised::RedirectChainRefused,
                Check::RedirectChecked,
            ),
            (
                "altered-resolution",
                internal.clone(),
                internal.clone(),
                RefusalReason::AddressNotAllowed,
                0,
                Exercised::AddressRefused,
                Check::ConnectedAddressChecked,
            ),
            (
                "redirect-to-private",
                self.endpoint("bounce"),
                internal.clone(),
                RefusalReason::AddressNotAllowed,
                1,
                Exercised::RedirectToPrivateAddressRefused,
                Check::ConnectedAddressChecked,
            ),
            (
                "ipv6-loopback",
                self.endpoint("loopback6"),
                self.endpoint("loopback6"),
                RefusalReason::AddressNotAllowed,
                0,
                Exercised::Ipv6LoopbackRefused,
                Check::ConnectedAddressChecked,
            ),
            (
                "ipv6-unique-local",
                self.endpoint("local6"),
                self.endpoint("local6"),
                RefusalReason::AddressNotAllowed,
                0,
                Exercised::Ipv6UniqueLocalRefused,
                Check::ConnectedAddressChecked,
            ),
            (
                "ipv4-mapped",
                self.endpoint("mapped"),
                self.endpoint("mapped"),
                RefusalReason::AddressNotAllowed,
                0,
                Exercised::Ipv4MappedRefused,
                Check::ConnectedAddressChecked,
            ),
        ];
        for (label, target, watched, reason, hop, fixture, check) in cases {
            let before = self.binding.received(&watched).len();
            let Some(seen) = self.probe(label, vec![call(target)], Output::Honest) else {
                continue;
            };
            if self.binding.received(&watched).len() > before {
                self.find(
                    check,
                    format!("{watched} received a request it must not, in {label}"),
                );
            } else if seen.record().is_some_and(|r| refused_call(r, reason, hop)) {
                self.saw(fixture);
            } else if seen.record().is_none() {
                self.find(
                    Check::RecordsTellTheTruth,
                    format!("no record of the call in {label}"),
                );
            }
        }
    }

    fn cross_module_calls(&mut self) {
        let victim = self.victim();
        let mut undeclared_kind = self.base_shape();
        undeclared_kind.calls = vec![(victim.clone(), vec!["deposit"])];
        for (label, shape, reason, fixture) in [
            (
                "undeclared-call",
                self.base_shape(),
                CallRefusalReason::UndeclaredCallee,
                Exercised::UndeclaredCallRefused,
            ),
            (
                "undeclared-kind",
                undeclared_kind,
                CallRefusalReason::UndeclaredKind,
                Exercised::UndeclaredKindRefused,
            ),
        ] {
            let before = self.binding.operations_received(&victim);
            let Some(seen) =
                self.probe_with(label, &shape, vec![call_module(&victim)], Output::Honest)
            else {
                continue;
            };
            let mut problems = Vec::new();
            if self.binding.operations_received(&victim) > before {
                problems.push(format!("the running callee received the call in {label}"));
            }
            for call in &seen.events.calls {
                if let Err(v) = validate_call(call) {
                    problems.push(format!("its record is invalid: {v:?}"));
                }
                if let Err(v) = validate_call_against(&seen.declaration, call) {
                    problems.push(format!(
                        "its record contradicts the caller's declaration: {v:?}"
                    ));
                }
            }
            let refused = seen.events.calls.iter().any(|c| {
                matches!(&c.outcome, CrossModuleOutcome::Refused(r) if r.reason == reason && r.callee.as_str() == victim)
            });
            if !refused && !seen.events.calls.is_empty() {
                problems.push(format!(
                    "the call wasn't recorded as refused for {reason:?}"
                ));
            }
            if problems.is_empty() && refused {
                self.saw(fixture);
            }
            for problem in problems {
                self.find(Check::UndeclaredCallRefused, problem);
            }
        }
    }

    /// The fixtures of what a module returns.
    fn output(&mut self) {
        let limit = self.config.output_limit;
        for (label, output, fixture) in [
            (
                "unparseable",
                Output::Unparseable,
                Exercised::UnparseableOutputRefused,
            ),
            (
                "another-kind",
                Output::AnotherKind,
                Exercised::OutputOfAnotherKindRefused,
            ),
            (
                "another-transaction",
                Output::AnotherTransaction,
                Exercised::OutputForAnotherTransactionRefused,
            ),
            (
                "unknown-field",
                Output::UnknownField,
                Exercised::OutputWithAnUnknownFieldRefused,
            ),
        ] {
            if let Some(seen) = self.probe(label, Vec::new(), output) {
                if seen.refused(PlatformRefusal::MalformedOutput) {
                    self.saw(fixture);
                } else {
                    self.find(
                        Check::OutputShapeChecked,
                        format!("{label} output wasn't refused as malformed"),
                    );
                }
            }
        }
        if let Some(seen) = self.probe("at-the-limit", Vec::new(), Output::PaddedTo(limit)) {
            let length = match &seen.handled {
                Handled::Answered(result) => {
                    serde_json::to_vec(&result.statement).map(|b| b.len()).ok()
                }
                Handled::Refused(_) => None,
            };
            match length {
                Some(length) if length == limit => self.saw(Exercised::OutputAtTheLimitPassed),
                Some(length) => self.find(
                    Check::OutputSizeBounded,
                    format!("the output passed on at the limit is {length} bytes, not {limit}"),
                ),
                None => self.find(
                    Check::OutputSizeBounded,
                    "output exactly at the limit wasn't passed on",
                ),
            }
        }
        if let Some(seen) = self.probe("over-the-limit", Vec::new(), Output::PaddedTo(limit + 1)) {
            if seen.refused(PlatformRefusal::OversizedOutput) {
                self.saw(Exercised::OversizedOutputRefused);
            } else {
                self.find(
                    Check::OutputSizeBounded,
                    "output one byte over the limit wasn't refused",
                );
            }
        }
    }

    /// Whether the provider's module answers as `version`: its evidence reference is `{provider}-{version}`, then
    /// `-{value}` for any value its steps obtained.
    fn answers_as(&mut self, provider: &str, version: &str) -> bool {
        let (handled, _, _) = self.handle(provider);
        let Handled::Answered(result) = handled else {
            return false;
        };
        let expected = format!("{provider}-{version}");
        serde_json::to_value(&result.statement).is_ok_and(|v| {
            v["outcome"]["evidence"][0]["reference"]
                .as_str()
                .is_some_and(|r| r == expected || r.starts_with(&format!("{expected}-")))
        })
    }

    /// Hands the provider one operation, and what came of its call to `label`'s endpoint.
    fn reaches(&mut self, provider: &str, label: &str) -> Reach {
        let endpoint = self.endpoint(label);
        let before = self.binding.received(&endpoint).len();
        let (_, events, _) = self.handle(provider);
        reach_of(
            &events,
            &endpoint,
            self.binding.received(&endpoint).len() > before,
            RefusalReason::UndeclaredDestination,
        )
    }

    /// Hands the release provider one operation, and what came of the capability `widened` names. Each is judged
    /// beside a control in the same operation: a secret no release declares, and a callee no release declares. If
    /// a control fails, the binding's own secret or call enforcement is broken, which the reach probes judge, and
    /// the answer is `Unclear`.
    fn lends(&mut self, provider: &str, widened: Widened) -> Reach {
        let (victim, nobody) = (self.victim(), self.name("nobody"));
        let (extra, spread) = (self.endpoint("extra"), self.endpoint("spread"));
        let before = (
            self.binding.received(&extra).len(),
            self.binding.received(&spread).len(),
            self.binding.operations_received(&victim),
            self.binding.operations_received(&nobody),
        );
        let (_, events, _) = self.handle(provider);
        let lookup_of = |name: &str| {
            events
                .lookups
                .iter()
                .find(|e| e.lookup.name.as_str() == name)
                .and_then(|e| e.outcome)
        };
        let call_to = |callee: &str| {
            events
                .calls
                .iter()
                .find(|c| c.callee() == callee)
                .map(|c| c.outcome.clone())
        };
        match widened {
            Widened::Destination => reach_of(
                &events,
                &extra,
                self.binding.received(&extra).len() > before.0,
                RefusalReason::UndeclaredDestination,
            ),
            Widened::Address => reach_of(
                &events,
                &spread,
                self.binding.received(&spread).len() > before.1,
                RefusalReason::AddressNotAllowed,
            ),
            Widened::Secret => {
                let control = lookup_of(RELEASE_UNLISTED)
                    == Some(LookupOutcome::Refused(
                        SecretRefusalReason::UndeclaredSecret,
                    ))
                    && lookup_of(OWN_SECRET) == Some(LookupOutcome::Resolved);
                match lookup_of(RELEASE_SECRET) {
                    _ if !control => Reach::Unclear,
                    Some(LookupOutcome::Refused(SecretRefusalReason::UndeclaredSecret)) => {
                        Reach::Refused
                    }
                    Some(LookupOutcome::Resolved) => Reach::Reached,
                    _ => Reach::Unclear,
                }
            }
            Widened::Callee => {
                let control = matches!(call_to(&nobody), Some(CrossModuleOutcome::Refused(r)) if r.reason == CallRefusalReason::UndeclaredCallee)
                    && self.binding.operations_received(&nobody) == before.3;
                let received = self.binding.operations_received(&victim) > before.2;
                match call_to(&victim) {
                    _ if !control => Reach::Unclear,
                    Some(CrossModuleOutcome::Refused(r))
                        if r.reason == CallRefusalReason::UndeclaredCallee && !received =>
                    {
                        Reach::Refused
                    }
                    // Passed on, answered or not: the callee received it, so the widening was lent.
                    Some(CrossModuleOutcome::Delivered(_) | CrossModuleOutcome::Unanswered(_))
                        if received =>
                    {
                        Reach::Reached
                    }
                    _ => Reach::Unclear,
                }
            }
        }
    }

    fn release_shape(
        &self,
        destinations: &[(&str, Value)],
        secrets: &[&'static str],
        calls: bool,
    ) -> Shape {
        Shape {
            destinations: destinations
                .iter()
                .map(|(l, a)| self.dest(l, a.clone()))
                .collect(),
            secrets: secrets.to_vec(),
            calls: if calls {
                vec![(self.victim(), vec!["withdrawal"])]
            } else {
                Vec::new()
            },
        }
    }

    /// A release of the release provider. Its module looks up its own secret, the secret only a later release
    /// declares and one no release declares; calls each endpoint a release may declare; and calls the other
    /// provider, which only a later release declares, and a callee no release declares.
    fn release_spec(&self, provider: &str, version: &str, shape: &Shape) -> ModuleSpec {
        let mut steps = vec![
            lookup(PartName::Outside, OWN_SECRET),
            lookup(PartName::Outside, RELEASE_SECRET),
            lookup(PartName::Outside, RELEASE_UNLISTED),
        ];
        steps.extend(
            ["api", "extra", "archive", "spread"]
                .iter()
                .map(|l| call(self.endpoint(l))),
        );
        steps.push(call_module(&self.victim()));
        steps.push(call_module(&self.name("nobody")));
        ModuleSpec {
            declaration: self.declaration(provider, version, shape),
            steps,
            output: Output::Honest,
            starts: true,
        }
    }

    /// The fixtures of admission: a first release, altered ones, and a sequence of widening and narrowing ones.
    fn releases(&mut self) {
        let provider = self.name("release");
        for (name, value) in [
            (OWN_SECRET, self.own_canary(&provider)),
            (RELEASE_SECRET, self.canary("release-token")),
            (RELEASE_UNLISTED, self.canary("release-unlisted")),
        ] {
            self.binding.seed_secret(&provider, name, &value);
        }
        let public = json!("public");
        // `spread` is served at 185.12.64.19, outside this pin, until a release opens it to `public`.
        let pinned = json!(["185.12.64.13/32"]);
        let first = self.release_spec(
            &provider,
            "1.0.0",
            &self.release_shape(
                &[("api", public.clone()), ("archive", public.clone())],
                &[OWN_SECRET],
                false,
            ),
        );
        let Some((binaries, assertion, Admitted::Artefact(artefact))) = self.release(&first) else {
            self.find(
                Check::AdmissionRecordsWhatItAdmits,
                "a first release wasn't admitted",
            );
            return;
        };
        let mut problems = Vec::new();
        if let Err(v) = validate_artefact(&artefact) {
            problems.push(format!("its artefact is invalid: {v:?}"));
        }
        if artefact.assertion != assertion {
            problems.push("its artefact holds another assertion than the one submitted".to_owned());
        }
        if artefact.received.sandboxed.value.as_str() != hex_digest(&binaries.sandboxed)
            || artefact.received.outside.value.as_str() != hex_digest(&binaries.outside)
        {
            problems.push("its received digests aren't the binaries'".to_owned());
        }
        if artefact.acceptance.required != AcceptanceMode::Fresh
            || artefact.acceptance.accepted.is_some()
        {
            problems.push("a first release didn't wait for a fresh acceptance".to_owned());
        }
        if self.binding.active_version(&provider).is_some() {
            problems.push("a first release ran before its acceptance".to_owned());
        }
        let accepted = self.binding.accept(&provider, "1.0.0", REVIEWER);
        if accepted.is_none() || !self.answers_as(&provider, "1.0.0") {
            problems.push("a first release didn't run once accepted".to_owned());
        }
        if problems.is_empty() {
            self.saw(Exercised::FirstReleaseAdmitted);
        }
        for problem in problems {
            self.find(Check::AdmissionRecordsWhatItAdmits, problem);
        }
        self.altered_releases();
        self.forged_releases();

        let Some(previous) = accepted else {
            return;
        };
        let base = |run: &Self, spread: &Value, secrets: &[&'static str], calls: bool| {
            run.release_shape(
                &[
                    ("api", public.clone()),
                    ("extra", public.clone()),
                    ("spread", spread.clone()),
                ],
                secrets,
                calls,
            )
        };
        let widened = self.release_shape(
            &[
                ("api", public.clone()),
                ("archive", public.clone()),
                ("extra", public.clone()),
                ("spread", pinned.clone()),
            ],
            &[OWN_SECRET],
            false,
        );
        let narrowed = base(self, &pinned, &[OWN_SECRET], false);
        let by_secret = base(self, &pinned, &[OWN_SECRET, RELEASE_SECRET], false);
        let by_callee = base(self, &pinned, &[OWN_SECRET, RELEASE_SECRET], true);
        let by_address = base(self, &public, &[OWN_SECRET, RELEASE_SECRET], true);
        let steps: [(&str, Shape, Held); 5] = [
            (
                "1.1.0",
                widened,
                Some((Exercised::WideningReleaseHeld, Widened::Destination)),
            ),
            ("1.2.0", narrowed, None),
            (
                "1.3.0",
                by_secret,
                Some((Exercised::WideningBySecretHeld, Widened::Secret)),
            ),
            (
                "1.4.0",
                by_callee,
                Some((Exercised::WideningByCalleeHeld, Widened::Callee)),
            ),
            (
                "1.5.0",
                by_address,
                Some((Exercised::WideningByAddressHeld, Widened::Address)),
            ),
        ];
        let mut previous = previous;
        for (version, shape, held) in steps {
            let next = self.release_spec(&provider, version, &shape);
            let Some(accepted) = self.successive(&previous, &next, held) else {
                return;
            };
            previous = accepted;
        }
    }

    /// Each part altered in turn, by a byte appended and by a byte flipped in place: all four must be refused as
    /// that part altered.
    fn altered_releases(&mut self) {
        let mut refused = 0;
        for part in [PartName::Sandboxed, PartName::Outside] {
            for (how, alter) in [("appended", false), ("flipped", true)] {
                let label = match part {
                    PartName::Sandboxed => format!("altered-sandboxed-{how}"),
                    PartName::Outside => format!("altered-outside-{how}"),
                };
                let spec = ModuleSpec {
                    declaration: self.declaration(&self.name(&label), "1.0.0", &self.base_shape()),
                    steps: Vec::new(),
                    output: Output::Honest,
                    starts: true,
                };
                let Some(mut binaries) = self.binding.build(&spec, self.language) else {
                    continue;
                };
                let Some(assertion) = self.assertion(&spec.declaration, &binaries) else {
                    continue;
                };
                let bytes = match part {
                    PartName::Sandboxed => &mut binaries.sandboxed,
                    PartName::Outside => &mut binaries.outside,
                };
                match (alter, bytes.last_mut()) {
                    (true, Some(last)) => *last ^= 1,
                    _ => bytes.push(0),
                }
                match self.binding.admit(&assertion, &binaries) {
                    Admitted::Refused(AdmissionRefusal::PartAltered(p)) if p == part => refused += 1,
                    Admitted::Refused(_) => {}
                    Admitted::Artefact(_) => self.find(
                        Check::AdmissionRecordsWhatItAdmits,
                        format!("a release whose {part:?} binary differs from its assertion ({how}) was admitted"),
                    ),
                }
            }
        }
        if refused == 4 {
            self.saw(Exercised::AlteredReleaseRefused);
        }
    }

    /// A release whose assertion doesn't verify is refused as unverified, before its binaries are judged: one
    /// signed by a key no advertisement lists, and one altered after it was signed. The same release, signed
    /// right, is then admitted: the refusals weren't for anything else.
    fn forged_releases(&mut self) {
        let spec = ModuleSpec {
            declaration: self.declaration(&self.name("forged"), "1.0.0", &self.base_shape()),
            steps: Vec::new(),
            output: Output::Honest,
            starts: true,
        };
        let Some(binaries) = self.binding.build(&spec, self.language) else {
            return;
        };
        let Some(genuine) = self.assertion(&spec.declaration, &binaries) else {
            return;
        };
        let mut altered = genuine.clone();
        altered.made_at = "2026-11-07T00:00:01Z".to_owned();
        let forgeries = [
            (
                "signed by a key no advertisement lists",
                sign_statement(&genuine, &another_key()),
            ),
            ("altered after it was signed", altered),
        ];
        let mut refused = 0;
        for (how, forged) in forgeries {
            match self.binding.admit(&forged, &binaries) {
                Admitted::Refused(AdmissionRefusal::AssertionUnverified) => refused += 1,
                other => self.find(
                    Check::AdmissionRecordsWhatItAdmits,
                    format!("a release whose assertion was {how} wasn't refused as unverified: {other:?}"),
                ),
            }
        }
        let genuine_admitted = matches!(
            self.binding.admit(&genuine, &binaries),
            Admitted::Artefact(_)
        );
        if !genuine_admitted {
            self.find(
                Check::AdmissionRecordsWhatItAdmits,
                "the release, signed right, wasn't admitted after its forgeries were refused",
            );
        }
        if refused == 2 && genuine_admitted {
            self.saw(Exercised::ForgedReleaseRefused);
        }
    }

    /// Admits `next` after `previous`, and checks it widens or narrows as intended, waits or runs as it should,
    /// lends no reach before its acceptance, gains it once accepted, keeps none it dropped, and is valid under the
    /// contract. `held` names a widening release's fixture and what it widens; `None` is the narrowing one. The
    /// accepted artefact, if it ran.
    fn successive(
        &mut self,
        previous: &AdmissionArtefact,
        next: &ModuleSpec,
        held: Held,
    ) -> Option<AdmissionArtefact> {
        let provider = next.declaration.provider.as_str().to_owned();
        let version = next.declaration.version.as_str().to_owned();
        let running = previous.declaration().version.as_str().to_owned();
        let computed = widening(previous.declaration(), &next.declaration);
        assert_eq!(
            computed.is_empty(),
            held.is_none(),
            "the suite's own releases widen as intended"
        );
        let Some((_, _, Admitted::Artefact(artefact))) = self.release(next) else {
            self.find(
                Check::WideningNeedsFreshAcceptance,
                format!("release {version} wasn't admitted"),
            );
            return None;
        };
        let mut problems = Vec::new();
        let mut unclear = false;
        if let Err(v) = validate_artefact(&artefact) {
            problems.push(format!("its artefact is invalid: {v:?}"));
        }
        if artefact.capabilities.widenings != computed {
            problems.push("its widenings aren't the computed ones".to_owned());
        }
        let accepted = if let Some((_, widened)) = held {
            if artefact.acceptance.required != AcceptanceMode::Fresh {
                problems.push("a widening release didn't require a fresh acceptance".to_owned());
            }
            if !self.answers_as(&provider, &running) {
                problems.push("a widening release ran before its fresh acceptance".to_owned());
            }
            match self.lends(&provider, widened) {
                Reach::Refused if widened == Widened::Destination => {
                    self.saw(Exercised::PendingReleaseUnreachable)
                }
                Reach::Refused => {}
                Reach::Reached => {
                    problems.push(format!("a waiting release lent its {widened:?} widening"))
                }
                _ => unclear = true,
            }
            self.binding.accept(&provider, &version, REVIEWER)
        } else {
            if artefact.acceptance.required != AcceptanceMode::Automatic
                || artefact.acceptance.accepted.is_none()
            {
                problems.push("a narrowing release wasn't accepted automatically".to_owned());
            }
            Some(*artefact.clone())
        };
        match &accepted {
            Some(accepted) => {
                if let Err(v) = validate_release(previous, accepted) {
                    problems.push(format!("it doesn't follow the previous release: {v:?}"));
                }
            }
            None => problems.push("its acceptance wasn't recorded".to_owned()),
        }
        if !self.answers_as(&provider, &version) {
            problems.push("it didn't run once accepted".to_owned());
        }
        match held {
            Some((_, widened)) => match self.lends(&provider, widened) {
                Reach::Reached => {}
                Reach::Refused => problems.push(format!(
                    "an accepted release's {widened:?} widening wasn't applied"
                )),
                _ => unclear = true,
            },
            None => match self.reaches(&provider, "archive") {
                Reach::Refused => self.saw(Exercised::NarrowedReachRefused),
                Reach::RefusedButDelivered | Reach::Unclear => unclear = true,
                Reach::Reached => {
                    problems.push(
                        "a narrowing release's dropped destination was still reachable".to_owned(),
                    );
                }
            },
        }
        if problems.is_empty() && !unclear {
            self.saw(held.map_or(Exercised::NarrowingReleaseRan, |(fixture, _)| fixture));
        }
        for problem in problems {
            self.find(
                Check::WideningNeedsFreshAcceptance,
                format!("release {version}: {problem}"),
            );
        }
        accepted
    }

    /// The probes of `AC-19` and `REQ-MDL-13`: one provider upgraded, failing to upgrade and restarted, beside
    /// another that must not notice.
    fn isolation(&mut self) {
        let steady = self.name("steady");
        let moving = self.name("upgrading");
        let spec = |run: &Self, provider: &str, version: &str, starts: bool| ModuleSpec {
            declaration: run.declaration(provider, version, &run.base_shape()),
            steps: Vec::new(),
            output: Output::Honest,
            starts,
        };
        let (steady_spec, moving_spec) = (
            spec(self, &steady, "1.0.0", true),
            spec(self, &moving, "1.0.0", true),
        );
        if !self.install(&steady_spec) || !self.install(&moving_spec) {
            self.find(
                Check::ProvidersAreIsolated,
                "the isolation probes' modules didn't install",
            );
            return;
        }
        let steady_process = self.binding.process(&steady);
        let mut problems = Vec::new();
        if steady_process.is_none() || steady_process == self.binding.process(&moving) {
            problems.push("two providers don't run in two processes".to_owned());
        }
        let undisturbed = |run: &mut Self, when: &str, problems: &mut Vec<String>| {
            if run.binding.process(&steady) != steady_process || !run.answers_as(&steady, "1.0.0") {
                problems.push(format!("the other provider was disturbed {when}"));
            }
        };
        undisturbed(self, "at the start", &mut problems);

        // An upgrade, which changes no capability and so runs at once.
        let upgrade = spec(self, &moving, "1.1.0", true);
        let upgraded = matches!(self.release(&upgrade), Some((_, _, Admitted::Artefact(_))))
            && self.binding.active_version(&moving).as_deref() == Some("1.1.0")
            && self.answers_as(&moving, "1.1.0");
        if !upgraded {
            problems.push("an upgrade didn't run".to_owned());
        }
        undisturbed(self, "by an upgrade", &mut problems);

        // A failed upgrade: admitted, but it doesn't start. Refused at admission, it proves nothing, and the fixture
        // stays uncovered.
        let failing = spec(self, &moving, "1.2.0", false);
        if let Some((_, _, Admitted::Artefact(artefact))) = self.release(&failing) {
            // It widens nothing, so it takes an automatic acceptance and is started at once. A binding holding it
            // as waiting never starts it, so the suite accepts it first: only then can it fail.
            if artefact.acceptance.required != AcceptanceMode::Automatic {
                problems.push(
                    "a release that widens nothing didn't take an automatic acceptance".to_owned(),
                );
            }
            if artefact.acceptance.accepted.is_none() {
                self.binding.accept(&moving, "1.2.0", REVIEWER);
            }
            let kept = self.binding.active_version(&moving).as_deref() == Some("1.1.0")
                && self.answers_as(&moving, "1.1.0");
            if kept {
                self.saw(Exercised::FailedUpgradeKeptThePrevious);
            } else {
                problems
                    .push("a failed upgrade didn't leave the previous version running".to_owned());
            }
        }
        undisturbed(self, "by a failed upgrade", &mut problems);

        // A restart.
        let before = self.binding.process(&moving);
        let restarted = self.binding.restart(&moving)
            && self.binding.process(&moving).is_some()
            && self.binding.process(&moving) != before
            && self.answers_as(&moving, "1.1.0");
        if restarted {
            self.saw(Exercised::RestartedProviderAnswered);
        } else {
            problems.push("a restarted provider didn't answer from a new process".to_owned());
        }
        undisturbed(self, "by a restart", &mut problems);

        if problems.is_empty() {
            self.saw(Exercised::OtherProviderUndisturbed);
        }
        for problem in problems {
            self.find(Check::ProvidersAreIsolated, problem);
        }
    }

    /// No other provider's canary reached any endpoint in this language, whichever probe sent it.
    fn no_canary_anywhere(&mut self) {
        let endpoints = self.every_endpoint();
        for (canary, check) in [
            (self.canary("secret"), Check::OtherProvidersSecrets),
            (self.canary("state"), Check::OtherProvidersState),
        ] {
            let reached = endpoints.iter().any(|e| {
                self.binding
                    .received(e)
                    .iter()
                    .any(|body| body.contains(&canary))
            });
            let already = self
                .report
                .findings
                .iter()
                .any(|f| f.check == check && f.language == self.language);
            if reached && !already {
                self.find(check, "another provider's canary reached an endpoint");
            }
        }
    }
}

fn lookup(part: PartName, name: &str) -> Step {
    Step::LookUpSecret {
        part,
        operation: OPERATION.to_owned(),
        name: name.to_owned(),
    }
}

fn read(part: PartName, key: &str) -> Step {
    Step::ReadState {
        part,
        key: key.to_owned(),
    }
}

fn call(endpoint: Endpoint) -> Step {
    Step::Call {
        operation: OPERATION.to_owned(),
        endpoint,
    }
}

fn call_module(callee: &str) -> Step {
    Step::CallModule {
        callee: callee.to_owned(),
        kind: OperationKind::Withdrawal,
    }
}
