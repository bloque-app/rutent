//! The thin [`Host`] client of the VOS program (`T-010-05`): every rule of the host contract runs in the program,
//! in one handler step a submit; this side sends the input, reads the answer back with the contract's own types, and
//! drives the suite's faults.
//!
//! The program runs `rutent-stm`'s `Stm`, whatever `M` a caller names: `M` only says which state machine's states
//! the caller expects, as the suite's probe does. The suite scores this client as `Harness<Probe>`, which rests on
//! `rutent-stm`'s test that `Stm` reaches the probe's states over the suite's own scenarios.

use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rutent_contracts::state_machine::host::testing::{
    CrashPoint, FaultSeam, Harness, HarnessError,
};
use rutent_contracts::state_machine::host::{
    self as contract, Delivery, Host, HostError, RecordRef, RefusalReason, RefusalRecord,
    SnapshotEntry, SnapshotStream, StreamCheck, TransitionRecord, Version,
};
use rutent_contracts::state_machine::{self as machine, Id, Order, RecordedInput, StateMachine};
use rutent_stm::Stm;
use rutent_stm_vos_program::{
    Imported, OrderRow, RecordRow, RefusalRow, Refused, Released, Status, Submitted,
};
use serde::de::DeserializeOwned;
use vos_node::client::Client;
use vos_wire::ids::{AgentId, MethodId};
use vos_wire::ingress::{Consistency, Response};
use vos_wire::result::Outcome;
use vos_wire::{Decode, Encode};

use crate::cluster::{Cluster, ClusterConfig, INSTANCE};

/// The most entries an import chunk carries, and the most bytes: one call each, under VOS's 64 KiB request cap
/// and within the chunk's 100M gas. Every run note states them.
pub const IMPORT_ENTRIES: usize = 8;
/// The most bytes of entries an import chunk carries.
pub const IMPORT_BYTES: usize = 48 << 10;

pub fn unavailable(reason: impl Into<String>) -> HostError {
    HostError::Unavailable {
        reason: reason.into(),
    }
}

fn refused(reason: impl Into<String>) -> HostError {
    HostError::SnapshotRefused {
        reason: reason.into(),
    }
}

fn parse<T: DeserializeOwned>(bytes: &[u8], what: &str) -> Result<T, HostError> {
    serde_json::from_slice(bytes).map_err(|e| unavailable(format!("{what} the program holds: {e}")))
}

fn order_id(bytes: Vec<u8>) -> Result<Id, HostError> {
    String::from_utf8(bytes)
        .ok()
        .and_then(Id::new)
        .ok_or_else(|| unavailable("an order id the program holds is not an id"))
}

/// A stored record as the contract's record, read with the original `contracts/` types (which run its checks).
pub fn record_of(order: &Id, seq: u64, row: &RecordRow) -> Result<TransitionRecord, HostError> {
    let record = TransitionRecord {
        order_id: order.clone(),
        seq,
        input: parse(&row.input, "an input")?,
        state_after: row
            .state
            .as_ref()
            .map(|s| parse(s, "a state"))
            .transpose()?,
    };
    record
        .check()
        .map_err(|v| unavailable(format!("a record the program holds: {v}")))?;
    Ok(record)
}

fn format_error(refusal: Refused) -> HostError {
    match refusal {
        Refused::Format { stored, expected } => unavailable(format!(
            "the program reads format {expected}, and its rows are format {stored}: it refuses them"
        )),
        Refused::NotHalted => HostError::NotHalted,
        Refused::Import(why) | Refused::Archive(why) => {
            unavailable(String::from_utf8_lossy(&why).into_owned())
        }
    }
}

