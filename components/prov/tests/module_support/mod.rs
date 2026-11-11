//! What the module runtime's tests share: the stand-ins the guests' `build.sh` makes, the module-boundary suite's
//! binding over the platform (`WasmBinding`, `ModuleBinding` and `BoundaryHarness` for a test world), and modules,
//! releases and operations written the way the suite writes them.
//!
//! The binding is the tests': the platform imports nothing of ours but `contracts/` (`DEC-2`), so the suite's types
//! are mapped to and from the platform's here.

#![allow(dead_code)]

pub mod program;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::net::IpAddr;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use rutent_conformance::boundary as suite;
use rutent_conformance::boundary::{
    BoundaryHarness, Handled, ModuleBinding, ModuleSpec, Output, Step,
};
use rutent_conformance::role::{Received, SuiteContext, placeholder_signature};
use rutent_conformance::signing::{
    Ed25519Key, advertisement, advertising_key, public_key, sign_statement, statements_key,
};
use rutent_contracts::module_interface::{
    AdmissionArtefact, BinaryFormat, BinaryPart, Declaration, Digest, DigestAlgorithm, DigestValue,
    Identifier, Parts, RELEASE_ASSERTION_SHAPE, ReleaseAssertion, RouteReference,
};
use rutent_contracts::provider_role::{
    Advertisement, Operation, Signature, SignatureScheme, Statement,
};
use rutent_prov::catalogue::{
    Bond, Catalogue, Category, Grant, GrantTerms, Instant, ReversalCapacity, tuples_of,
};
use rutent_prov::module::children::Paths;
use rutent_prov::module::platform::{Platform, World};
use rutent_prov::module::types;
use rutent_prov::module::wire::sha256;
use rutent_prov::proxy::NamedCall;
use serde_json::json;

pub use program::Probe;

/// The provider categories, each with a stand-in in each language.
pub const CATEGORIES: [&str; 4] = ["reserve", "exchange", "ramp", "verifier"];

/// The languages.
pub const LANGUAGES: [&str; 2] = ["rs", "c"];

/// The one outside operation the tests' declarations declare.
pub const OPERATION: &str = "api.call";

/// The stand-ins' directory: `RUTENT_WASM_STANDINS` if set, else the one `module-guests/build.sh` writes into the build
/// directory, built the first time a test asks and again when a guest's source is newer.
pub fn standins() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = match std::env::var_os("RUTENT_WASM_STANDINS") {
            Some(dir) => PathBuf::from(dir),
            None => built_standins(),
        };
        assert!(
            dir.join("rs-reserve-sandboxed.wasm").exists(),
            "{} holds no stand-ins: run module-guests/build.sh",
            dir.display()
        );
        dir
    })
    .clone()
}

/// The newest modification time of any file under `dir`.
fn newest(dir: &Path) -> std::time::SystemTime {
    let mut newest = std::time::SystemTime::UNIX_EPOCH;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return newest;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let time = if path.is_dir() {
            newest_of(&path)
        } else {
            entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        };
        newest = newest.max(time);
    }
    newest
}

fn newest_of(dir: &Path) -> std::time::SystemTime {
    newest(dir)
}

/// Builds the stand-ins with `module-guests/build.sh` into the build directory, unless they are built and current.
fn built_standins() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let exe = std::env::current_exe().expect("the test's own path");
    // target/<profile>/deps/<test> -> target
    let target = exe
        .ancestors()
        .nth(3)
        .expect("a build directory")
        .to_path_buf();
    let out = target.join("rutent-wasm-standins");
    let built = out.join("built");
    let sources = [
        newest(&manifest.join("module-guests")),
        newest(&manifest.join("src/module/wit")),
        newest(&manifest.join("../../conformance/src")),
        newest(&manifest.join("../../contracts/src")),
    ]
    .into_iter()
    .max()
    .expect("a time");
    let current = std::fs::metadata(&built)
        .and_then(|m| m.modified())
        .is_ok_and(|built| built >= sources);
    if !current {
        let status = std::process::Command::new(manifest.join("module-guests/build.sh"))
            .arg(&out)
            .env("CARGO_TARGET_DIR", target.join("module-guests"))
            .status()
            .expect("module-guests/build.sh runs");
        assert!(status.success(), "module-guests/build.sh failed: {status}");
    }
    out
}

