//! The scenario assembly: Core's three parts put together, with the libraries, against simulated providers, to run
//! `M3`'s scenarios (`PLAN.md` §4.4).
//!
//! Core's three crates import none of one another (`DEC-2`); this package is where they meet, each handed the
//! others' calls of `rutent_contracts::core_internal::calls`. It may import every crate of ours, and nothing in the
//! workspace imports it. `T-020-01` left it empty; `F-024` owns it.
