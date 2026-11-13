//! Notices: every change of an order's state notified to its originating user and the operator, and delivery to the
//! operator watched (`T-023-01`, `T-023-02`). `F-023` owns this module.
//!
//! Declared by `T-020-01`; only `F-023`'s rows edit it (`PLAN.md` §4.4).
//!
//! **From the log, never from the caller** (`REQ-ORD-20`, `AC-41`). [`Notices::notify`] reads the log's entries,
//! each with its receipt, as the log holds them, in its order, from where it last stopped, as `Stuck::follow` and
//! policy's charges read them. Every input the state machine applies to an order (`Stm::open`, `Stm::transition`), a
//! failure included (a segment failed, a withdrawal cancelled, a part stuck, an order failed), is one change; so is
//! something recorded after an order ended (`afterwards`, `REQ-ORD-31`, `REQ-ORD-25`) and a refusal naming an order
//! Core follows. An input the log takes and the machine doesn't apply (a repeat, a late report) and an input the log
//! recorded as not taken change nothing and are notified to no one. Each change is the order's next sequence,
//! counted from 1 in the log's order, so the same log gives the same notices, byte for byte, after a restart: what is
//! signed holds the change's recorded time, unconverted, never the clock's. A read must extend the one before: the
//! receipt of the last entry read must be where it was ([`NoticeError::NotThisLog`]).
//!
//! **What a read compares.** Only the receipt of the last entry read (the log's name, its index and the entry's
//! digest). That rests on the log being append-only, as core-internal's is: an entry already read is never changed,
//! so a log that still holds the last entry read at its place holds every one before it. A log that rewrote an
//! earlier entry and kept the last would pass unseen; no such log exists, and comparing every entry would cost more
//! the longer the log.
//!
//! **At least once, and caught up on a restart.** The notices read the log from its first entry when built afresh,
//! as after a restart, and rebuild every change; what the [`Ledger`] holds as delivered is not sent again
//! ([`Notices::resuming`]), so a restart sends only what the transport hadn't taken. A mark that couldn't be kept
//! leaves its notice to be sent again, and a receiver drops each copy by its sequence.
//!
//! **Its receivers come from Core's records.** The user is the requester the order's `acceptance` names, receiving
//! as `customer`; the operator is the deployment's operator party, set when the notices are built, receiving as
//! `operator`. An order the log holds no acceptance for is notified to the operator alone. A notice carries the
//! order's state as `Reads::OrderState` reads it (`REQ-ORD-5`): its state, each part's state and when it reached
//! them; and what changed: the change's kind, the part, a stuck part's reason, the linked order of a hand-on (or of one
//! taken back), a refusal's class, an `afterwards` event; all of the customer's own order. Never a provider, segment, amount,
//! account or a provider's text (`REQ-AUZ-7`, `REQ-ORD-18`). A raise and a charge counted far ahead are the
//! operator's alone. A push asks policy nothing; asking for the current state does.
//!
//! **Authenticated for its receiver.** Each notice is sealed for one receiver: the deployment, the receiver's party
//! and role, and what it is about, signed with the deployment's notice key ([`NoticeKey`], over
//! [`wire::signing_input`]). A receiver takes it only once that checks and it is addressed to it ([`Inbox::take`]),
//! so a notice for another receiver fails, and a copy re-addressed fails its signature.
//!
//! **Safe repeated or reordered.** Each order's notice carries the order's whole state after its change, at its
//! sequence; the receiver keeps the latest sequence and takes only a later one ([`Inbox`]).
//!
//! **The current state on request** ([`Notices::current`]). A reader asks for an order's state; policy checks the
//! read (`Reads::OrderState`) and records it either way. A reader policy refuses is told [`CurrentError::Refused`]
//! and nothing else, whatever policy's reason, so it can't tell an order that exists from one that doesn't; the
//! reason stays in the log. An allowed reader is answered with the order's latest change as the log holds it to its
//! end, sealed for it and marked as the current state; asking sends nothing to anyone.
//!
//! **The operator's channel** ([`Channel`]). `Operator::raise` (the checks' differences, a change in a provider's
//! p99 from `Stuck::learn`) is sealed for the operator alone and sent; it answers delivered only when the transport
//! took it. [`Notices::ahead`] does the same for each charge policy set apart as counted more than a window ahead of
//! Core's clock (`Charged::ahead()`, `T-021-05`), each once by its party and request while policy sets it apart.
//!
//! **Delivery.** Each notice goes through the [`Transport`] the assembly gives. One the transport didn't take is kept
//! ([`Notices::undelivered`]) and tried again by [`Notices::retry`], at most [`BOUND`] tries in all (counted from the
//! ledger, so a restart gives no fresh bound); every try is recorded in the [`Ledger`], and a notice that used them is
//! set apart ([`Notices::abandoned`]). See the `delivery` module's notes.
//!
//! **The watch** ([`Watch`], `REQ-ORD-7`). The path that warns the operator has a heartbeat of its own; a path that
//! answers nothing for [`SILENCE`] seconds is noticed on a second path.

