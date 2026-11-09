//! A simulated public network behind [`PublicNetwork`]: the reference the real networks are tested against
//! (`T-015-02`, `DEC-13`, `REQ-ORD-39`).
//!
//! It is a chain of blocks driven by hand. Movements are recorded in the tip block, blocks are added, a reorg
//! undoes the newest blocks' movements where the rules allow, and a checkpoint covers what is recorded so far. The
//! network, not the checker, says when a movement is final.

use crate::network::{
    Finality, Movement, MovementId, NetworkRules, PublicNetwork, ReorgRule, Status,
};
use std::collections::BTreeMap;

/// Why a movement could not be recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordRefused {
    /// The network already holds this id and it is not reorged out.
    AlreadyRecorded(MovementId),
    /// The id was reorged out, and this is not the movement it first named: a transaction id fixes its content.
    ContentChanged(MovementId),
}

#[derive(Debug, Clone)]
enum Entry {
    Included {
        movement: Movement,
        height: u64,
        seq: u64,
    },
    ReorgedOut(Movement),
}

/// A simulated public network with the finality and reorg rules it is given.
#[derive(Debug, Clone)]
pub struct SimulatedNetwork {
    name: String,
    rules: NetworkRules,
    tip: u64,
    /// Entries recorded with a sequence number below this are covered by a checkpoint.
    checkpoint_seq: u64,
    next_seq: u64,
    entries: BTreeMap<MovementId, Entry>,
}

impl SimulatedNetwork {
    /// A new, empty network at height 0.
    pub fn new(name: impl Into<String>, rules: NetworkRules) -> Self {
        Self {
            name: name.into(),
            rules,
            tip: 0,
            checkpoint_seq: 0,
            next_seq: 0,
            entries: BTreeMap::new(),
        }
    }

    /// The tip block's height.
    pub fn height(&self) -> u64 {
        self.tip
    }

    /// Records a movement in the tip block. A movement id already on the network, not reorged out, is refused. One
    /// that was reorged out may be recorded again, in a new block, only with the movement it first named.
    pub fn record(&mut self, id: MovementId, movement: Movement) -> Result<(), RecordRefused> {
        match self.entries.get(&id) {
            Some(Entry::Included { .. }) => return Err(RecordRefused::AlreadyRecorded(id)),
            Some(Entry::ReorgedOut(original)) if *original != movement => {
                return Err(RecordRefused::ContentChanged(id));
            }
            _ => {}
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.entries.insert(
            id,
            Entry::Included {
                movement,
                height: self.tip,
                seq,
            },
        );
        Ok(())
    }

    /// Adds `blocks` blocks on the tip.
    pub fn advance(&mut self, blocks: u64) {
        self.tip = self.tip.saturating_add(blocks);
    }

    /// A checkpoint now covers every movement recorded so far, and none recorded after it, even in the same block.
    pub fn checkpoint(&mut self) {
        self.checkpoint_seq = self.next_seq;
    }

    /// Undoes the newest `depth` blocks' movements that the rules let a reorg undo. A final movement is never
    /// undone, and neither is one the reorg rule protects. Returns the movements undone.
    pub fn reorg(&mut self, depth: u64) -> Vec<MovementId> {
        let undone: Vec<MovementId> = self
            .entries
            .iter()
            .filter(|(_, entry)| match entry {
                Entry::Included { height, seq, .. } => {
                    self.confirmations(*height) <= depth && self.reorgable(*height, *seq)
                }
                Entry::ReorgedOut(_) => false,
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in &undone {
            if let Some(Entry::Included { movement, .. }) = self.entries.get(id).cloned() {
                self.entries.insert(id.clone(), Entry::ReorgedOut(movement));
            }
        }
        undone
    }

    fn confirmations(&self, height: u64) -> u64 {
        self.tip - height + 1
    }

    fn is_final(&self, height: u64, seq: u64) -> bool {
        match self.rules.finality {
            Finality::Instant => true,
            Finality::Confirmations(n) => self.confirmations(height) >= u64::from(n.max(1)),
            Finality::Checkpoint => seq < self.checkpoint_seq,
        }
    }

    fn reorgable(&self, height: u64, seq: u64) -> bool {
        !self.is_final(height, seq)
            && match self.rules.reorg {
                ReorgRule::Never => false,
                ReorgRule::DepthBound(d) => self.confirmations(height) <= u64::from(d),
                ReorgRule::UntilFinal => true,
            }
    }
}

impl PublicNetwork for SimulatedNetwork {
    fn network(&self) -> &str {
        &self.name
    }

    fn rules(&self) -> NetworkRules {
        self.rules
    }

    fn status(&self, id: &MovementId) -> Status {
        match self.entries.get(id) {
            None => Status::Unknown,
            Some(Entry::ReorgedOut(_)) => Status::ReorgedOut,
            Some(Entry::Included {
                movement,
                height,
                seq,
            }) if self.is_final(*height, *seq) => Status::Final(movement.clone()),
            Some(Entry::Included { movement, .. }) => Status::Pending(movement.clone()),
        }
    }
}
