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
//! component and so escape the scope its party set, and the role it acts in (`T-021-14`), so a customer's login can't
//! state itself the operator, a regulator or a checker and be taken for one. An actor the register says acts for
//! another party, or is another kind, or acts in another role, acts for no accountable party, and is refused so
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
//! # The customer's acceptance of a fault
//!
//! A customer accepts a provider's showing that a failure was its fault by a decision, `accept_fault`
//! ([`Action::AcceptFault`]): no customer signs a statement the log can attribute, so the log holds the decision whole
//! and the `reversal_carrier` rests on it (`reversal-carrier-rests-on-its-acceptance`, `REQ-ORD-11`). It is decided
//! and never carried out, so any effect of it is refused. Only the order's requester, or its agent within the scope its
//! party set (`REQ-AUZ-8`), in the customer's role, accepts it (`Source::Requester`); the operator accepts by its own
//! command instead. The showing it names must be one the log holds, received no later than the acceptance is decided
//! (`Source::Showing`). The session it names must be the requester's own and still open, by Core's clock and when the
//! acceptance is decided, which only the engine's register holds (`Register::refuses_acceptance`).
//!
//! # The operator's permissions
//!
//! Granting, accepting a module, revoking, marking stale, declaring a version exploited, setting a tuple's slippage,
//! and every other catalogue change, promotion of a staged release and rolling its staging back included
//! (`CatalogueChange::RollBack`, `T-021-12`, ADR-0014), are each a [`Permission`] of its own
//! (`REQ-AUZ-6`), and so is posting a route. Each is held actor by actor, not by the operator's party, since every
//! login of the operator acts for that one party: a permission held by the party would let each of them make every
//! change, which is the insider `THR-88` names. The fixed rule refuses a catalogue change asked in any role but the
//! operator's, the publisher's included (`REQ-MDL-5`), and asks the deployment's register of permissions, an input
//! like any other (`Source::Permissions`), whether the actor holds the one permission the change is. A register that
//! says no refuses, one that says nothing gives a missing input and so a refusal, one out of reach not known
//! (`REQ-AUZ-5`). Each decision's entry records its actor and the change, and each role it acts in apart, even where
//! one party holds several (`REQ-AUD-8`). The login must also be the deployment operator's by the register
//! (`Source::OfOperator`, `T-021-15`): a login of another party that the register lists in the operator's role holds no
//! catalogue change and sets no setting.
//!
//! Listing a public network with the reader service that reads it, and delisting one (`list_public_network`,
//! `delist_public_network`, `DEC-28`, `T-020-59`), are each a permission of its own beside the lifecycle permissions,
//! held the same way. A reader is the operator's own, so the change names the operator's own party as its provider; one
//! naming any other is refused before the register is asked.
//!
//! # Erasing a statement
//!
//! Erasing a kept statement's bytes is a permission of its own, [`Permission::EraseStatement`], held actor by actor as
//! each catalogue change is, so no login holds it by holding another (`REQ-AUZ-1`, `DEC-18`). Only an operator's login
//! the register lists for it is permitted; every other role is refused before the deployment's policy is asked, Core's
//! own actors too, and a register that says nothing gives a missing input and so a refusal. The decision's entry holds
//! the request whole, naming the commitment and the actor, which the evidence store's erase reads as its permit.
//!
//! An erasure that overrides the citations a later entry makes of the statement (`overrides_citations`, `T-021-13`)
//! asks a permission of its own beside the erase permission, [`Permission::EraseOverridingCitations`]: both, for an
//! operator's login alone. The override permission exempts an erasure from the citation check and erases nothing by
//! itself, and the erase permission alone leaves the default, a cited statement is kept, in force; an override is a
//! decision of its own that names the statement and the override (`REQ-AUD-13`). Core's actors never carry it
//! onward.
//!
//! # Granting and revoking a read
//!
//! A regulator's or a checker's read rests on a permission granted to it (`REQ-AUZ-7`, `REQ-AUD-12`), so who grants
//! one, and who revokes it, decides who reads the whole trail. Granting a read and revoking one are each a
//! [`Permission`] of its own, [`Permission::GrantRead`] and [`Permission::RevokeRead`], held actor by actor as each
//! catalogue change is, so a login listed for one holds not the other (`REQ-AUZ-6`). Only an operator's login is asked
//! the register; every other role is refused before the deployment's policy is asked. The login must also be the
//! deployment operator's by the register, in its party or reaching it (`Source::OfOperator`), as a stuck order's
//! decider is, so a login of another party the register lists in the operator's role grants nothing. The grantee is a
//! regulator or a checker by its request's shape (`PermissionChange::check`), and the register of actors binds that
//! role to its login as it binds every actor's, so a grant to a login stating a role it isn't listed in is refused as
//! acting for no accountable party. A grantee is never the operator's own: the register's word that its login is the
//! deployment operator's (`Source::OfOperator`) refuses the grant, the grantor granting itself included, since a
//! regulator or a checker is outside the operator. A revocation names a grant made, its permission, grantee and scope
//! (`Register`, in `session`). Each decision's entry records the change whole, its grantor and grantee each in its
//! role.
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

