//! One reader's worker: the thread that owns its process, starts it, hands it questions one at a time, makes its
//! egress calls, and restarts it when it crashes, hangs or hits a limit (`T-015-04` parts 2 and 5).
//!
//! Every event reaches the worker on one channel: a question, a frame from the reader's process (read on a thread of
//! its own, bounded), or the process's end. The worker is in one phase at a time:
//!
//! - **starting**: the process runs its start-up checks; every question is unreadable (`checks_not_passed`);
//! - **ready**: the checks passed, and their identity is the configuration's; the next question is handed over;
//! - **asking**: one question is with the reader; the others wait, up to the reader's bound;
//! - **down**: the process ended, hung, hit a limit or failed its checks; every question is unreadable until the host
//!   starts it again.

use std::collections::VecDeque;
use std::io::Read;
use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rutent_contracts::module_interface::Identifier;
use rutent_contracts::reader::{
    Answered, NetworkIdentity, ReaderAnswer, ReaderQuestion, TrustRule,
};

use crate::config::{READ_FRAMES, ReaderConfig, WRITE_BYTES, WRITE_FRAMES};
use crate::egress::{ReaderEgress, Serving};
use crate::host::ReaderState;
use crate::jail;
use crate::records::START_UP;
use crate::time::past;
use crate::wire::{FromReader, RpcResult, ToReader, read_frame, write_frame};

/// The most lines the host keeps of a reader's log.
const LOG_LINES: usize = 256;

/// The longest log line kept, in bytes.
const LOG_LINE: usize = 4096;

/// What the host asks an RPC pool for itself, to hold an EVM reader's reported chain id to the pool's.
const CHAIN_ID: &str = r#"{"jsonrpc":"2.0","id":1,"method":"eth_chainId"}"#;

/// What the worker gives back for a question: the answer's body to sign, or why it is unreadable.
pub(crate) type Reply = Result<ReaderAnswer, &'static str>;

/// Why a question is dropped: its deadline passed, on the host's clock, before the reader was handed it. The host
/// answers it with nothing (`T-015-05`).
pub(crate) const PAST_DEADLINE: &str = "question_past_deadline";

/// What reaches a worker.
pub(crate) enum Event {
    /// A question, and where its reply goes.
    Ask(ReaderQuestion, Sender<Reply>),
    /// A frame from the process of this generation, and its size on the wire.
    Frame(u64, FromReader, usize),
    /// The process of this generation ended its channel, broke it or went over a bound: why.
    Gone(u64, &'static str),
    /// The host is stopping.
    Stop,
}

/// What the host and the worker share about one reader.
#[derive(Debug)]
pub(crate) struct Shared {
    /// Its phase, as the host reports it.
    pub state: Mutex<ReaderState>,
    /// The last lines of its log.
    pub log: Mutex<VecDeque<String>>,
    /// Questions submitted and not yet replied to.
    pub pending: AtomicUsize,
    /// How many times its process was started.
    pub starts: AtomicUsize,
}

impl Shared {
    pub(crate) fn new() -> Self {
        Shared {
            state: Mutex::new(ReaderState::Starting),
            log: Mutex::new(VecDeque::new()),
            pending: AtomicUsize::new(0),
            starts: AtomicUsize::new(0),
        }
    }
}

/// The frames of one reader's process waiting for its worker: counted when its output thread reads them, and again
/// when the worker takes them, so a reader that writes faster than the host reads is stopped, never held in memory.
#[derive(Debug, Default)]
pub(crate) struct Gauge {
    frames: AtomicUsize,
    bytes: AtomicUsize,
    /// Why the reader's process is to be stopped, as its output or input thread first found it: the worker reads it on
    /// every turn, ahead of the frames still waiting.
    stopped: Mutex<Option<&'static str>>,
}

impl Gauge {
    /// Counts a frame of `size` bytes in; false if that takes it over `frames` frames or `bytes` bytes.
    fn take(&self, size: usize, frames: usize, bytes: usize) -> bool {
        let n = self.frames.fetch_add(1, Ordering::SeqCst) + 1;
        let b = self.bytes.fetch_add(size, Ordering::SeqCst) + size;
        n <= frames && b <= bytes
    }

