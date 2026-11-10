//! Core's order orchestration, component CMP-CORE-ORCH of the SAD.
//!
//! It takes an order from request to close, binding the router, the state machine, the provider system and the
//! evidence checker through their public contracts (`DEC-2`, `REQ-MOD-3`). It reaches policy and the log only through
//! the calls of `rutent_contracts::core_internal::calls`, never by importing `rutent-core-authz` or
//! `rutent-core-audit`.
//!
//! One module per row of `F-020` (`PLAN.md` §4.4), and `notice`, which `F-023` owns. Only a module's owning row
//! edits it.

pub mod arrival;
pub mod closing;
pub mod hand_on;
pub mod intake;
pub mod notice;
pub mod quote;
pub mod recalc;
pub mod reversal;
pub mod rounds;
pub mod segments;
pub mod snapshot;
pub mod stuck;
