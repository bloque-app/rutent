//! The simulated providers, their world and the platform they are admitted to (`T-024-01`).
//!
//! - [`disk`]: the platform's two executables and the simulated providers' components, built when first needed.
//! - [`world`]: [`SimWorld`], the platform's world: scripts, journals, simulated public networks and the rails.
//! - [`orders`]: an order's state machine, fed as a host feeds it.
//! - [`fleet`]: [`Fleet`], a platform with simulated providers admitted to it like any module.
//!
//! What the providers are, and what a script can make them do, is `rutent_conformance::sim`.

pub mod disk;
pub mod fleet;
pub mod orders;
pub mod world;

pub use fleet::{Fleet, InProcess, Installed};
pub use world::SimWorld;
