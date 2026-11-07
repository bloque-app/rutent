//! Linux only: how a process started by `isolation::spawn` confines itself, inside the namespaces `unshare`
//! gave it, before it reads any input. See `isolation` for what each role gets. Written for the scored run on
//! `x86_64-unknown-linux-gnu`; not yet run.

use std::ffi::CString;

use crate::isolation::Role;

fn cstr(text: &str) -> Result<CString, String> {
    CString::new(text).map_err(|e| e.to_string())
}

fn check(result: libc::c_int, what: &str) -> Result<(), String> {
    if result == 0 {
        Ok(())
    } else {
        Err(format!("{what}: {}", std::io::Error::last_os_error()))
    }
}

fn mount(
    source: &str,
    target: &str,
    fstype: Option<&str>,
    flags: libc::c_ulong,
    data: Option<&str>,
) -> Result<(), String> {
    let (source, target) = (cstr(source)?, cstr(target)?);
    let fstype = fstype.map(cstr).transpose()?;
    let data = data.map(cstr).transpose()?;
    // SAFETY: every pointer is to a NUL-terminated string that outlives the call, or null.
    let result = unsafe {
        libc::mount(
            source.as_ptr(),
            target.as_ptr(),
            fstype.as_ref().map_or(std::ptr::null(), |f| f.as_ptr()),
            flags,
            data.as_ref()
                .map_or(std::ptr::null(), |d| d.as_ptr().cast()),
        )
    };
    check(result, &format!("mount {}", target.to_string_lossy()))
}

fn rlimit(resource: libc::__rlimit_resource_t, value: libc::rlim_t) -> Result<(), String> {
    let limit = libc::rlimit {
        rlim_cur: value,
        rlim_max: value,
    };
    // SAFETY: `limit` is a valid rlimit.
    check(unsafe { libc::setrlimit(resource, &limit) }, "setrlimit")
}

/// `mount_setattr(2)`'s argument, which `libc` doesn't define for glibc.
#[repr(C)]
struct MountAttr {
    attr_set: u64,
    attr_clr: u64,
    propagation: u64,
    userns_fd: u64,
}

const MOUNT_ATTR_RDONLY: u64 = 0x1;
const MOUNT_ATTR_NOSUID: u64 = 0x2;
const MOUNT_ATTR_NODEV: u64 = 0x4;
const AT_RECURSIVE: libc::c_uint = 0x8000;

/// Sets and clears mount attributes on `path` and every mount under it (`mount_setattr`, Linux 5.12 or later).
fn set_attributes(path: &str, set: u64, clear: u64) -> Result<(), String> {
    let target = cstr(path)?;
    let attr = MountAttr {
        attr_set: set,
        attr_clr: clear,
        propagation: 0,
        userns_fd: 0,
    };
    // SAFETY: the path is NUL-terminated, and `attr` is a valid `mount_attr` of the size passed.
    let result = unsafe {
        libc::syscall(
            libc::SYS_mount_setattr,
            libc::AT_FDCWD,
            target.as_ptr(),
            AT_RECURSIVE,
            &attr as *const MountAttr,
            std::mem::size_of::<MountAttr>(),
        )
    };
    check(i32::from(result != 0), &format!("mount_setattr {path}"))
}

/// Makes every mount read-only, `nosuid` and `nodev`, but `/proc`, which PolkaVM's sandbox child writes its own ID
/// maps through, and which stays writable.
fn read_only_everywhere() -> Result<(), String> {
    set_attributes(
        "/",
        MOUNT_ATTR_RDONLY | MOUNT_ATTR_NOSUID | MOUNT_ATTR_NODEV,
        0,
    )?;
    set_attributes("/proc", 0, MOUNT_ATTR_RDONLY)
}

