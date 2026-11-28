//! The inbound commands and statements that end an order or answer a failed segment's fault, as Core's own entry points
//! (`T-020-61`, item 10; `REQ-ORD-11`, `REQ-ORD-12`, `REQ-ORD-25`, `REQ-ORD-31`, `REQ-STM-6`): the deployment hands each
//! to Core over the same ports Core's schedule runs on ([`Ports`]), as `stuck::command` does the stuck commands, and every
//! step is Core's. The reversal a failed segment owes is never a command: Core's run owes, asks and confirms it on its
//! ticks ([`crate::schedule::run`]).
//!
//! - [`end`]: the customer's request to end an order without completing (`REQ-ORD-12`): policy's `end_order`, which
//!   [`super::Reversals::end`] asks itself before anything of the order is told, the order's requester in the customer's
//!   role or an operator's login holding the permission.
//! - [`show_fault`], [`accept_fault`], [`dispute`]: the failed provider's showing that the fault was the customer's or the
//!   operator's, and the named party's acceptance or dispute of it (`REQ-ORD-11`). The customer's acceptance is policy's
//!   permitted decision of its `accept_fault` in its open session, which Core asks itself and then reads from its own
//!   log; the operator's is its command, bound by the register, on policy's permitted decision of the login's own
//!   `accept_fault`, which Core asks first under the operator's permission (`T-020-68`). A dispute is policy's
//!   `dispute_fault`, which [`super::Reversals::dispute`] asks itself.
//! - [`returned`], [`late_approval`], [`report_late`]: statements that arrive after an order ended or a segment's answer
//!   was taken, kept as their maker's and moving nothing (`REQ-ORD-31`, `REQ-ORD-25`, `REQ-STM-6`).
//! - [`confirm_undone`]: the question whether a complete part's start was undone, asked of its origin reserve where a
//!   report says so (`AC-8`): its answer is the reserve's own word, and a proven undoing marks the part failed.

use rutent_contracts::core_internal::{
    Action, Actor, DecisionRequest, Entry, Receipt, Role, ShowingNamed,
};
use rutent_contracts::router_api::Id;

use super::{Accepting, Afterward, At, Carrier, Confirmation, ReversalError};
use crate::schedule::Ports;

/// The part of `order` that `segment` is of, as the segments follow it.
fn part_of(ports: &dyn Ports, order: &Id, segment: &Id) -> Result<Id, ReversalError> {
    ports
        .segments()
        .order(order)
        .ok_or(ReversalError::Unknown)?
        .parts
        .iter()
        .find(|p| {
            p.segments
                .iter()
                .any(|s| s.segment_id.as_str() == segment.as_str())
        })
        .and_then(|p| Id::new(p.part_id.as_str()))
        .ok_or(ReversalError::Unknown)
}

/// Runs `with` over the reversals the deployment lends, followed from the log first; [`ReversalError::Unknown`] where
/// it lent nothing.
fn lent<T>(
    ports: &mut dyn Ports,
    order: &Id,
    with: &mut dyn FnMut(
        &mut super::Reversals,
        &mut super::Parts<'_>,
        &mut crate::schedule::ReversalDesk<'_>,
    ) -> Result<T, ReversalError>,
) -> Result<T, ReversalError> {
    let history = ports.history();
    let mut done = Err(ReversalError::Unknown);
    ports.with_reversal_desk(&mut |reversals, parts, desk| {
        reversals.follow(order, &history);
        done = with(reversals, parts, desk);
    });
    ports.fed();
    done
}

/// Asks policy to decide `action`, asked by `by`: policy appends its decision to the log, and only its permit goes on.
///
/// # Errors
///
/// [`ReversalError::NotAnAcceptance`] where policy couldn't decide; [`ReversalError::NotPermitted`] where it refused.
fn decided(ports: &mut dyn Ports, by: &Actor, action: Action) -> Result<(), ReversalError> {
    let request = DecisionRequest {
        request: ports.fresh(),
        caller: by.clone(),
        action,
        at: super::journal::now(ports.clock()).map_err(ReversalError::Append)?,
    };
    let mut decided = Err(ReversalError::Unknown);
    ports.with_reversal_desk(&mut |_, _, desk| {
        decided = desk
            .decisions
            .decide(&request)
            .map_err(|_| ReversalError::NotAnAcceptance);
    });
    ports.fed();
    let decided = decided?;
    if decided.permit().is_none() {
        return Err(ReversalError::NotPermitted(decided.outcome));
    }
    Ok(())
}

