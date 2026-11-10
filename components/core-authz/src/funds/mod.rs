//! The authorisation to move funds: one order, its destination, its to asset and any floor, and a recalculated route
//! only inside it.
//!
//! Declared by `T-020-01`; `T-021-04` owns it, and only its owning row edits it (`PLAN.md` §4.4).

use rutent_contracts::core_internal::calls::{AppendFailure, Authorisations, Log};
use rutent_contracts::core_internal::{Authorisation, Decision, What};

use crate::engine::Unbuilt;

/// Until this row builds it: no authorisation is registered, so none can be acted under.
impl<L: Log> Authorisations for Unbuilt<L> {
    fn authorise(&mut self, authorisation: &Authorisation) -> Result<Decision, AppendFailure> {
        let a = authorisation;
        let what = |decision| What::Authorise {
            authorisation: a.clone(),
            decision,
        };
        self.not_known(&a.authorisation, &a.given_by, what, &a.at)
    }
}
