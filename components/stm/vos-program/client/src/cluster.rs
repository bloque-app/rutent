//! VOS nodes as separate OS processes, one or three of one space, each a `vos node run` with its own data
//! directory, settings file and faults file, on the runner host. Modelled on VOS's own `node/checker`'s
//! `processes`, which can't be reused here: it names its own agent program and passes no settings file, and each
//! node's executors must be pinned.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::net::{SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use ed25519_dalek::SigningKey;
use vos_node::client::{Client, Operator, Target};
use vos_wire::ids::{AgentId, InstanceId};
use vos_wire::ingress::{Install, NodeId, ReplicaStatus, Response};
use vos_wire::result::Outcome;

use crate::platform::Backend;

/// Every `vos` process a cluster started in its space, with its log: [`backend_proof`](crate::platform::backend_proof) checks each one's backend at the end of a test.
pub static STARTED: std::sync::Mutex<Vec<(u32, PathBuf)>> = std::sync::Mutex::new(Vec::new());

/// The `vos` processes that answered a query of the program on their own replica: each has run the program, so on
/// the JIT its perf map must name it.
pub static PROBED: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());

/// The program's instance in its agent.
pub const INSTANCE: InstanceId = InstanceId(1);
/// The client key the host signs its calls with.
pub const CLIENT_SEED: [u8; 32] = [7; 32];

/// Which node binary a cluster runs.
#[derive(Debug, Clone)]
pub struct NodeBuild {
    /// The `vos` binary.
    pub vos: PathBuf,
    /// Whether it was built with VOS's `faults` feature: a test build, which reads `VOS_FAULTS`.
    pub faults: bool,
}

impl NodeBuild {
    /// What a run note calls it.
    pub fn name(&self) -> &'static str {
        if self.faults { "faults" } else { "scored" }
    }
}

/// How a cluster's nodes run.
#[derive(Debug, Clone)]
pub struct ClusterConfig {
    pub build: NodeBuild,
    /// VOS's default agent program, which `vos node run` needs.
    pub agent_elf: PathBuf,
    /// The rutent program's ELF, installed as the agent's instance.
    pub program_elf: PathBuf,
    /// The nodes that replicate the program: 1 or 3.
    pub replicas: usize,
    /// Nodes listed in the space but not started, for a replacement to bring in (`R-02`, `R-03`).
    pub spares: usize,
    /// Each node's executors (its settings file's `executors`).
    pub executors: usize,
    pub backend: Backend,
    /// Lines of every node's faults file read once at start, such as `compact 16 4`; only a faults build reads them.
    pub settings: String,
    /// How long to wait for a node to start, a leader, an operator's command, a schema.
    pub wait: Duration,
    /// When set, each node runs under `strace -f -c -e trace=fsync,fdatasync,sync_file_range -o <dir>/node-N.strace`.
    pub strace: Option<PathBuf>,
}

/// A running node, killed (SIGKILL) when dropped.
pub struct Running {
    child: Child,
    /// The `vos` process: the child, or under strace the child's own child.
    pub pid: u32,
}

impl Running {
    fn kill(&mut self) {
        if self.pid != self.child.id() {
            // Under strace: kill the node itself, so strace sees it end and writes its summary.
            // SAFETY: a signal to a pid this harness started.
            unsafe { libc::kill(self.pid as libc::pid_t, libc::SIGKILL) };
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                if let Ok(Some(_)) = self.child.try_wait() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.kill();
    }
}

/// The nodes of one cluster, laid out under one directory: `node-N/` (its data), `node-N.toml`, `node-N.faults`,
/// `node-N.log`, `space`, `agent`.
pub struct Cluster {
    pub dir: PathBuf,
    pub config: ClusterConfig,
    keys: Vec<String>,
    peers: Vec<SocketAddr>,
    running: Vec<Option<Running>>,
    generation: u64,
    /// The agent holding the program, once created.
    pub agent: Option<AgentId>,
}

/// An error of the harness or of VOS, as text.
pub type Failed = String;

fn free_udp_port() -> u16 {
    UdpSocket::bind("127.0.0.1:0")
        .and_then(|s| s.local_addr())
        .map(|a| a.port())
        .expect("a free UDP port")
}

fn hex32(text: &str) -> Result<[u8; 32], Failed> {
    let text = text.trim();
    if text.len() != 64 {
        return Err("a key file is not 32 bytes of hex".into());
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16)
            .map_err(|_| "a key file is not hex".to_string())?;
    }
    Ok(out)
}

