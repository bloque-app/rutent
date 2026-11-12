//! The authorisation to move funds: one order, its destination, its to asset and any floor, and a recalculated route
//! only inside it.
//!
//! Declared by `T-020-01`; `T-021-04` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! # An authorisation
//!
//! An authorisation names one order, its destination, its to asset and any floor the customer set (`REQ-AUZ-3`), and
//! the parts of the order it covers: those it names, or every part where it names none (`parts`).
//! [`Authorisations::authorise`] decides it through the engine's one consultation, as every policy call is decided:
//! the fixed rules first, every actor acting for the party it states (`REQ-AUZ-8`) and who may authorise at the
//! origin (`REQ-AUZ-9`, `T-021-03`), then the deployment's policy. Its answer is recorded before it is given.
//!
//! # The register is the log's
//!
//! The register a move is held to, [`Authorised`], holds nothing of its own. Before every authorisation and every
//! move the engine reads, on its policy handle itself ([`Recorded`], one wire with the append), every authorisation
//! the log holds as permitted ([`permitted_in`]), in log order, and decides on that alone. So an engine opened again
//! after a restart, or beside another over the same log, holds what the log holds, and its own permit reaches the
//! register only through the log. A read that fails, doesn't extend the one before it, or lacks the engine's own last
//! permitted append is refused, and so is every authorisation and every move on it: the register is an input the
//! engine doesn't have (`REQ-AUZ-5`).
//!
//! # A move acts only inside the authorisation in force
//!
//! A move names the authorisation it acts under and the end of the route it belongs to as that route stands now,
//! recalculated or not. It is permitted only where that authorisation is in force for the move's own part, the part
//! its segment belongs to by the plans the log holds ([`Recorded::part_of`]), and the route ends inside it: at its
//! destination, the reserve, the node and the account, in its to asset, delivering at least its floor. Anything else
//! is refused as outside the authorisation, and needs a new one (`REQ-AUZ-3`, `INV-10`). A move whose segment no plan
//! lists is held to the order as a whole, which only an authorisation naming no part speaks for. These are what the
//! log holds a move's effect to (`move-rests-on-its-authorisation`: the order, the four ends, the part, in force for
//! it); the register is stricter than the log in one way, that an id replaced covers nothing more even once its
//! terms are given again under another.
//!
//! # A new authorisation
//!
//! Each part's terms in force are set by the first permitted authorisation that speaks for it. Another with the same
//! order, destination, to asset, floor and origin reserve (the terms the log compares) states them again, and the
//! moves under either stay inside. One that names anything else is a new authorisation: for each part it speaks for,
//! it replaces the terms in force, whose ids then cover no move of that part; a part it doesn't name goes on under
//! its own (`REQ-ORD-43`: one part's new destination strands no other), and
//! terms given again after others replaced them are in force under their new id alone. Only the customer gives a
//! new one, its person or its agent, whose party the roles hold to the order's requester: the customer's own choice
//! after a failed final deposit is such a new authorisation (`REQ-ORD-43`), and nobody else's choice is
//! (`INV-10`). The operator acting for the user and the origin reserve authorise the order at its origin, where the
//! roles let them, and change nothing of it after: their authorisation only ever joins the terms in force, even one
//! of other terms that engines racing over one log left there. An authorisation's id names that authorisation only: the same
//! again, while in force, is decided again; anything else under it, or it once replaced, is refused.
//!
//! # Where the register's rule stands
//!
//! These rules read what the log holds as permitted, which a fixed rule, a plain function of the request, can't, so
//! they are held to the consultation's verdict rather than inside it. Unlike `T-021-02`'s grant rule, which is asked
//! before the consultation, they are held after it: a refusal of the fixed rules or of the deployment's policy stands,
//! so a mover acting for no accountable party is refused as such (`REQ-AUZ-8`, held before any other rule), and
//! otherwise the register's refusal is the answer, since a refusal beats not known. The register never permits: it
//! only refuses what the verdict would have let through or left unknown.

use rutent_contracts::core_internal::calls::{AppendFailure, Authorisations, PolicyLog};
use rutent_contracts::core_internal::{
    Action, Answer, Authorisation, Decision, Entry, Outcome, RefusalClass, Role, VerifiedEntry,
    What,
};
use rutent_contracts::router_api::Id;

use rutent_contracts::state_machine::Observed;

