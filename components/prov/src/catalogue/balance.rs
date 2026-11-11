//! Balance checks and the eligibility of a reserve that doesn't answer (`REQ-CAT-3`, `REQ-CAT-4`, `REQ-CAT-19`).
//!
//! A balance check answers yes or no for a stated amount, and only inside the session of the account holder or
//! their delegate. Anything that reveals whether funds suffice counts as one, a dry-run withdrawal included. How
//! many count is bounded per account, and every check is logged. The caller passes `at`, as everywhere in the
//! catalogue.
//!
//! The allowance of ADR-0013 (higher rate for unmatched checks, refunds) is not built: `DEC-15` makes it the
//! design of record and gives it no task. The bound here is the plain count `REQ-CAT-3` asks for.

use std::collections::BTreeMap;

use rutent_contracts::provider_role::{
    BalanceOutcome, BalanceRefusal, BalanceVerificationResult, Category, NoDetail, Operation,
    Refusal, ReserveMovementOutcome, ResultBody,
};

use super::{Catalogue, Instant, Unusable};

/// How often an account's checks may count: at most `max_counted` inside any `window_secs` seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckBound {
    /// The most checks that may count inside one window.
    pub max_counted: u32,
    /// The window's length, in seconds.
    pub window_secs: u64,
}

/// What kind of check it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckKind {
    /// A reserve's balance verification.
    BalanceVerification,
    /// A withdrawal dry run, which reveals sufficiency as much as a verification does (`REQ-CAT-4`).
    DryRunWithdrawal,
}

/// Why a check wasn't sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckRefusal {
    /// The operation reveals nothing about sufficiency, so it isn't a check.
    NotACheck,
    /// No session of the account holder or their delegate stands behind it (`REQ-CAT-3`).
    NoSession,
    /// The session belongs to another account.
    SessionNotForAccount,
    /// The operator has set no bound on checks, so none is allowed (`REQ-CAT-3`: the bound MUST exist).
    NoBoundSet,
    /// The operator has set no time to answer, so none is allowed (`REQ-CAT-19`).
    NoAnswerTimeSet,
    /// The reserve can't be asked: no grant in force covers reserves (`REQ-CAT-9`, `REQ-CAT-2`).
    Unusable(Unusable),
    /// The account's counted checks have reached the bound.
    OverBound,
}

/// One logged check, sent or refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRecord {
    /// The account it concerned.
    pub account: String,
    /// The reserve asked.
    pub provider: String,
    /// The operation's transaction.
    pub transaction: String,
    /// Its kind.
    pub kind: CheckKind,
    /// When it was made.
    pub at: Instant,
    /// The holder's own order it was for, if any: such a check is logged and never counted.
    pub own_order: Option<String>,
    /// Why it wasn't sent, if it wasn't.
    pub refused: Option<CheckRefusal>,
    /// Whether it counts against the bound.
    pub counted: bool,
}

/// A check sent and not yet answered. The answer names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckTicket {
    id: u64,
    transaction: String,
    provider: String,
    kind: CheckKind,
}

impl CheckTicket {
    /// The reserve asked.
    pub fn provider(&self) -> &str {
        &self.provider
    }
}

/// What the asker is told: yes or no for the stated amount, or that it can't be told. Nothing else leaves the
/// catalogue, whatever the reserve sent (`INV-9`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The funds suffice.
    Yes,
    /// They don't.
    No,
    /// The reserve refused, by class, with no time (`INV-9`).
    Refused(BalanceRefusal),
    /// Not known: a mismatched or unusable answer. Value doesn't move on it.
    NotKnown,
}

/// Whether a reserve may be on routes now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eligibility {
    /// Nothing keeps it off routes on account of silence.
    Eligible,
    /// It didn't answer a check in time, and is out since then (`REQ-CAT-19`).
    TemporarilyUnavailable {
        /// The moment it was due.
        since: Instant,
    },
}

