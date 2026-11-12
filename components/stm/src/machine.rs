//! The transition function: [`Stm`], the contract's [`StateMachine`].
//!
//! # The states a segment moves through
//!
//! A segment only moves forward, along these edges and no others, so an input delivered again, or out of order,
//! finds the segment where it can't apply and changes nothing (`REQ-STM-4`, `INV-8`):
//!
//! ```text
//! pending -> started -> settled
//!                    -> failed_reversal_owed -> reversal_complete
//!                    -> outcome_unknown -> settled | failed_reversal_owed | reversal_complete
//! settled -> reversal_complete          (the starting segment only: the part, and so the order, fails)
//! ```
//!
//! A deposit has no `reversal_completed`: a failed deposit leaves the value where it was, at its `from` node
//! (`INV-2`, `REQ-ORD-43`), so it owes nothing back. Elapsed time is not an input of any of these (`REQ-STM-5`).
//!
//! # One source for the rules
//!
//! Every rule is the contract's, published in [`rules`](rutent_contracts::state_machine::rules), and so is the one
//! dispatch table: `transition` is the contract's `rules::transition`, which core-internal's reading of a part's
//! standing steps by too, so the two can't drift apart (`T-020-01`).
//!
//! No transition reads an amount.

use rutent_contracts::state_machine::rules::{open, transition};
use rutent_contracts::state_machine::{Order, RecordedInput, StateMachine};

/// The state machine of rutent's orders: `open` from the input that accepted a route, `transition` on every
/// recorded input after. Pure and deterministic: no clock, no randomness, no I/O, no amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stm;

impl StateMachine for Stm {
    fn open(input: &RecordedInput) -> Option<Order> {
        open(input)
    }

    fn transition(state: &Order, input: &RecordedInput) -> Order {
        transition(state, input)
    }
}
