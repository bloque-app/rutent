//! Core carries out an erasure of a kept statement (`T-020-37`, `AC-36`, `REQ-AUD-13`, `DEC-18`).
//!
//! An erasure is a decided action like any other. [`erase_statement`] runs it in this order, each step once:
//!
//! 1. asks the evidence store whether it would erase the statement ([`Statements::erasable`]): one it keeps, that no
//!    entry rests on and that is no advertisement. A refusal ends it before anything is asked of policy, so no
//!    erasure is decided that the store would refuse, and nothing is recorded (`NotErasable`);
//! 2. asks policy to decide `erase_statement` for the commitment, as the caller's own act; a refusal or an answer
//!    that isn't a permit ends it, and no effect is recorded (`NotPermitted`: nothing is erased, `DEC-18`);
//! 3. appends the effect on the permit, recorded before it takes place (`INV-7`), and asks the store again: the
//!    log may have taken a citation since step 1 (under an override, since the permitting decision:
//!    [`Statements::erasable_overriding_since`]). A store that now refuses gets the effect's result `failed`
//!    appended, and the statement stays (`Failed`, `REQ-AUD-13`);
//! 4. appends the effect's `done` result, which is what the store erases on (`BoundStore::erase` takes its receipt
//!    and reads the decision back through it);
//! 5. has the store erase the statement's bytes, its commitment and reference staying in the log;
//! 6. appends the `erasure` entry, which the log admits only after that done result
//!    (`erasure-is-carried-out-on-its-decision`) and names the kept statement
//!    (`erasure-names-a-kept-statement`).
//!
//! After the done result the erasure can't be undone: the log counts the statement erased from there
//! (`nothing-cites-an-erased-statement`). So a store that can't be reached now, or an `erasure` entry the log can't
//! take, leaves an [`Unsettled`], not a failure: [`Unsettled::settle`] carries out the rest, and the store's erasure
//! of one erased already is `Ok` (`INV-8`). The log's own row refuses an `erasure` entry that says `failed` after a
//! done result, so a failure there is never recorded as one: it is the evidence store's finding
//! (`unsettled_erasures`) until it is settled.
//!
//! By default a statement an entry rests on isn't erased (`EVIDENCE_RESTED_ON`, `REQ-AUD-13`). The operator may
//! override that through a decided erasure (`T-020-42`, item `erasure-of-cited-evidence`): [`erase_statement_overriding`]
//! runs the same steps with the override's check ([`Statements::erasable_overriding`]) in place of the default's, and
//! asks policy for an `erase_statement` that sets `overrides_citations`, which policy permits an operator's login holding
//! both permissions alone (`T-021-13`). The store honours the flag only from that permitting decision (`T-022-10`), and
//! the entries that cite the statement stay as they are. Core sets the flag nowhere else, and [`erase_statement`] never.
//!
//! Core reaches the store only through [`Statements`]; the assembly implements it over the bound store, the only
//! handle that erases (`T-022-03`). Nothing here decides who may erase: that is policy's, and Core's own actors are
//! never the caller.

use rutent_contracts::core_internal::calls::{AppendFailure, Decisions, Log};
use rutent_contracts::core_internal::{
    Action, Actor, Answer, DecisionRequest, Outcome, Receipt, Token, What,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::router_api::Timestamp;

use crate::rounds::Fresh;
use crate::segments::journal::{APPEND_TRIES, append_general, verified};
use crate::snapshot::Clock;

/// What Core asks of the evidence store: the checks it runs before it erases, and the erasure itself.
pub trait Statements {
    /// Whether the store would erase the statement kept under `commitment` now.
    ///
    /// # Errors
    ///
    /// The store's refusal, or `Unavailable`.
    fn erasable(&mut self, commitment: &Digest) -> Result<(), AppendFailure>;

    /// [`Statements::erasable`] for an erasure the operator asked to override the citations (`T-020-42`): every rule
    /// but the one that refuses a statement an entry rests on. It answers what the operator may ask, never what the
    /// store does, which is the permitting decision's word. The default is the refusing check, so a store that says
    /// nothing of an override never lifts one.
    ///
    /// # Errors
    ///
    /// The store's refusal, or `Unavailable`.
    fn erasable_overriding(&mut self, commitment: &Digest) -> Result<(), AppendFailure> {
        self.erasable(commitment)
    }

    /// Whether the statement may still be erased on the override the permitting decision at `decision` made
    /// (`T-020-42`): the override lifts only the citations made before that decision, so one the log took between the
    /// decision and the effect refuses it. Run after the effect is recorded. The default refuses, so a store that
    /// can't say never erases on an override.
    ///
    /// # Errors
    ///
    /// The store's refusal, or `Unavailable`.
    fn erasable_overriding_since(
        &mut self,
        _commitment: &Digest,
        _decision: &Receipt,
    ) -> Result<(), AppendFailure> {
        Err(AppendFailure::Refused(
            rutent_contracts::core_internal::Violation(
                "a store that can't say whether a citation came after the decision refuses the override",
            ),
        ))
    }

    /// Erases the statement the erasure decided at `result`, the receipt of its effect's `done` result. One erased
    /// already is `Ok`.
    ///
    /// # Errors
    ///
    /// The store's refusal, or `Unavailable`: the call may be made again.
    fn erase(&mut self, result: &Receipt) -> Result<(), AppendFailure>;
}

impl dyn Statements + '_ {
    /// The check the erasure runs, the override's where the operator asked for it and the default's otherwise.
    fn check(&mut self, commitment: &Digest, overriding: bool) -> Result<(), AppendFailure> {
        match overriding {
            true => self.erasable_overriding(commitment),
            false => self.erasable(commitment),
        }
    }
}

