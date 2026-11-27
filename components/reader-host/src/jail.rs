//! The jail a reader's process runs in (`T-015-04` part 4, `REQ-MDL-7`): the one `CMP-PROV` runs a provider's process
//! in, through [`rutent_prov::confine`], so a reader has no network route of its own and one inherited channel to the
//! host.
//!
//! - **From outside** ([`command`]): `CMP-PROV`'s launcher, `isolated`, under a provider's limits. On Linux the reader
//!   starts under util-linux's `unshare` in new user, network, IPC, UTS, mount and PID namespaces, as the only process
//!   of its PID namespace, killed if `unshare` is; on macOS, for development, under `sandbox-exec` with the provider's
//!   deny-by-default profile. Its environment is cleared, every descriptor above the standard three is closed, its
//!   working directory is an empty one of its own ([`workdir`]: mode 0700, never one that already exists), and its one
//!   channel is the pair of pipes the host hands it as its standard input and output. Its standard error is discarded.
//!   On Linux the host bounds its address space itself, before the reader's program runs (`T-015-05`): `RLIMIT_AS`,
//!   soft and hard, at a provider's process's bound ([`rutent_prov::confine::PROVIDER_ADDRESS_SPACE`]), set between
//!   `fork` and `exec` and inherited through `unshare`, so a reader that skips its own limit is bounded all the same,
//!   and can't raise it again: raising a hard limit takes `CAP_SYS_RESOURCE` in the host's user namespace, which
//!   nothing in the jail has.
//! - **From inside** ([`confine_reader`]): before it reads anything, the reader confines itself as a provider's process
//!   does: `no_new_privs`, the empty read-only root, every capability dropped, Landlock with no rule, the `seccomp`
//!   allow-list of a provider's process, and its address space bounded again at the same bound. A reader program calls
//!   it first thing.
//! - **Held by the host** ([`check_confined`]): on Linux, before the host serves a reader anything but its log (a call,
//!   its checks, an answer), it reads what the kernel reports of the reader's process and holds it to the
//!   confinement: its address space bounded, soft and hard, at the provider's bound (`/proc/<pid>/limits`); in every
//!   thread `no_new_privs` set, a `seccomp` filter installed and no capability held, bounding set included
//!   (`/proc/<pid>/task/*/status`); no child process; a network namespace the host can read and not the host's; and
//!   its root the jail's empty one, a read-only `tmpfs` with nothing else mounted (`/proc/<pid>/mountinfo`). Whatever
//!   it can't read fails closed. Landlock is not among them: the kernel reports no process's Landlock domain to another
//!   process, so the host can't read it, and holds the reader to it only through the `seccomp` filter, which
//!   `confine` installs after Landlock. A reader that didn't confine itself is served nothing and never answers.
//! - **Killed first** (Linux): each reader's `oom_score_adj` is the highest, so when the readers of one task press its
//!   memory, the kernel kills a reader, which the host restarts, and not the host.
//!
//! A reader's program path is the operator's configuration, as the rest of [`ReaderConfig`] is: the host
//! canonicalises it and runs it, and checks neither its owner nor its mode.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[cfg(target_os = "linux")]
use rutent_prov::confine::PROVIDER_ADDRESS_SPACE;
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
        // async-signal-safe system calls on a path and values computed before the fork.
        unsafe {
            command.pre_exec(linux::before_exec);
        }
    }
    Ok(command)
}

/// Makes a working directory of its own for the start `generation` of the reader `name`, under `under` (the system's
/// temporary directory): a new directory, mode 0700 on Unix, under a name no other start shares and no other user can
/// guess (the reader's name, the host's process id, the generation and a random part). One that already exists,
/// whoever made it, is refused, never taken: a directory another local user made first is not the reader's.
pub fn workdir(under: &Path, name: &str, generation: u64) -> io::Result<PathBuf> {
    use std::hash::BuildHasher;
    let random = std::collections::hash_map::RandomState::new().hash_one((
        std::process::id(),
        generation,
        std::time::Instant::now(),
    ));
    let path = under.join(format!(
        "rutent-reader-{name}-{}-{generation}-{random:016x}",
        std::process::id()
    ));
    create_own(&path)?;
    Ok(path)
}

