//! Admission (`REQ-MDL-1`, `REQ-MDL-2`, `REQ-MDL-15`, `REQ-MDL-16`): what the admitting compiler takes and refuses,
//! where it runs, and what the admission artefact records (`B-02`, `B-08` to `B-10`, `B-12`, `B-15`, `G-04`).

#[path = "module_support/mod.rs"]
mod common;

use common::{alive, answers_as, assertion, binding, install, languages, spec};
use rutent_conformance::boundary::{AdmissionRefusal, Admitted, ModuleBinding};
use rutent_contracts::module_interface::{Parts, validate_artefact};
use rutent_prov::module::children::compile;
use rutent_prov::module::engine;
use rutent_prov::module::wire::sha256;

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
            let first = compile(
                &common::paths(),
                &dir,
                &sandboxed,
                &outside,
                &Default::default(),
            )
            .expect("it compiles");
            let second = compile(
                &common::paths(),
                &dir,
                &sandboxed,
                &outside,
                &Default::default(),
            )
            .expect("it compiles");
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

/// A component whose sandboxed world imports the binding's two interfaces and `import` besides, built by the guests'
/// tool with `wit-component`'s dummy module: its functions do nothing, which is enough for admission to judge its
/// imports. `import` names the file: `control` (nothing more), `clock`, `file`, `socket` or `undeclared`.
fn hostile(import: &str) -> Vec<u8> {
    std::fs::read(common::standins().join(format!("hostile-{import}.wasm")))
        .expect("the hostile components, which module-guests/build.sh writes")
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
        sandboxed: hostile("control"),
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
        ("a clock", "clock"),
        ("a file", "file"),
        ("a socket", "socket"),
        ("an undeclared host function", "undeclared"),
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
    limits: rutent_prov::module::sandbox::Limits,
    readable: &std::path::Path,
    socket: &std::path::Path,
) -> serde_json::Value {
    // The process it must not signal: the platform's own, which in a PID namespace of its own it can't name.
    let platform = std::process::id().to_string();
    let mut command = rutent_prov::module::sandbox::isolated(
        executable,
        &[
            "--probe",
            readable.to_str().expect("a path"),
            socket.to_str().expect("a path"),
            &platform,
        ],
        dir,
        limits,
    )
    .expect("an isolated command");
    let output = command
        .stdin(std::process::Stdio::null())
        .output()
        .expect("the probe ran");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "{}: no probe report ({e}), {}; its standard error: {}",
            executable.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// Opens `path` as a descriptor that would survive `exec`: close-on-exec cleared, as a shell leaves one.
fn inheritable(path: &std::path::Path) -> std::fs::File {
    use std::os::fd::AsRawFd;
    unsafe extern "C" {
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    }
    const F_SETFD: i32 = 2;
    let file = std::fs::File::open(path).expect("the file opens");
    // SAFETY: F_SETFD with 0 clears close-on-exec on a descriptor this test owns.
    assert_eq!(
        unsafe { fcntl(file.as_raw_fd(), F_SETFD, 0) },
        0,
        "close-on-exec was not cleared"
    );
    file
}

// verifies: REQ-MDL-18, REQ-MDL-13
#[test]
fn the_compiler_and_every_provider_process_reach_no_network_no_home_file_no_other_process_and_inherit_nothing()
 {
    let dir = workdir("probe");
    let paths = common::paths();
    // A file in the user's home directory, which no child may read, held open by the platform without
    // close-on-exec, which no child may inherit.
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .expect("a home directory");
    let readable = std::fs::read_dir(&home)
        .expect("the home directory")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.is_file() && std::fs::read(p).is_ok())
        .expect("a readable file in the home directory");
    let held = inheritable(&readable);
    // A Unix-domain socket the platform listens on, which no child may connect to.
    let socket = dir.join("probe.sock");
    let _ = std::fs::remove_file(&socket);
    let listener = std::os::unix::net::UnixListener::bind(&socket).expect("a Unix socket");
    let children = [
        (
            &paths.compiler,
            rutent_prov::module::sandbox::Limits::COMPILER,
        ),
        (
            &paths.provider,
            rutent_prov::module::sandbox::Limits::PROVIDER,
        ),
    ];
    for (executable, limits) in children {
        let own = dir.join(executable.file_name().expect("a name"));
        std::fs::create_dir_all(&own).expect("a work directory");
        let report = probe(executable, &own, limits, &readable, &socket);
        let name = executable.display();
        eprintln!("{name}: {report}");
        for (field, what) in [
            (
                "network_denied",
                "a TCP connection to the loopback address was made",
            ),
            (
                "unix_socket_denied",
                "a Unix-domain socket was connected to",
            ),
            ("file_write_denied", "a file could be written"),
            ("process_denied", "a process could be started"),
            ("signal_denied", "another process could be signalled"),
            ("file_read_denied", "a file in the home directory was read"),
            (
                "process_list_denied",
                "another process was listed (KERN_PROC_ALL on macOS, /proc on Linux)",
            ),
            (
                "process_args_denied",
                "another process's arguments were read (KERN_PROCARGS2 on macOS, /proc on Linux)",
            ),
        ] {
            assert_eq!(report[field], true, "{name}: {what}");
        }
        if cfg!(target_os = "macos") {
            assert_eq!(
                report["process_info_denied"], true,
                "{name}: another process's information was read"
            );
            assert_eq!(
                report["iokit_denied"], true,
                "{name}: the serial number was read through IOKit"
            );
        }
        assert_eq!(
            report["open_descriptors"],
            serde_json::json!([]),
            "{name}: it inherited file descriptors"
        );
    }
    drop((held, listener));
    let _ = std::fs::remove_dir_all(dir);
}
