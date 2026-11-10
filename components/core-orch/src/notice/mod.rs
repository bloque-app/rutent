//! Notices: every change of an order's state notified to its originating user and the operator, and delivery to the
//! operator watched (`T-023-01`, `T-023-02`). `F-023` owns this module.
//!
//! Declared by `T-020-01`; only `F-023`'s rows edit it (`PLAN.md` §4.4).

use rutent_contracts::core_internal::Raise;
use rutent_contracts::core_internal::calls::{Delivery, Operator};

/// The operator's channel before `F-023` builds it: it delivers nothing, and says so, so every raise stays
/// recorded as undelivered.
#[derive(Debug, Clone, Copy, Default)]
pub struct Unbuilt;

impl Operator for Unbuilt {
    fn raise(&mut self, _raise: &Raise) -> Delivery {
        Delivery::Undelivered
    }
}
