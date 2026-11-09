//! The platform a run asserts before it starts, fail closed (re-score brief, section 4): Linux x86_64 on kernel
//! 6.7 or later, and a userfaultfd that gives asynchronous write-protect, which VOS's JIT needs
//! (`pvm/runtime/src/recompiler/write_tracking.rs`). Also the facts every run note records.

/// How the nodes run the PVM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// `VOS_PVM_BACKEND=recompiler`, the JIT, forced: without write-protect the node errors instead of falling
    /// back. Every node also runs with `VOS_PVM_PERF_MAP=1`, and each must name the program in its perf map.
    Jit,
    /// `VOS_PVM_BACKEND=interpreter`: `R-09`'s interpreter leg, whose perf map must be absent or empty; and every
    /// development run off Linux x86_64, which is never scored.
    Interpreter,
}

impl Backend {
    /// The value of `VOS_PVM_BACKEND`.
    pub fn env(self) -> &'static str {
        match self {
            Backend::Jit => "recompiler",
            Backend::Interpreter => "interpreter",
        }
    }
}

/// What a run note records of the machine it ran on.
#[derive(Debug, Clone)]
pub struct Facts {
    pub os: String,
    pub kernel: String,
    pub cpu: String,
    pub vcpus: usize,
    pub memory_mib: u64,
    /// The userfaultfd probe: `Ok` when write-protect is there, else why not.
    pub uffd: Result<(), String>,
}

impl Facts {
    /// The facts of this machine.
    pub fn read() -> Facts {
        let kernel = command("uname", &["-r"]);
        let os = command("uname", &["-sm"]);
        let cpu = std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|text| {
                text.lines()
                    .find(|l| l.starts_with("model name"))
                    .and_then(|l| l.split_once(':'))
                    .map(|(_, v)| v.trim().to_string())
            })
            .unwrap_or_else(|| command("sysctl", &["-n", "machdep.cpu.brand_string"]));
        let vcpus = std::thread::available_parallelism().map_or(1, |n| n.get());
        let memory_mib = std::fs::read_to_string("/proc/meminfo")
            .ok()
            .and_then(|text| {
                text.lines()
                    .find(|l| l.starts_with("MemTotal:"))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|kib| kib.parse::<u64>().ok())
                    .map(|kib| kib / 1024)
            })
            .or_else(|| {
                command("sysctl", &["-n", "hw.memsize"])
                    .parse::<u64>()
                    .ok()
                    .map(|b| b >> 20)
            })
            .unwrap_or(0);
        Facts {
            os,
            kernel,
            cpu,
            vcpus,
            memory_mib,
            uffd: uffd_probe(),
        }
    }

    /// The kernel's version as (major, minor), if `uname -r` reads as one.
    pub fn kernel_version(&self) -> Option<(u32, u32)> {
        let mut parts = self.kernel.split(['.', '-']);
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        Some((major, minor))
    }

    /// Fails unless this machine can run the JIT: Linux x86_64, kernel 6.7 or later, and the userfaultfd probe.
    pub fn assert_jit(&self) -> Result<(), String> {
        if self.os != "Linux x86_64" {
            return Err(format!("the JIT needs Linux x86_64, not {}", self.os));
        }
        match self.kernel_version() {
            Some(v) if v >= (6, 7) => {}
            _ => {
                return Err(format!(
                    "the JIT needs kernel 6.7 or later, not {}",
                    self.kernel
                ));
            }
        }
        self.uffd
            .clone()
            .map_err(|e| format!("the userfaultfd probe failed: {e}"))
    }

    /// The facts as the rows of a run note's table.
    pub fn markdown(&self) -> String {
        format!(
            "| OS, architecture | {} |\n| Kernel | {} |\n| CPU | {} |\n| vCPUs | {} |\n| Memory | {} MiB |\n| userfaultfd write-protect probe | {} |\n",
            self.os,
            self.kernel,
            self.cpu,
            self.vcpus,
            self.memory_mib,
            match &self.uffd {
                Ok(()) => "succeeded".to_string(),
                Err(e) => format!("failed: {e}"),
            }
        )
    }
}

fn command(program: &str, args: &[&str]) -> String {
    std::process::Command::new(program)
        .args(args)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Opens a userfaultfd with `UFFD_USER_MODE_ONLY` and asks for the features VOS's write tracking asks for,
/// asynchronous write-protect included, then opens `/proc/self/pagemap`, as
/// `pvm/runtime/src/recompiler/write_tracking.rs` does.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn uffd_probe() -> Result<(), String> {
    const UFFD_USER_MODE_ONLY: libc::c_int = 1;
    const UFFD_API: u64 = 0xaa;
    const UFFD_FEATURE_WP_HUGETLBFS_SHMEM: u64 = 1 << 12;
    const UFFD_FEATURE_WP_ASYNC: u64 = 1 << 15;
    const UFFDIO_API: libc::Ioctl = 0xc018_aa3f_u32 as libc::Ioctl;
    #[repr(C)]
    struct UffdioApi {
        api: u64,
        features: u64,
        ioctls: u64,
    }
    let error = |what: &str| format!("{what}: {}", std::io::Error::last_os_error());
    // SAFETY: syscalls with checked results, on a struct laid out as the kernel's uapi; both descriptors are closed.
    unsafe {
        let uffd = libc::syscall(
            libc::SYS_userfaultfd,
            libc::O_CLOEXEC | libc::O_NONBLOCK | UFFD_USER_MODE_ONLY,
        ) as libc::c_int;
        if uffd < 0 {
            return Err(error("userfaultfd(UFFD_USER_MODE_ONLY)"));
        }
        let features = UFFD_FEATURE_WP_ASYNC | UFFD_FEATURE_WP_HUGETLBFS_SHMEM;
        let mut api = UffdioApi {
            api: UFFD_API,
            features,
            ioctls: 0,
        };
        let asked = libc::ioctl(uffd, UFFDIO_API, &mut api);
        let reason = error("UFFDIO_API with asynchronous write-protect");
        libc::close(uffd);
        if asked != 0 || api.features & features != features {
            return Err(reason);
        }
        let pagemap = libc::open(
            c"/proc/self/pagemap".as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC,
        );
        if pagemap < 0 {
            return Err(error("/proc/self/pagemap"));
        }
        libc::close(pagemap);
    }
    Ok(())
}

/// Off Linux x86_64 there is no userfaultfd, and no JIT.
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
pub fn uffd_probe() -> Result<(), String> {
    Err("no userfaultfd off Linux x86_64".into())
}
