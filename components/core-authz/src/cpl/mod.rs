//! v0's part of `REQ-CPL`: questions about people identified, bounded and resting on an authority.
//!
//! Declared by `T-020-01`; `T-021-07` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! # A question about a person
//!
//! Every question about a person is put to policy before any provider or verifier is asked it
//! ([`PersonQuestions::ask_about_person`]), decided through the engine's one consultation and recorded before it is
//! answered (`REQ-CPL-2`, `REQ-CPL-5`, `REQ-CPL-6`).
//!
//! - **Identified.** The asker is bound to the accountable party it states, as the kind it states, by `T-021-03`'s
//!   rule, and an agent to its party's scope for asking about people (`Scope::AskAboutPerson`) by `T-021-05`'s, both
//!   before this row's.
//! - **On the person's own authority.** This row's entry in [`FIXED`](crate::engine) asks the person's record of
//!   consents whether the consent the question names is that person's and lets the asker's party ask
//!   (`Source::Consent`, the input the asker's party): a no refuses `not_permitted`, nothing is a missing input. Who
//!   is asked under the person's own consent is that consent's to bound, and the set asked is bounded below; it isn't
//!   held to a shortlist, since a person may consent before any route is built.
//! - **The order's own question, about one of its parties.** On a shortlisted route the same fixed rule holds the
//!   asker to the order's requester (`Source::Requester`, the input the asker's party), or, stating Core's role, to an
//!   actor the register lists as carrying the order's authority onward (`Permission::CarryOnward`), and the person
//!   asked about to the order's parties (`Source::OrderParties`, the input the person): a no refuses `not_permitted`
//!   (`REQ-CPL-2`, `THR-45`; round 1 of the review).
//! - **On a shortlisted route** (read from the log on policy's handle). The shortlist the question cites must be the
//!   order's latest shortlist round's record, so a recalculation's new shortlist puts the providers the old one named
//!   off the route; the providers and verifiers the authority names must be among that record's shortlisted
//!   providers and the verifiers its tuples need; and everyone asked must be among them (`REQ-CPL-6`). Anything else
//!   refuses `not_permitted`.
//! - **Bounded at the party.** Questions about people are charged to the asker's party before they are asked
//!   (`Charges::charge` with `Work::PersonQuestion`, which rounds does, `T-021-05`), so a party's limit on asking about
//!   people binds. The engine holds each question to that: the questions of the order the log holds as permitted for
//!   the asker's party, with this one, must not outnumber the permitted `person_question` units charged to that party
//!   for the order, or it is refused `over_bound`. It counts nothing itself, so nothing is counted twice; the same
//!   question asked again, under its own request id, counts once, and the id reused for another question is refused
//!   `not_permitted`, so no party asks past its charges under one id (round 1 of the review).
//! - **The set asked, recorded and bounded** (`REQ-CPL-5`). The set asked about an order across its recalculations
//!   is the log's: everyone the permitted questions of the order asked, with those the question says were asked before
//!   and those it asks. Past [`ASKED_BOUND`] the answer is past the bound, which rounds read as no route offered and
//!   recalculation as no route left (`REQ-ORD-28`).
//!
//! The rules read from the log run only where the consultation didn't refuse, and never permit: a refusal of the
//! consultation stands, and otherwise theirs is the answer, since a refusal beats not known. A log that can't be read
//! refuses as missing an input. That the read is of the very log the handle appends to is the assembly's word, as
//! `T-021-04`'s and `T-021-06`'s reads are; each rule here looks for what lets the question through, so a stale read
//! refuses rather than permits, except that a read missing permitted questions could undercount the set asked, which
//! the question's own `asked_before` and the log's rows `provider-asked-before-was-asked` and
//! `asked-before-is-complete` hold.

use std::collections::BTreeSet;

use rutent_contracts::core_internal::calls::{AppendFailure, PersonQuestions, PolicyLog};
use rutent_contracts::core_internal::{
    Answer, Authority, Entry, Outcome, PersonAnswer, PersonQuestion, Receipt, RefusalClass, Role,
    RoutingRound, VerifiedEntry, What, Work,
};
use rutent_contracts::provider_role::SafeInteger;
use rutent_contracts::router_api::Id;

use crate::engine::{Asked, Engine, Input, Need, Policy, Reply, Ruling, Said, Source, Sources};
use crate::roles::Permission;
use crate::stuck::Receipted;

