//! The platform a run asserts before it starts, fail closed (re-score brief, section 4): Linux x86_64 on kernel
//! 6.7 or later, and a userfaultfd that gives asynchronous write-protect, which VOS's JIT needs
//! (`pvm/runtime/src/recompiler/write_tracking.rs`). Also the facts every run note records.

use std::path::Path;

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

/// The name the PVM's perf map gives a program it compiles without registered functions
/// (`pvm/runtime/src/perf_map.rs`, `program_name`): `pvm:<first 8 bytes of BLAKE2b-256 of its code, hex>:<code
/// length>`, for the program the node derives from `elf`, as `vos_pvm_compiler::link_elf_spi` links it.
pub fn perf_map_name(elf: &[u8]) -> Result<String, String> {
    let blob =
        vos_pvm_compiler::link_elf_spi(elf).map_err(|e| format!("linking the program: {e:?}"))?;
    let program = vos_pvm::spi::parse_standard_program(&blob)
        .ok_or_else(|| "the linked program is not a standard program".to_string())?;
    let code = &program.code.code;
    let hash = vos_pvm::precompile::blake2b_256(code);
    let hex: String = hash[..8].iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("pvm:{hex}:{}", code.len()))
}

/// What a node's perf map shows: its line count, and how many name the program.
pub fn perf_map_lines(pid: u32, program: &str) -> (usize, usize) {
    let path = format!("/tmp/perf-{pid}.map");
    let text = std::fs::read_to_string(Path::new(&path)).unwrap_or_default();
    let lines = text.lines().count();
    let naming = text.lines().filter(|l| l.contains(program)).count();
    (lines, naming)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(os: &str, kernel: &str, uffd: Result<(), String>) -> Facts {
        Facts {
            os: os.into(),
            kernel: kernel.into(),
            cpu: "a CPU".into(),
            vcpus: 4,
            memory_mib: 16_000,
            uffd,
        }
    }

    // verifies: DEC-6
    #[test]
    fn linux_x86_64_on_kernel_6_7_or_later_with_write_protect_passes_the_assertion() {
        assert_eq!(
            machine("Linux x86_64", "6.7.0-1-azure", Ok(())).assert_jit(),
            Ok(())
        );
        assert_eq!(
            machine("Linux x86_64", "7.0.0-1019-azure", Ok(())).assert_jit(),
            Ok(())
        );
    }

    // verifies: DEC-6
    #[test]
    fn a_kernel_before_6_7_fails_the_assertion() {
        let refused = machine("Linux x86_64", "6.6.30-generic", Ok(())).assert_jit();
        assert!(refused.is_err_and(|e| e.contains("6.7")));
    }

    // verifies: DEC-6
    #[test]
    fn a_machine_off_linux_x86_64_fails_the_assertion() {
        assert!(
            machine("Darwin arm64", "25.6.0", Ok(()))
                .assert_jit()
                .is_err()
        );
        assert!(
            machine("Linux aarch64", "6.8.0", Ok(()))
                .assert_jit()
                .is_err()
        );
    }

    // verifies: DEC-6
    #[test]
    fn a_failed_userfaultfd_probe_fails_the_assertion() {
        let refused = machine("Linux x86_64", "6.8.0", Err("EPERM".into())).assert_jit();
        assert!(refused.is_err_and(|e| e.contains("userfaultfd")));
    }

    // verifies: DEC-6
    #[test]
    fn a_kernel_release_that_does_not_read_as_a_version_fails_the_assertion() {
        assert!(
            machine("Linux x86_64", "unknown", Ok(()))
                .assert_jit()
                .is_err()
        );
    }
}
