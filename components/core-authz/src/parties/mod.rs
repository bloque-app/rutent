//! Agents and accountable parties: the limits on work and on asking about people bind at the party.
//!
//! Declared by `T-020-01`; `T-021-05` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! # Every agent acts for an accountable party
//!
//! External authentication says who acts (`Actor::acts`) and nothing more (`REQ-AUZ-1`). `T-021-03`'s fixed rule,
//! the first the engine asks, holds every actor a request names to the party the deployment's register of actors
//! says it acts for (`Source::ActsFor`), so a party is bound to its actor before any other rule reads it. This
//! row's fixed rule, asked second, holds an agent (`ActorKind::Agent`) to two things more (`REQ-AUZ-8`):
//!
//! - **an accountable party other than itself.** An agent that states itself its own party answers to no one, and is
//!   refused as acting for no accountable party, whatever it asks and whatever the register says;
//! - **the scope that party set.** Each call an agent makes, every policy call and each decided action, is a
//!   [`Scope`], asked of the party's register of its agents (`Source::AgentScope`, the input the agent's `acts`). A
//!   register that says no refuses the call as not permitted, one that says nothing gives a missing input and so a
//!   refusal, one out of reach not known (`REQ-AUZ-5`). An authorisation is a scope of its own, so an agent gives
//!   one, the customer's new destination after a failed final deposit included, only where its party let it.
//!
//! The kind of actor is the register's, not the request's: `T-021-03`'s rule asks the register of actors for the
//! login, its party and its kind together, so an agent's login stating itself a person or a component acts for no
//! one, and escapes neither rule. A person or a component is its party's own, and is asked no scope. A route's
//! suggester is asked no scope either: it doesn't act in the call, the operator who posts the route does. Who a party's agents are, and what each may
//! do, is the deployment's register to keep and record (`F-030`); the engine only asks.
//!
//! # Limits bind at the party
//!
//! Orchestration charges each piece of work to the accountable party its caller acts for before the work is done
//! ([`Charges::charge`]; the charge's own check holds its `party` to its caller's). The engine decides the charge
//! through its one consultation, so a caller acting for no accountable party, or an agent outside its scope, has no
//! allowance, and then holds it to the party's bound in the window now. There are two limits, each per party:
//!
//! - **work** ([`Limit::Work`]): the caller's own requests and the requests an order fans out to providers on its
//!   behalf, which count against the one bound together (`REQ-OPS-1`);
//! - **asking about people** ([`Limit::People`]): questions about people (`REQ-AUZ-8`, `REQ-CPL-2`).
//!
//! The counter is keyed by the party, never by the actor, so a thousand agents of one party share its one bound
//! (`AC-18`). A charge that fits is counted only once its entry is in the log; one over the bound counts nothing and
//! its work is not done. A charge's id names that charge only: the same charge again counts once, and anything else
//! under an id already counted is refused.
//!
//! The window is a fixed one, [`WINDOW_SECS`] long, read from Core's own clock (`session::Clock`, the one sessions
//! are bounded by), never from the time a charge states. It only moves forward: Core's clock moving back keeps the
//! count going in the latest window; a charge counted more than a window ahead of Core's now never chooses the window
//! but still counts in it, so neither a step back nor a jump forward gives a fresh bound. A fixed window lets a party spend its bound at the end of one window and again
//! at the start of the next, twice the bound in a moment; that burst is accepted, and a sliding window is the
//! deployment's to ask for. Its bounds are the deployment's ([`Bounds`], [`Engine::set_bounds`]), [`Uniform`] until it
//! hands others. The counter is the log's: before every charge the engine reads the
//! log's permitted `charge` entries through policy's handle ([`Recorded::charged`], [`charged_in`]) and rebuilds it
//! from them ([`Charged::rebuild`]), so an engine opened again after a restart, or beside another, holds what the log
//! holds. Each charge's entry is recorded at the moment Core's clock counted it ([`Charged::counted_at`]), never at the
//! time the charge states, so the rebuild finds it in the window it was counted in. A log that can't be read leaves
//! the counter an input the decision doesn't have, and the charge is refused.
//!
//! # Who answers for an actor
//!
//! [`accountable`] and [`same_accountable_actor`] are the one definition of an actor's accountable party and of two
//! actors being one there, for the rows that compare actors: `T-021-06`, which holds a stuck order's decider and mover
//! apart, and `T-021-07`, which bounds questions about people at the asker's party. Two actors are one where they act
//! for one party or are one login, since a login may act for more than one party. Each reads a party the consultation
//! has bound, so it is read only of a call the fixed rules let through.

