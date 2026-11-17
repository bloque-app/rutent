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
//! **Opening** reads every record back and runs step 2 again on each stated entry the store marks `verified`, over the
//! history before it, so an entry no kept statement bears out doesn't open as sound; a statement erased since is no
//! loss, and the log opens ([`AuditLog::open`], `T-022-08`).
//!
//! `core-audit`'s own checks (`T-022-05`, `T-022-06`) append through a [`General`] handle, never through this type,
//! so they can write no decision entry, and, since a [`GeneralEntry`] holds none attributed `verified`, attribute no
//! statement. [`Policy`] is the matching handle for the policy engine, which writes nothing else.

pub mod kept;
pub mod keys;
pub mod merkle;
pub mod signatures;
pub mod store;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use rutent_contracts::core_internal::calls::{
    AppendFailure, DECISION_ON_THE_GENERAL_HANDLE, DecisionEntry, GeneralEntry, KeptStatements,
    Log, PolicyLog, RECORD_ON_THE_POLICY_HANDLE, is_decision,
};
use rutent_contracts::core_internal::{
    Attribution, Entry, KeptStatement, Receipt, STATED_WITHOUT_ITS_KEPT_BYTES, StatedEntry,
    Through, Violation, admit_entry, attribute,
};
use rutent_contracts::module_interface::{Digest, DigestAlgorithm, DigestValue};
use rutent_contracts::provider_role::{ADVERTISEMENT_SHAPE, SafeInteger, to_canonical};
use rutent_contracts::router_api::Id;

pub use kept::KeptInMemory;
pub use keys::{Clock, Settings, SystemClock};
pub use merkle::Hash;
pub use signatures::Ed25519Strict;
pub use store::{FileStore, MemoryStore, Store, StoreError};

use keys::{Keys, Register};

/// The refusal of an entry with no canonical JSON.
pub const NO_CANONICAL_FORM: &str = "an entry of the log has an RFC 8785 canonical form";

/// The refusal to open a store holding a record that isn't an entry in its canonical form.
pub const NOT_A_RECORD_OF_THE_LOG: &str =
    "every record of the log's store is an entry in its RFC 8785 canonical form";

/// The refusal to open a store holding an advertisement whose bytes the evidence store doesn't keep as its
/// commitment names them: the register read from the log would come back without it, a rotation lost or a dropped
/// key revived. An advertisement is a provider's public statement, which no erasure reaches (`REQ-AUD-13`).
pub const ADVERTISEMENT_NOT_KEPT: &str =
    "every advertisement the log recorded is kept, its bytes bearing out its commitment";

/// The refusal of a stated entry whose kept bytes don't bear out its commitment: SHA-256 of the salt and the bytes.
pub const KEPT_BYTES_DIFFER: &str = "a stated entry's kept bytes bear out its commitment: SHA-256 of the salt followed by the bytes";

/// The refusal to open a store holding a stated entry marked `verified` that the statement kept under its commitment
/// contradicts: the append's own attribution, run again over the history before it, the register as it then stood
/// and the kept bytes, doesn't reach the entry the store holds (`REQ-AUD-2`, `T-022-08`).
pub const KEPT_STATEMENT_CONTRADICTS: &str = "every stated entry the log's store marks verified is borne out, on opening, by the statement kept under its commitment, as its append attributed it";

/// Why the log didn't open ([`AuditLog::open_naming`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotOpened {
    /// As [`AuditLog::open`] answers: a store or an evidence store that can't be read now, or a record that breaks
    /// a rule of the log's.
    Failed(AppendFailure),
    /// A stated entry the store marks `verified` that the statement kept under its commitment contradicts: the
    /// receipt it has at its place in the store. Nothing is repaired or dropped.
    Contradicted {
        /// The entry.
        entry: Receipt,
    },
}

impl From<AppendFailure> for NotOpened {
    fn from(failure: AppendFailure) -> Self {
        NotOpened::Failed(failure)
    }
}

impl From<NotOpened> for AppendFailure {
    fn from(not: NotOpened) -> Self {
        match not {
            NotOpened::Failed(failure) => failure,
            NotOpened::Contradicted { .. } => {
                AppendFailure::Refused(Violation(KEPT_STATEMENT_CONTRADICTS))
            }
        }
    }
}

/// The receipt of the record `bytes` at `index` of the log named `log`: SHA-256 of the record, at its place.
fn receipt_of(log: &Id, index: usize, bytes: &[u8]) -> Receipt {
    Receipt {
        log: log.clone(),
        index: SafeInteger::try_from(index as u64).expect("a log of fewer than 2^53 entries"),
        entry: digest(&merkle::sha256(bytes)),
    }
}

