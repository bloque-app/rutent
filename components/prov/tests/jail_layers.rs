//! Each layer of the provider's jail probed alone (`T-025-02`, `DEC-27`, `THR-85`, `THR-79`, `GUA-5`,
//! `REQ-MDL-11`). For each layer SAD section 6's `THR-85` row names, a hostile act that only that layer stops runs in
//! a jail composed with that layer alone (`rutent_prov::module::jail`, test-only), beside a control: the same act in
//! a jail with the layer left out, which must succeed, so that no probe passes for want of the power to do harm. A
//! jail that can't be established makes the probe exit without a result, and the test fails: nothing passes
//! vacuously. The layers are Linux's, so off Linux each test is ignored with its reason; the Linux job of
//! `.github/workflows/jail-probes.yml` runs them, and each prints the run id of the CI run it ran in.

#![allow(clippy::needless_return)]

use rutent_prov::module::jail::{self, Layer, Layers};
use rutent_prov::module::sandbox::{Limits, UNSHARE};

fn only(layers: &[Layer]) -> Layers {
    layers.iter().fold(Layers::none(), |set, l| set.with(*l))
}

#[cfg(target_os = "linux")]
mod rig {
    use std::os::fd::AsRawFd;
    use std::path::PathBuf;
    use std::process::Child;
    use std::time::{Duration, Instant};

    use super::only;
    use rutent_prov::module::jail::{self, Layer, Layers};
    use rutent_prov::module::sandbox::Limits;

    pub fn run_id() -> String {
        std::env::var("GITHUB_RUN_ID").unwrap_or_else(|_| "local".to_owned())
    }

    pub fn note(test: &str, text: &str) {
        eprintln!("jail-probe run {}: {test}: {text}", run_id());
    }

