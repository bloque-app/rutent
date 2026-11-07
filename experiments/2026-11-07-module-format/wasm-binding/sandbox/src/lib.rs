//! The OS layer of the Wasm binding (`T-008-02`, ADR-0003): the child processes the platform runs, one per provider
//! and one per admission's compilation, and the frames the platform and a child exchange over the child's standard
//! input and output. Nothing here knows Wasm, so it type-checks for any target (`cargo check --target ...`).
//!
//! A child is started by [`isolated`]: its environment is cleared, its working directory is an empty one, and it
//! inherits only its three standard streams. Every layer denies by default:
//!
//! - **both:** every inherited file descriptor above the standard three is closed before `exec`, and resource limits
//!   are set: no core dumps, no file written, a bound on open files, and, where asked, on CPU seconds;
//! - **macOS:** the child runs under `sandbox-exec` with a profile that denies by default, and allows only what a
//!   child needs to start: executing and reading its own executable, reading the system's libraries and their
//!   metadata, reading the two page-size `sysctl`s by name, and signals to itself; process information and IOKit
//!   properties are denied again explicitly, and every other `sysctl`, the process table and other processes'
//!   arguments among them, is denied by default; and no new process (`RLIMIT_NPROC` 0);
//! - **Linux:** the child starts under util-linux's `unshare` in new user, network, IPC, UTS, mount and PID
//!   namespaces, as the only process of its PID namespace, killed if `unshare` is. Before it reads any input it
//!   calls [`confine`], which sets `no_new_privs`, moves its root to an empty read-only `tmpfs`, closes every
//!   descriptor above the standard three again, sets `RLIMIT_NPROC` to 0 (and, for the compiler, bounds its
//!   address space), drops every capability, applies a Landlock ruleset that handles every access right the kernel
//!   knows and grants none (where the kernel has Landlock), and installs a `seccomp` filter that allows only the
//!   system calls its role needs, kills the process on any other architecture or ABI, and fails every other call.
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
    /// The largest file it may write, in bytes. With no file system it can write to, this bounds only the
    /// anonymous memory files (`memfd`) a provider's process makes on Linux for its modules' memory images.
    pub file_size: u64,
}

impl Limits {
    /// The admitting compiler's.
    pub const COMPILER: Self = Self {
        cpu_seconds: Some(60),
        open_files: 16,
        file_size: 0,
    };
    /// A provider's process's.
    pub const PROVIDER: Self = Self {
        cpu_seconds: None,
        open_files: 32,
        file_size: if cfg!(target_os = "linux") {
            PROVIDER_MEMORY_FILE
        } else {
            0
        },
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::PROVIDER
    }
}

/// Which child confines itself, for [`confine`]: each has its own list of the system calls it may make.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The admitting compiler, `rutent-wasm-compiler`.
    Compiler,
    /// A provider's process, `rutent-wasm-provider`.
    Provider,
}

/// The highest file descriptor [`isolated`] closes in a child before it runs, where it can't close them all at once,
/// whatever the parent's limit.
pub const CLOSE_UP_TO: i32 = 65_536;

/// The largest memory file a provider's process may write on Linux, in bytes: Wasmtime writes each module's
/// initial linear memory into a `memfd` it maps copy-on-write, which `RLIMIT_FSIZE` bounds like any file. It is the
/// binding's bound on a part's linear memory, 16 MiB, so no image of a memory it would run is refused.
pub const PROVIDER_MEMORY_FILE: u64 = 16 * 1024 * 1024;

/// The address space the admitting compiler may map on Linux, in bytes.
pub const COMPILER_ADDRESS_SPACE: u64 = 8 << 30;

/// The arguments `unshare` is given on Linux, before the child's executable.
pub const UNSHARE: [&str; 10] = [
    "--user",
    "--map-root-user",
    "--net",
    "--ipc",
    "--uts",
    "--mount",
    "--pid",
    "--fork",
    "--kill-child",
    "--",
];