    /// Counts a frame of `size` bytes out.
    fn give(&self, size: usize) {
        self.frames.fetch_sub(1, Ordering::SeqCst);
        self.bytes.fetch_sub(size, Ordering::SeqCst);
    }

    /// Marks the process to be stopped for `why`, unless it already is for another reason; gives the reason it is.
    fn stop(&self, why: &'static str) -> &'static str {
        match self.stopped.lock() {
            Ok(mut stopped) => stopped.get_or_insert(why),
            Err(_) => why,
        }
    }

    /// Why the process is to be stopped, if it is.
    fn stopped(&self) -> Option<&'static str> {
        self.stopped.lock().ok().and_then(|s| *s)
    }
}

/// Reads frames from a reader's output and hands them to its worker, until the output ends, a frame breaks the
/// wire or its bound, or the frames waiting reach the gauge's bound: what the reader's output thread runs.
pub(crate) fn forward_frames(
    output: &mut impl Read,
    to: &Sender<Event>,
    generation: u64,
    gauge: &Gauge,
    max_frame: usize,
) {
    let waiting_bytes = max_frame.saturating_mul(4);
    loop {
        let event = match read_frame(output, max_frame) {
            Ok(Some(bytes)) => match serde_json::from_slice::<FromReader>(&bytes) {
                Ok(_) if !gauge.take(bytes.len(), READ_FRAMES, waiting_bytes) => {
                    Event::Gone(generation, gauge.stop("reader_hit_a_limit"))
                }
                Ok(frame) => Event::Frame(generation, frame, bytes.len()),
                Err(_) => Event::Gone(generation, "reader_sent_malformed_frame"),
            },
            Ok(None) => Event::Gone(generation, "reader_crashed"),
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                Event::Gone(generation, gauge.stop("reader_hit_a_limit"))
            }
            Err(_) => Event::Gone(generation, "reader_crashed"),
        };
        let last = matches!(event, Event::Gone(..));
        if to.send(event).is_err() || last {
            return;
        }
    }
}

enum Phase {
    Starting {
        until: Instant,
    },
    Ready,
    Asking {
        question: ReaderQuestion,
        reply: Sender<Reply>,
        until: Instant,
        tainted: bool,
        agreed: bool,
    },
    Down {
        restart_at: Instant,
        why: &'static str,
    },
}

struct Running {
    child: Child,
    pid: u32,
    /// The frames for the reader, written by a thread of their own, so a reader that stops reading never holds the
    /// worker: a queue of at most [`WRITE_FRAMES`] frames and [`WRITE_BYTES`] bytes.
    writer: SyncSender<Vec<u8>>,
    unwritten: Arc<AtomicUsize>,
    gauge: Arc<Gauge>,
    workdir: PathBuf,
    /// Whether the host has held the process to its jail: before then it is served nothing.
    confined: bool,
    /// The egress calls asked for in the phase it is in.
    calls: u32,
}

impl Running {
    fn stop(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.workdir);
    }
}

pub(crate) struct Worker {
    config: Arc<ReaderConfig>,
    egress: ReaderEgress,
    shared: Arc<Shared>,
    events: Receiver<Event>,
    sender: Sender<Event>,
    running: Option<Running>,
    generation: u64,
    phase: Phase,
    queue: VecDeque<(ReaderQuestion, Sender<Reply>)>,
}