impl Cluster {
    /// The nodes `1..=replicas + spares`, with keys, a space file and settings, the replicas started; or the
    /// cluster at `dir` as a kill left it, its nodes started again.
    pub fn open(dir: &Path, config: ClusterConfig) -> Result<Cluster, Failed> {
        let fresh = !dir.join("space").exists() && !dir.join("node-1").exists();
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let count = config.replicas + config.spares;
        let mut cluster = Cluster {
            dir: dir.to_path_buf(),
            config,
            keys: Vec::new(),
            peers: Vec::new(),
            running: (0..count).map(|_| None).collect(),
            generation: 0,
            agent: None,
        };
        if fresh {
            // A first run alone makes each node's key.
            for n in 1..=count {
                let started = cluster.spawn(n, false)?;
                drop(started);
                let key = std::fs::read_to_string(cluster.data(n).join(vos_node::NODE_KEY))
                    .map_err(|e| format!("node {n}'s key: {e}"))?;
                cluster.keys.push(key.trim().to_string());
            }
            cluster.peers = (0..count)
                .map(|_| SocketAddr::from(([127, 0, 0, 1], free_udp_port())))
                .collect();
            if cluster.config.replicas > 1 {
                let space: String = (1..=count)
                    .map(|n| {
                        format!(
                            "{n} {} {} http://node-{n}\n",
                            cluster.keys[n - 1],
                            cluster.peers[n - 1]
                        )
                    })
                    .collect();
                std::fs::write(dir.join("space"), space).map_err(|e| e.to_string())?;
            } else {
                std::fs::write(dir.join("space"), "").map_err(|e| e.to_string())?;
            }
        } else {
            for n in 1..=count {
                let key = std::fs::read_to_string(cluster.data(n).join(vos_node::NODE_KEY))
                    .unwrap_or_default();
                cluster.keys.push(key.trim().to_string());
            }
            let space = std::fs::read_to_string(dir.join("space")).map_err(|e| e.to_string())?;
            cluster.peers = space
                .lines()
                .filter_map(|l| l.split_whitespace().nth(2)?.parse().ok())
                .collect();
            if let Ok(text) = std::fs::read_to_string(dir.join("agent")) {
                cluster.agent = text.trim().parse().ok().map(AgentId);
            }
        }
        for n in 1..=cluster.config.replicas {
            if cluster.data(n).exists() {
                cluster.start(n)?;
            }
        }
        Ok(cluster)
    }

    pub fn count(&self) -> usize {
        self.running.len()
    }

    pub fn data(&self, n: usize) -> PathBuf {
        self.dir.join(format!("node-{n}"))
    }

    fn faults_file(&self, n: usize) -> PathBuf {
        self.dir.join(format!("node-{n}.faults"))
    }

    pub fn log(&self, n: usize) -> PathBuf {
        self.dir.join(format!("node-{n}.log"))
    }

    pub fn target(&self, n: usize) -> Target {
        Target::socket(self.data(n).join("api.sock"))
    }

    /// Each node's key, by its bytes, and its target: where a client's redirect or failover goes.
    pub fn routes(&self) -> HashMap<[u8; 32], Target> {
        (1..=self.count())
            .filter_map(|n| Some((hex32(&self.keys[n - 1]).ok()?, self.target(n))))
            .collect()
    }

    /// A client with every node's route, signing with `seed`, calling node `first` first.
    pub fn client_with(&self, seed: [u8; 32], first: usize) -> Client {
        Client::new(&self.target(first), SigningKey::from_bytes(&seed)).with_routes(self.routes())
    }

    /// The host's client, calling the first running node first.
    pub fn client(&self) -> Client {
        let first = (1..=self.count())
            .find(|n| self.is_running(*n))
            .unwrap_or(1);
        self.client_with(CLIENT_SEED, first)
    }

    pub fn is_running(&self, n: usize) -> bool {
        self.running[n - 1].is_some()
    }

