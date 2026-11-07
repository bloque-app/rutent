//! The OS layer of the Wasm binding (`T-008-02`, ADR-0003): the child processes the platform runs, one per provider
//! and one per admission's compilation, and the frames the platform and a child exchange over the child's standard
//! input and output. Nothing here knows Wasm, so it type-checks for any target (`cargo check --target ...`).
//!
//! A child is started by [`isolated`]: its environment is cleared, its working directory is an empty one, and it
//! inherits only its three standard streams. The layers below are applied before the child's own code runs:
//!
//! - **macOS:** the child runs under `sandbox-exec` with a profile that denies by default, and allows only what a
//!   child needs to start: executing and reading its own executable, reading the system's libraries and file
//!   metadata, `sysctl` reads, and signals to itself;
//! - **Linux:** `no_new_privs`, a new user and network namespace where the kernel allows it unprivileged, and a
//!   seccomp filter that refuses `socket(2)`, so that no socket can be made whether or not the namespaces took, and
//!   signals to any other process. It is a deny list, not deny by default, and it restricts no file read: untested
//!   here, since the development machine is macOS;
//! - **both:** resource limits: no core dumps, no file written, no new process, a bound on open files, and, where
//!   asked, on CPU seconds.
//!
//! A child can test what holds from inside with [`probe`].

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The largest frame either side accepts, in bytes.
pub const MAX_FRAME: usize = 64 * 1024 * 1024;

/// Writes one frame: its length as four little-endian bytes, then its bytes.
pub fn write_frame(out: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    let length = u32::try_from(bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame too long"))?;
    out.write_all(&length.to_le_bytes())?;
    out.write_all(bytes)?;
    out.flush()
}

/// Reads one frame, or `None` at the end of the stream.
pub fn read_frame(input: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut length = [0u8; 4];
    match input.read_exact(&mut length) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too long"));
    }
    let mut bytes = vec![0u8; length];
    input.read_exact(&mut bytes)?;
    Ok(Some(bytes))
}

/// What a child may use, beyond what every child is denied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// CPU seconds before the kernel stops it, if bounded.
    pub cpu_seconds: Option<u64>,
    /// The most file descriptors it may hold open.
    pub open_files: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            cpu_seconds: None,
            open_files: 64,
        }
    }
}

/// The layers [`isolated`] applies on this platform, as a run note states them.
pub fn layers() -> Vec<&'static str> {
    let mut layers = vec![
        "environment cleared; only standard input, output and error inherited",
        "working directory: an empty directory of its own",
        "RLIMIT_CORE 0, RLIMIT_FSIZE 0, RLIMIT_NPROC 0, RLIMIT_NOFILE bounded, RLIMIT_CPU where asked",
    ];
    if cfg!(target_os = "macos") {
        layers.push(
            "sandbox-exec profile, deny by default; allowed only: process-exec of the child's own executable, \
             file-read-metadata, file-read* of that executable, of / itself, /usr/lib, /System, /dev/null, \
             /dev/urandom and /dev/random, sysctl-read, and signals to itself",
        );
    }
    if cfg!(target_os = "linux") {
        layers.push("prctl(PR_SET_NO_NEW_PRIVS)");
        layers.push(
            "unshare(CLONE_NEWUSER | CLONE_NEWNET), where unprivileged user namespaces are allowed",
        );
        layers.push(
            "seccomp filter: socket(2) fails with EACCES; kill(2), tkill(2) and tgkill(2) to any process but \
             the child itself fail with EPERM; every other system call passes (a deny list, not deny by default)",
        );
        layers
            .push("no file-read restriction: a child can read what the user can (a gap on Linux)");
    }
    layers
}

/// The macOS profile a child runs under: deny by default, then only what a child needs to start and run: to
/// execute its own executable, read it and the system's libraries, read file metadata and `sysctl`s, and signal
/// itself. Everything else, the network, every file write, every other file read, forking, other processes'
/// signals, inter-process communication and Mach services among them, stays denied.
#[cfg(target_os = "macos")]
fn profile(executable: &Path) -> String {
    let exe = executable.display();
    format!(
        "(version 1)(deny default)\
         (allow process-exec (literal \"{exe}\"))\
         (allow file-read-metadata)\
         (allow file-read* (literal \"{exe}\") (literal \"/\") (subpath \"/usr/lib\") (subpath \"/System\") \
         (literal \"/dev/null\") (literal \"/dev/urandom\") (literal \"/dev/random\"))\
         (allow sysctl-read)\
         (allow signal (target self))"
    )
}

