//! The platform with simulated providers admitted to it: each one a pair of Wasm components, listed in the
//! catalogue and admitted like any module (`REQ-MOD-4`, `DEC-23`).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant as StdInstant};

use rutent_conformance::role::{
    BankCredit, Received, RoleHarness, RoleProvider, SuiteContext, placeholder_signature,
};
use rutent_conformance::signing::{Signing, public_key, real, sign_statement};
use rutent_conformance::sim::{
    BEGIN, BOOK, CHAIN, CREDENTIAL, DEPLOYMENT_KEY, SETTLE, Spec, Standin, chain_host, rail_host,
};
use rutent_contracts::module_interface::{
    AdmissionArtefact, BinaryFormat, BinaryPart, Declaration, Digest, DigestAlgorithm, DigestValue,
    Parts, RELEASE_ASSERTION_SHAPE, ReleaseAssertion,
};
use rutent_contracts::provider_role::{
    Advertisement, ArrivalAnswer, ArrivalNotice, ArrivalQuestion, KeyPurpose, Operation,
    ResultBody, Signature, SignatureScheme, Statement,
};
use rutent_evidence::{Finality, NetworkRules, ReorgRule};
use rutent_prov::catalogue::{
    Bond, Catalogue, Category, Grant, GrantTerms, Instant, ReversalCapacity, tuples_of,
};
use rutent_prov::module::platform::{OperationClock, Platform};
use rutent_prov::module::types::{Admitted, Envelope, Handled};
use rutent_prov::module::wire::sha256;
use serde_json::json;

use super::disk;
use super::world::SimWorld;

/// Every kind of operation a module may be handed: a simulated provider refuses what is outside its role itself,
/// as the role asks. A hold request among them (`T-020-56`): only a reserve whose declaration says it answers them holds
/// the quote asked, and Core asks no other.
pub const KINDS: [&str; 10] = [
    "withdrawal",
    "deposit",
    "transfer",
    "conversion",
    "balance_verification",
    "judgement",
    "reversal",
    "reversal_confirmation",
    "status_query",
    "hold",
];

/// The clock the admissions are judged at: 2026-11-07T00:00:00Z.
const ADMITTED_AT: Instant = Instant(1_794_009_600);

/// A stand-in as a provider the role suite can run, in process.
pub struct InProcess(pub Box<dyn Standin>);

impl RoleProvider for InProcess {
    fn advertisement(&self) -> Received<Advertisement> {
        self.0.advertisement()
    }

    fn handle(&mut self, operation: &Statement<Operation>) -> Received<ResultBody> {
        self.0.handle(operation)
    }

    fn state(&self) -> String {
        self.0.state()
    }

    fn status(&mut self, transaction: &str) -> Option<Received<ResultBody>> {
        self.0.status(transaction)
    }

    fn arrival(
        &mut self,
        question: &Statement<ArrivalQuestion>,
    ) -> Option<Received<ArrivalAnswer>> {
        self.0.arrival(question)
    }

    fn arrival_notice(&mut self, reference: &str) -> Option<Received<ArrivalNotice>> {
        self.0.arrival_notice(reference)
    }
}

impl RoleHarness for InProcess {
    fn credit_arrival(&mut self, credit: &BankCredit) -> bool {
        self.0.credit_arrival(credit)
    }

    fn force_outcome_unknown(&mut self) -> bool {
        self.0.force_outcome_unknown()
    }

    fn force_outcome_unknown_then_failure(&mut self) -> bool {
        self.0.force_outcome_unknown_then_failure()
    }

    fn force_failure(&mut self) -> bool {
        self.0.force_failure()
    }

    fn credit_reversal(&mut self, segment: &str, node: &str, asset: &str, amount: &str) -> bool {
        self.0.credit_reversal(segment, node, asset, amount)
    }
}

/// A simulated provider admitted to a platform.
#[derive(Debug, Clone)]
pub struct Installed {
    /// Its identity.
    pub spec: Spec,
    /// Its advertisement, signed for real: the one the catalogue holds.
    pub advertisement: Received<Advertisement>,
    /// Its declaration, the one its release asserts.
    pub declaration: Declaration,
    /// Its release as accepted, which Core records for a verifier (`T-024-12`): a judgement's origin names the
    /// release of its maker.
    pub released: Option<AdmissionArtefact>,
}

