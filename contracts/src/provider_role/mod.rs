//! The provider role contract: what a provider advertises and declares to join the catalogue (`REQ-CAT-8`,
//! `DEC-4`).
//!
//! The language-neutral artefacts are the JSON Schemas under `contracts/provider-role/schemas/`. The types here
//! mirror them with `serde`, and [`validate`] enforces the rules a schema can't express, such as a ramp's two
//! nodes being different. `contracts/provider-role/README.md` lists which rule lives where.

mod types;
mod validate;

pub use types::*;
pub use validate::{RuleViolation, validate};

/// The contract's version: the first heading of `contracts/provider-role/CHANGELOG.md`.
pub const VERSION: &str = "0.2.0";
