//! Roles and permissions: only the operator posts a route, and each catalogue change is a permission of its own.
//!
//! Declared by `T-020-01`; `T-021-03` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! # Who an actor acts for
//!
//! External authentication establishes who acts (`Actor::acts`) and nothing more (`REQ-AUZ-1`), so the party a
//! request says an actor acts for is the request's word until the deployment's register of actors says yes
//! (`Source::ActsFor`). A fixed rule, the first asked, holds every actor a request names to it, on every call: who
//! asks, gives, decides, moves, reads, grants or asks about a person, a route's suggester and a permission's grantee.
//! The register binds the kind of actor too (`T-021-05`), so an agent's login can't state itself a person or a
//! component and so escape the scope its party set. An actor the register says acts for another party, or is
//! another kind, acts for no accountable party, and is refused so
//! (`REQ-AUZ-8`), whatever else the request would pass; one the register says nothing of is a missing input. So every
//! rule that reads a stated party, the sessions' and reads' of `T-021-02` among them, reads one the register has
//! bound to its actor.
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
//! A user authorises at its origin, and the route carries that onward (`REQ-AUZ-9`). The role a request states is
//! never enough on its own here either: each actor is held to a record that says it is what it states.
//!
//! - **The user**, a customer or its agent, moves value on, and authorises, only an order whose requester its party
//!   is, which the order's record says (`Source::Requester`).
//! - **Core** moves value carrying the user's authorisation onward only through an actor of its own the register
//!   lists for it ([`Permission::CarryOnward`]), so no other actor moves value by stating Core's role.
//! - **The operator** acts for the user only through a login the register lists for it ([`Permission::ActForUser`]),
//!   at the order's own origin reserve, which the order's record says (`Source::OrderOrigin`), and only where that
//!   reserve declared at publishing (`REQ-CAT-6`, `Source::Declarations`) that it can't require the user's own
//!   withdrawal ([`NO_USER_WITHDRAWAL`]). The operator files an authorisation also where the reserve declared it
//!   starts routes on automatic deposits ([`AUTOMATIC_DEPOSIT`], `REQ-ORD-13`). Any listed login may act so at a
//!   reserve that declared it: the permission is one, not one per reserve.
//! - **The origin reserve itself** authorises its order's withdrawal with no user present where it declared it starts
//!   routes on automatic deposits, and is the order's origin (`REQ-ORD-13`). It moves no value of its own asking.
//!
//! No other role acts for the user. A reserve that never declared gives a missing input, one that declared the other
//! way says no, and either is a refusal; one out of reach gives not known. The decision's entry records who acted
//! (`who`, the request's caller), and an authorisation's its giver (`given_by`).
//!
//! The rule on an authorisation holds on every `authorise`, which `T-021-04` decides through the engine's
//! consultation, as every call is.

use rutent_contracts::core_internal::{Action, Actor, CatalogueChange, RefusalClass, Role};
use rutent_contracts::router_api::Id;

use crate::engine::{Asked, Input, Need, Reply, Ruling, Said, Source};

/// The declaration a reserve makes at publishing that it can't require the user's own withdrawal (`REQ-CAT-6`), the
/// one under which the operator may act for the user there (`REQ-AUZ-9`): the input the fixed rule asks of the
/// reserve's declarations.
pub const NO_USER_WITHDRAWAL: &str = "cannot_require_user_withdrawal";

/// The declaration a reserve makes at publishing that it starts routes on automatic deposits (`REQ-CAT-6`), under
/// which it, or the operator filing for it, authorises the withdrawal with no user present (`REQ-ORD-13`). It is
/// apart from [`NO_USER_WITHDRAWAL`].
pub const AUTOMATIC_DEPOSIT: &str = "starts_on_automatic_deposit";

/// One of the operator's permissions, each separate (`REQ-AUZ-2`, `REQ-AUZ-6`): posting a route, or one catalogue
/// change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Permission {
    /// Posting a route (`REQ-AUZ-2`).
    PostRoute,
    /// Making one kind of catalogue change, and no other (`REQ-AUZ-6`).
    Change(CatalogueChange),
    /// One of Core's own actors moving value on the user's authorisation, carrying it onward (`REQ-AUZ-9`).
    CarryOnward,
    /// An operator's login acting for a user: moving value or giving an authorisation, at a reserve that declared
    /// it (`REQ-AUZ-9`, `REQ-ORD-13`).
    ActForUser,
    /// An operator's login reading an order's recorded state and age, and nothing else of it, without a read
    /// permission (`REQ-ORD-5`). Asked by `T-021-02`'s read rule.
    ReadOrderState,
    /// An operator's login deciding a stuck order's outcome, delivered or handed on, naming its evidence: the decide
    /// step, a permission apart from the move's (`REQ-AUZ-4`, ADR-0011, `DEC-14`). Asked by `T-021-06`'s rule.
    DecideStuck,
    /// An operator's login moving the value a stuck order's decision named: the move step, apart from the decide
    /// step's (`REQ-AUZ-4`, ADR-0011, `DEC-14`). Asked by `T-021-06`'s rule.
    MoveStuck,
    /// An actor confirming a move's outcome, once unknown, that its mover reported: the second actor's word
    /// (`REQ-AUZ-4`, ADR-0011). Asked by `T-021-06`'s rule.
    ConfirmOutcome,
    /// An operator's login asking a failed segment's provider to reverse it (`REQ-ORD-3`, `REQ-ORD-11`). Core's own
    /// actors ask it through [`Permission::CarryOnward`].
    AskReversal,
}

