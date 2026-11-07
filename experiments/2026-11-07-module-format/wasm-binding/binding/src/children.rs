//! The platform's side of its children: the admitting compiler, run once per admission, and each provider's
//! process. Both are started isolated (`rutent_wasm_sandbox::isolated`).

use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout};
use std::time::{Duration, Instant};

use rutent_wasm_sandbox::{Limits, isolated, read_frame, write_frame};

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

/// Relays a child's standard error to the platform's, each line labelled with the child it came from.
fn relay(stderr: Option<impl Read + Send + 'static>, label: String) {
    if let Some(stderr) = stderr {
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                eprintln!("[{label}] {line}");
            }
        });
    }
}

use crate::engine;
use crate::wire::{CompileRequest, Compiled, FromProvider, Reply, ToProvider};

/// Where the binding's executables and stand-ins are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// The admitting compiler, `rutent-wasm-compiler`.
    pub compiler: PathBuf,
    /// A provider's process, `rutent-wasm-provider`.
    pub provider: PathBuf,
    /// The directory of built stand-ins, `{language}-{category}-{part}.wasm`, that `run.sh` writes.
    pub standins: PathBuf,
}

impl Paths {
    /// The paths `run.sh` gives through `RUTENT_WASM_STANDINS`, with the executables beside `executables`.
    pub fn new(executables: &Path, standins: &Path) -> Self {
        Self {
            compiler: executables.join("rutent-wasm-compiler"),
            provider: executables.join("rutent-wasm-provider"),
            standins: standins.to_path_buf(),
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

/// Compiles a release's two portable binaries in a fresh, isolated compiler process. The machine code, or why it
/// was refused.
pub fn compile(
    paths: &Paths,
    workdir: &Path,
    sandboxed: &[u8],
    outside: &[u8],
) -> Result<Machine, String> {
    let started = Instant::now();
    let own = Workdir::new(workdir)?;
    let mut child = isolated(&paths.compiler, &[], &own.0, Limits::COMPILER)
        .and_then(|mut c| c.spawn())
        .map_err(|e| format!("the compiler didn't start: {e}"))?;
    relay(child.stderr.take(), format!("compiler {}", child.id()));
    let result = (|| {
        let mut stdin = BufWriter::new(child.stdin.take().ok_or("no stdin")?);
        let mut stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        let request = CompileRequest {
            platform_version: engine::platform_version(),
        };
        let request = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
        for frame in [request.as_slice(), sandboxed, outside] {
            write_frame(&mut stdin, frame).map_err(|e| format!("the compiler: {e}"))?;
        }
        stdin.flush().map_err(|e| e.to_string())?;
        drop(stdin);
        let mut parts = Vec::new();
        for _ in 0..2 {
            let verdict = read_frame(&mut stdout)
                .map_err(|e| e.to_string())?
                .ok_or("the compiler stopped without a verdict")?;
            let verdict: Compiled = serde_json::from_slice(&verdict).map_err(|e| e.to_string())?;
            let code = read_frame(&mut stdout)
                .map_err(|e| e.to_string())?
                .ok_or("the compiler stopped without machine code")?;
            parts.push((verdict, code));
        }
        Ok::<_, String>(parts)
    })();
    let status = child.wait().map_err(|e| e.to_string())?;
    let mut parts = result?;
    if !status.success() {
        return Err(format!("the compiler exited with {status}"));
    }
    let (outside_verdict, outside_code) = parts.pop().ok_or("no outside part")?;
    let (sandboxed_verdict, sandboxed_code) = parts.pop().ok_or("no sandboxed part")?;
    for verdict in [&sandboxed_verdict, &outside_verdict] {
        if let Some(why) = &verdict.refusal {
            return Err(format!("the {} part was refused: {why}", verdict.part));
        }
    }
    Ok(Machine {
        sandboxed: sandboxed_code,
        outside: outside_code,
        verdicts: [sandboxed_verdict, outside_verdict],
        took: started.elapsed(),
    })
}

/// One provider's running process.
pub struct ProviderProcess {
    child: Child,
    _workdir: Workdir,
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    /// How long it took from spawning to both parts started.
    pub took: Duration,
}

impl ProviderProcess {
    /// Starts a process for `machine`, in a working directory of its own under `workdir`, its standard error
    /// relayed under `label`, and waits for both parts to start. Why not, if they didn't. `admitted` is
    /// each part's machine-code digest as the admission artefact records it, `[sandboxed, outside]`: the process
    /// checks the code it is given against them before it loads or instantiates anything.
    pub fn spawn(
        paths: &Paths,
        workdir: &Path,
        label: &str,
        machine: &Machine,
        admitted: [&str; 2],
    ) -> Result<Self, String> {
        let started = Instant::now();
        let own = Workdir::new(workdir)?;
        let mut child = isolated(&paths.provider, &[], &own.0, Limits::PROVIDER)
            .and_then(|mut c| c.spawn())
            .map_err(|e| format!("the provider's process didn't start: {e}"))?;
        relay(child.stderr.take(), format!("{label} {}", child.id()));
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("the provider's process has no pipes".to_owned());
        };
        let mut process = Self {
            child,
            _workdir: own,
            stdin: BufWriter::new(stdin),
            stdout: BufReader::new(stdout),
            took: Duration::ZERO,
        };
        let load = ToProvider::Load {
            sandboxed: admitted[0].to_owned(),
            outside: admitted[1].to_owned(),
        };
        // A process that refuses the code stops reading and says why; a write that fails because of that is read
        // past, so that the platform hears its reason.
        let _ = process.send(&load);
        for code in [&machine.sandboxed, &machine.outside] {
            if write_frame(&mut process.stdin, code).is_err() {
                break;
            }
        }
        match process.receive() {
            Some(FromProvider::Ready) => {
                process.took = started.elapsed();
                Ok(process)
            }
            Some(FromProvider::Failed { message }) => Err(message),
            _ => Err("the provider's process stopped while starting".to_owned()),
        }
    }

    /// Its operating-system process id.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Sends one message.
    pub fn send(&mut self, message: &ToProvider) -> Result<(), String> {
        let bytes = serde_json::to_vec(message).map_err(|e| e.to_string())?;
        write_frame(&mut self.stdin, &bytes).map_err(|e| e.to_string())
    }

    /// Answers a request it made.
    pub fn reply(&mut self, reply: &Reply) -> Result<(), String> {
        let bytes = serde_json::to_vec(reply).map_err(|e| e.to_string())?;
        write_frame(&mut self.stdin, &bytes).map_err(|e| e.to_string())
    }

    /// The next message, or `None` if it is gone or said something unreadable.
    pub fn receive(&mut self) -> Option<FromProvider> {
        let frame = read_frame(&mut self.stdout).ok()??;
        serde_json::from_slice(&frame).ok()
    }

    /// Whether the process is still running.
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for ProviderProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
