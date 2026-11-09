//! The platform's side of its children: the admitting compiler, run once per admission, and each provider's
//! process. Both are started isolated (`crate::module::sandbox::isolated`).

use std::collections::VecDeque;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::module::limits::MAX_PROVIDER_FRAME;
use crate::module::sandbox::{Limits, isolated, read_frame, read_frame_up_to, write_frame};

/// A working directory of one child's own: empty, made before it starts and removed after it ends.
struct Workdir(PathBuf);

impl Workdir {
    fn new(base: &Path) -> Result<Self, String> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = base.join(format!("child-{}", NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(Self(dir))
    }
}

impl Drop for Workdir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The most of one child's standard error the binding keeps, as pvm-binding keeps it.
pub const STDERR_CAP: usize = 64 * 1024;

/// What one child wrote to its standard error, as the binding kept it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stderr {
    /// Whose: `compiler <pid>`, or the provider and its process id.
    pub label: String,
    /// The first [`STDERR_CAP`] bytes.
    pub bytes: Vec<u8>,
    /// Whether it wrote more, which was read and dropped.
    pub truncated: bool,
}

/// How many children's standard errors the platform keeps: the newest, so that a platform that restarts providers
/// for long holds a bounded amount (`REQ-MDL-11`, `NFR-3`).
pub const MAX_STDERRS: usize = 64;

/// The standard errors of the children, newest last, at most `capacity` of them: starting one past the cap drops
/// the oldest, and a child's thread that goes on writing to an entry already dropped has its bytes read and
/// dropped.
#[derive(Debug)]
pub struct StderrLog {
    entries: VecDeque<(u64, Stderr)>,
    next: u64,
    capacity: usize,
}

impl Default for StderrLog {
    fn default() -> Self {
        Self::with_capacity(MAX_STDERRS)
    }
}

impl StderrLog {
    /// A log keeping at most `capacity` entries.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            next: 0,
            capacity,
        }
    }

    /// Keeps at most `capacity` entries from now on, dropping the oldest at once if it holds more.
    pub fn set_capacity(&mut self, capacity: usize) {
        self.capacity = capacity;
        while self.entries.len() > capacity {
            self.entries.pop_front();
        }
    }

    /// The entries, oldest first.
    pub fn entries(&self) -> Vec<Stderr> {
        self.entries.iter().map(|(_, e)| e.clone()).collect()
    }

    /// How many entries it holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Opens an entry for a child that wrote something, and returns the number that names it.
    fn open(&mut self, label: &str) -> u64 {
        let id = self.next;
        self.next += 1;
        self.entries.push_back((
            id,
            Stderr {
                label: label.to_owned(),
                ..Stderr::default()
            },
        ));
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
        id
    }

    /// Appends what a child wrote to its entry, to the entry's cap; nothing if the entry was dropped.
    fn append(&mut self, id: u64, bytes: &[u8]) {
        if let Some((_, kept)) = self.entries.iter_mut().find(|(i, _)| *i == id) {
            let room = STDERR_CAP.saturating_sub(kept.bytes.len());
            kept.bytes
                .extend_from_slice(&bytes[..bytes.len().min(room)]);
            kept.truncated |= bytes.len() > room;
        }
    }
}

/// Every child's standard error, kept apart from the platform's own: none of it reaches the platform's stream.
pub type Stderrs = Arc<Mutex<StderrLog>>;

/// Reads a child's standard error to its end on a thread of its own, keeping at most [`STDERR_CAP`] bytes under
/// `label` in `into`, as they arrive, among the newest [`MAX_STDERRS`] (or the log's capacity); the rest is read and
/// dropped, so the child never blocks on a full pipe.
pub fn capture(stderr: Option<impl Read + Send + 'static>, label: String, into: &Stderrs) {
    let Some(mut stderr) = stderr else {
        return;
    };
    let into = Arc::clone(into);
    let _ = std::thread::Builder::new()
        .name("child-stderr".to_owned())
        .spawn(move || {
            let mut slot = None;
            let mut buf = [0u8; 4096];
            while let Ok(n) = stderr.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let Ok(mut all) = into.lock() else {
                    continue;
                };
                let id = *slot.get_or_insert_with(|| all.open(&label));
                all.append(id, &buf[..n]);
            }
        });
}