/// The entries `records` hold, as the log's file holds them, each with the receipt it has at its place in the log
/// named `log`, read as the log reads them back but with nothing attributed, kept or registered: what a reader
/// outside the log holds (`audit-check`). A record that isn't an entry in its canonical form is passed over, its
/// place kept, for the checker finds it (`Finding::RecordNotAnEntry`).
pub fn read_history(log: &Id, records: &[Vec<u8>]) -> Vec<(Receipt, Entry)> {
    records
        .iter()
        .enumerate()
        .filter_map(|(index, bytes)| {
            entry_of(bytes).map(|entry| (receipt_of(log, index, bytes), entry))
        })
        .collect()
}

/// The entry `bytes` are the canonical record of, where they are one.
fn entry_of(bytes: &[u8]) -> Option<Entry> {
    serde_json::from_slice(bytes)
        .ok()
        .filter(|e: &Entry| e.check().is_ok() && record(e).ok().as_deref() == Some(bytes))
}

/// Whether `entry` is a stated entry of an advertisement's shape.
fn is_advertisement(entry: &Entry) -> bool {
    matches!(entry, Entry::Stated(s) if s.what.statement.as_str() == ADVERTISEMENT_SHAPE)
}

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
    state: State,
    /// A write failed, so the store may hold a record the history doesn't: read it back before the next append.
    stale: bool,
}

/// What the log holds in memory, all read from its store: the history, the tree's leaves, each record's place by its
/// digest, and the register.
#[derive(Default)]
struct State {
    history: Vec<(Receipt, Entry)>,
    leaves: Vec<Hash>,
    recorded: HashMap<Hash, usize>,
    register: Register,
}

impl State {
    /// Takes a record the store holds into the history, the tree and the register, and answers its receipt.
    fn take(
        &mut self,
        settings: &Settings,
        bytes: &[u8],
        entry: Entry,
        advertisement: Option<&[u8]>,
    ) -> Receipt {
        let hash = merkle::sha256(bytes);
        let index = self.history.len();
        let receipt = receipt_of(&settings.log, index, bytes);
        self.register
            .record(settings, &self.history, &entry, advertisement);
        self.leaves.push(merkle::leaf_hash(bytes));
        self.recorded.insert(hash, index);
        self.history.push((receipt.clone(), entry));
        receipt
    }
}

/// The bytes `kept` holds, where they bear its commitment out: SHA-256 of the decoded salt followed by them.
fn bearing_out(kept: &KeptStatement) -> Option<Vec<u8>> {
    let mut salted = kept::from_base64url(kept.salt.as_str())?;
    let bytes = kept::from_base64url(kept.bytes.as_str())?;
    salted.extend_from_slice(&bytes);
    (digest(&merkle::sha256(&salted)) == kept.commitment).then_some(bytes)
}

/// What the log's attribution of a stated entry reached.
enum Attributed {
    /// The evidence store keeps nothing under the entry's commitment: never kept, or erased since (`REQ-AUD-13`). A
    /// statement answered under another commitment than the one asked is nothing kept under that one.
    NotKept,
    /// The evidence store keeps bytes under the commitment that don't bear it out.
    BytesDiffer,
    /// The entry the log records, attributed from the kept bytes, and those bytes.
    Recorded(Box<StatedEntry>, Vec<u8>),
}

/// The log's one attribution of `claim`, a stated entry as its writer hands it over, unverified: the bytes kept under
/// its commitment held to it (SHA-256 of the salt followed by the bytes), then `core-internal`'s [`attribute`] over
/// `state`'s history and register as they stand before the entry, with a strict Ed25519 check. The append runs it on
/// every stated entry; the opening runs it again on every one the store marks `verified`, over the history and the
/// register read back to that entry's place, which are the ones the append ran it over (`T-022-08`). A store it
/// can't read fails it, so neither path takes an unread store for an erased statement.
fn attribute_stated<K: KeptStatements>(
    settings: &Settings,
    state: &State,
    kept: &mut K,
    claim: &StatedEntry,
) -> Result<Attributed, AppendFailure> {
    let Some(statement) = kept
        .kept(&claim.commitment)?
        .filter(|k| k.commitment == claim.commitment)
    else {
        return Ok(Attributed::NotKept);
    };
    let Some(bytes) = bearing_out(&statement) else {
        return Ok(Attributed::BytesDiffer);
    };
    let mut keys = Keys {
        settings,
        register: &state.register,
    };
    let recorded = attribute(claim, &state.history, kept, &mut keys, &Ed25519Strict)?;
    Ok(Attributed::Recorded(Box::new(recorded), bytes))
}