/// The platform's executables.
pub fn paths() -> Paths {
    Paths {
        compiler: PathBuf::from(env!("CARGO_BIN_EXE_rutent-wasm-compiler")),
        provider: PathBuf::from(env!("CARGO_BIN_EXE_rutent-wasm-provider")),
    }
}

/// The languages the tests run, from `RUTENT_WASM_LANGUAGES` (comma-separated), by default both.
pub fn languages() -> Vec<String> {
    std::env::var("RUTENT_WASM_LANGUAGES")
        .map(|l| l.split(',').map(str::to_owned).collect())
        .unwrap_or_else(|_| LANGUAGES.iter().map(|l| (*l).to_owned()).collect())
}

fn endpoint_from(endpoint: &suite::Endpoint) -> types::Endpoint {
    types::Endpoint {
        scheme: endpoint.scheme,
        host: endpoint.host.clone(),
        port: endpoint.port,
    }
}

/// The world the module-boundary suite seeds: each provider's secrets and state, the endpoints it serves, what
/// they received, and the secret values the proxy delivered.
#[derive(Default)]
pub struct TestWorld {
    secrets: BTreeMap<(String, String), String>,
    delivered: BTreeMap<String, Vec<String>>,
    state: BTreeMap<(String, String), String>,
    endpoints: HashMap<types::Endpoint, (IpAddr, Option<types::Endpoint>)>,
    received: HashMap<types::Endpoint, Vec<String>>,
    /// The whole request each endpoint received: its method, target, headers and body.
    requests: HashMap<types::Endpoint, Vec<ReceivedRequest>>,
    /// The secrets whose store is down: a lookup of one is `store_unavailable`, never `not_found`.
    unavailable: BTreeSet<(String, String)>,
    /// Every state key the platform asked the world for, in order.
    pub state_asked: Vec<String>,
    /// The private half of each provider's statement key, as the store holds it: the base64url text of its seed.
    /// A provider with none here has the key `derive` gives it, or else the suites' standard statements key
    /// ([`statements_key`]).
    statement_keys: BTreeMap<String, String>,
    derive: Option<fn(&str) -> Ed25519Key>,
    /// The world's clock, set by [`TestWorld::set_clock`]: [`WORLD_AT`] until it is.
    clock: Option<Instant>,
    /// The providers whose statement key the store can't give: the key store is down for them.
    keys_down: BTreeSet<String>,
}

/// The test world's clock until a test sets it: 2026-11-07T00:00:00Z.
pub const WORLD_AT: Instant = Instant(1_794_009_600);

/// A request an endpoint received, as the connector would have sent it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedRequest {
    /// The method's name on the wire.
    pub method: String,
    /// The origin-form target.
    pub target: String,
    /// The headers, in order, as name and value.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: String,
}

impl TestWorld {
    /// Takes down the store holding `provider`'s secret `name`: a lookup of it is refused as `store_unavailable`.
    pub fn make_store_unavailable(&mut self, provider: &str, name: &str) {
        self.unavailable
            .insert((provider.to_owned(), name.to_owned()));
    }

    /// The whole requests `endpoint` received, in order.
    pub fn requests_received(&self, endpoint: &suite::Endpoint) -> Vec<ReceivedRequest> {
        self.requests
            .get(&endpoint_from(endpoint))
            .cloned()
            .unwrap_or_default()
    }

    /// Derives each provider's statement key with `derive`, for the providers it holds no key of its own for.
    pub fn derive_statement_keys(&mut self, derive: fn(&str) -> Ed25519Key) {
        self.derive = Some(derive);
    }

    /// Sets the world's clock, which the platform's records take their times from.
    pub fn set_clock(&mut self, at: Instant) {
        self.clock = Some(at);
    }

    /// Takes down the store holding `provider`'s statement key: it is refused as `store_unavailable`.
    pub fn make_key_store_unavailable(&mut self, provider: &str) {
        self.keys_down.insert(provider.to_owned());
    }

    /// Holds `key` as `provider`'s statement key, in place of the standard one.
    pub fn hold_statement_key(&mut self, provider: &str, key: &Ed25519Key) {
        self.statement_keys.insert(
            provider.to_owned(),
            rutent_conformance::signing::base64url(key.as_bytes()),
        );
    }
}

