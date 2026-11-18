//! Modules that probe the boundary (`T-024-07`, `AC-20`, `GUA-5`): the module runtime's stand-ins, in Rust and in C,
//! each a pair of components admitted to the platform Core's segments run on, as any release is, whose program says
//! which reads to attempt. Each is a provider of its own, named for its language, beside the simulated providers; the
//! scenario seeds another provider's secret and state with canaries and has each probe try to read them from
//! each of its two parts.
//!
//! The stand-ins are the module guests' (`components/prov/module-guests/`), built by their own `build.sh` the first
//! time a scenario asks ([`super::disk::standins`]); the program a stand-in runs is written into a slot of its
//! component, in the line format the guests define (`module-guests/rust/src/lib.rs`, `c/standin.c`), as the prov
//! tests' binding writes it. Only the lines this row needs are written.
//!
//! What the sandbox is, on every platform: a WebAssembly component in the platform's engine, in a process of its own
//! per provider, whose host calls the platform serves and judges. What only Linux adds is the operating system's
//! jail around that process (Landlock, a `seccomp` filter, no capabilities): `T-025-02`'s jail probes job shows that,
//! and no scenario here depends on it.

use rutent_conformance::role::{Received, placeholder_signature};
use rutent_conformance::signing::{advertisement, advertising_key, public_key, statements_key};
use rutent_contracts::module_interface::{
    AdmissionArtefact, BinaryFormat, BinaryPart, Declaration, Digest, DigestAlgorithm, DigestValue,
    PartName, Parts, RELEASE_ASSERTION_SHAPE,
};
use rutent_contracts::provider_role::{Operation, Signature, SignatureScheme, Statement};
use rutent_prov::module::types::{Admitted, Events, Handled};
use rutent_prov::module::wire::sha256;
use serde_json::json;

use super::Fleet;
use super::disk;

/// The two languages of the stand-ins.
pub const LANGUAGES: [&str; 2] = ["rs", "c"];

/// The name both providers give a secret: the probe declares it, and so does the victim, which holds it.
pub const SHARED_SECRET: &str = "shared.key";

/// The name both providers give a state entry: only the victim's is seeded.
pub const SHARED_STATE: &str = "shared.state";

/// The one outside operation a probe's declaration names.
const OPERATION: &str = "api.call";

/// The slot's size, in bytes, and its first line: the stand-ins' (`module-guests`).
const SLOT: usize = 8192;
const MAGIC: &[u8] = b"RUTENT-PROGRAM-SLOT-V1\n";

/// One read a probe attempts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// A lookup of the secret `name`, made by `part`, under the probe's own declared operation.
    Secret(PartName, String),
    /// A read of the state entry `key`, made by `part`.
    State(PartName, String),
}

/// A probe module admitted to the platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// Its provider's name.
    pub provider: String,
    /// The language its stand-in is in.
    pub language: String,
}

/// What handling one operation at a probe left: how it ended, and every event the platform recorded.
#[derive(Debug)]
pub struct Probed {
    /// The module's answer, or the platform's refusal.
    pub handled: Handled,
    /// The lookups, state reads, calls and records the operation made.
    pub events: Events,
}

fn part(name: PartName) -> &'static str {
    match name {
        PartName::Sandboxed => "sandboxed",
        PartName::Outside => "outside",
    }
}

/// The program that makes a stand-in of `provider` attempt `reaches`, then answer honestly.
fn program(provider: &str, reaches: &[Reach]) -> String {
    let mut lines = vec![
        format!("provider {provider}"),
        "version 1.0.0".to_owned(),
        "starts 1".to_owned(),
        format!("serves {OPERATION}"),
        "output honest".to_owned(),
    ];
    for reach in reaches {
        lines.push(match reach {
            Reach::Secret(p, name) => format!("secret {} {OPERATION} {name}", part(*p)),
            Reach::State(p, key) => format!("state {} {key}", part(*p)),
        });
    }
    let mut text = lines.join("\n");
    text.push_str("\nend\n");
    text
}

/// `template` with its one empty slot holding `program`.
fn patch(template: &[u8], program: &str) -> Option<Vec<u8>> {
    let empty = |at: usize| {
        at + SLOT <= template.len()
            && template[at + MAGIC.len()..at + SLOT]
                .iter()
                .all(|b| *b == b'#')
    };
    let mut found = template
        .windows(MAGIC.len())
        .enumerate()
        .filter(|(i, w)| *w == MAGIC && empty(*i))
        .map(|(i, _)| i);
    let at = found.next()?;
    let body = program.as_bytes();
    if found.next().is_some() || MAGIC.len() + body.len() > SLOT {
        return None;
    }
    let mut out = template.to_vec();
    out[at + MAGIC.len()..at + MAGIC.len() + body.len()].copy_from_slice(body);
    Some(out)
}

