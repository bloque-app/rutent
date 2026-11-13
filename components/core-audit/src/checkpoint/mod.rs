//! Checkpoints and the outside checker (ADR-0005, `REQ-AUD-14`, `REQ-AUD-9`, `REQ-AUD-3`, `REQ-AUD-10`, `T-022-04`).
//!
//! Declared by `T-020-01`; `T-022-04` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! - [`Checkpointer`] publishes a [`Checkpoint`] for every tick of a [`Schedule`] fixed in advance, whether or not
//!   the log moved: a short, fixed-size, signed commitment to the log's root, salted afresh each time, with no raw
//!   size and no root in it. What it commits to, and the salt, are its [`Opening`], which stays in core-audit's own
//!   checkpoint store (outside the log, ADR-0005) and goes to the checker with the history.
//! - [`Checker`] is the check, run by a party outside the operator, or by the operator itself as its own review
//!   (`REQ-AUD-3`): from the history's records, the checkpoints it holds as they were published, and their openings,
//!   and nothing else. It needs no [`AuditLog`], no evidence store and no key but the log's
//!   public one. The `audit-check` binary runs it from files.
//!
//! **Why the salt and the size stay out of what is published.** A checkpoint is public, so anyone may hold a
//! checkpoint and some root of the log's: the empty log's is SHA-256 of nothing, and a party's receipt names the root
//! and the size it was issued at. Were the salt published, such a holder could test a checkpoint against that root and
//! learn the log hadn't moved, which is activity (`REQ-AUD-14`, `THR-130`). So the commitment is SHA-256 over the
//! tick, a fresh salt, the size and the root, and only the holder of the opening, which is the checker's, can open it.

mod checker;

use std::cell::RefCell;
use std::rc::Rc;

use ed25519_dalek::{Signer as _, SigningKey};
use rutent_contracts::core_internal::Violation;
use rutent_contracts::core_internal::calls::{AppendFailure, KeptStatements};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{PublicKey, SafeInteger, signing_input};
use rutent_contracts::router_api::{Id, Timestamp};
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

pub use checker::{Checker, Finding, Report, ReviewError, read_lines, split_kept};

use crate::log::kept::{base64url, from_base64url};
use crate::log::merkle::Hash;
use crate::log::{AuditLog, Store, StoreError, digest};
use crate::receipts::ReceiptSignature;
use crate::receipts::receipt::timestamp;

/// A checkpoint's `shape`.
pub const CHECKPOINT_SHAPE: &str = "urn:rutent:core-audit:checkpoint";

/// An opening's `shape`.
pub const OPENING_SHAPE: &str = "urn:rutent:core-audit:checkpoint-opening";

/// The domain a commitment is hashed under, so no other SHA-256 of the same parts reads as one.
const DOMAIN: &[u8] = b"rutent:core-audit:checkpoint:v1";

/// The refusal of a checkpointer whose key is the deployment's `operator_key`.
pub const KEY_OF_THE_OPERATOR: &str =
    "a checkpoint is signed with the log's own key, never the operator's statement key";

/// The refusal to open on a checkpoint store holding a record that isn't a checkpoint the log's key signed with its
/// opening, in its place on the schedule.
pub const NOT_ITS_CHECKPOINTS: &str = "every record of the checkpoint store is a checkpoint the log's key signed, with its opening, one tick after the last";

/// The refusal to open on a log that doesn't hold against the checkpoints already issued for it: a new checkpoint
/// over a history that fails an old one would vouch for a rewrite.
pub const HISTORY_FAILS_ITS_CHECKPOINTS: &str =
    "the log holds against every checkpoint already issued for it";

/// The refusal of a schedule with no period.
pub const NO_PERIOD: &str = "a schedule's ticks are a whole number of seconds apart, above none";

/// The ticks, fixed in advance and independent of activity: tick `k` falls at `first` plus `k` periods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    first_nanos: i128,
    every_seconds: u64,
}