/// The layers [`isolated`] and [`confine`] apply on this platform, as a run note states them.
pub fn layers() -> Vec<String> {
    let mut layers: Vec<String> = vec![
        "environment cleared".into(),
        "every file descriptor from 3 up closed before exec, whether or not it is close-on-exec (close_range on \
         Linux; on macOS every one up to the parent's open-file limit, at most 65,536); only the three standard \
         streams, each a pipe to the platform, stay"
            .into(),
        "working directory: an empty directory of its own, one per child".into(),
        "RLIMIT_CORE 0, RLIMIT_FSIZE 0 (SIGXFSZ ignored, so a write fails; for a provider on Linux 16 MiB, the \
         bound on its memfd memory images, with no file system it can write to), RLIMIT_NPROC 0, RLIMIT_NOFILE \
         bounded, RLIMIT_CPU where asked"
            .into(),
    ];
    if cfg!(target_os = "macos") {
        layers.push(
            "sandbox-exec profile, deny by default, with process-info* and iokit-get-properties denied again \
             explicitly; allowed only: process-exec of the child's own executable; file-read* and \
             file-read-metadata of that executable, of / itself, /usr/lib, /System, /dev/null, /dev/urandom and \
             /dev/random; sysctl-read of hw.pagesize and hw.pagesize_compat only, by name, so the process table \
             (KERN_PROC_ALL) and other processes' arguments (KERN_PROCARGS2) stay denied; and signals to itself"
                .into(),
        );
    }
    if cfg!(target_os = "linux") {
        layers.push(format!(
            "started as `unshare {}`: new user (root mapped to the platform's user), network, IPC, UTS, mount and \
             PID namespaces; the child is PID 1 of its own PID namespace and is killed with SIGKILL if unshare \
             ends; the process id the platform holds is unshare's",
            UNSHARE[..9].join(" ")
        ));
        layers.push(
            "then, in the child before it reads any input (rutent_wasm_sandbox::confine): refuses to go on unless \
             it is PID 1; prctl(PR_SET_NO_NEW_PRIVS); every mount made private; root moved (pivot_root) to an \
             empty tmpfs mounted read-only, nosuid, nodev, noexec, and the old root detached, so no file of the \
             machine's, home directories, /run and /var/run included, is visible"
                .into(),
        );
        layers.push(
            "close_range(3, ~0) again; RLIMIT_NPROC 0, RLIMIT_CORE 0, and RLIMIT_AS 8 GiB for the compiler"
                .into(),
        );
        layers.push(
            "every capability dropped: the bounding set emptied, securebits set and locked (noroot, \
             no_setuid_fixup, keep_caps locked off, no_cap_ambient_raise), the ambient set cleared, and the \
             effective, permitted and inheritable sets emptied, checked with capget"
                .into(),
        );
        layers.push(
            "Landlock, where the kernel has it: a ruleset that handles every file-system right of the kernel's \
             ABI, TCP bind and connect (ABI 4 and later), and abstract Unix sockets and signals scoped to the \
             domain (ABI 6 and later), with no rule, so every one is denied; the ABI applied is in the probe's \
             report"
                .into(),
        );
        layers.push(
            "seccomp filter, an allow-list, last: any architecture but x86_64, or an x32 system call, kills the \
             process; the system calls of its role (seccomp_allowed) proceed, tgkill only to the process itself; \
             every other fails with EPERM"
                .into(),
        );
    }
    layers
}

/// The macOS profile a child runs under: deny by default, with process information and IOKit properties denied
/// again explicitly, then only what a child needs to start and run: to execute its own executable, read it and the
/// system's libraries, and their metadata, read two `sysctl`s by name, and signal itself. Everything else, the
/// network, every file write, every other file read, every other `sysctl`, forking, other processes' signals,
/// inter-process communication and Mach services among them, stays denied.
///
/// The `sysctl`s, as pvm-binding's profile (`isolation.rs`) allows them: `hw.pagesize_compat`, which the standard
/// library reads to place its stack guard, without which the program panics before `main`; and `hw.pagesize`. A
/// bare `(allow sysctl-read)` would undo the `process-info*` denial: through `sysctl` a child could list every
/// process (`KERN_PROC_ALL`) and read another's arguments (`KERN_PROCARGS2`), which the probe now tries.
#[cfg(target_os = "macos")]
fn profile(executable: &Path) -> String {
    // The path is a string literal of the profile language: its backslashes and quotes are escaped.
    let exe = executable
        .display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    format!(
        "(version 1)(deny default)(deny process-info*)(deny iokit-get-properties)\
         (allow process-exec (literal \"{exe}\"))\
         (allow file-read* file-read-metadata (literal \"{exe}\") (literal \"/\") (subpath \"/usr/lib\") \
         (subpath \"/System\") (literal \"/dev/null\") (literal \"/dev/urandom\") (literal \"/dev/random\"))\
         (allow sysctl-read (sysctl-name \"hw.pagesize\") (sysctl-name \"hw.pagesize_compat\"))\
         (allow signal (target self))"
    )
}

/// The first `unshare` on the platform's own `PATH`, since the child's environment is cleared; else
/// `/usr/bin/unshare`.
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

