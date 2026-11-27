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
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, ChildStdin};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rutent_contracts::module_interface::Identifier;
use rutent_contracts::reader::{Answered, ReaderAnswer, ReaderQuestion};

use crate::config::ReaderConfig;
use crate::egress::{ReaderEgress, Serving};
use crate::host::ReaderState;
use crate::jail;
use crate::records::START_UP;
use crate::wire::{FromReader, ToReader, read_frame, write_frame};

/// The most lines the host keeps of a reader's log.
const LOG_LINES: usize = 256;

/// The longest log line kept, in bytes.
const LOG_LINE: usize = 4096;

/// What the worker gives back for a question: the answer's body to sign, or why it is unreadable.
pub(crate) type Reply = Result<ReaderAnswer, &'static str>;

/// What reaches a worker.
pub(crate) enum Event {
    /// A question, and where its reply goes.
    Ask(ReaderQuestion, Sender<Reply>),
    /// A frame from the process of this generation.
    Frame(u64, FromReader),
    /// The process of this generation ended its channel, or broke it: why.
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
    },
    Down {
        restart_at: Instant,
        why: &'static str,
    },
}

struct Running {
    child: Child,
    pid: u32,
    stdin: ChildStdin,
    workdir: PathBuf,
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

    /// Runs until the host stops.
    pub(crate) fn run(mut self) {
        self.start();
        loop {
            let timer = match &self.phase {
                Phase::Starting { until } | Phase::Asking { until, .. } => Some(*until),
                Phase::Down { restart_at, .. } => Some(*restart_at),
                Phase::Ready => None,
            };
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
                Some(Event::Frame(generation, frame)) if generation == self.generation => {
                    self.on_frame(frame)
                }
                Some(Event::Gone(generation, why)) if generation == self.generation => {
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
        let workdir = std::env::temp_dir().join(format!(
            "rutent-reader-{}-{}-{generation}",
            self.config.name,
            std::process::id()
        ));
        let started = std::fs::create_dir_all(&workdir)
            .and_then(|()| jail::command(&self.config, &workdir))
            .and_then(|mut command| command.spawn());
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
        let (Some(stdin), Some(mut stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            self.phase = Phase::Down {
                restart_at: Instant::now() + self.config.limits.restart_after(),
                why: "reader_did_not_start",
            };
            self.set_state(ReaderState::Down);
            return;
        };
        let sender = self.sender.clone();
        let max = self.config.limits.max_frame as usize;
        std::thread::spawn(move || {
            loop {
                let event = match read_frame(&mut stdout, max) {
                    Ok(Some(bytes)) => match serde_json::from_slice::<FromReader>(&bytes) {
                        Ok(frame) => Event::Frame(generation, frame),
                        Err(_) => Event::Gone(generation, "reader_sent_malformed_frame"),
                    },
                    Ok(None) => Event::Gone(generation, "reader_crashed"),
                    Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                        Event::Gone(generation, "reader_hit_a_limit")
                    }
                    Err(_) => Event::Gone(generation, "reader_crashed"),
                };
                let last = matches!(event, Event::Gone(..));
                if sender.send(event).is_err() || last {
                    return;
                }
            }
        });
        let pid = child.id();
        self.running = Some(Running {
            child,
            pid,
            stdin,
            workdir,
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

    /// Writes a frame to the reader; a reader that can't be written to is gone.
    fn send(&mut self, frame: &ToReader) {
        let written = match (&mut self.running, serde_json::to_vec(frame)) {
            (Some(running), Ok(bytes)) => {
                write_frame(&mut running.stdin, &bytes).and_then(|()| running.stdin.flush())
            }
            _ => Err(std::io::Error::other("no reader")),
        };
        if written.is_err() {
            self.down("reader_crashed");
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
            Phase::Ready => self.ask(question, reply),
        }
    }

    fn ask(&mut self, question: ReaderQuestion, reply: Sender<Reply>) {
        self.phase = Phase::Asking {
            question: question.clone(),
            reply,
            until: Instant::now() + self.config.limits.hang_after(),
            tainted: false,
        };
        self.send(&ToReader::Question { question });
    }

    fn next(&mut self) {
        self.phase = Phase::Ready;
        self.set_state(ReaderState::Ready);
        if let Some((question, reply)) = self.queue.pop_front() {
            self.ask(question, reply);
        }
    }

    fn serving(&self) -> Serving {
        let request = match &self.phase {
            Phase::Asking { question, .. } => question.request.as_str(),
            _ => START_UP,
        };
        Serving {
            request: Identifier::new(request)
                .unwrap_or_else(|| Identifier::new(START_UP).expect("non-empty")),
        }
    }

    fn on_frame(&mut self, frame: FromReader) {
        match frame {
            FromReader::Log { line } => self.log(line),
            FromReader::Checked { identity } => {
                if matches!(self.phase, Phase::Starting { .. }) {
                    // Its checks are taken only from a reader in the provider's jail, confined as it is.
                    let unconfined = self
                        .running
                        .as_ref()
                        .and_then(|r| jail::check_confined(r.pid));
                    if let Some(why) = unconfined {
                        self.log(format!("host: the reader is not confined: {why}"));
                        self.down(why);
                    } else if identity == self.config.identity {
                        self.log("host: start-up checks passed".to_owned());
                        self.next();
                    } else {
                        self.log("host: start-up checks found another network".to_owned());
                        self.down("checks_failed");
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
                let agreed = matches!(result, crate::wire::RpcResult::Agreed { .. });
                if let Phase::Asking { tainted, .. } = &mut self.phase {
                    // Only what the quorum agreed on may stand behind an answer; and a reader that asks for a
                    // destination it didn't declare isn't keeping to its list.
                    *tainted |= undeclared || !agreed;
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
                let result = self.egress.send(conn, &bytes);
                self.send(&ToReader::Received { conn, result });
            }
            FromReader::Close { conn } => self.egress.close(conn),
            FromReader::Answer { answer } => {
                let Phase::Asking {
                    question,
                    reply,
                    tainted,
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
                let reply_value = match held {
                    Ok(body) if tainted && !matches!(body.answer, Answered::Unreadable { .. }) => {
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