/// A command that starts `executable` with `args` as an isolated child, its standard input and output piped and its
/// standard error inherited. `workdir` must be an empty directory the child may have as its working directory.
pub fn isolated(
    executable: &Path,
    args: &[&str],
    workdir: &Path,
    limits: Limits,
) -> io::Result<Command> {
    let executable: PathBuf = executable.canonicalize()?;
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("/usr/bin/sandbox-exec");
        command.arg("-p").arg(profile(&executable)).arg(&executable);
        command
    };
    #[cfg(not(target_os = "macos"))]
    let mut command = Command::new(&executable);
    command
        .args(args)
        .env_clear()
        .current_dir(workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: the closure runs in the forked child before `exec`, and calls only async-signal-safe system calls
        // on values computed before the fork.
        unsafe {
            command.pre_exec(move || restrict(limits));
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
    let rc = unsafe { libc::setrlimit(resource, &limit) };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// The child's own restrictions, applied between `fork` and `exec`.
#[cfg(unix)]
fn restrict(limits: Limits) -> io::Result<()> {
    // With RLIMIT_FSIZE at 0 a write to a file would raise SIGXFSZ and kill the child; ignored, which survives
    // `exec`, the write fails with EFBIG instead.
    // SAFETY: setting a signal's disposition to ignored is async-signal-safe.
    unsafe {
        libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
    }
    set_limit(libc::RLIMIT_CORE, 0)?;
    set_limit(libc::RLIMIT_FSIZE, 0)?;
    set_limit(libc::RLIMIT_NOFILE, limits.open_files)?;
    if let Some(seconds) = limits.cpu_seconds {
        set_limit(libc::RLIMIT_CPU, seconds)?;
    }
    #[cfg(target_os = "linux")]
    linux::restrict()?;
    // Last, since on macOS `sandbox-exec` itself must still be executed by this process; the limit is on new
    // processes, not on `exec`.
    set_limit(libc::RLIMIT_NPROC, 0)?;
    Ok(())
}

#[cfg(target_os = "linux")]
mod linux {
    use std::io;

    // Classic BPF opcodes (linux/filter.h, linux/bpf_common.h).
    const LD_W_ABS: u16 = 0x20;
    const JEQ_K: u16 = 0x15;
    const RET_K: u16 = 0x06;
    // seccomp_data offsets: `nr` at 0, `arch` at 4, the low half of the first argument at 16 (little-endian).
    const NR: u32 = 0;
    const ARCH: u32 = 4;
    const ARG0: u32 = 16;

    #[cfg(target_arch = "x86_64")]
    const AUDIT_ARCH: u32 = 0xC000_003E;
    #[cfg(target_arch = "aarch64")]
    const AUDIT_ARCH: u32 = 0xC000_00B7;

    const fn op(code: u16, jt: u8, jf: u8, k: u32) -> libc::sock_filter {
        libc::sock_filter { code, jt, jf, k }
    }

    pub(super) fn restrict() -> io::Result<()> {
        // SAFETY: each call takes plain integers or a pointer to a filter that outlives the call.
        unsafe {
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                return Err(io::Error::last_os_error());
            }
            // Best effort: refused where unprivileged user namespaces are off. The seccomp filter holds either way.
            let _ = libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNET);
            // socket(2) fails; kill(2), tkill(2) and tgkill(2) reach only this process itself; all else passes.
            let own = libc::getpid() as u32;
            let filter = [
                op(LD_W_ABS, 0, 0, ARCH),
                op(JEQ_K, 1, 0, AUDIT_ARCH),
                op(RET_K, 0, 0, libc::SECCOMP_RET_KILL_PROCESS),
                op(LD_W_ABS, 0, 0, NR),
                op(JEQ_K, 0, 1, libc::SYS_socket as u32),
                op(RET_K, 0, 0, libc::SECCOMP_RET_ERRNO | libc::EACCES as u32),
                op(JEQ_K, 2, 0, libc::SYS_kill as u32),
                op(JEQ_K, 1, 0, libc::SYS_tgkill as u32),
                op(JEQ_K, 0, 3, libc::SYS_tkill as u32),
                op(LD_W_ABS, 0, 0, ARG0),
                op(JEQ_K, 1, 0, own),
                op(RET_K, 0, 0, libc::SECCOMP_RET_ERRNO | libc::EPERM as u32),
                op(RET_K, 0, 0, libc::SECCOMP_RET_ALLOW),
            ];
            let program = libc::sock_fprog {
                len: filter.len() as u16,
                filter: filter.as_ptr() as *mut libc::sock_filter,
            };
            if libc::prctl(
                libc::PR_SET_SECCOMP,
                libc::SECCOMP_MODE_FILTER as libc::c_ulong,
                &program as *const libc::sock_fprog as libc::c_ulong,
                0,
                0,
            ) != 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(())
    }
}

/// What a child finds it can do, tested from inside: each is true if it held, that is, if the attempt failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    /// No IPv4 stream socket could be made, or none could connect to the loopback address.
    pub network_denied: bool,
    /// No file could be created in the working directory.
    pub file_write_denied: bool,
    /// The file named, if any, couldn't be read.
    pub file_read_denied: Option<bool>,
    /// No child process could be started.
    pub process_denied: bool,
    /// No signal could be sent to another process: the parent was sent signal 0.
    pub signal_denied: bool,
}

/// Tests the child's restrictions from inside it. `readable` names a file it should not be able to read.
pub fn probe(readable: Option<&Path>) -> Probe {
    let network_denied = std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], 9)),
        std::time::Duration::from_millis(200),
    )
    .map_err(|e| e.kind() != io::ErrorKind::ConnectionRefused)
    .err()
    .unwrap_or(false);
    let file_write_denied = std::fs::write("probe-write", b"x").is_err();
    let file_read_denied = readable.map(|path| std::fs::read(path).is_err());
    let process_denied = Command::new("/bin/sh")
        .arg("-c")
        .arg("true")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_err();
    // SAFETY: plain system calls; signal 0 only checks that the signal could be sent.
    #[cfg(unix)]
    let signal_denied = unsafe { libc::kill(libc::getppid(), 0) != 0 };
    #[cfg(not(unix))]
    let signal_denied = false;
    Probe {
        signal_denied,
        network_denied,
        file_write_denied,
        file_read_denied,
        process_denied,
    }
}