use crate::module::engine;
use crate::module::wire::{CompileRequest, Compiled, FromProvider, Reply, ToProvider};

/// Where the platform's two executables are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// The admitting compiler, `rutent-wasm-compiler`.
    pub compiler: PathBuf,
    /// A provider's process, `rutent-wasm-provider`.
    pub provider: PathBuf,
}

impl Paths {
    /// The two executables, found in `executables`.
    pub fn new(executables: &Path) -> Self {
        Self {
            compiler: executables.join("rutent-wasm-compiler"),
            provider: executables.join("rutent-wasm-provider"),
        }
    }
}

/// The machine code of one release, as the admitting compiler made it, with each part's digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Machine {
    /// The sandboxed part's machine code.
    pub sandboxed: Vec<u8>,
    /// The outside part's machine code.
    pub outside: Vec<u8>,
    /// What the compiler said of each part.
    pub verdicts: [Compiled; 2],
    /// How long the compiler's process took, start to finish.
    pub took: Duration,
}

/// Why a release's binaries weren't compiled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileError {
    /// The compiler refused a part: for an import it may not name, or otherwise.
    Refused {
        /// The part, `sandboxed` or `outside`.
        part: String,
        /// Whether it was for a disallowed import.
        disallowed_import: bool,
        /// The compiler's reason, for the admission log.
        why: String,
    },
    /// The compiler didn't give a verdict: it failed to start, crashed or was stopped.
    Failed(String),
    /// The compiler was still working when its wall-time deadline passed, and was killed and reaped
    /// (`REQ-MDL-11`). Its operating-system process id.
    Overdue(u32),
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused { part, why, .. } => write!(f, "the {part} part was refused: {why}"),
            Self::Failed(why) => f.write_str(why),
            Self::Overdue(pid) => write!(f, "the compiler (process {pid}) passed its deadline"),
        }
    }
}

impl From<String> for CompileError {
    fn from(why: String) -> Self {
        Self::Failed(why)
    }
}

impl From<&str> for CompileError {
    fn from(why: &str) -> Self {
        Self::Failed(why.to_owned())
    }
}