impl Schedule {
    /// The schedule whose tick 0 is at `first` and whose ticks are `every_seconds` apart.
    pub fn new(first: &Timestamp, every_seconds: u64) -> Result<Self, Violation> {
        if every_seconds == 0 {
            return Err(Violation(NO_PERIOD));
        }
        Ok(Schedule {
            first_nanos: first.unix_nanos(),
            every_seconds,
        })
    }

    fn period(&self) -> i128 {
        i128::from(self.every_seconds) * 1_000_000_000
    }

    /// When tick `tick` falls, to the second.
    pub fn at(&self, tick: u64) -> Option<Timestamp> {
        timestamp(self.first_nanos + i128::from(tick) * self.period())
    }

    /// The latest tick that has fallen by `nanos` (nanoseconds since the epoch), or `None` before the first.
    pub fn tick_at(&self, nanos: i128) -> Option<u64> {
        let since = nanos.checked_sub(self.first_nanos).filter(|s| *s >= 0)?;
        u64::try_from(since / self.period()).ok()
    }

    /// The latest tick whose period has ended by `nanos`: the last a checker holds a checkpoint missing against,
    /// since the one now falling may be minutes from its checkpoint. `None` while none has ended.
    pub fn closed_through(&self, nanos: i128) -> Option<u64> {
        self.tick_at(nanos)?.checked_sub(1)
    }
}

/// What the operator publishes for one tick: fixed in size, and nothing but a commitment. It holds no size, no
/// root, no count and no salt (`REQ-AUD-14`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    /// Always [`CHECKPOINT_SHAPE`].
    pub shape: String,
    /// The log.
    pub log: Id,
    /// The tick, as 16 lower-case hexadecimal digits, so a checkpoint's size never tells how far along the schedule
    /// it is.
    pub tick: String,
    /// When the tick fell on the schedule, to the second: the schedule's time, never the time of writing.
    pub at: Timestamp,
    /// SHA-256 of the domain, the log, the tick, the salt, the size and the root.
    pub commitment: Digest,
    /// The log's signature.
    pub signature: ReceiptSignature,
}

/// What a checkpoint commits to, and the salt: kept in the checkpoint store, and handed to the checker with the
/// history. It is not published.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Opening {
    /// Always [`OPENING_SHAPE`].
    pub shape: String,
    /// The log.
    pub log: Id,
    /// The tick, as the checkpoint has it.
    pub tick: String,
    /// The salt, 32 fresh bytes, base64url without padding.
    pub salt: String,
    /// The number of entries the log held, whose root the checkpoint commits to.
    pub size: SafeInteger,
}

/// A record of the checkpoint store: a checkpoint as published, and its opening.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Kept {
    /// The checkpoint, as it was published.
    pub checkpoint: Checkpoint,
    /// Its opening.
    pub opening: Opening,
}

/// A record of the checkpoint store that says the caller has published every checkpoint through a tick
/// ([`Checkpointer::acknowledge`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    /// The last tick published, as a checkpoint has it.
    pub published_through: String,
}

/// The refusal of an acknowledgement of a tick the store holds no checkpoint for.
pub const NO_SUCH_CHECKPOINT: &str =
    "only a checkpoint the store holds is acknowledged as published";

/// A tick as 16 lower-case hexadecimal digits.
pub fn tick_text(tick: u64) -> String {
    format!("{tick:016x}")
}

/// The tick a text of 16 lower-case hexadecimal digits names.
pub fn tick_of(text: &str) -> Option<u64> {
    (text.len() == 16 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
        .then(|| u64::from_str_radix(text, 16).ok())
        .flatten()
}

/// The commitment to the root of the first `size` entries of `log`, at `tick`, under `salt`.
pub fn commitment(log: &Id, tick: u64, salt: &[u8], size: u64, root: &Hash) -> Digest {
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN);
    hasher.update((log.as_str().len() as u32).to_be_bytes());
    hasher.update(log.as_str().as_bytes());
    hasher.update(tick.to_be_bytes());
    hasher.update(salt);
    hasher.update(size.to_be_bytes());
    hasher.update(root);
    digest(&hasher.finalize().into())
}

