//! The log: a Merkle log in `CMP-CORE-AUDIT`'s own store, whose append answers before the effect it records
//! (ADR-0005, `DEC-12`, `T-022-01`).
//!
//! Declared by `T-020-01`; `T-022-01` owns it, and only its owning row edits it (`PLAN.md` §4.4).
//!
//! [`AuditLog`] implements both of `core-internal`'s handles on one log: [`Log`], orchestration's and the checks',
//! and [`PolicyLog`], policy's. Every append, through either, runs the same steps, and records nothing unless each
//! passes:
//!
//! 1. The entry's own `check`, which recurses into everything it holds (`REQ-AUD-1`, `REQ-AUD-6`: no shape of the
//!    contract holds a secret's value or a balance, and identity only by reference).
//! 2. For a stated entry, `core-internal`'s [`attribute`]: the log, not the writer, attributes it, from the bytes
//!    the evidence store keeps under its commitment ([`KeptStatements`]), the keys in the log's own register
//!    ([`keys`]) and a strict Ed25519 check ([`Ed25519Strict`]), and refuses one whose bytes aren't kept
//!    (`REQ-AUD-2`). The receipt is of the entry recorded.
//! 3. An equal entry already in the log answers with its own receipt and is not appended again, so a retry after a
//!    failed append records it once (`REQ-MDL-8`).
//! 4. [`admit_entry`], the whole citation table, over the log's history, on the path the entry came by.
//! 5. The record, the entry's RFC 8785 canonical JSON, is written to the log's [`Store`] and made durable. Only then
//!    does the log take it into its history, its tree and its register, and answer the receipt: SHA-256 of that
//!    record, at its index. Where the store fails, the log answers [`AppendFailure::Unavailable`] and changes
//!    nothing; since a failed write may still have reached the store, it reads the store back before its next
//!    append (`INV-7`, `NFR-5`).
//!
//! `core-audit`'s own checks (`T-022-05`, `T-022-06`) append through a [`General`] handle, never through this type,
//! so they can write no decision entry, and, since a [`GeneralEntry`] holds none attributed `verified`, attribute no
//! statement.

pub mod kept;
pub mod keys;
pub mod merkle;
pub mod signatures;
pub mod store;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use rutent_contracts::core_internal::calls::{
    AppendFailure, DecisionEntry, GeneralEntry, KeptStatements, Log, PolicyLog,
};
use rutent_contracts::core_internal::{Entry, Receipt, Through, Violation, admit_entry, attribute};
use rutent_contracts::module_interface::{Digest, DigestAlgorithm, DigestValue};
use rutent_contracts::provider_role::{ADVERTISEMENT_SHAPE, SafeInteger, to_canonical};

pub use kept::KeptInMemory;
pub use keys::Settings;
pub use merkle::Hash;
pub use signatures::Ed25519Strict;
pub use store::{FileStore, MemoryStore, Store, StoreError};

use keys::{Keys, Register};

/// The refusal of an entry with no canonical JSON.
pub const NO_CANONICAL_FORM: &str = "an entry of the log has an RFC 8785 canonical form";

/// The refusal to open a store holding a record that isn't an entry in its canonical form.
pub const NOT_A_RECORD_OF_THE_LOG: &str =
    "every record of the log's store is an entry in its RFC 8785 canonical form";

/// The log and its checks before their rows build them: they take no entry, keep no evidence and record no check,
/// answering each as unavailable, so no action that waits on a receipt takes effect (`INV-7`). Each trait's stub
/// sits in the module of the row that owns that trait; this one's [`Log`] and [`PolicyLog`] are `T-022-01`'s, which
/// [`AuditLog`] implements.
#[derive(Debug, Clone, Copy, Default)]
pub struct Unbuilt;

impl Log for Unbuilt {
    fn append(&mut self, _entry: &GeneralEntry) -> Result<Receipt, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
}

impl PolicyLog for Unbuilt {
    fn append_decision(&mut self, _entry: &DecisionEntry) -> Result<Receipt, AppendFailure> {
        Err(AppendFailure::Unavailable)
    }
}

