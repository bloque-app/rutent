//! The test harness the host-contract suite drives (`T-005-03`). Part of the contract crate, but not of what a
//! production host implements: [`super::Host`] alone is the frozen production contract.
//!
//! Every candidate is driven through [`Harness`]: a fresh store, start, kill, reopen, and import, whole or stopped
//! part-way. For an in-process candidate,
//! killing drops the host's memory and keeps its store; for a runtime in another process, such as VOS, killing
//! stops that process and reopening starts it again on the same data. An in-process candidate can also implement
//! [`FaultSeam`], which stops a submit at a named point. For an out-of-process runtime the named points
//! are best-effort, reached by timing a kill rather than by a hook.

use std::fmt;

use super::{Host, HostError, SnapshotStream};
use crate::state_machine::StateMachine;

/// A point in a submit after which the suite can stop the host abruptly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CrashPoint {
    /// After the input is received, before the transition is computed.
    AfterReceived,
    /// After the transition is computed, before its record is durable.
    AfterComputed,
    /// After the record is durable, before it is acknowledged.
    AfterDurable,
}

/// A harness failure, apart from the host's own errors: harness-only, never returned by a production host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HarnessError {
    /// The host refused or failed.
    Host(HostError),
    /// The harness stopped the host on purpose, at a point it was asked to.
    Crashed,
    /// The harness couldn't create, start, stop or reach the runtime.
    Runtime {
        /// What failed.
        reason: String,
    },
}

impl fmt::Display for HarnessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HarnessError::Host(e) => write!(f, "host error: {e:?}"),
            HarnessError::Crashed => f.write_str("stopped by the harness"),
            HarnessError::Runtime { reason } => write!(f, "runtime: {reason}"),
        }
    }
}

impl std::error::Error for HarnessError {}

impl From<HostError> for HarnessError {
    fn from(error: HostError) -> Self {
        HarnessError::Host(error)
    }
}

/// How the suite gets, starts, kills and reopens a candidate, the same way in-process and out of process.
pub trait Harness<M: StateMachine> {
    /// The candidate host.
    type Host: Host<M>;

    /// A new, empty store.
    fn fresh_store(&mut self) -> Result<<Self::Host as Host<M>>::Store, HarnessError>;

    /// A host started on a store: [`Host::open`], or a process started on its data.
    fn start(&mut self, store: <Self::Host as Host<M>>::Store) -> Result<Self::Host, HarnessError>;

    /// Stops the host abruptly, as a killed process: no cleanup. Returns its store as the stop leaves it.
    fn kill(&mut self, host: Self::Host) -> Result<<Self::Host as Host<M>>::Store, HarnessError>;

    /// A host started again on a store a kill left, recovering what is durable in it.
    fn reopen(&mut self, store: <Self::Host as Host<M>>::Store)
    -> Result<Self::Host, HarnessError>;

    /// A host imported from the stream onto a fresh store: [`Host::import`], or a process started to import it.
    fn import(
        &mut self,
        store: <Self::Host as Host<M>>::Store,
        stream: SnapshotStream<'_>,
    ) -> Result<Self::Host, HarnessError>;

    /// Imports the stream onto the store but stops abruptly after this many entries, and returns the store as the
    /// stop leaves it; [`Harness::reopen`] on it must fail with [`HostError::ImportIncomplete`]. Out of process,
    /// the stop is a kill timed to the count, best-effort.
    fn import_crashing_after(
        &mut self,
        store: <Self::Host as Host<M>>::Store,
        stream: SnapshotStream<'_>,
        entries: u64,
    ) -> Result<<Self::Host as Host<M>>::Store, HarnessError>;
}

/// A harness that can stop an in-process host at a named point.
pub trait FaultSeam<M: StateMachine>: Harness<M> {
    /// The host's next submit stops abruptly after the point: no work after it and no cleanup. The submit returns
    /// [`HostError::Unavailable`], an unknown outcome, and the host is fit only for [`Harness::kill`].
    fn crash_at(&mut self, host: &mut Self::Host, point: CrashPoint);
}