/// The most ticks one run issues, and the most checkpoints it hands out; the next run continues.
pub const MOST_PER_RUN: u64 = 64;

/// What a run of [`Checkpointer::run_due`] did: the checkpoints it published, in tick order, and why it stopped
/// short of the schedule, if it did. A tick it didn't reach is tried again on the next run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    /// Each checkpoint's published bytes.
    pub published: Vec<Vec<u8>>,
    /// Why it stopped before the last due tick.
    pub stopped: Option<AppendFailure>,
    /// The tick of the last checkpoint in `published`, which [`Checkpointer::acknowledge`] takes.
    pub through: Option<u64>,
}

/// The issuer of checkpoints: the log's key, bound to the log it commits to and to its own store, apart from the
/// log's and the evidence store's (ADR-0005). Only the assembly makes one (`T-024-02`).
///
/// **One checkpoint per due tick, whatever the log did.** [`Checkpointer::run_due`] takes the time from the log's
/// clock only to find which ticks are due; a checkpoint's own time is the schedule's. It never reads whether the
/// log moved to decide whether to issue, so an idle period ticks exactly as a busy one does (`G-11`). A tick that
/// fell while nothing ran (a crash, a salt that couldn't be drawn, a store that failed) is issued on the next run,
/// in order, over the log as it then is: no tick is skipped, and a checkpoint says nothing of when it was written.
pub struct Checkpointer<S: Store, K: KeptStatements, C: Store> {
    key: SigningKey,
    public: PublicKey,
    log: Rc<RefCell<AuditLog<S, K>>>,
    store: C,
    schedule: Schedule,
    kept: Vec<Kept>,
    /// The last tick the caller acknowledged as published, durably.
    acked: Option<u64>,
    /// A write failed, so the store may hold a record `kept` doesn't: read it back before the next issue.
    stale: bool,
}

impl<S: Store, K: KeptStatements, C: Store> Checkpointer<S, K, C> {
    /// The checkpointer signing with `key` for `log`, keeping its checkpoints in `store`, on `schedule`. It refuses
    /// the deployment's `operator_key`, a store holding a record that isn't a checkpoint of this log in its place
    /// (a removed one, a changed one, one out of order), and a log that doesn't hold against the checkpoints the
    /// store already has.
    pub fn new(
        log: Rc<RefCell<AuditLog<S, K>>>,
        store: C,
        key: SigningKey,
        schedule: Schedule,
    ) -> Result<Self, AppendFailure> {
        let public = PublicKey::new(base64url(key.verifying_key().as_bytes()))
            .expect("an Ed25519 key's 32 bytes in base64url are a public key");
        if public == log.borrow().settings().deployment.operator_key {
            return Err(AppendFailure::Refused(Violation(KEY_OF_THE_OPERATOR)));
        }
        let mut checkpointer = Checkpointer {
            key,
            public,
            log,
            store,
            schedule,
            kept: Vec::new(),
            acked: None,
            stale: true,
        };
        (checkpointer.kept, checkpointer.acked) = checkpointer.read_back()?;
        checkpointer.stale = false;
        let held: Vec<Vec<u8>> = checkpointer
            .kept
            .iter()
            .map(|k| serde_json::to_vec(&k.checkpoint).expect("a checkpoint is JSON"))
            .collect();
        let openings: Vec<Vec<u8>> = checkpointer
            .kept
            .iter()
            .map(|k| serde_json::to_vec(&k.opening).expect("an opening is JSON"))
            .collect();
        let through = checkpointer
            .kept
            .last()
            .and_then(|k| tick_of(&k.opening.tick));
        let records: Vec<Vec<u8>> = checkpointer
            .log
            .borrow()
            .entries()
            .iter()
            .filter_map(|(_, e)| crate::log::record(e).ok())
            .collect();
        let settings = checkpointer.log.borrow().settings().clone();
        let report = Checker::new(checkpointer.public.clone(), settings.log, schedule)
            .check(&records, &held, &openings, through);
        if !report.is_clean() {
            return Err(AppendFailure::Refused(Violation(
                HISTORY_FAILS_ITS_CHECKPOINTS,
            )));
        }
        Ok(checkpointer)
    }