/// How many providers and verifiers may be asked about one order's people, across its recalculations (`REQ-CPL-5`).
/// Default, reversible: a deployment's own, lower bound is its policy service's to add.
pub const ASKED_BOUND: u64 = 8;

/// The set asked about an order, with a question's own: within its bound, or past it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetAsked {
    /// Within the bound.
    Within,
    /// Past it.
    Past {
        /// The bound.
        bound: SafeInteger,
    },
}

/// One input the fixed rule needs, the word it must give, and the refusal where it gives the other.
struct Held {
    need: Need,
    must: Said,
    or: RefusalClass,
}

/// What the fixed rule holds a question about a person to.
///
/// - On the person's own authority: the record of consents' word that the consent it names is that person's and lets
///   the asker's party ask.
/// - On a shortlisted route: the question is the order's own (`REQ-CPL-2`, `THR-45`). Its asker acts for the order's
///   requester (`Source::Requester`, the input the asker's party), or, stating Core's role, is an actor the register
///   lists as carrying the order's authority onward (`Permission::CarryOnward`, by its login, as Core's moves are);
///   and the person asked about is one of the order's parties (`Source::OrderParties`, the input the person), the
///   parties a provider's rules name (`REQ-ORD-17`).
fn held(asked: Asked<'_>) -> Vec<Held> {
    let Asked::AskAboutPerson(question) = asked else {
        return Vec::new();
    };
    let yes = |input: &Id, from: Source| Held {
        need: Need {
            input: input.clone(),
            from,
        },
        must: Said::Yes,
        or: RefusalClass::NotPermitted,
    };
    let asker = &question.asker;
    match &question.authority {
        Authority::Person { consent } => vec![yes(
            &asker.party,
            Source::Consent {
                person: question.person.clone(),
                consent: consent.clone(),
            },
        )],
        Authority::ShortlistedRoute { .. } => vec![
            if asker.role == Role::Core {
                yes(
                    &asker.acts,
                    Source::Permissions {
                        party: asker.party.clone(),
                        permission: Permission::CarryOnward,
                    },
                )
            } else {
                yes(&asker.party, Source::Requester(question.order.clone()))
            },
            yes(
                &question.person,
                Source::OrderParties(question.order.clone()),
            ),
        ],
    }
}

/// What the fixed rule needs.
pub(crate) fn needs(asked: Asked<'_>) -> Reply<Vec<Need>> {
    Reply::Given(held(asked).into_iter().map(|h| h.need).collect())
}

/// Its ruling: each input must give the word the rule holds it to, the first that doesn't refusing for its class; an
/// input not given is missing. Every other call passes.
pub(crate) fn rule(asked: Asked<'_>, inputs: &[Input]) -> Reply<Ruling> {
    for h in held(asked) {
        match inputs.iter().find(|i| i.need == h.need).map(|i| i.said) {
            Some(word) if word == h.must => {}
            Some(_) => return Reply::Given(Ruling::Refuse(h.or)),
            None => return Reply::Missing,
        }
    }
    Reply::Given(Ruling::Permit)
}

/// The verified entries `entries` hold as permitted.
fn permitted(entries: &[(Receipt, Entry)]) -> impl Iterator<Item = &What> {
    entries.iter().filter_map(|(_, entry)| match entry {
        Entry::Verified(VerifiedEntry {
            what,
            outcome: Outcome::Permitted {},
            ..
        }) => Some(what),
        _ => None,
    })
}

/// The questions about `order`'s people the log holds as permitted.
fn asked_of<'a>(
    entries: &'a [(Receipt, Entry)],
    order: &'a Id,
) -> impl Iterator<Item = &'a PersonQuestion> {
    permitted(entries).filter_map(move |what| match what {
        What::AskAboutPerson { question } if &question.order == order => Some(question),
        _ => None,
    })
}

/// On a shortlisted route: the shortlist cited is the order's latest shortlist round's record, and the authority's
/// providers and verifiers, and everyone asked, are among the providers it shortlisted and the verifiers its tuples
/// need. A question on the person's own authority passes.
fn on_the_route(question: &PersonQuestion, entries: &[(Receipt, Entry)]) -> Option<RefusalClass> {
    let Authority::ShortlistedRoute {
        shortlist,
        providers,
        verifiers,
    } = &question.authority
    else {
        return None;
    };
    let latest = entries.iter().rev().find_map(|(_, entry)| match entry {
        Entry::Verified(VerifiedEntry {
            what:
                What::DecisionRecord {
                    order,
                    round: RoutingRound::Shortlist,
                    record,
                    shortlisted,
                    verifiers: needed,
                    ..
                },
            ..
        }) if order == &question.order => Some((record, shortlisted, needed)),
        _ => None,
    });
    let Some((record, shortlisted, needed)) = latest else {
        return Some(RefusalClass::NotPermitted);
    };
    let shortlisted = shortlisted.as_deref().unwrap_or_default();
    let on = record == shortlist
        && providers.iter().all(|p| shortlisted.contains(p))
        && verifiers.iter().all(|v| needed.contains(v))
        && question
            .ask
            .iter()
            .all(|a| shortlisted.contains(a) || needed.contains(a));
    (!on).then_some(RefusalClass::NotPermitted)
}