use rutent_contracts::core_internal::{
    Action, Actor, CatalogueChange, RefusalClass, Role, SettingKind,
};
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
    /// Setting one of the operator's settings, and no other: the fee formula, a tuple's slippage limit or evidence
    /// floor, a public node's settings, a provider's or a publisher's ceiling, a job's cadence, a protocol
    /// version's cutover date or the lifetime of a deposit's wait for its customer, each a permission of its own (`REQ-AUZ-6`, `T-021-15`). `Change(SetSetting)` is no
    /// permission anyone holds: it names no setting.
    Setting(SettingKind),
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
    /// An operator's login asking a segment's provider for its late status (`REQ-ORD-1`, `T-021-09`). Core's own
    /// actors ask it through [`Permission::CarryOnward`].
    AskStatus,
    /// An operator's login reading a holding account's balance from the module that holds it (`REQ-AUD-15`, `T-020-63`), or
    /// one of Core's own actors carrying that standing permission onward, which holds it beside [`Permission::CarryOnward`].
    ReadHoldingBalance,
    /// An operator's login asking a segment's provider to carry out the segment's own operation again (`REQ-ORD-1`,
    /// `T-020-63`), or one of Core's own actors carrying that standing permission onward beside [`Permission::CarryOnward`].
    RepeatOperation,
    /// An operator's login filing an arrival anew against the order it differed from (`REQ-ORD-23`, `T-020-52`),
    /// a permission of its own: the deployment operator's, by the register. A reserve files its own arrival by being
    /// the order's origin reserve, with no permission of the operator's.
    RefileArrival,
    /// An actor declining a mover's report of a move's outcome, on its own evidence: the confirming actor's other
    /// word, a permission apart from confirming (`REQ-AUZ-4`, ADR-0011, `T-021-09`). Asked by `T-021-09`'s rule.
    DeclineOutcome,
    /// An operator's login erasing a kept statement's bytes from the evidence store, the commitment staying in the
    /// log, a permission of its own as each catalogue change is (`REQ-AUZ-1`, `REQ-AUD-13`, `DEC-18`). Asked by
    /// `T-021-10`'s rule.
    EraseStatement,
    /// An operator's login erasing a kept statement's bytes although a later entry cites it (`T-020-39`'s
    /// `overrides_citations`), a permission of its own, never carried onward and asked beside
    /// [`Permission::EraseStatement`], never in its place, so it erases nothing alone (`REQ-AUZ-6`, `REQ-AUD-13`).
    /// Asked by `T-021-13`'s rule.
    EraseOverridingCitations,
    /// An operator's login granting a regulator or a checker a read permission (`REQ-AUZ-7`, `REQ-AUD-12`), a
    /// permission of its own. Asked by `T-021-14`'s rule.
    GrantRead,
    /// An operator's login revoking a read permission granted (`REQ-AUZ-7`), a permission of its own, apart from
    /// granting. Asked by `T-021-14`'s rule.
    RevokeRead,
}

/// What an action needs, as the fixed rule holds it: inputs that must each say yes (`all`), and, where `any` isn't
/// empty, inputs at least one of which must say yes, every one of them given; and inputs that must each say no
/// (`none`).
#[derive(Debug, Default)]
struct Needs {
    all: Vec<Need>,
    any: Vec<Need>,
    none: Vec<Need>,
}