/// The alert raised when a reserve is marked (`NFR-8`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alert {
    /// The silent reserve.
    pub provider: String,
    /// When the unanswered check was sent.
    pub asked_at: Instant,
    /// When the mark took effect: the moment the answer was due.
    pub marked_at: Instant,
    /// The check it didn't answer.
    pub transaction: String,
}

#[derive(Debug, Clone)]
struct SessionEntry {
    account: String,
    expires_at: Instant,
}

#[derive(Debug, Clone)]
struct Pending {
    provider: String,
    transaction: String,
    asked_at: Instant,
    marked: bool,
}

/// The sessions known, the bound, the log, and what waits for an answer.
#[derive(Debug, Clone, Default)]
pub(super) struct BalanceChecks {
    sessions: BTreeMap<String, SessionEntry>,
    bound: Option<CheckBound>,
    answer_within: Option<u64>,
    log: Vec<CheckRecord>,
    pending: BTreeMap<u64, Pending>,
    next: u64,
    unavailable: BTreeMap<String, Instant>,
    alerts: Vec<Alert>,
}

impl BalanceChecks {
    /// Whether `provider` is marked silent at `at`.
    ///
    /// A check unanswered past its due time counts as silence without waiting for a sweep, so the mark never
    /// depends on someone running one (`REQ-CAT-19`).
    pub(super) fn unavailable_since(&self, provider: &str, at: Instant) -> Option<Instant> {
        let marked = self.unavailable.get(provider).copied().filter(|s| *s <= at);
        let due = self.due_pending(provider, at, None).min();
        marked.into_iter().chain(due).min()
    }

    /// The due times, at `at`, of the provider's pending checks, leaving out `except`.
    fn due_pending(
        &self,
        provider: &str,
        at: Instant,
        except: Option<u64>,
    ) -> impl Iterator<Item = Instant> + '_ {
        let within = self.answer_within;
        let provider = provider.to_owned();
        self.pending
            .iter()
            .filter(move |(id, p)| p.provider == provider && Some(**id) != except)
            .filter_map(move |(_, p)| within.map(|w| Instant(p.asked_at.0.saturating_add(w))))
            .filter(move |due| *due <= at)
    }

    fn counted(&self, account: &str, at: Instant) -> u32 {
        let window = self.bound.map_or(u64::MAX, |b| b.window_secs);
        self.log
            .iter()
            .filter(|r| r.account == account && r.counted && r.at <= at)
            .filter(|r| r.at.0.saturating_add(window) > at.0)
            .count() as u32
    }

    /// Marks the pending check if it is due, once. Returns the alert raised.
    fn mark_if_due(&mut self, id: u64, at: Instant) -> Option<Alert> {
        let within = self.answer_within?;
        let pending = self.pending.get_mut(&id)?;
        let due = Instant(pending.asked_at.0.saturating_add(within));
        if pending.marked || at < due {
            return None;
        }
        pending.marked = true;
        let alert = Alert {
            provider: pending.provider.clone(),
            asked_at: pending.asked_at,
            marked_at: due,
            transaction: pending.transaction.clone(),
        };
        let since = self
            .unavailable
            .entry(pending.provider.clone())
            .or_insert(due);
        *since = (*since).min(due);
        self.alerts.push(alert.clone());
        Some(alert)
    }
}

/// Whether an operation reveals whether funds suffice, and so counts as a balance check (`REQ-CAT-4`): a balance
/// verification, or a withdrawal dry run. A real withdrawal is the order itself.
pub fn reveals_sufficiency(operation: &Operation) -> bool {
    match operation {
        Operation::BalanceVerification(_) => true,
        Operation::Withdrawal(w) => w.dry_run,
        _ => false,
    }
}

impl Catalogue {
    /// Sets how often an account's checks may count (`REQ-CAT-3`). With none set, no check is allowed.
    pub fn set_check_bound(&mut self, bound: CheckBound) {
        self.checks.bound = Some(bound);
    }