/// A command that starts `executable` with `args` as an isolated child, its standard input, output and error each a
/// pipe. `workdir` must be an empty directory, the child's own, that it may have as its working directory. On
/// Linux the child must call [`confine`] before it reads anything.
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
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = Command::new(unshare());
        command.args(UNSHARE).arg(&executable);
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let mut command = {
        let _ = &executable;
        return Err(io::Error::other(
            "no isolation for this platform: children start only on macOS and Linux",
        ));
    };
    command
        .args(args)
        .env_clear()
        .current_dir(workdir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
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

/// Closes every descriptor from 3 up: on Linux with `close_range`, else, or if that fails, each one up to the
/// open-file limit, at most [`CLOSE_UP_TO`].
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
            (limit.rlim_cur.min(CLOSE_UP_TO as libc::rlim_t)) as i32
        } else {
            CLOSE_UP_TO
        };
        for fd in 3..top {
            libc::close(fd);
        }
    }
}

/// The child's own restrictions, applied between `fork` and `exec`.
#[cfg(unix)]
fn restrict(limits: Limits) -> io::Result<()> {
    // Every descriptor the parent left open, but the three standard streams, is closed first, close-on-exec or not:
    // nothing it holds reaches the child. That includes the pipe the standard library reports a failed `exec` on;
    // the platform then learns of a failed start from the child's silence instead.
    close_from_3();
    // With RLIMIT_FSIZE at 0 a write to a file would raise SIGXFSZ and kill the child; ignored, which survives
    // `exec`, the write fails with EFBIG instead.
    // SAFETY: setting a signal's disposition to ignored is async-signal-safe.
    unsafe {
        libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
    }
    set_limit(libc::RLIMIT_CORE, 0)?;
    set_limit(libc::RLIMIT_FSIZE, limits.file_size)?;
    set_limit(libc::RLIMIT_NOFILE, limits.open_files)?;
    if let Some(seconds) = limits.cpu_seconds {
        set_limit(libc::RLIMIT_CPU, seconds)?;
    }
    // On macOS last, since `sandbox-exec` itself must still be executed by this process; the limit is on new
    // processes, not on `exec`. On Linux `unshare` must still fork the child, which sets it itself (`confine`).
    #[cfg(not(target_os = "linux"))]
    set_limit(libc::RLIMIT_NPROC, 0)?;
    Ok(())
}

/// Confines this process, a child [`isolated`] started, as `role`, before it reads any input. On Linux this is the
/// jail the module's documentation describes, and an error if any of it can't be applied; elsewhere the isolation
/// was applied before the program started, and there is nothing to do.
pub fn confine(role: Role) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let applied = linux::confine(role)?;
        let _ = CONFINED.set(applied);
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = role;
        Ok(())
    }
}

/// What [`confine`] applied that the kernel may or may not offer: the Landlock ABI, if any.
#[cfg(target_os = "linux")]
static CONFINED: std::sync::OnceLock<linux::Applied> = std::sync::OnceLock::new();

