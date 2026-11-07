//! VOS as rutent's whole runtime (`EXP-1`'s re-score, `T-007-03`): the state machine is a VOS program
//! (`../program`), and this crate is its thin [`Host`](rutent_contracts::state_machine::host::Host) client, the
//! node processes it runs on, one node or three, the platform it asserts, and the rows it runs.
//!
//! Everything here is behind the `vos` feature, the flag that enables the candidate: without it the library is
//! empty and nothing of VOS is compiled or linked.

#[cfg(feature = "vos")]
pub mod cluster;
#[cfg(feature = "vos")]
pub mod host;
#[cfg(feature = "vos")]
pub mod platform;
