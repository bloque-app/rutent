//! Reading a private node's record (`REQ-ORD-41`, `REQ-AUZ-1`, `T-020-58`).
//!
//! Closing says which records it reads ([`Grading::records_wanted`](super::Grading::records_wanted)), and the tick's job which orders are still unsettled at
//! a node; for each read ([`Segments::read_node_record`]) the segments do what they do for any action policy decides: draw
//! a fresh transaction, have policy decide `read_node_record` as Core, which carries the operator's standing permission
//! onward (`core-authz`'s roles, as `ask_status` is decided), record the effect before anything is sent, act on it once,
//! put the question, scoped to the transactions asked about, to the platform as that effect lets
//! ([`Modules::ask_for_record`](super::Modules::ask_for_record)) and record how it came out. What came back goes to the grading, which takes it only as
//! `closing::publishers::taken` allows. A refusal by policy asks nothing and the grading reads no record of that
//! provider: it fails closed. A read is always recorded to its result: where the result can't be written the answer is
//! not used.
//!
//! **Asked again, but not at every grading** (`T-020-67`). A publisher may not have its record yet, so a read whose answer
//! Core did not take (`not_known`, an answer the binding checks refuse, a read that failed) is asked again, but only once
//! the retry cadence has passed since the last (the schedule's own for the publishers' records, [`Job::PublishersRecords`]:
//! [`Segments::set_record_retry_seconds`] sets another), and no more than [`MAX_RECORD_TRIES`] times for a segment, node and
//! provider. Past that Core asks no more, and the segment waits on what it waits on, which the stuck check reports. The
//! tries are the log's `read_node_record` effects, so a Core that reopens counts them again from the log
//! ([`Segments::reopened`](super::Segments::reopened)).

use rutent_contracts::core_internal::{Action, DecisionRequest, Effect, Outcome, What};
use rutent_contracts::provider_role::{Operation, Statement};
use rutent_contracts::router_api::Id;

use rutent_contracts::router_api::Timestamp;

use super::journal;
use super::ports::{Answered, ToGrade};
use super::{Parts, Segments, now, sign};
use crate::closing::publishers;
use crate::schedule::Job;

/// The most times Core asks one provider for one node's record on behalf of one segment (`T-020-67`): at the retry cadence
/// of an hour, a day's worth. Counted from the log's `read_node_record` effects, so it holds across a restart.
pub const MAX_RECORD_TRIES: u32 = 24;

/// The most times the tick asks one provider for one node's record on behalf of one segment while a Core lives
/// (`T-020-67`): its own bound, three days at the hourly retry, apart from the grading's [`MAX_RECORD_TRIES`], so that a
/// segment that waits long is still graded with a record the publisher has by then. Not rebuilt on a restart.
pub const MAX_TICK_TRIES: u32 = 72;

/// Whether `retry` seconds have passed since `last` at `now`, a clock that went back counting as passed.
fn now_or_back(now: u64, last: u64, retry: u64) -> bool {
    now < last || now - last >= retry
}

/// What the log says of the reads of one segment, node and provider: how many Core put, and when the last was, in whole
/// seconds of Core's clock.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Tries {
    pub(super) count: u32,
    pub(super) last: u64,
}

/// A key of the tries: order, segment, node, provider.
type Key = (String, String, String, String);

/// One read of a private node's record: whose it is for (an order's segment that passes the node, which the action
/// names), the node and the provider asked, the transactions asked about, the bounds the question is made in, the
/// operator in whose name it is signed and Core, who asks policy and records it.
#[derive(Debug, Clone, Copy)]
pub struct RecordRead<'a> {
    /// The order.
    pub order: &'a Id,
    /// A segment of the order that passes the node.
    pub segment: &'a Id,
    /// The private node.
    pub node: &'a str,
    /// The provider of the node's publisher that is asked.
    pub provider: &'a str,
    /// The transactions the question is about: the segments whose record is asked for, at least one.
    pub about: &'a [Id],
    /// The bounds the question is made in: the deployment and protocol version of the order's route record.
    pub bounds: &'a rutent_contracts::router_api::PolicyBounds,
    /// The operator, in whose name the question is signed.
    pub operator: &'a rutent_contracts::core_internal::Actor,
    /// Core, who asks policy and records the effect and its result.
    pub core: &'a rutent_contracts::core_internal::Actor,
}