    pub fn workdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rutent-jail-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a work directory");
        dir
    }

    /// Runs `probe` in a jail of exactly `layers`, and returns its result line. Anything but a result, a jail that
    /// couldn't be made included, fails the test.
    pub fn run(layers: Layers, probe: &str, argument: Option<&str>) -> String {
        let names = layers.names();
        let mut args = vec![names.as_str(), probe];
        args.extend(argument);
        let dir = workdir(probe);
        let exe = PathBuf::from(env!("CARGO_BIN_EXE_rutent-jail-probe"));
        let mut command = jail::isolated(&exe, &args, &dir, Limits::PROVIDER, layers)
            .unwrap_or_else(|e| panic!("no jail of [{names}] could be composed: {e}"));
        let child = command
            .spawn()
            .unwrap_or_else(|e| panic!("[{names}] didn't start: {e}"));
        let pid = child.id();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(child.wait_with_output());
        });
        let out = match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(out) => out.expect("the probe's output"),
            Err(_) => {
                // SAFETY: kill takes two integers.
                unsafe { libc::kill(pid as i32, libc::SIGKILL) };
                panic!("[{names}] {probe}: hung, killed at 60 s");
            }
        };
        let _ = std::fs::remove_dir_all(dir);
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        assert!(
            out.status.success(),
            "[{names}] {probe}: no result, so the jail wasn't established or the probe didn't run: {} {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
        let result = stdout
            .strip_prefix(&format!("{probe} "))
            .unwrap_or_else(|| panic!("[{names}] {probe}: unexpected output {stdout:?}"))
            .to_owned();
        note(probe, &format!("[{names}] -> {result}"));
        result
    }

    pub fn field<'a>(result: &'a str, key: &str) -> &'a str {
        result
            .split_whitespace()
            .find_map(|w| w.strip_prefix(&format!("{key}=")))
            .unwrap_or_else(|| panic!("no {key} in {result:?}"))
    }

    pub fn hostname() -> String {
        let mut buf = [0u8; 128];
        // SAFETY: a buffer and its length.
        unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
        String::from_utf8_lossy(&buf[..buf.iter().position(|b| *b == 0).unwrap_or(0)]).into_owned()
    }

    /// A process of this user the jail should not be able to signal.
    pub fn sleeper() -> Child {
        std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .expect("sleep")
    }

    pub fn dead_within(child: &mut Child, wait: Duration) -> bool {
        let until = Instant::now() + wait;
        while Instant::now() < until {
            if child.try_wait().expect("try_wait").is_some() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    /// A descriptor the child would inherit: close-on-exec cleared.
    pub fn inheritable() -> (std::fs::File, i32) {
        let file = std::fs::File::open("/dev/null").expect("/dev/null");
        let fd = file.as_raw_fd();
        // SAFETY: F_SETFD on a descriptor this function owns.
        assert_eq!(unsafe { libc::fcntl(fd, libc::F_SETFD, 0) }, 0);
        (file, fd)
    }

    use Layer::*;

    pub fn host_users() {
        let control = run(Layers::none(), "host-users", None);
        assert_eq!(
            field(&control, "owner"),
            "0",
            "the control can't see the host's root: {control}"
        );
        let jailed = run(only(&[User]), "host-users", None);
        assert_eq!(
            field(&jailed, "owner"),
            "65534",
            "the host's root is visible: {jailed}"
        );
        assert!(
            jailed.contains("switch=refused EINVAL"),
            "a host user could be taken: {jailed}"
        );
    }

    pub fn network() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a listener");
        listener.set_nonblocking(true).expect("nonblocking");
        let port = listener.local_addr().expect("addr").port().to_string();
        let control = run(only(&[User]), "network", Some(&port));
        assert_eq!(
            control, "allowed",
            "the control can't reach the host's loopback"
        );
        assert!(
            listener.accept().is_ok(),
            "the control's connection never arrived"
        );
        let jailed = run(only(&[User, Net]), "network", Some(&port));
        assert!(
            jailed.starts_with("refused"),
            "the host's loopback was reached: {jailed}"
        );
        assert!(
            matches!(listener.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
            "the listener saw a connection from the jail"
        );
    }

    struct Segment(libc::c_int);
    impl Drop for Segment {
        fn drop(&mut self) {
            // SAFETY: removing the segment this guard made.
            unsafe { libc::shmctl(self.0, libc::IPC_RMID, std::ptr::null_mut()) };
        }
    }

    pub fn ipc() {
        // SAFETY: shmget takes three integers.
        let id = unsafe { libc::shmget(libc::IPC_PRIVATE, 4096, libc::IPC_CREAT | 0o600) };
        assert!(id >= 0, "no shared memory segment could be made");
        let _segment = Segment(id);
        let id = id.to_string();
        let control = run(only(&[User]), "ipc", Some(&id));
        assert_eq!(
            control, "allowed",
            "the control can't attach the host's segment"
        );
        let jailed = run(only(&[User, Ipc]), "ipc", Some(&id));
        assert!(
            jailed.starts_with("refused"),
            "the host's segment was attached: {jailed}"
        );
    }

    /// What this shows: a rename made inside the UTS namespace takes effect only there and never reaches the host.
    /// It does not show that the namespace alone refuses anything: the user namespace already refuses the rename
    /// (the control below), so against this jail's unprivileged process the UTS namespace is depth.
    pub fn uts() {
        let before = hostname();
        // The jail holds the power to rename a host it shares nothing of: inside it the change takes effect.
        let jailed = run(only(&[User, Uts]), "uts", None);
        assert!(
            jailed.starts_with("allowed"),
            "the probe had no power to change a hostname: {jailed}"
        );
        assert_eq!(field(&jailed, "hostname"), "rutent-jail-probe");
        assert_eq!(hostname(), before, "the host's hostname changed");
        // With no UTS namespace the user namespace's root can't, which is why the namespace stops nothing more
        // alone against this jail's unprivileged process (F-025's notes).
        let without = run(only(&[User]), "uts", None);
        assert!(without.starts_with("refused"), "{without}");
        assert_eq!(hostname(), before, "the host's hostname changed");
    }

    pub fn pid() {
        let mut target = sleeper();
        let control = run(only(&[User]), "signal", Some(&target.id().to_string()));
        assert_eq!(
            control, "allowed",
            "the control can't signal a host process"
        );
        assert!(
            dead_within(&mut target, Duration::from_secs(5)),
            "the control's signal never arrived"
        );
        let mut target = sleeper();
        let jailed = run(only(&[User, Pid]), "signal", Some(&target.id().to_string()));
        assert!(
            jailed.starts_with("refused ESRCH"),
            "a host process was signalled: {jailed}"
        );
        let gone = dead_within(&mut target, Duration::from_millis(700));
        let _ = target.kill();
        assert!(!gone, "the host process died");
    }

    pub fn mount() {
        // The control has the mount namespace too, without the pivot: the pivot is what empties the root.
        let control = run(only(&[User, Mount]), "root", None);
        assert_eq!(
            field(&control, "passwd"),
            "readable",
            "the control can't read the host's files"
        );
        let jailed = run(only(&[User, Mount, EmptyRoot]), "root", None);
        assert_eq!(
            field(&jailed, "entries"),
            "0",
            "the root isn't empty: {jailed}"
        );
        assert_eq!(
            field(&jailed, "passwd"),
            "absent",
            "a host file is visible: {jailed}"
        );
    }

    pub fn no_new_privs() {
        let control = run(Layers::none(), "privilege", None);
        assert!(
            control.starts_with("allowed"),
            "the control gains no privilege (no passwordless sudo on this machine?), so the hazard isn't shown: {control}"
        );
        let jailed = run(only(&[NoNewPrivs]), "privilege", None);
        assert_eq!(jailed, "refused", "privilege was gained");
    }

    pub fn capabilities() {
        let control = run(only(&[User]), "capability", None);
        assert_eq!(
            control, "allowed",
            "the user namespace's root can't use a capability: {control}"
        );
        let jailed = run(only(&[User, DropCaps]), "capability", None);
        assert_eq!(jailed, "refused EPERM", "a capability was used");
    }

    pub fn landlock() {
        let control = run(only(&[NoNewPrivs]), "filesystem", None);
        assert_eq!(control, "allowed", "the control can't read a host file");
        let jailed = run(only(&[NoNewPrivs, Landlock]), "filesystem", None);
        assert_eq!(
            jailed, "refused EACCES",
            "Landlock stopped nothing (a kernel without it? the layer then protects nothing here): {jailed}"
        );
    }

    pub fn seccomp() {
        let control = run(only(&[NoNewPrivs]), "syscall", None);
        assert_eq!(control, "allowed", "the control can't make the call");
        let jailed = run(only(&[NoNewPrivs, Seccomp]), "syscall", None);
        assert_eq!(
            jailed, "refused EPERM",
            "a call outside the allow-list was made"
        );
    }

    pub fn descriptors() {
        let (_file, fd) = inheritable();
        let present = format!("fds=[{fd}]");
        let control = run(Layers::none(), "descriptors", None);
        assert_eq!(
            control, present,
            "the control didn't inherit the descriptor"
        );
        for (layer, what) in [
            (CloseBeforeExec, "before exec"),
            (CloseInJail, "in the jail"),
        ] {
            let jailed = run(only(&[layer]), "descriptors", None);
            assert_eq!(jailed, "fds=[]", "the descriptor survived the close {what}");
        }
        // And the whole jail, as shipped.
        let exe = PathBuf::from(env!("CARGO_BIN_EXE_rutent-wasm-provider"));
        let dir = workdir("shipped-descriptors");
        let out =
            rutent_prov::module::sandbox::isolated(&exe, &["--probe"], &dir, Limits::PROVIDER)
                .expect("the shipped jail")
                .output()
                .expect("the provider's probe");
        let report: serde_json::Value = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("no report: {e}: {}", String::from_utf8_lossy(&out.stderr)));
        assert_eq!(
            report["open_descriptors"],
            serde_json::json!([]),
            "the shipped jail kept a descriptor"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    pub fn address_space() {
        use rutent_prov::module::sandbox::PROVIDER_ADDRESS_SPACE;
        let past = (PROVIDER_ADDRESS_SPACE + 4096).to_string();
        let control = run(Layers::none(), "address-space", Some(&past));
        assert_eq!(
            field(&control, "past"),
            "allowed",
            "the control can't map past the limit: {control}"
        );
        let jailed = run(only(&[AddressSpace]), "address-space", Some(&past));
        assert_eq!(field(&jailed, "limit"), PROVIDER_ADDRESS_SPACE.to_string());
        assert_eq!(field(&jailed, "small"), "allowed");
        assert_eq!(field(&jailed, "past"), "refused", "{jailed}");
        assert!(
            jailed.contains("past=refused ENOMEM"),
            "refused for another reason: {jailed}"
        );
        // `EXP-2`'s `G-05` as shipped: the provider's own probe, in its whole jail, on this run.
        let exe = PathBuf::from(env!("CARGO_BIN_EXE_rutent-wasm-provider"));
        let dir = workdir("g05");
        let out =
            rutent_prov::module::sandbox::isolated(&exe, &["--probe"], &dir, Limits::PROVIDER)
                .expect("the shipped jail")
                .output()
                .expect("the provider's probe");
        let report: serde_json::Value = serde_json::from_slice(&out.stdout)
            .unwrap_or_else(|e| panic!("no report: {e}: {}", String::from_utf8_lossy(&out.stderr)));
        note("G-05", &format!("provider --probe: {report}"));
        assert_eq!(report["address_space_limit"], PROVIDER_ADDRESS_SPACE);
        assert_eq!(report["address_space_denied"], true);
        let _ = std::fs::remove_dir_all(dir);
    }

    pub fn release_scan() {
        use std::process::Command;
        let release = match std::env::var_os("RUTENT_RELEASE_PROVIDER") {
            Some(path) => PathBuf::from(path),
            None => {
                let target = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("jail-release");
                let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
                let status = Command::new(cargo)
                    .args([
                        "build",
                        "--release",
                        "--locked",
                        "-p",
                        "rutent-prov",
                        "--bin",
                        "rutent-wasm-provider",
                    ])
                    .arg("--manifest-path")
                    .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
                    .arg("--target-dir")
                    .arg(&target)
                    .status()
                    .expect("cargo");
                assert!(status.success(), "the release build failed");
                target.join("release/rutent-wasm-provider")
            }
        };
        let shipped =
            std::fs::read(&release).unwrap_or_else(|e| panic!("{}: {e}", release.display()));
        // The control for the string scan: the probe binary, which uses the builder, carries every layer's name, so a
        // name absent from the provider is absent for want of the code, not of the scan. (A provider built with the
        // feature is no control: it never calls the builder, so its names may fold away.)
        let probe =
            std::fs::read(env!("CARGO_BIN_EXE_rutent-jail-probe")).expect("the probe binary");
        let has = |haystack: &[u8], needle: &str| {
            haystack
                .windows(needle.len())
                .any(|w| w == needle.as_bytes())
        };
        for (_, name) in jail::LAYER_NAMES {
            assert!(
                has(&probe, name),
                "the control binary lacks {name}: the scan would pass for nothing"
            );
            assert!(
                !has(&shipped, name),
                "the release provider carries {name}: a way to switch a layer off"
            );
        }
        assert!(
            has(&shipped, "pivot_root"),
            "the scan read a binary that is not the provider's jail"
        );
        // By construction, not by string: the features Cargo resolves for the release command line. The scan above
        // can't tell a build that carries the feature from one that doesn't if the optimiser folds the names away,
        // so a leak of `jail-layers` is caught here, in the resolved graph.
        let tree = |edges: &str| {
            let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
            let out = Command::new(cargo)
                .args([
                    "tree",
                    "--locked",
                    "-p",
                    "rutent-prov",
                    "-e",
                    edges,
                    "--prefix",
                    "none",
                ])
                .arg("--manifest-path")
                .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
                .output()
                .expect("cargo tree");
            assert!(
                out.status.success(),
                "cargo tree: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        let shipped_graph = tree("normal,build,features");
        let test_graph = tree("normal,build,dev,features");
        assert!(
            test_graph.contains("feature \"jail-layers\""),
            "the control graph (with dev-dependencies) doesn't show the feature: the check would pass for nothing"
        );
        assert!(
            !shipped_graph.contains("jail-layers"),
            "the release command line resolves the jail-layers feature:\n{shipped_graph}"
        );
        // No feature of the package, `default` and its chains included, enables it, so only the package's own
        // dev-dependency can. `cargo metadata` parses the manifest, a multi-line array included.
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let meta = Command::new(cargo)
            .args(["metadata", "--no-deps", "--format-version", "1", "--locked"])
            .arg("--manifest-path")
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
            .output()
            .expect("cargo metadata");
        assert!(meta.status.success(), "cargo metadata failed");
        let meta: serde_json::Value = serde_json::from_slice(&meta.stdout).expect("metadata");
        let package = meta["packages"]
            .as_array()
            .and_then(|ps| ps.iter().find(|p| p["name"] == "rutent-prov"))
            .expect("the package");
        for (feature, enables) in package["features"].as_object().expect("features") {
            let leaks = enables
                .as_array()
                .expect("a list")
                .iter()
                .any(|e| e.as_str().is_some_and(|e| e.contains("jail-layers")));
            assert!(
                !leaks || feature == "jail-layers",
                "feature {feature} enables jail-layers"
            );
        }
        assert!(
            package["features"].get("jail-layers").is_some(),
            "the feature is gone: the checks mean nothing"
        );
        note(
            "release scan",
            &format!("{} carries no layer switch", release.display()),
        );
    }
}

// verifies: THR-85
#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "the jail's layers are Linux's: the Linux job runs this"
)]
fn probe_for_the_hosts_users_is_refused_by_the_user_namespace_alone() {
    #[cfg(target_os = "linux")]
    rig::host_users();
    #[cfg(not(target_os = "linux"))]
    panic!("Linux only: an ignored test run by hand off Linux must not pass");
}

// verifies: THR-85, THR-79
#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "the jail's layers are Linux's: the Linux job runs this"
)]
fn network_probe_is_refused_by_the_network_namespace_alone() {
    #[cfg(target_os = "linux")]
    rig::network();
    #[cfg(not(target_os = "linux"))]
    panic!("Linux only: an ignored test run by hand off Linux must not pass");
}