mod delivery;
mod inbox;
mod watch;
pub mod wire;

use std::collections::{BTreeMap, BTreeSet};

use rutent_contracts::core_internal::calls::{AppendFailure, Delivery, Operator, Reads};
use rutent_contracts::core_internal::{
    Actor, Charge, Entry, Outcome, Raise, ReadCheck, ReadOutcome, Receipt, Role, What,
};
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_contracts::state_machine::{self as stm, Observed, RecordedInput, StateMachine};
use rutent_stm::Stm;

use crate::rounds::Fresh;
use crate::snapshot::Clock;

pub use delivery::{Attempt, BOUND, Key, Ledger, Retried, Volatile};
pub use inbox::{Inbox, NotTaken, Taken, Verifier};
pub use watch::{INTERVAL, Path, SILENCE, Tick, Watch};
pub use wire::{
    About, AheadSeen, At, Change, DOMAIN, Notice, NoticeKey, PartState, Receiver, View,
    signing_input,
};

/// The operator's channel before `F-023` builds it: it delivers nothing, and says so, so every raise stays
/// recorded as undelivered.
#[derive(Debug, Clone, Copy, Default)]
pub struct Unbuilt;

impl Operator for Unbuilt {
    fn raise(&mut self, _raise: &Raise) -> Delivery {
        Delivery::Undelivered
    }
}

/// How a sealed notice reaches its receiver. The assembly gives it; [`Notices::retry`] retries what it refused, and
/// [`Watch`] watches the one that warns the operator.
pub trait Transport {
    /// Hands `notice`, sealed, to `to`: true only where it was taken for delivery.
    fn send(&mut self, to: &Receiver, notice: &[u8]) -> bool;
}

/// A notice the transport didn't take, kept for [`Notices::retry`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Undelivered {
    /// Its receiver.
    pub to: Receiver,
    /// The sealed notice.
    pub notice: Vec<u8>,
    /// What names it in the ledger, where it has a place in the log to be rebuilt from.
    pub key: Option<Key>,
    /// How many times it was tried.
    pub attempts: u32,
}

/// Why the log's entries weren't all notified. What was notified before the error stays notified, and the next call
/// carries on from the entry that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeError {
    /// The entries don't extend those read before: a shorter or another log.
    NotThisLog,
    /// The notice key couldn't sign now: nothing of the entry was sent.
    Unsigned,
    /// Core's clock gave no time a notice can carry.
    NoTime,
}

/// Why the current state wasn't answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CurrentError {
    /// Policy refused the read, and recorded it. Nothing more is said, whatever its reason: an order that doesn't
    /// exist and another party's are refused alike.
    Refused,
    /// Policy couldn't record the check, so it answered nothing.
    Append(AppendFailure),
    /// The read was allowed, but the log holds no change of such an order.
    Unknown,
    /// The entries don't extend those read before: no state is answered from them.
    NotThisLog,
    /// The notice key couldn't sign now.
    Unsigned,
    /// Core's clock gave no time to check the read at.
    NoTime,
}

