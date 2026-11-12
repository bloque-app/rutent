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
//! move the engine reads, through the [`Recorded`] the assembly hands it over the same log as its policy handle, every
//! authorisation the log holds as permitted ([`permitted_in`]), in log order, and decides on that alone. So an engine
//! opened again after a restart, or beside another over the same log, holds what the log holds, and its own permit
//! reaches the register only through the log. An engine handed no read, or whose read fails, refuses every
//! authorisation and every move: the register is an input it doesn't have (`REQ-AUZ-5`).
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
//! An order's authorisation in force is the last one permitted for it. Another permitted with the same order,
//! destination, to asset and floor states it again, and the moves under either stay inside. One that names anything
//! else, the floor included, is a new authorisation: it replaces the one in force, which then covers no move, and
//! terms given again after others replaced them are in force under their new id alone. Only the customer gives a
//! new one, its person or its agent, whose party the roles hold to the order's requester: the customer's own choice
//! after a failed final deposit is such a new authorisation (`REQ-ORD-43`), and nobody else's choice is
//! (`INV-10`). The operator acting for the user and the origin reserve authorise the order at its origin, where the
//! roles let them, and change nothing of it after. An authorisation's id names that authorisation only: the same
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

/// A read of the log policy appends to: the authorisations it holds as permitted. The assembly hands the engine one
/// over the same log as its policy handle ([`Engine::set_record`]), so the register is the log's, across a restart
/// and across engines.
pub trait Recorded: std::fmt::Debug {
    /// Every authorisation the log holds as permitted, in log order: [`permitted_in`] over its entries. The read
    /// returns owned values and holds nothing of the log once it answers.
    fn permitted(&mut self) -> Result<Vec<Authorisation>, AppendFailure>;
}

/// The authorisations `entries` hold as permitted, in their order: each verified `authorise` entry whose outcome is
/// `permitted`. The one definition every [`Recorded`] reads by.
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
    record: Option<Box<dyn Recorded>>,
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

    /// The authorisation in force for `order`: the last one permitted for it.
    pub fn in_force(&self, order: &Id) -> Option<&Authorisation> {
        self.permitted.iter().rev().find(|a| &a.order == order)
    }

    /// The authorisation `authorisation` names, where it is in force for `order`: permitted for it with the terms of
    /// the one in force, and none of other terms permitted for it since.
    fn in_force_as(&self, order: &Id, authorisation: &Id) -> Option<&Authorisation> {
        let current = self.in_force(order)?;
        self.permitted
            .iter()
            .rev()
            .filter(|a| &a.order == order)
            .take_while(|a| same_terms(a, current))
            .find(|a| &a.authorisation == authorisation)
    }

    /// Reads the register again from the log, in place of what it held: the log alone says what is permitted. Without
    /// a read, or where the read fails, the register is an input the decision doesn't have, and it is refused.
    fn read(&mut self) -> Result<(), RefusalClass> {
        let record = self.record.as_mut().ok_or(RefusalClass::MissingInput)?;
        self.permitted = record.permitted().map_err(|_| RefusalClass::MissingInput)?;
        Ok(())
    }

    /// Why `action` is refused, where it is a move that doesn't act inside the authorisation it names: one in force
    /// for its order, by the log as read now, its route ending at its destination, in its to asset, delivering at
    /// least its floor.
    pub(crate) fn refuses_move(&mut self, action: &Action) -> Option<RefusalClass> {
        let Action::MoveValue {
            order,
            authorisation,
            route,
            ..
        } = action
        else {
            return None;
        };
        if let Err(class) = self.read() {
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
    fn refuses(&mut self, a: &Authorisation) -> Option<RefusalClass> {
        if let Err(class) = self.read() {
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

/// A read that holds `authorisations` as the log's, for another row's tests whose moves act inside them.
#[cfg(test)]
#[derive(Debug)]
pub(crate) struct Holding(pub(crate) Vec<Authorisation>);

#[cfg(test)]
impl Recorded for Holding {
    fn permitted(&mut self) -> Result<Vec<Authorisation>, AppendFailure> {
        Ok(self.0.clone())
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

    /// Hands the engine its read of the log it appends to. Until it has one, it refuses every authorisation and
    /// every move.
    pub fn set_record(&mut self, record: Box<dyn Recorded>) {
        self.authorised.record = Some(record);
    }
}

/// An authorisation decided through the one consultation, held to the register as the log holds it, and recorded
/// before it is answered. A permitted one reaches the register only through the log: the next read finds it there.
impl<L: PolicyLog, P: Policy, S: Sources> Authorisations for Engine<L, P, S> {
    fn authorise(&mut self, authorisation: &Authorisation) -> Result<Decision, AppendFailure> {
        let a = authorisation;
        let answer = held(
            self.consult(Asked::Authorise(a)),
            self.authorised.refuses(a),
        );
        let what = |decision| What::Authorise {
            authorisation: a.clone(),
            decision,
        };
        self.answer(&a.authorisation, &a.given_by, what, &a.at, answer)
    }
}

#[cfg(test)]
mod tests;
