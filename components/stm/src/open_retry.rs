//! A bounded retry of an open refused while the platform spawns children (`REQ-OPS-3`, `F-007`'s notes).
//!
//! A process that spawns a child hands it copies of every descriptor the parent has open, until the child
//! executes. An `flock` belongs to the open file description, so while a copy exists a host that was just dropped
//! still holds its store's lock, and a reopen is refused with "the store is open in another host". The refusal is
//! the store failing closed, so no record is at risk, and it passes when the child executes: a few milliseconds.
//! Anything that opens a store while it spawns, as module workers do, retries it through [`retry_open`].
//!
//! The retry is bounded and matches one refusal only. A store held open by a second live host is still refused
//! once the attempts run out, and every other failure is returned at once.

use std::time::Duration;

use rutent_contracts::state_machine::host::HostError;

/// The reason a store gives when another host holds its lock. The purpose-built store refuses with it, and
/// [`is_open_elsewhere`] reads it.
pub const STORE_OPEN_ELSEWHERE: &str = "the store is open in another host";

/// How long an open is retried: `attempts` tries in all, `pause` between two of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenRetry {
    /// The most tries, the first included. At least one is made.
    pub attempts: u32,
    /// The pause between a refused try and the next.
    pub pause: Duration,
}

impl OpenRetry {
    /// The window of a spawn: 40 tries 10 ms apart, so a store whose holder is a child about to execute is
    /// waited for, and a store held by a live host is refused after about 400 ms.
    pub const SPAWN_WINDOW: OpenRetry = OpenRetry {
        attempts: 40,
        pause: Duration::from_millis(10),
    };
}

impl Default for OpenRetry {
    fn default() -> Self {
        OpenRetry::SPAWN_WINDOW
    }
}

/// Whether `error` is the refusal of a store that another host holds open: the one an open retries.
pub fn is_open_elsewhere(error: &HostError) -> bool {
    matches!(error, HostError::Unavailable { reason } if reason == STORE_OPEN_ELSEWHERE)
}

/// Runs `open`, and runs it again after `policy.pause` while it fails with an error `retryable` names, up to
/// `policy.attempts` tries. Another failure, and the last of the retryable ones, are returned as they are.
pub fn retry_open<T>(
    policy: OpenRetry,
    retryable: impl Fn(&HostError) -> bool,
    mut open: impl FnMut() -> Result<T, HostError>,
) -> Result<T, HostError> {
    let mut tries = 1;
    loop {
        match open() {
            Err(e) if retryable(&e) && tries < policy.attempts => {
                tries += 1;
                std::thread::sleep(policy.pause);
            }
            other => return other,
        }
    }
}