/// A charge policy set apart as counted more than a window ahead of Core's now (`Charged::ahead()`): the charge and
/// the moment it was counted at. The assembly hands them over after each charge whose read set any apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AheadCharge {
    /// The charge.
    pub charge: Charge,
    /// The moment it was counted at, by Core's clock.
    pub counted_at: Timestamp,
}

/// An order the notices follow.
#[derive(Debug, Clone)]
struct Followed {
    order: stm::Order,
    since: stm::Timestamp,
    sequence: u64,
    latest: About,
}

/// Core's notices: where the reading of the log stopped, the orders followed from it, and what wasn't delivered.
#[derive(Debug)]
pub struct Notices {
    deployment: Id,
    operator: Receiver,
    /// How many entries were read.
    read: usize,
    /// The receipt of the last entry read: a later read must hold it where it was. This compares that entry alone,
    /// which holds for an append-only log (see the module's notes on what a read compares).
    last: Option<Receipt>,
    requesters: BTreeMap<String, Id>,
    orders: BTreeMap<String, Followed>,
    ahead: BTreeSet<(String, String)>,
    undelivered: Vec<Undelivered>,
    abandoned: Vec<Undelivered>,
    ledger: Box<dyn Ledger>,
    bound: u32,
}

impl Notices {
    /// The notices of `deployment`, whose operator is the party `operator`.
    pub fn new(deployment: Id, operator: Id) -> Self {
        Self {
            deployment,
            operator: Receiver {
                party: operator,
                role: Role::Operator,
            },
            read: 0,
            last: None,
            requesters: BTreeMap::new(),
            orders: BTreeMap::new(),
            ahead: BTreeSet::new(),
            undelivered: Vec::new(),
            abandoned: Vec::new(),
            ledger: Box::new(Volatile::default()),
            bound: BOUND,
        }
    }

    /// The notices of `deployment`, whose operator is the party `operator`, resuming from `ledger`, the durable record
    /// of what was delivered.
    pub fn resuming(deployment: Id, operator: Id, ledger: Box<dyn Ledger>) -> Self {
        Self {
            ledger,
            ..Self::new(deployment, operator)
        }
    }

    /// The same notices with every notice tried at most `bound` times, the first send included (at least once).
    pub fn bounded(mut self, bound: u32) -> Self {
        self.bound = bound.max(1);
        self
    }

    /// Tries once more each notice the transport didn't take, in the order they were sent, and records each try. A
    /// notice that uses its last try now is abandoned. The assembly calls this on a schedule of its own.
    pub fn retry(&mut self, transport: &mut dyn Transport) -> Retried {
        let pending = std::mem::take(&mut self.undelivered);
        let abandoned = self.abandoned.len();
        let mut retried = Retried::default();
        for held in pending {
            if self.attempt(transport, held) {
                retried.delivered += 1;
            }
        }
        retried.abandoned = self.abandoned.len() - abandoned;
        retried.failed = self.undelivered.len();
        retried
    }

    /// The notices that used all their attempts, in the order they were given up.
    pub fn abandoned(&self) -> &[Undelivered] {
        &self.abandoned
    }

    /// The operator, as it receives.
    pub fn operator(&self) -> &Receiver {
        &self.operator
    }

    /// The notices the transport didn't take, in the order they were sent.
    pub fn undelivered(&self) -> &[Undelivered] {
        &self.undelivered
    }

    /// The receivers of `order`'s notices: the operator, and the order's requester as its customer once the log
    /// holds its acceptance.
    pub fn receivers(&self, order: &Id) -> Vec<Receiver> {
        self.receivers_of(order.as_str())
    }