/// The capability sets `capset(2)` takes, which `libc` doesn't define.
#[repr(C)]
struct CapHeader {
    version: u32,
    pid: libc::c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CapData {
    effective: u32,
    permitted: u32,
    inheritable: u32,
}

/// Drops every capability this process holds in its user namespace: effective, permitted and inheritable, and
/// the ambient set. It keeps the bounding set, which PolkaVM's sandbox needs for the processes it starts in user
/// namespaces of their own.
fn drop_capabilities() -> Result<(), String> {
    const LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;
    let header = CapHeader {
        version: LINUX_CAPABILITY_VERSION_3,
        pid: 0,
    };
    let data = [CapData {
        effective: 0,
        permitted: 0,
        inheritable: 0,
    }; 2];
    // SAFETY: the header and two data words are the layout `capset` takes for version 3.
    let result =
        unsafe { libc::syscall(libc::SYS_capset, &header as *const CapHeader, data.as_ptr()) };
    check(i32::from(result != 0), "capset")?;
    // SAFETY: plain prctl with integer arguments.
    check(
        unsafe {
            libc::prctl(
                libc::PR_CAP_AMBIENT,
                libc::PR_CAP_AMBIENT_CLEAR_ALL,
                0,
                0,
                0,
            )
        },
        "clear ambient capabilities",
    )
}

/// Moves the root to an empty `tmpfs` and makes it read-only.
fn empty_root() -> Result<(), String> {
    mount(
        "none",
        "/tmp",
        Some("tmpfs"),
        libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC,
        Some("size=64k,mode=0755"),
    )?;
    let old = cstr("/tmp/old")?;
    // SAFETY: NUL-terminated path.
    check(
        unsafe { libc::mkdir(old.as_ptr(), 0o700) },
        "mkdir /tmp/old",
    )?;
    let (new_root, put_old) = (cstr("/tmp")?, cstr("/tmp/old")?);
    // SAFETY: both paths are NUL-terminated; `/tmp` is a mount point and `/tmp/old` is under it.
    let result =
        unsafe { libc::syscall(libc::SYS_pivot_root, new_root.as_ptr(), put_old.as_ptr()) };
    check(i32::from(result != 0), "pivot_root")?;
    let (slash, old) = (cstr("/")?, cstr("/old")?);
    // SAFETY: NUL-terminated paths.
    check(unsafe { libc::chdir(slash.as_ptr()) }, "chdir /")?;
    // SAFETY: NUL-terminated path.
    check(
        unsafe { libc::umount2(old.as_ptr(), libc::MNT_DETACH) },
        "umount /old",
    )?;
    // SAFETY: NUL-terminated path.
    check(unsafe { libc::rmdir(old.as_ptr()) }, "rmdir /old")?;
    mount(
        "none",
        "/",
        None,
        libc::MS_REMOUNT | libc::MS_RDONLY | libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC,
        None,
    )
}

/// Hides each existing directory under an empty, read-only `tmpfs`.
fn hide(paths: &[String]) -> Result<(), String> {
    for path in paths {
        if std::path::Path::new(path).is_dir() {
            mount(
                "none",
                path,
                Some("tmpfs"),
                libc::MS_RDONLY | libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC,
                Some("size=0,mode=0555"),
            )?;
        }
    }
    Ok(())
}

/// The system calls the admitting compiler may make; every other fails with `EPERM`.
#[cfg(target_arch = "x86_64")]
const ADMIT_SYSCALLS: [libc::c_long; 36] = [
    libc::SYS_read,
    libc::SYS_write,
    libc::SYS_readv,
    libc::SYS_writev,
    libc::SYS_pread64,
    libc::SYS_close,
    libc::SYS_fstat,
    libc::SYS_newfstatat,
    libc::SYS_statx,
    libc::SYS_lseek,
    libc::SYS_poll,
    libc::SYS_fcntl,
    libc::SYS_mmap,
    libc::SYS_mprotect,
    libc::SYS_munmap,
    libc::SYS_mremap,
    libc::SYS_madvise,
    libc::SYS_brk,
    libc::SYS_memfd_create,
    libc::SYS_ftruncate,
    libc::SYS_rt_sigaction,
    libc::SYS_rt_sigprocmask,
    libc::SYS_rt_sigreturn,
    libc::SYS_sigaltstack,
    libc::SYS_futex,
    libc::SYS_sched_yield,
    libc::SYS_sched_getaffinity,
    libc::SYS_getrandom,
    libc::SYS_clock_gettime,
    libc::SYS_nanosleep,
    libc::SYS_getpid,
    libc::SYS_gettid,
    libc::SYS_tgkill,
    libc::SYS_rseq,
    libc::SYS_exit,
    libc::SYS_exit_group,
];

/// Installs the admitting compiler's `seccomp` filter: the architecture must be `x86_64` and not the x32 ABI,
/// or the process is killed; an allowed call proceeds; any other fails with `EPERM`.
#[cfg(target_arch = "x86_64")]
fn seccomp() -> Result<(), String> {
    const LD_W_ABS: u16 = 0x20;
    const JEQ_K: u16 = 0x15;
    const JGE_K: u16 = 0x35;
    const RET_K: u16 = 0x06;
    const AUDIT_ARCH_X86_64: u32 = 0xc000_003e;
    const X32_SYSCALL_BIT: u32 = 0x4000_0000;
    const RET_KILL_PROCESS: u32 = 0x8000_0000;
    const RET_ALLOW: u32 = 0x7fff_0000;
    const RET_ERRNO: u32 = 0x0005_0000;
    let op = |code: u16, jt: u8, jf: u8, k: u32| libc::sock_filter { code, jt, jf, k };
    let n = ADMIT_SYSCALLS.len();
    let mut filter = vec![
        op(LD_W_ABS, 0, 0, 4),
        op(JEQ_K, 1, 0, AUDIT_ARCH_X86_64),
        op(RET_K, 0, 0, RET_KILL_PROCESS),
        op(LD_W_ABS, 0, 0, 0),
        op(JGE_K, 0, 1, X32_SYSCALL_BIT),
        op(RET_K, 0, 0, RET_KILL_PROCESS),
    ];
    for (i, call) in ADMIT_SYSCALLS.iter().enumerate() {
        // An allowed call jumps over the remaining comparisons and the refusal, to the allow.
        let skip = u8::try_from(n - i).map_err(|e| e.to_string())?;
        filter.push(op(
            JEQ_K,
            skip,
            0,
            u32::try_from(*call).map_err(|e| e.to_string())?,
        ));
    }
    filter.push(op(RET_K, 0, 0, RET_ERRNO | libc::EPERM as u32));
    filter.push(op(RET_K, 0, 0, RET_ALLOW));
    let program = libc::sock_fprog {
        len: u16::try_from(filter.len()).map_err(|e| e.to_string())?,
        filter: filter.as_mut_ptr(),
    };
    // SAFETY: `program` points at `filter`, which outlives the call; the kernel copies it.
    let result = unsafe {
        libc::prctl(
            libc::PR_SET_SECCOMP,
            libc::SECCOMP_MODE_FILTER,
            &program as *const libc::sock_fprog,
        )
    };
    check(result, "seccomp")
}

#[cfg(not(target_arch = "x86_64"))]
fn seccomp() -> Result<(), String> {
    Err("the admitting compiler's seccomp filter is written for x86_64 only".to_owned())
}

/// Confines this process as `role`; `hide` names more directories a worker hides.
pub fn enter(role: Role, hide_also: &[String], skip: &[String]) -> Result<(), String> {
    let skipped = |step: &str| skip.iter().any(|s| s == step);
    // SAFETY: plain prctl with integer arguments.
    check(
        unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) },
        "no_new_privs",
    )?;
    mount("none", "/", None, libc::MS_REC | libc::MS_PRIVATE, None)?;
    rlimit(libc::RLIMIT_CORE, 0)?;
    match role {
        Role::Admit => {
            empty_root()?;
            rlimit(libc::RLIMIT_NOFILE, 64)?;
            rlimit(libc::RLIMIT_NPROC, 0)?;
            rlimit(libc::RLIMIT_AS, 8 << 30)?;
            seccomp()
        }
        Role::Worker => {
            let mut paths: Vec<String> = [
                "/home", "/root", "/tmp", "/run", "/var/run", "/mnt", "/media", "/srv",
            ]
            .iter()
            .map(|p| (*p).to_owned())
            .collect();
            paths.extend(hide_also.iter().cloned());
            if !skipped("hide") {
                hide(&paths)?;
            }
            if !skipped("read-only") {
                read_only_everywhere()?;
            }
            if !skipped("nofile") {
                rlimit(libc::RLIMIT_NOFILE, 1024)?;
            }
            if skipped("capabilities") {
                return Ok(());
            }
            drop_capabilities()
        }
    }
}