use std::collections::HashMap;
use std::time::Duration;

use rutent_contracts::core_internal::calls::{AppendFailure, Charges, PolicyLog};
use rutent_contracts::core_internal::{
    Action, Actor, ActorKind, Answer, CatalogueChange, Charge, ChargeAnswer, ChargeOutcome, Entry,
    Outcome, RefusalClass, VerifiedEntry, What, Work,
};
use rutent_contracts::provider_role::SafeInteger;
use rutent_contracts::router_api::Id;

use crate::engine::{
    Asked, Engine, Input, Need, Policy, Reply, Ruling, Said, Source, Sources, charge_outcome,
};
use crate::funds::Recorded;

/// How long one window of the bounds lasts, in seconds, by Core's clock: each party's count starts again at each
/// window's start.
pub const WINDOW_SECS: u64 = 3_600;

/// The bound on a party's work in one window, until the deployment hands its own.
pub const WORK_BOUND: u64 = 10_000;

/// The bound on a party's questions about people in one window, until the deployment hands its own.
pub const PEOPLE_BOUND: u64 = 1_000;

/// The call an agent makes, as the scope its party set names it: one per policy call, and one per decided action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Moving value on an order (`decide`, `move_value`).
    MoveValue,
    /// Posting a route (`decide`, `post_route`).
    PostRoute,
    /// One kind of catalogue change (`decide`, `change_catalogue`).
    ChangeCatalogue(CatalogueChange),
    /// Erasing a kept statement (`decide`, `erase_statement`).
    EraseStatement,
    /// Giving an authorisation to move funds, a new destination included (`authorise`).
    Authorise,
    /// A stuck order's decide step (`decide_stuck`).
    DecideStuck,
    /// A stuck order's move step (`move_stuck`, and `resolve_stuck`, which travels only as one).
    MoveStuck,
    /// Opening a session (`open_session`).
    OpenSession,
    /// Reading (`check_read`).
    CheckRead,
    /// Granting a read permission (`grant_permission`).
    GrantPermission,
    /// Revoking a read permission (`revoke_permission`).
    RevokePermission,
    /// Causing work of one kind (`charge`).
    Charge(Work),
    /// Asking about a person (`ask_about_person`).
    AskAboutPerson,
}

impl Scope {
    /// The call `asked` is, as a scope names it.
    pub fn of(asked: Asked<'_>) -> Scope {
        match asked {
            Asked::Decide(request) => match &request.action {
                Action::MoveValue { .. } => Scope::MoveValue,
                Action::PostRoute { .. } => Scope::PostRoute,
                Action::ChangeCatalogue { change, .. } => Scope::ChangeCatalogue(*change),
                Action::EraseStatement { .. } => Scope::EraseStatement,
                Action::ResolveStuck { .. } => Scope::MoveStuck,
            },
            Asked::Authorise(_) => Scope::Authorise,
            Asked::DecideStuck(_) => Scope::DecideStuck,
            Asked::MoveStuck(_) => Scope::MoveStuck,
            Asked::OpenSession(_) => Scope::OpenSession,
            Asked::CheckRead(_) => Scope::CheckRead,
            Asked::GrantPermission(_) => Scope::GrantPermission,
            Asked::RevokePermission(_) => Scope::RevokePermission,
            Asked::Charge(c) => Scope::Charge(c.work),
            Asked::AskAboutPerson(_) => Scope::AskAboutPerson,
        }
    }
}

