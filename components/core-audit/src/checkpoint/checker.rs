//! The outside checker (`REQ-AUD-9`, `REQ-AUD-10`, `INV-6`): the check, from bytes alone.

use std::collections::BTreeMap;
use std::fmt;

use rutent_contracts::core_internal::Entry;
use rutent_contracts::provider_role::PublicKey;
use rutent_contracts::router_api::Id;
use rutent_prov::catalogue::verify_envelope;
use sha2::{Digest as _, Sha256};

use super::{
    CHECKPOINT_SHAPE, Checkpoint, Kept, OPENING_SHAPE, Opening, Schedule, commitment, salt_of,
    tick_of,
};
use crate::log::merkle::{self, Hash, leaf_hash};
use crate::log::{Store, StoreError, record};

/// What the check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finding {
    /// A record of the history that isn't an entry in its RFC 8785 canonical form.
    RecordNotAnEntry {
        /// Its place in the history.
        index: usize,
    },
    /// A checkpoint held that isn't one, isn't of this log, isn't on the schedule, or isn't signed by the log's key.
    NotACheckpoint {
        /// Its place among those held.
        position: usize,
    },
    /// Two different checkpoints held for one tick, both signed: the operator said two things.
    Equivocation {
        /// The tick.
        tick: u64,
    },
    /// Ticks the schedule fixed that no checkpoint held answers, `from` to `to` inclusive.
    Missing {
        /// The first tick.
        from: u64,
        /// The last tick.
        to: u64,
    },
    /// A checkpoint held with no opening.
    NoOpening {
        /// The tick.
        tick: u64,
    },
    /// A checkpoint whose opening doesn't make it a commitment to any root of this history: a record was altered
    /// or removed since, or the opening isn't its own.
    DoesNotHold {
        /// The tick.
        tick: u64,
    },
    /// A checkpoint whose opening names more entries than the history holds: records were removed.
    HistoryShorter {
        /// The tick.
        tick: u64,
    },
    /// A checkpoint of a later tick that commits to fewer entries than an earlier one.
    RolledBack {
        /// The later tick.
        tick: u64,
    },
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

/// What a check answered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Every finding, none where the history holds against every checkpoint.
    pub findings: Vec<Finding>,
}

impl Report {
    /// Whether the check found nothing.
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Why a review couldn't be made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewError {
    /// A store couldn't be read.
    Store(StoreError),
}

/// The checker.
#[derive(Debug, Clone)]
pub struct Checker {
    key: PublicKey,
    log: Id,
    schedule: Schedule,
    from: u64,
}

impl Checker {
    /// The checker of the log named `log`, whose checkpoints `key` signs, on `schedule`.
    pub fn new(key: PublicKey, log: Id, schedule: Schedule) -> Self {
        Checker {
            key,
            log,
            schedule,
            from: 0,
        }
    }

    /// This checker, whose watch began at tick `from`: it holds no checkpoint missing before it.
    pub fn from(mut self, from: u64) -> Self {
        self.from = from;
        self
    }

    /// Reads `bytes` as a checkpoint of this log, signed by its key, on its schedule: its tick and it. `None` for
    /// anything else.
    pub(crate) fn read_checkpoint(&self, bytes: &[u8]) -> Option<(u64, Checkpoint)> {
        let checkpoint: Checkpoint = serde_json::from_slice(bytes).ok()?;
        let tick = tick_of(&checkpoint.tick)?;
        let holds = checkpoint.shape == CHECKPOINT_SHAPE
            && checkpoint.log == self.log
            && self.schedule.at(tick).as_ref() == Some(&checkpoint.at)
            && checkpoint.signature.scheme == "ed25519"
            && checkpoint.signature.key == self.key
            && verify_envelope(&self.key, &checkpoint.signature.value, bytes).is_ok();
        holds.then_some((tick, checkpoint))
    }

    /// Reads `bytes` as an opening of this log's.
    fn read_opening(&self, bytes: &[u8]) -> Option<(u64, Opening)> {
        let opening: Opening = serde_json::from_slice(bytes).ok()?;
        (opening.shape == OPENING_SHAPE && opening.log == self.log)
            .then(|| tick_of(&opening.tick).map(|tick| (tick, opening)))
            .flatten()
    }