impl From<Vec<Need>> for Needs {
    fn from(all: Vec<Need>) -> Self {
        Needs {
            all,
            ..Needs::default()
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
                // An operator's setting (`T-020-45`, `T-021-15`): an operator's login, the deployment operator's by the
                // register (`Source::OfOperator`, as a read's grant asks it), that the register lists for the one
                // permission the setting is, one for each kind, so the fee formula's holder sets no slippage limit,
                // and no other role, Core's own actors included. The decision's entry records the request whole, so it
                // names the setting and its value, and who set it. A request that names no setting has nothing to
                // hold a permission to and gets no rule (it fails its check before it is consulted).
                Action::ChangeCatalogue {
                    change: CatalogueChange::SetSetting,
                    setting,
                    ..
                } => setting.as_ref().and_then(|setting| {
                    of_operator_holds(caller, Permission::Setting(setting.kind()))
                }),
                // Listing or delisting a public network with the reader service that reads it (`DEC-28`, `T-020-59`):
                // a permission of its own apiece, beside the lifecycle permissions, for an operator's login the register
                // lists for it and that is the deployment operator's (`Source::OfOperator`), and no other role. A reader
                // is the operator's own, so the change names the operator's own party as its provider: one naming
                // another party gets no rule, and so is refused before the register is asked.
                Action::ChangeCatalogue {
                    change:
                        change @ (CatalogueChange::ListPublicNetwork
                        | CatalogueChange::DelistPublicNetwork),
                    provider,
                    ..
                } => (provider == &caller.party)
                    .then(|| of_operator_holds(caller, Permission::Change(*change)))
                    .flatten(),
                Action::ChangeCatalogue { change, .. } => {
                    of_operator_holds(caller, Permission::Change(*change))
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
                // Erasing a kept statement's bytes (`REQ-AUD-13`, `DEC-18`, `T-021-10`): an operator's login the register
                // lists for a permission of its own, as each catalogue change has one, and no other role, Core's own
                // actors included. The decision's entry records the request whole, so it names the commitment erased
                // and who asked. That the log's statement is one it keeps, and that its erasure is decided, the log holds
                // (`BoundStore::erase`).
                // An erasure overriding the statement's citations (`T-020-39`'s `overrides_citations`, `T-021-13`): an
                // operator's login the register lists for both the erase permission and a permission of its own, and no
                // other role, Core's own actors included. The override exempts an erasure from the citation check; it
                // never erases by itself, so the permission to erase is asked beside it. The default, that an erasure
                // waits while a later entry cites the statement, is the log's and the store's
                // (`erasure-waits-while-anything-rests-on-the-statement`), and an erasure without the override never
                // reaches this arm.
                Action::EraseStatement {
                    overrides_citations: true,
                    ..
                } => operator_holds(caller, Permission::EraseStatement)
                    .map(|mut needs| {
                        needs.push(listed(caller, Permission::EraseOverridingCitations));
                        needs
                    })
                    .map(Needs::from),
                Action::EraseStatement { .. } => {
                    operator_holds(caller, Permission::EraseStatement).map(Needs::from)
                }
                // Asking a segment's provider for a late status (core-internal's `T-020-21` round, `T-021-09`), as
                // a reversal is asked: an operator's login the register lists for it, a permission of its own, or
                // one of Core's own actors carrying the order on. Nobody else, and an unknown role fails closed.
                // That the provider is the one its segment's move went to the log holds
                // (`status-is-asked-of-the-segments-provider`).
                Action::AskStatus { .. } => match caller.role {
                    Role::Operator => {
                        operator_holds(caller, Permission::AskStatus).map(Needs::from)
                    }
                    Role::Core => Some(vec![listed(caller, Permission::CarryOnward)].into()),
                    Role::Customer
                    | Role::Provider
                    | Role::Verifier
                    | Role::Regulator
                    | Role::Checker => None,
                },
                // Reading a holding account's balance from its module (`REQ-AUD-15`, `T-020-63`): an operator's login the
                // register lists for it, a permission of its own, or one of Core's own actors, which carries the operator's
                // standing permission onward: Core's actor listed for `CarryOnward` and for this permission both, as every
                // other decision Core starts is asked as Core. Nobody else, and an unknown role fails closed. Which module
                // and node, the question's answer holds.
                Action::ReadHoldingBalance { .. } => match caller.role {
                    Role::Operator => {
                        operator_holds(caller, Permission::ReadHoldingBalance).map(Needs::from)
                    }
                    Role::Core => Some(
                        vec![
                            listed(caller, Permission::CarryOnward),
                            listed(caller, Permission::ReadHoldingBalance),
                        ]
                        .into(),
                    ),
                    Role::Customer
                    | Role::Provider
                    | Role::Verifier
                    | Role::Regulator
                    | Role::Checker => None,
                },
                // Carrying a segment's own operation out again (`REQ-ORD-1`, `T-020-63`), which may be that move's first
                // carrying-out: an operator's login the register lists for it, a permission of its own, or one of Core's own
                // actors, which carries the operator's standing permission onward, listed for `CarryOnward` and for this
                // permission both. Nobody else, and an unknown role fails closed.
                Action::RepeatOperation { .. } => match caller.role {
                    Role::Operator => {
                        operator_holds(caller, Permission::RepeatOperation).map(Needs::from)
                    }
                    Role::Core => Some(
                        vec![
                            listed(caller, Permission::CarryOnward),
                            listed(caller, Permission::RepeatOperation),
                        ]
                        .into(),
                    ),
                    Role::Customer
                    | Role::Provider
                    | Role::Verifier
                    | Role::Regulator
                    | Role::Checker => None,
                },
                // Asking a failed segment's provider to reverse it (`REQ-ORD-3`, `REQ-ORD-11`): an operator's login the
                // register lists for it, a permission of its own, or one of Core's own actors carrying the order on.
                // Which provider is asked, and that its segment owes the reversal, the log holds
                // (`reversal-is-asked-of-its-failed-provider`).
                // The customer's acceptance of a showing that a failure was its fault (`REQ-ORD-11`): the order's
                // requester, or its agent, the party the order's record says, and nobody else, not even the
                // operator, which accepts by its own `accept_fault` command. The showing must be one the log holds,
                // received no later than the acceptance is decided. That its session is open the register holds
                // (`Register::refuses_acceptance`). It is decided and never an effect
                // (`reversal-carrier-rests-on-its-acceptance`).
                Action::AcceptFault {
                    order,
                    segment,
                    showing,
                    ..
                } => match caller.role {
                    Role::Customer => Some(
                        vec![
                            requester(caller, order),
                            Need {
                                input: order.clone(),
                                from: Source::Showing {
                                    order: order.clone(),
                                    segment: segment.clone(),
                                    showing: showing.clone(),
                                    decided_at: request.at.clone(),
                                },
                            },
                        ]
                        .into(),
                    ),
                    Role::Operator
                    | Role::Provider
                    | Role::Verifier
                    | Role::Regulator
                    | Role::Checker
                    | Role::Core => None,
                },
                // Filing an arrival anew against its order (`REQ-ORD-23`, `T-020-52`): the filer is the caller itself, a
                // login of the deployment operator's the register lists for a permission of its own, or the reserve the
                // arrival came to, acting for itself, which the order's record says is its origin. Nobody else, Core's
                // own actors and the user included. That the arrival differed from this order is the log's
                // (`refiling-files-an-arrival-anew`); the refiling cites this decision
                // (`refiling-rests-on-policys-permit`). It is decided and never an effect.
                Action::RefileArrival { order, filer, .. } if filer == caller => {
                    match caller.role {
                        Role::Operator => of_operator_holds(caller, Permission::RefileArrival),
                        Role::Provider => Some(vec![origin(order, &caller.party)].into()),
                        Role::Customer
                        | Role::Verifier
                        | Role::Regulator
                        | Role::Checker
                        | Role::Core => None,
                    }
                }
                // A filer other than the caller is no one's act.
                Action::RefileArrival { .. } => None,
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
                    none: Vec::new(),
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
        // Granting or revoking a read permission (`REQ-AUZ-7`, `T-021-14`): an operator's login the register lists for
        // the one permission the call is, of the deployment operator by the register, in its party or reaching it,
        // and no other role. Who the grantee is, a regulator or a checker, its request's shape holds
        // (`PermissionChange::check`), and the register of actors binds the role it states to its login, as every
        // actor's ([`acts_for`]).
        // A grantee is never the operator's own (`T-021-14`, fix round 1): a regulator or a checker is outside the
        // operator, so the register's word that the grantee's login is the operator's refuses the grant, the grantor
        // granting itself among them.
        Asked::GrantPermission(change) => {
            of_operator_holds(&change.by, Permission::GrantRead).map(|mut needs| {
                needs.none.push(Need {
                    input: change.grantee.acts.clone(),
                    from: Source::OfOperator,
                });
                needs
            })
        }
        Asked::RevokePermission(change) => of_operator_holds(&change.by, Permission::RevokeRead),
        _ => Some(Needs::default()),
    }
}

/// The register's word that `caller` holds `permission`, asked only of a caller in the operator's role: no other
/// role holds one.
fn operator_holds(caller: &Actor, permission: Permission) -> Option<Vec<Need>> {
    (caller.role == Role::Operator).then(|| vec![listed(caller, permission)])
}

/// The register's word that `caller` holds `permission` and is the deployment operator's (`Source::OfOperator`), asked
/// only of a caller in the operator's role: a login of another party listed for it, or in the operator's role, is not
/// the operator's.
fn of_operator_holds(caller: &Actor, permission: Permission) -> Option<Needs> {
    operator_holds(caller, permission).map(|mut needs| {
        needs.push(Need {
            input: caller.acts.clone(),
            from: Source::OfOperator,
        });
        needs.into()
    })
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
    Reply::Given(
        needs
            .all
            .into_iter()
            .chain(needs.any)
            .chain(needs.none)
            .collect(),
    )
}

/// The fixed rule's ruling over what [`needs_of`] names: every input of `all` says yes and, where `any` names some, at
/// least one of them does, and every input of `none` says no. A no refuses, and an input not given is missing. A call no rule lets its caller make is
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
    for need in &needs.none {
        match said(need) {
            Some(Said::No) => {}
            Some(Said::Yes) => refused = true,
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
        Asked::DeclineOutcome(d) => vec![&d.decliner],
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
                role: actor.role,
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
