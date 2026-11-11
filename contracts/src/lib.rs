//! The contracts: the schemas and shared shapes every component meets the others through (DEC-2, DEC-4).
//!
//! The crate is `no_std` with `alloc`. Its default `std` feature only turns on `std` for the dependencies and for
//! the tests; nothing here needs it (DEC-26, `REQ-OPS-3`).
#![no_std]

extern crate alloc;
#[cfg(any(feature = "std", test))]
extern crate std;

pub mod module_interface;
pub mod provider_role;
pub mod router_api;
pub mod state_machine;
