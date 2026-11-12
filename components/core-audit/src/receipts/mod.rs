//! Receipts, and evidence apart from the log (ADR-0005, `REQ-AUD-5`, `REQ-AUD-13`, `T-022-03`).
//!
//! Declared by `T-020-01`; `T-022-03` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! - [`EvidenceStore`] keeps each statement's bytes as received, apart from the log, named from it by a salted
//!   commitment (`EXP-1`'s `G-10`), durably on a [`DirectoryShelf`]; it answers the log through [`LogsRead`]
//!   ([`KeptStatements`](rutent_contracts::core_internal::calls::KeptStatements)) and orchestration and the parties
//!   through [`Keeper`] ([`Evidence`]), whose reads policy allows first. It replaces `T-022-01`'s stand-in,
//!   `log::KeptInMemory`, with no change to the log.
//! - [`EvidenceStore::bind`] binds the store, once, to the log it serves, reconciling the two; [`BoundStore::erase`]
//!   then erases, outside the log, only what that log's own `erasure` entry, the operator's, names, and nothing an
//!   entry rests on; the reference stays and history still checks (`REQ-AUD-13`).
//! - [`Issuer`] signs a party's receipt of an entry about its own part, in the style of RFC 9943, which
//!   [`holds_against`] checks against the history the operator shows (`REQ-AUD-5`).

pub mod receipt;
pub mod store;

use rutent_contracts::core_internal::ReadCheck;
use rutent_contracts::core_internal::calls::{AppendFailure, Evidence, Fetched, Kept};

use crate::log::Unbuilt;

pub use receipt::{
    Issued, Issuer, PartyReceipt, RECEIPT_SHAPE, ReceiptSignature, check_kept, check_receipt,
    holds_against,
};
pub use store::{
    BoundStore, DirectoryShelf, EvidenceStore, Keeper, LogsRead, MemoryShelf, Shelf, ShelfContents,
    ShelfError, Shelved, bearing_out,
};

/// The unbuilt stub, which the assembly's `Assembly::unbuilt` still builds on: no evidence is kept, so no stated
/// entry can name any, and none is handed out.
impl Evidence for Unbuilt {
    fn keep(&mut self, _received: &[u8]) -> Result<Kept, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }

    fn fetch(&mut self, _check: &ReadCheck) -> Result<Fetched, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
}
