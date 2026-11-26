//! A regulator's read of the audit trail (`REQ-AUD-12`, `REQ-AUZ-7`, `T-022-06`).
//!
//! A regulator reads the trail only by an identified permission, which policy holds (`T-021-02`): it grants the
//! permission, answers each read allowed or refused, and records the read either way, naming who read, what and
//! when. [`read_trail`] is the trail's side of that. It asks policy first, before anything of the trail is looked
//! at, so a refused reader learns nothing of it; it then holds the answer to the log, never to the word of whoever
//! gave it. The trail is released only where the log holds, at the answer's receipt, the entry that records this
//! read with this outcome (`INV-7`'s order: the record before the effect). A read policy allowed but the log doesn't
//! show recorded is not served, and an answer with no record behind it is no answer.
//!
//! What it can reach is what its type parameter gives it: [`ReadChecks`], which asks and nothing else, so it can
//! neither grant nor revoke a permission, though the engine behind it can. It writes no entry: the record is
//! policy's, one entry for the read, `check_read`, holding the check (the reader in its role, what it reads, the
//! permission, the time) and its outcome, and nothing that `REQ-AUD-6` keeps out of the log.

use rutent_contracts::core_internal::calls::{AppendFailure, Reads as Policy};
use rutent_contracts::core_internal::{
    Entry, ReadAnswer, ReadCheck, ReadOutcome, Reads, Receipt, What,
};

/// What `read_trail` may ask of policy: a read check, and nothing else. A blanket impl over policy's own `Reads`
/// narrows it, so this module can't reach its grant or its revocation.
pub trait ReadChecks {
    /// Asks policy to decide and record a read.
    fn check_read(&mut self, check: &ReadCheck) -> Result<ReadAnswer, AppendFailure>;
}

impl<T: Policy> ReadChecks for T {
    fn check_read(&mut self, check: &ReadCheck) -> Result<ReadAnswer, AppendFailure> {
        Policy::check_read(self, check)
    }
}

/// The refusal to serve a read that isn't of the trail: the trail's reader serves nothing else.
pub const NOT_THE_TRAIL: &str =
    "this reads the trail, and a read of anything else is policy's alone";

/// Why a trail read was not served, though policy answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The check reads something other than the trail; policy was not asked.
    NotTheTrail,
    /// Policy answered another request than the one asked.
    AnswerOfAnother,
    /// The log holds no entry at the answer's receipt that records this read with this outcome: nothing is served.
    Unrecorded,
    /// Policy or the log couldn't answer.
    Policy(AppendFailure),
}

/// The trail as a regulator reads it: every entry the log held when the read was recorded, up to and including the
/// record of the read itself, each with its receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trail {
    /// The receipt of the entry that records this read.
    pub read: Receipt,
    /// The entries, in order.
    pub entries: Vec<(Receipt, Entry)>,
}

/// What a trail read came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Read {
    /// Policy allowed it, and the log shows it recorded: the trail.
    Released(Trail),
    /// Policy refused it, and the log shows the refusal recorded: policy's answer, and nothing of the trail.
    Refused(ReadAnswer),
}

/// Reads the audit trail for `check.reader`, as `policy` allows, and answers the trail only once the log shows the
/// read recorded. `history` hands over the log's entries with their receipts, and is called only after policy has
/// answered, so the record of the read is among them.
pub fn read_trail<P: ReadChecks>(
    policy: &mut P,
    check: &ReadCheck,
    history: impl FnOnce() -> Vec<(Receipt, Entry)>,
) -> Result<Read, Failure> {
    if check.reads != (Reads::Trail {}) {
        return Err(Failure::NotTheTrail);
    }
    let answer = policy.check_read(check).map_err(Failure::Policy)?;
    if answer.request != check.request {
        return Err(Failure::AnswerOfAnother);
    }
    let mut entries = history();
    let at = entries
        .iter()
        .position(|(receipt, _)| receipt == &answer.receipt)
        .ok_or(Failure::Unrecorded)?;
    let records_it = match &entries[at].1 {
        Entry::Verified(v) => {
            matches!(&v.what, What::CheckRead { check: recorded } if recorded == check)
                && v.outcome == answer.outcome.outcome()
        }
        Entry::Stated(_) => false,
    };
    if !records_it {
        return Err(Failure::Unrecorded);
    }
    match answer.outcome {
        ReadOutcome::Refused { .. } => Ok(Read::Refused(answer)),
        ReadOutcome::Allowed {} => {
            entries.truncate(at + 1);
            Ok(Read::Released(Trail {
                read: answer.receipt,
                entries,
            }))
        }
    }
}