impl<S: Store, K: KeptStatements> AuditLog<S, K> {
    /// Opens the log kept in `store`, reading back every record it holds, with `kept` the evidence store's read and
    /// `settings` the deployment's. It refuses a store holding a record that isn't an entry in its canonical form,
    /// and one holding an advertisement whose bytes `kept` doesn't keep as its commitment names them: the register
    /// would otherwise come back without it.
    ///
    /// It re-attributes what it reads back (`REQ-AUD-2`, `T-022-08`): every stated entry the store marks `verified` is
    /// held to the statement kept under its commitment by the append's own attribution, over the history and the
    /// register before it, so by the same checks (commitment, deployment, maker, shape, time, key, transaction, a
    /// strict signature, and, from the register, that the key is the maker's and the party the one it states for).
    /// Where the kept statement doesn't reach the entry the store holds, the opening is refused
    /// ([`KEPT_STATEMENT_CONTRADICTS`]; [`AuditLog::open_naming`] names the entry), and nothing is repaired or
    /// dropped. Where the evidence store keeps nothing under the commitment, the statement was erased since, which is
    /// no loss: history must still check after an erasure (`REQ-AUD-13`, `AC-36`), so the log opens, and the bound
    /// evidence store reports bytes gone with no erasure (`receipts::BoundStore::missing`). An evidence store that
    /// can't be read answers [`AppendFailure::Unavailable`], and the log doesn't open. An entry marked unverified is
    /// held to nothing here.
    ///
    /// Its cost: one read of the evidence store and one strict Ed25519 check for each `verified` stated entry, beside
    /// the read back the log already makes; for a module the operator publishes, `attribute` also walks the history
    /// for its releases, as its append did.
    pub fn open(settings: Settings, store: S, kept: K) -> Result<Self, AppendFailure> {
        Self::open_naming(settings, store, kept).map_err(AppendFailure::from)
    }

    /// Opens the log as [`AuditLog::open`] does, naming the entry a kept statement contradicts where that refuses
    /// the opening ([`NotOpened::Contradicted`]).
    pub fn open_naming(settings: Settings, store: S, kept: K) -> Result<Self, NotOpened> {
        let mut log = AuditLog {
            settings,
            store,
            kept,
            state: State::default(),
            stale: true,
        };
        log.state = log.read_back()?;
        log.stale = false;
        Ok(log)
    }

    /// The history, the tree and the register as the store and the evidence store hold them, built apart, so a read
    /// that fails part-way leaves the log's own as they were. Each stated entry marked `verified` is re-attributed as
    /// [`AuditLog::open`] says.
    fn read_back(&mut self) -> Result<State, NotOpened> {
        let records = self.store.records().map_err(|e| match e {
            StoreError::Unavailable(_) => AppendFailure::Unavailable,
            StoreError::Corrupt(_) => AppendFailure::Refused(Violation(NOT_A_RECORD_OF_THE_LOG)),
        })?;
        let mut state = State::default();
        for bytes in records {
            let entry = entry_of(&bytes)
                .ok_or(AppendFailure::Refused(Violation(NOT_A_RECORD_OF_THE_LOG)))?;
            let advertisement = match &entry {
                Entry::Stated(s) if is_advertisement(&entry) => Some(
                    self.kept
                        .kept(&s.commitment)?
                        .filter(|k| k.commitment == s.commitment)
                        .and_then(|k| bearing_out(&k))
                        .ok_or(AppendFailure::Refused(Violation(ADVERTISEMENT_NOT_KEPT)))?,
                ),
                // Only an advertisement's bytes are read back for the register: a statement erased since is no loss
                // to it (`REQ-AUD-13`).
                _ => None,
            };
            if let Entry::Stated(stated) = &entry
                && let Attribution::Verified { who } = &stated.attribution
            {
                let claim = StatedEntry {
                    attribution: Attribution::Unverified {
                        claimed_by: who.clone(),
                    },
                    ..stated.clone()
                };
                match attribute_stated(&self.settings, &state, &mut self.kept, &claim)? {
                    Attributed::NotKept => {}
                    Attributed::Recorded(again, _) if *again == *stated => {}
                    Attributed::Recorded(..) | Attributed::BytesDiffer => {
                        return Err(NotOpened::Contradicted {
                            entry: receipt_of(&self.settings.log, state.history.len(), &bytes),
                        });
                    }
                }
            }
            state.take(&self.settings, &bytes, entry, advertisement.as_deref());
        }
        Ok(state)
    }