/// The accountable party `actor` acts for: the one its limits bind at and actors are compared at (`REQ-AUZ-8`,
/// ADR-0011). It is bound to the actor only once the consultation let the call through (`T-021-03`'s fixed rule), so
/// read it only of such a call.
pub fn accountable(actor: &Actor) -> &Id {
    &actor.party
}

/// Whether `a` and `b` count as one accountable actor where actors are compared (`REQ-AUZ-4`, `REQ-AUZ-8`): they act
/// for one accountable party, as two agents of one party or its person and its agent do, or they are one login,
/// which may act for more than one party (an operator's login deciding for one desk and moving for another). Two
/// actors are distinct only where both their parties and their logins differ.
pub fn same_accountable_actor(a: &Actor, b: &Actor) -> bool {
    accountable(a) == accountable(b) || a.acts == b.acts
}

/// What the fixed rule needs for `asked`: the party's word that its agent may make the call, for an agent with an
/// accountable party other than itself; nothing for anyone else.
pub(crate) fn needs(asked: Asked<'_>) -> Reply<Vec<Need>> {
    Reply::Given(scope_need(asked).into_iter().collect())
}

/// The scope the caller of `asked` is held to, where it is an agent acting for a party other than itself.
fn scope_need(asked: Asked<'_>) -> Option<Need> {
    let caller = asked.caller();
    (caller.kind == ActorKind::Agent && caller.party != caller.acts).then(|| Need {
        input: caller.acts.clone(),
        from: Source::AgentScope {
            party: caller.party.clone(),
            scope: Scope::of(asked),
        },
    })
}

/// The fixed rule's ruling: an agent that is its own party acts for no accountable party; an agent acts only within
/// the scope its party set, a no refusing and nothing missing; anyone else passes.
pub(crate) fn rule(asked: Asked<'_>, inputs: &[Input]) -> Reply<Ruling> {
    let caller = asked.caller();
    if caller.kind != ActorKind::Agent {
        return Reply::Given(Ruling::Permit);
    }
    let Some(need) = scope_need(asked) else {
        return Reply::Given(Ruling::Refuse(RefusalClass::NoAccountableParty));
    };
    match inputs.iter().find(|i| i.need == need).map(|i| i.said) {
        Some(Said::Yes) => Reply::Given(Ruling::Permit),
        Some(Said::No) => Reply::Given(Ruling::Refuse(RefusalClass::NotPermitted)),
        None => Reply::Missing,
    }
}

/// One of the two limits a party's work is held to (`REQ-AUZ-8`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Limit {
    /// Work: the caller's own requests and those an order fans out to providers, together (`REQ-OPS-1`).
    Work,
    /// Asking about people (`REQ-CPL-2`).
    People,
}

impl Limit {
    /// The limit `work` counts against.
    pub fn of(work: Work) -> Limit {
        match work {
            Work::Request | Work::FanOut => Limit::Work,
            Work::PersonQuestion => Limit::People,
        }
    }
}

/// The deployment's bounds: how much of each limit a party has in one window.
pub trait Bounds: std::fmt::Debug {
    /// The bound on `limit` for `party` in one window.
    fn bound(&self, party: &Id, limit: Limit) -> SafeInteger;
}

/// One bound per limit, the same for every party: [`WORK_BOUND`] and [`PEOPLE_BOUND`] by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Uniform {
    /// The bound on work.
    pub work: SafeInteger,
    /// The bound on questions about people.
    pub people: SafeInteger,
}

impl Default for Uniform {
    fn default() -> Self {
        let safe = |n| SafeInteger::try_from(n).expect("a safe integer");
        Uniform {
            work: safe(WORK_BOUND),
            people: safe(PEOPLE_BOUND),
        }
    }
}