/// An erasure asked for.
#[derive(Debug, Clone)]
pub struct Erasure<'a> {
    /// The commitment the statement is kept under.
    pub commitment: &'a Digest,
    /// Why, as a label to read: policy branches on none.
    pub reason: &'a Token,
    /// Who asks: the operator's login. Policy decides whether it may.
    pub caller: &'a Actor,
    /// Core, whose records these are.
    pub core: &'a Actor,
}

/// An erasure carried out and recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Erased {
    /// The effect's receipt.
    pub effect: Receipt,
    /// The effect's `done` result.
    pub result: Receipt,
    /// The `erasure` entry.
    pub entry: Receipt,
}

/// Why nothing was erased, or not yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotErased {
    /// The store wouldn't erase it, before anything was asked: nothing was decided or recorded.
    NotErasable(AppendFailure),
    /// Policy didn't permit it: no effect was recorded, and nothing was erased.
    NotPermitted(Answer),
    /// The log refused or failed an append before the effect's receipt, or the clock gave no time: nothing changed.
    Append(AppendFailure),
    /// The store refused after the effect was recorded: the effect's result is `failed`, and the statement stays.
    Failed {
        /// The effect's receipt.
        effect: Receipt,
        /// The `failed` result.
        result: Receipt,
        /// The store's refusal.
        why: AppendFailure,
    },
    /// The erasure was decided and its result is done, but the store or the log couldn't finish it now.
    Unsettled(Unsettled),
}

impl From<AppendFailure> for Box<NotErased> {
    fn from(failure: AppendFailure) -> Self {
        Box::new(NotErased::Append(failure))
    }
}

/// An erasure past its `done` result that isn't finished: the rest is carried out by [`Unsettled::settle`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsettled {
    commitment: Digest,
    core: Actor,
    effect: Receipt,
    result: Receipt,
    /// Why it stopped.
    pub why: AppendFailure,
}

impl Unsettled {
    /// The effect's `done` result.
    pub fn result(&self) -> &Receipt {
        &self.result
    }

    /// Has the store erase the statement, if it hasn't, and appends the `erasure` entry.
    ///
    /// # Errors
    ///
    /// Itself again, boxed, with why it stopped, where the store or the log can't do it now.
    pub fn settle(
        self,
        log: &mut dyn Log,
        statements: &mut dyn Statements,
        clock: &dyn Clock,
    ) -> Result<Erased, Box<Unsettled>> {
        let stopped = |me: Unsettled, why| Unsettled { why, ..me };
        let mut last = AppendFailure::Unavailable;
        let mut erased = false;
        for _ in 0..APPEND_TRIES {
            match statements.erase(&self.result) {
                Ok(()) => {
                    erased = true;
                    break;
                }
                Err(AppendFailure::Unavailable) => last = AppendFailure::Unavailable,
                Err(refused) => return Err(Box::new(stopped(self, refused))),
            }
        }
        if !erased {
            return Err(Box::new(stopped(self, last)));
        }
        let at = match now(clock) {
            Ok(at) => at,
            Err(why) => return Err(Box::new(stopped(self, why))),
        };
        let entry = match built_with(
            &self.core,
            What::Erasure {
                evidence: self.commitment.clone(),
            },
            &at,
            Outcome::Done {},
        )
        .and_then(|entry| append_general(log, &entry))
        {
            Ok(entry) => entry,
            Err(why) => return Err(Box::new(stopped(self, why))),
        };
        Ok(Erased {
            effect: self.effect,
            result: self.result,
            entry,
        })
    }
}

