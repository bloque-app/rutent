//! How the binding starts its other processes, and the operating-system restrictions each runs under.
//!
//! - **The admitting compiler** parses and compiles hostile input, so it runs in a fresh process per admission,
//!   with no network and no file writes: on macOS under `sandbox-exec`, on Linux under `unshare` in new user,
//!   network, IPC and UTS namespaces (ADR-0003, scoring sheet `B-08`).
//! - **A provider's worker** runs on macOS under the same `sandbox-exec` profile. On Linux it runs as a plain
//!   process, and PolkaVM's Linux sandbox runs each guest in its own worker process, in new namespaces, with
//!   `seccomp` and resource limits (scoring sheet `G-20`).
//!
//! If the wrapper a platform needs isn't there, the binding refuses to start the process, unless
//! `RUTENT_PVM_UNISOLATED=1` is set, which the run's report then states.

use std::path::Path;
use std::process::{Child, Command, Stdio};

/// The macOS profile: everything the process needs is its pipes.
const MACOS_PROFILE: &str =
    "(version 1)(allow default)(deny network*)(deny file-write*)(deny process-fork)";

/// Which process is started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The admitting compiler.
    Admit,
    /// A provider's worker.
    Worker,
}

/// Whether the processes run without their wrapper, as `RUTENT_PVM_UNISOLATED=1` asks.
pub fn unisolated() -> bool {
    std::env::var("RUTENT_PVM_UNISOLATED").is_ok_and(|v| v == "1")
}

/// The restriction a role runs under on this platform, for the run note.
pub fn describe(role: Role) -> String {
    if unisolated() {
        return "none: RUTENT_PVM_UNISOLATED=1".to_owned();
    }
    match (cfg!(target_os = "macos"), cfg!(target_os = "linux"), role) {
        (true, _, _) => format!("sandbox-exec -p '{MACOS_PROFILE}'"),
        (_, true, Role::Admit) => "unshare --user --map-root-user --net --ipc --uts".to_owned(),
        (_, true, Role::Worker) => {
            "a plain process; each guest in PolkaVM's Linux sandbox worker".to_owned()
        }
        _ => "unsupported platform".to_owned(),
    }
}

/// Starts `exe` in its role's isolation, with piped standard input and output and an empty environment.
pub fn spawn(role: Role, exe: &Path) -> std::io::Result<Child> {
    let mut command = if unisolated() {
        Command::new(exe)
    } else if cfg!(target_os = "macos") {
        let mut c = Command::new("/usr/bin/sandbox-exec");
        c.arg("-p").arg(MACOS_PROFILE).arg(exe);
        c
    } else if cfg!(target_os = "linux") && role == Role::Admit {
        let mut c = Command::new("unshare");
        c.args(["--user", "--map-root-user", "--net", "--ipc", "--uts"])
            .arg(exe);
        c
    } else if cfg!(target_os = "linux") {
        Command::new(exe)
    } else {
        return Err(std::io::Error::other(
            "no isolation for this platform; set RUTENT_PVM_UNISOLATED=1 to run without",
        ));
    };
    command
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
}