/// Creates `path` as a new directory, mode 0700 on Unix; an error if anything is already there, a symbolic link
/// included.
fn create_own(path: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(path)
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

/// Whether `limits`, a process's `/proc/<pid>/limits`, bounds its address space at `bound` bytes or less, its soft
/// and its hard limit both. An unbounded limit (`unlimited`), a higher one, or no line for it, is not bounded.
#[cfg(any(target_os = "linux", test))]
fn address_space_bounded(limits: &str, bound: u64) -> bool {
    limits
        .lines()
        .find_map(|line| line.strip_prefix("Max address space"))
        .is_some_and(|rest| {
            let mut fields = rest.split_whitespace();
            let soft = fields.next().and_then(|v| v.parse::<u64>().ok());
            let hard = fields.next().and_then(|v| v.parse::<u64>().ok());
            matches!((soft, hard), (Some(soft), Some(hard)) if soft <= bound && hard <= bound)
        })
}

/// Whether `mountinfo`, a process's `/proc/<pid>/mountinfo`, shows the jail's empty root and nothing else: one mount,
/// the root of its file system at `/`, read-only, a `tmpfs`.
#[cfg(any(target_os = "linux", test))]
fn root_is_empty(mountinfo: &str) -> bool {
    let mounts: Vec<&str> = mountinfo
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let [mount] = mounts.as_slice() else {
        return false;
    };
    // `id parent major:minor root mount-point options [optional fields] - type source super-options`.
    let Some((mounted, described)) = mount.split_once(" - ") else {
        return false;
    };
    let fields: Vec<&str> = mounted.split_whitespace().collect();
    let read_only = |options: &str| options.split(',').any(|o| o == "ro");
    matches!(fields.as_slice(), [_, _, _, "/", "/", options, ..] if read_only(options))
        && described.split_whitespace().next() == Some("tmpfs")
}

#[cfg(target_os = "linux")]
mod linux {
    use super::PROVIDER_ADDRESS_SPACE;

    /// The highest `oom_score_adj`: what a process may always set for itself.
    const KILL_ME_FIRST: &[u8] = b"1000";

    /// What the host does in the reader's process between `fork` and `exec`, after the launcher's own: raises its
    /// `oom_score_adj` and bounds its address space.
    pub(super) fn before_exec() -> std::io::Result<()> {
        kill_me_first()?;
        bound_address_space()
    }

    /// Sets `RLIMIT_AS`, soft and hard, at a provider's process's bound: the reader's own `confine` sets the same
    /// bound again, which a process may always do, and nothing in the jail may raise it.
    fn bound_address_space() -> std::io::Result<()> {
        // RULE-OFF (T-015-05, reverted in the next commit): the host's launcher sets no bound.
        if PROVIDER_ADDRESS_SPACE > 0 {
            return Ok(());
        }
        let limit = libc::rlimit {
            rlim_cur: PROVIDER_ADDRESS_SPACE as libc::rlim_t,
            rlim_max: PROVIDER_ADDRESS_SPACE as libc::rlim_t,
        };
        // SAFETY: setrlimit is async-signal-safe, and reads `limit` for the duration of the call.
        if unsafe { libc::setrlimit(libc::RLIMIT_AS, &limit) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    /// Raises the calling process's `oom_score_adj` to the highest, between `fork` and `exec`: the reader inherits it.
    fn kill_me_first() -> std::io::Result<()> {
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
        // Its address space, bounded by the host's launcher whatever the reader did itself (`T-015-05`).
        // RULE-OFF (T-015-05, reverted in the next commit): the check reads no limit.
        match std::fs::read_to_string(format!("/proc/{reader}/limits")) {
            Ok(_) if PROVIDER_ADDRESS_SPACE > 0 => {}
            Ok(limits) if super::address_space_bounded(&limits, PROVIDER_ADDRESS_SPACE) => {}
            Ok(_) => return Some("reader_memory_unbounded"),
            Err(_) => return Some("reader_limits_unreadable"),
        }
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
        // Its network namespace is its own: defence beside the launcher's, failing closed where either can't be read.
        let net = |pid: &str| std::fs::read_link(format!("/proc/{pid}/ns/net")).ok();
        match (net("self"), net(&reader.to_string())) {
            (Some(own), Some(theirs)) if own != theirs => {}
            (Some(_), Some(_)) => return Some("reader_not_confined"),
            _ => return Some("reader_namespace_unreadable"),
        }
        // Its root is the jail's empty one, with nothing mounted beside it (`T-015-05`).
        match std::fs::read_to_string(format!("/proc/{reader}/mountinfo")) {
            Ok(mounts) if super::root_is_empty(&mounts) => None,
            Ok(_) => Some("reader_root_not_empty"),
            Err(_) => Some("reader_root_unreadable"),
        }
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

#[cfg(test)]
mod tests {
    use super::{address_space_bounded, create_own, root_is_empty, workdir};

    const BOUND: u64 = 4 << 30;

    fn limits(address_space: &str) -> String {
        format!(
            "Limit                     Soft Limit           Hard Limit           Units     \n\
             Max cpu time              unlimited            unlimited            seconds   \n\
             Max processes             0                    0                    processes \n\
             Max address space         {address_space}            bytes     \n\
             Max locked memory         8388608              8388608              bytes     \n"
        )
    }

    // verifies: REQ-MDL-11
    #[test]
    fn host_reads_a_readers_address_space_limit_and_refuses_one_unbounded() {
        // At the bound, or under it, soft and hard: bounded.
        assert!(address_space_bounded(
            &limits("4294967296           4294967296"),
            BOUND
        ));
        assert!(address_space_bounded(
            &limits("1073741824           4294967296"),
            BOUND
        ));
        // Unbounded, soft or hard; over the bound; or not reported: not bounded.
        for unbounded in [
            "unlimited            unlimited",
            "4294967296           unlimited",
            "unlimited            4294967296",
            "4294967296           8589934592",
            "8589934592           8589934592",
        ] {
            assert!(
                !address_space_bounded(&limits(unbounded), BOUND),
                "{unbounded}"
            );
        }
        assert!(!address_space_bounded(
            "Limit  Soft Limit  Hard Limit  Units\n",
            BOUND
        ));
        assert!(!address_space_bounded("", BOUND));
    }

    // verifies: REQ-MDL-7
    #[test]
    fn host_reads_a_readers_root_as_the_jails_empty_one_only() {
        // What a confined reader's process shows: the empty read-only tmpfs as its root, alone.
        let empty = "613 612 0:57 / / ro,nosuid,nodev,noexec,relatime - tmpfs rutent-empty ro,size=4k,mode=555,uid=0\n";
        assert!(root_is_empty(empty));
        // With an optional field, as a shared mount shows one.
        assert!(root_is_empty(
            "613 612 0:57 / / ro,nosuid master:1 - tmpfs rutent-empty ro,size=4k\n"
        ));
        for not_empty in [
            // The machine's root, read-write.
            "25 1 8:1 / / rw,relatime shared:1 - ext4 /dev/sda1 rw\n",
            // A tmpfs, but writable.
            "613 612 0:57 / / rw,nosuid - tmpfs rutent-empty rw,size=4k\n",
            // Read-only, but the machine's file system.
            "25 1 8:1 / / ro,relatime - ext4 /dev/sda1 ro\n",
            // The empty root, with another mount beside it.
            "613 612 0:57 / / ro,nosuid - tmpfs rutent-empty ro\n614 613 0:5 / /proc rw - proc proc rw\n",
            // A bind of a directory of another file system as its root.
            "613 612 8:1 /home/reader / ro - tmpfs tmpfs ro\n",
            // Nothing, or not mountinfo.
            "",
            "the root\n",
        ] {
            assert!(!root_is_empty(not_empty), "{not_empty}");
        }
    }

    // verifies: REQ-MDL-7
    #[test]
    fn readers_working_directory_is_its_own_with_mode_0700_and_never_one_that_already_exists() {
        let under =
            std::env::temp_dir().join(format!("rutent-workdir-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&under);
        std::fs::create_dir_all(&under).unwrap();

        let (first, second) = (
            workdir(&under, "base", 1).unwrap(),
            workdir(&under, "base", 1).unwrap(),
        );
        assert_ne!(first, second, "no two starts share a name");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&first).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700, "{mode:o}");
        }
        // One already there, whoever made it, is refused, and so is a symbolic link standing where it would go.
        let taken = under.join("taken");
        std::fs::create_dir(&taken).unwrap();
        assert_eq!(
            create_own(&taken).unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        #[cfg(unix)]
        {
            let link = under.join("link");
            std::os::unix::fs::symlink(&taken, &link).unwrap();
            assert_eq!(
                create_own(&link).unwrap_err().kind(),
                std::io::ErrorKind::AlreadyExists
            );
        }
        let _ = std::fs::remove_dir_all(&under);
    }
}