impl Bounds for Uniform {
    fn bound(&self, _: &Id, limit: Limit) -> SafeInteger {
        match limit {
            Limit::Work => self.work,
            Limit::People => self.people,
        }
    }
}

/// Where a charge stands against its party's bound: it fits, with what would be left, or it is over the bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// It fits, and this is what is left after it.
    Fits {
        /// What is left.
        remaining: SafeInteger,
    },
    /// It doesn't.
    Over {
        /// The bound.
        bound: SafeInteger,
    },
}

/// The work charged to each party in the window now, and the bounds it is held to.
#[derive(Debug)]
pub struct Charged {
    bounds: Box<dyn Bounds>,
    /// The latest window counted in: its start, in whole windows since the epoch, by Core's clock. It only moves
    /// forward.
    window: u64,
    used: HashMap<(Id, Limit), u64>,
    /// The charges counted in the window now, by their party and their id.
    counted: HashMap<(Id, Id), Charge>,
    /// The log's read the counter was last rebuilt from, whole: the next read must extend it.
    last_read: Vec<Counted>,
    /// The engine's own last charge counted within its bound, as its entry holds it: the next read must hold it.
    appended: Option<Counted>,
    /// The engine's own charges counted in the window now: no rebuild counts the window below them.
    own: Vec<Counted>,
    /// The charges the last read held counted more than a window ahead of Core's now, set apart for the operator.
    ahead: Vec<Counted>,
}

impl Default for Charged {
    fn default() -> Self {
        Charged {
            bounds: Box::new(Uniform::default()),
            window: 0,
            used: HashMap::new(),
            counted: HashMap::new(),
            last_read: Vec::new(),
            appended: None,
            own: Vec::new(),
            ahead: Vec::new(),
        }
    }
}

impl Charged {
    /// The charges the log's last read held counted more than a window ahead of Core's now, as after the clock
    /// jumped forward and came back, or from another engine whose clock runs ahead: none of them chose the window,
    /// and each still counts in it. The operator's to look at (`T-023-01` surfaces them), since core-internal has no
    /// raise for them.
    pub fn ahead(&self) -> &[Counted] {
        &self.ahead
    }

    /// Whether `read`, the log's read now, can stand for the log: it extends the read before it, so it drops and
    /// reorders nothing that read held, and it holds the engine's own last charge counted within its bound. Like
    /// `T-021-04`'s read, that catches a stale or truncated read, and another log's once the engine has counted; a
    /// first read of another log, or one frozen after the engine's last charge, is the assembly's word.
    pub(crate) fn stands(&self, read: &[Counted]) -> bool {
        read.starts_with(&self.last_read)
            && self.appended.as_ref().is_none_or(|own| read.contains(own))
    }

    /// Notes the engine's own charge `own`, counted within its bound, as its entry holds it.
    pub(crate) fn counted(&mut self, own: Counted) {
        self.take(&own.charge);
        self.own.push(own.clone());
        self.appended = Some(own);
    }

    /// What `party` has used of `limit` in the window now.
    pub fn used(&self, party: &Id, limit: Limit) -> u64 {
        self.used.get(&(party.clone(), limit)).copied().unwrap_or(0)
    }

    /// The moment a charge counted at Core's `now` is counted at: `now`, or the start of the latest window where the
    /// clock has moved back before it, so a rebuild puts it in the window it was counted in.
    pub fn counted_at(&self, now: Duration) -> Duration {
        now.max(Duration::from_secs(self.window.saturating_mul(WINDOW_SECS)))
    }

    /// The key a charge is counted under: its party's, and its id within that party.
    fn key(charge: &Charge) -> (Id, Id) {
        (charge.party.clone(), charge.request.clone())
    }

