//! The PolkaVM engine as the binding sets it, every setting explicit, and the checks admission makes of a part.
//!
//! On `x86_64` Linux the binding uses PolkaVM's compiler and its Linux sandbox, where each guest runs in a
//! sandbox worker process. Everywhere else PolkaVM has neither (`polkavm` 0.38.0, `src/lib.rs:11-19`), and the
//! binding uses the interpreter, which runs the guest inside the provider's worker process. The `generic`
//! sandbox is never used: it needs the `generic-sandbox` feature, which is experimental (scoring sheet `G-15`).

use polkavm::{
    BackendKind, Config, Engine, GasMeteringKind, Module, ModuleConfig, ProgramBlob, SandboxKind,
};
use rutent_contracts::module_interface::PartName;

use crate::sha256::hex_digest;

/// The binding's version, named in each artefact's `build.platform_version`.
pub const BINDING_VERSION: &str = "0.1.0";
/// The PolkaVM version the binding pins.
pub const POLKAVM_VERSION: &str = "0.38.0";
/// The gas each call into a guest starts with: `start`, `handle` or `serve`.
pub const GAS_PER_CALL: i64 = 50_000_000;

/// Whether this build of the binding has PolkaVM's compiler and Linux sandbox.
pub const COMPILER: bool = cfg!(all(target_arch = "x86_64", target_os = "linux"));

/// The host functions a sandboxed part may import: the operation in, the contract's outside request, the result
/// out. Nothing else (`REQ-MDL-6`, `REQ-MDL-16`, `REQ-MDL-18`).
pub const SANDBOXED_IMPORTS: [&str; 3] = ["rt_input", "rt_outside", "rt_output"];
/// The exports a sandboxed part must have.
pub const SANDBOXED_EXPORTS: [&str; 2] = ["start", "handle"];
/// The host functions an outside part may import: the request in, the response out, and the binding's proxy,
/// state, egress and cross-module calls.
pub const OUTSIDE_IMPORTS: [&str; 6] = [
    "rt_request",
    "rt_respond",
    "rt_secret",
    "rt_state",
    "rt_egress",
    "rt_call",
];
/// The exports an outside part must have.
pub const OUTSIDE_EXPORTS: [&str; 2] = ["start", "serve"];

/// The backend this build uses.
pub fn backend() -> BackendKind {
    if COMPILER {
        BackendKind::Compiler
    } else {
        BackendKind::Interpreter
    }
}

/// The binding's engine configuration. Nothing is read from the environment (`Config::from_env` is never
/// called), so no `POLKAVM_*` variable can change a setting (scoring sheet `G-30`).
pub fn config(worker_count: usize) -> Config {
    let mut config = Config::new();
    config.set_backend(Some(backend()));
    config.set_sandbox(COMPILER.then_some(SandboxKind::Linux));
    config.set_crosscheck(false);
    config.set_allow_experimental(false);
    config.set_allow_dynamic_paging(false);
    config.set_sandboxing_enabled(true);
    config.set_cache_enabled(false);
    config.set_worker_count(worker_count);
    config
}

/// The binding's module configuration: synchronous gas metering, no dynamic paging, strict parsing.
pub fn module_config() -> ModuleConfig {
    let mut config = ModuleConfig::new();
    config.set_gas_metering(Some(GasMeteringKind::Sync));
    config.set_dynamic_paging(false);
    config.set_strict(true);
    config.set_cache_by_hash(false);
    config.set_step_tracing(false);
    config
}

/// Every engine setting, for the run note.
pub fn settings() -> Vec<(&'static str, String)> {
    vec![
        ("backend", format!("{:?}", backend())),
        (
            "sandbox",
            if COMPILER {
                "Linux".to_owned()
            } else {
                "none: the interpreter runs in the worker process".to_owned()
            },
        ),
        ("crosscheck", "false".to_owned()),
        ("allow_experimental", "false".to_owned()),
        ("allow_dynamic_paging", "false".to_owned()),
        ("sandboxing_enabled", "true".to_owned()),
        ("cache_enabled", "false".to_owned()),
        ("gas_metering", "Sync".to_owned()),
        ("gas_per_call", GAS_PER_CALL.to_string()),
        ("dynamic_paging", "false".to_owned()),
        ("strict", "true".to_owned()),
        ("cache_by_hash", "false".to_owned()),
        ("step_tracing", "false".to_owned()),
        ("polkavm features", "std (default) only".to_owned()),
    ]
}

/// The platform version an artefact names: the binding, PolkaVM, and the backend that made what runs.
pub fn platform_version() -> String {
    let backend = if COMPILER {
        "compiler-linux-sandbox"
    } else {
        "interpreter"
    };
    format!("rutent-pvm-binding-{BINDING_VERSION}+polkavm-{POLKAVM_VERSION}+{backend}")
}

/// What admission or a worker made of one part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compiled {
    /// Under the compiler, the SHA-256 of the machine code. Under the interpreter, which makes none, the SHA-256
    /// of the domain-separated blob the interpreter runs: never machine code (scoring sheet `G-18`).
    pub digest: String,
    /// Whether `digest` is of machine code.
    pub machine_code: bool,
    /// The machine code's length, if any.
    pub machine_code_len: Option<usize>,
}

/// The digest that stands for a part's code under the interpreter.
pub fn interpreted_digest(blob: &[u8]) -> String {
    let mut bytes = b"rutent-pvm-interpreted-blob\0".to_vec();
    bytes.extend_from_slice(blob);
    hex_digest(&bytes)
}

/// Parses a part's blob and checks its imports and exports against the part's allow-lists.
pub fn check(part: PartName, bytes: &[u8]) -> Result<ProgramBlob, String> {
    let blob = ProgramBlob::parse(bytes.to_vec().into()).map_err(|e| format!("unreadable: {e}"))?;
    if !blob.is_64_bit() {
        return Err("not a 64-bit program".to_owned());
    }
    let (imports, exports): (&[&str], &[&str]) = match part {
        PartName::Sandboxed => (&SANDBOXED_IMPORTS, &SANDBOXED_EXPORTS),
        PartName::Outside => (&OUTSIDE_IMPORTS, &OUTSIDE_EXPORTS),
    };
    for import in blob.imports().iter() {
        let Some(import) = import else {
            return Err("an unnamed import".to_owned());
        };
        let name = String::from_utf8_lossy(import.as_bytes()).into_owned();
        if !imports.contains(&name.as_str()) {
            return Err(format!("imports {name:?}, which the {part:?} part may not"));
        }
    }
    let found: Vec<String> = blob
        .exports()
        .map(|e| String::from_utf8_lossy(e.symbol().as_bytes()).into_owned())
        .collect();
    for export in exports {
        if !found.iter().any(|f| f == export) {
            return Err(format!("doesn't export {export:?}"));
        }
    }
    Ok(blob)
}

/// Compiles a checked blob with the binding's settings; the module and what it made.
pub fn compile(
    engine: &Engine,
    raw: &[u8],
    blob: ProgramBlob,
) -> Result<(Module, Compiled), String> {
    let module = Module::from_blob(engine, &module_config(), blob).map_err(|e| e.to_string())?;
    let compiled = match module.machine_code() {
        Some(code) => Compiled {
            digest: hex_digest(code),
            machine_code: true,
            machine_code_len: Some(code.len()),
        },
        None => Compiled {
            digest: interpreted_digest(raw),
            machine_code: false,
            machine_code_len: None,
        },
    };
    Ok((module, compiled))
}