/// Kills a child started by [`isolated`] with SIGKILL, as a crash would, and reaps it: on Linux the process that
/// runs the module (the one child of `unshare`) first, then `unshare`, so that nothing of it answers after this
/// returns.
fn kill_child(child: &mut Child) {
    if cfg!(target_os = "linux")
        && let Some(module) = module_pid_of(child.id())
    {
        unsafe extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        // SAFETY: a plain system call on the one child of a process this binding started and hasn't reaped,
        // which therefore can't have been reused; SIGKILL is 9.
        unsafe { kill(module as i32, 9) };
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// The process that runs the module under the process `pid` this binding started: on Linux the one child of
/// `unshare`, as `/proc` lists it, if it is still there; elsewhere `pid` itself.
fn module_pid_of(pid: u32) -> Option<u32> {
    if cfg!(target_os = "linux") {
        std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children"))
            .ok()
            .and_then(|children| children.split_whitespace().next()?.parse().ok())
    } else {
        Some(pid)
    }
}

/// How long the platform waits for the admitting compiler, by default, from starting it to its exit: its CPU is
/// bounded to 60 seconds by the kernel, which a compiler that waits instead of working isn't.
pub const COMPILE_DEADLINE: Duration = Duration::from_secs(120);

/// Compiles a release's two portable binaries in a fresh, isolated compiler process. The machine code, or why it
/// was refused. The compiler has `within` from its start, wall time, to exit; one still running then is killed and
/// reaped, and the release is given as [`CompileError::Overdue`] (`REQ-MDL-11`).
pub fn compile(
    paths: &Paths,
    workdir: &Path,
    sandboxed: &[u8],
    outside: &[u8],
    stderrs: &Stderrs,
    within: Duration,
) -> Result<Machine, CompileError> {
    let started = Instant::now();
    let own = Workdir::new(workdir)?;
    let mut child = isolated(&paths.compiler, &[], &own.0, Limits::COMPILER)
        .and_then(|mut c| c.spawn())
        .map_err(|e| format!("the compiler didn't start: {e}"))?;
    let pid = child.id();
    capture(child.stderr.take(), format!("compiler {pid}"), stderrs);
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        kill_child(&mut child);
        return Err("the compiler has no pipes".into());
    };
    let request = CompileRequest {
        platform_version: engine::platform_version(),
    };
    let request = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
    // The exchange is on threads of its own, so that a compiler that stops reading or never answers blocks nothing
    // here: this waits for its verdicts until the deadline, and then kills it, which ends both threads.
    let result = std::thread::scope(|scope| {
        scope.spawn(move || {
            let mut stdin = BufWriter::new(stdin);
            for frame in [request.as_slice(), sandboxed, outside] {
                if write_frame(&mut stdin, frame).is_err() {
                    return;
                }
            }
            let _ = stdin.flush();
        });
        let (done, verdicts) = mpsc::channel();
        scope.spawn(move || {
            let mut stdout = BufReader::new(stdout);
            let _ = done.send(read_verdicts(&mut stdout));
        });
        let wait = (started + within).saturating_duration_since(Instant::now());
        match verdicts.recv_timeout(wait) {
            Ok(parts) => Some(parts),
            Err(_) => {
                kill_child(&mut child);
                None
            }
        }
    });
    let Some(parts) = result else {
        return Err(CompileError::Overdue(pid));
    };
    // A compiler that answered but doesn't exit is as overdue as one that doesn't answer.
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < started + within => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(None) => {
                kill_child(&mut child);
                return Err(CompileError::Overdue(pid));
            }
            Err(e) => return Err(e.to_string().into()),
        }
    };
    let mut parts = parts?;
    if !status.success() {
        return Err(format!("the compiler exited with {status}").into());
    }
    let (outside_verdict, outside_code) = parts.pop().ok_or("no outside part")?;
    let (sandboxed_verdict, sandboxed_code) = parts.pop().ok_or("no sandboxed part")?;
    for verdict in [&sandboxed_verdict, &outside_verdict] {
        if let Some(why) = &verdict.refusal {
            return Err(CompileError::Refused {
                part: verdict.part.clone(),
                disallowed_import: verdict.disallowed_import,
                why: why.clone(),
            });
        }
    }
    Ok(Machine {
        sandboxed: sandboxed_code,
        outside: outside_code,
        verdicts: [sandboxed_verdict, outside_verdict],
        took: started.elapsed(),
    })
}

/// The compiler's two verdicts, each with its machine code.
fn read_verdicts(stdout: &mut impl Read) -> Result<Vec<(Compiled, Vec<u8>)>, String> {
    let mut parts = Vec::new();
    for _ in 0..2 {
        let verdict = read_frame(stdout)
            .map_err(|e| e.to_string())?
            .ok_or("the compiler stopped without a verdict")?;
        let verdict: Compiled = serde_json::from_slice(&verdict).map_err(|e| e.to_string())?;
        let code = read_frame(stdout)
            .map_err(|e| e.to_string())?
            .ok_or("the compiler stopped without machine code")?;
        parts.push((verdict, code));
    }
    Ok(parts)
}

/// Reads a provider's process's frames on a thread of its own, into a queue that holds one: a process that sends
/// faster than the platform takes is stopped by its full pipe, so what the platform holds of it is bounded
/// (`REQ-MDL-11`, `THR-57`). The thread ends at the end of the stream, a frame past [`MAX_PROVIDER_FRAME`], or
/// when the receiver is dropped.
fn pump(stdout: impl Read + Send + 'static) -> Receiver<Option<Vec<u8>>> {
    let (incoming, from) = mpsc::sync_channel::<Option<Vec<u8>>>(1);
    let _ = std::thread::Builder::new()
        .name("provider-stdout".to_owned())
        .spawn(move || {
            let mut stdout = BufReader::new(stdout);
            loop {
                let frame = read_frame_up_to(&mut stdout, MAX_PROVIDER_FRAME)
                    .ok()
                    .flatten();
                let end = frame.is_none();
                if incoming.send(frame).is_err() || end {
                    break;
                }
            }
        });
    from
}

/// What waiting for a provider's process gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Received {
    /// Its next message.
    Message(FromProvider),
    /// It is gone, or said something unreadable.
    Gone,
    /// Nothing came before the deadline.
    Overdue,
}

