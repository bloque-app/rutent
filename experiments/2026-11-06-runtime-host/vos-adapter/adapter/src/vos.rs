//! The VOS host, behind the `vos` feature: see the crate's documentation.

use std::io::{BufRead, BufReader};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use ed25519_dalek::SigningKey;
use rutent_contracts::state_machine::host::testing::{
    CrashPoint, FaultSeam, Harness, HarnessError,
};
use rutent_contracts::state_machine::host::{
    self as contract, Delivery, Host, HostError, RecordRef, RefusalReason, RefusalRecord,
    SnapshotEntry, SnapshotStream, StreamCheck, TransitionRecord, Version,
};
use rutent_contracts::state_machine::{self as machine, Id, Order, RecordedInput, StateMachine};
use rutent_vos_journal::{Appended, Imported, OrderRow, RecordRow, RefusalRow, Status};
use serde::Serialize;
use serde_json::Value;
use vos_node::client::{Client, Operator, Target};
use vos_wire::ids::{AgentId, InstanceId, MethodId};
use vos_wire::ingress::{Consistency, Install, NodeId, Response};
use vos_wire::result::Outcome;
use vos_wire::{Decode, Encode};

/// The journal's instance in its agent.
const INSTANCE: InstanceId = InstanceId(1);
/// The client key the adapter signs its calls with.
const CLIENT_SEED: [u8; 32] = [7; 32];

/// Where the adapter finds VOS and keeps its stores, and how it times its kills.
#[derive(Debug, Clone)]
pub struct VosConfig {
    /// The `vos` binary, built with `--features faults`.
    pub vos: PathBuf,
    /// VOS's default agent program (`vos-agent.elf`), which `vos node run` needs.
    pub agent_elf: PathBuf,
    /// The journal actor's ELF.
    pub journal_elf: PathBuf,
    /// The directory the stores are made in. Keep it short: the node's API socket is made in each store, and a
    /// unix socket's path is limited (104 bytes on macOS).
    pub base: PathBuf,
    /// Lines of VOS's fault file read once at start, such as `compact 16 4`; empty for VOS's defaults.
    pub settings: String,
    /// How long the adapter waits on the node: to start, to serve the journal's schema, to take its faults.
    pub wait: Duration,
    /// How long after sending a held append the node is killed, at [`CrashPoint::AfterComputed`].
    pub computed_kill_after: Duration,
    /// Whether VOS holds every commit before the append at [`CrashPoint::AfterComputed`] is sent. Always true but
    /// in the control pass, which leaves commits free to show that the kill would otherwise land after VOS made
    /// the append durable, so the hold, not the timing, is what keeps it from being durable.
    pub hold_at_computed: bool,
}

/// A store: one VOS node's data directory, with its fault file, log and agent id beside it.
#[derive(Debug)]
pub struct VosStore {
    dir: PathBuf,
    config: Arc<VosConfig>,
}

impl VosStore {
    /// The data directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn faults(&self) -> PathBuf {
        self.dir.with_extension("faults")
    }

    fn taken(&self) -> PathBuf {
        self.dir.with_extension("taken")
    }

    fn log(&self) -> PathBuf {
        self.dir.with_extension("log")
    }

    fn agent_file(&self) -> PathBuf {
        self.dir.with_extension("agent")
    }
}

/// A running node, killed when dropped.
struct Node {
    child: Child,
    socket: PathBuf,
}

impl Node {
    fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        self.kill();
    }
}

fn unavailable(reason: impl Into<String>) -> HostError {
    HostError::Unavailable {
        reason: reason.into(),
    }
}

fn refused(reason: impl Into<String>) -> HostError {
    HostError::SnapshotRefused {
        reason: reason.into(),
    }
}

