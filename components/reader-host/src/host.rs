//! The host: its readers, each behind a worker of its own, and the signing of each answer (`T-015-04` parts 2, 3 and
//! 6).

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rutent_contracts::reader::{Answered, ReaderAnswer, ReaderQuestion, Reason};
use rutent_contracts::router_api::Id;

use crate::config::{ConfigError, HostConfig, ReaderConfig};
use crate::egress::{Network, ReaderEgress};
use crate::keys::{Envelope, resolve_key, sign_answer};
use crate::records::CallRecords;
use crate::time::{now_rfc3339, past};
use crate::worker::{Event, PAST_DEADLINE, Reply, Shared, Worker};
use rutent_prov::keys::Resolver;

/// Where a reader stands, as the host reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderState {
    /// Its process is running its start-up checks: every question is unreadable.
    Starting,
    /// Its checks passed: it answers.
    Ready,
    /// Its process ended, hung, hit a limit or failed its checks, and waits to be started again: every question is
    /// unreadable.
    Down,
}

struct Reader {
    config: Arc<ReaderConfig>,
    events: Sender<Event>,
    shared: Arc<Shared>,
}

/// What the host is handed besides its configuration: the store it resolves the readers' keys from, the network it
/// reaches for them, and where its records go.
pub struct Services {
    /// The readers' statement keys: `T-014-01`'s resolver, the keys of a vault bucket in production.
    pub keys: Arc<dyn Resolver + Send + Sync>,
    /// The network.
    pub network: Network,
    /// The records of the readers' calls.
    pub records: Arc<dyn CallRecords>,
}