    /// The log's key, which every checkpoint it issues is checked against.
    pub fn key(&self) -> &PublicKey {
        &self.public
    }

    /// Every checkpoint issued, with its opening, in tick order.
    pub fn kept(&self) -> &[Kept] {
        &self.kept
    }

    /// The checkpoint store's records, read back and held to what a checkpoint store holds.
    fn read_back(&mut self) -> Result<(Vec<Kept>, Option<u64>), AppendFailure> {
        let refused = || AppendFailure::Refused(Violation(NOT_ITS_CHECKPOINTS));
        let records = self.store.records().map_err(|e| match e {
            StoreError::Unavailable(_) => AppendFailure::Unavailable,
            StoreError::Corrupt(_) => refused(),
        })?;
        let log = self.log.borrow().settings().log.clone();
        let checker = Checker::new(self.public.clone(), log.clone(), self.schedule);
        let mut kept: Vec<Kept> = Vec::new();
        let mut acked: Option<u64> = None;
        for bytes in records {
            if let Ok(cursor) = serde_json::from_slice::<Cursor>(&bytes) {
                // An acknowledgement names a checkpoint already in the store, and never goes back.
                let through = tick_of(&cursor.published_through).ok_or_else(refused)?;
                if through >= kept.len() as u64 || acked.is_some_and(|a| through < a) {
                    return Err(refused());
                }
                acked = Some(through);
                continue;
            }
            let record: Kept = serde_json::from_slice(&bytes).map_err(|_| refused())?;
            let published = serde_json::to_vec(&record.checkpoint).map_err(|_| refused())?;
            let tick = checker.read_checkpoint(&published).ok_or_else(refused)?.0;
            let next = kept
                .last()
                .map_or(0, |k| tick_of(&k.opening.tick).unwrap_or(0) + 1);
            if record.opening.tick != record.checkpoint.tick
                || record.opening.log != log
                || record.opening.shape != OPENING_SHAPE
                || tick != next
            {
                return Err(refused());
            }
            kept.push(record);
        }
        Ok((kept, acked))
    }

    /// Publishes a checkpoint for every tick due by the log's clock that hasn't one, in order, and hands out every
    /// checkpoint stored that the caller hasn't [acknowledged](Checkpointer::acknowledge) as published: those of
    /// this run, and any an earlier run or a process before it stored and never handed over (a write that reached
    /// the store and was reported failed, a restart between the store and the answer). So every checkpoint stored
    /// is published at least once, and a checker is never missing a tick the store holds.
    pub fn run_due(&mut self) -> Ran {
        let mut ran = Ran {
            published: Vec::new(),
            stopped: None,
            through: None,
        };
        self.issue_due(&mut ran);
        let from = self.acked.map_or(0, |a| a as usize + 1);
        for kept in self.kept.iter().skip(from).take(MOST_PER_RUN as usize) {
            ran.published
                .push(serde_json::to_vec(&kept.checkpoint).expect("a checkpoint is JSON"));
            ran.through = tick_of(&kept.opening.tick);
        }
        ran
    }