    fn receivers_of(&self, order: &str) -> Vec<Receiver> {
        let mut to = vec![self.operator.clone()];
        if let Some(requester) = self.requesters.get(order) {
            to.push(Receiver {
                party: requester.clone(),
                role: Role::Customer,
            });
        }
        to
    }

    /// Where `recorded`, the log from its first entry, takes up the reading: past the entries read, once the last of
    /// them is where it was, by its receipt.
    fn unread<'r>(
        &self,
        recorded: &'r [(Receipt, Entry)],
    ) -> Result<&'r [(Receipt, Entry)], NoticeError> {
        let holds = match (self.read, &self.last) {
            (0, _) => true,
            (n, Some(last)) => recorded.get(n - 1).is_some_and(|(r, _)| r == last),
            (_, None) => false,
        };
        if holds {
            Ok(&recorded[self.read..])
        } else {
            Err(NoticeError::NotThisLog)
        }
    }

    /// Notifies every change the log's `recorded` entries hold past those read before, to each of its order's
    /// receivers, and answers how many notices were sent. `recorded` is the log as it holds it, from its first entry,
    /// each entry with its receipt.
    pub fn notify(
        &mut self,
        recorded: &[(Receipt, Entry)],
        notice_key: &mut dyn NoticeKey,
        transport: &mut dyn Transport,
    ) -> Result<usize, NoticeError> {
        let unread = self.unread(recorded)?;
        let mut sent = 0;
        for (receipt, entry) in unread {
            if let Some(order) = about(entry)
                && let Some(followed) = change(self.orders.get(order), entry)
            {
                let mut sealed = Vec::new();
                for to in self.receivers_of(order) {
                    let key = Key::Order {
                        to: to.clone(),
                        order: order.to_owned(),
                        sequence: followed.sequence,
                    };
                    if self.ledger.delivered(&key) {
                        continue;
                    }
                    let notice = Notice {
                        deployment: self.deployment.clone(),
                        to: to.clone(),
                        about: followed.latest.clone(),
                        current: false,
                    };
                    let notice = wire::seal(notice_key, notice).ok_or(NoticeError::Unsigned)?;
                    sealed.push((to, key, notice));
                }
                self.orders.insert(order.to_owned(), followed);
                for (to, key, notice) in sealed {
                    if self.send(transport, to, Some(key), notice).is_some() {
                        sent += 1;
                    }
                }
            }
            if let Entry::Verified(v) = entry
                && let What::Acceptance { order, parties, .. } = &v.what
            {
                self.requesters
                    .entry(order.as_str().to_owned())
                    .or_insert_with(|| parties.requester.clone());
            }
            if let Entry::Verified(v) = entry
                && let What::Raise { raise } = &v.what
                && v.outcome == (Outcome::Failed {})
            {
                sent += self.resend(raise, notice_key, transport)?;
            }
            self.read += 1;
            self.last = Some(receipt.clone());
        }
        Ok(sent)
    }

    /// A raise the log records as failed, which the log never updates: sent again unless the ledger holds it
    /// delivered (`send` sees to that), its tries counted on from the ledger. One still kept from this run (undelivered or abandoned) is
    /// the retry's, not sent here. Answers how many were sent.
    fn resend(
        &mut self,
        raise: &Raise,
        key: &mut dyn NoticeKey,
        transport: &mut dyn Transport,
    ) -> Result<usize, NoticeError> {
        let named = Key::Raise {
            raise: raise.raise.as_str().to_owned(),
        };
        let kept = self
            .undelivered
            .iter()
            .chain(&self.abandoned)
            .any(|u| u.key.as_ref() == Some(&named));
        if raise.check().is_err() || kept {
            return Ok(0);
        }
        let to = self.operator.clone();
        let notice = Notice {
            deployment: self.deployment.clone(),
            to: to.clone(),
            about: About::Raise {
                raise: raise.clone(),
            },
            current: false,
        };
        let sealed = wire::seal(key, notice).ok_or(NoticeError::Unsigned)?;
        Ok(usize::from(
            self.send(transport, to, Some(named), sealed).is_some(),
        ))
    }

    /// Sends `notice` to `to`: `Some(taken)` for a try made, `None` where none was: it used the bound before (it is set
    /// apart as abandoned).
    fn send(
        &mut self,
        transport: &mut dyn Transport,
        to: Receiver,
        key: Option<Key>,
        notice: Vec<u8>,
    ) -> Option<bool> {
        let tried = match &key {
            // A raise delivered already is not sent again.
            Some(key @ Key::Raise { .. }) if self.ledger.delivered(key) => return None,
            Some(key) => self.ledger.attempts(key),
            None => 0,
        };
        let held = Undelivered {
            to,
            notice,
            key,
            attempts: tried,
        };
        if tried >= self.bound {
            self.abandoned.push(held);
            return None;
        }
        Some(self.attempt(transport, held))
    }

    /// One more try of `held`, recorded; one the transport didn't take is kept, or abandoned on its last try.
    fn attempt(&mut self, transport: &mut dyn Transport, mut held: Undelivered) -> bool {
        held.attempts += 1;
        let taken = transport.send(&held.to, &held.notice);
        if let Some(key) = &held.key {
            // A mark that couldn't be kept leaves the notice sendable after a restart: at least once.
            let _ = self.ledger.record(&Attempt {
                key: key.clone(),
                number: held.attempts,
                taken,
            });
        }
        if !taken {
            if held.attempts >= self.bound {
                self.abandoned.push(held);
            } else {
                self.undelivered.push(held);
            }
        }
        taken
    }

    /// The current state of `order`, for `reader`, once policy allows the read (`Reads::OrderState`), which it
    /// records either way. A refused read is told [`CurrentError::Refused`] alone. An allowed one is answered with
    /// the order's latest change as `recorded`, the log from its first entry, holds it to its end, sealed for the
    /// reader (the deployment's operator for an operator's login, else the reader's party in its role) and marked
    /// as the current state. Nothing is sent and nothing of the reading moves: the changes not notified yet go out
    /// on the next [`Notices::notify`], to each of their receivers.
    #[allow(clippy::too_many_arguments)]
    pub fn current(
        &self,
        recorded: &[(Receipt, Entry)],
        reads: &mut dyn Reads,
        clock: &dyn Clock,
        fresh: &mut dyn Fresh,
        key: &mut dyn NoticeKey,
        reader: &Actor,
        permission: Option<&Id>,
        order: &Id,
    ) -> Result<Vec<u8>, CurrentError> {
        let at = clock.now().timestamp().ok_or(CurrentError::NoTime)?;
        let answer = reads
            .check_read(&ReadCheck {
                request: fresh.id(),
                reader: reader.clone(),
                reads: rutent_contracts::core_internal::Reads::OrderState {
                    order: order.clone(),
                },
                permission: permission.cloned(),
                at,
            })
            .map_err(CurrentError::Append)?;
        if !matches!(answer.outcome, ReadOutcome::Allowed {}) {
            return Err(CurrentError::Refused);
        }
        let unread = self
            .unread(recorded)
            .map_err(|_| CurrentError::NotThisLog)?;
        let mut followed = self.orders.get(order.as_str()).cloned();
        for (_, entry) in unread {
            if about(entry) == Some(order.as_str())
                && let Some(next) = change(followed.as_ref(), entry)
            {
                followed = Some(next);
            }
        }
        let followed = followed.ok_or(CurrentError::Unknown)?;
        let to = if reader.role == Role::Operator && reader.party == self.operator.party {
            self.operator.clone()
        } else {
            Receiver {
                party: reader.party.clone(),
                role: reader.role,
            }
        };
        wire::seal(
            key,
            Notice {
                deployment: self.deployment.clone(),
                to,
                about: followed.latest,
                current: true,
            },
        )
        .ok_or(CurrentError::Unsigned)
    }

    /// Notifies the operator, alone, of each charge in `charges` not notified before, by its party and request, and
    /// answers how many were sent: the party, the request id, the kind of work and its count, the moment it was
    /// counted at and Core's now, so the operator sees a clock that jumped and what it cost the party. `charges` is
    /// all policy sets apart now: what is kept of those notified is only what it still sets apart, so the record
    /// stays as small as policy's, and a charge set apart again later is sent again, which the operator's inbox holds
    /// once.
    pub fn ahead(
        &mut self,
        charges: &[AheadCharge],
        clock: &dyn Clock,
        key: &mut dyn NoticeKey,
        transport: &mut dyn Transport,
    ) -> Result<usize, NoticeError> {
        let now = clock.now().timestamp().ok_or(NoticeError::NoTime)?;
        let key_of = |a: &AheadCharge| {
            (
                a.charge.party.as_str().to_owned(),
                a.charge.request.as_str().to_owned(),
            )
        };
        let still: BTreeSet<_> = charges.iter().map(key_of).collect();
        self.ahead.retain(|seen| still.contains(seen));
        let mut sent = 0;
        for ahead in charges {
            let seen = key_of(ahead);
            if self.ahead.contains(&seen) {
                continue;
            }
            let notice = Notice {
                deployment: self.deployment.clone(),
                to: self.operator.clone(),
                about: About::Ahead {
                    charge: AheadSeen {
                        party: ahead.charge.party.clone(),
                        request: ahead.charge.request.clone(),
                        work: ahead.charge.work,
                        count: ahead.charge.count.get(),
                        counted_at: ahead.counted_at.clone(),
                        now: now.clone(),
                    },
                },
                current: false,
            };
            let sealed = wire::seal(key, notice).ok_or(NoticeError::Unsigned)?;
            self.ahead.insert(seen);
            let to = self.operator.clone();
            self.send(transport, to, None, sealed);
            sent += 1;
        }
        Ok(sent)
    }
}