impl Worker {
    pub(crate) fn new(
        config: Arc<ReaderConfig>,
        egress: ReaderEgress,
        shared: Arc<Shared>,
        events: Receiver<Event>,
        sender: Sender<Event>,
    ) -> Self {
        Worker {
            config,
            egress,
            shared,
            events,
            sender,
            running: None,
            generation: 0,
            phase: Phase::Down {
                restart_at: Instant::now(),
                why: "checks_not_passed",
            },
            queue: VecDeque::new(),
        }
    }

    fn set_state(&self, state: ReaderState) {
        if let Ok(mut s) = self.shared.state.lock() {
            *s = state;
        }
    }

    fn reply(&self, to: &Sender<Reply>, reply: Reply) {
        self.shared.pending.fetch_sub(1, Ordering::SeqCst);
        let _ = to.send(reply);
    }

    fn log(&self, line: String) {
        let mut line = line;
        if line.len() > LOG_LINE {
            let mut end = LOG_LINE;
            while !line.is_char_boundary(end) {
                end -= 1;
            }
            line.truncate(end);
        }
        if let Ok(mut log) = self.shared.log.lock() {
            if log.len() == LOG_LINES {
                log.pop_front();
            }
            log.push_back(line);
        }
    }

    fn timer(&self) -> Option<Instant> {
        match &self.phase {
            Phase::Starting { until } | Phase::Asking { until, .. } => Some(*until),
            Phase::Down { restart_at, .. } => Some(*restart_at),
            Phase::Ready => None,
        }
    }

    /// Runs until the host stops.
    pub(crate) fn run(mut self) {
        self.start();
        loop {
            // The phase's bound is read on every turn, not only when no event comes: a reader that keeps talking
            // is stopped at its bound as one that is silent is.
            if let Some(why) = self.running.as_ref().and_then(|r| r.gauge.stopped()) {
                self.down(why);
                continue;
            }
            let timer = self.timer();
            if let Some(at) = timer
                && Instant::now() >= at
            {
                self.on_timer();
                continue;
            }
            let event = match timer {
                Some(at) => match self
                    .events
                    .recv_timeout(at.saturating_duration_since(Instant::now()))
                {
                    Ok(event) => Some(event),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => Some(Event::Stop),
                },
                None => Some(self.events.recv().unwrap_or(Event::Stop)),
            };
            match event {
                None => self.on_timer(),
                Some(Event::Stop) => break,
                Some(Event::Ask(question, reply)) => self.on_ask(question, reply),
                // What the process of this generation sent before it was stopped is dropped with it.
                Some(Event::Frame(generation, frame, size))
                    if generation == self.generation && self.running.is_some() =>
                {
                    if let Some(running) = &self.running {
                        running.gauge.give(size);
                    }
                    self.on_frame(frame)
                }
                Some(Event::Gone(generation, why))
                    if generation == self.generation && self.running.is_some() =>
                {
                    self.down(why)
                }
                Some(_) => {}
            }
        }
        self.down("host_stopping");
        while let Ok(event) = self.events.try_recv() {
            if let Event::Ask(_, reply) = event {
                self.reply(&reply, Err("host_stopping"));
            }
        }
    }