use crate::engine::{Asked, Engine, Policy, Sources};

/// Why the log couldn't be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadFailure {
    /// The log couldn't be reached, or answered nothing.
    Unavailable,
}

/// The read of the log policy appends to, on policy's handle itself: the authorisations the log holds as permitted.
/// The engine asks its log handle to be both (`L: PolicyLog + Recorded`), so the read and the append are one wire, and
/// the register is the log's across a restart and across engines.
///
/// What the engine can check of a read is narrow. It refuses a read that doesn't extend the one before it, which
/// catches one that drops or reorders what it held, and one that lacks the engine's own last permitted append, which
/// catches one frozen before it, or of another log, once the engine has appended. Nothing else of a partial, stale or
/// misdirected read is caught: a first read, or one frozen after the engine's last append, that is of another log or
/// misses what another engine appended, passes both checks. That the read is of the very log the handle appends to,
/// whole and current, is the assembly's word, and so is the part it gives a segment: no check of the engine's holds
/// [`Recorded::part_of`] to the plans the log really holds.
pub trait Recorded {
    /// Every authorisation the log holds as permitted, in log order: [`permitted_in`] over its entries. The read
    /// returns owned values and holds nothing of the log once it answers.
    fn permitted(&mut self) -> Result<Vec<Authorisation>, ReadFailure>;

    /// Every charge the log holds as permitted, each with the moment the engine counted it, in log order:
    /// [`crate::parties::charged_in`] over its entries. The counter a charge is held to is rebuilt from it before
    /// every charge, so it is the log's across a restart and across engines (`T-021-05`).
    fn charged(&mut self) -> Result<Vec<crate::parties::Counted>, ReadFailure>;

    /// The part of `order` that `segment` belongs to, by the plans the log holds: [`part_in`] over its entries; none
    /// where no plan lists the segment. A move is held to the authorisation in force for its own part, which the move
    /// doesn't name (`T-021-04`).
    fn part_of(&mut self, order: &Id, segment: &Id) -> Result<Option<Id>, ReadFailure>;
}

/// The part of `order` that `segment` belongs to, by the plans `entries` hold: a `route_chosen` part listing it, or
/// the part a `route_recalculated` or `destination_changed` gave it to. The one definition every [`Recorded`] reads
/// by, and core-internal's own reading of a segment's part, the private `part_of_segment` its relations hold a move
/// to (`move-rests-on-its-authorisation`, `contracts/src/core_internal/relations.rs`).
pub fn part_in<'a>(
    entries: impl IntoIterator<Item = &'a Entry>,
    order: &Id,
    segment: &Id,
) -> Option<Id> {
    entries.into_iter().find_map(|entry| {
        let Entry::Verified(VerifiedEntry {
            what: What::Input { input },
            ..
        }) = entry
        else {
            return None;
        };
        if input.subject.order.as_str() != order.as_str() {
            return None;
        }
        match &input.observed {
            Observed::RouteChosen { parts, .. } => parts
                .iter()
                .find(|p| {
                    p.segments
                        .iter()
                        .any(|s| s.segment_id.as_str() == segment.as_str())
                })
                .and_then(|p| Id::new(p.part_id.as_str())),
            Observed::RouteRecalculated { segments, .. }
            | Observed::DestinationChanged { segments, .. } => segments
                .iter()
                .any(|s| s.segment_id.as_str() == segment.as_str())
                .then_some(input.subject.part.as_ref())
                .flatten()
                .and_then(|p| Id::new(p.as_str())),
            _ => None,
        }
    })
}

/// The authorisations `entries` hold as permitted, in their order: each verified `authorise` entry whose outcome is
/// `permitted`. The one definition every [`Recorded`] reads by. It is core-internal's own reading of a permitted
/// entry, the private `permitted` its relations hold an effect to (`contracts/src/core_internal/relations.rs`): a
/// verified entry whose outcome is `permitted`, here of an `authorise`.
pub fn permitted_in<'a>(entries: impl IntoIterator<Item = &'a Entry>) -> Vec<Authorisation> {
    entries
        .into_iter()
        .filter_map(|entry| match entry {
            Entry::Verified(VerifiedEntry {
                what: What::Authorise { authorisation, .. },
                outcome: Outcome::Permitted {},
                ..
            }) => Some(authorisation.clone()),
            _ => None,
        })
        .collect()
}

