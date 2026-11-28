//! The operator's decided commands that resolve a stuck order, as Core's own entry points (`T-020-61`, item 4;
//! `REQ-AUZ-4`, `REQ-AUZ-1`, `AC-6`): the deployment hands a command to Core over the same ports Core's schedule runs on
//! ([`Ports`]), and every step is Core's, through policy.
//!
//! - [`decide_stuck`]: the decide step of an order's stuck part, asked of policy ([`Stuck::decide`](super::Stuck::decide)):
//!   the part is the order's, as Core started it, never the caller's to name.
//! - [`move_stuck`]: the move step, carrying a decided resolution out through the production mover, policy deciding it
//!   and the effect recorded before the platform is asked ([`Stuck::carry_out`](super::Stuck::carry_out)). A hand-on's
//!   withdrawal done this way is handed on, and its linked order run, by Core's own run on its next tick.
//!
//! No single actor both decides the outcome and moves value: that is `Stuck`'s rule and policy's, held here as
//! everywhere.

use rutent_contracts::core_internal::calls::AppendFailure;
use rutent_contracts::core_internal::{Actor, Decided, Decision, Resolution};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::router_api::Id;

use super::{Deciding, Moved, Moving, ResolveError};
use crate::schedule::Ports;

/// The decide step of `order`'s stuck part, by `decider`, naming `resolution` and the evidence relied on, asked of policy
/// (`REQ-AUZ-4`). The part is the one of the order, as Core started it, that Core holds stuck: any part of a split order.
///
/// # Errors
///
/// [`ResolveError::NotStuck`] where Core started no such order; otherwise as [`Stuck::decide`](super::Stuck::decide).
pub fn decide_stuck(
    ports: &mut dyn Ports,
    order: &Id,
    decider: &Actor,
    resolution: &Resolution,
    evidence: &[Digest],
) -> Result<Decision, ResolveError> {
    // The part Core holds stuck, of the order as Core started it: any part of a split order, never one the caller names.
    let part = ports
        .intake()
        .started(order)
        .map(crate::intake::Started::parts)
        .unwrap_or_default()
        .into_iter()
        .map(|placed| placed.part)
        .find(|part| ports.stuck().is_stuck(order, part))
        .ok_or(ResolveError::NotStuck)?;
    let history = ports.history();
    let mut decided = Err(ResolveError::Append(AppendFailure::Unavailable));
    ports.with_stuck(&mut |stuck, parts, policy, _| {
        decided = stuck.decide(
            parts,
            policy,
            Deciding {
                order,
                part: &part,
                decider,
                resolution,
                evidence,
            },
            &history,
        );
    });
    ports.fed();
    decided
}

/// The move step of `order`'s stuck part, by `mover`, carrying out `carries_out`, the decide step's decision: policy
/// decides it, and the production mover hands the platform the effect it records (`REQ-AUZ-4`).
///
/// # Errors
///
/// As [`Stuck::carry_out`](super::Stuck::carry_out); [`ResolveError::Append`] where the deployment lent nothing.
pub fn move_stuck(
    ports: &mut dyn Ports,
    order: &Id,
    carries_out: &Decided,
    mover: &Actor,
) -> Result<Moved, ResolveError> {
    let core = ports.actors().core;
    let mut moved = Err(ResolveError::Append(AppendFailure::Unavailable));
    ports.with_mover(&mut |stuck, parts, policy, delivering| {
        moved = stuck.carry_out(
            parts,
            policy,
            delivering,
            Moving {
                order,
                carries_out,
                mover,
                core: &core,
            },
        );
    });
    ports.fed();
    moved
}
