//! A party's receipt (ADR-0005, `REQ-AUD-5`, `T-022-03`): in the style of RFC 9943's receipts, the log's signed
//! word that an entry sits at its index in the tree whose root it names, with the inclusion proof that shows it
//! (RFC 9162 §2.1.3). A party asks for one of an entry about its own part, and keeps it with the entry and, for a
//! statement, the kept bytes it fetched: what it stated and what it was told. Later, against any history the
//! operator shows, [`holds_against`] says whether the record the receipt was issued for is still there as it was. A
//! rewrite, or a removal, by anyone with the operator's access shows, since the operator can't sign again a receipt
//! the party already holds.
//!
//! **The log's own key signs it**, apart from the deployment's `operator_key`, which signs Core's questions as the
//! operator's statements: a key signs for one purpose (`Issuer::new` refuses the operator's). The receipt is signed
//! as provider-role signs a statement, Ed25519 over the RFC 8785 canonical JSON without `signature.value`, and is
//! checked by the catalogue's strict verifier; its `shape`, [`RECEIPT_SHAPE`], keeps it from reading as any
//! statement.
//!
//! **Only on a read policy allows.** The issuer asks policy ([`calls::Reads`]) first, then issues only for an entry
//! the read covers: a kept statement's stated entry for a read of that statement (`reads: evidence`, the party's
//! own, `REQ-AUD-5`), an entry naming the read's order (and segment) for a read of an order (or a segment), and any
//! entry for a read of the trail. A read of an order's state alone covers no entry.

use std::cell::RefCell;
use std::rc::Rc;

use ed25519_dalek::{Signer as _, SigningKey};
use rutent_contracts::core_internal::calls::{self, AppendFailure, KeptStatements, Signatures};
use rutent_contracts::core_internal::{
    Entry, KeptStatement, ReadAnswer, ReadCheck, ReadOutcome, Reads as Read, Receipt, StatedEntry,
    Violation,
};
use rutent_contracts::module_interface::Digest;
use rutent_contracts::provider_role::{PublicKey, SafeInteger, signing_input};
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_prov::catalogue::verify_envelope;
use serde::{Deserialize, Serialize};

use super::store::bearing_out;
use crate::log::kept::base64url;
use crate::log::merkle::{self, Hash, leaf_hash, sha256, verify_inclusion};
use crate::log::{AuditLog, Ed25519Strict, Store, digest, record};

/// A party's receipt's `shape`.
pub const RECEIPT_SHAPE: &str = "urn:rutent:core-audit:party-receipt";

/// The refusal of an issuer whose key is the deployment's `operator_key`.
pub const KEY_ACROSS_PURPOSES: &str =
    "a party's receipt is signed with the log's own key, never the operator's statement key";

/// The refusal of a receipt of an entry the read doesn't cover, or of no entry.
pub const NOT_COVERED: &str =
    "a receipt is issued only for an entry of the log the read allowed covers";

/// The refusal of a receipt that isn't one, isn't signed by the log's key, or doesn't show its entry in its tree.
pub const NOT_A_RECEIPT: &str = "a party's receipt is the log's signed word that the entry it names sits in the tree whose root it names";

/// The refusal of a kept statement that isn't the one its stated entry names.
pub const NOT_THE_ENTRYS_STATEMENT: &str = "a kept statement bears out its stated entry's commitment, and its signature is the entry's key's";

/// The signature on a party's receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptSignature {
    /// Always `ed25519`.
    pub scheme: String,
    /// The log's key.
    pub key: PublicKey,
    /// The signature, base64url without padding.
    pub value: String,
}

/// A party's receipt, as it is signed and kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartyReceipt {
    /// Always [`RECEIPT_SHAPE`].
    pub shape: String,
    /// The log.
    pub log: Id,
    /// The entry's position in it, from 0.
    pub index: SafeInteger,
    /// The SHA-256 digest of the entry's record, its RFC 8785 canonical JSON: the log's own receipt's digest.
    pub entry: Digest,
    /// The size of the tree the receipt is of: the log's, when it was issued.
    pub tree_size: SafeInteger,
    /// The root of that tree (RFC 9162 §2.1).
    pub root: Digest,
    /// The inclusion proof of the entry in that tree, innermost sibling first (RFC 9162 §2.1.3).
    pub inclusion: Vec<Digest>,
    /// When it was issued, by the log's clock.
    pub issued_at: Timestamp,
    /// The log's signature.
    pub signature: ReceiptSignature,
}

/// What the issuer answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Issued {
    /// The receipt, its signed bytes as the party keeps them.
    Receipt(Vec<u8>),
    /// Policy refused the read, and recorded it.
    Refused(ReadAnswer),
}