    /// Appends `entry`, which came by `through`, as the module's steps say.
    fn append_through(
        &mut self,
        entry: &Entry,
        through: Through,
    ) -> Result<Receipt, AppendFailure> {
        // A store that can't be read back now says nothing of the entry, which breaks no rule: its writer keeps it
        // and appends it again (`REQ-MDL-8`). What the log answered before must still be there, at its place.
        if self.stale {
            let state = self.read_back().map_err(|_| AppendFailure::Unavailable)?;
            if !state.history.starts_with(&self.state.history) {
                return Err(AppendFailure::Unavailable);
            }
            self.state = state;
            self.stale = false;
        }
        entry.check().map_err(AppendFailure::Refused)?;
        // The path is held to the entry's kind before anything else, the retry rule included, as `admit_entry`
        // holds it again: the handles' types already keep each kind to its path.
        match (
            through,
            matches!(entry, Entry::Verified(v) if is_decision(&v.what)),
        ) {
            (Through::General, true) => {
                return Err(AppendFailure::Refused(Violation(
                    DECISION_ON_THE_GENERAL_HANDLE,
                )));
            }
            (Through::Policy, false) => {
                return Err(AppendFailure::Refused(Violation(
                    RECORD_ON_THE_POLICY_HANDLE,
                )));
            }
            _ => {}
        }
        let (entry, advertisement) = match entry {
            Entry::Stated(claim) => {
                match attribute_stated(&self.settings, &self.state, &mut self.kept, claim)? {
                    Attributed::NotKept => {
                        return Err(AppendFailure::Refused(Violation(
                            STATED_WITHOUT_ITS_KEPT_BYTES,
                        )));
                    }
                    Attributed::BytesDiffer => {
                        return Err(AppendFailure::Refused(Violation(KEPT_BYTES_DIFFER)));
                    }
                    Attributed::Recorded(recorded, bytes) => (
                        Entry::Stated(*recorded),
                        is_advertisement(entry).then_some(bytes),
                    ),
                }
            }
            Entry::Verified(_) => (entry.clone(), None),
        };
        let bytes = record(&entry).map_err(AppendFailure::Refused)?;
        if let Some(&index) = self.state.recorded.get(&merkle::sha256(&bytes)) {
            return Ok(self.state.history[index].0.clone());
        }
        // The deployment's operator as the rows read it: the maker it signs its own statements as, and its party.
        let operator = rutent_contracts::core_internal::DeploymentOperator {
            maker: self.settings.operator.clone(),
            party: self.settings.operator_party.clone(),
        };
        admit_entry(&self.state.history, &entry, through, &operator)
            .map_err(AppendFailure::Refused)?;
        self.state.register.admits(&self.settings, &entry)?;
        if self.store.append(&bytes).is_err() {
            self.stale = true;
            return Err(AppendFailure::Unavailable);
        }
        Ok(self
            .state
            .take(&self.settings, &bytes, entry, advertisement.as_deref()))
    }

    /// Every entry of the log with its receipt, in order.
    pub fn entries(&self) -> &[(Receipt, Entry)] {
        &self.state.history
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        self.state.history.len()
    }

    /// Whether the log holds no entry.
    pub fn is_empty(&self) -> bool {
        self.state.history.is_empty()
    }

    /// The root of the tree over every entry (RFC 9162 §2.1).
    pub fn root(&self) -> Hash {
        merkle::root(&self.state.leaves)
    }

    /// The root of the tree over the first `size` entries, or `None` where the log holds fewer.
    pub fn root_at(&self, size: usize) -> Option<Hash> {
        let leaves = &self.state.leaves;
        (size <= leaves.len()).then(|| merkle::root(&leaves[..size]))
    }

    /// The inclusion proof of entry `index` in the tree over the first `size` entries, or `None` where there is no
    /// such entry (RFC 9162 §2.1.3); [`merkle::verify_inclusion`] checks it.
    pub fn inclusion(&self, index: usize, size: usize) -> Option<Vec<Hash>> {
        let leaves = &self.state.leaves;
        (size <= leaves.len())
            .then(|| merkle::inclusion_proof(index, &leaves[..size]))
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

/// Policy's handle on a shared log: it implements [`PolicyLog`] and nothing else, and isn't `Clone`, so the one
/// whoever holds the log hands the policy engine is the only one it made.
pub struct Policy<L>(Rc<RefCell<L>>);

impl<L: PolicyLog> Policy<L> {
    /// Policy's handle on `log`.
    pub fn new(log: &Rc<RefCell<L>>) -> Self {
        Policy(Rc::clone(log))
    }
}

impl<L: PolicyLog> PolicyLog for Policy<L> {
    fn append_decision(&mut self, entry: &DecisionEntry) -> Result<Receipt, AppendFailure> {
        self.0.borrow_mut().append_decision(entry)
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
