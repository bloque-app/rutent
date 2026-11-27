//! The jail a reader's process runs in (`T-015-04` part 4, `REQ-MDL-7`): the one `CMP-PROV` runs a provider's process
//! in, through [`rutent_prov::confine`], so a reader has no network route of its own and one inherited channel to the
//! host.
//!
//! - **From outside** ([`command`]): `CMP-PROV`'s launcher, `isolated`, under a provider's limits. On Linux the reader
//!   starts under util-linux's `unshare` in new user, network, IPC, UTS, mount and PID namespaces, as the only process
//!   of its PID namespace, killed if `unshare` is; on macOS, for development, under `sandbox-exec` with the provider's
//!   deny-by-default profile. Its environment is cleared, every descriptor above the standard three is closed, its
//!   working directory is an empty one, and its one channel is the pair of pipes the host hands it as its standard input
//!   and output. Its standard error is discarded.
//! - **From inside** ([`confine_reader`]): before it reads anything, the reader confines itself as a provider's process
//!   does: `no_new_privs`, the empty read-only root, every capability dropped, Landlock with no rule, the `seccomp`
//!   allow-list of a provider's process, and its address space bounded
//!   ([`rutent_prov::confine::PROVIDER_ADDRESS_SPACE`]). A reader program calls it first thing.
//! - **Held by the host** ([`check_confined`]): on Linux, before the host serves a reader anything but its log (a call,
//!   its checks, an answer), it reads the status of every thread of the reader's process and holds each to the
//!   confinement: `no_new_privs` set, a `seccomp` filter installed and no capability held, bounding set included. The
//!   reader may have no child process, and its network namespace must not be the host's. A reader that didn't confine
//!   itself is served nothing and never answers.
//! - **Killed first** (Linux): each reader's `oom_score_adj` is the highest, so when the readers of one task press its
//!   memory, the kernel kills a reader, which the host restarts, and not the host.

use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

use rutent_prov::confine::{Limits, Role, confine, isolated};

use crate::config::ReaderConfig;

/// A command that starts `config`'s reader in its jail, in `workdir`, an empty directory of its own, with its
/// standard input and output each a pipe to the host and its standard error discarded.
pub fn command(config: &ReaderConfig, workdir: &Path) -> io::Result<Command> {
    let args: Vec<&str> = config.args.iter().map(String::as_str).collect();
    let mut command = isolated(&config.program, &args, workdir, Limits::PROVIDER)?;
    command.stderr(Stdio::null());
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: the closure runs in the forked child before `exec`, after the launcher's own, and calls only
        // async-signal-safe system calls on a path computed before the fork.
        unsafe {
            command.pre_exec(linux::kill_me_first);
        }
    }
    Ok(command)
}

/// Confines the calling process as a provider's process is confined, before it reads any input: what a reader
/// program calls first. On Linux this is the in-process half of the jail, and an error if any of it can't be applied;
/// elsewhere the isolation was applied before the program started, and there is nothing to do.
pub fn confine_reader() -> Result<(), String> {
    confine(Role::Provider)
}

/// Holds the reader that `launched` (the process the host started: `unshare` on Linux) to its confinement, from its
/// status as the kernel reports it: `None` if it holds, else why not. Off Linux the jail is applied from outside before
/// the reader starts, so there is nothing to read.
pub fn check_confined(launched: u32) -> Option<&'static str> {
    #[cfg(target_os = "linux")]
    {
        linux::check(launched)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = launched;
        None
    }
}

#[cfg(target_os = "linux")]
mod linux {
    /// The highest `oom_score_adj`: what a process may always set for itself.
    const KILL_ME_FIRST: &[u8] = b"1000";

    /// Raises the calling process's `oom_score_adj` to the highest, between `fork` and `exec`: the reader inherits it.
    pub(super) fn kill_me_first() -> std::io::Result<()> {
        // SAFETY: open, write and close on a NUL-terminated literal path and a byte buffer, each async-signal-safe.
        unsafe {
            let fd = libc::open(
                c"/proc/self/oom_score_adj".as_ptr(),
                libc::O_WRONLY | libc::O_CLOEXEC,
            );
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let written = libc::write(fd, KILL_ME_FIRST.as_ptr().cast(), KILL_ME_FIRST.len());
            libc::close(fd);
            if written != KILL_ME_FIRST.len() as isize {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(())
    }

    /// The process ids whose parent is `parent`.
    fn children(parent: u32) -> Vec<u32> {
        let wanted = format!("PPid:\t{parent}");
        let Ok(entries) = std::fs::read_dir("/proc") else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter_map(|entry| {
                let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
                let status = std::fs::read_to_string(entry.path().join("status")).ok()?;
                status.lines().any(|l| l == wanted).then_some(pid)
            })
            .collect()
    }

    /// Whether one thread's status shows the jail: `no_new_privs`, a `seccomp` filter, and no capability.
    fn holds(status: &str) -> bool {
        let field = |name: &str| {
            status
                .lines()
                .find_map(|l| l.strip_prefix(name).map(str::trim).map(str::to_owned))
        };
        let none = "0000000000000000";
        field("NoNewPrivs:").as_deref() == Some("1")
            && field("Seccomp:").as_deref() == Some("2")
            && ["CapInh:", "CapPrm:", "CapEff:", "CapBnd:"]
                .iter()
                .all(|cap| field(cap).as_deref() == Some(none))
            && field("CapAmb:").as_deref().is_none_or(|v| v == none)
    }

    pub(super) fn check(launched: u32) -> Option<&'static str> {
        // `unshare --fork` starts the reader as its one child.
        let launched_children = children(launched);
        let [reader] = launched_children.as_slice() else {
            return Some("reader_process_not_found");
        };
        let reader = *reader;
        let Ok(tasks) = std::fs::read_dir(format!("/proc/{reader}/task")) else {
            return Some("reader_process_not_found");
        };
        let mut threads = 0;
        for task in tasks.flatten() {
            threads += 1;
            match std::fs::read_to_string(task.path().join("status")) {
                Ok(status) if holds(&status) => {}
                _ => return Some("reader_not_confined"),
            }
        }
        if threads == 0 || !children(reader).is_empty() {
            return Some("reader_not_confined");
        }
        // Its network namespace is its own, where the host can read it: defence beside the launcher's.
        let net = |pid: &str| std::fs::read_link(format!("/proc/{pid}/ns/net")).ok();
        if let (Some(own), Some(theirs)) = (net("self"), net(&reader.to_string()))
            && own == theirs
        {
            return Some("reader_not_confined");
        }
        None
    }
}

/// Makes the host's own process undumpable on Linux, so a process of the same user can't read its memory or its
/// environment through `/proc`, nor trace it: where the host's tokens and certificates are. The binary calls it at
/// start.
pub fn harden_host() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    // SAFETY: prctl takes plain integers.
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