/// How a read came out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Read {
    /// Policy refused it: nothing was asked and nothing recorded but policy's own refusal.
    Denied,
    /// It couldn't be done now (an entry the log couldn't take, no answer from the platform): worth trying again.
    Failed,
    /// The question as put, the answer as the platform passed it on, and Core's time when it got it.
    Answered(Box<Statement<Operation>>, Vec<u8>, Timestamp),
}

impl Segments {
    /// How often a node-record read Core didn't take is asked again, in seconds: the deployment's setting, else the
    /// schedule's cadence for the publishers' records.
    pub fn record_retry_seconds(&self) -> u64 {
        self.record_retry
            .unwrap_or(Job::PublishersRecords.default_seconds())
    }

    /// Sets how often a node-record read Core didn't take is asked again (the operator's `publishers_records` cadence).
    pub fn set_record_retry_seconds(&mut self, seconds: u64) {
        self.record_retry = Some(seconds);
    }

    /// How many times Core has put the read of `node`'s record to `provider` for `segment` of `order`, as the log holds it.
    pub fn record_tries(&self, order: &Id, segment: &Id, node: &str, provider: &str) -> u32 {
        self.record_tries
            .get(&(
                order.as_str().to_owned(),
                segment.as_str().to_owned(),
                node.to_owned(),
                provider.to_owned(),
            ))
            .map_or(0, |tries| tries.count)
    }

    /// Notes a read put at `at` (whole seconds of Core's clock): counted once its effect is in the log.
    pub(super) fn note_record_try(&mut self, key: Key, at: u64) {
        let tries = self.record_tries.entry(key).or_default();
        tries.count = tries.count.saturating_add(1);
        tries.last = tries.last.max(at);
    }

    /// Notes a tick's read put (`T-020-67`), counted once its effect is in the log: a read that failed before its effect
    /// counts no try (the milestone reviewer's check of `T-020-67`, Low 3), and a reopened Core counts each from the log's
    /// `by: tick` effects (Low 1).
    pub(super) fn note_tick_try(&mut self, key: Key, at: u64) {
        *self.tick_tries.entry(key.clone()).or_insert(0) += 1;
        let last = self.tick_last.entry(key).or_insert(at);
        *last = (*last).max(at);
    }

    /// That the tick took the record of `key` (rebuilt from the log on reopening): no tick asks for it again.
    pub(super) fn note_tick_done(&mut self, key: Key) {
        self.tick_done.insert(key);
    }

    /// How many times the tick has put the read of `node`'s record to `provider` for `segment` of `order`.
    pub fn tick_tries(&self, order: &Id, segment: &Id, node: &str, provider: &str) -> u32 {
        self.tick_tries
            .get(&(
                order.as_str().to_owned(),
                segment.as_str().to_owned(),
                node.to_owned(),
                provider.to_owned(),
            ))
            .copied()
            .unwrap_or(0)
    }

    /// Whether a read for `key` may be put at `now`: its record isn't taken, fewer than [`MAX_RECORD_TRIES`] have been put,
    /// and the retry cadence has passed since the last (a clock that went back makes it due: it fails closed on the side
    /// of asking, as the status cadence does, but never past the cap).
    fn record_due(&self, key: &Key, now: u64) -> bool {
        self.record_tries.get(key).is_none_or(|tries| {
            tries.count < MAX_RECORD_TRIES
                && now_or_back(now, tries.last, self.record_retry_seconds())
        })
    }

