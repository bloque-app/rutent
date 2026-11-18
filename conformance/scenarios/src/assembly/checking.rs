//! The outside checker's side of the assembly (`T-024-07`, `AC-22`, `AC-37`): a checkpointer over Core's log, signing
//! with the log's own key (never the operator's) on a schedule fixed in advance, and the history, the checkpoints and
//! their openings as the checker takes them, as bytes.
//!
//! The checkpointer only reads the log (it takes its size and its root), so it is no second handle that writes, and
//! the crate's hold on the log stays as `T-024-02` left it. It keeps its checkpoints in a store of its own, apart from
//! the log's and the evidence store's (ADR-0005). The deployment's schedule is `F-030`'s to set; the assembly's is the
//! scenario's start and an hour between ticks.

use rutent_conformance::signing::{public_key, signing_key};
use rutent_contracts::core_internal::calls::AppendFailure;
use rutent_contracts::provider_role::PublicKey;
use rutent_contracts::router_api::Id;
use rutent_core_audit::checkpoint::{Checker, Checkpointer, Kept, Schedule};
use rutent_core_audit::log::{MemoryStore, record};
use rutent_core_audit::receipts::store::LogsRead;

use super::durable::LogDisk;
use super::{Core, CoreLog, LOG_NAME};
use crate::kit;

/// Seconds between two ticks of the assembly's checkpoint schedule.
pub const CHECKPOINT_EVERY: u64 = 3_600;

/// A checkpointer over Core's log, on the assembly's store of its own.
pub type CoreCheckpointer = Checkpointer<LogDisk, LogsRead, MemoryStore>;

/// The log's own signing key: one the deployment's operator doesn't hold, which signs checkpoints as it signs
/// parties' receipts (`T-022-04`).
pub fn log_key() -> ed25519_dalek::SigningKey {
    signing_key("the log of rutent-scenarios", 0)
}

/// The public half of [`log_key`], which the checker is given.
pub fn log_public_key() -> PublicKey {
    public_key(&log_key())
}

/// The assembly's schedule: tick 0 at the scenario's start ([`kit::START`]), then every [`CHECKPOINT_EVERY`] seconds.
///
/// # Panics
///
/// If the start isn't a time a schedule can begin at.
pub fn schedule() -> Schedule {
    let first = rutent_prov::catalogue::Instant(kit::START)
        .rfc3339()
        .and_then(|t| rutent_contracts::router_api::Timestamp::new(&t))
        .expect("the scenario's start is a time");
    Schedule::new(&first, CHECKPOINT_EVERY).expect("a schedule")
}

/// The outside checker of Core's log: it holds the log's name, the log's public key and the schedule, and nothing
/// else of the deployment: no log handle, no evidence store, no secret.
pub fn checker() -> Checker {
    Checker::new(
        log_public_key(),
        Id::new(LOG_NAME).expect("a name"),
        schedule(),
    )
}

/// The history as the checker is handed it: each entry's record, in order.
pub fn history_of(log: &CoreLog) -> Vec<Vec<u8>> {
    log.entries()
        .iter()
        .filter_map(|(_, entry)| record(entry).ok())
        .collect()
}

/// Every opening `kept` holds, as bytes, in tick order.
pub fn openings_of(kept: &[Kept]) -> Vec<Vec<u8>> {
    kept.iter()
        .map(|k| serde_json::to_vec(&k.opening).expect("an opening is JSON"))
        .collect()
}

impl Core {
    /// A checkpointer over Core's log, signing with [`log_key`] on [`schedule`], keeping its checkpoints in a store
    /// of its own. It reads the log and writes nothing to it.
    ///
    /// # Errors
    ///
    /// If the checkpointer refuses the log or its store.
    pub fn checkpointer(&self) -> Result<CoreCheckpointer, AppendFailure> {
        Checkpointer::new(
            std::rc::Rc::clone(&self.log),
            MemoryStore::new(),
            log_key(),
            schedule(),
        )
    }
}