/// Ends `order` without completing, on `by`'s request, as policy decides its `end_order` (`Action::EndOrder`): the
/// order's requester in the customer's role, or an operator's login holding the permission (see
/// [`super::Reversals::end`], which asks it).
///
/// # Errors
///
/// Where policy doesn't permit it, before anything of the order; otherwise as [`super::Reversals::end`].
pub fn end(ports: &mut dyn Ports, order: &Id, by: &Actor) -> Result<Receipt, ReversalError> {
    lent(ports, order, &mut |reversals, parts, desk| {
        reversals.end(parts, &mut *desk.decisions, order, by)
    })
}

/// Hands Core the failed provider's showing that the failure of `segment` of `order` was not its own: its signed failed
/// result for the segment's own transaction, naming the customer's fault or the operator's (see
/// [`super::Reversals::show_fault`]).
///
/// # Errors
///
/// As [`super::Reversals::show_fault`].
pub fn show_fault(
    ports: &mut dyn Ports,
    order: &Id,
    segment: &Id,
    bytes: &[u8],
) -> Result<Receipt, ReversalError> {
    let part = part_of(ports, order, segment)?;
    lent(ports, order, &mut |reversals, parts, _| {
        reversals.show_fault(
            parts,
            At {
                order,
                part: &part,
                segment,
            },
            bytes,
        )
    })
}

/// The party the showing of `segment` names, `by`, accepting it: the customer through policy's decision of its
/// `accept_fault` in `session`, which Core asks and then reads from its own log; the operator by its command, bound by
/// the register of actors, once policy permitted the login's own `accept_fault`, which names no session and which Core
/// asks first (`T-020-68`; see [`super::Reversals::accept_fault`]).
///
/// # Errors
///
/// [`ReversalError::NoShowing`] where the log holds no showing of the segment; [`ReversalError::NotPermitted`] where
/// policy didn't permit the acceptance; otherwise as [`super::Reversals::accept_fault`].
pub fn accept_fault(
    ports: &mut dyn Ports,
    order: &Id,
    segment: &Id,
    by: &Actor,
    session: &Id,
) -> Result<Carrier, ReversalError> {
    let part = part_of(ports, order, segment)?;
    let customer = by.role == Role::Customer;
    // Policy decides either party's acceptance, naming the showing the log holds (`T-020-68`): the customer's in its
    // session, the operator's in none, under the operator's permission.
    let action = {
        let showing = ports
            .history()
            .iter()
            .rev()
            .find_map(|(_, e)| match e {
                Entry::Stated(s)
                    if s.what.statement.as_str()
                        == rutent_contracts::provider_role::RESULT_SHAPE
                        && s.about.as_ref().and_then(|a| a.order.as_ref()) == Some(order)
                        && s.about.as_ref().and_then(|a| a.segment.as_ref()) == Some(segment) =>
                {
                    Some(s.clone())
                }
                _ => None,
            })
            .ok_or(ReversalError::NoShowing)?;
        let transaction = showing
            .about
            .as_ref()
            .and_then(|a| a.transaction.clone())
            .ok_or(ReversalError::NoShowing)?;
        Action::AcceptFault {
            order: order.clone(),
            segment: segment.clone(),
            showing: ShowingNamed {
                transaction,
                commitment: showing.commitment,
            },
            session: customer.then(|| session.clone()),
        }
    };
    // Policy appends its decision to the log, where the reversals find it: asked first, then the log read again.
    decided(ports, by, action)?;
    lent(ports, order, &mut |reversals, parts, desk| {
        reversals.accept_fault(
            parts,
            &mut *desk.actors,
            At {
                order,
                part: &part,
                segment,
            },
            if customer {
                Accepting::Customer { by }
            } else {
                Accepting::Operator { by }
            },
        )
    })
}