    /// The `vos` process ids of the running nodes, by number.
    pub fn pids(&self) -> Vec<(usize, u32)> {
        (1..=self.count())
            .filter_map(|n| self.running[n - 1].as_ref().map(|r| (n, r.pid)))
            .collect()
    }

    /// Start node `n`'s process: alone on its data directory (`in_space` false, a first run that makes its key),
    /// or in the space with its settings and faults file. The process once it said where its API is.
    fn spawn(&self, n: usize, in_space: bool) -> Result<Running, Failed> {
        let config = &self.config;
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.log(n))
            .map_err(|e| e.to_string())?;
        let settings = self.dir.join(format!("node-{n}.toml"));
        std::fs::write(&settings, format!("executors = {}\n", config.executors))
            .map_err(|e| e.to_string())?;
        let mut command = match (&config.strace, in_space) {
            (Some(dir), true) => {
                let mut c = Command::new("strace");
                c.args([
                    "-f",
                    "-c",
                    "-e",
                    "trace=fsync,fdatasync,sync_file_range",
                    "-o",
                ])
                .arg(dir.join(format!("node-{n}.strace")))
                .arg(&config.build.vos);
                c
            }
            _ => Command::new(&config.build.vos),
        };
        command
            .args(["node", "run", "--data"])
            .arg(self.data(n))
            .arg("--agent")
            .arg(&config.agent_elf)
            .arg("--config")
            .arg(&settings)
            .env("VOS_PVM_BACKEND", config.backend.env())
            .stdout(Stdio::piped())
            .stderr(log);
        if config.backend == Backend::Jit {
            command.env("VOS_PVM_PERF_MAP", "1");
        } else {
            command.env_remove("VOS_PVM_PERF_MAP");
        }
        if in_space {
            if config.replicas > 1 {
                command
                    .arg("--space")
                    .arg(self.dir.join("space"))
                    .arg("--peer-bind")
                    .arg(self.peers[n - 1].to_string());
            }
            command.env("VOS_FAULTS", self.faults_file(n));
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("starting node {n}: {e}"))?;
        let stdout = child.stdout.take().expect("piped stdout");
        let (lines, read) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                if lines.send(line).is_err() {
                    return;
                }
            }
        });
        let pid = child.id();
        let mut running = Running { child, pid };
        let until = Instant::now() + config.wait;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            match read.recv_timeout(left) {
                Ok(line) if line.starts_with("api on ") => break,
                Ok(_) => {}
                Err(e) => {
                    let log = std::fs::read_to_string(self.log(n)).unwrap_or_default();
                    let tail: Vec<&str> = log.lines().rev().take(8).collect();
                    // A node that exited early (its output closed) is the refusal an open retries; one that was
                    // merely slow is not, and its wait was long already.
                    return Err(match e {
                        mpsc::RecvTimeoutError::Disconnected => {
                            format!("node {n} exited before it started; its log ends: {tail:?}")
                        }
                        mpsc::RecvTimeoutError::Timeout => format!(
                            "node {n} did not start in {} s; its log ends: {tail:?}",
                            config.wait.as_secs()
                        ),
                    });
                }
            }
        }
        if config.strace.is_some() && in_space {
            // The node is strace's child.
            let children = std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children"))
                .unwrap_or_default();
            running.pid = children
                .split_whitespace()
                .next()
                .and_then(|p| p.parse().ok())
                .ok_or_else(|| format!("node {n}'s pid under strace"))?;
        }
        if in_space {
            STARTED.lock().unwrap().push((running.pid, self.log(n)));
        }
        Ok(running)
    }

    /// Start node `n` in the space, its faults file holding the settings alone.
    pub fn start(&mut self, n: usize) -> Result<(), Failed> {
        std::fs::write(self.faults_file(n), &self.config.settings).map_err(|e| e.to_string())?;
        let running = self.spawn(n, true)?;
        self.running[n - 1] = Some(running);
        if self.agent.is_some() {
            self.probe(n, Duration::from_secs(5));
        }
        Ok(())
    }

    /// Asks node `n`'s own replica a query of the program (`status`, on whatever it has applied), so the node runs
    /// the program: once it answers, the node's pid is in [`PROBED`]. False if it did not answer in `wait`, as a
    /// replica that does not hold the agent yet does not.
    pub fn probe(&self, n: usize, wait: Duration) -> bool {
        let (Some(agent), Some(pid)) = (self.agent, self.running[n - 1].as_ref().map(|r| r.pid))
        else {
            return false;
        };
        let until = Instant::now() + wait;
        let client = Client::new(&self.target(n), SigningKey::from_bytes(&[3; 32]));
        loop {
            let method = vos_node::client::schema(&self.target(n), agent, INSTANCE)
                .ok()
                .flatten()
                .and_then(|schema| schema.method_id("status"));
            if let Some(method) = method {
                let not_after = vos_node::wall_clock() + 60_000;
                let answer = client.query_with(
                    agent,
                    INSTANCE,
                    method,
                    &[],
                    not_after,
                    vos_wire::ingress::Consistency::Applied { at: 0 },
                );
                if let Ok(Response::Ran { .. }) = answer {
                    PROBED.lock().unwrap().push(pid);
                    return true;
                }
            }
            if Instant::now() > until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Kill node `n` with SIGKILL.
    pub fn kill(&mut self, n: usize) {
        if let Some(mut running) = self.running[n - 1].take() {
            running.kill();
        }
    }

    /// Kill every node.
    pub fn kill_all(&mut self) {
        for n in 1..=self.count() {
            self.kill(n);
        }
    }

    /// Node `n` loses its data directory: killed, and its directory removed, never to run from it again.
    pub fn lose(&mut self, n: usize) -> Result<(), Failed> {
        self.kill(n);
        std::fs::remove_dir_all(self.data(n)).map_err(|e| e.to_string())
    }

    /// Node `n`'s operator, signing with the operator key in its data directory.
    pub fn operator(&self, n: usize) -> Result<Operator, Failed> {
        let read = |name: &str| {
            std::fs::read_to_string(self.data(n).join(name))
                .map_err(|e| format!("node {n}'s {name}: {e}"))
                .and_then(|text| hex32(&text))
        };
        Ok(Operator::signing_within(
            &self.target(n),
            SigningKey::from_bytes(&read(vos_node::OPERATOR_KEY)?),
            NodeId(read(vos_node::NODE_KEY)?),
            Duration::from_secs(5),
        ))
    }

    /// Node `n`'s replica of the program's agent.
    pub fn status(&self, n: usize) -> Option<ReplicaStatus> {
        let agent = self.agent?;
        if !self.is_running(n) {
            return None;
        }
        match self.operator(n).ok()?.status(agent) {
            Ok(Response::Status(status)) => Some(status),
            _ => None,
        }
    }

    /// The running node leading the agent at the highest term, once one does.
    pub fn leader(&self) -> Result<usize, Failed> {
        let until = Instant::now() + self.config.wait;
        loop {
            let leading = (1..=self.count())
                .filter_map(|n| self.status(n).filter(|s| s.leads).map(|s| (s.term, n)))
                .max()
                .map(|(_, n)| n);
            if let Some(n) = leading {
                return Ok(n);
            }
            if Instant::now() > until {
                return Err("no node leads the agent".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// An operator's command at whichever running node takes it, retried until `accepted` or the wait ends.
    pub fn operate(
        &self,
        what: &str,
        command: impl Fn(&Operator) -> Option<Response>,
        accepted: impl Fn(&Response) -> bool,
    ) -> Result<Response, Failed> {
        let until = Instant::now() + self.config.wait;
        let mut last = None;
        loop {
            for n in (1..=self.count()).filter(|n| self.is_running(*n)) {
                if let Ok(operator) = self.operator(n)
                    && let Some(answer) = command(&operator)
                {
                    if accepted(&answer) {
                        return Ok(answer);
                    }
                    last = Some(answer);
                }
            }
            if Instant::now() > until {
                return Err(format!("{what}: no node took it; the last answer {last:?}"));
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Create the program's agent on the replicas and install the program; remembered in `agent`.
    pub fn create_program(&mut self) -> Result<AgentId, Failed> {
        let replicas: Vec<u8> = if self.config.replicas > 1 {
            (1..=self.config.replicas as u8).collect()
        } else {
            Vec::new()
        };
        let created = self.operate(
            "creating the agent",
            |o| o.create_agent(replicas.clone()).ok(),
            |a| matches!(a, Response::Created(_) | Response::Unconfirmed(_)),
        )?;
        let agent = match created {
            Response::Created(agent) | Response::Unconfirmed(agent) => agent,
            _ => unreachable!(),
        };
        self.agent = Some(agent);
        std::fs::write(self.dir.join("agent"), agent.0.to_string()).map_err(|e| e.to_string())?;
        let elf = std::fs::read(&self.config.program_elf)
            .map_err(|e| format!("the program's ELF: {e}"))?;
        self.install(&elf)?;
        for n in 1..=self.count() {
            if self.is_running(n) {
                self.probe(n, self.config.wait);
            }
        }
        Ok(agent)
    }

    /// Install `elf` as the program's instance, over whatever is installed: at the tip that touches no row.
    pub fn install(&self, elf: &[u8]) -> Result<(), Failed> {
        let agent = self.agent.ok_or("no agent")?;
        let install = Install {
            instance: INSTANCE,
            elf: elf.to_vec(),
            roles: Vec::new(),
        };
        let answer = self.operate(
            "installing the program",
            |o| o.install(agent, &install).ok(),
            |a| matches!(a, Response::Ran { .. }),
        )?;
        match answer {
            Response::Ran {
                outcome: Outcome::Done(_),
                ..
            } => Ok(()),
            other => Err(format!("installing the program: {other:?}")),
        }
    }

    /// Replace replica `remove` by node `add` (started first), at the leader: the replicas once it committed.
    pub fn replace(&mut self, remove: usize, add: usize) -> Result<Vec<u8>, Failed> {
        let agent = self.agent.ok_or("no agent")?;
        if !self.is_running(add) {
            self.start(add)?;
        }
        let answer = self.operate(
            "replacing a replica",
            |o| o.replace(agent, remove as u8, add as u8).ok(),
            |a| matches!(a, Response::Placed(_)),
        )?;
        match answer {
            Response::Placed(replicas) => {
                self.probe(add, self.config.wait);
                Ok(replicas)
            }
            other => Err(format!("replacing a replica: {other:?}")),
        }
    }

    /// Write node `n`'s faults file with the settings and `lines`, and wait until the node says they are in force.
    /// Only a faults build reads the file.
    pub fn set_faults(&mut self, n: usize, lines: &str) -> Result<(), Failed> {
        if !self.config.build.faults {
            return Err("a scored build injects no faults".into());
        }
        self.generation += 1;
        let text = format!(
            "{}\n{lines}\ngeneration {}\n",
            self.config.settings, self.generation
        );
        std::fs::write(self.faults_file(n), text).map_err(|e| e.to_string())?;
        let taken = self.faults_file(n).with_extension("taken");
        let until = Instant::now() + self.config.wait;
        loop {
            if std::fs::read_to_string(&taken)
                .is_ok_and(|t| t.trim() == self.generation.to_string())
            {
                return Ok(());
            }
            if Instant::now() > until {
                return Err(format!("node {n} did not take its faults"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// The faults on every running node.
    pub fn set_faults_all(&mut self, lines: &str) -> Result<(), Failed> {
        for n in (1..=self.count())
            .filter(|n| self.is_running(*n))
            .collect::<Vec<_>>()
        {
            self.set_faults(n, lines)?;
        }
        Ok(())
    }

    /// Node `n`'s stage timings: each stage's name, how many values it recorded, and their sum.
    pub fn stages(&self, n: usize) -> Result<Vec<(String, u64, u64)>, Failed> {
        match self.operator(n)?.stats() {
            Ok(Response::Stats(stats)) => Ok(stats
                .stages
                .into_iter()
                .map(|stage| (stage.name, stage.counts.iter().sum(), stage.total))
                .collect()),
            other => Err(format!("node {n}'s stats: {other:?}")),
        }
    }

    /// How many values node `n`'s stage `name` recorded.
    pub fn stage_count(&self, n: usize, name: &str) -> u64 {
        self.stages(n)
            .ok()
            .and_then(|s| s.into_iter().find(|(stage, _, _)| stage == name))
            .map_or(0, |(_, count, _)| count)
    }
}