/// The order `entry` is about, where it is one the notices read: an input's, an `afterwards`', a refusal naming
/// one.
fn about(entry: &Entry) -> Option<&str> {
    let Entry::Verified(v) = entry else {
        return None;
    };
    match &v.what {
        What::Input { input } => Some(input.subject.order.as_str()),
        What::Afterwards { order, .. }
        | What::OrderRefusal {
            order: Some(order), ..
        } => Some(order.as_str()),
        _ => None,
    }
}

/// How the order `entry` is about is followed after it, from `held`, how it was followed before: `None` where the
/// entry changes nothing. An input the machine applies is always a change, and nothing in it or in an entry's time
/// is converted, so no change can be dropped and leave the order followed behind the log.
fn change(held: Option<&Followed>, entry: &Entry) -> Option<Followed> {
    let Entry::Verified(v) = entry else {
        return None;
    };
    match &v.what {
        What::Input { input } => applied(held, input),
        What::Afterwards { event, .. } => noted(
            held?,
            At::from(&v.when.at),
            Change::Afterwards { event: *event },
        ),
        What::OrderRefusal { class, .. } => noted(
            held?,
            At::from(&v.when.at),
            Change::Refused { class: *class },
        ),
        _ => None,
    }
}

/// An input the state machine applies, and the order after it.
fn applied(held: Option<&Followed>, input: &RecordedInput) -> Option<Followed> {
    let (next, sequence) = match (held, &input.observed) {
        (None, Observed::RouteChosen { .. }) => (Stm::open(input)?, 1),
        (None, _) => return None,
        (Some(f), _) => {
            let next = Stm::transition(&f.order, input);
            if next.last_input.outcome != (stm::Outcome::Applied {}) {
                return None;
            }
            (next, f.sequence + 1)
        }
    };
    let (reason, linked_order) = match &input.observed {
        Observed::StuckDeclared { reason } => (Some(*reason), None),
        Observed::HandedOn { linked_order, .. } | Observed::HandOnTakenBack { linked_order } => {
            (None, Some(linked_order.clone()))
        }
        _ => (None, None),
    };
    let change = Change::Input {
        input: kind(&input.observed).to_owned(),
        part: input.subject.part.clone(),
        reason,
        linked_order,
    };
    let since = input.recorded_at.clone();
    Some(Followed {
        latest: About::Order {
            order: next.order_id.clone(),
            sequence,
            at: At::from(&since),
            change,
            view: view(&next, &since),
        },
        order: next,
        since,
        sequence,
    })
}