impl World for TestWorld {
    fn now(
        &mut self,
    ) -> Result<rutent_contracts::module_interface::Time, rutent_prov::module::platform::ClockError>
    {
        let at = self.clock.unwrap_or(WORLD_AT);
        at.rfc3339()
            .and_then(rutent_contracts::module_interface::Time::new)
            .ok_or(rutent_prov::module::platform::ClockError::PastYear9999)
    }

    fn secret(&mut self, provider: &str, name: &str) -> Option<String> {
        let value = self
            .secrets
            .get(&(provider.to_owned(), name.to_owned()))
            .cloned()?;
        self.delivered
            .entry(provider.to_owned())
            .or_default()
            .push(value.clone());
        Some(value)
    }

    fn state(&mut self, provider: &str, key: &str) -> Option<String> {
        self.state_asked.push(key.to_owned());
        self.state
            .get(&(provider.to_owned(), key.to_owned()))
            .cloned()
    }

    fn route(&mut self, endpoint: &types::Endpoint) -> Option<(IpAddr, Option<types::Endpoint>)> {
        self.endpoints.get(endpoint).cloned()
    }

    fn deliver(&mut self, endpoint: &types::Endpoint, body: &str) {
        self.received
            .entry(endpoint.clone())
            .or_default()
            .push(body.to_owned());
    }

    fn receive(&mut self, endpoint: &types::Endpoint, request: &rutent_prov::egress::Request) {
        self.requests
            .entry(endpoint.clone())
            .or_default()
            .push(ReceivedRequest {
                method: request.method().as_str().to_owned(),
                target: request.target().to_owned(),
                headers: request
                    .headers()
                    .iter()
                    .map(|h| (h.name().to_owned(), h.value().to_owned()))
                    .collect(),
                body: String::from_utf8_lossy(request.body()).into_owned(),
            });
        self.deliver(endpoint, &String::from_utf8_lossy(request.body()));
    }

    fn lookup(
        &mut self,
        caller: &rutent_prov::proxy::Caller,
        lookup: &rutent_contracts::module_interface::SecretLookup,
        judged: Result<(), rutent_prov::proxy::Refusal>,
    ) -> Result<
        rutent_contracts::module_interface::SecretValue,
        rutent_contracts::module_interface::SecretRefusalReason,
    > {
        use rutent_contracts::module_interface::{SecretRefusalReason, SecretValue};
        judged.map_err(|refusal| refusal.wire_reason())?;
        let key = (
            caller.provider().as_str().to_owned(),
            lookup.name.as_str().to_owned(),
        );
        if self.unavailable.contains(&key) {
            return Err(SecretRefusalReason::StoreUnavailable);
        }
        self.secret(&key.0, &key.1)
            .and_then(SecretValue::new)
            .ok_or(SecretRefusalReason::NotFound)
    }

    fn statement_key(
        &mut self,
        caller: &rutent_prov::proxy::Caller,
        _: &Identifier,
    ) -> Result<
        rutent_contracts::module_interface::SecretValue,
        rutent_contracts::module_interface::SecretRefusalReason,
    > {
        let provider = caller.provider().as_str();
        if self.keys_down.contains(provider) {
            return Err(rutent_contracts::module_interface::SecretRefusalReason::StoreUnavailable);
        }
        let key = self
            .statement_keys
            .get(provider)
            .cloned()
            .unwrap_or_else(|| {
                let derived = self
                    .derive
                    .map_or_else(|| statements_key(provider), |derive| derive(provider));
                rutent_conformance::signing::base64url(derived.as_bytes())
            });
        rutent_contracts::module_interface::SecretValue::new(key)
            .ok_or(rutent_contracts::module_interface::SecretRefusalReason::NotFound)
    }
}

/// The platform as the module-boundary suite drives it, with the stand-ins of one category in the tests'
/// languages. Everything the platform offers beyond the suite's traits is reached through `Deref`.
pub struct WasmBinding {
    platform: Platform<TestWorld>,
    /// Who vouches for releases: the catalogue, holding the advertisement of each provider listed, under a grant.
    catalogue: Catalogue,
    listed: BTreeSet<String>,
    languages: Vec<String>,
    category: String,
    standins: PathBuf,
    limits: Vec<types::LimitEvent>,
    unknown: Vec<types::UnknownOutcome>,
    /// Every release accepted, whose declared calls the suite's route names (`WasmBinding::handle`).
    accepted: Vec<Declaration>,
}

impl Deref for WasmBinding {
    type Target = Platform<TestWorld>;

    fn deref(&self) -> &Self::Target {
        &self.platform
    }
}

