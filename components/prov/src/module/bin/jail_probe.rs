//! A hostile probe, run inside a jail of chosen layers (`T-025-02`). Test-only: built only with the `jail-layers`
//! feature. `rutent-jail-probe <layers> <probe> [argument]` confines itself under exactly `<layers>`
//! (comma-separated `jail-layer:` names) and then tries one hostile act, printing one line: `<probe> <result>`.
//! A probe that can't establish its jail exits 3 without printing a result, so no result is ever vacuous.

use std::process::ExitCode;

#[cfg(target_os = "linux")]
fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

#[cfg(target_os = "linux")]
fn name(errno: i32) -> &'static str {
    match errno {
        libc::EPERM => "EPERM",
        libc::EACCES => "EACCES",
        libc::ENOENT => "ENOENT",
        libc::EINVAL => "EINVAL",
        libc::ESRCH => "ESRCH",
        libc::ENOMEM => "ENOMEM",
        libc::ENETUNREACH => "ENETUNREACH",
        libc::ECONNREFUSED => "ECONNREFUSED",
        libc::EIDRM => "EIDRM",
        libc::EFBIG => "EFBIG",
        libc::EAGAIN => "EAGAIN",
        _ => "OTHER",
    }
}

#[cfg(target_os = "linux")]
fn verdict(ok: bool) -> String {
    if ok {
        "allowed".to_owned()
    } else {
        format!("refused {}", name(errno()))
    }
}

#[cfg(target_os = "linux")]
fn run(probe: &str, argument: Option<&str>) -> Result<String, String> {
    use std::net::{SocketAddr, TcpStream};
    use std::os::unix::fs::MetadataExt;
    use std::time::Duration;
    let number = |text: Option<&str>| -> Result<i64, String> {
        text.ok_or("the probe needs an argument")?
            .parse()
            .map_err(|e| format!("{e}"))
    };
    Ok(match probe {
        "host-users" => {
            let owner = std::fs::metadata("/etc/passwd")
                .map_err(|e| e.to_string())?
                .uid();
            // SAFETY: setresuid takes three integers; 1 is a host user the jail has no mapping for.
            let switched = unsafe { libc::setresuid(1, 1, 1) } == 0;
            format!("owner={owner} switch={}", verdict(switched))
        }
        "network" => {
            let port = number(argument)? as u16;
            let addr = SocketAddr::from(([127, 0, 0, 1], port));
            match TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
                Ok(_) => "allowed".to_owned(),
                Err(e) => format!("refused {}", name(e.raw_os_error().unwrap_or(0))),
            }
        }
        "ipc" => {
            let id = number(argument)? as libc::c_int;
            // SAFETY: shmat of an id the parent made; the mapping is detached at once.
            let at = unsafe { libc::shmat(id, std::ptr::null(), 0) };
            if at as isize == -1 {
                format!("refused {}", name(errno()))
            } else {
                // SAFETY: `at` is the address shmat returned.
                unsafe { libc::shmdt(at) };
                "allowed".to_owned()
            }
        }
        "uts" => {
            let wanted = b"rutent-jail-probe";
            // SAFETY: a pointer and the length of a byte string.
            let changed = unsafe { libc::sethostname(wanted.as_ptr().cast(), wanted.len()) } == 0;
            let mut buf = [0u8; 64];
            // SAFETY: a buffer and its length.
            unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
            let seen =
                String::from_utf8_lossy(&buf[..buf.iter().position(|b| *b == 0).unwrap_or(0)])
                    .into_owned();
            format!("{} hostname={seen}", verdict(changed))
        }
        "signal" => {
            let pid = number(argument)? as libc::pid_t;
            // SAFETY: kill takes two integers.
            let sent = unsafe { libc::kill(pid, libc::SIGTERM) } == 0;
            if sent {
                "allowed".to_owned()
            } else {
                format!("refused {}", name(errno()))
            }
        }
        "root" => {
            let entries = std::fs::read_dir("/")
                .map(|d| d.count())
                .map_err(|e| e.to_string())?;
            let passwd = std::fs::read("/etc/passwd").is_ok();
            format!(
                "entries={entries} passwd={}",
                if passwd { "readable" } else { "absent" }
            )
        }
        "privilege" => {
            let out = std::process::Command::new("/usr/bin/sudo")
                .args(["-n", "/usr/bin/id", "-u"])
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output()
                .map_err(|e| e.to_string())?;
            let uid = String::from_utf8_lossy(&out.stdout).trim().to_owned();
            if out.status.success() && uid == "0" {
                "allowed euid=0".to_owned()
            } else {
                "refused".to_owned()
            }
        }
        "capability" => {
            // SAFETY: chroot to the working directory, a NUL-terminated path.
            let ok = unsafe { libc::chroot(c".".as_ptr()) } == 0;
            verdict(ok)
        }
        "filesystem" => match std::fs::read("/etc/passwd") {
            Ok(_) => "allowed".to_owned(),
            Err(e) => format!("refused {}", name(e.raw_os_error().unwrap_or(0))),
        },
        "syscall" => {
            // SAFETY: socket(AF_INET, SOCK_STREAM, 0) by number, which no allow-list entry covers.
            let fd =
                unsafe { libc::syscall(libc::SYS_socket, libc::AF_INET, libc::SOCK_STREAM, 0) };
            if fd >= 0 {
                // SAFETY: closing the descriptor just made.
                unsafe { libc::close(fd as libc::c_int) };
                "allowed".to_owned()
            } else {
                format!("refused {}", name(errno()))
            }
        }
        "descriptors" => {
            format!("fds={:?}", rutent_prov::module::sandbox::open_descriptors())
        }
        "address-space" => {
            let size = number(argument)? as usize;
            let mut limit = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            // SAFETY: `limit` is valid for the call.
            unsafe { libc::getrlimit(libc::RLIMIT_AS, &mut limit) };
            let map = |size: usize| {
                // SAFETY: an anonymous, inaccessible, unreserved mapping, unmapped at once if made.
                unsafe {
                    let at = libc::mmap(
                        std::ptr::null_mut(),
                        size,
                        libc::PROT_NONE,
                        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_NORESERVE,
                        -1,
                        0,
                    );
                    if at == libc::MAP_FAILED {
                        format!("refused {}", name(errno()))
                    } else {
                        libc::munmap(at, size);
                        "allowed".to_owned()
                    }
                }
            };
            let limited = limit.rlim_cur != libc::RLIM_INFINITY;
            format!(
                "limit={} small={} past={}",
                if limited { limit.rlim_cur } else { 0 },
                map(4096),
                map(size)
            )
        }
        other => return Err(format!("no such probe: {other}")),
    })
}

#[cfg(target_os = "linux")]
fn main() -> ExitCode {
    use rutent_prov::module::jail::{Layers, confine};
    use rutent_prov::module::sandbox::Role;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(layers), Some(probe)) = (args.first(), args.get(1)) else {
        eprintln!("usage: rutent-jail-probe <layers> <probe> [argument]");
        return ExitCode::from(3);
    };
    let layers = match Layers::parse(layers) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(3);
        }
    };
    if let Err(e) = confine(Role::Provider, layers) {
        eprintln!("jail not established: {e}");
        return ExitCode::from(3);
    }
    match run(probe, args.get(2).map(String::as_str)) {
        Ok(result) => {
            println!("{probe} {result}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("probe failed to run: {e}");
            ExitCode::from(3)
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn main() -> ExitCode {
    eprintln!("rutent-jail-probe: the layers it probes are Linux's; nothing to run here");
    ExitCode::from(3)
}