// verifies: THR-85
#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "the jail's layers are Linux's: the Linux job runs this"
)]
fn shared_memory_probe_is_refused_by_the_ipc_namespace_alone() {
    #[cfg(target_os = "linux")]
    rig::ipc();
    #[cfg(not(target_os = "linux"))]
    panic!("Linux only: an ignored test run by hand off Linux must not pass");
}

// verifies: THR-85
#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "the jail's layers are Linux's: the Linux job runs this"
)]
fn hostname_change_is_refused_by_the_uts_namespace_alone() {
    #[cfg(target_os = "linux")]
    rig::uts();
    #[cfg(not(target_os = "linux"))]
    panic!("Linux only: an ignored test run by hand off Linux must not pass");
}

// verifies: THR-85
#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "the jail's layers are Linux's: the Linux job runs this"
)]
fn signal_to_a_host_process_is_refused_by_the_pid_namespace_alone() {
    #[cfg(target_os = "linux")]
    rig::pid();
    #[cfg(not(target_os = "linux"))]
    panic!("Linux only: an ignored test run by hand off Linux must not pass");
}

// verifies: THR-85
#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "the jail's layers are Linux's: the Linux job runs this"
)]
fn probe_in_the_mount_namespace_sees_only_the_empty_root() {
    #[cfg(target_os = "linux")]
    rig::mount();
    #[cfg(not(target_os = "linux"))]
    panic!("Linux only: an ignored test run by hand off Linux must not pass");
}

