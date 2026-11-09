//! Replay of a private node's recorded inner segments (`REQ-ORD-41`, `DEC-24`, ADR-0005, `THR-131`).
//!
//! A provider on a private node records each segment inside it: the asset and amount in and out, and the fee. Where
//! the value then leaves onto a public node, [`replay_route`] replays the record and compares the result with the
//! public movement out and with the stated outcome. A mismatch is recorded. A match is recorded as consistency with
//! the public exit and nothing more: a provider can make up segments to fit its exit (`THR-131`), so replay detects
//! and never proves. The replay produces no [`Check`](crate::Check) and so can lift no grade. A route ending inside
//! the node is left as its publisher's word.

use crate::network::{Expected, MovementId, PublicNetwork, Unmatched, verify_movement};

/// One segment inside a private node, as its provider recorded it. Amounts are decimal integer strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedSegment {
    /// The asset that went in.
    pub asset_in: String,
    /// The amount that went in.
    pub amount_in: String,
    /// The asset that came out.
    pub asset_out: String,
    /// The amount that came out.
    pub amount_out: String,
    /// The fee, in the asset that went in.
    pub fee: String,
}

/// The segments recorded inside one private node, in the order they ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRecord {
    /// The private node's identifier.
    pub node: String,
    /// Its recorded segments.
    pub segments: Vec<RecordedSegment>,
}

/// What a provider states of the value leaving onto the public node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stated {
    /// The asset.
    pub asset: String,
    /// The amount.
    pub amount: String,
}

/// The public movement out of the last private node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicExit {
    /// The movement on the public network. One movement evidences one exit, never two.
    pub movement: MovementId,
    /// The public network.
    pub network: String,
    /// The route's one destination on it.
    pub destination: String,
    /// The outcome the provider stated.
    pub stated: Stated,
}

/// Where a route's value ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteEnd {
    /// It leaves the private nodes onto a public one.
    Public(PublicExit),
    /// It stays inside the node: its publisher's word, with no replay check.
    InsideNode,
}

/// What replaying one node's record gives: what went in and what came out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replayed {
    /// The asset that entered the node.
    pub asset_in: String,
    /// The amount that entered it.
    pub amount_in: String,
    /// The asset that left it.
    pub asset: String,
    /// The amount that left it.
    pub amount: String,
}

/// A way a record disagrees with itself, with what the public network shows, or with what was stated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mismatch {
    /// An amount or fee is not a canonical decimal integer.
    Malformed {
        /// The node.
        node: String,
        /// The segment's position in the node's record.
        at: usize,
    },
    /// A segment does not take in what the one before it gave out.
    BrokenChain {
        /// The node.
        node: String,
        /// The segment's position.
        at: usize,
    },
    /// Where the asset stays the same, amount in is not amount out plus fee.
    FeeMismatch {
        /// The node.
        node: String,
        /// The segment's position.
        at: usize,
    },
    /// One private node does not hand on what the next records taking in.
    HandoffBroken {
        /// The node that hands on.
        from: String,
        /// The node that takes in.
        to: String,
    },
    /// The public movement out is not a final record of the replayed value to the route's destination.
    PublicExit(Unmatched),
    /// The replayed value is not the outcome the provider stated.
    StatedOutcome {
        /// The asset the replay gives.
        replayed_asset: String,
        /// The amount the replay gives.
        replayed_amount: String,
    },
}

/// Why a public exit cannot be replayed at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unreplayable {
    /// No private node was recorded.
    NoNodes,
    /// This node recorded no segments, so there is nothing to compare.
    NothingRecorded(String),
}

/// Value the replay found to agree with the public exit: consistency, not proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consistency {
    /// How many private nodes were replayed.
    pub nodes: usize,
    /// The asset that left.
    pub asset: String,
    /// The amount that left.
    pub amount: String,
}

impl Consistency {
    /// Whether this shows the recorded segments happened. It never does (`THR-131`).
    pub fn proves_segments_happened(&self) -> bool {
        false
    }
}

/// The record a replay leaves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Replay {
    /// The route ends inside the node: no replay check, the publisher's word.
    PublishersWord,
    /// There was nothing to replay.
    Unreplayable(Unreplayable),
    /// Everything recorded, in order. Never empty.
    Mismatch(Vec<Mismatch>),
    /// Consistent with the public exit, as recorded. Not proof.
    Consistent(Consistency),
}

