//! The contracts: the schemas and shared shapes every component meets the others through (DEC-2, DEC-4).
#![no_std]
extern crate alloc;

#[cfg(any())]
pub mod module_interface;
pub mod provider_role;
pub mod router_api;
pub mod state_machine;