/// A declaration of `provider` at 1.0.0 that handles withdrawals and has one outside operation, [`OPERATION`],
/// reaching its own endpoint with the secret [`SHARED_SECRET`], which it declares and holds none of.
fn declaration(provider: &str) -> Declaration {
    serde_json::from_value(json!({
        "provider": provider,
        "version": "1.0.0",
        "interface": rutent_contracts::module_interface::VERSION,
        "sandboxed": { "handles": ["withdrawal"] },
        "outside": {
            "operations": [{ "name": OPERATION, "destinations": ["api"], "secrets": [SHARED_SECRET] }],
            "destinations": [{
                "name": "api",
                "scheme": "https",
                "host": format!("api.{provider}.example"),
                "port": 443,
                "addresses": "public",
            }],
            "secrets": [SHARED_SECRET],
            "calls": [],
            "statement_key": public_key(&statements_key(provider)),
        },
    }))
    .expect("a valid declaration")
}

/// The probe's release assertion, signed with its standard advertising key.
fn assertion(
    context: &rutent_conformance::role::SuiteContext,
    declaration: &Declaration,
    binaries: &Parts<Vec<u8>>,
) -> Statement<rutent_contracts::module_interface::ReleaseAssertion> {
    let key = advertising_key(declaration.provider.as_str());
    let digest = |bytes: &[u8]| BinaryPart {
        format: BinaryFormat::new("wasm-component").expect("a format name"),
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
            key: public_key(&key),
            value: placeholder_signature(),
        },
        statement: rutent_contracts::module_interface::ReleaseAssertion {
            declaration: declaration.clone(),
            parts: Parts {
                sandboxed: digest(&binaries.sandboxed),
                outside: digest(&binaries.outside),
            },
        },
    };
    rutent_conformance::signing::sign_statement(&unsigned, &key)
}

/// Admits a probe of `language` that attempts `reaches` to `fleet`'s platform: its advertisement listed, its two
/// components built from the stand-ins and asserted in a release signed by its own key, admitted and given the fresh
/// acceptance a first release waits for. Its statements key is held by the world, as a simulated provider's is.
///
/// # Panics
///
/// If the stand-ins can't be built, or the platform refuses the release.
pub fn install(fleet: &mut Fleet, language: &str, reaches: &[Reach]) -> Probe {
    let provider = format!("probe-{language}");
    let context = fleet.context().clone();
    let advertised = Received::from_bytes(advertisement(&provider, &context))
        .expect("a probe's advertisement is a statement");
    assert!(
        fleet.list(&advertised),
        "{provider}: the catalogue wouldn't take the advertisement"
    );
    fleet
        .world()
        .hold_statement_key(&provider, &statements_key(&provider));
    let standins = disk::standins();
    let template = |part: &str| {
        std::fs::read(standins.join(format!("{language}-reserve-{part}.wasm")))
            .unwrap_or_else(|e| panic!("the {language} stand-in's {part} part: {e}"))
    };
    let text = program(&provider, reaches);
    let binaries = Parts {
        sandboxed: patch(&template("sandboxed"), &text).expect("the program fits the slot"),
        outside: patch(&template("outside"), &text).expect("the program fits the slot"),
    };
    let declaration = declaration(&provider);
    let admitted = fleet.admit(&assertion(&context, &declaration, &binaries), &binaries);
    let Admitted::Artefact(artefact) = admitted else {
        panic!("{provider}: admission refused the release: {admitted:?}");
    };
    let artefact: AdmissionArtefact = *artefact;
    assert!(
        artefact.acceptance.accepted.is_none(),
        "{provider}: a first release waits for its acceptance"
    );
    fleet.accept(&provider, &artefact);
    assert_eq!(
        fleet.platform.active_version(&provider).as_deref(),
        Some("1.0.0"),
        "{provider} runs"
    );
    Probe {
        provider,
        language: language.to_owned(),
    }
}

/// A withdrawal of `provider`, asked in the deployment's own context.
fn withdrawal(fleet: &Fleet, provider: &str, n: usize) -> Statement<Operation> {
    let context = fleet.context();
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

/// Hands `probe` one withdrawal, and takes every event the platform recorded for it.
pub fn handle(fleet: &mut Fleet, probe: &Probe, n: usize) -> Probed {
    let operation = withdrawal(fleet, &probe.provider, n);
    let _ = fleet.platform.take_events();
    let handled = fleet.handle(&probe.provider, &operation);
    let events = fleet.platform.take_events();
    Probed { handled, events }
}
