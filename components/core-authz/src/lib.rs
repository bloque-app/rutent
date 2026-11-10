//! Core's policy and authorization engine, component CMP-CORE-AUTHZ of the SAD.
//!
//! It answers the policy calls of `rutent_contracts::core_internal::calls`, appending each answer to the log through
//! that contract's `Log` before it gives it. It imports neither `rutent-core-orch` nor `rutent-core-audit`
//! (`DEC-2`).
//!
//! One module per row of `F-021` (`PLAN.md` §4.4). Only a module's owning row edits it. Until `T-021-01` builds the
//! engine, [`engine::Unbuilt`] answers every call without permitting anything.

pub mod cpl;
pub mod engine;
pub mod funds;
pub mod parties;
pub mod roles;
pub mod session;
pub mod stuck;
