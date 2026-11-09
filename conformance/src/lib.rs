//! The conformance suites that check implementations against the contracts (DEC-2).

#[cfg(feature = "signing")]
pub mod boundary;
pub mod host;
pub mod role;
pub mod router;
#[cfg(feature = "signing")]
pub mod signing;