/// The reader host: several readers, each its own child process behind a worker of its own, answering Core's
/// questions signed with each reader's key.
pub struct Host {
    config: HostConfig,
    keys: Arc<dyn Resolver + Send + Sync>,
    readers: BTreeMap<String, Reader>,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

impl core::fmt::Debug for Host {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Host")
            .field("readers", &self.readers.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl Host {
    /// Starts every reader `config` names, each refusing until its start-up checks pass.
    pub fn start(config: HostConfig, services: Services) -> Result<Self, ConfigError> {
        config.check()?;
        let mut readers = BTreeMap::new();
        let mut workers = Vec::new();
        for reader in &config.readers {
            let reader = Arc::new(reader.clone());
            let egress = ReaderEgress::new(
                reader.clone(),
                services.network.clone(),
                services.records.clone(),
            )
            .ok_or_else(|| {
                ConfigError(format!(
                    "reader {}: its pool can't be declared",
                    reader.name
                ))
            })?;
            let (events, receiver) = mpsc::channel();
            let shared = Arc::new(Shared::new());
            let worker = Worker::new(
                reader.clone(),
                egress,
                shared.clone(),
                receiver,
                events.clone(),
            );
            let handle = std::thread::Builder::new()
                .name(format!("reader {}", reader.name))
                .spawn(move || worker.run())
                .map_err(|e| ConfigError(format!("reader {}: no worker: {e}", reader.name)))?;
            workers.push(handle);
            readers.insert(
                reader.name.as_str().to_owned(),
                Reader {
                    config: reader,
                    events,
                    shared,
                },
            );
        }
        Ok(Host {
            config,
            keys: services.keys,
            readers,
            workers: Mutex::new(workers),
        })
    }

    /// The configuration the host runs.
    pub fn config(&self) -> &HostConfig {
        &self.config
    }

    /// Answers a question Core sent, as the bytes it sent: the reader's signed answer, as the statement's bytes, or
    /// `None` where there is no reader to sign for (a question that doesn't read as one, a reader this host doesn't
    /// run, or a key the store can't give), which Core reads as unreadable.
    ///
    /// The answer names the question's request and what it asked, and the reader's configured version, whatever the
    /// reader said: the host signs the reader's own answer only if it names them and holds to its shape, and an
    /// unreadable one otherwise. The question's deadline is Core's, which Core holds the answer to on its own clock;
    /// the host reads it only to drop a question already past it on the host's clock, on arrival or when its turn
    /// comes, and answers that one with nothing (`T-015-05`), so its queue never fills with questions no answer can
    /// meet.
    pub fn answer(&self, question: &[u8]) -> Option<Vec<u8>> {
        let question: ReaderQuestion = serde_json::from_slice(question).ok()?;
        let reader = self.readers.get(question.reader.as_str())?;
        if past(&question.deadline) {
            return None;
        }
        let reply = if question.network != reader.config.network {
            Err("question_about_another_network")
        } else {
            self.ask(reader, &question)
        };
        if matches!(reply, Err(why) if why == PAST_DEADLINE) {
            return None;
        }
        let body = reply.unwrap_or_else(|why| ReaderAnswer {
            request: question.request.clone(),
            network: question.network.clone(),
            version: reader.config.version.clone(),
            asked: question.asked.clone(),
            answer: Answered::Unreadable {
                why: Reason::new(why)
                    .unwrap_or_else(|| Reason::new("unreadable").expect("a reason")),
            },
        });
        let due = Some(Instant::now() + reader.config.limits.egress_give_up_after());
        let secret = resolve_key(
            &*self.keys,
            &reader.config.name,
            &reader.config.statement_key,
            &self.config.deployment,
            due,
        )
        .ok()?;
        let made_at = now_rfc3339();
        let envelope = Envelope {
            deployment: &self.config.deployment,
            protocol_version: &self.config.protocol_version,
            maker: &reader.config.name,
            key: &reader.config.statement_key,
            made_at: &made_at,
        };
        sign_answer(&envelope, body, &secret).ok()
    }

    fn ask(&self, reader: &Reader, question: &ReaderQuestion) -> Reply {
        let limit = reader.config.limits.pending as usize;
        let pending = &reader.shared.pending;
        if pending.fetch_add(1, Ordering::SeqCst) >= limit {
            pending.fetch_sub(1, Ordering::SeqCst);
            return Err("too_many_pending_questions");
        }
        let (reply, answer) = mpsc::channel();
        if reader
            .events
            .send(Event::Ask(question.clone(), reply))
            .is_err()
        {
            pending.fetch_sub(1, Ordering::SeqCst);
            return Err("reader_down");
        }
        // The worker replies to every question it takes: answered, or unreadable once the reader hangs, crashes or the
        // host stops. The asker waits no longer than every question ahead of it and its own could take, each at most
        // its time bound and one egress call past it, so a link thread is held by one reader's share alone.
        answer
            .recv_timeout(ask_bound(&reader.config))
            .unwrap_or(Err("reader_hung"))
    }

    /// Where the reader `name` stands, if the host runs it.
    pub fn state(&self, name: &Id) -> Option<ReaderState> {
        let reader = self.readers.get(name.as_str())?;
        reader.shared.state.lock().ok().map(|s| *s)
    }

    /// Waits up to `within` for the reader `name` to pass its start-up checks; whether it did.
    pub fn wait_ready(&self, name: &Id, within: Duration) -> bool {
        let until = Instant::now() + within;
        loop {
            if self.state(name) == Some(ReaderState::Ready) {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// The last lines of the reader `name`'s log: what its process said, and what the host said of it.
    pub fn log(&self, name: &Id) -> Vec<String> {
        self.readers
            .get(name.as_str())
            .and_then(|r| {
                r.shared
                    .log
                    .lock()
                    .ok()
                    .map(|log| log.iter().cloned().collect())
            })
            .unwrap_or_default()
    }

    /// How many times the reader `name`'s process was started.
    pub fn starts(&self, name: &Id) -> usize {
        self.readers
            .get(name.as_str())
            .map_or(0, |r| r.shared.starts.load(Ordering::SeqCst))
    }
}

/// How long one asker waits for a reader's reply: the questions that may wait ahead of it and its own, each at most
/// `hang_after_ms` and one egress call's bound past it, which is when the worker next reads its bound.
pub(crate) fn ask_bound(config: &ReaderConfig) -> Duration {
    let limits = &config.limits;
    (limits.hang_after() + limits.egress_give_up_after()) * (limits.pending + 1)
}

impl Drop for Host {
    /// Stops every reader's process, and answers whatever is still waiting as unreadable.
    fn drop(&mut self) {
        for reader in self.readers.values() {
            let _ = reader.events.send(Event::Stop);
        }
        if let Ok(mut workers) = self.workers.lock() {
            for worker in workers.drain(..) {
                let _ = worker.join();
            }
        }
    }
}
