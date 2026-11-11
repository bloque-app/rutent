//! Resolving a stuck order: decide and move as separate permissions, their actors compared at the accountable party.
//!
//! Declared by `T-020-01`; `T-021-06` owns it, and only its owning row edits it (`PLAN.md` §4.4).

use rutent_contracts::core_internal::calls::{AppendFailure, PolicyLog, StuckOrders};
use rutent_contracts::core_internal::{Decision, StuckDecide, StuckMove, What};

use crate::engine::Unbuilt;

/// Until this row builds it: neither step of a stuck order's resolution is permitted.
impl<L: PolicyLog> StuckOrders for Unbuilt<L> {
    fn decide_stuck(&mut self, request: &StuckDecide) -> Result<Decision, AppendFailure> {
        request.check().map_err(AppendFailure::Refused)?;
        let what = |decision| What::StuckDecide {
            request: request.clone(),
            decision,
        };
        self.not_known(&request.request, &request.decider, what, &request.at)
    }

    fn move_stuck(&mut self, request: &StuckMove) -> Result<Decision, AppendFailure> {
        let what = |decision| What::StuckMove {
            request: request.clone(),
            decision,
        };
        self.not_known(&request.request, &request.mover, what, &request.at)
    }
}