/// The operations a plain wallet handles.
pub const WALLET_HANDLES: [&str; 4] = [
    "deposit",
    "withdrawal",
    "balance_verification",
    "holding_balance",
];

/// The plain wallet of a public node, admitted to the platform on the operator's own signed release (`T-024-17`).
#[derive(Debug, Clone)]
pub struct InstalledWallet {
    /// Its provider identifier: the module the operator publishes.
    pub provider: String,
    /// The release's version.
    pub version: String,
    /// The release's declaration: no outside operation, and the statements key its answers are signed with.
    pub declaration: Declaration,
}

impl InstalledWallet {
    /// The statements key the wallet's answers are signed with, which its release declares.
    pub fn statement_key(&self) -> rutent_contracts::provider_role::PublicKey {
        self.declaration.outside.statement_key.clone()
    }
}

/// The simulated public networks a provider's tuples are on (`Spec::networks`).
pub(crate) fn networks_of(spec: &Spec) -> Vec<String> {
    spec.networks()
}

/// The clock every scenario platform's operations run on (`T-013-08`): it stands still, so that whether a quote's or
/// an operation's call is answered never depends on how long a loaded machine took to start the simulated
/// providers. Only a scenario that scripts a silent or late provider, which is about a deadline passing, has its
/// clock follow the operating system's time, from the moment it queues that behaviour and without a jump.
#[derive(Debug)]
struct ScenarioClock {
    wall_time: Arc<AtomicBool>,
    /// The time the clock has reached, and when it began to follow real time, if it has.
    state: Mutex<(StdInstant, Option<StdInstant>)>,
}

impl ScenarioClock {
    fn new(wall_time: Arc<AtomicBool>) -> Self {
        Self {
            wall_time,
            state: Mutex::new((StdInstant::now(), None)),
        }
    }

    fn following(&self) -> bool {
        self.wall_time.load(Ordering::SeqCst)
    }
}

impl OperationClock for ScenarioClock {
    fn now(&self) -> StdInstant {
        let Ok(mut state) = self.state.lock() else {
            return StdInstant::now();
        };
        if state.1.is_none() && self.following() {
            state.1 = Some(StdInstant::now());
        }
        state.0 + state.1.map_or(Duration::ZERO, |since| since.elapsed())
    }

    fn follows_wall_time(&self) -> bool {
        false
    }

    fn wall_due(&self, due: StdInstant) -> StdInstant {
        let wait = if self.following() {
            due.saturating_duration_since(self.now())
        } else {
            Duration::from_secs(60)
        };
        StdInstant::now() + wait
    }
}

/// A platform over a [`SimWorld`], with the catalogue that vouches for its releases.
pub struct Fleet {
    /// The platform. Its world is the simulation's: [`SimWorld`].
    pub platform: Platform<SimWorld>,
    catalogue: Catalogue,
    listed: BTreeSet<String>,
    context: SuiteContext,
    installed: BTreeMap<String, Installed>,
    /// The placeholder-key seeds in use, each with the provider that holds it.
    seeds: BTreeMap<u8, String>,
    /// The transactions whose late answers the platform has taken ([`Fleet::late_answers`]), each once.
    late_taken: BTreeSet<String>,
}

impl Fleet {
    /// A platform with no provider, whose upgrades take effect at once and whose operations have the deadline
    /// `deadline`.
    pub fn new(deadline: std::time::Duration) -> Self {
        Self::with_context(deadline, SuiteContext::default())
    }