/// `value` as JSON with every object's keys sorted and no whitespace: the canonical bytes the journal compares.
pub fn canonical<T: Serialize>(value: &T) -> Result<Vec<u8>, HostError> {
    fn sorted(value: Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut entries: Vec<(String, Value)> = map.into_iter().collect();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                Value::Object(entries.into_iter().map(|(k, v)| (k, sorted(v))).collect())
            }
            Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
            other => other,
        }
    }
    let value = serde_json::to_value(value).map_err(|e| unavailable(e.to_string()))?;
    serde_json::to_vec(&sorted(value)).map_err(|e| unavailable(e.to_string()))
}

fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8], what: &str) -> Result<T, HostError> {
    serde_json::from_slice(bytes).map_err(|e| unavailable(format!("{what} the journal holds: {e}")))
}

fn order_id(bytes: Vec<u8>) -> Result<Id, HostError> {
    String::from_utf8(bytes)
        .ok()
        .and_then(Id::new)
        .ok_or_else(|| unavailable("an order id the journal holds is not an id"))
}

fn hex32(text: &str) -> Result<[u8; 32], HostError> {
    let text = text.trim();
    let mut out = [0u8; 32];
    if text.len() != 64 {
        return Err(unavailable("a key file is not 32 bytes of hex"));
    }
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16)
            .map_err(|_| unavailable("a key file is not hex"))?;
    }
    Ok(out)
}

/// The node's operator, signing with the operator key in the store's data directory.
fn operator_of(store: &VosStore, target: &Target) -> Result<Operator, HostError> {
    let read = |name: &str| {
        std::fs::read_to_string(store.dir.join(name))
            .map_err(|e| unavailable(format!("{name}: {e}")))
            .and_then(|text| hex32(&text))
    };
    Ok(Operator::signing(
        target,
        SigningKey::from_bytes(&read(vos_node::OPERATOR_KEY)?),
        NodeId(read(vos_node::NODE_KEY)?),
    ))
}

/// Starts `vos node run` on the store, with its fault file holding the settings alone, and waits for its API.
fn start_node(store: &VosStore) -> Result<Node, HostError> {
    let config = &store.config;
    std::fs::write(store.faults(), &config.settings).map_err(|e| unavailable(e.to_string()))?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(store.log())
        .map_err(|e| unavailable(e.to_string()))?;
    let mut child = Command::new(&config.vos)
        .args(["node", "run", "--data"])
        .arg(&store.dir)
        .arg("--agent")
        .arg(&config.agent_elf)
        .env("VOS_FAULTS", store.faults())
        .stdout(Stdio::piped())
        .stderr(log)
        .spawn()
        .map_err(|e| unavailable(format!("starting vos: {e}")))?;
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
    let mut node = Node {
        child,
        socket: PathBuf::new(),
    };
    let until = Instant::now() + config.wait;
    loop {
        let left = until.saturating_duration_since(Instant::now());
        let line = read
            .recv_timeout(left)
            .map_err(|_| unavailable("vos did not say where its API is"))?;
        if let Some(socket) = line.strip_prefix("api on ") {
            node.socket = PathBuf::from(socket);
            return Ok(node);
        }
    }
}

/// The journal's method ids, read from its installed schema.
#[derive(Debug, Clone, Copy)]
struct Methods {
    append: MethodId,
    halt: MethodId,
    resume: MethodId,
    begin_import: MethodId,
    import_record: MethodId,
    import_refusal: MethodId,
    finish_import: MethodId,
    status: MethodId,
    orders_from: MethodId,
    order: MethodId,
    record: MethodId,
    records_from: MethodId,
    refusals_from: MethodId,
}

/// A host of rutent's host contract on a VOS node.
pub struct VosHost<M> {
    store: VosStore,
    node: Node,
    target: Target,
    client: Client,
    agent: AgentId,
    methods: Methods,
    /// Fault-seam machinery: the crash point the suite set, if any.
    crash: Option<CrashPoint>,
    /// Fault-seam machinery: set once the host stopped at a crash point.
    stopped: bool,
    /// The last generation of faults written to the node's fault file.
    generation: u64,
    machine: PhantomData<M>,
}