    /// Starts the reader's process, in its jail, and hands it its start frame. A start that fails is a reader down,
    /// started again after its wait.
    fn start(&mut self) {
        self.egress.reset();
        self.generation += 1;
        let generation = self.generation;
        self.shared.starts.fetch_add(1, Ordering::SeqCst);
        // A directory of its own, new, mode 0700: one already there is refused, and left as it is (`T-015-05`).
        let workdir =
            match jail::workdir(&std::env::temp_dir(), self.config.name.as_str(), generation) {
                Ok(workdir) => workdir,
                Err(e) => {
                    self.log(format!(
                        "host: the reader's working directory wasn't made: {e}"
                    ));
                    self.phase = Phase::Down {
                        restart_at: Instant::now() + self.config.limits.restart_after(),
                        why: "reader_did_not_start",
                    };
                    self.set_state(ReaderState::Down);
                    return;
                }
            };
        let started = jail::command(&self.config, &workdir).and_then(|mut command| command.spawn());
        let mut child = match started {
            Ok(child) => child,
            Err(e) => {
                self.log(format!("host: the reader didn't start: {e}"));
                let _ = std::fs::remove_dir_all(&workdir);
                self.phase = Phase::Down {
                    restart_at: Instant::now() + self.config.limits.restart_after(),
                    why: "reader_did_not_start",
                };
                self.set_state(ReaderState::Down);
                return;
            }
        };
        let (Some(mut stdin), Some(mut stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            self.phase = Phase::Down {
                restart_at: Instant::now() + self.config.limits.restart_after(),
                why: "reader_did_not_start",
            };
            self.set_state(ReaderState::Down);
            return;
        };
        let (sender, gauge) = (self.sender.clone(), Arc::new(Gauge::default()));
        let max = self.config.limits.max_frame as usize;
        let reading = gauge.clone();
        std::thread::spawn(move || forward_frames(&mut stdout, &sender, generation, &reading, max));
        let (writer, frames) = std::sync::mpsc::sync_channel::<Vec<u8>>(WRITE_FRAMES);
        let unwritten = Arc::new(AtomicUsize::new(0));
        let (written, gone, writing) = (unwritten.clone(), self.sender.clone(), gauge.clone());
        std::thread::spawn(move || {
            for frame in frames {
                if write_frame(&mut stdin, &frame).is_err() {
                    let _ = gone.send(Event::Gone(generation, writing.stop("reader_crashed")));
                    return;
                }
                written.fetch_sub(frame.len(), Ordering::SeqCst);
            }
        });
        let pid = child.id();
        self.running = Some(Running {
            child,
            pid,
            writer,
            unwritten,
            gauge,
            workdir,
            confined: false,
            calls: 0,
        });
        self.phase = Phase::Starting {
            until: Instant::now() + self.config.limits.start_within(),
        };
        self.set_state(ReaderState::Starting);
        let start = ToReader::Start {
            reader: self.config.name.clone(),
            network: self.config.network.clone(),
            version: self.config.version.clone(),
            identity: self.config.identity.clone(),
            trust: self.config.trust.clone(),
            light_client: self.config.light_client.clone(),
        };
        self.send(&start);
    }

    /// Hands a frame to the reader's writer. A reader whose frames wait past their bound has stopped reading, and is
    /// stopped; one that can't be written to is gone.
    fn send(&mut self, frame: &ToReader) {
        let queued = match (&self.running, serde_json::to_vec(frame)) {
            (Some(running), Ok(bytes)) => {
                let size = bytes.len();
                if running.unwritten.fetch_add(size, Ordering::SeqCst) + size > WRITE_BYTES {
                    Err("reader_stopped_reading")
                } else {
                    match running.writer.try_send(bytes) {
                        Ok(()) => Ok(()),
                        Err(TrySendError::Full(_)) => Err("reader_stopped_reading"),
                        Err(TrySendError::Disconnected(_)) => Err("reader_crashed"),
                    }
                }
            }
            _ => Err("reader_crashed"),
        };
        if let Err(why) = queued {
            self.down(why);
        }
    }

    /// Stops the process, answers the question it was on and every one waiting as unreadable for `why`, and waits to
    /// start it again.
    fn down(&mut self, why: &'static str) {
        if let Some(running) = self.running.take() {
            running.stop();
        }
        self.egress.reset();
        let phase = std::mem::replace(
            &mut self.phase,
            Phase::Down {
                restart_at: Instant::now() + self.config.limits.restart_after(),
                why,
            },
        );
        if let Phase::Asking { reply, .. } = phase {
            self.reply(&reply, Err(why));
        }
        for (_, reply) in std::mem::take(&mut self.queue) {
            self.reply(&reply, Err("reader_restarting"));
        }
        self.set_state(ReaderState::Down);
    }

    fn on_timer(&mut self) {
        match &self.phase {
            Phase::Starting { .. } => self.down("checks_timed_out"),
            Phase::Asking { .. } => self.down("reader_hung"),
            Phase::Down { restart_at, .. } if Instant::now() >= *restart_at => self.start(),
            _ => {}
        }
    }

    fn on_ask(&mut self, question: ReaderQuestion, reply: Sender<Reply>) {
        match &self.phase {
            Phase::Starting { .. } => self.reply(&reply, Err("checks_not_passed")),
            Phase::Down { why, .. } => {
                let why = *why;
                self.reply(&reply, Err(why))
            }
            Phase::Asking { .. } => self.queue.push_back((question, reply)),
            Phase::Ready => {
                let _ = self.ask(question, reply);
            }
        }
    }

    /// Hands `question` to the reader, or drops it, unasked, if its deadline has passed: whether it was handed over.
    /// A dropped question leaves the phase as it was: the caller takes the next.
    fn ask(&mut self, question: ReaderQuestion, reply: Sender<Reply>) -> bool {
        // A question that waited past its deadline is dropped, never handed over: Core would refuse its answer
        // (`T-015-05`).
        if past(&question.deadline) {
            self.reply(&reply, Err(PAST_DEADLINE));
            return false;
        }
        if let Some(running) = &mut self.running {
            running.calls = 0;
        }
        self.phase = Phase::Asking {
            question: question.clone(),
            reply,
            until: Instant::now() + self.config.limits.hang_after(),
            tainted: false,
            agreed: false,
        };
        self.send(&ToReader::Question { question });
        true
    }

    /// Takes the next question waiting, dropping each one past its deadline on the way, in a loop and never by
    /// recursion: however many dead questions wait, the worker's stack holds one frame (`T-015-05`'s review, L2).
    fn next(&mut self) {
        self.phase = Phase::Ready;
        self.set_state(ReaderState::Ready);
        while let Some((question, reply)) = self.queue.pop_front() {
            if self.ask(question, reply) {
                return;
            }
        }
    }

    fn serving(&self) -> Serving {
        let (request, until) = match &self.phase {
            Phase::Asking {
                question, until, ..
            } => (question.request.as_str(), Some(*until)),
            Phase::Starting { until } => (START_UP, Some(*until)),
            _ => (START_UP, None),
        };
        Serving {
            request: Identifier::new(request)
                .unwrap_or_else(|| Identifier::new(START_UP).expect("non-empty")),
            until,
        }
    }

    /// Counts one egress call in the phase; false once the reader asked for more than its bound.
    fn count_call(&mut self) -> bool {
        let bound = self.config.limits.calls_per_question;
        self.running.as_mut().is_some_and(|r| {
            r.calls += 1;
            r.calls <= bound
        })
    }

    /// For an RPC pool's reader, the chain id the pool itself agrees on, asked by the host, never taken from the
    /// reader's report alone: whether it is the configured one.
    fn pool_names_the_configured_chain(&mut self) -> bool {
        let (TrustRule::RpcPool { endpoints, .. }, NetworkIdentity::ChainId { id }) =
            (&self.config.trust, &self.config.identity)
        else {
            return true;
        };
        let endpoints: Vec<String> = endpoints.iter().map(|e| e.as_str().to_owned()).collect();
        let serving = self.serving();
        let (RpcResult::Agreed { body }, false) = self.egress.rpc(&serving, &endpoints, CHAIN_ID)
        else {
            return false;
        };
        serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["result"].as_str().map(str::to_owned))
            .and_then(|hex| u128::from_str_radix(hex.strip_prefix("0x")?, 16).ok())
            .is_some_and(|n| n.to_string() == id.as_str())
    }

