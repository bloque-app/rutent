//! Roles and permissions: only the operator posts a route, and each catalogue change is a permission of its own.
//!
//! Declared by `T-020-01`; `T-021-03` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! # Posting a route
//!
//! Only the operator posts a route (`REQ-AUZ-2`). The fixed rule refuses a `post_route` asked in any other role
//! before the deployment's policy is asked, so a provider's suggestion has no standing of its own: it reaches a
//! route only as the operator's post, whose request names who suggested it (`suggested_by`), and the decision's entry
//! records that request whole, the operator as who acted (`THR-90`). A refused post gives no permit, so no effect
//! rests on it.
//!
//! # The operator's permissions
//!
//! Granting, accepting a module, revoking, marking stale, declaring a version exploited, setting a tuple's slippage,
//! and every other catalogue change, promotion of a staged release included, are each a [`Permission`] of its own
//! (`REQ-AUZ-6`), and so is posting a route. Each is held actor by actor, not by the operator's party, since every
//! login of the operator acts for that one party: a permission held by the party would let each of them make every
//! change, which is the insider `THR-88` names. The fixed rule refuses a catalogue change asked in any role but the
//! operator's, the publisher's included (`REQ-MDL-5`), and asks the deployment's register of permissions, an input
//! like any other (`Source::Permissions`), whether the actor holds the one permission the change is. A register that
//! says no refuses, one that says nothing gives a missing input and so a refusal, one out of reach not known
//! (`REQ-AUZ-5`). Each decision's entry records its actor and the change, and each role it acts in apart, even where
//! one party holds several (`REQ-AUD-8`).
//!
//! # Acting for a user
//!
//! A user authorises at its origin, and the route carries that onward (`REQ-AUZ-9`). The operator may act for the
//! user only at a reserve that declared, at publishing, that it can't require the user's own withdrawal
//! (`REQ-CAT-6`): the fixed rule asks that reserve's declarations (`Source::Declarations`) whether it made
//! [`NO_USER_WITHDRAWAL`], for an authorisation the operator gives and for a value-moving action the operator asks for,
//! at the order's origin reserve. A reserve that never declared it gives a missing input, one that declared it can
//! require the user's own withdrawal says no, and either is a refusal. An authorisation is given by the user (a
//! customer, or its agent) or, so, by the operator, and by no other role; value is moved on the request of the user,
//! of Core carrying the user's authorisation onward, or, so, of the operator, and of no other role. The decision's
//! entry records who acted (`who`, the request's caller), and an authorisation's its giver (`given_by`).
//!
//! The rule on an authorisation holds from when `T-021-04` builds `authorise` through the engine's consultation, as
//! every call does; until then its stub permits nothing.

use rutent_contracts::core_internal::{Action, Actor, CatalogueChange, RefusalClass, Role};
use rutent_contracts::router_api::Id;

use crate::engine::{Asked, Input, Need, Reply, Ruling, Said, Source};

/// The declaration a reserve makes at publishing that it can't require the user's own withdrawal (`REQ-CAT-6`), the
/// one under which the operator may act for the user there (`REQ-AUZ-9`): the input the fixed rule asks of the
/// reserve's declarations.
pub const NO_USER_WITHDRAWAL: &str = "cannot_require_user_withdrawal";

/// One of the operator's permissions, each separate (`REQ-AUZ-2`, `REQ-AUZ-6`): posting a route, or one catalogue
/// change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Permission {
    /// Posting a route (`REQ-AUZ-2`).
    PostRoute,
    /// Making one kind of catalogue change, and no other (`REQ-AUZ-6`).
    Change(CatalogueChange),
}

/// What an action needs, as the fixed rule holds it: the inputs that must each say yes, or `None` where no rule lets
/// its caller ask for it.
fn needs_of(asked: Asked<'_>) -> Option<Vec<Need>> {
    match asked {
        Asked::Decide(request) => match &request.action {
            Action::PostRoute { .. } => holds(&request.caller, Permission::PostRoute),
            Action::ChangeCatalogue { change, .. } => {
                holds(&request.caller, Permission::Change(*change))
            }
            Action::MoveValue { origin_reserve, .. } => match request.caller.role {
                Role::Customer | Role::Core => Some(Vec::new()),
                Role::Operator => Some(vec![declared(origin_reserve)]),
                Role::Provider | Role::Verifier | Role::Regulator | Role::Checker => None,
            },
            // A request carrying it fails its check before it is consulted (`REQ-AUZ-4`).
            Action::ResolveStuck { .. } => None,
        },
        Asked::Authorise(authorisation) => match authorisation.given_by.role {
            Role::Customer => Some(Vec::new()),
            Role::Operator => Some(vec![declared(&authorisation.origin_reserve)]),
            Role::Provider | Role::Verifier | Role::Regulator | Role::Checker | Role::Core => None,
        },
        _ => Some(Vec::new()),
    }
}

/// The register's word that `caller` holds `permission`, asked only of a caller in the operator's role: no other
/// role holds one.
fn holds(caller: &Actor, permission: Permission) -> Option<Vec<Need>> {
    (caller.role == Role::Operator).then(|| {
        vec![Need {
            input: caller.acts.clone(),
            from: Source::Permissions {
                party: caller.party.clone(),
                permission,
            },
        }]
    })
}

/// The reserve's word that it declared it can't require the user's own withdrawal.
fn declared(reserve: &Id) -> Need {
    Need {
        input: Id::new(NO_USER_WITHDRAWAL).expect("a valid identifier"),
        from: Source::Declarations(reserve.clone()),
    }
}

/// What the fixed rule needs for `asked`: nothing for a call it refuses outright, or one it doesn't hold.
pub(crate) fn needs(asked: Asked<'_>) -> Reply<Vec<Need>> {
    Reply::Given(needs_of(asked).unwrap_or_default())
}

/// The fixed rule's ruling: a route posted, and a catalogue changed, only by an operator's actor holding that one
/// permission; an authorisation given, and value moved, by the operator only at a reserve that declared it can't
/// require the user's own withdrawal, and never in a role that can't act for the user. Every other call passes. Each
/// input it needs must say yes: a no refuses, and one not given is missing.
pub(crate) fn rule(asked: Asked<'_>, inputs: &[Input]) -> Reply<Ruling> {
    let Some(needs) = needs_of(asked) else {
        return Reply::Given(Ruling::Refuse(RefusalClass::NotPermitted));
    };
    for need in needs {
        match inputs.iter().find(|i| i.need == need) {
            Some(Input {
                said: Said::Yes, ..
            }) => {}
            Some(_) => return Reply::Given(Ruling::Refuse(RefusalClass::NotPermitted)),
            None => return Reply::Missing,
        }
    }
    Reply::Given(Ruling::Permit)
}

#[cfg(test)]
mod tests;
