//! The order and segment state machine, component CMP-STM of the SAD.
//!
//! The library is `no_std` + `alloc` (`DEC-4`, `DEC-26`), so the VOS program can run it: build it with
//! `default-features = false` for such a target. It imports nothing of rutent's but `contracts/` (`DEC-2`).
//!
//! - [`Stm`] is the contract's [`StateMachine`](rutent_contracts::state_machine::StateMachine): `open` and
//!   `transition`, pure and deterministic, over the published state lists and the published rules of the
//!   contract.
//! - [`order_state_of`] is how an order's state follows from its parts' states.
//! - [`handed_on_route_starts_where_value_sits`] is the start rule across two orders, a function Core calls with
//!   both.
//!
//! No transition reads an amount: which amount the next segment moves, and the remainder of an `exact_out`,
//! are `F-020`'s.
//!
//! The in-memory reference host of `T-005-04` is built with the `std` feature, and the purpose-built store of
//! `EXP-1` (`T-007-02`) only with the `purpose-built-store` feature, off by default.
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod hand_on;
mod machine;
mod parts;
#[cfg(feature = "purpose-built-store")]
pub mod purpose_built_store;
#[cfg(feature = "std")]
pub mod reference_host;

pub use hand_on::{HandOnRefusal, handed_on_route_starts_where_value_sits};
pub use machine::Stm;
pub use parts::{is_ended, order_state_of};