    fn on_frame(&mut self, frame: FromReader) {
        if let FromReader::Log { line } = frame {
            self.log(line);
            return;
        }
        // Nothing but its log is taken from a process the host hasn't held to its jail: not a call, not a check, not
        // an answer.
        if let Some(running) = &mut self.running
            && !running.confined
        {
            if let Some(why) = jail::check_confined(running.pid) {
                self.log(format!("host: the reader is not confined: {why}"));
                self.down(why);
                return;
            }
            running.confined = true;
        }
        if matches!(
            frame,
            FromReader::Rpc { .. } | FromReader::Open { .. } | FromReader::Send { .. }
        ) && !self.count_call()
        {
            self.log("host: the reader asked for more calls than its bound".to_owned());
            self.down("reader_hit_a_limit");
            return;
        }
        match frame {
            FromReader::Log { .. } => {}
            FromReader::Checked { identity } => {
                if matches!(self.phase, Phase::Starting { .. }) {
                    if identity != self.config.identity {
                        self.log("host: start-up checks found another network".to_owned());
                        self.down("checks_failed");
                    } else if !self.pool_names_the_configured_chain() {
                        self.log("host: the pool found another network".to_owned());
                        self.down("checks_failed");
                    } else {
                        self.log("host: start-up checks passed".to_owned());
                        self.next();
                    }
                }
            }
            FromReader::Rpc {
                call,
                endpoints,
                body,
            } => {
                let serving = self.serving();
                let (result, undeclared) = self.egress.rpc(&serving, &endpoints, &body);
                let ok = matches!(result, RpcResult::Agreed { .. });
                if let Phase::Asking {
                    tainted, agreed, ..
                } = &mut self.phase
                {
                    // Only what the quorum agreed on may stand behind an answer; and a reader that asks for a
                    // destination it didn't declare isn't keeping to its list.
                    *tainted |= undeclared || !ok;
                    *agreed |= ok;
                }
                self.send(&ToReader::Rpc { call, result });
            }
            FromReader::Open {
                call,
                protocol,
                host,
                port,
            } => {
                let serving = self.serving();
                let result = self.egress.open(&serving, protocol, &host, port);
                self.send(&ToReader::Opened { call, result });
            }
            FromReader::Send { conn, bytes } => {
                let serving = self.serving();
                let result = self.egress.send(&serving, conn, &bytes);
                self.send(&ToReader::Received { conn, result });
            }
            FromReader::Close { conn } => self.egress.close(conn),
            FromReader::Answer { answer } => {
                let Phase::Asking {
                    question,
                    reply,
                    tainted,
                    agreed,
                    ..
                } = std::mem::replace(&mut self.phase, Phase::Ready)
                else {
                    // An answer to nothing asked: a reader not keeping to the wire.
                    self.down("reader_answered_unasked");
                    return;
                };
                let held = match serde_json::from_value::<ReaderAnswer>(answer) {
                    Ok(body) => holds(&self.config, &question, &body).map(|()| body),
                    Err(_) => Err("answer_breaks_its_shape"),
                };
                // An RPC pool's reader answers a movement only from what the pool agreed on in this question.
                let pool = matches!(self.config.trust, TrustRule::RpcPool { .. });
                let reply_value = match held {
                    Ok(body) if tainted && !matches!(body.answer, Answered::Unreadable { .. }) => {
                        Err("trust_rule_not_met")
                    }
                    Ok(body)
                        if pool
                            && !agreed
                            && matches!(
                                body.answer,
                                Answered::MovementStatus { .. } | Answered::MovementOf { .. }
                            ) =>
                    {
                        Err("trust_rule_not_met")
                    }
                    other => other,
                };
                self.reply(&reply, reply_value);
                self.next();
            }
        }
    }
}

