//! The engine's compile target (`T-025-01`): the machine code the admitting compiler makes is for the target's
//! baseline, and no AVX feature is on in it. `GHSA-vc8c-j3xm-xj73` (CVE-2026-24116) is a miscompile of `f64.copysign`
//! on x86-64 with AVX; the pin carries its fix, and this holds the other half of the reading in `F-025`'s notes: the
//! engine never turns AVX on in the first place (`THR-85`, `DEC-27`).
//!
//! The test reads the machine code's own header. Wasmtime writes the target triple and every Cranelift CPU flag a
//! piece of machine code was made under into it (it refuses to load code made under flags the host lacks), and has no
//! accessor for them, so the test compiles with the engine [`engine::engine_from`] hands the compiler and the
//! provider processes, and reads what came out.

use rutent_prov::module::engine;
use serde::Deserialize;

/// One Cranelift flag's value as Wasmtime writes it (`FlagValue`).
#[derive(Debug, Deserialize, PartialEq)]
enum Flag {
    Enum(String),
    Num(u8),
    Bool(bool),
}

/// The head of Wasmtime's `Metadata`, as `postcard` has it: the target, the shared flags and the CPU flags. What
/// follows (the tunables, the features) is left unread.
#[derive(Debug, Deserialize)]
struct Header {
    target: String,
    #[allow(dead_code)]
    shared_flags: Vec<(String, Flag)>,
    isa_flags: Vec<(String, Flag)>,
}

/// An empty core module: enough for the compiler to make a machine-code file with the header.
const EMPTY_MODULE: [u8; 8] = [0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];

/// The header of the machine code `engine` makes.
fn header(engine: &wasmtime::Engine) -> Header {
    use object::read::elf::ElfFile64;
    use object::{Object, ObjectSection};

    let elf = engine
        .precompile_module(&EMPTY_MODULE)
        .expect("machine code");
    let file = ElfFile64::<object::Endianness>::parse(&elf[..]).expect("an ELF file");
    let section = file
        .section_by_name(".wasmtime.engine")
        .expect("the engine's section");
    let data = section.data().expect("the section's bytes");
    // A version byte, a length byte, the version string, then the `postcard` metadata.
    let at = 2 + usize::from(data[1]);
    let (header, _) = postcard::take_from_bytes::<Header>(&data[at..]).expect("the metadata");
    // A format change that left these empty would let every flag assertion pass on nothing.
    assert!(
        !header.isa_flags.is_empty() && !header.shared_flags.is_empty(),
        "the header names no CPU flags: its format changed"
    );
    header
}

/// Every flag of `header` that is on, by name: a bool that is true, or a number that isn't zero.
fn on(flags: &[(String, Flag)]) -> Vec<&str> {
    flags
        .iter()
        .filter(|(_, v)| matches!(v, Flag::Bool(true)) || matches!(v, Flag::Num(n) if *n != 0))
        .map(|(k, _)| k.as_str())
        .collect()
}

/// The flags the triple `target` stands for with nothing added, as Cranelift itself has them: the baseline.
fn baseline(target: &str) -> Vec<String> {
    let triple: target_lexicon::Triple = target.parse().expect("a triple");
    let isa = cranelift_codegen::isa::lookup(triple)
        .expect("an ISA for the triple")
        .finish(cranelift_codegen::settings::Flags::new(
            cranelift_codegen::settings::builder(),
        ))
        .expect("the ISA with its own baseline");
    isa.isa_flags()
        .iter()
        .filter(|v| v.as_bool() == Some(true))
        .map(|v| v.name.to_owned())
        .collect()
}

