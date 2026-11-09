//! The retry of an open refused while the platform spawns children (`REQ-OPS-3`, `F-007`'s notes).
//!
//! A node that starts on a data directory whose lock a just-killed node, or a child still to execute, holds exits
//! at once, and the open fails with "exited before it started". The refusal is the store failing closed and passes in
//! milliseconds, so the open of a cluster that exists is retried through `rutent-stm`'s bounded
//! [`retry_open`](rutent_stm::open_retry::retry_open). A cluster that does not exist yet is opened once: a node
//! that fails to start the first time has a different fault.

use rutent_contracts::state_machine::host::HostError;
use rutent_stm::open_retry::{OpenRetry, retry_open};

/// Whether `error` is a node that exited before it started, the refusal an open of an existing cluster retries. A
/// node that was only slow to start (the wait ran out) is not.
pub fn refused_while_spawning(error: &HostError) -> bool {
    matches!(error, HostError::Unavailable { reason } if reason.contains("exited before it started"))
}

/// Runs `open`, retried for the spawn window when the cluster `reopening` already exists, and once when it is new.
pub fn open_retrying<T>(
    reopening: bool,
    open: impl FnMut() -> Result<T, HostError>,
) -> Result<T, HostError> {
    let policy = if reopening {
        OpenRetry::SPAWN_WINDOW
    } else {
        OpenRetry {
            attempts: 1,
            ..OpenRetry::SPAWN_WINDOW
        }
    };
    retry_open(policy, refused_while_spawning, open)
}
