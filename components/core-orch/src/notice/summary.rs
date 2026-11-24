//! The summary entry of each notice (`T-023-03`, `REQ-ORD-20`, `REQ-ORD-7`): once a notice is delivered, or abandoned
//! for good, the log holds one `notice_summary` of it, and that entry, not a store of this module's own, is what a
//! restart reads to send it no more.
//!
//! **What an entry holds.** The receiver's party and role, what the notice was of (an order's change at its
//! sequence, or a raise's id), the number of the last try and how it ended. Nothing of the sealed notice, its
//! signature, the order's state or a raise's figures: identity appears as a reference only (`REQ-AUD-6`).
//!
//! **Once.** A notice is named by its [`Key`]. It is queued when it is delivered or abandoned, written by
//! [`Notices::summarise`], and known from then on as ended: the log's own entries are read for the summaries it holds
//! (`learn`) before anything is sent, so a notice a summary shows ended is neither sent nor summarised again, after a
//! restart as in the same run. An entry is built once and handed again unchanged while the log can't take it now, so
//! a retried append is one entry.
//!
//! **A raise's summary cites a raise of the log** (`notice-summary-names-its-raise`). The check records a raise after
//! the channel sent it, so a raise's summary waits until the log holds the raise's entry, and is never handed to the
//! log before.
//!
//! **What stays the module's.** The tries before the last are the [`Ledger`](super::Ledger)'s: they are counted in
//! the process, and the log keeps none of them.

use std::collections::HashSet;

use rutent_contracts::core_internal::calls::{AppendFailure, GeneralEntry, Log};
use rutent_contracts::core_internal::{
    Actor, Count, Entry, NoticeEnd, NoticeOf, NoticeReceiver, Outcome, RELATIONS, Receipt,
    TimeSource, VerifiedEntry, VerifiedKind, Violation, What, When,
};
use rutent_contracts::router_api::Id;

use super::{Key, NoticeError, Notices, Receiver};
use crate::snapshot::Clock;

/// How many times in all an append the log can't take now is tried in one pass.
const APPEND_TRIES: usize = 3;

/// A summary not written yet.
#[derive(Debug, Clone)]
pub(super) struct Pending {
    pub(super) key: Key,
    pub(super) last_try: u32,
    pub(super) end: NoticeEnd,
    /// The entry, built on the first pass and handed again as it is.
    entry: Option<GeneralEntry>,
}

impl Pending {
    pub(super) fn new(key: Key, last_try: u32, end: NoticeEnd) -> Self {
        Pending {
            key,
            last_try,
            end,
            entry: None,
        }
    }
}

/// What one pass of summarising did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Summarised {
    /// Summaries the log took now.
    pub written: usize,
    /// Summaries kept for a later pass: the log couldn't take them now, or a raise's entry isn't in it yet.
    pub waiting: usize,
    /// Summaries kept in [`Notices::refused`] this pass: the log refused them, or none could be built, for a reason
    /// other than the log already holding another summary of the notice (that one is dropped, nothing is lost).
    pub refused: usize,
}

/// A summary the log refused, or that couldn't be built, for a reason that is no mere conflict with another summary of
/// the notice: a rule broken. It is kept for the assembly (`F-024`) to surface to the operator, since nothing else
/// says the notice has no summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    /// The notice.
    pub key: Key,
    /// The rule broken.
    pub violation: Violation,
    /// The entry the log refused, where one was built.
    pub entry: Option<GeneralEntry>,
}

/// Whether `violation` is the log's `one-summary-per-notice`: it already holds another summary of the notice.
fn is_conflict(violation: &Violation) -> bool {
    RELATIONS
        .iter()
        .find(|row| row.name == "one-summary-per-notice")
        .is_some_and(|row| row.refusal == violation.0)
}

/// The refusal of a summary no entry can be built for.
const NOT_BUILT: Violation =
    Violation("a notice's summary can be built: ids and a last try of 1 or more");

/// How the log reads a raise: its own entry, which is written once and never updated, and the summary of its
/// notice, which closes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RaiseReading {
    /// Its notice was delivered: the raise's entry says `done`, or its summary says delivered.
    Delivered,
    /// Its notice isn't known to have been delivered, nor given up: the entry says `failed`, and no summary closes it.
    Undelivered,
    /// Its notice was given up: its summary says abandoned.
    Abandoned,
}