impl<M> std::fmt::Debug for VosHost<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VosHost")
            .field("store", &self.store.dir)
            .field("agent", &self.agent.0)
            .finish()
    }
}

impl<M> VosHost<M> {
    /// Starts the node on the store, creating the journal's agent and installing it on a new store.
    fn boot(store: VosStore) -> Result<Self, HostError> {
        let node = start_node(&store)?;
        let target = Target::socket(node.socket.clone());
        let agent = match std::fs::read_to_string(store.agent_file()) {
            Ok(text) => AgentId(
                text.trim()
                    .parse()
                    .map_err(|_| unavailable("the agent file is not an agent id"))?,
            ),
            Err(_) => {
                let operator = operator_of(&store, &target)?;
                let agent = match operator.create_agent(Vec::new()) {
                    Ok(Response::Created(agent)) => agent,
                    other => return Err(unavailable(format!("creating the agent: {other:?}"))),
                };
                let elf = std::fs::read(&store.config.journal_elf)
                    .map_err(|e| unavailable(format!("the journal's ELF: {e}")))?;
                let install = Install {
                    instance: INSTANCE,
                    elf,
                    roles: Vec::new(),
                };
                match operator.install(agent, &install) {
                    Ok(Response::Ran {
                        outcome: Outcome::Done(_),
                        ..
                    }) => {}
                    other => {
                        return Err(unavailable(format!("installing the journal: {other:?}")));
                    }
                }
                std::fs::write(store.agent_file(), agent.0.to_string())
                    .map_err(|e| unavailable(e.to_string()))?;
                agent
            }
        };
        let client = Client::new(&target, SigningKey::from_bytes(&CLIENT_SEED));
        let until = Instant::now() + store.config.wait;
        let schema = loop {
            if let Ok(Some(schema)) = client.schema(agent, INSTANCE) {
                break schema;
            }
            if Instant::now() > until {
                return Err(unavailable("the journal's schema was not served"));
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let id = |name: &str| {
            schema
                .method_id(name)
                .ok_or_else(|| unavailable(format!("the journal has no method {name}")))
        };
        let methods = Methods {
            append: id("append")?,
            halt: id("halt")?,
            resume: id("resume")?,
            begin_import: id("begin_import")?,
            import_record: id("import_record")?,
            import_refusal: id("import_refusal")?,
            finish_import: id("finish_import")?,
            status: id("status")?,
            orders_from: id("orders_from")?,
            order: id("order")?,
            record: id("record")?,
            records_from: id("records_from")?,
            refusals_from: id("refusals_from")?,
        };
        Ok(VosHost {
            store,
            node,
            target,
            client,
            agent,
            methods,
            crash: None,
            stopped: false,
            generation: 0,
            machine: PhantomData,
        })
    }

    /// The journal agent's replica, as the node's operator reads it: where its log starts after compaction, its
    /// last index, its applied index, and the sessions its applied state holds.
    pub fn log_status(&self) -> Result<(u64, u64, u64, u64), HostError> {
        match operator_of(&self.store, &self.target)?.status(self.agent) {
            Ok(Response::Status(status)) => Ok((
                status.log_start,
                status.last_index,
                status.applied,
                status.sessions,
            )),
            other => Err(unavailable(format!("the replica's status: {other:?}"))),
        }
    }

    /// The node's stage timings, as its operator reads them: each stage's name, how many values it recorded and
    /// their mean, in microseconds for the stages that time.
    pub fn stage_means(&self) -> Result<Vec<(String, u64, u64)>, HostError> {
        match operator_of(&self.store, &self.target)?.stats() {
            Ok(Response::Stats(stats)) => Ok(stats
                .stages
                .into_iter()
                .map(|stage| {
                    let count: u64 = stage.counts.iter().sum();
                    let mean = stage.total.checked_div(count).unwrap_or(0);
                    (stage.name, count, mean)
                })
                .collect()),
            other => Err(unavailable(format!("the node's stats: {other:?}"))),
        }
    }

    /// Kills the node, as a `SIGKILL`, and gives back its store as the kill left it.
    pub fn kill(mut self) -> VosStore {
        self.node.kill();
        self.store
    }

    fn call<T: Decode>(&mut self, method: MethodId, arguments: Vec<u8>) -> Result<T, HostError> {
        match self.client.call(self.agent, INSTANCE, method, &arguments) {
            Ok(Response::Ran {
                outcome: Outcome::Done(bytes),
                ..
            }) => T::decode(&bytes)
                .map(|(value, _)| value)
                .map_err(|e| unavailable(format!("an answer of the journal: {e:?}"))),
            Ok(other) => Err(unavailable(format!("VOS answered {other:?}"))),
            Err(e) => Err(unavailable(format!("VOS: {e}"))),
        }
    }

    fn query<T: Decode>(&self, method: MethodId, arguments: Vec<u8>) -> Result<T, HostError> {
        let not_after = vos_node::wall_clock() + 60_000;
        match self.client.query_with(
            self.agent,
            INSTANCE,
            method,
            &arguments,
            not_after,
            Consistency::Linearizable,
        ) {
            Ok(Response::Ran {
                outcome: Outcome::Done(bytes),
                ..
            }) => T::decode(&bytes)
                .map(|(value, _)| value)
                .map_err(|e| unavailable(format!("an answer of the journal: {e:?}"))),
            Ok(other) => Err(unavailable(format!("VOS answered a query with {other:?}"))),
            Err(e) => Err(unavailable(format!("VOS: {e}"))),
        }
    }

    fn status(&self) -> Result<Status, HostError> {
        self.query(self.methods.status, Vec::new())
    }

    fn order_row(&self, order: &Id) -> Result<Option<(u64, OrderRow)>, HostError> {
        self.query(
            self.methods.order,
            order.as_str().as_bytes().to_vec().encode(),
        )
    }

    fn latest_state(&self, order: &Id) -> Result<Option<Order>, HostError> {
        let Some((place, row)) = self.order_row(order)? else {
            return Ok(None);
        };
        if row.latest_state == 0 {
            return Ok(None);
        }
        let record: Option<RecordRow> = self.query(
            self.methods.record,
            [place.encode(), row.latest_state.encode()].concat(),
        )?;
        let state = record
            .and_then(|r| r.state)
            .ok_or_else(|| unavailable("an order's latest state is missing"))?;
        parse(&state, "a state").map(Some)
    }

    /// Moves the node's clock `ms` ahead, through VOS's `clock` fault, and waits until the node has taken it.
    pub fn move_clock(&mut self, ms: i64) -> Result<(), HostError> {
        self.set_faults(&format!("clock {ms}"))
    }

    /// Writes VOS's fault file with the settings and `lines`, and waits until the node says they are in force.
    fn set_faults(&mut self, lines: &str) -> Result<(), HostError> {
        self.generation += 1;
        let text = format!(
            "{}\n{lines}\ngeneration {}\n",
            self.store.config.settings, self.generation
        );
        std::fs::write(self.store.faults(), text).map_err(|e| unavailable(e.to_string()))?;
        let until = Instant::now() + self.store.config.wait;
        loop {
            if std::fs::read_to_string(self.store.taken())
                .is_ok_and(|t| t.trim() == self.generation.to_string())
            {
                return Ok(());
            }
            if Instant::now() > until {
                return Err(unavailable("the node did not take its faults"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Fault-seam machinery: stops the host at `point`, killing its node.
    fn stop(&mut self, point: CrashPoint) -> HostError {
        self.node.kill();
        self.stopped = true;
        unavailable(format!("stopped by the harness at {point:?}"))
    }

    fn refuse_if_stopped(&self) -> Result<(), HostError> {
        if self.stopped {
            return Err(unavailable("stopped by the harness"));
        }
        Ok(())
    }

    /// Imports the stream onto this host's empty journal; with a limit, stops after that many entries and
    /// returns false.
    fn import_from(
        &mut self,
        stream: SnapshotStream<'_>,
        limit: Option<u64>,
    ) -> Result<bool, HostError> {
        match self.call(self.methods.begin_import, Vec::new())? {
            Imported::Taken => {}
            Imported::Refused(why) => {
                return Err(refused(String::from_utf8_lossy(&why).into_owned()));
            }
        }
        let mut check = StreamCheck::new();
        for (n, entry) in stream.enumerate() {
            if limit == Some(n as u64) {
                return Ok(false);
            }
            let entry = entry?;
            check.push(&entry).map_err(|e| refused(e.to_string()))?;
            let taken: Imported = match entry {
                SnapshotEntry::Header {
                    host_version,
                    state_machine_version,
                } => {
                    if host_version.as_str() != contract::VERSION
                        || state_machine_version.as_str() != machine::VERSION
                    {
                        return Err(refused(format!(
                            "this host reads host contract {} and state machine {}, not {} and {}",
                            contract::VERSION,
                            machine::VERSION,
                            host_version.as_str(),
                            state_machine_version.as_str()
                        )));
                    }
                    Imported::Taken
                }
                SnapshotEntry::Record(r) => {
                    let state = r.state_after.as_ref().map(canonical).transpose()?;
                    let arguments = [
                        r.order_id.as_str().as_bytes().to_vec().encode(),
                        r.seq.encode(),
                        r.input.record_id.as_str().as_bytes().to_vec().encode(),
                        canonical(&r.input)?.encode(),
                        state.encode(),
                    ]
                    .concat();
                    self.call(self.methods.import_record, arguments)?
                }
                SnapshotEntry::Refusal(r) => {
                    let arguments = [
                        r.seq.encode(),
                        canonical(&r.input)?.encode(),
                        r.existing.order_id.as_str().as_bytes().to_vec().encode(),
                        r.existing.seq.encode(),
                    ]
                    .concat();
                    self.call(self.methods.import_refusal, arguments)?
                }
                SnapshotEntry::End { .. } => Imported::Taken,
            };
            if let Imported::Refused(why) = taken {
                return Err(refused(String::from_utf8_lossy(&why).into_owned()));
            }
        }
        check.finish().map_err(|e| refused(e.to_string()))?;
        match self.call(self.methods.finish_import, Vec::new())? {
            Imported::Taken => Ok(true),
            Imported::Refused(why) => Err(refused(String::from_utf8_lossy(&why).into_owned())),
        }
    }
}

impl<M: StateMachine> Host<M> for VosHost<M> {
    type Store = VosStore;

    fn open(store: VosStore) -> Result<Self, HostError> {
        let host = Self::boot(store)?;
        if host.status()?.importing {
            return Err(HostError::ImportIncomplete);
        }
        Ok(host)
    }

    fn submit(&mut self, input: RecordedInput) -> Result<Delivery, HostError> {
        self.refuse_if_stopped()?;
        let point = self.crash.take();
        if point == Some(CrashPoint::AfterReceived) {
            return Err(self.stop(CrashPoint::AfterReceived));
        }
        let order = input.subject.order.clone();
        let next = self.order_row(&order)?.map_or(1, |(_, row)| row.len + 1);
        let state_after = match self.latest_state(&order)? {
            Some(state) => Some(M::transition(&state, &input)),
            None => M::open(&input),
        };
        // A handler's arguments are its message's fields, encoded one after another.
        let arguments = [
            order.as_str().as_bytes().to_vec().encode(),
            next.encode(),
            input.record_id.as_str().as_bytes().to_vec().encode(),
            canonical(&input)?.encode(),
            state_after.as_ref().map(canonical).transpose()?.encode(),
        ]
        .concat();
        if point == Some(CrashPoint::AfterComputed) {
            // Every commit held, the append sent, and the node killed while it holds the refined batch.
            if self.store.config.hold_at_computed {
                self.set_faults("hold_commits")?;
            }
            let (target, agent, method) = (self.target.clone(), self.agent, self.methods.append);
            let sending = std::thread::spawn(move || {
                let mut client = Client::new(&target, SigningKey::from_bytes(&CLIENT_SEED));
                let _ = client.call(agent, INSTANCE, method, &arguments);
            });
            std::thread::sleep(self.store.config.computed_kill_after);
            let stopped = self.stop(CrashPoint::AfterComputed);
            let _ = sending.join();
            return Err(stopped);
        }
        let answer: Appended = self.call(self.methods.append, arguments)?;
        match answer {
            Appended::Recorded => {
                if point == Some(CrashPoint::AfterDurable) {
                    return Err(self.stop(CrashPoint::AfterDurable));
                }
                Ok(Delivery::Recorded(TransitionRecord {
                    order_id: order,
                    seq: next,
                    input,
                    state_after,
                }))
            }
            Appended::Already { order, seq, state } => {
                let state_after = state.map(|s| parse(&s, "a state")).transpose()?;
                Ok(Delivery::AlreadyRecorded(TransitionRecord {
                    order_id: order_id(order)?,
                    seq,
                    input,
                    state_after,
                }))
            }
            Appended::Reused {
                refusal,
                order,
                seq,
            } => Err(HostError::RecordIdReused {
                refusal: Box::new(RefusalRecord {
                    seq: refusal,
                    input,
                    reason: RefusalReason::RecordIdReused,
                    existing: RecordRef {
                        order_id: order_id(order)?,
                        seq,
                    },
                }),
            }),
            Appended::Halted => Err(HostError::Halted),
            Appended::OutOfSequence { next } => Err(unavailable(format!(
                "the order's next record is {next}: submitting again is safe"
            ))),
            Appended::Importing => Err(unavailable("an import has not finished")),
        }
    }

    fn orders(&self) -> Result<Vec<Id>, HostError> {
        let mut out = Vec::new();
        let mut from = 1;
        loop {
            let page: Vec<(u64, OrderRow)> = self.query(self.methods.orders_from, from.encode())?;
            let Some((last, _)) = page.last() else {
                return Ok(out);
            };
            from = last + 1;
            for (_, row) in page {
                out.push(order_id(row.id)?);
            }
        }
    }

    fn state(&self, order: &Id) -> Result<Option<Order>, HostError> {
        self.latest_state(order)
    }

    fn journal(&self, order: &Id) -> Result<Vec<TransitionRecord>, HostError> {
        let Some((place, row)) = self.order_row(order)? else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        let mut from = 1;
        while from <= row.len {
            let page: Vec<(u64, RecordRow)> = self.query(
                self.methods.records_from,
                [place.encode(), from.encode()].concat(),
            )?;
            let Some((last, _)) = page.last() else {
                return Err(unavailable("a record of the journal is missing"));
            };
            from = last + 1;
            for (seq, record) in page {
                out.push(TransitionRecord {
                    order_id: order.clone(),
                    seq,
                    input: parse(&record.input, "an input")?,
                    state_after: record.state.map(|s| parse(&s, "a state")).transpose()?,
                });
            }
        }
        Ok(out)
    }

    fn refusals(&self) -> Result<Vec<RefusalRecord>, HostError> {
        let mut out = Vec::new();
        let mut from = 1;
        loop {
            let page: Vec<(u64, RefusalRow)> =
                self.query(self.methods.refusals_from, from.encode())?;
            let Some((last, _)) = page.last() else {
                return Ok(out);
            };
            from = last + 1;
            for (seq, refusal) in page {
                out.push(RefusalRecord {
                    seq,
                    input: parse(&refusal.input, "a refused input")?,
                    reason: RefusalReason::RecordIdReused,
                    existing: RecordRef {
                        order_id: order_id(refusal.order)?,
                        seq: refusal.seq,
                    },
                });
            }
        }
    }

    fn halt(&mut self) -> Result<(), HostError> {
        self.refuse_if_stopped()?;
        if self.call::<bool>(self.methods.halt, Vec::new())? {
            Ok(())
        } else {
            Err(unavailable("an import has not finished"))
        }
    }

    fn resume(&mut self) -> Result<(), HostError> {
        self.refuse_if_stopped()?;
        if self.call::<bool>(self.methods.resume, Vec::new())? {
            Ok(())
        } else {
            Err(unavailable("an import has not finished"))
        }
    }

    fn export(&self) -> Result<SnapshotStream<'_>, HostError> {
        if !self.status()?.halted {
            return Err(HostError::NotHalted);
        }
        let mut entries = vec![SnapshotEntry::Header {
            host_version: Version::new(contract::VERSION)
                .expect("the host contract's version is MAJOR.MINOR.PATCH"),
            state_machine_version: Version::new(machine::VERSION)
                .expect("the state-machine contract's version is MAJOR.MINOR.PATCH"),
        }];
        let mut records = 0;
        for order in Host::<M>::orders(self)? {
            for record in Host::<M>::journal(self, &order)? {
                records += 1;
                entries.push(SnapshotEntry::Record(record));
            }
        }
        let refusals = Host::<M>::refusals(self)?;
        let refused = refusals.len() as u64;
        entries.extend(refusals.into_iter().map(SnapshotEntry::Refusal));
        entries.push(SnapshotEntry::End {
            records,
            refusals: refused,
        });
        Ok(Box::new(entries.into_iter().map(Ok)))
    }

    fn import(store: VosStore, stream: SnapshotStream<'_>) -> Result<Self, HostError> {
        let mut host = Self::boot(store)?;
        host.import_from(stream, None)?;
        Ok(host)
    }
}

/// The harness the host-contract suite drives the VOS host through: each store a directory under the
/// configuration's base, each host a `vos node run` process on it.
#[derive(Debug)]
pub struct VosHarness<M> {
    config: Arc<VosConfig>,
    prefix: String,
    next: u64,
    machine: PhantomData<M>,
}

impl<M> VosHarness<M> {
    /// A harness making its stores as `<base>/<prefix><n>`.
    pub fn new(config: VosConfig, prefix: &str) -> Self {
        VosHarness {
            config: Arc::new(config),
            prefix: prefix.into(),
            next: 0,
            machine: PhantomData,
        }
    }
}

impl<M: StateMachine> Harness<M> for VosHarness<M> {
    type Host = VosHost<M>;

    fn fresh_store(&mut self) -> Result<VosStore, HarnessError> {
        self.next += 1;
        let dir = self
            .config
            .base
            .join(format!("{}{}", self.prefix, self.next));
        if dir.exists() {
            return Err(HarnessError::Runtime {
                reason: format!("{} exists", dir.display()),
            });
        }
        Ok(VosStore {
            dir,
            config: Arc::clone(&self.config),
        })
    }

    fn start(&mut self, store: VosStore) -> Result<VosHost<M>, HarnessError> {
        Ok(VosHost::open(store)?)
    }

    fn kill(&mut self, host: VosHost<M>) -> Result<VosStore, HarnessError> {
        Ok(host.kill())
    }

    fn reopen(&mut self, store: VosStore) -> Result<VosHost<M>, HarnessError> {
        Ok(VosHost::open(store)?)
    }

    fn import(
        &mut self,
        store: VosStore,
        stream: SnapshotStream<'_>,
    ) -> Result<VosHost<M>, HarnessError> {
        Ok(VosHost::import(store, stream)?)
    }

    fn import_crashing_after(
        &mut self,
        store: VosStore,
        stream: SnapshotStream<'_>,
        entries: u64,
    ) -> Result<VosStore, HarnessError> {
        let mut host = VosHost::<M>::boot(store)?;
        host.import_from(stream, Some(entries))?;
        Ok(host.kill())
    }
}

impl<M: StateMachine> FaultSeam<M> for VosHarness<M> {
    fn crash_at(&mut self, host: &mut VosHost<M>, point: CrashPoint) {
        host.crash = Some(point);
    }
}
