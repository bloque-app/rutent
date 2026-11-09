//! The thin `Host` client of the VOS program (`T-010-05`, `DEC-26`): `CMP-STM` on VOS, one node or three.
//!
//! This package links VOS and is AGPL-3.0-or-later (`vos-licence-v0`); `rutent-stm` is not. Everything that links
//! VOS is behind the `vos` feature, off by default: without it the library holds only [`retry`] and nothing of VOS
//! is compiled or linked.

#[cfg(feature = "vos")]
pub mod cluster;
#[cfg(feature = "vos")]
pub mod host;
#[cfg(feature = "vos")]
pub mod platform;
pub mod retry;
