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
//! # One accountable actor: "reaches", never "meets"
//!
//! Two actors count as one where they share a login, share a party, or either's `ActsFor` chain reaches the other's
//! party (`Source::Reaches`): one login may state several parties, and a desk's bot acting for the desk is the desk.
//! Two actors whose chains only meet at a party both reach, two desks of one operator, are distinct: ADR-0011's
//! separate staff within one party. The rule is one, `Engine::one_actor`, for the decider against the mover and
//! for the reporter against the confirmer. The same reading holds an actor to a party: an actor is a party's where
//! its login or party is that party, or its chain reaches it (`Engine::of_party`): the order's customer, the
//! provider holding the funds, the deployment's operator.
//!
//! # What the fixed rule holds, from the request
//!
//! This row's entry in [`FIXED`](crate::engine), asked after every earlier row's, so each actor is already bound to
//! the party it states (`T-021-03`) and an agent to its party's scope (`T-021-05`):
//!
//! - **Separate permissions.** Deciding, moving and confirming are each a permission of the deployment's register,
//!   held login by login ([`Permission::DecideStuck`], [`Permission::MoveStuck`], [`Permission::ConfirmOutcome`]),
//!   so one alone allows only its own step, and no login holds the others by its party. Reporting a move's outcome
//!   needs no permission of its own: only the move's own mover reports, which the log holds
//!   (`outcome-report-rests-on-its-move`).
//! - **Deciding and moving are the operator's** (`REQ-ORD-28`: the operator resolves a stuck order and bears it). A
//!   role is a request's word, so a decider or a mover must both state the operator's role and be the deployment
//!   operator's by the register, in its party or reaching it (`Source::OfOperator`). A confirmer may act in any role
//!   the register gives the permission to.
//! - **The holder never decides** (`THR-70`). A decider that is, or whose chain reaches, the provider the decision
//!   names (`resolution.by`) is refused.
//! - **Never the order's customer, directly.** Neither the decider nor the confirmer acts for a party of the order,
//!   its requester, beneficiary or payer (`Source::OrderParties` must say no). Asked by party, not by role.
//! - **Evidence the store keeps, about the order.** A decision names its evidence by the commitment the evidence store
//!   answered when it kept it, at least one and each once, or it fails its check and is refused unrecorded. Each must
//!   be one the store keeps, about the order or about no order in particular (`Source::Kept`), and at least one about
//!   the order itself (`Source::KeptAbout`), or the decision names no evidence and is refused as missing an input
//!   (`REQ-AUZ-4`).
//!
//! # What the log holds, read on policy's handle
//!
//! A move names the decision it carries out, not its decider; a confirmation names its report and its move by
//! receipt; and the order's parties are its acceptance's. A plain function of the request can't see them, so these
//! rules read the log on policy's handle ([`Receipted`]) and are held to the consultation's verdict, as `T-021-04`'s
//! register is: a refusal of the fixed rules or of the deployment's policy stands, and otherwise this row's refusal is
//! the answer, since a refusal beats not known. They never permit.
//!
//! - **The order's customer, along its chain** (`THR-70`). A decider or a confirmer whose chain reaches any party the
//!   log's acceptance of the order names is refused; an order the log has no acceptance of is a missing input.
//! - **The move.** It carries out a decision the log holds as permitted, of its order, with the request and decision
//!   it names, or it is refused. Its mover and that decision's decider are one actor, refused as the same actor
//!   (`REQ-AUZ-4`, `REQ-AUZ-8`), or its mover is the desk of the provider the decision names, refused.
//! - **The confirmation.** It cites a report the log holds as permitted, of the same move, order and outcome, or it is
//!   refused. Its confirmer and the reporter are one actor, refused as the same actor, which the log, reading no
//!   register, can't see along the chains (core-internal's README); or its confirmer is the desk of the provider
//!   holding the move's funds, the move's own provider for a segment's `move_value`, the resolution's for a
//!   `resolve_stuck`, refused.
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

