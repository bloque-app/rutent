//! Resolving a stuck order: decide and move as separate permissions, their actors compared at the accountable party.
//!
//! Declared by `T-020-01`; `T-021-06` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! # Two steps, two actors
//!
//! A stuck order is resolved in two steps, each a policy call of its own (`REQ-AUZ-4`, ADR-0011, `DEC-14`): one
//! actor decides the outcome, the provider that moves it, where to or on to which linked order, the transaction it
//! goes under and the evidence it relied on ([`StuckOrders::decide_stuck`]); another moves the value that decision
//! named ([`StuckOrders::move_stuck`]), and only that move's permit backs the `resolve_stuck` effect. A move whose
//! outcome comes back unknown settles on two actors' word too: its mover reports it
//! ([`StuckOrders::report_outcome`]) and another confirms the report ([`StuckOrders::confirm_outcome`]). Each call
//! reaches the engine's one consultation, the fixed rules first and then the deployment's policy, and its answer is
//! recorded before it is given.
//!
//! # What the fixed rule holds, from the request
//!
//! This row's entry in [`FIXED`](crate::engine), asked after every earlier row's, so each actor is already bound to
//! the party it states (`T-021-03`) and an agent to its party's scope (`T-021-05`):
//!
//! - **Separate permissions.** Deciding, moving and confirming are each a permission of the deployment's register,
//!   held login by login ([`Permission::DecideStuck`], [`Permission::MoveStuck`], [`Permission::ConfirmOutcome`]),
//!   so one alone allows only its own step, and no login holds the others by its party. Deciding and moving are the
//!   operator's (`REQ-ORD-28`: the operator resolves a stuck order and bears it): a decider or a mover in any other
//!   role is refused, whatever the register says. A confirmer may act in any role the register lists it for.
//!   Reporting a move's outcome needs no permission of its own: only the move's own mover reports, which the log
//!   holds (`outcome-report-rests-on-its-move`).
//! - **Never the order's customer** (`THR-70`). Neither the decider nor the confirmer acts for a party of the order,
//!   its requester, beneficiary or payer, as the log's record of its acceptance names them (`Source::OrderParties`).
//!   It is asked by party, since the register binds a login's party and kind but not its role, so a customer's login
//!   stating the operator's role is still the customer.
//! - **Evidence the store keeps.** A decision names its evidence by the commitment the evidence store answered when it
//!   kept it, at least one and each once, or it fails its check and is refused unrecorded. Each must be one the store
//!   keeps, about the order or about no order in particular (`Source::Kept`), or the decision names no evidence and
//!   is refused as missing an input (`REQ-AUZ-4`).
//!
//! # What the log holds, read on policy's handle
//!
//! A move names the decision it carries out, not its decider, and a confirmation names its report and its move by
//! receipt, so a plain function of the request can't compare them. These rules read the log on policy's handle
//! ([`Receipted`]) and are held to the consultation's verdict, as `T-021-04`'s register is: a refusal of the fixed
//! rules or of the deployment's policy stands, and otherwise this row's refusal is the answer, since a refusal beats
//! not known. They never permit.
//!
//! - **The move.** It carries out a decision the log holds as permitted, of its order, with the request and decision
//!   it names, or it is refused. Its mover and that decision's decider count as one actor where
//!   [`same_accountable_actor`] says so, they act for one party or are one login, and the move is refused as the same
//!   actor (`REQ-AUZ-4`, `REQ-AUZ-8`): two agents of one party, or its person and its agent, can't split the steps.
//!   Nor does the desk of the provider holding the funds, the one the decision names (`resolution.by`), move them.
//! - **The confirmation.** It cites a report the log holds as permitted, of the same move, order and outcome, or it is
//!   refused. Its confirmer is refused as the same actor where it and the reporter are one by
//!   [`same_accountable_actor`], or where their `ActsFor` chains reach a common accountable party
//!   (`Source::CommonParty`), which the log, reading no register, can't see (core-internal's README). Nor does the
//!   desk of the provider holding the funds confirm: the move's own provider for a segment's `move_value`, the
//!   resolution's for a `resolve_stuck`.
//! - **The holder's desk.** An actor is the desk of the provider holding the funds where its party or its login is
//!   that provider, or its chain reaches it.
//!
//! A log that can't be read refuses as missing an input; a register of chains out of reach leaves the answer not
//! known. That a read is of the very log the handle appends to, whole, is the assembly's word, as `T-021-04`'s and
//! `T-021-05`'s reads are: every rule here looks a step up and refuses where it isn't found, so a stale read fails
//! closed, and only a read of another log that names another decider could mislead it.

