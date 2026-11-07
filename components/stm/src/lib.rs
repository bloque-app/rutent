//! The order and segment state machine, component CMP-STM of the SAD.
//!
//! The purpose-built store of `EXP-1` (`T-007-02`), a host of the host contract on real disk, is built only with
//! the `purpose-built-store` feature, off by default.

#[cfg(feature = "purpose-built-store")]
pub mod purpose_built_store;
pub mod reference_host;
