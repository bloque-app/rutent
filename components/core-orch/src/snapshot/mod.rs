//! The snapshot Core hands the router: the catalogue read at Core's own clock, exposures from Core's records, the
//! `calls` figure, and each statement tied to its maker's key before a record is formed.
//!
//! Declared by `T-020-01`; `T-020-05` owns it, and only its owning row edits it (`PLAN.md` §4.4).