// verifies: THR-85
#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "the jail's layers are Linux's: the Linux job runs this"
)]
fn privilege_gain_is_refused_by_no_new_privs_alone() {
    #[cfg(target_os = "linux")]
    rig::no_new_privs();
    #[cfg(not(target_os = "linux"))]
    panic!("Linux only: an ignored test run by hand off Linux must not pass");
}

// verifies: THR-85
#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "the jail's layers are Linux's: the Linux job runs this"
)]
fn capability_use_is_refused_by_the_dropped_capabilities_alone() {
    #[cfg(target_os = "linux")]
    rig::capabilities();
    #[cfg(not(target_os = "linux"))]
    panic!("Linux only: an ignored test run by hand off Linux must not pass");
}

// verifies: THR-85
#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "the jail's layers are Linux's: the Linux job runs this"
)]
fn filesystem_probe_is_refused_by_landlock_alone() {
    #[cfg(target_os = "linux")]
    rig::landlock();
    #[cfg(not(target_os = "linux"))]
    panic!("Linux only: an ignored test run by hand off Linux must not pass");
}

// verifies: THR-85
#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "the jail's layers are Linux's: the Linux job runs this"
)]
fn forbidden_system_call_is_refused_by_seccomp_alone() {
    #[cfg(target_os = "linux")]
    rig::seccomp();
    #[cfg(not(target_os = "linux"))]
    panic!("Linux only: an ignored test run by hand off Linux must not pass");
}

