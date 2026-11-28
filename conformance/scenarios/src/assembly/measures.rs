//! The bulk measures' writer on Core's log (`T-022-06`): the one door by which a measure record reaches the assembled
//! Core's log, since nothing outside this crate holds a handle on it (the crate's docs).
//!
//! It hands out `CMP-CORE-AUDIT`'s [`Record`], whose type is its reach: a general handle on the log, which takes no
//! decision entry and attributes no statement, and Core's clock. Orchestration's own caller of it is
//! Core's schedule (`T-020-31`, `T-020-48`): it hands the writer the records the segments' endings and the completed
//! reversals gave, which [`rutent_core_orch::segments::Segments`] keeps. A scenario that writes a record by hand
//! (`T-022-06`'s counting test) still can, through [`Core::measure_writer`].

use rutent_core_audit::log::General;
use rutent_core_audit::measures::Record;

use super::{Core, CoreLog};
use crate::kit::{self, SharedClock};

impl Core {
    /// The writer of measure records, on a general handle on Core's log and on Core's clock.
    pub fn measure_writer(&self) -> Record<CoreLog, SharedClock> {
        Record::new(
            Record::<CoreLog, SharedClock>::core_audit(kit::id(kit::CORE_PARTY)),
            General::new(&self.log),
            self.clock.clone(),
        )
    }
}
