//! EXP-2's PolkaVM binding of the module-boundary suite (`T-008-03`, `REQ-MDL-15`, `REQ-MDL-18`, `AC-20`).
//!
//! - [`binding::PvmBinding`] implements the suite's `ModuleBinding` and `BoundaryHarness` for PolkaVM programs.
//! - `rutent-pvm-worker` is a provider's worker process; `rutent-pvm-admit` the admitting compiler, in its own
//!   isolated process; `rutent-pvm-tool` builds the stand-ins; `rutent-pvm-suite` runs the suite and prints the
//!   report.
//! - [`engine`] holds every PolkaVM setting the binding makes, and the allow-lists admission checks each part's
//!   imports against.

pub mod binding;
pub mod egress;
pub mod engine;
pub mod isolation;
#[cfg(target_os = "linux")]
pub mod jail;
pub mod probe;
pub mod protocol;
pub mod sha256;
pub mod spec;
