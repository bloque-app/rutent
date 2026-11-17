//! The evidence store's statements as they are on the disk, read and nothing else: what a reader outside the
//! operator holds when it is handed the store's directory (`audit-check --evidence`, `T-022-08`).
//!
//! The directory is the one `receipts`' [`DirectoryShelf`](crate::receipts::DirectoryShelf) writes:
//! `kept/<commitment>.json` for each statement kept, and `erased/<commitment>` for each erased. This reader takes no
//! lock, makes nothing, removes nothing and repairs nothing: it answers each commitment with the statement filed
//! under it, as filed, so a statement whose bytes don't bear out its commitment is answered as it is and `unproven`
//! finds it. It reads no index and holds nothing but what the files say.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;

use rutent_contracts::core_internal::KeptStatement;
use rutent_contracts::core_internal::calls::{AppendFailure, KeptStatements};
use rutent_contracts::module_interface::{Digest, DigestValue};

use crate::receipts::Shelved;

/// The statements the evidence store's directory holds, read once.
#[derive(Debug, Clone, Default)]
pub struct KeptOnDisk {
    kept: HashMap<String, KeptStatement>,
}

impl KeptOnDisk {
    /// Reads the evidence store's directory `dir`. It answers why it can't where `dir`, its `kept` or its `erased`
    /// can't be read, or holds a file the store doesn't write: such a directory is no evidence store this reads, and
    /// nothing it holds is taken for an erasure.
    ///
    /// A statement half written (`.part`) was never answered as kept, and is passed over. A statement marked erased
    /// whose bytes are still filed, an erasure cut short between its mark and their removal, is taken as erased, as
    /// the store takes it on its next opening.
    pub fn read(dir: &Path) -> Result<Self, String> {
        let names =
            |sub: &str| -> Result<Vec<String>, String> {
                let at = dir.join(sub);
                let mut names = Vec::new();
                for item in
                    fs::read_dir(&at).map_err(|e| format!("{} can't be read: {e}", at.display()))?
                {
                    let item = item.map_err(|e| format!("{} can't be read: {e}", at.display()))?;
                    names.push(item.file_name().into_string().map_err(|n| {
                        format!("{sub}/{n:?} is no name the evidence store writes")
                    })?);
                }
                Ok(names)
            };
        let mut erased = BTreeSet::new();
        for name in names("erased")? {
            DigestValue::new(name.clone())
                .ok_or_else(|| format!("erased/{name} names no commitment"))?;
            erased.insert(name);
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
            if !erased.contains(&value) {
                kept.insert(value, shelved.kept);
            }
        }
        Ok(KeptOnDisk { kept })
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