/// `nanos` after the epoch as an RFC 3339 UTC time, to the second.
fn timestamp(nanos: i128) -> Option<Timestamp> {
    let secs = i64::try_from(nanos.div_euclid(1_000_000_000)).ok()?;
    let (days, secs) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Days since 1970-01-01 to a civil date (proleptic Gregorian).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    Timestamp::new(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs / 3_600,
        secs / 60 % 60,
        secs % 60
    ))
}

/// A hash as the contract's [`Digest`], and back.
fn hash_of(d: &Digest) -> Option<Hash> {
    let hex = d.value.as_str();
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

/// Whether `entry` names `order`, and `segment` where one is given: a stated entry by its `about`, a verified one
/// by its `what`'s own `order` and `segment`.
fn names(entry: &Entry, order: &Id, segment: Option<&Id>) -> bool {
    let (o, s) = match entry {
        Entry::Stated(s) => match &s.about {
            Some(a) => (a.order.clone(), a.segment.clone()),
            None => (None, None),
        },
        Entry::Verified(v) => {
            let what = serde_json::to_value(&v.what).unwrap_or_default();
            let id = |field: &str| what[field].as_str().and_then(Id::new);
            (id("order"), id("segment"))
        }
    };
    o.as_ref() == Some(order) && segment.is_none_or(|segment| s.as_ref() == Some(segment))
}

/// Whether a read of `reads` covers `entry`.
fn covers(reads: &Read, entry: &Entry) -> bool {
    match reads {
        Read::Evidence { commitment } => {
            matches!(entry, Entry::Stated(s) if s.commitment == *commitment)
        }
        Read::Order { order } => names(entry, order, None),
        // An order's recorded state and age is the state machine's reading of the order, which no single entry of
        // the log is: the inputs that make it say more (a route chosen, its decision record), so none is covered.
        Read::OrderState { .. } => false,
        Read::Segment { order, segment, .. } => names(entry, order, Some(segment)),
        Read::Trail {} => true,
        Read::Resource { .. } => false,
    }
}

/// The issuer of parties' receipts: the log's own signing key, bound once to the log it signs for and to policy's
/// read check, as [`Keeper`](super::Keeper) is. Only the assembly makes one (`T-024-02`).
///
/// The secret is held as an [`ed25519_dalek::SigningKey`], which zeroises itself when dropped (the crate's `zeroize`
/// feature); the caller hands it over by value, so no copy of it stays with the issuer's maker beyond its own.
pub struct Issuer<S: Store, K: KeptStatements> {
    key: SigningKey,
    public: PublicKey,
    log: Rc<RefCell<AuditLog<S, K>>>,
    reads: Box<dyn calls::Reads>,
}

impl<S: Store, K: KeptStatements> Issuer<S, K> {
    /// The issuer signing with `key` for `log`, issuing on what `reads`, policy's read check, allows. It refuses
    /// the deployment's `operator_key`, which signs the operator's statements.
    pub fn new(
        log: Rc<RefCell<AuditLog<S, K>>>,
        reads: Box<dyn calls::Reads>,
        key: SigningKey,
    ) -> Result<Self, Violation> {
        let public = PublicKey::new(base64url(key.verifying_key().as_bytes()))
            .expect("an Ed25519 key's 32 bytes in base64url are a public key");
        if public == log.borrow().settings().deployment.operator_key {
            return Err(Violation(KEY_ACROSS_PURPOSES));
        }
        Ok(Issuer {
            key,
            public,
            log,
            reads,
        })
    }

    /// The log's key, which every receipt it issues is checked against.
    pub fn key(&self) -> &PublicKey {
        &self.public
    }

    /// Issues `check.reader` a receipt of the entry at `index` of the log, on policy's allowing `check`, and only
    /// where the read covers that entry. Policy is asked first, with no borrow of the log held, since it records
    /// the read there.
    pub fn issue(&mut self, check: &ReadCheck, index: usize) -> Result<Issued, AppendFailure> {
        let answer = self.reads.check_read(check)?;
        if answer.request != check.request {
            return Err(AppendFailure::Refused(Violation(
                super::store::ANSWER_OF_ANOTHER_READ,
            )));
        }
        if let ReadOutcome::Refused { .. } = answer.outcome {
            return Ok(Issued::Refused(answer));
        }
        let log = self.log.borrow();
        let refused = || AppendFailure::Refused(Violation(NOT_COVERED));
        let (receipt, entry) = log.entries().get(index).ok_or_else(refused)?;
        if !covers(&check.reads, entry) {
            return Err(refused());
        }
        let size = log.len();
        let root = log.root_at(size).ok_or_else(refused)?;
        let inclusion = log.inclusion(index, size).ok_or_else(refused)?;
        let issued_at =
            timestamp(log.settings().clock.unix_nanos()).ok_or(AppendFailure::Unavailable)?;
        let mut receipt = PartyReceipt {
            shape: RECEIPT_SHAPE.to_owned(),
            log: receipt.log.clone(),
            index: receipt.index,
            entry: receipt.entry.clone(),
            tree_size: SafeInteger::try_from(size as u64)
                .expect("a log of fewer than 2^53 entries"),
            root: digest(&root),
            inclusion: inclusion.iter().map(digest).collect(),
            issued_at,
            signature: ReceiptSignature {
                scheme: "ed25519".to_owned(),
                key: self.public.clone(),
                value: String::new(),
            },
        };
        let unsigned = serde_json::to_vec(&receipt).map_err(|_| AppendFailure::Unavailable)?;
        let input = signing_input(&unsigned).map_err(|_| AppendFailure::Unavailable)?;
        receipt.signature.value = base64url(&self.key.sign(&input).to_bytes());
        serde_json::to_vec(&receipt)
            .map(Issued::Receipt)
            .map_err(|_| AppendFailure::Unavailable)
    }
}

/// Reads `bytes` as a party's receipt of `entry` from the log named `log`, signed with `key`, that log's: it must
/// be one, of that log, its signature strictly `key`'s over its signing input, its entry digest that of `entry`'s
/// record, and its inclusion proof must lead from that record to its root. Run it, and keep only the receipt it
/// answers, before [`holds_against`], which trusts the receipt it is given.
pub fn check_receipt(
    bytes: &[u8],
    key: &PublicKey,
    log: &Id,
    entry: &Entry,
) -> Result<PartyReceipt, Violation> {
    let refused = || Violation(NOT_A_RECEIPT);
    let receipt: PartyReceipt = serde_json::from_slice(bytes).map_err(|_| refused())?;
    let record = record(entry)?;
    let (index, size) = (
        receipt.index.get() as usize,
        receipt.tree_size.get() as usize,
    );
    let proof: Option<Vec<Hash>> = receipt.inclusion.iter().map(hash_of).collect();
    let holds = receipt.shape == RECEIPT_SHAPE
        && receipt.log == *log
        && receipt.signature.scheme == "ed25519"
        && receipt.signature.key == *key
        && verify_envelope(key, &receipt.signature.value, bytes).is_ok()
        && receipt.entry == digest(&sha256(&record))
        && match (proof, hash_of(&receipt.root)) {
            (Some(proof), Some(root)) => {
                verify_inclusion(&leaf_hash(&record), index, size, &proof, &root)
            }
            _ => false,
        };
    holds.then_some(receipt).ok_or_else(refused)
}

/// Whether `history`, as the operator now shows it, still holds what `receipt` was issued for: the entry it names,
/// at its index, in a tree of its size whose root is the one it names. A record changed, or removed, before that
/// size answers `false`.
///
/// It trusts `receipt`: a [`PartyReceipt`] can be built or read by anyone, so hand it only one
/// [`check_receipt`] answered, which holds the log's signature over it.
pub fn holds_against(receipt: &PartyReceipt, history: &[(Receipt, Entry)]) -> bool {
    let (index, size) = (
        receipt.index.get() as usize,
        receipt.tree_size.get() as usize,
    );
    let Some(history) = history.get(..size) else {
        return false;
    };
    let records: Option<Vec<Vec<u8>>> = history.iter().map(|(_, e)| record(e).ok()).collect();
    let Some(records) = records else {
        return false;
    };
    let leaves: Vec<Hash> = records.iter().map(|r| leaf_hash(r)).collect();
    records
        .get(index)
        .is_some_and(|r| digest(&sha256(r)) == receipt.entry)
        && digest(&merkle::root(&leaves)) == receipt.root
}

/// Checks `kept` against the stated entry that names it, as `core-internal`'s README gives the steps: SHA-256 of
/// its salt followed by its bytes is the entry's commitment, and the bytes are an envelope whose signature's scheme
/// and key are the entry's `signed_with` and whose signature verifies over its signing input.
pub fn check_kept(kept: &KeptStatement, entry: &StatedEntry) -> Result<(), Violation> {
    let refused = || Violation(NOT_THE_ENTRYS_STATEMENT);
    if kept.commitment != entry.commitment {
        return Err(refused());
    }
    let bytes = bearing_out(kept).ok_or_else(refused)?;
    let envelope: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| refused())?;
    let signed_with = serde_json::to_value(&entry.signed_with).map_err(|_| refused())?;
    let named = envelope["signature"]["scheme"] == signed_with["scheme"]
        && envelope["signature"]["key"] == signed_with["key"];
    (named && Ed25519Strict.verifies(&bytes))
        .then_some(())
        .ok_or_else(refused)
}
