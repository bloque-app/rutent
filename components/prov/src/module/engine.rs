//! The one engine configuration the admitting compiler and every provider's process share. Machine code made by
//! one is loaded by the other, so they must agree setting for setting; Wasmtime refuses machine code made under
//! another configuration.

use wasmtime::{Config, Engine, OptLevel, Strategy, WasmFeatures};

/// The portable binary format this binding admits, as a release assertion names it (item `exp2-format-names`).
pub const FORMAT: &str = "wasm-component";

/// The runtime and its pinned version (item `exp2-wasm-runtime`).
pub const WASMTIME_VERSION: &str = "48.0.8";

/// The binding's own version, which names, with the runtime and the target, what made a release's machine code
/// (`build.platform_version`). It changed with `T-013-02`'s engine settings, which change the machine code.
pub const BINDING_VERSION: &str = "0.3.0";

/// The compilation target, set explicitly so that the machine code doesn't depend on the compiling machine's CPU
/// (ADR-0014, `G-03`). It is the build's own triple; no CPU feature is enabled beyond the triple's baseline.
#[cfg(all(target_arch = "aarch64", target_os = "macos"))]
pub const TARGET: &str = "aarch64-apple-darwin";
/// The compilation target, set explicitly (see the macOS one).
#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
pub const TARGET: &str = "x86_64-unknown-linux-gnu";
/// The compilation target, set explicitly (see the macOS one).
#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
pub const TARGET: &str = "aarch64-unknown-linux-gnu";
/// The compilation target, set explicitly (see the macOS one).
#[cfg(all(target_arch = "x86_64", target_os = "macos"))]
pub const TARGET: &str = "x86_64-apple-darwin";

/// Fuel each part gets per operation: the CPU limit (`REQ-MDL-11`, `B-03`). The sandboxed part's call gets it,
/// and every call into the outside part made for that operation's outside requests draws on one budget of it
/// together, not a fresh one each. Instantiating and starting a part get it again. Fuel is deterministic
/// (`G-06`).
pub const FUEL_PER_PART: u64 = 200_000_000;

/// The most linear memory one part's instance may hold, in bytes (`REQ-MDL-11`, `B-04`).
pub const MEMORY_PER_PART: usize = 16 * 1024 * 1024;

/// The address space the engine reserves for one linear memory, in bytes: what the part may hold, no more
/// (`G-05`). Wasmtime's default is 4 GiB for each memory on a 64-bit machine, which would put a provider's process
/// at several GiB of address space for two parts and leave no bound on it that a part's own limit doesn't already
/// make. With the reservation this small, the engine checks each access against the memory's size and leaves no
/// guard region either (the part is slower; it is also bounded), so the process's address space is its parts' memory
/// and the engine's own, which `RLIMIT_AS` then bounds (`sandbox::PROVIDER_ADDRESS_SPACE`).
pub const MEMORY_RESERVATION: u64 = MEMORY_PER_PART as u64;

/// The most elements any one table of a part may hold. The stand-ins' largest table has 51.
pub const TABLE_ELEMENTS: usize = 1024;

/// The most linear memories a part's store may hold: each stand-in's component defines one, in its main module.
pub const MEMORIES_PER_PART: usize = 1;

/// The largest portable binary admission takes, per part, in bytes (`B-09`).
pub const MAX_BINARY: usize = 8 * 1024 * 1024;

/// The deepest wasm stack a call may use, in bytes.
pub const MAX_WASM_STACK: usize = 512 * 1024;

/// The exact platform version that makes the machine code: the binding, the runtime and the target
/// (`build.platform_version`, `REQ-MDL-2`).
pub fn platform_version() -> String {
    format!("rutent-wasm-binding-{BINDING_VERSION}.wasmtime-{WASMTIME_VERSION}.{TARGET}")
}

/// Every WebAssembly feature the engine accepts, and nothing else: the ones all sixteen stand-ins fail to compile
/// without, found by turning each off in turn. Every other proposal is off, among them function references, GC,
/// exceptions, extended constants, tail calls, SIMD, threads, multiple memories, memory64, multi-value, mutable
/// globals across modules and saturating conversions. `REFERENCE_TYPES` carries `CALL_INDIRECT_OVERLONG`, the
/// encoding both toolchains use for `call_indirect`.
pub const FEATURES: WasmFeatures = WasmFeatures::FLOATS
    .union(WasmFeatures::SIGN_EXTENSION)
    .union(WasmFeatures::BULK_MEMORY)
    .union(WasmFeatures::REFERENCE_TYPES)
    .union(WasmFeatures::COMPONENT_MODEL);

/// The configuration, every setting the binding makes stated here and in [`settings`].
pub fn config() -> wasmtime::Result<Config> {
    let mut config = Config::new();
    config.target(TARGET)?;
    config
        .strategy(Strategy::Cranelift)
        .cranelift_opt_level(OptLevel::Speed)
        .cranelift_nan_canonicalization(true)
        .consume_fuel(true)
        .epoch_interruption(false)
        // Every feature off, then exactly [`FEATURES`] on.
        .wasm_features(WasmFeatures::all(), false)
        .wasm_features(FEATURES, true)
        .relaxed_simd_deterministic(true)
        .debug_info(false)
        .generate_address_map(false)
        .max_wasm_stack(MAX_WASM_STACK)
        .memory_reservation(MEMORY_RESERVATION)
        .memory_reservation_for_growth(0)
        .memory_guard_size(0);
    Ok(config)
}

/// The names of the features in [`FEATURES`], as wasmparser names them.
pub fn feature_names() -> Vec<String> {
    FEATURES
        .iter_names()
        .map(|(name, _)| name.to_owned())
        .collect()
}

/// The engine both the compiler and the provider processes use.
pub fn engine() -> wasmtime::Result<Engine> {
    Engine::new(&config()?)
}

/// Every setting [`config`] makes, for the run note.
pub fn settings() -> Vec<(&'static str, String)> {
    vec![
        ("target", TARGET.to_owned()),
        ("strategy", "Cranelift".to_owned()),
        ("cranelift_opt_level", "Speed".to_owned()),
        ("cranelift_nan_canonicalization", "true".to_owned()),
        (
            "consume_fuel",
            format!(
                "true, {FUEL_PER_PART} per part per operation, the outside part's calls for one operation drawing \
                 on one budget; instantiation and start metered alike"
            ),
        ),
        ("epoch_interruption", "false".to_owned()),
        (
            "wasm features (all off, then exactly these on)",
            feature_names().join(", "),
        ),
        ("relaxed_simd_deterministic", "true".to_owned()),
        ("debug_info", "false".to_owned()),
        ("generate_address_map", "false".to_owned()),
        ("max_wasm_stack", format!("{MAX_WASM_STACK} bytes")),
        (
            "memory_reservation / memory_reservation_for_growth / memory_guard_size",
            format!(
                "{MEMORY_RESERVATION} bytes / 0 / 0: no address space beyond the memory a part may hold"
            ),
        ),
        (
            "store limits (StoreLimits)",
            format!(
                "{MEMORY_PER_PART} bytes of linear memory per part, {TABLE_ELEMENTS} elements per table, 16 \
                 instances, 4 tables, {MEMORIES_PER_PART} memory"
            ),
        ),
        (
            "cargo features",
            "cranelift, runtime, component-model, std, anyhow; default features off (no cache, \
             parallel-compilation, gc, threads, async, profiling, pooling-allocator, wat, coredump)"
                .to_owned(),
        ),
    ]
}
