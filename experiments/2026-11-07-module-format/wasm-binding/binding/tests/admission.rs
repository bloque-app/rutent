//! Admission (`REQ-MDL-1`, `REQ-MDL-2`, `REQ-MDL-15`, `REQ-MDL-16`): what the admitting compiler takes and refuses,
//! where it runs, and what the admission artefact records (`B-02`, `B-08` to `B-10`, `B-12`, `B-15`, `G-04`).

mod common;

use common::{alive, answers_as, assertion, binding, install, languages, spec};
use rutent_conformance::boundary::{AdmissionRefusal, Admitted, ModuleBinding};
use rutent_contracts::module_interface::{Parts, validate_artefact};
use rutent_wasm_binding::children::compile;
use rutent_wasm_binding::engine;
use rutent_wasm_binding::wire::sha256;

fn workdir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rutent-wasm-test-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a work directory");
    dir
}

// verifies: REQ-MDL-2, REQ-MDL-15, DEC-4
#[test]
fn the_admission_artefact_records_the_platform_version_and_each_parts_machine_code() {
    for language in languages() {
        let mut binding = binding("reserve");
        let provider = format!("recorded-{language}");
        let admitted = install(
            &mut binding,
            &spec(&provider, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[],
        );
        let Admitted::Artefact(artefact) = admitted else {
            panic!("the stand-in wasn't admitted");
        };
        assert_eq!(
            validate_artefact(&artefact),
            Ok(()),
            "the artefact is invalid"
        );
        assert_eq!(
            artefact.build.platform_version.as_str(),
            engine::platform_version()
        );
        let machine = binding
            .machine(&provider, "1.0.0")
            .expect("its machine code is kept");
        assert_eq!(
            artefact.build.machine_code.sandboxed.value.as_str(),
            sha256(&machine.sandboxed)
        );
        assert_eq!(
            artefact.build.machine_code.outside.value.as_str(),
            sha256(&machine.outside)
        );
        for part in [
            &artefact.assertion.statement.parts.sandboxed,
            &artefact.assertion.statement.parts.outside,
        ] {
            assert_eq!(part.format.as_str(), "wasm-component");
        }
    }
}

// verifies: REQ-MDL-2
#[test]
fn compiling_the_same_binary_twice_in_two_compiler_processes_gives_the_same_machine_code() {
    let binding = binding("reserve");
    let dir = workdir("determinism");
    for language in languages() {
        for category in common::CATEGORIES {
            let sandboxed = binding
                .template(&language, category, "sandboxed")
                .expect("a stand-in");
            let outside = binding
                .template(&language, category, "outside")
                .expect("a stand-in");
            let first = compile(&common::paths(), &dir, &sandboxed, &outside).expect("it compiles");
            let second =
                compile(&common::paths(), &dir, &sandboxed, &outside).expect("it compiles");
            assert_eq!(
                sha256(&first.sandboxed),
                sha256(&second.sandboxed),
                "{language}-{category} sandboxed"
            );
            assert_eq!(
                sha256(&first.outside),
                sha256(&second.outside),
                "{language}-{category} outside"
            );
        }
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// A component whose sandboxed world imports the binding's two interfaces and `imports` besides, written with
/// wit-component's dummy module: its functions do nothing, which is enough for admission to judge its imports.
fn hostile(imports: &str) -> Vec<u8> {
    use wit_parser_of_wit_component::{ManglingAndAbi, Resolve};
    let wit = format!(
        r#"package rutent:binding@0.1.0;

interface outside-requests {{ request: func(outside-request: string) -> string; }}
interface denied {{
    look-up-secret: func(operation: string, name: string);
    read-state: func(key: string);
}}
interface undeclared {{ anything: func(what: string) -> string; }}

world sandboxed {{
    import outside-requests;
    import denied;
    {imports}
    export start: func() -> result<_, string>;
    export handle: func(operation: string) -> list<u8>;
}}

package wasi:clocks@0.2.0 {{ interface wall-clock {{ now: func() -> u64; }} }}
package wasi:filesystem@0.2.0 {{ interface types {{ read: func(path: string) -> list<u8>; }} }}
package wasi:sockets@0.2.0 {{ interface tcp {{ connect: func(host: string, port: u16) -> bool; }} }}
"#
    );
    let mut resolve = Resolve::default();
    let package = resolve
        .push_str("hostile.wit", &wit)
        .expect("the hostile world parses");
    let world = resolve
        .select_world(&[package], Some("rutent:binding/sandboxed@0.1.0"))
        .expect("a world");
    let mut module = wit_component::dummy_module(&resolve, world, ManglingAndAbi::Standard32);
    wit_component::embed_component_metadata(
        &mut module,
        &resolve,
        world,
        wit_component::StringEncoding::UTF8,
    )
    .expect("metadata");
    wit_component::ComponentEncoder::default()
        .module(&module)
        .and_then(|e| e.validate(true).encode())
        .expect("a component")
}

// verifies: REQ-MDL-16, REQ-MDL-6, REQ-MDL-18
#[test]
fn a_part_importing_anything_beyond_its_parts_interface_is_refused_at_admission() {
    let mut binding = binding("reserve");
    let language = languages().remove(0);
    let base = spec(&format!("importer-{language}"), "1.0.0", Vec::new(), true);
    let honest = binding
        .build_with(&base, &language, "reserve", &[])
        .expect("a stand-in");
    // The control: the same hostile build, importing nothing more, is admitted.
    let control = Parts {
        sandboxed: hostile(""),
        outside: honest.outside.clone(),
    };
    let admitted = binding.admit(&assertion(&base.declaration, &control), &control);
    assert!(
        matches!(admitted, Admitted::Artefact(_)),
        "the control was refused: {admitted:?}"
    );
    let mut cases: Vec<(&str, Parts<Vec<u8>>)> = vec![
        (
            "the outside part's interface, as the sandboxed part",
            Parts {
                sandboxed: honest.outside.clone(),
                outside: honest.outside.clone(),
            },
        ),
        (
            "the sandboxed part's interface, as the outside part",
            Parts {
                sandboxed: honest.sandboxed.clone(),
                outside: honest.sandboxed.clone(),
            },
        ),
    ];
    for (what, import) in [
        ("a clock", "import wasi:clocks/wall-clock@0.2.0;"),
        ("a file", "import wasi:filesystem/types@0.2.0;"),
        ("a socket", "import wasi:sockets/tcp@0.2.0;"),
        ("an undeclared host function", "import undeclared;"),
    ] {
        cases.push((
            what,
            Parts {
                sandboxed: hostile(import),
                outside: honest.outside.clone(),
            },
        ));
    }
    for (what, binaries) in cases {
        let admitted = binding.admit(&assertion(&base.declaration, &binaries), &binaries);
        assert_eq!(
            admitted,
            Admitted::Refused(AdmissionRefusal::UnreadableBinary),
            "a part importing {what} was admitted"
        );
    }
}

// verifies: REQ-MDL-1, REQ-MDL-13
#[test]
fn a_malformed_or_hostile_binary_is_refused_and_the_running_providers_keep_running() {
    for language in languages() {
        let mut binding = binding("reserve");
        let steady = format!("steady-{language}");
        install(
            &mut binding,
            &spec(&steady, "1.0.0", Vec::new(), true),
            &language,
            "reserve",
            &[],
        );
        let steady_pid = binding.process(&steady).expect("it runs");
        let base = spec(&format!("hostile-{language}"), "1.0.0", Vec::new(), true);
        let honest = binding
            .build_with(&base, &language, "reserve", &[])
            .expect("a stand-in");
        let mut noise = Vec::with_capacity(4096);
        let mut x: u32 = 0x2545_f491;
        for _ in 0..4096 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            noise.push(x as u8);
        }
        let mut header_then_noise = b"\0asm\x0d\0\x01\0".to_vec();
        header_then_noise.extend_from_slice(&noise);
        let mut flipped = honest.sandboxed.clone();
        let middle = flipped.len() / 2;
        for byte in &mut flipped[middle..middle + 64] {
            *byte ^= 0xa5;
        }
        let cases: Vec<(&str, Vec<u8>)> = vec![
            ("empty", Vec::new()),
            (
                "truncated",
                honest.sandboxed[..honest.sandboxed.len() / 2].to_vec(),
            ),
            ("noise", noise),
            ("a component header followed by noise", header_then_noise),
            ("64 bytes corrupted in the middle", flipped),
            ("oversized", vec![0u8; engine::MAX_BINARY + 1]),
        ];
        for (what, bytes) in cases {
            let binaries = Parts {
                sandboxed: bytes,
                outside: honest.outside.clone(),
            };
            let admitted = binding.admit(&assertion(&base.declaration, &binaries), &binaries);
            assert_eq!(
                admitted,
                Admitted::Refused(AdmissionRefusal::UnreadableBinary),
                "a binary {what} wasn't refused as unreadable"
            );
            assert_eq!(
                binding.process(&steady),
                Some(steady_pid),
                "admitting it touched a provider's process"
            );
        }
        assert!(
            alive(steady_pid) && answers_as(&mut binding, &steady, "1.0.0", 1),
            "a provider stopped"
        );
    }
}

// verifies: REQ-MDL-15
#[test]
fn machine_code_offered_as_a_portable_binary_is_refused() {
    let mut binding = binding("reserve");
    let language = languages().remove(0);
    let provider = format!("precompiled-{language}");
    install(
        &mut binding,
        &spec(&provider, "1.0.0", Vec::new(), true),
        &language,
        "reserve",
        &[],
    );
    let machine = binding
        .machine(&provider, "1.0.0")
        .expect("its machine code");
    let next = spec(&provider, "1.0.1", Vec::new(), true);
    let binaries = Parts {
        sandboxed: machine.sandboxed.clone(),
        outside: machine.outside.clone(),
    };
    let admitted = binding.admit(&assertion(&next.declaration, &binaries), &binaries);
    assert_eq!(
        admitted,
        Admitted::Refused(AdmissionRefusal::UnreadableBinary),
        "precompiled code was taken"
    );
}

fn probe(
    executable: &std::path::Path,
    dir: &std::path::Path,
    readable: &std::path::Path,
) -> serde_json::Value {
    let mut command = rutent_wasm_sandbox::isolated(
        executable,
        &["--probe", readable.to_str().expect("a path")],
        dir,
        rutent_wasm_sandbox::Limits::default(),
    )
    .expect("an isolated command");
    let output = command
        .stdin(std::process::Stdio::null())
        .output()
        .expect("the probe ran");
    serde_json::from_slice(&output.stdout).expect("the probe's report")
}

// verifies: REQ-MDL-18, REQ-MDL-13
#[test]
fn the_compiler_and_every_provider_process_can_reach_no_network_no_home_file_no_other_process_and_write_nothing()
 {
    let dir = workdir("probe");
    let paths = common::paths();
    // A file in the user's home directory, which no child may read.
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .expect("a home directory");
    let readable = std::fs::read_dir(&home)
        .expect("the home directory")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.is_file() && std::fs::read(p).is_ok())
        .expect("a readable file in the home directory");
    for executable in [&paths.compiler, &paths.provider] {
        let report = probe(executable, &dir, &readable);
        eprintln!("{}: {report}", executable.display());
        assert_eq!(
            report["network_denied"],
            true,
            "{}: the network was reachable",
            executable.display()
        );
        assert_eq!(
            report["file_write_denied"],
            true,
            "{}: a file could be written",
            executable.display()
        );
        assert_eq!(
            report["process_denied"],
            true,
            "{}: a process could be started",
            executable.display()
        );
        // On every platform: on Linux this fails until a file-read layer is built (see the run note).
        assert_eq!(
            report["file_read_denied"],
            true,
            "{}: a file in the home directory was read",
            executable.display()
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}