/// What an action needs, as the fixed rule holds it: inputs that must each say yes (`all`), and, where `any` isn't
/// empty, inputs at least one of which must say yes, every one of them given.
#[derive(Debug, Default)]
struct Needs {
    all: Vec<Need>,
    any: Vec<Need>,
}

impl From<Vec<Need>> for Needs {
    fn from(all: Vec<Need>) -> Self {
        Needs {
            all,
            any: Vec::new(),
        }
    }
}

/// What `asked` needs, or `None` where no rule lets its caller ask for it. The role a request states is never enough
/// on its own: each is held to a record that says the actor is what it states.
fn needs_of(asked: Asked<'_>) -> Option<Needs> {
    match asked {
        Asked::Decide(request) => {
            let caller = &request.caller;
            match &request.action {
                Action::PostRoute { .. } => {
                    operator_holds(caller, Permission::PostRoute).map(Needs::from)
                }
                Action::ChangeCatalogue { change, .. } => {
                    operator_holds(caller, Permission::Change(*change)).map(Needs::from)
                }
                Action::MoveValue {
                    order,
                    origin_reserve,
                    ..
                } => match caller.role {
                    // The user, or its agent, moving value on its own order.
                    Role::Customer => Some(vec![requester(caller, order)].into()),
                    // One of Core's own actors carrying the user's authorisation onward.
                    Role::Core => Some(vec![listed(caller, Permission::CarryOnward)].into()),
                    // An operator's login, at the order's own origin reserve, which declared it can't require the user's
                    // own withdrawal.
                    Role::Operator => Some(
                        vec![
                            listed(caller, Permission::ActForUser),
                            origin(order, origin_reserve),
                            declared(origin_reserve, NO_USER_WITHDRAWAL),
                        ]
                        .into(),
                    ),
                    Role::Provider | Role::Verifier | Role::Regulator | Role::Checker => None,
                },
                // A request carrying it fails its check before it is consulted (`REQ-AUZ-4`).
                Action::ResolveStuck { .. } => None,
                // No rule lets anyone ask it yet, so policy refuses it: the operator's permission to erase a kept
                // statement is a hand-off to policy's rows (core-internal's README, `REQ-AUD-13`).
                Action::EraseStatement { .. } => None,
                // Asking a failed segment's provider to reverse it (`REQ-ORD-3`, `REQ-ORD-11`): an operator's login the
                // register lists for it, a permission of its own, or one of Core's own actors carrying the order on.
                // Which provider is asked, and that its segment owes the reversal, the log holds
                // (`reversal-is-asked-of-its-failed-provider`).
                Action::AskReversal { .. } => match caller.role {
                    Role::Operator => {
                        operator_holds(caller, Permission::AskReversal).map(Needs::from)
                    }
                    Role::Core => Some(vec![listed(caller, Permission::CarryOnward)].into()),
                    Role::Customer
                    | Role::Provider
                    | Role::Verifier
                    | Role::Regulator
                    | Role::Checker => None,
                },
            }
        }
        Asked::Authorise(a) => {
            let by = &a.given_by;
            match by.role {
                // The user, or its agent, authorising its own order at its origin.
                Role::Customer => Some(vec![requester(by, &a.order)].into()),
                // An operator's login at the order's own origin reserve, which declared it can't require the user's own
                // withdrawal, or that it starts routes on automatic deposits, the operator filing for it.
                Role::Operator => Some(Needs {
                    all: vec![
                        listed(by, Permission::ActForUser),
                        origin(&a.order, &a.origin_reserve),
                    ],
                    any: vec![
                        declared(&a.origin_reserve, NO_USER_WITHDRAWAL),
                        declared(&a.origin_reserve, AUTOMATIC_DEPOSIT),
                    ],
                }),
                // The order's origin reserve itself, which declared it starts routes on automatic deposits.
                Role::Provider if by.party == a.origin_reserve => Some(
                    vec![
                        origin(&a.order, &a.origin_reserve),
                        declared(&a.origin_reserve, AUTOMATIC_DEPOSIT),
                    ]
                    .into(),
                ),
                Role::Provider | Role::Verifier | Role::Regulator | Role::Checker | Role::Core => {
                    None
                }
            }
        }
        _ => Some(Needs::default()),
    }
}

/// The register's word that `caller` holds `permission`, asked only of a caller in the operator's role: no other
/// role holds one.
fn operator_holds(caller: &Actor, permission: Permission) -> Option<Vec<Need>> {
    (caller.role == Role::Operator).then(|| vec![listed(caller, permission)])
}

/// The register's word that the actor `caller` names, by its `acts` within its party, holds `permission`.
fn listed(caller: &Actor, permission: Permission) -> Need {
    Need {
        input: caller.acts.clone(),
        from: Source::Permissions {
            party: caller.party.clone(),
            permission,
        },
    }
}