/// A SHA-256 hash as the contract's [`Digest`].
pub fn digest(hash: &Hash) -> Digest {
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    Digest {
        algorithm: DigestAlgorithm::Sha256,
        value: DigestValue::new(hex).expect("64 lower-case hexadecimal characters"),
    }
}

/// An entry's record: its RFC 8785 canonical JSON, the `Entry` exactly as the contract serialises it. A receipt's
/// digest is SHA-256 of it, and the tree's leaf is it.
pub fn record(entry: &Entry) -> Result<Vec<u8>, Violation> {
    to_canonical(entry).map_err(|_| Violation(NO_CANONICAL_FORM))
}

/// `CMP-CORE-AUDIT`'s log (ADR-0005): its history, kept in its own store `S`, under a Merkle tree; the evidence
/// store's read `K`, from which it attributes statements; and its register of whose keys sign what, kept from its
/// own records and the deployment's [`Settings`].
pub struct AuditLog<S: Store, K: KeptStatements> {
    settings: Settings,
    store: S,
    kept: K,
    history: Vec<(Receipt, Entry)>,
    leaves: Vec<Hash>,
    recorded: HashMap<Hash, usize>,
    register: Register,
    /// A write failed, so the store may hold a record the history doesn't: read it back before the next append.
    stale: bool,
}

impl<S: Store, K: KeptStatements> AuditLog<S, K> {
    /// Opens the log kept in `store`, reading back every record it holds, with `kept` the evidence store's read and
    /// `settings` the deployment's. It refuses a store holding a record that isn't an entry in its canonical form.
    pub fn open(settings: Settings, store: S, kept: K) -> Result<Self, AppendFailure> {
        let mut log = AuditLog {
            settings,
            store,
            kept,
            history: Vec::new(),
            leaves: Vec::new(),
            recorded: HashMap::new(),
            register: Register::default(),
            stale: true,
        };
        log.reload()?;
        Ok(log)
    }

    /// Rebuilds the history, the tree and the register from the store.
    fn reload(&mut self) -> Result<(), AppendFailure> {
        let records = self.store.records().map_err(|e| match e {
            StoreError::Unavailable(_) => AppendFailure::Unavailable,
            StoreError::Corrupt(_) => AppendFailure::Refused(Violation(NOT_A_RECORD_OF_THE_LOG)),
        })?;
        self.history.clear();
        self.leaves.clear();
        self.recorded.clear();
        self.register = Register::default();
        for bytes in records {
            let entry: Entry = serde_json::from_slice(&bytes)
                .ok()
                .filter(|e: &Entry| e.check().is_ok() && record(e).ok().as_ref() == Some(&bytes))
                .ok_or(AppendFailure::Refused(Violation(NOT_A_RECORD_OF_THE_LOG)))?;
            let advertisement = self.advertisement(&entry)?;
            self.take(&bytes, entry, advertisement.as_deref());
        }
        self.stale = false;
        Ok(())
    }

    /// The bytes kept for a stated entry of an advertisement's shape, which the register reads.
    fn advertisement(&mut self, entry: &Entry) -> Result<Option<Vec<u8>>, AppendFailure> {
        match entry {
            Entry::Stated(s) if s.what.statement.as_str() == ADVERTISEMENT_SHAPE => Ok(self
                .kept
                .kept(&s.commitment)?
                .filter(|k| k.commitment == s.commitment)
                .and_then(|k| kept::from_base64url(k.bytes.as_str()))),
            _ => Ok(None),
        }
    }

    /// Takes a record the store holds into the history, the tree and the register, and answers its receipt.
    fn take(&mut self, bytes: &[u8], entry: Entry, advertisement: Option<&[u8]>) -> Receipt {
        let hash = merkle::sha256(bytes);
        let index = self.history.len();
        let receipt = Receipt {
            log: self.settings.log.clone(),
            index: SafeInteger::try_from(index as u64).expect("a log of fewer than 2^53 entries"),
            entry: digest(&hash),
        };
        self.register.record(&self.settings, &entry, advertisement);
        self.leaves.push(merkle::leaf_hash(bytes));
        self.recorded.insert(hash, index);
        self.history.push((receipt.clone(), entry));
        receipt
    }