    /// [`Fleet::new`], in `context`: the assembly's, whose deployment names the operator's real key, so the log
    /// verifies Core's questions by it (`T-024-02`).
    pub fn with_context(deadline: std::time::Duration, context: SuiteContext) -> Self {
        let mut catalogue = Catalogue::new();
        catalogue.set_context(context.deployment.clone(), &context.protocol_version);
        let wall_time = Arc::new(AtomicBool::new(false));
        let mut world = SimWorld::new();
        world.wall_time = Some(wall_time.clone());
        let mut platform = Platform::new(
            disk::paths(),
            world,
            Envelope {
                deployment: context.deployment.clone(),
                protocol_version: context.protocol_version.clone(),
            },
        )
        .expect("a platform");
        platform.set_staging(None);
        // Starting a simulated provider is the machine's work, which a stepped clock can't move: on a loaded machine
        // it can pass the 10 s a production start is given, and no scenario is about start-up time.
        platform.set_start_deadline(std::time::Duration::from_secs(120));
        platform.set_operation_clock(Arc::new(ScenarioClock::new(wall_time)));
        platform.set_deadline(deadline);
        platform.set_output_limit(1 << 20);
        Self {
            platform,
            catalogue,
            listed: BTreeSet::new(),
            context,
            installed: BTreeMap::new(),
            seeds: BTreeMap::new(),
            late_taken: BTreeSet::new(),
        }
    }

    /// The context every statement is in.
    pub fn context(&self) -> &SuiteContext {
        &self.context
    }

    /// The simulated world.
    pub fn world(&mut self) -> &mut SimWorld {
        &mut self.platform.world
    }

    /// The provider installed under `provider`.
    pub fn provider(&self, provider: &str) -> Option<&Installed> {
        self.installed.get(provider)
    }