fn amount(text: &str) -> Option<u128> {
    let n: u128 = text.parse().ok()?;
    (n.to_string() == text).then_some(n)
}

/// Replays one node's recorded segments: each must take in what the one before gave out, and where the asset stays
/// the same the fee must account for the difference. Where the asset changes, the rate is the node's own and no
/// fee check is made. Pure: the result depends on the segments and nothing else.
pub fn replay_node(node: &str, segments: &[RecordedSegment]) -> Result<Replayed, Vec<Mismatch>> {
    let mut found = Vec::new();
    let at_fault = |kind: fn(String, usize) -> Mismatch, at: usize| kind(node.to_owned(), at);
    for (at, seg) in segments.iter().enumerate() {
        let parsed = (
            amount(&seg.amount_in),
            amount(&seg.amount_out),
            amount(&seg.fee),
        );
        let (Some(taken), Some(given), Some(fee)) = parsed else {
            found.push(at_fault(|node, at| Mismatch::Malformed { node, at }, at));
            continue;
        };
        if let Some(prev) = at.checked_sub(1).map(|i| &segments[i])
            && (prev.asset_out != seg.asset_in || prev.amount_out != seg.amount_in)
        {
            found.push(at_fault(|node, at| Mismatch::BrokenChain { node, at }, at));
        }
        if seg.asset_in == seg.asset_out && given.checked_add(fee) != Some(taken) {
            found.push(at_fault(|node, at| Mismatch::FeeMismatch { node, at }, at));
        }
    }
    match (segments.first(), segments.last()) {
        (Some(first), Some(last)) if found.is_empty() => Ok(Replayed {
            asset_in: first.asset_in.clone(),
            amount_in: first.amount_in.clone(),
            asset: last.asset_out.clone(),
            amount: last.amount_out.clone(),
        }),
        _ => Err(found),
    }
}

/// Replays a route's private nodes, in route order, against its end.
///
/// A route ending inside the node records no check. Otherwise every node is replayed; a fault inside a node is
/// recorded alone, since what that node gives out can't be trusted for the comparisons after it. When every node
/// holds up, what one hands on must be what the next took in, and what the last gives out is compared with the
/// public movement out (the network alone decides it is final) and with the stated outcome. All the mismatches found
/// are recorded; none found is consistency only.
pub fn replay_route(nodes: &[NodeRecord], end: &RouteEnd, net: &dyn PublicNetwork) -> Replay {
    let RouteEnd::Public(exit) = end else {
        return Replay::PublishersWord;
    };
    if nodes.is_empty() {
        return Replay::Unreplayable(Unreplayable::NoNodes);
    }
    if let Some(empty) = nodes.iter().find(|n| n.segments.is_empty()) {
        return Replay::Unreplayable(Unreplayable::NothingRecorded(empty.node.clone()));
    }
    let mut replayed = Vec::new();
    let mut faults = Vec::new();
    for record in nodes {
        match replay_node(&record.node, &record.segments) {
            Ok(r) => replayed.push(r),
            Err(found) => faults.extend(found),
        }
    }
    if !faults.is_empty() {
        return Replay::Mismatch(faults);
    }
    let mut found = Vec::new();
    for (pair, names) in replayed.windows(2).zip(nodes.windows(2)) {
        if pair[0].asset != pair[1].asset_in || pair[0].amount != pair[1].amount_in {
            found.push(Mismatch::HandoffBroken {
                from: names[0].node.clone(),
                to: names[1].node.clone(),
            });
        }
    }
    let Some(last) = replayed.last() else {
        return Replay::Unreplayable(Unreplayable::NoNodes);
    };
    let expected = Expected {
        network: exit.network.clone(),
        destination: exit.destination.clone(),
        asset: last.asset.clone(),
        amount: last.amount.clone(),
    };
    if let Err(why) = verify_movement(net, &exit.movement, &expected) {
        found.push(Mismatch::PublicExit(why));
    }
    if exit.stated.asset != last.asset || exit.stated.amount != last.amount {
        found.push(Mismatch::StatedOutcome {
            replayed_asset: last.asset.clone(),
            replayed_amount: last.amount.clone(),
        });
    }
    if found.is_empty() {
        Replay::Consistent(Consistency {
            nodes: replayed.len(),
            asset: last.asset.clone(),
            amount: last.amount.clone(),
        })
    } else {
        Replay::Mismatch(found)
    }
}