    /// Appends `entry`, which came by `through`, as the module's steps say.
    fn append_through(
        &mut self,
        entry: &Entry,
        through: Through,
    ) -> Result<Receipt, AppendFailure> {
        // A store that can't be read back now says nothing of the entry, which breaks no rule: its writer keeps it
        // and appends it again (`REQ-MDL-8`).
        if self.stale && self.reload().is_err() {
            return Err(AppendFailure::Unavailable);
        }
        entry.check().map_err(AppendFailure::Refused)?;
        let (entry, advertisement) = match entry {
            Entry::Stated(claim) => {
                let advertisement = self.advertisement(entry)?;
                let mut keys = Keys {
                    settings: &self.settings,
                    register: &self.register,
                };
                let recorded = attribute(claim, &mut self.kept, &mut keys, &Ed25519Strict)?;
                (Entry::Stated(recorded), advertisement)
            }
            Entry::Verified(_) => (entry.clone(), None),
        };
        let bytes = record(&entry).map_err(AppendFailure::Refused)?;
        if let Some(&index) = self.recorded.get(&merkle::sha256(&bytes)) {
            return Ok(self.history[index].0.clone());
        }
        admit_entry(&self.history, &entry, through, &self.settings.operator)
            .map_err(AppendFailure::Refused)?;
        if self.store.append(&bytes).is_err() {
            self.stale = true;
            return Err(AppendFailure::Unavailable);
        }
        Ok(self.take(&bytes, entry, advertisement.as_deref()))
    }

    /// Every entry of the log with its receipt, in order.
    pub fn entries(&self) -> &[(Receipt, Entry)] {
        &self.history
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        self.history.len()
    }

    /// Whether the log holds no entry.
    pub fn is_empty(&self) -> bool {
        self.history.is_empty()
    }

    /// The root of the tree over every entry (RFC 9162 §2.1).
    pub fn root(&self) -> Hash {
        merkle::root(&self.leaves)
    }

    /// The root of the tree over the first `size` entries, or `None` where the log holds fewer.
    pub fn root_at(&self, size: usize) -> Option<Hash> {
        (size <= self.leaves.len()).then(|| merkle::root(&self.leaves[..size]))
    }

    /// The inclusion proof of entry `index` in the tree over the first `size` entries, or `None` where there is no
    /// such entry (RFC 9162 §2.1.3); [`merkle::verify_inclusion`] checks it.
    pub fn inclusion(&self, index: usize, size: usize) -> Option<Vec<Hash>> {
        (size <= self.leaves.len())
            .then(|| merkle::inclusion_proof(index, &self.leaves[..size]))
            .flatten()
    }

    /// The settings the log was opened with.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }
}

impl<S: Store, K: KeptStatements> Log for AuditLog<S, K> {
    fn append(&mut self, entry: &GeneralEntry) -> Result<Receipt, AppendFailure> {
        self.append_through(entry.entry(), Through::General)
    }
}

impl<S: Store, K: KeptStatements> PolicyLog for AuditLog<S, K> {
    fn append_decision(&mut self, entry: &DecisionEntry) -> Result<Receipt, AppendFailure> {
        self.append_through(&Entry::Verified(entry.entry().clone()), Through::Policy)
    }
}

/// A general handle on a shared log: it implements [`Log`] and nothing else, so a part holding it, `core-audit`'s
/// own checks among them (`T-022-05`, `T-022-06`), can write no decision entry and, since its entries reach the log
/// unverified, attribute no statement.
pub struct General<L>(Rc<RefCell<L>>);

impl<L: Log> General<L> {
    /// A general handle on `log`.
    pub fn new(log: &Rc<RefCell<L>>) -> Self {
        General(Rc::clone(log))
    }
}

impl<L: Log> Log for General<L> {
    fn append(&mut self, entry: &GeneralEntry) -> Result<Receipt, AppendFailure> {
        self.0.borrow_mut().append(entry)
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
            log.append_decision(&fixture!("verified-entry-of-a-decision")),
            unavailable
        );
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
