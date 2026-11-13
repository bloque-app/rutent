//! Core's policy and authorization engine, component CMP-CORE-AUTHZ of the SAD.
//!
//! It answers the policy calls of `rutent_contracts::core_internal::calls`, appending each answer to the log through
//! that contract's `PolicyLog`, policy's handle on the log, which the assembly hands to it alone, before it gives it.
//! It imports neither `rutent-core-orch` nor `rutent-core-audit` (`DEC-2`).
//!
//! One module per row of `F-021` (`PLAN.md` §4.4). Only a module's owning row edits it. [`engine::Engine`] decides on
//! the fixed rules and the deployment's policy, and every policy call is built on it, each in its row's module;
//! [`engine::Unbuilt`] is the engine with no policy service and no sources, which permits nothing.

pub mod cpl;
pub mod engine;
pub mod funds;
pub mod parties;
pub mod roles;
pub mod session;
pub mod stuck;