    /// Takes an advertisement into the catalogue, as the operator does: a grant for what it offers, its advertising
    /// key trusted for advertising, and the advertisement joined. False if the catalogue refuses any step.
    pub fn list(&mut self, advertisement: &Received<Advertisement>) -> bool {
        let statement = advertisement.statement();
        let ad = &statement.statement;
        if !self.listed.insert(ad.provider.clone()) {
            return false;
        }
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
                party: format!("{} Ltd", ad.publisher),
                granted_by: "operator-admin".into(),
                granted_at: Instant(1_000),
                expires_at: Instant(5_000_000_000),
                categories,
                tuples: tuples_of(ad),
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
                && self
                    .catalogue
                    .join(advertisement.bytes(), ADMITTED_AT)
                    .is_ok()
        })
    }

    /// Admits the simulated provider `spec` names, as any module is: its advertisement listed, its two components
    /// asserted in a release the provider signs, admitted, and given its fresh acceptance. Its rail and the public
    /// networks its tuples are on are served first.
    ///
    /// # Panics
    ///
    /// If the provider's placeholder keys are another installed provider's: two providers can't share a key
    /// (`Spec::seed`). Rename one.
    pub fn install(&mut self, spec: Spec) -> Installed {
        for seed in [spec.seed() % 64, (spec.seed() + 30) % 64] {
            if let Some(other) = self.seeds.insert(seed, spec.provider.clone()) {
                assert_eq!(
                    other, spec.provider,
                    "{} and {other} have the same placeholder keys (seed {seed}): rename one",
                    spec.provider
                );
            }
        }
        let twin = Signing(InProcess(spec.make(&self.context)));
        let placeholders = InProcess(spec.make(&self.context))
            .advertisement()
            .into_statement()
            .statement;
        let advertisement = twin.advertisement();
        let ad = advertisement.statement().statement.clone();
        let key_of = |keys: &[rutent_contracts::provider_role::SigningKey], purpose| {
            keys.iter()
                .find(|k| k.purpose == purpose)
                .expect("a key")
                .public_key
                .clone()
        };
        let statements = key_of(&ad.keys, KeyPurpose::Statements);
        // The world holds the private half of the statements key the declaration names, which the stand-in's
        // placeholder stands for; the platform resolves it to sign the module's results.
        self.platform.world.hold_statement_key(
            &spec.provider,
            &real(key_of(&placeholders.keys, KeyPurpose::Statements).as_str()),
        );
        self.platform.world.serve_rail(&spec.provider);
        self.platform.world.seed_state(
            &spec.provider,
            DEPLOYMENT_KEY,
            &serde_json::to_string(&self.context.deployment).expect("a deployment is JSON"),
        );
        if let Some(opening) = spec.opening {
            self.platform.world.seed_state(
                &spec.provider,
                rutent_conformance::sim::OPENING_KEY,
                &opening.to_string(),
            );
        }
        let networks = networks_of(&spec);
        for network in &networks {
            if self.platform.world.network(network).is_none() {
                let rules = if network == "polygon" || network == "bsc" {
                    NetworkRules {
                        finality: Finality::Confirmations(2),
                        reorg: ReorgRule::DepthBound(2),
                    }
                } else {
                    NetworkRules {
                        finality: Finality::Instant,
                        reorg: ReorgRule::Never,
                    }
                };
                self.platform.world.serve_network(network, rules);
            }
        }
        let declaration = declaration(&spec, "1.0.0", statements.as_str(), &networks);
        let guests = disk::guests();
        let role = format!("{:?}", spec.role).to_lowercase();
        let binaries = Parts {
            sandboxed: std::fs::read(guests.join(format!("sim-{role}-sandboxed.wasm")))
                .expect("the simulated provider's sandboxed part"),
            outside: std::fs::read(guests.join("sim-outside.wasm"))
                .expect("the simulated providers' outside part"),
        };
        assert!(
            self.list(&advertisement),
            "{}: the catalogue wouldn't take the advertisement",
            spec.provider
        );
        let advertising = key_of(&placeholders.keys, KeyPurpose::Advertising);
        let admitted = self.admit(
            &assertion_in(
                &self.context,
                &declaration,
                &binaries,
                &real(advertising.as_str()),
            ),
            &binaries,
        );
        let Admitted::Artefact(artefact) = admitted else {
            panic!(
                "{}: admission refused the release: {admitted:?}",
                spec.provider
            );
        };
        assert!(
            artefact.acceptance.accepted.is_none(),
            "{}: a first release waits for its acceptance",
            spec.provider
        );
        let released = self.accept(&spec.provider, &artefact);
        let installed = Installed {
            spec: spec.clone(),
            advertisement,
            declaration,
            released,
        };
        self.installed.insert(spec.provider, installed.clone());
        installed
    }

    /// Admits the plain wallet `provider` as a module the operator publishes: recorded as such in the platform's
    /// catalogue, its two components asserted in a release the operator signs with its own key, admitted, and given
    /// its first release's fresh acceptance, so it runs. The world holds the private half of its statements key, as
    /// it does a simulated provider's, for the platform to sign its answers.
    ///
    /// # Panics
    ///
    /// If the wallet is built nowhere, or the platform refuses its release.
    pub fn install_wallet(&mut self, provider: &str) -> InstalledWallet {
        let version = "1.0.0".to_owned();
        let statements = rutent_conformance::signing::statements_key(provider);
        let operator = crate::kit::RealKey::assembly().0;
        self.catalogue
            .publish_operator_module(provider)
            .expect("the operator publishes the wallet");
        self.platform
            .world
            .hold_statement_key(provider, &statements);
        let mut value = json!({
            "provider": provider,
            "version": version,
            "interface": rutent_contracts::module_interface::VERSION,
            "sandboxed": { "handles": WALLET_HANDLES },
            "outside": {
                "operations": [],
                "destinations": [],
                "secrets": [],
                "calls": [],
                "statement_key": public_key(&statements),
            },
        });
        value["outside"]["statement_key"] = public_key(&statements).as_str().into();
        let declaration: Declaration =
            serde_json::from_value(value).expect("a valid wallet declaration");
        let guests = disk::wallet_guests();
        let binaries = Parts {
            sandboxed: std::fs::read(guests.join("plain-wallet-sandboxed.wasm"))
                .expect("the wallet's sandboxed part"),
            outside: std::fs::read(guests.join("plain-wallet-outside.wasm"))
                .expect("the wallet's outside part"),
        };
        let mut assertion = assertion_in(&self.context, &declaration, &binaries, &operator);
        // The operator's release is made by the operator, not by the module.
        assertion.maker.clone_from(&self.context.operator);
        let assertion = sign_statement(&assertion, &operator);
        let admitted = self.admit(&assertion, &binaries);
        let Admitted::Artefact(artefact) = admitted else {
            panic!("{provider}: admission refused the wallet's release: {admitted:?}");
        };
        if artefact.acceptance.accepted.is_none() {
            self.accept(provider, &artefact)
                .expect("the first release is accepted");
        }
        assert!(
            self.platform.active_version(provider).is_some(),
            "{provider}: the wallet runs"
        );
        InstalledWallet {
            provider: provider.to_owned(),
            version,
            declaration,
        }
    }

    /// Admits one release.
    pub fn admit(
        &mut self,
        assertion: &Statement<ReleaseAssertion>,
        binaries: &Parts<Vec<u8>>,
    ) -> Admitted {
        let received = serde_json::to_vec(assertion).expect("an assertion is JSON");
        self.platform
            .admit(&received, binaries, &mut self.catalogue, ADMITTED_AT)
    }

    /// Gives a waiting release its fresh acceptance.
    pub fn accept(
        &mut self,
        provider: &str,
        artefact: &AdmissionArtefact,
    ) -> Option<AdmissionArtefact> {
        self.platform.accept(
            provider,
            &artefact.assertion_digest,
            "test-reviewer",
            &self.catalogue,
            ADMITTED_AT,
        )
    }

    /// The answers simulated providers give past their deadline, for the operations named by `transactions`
    /// (`T-024-23`, `AC-5`): each provider whose books hold an operation it answered late, one it took effect on and
    /// never answered within its deadline, answers it now, as the module that served it signs it, once. The platform
    /// ends a call at its deadline and kills the process, so an answer that comes after is the provider's own repeat of
    /// what it did, which the platform takes: provider, transaction and the signed answer.
    pub fn late_answers(
        &mut self,
        transactions: &BTreeSet<String>,
    ) -> Vec<(String, String, Vec<u8>)> {
        let providers: Vec<String> = self.installed.keys().cloned().collect();
        let mut answers = Vec::new();
        for provider in providers {
            let owed: Vec<(String, Statement<Operation>)> = self
                .platform
                .world
                .journal(&provider)
                .iter()
                .filter(|entry| entry.behaviour.late)
                .filter_map(|entry| {
                    let transaction =
                        rutent_conformance::sim::transaction_of(&entry.operation.statement)?;
                    (transactions.contains(&transaction) && !self.late_taken.contains(&transaction))
                        .then(|| (transaction, entry.operation.clone()))
                })
                .collect();
            for (transaction, operation) in owed {
                if let Handled::Answered(result) = self.platform.handle(&provider, &operation)
                    && let Ok(bytes) = serde_json::to_vec(&*result)
                {
                    self.late_taken.insert(transaction.clone());
                    answers.push((provider.clone(), transaction, bytes));
                }
            }
        }
        answers
    }

    /// Hands one operation to `provider`'s module.
    pub fn handle(&mut self, provider: &str, operation: &Statement<Operation>) -> Handled {
        self.platform.handle(provider, operation)
    }

    /// Admits a release of the installed provider `provider`, signed with its advertising key, as `release` says
    /// (`T-024-07`): its parts are the provider's own, as installed, so a release that changes nothing but its
    /// version is the same module; a widening one declares a destination more; a corrupt one carries a sandboxed part
    /// that is no component, which the platform's compiler refuses. A release that needs a fresh acceptance waits for
    /// it ([`Fleet::accept`]); one that doesn't is accepted automatically.
    ///
    /// # Panics
    ///
    /// If `provider` isn't installed, or its guests are missing.
    pub fn release(&mut self, provider: &str, release: &Release) -> Admitted {
        let installed = self
            .installed
            .get(provider)
            .unwrap_or_else(|| panic!("{provider} isn't installed"))
            .clone();
        let placeholders = InProcess(installed.spec.make(&self.context))
            .advertisement()
            .into_statement()
            .statement;
        let advertising = placeholders
            .keys
            .iter()
            .find(|k| k.purpose == KeyPurpose::Advertising)
            .expect("an advertising key")
            .public_key
            .clone();
        let mut declaration =
            serde_json::to_value(&installed.declaration).expect("a declaration is JSON");
        declaration["version"] = json!(release.version);
        if release.widen {
            declaration["outside"]["destinations"]
                .as_array_mut()
                .expect("destinations")
                .push(json!({
                    "name": "audit-sink",
                    "scheme": "https",
                    "host": "audit-sink.example",
                    "port": 443,
                    "addresses": "public",
                }));
        }
        let declaration: Declaration =
            serde_json::from_value(declaration).expect("a valid declaration");
        let guests = disk::guests();
        let role = format!("{:?}", installed.spec.role).to_lowercase();
        let binaries = Parts {
            sandboxed: if release.corrupt {
                b"this is no component".to_vec()
            } else {
                std::fs::read(guests.join(format!("sim-{role}-sandboxed.wasm")))
                    .expect("the simulated provider's sandboxed part")
            },
            outside: std::fs::read(guests.join("sim-outside.wasm"))
                .expect("the simulated providers' outside part"),
        };
        let assertion = assertion_in(
            &self.context,
            &declaration,
            &binaries,
            &real(advertising.as_str()),
        );
        self.admit(&assertion, &binaries)
    }
}