/// How `recorded`, the log from its first entry, reads the raise `raise`: `None` where the log holds neither the raise
/// nor a summary of its notice (a summary alone is read, as `Delivered` or `Abandoned`). A summary of the raise's
/// notice outranks the raise's own entry, which is never updated.
pub fn raise_reading(recorded: &[(Receipt, Entry)], raise: &Id) -> Option<RaiseReading> {
    let mut reading = None;
    for (_, entry) in recorded {
        let Entry::Verified(v) = entry else {
            continue;
        };
        match &v.what {
            What::Raise { raise: r } if &r.raise == raise && reading.is_none() => {
                reading = Some(if v.outcome == (Outcome::Done {}) {
                    RaiseReading::Delivered
                } else {
                    RaiseReading::Undelivered
                });
            }
            What::NoticeSummary {
                of: NoticeOf::Raise { raise: r },
                ended,
                ..
            } if r == raise => {
                return Some(match ended {
                    NoticeEnd::Delivered => RaiseReading::Delivered,
                    NoticeEnd::Abandoned => RaiseReading::Abandoned,
                });
            }
            _ => {}
        }
    }
    reading
}

/// The notice a summary names, as the module keys it.
fn key_of(to: &NoticeReceiver, of: &NoticeOf) -> Key {
    match of {
        NoticeOf::Order { order, sequence } => Key::Order {
            to: Receiver {
                party: to.party.clone(),
                role: to.role,
            },
            order: order.as_str().to_owned(),
            sequence: sequence.get(),
        },
        NoticeOf::Raise { raise } => Key::Raise {
            raise: raise.as_str().to_owned(),
        },
        NoticeOf::Waiting { order, quote } => Key::Waiting {
            to: Receiver {
                party: to.party.clone(),
                role: to.role,
            },
            order: order.as_str().to_owned(),
            quote: serde_json::to_string(quote).ok(),
        },
    }
}

impl Notices {
    /// Whether the notice named `key` is known delivered: by the ledger, or by a summary the log holds.
    pub(super) fn is_delivered(&self, key: &Key) -> bool {
        self.ledger.delivered(key) || matches!(self.ended.get(key), Some((NoticeEnd::Delivered, _)))
    }

    /// Queues the summary of the notice named `key`, once it ended, unless the log holds one or it is queued.
    pub(super) fn queue_summary(&mut self, key: &Key, last_try: u32, end: NoticeEnd) {
        // The end of a wait has no summary: the ledger's mark is all of it.
        if matches!(key, Key::Waiting { quote: None, .. })
            || self.ended.contains_key(key)
            || self.pending.iter().any(|p| &p.key == key)
        {
            return;
        }
        self.pending.push(Pending::new(key.clone(), last_try, end));
    }

    /// Reads the summaries and raises `recorded` holds past those learned from before. A summary the log holds
    /// ends its notice here, and closes the one queued of it.
    pub(super) fn learn(&mut self, recorded: &[(Receipt, Entry)]) {
        // A shorter log than one learned from is no news: what was learned of it stands.
        let fresh = recorded.get(self.learned..).unwrap_or_default();
        for (_, entry) in fresh {
            let Entry::Verified(v) = entry else {
                continue;
            };
            match &v.what {
                What::NoticeSummary {
                    to,
                    of,
                    last_try,
                    ended,
                } => {
                    let key = key_of(to, of);
                    let last = u32::try_from(last_try.get()).unwrap_or(u32::MAX);
                    self.ended.entry(key.clone()).or_insert((*ended, last));
                    self.pending.retain(|p| p.key != key);
                }
                What::Raise { raise } => {
                    self.raises.insert(raise.raise.as_str().to_owned());
                }
                _ => {}
            }
        }
        self.learned = self.learned.max(recorded.len());
    }

    /// The summaries the log refused for a rule broken, in the order they were refused, and never retried: the
    /// assembly surfaces each to the operator.
    pub fn refused(&self) -> &[Refused] {
        &self.refused
    }