/// What the fixed rule holds a call to: inputs that must each give their word (`all`), and, where `any` isn't empty,
/// inputs at least one of which must say yes, or the call is refused as missing an input.
#[derive(Default)]
struct Rules {
    all: Vec<Held>,
    any: Vec<Need>,
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

/// The register's word that `actor` is the deployment operator's.
fn operators(actor: &Actor) -> Held {
    Held {
        need: Need {
            input: actor.acts.clone(),
            from: Source::OfOperator,
        },
        must: Said::Yes,
        or: RefusalClass::NotPermitted,
    }
}

/// The register's word that `from` reaches `to` along its chain.
fn reaching(from: &Id, to: &Id) -> Need {
    Need {
        input: from.clone(),
        from: Source::Reaches {
            from: from.clone(),
            to: to.clone(),
        },
    }
}

/// Whether `actor` is `party` by its own fields: its party or its login.
fn is(actor: &Actor, party: &Id) -> bool {
    &actor.party == party || &actor.acts == party
}

/// What the fixed rule holds `asked` to, or `None` where it refuses it outright: a decider or mover not in the
/// operator's role, or a decider that is the provider its decision names.
fn rules_of(asked: Asked<'_>) -> Option<Rules> {
    match asked {
        Asked::DecideStuck(r) => {
            let holder = holder_of(&r.resolution);
            (r.decider.role == Role::Operator && !is(&r.decider, holder)).then(|| {
                let mut all = vec![
                    listed(&r.decider, Permission::DecideStuck),
                    operators(&r.decider),
                    not_of(&r.order, &r.decider),
                    Held {
                        need: reaching(&r.decider.acts, holder),
                        must: Said::No,
                        or: RefusalClass::NotPermitted,
                    },
                ];
                all.extend(r.evidence.iter().map(|commitment| Held {
                    need: Need {
                        input: r.order.clone(),
                        from: Source::Kept(commitment.clone()),
                    },
                    must: Said::Yes,
                    or: RefusalClass::MissingInput,
                }));
                let any = r
                    .evidence
                    .iter()
                    .map(|commitment| Need {
                        input: r.order.clone(),
                        from: Source::KeptAbout(commitment.clone()),
                    })
                    .collect();
                Rules { all, any }
            })
        }
        Asked::MoveStuck(m) => (m.mover.role == Role::Operator).then(|| Rules {
            all: vec![listed(&m.mover, Permission::MoveStuck), operators(&m.mover)],
            any: Vec::new(),
        }),
        Asked::ConfirmOutcome(c) => Some(Rules {
            all: vec![
                listed(&c.confirmer, Permission::ConfirmOutcome),
                not_of(&c.order, &c.confirmer),
            ],
            any: Vec::new(),
        }),
        _ => Some(Rules::default()),
    }
}

/// What the fixed rule needs for `asked`: nothing for a call it refuses outright, or one it doesn't hold.
pub(crate) fn needs(asked: Asked<'_>) -> Reply<Vec<Need>> {
    let rules = rules_of(asked).unwrap_or_default();
    Reply::Given(
        rules
            .all
            .into_iter()
            .map(|h| h.need)
            .chain(rules.any)
            .collect(),
    )
}

/// The fixed rule's ruling: a call it refuses outright is refused; otherwise each input of `all` must give the word
/// the rule holds it to, the first that doesn't refusing for its class, and, where `any` names some, one of them must
/// say yes. An input not given is missing. Every call the rule doesn't hold passes.
pub(crate) fn rule(asked: Asked<'_>, inputs: &[Input]) -> Reply<Ruling> {
    let Some(rules) = rules_of(asked) else {
        return Reply::Given(Ruling::Refuse(RefusalClass::NotPermitted));
    };
    let said = |need: &Need| inputs.iter().find(|i| &i.need == need).map(|i| i.said);
    let mut refusal = None;
    for h in &rules.all {
        match said(&h.need) {
            Some(word) if word == h.must => {}
            Some(_) => {
                refusal.get_or_insert(h.or);
            }
            None => return Reply::Missing,
        }
    }
    let mut any = rules.any.is_empty();
    for need in &rules.any {
        match said(need) {
            Some(Said::Yes) => any = true,
            Some(Said::No) => {}
            None => return Reply::Missing,
        }
    }
    if !any {
        refusal.get_or_insert(RefusalClass::MissingInput);
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

/// The first check of `checks` that isn't clear, asked in order and no further.
macro_rules! first {
    ($($check:expr),+ $(,)?) => {{
        let mut found = Check::Clear;
        $(
            if found == Check::Clear {
                found = $check;
            }
        )+
        found
    }};
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

/// The parties the log's acceptances of `order` name: its requester, beneficiary and payer, each once.
fn parties_of(entries: &[(Receipt, Entry)], order: &Id) -> Vec<Id> {
    let mut parties: Vec<Id> = Vec::new();
    for (_, entry) in entries {
        if let Entry::Verified(VerifiedEntry {
            what:
                What::Acceptance {
                    order: o,
                    parties: p,
                    ..
                },
            ..
        }) = entry
            && o == order
        {
            for party in [&p.requester, &p.beneficiary, &p.payer] {
                if !parties.contains(party) {
                    parties.push(party.clone());
                }
            }
        }
    }
    parties
}

impl<L: PolicyLog + Receipted, P, S: Sources> Engine<L, P, S> {
    /// The register's word that `from` reaches `to`: a refusal for `class` where it does.
    fn reaches(&mut self, from: &Id, to: &Id, class: RefusalClass) -> Check {
        match self.sources.supply(&reaching(from, to)) {
            Reply::Given(Said::No) => Check::Clear,
            Reply::Given(Said::Yes) => Check::Refuse(class),
            Reply::Missing => Check::Refuse(RefusalClass::MissingInput),
            Reply::Unreachable => Check::NotKnown(NotKnownCause::VerifierUnreachable),
        }
    }

    /// Whether `actor` is `party`'s: its party or its login is `party`, or its chain reaches it. A refusal for
    /// `class` where it is.
    fn of_party(&mut self, actor: &Actor, party: &Id, class: RefusalClass) -> Check {
        if is(actor, party) {
            return Check::Refuse(class);
        }
        self.reaches(&actor.acts, party, class)
    }

    /// Whether `a` and `b` are one accountable actor: one login, one party, or either's chain reaching the other's
    /// party. Chains that only meet are two actors. Refused as the same actor where they are one.
    fn one_actor(&mut self, a: &Actor, b: &Actor) -> Check {
        if same_accountable_actor(a, b) {
            return Check::Refuse(RefusalClass::SameActor);
        }
        first!(
            self.reaches(&a.acts, &b.party, RefusalClass::SameActor),
            self.reaches(&b.acts, &a.party, RefusalClass::SameActor),
        )
    }

    /// Whether `actor` is, or reaches, any party of `order` the log's acceptances name: refused where it does, and a
    /// missing input where the log has no acceptance of it.
    fn not_the_customer(
        &mut self,
        entries: &[(Receipt, Entry)],
        actor: &Actor,
        order: &Id,
    ) -> Check {
        let parties = parties_of(entries, order);
        if parties.is_empty() {
            return Check::Refuse(RefusalClass::MissingInput);
        }
        for party in &parties {
            let check = self.of_party(actor, party, RefusalClass::NotPermitted);
            if check != Check::Clear {
                return check;
            }
        }
        Check::Clear
    }

    /// The decision's rule read from the log: its decider is, or reaches, no party of the order.
    fn decide_check(&mut self, request: &StuckDecide) -> Check {
        let Ok(entries) = self.log.receipted() else {
            return Check::Refuse(RefusalClass::MissingInput);
        };
        self.not_the_customer(&entries, &request.decider, &request.order)
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
        for d in decided {
            let check = first!(
                self.one_actor(&d.decider, &request.mover),
                self.of_party(
                    &request.mover,
                    holder_of(&d.resolution),
                    RefusalClass::NotPermitted
                ),
            );
            if check != Check::Clear {
                return check;
            }
        }
        Check::Clear
    }

    /// The confirmation's rules, read from the log: it cites a permitted report of its move, order and outcome, whose
    /// reporter isn't its confirmer's accountable actor; its confirmer is, or reaches, no party of the order; and it
    /// isn't the desk of the provider holding the move's funds.
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
        first!(
            self.one_actor(&reporter, &request.confirmer),
            self.not_the_customer(&entries, &request.confirmer, &request.order),
            self.of_party(&request.confirmer, &holder, RefusalClass::NotPermitted),
        )
    }
}

/// A stuck order's steps and a move's outcome, each decided through the one consultation, held to what the log holds
/// where the request alone can't say, and recorded before it is answered.
impl<L: PolicyLog + Receipted, P: Policy, S: Sources> StuckOrders for Engine<L, P, S> {
    fn decide_stuck(&mut self, request: &StuckDecide) -> Result<Decision, AppendFailure> {
        request.check().map_err(AppendFailure::Refused)?;
        let consulted = self.consult(Asked::DecideStuck(request));
        let check = match consulted {
            Answer::Refuse { .. } => Check::Clear,
            _ => self.decide_check(request),
        };
        let answer = held(consulted, check);
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