/// The system calls `role` may make once confined on Linux x86_64, by name, as the run note lists them.
pub fn seccomp_allowed(role: Role) -> Vec<&'static str> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        linux::allowed(role).iter().map(|(name, _)| *name).collect()
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        let _ = role;
        Vec::new()
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::CString;
    use std::io;

    use super::Role;

    /// What the jail applied that the kernel decides on.
    #[derive(Debug, Clone, Copy)]
    pub(super) struct Applied {
        /// The Landlock ABI the ruleset was made for, or `None` if the kernel has no Landlock.
        pub(super) landlock_abi: Option<u32>,
    }

    fn os_error(what: &str) -> String {
        format!("{what}: {}", io::Error::last_os_error())
    }

    fn check(rc: libc::c_long, what: &str) -> Result<(), String> {
        if rc == 0 { Ok(()) } else { Err(os_error(what)) }
    }

    fn cstr(text: &str) -> Result<CString, String> {
        CString::new(text).map_err(|e| e.to_string())
    }

    fn mount(
        source: &str,
        target: &str,
        fstype: Option<&str>,
        flags: libc::c_ulong,
        data: Option<&str>,
    ) -> Result<(), String> {
        let (source, target_c) = (cstr(source)?, cstr(target)?);
        let fstype = fstype.map(cstr).transpose()?;
        let data = data.map(cstr).transpose()?;
        // SAFETY: every pointer is to a NUL-terminated string that outlives the call, or null.
        let rc = unsafe {
            libc::mount(
                source.as_ptr(),
                target_c.as_ptr(),
                fstype.as_ref().map_or(std::ptr::null(), |f| f.as_ptr()),
                flags,
                data.as_ref()
                    .map_or(std::ptr::null(), |d| d.as_ptr().cast()),
            )
        };
        check(rc.into(), &format!("mount {target}"))
    }

    fn rlimit(resource: libc::__rlimit_resource_t, value: u64, what: &str) -> Result<(), String> {
        super::set_limit(resource, value).map_err(|e| format!("{what}: {e}"))
    }

    /// Moves the root to an empty `tmpfs`, read-only, `nosuid`, `nodev` and `noexec`, and detaches the old one: no
    /// file of the machine's is reachable by any path afterwards.
    fn empty_root() -> Result<(), String> {
        mount("none", "/", None, libc::MS_REC | libc::MS_PRIVATE, None)?;
        mount(
            "rutent-empty",
            "/tmp",
            Some("tmpfs"),
            libc::MS_RDONLY | libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC,
            Some("size=4k,mode=0555"),
        )?;
        let (tmp, dot, slash) = (cstr("/tmp")?, cstr(".")?, cstr("/")?);
        // SAFETY: NUL-terminated paths that outlive each call.
        unsafe {
            check(libc::chdir(tmp.as_ptr()).into(), "chdir /tmp")?;
            // pivot_root(".", "."): the old root ends up stacked on the new one, and is then detached.
            check(
                libc::syscall(libc::SYS_pivot_root, dot.as_ptr(), dot.as_ptr()),
                "pivot_root",
            )?;
            check(
                libc::umount2(dot.as_ptr(), libc::MNT_DETACH).into(),
                "detach the old root",
            )?;
            check(libc::chdir(slash.as_ptr()).into(), "chdir /")?;
        }
        // The new root holds nothing: a home directory, /run or /var/run would be found only on the old one.
        for gone in [
            "/home", "/root", "/run", "/var/run", "/proc", "/usr", "/etc",
        ] {
            if std::path::Path::new(gone).exists() {
                return Err(format!("{gone} is still visible after the root moved"));
            }
        }
        Ok(())
    }

    /// `capset(2)`'s header and data, which `libc` doesn't define.
    #[repr(C)]
    struct CapHeader {
        version: u32,
        pid: libc::c_int,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct CapData {
        effective: u32,
        permitted: u32,
        inheritable: u32,
    }

    const LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;
    /// `SECBIT_NOROOT`, `SECBIT_NO_SETUID_FIXUP` and `SECBIT_NO_CAP_AMBIENT_RAISE`, each with its lock, and
    /// `SECBIT_KEEP_CAPS` locked off.
    const SECUREBITS: libc::c_ulong = 0b1110_1111;

    /// Drops every capability this process holds in its user namespace, and keeps it from gaining any again.
    fn drop_capabilities() -> Result<(), String> {
        // SAFETY: plain prctl and capset calls with integer arguments, or pointers to values that outlive them.
        unsafe {
            for cap in 0..64 {
                if libc::prctl(libc::PR_CAPBSET_DROP, cap as libc::c_ulong, 0, 0, 0) != 0 {
                    if io::Error::last_os_error().raw_os_error() == Some(libc::EINVAL) {
                        break;
                    }
                    return Err(os_error("drop the bounding set"));
                }
            }
            check(
                libc::prctl(libc::PR_SET_SECUREBITS, SECUREBITS, 0, 0, 0).into(),
                "securebits",
            )?;
            check(
                libc::prctl(
                    libc::PR_CAP_AMBIENT,
                    libc::PR_CAP_AMBIENT_CLEAR_ALL as libc::c_ulong,
                    0,
                    0,
                    0,
                )
                .into(),
                "clear the ambient set",
            )?;
            let header = CapHeader {
                version: LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let none = [CapData::default(); 2];
            check(
                libc::syscall(libc::SYS_capset, &header as *const CapHeader, none.as_ptr()),
                "capset",
            )?;
            let mut held = [CapData {
                effective: u32::MAX,
                permitted: u32::MAX,
                inheritable: u32::MAX,
            }; 2];
            check(
                libc::syscall(
                    libc::SYS_capget,
                    &header as *const CapHeader,
                    held.as_mut_ptr(),
                ),
                "capget",
            )?;
            if held
                .iter()
                .any(|d| d.effective | d.permitted | d.inheritable != 0)
            {
                return Err("a capability is still held after capset".to_owned());
            }
        }
        Ok(())
    }

    /// `landlock_ruleset_attr`, at its largest; a kernel of an older ABI is given only the fields it knows.
    #[repr(C)]
    struct RulesetAttr {
        handled_access_fs: u64,
        handled_access_net: u64,
        scoped: u64,
    }

    const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1;

    /// A Landlock ruleset that handles every right the kernel's ABI knows and grants none, applied to this process;
    /// the ABI, or `None` if the kernel has no Landlock.
    fn landlock() -> Result<Option<u32>, String> {
        // SAFETY: asking for the ABI version takes a null attribute of size 0.
        let abi = unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                std::ptr::null::<RulesetAttr>(),
                0usize,
                LANDLOCK_CREATE_RULESET_VERSION,
            )
        };
        if abi < 1 {
            return match io::Error::last_os_error().raw_os_error() {
                Some(libc::ENOSYS) | Some(libc::EOPNOTSUPP) => Ok(None),
                _ => Err(os_error("landlock ABI")),
            };
        }
        let abi = abi as u32;
        // ABI 1: EXECUTE to MAKE_SYM; 2: REFER; 3: TRUNCATE; 5: IOCTL_DEV.
        let mut fs: u64 = (1 << 13) - 1;
        if abi >= 2 {
            fs |= 1 << 13;
        }
        if abi >= 3 {
            fs |= 1 << 14;
        }
        if abi >= 5 {
            fs |= 1 << 15;
        }
        let attr = RulesetAttr {
            handled_access_fs: fs,
            // ABI 4: BIND_TCP and CONNECT_TCP.
            handled_access_net: if abi >= 4 { 0b11 } else { 0 },
            // ABI 6: ABSTRACT_UNIX_SOCKET and SIGNAL.
            scoped: if abi >= 6 { 0b11 } else { 0 },
        };
        let size: usize = match abi {
            1..=3 => 8,
            4 | 5 => 16,
            _ => 24,
        };
        // SAFETY: `attr` is a ruleset attribute of at least `size` bytes, which outlives the call.
        let fd = unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                &attr as *const RulesetAttr,
                size,
                0u32,
            )
        };
        if fd < 0 {
            return Err(os_error("landlock ruleset"));
        }
        // SAFETY: `fd` is the ruleset just made; it is closed once applied.
        let rc = unsafe { libc::syscall(libc::SYS_landlock_restrict_self, fd, 0u32) };
        let error = (rc != 0).then(|| os_error("landlock restrict_self"));
        // SAFETY: closing the descriptor this function opened.
        unsafe { libc::close(fd as libc::c_int) };
        match error {
            Some(e) => Err(e),
            None => Ok(Some(abi)),
        }
    }

    /// The system calls `role` may make once confined, with their numbers. What both children need: their pipes,
    /// memory (Wasmtime's code and linear memories, and the allocator), signal handling (Wasmtime's trap handlers
    /// and their alternate stack), locks, randomness for hash maps, clocks, their own ids, and exiting. A provider
    /// also makes memory images of its modules' data segments (`memfd_create`, `ftruncate`). Written from what the
    /// standard library, glibc and Wasmtime call on these paths, not pruned by tracing the calls that succeed, so
    /// an entry may go unused.
    #[cfg(target_arch = "x86_64")]
    pub(super) fn allowed(role: Role) -> Vec<(&'static str, libc::c_long)> {
        let mut calls = vec![
            ("read", libc::SYS_read),
            ("write", libc::SYS_write),
            ("readv", libc::SYS_readv),
            ("writev", libc::SYS_writev),
            ("close", libc::SYS_close),
            ("fstat", libc::SYS_fstat),
            ("newfstatat", libc::SYS_newfstatat),
            ("statx", libc::SYS_statx),
            ("lseek", libc::SYS_lseek),
            ("fcntl", libc::SYS_fcntl),
            ("mmap", libc::SYS_mmap),
            ("mprotect", libc::SYS_mprotect),
            ("munmap", libc::SYS_munmap),
            ("mremap", libc::SYS_mremap),
            ("madvise", libc::SYS_madvise),
            ("brk", libc::SYS_brk),
            ("rt_sigaction", libc::SYS_rt_sigaction),
            ("rt_sigprocmask", libc::SYS_rt_sigprocmask),
            ("rt_sigreturn", libc::SYS_rt_sigreturn),
            ("sigaltstack", libc::SYS_sigaltstack),
            ("futex", libc::SYS_futex),
            ("sched_yield", libc::SYS_sched_yield),
            ("getrandom", libc::SYS_getrandom),
            ("clock_gettime", libc::SYS_clock_gettime),
            ("getpid", libc::SYS_getpid),
            ("gettid", libc::SYS_gettid),
            ("exit", libc::SYS_exit),
            ("exit_group", libc::SYS_exit_group),
        ];
        if role == Role::Provider {
            calls.push(("memfd_create", libc::SYS_memfd_create));
            calls.push(("ftruncate", libc::SYS_ftruncate));
        }
        calls
    }

    /// Installs the `seccomp` filter, an allow-list: a call from any architecture but x86_64, or of the x32 ABI,
    /// kills the process; an allowed call proceeds; `tgkill` proceeds only to this process itself; every other fails
    /// with `EPERM`.
    #[cfg(target_arch = "x86_64")]
    fn seccomp(role: Role) -> Result<(), String> {
        const LD_W_ABS: u16 = 0x20;
        const JEQ_K: u16 = 0x15;
        const JGE_K: u16 = 0x35;
        const RET_K: u16 = 0x06;
        const AUDIT_ARCH_X86_64: u32 = 0xC000_003E;
        const X32_SYSCALL_BIT: u32 = 0x4000_0000;
        // seccomp_data: `nr` at 0, `arch` at 4, the low half of the first argument at 16 (little-endian).
        const NR: u32 = 0;
        const ARCH: u32 = 4;
        const ARG0: u32 = 16;
        let op = |code: u16, jt: u8, jf: u8, k: u32| libc::sock_filter { code, jt, jf, k };
        let kill = libc::SECCOMP_RET_KILL_PROCESS;
        let refuse = libc::SECCOMP_RET_ERRNO | libc::EPERM as u32;
        let allow = libc::SECCOMP_RET_ALLOW;
        let calls = allowed(role);
        let n = calls.len();
        // SAFETY: getpid takes nothing. In its own PID namespace this process is 1.
        let own = unsafe { libc::getpid() } as u32;
        let mut filter = vec![
            op(LD_W_ABS, 0, 0, ARCH),
            op(JEQ_K, 1, 0, AUDIT_ARCH_X86_64),
            op(RET_K, 0, 0, kill),
            op(LD_W_ABS, 0, 0, NR),
            op(JGE_K, 0, 1, X32_SYSCALL_BIT),
            op(RET_K, 0, 0, kill),
        ];
        // After the n comparisons: [tgkill?] [refuse] [load arg0] [arg0 == own?] [refuse] [allow].
        for (i, (_, nr)) in calls.iter().enumerate() {
            let to_allow = u8::try_from(n - i + 4).map_err(|e| e.to_string())?;
            filter.push(op(JEQ_K, to_allow, 0, *nr as u32));
        }
        filter.push(op(JEQ_K, 1, 0, libc::SYS_tgkill as u32));
        filter.push(op(RET_K, 0, 0, refuse));
        filter.push(op(LD_W_ABS, 0, 0, ARG0));
        filter.push(op(JEQ_K, 1, 0, own));
        filter.push(op(RET_K, 0, 0, refuse));
        filter.push(op(RET_K, 0, 0, allow));
        let program = libc::sock_fprog {
            len: u16::try_from(filter.len()).map_err(|e| e.to_string())?,
            filter: filter.as_mut_ptr(),
        };
        // SAFETY: `program` points at `filter`, which outlives the call; the kernel copies it.
        let rc = unsafe {
            libc::prctl(
                libc::PR_SET_SECCOMP,
                libc::SECCOMP_MODE_FILTER as libc::c_ulong,
                &program as *const libc::sock_fprog as libc::c_ulong,
                0,
                0,
            )
        };
        check(rc.into(), "seccomp")
    }

    #[cfg(not(target_arch = "x86_64"))]
    fn seccomp(_role: Role) -> Result<(), String> {
        Err(
            "the seccomp allow-list is written for x86_64 only: refusing to run unconfined"
                .to_owned(),
        )
    }

    /// The jail, in order; the first step that fails stops the child.
    pub(super) fn confine(role: Role) -> Result<Applied, String> {
        // SAFETY: getpid takes nothing.
        if unsafe { libc::getpid() } != 1 {
            return Err(
                "not started in a PID namespace of its own: start it through \
                        rutent_wasm_sandbox::isolated"
                    .to_owned(),
            );
        }
        // SAFETY: plain prctl with integer arguments.
        check(
            unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) }.into(),
            "no_new_privs",
        )?;
        empty_root()?;
        super::close_from_3();
        rlimit(libc::RLIMIT_NPROC, 0, "RLIMIT_NPROC")?;
        rlimit(libc::RLIMIT_CORE, 0, "RLIMIT_CORE")?;
        if role == Role::Compiler {
            rlimit(libc::RLIMIT_AS, super::COMPILER_ADDRESS_SPACE, "RLIMIT_AS")?;
        }
        drop_capabilities()?;
        let landlock_abi = landlock()?;
        seccomp(role)?;
        Ok(Applied { landlock_abi })
    }
}