    /// Stores a checkpoint for every tick due that has none, stopping at the first that fails.
    fn issue_due(&mut self, ran: &mut Ran) {
        let now = self.log.borrow().settings().clock.unix_nanos();
        let Some(due) = self.schedule.tick_at(now) else {
            return;
        };
        if self.stale {
            // A write failed: the store may hold a checkpoint that `kept` doesn't, and issuing its tick again
            // would be two words for one tick. What was answered before must still be there.
            match self.read_back() {
                Ok((read, acked)) if read.starts_with(&self.kept) => {
                    self.kept = read;
                    self.acked = acked.max(self.acked);
                    self.stale = false;
                }
                _ => {
                    ran.stopped = Some(AppendFailure::Unavailable);
                    return;
                }
            }
        }
        let next = self
            .kept
            .last()
            .map_or(0, |k| tick_of(&k.opening.tick).map_or(0, |t| t + 1));
        let last = due.min(next.saturating_add(MOST_PER_RUN - 1));
        for tick in next..=last {
            if let Err(failure) = self.issue(tick) {
                ran.stopped = Some(failure);
                break;
            }
        }
    }

    /// Records, durably, that the caller has published everything in `ran`, so no later run, nor a checkpointer
    /// opened on this store after a restart, hands them out again. An acknowledgement the store doesn't write is
    /// `Unavailable` and changes nothing: they are handed out again, which is safe.
    pub fn acknowledge(&mut self, ran: &Ran) -> Result<(), AppendFailure> {
        let Some(through) = ran.through else {
            return Ok(());
        };
        if through >= self.kept.len() as u64 {
            return Err(AppendFailure::Refused(Violation(NO_SUCH_CHECKPOINT)));
        }
        if self.acked.is_some_and(|a| a >= through) {
            return Ok(());
        }
        let record = serde_json::to_vec(&Cursor {
            published_through: tick_text(through),
        })
        .map_err(|_| AppendFailure::Unavailable)?;
        if self.store.append(&record).is_err() {
            self.stale = true;
            return Err(AppendFailure::Unavailable);
        }
        self.acked = Some(through);
        Ok(())
    }

    fn issue(&mut self, tick: u64) -> Result<Vec<u8>, AppendFailure> {
        let mut salt = [0u8; 32];
        getrandom::fill(&mut salt).map_err(|_| AppendFailure::Unavailable)?;
        let (size, root, log) = {
            let log = self.log.borrow();
            (log.len(), log.root(), log.settings().log.clone())
        };
        let at = self.schedule.at(tick).ok_or(AppendFailure::Unavailable)?;
        let mut checkpoint = Checkpoint {
            shape: CHECKPOINT_SHAPE.to_owned(),
            log: log.clone(),
            tick: tick_text(tick),
            at,
            commitment: commitment(&log, tick, &salt, size as u64, &root),
            signature: ReceiptSignature {
                scheme: "ed25519".to_owned(),
                key: self.public.clone(),
                value: String::new(),
            },
        };
        let unsigned = serde_json::to_vec(&checkpoint).map_err(|_| AppendFailure::Unavailable)?;
        let input = signing_input(&unsigned).map_err(|_| AppendFailure::Unavailable)?;
        checkpoint.signature.value = base64url(&self.key.sign(&input).to_bytes());
        let published = serde_json::to_vec(&checkpoint).map_err(|_| AppendFailure::Unavailable)?;
        let kept = Kept {
            checkpoint,
            opening: Opening {
                shape: OPENING_SHAPE.to_owned(),
                log,
                tick: tick_text(tick),
                salt: base64url(&salt),
                size: SafeInteger::try_from(size as u64).expect("a log of fewer than 2^53 entries"),
            },
        };
        let record = serde_json::to_vec(&kept).map_err(|_| AppendFailure::Unavailable)?;
        if self.store.append(&record).is_err() {
            self.stale = true;
            return Err(AppendFailure::Unavailable);
        }
        self.kept.push(kept);
        Ok(published)
    }
}

/// The salt an opening holds, decoded, where it is 32 bytes.
pub(crate) fn salt_of(opening: &Opening) -> Option<Vec<u8>> {
    from_base64url(&opening.salt).filter(|s| s.len() == 32)
}