/// The party the showing of `segment` names, `by`, disputing it, as policy decides its `dispute_fault`
/// (`Action::DisputeFault`), which [`super::Reversals::dispute`] asks: the part is declared stuck, in a dispute, through
/// the stuck orders, holding `by` to the party the showing names.
///
/// # Errors
///
/// Where policy doesn't permit it; otherwise as [`super::Reversals::dispute`].
pub fn dispute(
    ports: &mut dyn Ports,
    order: &Id,
    segment: &Id,
    by: &Actor,
) -> Result<Receipt, ReversalError> {
    let part = part_of(ports, order, segment)?;
    lent(ports, order, &mut |reversals, parts, desk| {
        reversals.dispute(
            parts,
            &mut *desk.decisions,
            &mut *desk.stuck,
            At {
                order,
                part: &part,
                segment,
            },
            by,
        )
    })
}

/// Records that `order`, complete, was returned: `bytes`, its destination reserve's report (see
/// [`super::Reversals::returned`]).
///
/// # Errors
///
/// As [`super::Reversals::returned`].
pub fn returned(
    ports: &mut dyn Ports,
    order: &Id,
    bytes: &[u8],
) -> Result<Afterward, ReversalError> {
    lent(ports, order, &mut |reversals, parts, _| {
        reversals.returned(parts, order, bytes)
    })
}

/// Records an origin approval that arrived after `order` ended: `bytes`, its origin reserve's statement (see
/// [`super::Reversals::late_approval`]).
///
/// # Errors
///
/// As [`super::Reversals::late_approval`].
pub fn late_approval(
    ports: &mut dyn Ports,
    order: &Id,
    bytes: &[u8],
) -> Result<Afterward, ReversalError> {
    lent(ports, order, &mut |reversals, parts, _| {
        reversals.late_approval(parts, order, bytes)
    })
}

/// Keeps `bytes`, a provider's result about `segment` of `order` arriving after its answer was taken, as its maker's
/// statement, moving nothing; `presented_by` is who handed it to Core, as the deployment's transport authenticated it
/// (see [`super::Reversals::later`]).
///
/// # Errors
///
/// As [`super::Reversals::later`].
pub fn report_late(
    ports: &mut dyn Ports,
    order: &Id,
    segment: &Id,
    bytes: &[u8],
    presented_by: &Actor,
) -> Result<Receipt, ReversalError> {
    let part = part_of(ports, order, segment)?;
    lent(ports, order, &mut |reversals, parts, _| {
        reversals.later(
            parts,
            At {
                order,
                part: &part,
                segment,
            },
            bytes,
            presented_by,
        )
    })
}

/// Asks the origin reserve of the part `withdrawal` starts whether its start was undone, where a report says so, and
/// marks the part failed where its answer proves it (`AC-8`, see [`super::Reversals::confirm_reversal`]). The question is signed
/// under the bounds of the route the order runs on now.
///
/// # Errors
///
/// [`ReversalError::Unknown`] where Core started no such order; otherwise as [`super::Reversals::confirm_reversal`].
pub fn confirm_undone(
    ports: &mut dyn Ports,
    order: &Id,
    withdrawal: &Id,
) -> Result<Confirmation, ReversalError> {
    let part = part_of(ports, order, withdrawal)?;
    let bounds = ports
        .intake()
        .started(order)
        .map(|s| s.record.bounds.clone())
        .ok_or(ReversalError::Unknown)?;
    lent(ports, order, &mut |reversals, parts, desk| {
        let mut asking = super::Asking {
            bounds: &bounds,
            key: &mut *desk.key,
            providers: desk.platform.providers(),
            records: &desk.records,
            closing: desk.closing,
        };
        reversals.confirm_reversal(
            parts,
            &mut asking,
            At {
                order,
                part: &part,
                segment: withdrawal,
            },
        )
    })
}
