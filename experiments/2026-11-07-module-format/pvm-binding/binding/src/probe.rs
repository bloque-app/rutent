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
