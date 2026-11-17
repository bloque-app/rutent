//! The evidence store's statements as they are on the disk, read and nothing else: what a reader outside the
//! operator holds when it is handed the store's directory (`audit-check --evidence`, `T-022-08`).
//!
//! The directory is the one `receipts`' [`DirectoryShelf`](crate::receipts::DirectoryShelf) writes:
//! `kept/<commitment>.json` for each statement kept, and `erased/<commitment>` for each erased. This reader takes no
//! lock, makes nothing, removes nothing and repairs nothing: it answers each commitment with the statement filed
//! under it, as filed, marked erased or not, so a statement whose bytes don't bear out its commitment is answered as
//! it is and `unproven` finds it. It reads no index and holds nothing but what the files say.
//!
//! What it doesn't keep is an erasure only where the history says so too ([`KeptOnDisk::erasures`]): a statement is
//! erased where the history holds a decided erasure of it and the store marks it erased, and in no other case.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;

use rutent_contracts::core_internal::calls::{AppendFailure, KeptStatements};
use rutent_contracts::core_internal::{Entry, KeptStatement, Receipt};
use rutent_contracts::module_interface::{Digest, DigestValue};

use crate::log::decided_erasures;
use crate::receipts::Shelved;

/// The statements the evidence store's directory holds, read once, and the commitments it marks erased.
#[derive(Debug, Clone, Default)]
pub struct KeptOnDisk {
    kept: HashMap<String, KeptStatement>,
    marked: BTreeSet<String>,
}

/// What the evidence store's directory says of the statements a history names, beside what `unproven` finds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Erasures {
    /// The stated entries whose statement was erased: not filed, marked erased, and the history holds a decided
    /// erasure of it (`REQ-AUD-13`). No finding.
    pub erased: Vec<Receipt>,
    /// The stated entries whose statement is neither filed nor marked erased: lost, with no erasure. A finding.
    pub lost: Vec<Receipt>,
    /// The commitments the store marks erased that the history holds no decided erasure of: bytes gone, or said to
    /// be, with no decision behind them (as `receipts`' `MARK_WITHOUT_ITS_ERASURE`). A finding.
    pub undecided: Vec<DigestValue>,
    /// The commitments the store marks erased whose bytes are still filed. A finding: the reader can't tell an
    /// erasure cut short from a mark made by hand, and the bytes are held to the entry either way.
    pub marked_yet_filed: Vec<DigestValue>,
}

impl Erasures {
    /// Whether it holds no finding.
    pub fn is_clean(&self) -> bool {
        self.lost.is_empty() && self.undecided.is_empty() && self.marked_yet_filed.is_empty()
    }
}

impl KeptOnDisk {
    /// Reads the evidence store's directory `dir`. It answers why it can't where `dir`, its `kept` or its `erased`
    /// can't be read, or holds a file the store doesn't write: such a directory is no evidence store this reads, and
    /// nothing it holds is taken for an erasure. A statement half written (`.part`) was never answered as kept, and is
    /// passed over.
    pub fn read(dir: &Path) -> Result<Self, String> {
        let names = |sub: &str| -> Result<Vec<String>, String> {
            let at = dir.join(sub);
            let unreadable = |e: std::io::Error| format!("{} can't be read: {e}", at.display());
            let mut names = Vec::new();
            for item in fs::read_dir(&at).map_err(unreadable)? {
                names.push(
                    item.map_err(unreadable)?
                        .file_name()
                        .into_string()
                        .map_err(|n| format!("{sub}/{n:?} is no name the evidence store writes"))?,
                );
            }
            Ok(names)
        };
        let mut marked = BTreeSet::new();
        for name in names("erased")? {
            DigestValue::new(name.clone())
                .ok_or_else(|| format!("erased/{name} names no commitment"))?;
            marked.insert(name);
        }
        let mut kept = HashMap::new();
        for name in names("kept")? {
            if name.ends_with(".part") {
                continue;
            }
            let path = dir.join("kept").join(&name);
            let text =
                fs::read(&path).map_err(|e| format!("{} can't be read: {e}", path.display()))?;
            let shelved: Shelved = serde_json::from_slice(&text)
                .map_err(|_| format!("kept/{name} is no statement the evidence store writes"))?;
            let value = shelved.kept.commitment.value.as_str().to_owned();
            if name != format!("{value}.json") {
                return Err(format!(
                    "kept/{name} is filed under another commitment than its own"
                ));
            }
            kept.insert(value, shelved.kept);
        }
        Ok(KeptOnDisk { kept, marked })
    }

    /// What the directory says of the statements `entries` name, held to the decided erasures `entries` hold: each
    /// stated entry whose statement isn't filed is erased (marked, and decided) or lost (not marked); each mark is
    /// held to a decided erasure, and to its bytes being gone. A mark that is decided but whose entry is filed is no
    /// erasure yet carried out, and nothing is said of it here.
    pub fn erasures(&self, entries: &[(Receipt, Entry)]) -> Erasures {
        let decided: BTreeSet<&str> = decided_erasures(entries)
            .into_iter()
            .map(|c| c.value.as_str())
            .collect();
        let mut out = Erasures::default();
        for (receipt, entry) in entries {
            let Entry::Stated(stated) = entry else {
                continue;
            };
            let value = stated.commitment.value.as_str();
            if self.kept.contains_key(value) {
                continue;
            }
            match (self.marked.contains(value), decided.contains(value)) {
                (true, true) => out.erased.push(receipt.clone()),
                (false, _) => out.lost.push(receipt.clone()),
                // A mark with no decided erasure: the mark's finding, below.
                (true, false) => {}
            }
        }
        for mark in &self.marked {
            let value = DigestValue::new(mark.clone()).expect("checked when read");
            if !decided.contains(mark.as_str()) {
                out.undecided.push(value.clone());
            }
            if self.kept.contains_key(mark) {
                out.marked_yet_filed.push(value);
            }
        }
        out
    }
}

impl KeptStatements for KeptOnDisk {
    fn kept(&mut self, commitment: &Digest) -> Result<Option<KeptStatement>, AppendFailure> {
        Ok(self
            .kept
            .get(commitment.value.as_str())
            .filter(|k| k.commitment == *commitment)
            .cloned())
    }
}
