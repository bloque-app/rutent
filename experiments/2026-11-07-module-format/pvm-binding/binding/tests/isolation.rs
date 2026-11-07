//! The worker's and the admitting compiler's isolation (ADR-0003, scoring sheet `B-08`). Each program runs a
//! probe in place of its work, inside the isolation the binding starts it in, and each probe must be refused:
//! another file of the user's, a signal to another process, a network connection and a file write. A control
//! probe shows the program ran inside its isolation at all.

use std::net::TcpListener;
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
        let _ = other.kill();
        let _ = other.wait();
        refused(role, &exe, "connect", &address);
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
}