    /// Checks the history, its `records` as the log's file holds them, against the checkpoints `held`, as the
    /// checker received each when it was published, and their `openings`, as the operator hands them over,
    /// through tick `through` (`None` where none is due yet). It reads nothing else.
    ///
    /// A finding is made for each: a record that is no entry; a checkpoint held that is none of this log's, or not
    /// signed by its key, or not on its schedule; two for one tick; a tick no checkpoint answers; a checkpoint
    /// with no opening, or whose opening opens to no root of this history, or to a size past its end; and a size
    /// that falls from one tick to the next. The openings are the operator's, so are trusted for nothing: an
    /// opening that doesn't make the checkpoint's commitment, which only the log it was issued over does, fails.
    pub fn check(
        &self,
        records: &[Vec<u8>],
        held: &[Vec<u8>],
        openings: &[Vec<u8>],
        through: Option<u64>,
    ) -> Report {
        let mut findings = Vec::new();
        for (index, bytes) in records.iter().enumerate() {
            let canonical = serde_json::from_slice::<Entry>(bytes)
                .ok()
                .is_some_and(|e| e.check().is_ok() && record(&e).ok().as_ref() == Some(bytes));
            if !canonical {
                findings.push(Finding::RecordNotAnEntry { index });
            }
        }
        let leaves: Vec<Hash> = records.iter().map(|r| leaf_hash(r)).collect();
        let roots = prefix_roots(&leaves);

        let mut by_tick: BTreeMap<u64, Vec<Checkpoint>> = BTreeMap::new();
        for (position, bytes) in held.iter().enumerate() {
            match self.read_checkpoint(bytes) {
                Some((tick, checkpoint)) => {
                    let same = by_tick.entry(tick).or_default();
                    if !same.contains(&checkpoint) {
                        same.push(checkpoint);
                    }
                }
                None => findings.push(Finding::NotACheckpoint { position }),
            }
        }
        let mut opened: BTreeMap<u64, Vec<Opening>> = BTreeMap::new();
        for bytes in openings {
            if let Some((tick, opening)) = self.read_opening(bytes) {
                opened.entry(tick).or_default().push(opening);
            }
        }

        // Each checkpoint against the history, through its openings.
        let mut sizes: Vec<(u64, u64)> = Vec::new();
        for (&tick, checkpoints) in &by_tick {
            if checkpoints.len() > 1 {
                findings.push(Finding::Equivocation { tick });
            }
            for checkpoint in checkpoints {
                let candidates = opened.get(&tick).map(Vec::as_slice).unwrap_or_default();
                if candidates.is_empty() {
                    findings.push(Finding::NoOpening { tick });
                    continue;
                }
                let mut size = None;
                let mut beyond = true;
                for opening in candidates {
                    let (Some(salt), n) = (salt_of(opening), opening.size.get()) else {
                        beyond = false;
                        continue;
                    };
                    match roots.get(n as usize) {
                        None => {}
                        Some(root) => {
                            beyond = false;
                            if commitment(&self.log, tick, &salt, n, root) == checkpoint.commitment
                            {
                                size = Some(n);
                                break;
                            }
                        }
                    }
                }
                match size {
                    Some(n) => sizes.push((tick, n)),
                    None if beyond => findings.push(Finding::HistoryShorter { tick }),
                    None => findings.push(Finding::DoesNotHold { tick }),
                }
            }
        }
        sizes.sort_unstable();
        for pair in sizes.windows(2) {
            if pair[1].1 < pair[0].1 {
                findings.push(Finding::RolledBack { tick: pair[1].0 });
            }
        }

        // The schedule: every tick from where the watch began, through the last due, answered.
        if let Some(through) = through {
            let mut run: Option<(u64, u64)> = None;
            for tick in self.from..=through {
                if by_tick.contains_key(&tick) {
                    if let Some((from, to)) = run.take() {
                        findings.push(Finding::Missing { from, to });
                    }
                } else {
                    run = Some((run.map_or(tick, |(from, _)| from), tick));
                }
            }
            if let Some((from, to)) = run {
                findings.push(Finding::Missing { from, to });
            }
        }
        Report { findings }
    }

