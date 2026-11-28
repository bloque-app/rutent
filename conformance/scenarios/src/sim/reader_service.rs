//! The simulated networks' reader host, run as a process of its own beside Core (`T-020-60`, `T-024-25`, `DEC-28`):
//! what the assembly's deployment runs so that Core reads every public record through its own adapter
//! (`rutent_core_orch::closing::Readers`), as a deployment runs its reader host apart from Core.
//!
//! The reader host is a dev-dependency of this crate, for its tests only (`DEC-28`): this library never links it. It
//! runs it instead as the program `examples/sim_reader.rs` is in its `host` mode ([`program`]), which runs
//! `T-024-25`'s harness (`tests/readers_support`): `rutent_reader_host`'s host, each listed network's simulated reader a
//! child process of it in its jail, behind its mTLS link, under the harness's test CA. The host reads the world's
//! endpoints over a loopback connection to the world ([`super::readers::WorldServer`]), so what each reader answers is
//! the world as it stands. It hands back where its link listens, the test CA's certificate and the client certificate
//! and key the CA issued for Core: what the deployment hands Core, with nothing else.
//!
//! [`ReaderService`] stops the host when dropped: its standard input closes, it stops its readers and ends, and is
//! waited for; the world's loopback server is stopped with it (the review of `T-024-25` at `64309ac3`, Low 5).

use std::io::{self, BufRead as _, BufReader, Write as _};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::{Duration, Instant};

use rutent_contracts::core_internal::PublicNetwork;
use serde_json::{Value, json};

use super::readers::{Endpoints, Plan, WorldServer, served_json};

/// How long the host is given to start every reader and pass their start-up checks.
const STARTS_WITHIN: Duration = Duration::from_secs(120);

/// How long the host is given to stop once its input closes, before it is killed.
const STOPS_WITHIN: Duration = Duration::from_secs(10);

/// The simulated reader's program, `examples/sim_reader.rs`, which is also the reader host's in its `host` mode: from
/// `RUTENT_SIM_READER` if set; else this process itself, where it is that example; else the example beside the test
/// binary running, in its profile's `examples/`, as `cargo test` builds every example before it runs a test; else, for a
/// run of one test binary that built no example, built here once per process by `cargo build --example sim_reader`
/// on this package's manifest into the target directory the test runs from (a fallback, which a full `cargo test`
/// never takes).
///
/// # Panics
///
/// If it isn't found and can't be built, naming the command.
pub fn program() -> PathBuf {
    static FOUND: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FOUND
        .get_or_init(|| {
            if let Some(path) = std::env::var_os("RUTENT_SIM_READER") {
                return PathBuf::from(path);
            }
            let exe = std::env::current_exe().expect("this process's own path");
            let dir = exe.parent().expect("a directory");
            if dir.file_name().is_some_and(|d| d == "examples")
                && exe
                    .file_stem()
                    .is_some_and(|s| s.to_string_lossy().starts_with("sim_reader"))
            {
                return exe;
            }
            // `<target>/<profile>/deps/<test>`: the example is in `<target>/<profile>/examples/`.
            let profile = dir.parent().expect("a profile's directory");
            let beside = profile
                .join("examples")
                .join(format!("sim_reader{}", std::env::consts::EXE_SUFFIX));
            if beside.is_file() {
                return beside;
            }
            build(profile)
        })
        .clone()
}

/// Builds the example into the target directory `profile` is in.
fn build(profile: &std::path::Path) -> PathBuf {
    let target = profile
        .parent()
        .expect("the profile's directory is in a target directory");
    let release = profile.file_name().is_some_and(|p| p == "release");
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let mut build = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    build
        .args(["build", "--quiet", "--example", "sim_reader"])
        .args(["--message-format", "json"])
        .arg("--manifest-path")
        .arg(&manifest)
        .arg("--target-dir")
        .arg(target);
    if release {
        build.arg("--release");
    }
    let out = build.output().expect("cargo runs");
    let built = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|m| {
            m["reason"] == "compiler-artifact"
                && m["target"]["name"] == "sim_reader"
                && m["target"]["kind"]
                    .as_array()
                    .is_some_and(|k| k.iter().any(|k| k == "example"))
        })
        .find_map(|m| m["executable"].as_str().map(PathBuf::from));
    match built {
        Some(path) if out.status.success() && path.is_file() => path,
        _ => panic!(
            "the simulated reader wasn't built: `cargo build --example sim_reader --manifest-path {}`: {}",
            manifest.display(),
            String::from_utf8_lossy(&out.stderr)
        ),
    }
}

/// The reader host, running as a process of its own for the listed networks, and the world's answers it reads.
pub struct ReaderService {
    child: Child,
    stdin: Option<ChildStdin>,
    address: SocketAddr,
    ca: String,
    chain: String,
    key: String,
    plan: Plan,
    listed: Vec<PublicNetwork>,
    endpoints: Endpoints,
    world: Option<WorldServer>,
}

