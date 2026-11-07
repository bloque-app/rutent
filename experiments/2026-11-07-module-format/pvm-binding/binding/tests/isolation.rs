//! The worker's and the admitting compiler's isolation (ADR-0003, scoring sheet `B-08`). Each program runs a
//! probe in place of its work, inside the isolation the binding starts it in, and each probe must be refused:
//! another file of the user's, a descriptor on one the platform holds, a signal to another process, another
//! process's information, a TCP connection, a Unix socket's, a file write, and on macOS the IOKit serial number.
//! A control probe shows the program ran inside its isolation at all, and each refusal that the isolation alone
//! makes is shown to be allowed without it, so the test can fail.

use std::net::TcpListener;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::Command;

use rutent_pvm_binding::isolation::{self, Isolation, Role};

fn programs() -> [(Role, PathBuf); 2] {
    [
        (
            Role::Worker,
            PathBuf::from(env!("CARGO_BIN_EXE_rutent-pvm-worker")),
        ),
        (
            Role::Admit,
            PathBuf::from(env!("CARGO_BIN_EXE_rutent-pvm-admit")),
        ),
    ]
}

fn probe(role: Role, exe: &Path, what: &str, argument: &str) -> String {
    let child = isolation::spawn(role, exe, Isolation::Required, ["--probe", what, argument])
        .expect("the program starts in its isolation");
    let output = child.wait_with_output().expect("it finishes");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn refused(role: Role, exe: &Path, what: &str, argument: &str) {
    let line = probe(role, exe, what, argument);
    assert!(
        line.starts_with(&format!("probe {what}: refused")),
        "{role:?}, {what} {argument}: {line:?}"
    );
}

/// What only the Linux confinement (`jail.rs`) gives, each probe telling one mechanism apart: the new PID
/// namespace; for the worker, `no_new_privs` and its resource limits; for the admitting compiler, its empty root
/// and its `seccomp` filter, which alone refuses a Unix socket inside a new network namespace.
fn linux_only(role: Role, exe: &Path) {
    assert_eq!(
        probe(role, exe, "pid", "-"),
        "probe pid: allowed 1",
        "{role:?}: not alone in a new PID namespace"
    );
    // A socket under /run, which the worker hides and the admitting compiler's empty root doesn't have.
    if let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        let path =
            PathBuf::from(dir).join(format!("rpv-{}-{}.sock", role.name(), std::process::id()));
        let _ = std::fs::remove_file(&path);
        if let Ok(listener) = UnixListener::bind(&path) {
            refused(role, exe, "unix", path.to_str().expect("a path"));
            drop(listener);
            let _ = std::fs::remove_file(&path);
        }
    }
    match role {
        Role::Worker => {
            assert_eq!(
                probe(role, exe, "status", "-"),
                "probe status: allowed NoNewPrivs=1 nofile=1024 core=0",
                "the worker's no_new_privs or resource limits"
            );
        }
        Role::Admit => {
            refused(role, exe, "read", "/proc/self/status");
            refused(role, exe, "socket", "-");
        }
    }
}

/// A regular file directly in the user's home directory, if there is one.
fn a_file_at_home() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    std::fs::read_dir(home)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.is_file() && std::fs::read(p).is_ok())
}

// verifies: ADR-0003, REQ-MDL-16, REQ-MDL-18
#[test]
fn the_worker_and_the_admitting_compiler_are_refused_the_users_files_signals_the_network_and_writes()
 {
    if !(cfg!(target_os = "macos") || cfg!(target_os = "linux")) {
        return;
    }
    let listener = TcpListener::bind("127.0.0.1:0").expect("a listener");
    let address = listener.local_addr().expect("its address").to_string();
    let socket = std::env::temp_dir().join(format!("rpv-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&socket);
    let unix = UnixListener::bind(&socket).expect("a Unix listener");
    // A descriptor the platform holds on a file of the user's, not closed on exec, as one it inherited would be.
    let held = std::fs::File::open(
        a_file_at_home()
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")),
    )
    .expect("a file to hold");
    let inherited = held.as_raw_fd();
    // SAFETY: clears the close-on-exec flag of a descriptor this process holds.
    assert_eq!(unsafe { libc::fcntl(inherited, libc::F_SETFD, 0) }, 0);
    for (role, exe) in programs() {
        // Without the binding's isolation the same probes are allowed, so their refusals below are its.
        let plain = |what: &str, argument: &str| {
            let output = Command::new(&exe)
                .args(["--probe", what, argument])
                .output()
                .expect("it runs");
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        };
        assert_eq!(
            plain("readfd", &inherited.to_string()),
            "probe readfd: allowed",
            "{role:?}"
        );
        assert_eq!(
            plain("unix", socket.to_str().expect("a path")),
            "probe unix: allowed",
            "{role:?}"
        );
    }
    for (role, exe) in programs() {
        assert_eq!(
            probe(role, &exe, "control", "-"),
            "probe control: allowed",
            "{role:?}: the program didn't run in its isolation"
        );
        let own = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        refused(role, &exe, "read", own.to_str().expect("a path"));
        if let Some(file) = a_file_at_home() {
            refused(role, &exe, "read", file.to_str().expect("a path"));
        }
        let mut other = Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("another process");
        refused(role, &exe, "signal", &other.id().to_string());
        assert!(
            other.try_wait().expect("its status").is_none(),
            "{role:?}: the other process was signalled"
        );
        refused(role, &exe, "procinfo", &other.id().to_string());
        let _ = other.kill();
        let _ = other.wait();
        refused(role, &exe, "connect", &address);
        refused(role, &exe, "unix", socket.to_str().expect("a path"));
        refused(role, &exe, "readfd", &inherited.to_string());
        if cfg!(target_os = "macos") {
            refused(role, &exe, "iokit", "-");
        }
        // Standard error is a pipe of the child's own, which the binding reads; nothing reaches the platform's.
        let child = isolation::spawn(
            role,
            &exe,
            Isolation::Required,
            ["--probe", "stderr", "marker-on-stderr"],
        )
        .expect("it starts");
        let output = child.wait_with_output().expect("it finishes");
        assert_eq!(
            output.stderr, b"marker-on-stderr",
            "{role:?}: its standard error wasn't its own pipe"
        );
        let written = std::env::temp_dir().join(format!(
            "rutent-pvm-probe-{}-{}",
            role.name(),
            std::process::id()
        ));
        refused(role, &exe, "write", written.to_str().expect("a path"));
        assert!(!written.exists(), "{role:?}: the file was written");
        refused(role, &exe, "write", "/rutent-pvm-probe");
        if cfg!(target_os = "linux") {
            linux_only(role, &exe);
        }
    }
    drop((held, unix));
    let _ = std::fs::remove_file(&socket);
}