/// What a child finds it can do, tested from inside: each `_denied` is true if it held, that is, if the attempt
/// failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// No IPv4 stream socket could connect to the loopback address.
    pub network_denied: bool,
    /// The Unix-domain socket named, if any, couldn't be connected to.
    pub unix_socket_denied: Option<bool>,
    /// No file could be created in the working directory.
    pub file_write_denied: bool,
    /// The file named, if any, couldn't be read.
    pub file_read_denied: Option<bool>,
    /// No child process could be started.
    pub process_denied: bool,
    /// No signal could be sent to another process: the process named, else the parent, was sent signal 0.
    pub signal_denied: bool,
    /// Another process's information couldn't be read: `proc_pidinfo` on the parent, on macOS.
    pub process_info_denied: Option<bool>,
    /// The machine's serial number couldn't be read through IOKit, on macOS.
    pub iokit_denied: Option<bool>,
    /// No other process could be listed: on macOS `sysctl` `CTL_KERN, KERN_PROC, KERN_PROC_ALL` (its size query and
    /// its read) failed or named no process but this one; on Linux `/proc` couldn't be listed or named no other.
    pub process_list_denied: bool,
    /// Another process's arguments couldn't be read: on macOS `sysctl` `CTL_KERN, KERN_PROCARGS2` of the process
    /// named, else the parent; on Linux its `/proc/<pid>/cmdline`.
    pub process_args_denied: bool,
    /// The file descriptors above the standard three open when the probe ran: none should be.
    pub open_descriptors: Vec<i32>,
    /// The Landlock ABI the child confined itself with, on Linux, if the kernel has Landlock.
    pub landlock_abi: Option<u32>,
}