    /// Whether `charge` names, under an id its party had counted in the window now, another charge.
    pub(crate) fn reissues(&self, charge: &Charge) -> bool {
        self.counted
            .get(&Self::key(charge))
            .is_some_and(|counted| counted != charge)
    }

    /// Where `charge` stands against its party's bound on the limit its work counts against. The same charge counted
    /// already fits, with what is left, and is not counted again.
    pub(crate) fn standing(&self, charge: &Charge) -> Standing {
        let limit = Limit::of(charge.work);
        let bound = self.bounds.bound(&charge.party, limit);
        let used = self.used(&charge.party, limit);
        let safe = |n: u64| SafeInteger::try_from(n).expect("no more than the bound");
        if self.counted.contains_key(&Self::key(charge)) {
            return Standing::Fits {
                remaining: safe(bound.get().saturating_sub(used)),
            };
        }
        match used.checked_add(charge.count.get()) {
            Some(after) if after <= bound.get() => Standing::Fits {
                remaining: safe(bound.get() - after),
            },
            _ => Standing::Over { bound },
        }
    }

    /// Counts `charge` against its party, once.
    pub(crate) fn take(&mut self, charge: &Charge) {
        if self.counted.contains_key(&Self::key(charge)) {
            return;
        }
        let key = (charge.party.clone(), Limit::of(charge.work));
        *self.used.entry(key).or_insert(0) += charge.count.get();
        self.counted.insert(Self::key(charge), charge.clone());
    }
}

/// A charge counted within its bound, with the moment it was counted at by Core's clock ([`Charged::counted_at`]), to
/// the second: what each of the log's permitted `charge` entries holds, at its entry's own time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Counted {
    /// The charge.
    pub charge: Charge,
    /// When it was counted, by Core's clock.
    pub at: Duration,
}

/// The charges `entries` hold as permitted, in their order, each with the moment the engine counted it: each
/// verified `charge` entry whose outcome is `permitted`, at its entry's own time, which the engine takes from Core's
/// clock ([`Charged::counted_at`]), never from the charge's `at`. The one definition every
/// [`crate::funds::Recorded`] reads charges by.
pub fn charged_in<'a>(entries: impl IntoIterator<Item = &'a Entry>) -> Vec<Counted> {
    entries
        .into_iter()
        .filter_map(|entry| match entry {
            Entry::Verified(VerifiedEntry {
                what: What::Charge { charge },
                outcome: Outcome::Permitted {},
                when,
                ..
            }) => Some(Counted {
                charge: charge.clone(),
                at: Duration::from_nanos(
                    u64::try_from(when.at.unix_nanos().max(0)).unwrap_or(u64::MAX),
                ),
            }),
            _ => None,
        })
        .collect()
}

impl Charged {
    /// Rebuilds the counter, in place of what it held, from the charges `counted` before: those of the latest window,
    /// the one `now` falls in or a later one any of them was counted in, each counted once under its party's id. The
    /// counter is a function of them, so an engine opened again after a restart, or beside another, holds what they
    /// hold. Its bounds stay as they are.
    ///
    /// A charge counted more than one window ahead of `now` doesn't choose the window, so a clock that jumped forward
    /// once and came back freezes no window, in this engine or in a fresh one over the same log; but it counts in the
    /// window chosen, so a clock stepping back more than a window, or two engines whose clocks are further apart than
    /// that, give no fresh bound. It is set apart for the operator ([`Charged::ahead`]). No window is counted below
    /// this engine's own charges in it.
    pub fn rebuild(&mut self, now: Duration, counted: impl IntoIterator<Item = Counted>) {
        let counted: Vec<Counted> = counted.into_iter().collect();
        let window_of = |at: Duration| at.as_secs() / WINDOW_SECS;
        let reach = window_of(now).saturating_add(1);
        let (near, ahead): (Vec<Counted>, Vec<Counted>) = counted
            .iter()
            .cloned()
            .partition(|c| window_of(c.at) <= reach);
        self.window = near
            .iter()
            .map(|c| window_of(c.at))
            .fold(window_of(now), u64::max);
        self.used.clear();
        self.counted.clear();
        for c in &near {
            if window_of(c.at) == self.window {
                self.take(&c.charge);
            }
        }
        // Ahead of Core's now, they don't choose the window, but they still count in it: a clock that stepped back,
        // or another engine's clock ahead of this one's, gives no fresh bound, and a jump forward takes off the bound
        // only what was charged during it, failing closed.
        for c in &ahead {
            self.take(&c.charge);
        }
        let window = self.window;
        self.own.retain(|c| window_of(c.at) == window);
        for c in self.own.clone() {
            self.take(&c.charge);
        }
        self.ahead = ahead;
        self.last_read = counted;
    }
}

