//! Resolving a stuck order: decide and move as separate permissions, their actors compared at the accountable party.
//!
//! Declared by `T-020-01`; `T-021-06` owns it, and only its owning row edits it (`PLAN.md` §4.4).

use rutent_contracts::core_internal::calls::{AppendFailure, PolicyLog, StuckOrders};
use rutent_contracts::core_internal::{
    Decision, OutcomeConfirmation, OutcomeReport, StuckDecide, StuckMove, What,
};

use crate::engine::Engine;

/// Until this row builds it, on every engine, so the assembly's reaches it as the unbuilt one does: no step of a stuck
/// order's resolution, nor a move's outcome reported or confirmed, is permitted. Each is recorded first and answered
/// not known.
impl<L: PolicyLog, P, S> StuckOrders for Engine<L, P, S> {
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

    fn report_outcome(&mut self, request: &OutcomeReport) -> Result<Decision, AppendFailure> {
        let what = |decision| What::ReportOutcome {
            request: request.clone(),
            decision,
        };
        self.not_known(&request.request, &request.reporter, what, &request.at)
    }

    fn confirm_outcome(
        &mut self,
        request: &OutcomeConfirmation,
    ) -> Result<Decision, AppendFailure> {
        let what = |decision| What::ConfirmOutcome {
            request: request.clone(),
            decision,
        };
        self.not_known(&request.request, &request.confirmer, what, &request.at)
    }
}