impl DerefMut for WasmBinding {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.platform
    }
}

impl WasmBinding {
    /// A binding whose stand-ins are `category`'s, in `languages` (`rs`, `c`).
    pub fn new(paths: Paths, languages: &[&str], category: &str) -> Result<Self, String> {
        let context = SuiteContext::default();
        let mut catalogue = Catalogue::new();
        catalogue.set_context(context.deployment.clone(), &context.protocol_version);
        let mut platform = Platform::new(
            paths,
            TestWorld::default(),
            types::Envelope {
                deployment: context.deployment,
                protocol_version: context.protocol_version,
            },
        )?;
        // The suite expects an accepted upgrade to run at once; the lifecycle's tests stage their own.
        platform.set_staging(None);
        Ok(Self {
            catalogue,
            listed: BTreeSet::new(),
            platform,
            languages: languages.iter().map(|l| (*l).to_owned()).collect(),
            category: category.to_owned(),
            standins: standins(),
            limits: Vec::new(),
            unknown: Vec::new(),
            accepted: Vec::new(),
        })
    }

    /// The limits modules hit since this was last asked, which the suite's events have no place for.
    pub fn take_limits(&mut self) -> Vec<types::LimitEvent> {
        std::mem::take(&mut self.limits)
    }

    /// The operations that ended as an outcome unknown since this was last asked, and what ended each, which the
    /// suite's events have no place for either. Like [`WasmBinding::take_limits`], they are kept when the events
    /// are taken.
    pub fn take_unknown(&mut self) -> Vec<types::UnknownOutcome> {
        std::mem::take(&mut self.unknown)
    }

    /// The directory the stand-ins are read from.
    pub fn standins(&self) -> &Path {
        &self.standins
    }

    /// The template of one stand-in part: `{language}-{category}-{part}.wasm`.
    pub fn template(&self, language: &str, category: &str, part: &str) -> Option<Vec<u8>> {
        std::fs::read(
            self.standins
                .join(format!("{language}-{category}-{part}.wasm")),
        )
        .ok()
    }

    /// Builds the module `spec` describes from `category`'s stand-in in `language`, with the tests' own `probes`
    /// after its steps.
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
        build_standin(spec, language, category, probes)
    }
}

/// Builds the module `spec` describes from `category`'s stand-in in `language`, with the tests' own `probes` after
/// its steps.
pub fn build_standin(
    spec: &ModuleSpec,
    language: &str,
    category: &str,
    probes: &[Probe],
) -> Option<Parts<Vec<u8>>> {
    let template = |part: &str| {
        std::fs::read(standins().join(format!("{language}-{category}-{part}.wasm"))).ok()
    };
    let text = program::encode(spec, probes);
    Some(Parts {
        sandboxed: program::patch(&template("sandboxed")?, &text)?,
        outside: program::patch(&template("outside")?, &text)?,
    })
}

impl WasmBinding {
    /// Takes a provider's signed advertisement into the catalogue, as the operator does: a grant for what it
    /// offers, with the key it is signed by trusted for advertising, and the advertisement joined. False if the
    /// catalogue refuses any step. A provider is listed once.
    pub fn list(&mut self, advertisement: &[u8]) -> bool {
        let Ok(received) = Received::<Advertisement>::from_bytes(advertisement.to_vec()) else {
            return false;
        };
        let statement = received.statement();
        let ad = &statement.statement;
        if !self.listed.insert(ad.provider.clone()) {
            return false;
        }
        let tuples = tuples_of(ad);
        let categories: Vec<Category> = [
            (ad.offers.reserve.is_some(), Category::Reserve),
            (ad.offers.exchange.is_some(), Category::Exchange),
            (ad.offers.ramp.is_some(), Category::Ramp),
            (ad.offers.verifier.is_some(), Category::Verifier),
        ]
        .into_iter()
        .filter_map(|(offered, category)| offered.then_some(category))
        .collect();
        let id = format!("grant-{}", ad.provider);
        let capacity = ReversalCapacity::check(
            Bond {
                asset: "USDC".into(),
                amount: 1_000,
                held_by: "escrow-co".into(),
            },
            500,
            "ops-lead",
            Instant(1_000),
        );
        let granted = capacity.ok().and_then(|reversal_capacity| {
            Grant::new(GrantTerms {
                id: id.clone(),
                provider: ad.provider.clone(),
                party: "Acme Payments Ltd".into(),
                granted_by: "operator-admin".into(),
                granted_at: Instant(1_000),
                expires_at: Instant(5_000_000_000),
                categories,
                tuples,
                reversal_capacity,
            })
            .ok()
        });
        granted.is_some_and(|grant| {
            self.catalogue.grant(grant).is_ok()
                && self
                    .catalogue
                    .trust_advertising_key(&id, statement.signature.key.clone())
                    .is_ok()
                && self.catalogue.join(received.bytes(), SUITE_AT).is_ok()
        })
    }

