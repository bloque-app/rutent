//! A separate file of the no_std port (program/SEPARATE): the crate root around the generated copy of the
//! host-contract suite's probe, `probe.rs`, which in `conformance` is the module `host::probe`.

#![no_std]
extern crate alloc;

mod probe;

pub use probe::*;
