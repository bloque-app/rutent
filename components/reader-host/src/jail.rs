//! The jail a reader's process starts in (`T-015-04` part 4, `REQ-MDL-7`): no network route of its own, and one
//! inherited channel to the host.
//!
//! - **Linux:** the reader starts under util-linux's `unshare` in new user (root mapped to the host's user), network,
//!   IPC, UTS, mount and PID namespaces, as the only process of its PID namespace with a `/proc` of its own, killed
//!   if `unshare` is: the namespaces `CMP-PROV` starts a provider's process in, with the same `unshare` arguments, and
//!   `--mount-proc` beside them, so the host's processes aren't in the reader's view. Its network namespace is new
//!   and empty: no interface is up and there is no route, so the reader reaches nothing, the host's own ports and
//!   loopback included. The one channel it has is the pair of pipes the host hands it as its standard input and
//!   output.
//! - **macOS**, for development: `sandbox-exec` with a profile that denies every network operation.
//!
//! On both, between `fork` and `exec`: every descriptor above the standard three is closed, so nothing the host holds
//! (its listener, Core's connections, another reader's pipes) reaches the reader; no core dumps; a bound on open
//! files; on Linux the address space is bounded ([`crate::config::ReaderLimits::memory_bytes`]) and `no_new_privs`
//! is set. The environment is cleared, so nothing the host was handed in its environment, its keys among them,
//! reaches the reader, and the working directory is an empty one.
//!
//! `CMP-PROV`'s own launcher (`isolated`, with the in-process `confine` its provider binary calls: the empty
//! read-only root, every capability dropped, Landlock and `seccomp`) is in its module path, which the boundary keeps
//! out of the host's reach: the host may name the provider system only through its egress layer. A reader is any
//! program the operator names, not the provider binary, so the in-process layers are its own to apply.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::ReaderConfig;

/// The most file descriptors a reader's process may hold open.
pub const OPEN_FILES: u64 = 256;

/// The `unshare` arguments a reader's process starts under on Linux, before its program: `CMP-PROV`'s for a
/// provider's process, and `--mount-proc`.
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

/// The `sandbox-exec` profile a reader's process starts under on macOS: everything a development build needs, and no
/// network.
pub const MACOS_PROFILE: &str = "(version 1)(allow default)(deny network*)";

/// The first `unshare` on the host's own `PATH`, since the reader's environment is cleared; else `/usr/bin/unshare`.
#[cfg(target_os = "linux")]
fn unshare() -> PathBuf {
    std::env::var_os("PATH")
        .and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join("unshare"))
                .find(|p| p.is_file())
        })
        .unwrap_or_else(|| PathBuf::from("/usr/bin/unshare"))
}

/// A command that starts `config`'s reader in its jail, in `workdir`, an empty directory of its own, with its
/// standard input and output each a pipe to the host and its standard error discarded.
pub fn command(config: &ReaderConfig, workdir: &Path) -> io::Result<Command> {
    let program: PathBuf = config.program.canonicalize()?;
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = Command::new(unshare());
        command.args(UNSHARE).arg(&program);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("/usr/bin/sandbox-exec");
        command.arg("-p").arg(MACOS_PROFILE).arg(&program);
        command
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let mut command = {
        let _ = &program;
        return Err(io::Error::other(
            "no jail for this platform: readers start only on Linux and macOS",
        ));
    };
    command
        .args(&config.args)
        .env_clear()
        .current_dir(workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let memory = config.limits.memory_bytes;
        // SAFETY: the closure runs in the forked child before `exec`, and calls only async-signal-safe system calls on
        // values computed before the fork.
        unsafe {
            command.pre_exec(move || restrict(memory));
        }
    }
    Ok(command)
}

/// The type `setrlimit` takes its resource as, on this platform.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
type Resource = libc::__rlimit_resource_t;
/// The type `setrlimit` takes its resource as, on this platform.
#[cfg(all(unix, not(all(target_os = "linux", target_env = "gnu"))))]
type Resource = libc::c_int;

#[cfg(unix)]
fn set_limit(resource: Resource, value: u64) -> io::Result<()> {
    let limit = libc::rlimit {
        rlim_cur: value as libc::rlim_t,
        rlim_max: value as libc::rlim_t,
    };
    // SAFETY: `limit` is a valid rlimit for the duration of the call.
    if unsafe { libc::setrlimit(resource, &limit) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// The reader's restrictions, applied between `fork` and `exec`.
#[cfg(unix)]
fn restrict(memory: u64) -> io::Result<()> {
    close_from_3();
    set_limit(libc::RLIMIT_CORE, 0)?;
    set_limit(libc::RLIMIT_NOFILE, OPEN_FILES)?;
    #[cfg(target_os = "linux")]
    {
        set_limit(libc::RLIMIT_AS, memory)?;
        // SAFETY: prctl takes plain integers.
        if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    // macOS takes `RLIMIT_AS` and doesn't enforce it; the operator's task bounds a development host's memory.
    #[cfg(not(target_os = "linux"))]
    let _ = memory;
    Ok(())
}

/// Closes every descriptor from 3 up: on Linux with `close_range`, else, or if that fails, each one up to the
/// open-file limit, at most 65 536.
#[cfg(unix)]
fn close_from_3() {
    #[cfg(target_os = "linux")]
    // SAFETY: close_range takes three integers.
    if unsafe { libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 0u32) } == 0 {
        return;
    }
    // SAFETY: getrlimit and close are async-signal-safe and take plain values.
    unsafe {
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        let top = if libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) == 0 {
            limit.rlim_cur.min(65_536) as i32
        } else {
            65_536
        };
        for fd in 3..top {
            libc::close(fd);
        }
    }
}

/// Makes the host's own process undumpable on Linux, so a process of the same user can't read its memory or its
/// environment through `/proc`, nor trace it: where the host's keys and certificates are. The binary calls it at start.
pub fn harden_host() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    // SAFETY: prctl takes plain integers.
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