/// The program's method ids, from its installed schema.
#[derive(Debug, Clone, Copy)]
pub struct Methods {
    pub submit: MethodId,
    pub halt: MethodId,
    pub resume: MethodId,
    pub begin_import: MethodId,
    pub import_chunk: MethodId,
    pub status: MethodId,
    pub orders_from: MethodId,
    pub order: MethodId,
    pub state: MethodId,
    pub records_from: MethodId,
    pub refusals_from: MethodId,
    pub released_from: MethodId,
    pub archived: MethodId,
    pub canonical_of: MethodId,
}

/// A store: a cluster's directory, its nodes stopped.
#[derive(Debug, Clone)]
pub struct VosStore {
    pub dir: PathBuf,
    pub config: Arc<ClusterConfig>,
}

/// rutent's host as the VOS program on one node or three.
pub struct VosHost {
    pub cluster: Cluster,
    pub client: Client,
    pub agent: AgentId,
    pub methods: Methods,
    crash: Option<CrashPoint>,
    stopped: bool,
}

impl std::fmt::Debug for VosHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VosHost")
            .field("dir", &self.cluster.dir)
            .finish()
    }
}

/// Decodes a handler's or a query's answer.
pub fn answer<T: Decode>(
    response: Result<Response, vos_node::client::ClientError>,
) -> Result<T, HostError> {
    match response {
        Ok(Response::Ran {
            outcome: Outcome::Done(bytes),
            ..
        }) => T::decode(&bytes)
            .map(|(value, _)| value)
            .map_err(|e| unavailable(format!("an answer of the program: {e:?}"))),
        // A handler that returned an error: the step failed, nothing it wrote is kept, and the detail is the error.
        Ok(Response::Ran {
            outcome: Outcome::Failed { code, detail },
            ..
        }) => match Refused::decode(&detail) {
            Ok((refusal, _)) => Err(format_error(refusal)),
            Err(_) => Err(unavailable(format!(
                "the step failed: {code:?}, {}",
                String::from_utf8_lossy(&detail)
            ))),
        },
        Ok(other) => Err(unavailable(format!("VOS answered {other:?}"))),
        Err(e) => Err(unavailable(format!("VOS: {e}"))),
    }
}