    /// Asks for each record the grading of `settled` reads, and hands the grading what came back. A read is not asked
    /// again for the segment once policy refused it or its record was taken; a failed one, or an answer that was no record
    /// yet, is tried again only once the retry cadence has passed, and up to [`MAX_RECORD_TRIES`] times (`T-020-67`).
    pub(super) fn read_records(
        &mut self,
        parts: &mut Parts<'_>,
        live: &super::Live<'_>,
        settled: &ToGrade<'_>,
    ) {
        for wanted in parts.grading.records_wanted(settled) {
            let asked = (
                live.order.as_str().to_owned(),
                settled.segment.as_str().to_owned(),
                wanted.node.clone(),
                wanted.provider.clone(),
            );
            if self.records_read.contains(&asked) {
                continue;
            }
            let Ok(at) = now(parts.clock) else {
                continue;
            };
            let seconds = u64::try_from(at.unix_nanos() / 1_000_000_000).unwrap_or(0);
            if !self.record_due(&asked, seconds) {
                continue;
            }
            let about: Vec<Id> = wanted.about.iter().filter_map(Id::new).collect();
            let read = RecordRead {
                order: live.order,
                segment: settled.segment,
                node: &wanted.node,
                provider: &wanted.provider,
                about: &about,
                bounds: &live.record.bounds,
                operator: live.operator,
                core: live.core,
            };
            let done = match self.read_node_record(parts, &read) {
                Read::Denied => true,
                Read::Failed => {
                    parts.grading.record_read(live.order, &wanted, None, &at);
                    false
                }
                Read::Answered(question, bytes, received) => parts.grading.record_read(
                    live.order,
                    &wanted,
                    Some((&question, bytes.as_slice())),
                    &received,
                ),
            };
            // Asked now, whatever came of it: a read that left no effect (nothing could be written) waits its cadence too.
            let tries = self.record_tries.entry(asked.clone()).or_default();
            tries.last = tries.last.max(seconds);
            if done {
                self.records_read.insert(asked);
            }
        }
    }

    /// One read: decided, recorded as an effect, asked, and its result recorded. Nothing is asked before its effect is in
    /// the log (`INV-7`), and an answer is handed back only once its result is.
    pub fn read_node_record(&mut self, parts: &mut Parts<'_>, read: &RecordRead<'_>) -> Read {
        self.ask_node_record(parts, read, true)
    }

    /// The tick's read of a node's record (`Job::PublishersRecords`, `T-020-67`): the same decided read, but held to its
    /// own due time (not before the retry cadence has passed since the tick's last read of the key), to a bound of its own
    /// ([`MAX_TICK_TRIES`], not counted among the grading's tries, so it never starves the grading), and never put once the
    /// tick has taken the key's record. `None` where it isn't due.
    pub fn read_node_record_on_tick(
        &mut self,
        parts: &mut Parts<'_>,
        read: &RecordRead<'_>,
    ) -> Option<Read> {
        let key = (
            read.order.as_str().to_owned(),
            read.segment.as_str().to_owned(),
            read.node.to_owned(),
            read.provider.to_owned(),
        );
        if self.tick_done.contains(&key)
            || self.tick_tries.get(&key).copied().unwrap_or(0) >= MAX_TICK_TRIES
        {
            return None;
        }
        let at = now(parts.clock).ok()?;
        let seconds = u64::try_from(at.unix_nanos() / 1_000_000_000).unwrap_or(0);
        let due = self
            .tick_last
            .get(&key)
            .is_none_or(|last| now_or_back(seconds, *last, self.record_retry_seconds()));
        if !due {
            return None;
        }
        let read = self.ask_node_record(parts, read, false);
        // Asked now, whatever came of it: a read that left no effect waits its cadence too, but counts no try; one that
        // did was counted when its effect was appended.
        let last = self.tick_last.entry(key).or_insert(seconds);
        *last = (*last).max(seconds);
        Some(read)
    }

    /// That the record of `node` asked of `provider` for `segment` of `order` was taken (by the tick's read, whose answer
    /// feeds the books): it is asked for no more by the grading or the tick.
    pub fn note_record_taken(&mut self, order: &Id, segment: &Id, node: &str, provider: &str) {
        let key = (
            order.as_str().to_owned(),
            segment.as_str().to_owned(),
            node.to_owned(),
            provider.to_owned(),
        );
        self.tick_done.insert(key.clone());
        self.records_read.insert(key);
    }