use rutent_contracts::core_internal::calls::{AppendFailure, PolicyLog, StuckOrders};
use rutent_contracts::core_internal::{
    Action, Actor, Answer, Decision, Entry, NotKnownCause, Outcome, OutcomeConfirmation,
    OutcomeReport, Receipt, RefusalClass, Resolution, Role, StuckDecide, StuckMove, VerifiedEntry,
    What,
};
use rutent_contracts::router_api::Id;

use crate::engine::{Asked, Engine, Input, Need, Policy, Reply, Ruling, Said, Source, Sources};
use crate::funds::ReadFailure;
use crate::parties::same_accountable_actor;
use crate::roles::Permission;

/// The read of the log policy appends to, on policy's handle itself: every entry it holds, each with the receipt the
/// log gave it, in log order. A stuck order's move names the decision it carries out, not its decider, and an
/// outcome's confirmation names its report and its move by receipt, so the engine reads them from the log, as the
/// log holds them, and finds each by its receipt as the contract's rows do.
pub trait Receipted {
    /// Every entry the log holds, each with its receipt, in log order. The read returns owned values and holds nothing
    /// of the log once it answers.
    fn receipted(&mut self) -> Result<Vec<(Receipt, Entry)>, ReadFailure>;
}

/// One input the fixed rule needs, the word it must give, and the refusal where it gives the other.
struct Held {
    need: Need,
    must: Said,
    or: RefusalClass,
}

/// The register's word that `actor`, by its login within its party, holds `permission`.
fn listed(actor: &Actor, permission: Permission) -> Held {
    Held {
        need: Need {
            input: actor.acts.clone(),
            from: Source::Permissions {
                party: actor.party.clone(),
                permission,
            },
        },
        must: Said::Yes,
        or: RefusalClass::NotPermitted,
    }
}

/// The order's record's word that `actor`'s party is none of `order`'s parties.
fn not_of(order: &Id, actor: &Actor) -> Held {
    Held {
        need: Need {
            input: actor.party.clone(),
            from: Source::OrderParties(order.clone()),
        },
        must: Said::No,
        or: RefusalClass::NotPermitted,
    }
}

/// What the fixed rule holds `asked` to, or `None` where no actor in its role may ask it.
fn held_of(asked: Asked<'_>) -> Option<Vec<Held>> {
    match asked {
        Asked::DecideStuck(r) => (r.decider.role == Role::Operator).then(|| {
            let mut held = vec![
                listed(&r.decider, Permission::DecideStuck),
                not_of(&r.order, &r.decider),
            ];
            held.extend(r.evidence.iter().map(|commitment| Held {
                need: Need {
                    input: r.order.clone(),
                    from: Source::Kept(commitment.clone()),
                },
                must: Said::Yes,
                or: RefusalClass::MissingInput,
            }));
            held
        }),
        Asked::MoveStuck(m) => {
            (m.mover.role == Role::Operator).then(|| vec![listed(&m.mover, Permission::MoveStuck)])
        }
        Asked::ConfirmOutcome(c) => Some(vec![
            listed(&c.confirmer, Permission::ConfirmOutcome),
            not_of(&c.order, &c.confirmer),
        ]),
        _ => Some(Vec::new()),
    }
}

/// What the fixed rule needs for `asked`: nothing for a call it refuses outright, or one it doesn't hold.
pub(crate) fn needs(asked: Asked<'_>) -> Reply<Vec<Need>> {
    Reply::Given(
        held_of(asked)
            .unwrap_or_default()
            .into_iter()
            .map(|h| h.need)
            .collect(),
    )
}