    /// Lists `provider` with its standard advertisement, if the test hasn't listed it itself: the tests that admit
    /// releases without the suite sign them with the provider's standard keys ([`assertion`]).
    fn list_if_unlisted(&mut self, provider: &str) {
        if !self.listed.contains(provider) {
            self.list(&advertisement(provider, &SuiteContext::default()));
        }
    }
}

/// The clock the suite's admissions are judged at.
const SUITE_AT: Instant = Instant(1_794_009_600);

impl ModuleBinding for WasmBinding {
    fn format(&self) -> String {
        self.platform.format()
    }

    fn languages(&self) -> Vec<String> {
        self.languages.clone()
    }

    fn set_output_limit(&mut self, bytes: usize) {
        self.platform.set_output_limit(bytes);
    }

    fn admit(
        &mut self,
        assertion: &Statement<ReleaseAssertion>,
        binaries: &Parts<Vec<u8>>,
    ) -> suite::Admitted {
        // The assertion as a provider sends it: its bytes.
        self.list_if_unlisted(assertion.statement.declaration.provider.as_str());
        let received = serde_json::to_vec(assertion).expect("the suite's assertion is JSON");
        match self
            .platform
            .admit(&received, binaries, &mut self.catalogue, SUITE_AT)
        {
            types::Admitted::Artefact(artefact) => {
                if artefact.acceptance.accepted.is_some() {
                    self.accepted.push(artefact.declaration().clone());
                }
                suite::Admitted::Artefact(artefact)
            }
            types::Admitted::Refused(refusal) => suite::Admitted::Refused(refusal),
            types::Admitted::Unverified(_) => {
                suite::Admitted::Refused(suite::AdmissionRefusal::AssertionUnverified)
            }
            types::Admitted::ClockOutOfRange => panic!("the suite's clock is in range"),
        }
    }

    fn accept(&mut self, provider: &str, version: &str, by: &str) -> Option<AdmissionArtefact> {
        // The suite accepts by version; the platform by the assertion's digest, which the waiting release names.
        let digest = self
            .platform
            .pending(provider)
            .filter(|a| a.declaration().version.as_str() == version)?
            .assertion_digest
            .clone();
        let accepted = self
            .platform
            .accept(provider, &digest, by, &self.catalogue, SUITE_AT);
        if let Some(artefact) = &accepted {
            self.accepted.push(artefact.declaration().clone());
        }
        accepted
    }

    fn active_version(&self, provider: &str) -> Option<String> {
        self.platform.active_version(provider)
    }

    fn process(&self, provider: &str) -> Option<suite::ProcessId> {
        self.platform.process(provider)
    }

    fn restart(&mut self, provider: &str) -> bool {
        self.platform.restart(provider)
    }

    /// The suite has no routes, so its route, `conformance-route` and `segment-1`, names every call an accepted
    /// release declares, in the operation's transaction, until the operation ends. The platform still judges each
    /// call against the caller's running declaration first (`T-014-04`). Tests of the route itself hand operations
    /// to the platform directly.
    fn handle(&mut self, provider: &str, operation: &Statement<Operation>) -> Handled {
        let transaction = serde_json::to_value(&operation.statement)
            .ok()
            .and_then(|v| v["transaction"].as_str().and_then(Identifier::new));
        if let Some(transaction) = &transaction {
            let route = RouteReference {
                decision_record: Identifier::new("conformance-route").expect("an id"),
                segment: Identifier::new("segment-1").expect("an id"),
            };
            for declaration in &self.accepted {
                for call in &declaration.outside.calls {
                    self.platform.routes.name(
                        transaction,
                        NamedCall {
                            route: route.clone(),
                            caller: declaration.provider.clone(),
                            callee: call.callee.clone(),
                            kinds: call.kinds.clone(),
                        },
                    );
                }
            }
        }
        let handled = self.platform.handle(provider, operation);
        if let Some(transaction) = &transaction {
            self.platform.routes.end(transaction);
        }
        match handled {
            types::Handled::Answered(result) => Handled::Answered(result),
            types::Handled::Refused(refusal) => Handled::Refused(refusal),
        }
    }

