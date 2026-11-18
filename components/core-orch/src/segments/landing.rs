//! Value that settles at a public inner node held by a holding account lands in it (`T-020-33`; `REQ-ORD-40`,
//! `INV-5`): the operator's plain wallet is asked to place it in the account, and the host attributes it to the order
//! (`Holdings::land`). Until this, nothing in Core called it, so where the value rested at a public node was the
//! scenario's word; now it is Core's own record, made where the segment that brought the value settled.
//!
//! **The wallet is asked once for a segment.** The landing is called by the assembly after each entry point that can
//! settle a segment ([`Segments::land_settled`](super::Segments::land_settled)), like the measure writer, and nothing
//! of a segment, its outcome, its entries or what it moved depends on it: a value that couldn't be placed leaves the
//! segment as it was. A landing the wallet couldn't take *now* (nothing passed out, so nothing is attributed) is asked
//! again on the next call; one whose outcome is unknown, or that the account or the wallet refused, is never asked
//! again: asking twice could attribute the value twice, and the account's own rules then refuse any withdrawal of
//! more than it holds, so nothing leaves the account that was never confirmed in it. It is counted
//! ([`Segments::landings_unconfirmed`](super::Segments::landings_unconfirmed)) for the operator's reading of the
//! account (`REQ-AUD-15`).

use std::collections::BTreeSet;

use rutent_contracts::router_api::Amount;

/// The operator's holding accounts, as the segments land value in them. The assembly gives it, over `Holdings` and the
/// platform that runs their wallets.
pub trait Landings {
    /// Whether `node` has a holding account, which value that settles there lands in. A node with none holds nothing
    /// of Core's.
    fn holds(&self, node: &str) -> bool;

    /// Places `amount` of `asset`, the value of `order` settled at `node`, in the node's holding account.
    fn land(&mut self, node: &str, asset: &str, order: &str, amount: &Amount) -> Landed;
}

/// What landing a value answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Landed {
    /// The wallet confirmed it and the account holds it for the order.
    Placed,
    /// The wallet could not take it now, and nothing passed out: it is asked again on the next call.
    NotNow,
    /// The wallet didn't confirm it, or its outcome is unknown: never asked again for this segment.
    Unconfirmed,
}

/// The segments whose value was landed, or couldn't be confirmed, by order and segment.
#[derive(Debug, Clone, Default)]
pub(super) struct Landing {
    placed: BTreeSet<(String, String)>,
    unconfirmed: BTreeSet<(String, String)>,
}

impl Landing {
    /// Whether the segment's landing needs no further call.
    pub(super) fn closed(&self, at: &(String, String)) -> bool {
        self.placed.contains(at) || self.unconfirmed.contains(at)
    }

    pub(super) fn close(&mut self, at: (String, String), landed: Landed) -> bool {
        match landed {
            Landed::Placed => {
                self.placed.insert(at);
                true
            }
            Landed::Unconfirmed => {
                self.unconfirmed.insert(at);
                false
            }
            Landed::NotNow => false,
        }
    }

    pub(super) fn unconfirmed(&self) -> usize {
        self.unconfirmed.len()
    }

    pub(super) fn placed(&self, at: &(String, String)) -> bool {
        self.placed.contains(at)
    }
}