    fn ask_node_record(
        &mut self,
        parts: &mut Parts<'_>,
        read: &RecordRead<'_>,
        counted: bool,
    ) -> Read {
        let (Some(node), Some(provider)) = (Id::new(read.node), Id::new(read.provider)) else {
            return Read::Failed;
        };
        if read.about.is_empty() {
            return Read::Failed;
        }
        let Ok(transaction) = self.claim_transaction(parts.fresh, parts.issued) else {
            return Read::Failed;
        };
        let action = Action::ReadNodeRecord {
            order: read.order.clone(),
            segment: read.segment.clone(),
            node,
            provider: provider.clone(),
            transaction: transaction.clone(),
            about: read.about.to_vec(),
            by: if counted {
                rutent_contracts::core_internal::ReadBy::Grading
            } else {
                rutent_contracts::core_internal::ReadBy::Tick
            },
        };
        let Ok(at) = now(parts.clock) else {
            return Read::Failed;
        };
        let request = DecisionRequest {
            request: parts.fresh.id(),
            caller: read.core.clone(),
            action: action.clone(),
            at,
        };
        let Ok(decision) = parts.decisions.decide(&request) else {
            return Read::Failed;
        };
        let Some(permit) = decision.permit() else {
            return Read::Denied;
        };
        let Ok(effect_at) = now(parts.clock) else {
            return Read::Failed;
        };
        let Ok(receipt) = journal::append(
            parts.log,
            journal::verified(
                read.core,
                What::Effect {
                    action: action.clone(),
                    decision: permit.clone(),
                },
                &effect_at,
                Outcome::ToTakeEffect {},
            ),
        ) else {
            return Read::Failed;
        };
        // The read is the log's from here: it counts as a try, the grading's or the tick's, which a restart counts again
        // from the effect.
        let key = (
            read.order.as_str().to_owned(),
            read.segment.as_str().to_owned(),
            read.node.to_owned(),
            read.provider.to_owned(),
        );
        let effect_seconds = u64::try_from(effect_at.unix_nanos() / 1_000_000_000).unwrap_or(0);
        if counted {
            self.note_record_try(key, effect_seconds);
        } else {
            self.note_tick_try(key, effect_seconds);
        }
        let effect = Effect {
            action,
            decision: permit,
            receipt: receipt.clone(),
        };
        // The result's time is read now, before anything is asked, so no early return can follow the question.
        let result_at = now(parts.clock);
        let question = self
            .act_on(&receipt)
            .then(|| {
                let signed = sign::sign(
                    parts.operator_key,
                    read.bounds,
                    &read.operator.acts,
                    result_at.as_ref().ok()?,
                    publishers::question(
                        read.provider,
                        read.node,
                        transaction.as_str(),
                        &read
                            .about
                            .iter()
                            .map(|t| t.as_str().to_owned())
                            .collect::<Vec<_>>(),
                    ),
                )
                .ok()?;
                serde_json::from_slice::<Statement<Operation>>(&signed).ok()
            })
            .flatten();
        let answered = match &question {
            Some(question) => parts.modules.ask_for_record(&effect, &provider, question),
            None => {
                Answered::Refused(rutent_contracts::module_interface::PlatformRefusal::Unavailable)
            }
        };
        let outcome = match (&question, &answered) {
            (_, Answered::Bytes(_)) => Outcome::Done {},
            (Some(_), Answered::Refused(_)) => Outcome::Unanswered {},
            (None, _) => Outcome::Failed {},
        };
        let wrote = result_at.ok().and_then(|at| {
            journal::append(
                parts.log,
                journal::verified(read.core, What::Result { effect: receipt }, &at, outcome),
            )
            .ok()
        });
        // An answer is used only once the entry that records how the read came out is in the log, and it is judged at the
        // time Core got it (`T-020-67`).
        match (question, answered, wrote, now(parts.clock)) {
            (Some(question), Answered::Bytes(bytes), Some(_), Ok(received)) => {
                // The time the answer is judged at: when Core got it, but no later than the question's time plus the
                // platform's own configured operation deadline (`T-020-67`).
                let latest = Timestamp::new(question.made_at.clone())
                    .and_then(|asked| u64::try_from(asked.unix_nanos() / 1_000_000_000).ok())
                    .and_then(|asked| {
                        asked.checked_add(parts.modules.operation_deadline().as_secs())
                    })
                    .and_then(|secs| rutent_prov::catalogue::Instant(secs).rfc3339())
                    .and_then(Timestamp::new);
                let judged = match latest {
                    Some(latest) if latest.unix_nanos() < received.unix_nanos() => latest,
                    _ => received,
                };
                Read::Answered(Box::new(question), bytes, judged)
            }
            _ => Read::Failed,
        }
    }
}
