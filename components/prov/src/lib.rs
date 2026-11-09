//! The provider system, component CMP-PROV of the SAD.
//!
//! It imports nothing of ours but `contracts/` (`DEC-2`). The package is split by module path, one feature to
//! each (`PLAN.md` section 4.3):
//!
//! - [`catalogue`]: grants, bounds, the publisher behind each provider and node, declarations, and reversal
//!   capacity (`F-012`).
//! - [`module`]: module admission, runtime and lifecycle (`F-013`).
//! - [`proxy`]: the shared credential proxy and its resolvers (`F-014`).
//! - [`egress`]: the outside part's egress (`F-014`).
//! - [`holding`]: holding-account modules at public nodes (`F-016`).
//!
//! This file belongs to `T-012-01`; a later feature fills its own module path and edits nothing here.

pub mod catalogue;
pub mod egress;
pub mod holding;
pub mod module;
pub mod proxy;