/// The fixed rule's ruling: a step no actor in its caller's role may take is refused; otherwise each input must give
/// the word the rule holds it to, the first that doesn't refusing for its class, and an input not given is missing.
/// Every call the rule doesn't hold passes.
pub(crate) fn rule(asked: Asked<'_>, inputs: &[Input]) -> Reply<Ruling> {
    let Some(held) = held_of(asked) else {
        return Reply::Given(Ruling::Refuse(RefusalClass::NotPermitted));
    };
    let mut refusal = None;
    for h in &held {
        match inputs.iter().find(|i| i.need == h.need).map(|i| i.said) {
            Some(said) if said == h.must => {}
            Some(_) => {
                refusal.get_or_insert(h.or);
            }
            None => return Reply::Missing,
        }
    }
    Reply::Given(refusal.map_or(Ruling::Permit, Ruling::Refuse))
}

/// What a rule read from the log found: nothing against the step, a refusal, or an input out of reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Check {
    Clear,
    Refuse(RefusalClass),
    NotKnown(NotKnownCause),
}

/// The consultation's `answer`, held to `check`: a refusal stands; otherwise the check's refusal is the answer, and
/// its not known is where the consultation would have let the step through. It never permits.
fn held(answer: Answer, check: Check) -> Answer {
    match (answer, check) {
        (Answer::Refuse { .. }, _) | (_, Check::Clear) => answer,
        (_, Check::Refuse(class)) => Answer::Refuse { class },
        (Answer::NotKnown { .. }, Check::NotKnown(_)) => answer,
        (_, Check::NotKnown(cause)) => Answer::NotKnown { cause },
    }
}

/// The verified entry `receipt` names in `entries`, where the log holds it as permitted.
fn permitted_at<'a>(entries: &'a [(Receipt, Entry)], receipt: &Receipt) -> Option<&'a What> {
    entries.iter().find_map(|(r, entry)| match entry {
        Entry::Verified(VerifiedEntry {
            what,
            outcome: Outcome::Permitted {},
            ..
        }) if r == receipt => Some(what),
        _ => None,
    })
}

/// The provider whose holding account holds a resolution's value: the one its decision names.
fn holder_of(resolution: &Resolution) -> &Id {
    match resolution {
        Resolution::Deliver { by, .. } | Resolution::HandOn { by, .. } => by,
    }
}

impl<L: PolicyLog + Receipted, P, S: Sources> Engine<L, P, S> {
    /// The register's word that the chains of `actor`'s login and of `with` meet: a refusal for `class` where they do.
    fn apart(&mut self, actor: &Actor, with: &Id, class: RefusalClass) -> Check {
        let need = Need {
            input: actor.acts.clone(),
            from: Source::CommonParty { with: with.clone() },
        };
        match self.sources.supply(&need) {
            Reply::Given(Said::No) => Check::Clear,
            Reply::Given(Said::Yes) => Check::Refuse(class),
            Reply::Missing => Check::Refuse(RefusalClass::MissingInput),
            Reply::Unreachable => Check::NotKnown(NotKnownCause::VerifierUnreachable),
        }
    }

    /// Whether `actor` is the desk of `holder`, the provider holding the funds: its party or its login, or its chain
    /// reaching it, refuses.
    fn not_the_holders(&mut self, actor: &Actor, holder: &Id) -> Check {
        if &actor.party == holder || &actor.acts == holder {
            return Check::Refuse(RefusalClass::NotPermitted);
        }
        self.apart(actor, holder, RefusalClass::NotPermitted)
    }

    /// The move's rules, read from the log: it carries out a permitted decision of its order, whose decider isn't its
    /// mover's accountable actor, and its mover isn't the desk of the provider the decision names.
    fn move_check(&mut self, request: &StuckMove) -> Check {
        let Ok(entries) = self.log.receipted() else {
            return Check::Refuse(RefusalClass::MissingInput);
        };
        let decided: Vec<&StuckDecide> = entries
            .iter()
            .filter_map(|(_, entry)| match entry {
                Entry::Verified(VerifiedEntry {
                    what:
                        What::StuckDecide {
                            request: decide,
                            decision,
                        },
                    outcome: Outcome::Permitted {},
                    ..
                }) if decide.request == request.carries_out.request
                    && decision == &request.carries_out.decision
                    && decide.order == request.order =>
                {
                    Some(decide)
                }
                _ => None,
            })
            .collect();
        if decided.is_empty() {
            return Check::Refuse(RefusalClass::NotPermitted);
        }
        if decided
            .iter()
            .any(|d| same_accountable_actor(&d.decider, &request.mover))
        {
            return Check::Refuse(RefusalClass::SameActor);
        }
        for d in decided {
            let check = self.not_the_holders(&request.mover, holder_of(&d.resolution));
            if check != Check::Clear {
                return check;
            }
        }
        Check::Clear
    }