/// A change recorded about an order the notices follow that leaves its state as it was.
fn noted(f: &Followed, at: At, change: Change) -> Option<Followed> {
    let sequence = f.sequence + 1;
    Some(Followed {
        latest: About::Order {
            order: f.order.order_id.clone(),
            sequence,
            at,
            change,
            view: view(&f.order, &f.since),
        },
        order: f.order.clone(),
        since: f.since.clone(),
        sequence,
    })
}

/// The operator's channel: each raise sealed for the operator alone and sent (ADR-0009). The assembly hands it
/// wherever `Operator` is asked for, the checks and `Stuck::learn` among them.
pub struct Channel<'a> {
    /// The notices, which hold the operator and keep what wasn't delivered.
    pub notices: &'a mut Notices,
    /// The notice key.
    pub key: &'a mut dyn NoticeKey,
    /// The transport.
    pub transport: &'a mut dyn Transport,
}

impl Operator for Channel<'_> {
    /// Delivered only when the raise checks, is sealed and the transport took it; otherwise undelivered, and the
    /// raise's entry records it so.
    fn raise(&mut self, raise: &Raise) -> Delivery {
        if raise.check().is_err() {
            return Delivery::Undelivered;
        }
        let to = self.notices.operator.clone();
        let key = Key::Raise {
            raise: raise.raise.as_str().to_owned(),
        };
        let notice = Notice {
            deployment: self.notices.deployment.clone(),
            to: to.clone(),
            about: About::Raise {
                raise: raise.clone(),
            },
            current: false,
        };
        let Some(sealed) = wire::seal(self.key, notice) else {
            return Delivery::Undelivered;
        };
        let taken = self
            .notices
            .send(self.transport, to, Some(key.clone()), sealed);
        // No try was made where it was delivered already, or used its bound before.
        if taken == Some(true) || (taken.is_none() && self.notices.ledger.delivered(&key)) {
            Delivery::Delivered
        } else {
            Delivery::Undelivered
        }
    }
}