    fn take_events(&mut self) -> suite::Events {
        let events = self.platform.take_events();
        self.limits.extend(events.limits);
        self.unknown.extend(events.unknown);
        suite::Events {
            records: events.records,
            lookups: events
                .lookups
                .into_iter()
                .map(|l| suite::LookupEvent {
                    provider: l.provider,
                    part: l.part,
                    lookup: l.lookup,
                    outcome: l.outcome.map(|o| match o {
                        types::LookupOutcome::Resolved => suite::LookupOutcome::Resolved,
                        types::LookupOutcome::Refused(reason) => {
                            suite::LookupOutcome::Refused(reason)
                        }
                    }),
                })
                .collect(),
            state_reads: events
                .state_reads
                .into_iter()
                .map(|s| suite::StateEvent {
                    provider: s.provider,
                    part: s.part,
                    key: s.key,
                    found: s.found,
                })
                .collect(),
            calls: events.calls,
        }
    }
}

impl BoundaryHarness for WasmBinding {
    fn list_provider(&mut self, advertisement: &[u8]) -> bool {
        self.list(advertisement)
    }

    fn build(&mut self, spec: &ModuleSpec, language: &str) -> Option<Parts<Vec<u8>>> {
        let category = self.category.clone();
        self.build_with(spec, language, &category, &[])
    }

    fn delivered_secrets(&self, provider: &str) -> Vec<String> {
        self.platform
            .world
            .delivered
            .get(provider)
            .cloned()
            .unwrap_or_default()
    }

    fn seed_secret(&mut self, provider: &str, name: &str, value: &str) {
        self.platform
            .world
            .secrets
            .insert((provider.to_owned(), name.to_owned()), value.to_owned());
    }

    fn seed_state(&mut self, provider: &str, key: &str, value: &str) {
        self.platform
            .world
            .state
            .insert((provider.to_owned(), key.to_owned()), value.to_owned());
    }

    fn serve(
        &mut self,
        endpoint: &suite::Endpoint,
        address: IpAddr,
        redirect: Option<&suite::Endpoint>,
    ) {
        self.platform.world.endpoints.insert(
            endpoint_from(endpoint),
            (address, redirect.map(endpoint_from)),
        );
    }

    fn received(&self, endpoint: &suite::Endpoint) -> Vec<String> {
        self.platform
            .world
            .received
            .get(&endpoint_from(endpoint))
            .cloned()
            .unwrap_or_default()
    }

    fn operations_received(&self, provider: &str) -> usize {
        self.platform.operations_received(provider)
    }
}

/// A binding over `category`'s stand-ins, in the tests' languages, with the suite's output limit.
pub fn binding(category: &str) -> WasmBinding {
    let languages = languages();
    let languages: Vec<&str> = languages.iter().map(String::as_str).collect();
    let mut binding = WasmBinding::new(paths(), &languages, category).expect("a binding");
    binding.set_output_limit(4096);
    binding
}
/// A declaration of `provider` at `version` handling `handles`, with one outside operation, `api.call`, reaching
/// `api.{provider}.example` with the secret `own.key`.
pub fn declaration(provider: &str, version: &str, handles: &[&str]) -> Declaration {
    serde_json::from_value(json!({
        "provider": provider,
        "version": version,
        "interface": rutent_contracts::module_interface::VERSION,
        "sandboxed": { "handles": handles },
        "outside": {
            "operations": [{ "name": OPERATION, "destinations": ["api"], "secrets": ["own.key"] }],
            "destinations": [{
                "name": "api",
                "scheme": "https",
                "host": format!("api.{provider}.example"),
                "port": 443,
                "addresses": "public",
            }],
            "secrets": ["own.key"],
            "calls": [],
            "statement_key": public_key(&statements_key(provider)),
        },
    }))
    .expect("a valid declaration")
}

/// A module of `provider` at `version` that handles withdrawals, takes `steps` and returns its honest output.
pub fn spec(provider: &str, version: &str, steps: Vec<Step>, starts: bool) -> ModuleSpec {
    ModuleSpec {
        declaration: declaration(provider, version, &["withdrawal"]),
        steps,
        output: Output::Honest,
        starts,
    }
}