    /// Sets how long a reserve has to answer a check (`REQ-CAT-19`, `NFR-8`: the time is the operator's). With
    /// none set, no check is allowed.
    pub fn set_answer_within(&mut self, seconds: u64) {
        self.checks.answer_within = Some(seconds);
    }

    /// Records a session of an account holder or their delegate, valid until `expires_at`. Core issues it; the
    /// catalogue only holds which account it speaks for.
    pub fn open_session(&mut self, id: &str, account: &str, expires_at: Instant) {
        self.checks.sessions.insert(
            id.to_owned(),
            SessionEntry {
                account: account.to_owned(),
                expires_at,
            },
        );
    }

    /// Sends a check: it must be one (`REQ-CAT-4`), inside a session of the account it concerns (`REQ-CAT-3`),
    /// to a reserve a grant in force covers, within the account's bound. It is logged either way and counted
    /// unless it is for the holder's own order (`own_order` names it). A reserve marked silent can still be
    /// asked: that is how it gets back.
    pub fn begin_check(
        &mut self,
        operation: &Operation,
        own_order: Option<&str>,
        at: Instant,
    ) -> Result<CheckTicket, CheckRefusal> {
        let (provider, transaction, kind, session_id, named_account) = match operation {
            Operation::BalanceVerification(b) => (
                &b.provider,
                &b.transaction,
                CheckKind::BalanceVerification,
                Some(&b.session),
                Some(&b.account),
            ),
            Operation::Withdrawal(w) if w.dry_run => (
                &w.provider,
                &w.transaction,
                CheckKind::DryRunWithdrawal,
                w.session.as_ref(),
                None,
            ),
            _ => return Err(CheckRefusal::NotACheck),
        };
        let session = session_id
            .and_then(|id| self.checks.sessions.get(id))
            .filter(|s| s.expires_at > at);
        // The account the check is charged to: the one it names, or else the session's own.
        let account = named_account
            .cloned()
            .or_else(|| session.map(|s| s.account.clone()));
        let refusal = self.refusal_for(provider, session, named_account, own_order, at);
        let counted = refusal.is_none() && own_order.is_none();
        if let Some(account) = account {
            self.checks.log.push(CheckRecord {
                account,
                provider: provider.clone(),
                transaction: transaction.clone(),
                kind,
                at,
                own_order: own_order.map(str::to_owned),
                refused: refusal.clone(),
                counted,
            });
        }
        if let Some(refusal) = refusal {
            return Err(refusal);
        }
        let id = self.checks.next;
        self.checks.next += 1;
        self.checks.pending.insert(
            id,
            Pending {
                provider: provider.clone(),
                transaction: transaction.clone(),
                asked_at: at,
                marked: false,
            },
        );
        Ok(CheckTicket {
            id,
            transaction: transaction.clone(),
            provider: provider.clone(),
            kind,
        })
    }

    fn refusal_for(
        &self,
        provider: &str,
        session: Option<&SessionEntry>,
        named_account: Option<&String>,
        own_order: Option<&str>,
        at: Instant,
    ) -> Option<CheckRefusal> {
        let Some(session) = session else {
            return Some(CheckRefusal::NoSession);
        };
        if named_account.is_some_and(|a| *a != session.account) {
            return Some(CheckRefusal::SessionNotForAccount);
        }
        let Some(bound) = self.checks.bound else {
            return Some(CheckRefusal::NoBoundSet);
        };
        if self.checks.answer_within.is_none() {
            return Some(CheckRefusal::NoAnswerTimeSet);
        }
        if let Err(unusable) =
            self.check_standing(provider, at, |g| g.covers_category(Category::Reserve))
        {
            return Some(CheckRefusal::Unusable(unusable));
        }
        if own_order.is_none() && self.checks.counted(&session.account, at) >= bound.max_counted {
            return Some(CheckRefusal::OverBound);
        }
        None
    }