/// The program's method ids, once a running node serves its schema.
pub fn methods(cluster: &Cluster, agent: AgentId) -> Result<Methods, HostError> {
    let until = Instant::now() + cluster.config.wait;
    let schema = loop {
        let served = (1..=cluster.count())
            .filter(|n| cluster.is_running(*n))
            .find_map(|n| {
                vos_node::client::schema(&cluster.target(n), agent, INSTANCE)
                    .ok()
                    .flatten()
            });
        if let Some(schema) = served {
            break schema;
        }
        if Instant::now() > until {
            return Err(unavailable("the program's schema was not served"));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let id = |name: &str| {
        schema
            .method_id(name)
            .ok_or_else(|| unavailable(format!("the program has no method {name}")))
    };
    Ok(Methods {
        submit: id("submit")?,
        halt: id("halt")?,
        resume: id("resume")?,
        begin_import: id("begin_import")?,
        import_chunk: id("import_chunk")?,
        status: id("status")?,
        orders_from: id("orders_from")?,
        order: id("order")?,
        state: id("state")?,
        records_from: id("records_from")?,
        refusals_from: id("refusals_from")?,
        released_from: id("released_from")?,
        archived: id("archived")?,
        canonical_of: id("canonical_of")?,
    })
}

/// A query on `client`, linearizable.
pub fn query<T: Decode>(
    client: &Client,
    agent: AgentId,
    method: MethodId,
    arguments: &[u8],
) -> Result<T, HostError> {
    let not_after = vos_node::wall_clock() + 60_000;
    answer(client.query_with(
        agent,
        INSTANCE,
        method,
        arguments,
        not_after,
        Consistency::Linearizable,
    ))
}

/// The arguments of a submit: the record id, and the input as JSON.
pub fn submit_arguments(input: &RecordedInput) -> Result<Vec<u8>, HostError> {
    let json = serde_json::to_vec(input).map_err(|e| unavailable(e.to_string()))?;
    Ok([
        input.record_id.as_str().as_bytes().to_vec().encode(),
        json.encode(),
    ]
    .concat())
}

/// What a submit's answer means under the host contract.
pub fn delivery(input: RecordedInput, answer: Submitted) -> Result<Delivery, HostError> {
    match answer {
        Submitted::Recorded { order, seq, row } => {
            Ok(Delivery::Recorded(record_of(&order_id(order)?, seq, &row)?))
        }
        Submitted::Already { order, seq, row } => Ok(Delivery::AlreadyRecorded(record_of(
            &order_id(order)?,
            seq,
            &row,
        )?)),
        Submitted::Reused {
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
        Submitted::Halted => Err(HostError::Halted),
        Submitted::Importing => Err(unavailable("an import has not finished")),
        Submitted::Invalid(why) => Err(unavailable(format!(
            "the program refused the input: {}",
            String::from_utf8_lossy(&why)
        ))),
        Submitted::Format { stored, expected } => {
            Err(format_error(Refused::Format { stored, expected }))
        }
        Submitted::Full => Err(unavailable(
            "the order's next sequence number would pass MAX_SEQ",
        )),
    }
}

impl VosHost {
    /// Starts the cluster on the store, creating the program's agent on a fresh one.
    pub fn boot(store: VosStore) -> Result<Self, HostError> {
        // A node killed a moment ago, or a child being spawned beside this open, can still hold the data
        // directory's lock; the open of a cluster that exists is retried for that window (`REQ-OPS-3`).
        let reopening = store.dir.join("space").exists() || store.dir.join("node-1").exists();
        let mut cluster = crate::retry::open_retrying(reopening, || {
            Cluster::open(&store.dir, (*store.config).clone()).map_err(unavailable)
        })?;
        let agent = match cluster.agent {
            Some(agent) => agent,
            None => cluster.create_program().map_err(unavailable)?,
        };
        let methods = methods(&cluster, agent)?;
        let client = cluster.client();
        Ok(VosHost {
            cluster,
            client,
            agent,
            methods,
            crash: None,
            stopped: false,
        })
    }

    /// The store, once every node is killed.
    pub fn kill(mut self) -> VosStore {
        self.cluster.kill_all();
        VosStore {
            dir: self.cluster.dir.clone(),
            config: Arc::new(self.cluster.config.clone()),
        }
    }

    /// One import chunk. A chunk the program refuses fails its step, so none of it is kept: the refusal is the
    /// step's error, an import refusal.
    pub fn import_chunk(&mut self, arguments: &[u8]) -> Result<Imported, HostError> {
        let response = self
            .client
            .call(self.agent, INSTANCE, self.methods.import_chunk, arguments);
        if let Ok(Response::Ran {
            outcome: Outcome::Failed { detail, .. },
            ..
        }) = &response
            && let Ok((why, _)) = Refused::decode(detail)
        {
            return Err(refused(match why {
                Refused::Import(why) => String::from_utf8_lossy(&why).into_owned(),
                other => format!("{other:?}"),
            }));
        }
        answer(response)
    }

    pub fn call<T: Decode>(&mut self, method: MethodId, arguments: &[u8]) -> Result<T, HostError> {
        answer(self.client.call(self.agent, INSTANCE, method, arguments))
    }

    pub fn query<T: Decode>(&self, method: MethodId, arguments: &[u8]) -> Result<T, HostError> {
        query(&self.client, self.agent, method, arguments)
    }

    pub fn status(&self) -> Result<Status, HostError> {
        self.query(self.methods.status, &[])
    }

    fn order_row(&self, order: &Id) -> Result<Option<(u64, OrderRow)>, HostError> {
        self.query::<Option<(u64, OrderRow)>>(
            self.methods.order,
            &order.as_str().as_bytes().to_vec().encode(),
        )
    }

    fn stop(&mut self, point: CrashPoint) -> HostError {
        self.cluster.kill_all();
        self.stopped = true;
        unavailable(format!("stopped by the harness at {point:?}"))
    }

    fn refuse_if_stopped(&self) -> Result<(), HostError> {
        if self.stopped {
            return Err(unavailable("stopped by the harness"));
        }
        Ok(())
    }

    /// Every released record from place `from` in write order, a page at a time; with `halted_only`, refused
    /// unless the host is halted.
    pub fn released(&self, from: u64, halted_only: bool) -> Result<Vec<Released>, HostError> {
        self.query::<Vec<Released>>(
            self.methods.released_from,
            &[from.encode(), halted_only.encode()].concat(),
        )
    }

    /// The canonical JSON the program writes for `json`, as a recorded input or as any JSON value.
    pub fn canonical_of(
        &self,
        json: &[u8],
        as_input: bool,
    ) -> Result<Result<Vec<u8>, String>, HostError> {
        let not_after = vos_node::wall_clock() + 60_000;
        let response = self.client.query_with(
            self.agent,
            INSTANCE,
            self.methods.canonical_of,
            &[json.to_vec().encode(), as_input.encode()].concat(),
            not_after,
            Consistency::Linearizable,
        );
        match response {
            Ok(Response::Ran {
                outcome: Outcome::Failed { detail, .. },
                ..
            }) => Ok(Err(Vec::<u8>::decode(&detail)
                .map(|(e, _)| String::from_utf8_lossy(&e).into_owned())
                .unwrap_or_default())),
            other => answer::<Vec<u8>>(other).map(Ok),
        }
    }

    /// Imports the stream onto this host's empty store, in chunks; with a limit, stops after that many entries and
    /// returns false.
    pub fn import_from(
        &mut self,
        stream: SnapshotStream<'_>,
        limit: Option<u64>,
    ) -> Result<bool, HostError> {
        match self.call::<Imported>(self.methods.begin_import, &[])? {
            Imported::Taken { .. } => {}
            Imported::Refused(why) => return Err(refused(format!("{why:?}"))),
        }
        let mut check = StreamCheck::new();
        let mut chunk: Vec<Vec<u8>> = Vec::new();
        let mut bytes = 0;
        let mut at = 0u64;
        let mut sent = 0u64;
        let mut ended = false;
        let send = |host: &mut VosHost,
                    chunk: &mut Vec<Vec<u8>>,
                    at: &mut u64|
         -> Result<bool, HostError> {
            if chunk.is_empty() {
                return Ok(false);
            }
            // The program may take only a part of the chunk, to keep a step's writes under VOS's cap: what it did not
            // take goes again from where it stopped.
            let mut ended = false;
            while !chunk.is_empty() {
                let arguments = [at.encode(), chunk.encode()].concat();
                match host.import_chunk(&arguments)? {
                    Imported::Taken { next, ended: end } => {
                        let taken = next
                            .checked_sub(*at)
                            .and_then(|n| usize::try_from(n).ok())
                            .filter(|n| *n > 0 && *n <= chunk.len())
                            .ok_or_else(|| unavailable("the program took no entry of a chunk"))?;
                        chunk.drain(..taken);
                        *at = next;
                        ended = end;
                    }
                    Imported::Refused(why) => return Err(refused(format!("{why:?}"))),
                }
            }
            Ok(ended)
        };
        for entry in stream {
            if limit == Some(sent) {
                send(self, &mut chunk, &mut at)?;
                return Ok(false);
            }
            let entry = entry?;
            check.push(&entry).map_err(|e| refused(e.to_string()))?;
            if let SnapshotEntry::Header {
                host_version,
                state_machine_version,
            } = &entry
                && (host_version.as_str() != contract::VERSION
                    || state_machine_version.as_str() != machine::VERSION)
            {
                return Err(refused(format!(
                    "this host reads host contract {} and state machine {}, not {} and {}",
                    contract::VERSION,
                    machine::VERSION,
                    host_version.as_str(),
                    state_machine_version.as_str()
                )));
            }
            let line = serde_json::to_vec(&entry).map_err(|e| unavailable(e.to_string()))?;
            if !chunk.is_empty()
                && (chunk.len() >= IMPORT_ENTRIES || bytes + line.len() > IMPORT_BYTES)
            {
                send(self, &mut chunk, &mut at)?;
                bytes = 0;
            }
            bytes += line.len();
            chunk.push(line);
            sent += 1;
        }
        if limit == Some(sent) {
            send(self, &mut chunk, &mut at)?;
            return Ok(false);
        }
        check.finish().map_err(|e| refused(e.to_string()))?;
        ended |= send(self, &mut chunk, &mut at)?;
        if !ended {
            return Err(refused("the program did not take the stream's end"));
        }
        Ok(true)
    }
}

impl<M: StateMachine> Host<M> for VosHost {
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
        let arguments = submit_arguments(&input)?;
        if point == Some(CrashPoint::AfterComputed) {
            return Err(self.stop_after_computed(arguments));
        }
        let answer: Submitted = self.call(self.methods.submit, &arguments)?;
        if point == Some(CrashPoint::AfterDurable) && matches!(answer, Submitted::Recorded { .. }) {
            return Err(self.stop(CrashPoint::AfterDurable));
        }
        delivery(input, answer)
    }

    fn orders(&self) -> Result<Vec<Id>, HostError> {
        let mut out = Vec::new();
        let mut from = 1u64;
        loop {
            let page =
                self.query::<Vec<(u64, OrderRow)>>(self.methods.orders_from, &from.encode())?;
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
        let state = self.query::<Option<Vec<u8>>>(
            self.methods.state,
            &order.as_str().as_bytes().to_vec().encode(),
        )?;
        state.map(|s| parse(&s, "a state")).transpose()
    }

    fn journal(&self, order: &Id) -> Result<Vec<TransitionRecord>, HostError> {
        let Some((place, row)) = self.order_row(order)? else {
            return Ok(Vec::new());
        };
        let mut out = Vec::new();
        let mut from = 1u64;
        while from <= row.len {
            let page = self.query::<Vec<(u64, RecordRow)>>(
                self.methods.records_from,
                &[place.encode(), from.encode()].concat(),
            )?;
            let Some((last, _)) = page.last() else {
                return Err(unavailable("a record of the journal is missing"));
            };
            from = last + 1;
            for (seq, record) in page {
                out.push(record_of(order, seq, &record)?);
            }
        }
        Ok(out)
    }

    fn refusals(&self) -> Result<Vec<RefusalRecord>, HostError> {
        let mut out = Vec::new();
        let mut from = 1u64;
        loop {
            let page =
                self.query::<Vec<(u64, RefusalRow)>>(self.methods.refusals_from, &from.encode())?;
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
        self.call::<()>(self.methods.halt, &[])
    }

    fn resume(&mut self) -> Result<(), HostError> {
        self.refuse_if_stopped()?;
        self.call::<()>(self.methods.resume, &[])
    }

    fn export(&self) -> Result<SnapshotStream<'_>, HostError> {
        let status = self.status()?;
        if !status.halted {
            return Err(HostError::NotHalted);
        }
        let mut entries = vec![SnapshotEntry::Header {
            host_version: Version::new(contract::VERSION).expect("MAJOR.MINOR.PATCH"),
            state_machine_version: Version::new(machine::VERSION).expect("MAJOR.MINOR.PATCH"),
        }];
        let mut records = 0u64;
        let mut from = 1u64;
        loop {
            let page = self.released(from, true)?;
            let Some(last) = page.last() else { break };
            from = last.index + 1;
            for released in page {
                records += 1;
                entries.push(SnapshotEntry::Record(record_of(
                    &order_id(released.order)?,
                    released.seq,
                    &released.row,
                )?));
            }
        }
        let refusals = Host::<Stm>::refusals(self)?;
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

impl VosHost {
    /// The after-computed stop: every node holds its commits (`hold_commits`, a faults build), the submit is
    /// sent, and once the leader's `refine` stage has counted it (the program ran: computed) every node is
    /// killed. Held, no node can have made it durable. On a scored build nothing can be held: the submit is sent
    /// and the nodes killed once it computed, and the note says the cell is not this row's test there.
    fn stop_after_computed(&mut self, arguments: Vec<u8>) -> HostError {
        if self.cluster.config.build.faults
            && let Err(e) = self.cluster.set_faults_all("hold_commits")
        {
            return unavailable(e);
        }
        let leader = self.cluster.leader().unwrap_or(1);
        let before = self.cluster.stage_count(leader, "refine");
        let routes = self.cluster.routes();
        let target = self.cluster.target(leader);
        let (agent, method) = (self.agent, self.methods.submit);
        let sending = std::thread::spawn(move || {
            let mut client = Client::new(
                &target,
                ed25519_dalek::SigningKey::from_bytes(&crate::cluster::CLIENT_SEED),
            )
            .with_routes(routes);
            let _ = client.call(agent, INSTANCE, method, &arguments);
        });
        let until = Instant::now() + Duration::from_secs(20);
        while self.cluster.stage_count(leader, "refine") <= before && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        let computed = self.cluster.stage_count(leader, "refine") > before;
        let stopped = self.stop(CrashPoint::AfterComputed);
        let _ = sending.join();
        if !computed {
            return unavailable(
                "stopped at AfterComputed, but the leader's refine stage never counted the submit",
            );
        }
        stopped
    }
}

/// The harness the suite drives the VOS program through: each store a cluster's directory under `base`.
pub struct VosHarness<M> {
    pub config: Arc<ClusterConfig>,
    pub base: PathBuf,
    prefix: String,
    next: u64,
    machine: PhantomData<M>,
}

impl<M> VosHarness<M> {
    pub fn new(config: ClusterConfig, base: PathBuf, prefix: &str) -> Self {
        VosHarness {
            config: Arc::new(config),
            base,
            prefix: prefix.into(),
            next: 0,
            machine: PhantomData,
        }
    }

    pub fn fresh(&mut self) -> Result<VosStore, HarnessError> {
        self.next += 1;
        let dir = self.base.join(format!("{}{}", self.prefix, self.next));
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
}

impl<M: StateMachine> Harness<M> for VosHarness<M> {
    type Host = VosHost;

    fn fresh_store(&mut self) -> Result<VosStore, HarnessError> {
        self.fresh()
    }

    fn start(&mut self, store: VosStore) -> Result<VosHost, HarnessError> {
        Ok(<VosHost as Host<M>>::open(store)?)
    }

    fn kill(&mut self, host: VosHost) -> Result<VosStore, HarnessError> {
        Ok(host.kill())
    }

    fn reopen(&mut self, store: VosStore) -> Result<VosHost, HarnessError> {
        Ok(<VosHost as Host<M>>::open(store)?)
    }

    fn import(
        &mut self,
        store: VosStore,
        stream: SnapshotStream<'_>,
    ) -> Result<VosHost, HarnessError> {
        Ok(<VosHost as Host<M>>::import(store, stream)?)
    }

    fn import_crashing_after(
        &mut self,
        store: VosStore,
        stream: SnapshotStream<'_>,
        entries: u64,
    ) -> Result<VosStore, HarnessError> {
        let mut host = VosHost::boot(store)?;
        host.import_from(stream, Some(entries))?;
        Ok(host.kill())
    }
}

impl<M: StateMachine> FaultSeam<M> for VosHarness<M> {
    fn crash_at(&mut self, host: &mut VosHost, point: CrashPoint) {
        host.crash = Some(point);
    }
}
