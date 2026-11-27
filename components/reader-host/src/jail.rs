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
//! - **Held by the host** ([`check_confined`]): on Linux, before the host takes a reader's start-up checks, it reads
//!   the reader's process status and holds it to the confinement: `no_new_privs` set, a `seccomp` filter installed and
//!   no capability held, bounding set included. A reader that didn't confine itself never answers.

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
    /// The status of the process whose parent is `parent`: on Linux, `unshare --fork` starts the reader as its one
    /// child.
    fn child_status(parent: u32) -> Option<String> {
        let wanted = format!("PPid:\t{parent}");
        std::fs::read_dir("/proc")
            .ok()?
            .flatten()
            .find_map(|entry| {
                let name = entry.file_name();
                let name = name.to_str()?;
                if !name.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                let status = std::fs::read_to_string(entry.path().join("status")).ok()?;
                status.lines().any(|l| l == wanted).then_some(status)
            })
    }

    pub(super) fn check(launched: u32) -> Option<&'static str> {
        let Some(status) = child_status(launched) else {
            return Some("reader_process_not_found");
        };
        let field = |name: &str| {
            status
                .lines()
                .find_map(|l| l.strip_prefix(name).map(str::trim).map(str::to_owned))
        };
        let none = "0000000000000000";
        let held = field("NoNewPrivs:").as_deref() == Some("1")
            && field("Seccomp:").as_deref() == Some("2")
            && ["CapInh:", "CapPrm:", "CapEff:", "CapBnd:"]
                .iter()
                .all(|cap| field(cap).as_deref() == Some(none))
            && field("CapAmb:").as_deref().is_none_or(|v| v == none);
        (!held).then_some("reader_not_confined")
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
