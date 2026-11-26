//! Checks over the project's records: the tag check (`DEC-21`), the plan check (`PLAN.md` §5) and the
//! requirement-to-evidence table (`T-024-09`).
//!
//! The names check (`T-024-24`) holds the tests the plan names to the tests the code has.
//!
//! All read plain text with `std` alone. They share one reading of which identifiers the documents define, so
//! a test tag and a task row are held to the same list.

pub mod evidence;
pub mod ids;
pub mod lints;
pub mod names;
pub mod plan;
mod table;
pub mod tags;

use std::fmt;

/// One thing a check reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// A short, stable name for the rule that was broken.
    pub rule: &'static str,
    /// Where it was found: `path:line` for source, a task or wave for the plan.
    pub location: String,
    /// What is wrong, in a sentence.
    pub message: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}: {}", self.rule, self.location, self.message)
    }
}
