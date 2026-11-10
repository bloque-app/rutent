//! Agents and accountable parties: the limits on work and on asking about people bind at the party.
//!
//! Declared by `T-020-01`; `T-021-05` owns it, and only its owning row edits it (`PLAN.md` §4.4).

use rutent_contracts::core_internal::calls::{AppendFailure, Charges, Log};
use rutent_contracts::core_internal::{Charge, ChargeAnswer, ChargeOutcome, What};
use rutent_contracts::provider_role::SafeInteger;

use crate::engine::Unbuilt;

/// Until this row builds it: every party's bound is 0, so no work is done.
impl<L: Log> Charges for Unbuilt<L> {
    fn charge(&mut self, charge: &Charge) -> Result<ChargeAnswer, AppendFailure> {
        charge.check().map_err(AppendFailure::Refused)?;
        let bound = SafeInteger::try_from(0).expect("0 is a safe integer");
        let outcome = ChargeOutcome::Over { bound };
        let what = What::Charge {
            charge: charge.clone(),
        };
        let receipt = self.record(&charge.caller, what, &charge.at, outcome.outcome())?;
        Ok(ChargeAnswer {
            request: charge.request.clone(),
            outcome,
            receipt,
        })
    }
}
