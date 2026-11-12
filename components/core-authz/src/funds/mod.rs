//! The authorisation to move funds: one order, its destination, its to asset and any floor, and a recalculated route
//! only inside it.
//!
//! Declared by `T-020-01`; `T-021-04` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! # An authorisation
//!
//! An authorisation names one order, its destination, its to asset and any floor the customer set (`REQ-AUZ-3`).
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
//! recalculated or not. It is permitted only where that authorisation is in force for the move's order and the route
//! ends inside it: at its destination, the reserve, the node and the account, in its to asset, delivering at least
//! its floor. Anything else is refused as outside the authorisation, and needs a new one (`REQ-AUZ-3`, `INV-10`).
//! These are the four the log holds a move's effect to (`move-rests-on-its-authorisation`); the register adds what
//! the log can't see, that an authorisation replaced covers nothing more.
//!
//! # A new authorisation
//!
//! An order's terms in force are set by its first permitted authorisation. Another permitted with the same order,
//! destination, to asset and floor states them again, and the moves under either stay inside. One that names anything
//! else, the floor included, is a new authorisation: it replaces the terms in force, whose ids then cover no move, and
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
/// whole and current, is the assembly's word.
pub trait Recorded {
    /// Every authorisation the log holds as permitted, in log order: [`permitted_in`] over its entries. The read
    /// returns owned values and holds nothing of the log once it answers.
    fn permitted(&mut self) -> Result<Vec<Authorisation>, ReadFailure>;
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
}

impl Authorised {
    /// Every authorisation permitted, oldest first.
    pub fn permitted(&self) -> &[Authorisation] {
        &self.permitted
    }

    /// The terms in force for `order`, and the ids in force under them, folding the order's permitted authorisations in
    /// log order. The first sets the terms. After it, one with the terms in force joins them; one with other terms
    /// replaces them, and the ids under them, only where the customer gave it. Another's authorisation of other terms,
    /// which the engine refuses and only engines racing over one log can leave there, never sets the terms: it is in
    /// force under none.
    fn standing(&self, order: &Id) -> Option<(&Authorisation, Vec<&Id>)> {
        let mut terms: Option<&Authorisation> = None;
        let mut ids: Vec<&Id> = Vec::new();
        for a in self.permitted.iter().filter(|a| &a.order == order) {
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

    /// The terms in force for `order`.
    pub fn in_force(&self, order: &Id) -> Option<&Authorisation> {
        self.standing(order).map(|(terms, _)| terms)
    }

    /// The terms `authorisation` acts under, where it is in force for `order`: permitted for it, and one of the ids in
    /// force under the terms in force.
    fn in_force_as(&self, order: &Id, authorisation: &Id) -> Option<&Authorisation> {
        self.standing(order)
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
    /// for its order, by the log as read now, its route ending at its destination, in its to asset, delivering at
    /// least its floor.
    pub(crate) fn refuses_move(
        &mut self,
        log: &mut impl Recorded,
        action: &Action,
    ) -> Option<RefusalClass> {
        let Action::MoveValue {
            order,
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
        let inside = self.in_force_as(order, authorisation).is_some_and(|a| {
            a.destination == route.destination
                && a.to_asset == route.to_asset
                && a.floor
                    .as_ref()
                    .is_none_or(|floor| &route.delivers >= floor.amount())
        });
        (!inside).then_some(RefusalClass::OutsideAuthorisation)
    }

    /// Why `a` is refused, where it is, by the log as read now: its id names another authorisation, or one no longer
    /// in force; or it is a new authorisation for an order with one in force, naming anything else, given by anyone
    /// but the customer.
    fn refuses(&mut self, log: &mut impl Recorded, a: &Authorisation) -> Option<RefusalClass> {
        if let Err(class) = self.read(log) {
            return Some(class);
        }
        if let Some(given) = self
            .permitted
            .iter()
            .find(|p| p.authorisation == a.authorisation)
        {
            let again = given == a && self.in_force_as(&a.order, &a.authorisation).is_some();
            return (!again).then_some(RefusalClass::NotPermitted);
        }
        let current = self.in_force(&a.order)?;
        let anew = !same_terms(current, a);
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