fn now(clock: &dyn Clock) -> Result<Timestamp, AppendFailure> {
    clock.now().timestamp().ok_or(AppendFailure::Refused(
        rutent_contracts::core_internal::Violation(
            "Core's clock reads a time a timestamp can hold",
        ),
    ))
}

/// Carries out an erasure, in the order the module's documentation gives.
///
/// # Errors
///
/// [`NotErased`], which names where it stopped; only [`NotErased::Unsettled`] leaves something to finish.
pub fn erase_statement(
    log: &mut dyn Log,
    decisions: &mut dyn Decisions,
    statements: &mut dyn Statements,
    clock: &dyn Clock,
    fresh: &mut dyn Fresh,
    erasure: &Erasure<'_>,
) -> Result<Erased, Box<NotErased>> {
    carry_out(log, decisions, statements, clock, fresh, erasure, false)
}

fn carry_out(
    log: &mut dyn Log,
    decisions: &mut dyn Decisions,
    statements: &mut dyn Statements,
    clock: &dyn Clock,
    fresh: &mut dyn Fresh,
    erasure: &Erasure<'_>,
    overriding: bool,
) -> Result<Erased, Box<NotErased>> {
    statements
        .check(erasure.commitment, overriding)
        .map_err(|why| Box::new(NotErased::NotErasable(why)))?;
    let action = Action::EraseStatement {
        commitment: erasure.commitment.clone(),
        reason: erasure.reason.clone(),
        // Set only where the operator asked for the override (`T-020-42`); policy decides whether it may.
        overrides_citations: overriding,
    };
    let request = DecisionRequest {
        request: fresh.id(),
        caller: erasure.caller.clone(),
        action: action.clone(),
        at: now(clock)?,
    };
    let decision = decisions.decide(&request)?;
    let Some(permit) = decision.permit() else {
        return Err(Box::new(NotErased::NotPermitted(decision.outcome)));
    };
    let effect = append_general(
        log,
        &built_with(
            erasure.core,
            What::Effect {
                action: action.clone(),
                decision: permit.clone(),
            },
            &now(clock)?,
            Outcome::ToTakeEffect {},
        )?,
    )?;
    // The log may have taken a citation of the statement since the first look.
    // Under an override, the citations made before the decision are lifted and none made after it is.
    let still = match overriding {
        true => statements.erasable_overriding_since(erasure.commitment, &decision.receipt),
        false => statements.erasable(erasure.commitment),
    };
    if let Err(why) = still {
        let result = append_general(
            log,
            &built_with(
                erasure.core,
                What::Result {
                    effect: effect.clone(),
                },
                &now(clock)?,
                Outcome::Failed {},
            )?,
        )?;
        return Err(Box::new(NotErased::Failed {
            effect,
            result,
            why,
        }));
    }
    let result = append_general(
        log,
        &built_with(
            erasure.core,
            What::Result {
                effect: effect.clone(),
            },
            &now(clock)?,
            Outcome::Done {},
        )?,
    )?;
    Unsettled {
        commitment: erasure.commitment.clone(),
        core: erasure.core.clone(),
        effect,
        result,
        why: AppendFailure::Unavailable,
    }
    .settle(log, statements, clock)
    .map_err(|unsettled| Box::new(NotErased::Unsettled(*unsettled)))
}

/// [`erase_statement`] where the operator asked to override the citations (`T-020-42`, item `erasure-of-cited-evidence`).
///
/// # Errors
///
/// [`NotErased`], as [`erase_statement`].
pub fn erase_statement_overriding(
    log: &mut dyn Log,
    decisions: &mut dyn Decisions,
    statements: &mut dyn Statements,
    clock: &dyn Clock,
    fresh: &mut dyn Fresh,
    erasure: &Erasure<'_>,
) -> Result<Erased, Box<NotErased>> {
    carry_out(log, decisions, statements, clock, fresh, erasure, true)
}

fn built_with(
    who: &Actor,
    what: What,
    at: &Timestamp,
    outcome: Outcome,
) -> Result<rutent_contracts::core_internal::calls::GeneralEntry, AppendFailure> {
    let entry = verified(who, what, at, outcome);
    entry.check().map_err(AppendFailure::Refused)?;
    rutent_contracts::core_internal::calls::GeneralEntry::try_from(entry)
        .map_err(AppendFailure::Refused)
}