    /// Writes the summary of each notice that ended (delivered, or abandoned for good) and has none in the log, as
    /// Core, `who`, at `clock`'s now, through the general handle. A summary the log can't take now is kept for the
    /// next pass; a raise's waits for its entry. Once written a summary is never written again, in this run or after
    /// a restart that reads the log. The assembly calls this after `notify` and `retry`, and after the checks
    /// recorded the raises they raised.
    pub fn summarise(
        &mut self,
        recorded: &[(Receipt, Entry)],
        who: &Actor,
        clock: &dyn Clock,
        log: &mut dyn Log,
    ) -> Result<Summarised, NoticeError> {
        self.unread(recorded)?;
        self.learn(recorded);
        let at = clock.now().timestamp().ok_or(NoticeError::NoTime)?;
        let mut done = Summarised::default();
        for mut held in std::mem::take(&mut self.pending) {
            if let Key::Raise { raise } = &held.key
                && !self.raises.contains(raise)
            {
                done.waiting += 1;
                self.pending.push(held);
                continue;
            }
            if held.entry.is_none() {
                let Some(entry) = self.build(&held, who, &at) else {
                    self.refused.push(Refused {
                        key: held.key,
                        violation: NOT_BUILT,
                        entry: None,
                    });
                    done.refused += 1;
                    continue;
                };
                held.entry = Some(entry);
            }
            let entry = held.entry.clone().expect("built above");
            match append(log, &entry) {
                Ok(_) => {
                    self.ended
                        .insert(held.key.clone(), (held.end, held.last_try));
                    done.written += 1;
                }
                Err(AppendFailure::Unavailable) => {
                    done.waiting += 1;
                    self.pending.push(held);
                }
                // The log holds another summary of the notice: it is summarised, and nothing is lost.
                Err(AppendFailure::Refused(violation)) if is_conflict(&violation) => {
                    self.ended
                        .entry(held.key.clone())
                        .or_insert((held.end, held.last_try));
                }
                Err(AppendFailure::Refused(violation)) => {
                    self.refused.push(Refused {
                        key: held.key,
                        violation,
                        entry: Some(entry),
                    });
                    done.refused += 1;
                }
            }
        }
        Ok(done)
    }

    /// The entry of `held`'s summary.
    fn build(
        &self,
        held: &Pending,
        who: &Actor,
        at: &rutent_contracts::router_api::Timestamp,
    ) -> Option<GeneralEntry> {
        let (to, of) = match &held.key {
            Key::Order {
                to,
                order,
                sequence,
            } => (
                NoticeReceiver {
                    party: to.party.clone(),
                    role: to.role,
                },
                NoticeOf::Order {
                    order: Id::new(order)?,
                    sequence: Count::new(*sequence)?,
                },
            ),
            Key::Raise { raise } => (
                NoticeReceiver {
                    party: self.operator.party.clone(),
                    role: self.operator.role,
                },
                NoticeOf::Raise {
                    raise: Id::new(raise)?,
                },
            ),
            Key::Waiting {
                to,
                order,
                quote: Some(quote),
            } => (
                NoticeReceiver {
                    party: to.party.clone(),
                    role: to.role,
                },
                NoticeOf::Waiting {
                    order: Id::new(order)?,
                    quote: serde_json::from_str(quote).ok()?,
                },
            ),
            Key::Waiting { quote: None, .. } => return None,
        };
        let entry = Entry::Verified(VerifiedEntry {
            kind: VerifiedKind::Verified,
            who: who.clone(),
            what: What::NoticeSummary {
                to,
                of,
                last_try: Count::new(u64::from(held.last_try.max(1)))?,
                ended: held.end,
            },
            when: When {
                at: at.clone(),
                source: TimeSource::CoreClock,
            },
            outcome: Outcome::Observed {},
        });
        entry.check().ok()?;
        GeneralEntry::try_from(entry).ok()
    }
}

/// Appends `entry`, trying again while the log can't take it now. A refusal is never retried.
fn append(log: &mut dyn Log, entry: &GeneralEntry) -> Result<Receipt, AppendFailure> {
    let mut last = AppendFailure::Unavailable;
    for _ in 0..APPEND_TRIES {
        match log.append(entry) {
            Ok(receipt) => return Ok(receipt),
            Err(AppendFailure::Unavailable) => last = AppendFailure::Unavailable,
            Err(refused) => return Err(refused),
        }
    }
    Err(last)
}

/// The raises the log holds, by id: what a raise's summary may cite.
pub(super) type Raises = HashSet<String>;
