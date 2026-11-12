//! The platform with simulated providers admitted to it: each one a pair of Wasm components, listed in the
//! catalogue and admitted like any module (`REQ-MOD-4`, `DEC-23`).

use std::collections::{BTreeMap, BTreeSet};

use rutent_conformance::role::{
    Received, RoleHarness, RoleProvider, SuiteContext, placeholder_signature,
};
use rutent_conformance::signing::{Signing, public_key, real, sign_statement};
use rutent_conformance::sim::{
    BEGIN, BOOK, CHAIN, DEPLOYMENT_KEY, Role, SETTLE, Spec, Standin, chain_host, rail_host,
};
use rutent_contracts::module_interface::{
    AdmissionArtefact, BinaryFormat, BinaryPart, Declaration, Digest, DigestAlgorithm, DigestValue,
    Parts, RELEASE_ASSERTION_SHAPE, ReleaseAssertion,
};
use rutent_contracts::provider_role::{
    Advertisement, KeyPurpose, Operation, ResultBody, Signature, SignatureScheme, Statement,
};
use rutent_evidence::{Finality, NetworkRules, ReorgRule};
use rutent_prov::catalogue::{
    Bond, Catalogue, Category, Grant, GrantTerms, Instant, ReversalCapacity, tuples_of,
};
use rutent_prov::module::platform::Platform;
use rutent_prov::module::types::{Admitted, Envelope, Handled};
use rutent_prov::module::wire::sha256;
use serde_json::json;

use super::disk;
use super::world::SimWorld;

/// Every kind of operation a module may be handed: a simulated provider refuses what is outside its role itself,
/// as the role asks.
pub const KINDS: [&str; 8] = [
    "withdrawal",
    "deposit",
    "transfer",
    "conversion",
    "balance_verification",
    "judgement",
    "reversal",
    "reversal_confirmation",
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
}

impl RoleHarness for InProcess {
    fn force_outcome_unknown(&mut self) -> bool {
        self.0.force_outcome_unknown()
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
}

/// The simulated public networks a provider's tuples are on.
fn networks_of(spec: &Spec) -> Vec<String> {
    match spec.role {
        Role::Reserve => vec![spec.bank()],
        Role::Exchange => vec!["polygon".to_owned()],
        Role::Ramp => vec!["bsc".to_owned()],
        Role::Verifier => Vec::new(),
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
        let mut platform = Platform::new(
            disk::paths(),
            SimWorld::new(),
            Envelope {
                deployment: context.deployment.clone(),
                protocol_version: context.protocol_version.clone(),
            },
        )
        .expect("a platform");
        platform.set_staging(None);
        platform.set_deadline(deadline);
        platform.set_output_limit(1 << 20);
        Self {
            platform,
            catalogue,
            listed: BTreeSet::new(),
            context,
            installed: BTreeMap::new(),
            seeds: BTreeMap::new(),
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
        self.accept(&spec.provider, &artefact);
        let installed = Installed {
            spec: spec.clone(),
            advertisement,
            declaration,
        };
        self.installed.insert(spec.provider, installed.clone());
        installed
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

    /// Hands one operation to `provider`'s module.
    pub fn handle(&mut self, provider: &str, operation: &Statement<Operation>) -> Handled {
        self.platform.handle(provider, operation)
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
                { "name": SETTLE, "destinations": ["rail"], "secrets": [] },
                { "name": CHAIN, "destinations": chains, "secrets": [] },
            ],
            "destinations": destinations,
            "secrets": [],
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