/// An order's state as a notice carries it.
fn view(order: &stm::Order, since: &stm::Timestamp) -> View {
    View {
        state: order.state,
        parts: order
            .parts
            .iter()
            .map(|p| PartState {
                part: p.part_id.clone(),
                state: p.state,
            })
            .collect(),
        since: since.clone(),
    }
}

/// An input's kind, as the state-machine contract names it.
fn kind(observed: &Observed) -> &'static str {
    match observed {
        Observed::RouteChosen { .. } => "route_chosen",
        Observed::RouteRecalculated { .. } => "route_recalculated",
        Observed::DestinationChanged { .. } => "destination_changed",
        Observed::NoRoute { .. } => "no_route",
        Observed::ProviderBound {} => "provider_bound",
        Observed::SegmentSettled { .. } => "segment_settled",
        Observed::SegmentFailed {} => "segment_failed",
        Observed::SegmentOutcomeUnknown {} => "segment_outcome_unknown",
        Observed::ReversalCompleted { .. } => "reversal_completed",
        Observed::WithdrawalCancelled {} => "withdrawal_cancelled",
        Observed::WaitingAtFloor {} => "waiting_at_floor",
        Observed::EndRequested {} => "end_requested",
        Observed::StuckDeclared { .. } => "stuck_declared",
        Observed::StuckResolved { .. } => "stuck_resolved",
        Observed::HandedOn { .. } => "handed_on",
        Observed::HandOnTakenBack { .. } => "hand_on_taken_back",
    }
}