// verifies: THR-85
#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "the jail's layers are Linux's: the Linux job runs this"
)]
fn inherited_descriptor_is_closed_inside_the_jail() {
    #[cfg(target_os = "linux")]
    rig::descriptors();
    #[cfg(not(target_os = "linux"))]
    panic!("Linux only: an ignored test run by hand off Linux must not pass");
}

// verifies: REQ-MDL-11, THR-85
#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "the jail's layers are Linux's: the Linux job runs this"
)]
fn mapping_past_the_address_space_limit_is_refused() {
    #[cfg(target_os = "linux")]
    rig::address_space();
    #[cfg(not(target_os = "linux"))]
    panic!("Linux only: an ignored test run by hand off Linux must not pass");
}

// verifies: GUA-5
#[test]
#[cfg_attr(
    not(target_os = "linux"),
    ignore = "the jail's layers are Linux's: the Linux job runs this"
)]
fn release_build_of_the_provider_binary_has_no_way_to_turn_a_jail_layer_off() {
    #[cfg(target_os = "linux")]
    rig::release_scan();
    #[cfg(not(target_os = "linux"))]
    panic!("Linux only: an ignored test run by hand off Linux must not pass");
}

// verifies: THR-85
#[test]
fn a_layer_that_cannot_stand_alone_is_refused_with_its_reason() {
    assert_eq!(
        jail::unshare_arguments(Layers::ALL),
        UNSHARE.to_vec(),
        "the builder's all isn't the shipped jail's"
    );
    for (layers, reason) in [
        (only(&[Layer::Net]), "user namespace"),
        (only(&[Layer::Pid]), "user namespace"),
        (only(&[Layer::User, Layer::EmptyRoot]), "mount namespace"),
        (only(&[Layer::EmptyRoot]), "mount namespace"),
        (only(&[Layer::DropCaps]), "no capability"),
        (only(&[Layer::Seccomp]), "no_new_privs"),
        (only(&[Layer::Landlock]), "no_new_privs"),
    ] {
        let why = layers
            .check()
            .expect_err("a composition that can't stand alone was accepted");
        assert!(why.contains(reason), "{why}");
        let exe = std::path::PathBuf::from(env!("CARGO_BIN_EXE_rutent-jail-probe"));
        let refused = jail::isolated(&exe, &[], &std::env::temp_dir(), Limits::PROVIDER, layers);
        assert!(
            refused.is_err(),
            "the builder started a jail that can't stand alone"
        );
    }
    assert!(
        only(&[Layer::User, Layer::Mount, Layer::EmptyRoot])
            .check()
            .is_ok()
    );
}