    /// The operator's own review of its stores (`REQ-AUD-3`): the history's file and the checkpoint store, read as
    /// they are on the disk, and checked as an outside checker checks. It has what the checker lacks, the
    /// checkpoint store itself, which an intruder without the log's key can't write checkpoints into, but it has no
    /// independent copy, so it claims nothing against full operator control (`REQ-AUD-9` is the checker's).
    pub fn review<H: Store, K: Store>(
        &self,
        history: &mut H,
        kept: &mut K,
        through: Option<u64>,
    ) -> Result<Report, ReviewError> {
        let records = history.records().map_err(ReviewError::Store)?;
        let kept = kept.records().map_err(ReviewError::Store)?;
        let (held, openings) = split_kept(&kept);
        Ok(self.check(&records, &held, &openings, through))
    }
}

/// The root of the tree over each prefix of `leaves`, from none to all: the root of the first `n`, at place `n`.
/// One pass, folding the roots of the perfect subtrees a prefix is made of from the right, as RFC 9162's tree
/// splits at the largest power of two below its size.
fn prefix_roots(leaves: &[Hash]) -> Vec<Hash> {
    let mut roots = Vec::with_capacity(leaves.len() + 1);
    roots.push(merkle::root(&[]));
    let mut peaks: Vec<(usize, Hash)> = Vec::new();
    for leaf in leaves {
        peaks.push((1, *leaf));
        while peaks.len() >= 2 && peaks[peaks.len() - 1].0 == peaks[peaks.len() - 2].0 {
            let (n, right) = peaks.pop().expect("two peaks");
            let (_, left) = peaks.pop().expect("two peaks");
            peaks.push((2 * n, node(&left, &right)));
        }
        let (_, last) = peaks[peaks.len() - 1];
        let root = peaks[..peaks.len() - 1]
            .iter()
            .rev()
            .fold(last, |right, (_, left)| node(left, &right));
        roots.push(root);
    }
    roots
}

/// The hash of an inner node (RFC 9162 §2.1.1): SHA-256 of `0x01` and its two children.
fn node(left: &Hash, right: &Hash) -> Hash {
    let mut hasher = Sha256::new();
    hasher.update([1u8]);
    hasher.update(left);
    hasher.update(right);
    hasher.finalize().into()
}

/// The records of a file as a store holds them: one to a line, a last line cut short dropped, nothing else read.
pub fn read_lines(bytes: &[u8]) -> Vec<Vec<u8>> {
    let whole = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    bytes[..whole]
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
        .map(<[u8]>::to_vec)
        .collect()
}

/// The published checkpoints and the openings of a checkpoint store's records. A record that isn't one is passed on
/// among the checkpoints, so the check finds it is none.
pub fn split_kept(records: &[Vec<u8>]) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let (mut published, mut openings) = (Vec::new(), Vec::new());
    for bytes in records {
        match serde_json::from_slice::<Kept>(bytes) {
            Ok(kept) => {
                published.push(serde_json::to_vec(&kept.checkpoint).expect("a checkpoint is JSON"));
                openings.push(serde_json::to_vec(&kept.opening).expect("an opening is JSON"));
            }
            Err(_) => published.push(bytes.clone()),
        }
    }
    (published, openings)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every prefix's root, folded in one pass, is the log's tree hash of that prefix.
    // verifies: DEC-12
    #[test]
    fn prefix_roots_are_the_logs_tree_hashes() {
        let leaves: Vec<Hash> = (0..70u8).map(|i| leaf_hash(&[i, 3])).collect();
        let roots = prefix_roots(&leaves);
        assert_eq!(roots.len(), 71);
        for (n, root) in roots.iter().enumerate() {
            assert_eq!(*root, merkle::root(&leaves[..n]), "{n}");
        }
    }
}