/// The open file descriptors from 3 up to [`CLOSE_UP_TO`].
pub fn open_descriptors() -> Vec<i32> {
    #[cfg(unix)]
    {
        // SAFETY: F_GETFD only reads a descriptor's flags.
        (3..CLOSE_UP_TO)
            .filter(|fd| unsafe { libc::fcntl(*fd, libc::F_GETFD) } != -1)
            .collect()
    }
    #[cfg(not(unix))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOServiceMatching(name: *const libc::c_char) -> *mut c_void;
        fn IOServiceGetMatchingService(port: u32, matching: *mut c_void) -> u32;
        fn IORegistryEntryGetProperty(
            entry: u32,
            name: *const libc::c_char,
            buffer: *mut libc::c_char,
            size: *mut u32,
        ) -> i32;
        fn IOObjectRelease(object: u32) -> i32;
    }

    /// Whether the platform's serial number could be read.
    pub(super) fn serial_number_readable() -> bool {
        // SAFETY: IOKit calls with valid C strings and a buffer of the size given; the matching dictionary is
        // consumed by IOServiceGetMatchingService.
        unsafe {
            let matching = IOServiceMatching(c"IOPlatformExpertDevice".as_ptr());
            if matching.is_null() {
                return false;
            }
            let service = IOServiceGetMatchingService(0, matching);
            if service == 0 {
                return false;
            }
            let mut buffer = [0 as libc::c_char; 128];
            let mut size = buffer.len() as u32;
            let rc = IORegistryEntryGetProperty(
                service,
                c"IOPlatformSerialNumber".as_ptr(),
                buffer.as_mut_ptr(),
                &mut size,
            );
            IOObjectRelease(service);
            rc == 0 && size > 0
        }
    }

    /// `struct kinfo_proc`'s size and `kp_proc.p_pid`'s offset in it, on 64-bit macOS (`<sys/sysctl.h>`); `libc`
    /// doesn't define the struct.
    const KINFO_PROC_SIZE: usize = 648;
    const KINFO_PROC_PID: usize = 40;

    /// Whether any process other than this one could be listed through `sysctl` `KERN_PROC_ALL`, by its size
    /// query or its read.
    pub(super) fn processes_listable() -> bool {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROC, libc::KERN_PROC_ALL, 0];
        let mut size: libc::size_t = 0;
        // SAFETY: a size query: a null buffer and a size to write.
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                std::ptr::null_mut(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        let counted = rc == 0 && size > KINFO_PROC_SIZE;
        let mut buffer = vec![0u8; size.max(KINFO_PROC_SIZE) * 2];
        let mut size = buffer.len();
        // SAFETY: the buffer is `size` bytes long.
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                buffer.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        // SAFETY: getpid takes nothing.
        let own = unsafe { libc::getpid() };
        let listed = rc == 0
            && buffer[..size.min(buffer.len())]
                .chunks_exact(KINFO_PROC_SIZE)
                .any(|entry| {
                    let pid = i32::from_ne_bytes(
                        entry[KINFO_PROC_PID..KINFO_PROC_PID + 4]
                            .try_into()
                            .unwrap_or_default(),
                    );
                    pid != own && pid != 0
                });
        counted || listed
    }

    /// Whether `pid`'s arguments could be read through `sysctl` `KERN_PROCARGS2`.
    pub(super) fn arguments_readable(pid: i32) -> bool {
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
        let mut buffer = vec![0u8; 1 << 20];
        let mut size = buffer.len();
        // SAFETY: the buffer is `size` bytes long.
        let rc = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                buffer.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        rc == 0 && size > 0
    }

    /// Whether another process's BSD information could be read.
    pub(super) fn process_info_readable(pid: i32) -> bool {
        // SAFETY: the buffer is a proc_bsdinfo of the size given.
        unsafe {
            let mut info: libc::proc_bsdinfo = std::mem::zeroed();
            let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            ) == size
        }
    }
}

