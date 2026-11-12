//! Notices: every change of an order's state notified to its originating user and the operator, and delivery to the
//! operator watched (`T-023-01`, `T-023-02`). `F-023` owns this module.
//!
//! Declared by `T-020-01`; only `F-023`'s rows edit it (`PLAN.md` §4.4).
//!
//! **From the log, never from the caller** (`REQ-ORD-20`, `AC-41`). [`Notices::notify`] reads the log's entries as
//! the log holds them, in its order, from where it last stopped, as `Stuck::follow` and policy's charges read them.
//! Every input the state machine applies to an order (`Stm::open`, `Stm::transition`), a failure included (a segment
//! failed, a withdrawal cancelled, a part stuck, an order failed), is one change; so is something recorded after an
//! order ended (`afterwards`, `REQ-ORD-31`, `REQ-ORD-25`) and a refusal naming an order Core follows. An input the
//! machine doesn't apply (a repeat, a late report, one not applicable) and an input the log recorded as not taken
//! change nothing and are notified to no one. Each change is the order's next sequence, counted from 1 in the log's
//! order, so the same log gives the same notices, byte for byte, after a restart: what is signed holds the
//! change's recorded time, never the clock's.
//!
//! **Its receivers come from Core's records.** The user is the requester the order's `acceptance` names, receiving
//! as `customer`; the operator is the deployment's operator party, set when the notices are built, receiving as
//! `operator`. An order the log holds no acceptance for is notified to the operator alone. A notice carries what
//! both may read of an order without a permission (`Reads::OrderState`, `REQ-ORD-5`): its state, each part's state,
//! when it reached them, and what changed, by kind; never a provider, segment, amount, account or a provider's text
//! (`REQ-AUZ-7`, `REQ-ORD-18`). A raise and a charge counted far ahead are the operator's alone.
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
//! read (`Reads::OrderState`) and records it either way, and only an allowed read is answered: the order's latest
//! notice, sealed for the reader. The check comes first, so a reader policy refuses learns nothing, not even whether
//! the order exists.
//!
//! **The operator's channel** ([`Channel`]). `Operator::raise` (the checks' differences, a change in a provider's
//! p99 from `Stuck::learn`) is sealed for the operator alone and sent; it answers delivered only when the transport
//! took it. [`Notices::ahead`] does the same for each charge policy set apart as counted more than a window ahead of
//! Core's clock (`Charged::ahead()`, `T-021-05`), each once by its party and request.
//!
//! **Delivery.** Each notice goes through the [`Transport`] the assembly gives. One the transport didn't take is kept
//! ([`Notices::undelivered`]); retrying it within a bound and recording it are `T-023-02`'s.

mod inbox;
pub mod wire;

use std::collections::{BTreeMap, BTreeSet};

use rutent_contracts::core_internal::calls::{AppendFailure, Delivery, Operator, Reads};
use rutent_contracts::core_internal::{
    Actor, Charge, Entry, Raise, ReadAnswer, ReadCheck, ReadOutcome, Role, What,
};
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_contracts::state_machine::{self as stm, Observed, RecordedInput, StateMachine};
use rutent_stm::Stm;

use crate::rounds::Fresh;
use crate::snapshot::Clock;

pub use inbox::{Inbox, NotTaken, Taken, Verifier};
pub use wire::{
    About, AheadSeen, Change, DOMAIN, Notice, NoticeKey, PartState, Receiver, View, signing_input,
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

/// How a sealed notice reaches its receiver. The assembly gives it; `T-023-02` watches it.
pub trait Transport {
    /// Hands `notice`, sealed, to `to`: true only where it was taken for delivery.
    fn send(&mut self, to: &Receiver, notice: &[u8]) -> bool;
}

/// A notice the transport didn't take, kept for `T-023-02`'s retry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Undelivered {
    /// Its receiver.
    pub to: Receiver,
    /// The sealed notice.
    pub notice: Vec<u8>,
}

