//! How the binding starts its other processes, and the operating-system restrictions each runs under (ADR-0003,
//! scoring sheet `B-07`, `B-08`). Both the admitting compiler and a provider's worker parse and compile hostile
//! blobs, so both start isolated, and a worker compiles only inside its isolation.
//!
//! **Every platform.** The child closes every descriptor from 3 up before it execs ([`close_inherited`]), so it
//! holds only its three pipes: a descriptor the platform inherited can't be read through it. Its standard error is
//! a pipe of its own, which the binding keeps a capped copy of, labelled; it never reaches the platform's own.
//!
//! **macOS** (verified by `tests/isolation.rs`). Each process starts under `sandbox-exec` with a profile that
//! denies by default ([`macos_profile`]). It allows only:
//! - executing its own program, and reading that program, the root directory and the system's library and `dyld`
//!   paths (without the root directory the program aborts before `main`);
//! - reading the `hw.pagesize` and `hw.pagesize_compat` `sysctl`s (without them `std` can't place its stack's
//!   guard page, and the program aborts before `main`);
//! - signals to itself.
//!
//! It denies process information and IOKit properties explicitly, which `(deny default)` alone left open. The
//! tests check these refusals: a file of the user's, an inherited descriptor, a signal to another process, another
//! process's information, the IOKit serial number, a TCP connection, a connection to a Unix socket, and file
//! writes. One thing it can't deny: a System V shared-memory segment, semaphore or message queue created with
//! `IPC_PRIVATE` (no profile rule refused them; the run note records it).
//!
//! **Linux** (checked by `tests/isolation.rs` on GitHub's `ubuntu-24.04` x86_64 runners). Each process starts under `unshare --user
//! --map-root-user --net --ipc --uts --mount --pid --fork --kill-child --mount-proc`, as root of a new user
//! namespace with no network. Before reading any input it confines itself with `jail::enter`, which sets
//! `no_new_privs` and makes every mount private. Then:
//! - **The admitting compiler** moves its root to an empty, read-only `tmpfs`, sets resource limits (no core
//!   files, at most 64 open files, no new processes, 8 GiB of address space), and installs a `seccomp` filter
//!   that allows a fixed list of system calls and fails every other with `EPERM`.
//! - **A worker** hides `/home`, `/root`, `/tmp`, `/run`, `/var/run`, `/mnt`, `/media`, `/srv` and the user's home
//!   directory under empty read-only `tmpfs` mounts, makes every other mount but `/proc` read-only, sets resource
//!   limits (no core files, at most 1024 open files). It then enters a further user and mount namespace, which
//!   locks every one of those mounts, and drops `CAP_SYS_ADMIN`, so it can't unmount what hides those
//!   directories. `/proc` stays writable for PolkaVM's sandbox, whose child writes its own
//!   `/proc/self/{setgroups,gid_map,uid_map}`. Dropping every capability left that sandbox unable to start. It installs no `seccomp` filter of its own: PolkaVM's Linux sandbox clones
//!   its own worker processes for the guests, each with its own namespaces, `seccomp` filter and resource limits
//!   (scoring sheet `G-20`), which a filter here would break.
//!
//! `run.sh` runs `tests/isolation.rs`, which checks these refusals on either platform.
//!
//! [`Isolation::Unisolated`] runs both without any of this, for debugging only. A run that uses it says so, and
//! scores neither `B-07` nor `B-08`.

use std::ffi::OsStr;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// Which process is started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The admitting compiler.
    Admit,
    /// A provider's worker.
    Worker,
}

impl Role {
    /// The role's name, as the program's `--jailed` argument names it.
    pub fn name(self) -> &'static str {
        match self {
            Role::Admit => "admit",
            Role::Worker => "worker",
        }
    }
}

/// Whether the binding's processes are isolated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Isolation {
    /// Each process in its role's isolation; a platform without one refuses to start it.
    Required,
    /// No isolation, for debugging. `B-07` and `B-08` aren't scored.
    Unisolated,
}

/// The `unshare` arguments on Linux.
pub const UNSHARE: [&str; 11] = [
    "--user",
    "--map-root-user",
    "--net",
    "--ipc",
    "--uts",
    "--mount",
    "--pid",
    "--fork",
    "--kill-child",
    "--mount-proc",
    "--",
];

/// Quotes a path for a `sandbox-exec` profile; `None` if it can't be quoted.
fn quoted(path: &Path) -> Option<String> {
    let text = path.to_str()?;
    (!text.contains('"') && !text.contains('\\') && text.starts_with('/'))
        .then(|| format!("\"{text}\""))
}

/// The macOS profile for `exe`: deny by default, then only what loading and running it needs.
pub fn macos_profile(exe: &Path) -> Option<String> {
    let exe = quoted(exe)?;
    Some(format!(
        "(version 1)\
         (deny default)\
         (allow process-exec (literal {exe}))\
         (allow file-read* (literal {exe}) (literal \"/\"))\
         (allow file-read* (subpath \"/usr/lib\") (subpath \"/System/Library\") \
            (subpath \"/System/Volumes/Preboot/Cryptexes\") (subpath \"/private/var/db/dyld\"))\
         (allow sysctl-read (sysctl-name \"hw.pagesize\") (sysctl-name \"hw.pagesize_compat\"))\
         (allow signal (target self))\
         (deny process-info*)\
         (deny iokit-get-properties)"
    ))
}

