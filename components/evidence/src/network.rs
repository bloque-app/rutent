//! The per-network interface for grade 1 and 2 claims (ADR-0007, `DEC-13`).
//!
//! Each public network settles three things: when a movement is final, what a reorg does, and how a movement ties
//! back to one segment. A real network joins as its own task (`F-031`); [`PublicNetwork`] is what it implements.

use crate::grade::Check;

/// When a movement on a network counts as final.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finality {
    /// Final as soon as it is recorded.
    Instant,
    /// Final after this many confirmations.
    Confirmations(u32),
    /// Final once a later checkpoint covers it.
    Checkpoint,
}

/// What the network can do to a movement that is not yet final.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReorgRule {
    /// A recorded movement is never undone.
    Never,
    /// A movement can be undone to this depth; beyond it, it is final.
    DepthBound(u32),
    /// A movement can be undone until the finality rule says otherwise.
    UntilFinal,
}

/// A network's settled rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetworkRules {
    /// The finality definition.
    pub finality: Finality,
    /// The reorg-handling rule.
    pub reorg: ReorgRule,
}

/// A movement's identifier on its network, such as a transaction hash. Never empty.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MovementId(String);

impl MovementId {
    /// The identifier, or `None` for empty text.
    pub fn new(text: impl Into<String>) -> Option<Self> {
        let text = text.into();
        (!text.is_empty()).then_some(Self(text))
    }

    /// The identifier as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A movement as the network records it. The amount is a decimal integer string, in the unit of the segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Movement {
    /// Where the value went.
    pub destination: String,
    /// The asset.
    pub asset: String,
    /// The amount.
    pub amount: String,
}

/// What a network knows of a movement now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// No such movement.
    Unknown,
    /// Recorded, not yet final.
    Pending(Movement),
    /// Recorded and final under the network's rules.
    Final(Movement),
    /// Recorded once, since undone by a reorg.
    ReorgedOut,
}

/// A public network the checker can read.
pub trait PublicNetwork {
    /// The network's name, as an outer node names it.
    fn network(&self) -> &str;
    /// The network's finality and reorg rules.
    fn rules(&self) -> NetworkRules;
    /// What the network knows of a movement now. Read-only.
    fn status(&self, id: &MovementId) -> Status;
}

/// What a segment's movement must be. One destination per route is what ties a movement to a segment; an amount
/// alone never does (`DEC-13`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expected {
    /// The network.
    pub network: String,
    /// The route's destination on it.
    pub destination: String,
    /// The asset.
    pub asset: String,
    /// The amount.
    pub amount: String,
}

/// Why a movement evidences no segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unmatched {
    /// The network knows no such movement.
    UnknownMovement,
    /// It is recorded but not final.
    NotFinal,
    /// It was reorged out.
    ReorgedOut,
    /// It is on another network than the segment's.
    WrongNetwork,
    /// It went to another destination.
    WrongDestination,
    /// It moved another asset.
    WrongAsset,
    /// It moved another amount.
    WrongAmount,
}

/// Whether a movement is a final public record of the segment.
pub fn verify_movement(
    net: &dyn PublicNetwork,
    id: &MovementId,
    expected: &Expected,
) -> Result<(), Unmatched> {
    if net.network() != expected.network {
        return Err(Unmatched::WrongNetwork);
    }
    let movement = match net.status(id) {
        Status::Unknown => return Err(Unmatched::UnknownMovement),
        Status::Pending(_) => return Err(Unmatched::NotFinal),
        Status::ReorgedOut => return Err(Unmatched::ReorgedOut),
        Status::Final(movement) => movement,
    };
    if movement.destination != expected.destination {
        Err(Unmatched::WrongDestination)
    } else if movement.asset != expected.asset {
        Err(Unmatched::WrongAsset)
    } else if movement.amount != expected.amount {
        Err(Unmatched::WrongAmount)
    } else {
        Ok(())
    }
}

/// The check a movement earns: a final public record when [`verify_movement`] passes, nothing otherwise.
pub fn public_record_check(net: &dyn PublicNetwork, id: &MovementId, expected: &Expected) -> Check {
    match verify_movement(net, id, expected) {
        Ok(()) => Check::public_record(id.clone()),
        Err(_) => Check::unchecked(),
    }
}