/// The provider's release assertion of `binaries` under `declaration`, signed with its standard advertising key, as
/// the suite writes one.
pub fn assertion(
    declaration: &Declaration,
    binaries: &Parts<Vec<u8>>,
) -> Statement<ReleaseAssertion> {
    assertion_signed_by(
        declaration,
        binaries,
        &advertising_key(declaration.provider.as_str()),
    )
}

/// [`assertion`], signed with `key`.
pub fn assertion_signed_by(
    declaration: &Declaration,
    binaries: &Parts<Vec<u8>>,
    key: &Ed25519Key,
) -> Statement<ReleaseAssertion> {
    let context = SuiteContext::default();
    let part = |bytes: &[u8]| BinaryPart {
        format: BinaryFormat::new("wasm-component").expect("a format name the contract takes"),
        digest: Digest {
            algorithm: DigestAlgorithm::Sha256,
            value: DigestValue::new(sha256(bytes)).expect("a digest"),
        },
    };
    let unsigned = Statement {
        deployment: context.deployment.clone(),
        protocol_version: context.protocol_version.clone(),
        shape: RELEASE_ASSERTION_SHAPE.to_owned(),
        maker: declaration.provider.as_str().to_owned(),
        made_at: "2026-11-07T00:00:00Z".to_owned(),
        signature: Signature {
            scheme: SignatureScheme::Ed25519,
            key: public_key(key),
            value: placeholder_signature(),
        },
        statement: ReleaseAssertion {
            declaration: declaration.clone(),
            parts: Parts {
                sandboxed: part(&binaries.sandboxed),
                outside: part(&binaries.outside),
            },
        },
    };
    sign_statement(&unsigned, key)
}

/// Builds `spec` from `category`'s stand-in in `language` with `probes`, admits it and gives it its fresh
/// acceptance if it waits for one. What admission gave.
pub fn install(
    binding: &mut WasmBinding,
    spec: &ModuleSpec,
    language: &str,
    category: &str,
    probes: &[Probe],
) -> suite::Admitted {
    let binaries = binding
        .build_with(spec, language, category, probes)
        .expect("the stand-in builds");
    let admitted = binding.admit(&assertion(&spec.declaration, &binaries), &binaries);
    if matches!(&admitted, suite::Admitted::Artefact(a) if a.acceptance.accepted.is_none()) {
        binding.accept(
            spec.declaration.provider.as_str(),
            spec.declaration.version.as_str(),
            "test-reviewer",
        );
    }
    admitted
}

/// A withdrawal asked of `provider`, numbered `n`.
pub fn withdrawal(provider: &str, n: usize) -> Statement<Operation> {
    let context = SuiteContext::default();
    serde_json::from_value(json!({
        "deployment": context.deployment,
        "protocol_version": context.protocol_version,
        "shape": rutent_contracts::provider_role::OPERATION_SHAPE,
        "maker": context.operator,
        "made_at": "2026-11-07T00:00:00Z",
        "signature": {
            "scheme": "ed25519",
            "key": context.deployment.operator_key,
            "value": placeholder_signature(),
        },
        "statement": {
            "kind": "withdrawal",
            "transaction": format!("tx-{provider}-{n}"),
            "provider": provider,
            "origin": { "network": "conformance-network" },
            "destination": "conformance-ledger",
            "asset": "COP",
            "amount": "1000",
            "dry_run": false,
            "inputs": {},
        },
    }))
    .expect("a valid operation")
}

/// Whether the provider's module answers a withdrawal as `version`, by its evidence reference.
pub fn answers_as(
    platform: &mut Platform<TestWorld>,
    provider: &str,
    version: &str,
    n: usize,
) -> bool {
    let types::Handled::Answered(result) = platform.handle(provider, &withdrawal(provider, n))
    else {
        return false;
    };
    let expected = format!("{provider}-{version}");
    serde_json::to_value(&result.statement).is_ok_and(|v| {
        v["outcome"]["evidence"][0]["reference"]
            .as_str()
            .is_some_and(|r| r == expected || r.starts_with(&format!("{expected}-")))
    })
}

/// Whether an operating-system process with this id exists.
pub fn alive(pid: u64) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    // SAFETY: signal 0 only checks that the process exists.
    unsafe { kill(pid as i32, 0) == 0 }
}