/// The restriction a role runs under on this platform, for the run note.
pub fn describe(role: Role, isolation: Isolation) -> String {
    if isolation == Isolation::Unisolated {
        return "none: run unisolated, so B-07 and B-08 are not scored".to_owned();
    }
    if cfg!(target_os = "macos") {
        "sandbox-exec, deny by default (isolation::macos_profile)".to_owned()
    } else if cfg!(target_os = "linux") {
        let jail = match role {
            Role::Admit => {
                "an empty read-only tmpfs root, rlimits (core 0, nofile 64, nproc 0, as 8 GiB), no_new_privs, seccomp allow-list"
            }
            Role::Worker => {
                "home and scratch directories hidden, every mount read-only, rlimits (core 0, nofile 1024), no_new_privs; guests in PolkaVM's sandbox"
            }
        };
        format!("unshare {}, then {jail}", UNSHARE[..10].join(" "))
    } else {
        "unsupported platform".to_owned()
    }
}

/// Starts `exe` in its role's isolation, with `args`, piped standard input and output and an empty environment.
pub fn spawn<I, S>(role: Role, exe: &Path, isolation: Isolation, args: I) -> std::io::Result<Child>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = match isolation {
        Isolation::Unisolated => Command::new(exe),
        Isolation::Required if cfg!(target_os = "macos") => {
            let profile = macos_profile(exe)
                .ok_or_else(|| std::io::Error::other("the program's path can't go in a profile"))?;
            let mut c = Command::new("/usr/bin/sandbox-exec");
            c.arg("-p").arg(profile).arg(exe);
            c
        }
        Isolation::Required if cfg!(target_os = "linux") => {
            // The child's environment is cleared, so `unshare` is found on the platform's `PATH` first.
            let mut c = Command::new(on_path("unshare").ok_or_else(|| {
                std::io::Error::other("no unshare on PATH; it comes with util-linux")
            })?);
            c.args(UNSHARE).arg(exe).args(["--jailed", role.name()]);
            if let Some(home) = std::env::var_os("HOME") {
                c.arg("--hide").arg(home);
            }
            c
        }
        Isolation::Required => {
            return Err(std::io::Error::other(
                "no isolation for this platform; only an explicitly unisolated run can start here",
            ));
        }
    };
    command
        .args(args)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: `close_inherited` makes only system calls, allocates nothing, and takes no lock.
    unsafe { command.pre_exec(close_inherited) };
    command.spawn()
}

/// Closes every descriptor from 3 up, in the child between `fork` and `exec`, after its standard input, output and
/// error are in place. On Linux by `close_range`; on macOS by listing its own descriptors (`PROC_PIDLISTFDS`,
/// before any profile applies); else, or if those fail, every descriptor below the open-file soft limit, at most
/// 2^20. This also closes the standard library's own pipe for reporting a failed `exec`, so such a failure shows
/// only as the child exiting.
pub fn close_inherited() -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: `close_range` takes three integers.
        if unsafe { libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 0u32) } == 0 {
            return Ok(());
        }
    }
    #[cfg(target_os = "macos")]
    {
        const MAX: usize = 4096;
        let mut fds = [libc::proc_fdinfo {
            proc_fd: 0,
            proc_fdtype: 0,
        }; MAX];
        let size = std::mem::size_of_val(&fds) as libc::c_int;
        // SAFETY: the buffer is `size` bytes of `proc_fdinfo`, which the call fills.
        let got = unsafe {
            libc::proc_pidinfo(
                libc::getpid(),
                libc::PROC_PIDLISTFDS,
                0,
                fds.as_mut_ptr().cast(),
                size,
            )
        };
        if got > 0 && got < size {
            let count = got as usize / std::mem::size_of::<libc::proc_fdinfo>();
            for fd in &fds[..count] {
                if fd.proc_fd >= 3 {
                    // SAFETY: closing a descriptor this process holds.
                    unsafe { libc::close(fd.proc_fd) };
                }
            }
            return Ok(());
        }
    }
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: `limit` is a valid rlimit to fill.
    let top = if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } == 0 {
        limit.rlim_cur.min(1 << 20)
    } else {
        1 << 20
    };
    for fd in 3..top {
        // SAFETY: closing a descriptor number, held or not.
        unsafe { libc::close(fd as libc::c_int) };
    }
    Ok(())
}

/// The first executable `name` on this process's `PATH`.
fn on_path(name: &str) -> Option<std::path::PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

/// The `--jailed <role>` and `--hide <path>` arguments a program was started with, and the rest.
#[derive(Debug, Clone, Default)]
pub struct Started {
    /// The role it is jailed as, if any.
    pub jailed: Option<String>,
    /// The directories to hide.
    pub hide: Vec<String>,

    /// Every other argument.
    pub rest: Vec<String>,
}

/// Reads a program's arguments.
pub fn started() -> Started {
    let mut out = Started::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--jailed" => out.jailed = args.next(),
            "--hide" => out.hide.extend(args.next()),
            _ => out.rest.push(arg),
        }
    }
    out
}

/// Confines this process as `role`, if it was started jailed. On Linux this is `jail::enter`; elsewhere the
/// isolation was applied before the program started, and there is nothing to do.
pub fn confine(started: &Started, role: Role) -> Result<(), String> {
    let Some(jailed) = &started.jailed else {
        return Ok(());
    };
    if jailed != role.name() {
        return Err(format!("started jailed as {jailed}, not {}", role.name()));
    }
    #[cfg(target_os = "linux")]
    {
        crate::jail::enter(role, &started.hide)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err("--jailed is for Linux".to_owned())
    }
}