/// Tests the child's restrictions from inside it. `readable` names a file it should not be able to read,
/// `unix_socket` a Unix-domain socket it should not be able to connect to, and `other` a process it should not be
/// able to signal; by default its parent, which in a PID namespace of its own it can't name.
pub fn probe(readable: Option<&Path>, unix_socket: Option<&Path>, other: Option<i32>) -> Probe {
    let open_descriptors = open_descriptors();
    let network_denied = std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], 9)),
        std::time::Duration::from_millis(200),
    )
    .map_err(|e| e.kind() != io::ErrorKind::ConnectionRefused)
    .err()
    .unwrap_or(false);
    #[cfg(unix)]
    let unix_socket_denied =
        unix_socket.map(|path| std::os::unix::net::UnixStream::connect(path).is_err());
    #[cfg(not(unix))]
    let unix_socket_denied = unix_socket.map(|_| false);
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
    let signal_denied = unsafe { libc::kill(other.unwrap_or_else(|| libc::getppid()), 0) != 0 };
    #[cfg(not(unix))]
    let signal_denied = {
        let _ = other;
        false
    };
    #[cfg(target_os = "macos")]
    // SAFETY: getppid takes nothing.
    let (process_info_denied, iokit_denied) = (
        Some(!macos::process_info_readable(unsafe { libc::getppid() })),
        Some(!macos::serial_number_readable()),
    );
    #[cfg(not(target_os = "macos"))]
    let (process_info_denied, iokit_denied) = (None, None);
    #[cfg(unix)]
    // SAFETY: getppid takes nothing.
    let target = other.unwrap_or_else(|| unsafe { libc::getppid() });
    #[cfg(target_os = "macos")]
    let (process_list_denied, process_args_denied) = (
        !macos::processes_listable(),
        !macos::arguments_readable(target),
    );
    #[cfg(not(target_os = "macos"))]
    let (process_list_denied, process_args_denied) = {
        // `/proc`, which the jail leaves unmounted: any process but this one listed, and the target's arguments.
        let own = std::process::id().to_string();
        let listed = std::fs::read_dir("/proc").is_ok_and(|entries| {
            entries.flatten().any(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name != own && !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit())
            })
        });
        #[cfg(unix)]
        let read = std::fs::read(format!("/proc/{target}/cmdline")).is_ok();
        #[cfg(not(unix))]
        let read = false;
        (!listed, !read)
    };
    #[cfg(target_os = "linux")]
    let landlock_abi = CONFINED.get().and_then(|a| a.landlock_abi);
    #[cfg(not(target_os = "linux"))]
    let landlock_abi = None;
    Probe {
        network_denied,
        unix_socket_denied,
        file_write_denied,
        file_read_denied,
        process_denied,
        signal_denied,
        process_info_denied,
        iokit_denied,
        process_list_denied,
        process_args_denied,
        open_descriptors,
        landlock_abi,
    }
}

/// The probe's report, as JSON, as both children print it for `--probe [file] [unix socket] [pid]`.
pub fn probe_report(args: &[String]) -> String {
    let path = |i: usize| args.get(i).map(Path::new);
    let other = args.get(2).and_then(|pid| pid.parse().ok());
    let p = probe(path(0), path(1), other);
    let flag = |b: Option<bool>| b.map_or("null".to_owned(), |b| b.to_string());
    format!(
        "{{\"network_denied\":{},\"unix_socket_denied\":{},\"file_write_denied\":{},\"file_read_denied\":{},\
         \"process_denied\":{},\"signal_denied\":{},\"process_info_denied\":{},\"iokit_denied\":{},\
         \"process_list_denied\":{},\"process_args_denied\":{},\
         \"open_descriptors\":{:?},\"landlock_abi\":{}}}",
        p.network_denied,
        flag(p.unix_socket_denied),
        p.file_write_denied,
        flag(p.file_read_denied),
        p.process_denied,
        p.signal_denied,
        flag(p.process_info_denied),
        flag(p.iokit_denied),
        p.process_list_denied,
        p.process_args_denied,
        p.open_descriptors,
        p.landlock_abi.map_or("null".to_owned(), |a| a.to_string()),
    )
}