    /// Takes a reserve's result for a check, and tells the asker yes or no, a class of refusal, or that it isn't
    /// known (`INV-9`). A result for another transaction or kind counts for nothing and leaves the check pending.
    /// An answer in time makes a reserve marked silent eligible again; a late one is delivered and doesn't.
    pub fn answer_check(
        &mut self,
        ticket: &CheckTicket,
        result: &ResultBody,
        at: Instant,
    ) -> Verdict {
        if !self.checks.pending.contains_key(&ticket.id) {
            return Verdict::NotKnown;
        }
        let Some(verdict) = verdict_of(ticket, result) else {
            return Verdict::NotKnown;
        };
        self.checks.mark_if_due(ticket.id, at);
        let late = self
            .checks
            .pending
            .get(&ticket.id)
            .is_some_and(|p| p.marked);
        self.checks.pending.remove(&ticket.id);
        // Another check of the same reserve still unanswered past its time keeps the mark.
        if !late
            && self
                .checks
                .due_pending(&ticket.provider, at, None)
                .next()
                .is_none()
        {
            self.checks.unavailable.remove(&ticket.provider);
        }
        verdict
    }

    /// Marks every reserve whose check went unanswered past the operator's time, and returns the alerts raised
    /// now (`REQ-CAT-19`, `NFR-8`). A reserve is out from the moment its answer was due, not from when this ran.
    pub fn sweep_silent(&mut self, at: Instant) -> Vec<Alert> {
        let ids: Vec<u64> = self.checks.pending.keys().copied().collect();
        ids.into_iter()
            .filter_map(|id| self.checks.mark_if_due(id, at))
            .collect()
    }

    /// Whether a reserve may be on routes at `at`, as far as silence goes.
    pub fn reserve_eligibility(&self, provider: &str, at: Instant) -> Eligibility {
        match self.checks.unavailable_since(provider, at) {
            Some(since) => Eligibility::TemporarilyUnavailable { since },
            None => Eligibility::Eligible,
        }
    }

    /// Every alert raised so far.
    pub fn alerts(&self) -> &[Alert] {
        &self.checks.alerts
    }

    /// Every check of an account, sent or refused, oldest first.
    pub fn check_log(&self, account: &str) -> Vec<&CheckRecord> {
        self.checks
            .log
            .iter()
            .filter(|r| r.account == account)
            .collect()
    }

    /// How many checks of an account count against its bound at `at`.
    pub fn counted_checks(&self, account: &str, at: Instant) -> u32 {
        self.checks.counted(account, at)
    }

    /// How many checks wait for an answer.
    pub fn pending_checks(&self) -> usize {
        self.checks.pending.len()
    }
}

/// What the asker may be told of a result, or `None` when the result isn't the answer to this check.
fn verdict_of(ticket: &CheckTicket, result: &ResultBody) -> Option<Verdict> {
    match (ticket.kind, result) {
        (
            CheckKind::BalanceVerification,
            ResultBody::BalanceVerification(BalanceVerificationResult {
                transaction,
                outcome,
            }),
        ) if *transaction == ticket.transaction => Some(match outcome {
            BalanceOutcome::Answered(a) if a.sufficient => Verdict::Yes,
            BalanceOutcome::Answered(_) => Verdict::No,
            BalanceOutcome::Refused(r) => Verdict::Refused(r.refusal.clone()),
        }),
        (CheckKind::DryRunWithdrawal, ResultBody::Withdrawal(r))
            if r.transaction == ticket.transaction =>
        {
            Some(match &r.outcome {
                ReserveMovementOutcome::DryRunAccepted(_) => Verdict::Yes,
                // A refusal states the time it lifts at; that would reveal more than yes or no (`INV-9`).
                ReserveMovementOutcome::Refused(r) => match &r.refusal {
                    Refusal::WaitsOnPerson(_) => {
                        Verdict::Refused(BalanceRefusal::WaitsOnPerson(NoDetail {}))
                    }
                    Refusal::Final(_) => Verdict::Refused(BalanceRefusal::Final(NoDetail {})),
                    Refusal::LiftsAtKnownTime(_) => Verdict::NotKnown,
                },
                _ => Verdict::NotKnown,
            })
        }
        _ => None,
    }
}