/// The authorisations the log holds as permitted, oldest first, as last read: the register a move is held to.
#[derive(Debug, Default)]
pub struct Authorised {
    permitted: Vec<Authorisation>,
    /// The engine's own last permitted authorisation, appended: every read after it must hold it.
    appended: Option<Authorisation>,
}

/// Whether `a` and `b` name the same order, destination, to asset and floor: what a move is held to.
fn same_terms(a: &Authorisation, b: &Authorisation) -> bool {
    a.order == b.order
        && a.destination == b.destination
        && a.to_asset == b.to_asset
        && a.floor == b.floor
        && a.origin_reserve == b.origin_reserve
}

/// Whether `a` speaks for `part` of its order: a part it names, or any part where it names none. For no part, the
/// order as a whole, which only an authorisation naming no part speaks for: a move whose segment no plan lists is held
/// to it, as the log holds one (`move-rests-on-its-authorisation`).
fn speaks_for(a: &Authorisation, part: Option<&Id>) -> bool {
    match (part, &a.parts) {
        (_, None) => true,
        (Some(p), Some(parts)) => parts.contains(p),
        (None, Some(_)) => false,
    }
}

impl Authorised {
    /// Every authorisation permitted, oldest first.
    pub fn permitted(&self) -> &[Authorisation] {
        &self.permitted
    }

    /// The terms in force for `part` of `order`, and the ids in force under them, folding the permitted authorisations
    /// that speak for it in log order. The first sets the terms. After it, one with the terms in force joins them; one
    /// with other terms replaces them, and the ids under them, only where the customer gave it. Another's authorisation
    /// of other terms, which the engine refuses and only engines racing over one log can leave there, never sets the
    /// terms: it is in force under none. A replacement naming one part replaces nothing for another.
    fn standing(&self, order: &Id, part: Option<&Id>) -> Option<(&Authorisation, Vec<&Id>)> {
        let mut terms: Option<&Authorisation> = None;
        let mut ids: Vec<&Id> = Vec::new();
        for a in self
            .permitted
            .iter()
            .filter(|a| &a.order == order && speaks_for(a, part))
        {
            match terms {
                Some(t) if same_terms(t, a) => {
                    if !ids.contains(&&a.authorisation) {
                        ids.push(&a.authorisation);
                    }
                }
                Some(_) if a.given_by.role != Role::Customer => {}
                _ => {
                    terms = Some(a);
                    ids = vec![&a.authorisation];
                }
            }
        }
        terms.map(|t| (t, ids))
    }

    /// The terms in force for `part` of `order`, or for the order as a whole where `part` is none.
    pub fn in_force(&self, order: &Id, part: Option<&Id>) -> Option<&Authorisation> {
        self.standing(order, part).map(|(terms, _)| terms)
    }

    /// The parts `a` is held to: those it names; or, naming none, the order as a whole and every part an authorisation
    /// of the order names, each of which it speaks for too.
    fn held<'a>(&'a self, a: &'a Authorisation) -> Vec<Option<&'a Id>> {
        if let Some(parts) = &a.parts {
            return parts.iter().map(Some).collect();
        }
        let mut held = vec![None];
        let named = self
            .permitted
            .iter()
            .filter(|p| p.order == a.order)
            .filter_map(|p| p.parts.as_ref())
            .flatten();
        for part in named {
            if !held.contains(&Some(part)) {
                held.push(Some(part));
            }
        }
        held
    }

    /// The register of `permitted`, the authorisations a log holds as permitted, in its order ([`permitted_in`]): for
    /// a part of Core outside the engine that needs the register's view of the same log, such as the assembly's
    /// start of an order.
    pub fn of(permitted: Vec<Authorisation>) -> Self {
        Authorised {
            permitted,
            appended: None,
        }
    }

    /// The terms `authorisation` acts under, where it is in force for `part` of `order` (the order as a whole where
    /// `part` is none): permitted for it, and one of the ids in force under the part's terms in force. The engine
    /// holds a move to exactly this, for the move's own part.
    pub fn in_force_as(
        &self,
        order: &Id,
        part: Option<&Id>,
        authorisation: &Id,
    ) -> Option<&Authorisation> {
        self.standing(order, part)
            .filter(|(_, ids)| ids.contains(&authorisation))
            .map(|(terms, _)| terms)
    }