/// One provider's running process. Its standard output is read, and its standard input written, each on a thread
/// of its own, so the platform never blocks on the process: it waits for a message only until a deadline.
pub struct ProviderProcess {
    child: Child,
    _workdir: Workdir,
    to: Sender<Vec<u8>>,
    from: Receiver<Option<Vec<u8>>>,
    /// How long it took from spawning to both parts started.
    pub took: Duration,
}

impl ProviderProcess {
    /// Starts a process for `machine`, in a working directory of its own under `workdir`, its standard error
    /// kept in `stderrs` under `label`, and waits for both parts to start, for at most `within`; a process that
    /// hasn't started by then is killed. Why not, if they didn't. `admitted` is each part's machine-code digest as
    /// the admission artefact records it, `[sandboxed, outside]`: the process checks the code it is given against
    /// them before it loads or instantiates anything.
    pub fn spawn(
        paths: &Paths,
        workdir: &Path,
        label: &str,
        machine: &Machine,
        admitted: [&str; 2],
        stderrs: &Stderrs,
        within: Duration,
    ) -> Result<Self, String> {
        let started = Instant::now();
        let own = Workdir::new(workdir)?;
        let mut child = isolated(&paths.provider, &[], &own.0, Limits::PROVIDER)
            .and_then(|mut c| c.spawn())
            .map_err(|e| format!("the provider's process didn't start: {e}"))?;
        capture(
            child.stderr.take(),
            format!("{label} {}", child.id()),
            stderrs,
        );
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("the provider's process has no pipes".to_owned());
        };
        let (to, outgoing) = mpsc::channel::<Vec<u8>>();
        // A write that fails ends the writer: a process that stops reading, or refuses its code and says why,
        // is then heard out by the reader.
        let _ = std::thread::Builder::new()
            .name("provider-stdin".to_owned())
            .spawn(move || {
                let mut stdin = BufWriter::new(stdin);
                for frame in outgoing {
                    if write_frame(&mut stdin, &frame).is_err() {
                        break;
                    }
                }
            });
        let from = pump(stdout);
        let mut process = Self {
            child,
            _workdir: own,
            to,
            from,
            took: Duration::ZERO,
        };
        let load = ToProvider::Load {
            sandboxed: admitted[0].to_owned(),
            outside: admitted[1].to_owned(),
        };
        let _ = process.send(&load);
        for code in [&machine.sandboxed, &machine.outside] {
            if process.to.send(code.clone()).is_err() {
                break;
            }
        }
        match process.receive_until(started + within) {
            Received::Message(FromProvider::Ready) => {
                process.took = started.elapsed();
                Ok(process)
            }
            Received::Message(FromProvider::Failed { message }) => Err(message),
            Received::Overdue => Err(format!(
                "the provider's process didn't start within {within:?}, and was killed"
            )),
            _ => Err("the provider's process stopped while starting".to_owned()),
        }
    }

    /// Its operating-system process id. On Linux that is `unshare`'s, whose one child runs the module.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// The process that runs the module: on Linux the one child of `unshare`, as `/proc` lists it, if it is
    /// still there; elsewhere the process itself.
    pub fn module_pid(&self) -> Option<u32> {
        module_pid_of(self.child.id())
    }

    /// Kills it with SIGKILL, as a crash would, and reaps it: on Linux the module's process first, then
    /// `unshare`, so that nothing of it answers after this returns.
    pub fn kill(&mut self) {
        kill_child(&mut self.child);
    }

    /// Sends one message; it never blocks on the process.
    pub fn send(&mut self, message: &ToProvider) -> Result<(), String> {
        let bytes = serde_json::to_vec(message).map_err(|e| e.to_string())?;
        self.to.send(bytes).map_err(|e| e.to_string())
    }

    /// Answers a request it made; it never blocks on the process.
    pub fn reply(&mut self, reply: &Reply) -> Result<(), String> {
        let bytes = serde_json::to_vec(reply).map_err(|e| e.to_string())?;
        self.to.send(bytes).map_err(|e| e.to_string())
    }

    /// The next message, waiting at most until `deadline`.
    pub fn receive_until(&mut self, deadline: Instant) -> Received {
        let wait = deadline.saturating_duration_since(Instant::now());
        match self.from.recv_timeout(wait) {
            Ok(Some(frame)) => {
                serde_json::from_slice(&frame).map_or(Received::Gone, Received::Message)
            }
            Ok(None) | Err(RecvTimeoutError::Disconnected) => Received::Gone,
            Err(RecvTimeoutError::Timeout) => Received::Overdue,
        }
    }

    /// Whether the process is still running.
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for ProviderProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // verifies: REQ-MDL-11
    #[test]
    fn a_childs_standard_error_is_kept_to_its_cap_labelled_and_the_rest_read_and_dropped() {
        let stderrs = Stderrs::default();
        let mut child = std::process::Command::new("/bin/sh")
            .args(["-c", "head -c 200000 /dev/zero | tr '\\0' x >&2; echo done"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("sh runs");
        capture(child.stderr.take(), "noisy".to_owned(), &stderrs);
        // The child finishes, so its pipe never filled.
        let output = child.wait_with_output().expect("it ends");
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "done");
        let kept = (0..200).find_map(|_| {
            std::thread::sleep(Duration::from_millis(10));
            let all = stderrs.lock().ok()?;
            all.entries().first().filter(|s| s.truncated).cloned()
        });
        let kept = kept.expect("its standard error was kept, and marked truncated");
        assert_eq!(kept.label, "noisy");
        assert_eq!(kept.bytes.len(), STDERR_CAP);
    }

    // verifies: REQ-MDL-11, NFR-3
    #[test]
    fn the_standard_errors_kept_are_the_newest_few_however_many_children_wrote_them() {
        const CAP: usize = 3;
        let stderrs = Stderrs::new(Mutex::new(StderrLog::with_capacity(CAP)));
        for n in 0..10 {
            let mut child = std::process::Command::new("/bin/sh")
                .args(["-c", &format!("echo child-{n} >&2")])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("sh runs");
            capture(child.stderr.take(), format!("child {n}"), &stderrs);
            child.wait().expect("it ends");
            // Each child's entry exists before the next starts, so the order is the children's.
            let seen = (0..500).any(|_| {
                std::thread::sleep(Duration::from_millis(10));
                stderrs.lock().is_ok_and(|all| {
                    all.entries()
                        .last()
                        .is_some_and(|e| e.label == format!("child {n}") && !e.bytes.is_empty())
                })
            });
            assert!(seen, "child {n}'s standard error wasn't kept");
            assert!(stderrs.lock().expect("the log").len() <= CAP);
        }
        let all = stderrs.lock().expect("the log").entries();
        let labels: Vec<_> = all.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["child 7", "child 8", "child 9"]);
        assert_eq!(all[2].bytes, b"child-9\n");
    }

    /// Frames of 1 KiB without end, counting the bytes taken from it.
    struct Flood(Arc<std::sync::atomic::AtomicUsize>, Vec<u8>, usize);

    impl Read for Flood {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.1.is_empty() {
                let mut frame = 1024u32.to_le_bytes().to_vec();
                frame.extend(std::iter::repeat_n(b'x', 1024));
                self.1 = frame;
                self.2 = 0;
            }
            let n = buf.len().min(self.1.len() - self.2);
            buf[..n].copy_from_slice(&self.1[self.2..self.2 + n]);
            self.2 += n;
            if self.2 == self.1.len() {
                self.1.clear();
            }
            self.0.fetch_add(n, std::sync::atomic::Ordering::SeqCst);
            Ok(n)
        }
    }

    // verifies: REQ-MDL-11, THR-57
    #[test]
    fn a_process_flooding_the_platform_with_frames_cannot_grow_what_the_platform_holds() {
        let taken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let from = pump(Flood(Arc::clone(&taken), Vec::new(), 0));
        // The platform reads none of it for a while: what is taken from the process stops at a few frames.
        std::thread::sleep(Duration::from_millis(500));
        let first = taken.load(std::sync::atomic::Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(500));
        let second = taken.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(first, second, "the reader kept reading with nobody taking");
        assert!(first <= 32 * 1024, "{first} bytes were held");
        // Taking one lets the reader go on, by one.
        assert!(matches!(from.recv(), Ok(Some(_))));
        drop(from);
    }
}