impl<L, P, S> Engine<L, P, S> {
    /// The work charged to each party in the window now.
    pub fn charged(&self) -> &Charged {
        &self.charged
    }

    /// Hands the engine the deployment's bounds, in place of [`Uniform`]'s.
    pub fn set_bounds(&mut self, bounds: Box<dyn Bounds>) {
        self.charged.bounds = bounds;
    }
}

/// `at` since the epoch, by Core's clock, as an RFC 3339 UTC time to the second: the time a charge's entry is
/// recorded at, which the rebuild reads it by.
fn timestamp(at: Duration) -> Option<rutent_contracts::router_api::Timestamp> {
    let secs = i64::try_from(at.as_secs()).ok()?;
    let (days, secs) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days since 1970-01-01 to a civil date (proleptic Gregorian).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    rutent_contracts::router_api::Timestamp::new(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs / 3_600,
        secs / 60 % 60,
        secs % 60
    ))
}

/// Work charged to the accountable party its caller acts for: the counter rebuilt from the log's read, the charge
/// decided through the one consultation, held to the party's bound in the latest window, and recorded at the moment
/// Core's clock counted it before it is answered. A log that can't be read leaves the counter an input the decision
/// doesn't have, and the charge is refused. Only a charge recorded within the bound is counted.
impl<L: PolicyLog + Recorded, P: Policy, S: Sources> Charges for Engine<L, P, S> {
    fn charge(&mut self, charge: &Charge) -> Result<ChargeAnswer, AppendFailure> {
        charge.check().map_err(AppendFailure::Refused)?;
        let now = self.register.clock.now();
        let answer = match self.log.charged() {
            Ok(read) if self.charged.stands(&read) => {
                self.charged.rebuild(now, read);
                if self.charged.reissues(charge) {
                    Answer::Refuse {
                        class: RefusalClass::NotPermitted,
                    }
                } else {
                    self.consult(Asked::Charge(charge))
                }
            }
            // A read that fails, or can't stand for the log: the counter is an input the decision doesn't have.
            _ => Answer::Refuse {
                class: RefusalClass::MissingInput,
            },
        };
        let outcome = charge_outcome(answer, self.charged.standing(charge));
        // Counted at Core's clock, to the second, as its entry holds it; a moment no entry can hold is refused.
        let counted_at = Duration::from_secs(self.charged.counted_at(now).as_secs());
        let at = timestamp(counted_at).ok_or(AppendFailure::Refused(
            rutent_contracts::core_internal::Violation(
                "Core's clock gives no time a charge's entry can hold",
            ),
        ))?;
        let what = What::Charge {
            charge: charge.clone(),
        };
        let receipt = self.record(&charge.caller, what, &at, outcome.outcome())?;
        if matches!(outcome, ChargeOutcome::Within { .. }) {
            self.charged.counted(Counted {
                charge: charge.clone(),
                at: counted_at,
            });
        }
        Ok(ChargeAnswer {
            request: charge.request.clone(),
            outcome,
            receipt,
        })
    }
}

#[cfg(test)]
mod tests;