/// A release of an installed simulated provider, for [`Fleet::release`] (`T-024-07`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// The version it is.
    pub version: String,
    /// Whether it declares a destination its provider's last release didn't, a widening of its capabilities that needs
    /// a fresh acceptance (`REQ-MDL-3`).
    pub widen: bool,
    /// Whether its sandboxed part is no component, a release the platform can't start or compile.
    pub corrupt: bool,
}

impl Release {
    /// A release at `version` that changes nothing else.
    pub fn at(version: &str) -> Self {
        Release {
            version: version.to_owned(),
            widen: false,
            corrupt: false,
        }
    }

    /// This release, widened.
    #[must_use]
    pub fn widened(mut self) -> Self {
        self.widen = true;
        self
    }

    /// This release, its sandboxed part corrupt.
    #[must_use]
    pub fn corrupt(mut self) -> Self {
        self.corrupt = true;
        self
    }
}

/// The declaration of a simulated provider's module at `version`: the four outside operations of
/// `rutent_conformance::sim`, the rail and the networks it may call, and the statements key it signs with.
pub fn declaration(
    spec: &Spec,
    version: &str,
    statement_key: &str,
    networks: &[String],
) -> Declaration {
    let mut destinations = vec![json!({
        "name": "rail",
        "scheme": "https",
        "host": rail_host(&spec.provider),
        "port": 443,
        "addresses": "public",
    })];
    let mut chains = Vec::new();
    for network in networks {
        let name = format!("chain-{network}");
        destinations.push(json!({
            "name": name,
            "scheme": "https",
            "host": chain_host(network),
            "port": 443,
            "addresses": "public",
        }));
        chains.push(name);
    }
    serde_json::from_value(json!({
        "provider": spec.provider,
        "version": version,
        "interface": rutent_contracts::module_interface::VERSION,
        "sandboxed": { "handles": KINDS },
        "outside": {
            "operations": [
                { "name": BEGIN, "destinations": [], "secrets": [] },
                { "name": BOOK, "destinations": [], "secrets": [] },
                { "name": SETTLE, "destinations": ["rail"], "secrets": [CREDENTIAL] },
                { "name": CHAIN, "destinations": chains, "secrets": [] },
            ],
            "destinations": destinations,
            "secrets": [CREDENTIAL],
            "calls": [],
            "statement_key": statement_key,
        },
    }))
    .expect("a valid declaration")
}

/// The provider's release assertion of `binaries` under `declaration`, signed with `key`, as the platform admits one.
pub fn assertion_signed_by(
    declaration: &Declaration,
    binaries: &Parts<Vec<u8>>,
    key: &rutent_conformance::signing::Ed25519Key,
) -> Statement<ReleaseAssertion> {
    assertion_in(&SuiteContext::default(), declaration, binaries, key)
}

/// [`assertion_signed_by`], in `context`.
pub fn assertion_in(
    context: &SuiteContext,
    declaration: &Declaration,
    binaries: &Parts<Vec<u8>>,
    key: &rutent_conformance::signing::Ed25519Key,
) -> Statement<ReleaseAssertion> {
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