    /// Reads the register again from `log`, in place of what it held: the log alone says what is permitted. Where the
    /// read fails, doesn't extend the read before it, or lacks the engine's own last permitted append, the register is
    /// an input the decision doesn't have, and it is refused; the register stays as it was read last.
    fn read(&mut self, log: &mut impl Recorded) -> Result<(), RefusalClass> {
        let read = log.permitted().map_err(|_| RefusalClass::MissingInput)?;
        let extends = read.starts_with(&self.permitted);
        let holds_own = self.appended.as_ref().is_none_or(|own| read.contains(own));
        if !(extends && holds_own) {
            return Err(RefusalClass::MissingInput);
        }
        self.permitted = read;
        Ok(())
    }

    /// Why `action` is refused, where it is a move that doesn't act inside the authorisation it names: one in force
    /// for the move's own part, by the log as read now, its route ending at its destination, in its to asset,
    /// delivering at least its floor. A part that can't be read is an input missing.
    pub(crate) fn refuses_move(
        &mut self,
        log: &mut impl Recorded,
        action: &Action,
    ) -> Option<RefusalClass> {
        let Action::MoveValue {
            order,
            segment,
            authorisation,
            route,
            ..
        } = action
        else {
            return None;
        };
        if let Err(class) = self.read(log) {
            return Some(class);
        }
        let Ok(part) = log.part_of(order, segment) else {
            return Some(RefusalClass::MissingInput);
        };
        let inside = self
            .in_force_as(order, part.as_ref(), authorisation)
            .is_some_and(|a| {
                a.destination == route.destination
                    && a.to_asset == route.to_asset
                    && a.floor
                        .as_ref()
                        .is_none_or(|floor| &route.delivers >= floor.amount())
            });
        (!inside).then_some(RefusalClass::OutsideAuthorisation)
    }

    /// Why `a` is refused, where it is, by the log as read now: its id names another authorisation, or one no longer
    /// in force for every part it is held to; or it is a new authorisation naming anything else than the terms in
    /// force for any part it is held to, given by anyone but the customer.
    fn refuses(&mut self, log: &mut impl Recorded, a: &Authorisation) -> Option<RefusalClass> {
        if let Err(class) = self.read(log) {
            return Some(class);
        }
        if let Some(given) = self
            .permitted
            .iter()
            .find(|p| p.authorisation == a.authorisation)
        {
            let again = given == a
                && self
                    .held(a)
                    .into_iter()
                    .all(|part| self.in_force_as(&a.order, part, &a.authorisation).is_some());
            return (!again).then_some(RefusalClass::NotPermitted);
        }
        let anew = self.held(a).into_iter().any(|part| {
            self.in_force(&a.order, part)
                .is_some_and(|current| !same_terms(current, a))
        });
        (anew && a.given_by.role != Role::Customer).then_some(RefusalClass::NotPermitted)
    }
}

/// The consultation's `answer`, held to the register's `refusal`: a refusal stands, and otherwise the register's
/// refusal, where it has one, is the answer. It never permits.
pub(crate) fn held(answer: Answer, refusal: Option<RefusalClass>) -> Answer {
    match (answer, refusal) {
        (Answer::Refuse { .. }, _) | (_, None) => answer,
        (_, Some(class)) => Answer::Refuse { class },
    }
}

impl<L, P, S> Engine<L, P, S> {
    /// The authorisations the engine permitted.
    pub fn authorised(&self) -> &Authorised {
        &self.authorised
    }
}

/// An authorisation decided through the one consultation, held to the register as the log holds it, and recorded
/// before it is answered. A permitted one reaches the register only through the log: the next read finds it there.
impl<L: PolicyLog + Recorded, P: Policy, S: Sources> Authorisations for Engine<L, P, S> {
    fn authorise(&mut self, authorisation: &Authorisation) -> Result<Decision, AppendFailure> {
        let a = authorisation;
        let consulted = self.consult(Asked::Authorise(a));
        let answer = held(consulted, self.authorised.refuses(&mut self.log, a));
        let what = |decision| What::Authorise {
            authorisation: a.clone(),
            decision,
        };
        let decision = self.answer(&a.authorisation, &a.given_by, what, &a.at, answer)?;
        if matches!(answer, Answer::Permit {}) {
            self.authorised.appended = Some(a.clone());
        }
        Ok(decision)
    }
}

#[cfg(test)]
mod tests;
