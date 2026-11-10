//! The log: a Merkle log in `CMP-CORE-AUDIT`'s own store, whose append answers before the effect it records.
//!
//! Declared by `T-020-01`; `T-022-01` owns it, and only its owning row edits it (`PLAN.md` §4.4).

use rutent_contracts::core_internal::Entry;
use rutent_contracts::core_internal::Receipt;
use rutent_contracts::core_internal::calls::{AppendFailure, Log};

/// The log and its checks before their rows build them: they take no entry, keep no evidence and record no check,
/// answering each as unavailable, so no action that waits on a receipt takes effect (`INV-7`). Each trait's stub
/// sits in the module of the row that owns that trait; this one, and its [`Log`], are `T-022-01`'s.
#[derive(Debug, Clone, Copy, Default)]
pub struct Unbuilt;

impl Log for Unbuilt {
    fn append(&mut self, _entry: &Entry) -> Result<Receipt, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    /// A valid fixture of the contract, read as whatever type the call site wants.
    macro_rules! fixture {
        ($name:expr $(,)?) => {{
            let path = format!(
                "{}/../../contracts/core-internal/fixtures/valid/{}.json",
                env!("CARGO_MANIFEST_DIR"),
                $name
            );
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
        }};
    }

    use super::*;
    use rutent_contracts::core_internal::calls::{Comparisons, Evidence, Measures};

    /// The unbuilt log answers every call unavailable, so it hands out no receipt an effect could rest on.
    // verifies: INV-7
    #[test]
    fn the_unbuilt_log_takes_nothing_and_gives_no_receipt() {
        let unavailable = Err(AppendFailure::Unavailable);
        let mut log = Unbuilt;
        assert_eq!(log.append(&fixture!("entry-verified")), unavailable);
        assert_eq!(
            log.report_pair(&fixture!("report-pair-of-a-route-segment")),
            unavailable
        );
        assert_eq!(
            log.holding_reading(&fixture!("holding-reading-at-a-private-node")),
            unavailable
        );
        assert_eq!(
            log.measure(&fixture!("measure-record-of-a-quote")),
            unavailable
        );
        assert_eq!(log.keep(b"{}"), Err(AppendFailure::Unavailable));
        assert_eq!(
            log.fetch(&fixture!("read-check-by-a-party-of-its-own-evidence")),
            Err(AppendFailure::Unavailable)
        );
    }
}
