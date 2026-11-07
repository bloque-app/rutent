//! The isolation probes (`tests/isolation.rs`, scoring sheet `B-08`). `rutent-pvm-worker` and `rutent-pvm-admit`
//! each take `--probe <what> <argument>`. They run it inside their isolation, in place of their work, and print one
//! line: `probe <what>: allowed`, or `probe <what>: refused: <error>`. A probe does nothing but try, and the
//! test reads the line.
//!
//! - `read <path>`: read a file.
//! - `signal <pid>`: send `SIGTERM` to another process.
//! - `connect <address:port>`: open a TCP connection.
//! - `write <path>`: create a file.
//! - `control -`: nothing, so the test knows the program ran inside its isolation.
//! - `pid -`: print its own process id, which is 1 in a new PID namespace.
//! - `status -`: print `NoNewPrivs`, and the soft limits on open files and core files, from `/proc/self` (Linux).
//! - `socket -`: open a Unix socket, which a new network namespace allows and only a `seccomp` filter refuses.
//! - `readfd <n>`: read from descriptor `n`, as the program inherited it.
//! - `unix <path>`: connect to the Unix socket at `path`.
//! - `procinfo <pid>`: read another process's information: its program's path (macOS `proc_pidpath`), or its
//!   `/proc/<pid>/status` (Linux).
//! - `iokit -`: read the machine's serial number from IOKit (macOS; IOKit is loaded only for this probe).
//! - `sysv -`: create, then remove, an `IPC_PRIVATE` System V shared-memory segment.
//! - `stderr <text>`: write `text` to standard error.
//! - `sandbox -`: compile the PolkaVM blob on standard input and instantiate it, with the binding's engine
//!   settings: under the compiler, in PolkaVM's Linux sandbox. It shows the guest sandbox works inside the
//!   program's own isolation.

use std::io::Write;

/// Runs a probe if the arguments ask for one; the exit code, or `None` if they don't.
pub fn run(args: &[String]) -> Option<i32> {
    let [flag, what, argument, ..] = args else {
        return None;
    };
    if flag != "--probe" {
        return None;
    }
    let outcome: Result<String, String> = match what.as_str() {
        "read" => std::fs::read(argument)
            .map(|_| String::new())
            .map_err(|e| e.to_string()),
        "signal" => match argument.parse::<libc::pid_t>() {
            // SAFETY: `kill` takes a process id and a signal number.
            Ok(pid) => match unsafe { libc::kill(pid, libc::SIGTERM) } {
                0 => Ok(String::new()),
                _ => Err(std::io::Error::last_os_error().to_string()),
            },
            Err(e) => Err(e.to_string()),
        },
        "connect" => std::net::TcpStream::connect(argument)
            .map(|_| String::new())
            .map_err(|e| e.to_string()),
        "write" => std::fs::write(argument, b"probe")
            .map(|_| String::new())
            .map_err(|e| e.to_string()),
        "control" => Ok(String::new()),
        "pid" => Ok(format!(" {}", std::process::id())),
        "status" => status(),
        "socket" => {
            // SAFETY: `socket` takes three integers and returns a descriptor or -1.
            let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
            if fd < 0 {
                Err(std::io::Error::last_os_error().to_string())
            } else {
                // SAFETY: `fd` is the descriptor just opened.
                unsafe { libc::close(fd) };
                Ok(String::new())
            }
        }
        "readfd" => match argument.parse::<libc::c_int>() {
            Ok(fd) => {
                let mut buf = [0u8; 16];
                // SAFETY: reads at most the buffer's length into it.
                match unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) } {
                    n if n >= 0 => Ok(String::new()),
                    _ => Err(std::io::Error::last_os_error().to_string()),
                }
            }
            Err(e) => Err(e.to_string()),
        },
        "unix" => std::os::unix::net::UnixStream::connect(argument)
            .map(|_| String::new())
            .map_err(|e| e.to_string()),
        "procinfo" => procinfo(argument),
        "iokit" => iokit(),
        "sysv" => {
            // SAFETY: plain System V calls with integer arguments.
            let id = unsafe { libc::shmget(libc::IPC_PRIVATE, 4096, libc::IPC_CREAT | 0o600) };
            if id < 0 {
                Err(std::io::Error::last_os_error().to_string())
            } else {
                // SAFETY: removes the segment just created.
                unsafe { libc::shmctl(id, libc::IPC_RMID, std::ptr::null_mut()) };
                Ok(String::new())
            }
        }
        "sandbox" => sandbox(),
        "stderr" => std::io::stderr()
            .write_all(argument.as_bytes())
            .map(|_| String::new())
            .map_err(|e| e.to_string()),
        _ => Err("unknown probe".to_owned()),
    };
    let line = match outcome {
        Ok(extra) => format!("probe {what}: allowed{extra}\n"),
        Err(e) => format!("probe {what}: refused: {e}\n"),
    };
    let _ = std::io::stdout().write_all(line.as_bytes());
    Some(0)
}

