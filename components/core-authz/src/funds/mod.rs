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
//! origin (`REQ-AUZ-9`, `T-021-03`), then the deployment's policy. Its answer is recorded before it is given, and a
//! permitted one is kept in the engine's register, [`Authorised`].
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
//! else is a new authorisation: it replaces the one in force, which then covers no move. Only the customer gives a
//! new one, its person or its agent, whose party the roles hold to the order's requester: the customer's own choice
//! after a failed final deposit is such a new authorisation (`REQ-ORD-43`), and nobody else's choice is
//! (`INV-10`). The operator acting for the user and the origin reserve authorise the order at its origin, where the
//! roles let them, and change nothing of it after. An authorisation's id names that authorisation only: the same
//! again is decided again, anything else under it refused.
//!
//! # Where the register's rule stands
//!
//! These rules read what the engine itself permitted, which a fixed rule, a plain function of the request, can't, so
//! they are held to the consultation's verdict rather than inside it. Unlike `T-021-02`'s grant rule, which is asked
//! before the consultation, they are held after it: a refusal of the fixed rules or of the deployment's policy stands,
//! so a mover acting for no accountable party is refused as such (`REQ-AUZ-8`, held before any other rule), and
//! otherwise the register's refusal is the answer, since a refusal beats not known. The register never permits: it
//! only refuses what the verdict would have let through or left unknown.

use rutent_contracts::core_internal::calls::{AppendFailure, Authorisations, PolicyLog};
use rutent_contracts::core_internal::{
    Action, Answer, Authorisation, Decision, RefusalClass, Role, What,
};
use rutent_contracts::router_api::Id;

use crate::engine::{Asked, Engine, Policy, Sources};

/// The authorisations the engine permitted, oldest first: the register a move is held to.
#[derive(Debug, Default)]
pub struct Authorised {
    permitted: Vec<Authorisation>,
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

    /// Why `action` is refused, where it is a move that doesn't act inside the authorisation it names: one in force
    /// for its order, its route ending at its destination, in its to asset, delivering at least its floor.
    pub(crate) fn refuses_move(&self, action: &Action) -> Option<RefusalClass> {
        let Action::MoveValue {
            order,
            authorisation,
            route,
            ..
        } = action
        else {
            return None;
        };
        let inside = self.in_force_as(order, authorisation).is_some_and(|a| {
            a.destination == route.destination
                && a.to_asset == route.to_asset
                && a.floor
                    .as_ref()
                    .is_none_or(|floor| &route.delivers >= floor.amount())
        });
        (!inside).then_some(RefusalClass::OutsideAuthorisation)
    }

    /// Why `a` is refused, where it is: its id names another authorisation, or one no longer in force; or it is a new
    /// authorisation for an order with one in force, naming anything else, given by anyone but the customer.
    fn refuses(&self, a: &Authorisation) -> Option<RefusalClass> {
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

    /// Keeps `authorisation` as if it had been permitted, for another row's tests whose moves act inside it.
    #[cfg(test)]
    pub(crate) fn plant(&mut self, authorisation: Authorisation) {
        self.keep(&authorisation);
    }

    /// Keeps `authorisation`, once permitted.
    fn keep(&mut self, authorisation: &Authorisation) {
        if !self.permitted.contains(authorisation) {
            self.permitted.push(authorisation.clone());
        }
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

/// An authorisation decided through the one consultation, held to the register, and recorded before it is answered;
/// a permitted one is kept, so the moves that name it are decided against it.
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
        let decision = self.answer(&a.authorisation, &a.given_by, what, &a.at, answer)?;
        if matches!(answer, Answer::Permit {}) {
            self.authorised.keep(a);
        }
        Ok(decision)
    }
}

#[cfg(test)]
mod tests;
