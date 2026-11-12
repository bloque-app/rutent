//! How an order's state follows from its parts' states (`REQ-ORD-42`, `REQ-STM-1`, `REQ-STM-6`): the contract's
//! published rules, re-exported so `CMP-STM`'s callers keep reading them here (`T-020-01`).

pub use rutent_contracts::state_machine::rules::{is_ended, order_state_of};