/// A request id names one question: one the log holds as permitted under this id must be this very question, or this
/// one is refused, so a party can't ask past its charges under an id already counted (round 1 of the review).
fn one_question_per_id(
    question: &PersonQuestion,
    entries: &[(Receipt, Entry)],
) -> Option<RefusalClass> {
    permitted(entries)
        .any(|what| {
            matches!(what, What::AskAboutPerson { question: earlier }
                if earlier.request == question.request && earlier != question)
        })
        .then_some(RefusalClass::NotPermitted)
}

/// Bounded at the party: the asker's party's permitted questions of the order, this one among them, don't outnumber
/// the questions about people its permitted charges for the order cover.
fn charged_for(question: &PersonQuestion, entries: &[(Receipt, Entry)]) -> Option<RefusalClass> {
    let party = &question.asker.party;
    let covered: u64 = permitted(entries)
        .filter_map(|what| match what {
            What::Charge { charge, .. }
                if charge.work == Work::PersonQuestion
                    && &charge.party == party
                    && charge.order.as_ref() == Some(&question.order) =>
            {
                Some(charge.count.get())
            }
            _ => None,
        })
        .sum();
    let asked: BTreeSet<&str> = asked_of(entries, &question.order)
        .filter(|q| &q.asker.party == party && q.request != question.request)
        .map(|q| q.request.as_str())
        .collect();
    let asked = u64::try_from(asked.len()).unwrap_or(u64::MAX);
    (asked.saturating_add(1) > covered).then_some(RefusalClass::OverBound)
}

/// The set asked about the order with this question: the log's, those it says were asked before, and those it asks.
fn set_asked(question: &PersonQuestion, entries: &[(Receipt, Entry)]) -> SetAsked {
    let set: BTreeSet<&str> = asked_of(entries, &question.order)
        .flat_map(|q| q.ask.iter())
        .chain(&question.asked_before)
        .chain(&question.ask)
        .map(Id::as_str)
        .collect();
    if u64::try_from(set.len()).unwrap_or(u64::MAX) > ASKED_BOUND {
        SetAsked::Past {
            bound: SafeInteger::try_from(ASKED_BOUND).unwrap_or_else(|_| unreachable!()),
        }
    } else {
        SetAsked::Within
    }
}

/// Every question about a person is decided through the engine's one consultation, held to the rules read from the
/// log, and recorded before it is answered.
impl<L: PolicyLog + Receipted, P: Policy, S: Sources> PersonQuestions for Engine<L, P, S> {
    fn ask_about_person(
        &mut self,
        question: &PersonQuestion,
    ) -> Result<PersonAnswer, AppendFailure> {
        question.check().map_err(AppendFailure::Refused)?;
        let consulted = self.consult(Asked::AskAboutPerson(question));
        let (answer, asked) = match consulted {
            Answer::Refuse { .. } => (consulted, SetAsked::Within),
            _ => match self.log.receipted() {
                Err(_) => (
                    Answer::Refuse {
                        class: RefusalClass::MissingInput,
                    },
                    SetAsked::Within,
                ),
                Ok(entries) => {
                    let refused = on_the_route(question, &entries)
                        .or_else(|| one_question_per_id(question, &entries))
                        .or_else(|| charged_for(question, &entries));
                    let answer = match refused {
                        Some(class) => Answer::Refuse { class },
                        None => consulted,
                    };
                    (answer, set_asked(question, &entries))
                }
            },
        };
        let outcome = crate::engine::person_outcome(answer, asked);
        let what = What::AskAboutPerson {
            question: question.clone(),
        };
        let receipt = self.record(&question.asker, what, &question.at, outcome.outcome())?;
        Ok(PersonAnswer {
            request: question.request.clone(),
            outcome,
            receipt,
        })
    }
}

#[cfg(test)]
mod tests;