/// Why the log's entries weren't all notified. What was notified before the error stays notified, and the next call
/// carries on from the entry that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeError {
    /// The entries don't extend those read before: a shorter or another log.
    NotThisLog,
    /// The notice key couldn't sign now: nothing of the entry was sent.
    Unsigned,
}

/// Why the current state wasn't answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CurrentError {
    /// Policy refused the read, and recorded it.
    Refused(ReadAnswer),
    /// Policy couldn't record the check, so it answered nothing.
    Append(AppendFailure),
    /// The read was allowed, but the notices follow no such order.
    Unknown,
    /// The notice key couldn't sign now.
    Unsigned,
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

/// Core's notices: the entries read, the orders followed from them, and what wasn't delivered. A later read must
/// extend the entries read, so another log, or a shorter one, is never read as this one.
#[derive(Debug, Clone)]
pub struct Notices {
    deployment: Id,
    operator: Receiver,
    read: Vec<Entry>,
    requesters: BTreeMap<String, Id>,
    orders: BTreeMap<String, Followed>,
    ahead: BTreeSet<(String, String)>,
    undelivered: Vec<Undelivered>,
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
            read: Vec::new(),
            requesters: BTreeMap::new(),
            orders: BTreeMap::new(),
            ahead: BTreeSet::new(),
            undelivered: Vec::new(),
        }
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

    /// Notifies every change the log's `recorded` entries hold past those read before, to each of its order's
    /// receivers, and answers how many notices were sent. `recorded` is the log as it holds it, from its first entry.
    pub fn notify(
        &mut self,
        recorded: &[Entry],
        key: &mut dyn NoticeKey,
        transport: &mut dyn Transport,
    ) -> Result<usize, NoticeError> {
        if !recorded.starts_with(&self.read) {
            return Err(NoticeError::NotThisLog);
        }
        let mut sent = 0;
        while let Some(entry) = recorded.get(self.read.len()) {
            if let Some((order, followed)) = self.change(entry) {
                let mut sealed = Vec::new();
                for to in self.receivers_of(&order) {
                    let notice = Notice {
                        deployment: self.deployment.clone(),
                        to: to.clone(),
                        about: followed.latest.clone(),
                    };
                    sealed.push((to, wire::seal(key, notice).ok_or(NoticeError::Unsigned)?));
                }
                self.orders.insert(order, followed);
                for (to, notice) in sealed {
                    sent += 1;
                    self.send(transport, to, notice);
                }
            }
            if let Entry::Verified(v) = entry
                && let What::Acceptance { order, parties, .. } = &v.what
            {
                self.requesters
                    .entry(order.as_str().to_owned())
                    .or_insert_with(|| parties.requester.clone());
            }
            self.read.push(entry.clone());
        }
        Ok(sent)
    }

    /// The order `entry` changes, by its id, and how the notices follow it after, without changing anything held:
    /// `None` where it changes no order the notices follow. An input the machine applies is always a change: nothing
    /// in it is converted, so no applied input can be dropped and leave the order followed behind the log.
    fn change(&self, entry: &Entry) -> Option<(String, Followed)> {
        let Entry::Verified(v) = entry else {
            return None;
        };
        match &v.what {
            What::Input { input } => self.applied(input),
            What::Afterwards { order, event, .. } => {
                self.noted(order, &v.when.at, Change::Afterwards { event: *event })
            }
            What::OrderRefusal {
                order: Some(order),
                class,
                ..
            } => self.noted(order, &v.when.at, Change::Refused { class: *class }),
            _ => None,
        }
    }

    /// An input the state machine applies, and the order after it.
    fn applied(&self, input: &RecordedInput) -> Option<(String, Followed)> {
        let key = input.subject.order.as_str().to_owned();
        let (next, sequence) = match (self.orders.get(&key), &input.observed) {
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
        let reason = match &input.observed {
            Observed::StuckDeclared { reason } => Some(*reason),
            _ => None,
        };
        let change = Change::Input {
            input: kind(&input.observed).to_owned(),
            part: input.subject.part.clone(),
            reason,
        };
        let at = input.recorded_at.clone();
        Some((
            key,
            Followed {
                latest: About::Order {
                    order: next.order_id.clone(),
                    sequence,
                    at: at.clone(),
                    change,
                    view: view(&next, &at),
                },
                order: next,
                since: at,
                sequence,
            },
        ))
    }

    /// A change recorded about an order the notices follow that leaves its state as it was.
    fn noted(&self, order: &Id, at: &Timestamp, change: Change) -> Option<(String, Followed)> {
        let f = self.orders.get(order.as_str())?;
        // A log entry's time (`Z`, at most nine digits of a second) is always a state-machine timestamp, whose
        // grammar is the wider; and a change that leaves the state as it was leaves the order followed as it was.
        let at = stm::Timestamp::new(at.as_str())?;
        let sequence = f.sequence + 1;
        Some((
            order.as_str().to_owned(),
            Followed {
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
            },
        ))
    }

    fn send(&mut self, transport: &mut dyn Transport, to: Receiver, notice: Vec<u8>) -> bool {
        let taken = transport.send(&to, &notice);
        if !taken {
            self.undelivered.push(Undelivered { to, notice });
        }
        taken
    }

    /// The current state of `order`, for `reader`, once policy allows the read (`Reads::OrderState`), which it
    /// records either way: the order's latest notice, sealed for the reader in the role it reads in. `recorded`
    /// is read first, as [`Notices::notify`] reads it, so the answer is the log's latest; a reader that missed a
    /// notice takes this one in its place.
    #[allow(clippy::too_many_arguments)]
    pub fn current(
        &mut self,
        recorded: &[Entry],
        reads: &mut dyn Reads,
        clock: &dyn Clock,
        fresh: &mut dyn Fresh,
        key: &mut dyn NoticeKey,
        transport: &mut dyn Transport,
        reader: &Actor,
        permission: Option<&Id>,
        order: &Id,
    ) -> Result<Vec<u8>, CurrentError> {
        let at = clock.now().timestamp().ok_or(CurrentError::Unsigned)?;
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
            return Err(CurrentError::Refused(answer));
        }
        // What the log holds is notified first; a log that doesn't extend what was read answers what was read.
        match self.notify(recorded, key, transport) {
            Ok(_) | Err(NoticeError::NotThisLog) => {}
            Err(NoticeError::Unsigned) => return Err(CurrentError::Unsigned),
        }
        let followed = self
            .orders
            .get(order.as_str())
            .ok_or(CurrentError::Unknown)?;
        wire::seal(
            key,
            Notice {
                deployment: self.deployment.clone(),
                to: Receiver {
                    party: reader.party.clone(),
                    role: reader.role,
                },
                about: followed.latest.clone(),
            },
        )
        .ok_or(CurrentError::Unsigned)
    }

    /// Notifies the operator, alone, of each charge in `charges` not notified before, by its party and request, and
    /// answers how many were sent: the party, the request id, the kind of work and its count, the moment it was
    /// counted at and Core's now, so the operator sees a clock that jumped and what it cost the party.
    pub fn ahead(
        &mut self,
        charges: &[AheadCharge],
        clock: &dyn Clock,
        key: &mut dyn NoticeKey,
        transport: &mut dyn Transport,
    ) -> Result<usize, NoticeError> {
        let now = clock.now().timestamp().ok_or(NoticeError::Unsigned)?;
        let mut sent = 0;
        for ahead in charges {
            let seen = (
                ahead.charge.party.as_str().to_owned(),
                ahead.charge.request.as_str().to_owned(),
            );
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
            };
            let sealed = wire::seal(key, notice).ok_or(NoticeError::Unsigned)?;
            self.ahead.insert(seen);
            let to = self.operator.clone();
            self.send(transport, to, sealed);
            sent += 1;
        }
        Ok(sent)
    }
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
        let notice = Notice {
            deployment: self.notices.deployment.clone(),
            to: to.clone(),
            about: About::Raise {
                raise: raise.clone(),
            },
        };
        let Some(sealed) = wire::seal(self.key, notice) else {
            return Delivery::Undelivered;
        };
        if self.notices.send(self.transport, to, sealed) {
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
    }
}
