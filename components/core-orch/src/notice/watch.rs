//! The watch on the path that warns the operator (`T-023-02`, `REQ-ORD-7`, `THR-59`).
//!
//! The path that carries the operator's warnings (a stuck order, a raise, a charge counted far ahead) can be cut
//! without a word: a transport that takes a notice and delivers nothing tells the sender nothing. So the path has a
//! heartbeat of its own. [`Watch::tick`] sends the operator a sealed heartbeat on that path every [`INTERVAL`]
//! seconds; the operator's side answers each one it receives ([`Watch::answered`]; the reference receiver's
//! [`super::Inbox`] says which beat it took). A path that answers nothing for [`SILENCE`] seconds is cut, and the cut
//! is noticed on a second path, a transport of its own, with a notice sealed for the operator alone, until that
//! path takes it. A cut is told once; an answer ends it, and a later cut is told again.
//!
//! **Runs and order.** Each watch has a `run`, an id the assembly makes fresh at every start, carried in each
//! heartbeat: an answer counts only for the newest beat of this run, so a beat replayed from before a restart, or a
//! late answer to an older beat, can't make a cut path look alive. A silence no longer than the interval is refused
//! ([`Watch::with`]).
//!
//! **The clock.** Silence is measured on Core's [`Clock`], which reads the host's wall time; Core has no monotonic
//! source (`F-024`'s notes). A clock stepped back hides a cut for the length of the step, and one stepped forward
//! raises a false cut. The assembly should hand the watch a monotonic [`Clock`] where it has one.
//!
//! A refused send is not read as a cut by itself, and an accepted one is not read as alive: only an answer counts.
//! A path that never answers is cut once [`SILENCE`] has passed since the watch began.

use rutent_contracts::core_internal::Role;
use rutent_contracts::router_api::Id;

use super::wire::{self, About, Notice, Receiver};
use super::{NoticeError, NoticeKey, Transport};
use crate::snapshot::{Clock, Reading};

/// Seconds between two heartbeats. Reversible default.
pub const INTERVAL: u64 = 30;
/// Seconds without an answer after which the path is cut: four beats. Reversible default.
pub const SILENCE: u64 = 120;

/// What a tick found of the watched path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Path {
    /// It answered within [`SILENCE`].
    Alive,
    /// It has answered nothing for [`SILENCE`] seconds or more, since this second.
    Cut {
        /// The second of the last answer, or of the watch's start.
        since: u64,
    },
}

/// What one tick did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tick {
    /// The heartbeat sent now, if one was due.
    pub beat: Option<u64>,
    /// What the watch found of the path.
    pub path: Path,
    /// Whether the second path took the notice of the cut now.
    pub told: bool,
}

/// The watch on the operator's warning path.
#[derive(Debug, Clone)]
pub struct Watch {
    deployment: Id,
    run: Id,
    operator: Receiver,
    interval: u64,
    silence: u64,
    beat: u64,
    next: u64,
    answered: u64,
    heard: u64,
    told: bool,
}

impl Watch {
    /// A watch on the path to `operator`, begun now, with the default [`INTERVAL`] and [`SILENCE`].
    pub fn new(deployment: Id, operator: Id, run: Id, clock: &dyn Clock) -> Self {
        Self::build(deployment, operator, run, clock, INTERVAL, SILENCE)
    }

    /// A watch of run `run` beating every `interval` seconds, and finding the path cut after `silence` seconds
    /// unanswered; `None` where `silence` is no longer than `interval`, which would call a healthy path cut
    /// between two beats.
    pub fn with(
        deployment: Id,
        operator: Id,
        run: Id,
        clock: &dyn Clock,
        interval: u64,
        silence: u64,
    ) -> Option<Self> {
        (silence > interval)
            .then(|| Self::build(deployment, operator, run, clock, interval, silence))
    }

    fn build(
        deployment: Id,
        operator: Id,
        run: Id,
        clock: &dyn Clock,
        interval: u64,
        silence: u64,
    ) -> Self {
        let now = clock.now().seconds;
        Self {
            deployment,
            run,
            operator: Receiver {
                party: operator,
                role: Role::Operator,
            },
            interval,
            silence,
            beat: 0,
            next: now,
            answered: 0,
            heard: now,
            told: false,
        }
    }

    /// The operator's side took heartbeat `beat`: the path is alive as of now. Only an answer to the newest beat
    /// sent, by this run, counts: a late answer to an older one can't hide a cut, and a beat replayed from another
    /// run is none.
    pub fn answered(&mut self, run: &Id, beat: u64, clock: &dyn Clock) {
        if run == &self.run && beat == self.beat && beat > self.answered {
            self.answered = beat;
            self.heard = clock.now().seconds;
            self.told = false;
        }
    }

    /// Sends the heartbeat if one is due, on `warning`, the path watched; finds whether the path is cut, and tells
    /// the operator so on `second`, once, until that path takes it.
    pub fn tick(
        &mut self,
        clock: &dyn Clock,
        key: &mut dyn NoticeKey,
        warning: &mut dyn Transport,
        second: &mut dyn Transport,
    ) -> Result<Tick, NoticeError> {
        let reading = clock.now();
        let now = reading.seconds;
        let at = reading.timestamp().ok_or(NoticeError::NoTime)?;
        let mut sent = None;
        if now >= self.next {
            let beat = self.beat + 1;
            let notice = self.sealed(
                key,
                About::Heartbeat {
                    run: self.run.clone(),
                    beat,
                    at: at.clone(),
                },
            )?;
            self.beat = beat;
            self.next = now.saturating_add(self.interval);
            // Whether the path took it says nothing of whether it arrived: only an answer counts.
            warning.send(&self.operator, &notice);
            sent = Some(beat);
        }
        let silent_for = now.saturating_sub(self.heard);
        if silent_for < self.silence {
            return Ok(Tick {
                beat: sent,
                path: Path::Alive,
                told: false,
            });
        }
        let mut told = false;
        if !self.told {
            let since = Reading::at(self.heard)
                .timestamp()
                .ok_or(NoticeError::NoTime)?;
            let notice = self.sealed(
                key,
                About::PathCut {
                    since,
                    silent_for,
                    last_beat: self.beat,
                },
            )?;
            told = second.send(&self.operator, &notice);
            self.told = told;
        }
        Ok(Tick {
            beat: sent,
            path: Path::Cut { since: self.heard },
            told,
        })
    }

    fn sealed(&self, key: &mut dyn NoticeKey, about: About) -> Result<Vec<u8>, NoticeError> {
        wire::seal(
            key,
            Notice {
                deployment: self.deployment.clone(),
                to: self.operator.clone(),
                about,
                current: false,
            },
        )
        .ok_or(NoticeError::Unsigned)
    }
}