/// The order's record's word that `caller`'s party is its requester, as the party its agents act in.
fn requester(caller: &Actor, order: &Id) -> Need {
    Need {
        input: caller.party.clone(),
        from: Source::Requester(order.clone()),
    }
}

/// The order's record's word that `reserve` is its origin reserve.
fn origin(order: &Id, reserve: &Id) -> Need {
    Need {
        input: reserve.clone(),
        from: Source::OrderOrigin(order.clone()),
    }
}

/// The reserve's word that it made `declaration` at publishing.
fn declared(reserve: &Id, declaration: &str) -> Need {
    Need {
        input: Id::new(declaration).expect("a valid identifier"),
        from: Source::Declarations(reserve.clone()),
    }
}

/// What the fixed rule needs for `asked`: nothing for a call it refuses outright, or one it doesn't hold.
pub(crate) fn needs(asked: Asked<'_>) -> Reply<Vec<Need>> {
    let needs = needs_of(asked).unwrap_or_default();
    Reply::Given(needs.all.into_iter().chain(needs.any).collect())
}

/// The fixed rule's ruling over what [`needs_of`] names: every input of `all` says yes and, where `any` names some, at
/// least one of them does. A no refuses, and an input not given is missing. A call no rule lets its caller make is
/// refused, and every call the rule doesn't hold passes.
pub(crate) fn rule(asked: Asked<'_>, inputs: &[Input]) -> Reply<Ruling> {
    let Some(needs) = needs_of(asked) else {
        return Reply::Given(Ruling::Refuse(RefusalClass::NotPermitted));
    };
    let said = |need: &Need| inputs.iter().find(|i| &i.need == need).map(|i| i.said);
    let mut refused = false;
    for need in &needs.all {
        match said(need) {
            Some(Said::Yes) => {}
            Some(Said::No) => refused = true,
            None => return Reply::Missing,
        }
    }
    let mut any_yes = needs.any.is_empty();
    for need in &needs.any {
        match said(need) {
            Some(Said::Yes) => any_yes = true,
            Some(Said::No) => {}
            None => return Reply::Missing,
        }
    }
    if refused || !any_yes {
        return Reply::Given(Ruling::Refuse(RefusalClass::NotPermitted));
    }
    Reply::Given(Ruling::Permit)
}

/// Every actor `asked` names, each of which acts only for the party it states: who asks, gives, decides, moves, reads,
/// grants or asks about a person, and the actors it names besides, a route's suggester and a permission's grantee.
fn actors_named<'a>(asked: Asked<'a>) -> Vec<&'a Actor> {
    match asked {
        Asked::Decide(request) => match &request.action {
            Action::PostRoute {
                suggested_by: Some(suggester),
                ..
            } => vec![&request.caller, suggester],
            _ => vec![&request.caller],
        },
        Asked::Authorise(a) => vec![&a.given_by],
        Asked::DecideStuck(r) => vec![&r.decider],
        Asked::MoveStuck(r) => vec![&r.mover],
        Asked::ReportOutcome(r) => vec![&r.reporter],
        Asked::ConfirmOutcome(c) => vec![&c.confirmer],
        Asked::OpenSession(r) => vec![&r.requester],
        Asked::CheckRead(c) => vec![&c.reader],
        Asked::GrantPermission(c) | Asked::RevokePermission(c) => vec![&c.by, &c.grantee],
        Asked::Charge(c) => vec![&c.caller],
        Asked::AskAboutPerson(q) => vec![&q.asker],
    }
}

/// The register's word, for each actor `asked` names, that it acts for the party it states, as the kind it states.
fn acts_for(asked: Asked<'_>) -> Vec<Need> {
    let mut needs: Vec<Need> = Vec::new();
    for actor in actors_named(asked) {
        let need = Need {
            input: actor.acts.clone(),
            from: Source::ActsFor {
                acts: actor.acts.clone(),
                party: actor.party.clone(),
                kind: actor.kind,
            },
        };
        if !needs.contains(&need) {
            needs.push(need);
        }
    }
    needs
}

/// What the rule that every actor acts for the party it states needs: the register's word for each actor `asked`
/// names, on every call.
pub(crate) fn acts_for_needs(asked: Asked<'_>) -> Reply<Vec<Need>> {
    Reply::Given(acts_for(asked))
}

/// Its ruling: an actor the register doesn't say acts for the party it states acts for no accountable party
/// (`REQ-AUZ-8`), whatever it asks; one the register says nothing of is a missing input.
pub(crate) fn acts_for_rule(asked: Asked<'_>, inputs: &[Input]) -> Reply<Ruling> {
    for need in acts_for(asked) {
        match inputs.iter().find(|i| i.need == need).map(|i| i.said) {
            Some(Said::Yes) => {}
            Some(Said::No) => {
                return Reply::Given(Ruling::Refuse(RefusalClass::NoAccountableParty));
            }
            None => return Reply::Missing,
        }
    }
    Reply::Given(Ruling::Permit)
}

#[cfg(test)]
mod tests;