// The engine's real configuration, for the build's own target and for both x86-64 ones, which an arm64 machine makes
// by cross-compiling: no flag with `avx` in its name is on, and nothing is on beyond what the triple's baseline has,
// so the machine code doesn't depend on the CPU that made it (`G-03`, ADR-0014). Switching `config.target(..)` off
// makes Cranelift read the machine's own CPU, and the second assertion catches it on any CPU with an extension the
// baseline lacks.
// verifies: THR-85, DEC-27
#[test]
fn engines_compile_target_enables_no_avx_feature() {
    // The build's own engine is the one the binding uses (`engine_from` refuses any other settings). The x86-64 ones are
    // made with `Engine::new` from the same settings for their target, which `engine_from` would refuse as another
    // target's, and are the ones an AVX flag could be on in.
    let own = (
        engine::TARGET,
        engine::engine().expect("the binding's engine"),
    );
    let x86 = ["x86_64-unknown-linux-gnu", "x86_64-apple-darwin"].map(|t| {
        let config = engine::config_for(t).expect("the configuration");
        (t, wasmtime::Engine::new(&config).expect("an x86-64 engine"))
    });
    for (target, engine) in std::iter::once(own).chain(x86) {
        let made = header(&engine);
        assert_eq!(made.target, target, "the target is the one named");
        let enabled = on(&made.isa_flags);
        let avx: Vec<_> = enabled.iter().filter(|n| n.contains("avx")).collect();
        assert!(avx.is_empty(), "{target}: AVX flags on: {avx:?}");
        let base = baseline(target);
        let beyond: Vec<_> = enabled
            .iter()
            .filter(|n| !base.iter().any(|b| b == **n))
            .collect();
        assert!(
            beyond.is_empty(),
            "{target}: CPU flags on beyond the triple's baseline: {beyond:?}"
        );
    }
}

// Three of the runtime advisories published on 2026-11-16 have ranges that include the pin and need a proposal the
// engine is meant to have off: `GHSA-rc4p-x33f-vpr2` (the GC proposal, whose function subtyping reaches a builtin that
// the build, with no `gc` cargo feature, lacks), `GHSA-43j2-gcp4-m22g` (the GC proposal again) and
// `GHSA-2674-w29h-wqvj` (the threads proposal, with no `threads` feature); `GHSA-xrvr-8678-5wp8` needs component-model
// async. `F-025`'s notes call each not exploitable on that ground, and this holds the ground: none is in the
// features, and a core module declaring a non-final function type, which only the GC proposal allows, is refused.
// verifies: THR-85, DEC-27
#[test]
fn engine_leaves_off_the_proposals_the_runtime_advisories_of_2026_11_16_need() {
    use wasmtime::WasmFeatures;
    for (name, feature) in [
        ("gc", WasmFeatures::GC),
        ("threads", WasmFeatures::THREADS),
        ("component-model-async", WasmFeatures::CM_ASYNC),
    ] {
        assert!(
            !engine::FEATURES.intersects(feature),
            "the engine's features hold {name}"
        );
    }
    let engine = engine::engine().expect("an engine");
    // A type section holding one function type: final (the default form) and then declared with `sub`, non-final.
    let module = |type_section: &[u8]| {
        let mut bytes = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        bytes.extend_from_slice(type_section);
        bytes
    };
    let final_type = module(&[0x01, 0x04, 0x01, 0x60, 0x00, 0x00]);
    let sub_type = module(&[0x01, 0x06, 0x01, 0x50, 0x00, 0x60, 0x00, 0x00]);
    engine
        .precompile_module(&final_type)
        .expect("a module with a final function type is compiled");
    let refused = engine
        .precompile_module(&sub_type)
        .expect_err("a module declaring a non-final function type is refused: GC is off");
    assert_refused_for_gc(&refused);

    // Providers ship components: the same type inside a component's core module is refused the same way, and the
    // same component with a final type is compiled.
    let component = |core: &[u8]| {
        let mut bytes = vec![0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00];
        bytes.push(0x01); // the core module section
        bytes.push(u8::try_from(core.len()).expect("a short module"));
        bytes.extend_from_slice(core);
        bytes
    };
    engine
        .precompile_component(&component(&final_type))
        .expect("a component with a final function type is compiled");
    let refused = engine
        .precompile_component(&component(&sub_type))
        .expect_err("a component whose core module declares a non-final function type is refused");
    assert_refused_for_gc(&refused);
}

/// The refusal is the GC proposal's (a sub type is its form), not any other error: wasmparser's message for a `sub`
/// type with the proposal off.
fn assert_refused_for_gc(error: &wasmtime::Error) {
    let text = format!("{error:#}");
    assert!(
        text.contains("gc proposal must be enabled to use subtypes"),
        "refused for another reason: {text}"
    );
}
