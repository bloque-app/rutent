//! Core's authenticated log and checkpoints, component CMP-CORE-AUDIT of the SAD.
//!
//! It implements the log and its checks of `rutent_contracts::core_internal::calls`, and raises differences to the
//! operator through that contract's `Operator`. It imports neither `rutent-core-orch` nor `rutent-core-authz`
//! (`DEC-2`).
//!
//! One module per row of `F-022` (`PLAN.md` §4.4). Only a module's owning row edits it. [`log::AuditLog`] is the
//! log (`T-022-01`); [`log::Unbuilt`], which takes no entry, stays the stub the other rows' traits sit on until they
//! are built, so nothing that waits on one of their receipts takes effect.

pub mod checkpoint;
pub mod compare;
pub mod log;
pub mod measures;
pub mod receipts;
pub mod statements;