/// Holds a reader's answer to the question it was asked (`T-015-04` part 3): the request, the network and the question
/// as asked, the reader's configured version, its own shape, and, for its network's rules, the identity and the trust
/// rule its configuration names. The host signs nothing else as the reader's answer.
pub(crate) fn holds(
    config: &ReaderConfig,
    question: &ReaderQuestion,
    body: &ReaderAnswer,
) -> Result<(), &'static str> {
    if body.request != question.request {
        return Err("answer_to_another_request");
    }
    if body.network != question.network {
        return Err("answer_about_another_network");
    }
    if body.asked != question.asked {
        return Err("answer_to_another_question");
    }
    if body.version != config.version {
        return Err("answer_of_another_version");
    }
    if body.check().is_err() {
        return Err("answer_breaks_its_shape");
    }
    if let Answered::NetworkRules { rules } = &body.answer
        && (rules.identity != config.identity || rules.trust != config.trust)
    {
        return Err("rules_not_as_configured");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Event, Gauge, PAST_DEADLINE, Phase, Shared, Worker, forward_frames};
    use crate::config::{MAX_PENDING, READ_FRAMES, ReaderConfig, ReaderLimits};
    use crate::egress::{Network, ReaderEgress};
    use crate::records::MemoryRecords;
    use crate::wire::{FromReader, write_frame};

    use std::net::{IpAddr, SocketAddr, TcpStream};
    use std::num::NonZeroU16;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    use ed25519_dalek::SigningKey;
    use rutent_contracts::module_interface::Host as HostName;
    use rutent_contracts::reader::{
        Answered, Asked, ChainId, NetworkIdentity, ReaderAnswer, ReaderQuestion, Reason, RequestId,
        RpcEndpoint, TrustRule,
    };
    use rutent_contracts::router_api::{Id, Timestamp, Version};
    use rutent_prov::egress::{
        ConnectError, Connector, Dial, Dialer, NameResolver, Request, Response,
    };

    /// A network that reaches nothing: the worker under test asks it nothing.
    struct Nowhere;

    impl NameResolver for Nowhere {
        fn resolve(&self, _host: &HostName, _port: NonZeroU16) -> Vec<IpAddr> {
            Vec::new()
        }
    }

    impl Connector for Nowhere {
        fn exchange(&self, _dial: &Dial<'_>, _request: &Request) -> Result<Response, ConnectError> {
            Err(ConnectError::Unreachable)
        }
    }

    impl Dialer for Nowhere {
        fn connect(&self, _to: SocketAddr, _timeout: Duration) -> std::io::Result<TcpStream> {
            Err(std::io::Error::other("nowhere"))
        }
    }

    fn question(n: u8, deadline: &str) -> ReaderQuestion {
        let mut bytes = [0x5au8; 16];
        bytes[0] = n;
        ReaderQuestion {
            request: RequestId::from_random(bytes).expect("a request id"),
            reader: Id::new("base").unwrap(),
            network: Id::new("base").unwrap(),
            deadline: Timestamp::new(deadline).unwrap(),
            asked: Asked::MovementStatus {
                movement: Id::new("0xmove").unwrap(),
            },
        }
    }

    // verifies: REQ-MDL-11
    #[test]
    fn thousands_of_expired_questions_behind_a_slow_one_are_dropped_without_growing_the_workers_stack()
     {
        let config = Arc::new(ReaderConfig {
            name: Id::new("base").unwrap(),
            network: Id::new("base").unwrap(),
            version: Version::new("0.1.0").unwrap(),
            identity: NetworkIdentity::ChainId {
                id: ChainId::new("8453").unwrap(),
            },
            trust: TrustRule::RpcPool {
                endpoints: vec![RpcEndpoint::new("https://rpc-a.base.example").unwrap()],
                quorum: 1,
            },
            program: "/nonexistent".into(),
            args: Vec::new(),
            statement_key: rutent_conformance::signing::public_key(&SigningKey::from_bytes(
                &[3u8; 32],
            )),
            light_client: None,
            limits: ReaderLimits {
                pending: MAX_PENDING,
                ..ReaderLimits::default()
            },
        });
        let network = Network {
            resolver: Arc::new(Nowhere),
            connector: Arc::new(Nowhere),
            dialer: Arc::new(Nowhere),
        };
        let egress =
            ReaderEgress::new(config.clone(), network, Arc::new(MemoryRecords::new())).unwrap();
        let (events, receiver) = std::sync::mpsc::channel();
        let mut worker = Worker::new(config, egress, Arc::new(Shared::new()), receiver, events);

        // The slow question is with the reader; behind it wait as many questions as the bound lets, each of them
        // already past its deadline.
        let slow = question(0, "2099-01-01T00:00:00Z");
        let (slow_reply, slow_replied) = std::sync::mpsc::channel();
        worker.phase = Phase::Asking {
            question: slow.clone(),
            reply: slow_reply,
            until: Instant::now() + Duration::from_secs(60),
            tainted: false,
            agreed: false,
        };
        let dead = question(1, "2000-01-01T00:00:00Z");
        let (dead_reply, dead_replied) = std::sync::mpsc::channel();
        let waiting = MAX_PENDING as usize - 1;
        for _ in 0..waiting {
            worker.queue.push_back((dead.clone(), dead_reply.clone()));
        }
        worker.shared.pending.store(waiting + 1, Ordering::SeqCst);

        // The slow one's answer arrives, and the worker takes the next: on a thread whose stack holds a few frames,
        // so one frame per dead question would overflow it and abort the test.
        let answer = serde_json::to_value(ReaderAnswer {
            request: slow.request.clone(),
            network: slow.network.clone(),
            version: Version::new("0.1.0").unwrap(),
            asked: slow.asked.clone(),
            answer: Answered::Unreadable {
                why: Reason::new("slow").unwrap(),
            },
        })
        .unwrap();
        let worker = std::thread::Builder::new()
            .stack_size(256 << 10)
            .spawn(move || {
                worker.on_frame(FromReader::Answer { answer });
                worker
            })
            .unwrap()
            .join()
            .expect("the worker's thread ended without overflowing its stack");

        assert!(
            slow_replied.try_recv().unwrap().is_ok(),
            "the slow one answered"
        );
        let dropped: Vec<_> = dead_replied.try_iter().collect();
        assert_eq!(dropped.len(), waiting, "every dead question was replied to");
        assert!(
            dropped
                .iter()
                .all(|r| matches!(r, Err(why) if *why == PAST_DEADLINE))
        );
        assert!(worker.queue.is_empty());
        assert!(matches!(worker.phase, Phase::Ready));
        assert_eq!(worker.shared.pending.load(Ordering::SeqCst), 0);
    }

    // verifies: REQ-MDL-11
    #[test]
    fn reader_whose_frames_wait_past_their_bound_is_stopped() {
        let mut wire = Vec::new();
        let frame = serde_json::to_vec(&FromReader::Log {
            line: "x".repeat(100),
        })
        .unwrap();
        for _ in 0..READ_FRAMES + 10 {
            write_frame(&mut wire, &frame).unwrap();
        }
        // Nobody takes the frames: the worker is busy.
        let (to, from) = std::sync::mpsc::channel();
        let gauge = Gauge::default();
        forward_frames(&mut wire.as_slice(), &to, 7, &gauge, 1 << 20);
        let events: Vec<Event> = from.try_iter().collect();
        assert_eq!(
            events.len(),
            READ_FRAMES + 1,
            "forwarded up to the bound, then stopped"
        );
        assert!(matches!(
            events.last(),
            Some(Event::Gone(7, "reader_hit_a_limit"))
        ));

        // By bytes: four frames' worth of the largest frame.
        let (to, from) = std::sync::mpsc::channel();
        forward_frames(&mut wire.as_slice(), &to, 8, &Gauge::default(), frame.len());
        let events: Vec<Event> = from.try_iter().collect();
        assert_eq!(events.len(), 5);
        assert!(matches!(
            events.last(),
            Some(Event::Gone(8, "reader_hit_a_limit"))
        ));
    }
}