/// `NoNewPrivs`, and the soft limits on open files and core files, as `/proc/self` states them.
fn status() -> Result<String, String> {
    let status = std::fs::read_to_string("/proc/self/status").map_err(|e| e.to_string())?;
    let limits = std::fs::read_to_string("/proc/self/limits").map_err(|e| e.to_string())?;
    let no_new_privs = status
        .lines()
        .find_map(|l| l.strip_prefix("NoNewPrivs:"))
        .map(str::trim)
        .unwrap_or("?");
    let soft = |name: &str| {
        limits
            .lines()
            .find_map(|l| l.strip_prefix(name))
            .and_then(|rest| rest.split_whitespace().next())
            .unwrap_or("?")
            .to_owned()
    };
    Ok(format!(
        " NoNewPrivs={no_new_privs} nofile={} core={}",
        soft("Max open files"),
        soft("Max core file size")
    ))
}

#[cfg(target_os = "macos")]
fn procinfo(argument: &str) -> Result<String, String> {
    let pid = argument.parse::<libc::c_int>().map_err(|e| e.to_string())?;
    let mut path = [0u8; 4096];
    // SAFETY: the buffer is its length in bytes.
    let n = unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), path.len() as u32) };
    if n > 0 {
        Ok(String::new())
    } else {
        Err(std::io::Error::last_os_error().to_string())
    }
}

#[cfg(not(target_os = "macos"))]
fn procinfo(argument: &str) -> Result<String, String> {
    std::fs::read(format!("/proc/{argument}/status"))
        .map(|_| String::new())
        .map_err(|e| e.to_string())
}

/// Reads `IOPlatformSerialNumber` through IOKit and CoreFoundation, loaded with `dlopen` for this probe only, so
/// the programs never load either otherwise.
#[cfg(target_os = "macos")]
fn iokit() -> Result<String, String> {
    use std::ffi::{CStr, c_char, c_void};
    type Matching = unsafe extern "C" fn(*const c_char) -> *mut c_void;
    type Service = unsafe extern "C" fn(u32, *mut c_void) -> u32;
    type Property = unsafe extern "C" fn(u32, *const c_void, *const c_void, u32) -> *const c_void;
    type MakeString = unsafe extern "C" fn(*const c_void, *const c_char, u32) -> *const c_void;
    const UTF8: u32 = 0x0800_0100;
    let open = |path: &CStr| {
        // SAFETY: the path is NUL-terminated.
        let handle = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_NOW) };
        (!handle.is_null())
            .then_some(handle)
            .ok_or_else(|| format!("dlopen {path:?} failed"))
    };
    let symbol = |handle: *mut c_void, name: &CStr| {
        // SAFETY: the handle is open and the name NUL-terminated.
        let found = unsafe { libc::dlsym(handle, name.as_ptr()) };
        (!found.is_null())
            .then_some(found)
            .ok_or_else(|| format!("no {name:?}"))
    };
    let iokit = open(c"/System/Library/Frameworks/IOKit.framework/IOKit")?;
    let cf = open(c"/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")?;
    // SAFETY: each symbol has the C signature it is cast to, as the macOS SDK declares it.
    unsafe {
        let matching: Matching = std::mem::transmute(symbol(iokit, c"IOServiceMatching")?);
        let service: Service = std::mem::transmute(symbol(iokit, c"IOServiceGetMatchingService")?);
        let property: Property =
            std::mem::transmute(symbol(iokit, c"IORegistryEntryCreateCFProperty")?);
        let make_string: MakeString =
            std::mem::transmute(symbol(cf, c"CFStringCreateWithCString")?);
        let entry = service(0, matching(c"IOPlatformExpertDevice".as_ptr()));
        if entry == 0 {
            return Err("no platform expert device".to_owned());
        }
        let key = make_string(std::ptr::null(), c"IOPlatformSerialNumber".as_ptr(), UTF8);
        if property(entry, key, std::ptr::null(), 0).is_null() {
            Err("no serial number".to_owned())
        } else {
            Ok(String::new())
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn iokit() -> Result<String, String> {
    Err("no IOKit on this platform".to_owned())
}

fn sandbox() -> Result<String, String> {
    use std::io::Read;
    let mut blob = Vec::new();
    std::io::stdin()
        .read_to_end(&mut blob)
        .map_err(|e| e.to_string())?;
    let engine =
        polkavm::Engine::new(&crate::engine::config(1)).map_err(|e| format!("engine: {e}"))?;
    let program = crate::engine::check(
        rutent_contracts::module_interface::PartName::Sandboxed,
        &blob,
    )?;
    let (module, compiled) = crate::engine::compile(&engine, &blob, program)?;
    let instance = module
        .instantiate()
        .map_err(|e| format!("instantiate: {e}"))?;
    Ok(format!(
        " backend={:?} machine_code={} sandbox_pid={:?}",
        engine.backend(),
        compiled.machine_code,
        instance.pid()
    ))
}
