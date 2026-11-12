//! The scenarios on three VOS replicas (`T-024-02`): `rutent-scenarios`'s checks, run with the state machine on a
//! cluster of three VOS nodes, so they are the reference host's run with only the host changed (`DEC-26`).
//!
//! [`Vos`] is the environment: each host it opens is the host server, `rutent-stm-vos-serve` (`serve/`), on a fresh
//! cluster of three nodes under a directory of the run's own, driven over its wire. The server is named
//! by `STM_VOS_SERVE`, and the nodes' binaries by `../test.sh`'s variables, which it reads; `../scenarios.sh` sets
//! them all. This crate links nothing of VOS: the module platform it runs builds only with a newer toolchain than
//! VOS's crates do.

use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use rutent_contracts::state_machine::host::{Delivery, HostError, SnapshotEntry, TransitionRecord};
use rutent_contracts::state_machine::{Id, Order, RecordedInput};
use rutent_scenarios::assembly::machine::{Environment, Machine};
use rutent_stm_vos_serve::{Answer, Request};

/// The environment variable naming the host server.
pub const SERVE: &str = "STM_VOS_SERVE";

fn unavailable(reason: impl Into<String>) -> HostError {
    HostError::Unavailable {
        reason: reason.into(),
    }
}

/// The two ends of a host server's pipe.
struct Pipe {
    to: ChildStdin,
    from: BufReader<ChildStdout>,
}

/// One host server, and its cluster. A read of the host takes `&self` and writes its call all the same, so the pipe
/// is behind a [`RefCell`], borrowed for one call at a time.
pub struct Remote {
    child: Child,
    pipe: RefCell<Pipe>,
}

impl Remote {
    fn start(dir: &PathBuf, replicas: usize, first: &Request) -> Result<Self, HostError> {
        let serve = std::env::var(SERVE)
            .map_err(|_| unavailable(format!("{SERVE} names the host server")))?;
        let mut child = Command::new(serve)
            .arg(dir)
            .arg(replicas.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| unavailable(format!("the host server: {e}")))?;
        let to = child
            .stdin
            .take()
            .ok_or_else(|| unavailable("the server's input"))?;
        let from = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| unavailable("the server's output"))?,
        );
        let remote = Remote {
            child,
            pipe: RefCell::new(Pipe { to, from }),
        };
        match remote.call(first)? {
            Answer::Done => Ok(remote),
            other => Err(failure(other)),
        }
    }

    fn call(&self, request: &Request) -> Result<Answer, HostError> {
        let line = serde_json::to_string(request).map_err(|e| unavailable(e.to_string()))?;
        let mut pipe = self.pipe.borrow_mut();
        writeln!(pipe.to, "{line}").map_err(|e| unavailable(e.to_string()))?;
        pipe.to.flush().map_err(|e| unavailable(e.to_string()))?;
        let mut answer = String::new();
        pipe.from
            .read_line(&mut answer)
            .map_err(|e| unavailable(e.to_string()))?;
        if answer.is_empty() {
            return Err(unavailable("the host server stopped"));
        }
        let answer: Answer =
            serde_json::from_str(&answer).map_err(|e| unavailable(e.to_string()))?;
        match answer {
            Answer::Failed { error } => Err(error.into()),
            other => Ok(other),
        }
    }
}

fn failure(answer: Answer) -> HostError {
    match answer {
        Answer::Failed { error } => error.into(),
        other => unavailable(format!("an answer of another call: {other:?}")),
    }
}

impl Drop for Remote {
    fn drop(&mut self) {
        let _ = self.call(&Request::Stop);
        let _ = self.child.wait();
    }
}

fn record(answer: Answer, delivered: bool) -> Result<TransitionRecord, HostError> {
    match answer {
        Answer::Recorded { record } if delivered => Ok(record),
        Answer::AlreadyRecorded { record } if !delivered => Ok(record),
        other => Err(failure(other)),
    }
}

impl Machine for Remote {
    fn submit(&mut self, input: RecordedInput) -> Result<Delivery, HostError> {
        match self.call(&Request::Submit {
            input: Box::new(input),
        })? {
            a @ Answer::Recorded { .. } => record(a, true).map(Delivery::Recorded),
            a @ Answer::AlreadyRecorded { .. } => record(a, false).map(Delivery::AlreadyRecorded),
            other => Err(failure(other)),
        }
    }

    fn orders(&self) -> Result<Vec<Id>, HostError> {
        self.ask(&Request::Orders, |a| match a {
            Answer::Orders { orders } => Some(orders),
            _ => None,
        })
    }

    fn state(&self, order: &Id) -> Result<Option<Order>, HostError> {
        self.ask(
            &Request::State {
                order: order.clone(),
            },
            |a| match a {
                Answer::State { state } => Some(state),
                _ => None,
            },
        )
    }

    fn journal(&self, order: &Id) -> Result<Vec<TransitionRecord>, HostError> {
        self.ask(
            &Request::Journal {
                order: order.clone(),
            },
            |a| match a {
                Answer::Journal { records } => Some(records),
                _ => None,
            },
        )
    }

    fn halt(&mut self) -> Result<(), HostError> {
        self.done(&Request::Halt)
    }

    fn resume(&mut self) -> Result<(), HostError> {
        self.done(&Request::Resume)
    }

    fn export(&self) -> Result<Vec<SnapshotEntry>, HostError> {
        self.ask(&Request::Export, |a| match a {
            Answer::Entries { entries } => Some(entries),
            _ => None,
        })
    }
}

impl Remote {
    /// A call that reads, answered as `pick` picks it.
    fn ask<T>(
        &self,
        request: &Request,
        pick: impl FnOnce(Answer) -> Option<T>,
    ) -> Result<T, HostError> {
        let answer = self.call(request)?;
        let shown = format!("{answer:?}");
        pick(answer).ok_or_else(|| unavailable(format!("an answer of another call: {shown}")))
    }

    fn done(&mut self, request: &Request) -> Result<(), HostError> {
        match self.call(request)? {
            Answer::Done => Ok(()),
            other => Err(failure(other)),
        }
    }
}

/// VOS on three replicas, as an environment: each host a fresh server and cluster under `base`.
pub struct Vos {
    base: PathBuf,
    prefix: String,
    next: usize,
}

impl Vos {
    /// Three replicas, their clusters under `base`, each named from `prefix`.
    pub fn three(base: &Base, prefix: &str) -> Self {
        Self {
            base: base.0.clone(),
            prefix: prefix.to_owned(),
            next: 0,
        }
    }

    fn dir(&mut self) -> PathBuf {
        self.next += 1;
        let dir = self.base.join(format!("{}{}", self.prefix, self.next));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }
}

impl Environment for Vos {
    fn name(&self) -> String {
        "VOS on three replicas".to_owned()
    }

    fn open(&mut self) -> Result<Box<dyn Machine>, HostError> {
        let dir = self.dir();
        Ok(Box::new(Remote::start(&dir, 3, &Request::Open)?))
    }

    fn import(&mut self, entries: Vec<SnapshotEntry>) -> Result<Box<dyn Machine>, HostError> {
        let dir = self.dir();
        Ok(Box::new(Remote::start(
            &dir,
            3,
            &Request::Import { entries },
        )?))
    }
}

/// A directory for a test's clusters, removed with it.
pub struct Base(pub PathBuf);

impl Base {
    /// A fresh directory for the test `name`.
    ///
    /// # Panics
    ///
    /// If it can't be made.
    pub fn new(name: &str) -> Base {
        let dir = std::env::temp_dir().join(format!("rsv-scenarios-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a directory for the clusters");
        Base(dir)
    }
}

impl Drop for Base {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