impl core::fmt::Debug for ReaderService {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ReaderService")
            .field("address", &self.address)
            .field("plan", &self.plan)
            .finish_non_exhaustive()
    }
}

impl ReaderService {
    /// Starts the reader host for `listed`, its readers under `plan`, over the endpoints the world serves, and waits
    /// for every reader to pass its start-up checks.
    ///
    /// # Errors
    ///
    /// If the host can't be started, or says why it didn't start.
    pub fn start(
        listed: &[PublicNetwork],
        endpoints: &Endpoints,
        plan: &Plan,
    ) -> io::Result<ReaderService> {
        let world = endpoints.serve_loopback()?;
        let served: serde_json::Map<String, Value> = endpoints
            .all()
            .iter()
            .map(|(host, served)| (host.clone(), served_json(served)))
            .collect();
        let config = json!({
            "listed": listed,
            "endpoints": served,
            "world": world.address().to_string(),
            "plan": plan.to_json(),
        });
        let mut child = Command::new(program())
            .arg("host")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let mut stdin = child.stdin.take().expect("its input");
        stdin.write_all(format!("{config}\n").as_bytes())?;
        stdin.flush()?;
        let stdout = child.stdout.take().expect("its output");
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(stdout).read_line(&mut line);
            let _ = send.send(line);
        });
        let refused = |why: String| {
            io::Error::other(format!("the simulated reader host didn't start: {why}"))
        };
        let Ok(line) = receive.recv_timeout(STARTS_WITHIN) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(refused("it said nothing in time".to_owned()));
        };
        let told: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        let text = |key: &str| told[key].as_str().map(str::to_owned);
        let (Some(address), Some(ca), Some(chain), Some(key)) =
            (text("address"), text("ca"), text("chain"), text("key"))
        else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(refused(text("error").unwrap_or(line)));
        };
        Ok(ReaderService {
            child,
            stdin: Some(stdin),
            address: address.parse().map_err(io::Error::other)?,
            ca,
            chain,
            key,
            plan: plan.clone(),
            listed: listed.to_vec(),
            endpoints: endpoints.clone(),
            world: Some(world),
        })
    }

    /// The same host started again under `plan`, for the same listings and endpoints: a new process, at a new address.
    ///
    /// # Errors
    ///
    /// As [`ReaderService::start`].
    pub fn restarted_under(self, plan: &Plan) -> io::Result<ReaderService> {
        let (listed, endpoints) = (self.listed.clone(), self.endpoints.clone());
        drop(self);
        ReaderService::start(&listed, &endpoints, plan)
    }

    /// Where the host's link listens: what the deployment's host setting holds.
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// The operator's CA (the harness's test CA), PEM.
    pub fn ca_pem(&self) -> &str {
        &self.ca
    }

    /// Core's client certificate the CA issued, PEM.
    pub fn chain_pem(&self) -> &str {
        &self.chain
    }

    /// Its key, PEM.
    pub fn key_pem(&self) -> &str {
        &self.key
    }

    /// Core's adapter over this host, as the deployment hands it to Core (`T-020-60`): Core's certificate and key and
    /// the operator's CA, and the address the host setting [`super::readers::HOST_SETTING`] holds, for `deployment`
    /// under `versions`; with `listed` taken in and each reader's start-up check run at Core's `clock`.
    ///
    /// # Errors
    ///
    /// If Core's side of the link can't be built from what the host handed back.
    pub fn core_readers(
        &self,
        deployment: rutent_contracts::provider_role::Deployment,
        versions: rutent_contracts::provider_role::ProtocolVersions,
        listed: &[PublicNetwork],
        clock: &dyn rutent_core_orch::snapshot::Clock,
    ) -> Result<rutent_core_orch::closing::Readers, rutent_core_orch::closing::readers::TlsRefusal>
    {
        let tls = rutent_core_orch::closing::readers::ReaderTls::new(
            self.ca.as_bytes(),
            self.chain.as_bytes(),
            self.key.as_bytes(),
        )?;
        let mut readers = rutent_core_orch::closing::Readers::new(tls, deployment, versions);
        readers.set_host(super::readers::HOST_SETTING, self.address);
        for network in listed {
            readers.list(network);
        }
        readers.check_all(clock);
        Ok(readers)
    }

    /// The faults its readers run under.
    pub fn plan(&self) -> &Plan {
        &self.plan
    }
}

impl Drop for ReaderService {
    fn drop(&mut self) {
        // Its input closed, the host stops its readers and ends.
        drop(self.stdin.take());
        let until = Instant::now() + STOPS_WITHIN;
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < until => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                _ => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break;
                }
            }
        }
        drop(self.world.take());
    }
}
