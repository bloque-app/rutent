//! A linked order's acceptance, handed to Core (`T-024-17`; `REQ-ORD-43`, `REQ-ORD-34`; `T-020-61` item 10).
//!
//! Core accepts a linked order itself ([`rutent_core_orch::intake::linked::accept_linked`]): it routes it from the node
//! with the rounds every order runs, plans it, keeps the customer's inputs, shows the quote, takes the customer's
//! acceptance on the starting provider's yes and asks policy for the authorisation under the customer's own floor. The
//! assembly hands the customer's request to that entry point over the deployment's ports, lending Core's own register
//! of linked routes, where Core keeps the route the order was accepted on for the mover to hold a hand-on's route to
//! (`T-020-68`): the assembly writes nothing there. It starts nothing: the order's `route_chosen` is the hand-on's,
//! recorded by Core's run once the value is handed on.

pub use rutent_core_orch::intake::linked::{AcceptedLinked, LinkedError, LinkedOrder};

use super::{Core, StepError};

impl Core {
    /// Hands the customer's acceptance of `linked` to Core's entry point, which keeps the route it was accepted on.
    ///
    /// # Errors
    ///
    /// As the entry point, each as [`StepError::NotNow`].
    pub fn accept_linked(&mut self, linked: &LinkedOrder) -> Result<AcceptedLinked, StepError> {
        let accepted = rutent_core_orch::intake::linked::accept_linked(&mut self.ports(), linked)
            .map_err(|why| {
            StepError::NotNow(match why {
                LinkedError::NotStarted => "the order handed on started",
                LinkedError::NoRoute(_) => "the rounds offer the linked order a route",
                LinkedError::NotLinked => "the linked route starts at an inner node",
                LinkedError::NoYes => "the starting reserve's answer is the log's",
                LinkedError::NoParties => "the log holds the order's acceptance",
                LinkedError::NotAuthorised => "policy authorises the linked order",
                LinkedError::Unrecorded(what) => what,
            })
        })?;
        self.feed()?;
        Ok(accepted)
    }
}
