//! What the binding's tests share: the paths of the binding's executables and of the stand-ins `run.sh` built, and
//! modules, releases and operations written the way the module-boundary suite writes them.

#![allow(dead_code)]

use std::path::PathBuf;

use rutent_conformance::boundary::{Admitted, Handled, ModuleBinding, ModuleSpec, Output, Step};
use rutent_conformance::role::{SuiteContext, placeholder_key, placeholder_signature};
use rutent_contracts::module_interface::{
    BinaryFormat, BinaryPart, Declaration, Digest, DigestAlgorithm, DigestValue, Parts,
    RELEASE_ASSERTION_SHAPE, ReleaseAssertion,
};
use rutent_contracts::provider_role::{Operation, Signature, SignatureScheme, Statement};
use rutent_wasm_binding::children::Paths;
use rutent_wasm_binding::platform::WasmBinding;
use rutent_wasm_binding::program::Probe;
use rutent_wasm_binding::wire::sha256;
use serde_json::json;

/// The provider categories, each with a stand-in in each language.
pub const CATEGORIES: [&str; 4] = ["reserve", "exchange", "ramp", "verifier"];

/// The languages.
pub const LANGUAGES: [&str; 2] = ["rs", "c"];

/// The one outside operation the tests' declarations declare.
pub const OPERATION: &str = "api.call";

/// The stand-ins' directory, from `RUTENT_WASM_STANDINS`, which `run.sh` sets after building them.
pub fn standins() -> PathBuf {
    let dir = std::env::var_os("RUTENT_WASM_STANDINS").map(PathBuf::from).expect(
        "RUTENT_WASM_STANDINS names no directory: run the tests through run.sh, which builds the stand-ins",
    );
    assert!(
        dir.join("rs-reserve-sandboxed.wasm").exists(),
        "{} holds no stand-ins: run run.sh",
        dir.display()
    );
    dir
}

/// The binding's paths.
pub fn paths() -> Paths {
    Paths {
        compiler: PathBuf::from(env!("CARGO_BIN_EXE_rutent-wasm-compiler")),
        provider: PathBuf::from(env!("CARGO_BIN_EXE_rutent-wasm-provider")),
        standins: standins(),
    }
}

/// The languages the tests run, from `RUTENT_WASM_LANGUAGES` (comma-separated), by default both.
pub fn languages() -> Vec<String> {
    std::env::var("RUTENT_WASM_LANGUAGES")
        .map(|l| l.split(',').map(str::to_owned).collect())
        .unwrap_or_else(|_| LANGUAGES.iter().map(|l| (*l).to_owned()).collect())
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
            "statement_key": placeholder_key(1),
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

/// The provider's signed release assertion of `binaries` under `declaration`, as the suite writes one.
pub fn assertion(
    declaration: &Declaration,
    binaries: &Parts<Vec<u8>>,
) -> Statement<ReleaseAssertion> {
    let context = SuiteContext::default();
    let part = |bytes: &[u8]| BinaryPart {
        format: BinaryFormat::new("wasm-component").expect("a format name the contract takes"),
        digest: Digest {
            algorithm: DigestAlgorithm::Sha256,
            value: DigestValue::new(sha256(bytes)).expect("a digest"),
        },
    };
    Statement {
        deployment: context.deployment.clone(),
        protocol_version: context.protocol_version.clone(),
        shape: RELEASE_ASSERTION_SHAPE.to_owned(),
        maker: declaration.provider.as_str().to_owned(),
        made_at: "2026-11-07T00:00:00Z".to_owned(),
        signature: Signature {
            scheme: SignatureScheme::Ed25519,
            key: placeholder_key(2),
            value: placeholder_signature(),
        },
        statement: ReleaseAssertion {
            declaration: declaration.clone(),
            parts: Parts {
                sandboxed: part(&binaries.sandboxed),
                outside: part(&binaries.outside),
            },
        },
    }
}

/// Builds `spec` from `category`'s stand-in in `language` with `probes`, admits it and gives it its fresh
/// acceptance if it waits for one. What admission gave.
pub fn install(
    binding: &mut WasmBinding,
    spec: &ModuleSpec,
    language: &str,
    category: &str,
    probes: &[Probe],
) -> Admitted {
    let binaries = binding
        .build_with(spec, language, category, probes)
        .expect("the stand-in builds");
    let admitted = binding.admit(&assertion(&spec.declaration, &binaries), &binaries);
    if matches!(&admitted, Admitted::Artefact(a) if a.acceptance.accepted.is_none()) {
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
pub fn answers_as(binding: &mut WasmBinding, provider: &str, version: &str, n: usize) -> bool {
    let Handled::Answered(result) = binding.handle(provider, &withdrawal(provider, n)) else {
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