    /// The confirmation's rules, read from the log: it cites a permitted report of its move, order and outcome, whose
    /// reporter neither is its confirmer's accountable actor nor shares an accountable party with it along their
    /// chains, and its confirmer isn't the desk of the provider holding the move's funds.
    fn confirm_check(&mut self, request: &OutcomeConfirmation) -> Check {
        let Ok(entries) = self.log.receipted() else {
            return Check::Refuse(RefusalClass::MissingInput);
        };
        let reporter = match permitted_at(&entries, &request.report) {
            Some(What::ReportOutcome {
                request: report, ..
            }) if report.effect == request.effect
                && report.order == request.order
                && report.outcome == request.outcome =>
            {
                report.reporter.clone()
            }
            _ => return Check::Refuse(RefusalClass::NotPermitted),
        };
        let holder = entries.iter().find_map(|(receipt, entry)| match entry {
            Entry::Verified(VerifiedEntry {
                what: What::Effect { action, .. },
                ..
            }) if receipt == &request.effect => match action {
                Action::MoveValue { provider, .. } => Some(provider.clone()),
                Action::ResolveStuck { resolution, .. } => Some(holder_of(resolution).clone()),
                _ => None,
            },
            _ => None,
        });
        let Some(holder) = holder else {
            return Check::Refuse(RefusalClass::NotPermitted);
        };
        if same_accountable_actor(&reporter, &request.confirmer) {
            return Check::Refuse(RefusalClass::SameActor);
        }
        let check = self.apart(&request.confirmer, &reporter.acts, RefusalClass::SameActor);
        if check != Check::Clear {
            return check;
        }
        self.not_the_holders(&request.confirmer, &holder)
    }
}

/// A stuck order's steps and a move's outcome, each decided through the one consultation, held to what the log holds
/// where the request alone can't say, and recorded before it is answered.
impl<L: PolicyLog + Receipted, P: Policy, S: Sources> StuckOrders for Engine<L, P, S> {
    fn decide_stuck(&mut self, request: &StuckDecide) -> Result<Decision, AppendFailure> {
        request.check().map_err(AppendFailure::Refused)?;
        let answer = self.consult(Asked::DecideStuck(request));
        let what = |decision| What::StuckDecide {
            request: request.clone(),
            decision,
        };
        self.answer(
            &request.request,
            &request.decider,
            what,
            &request.at,
            answer,
        )
    }

    fn move_stuck(&mut self, request: &StuckMove) -> Result<Decision, AppendFailure> {
        let consulted = self.consult(Asked::MoveStuck(request));
        let check = match consulted {
            Answer::Refuse { .. } => Check::Clear,
            _ => self.move_check(request),
        };
        let what = |decision| What::StuckMove {
            request: request.clone(),
            decision,
        };
        let answer = held(consulted, check);
        self.answer(&request.request, &request.mover, what, &request.at, answer)
    }

    fn report_outcome(&mut self, request: &OutcomeReport) -> Result<Decision, AppendFailure> {
        let answer = self.consult(Asked::ReportOutcome(request));
        let what = |decision| What::ReportOutcome {
            request: request.clone(),
            decision,
        };
        self.answer(
            &request.request,
            &request.reporter,
            what,
            &request.at,
            answer,
        )
    }

    fn confirm_outcome(
        &mut self,
        request: &OutcomeConfirmation,
    ) -> Result<Decision, AppendFailure> {
        let consulted = self.consult(Asked::ConfirmOutcome(request));
        let check = match consulted {
            Answer::Refuse { .. } => Check::Clear,
            _ => self.confirm_check(request),
        };
        let what = |decision| What::ConfirmOutcome {
            request: request.clone(),
            decision,
        };
        let answer = held(consulted, check);
        self.answer(
            &request.request,
            &request.confirmer,
            what,
            &request.at,
            answer,
        )
    }
}

#[cfg(test)]
mod tests;
