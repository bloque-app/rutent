//! Types mirroring the `urn:rutent:provider-role:*` schemas. Field names match the schemas, and every struct
//! denies unknown fields, as the schemas set `additionalProperties: false`.

use alloc::{borrow::ToOwned, format, string::String, vec::Vec};
use serde::{Deserialize, Deserializer, Serialize};

use super::envelope::{ADVERTISEMENT_SHAPE, StatementShape};

/// A provider's advertisement in the catalogue, the statement inside a `urn:rutent:provider-role:advertisement`
/// envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Advertisement {
    /// The provider's identifier in the catalogue.
    pub provider: String,
    /// The publisher behind the provider, and behind every private node it declares (`REQ-CAT-20`).
    pub publisher: String,
    /// The version of the provider this advertisement belongs to.
    pub version: String,
    /// The provider's public signing keys, made authoritative by the operator's grant (`REQ-CAT-2`).
    pub keys: Vec<SigningKey>,
    /// What the provider advertises, by category (`REQ-CAT-8`).
    pub offers: Offers,
    /// The provider-wide declarations (`REQ-CAT-10`, `REQ-CAT-16`, `REQ-ORD-39`).
    pub declarations: Declarations,
    /// The identifiers of the private nodes this provider declares and publishes, each once (`REQ-CAT-22`).
    /// Their publisher is the advertisement's `publisher`.
    pub private_nodes: Vec<String>,
}

impl StatementShape for Advertisement {
    const SHAPE: &'static str = ADVERTISEMENT_SHAPE;
}

/// One of a provider's public signing keys, with what it may sign.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigningKey {
    /// The scheme.
    pub scheme: super::envelope::SignatureScheme,
    /// The Ed25519 public key.
    pub public_key: PublicKey,
    /// What the key may sign.
    pub purpose: KeyPurpose,
}

/// What a provider's key may sign (`THR-4`). The two never overlap: a key is listed once, with one purpose.
///
/// - An **`advertising`** key signs the provider's advertisements and its modules' release assertions. Only the
///   provider holds it; no running module can resolve it.
/// - A **`statements`** key signs everything else the provider says: results, quotes, judgements and reversal
///   confirmations. A module's outside part resolves it through the credential proxy (its `statement_key`), so a
///   compromised outside part holds it. It therefore never signs an advertisement or a release assertion, and an
///   advertisement it signed can't add or promote an advertising key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyPurpose {
    /// Signs advertisements and release assertions.
    Advertising,
    /// Signs the provider's other statements.
    Statements,
}

/// An Ed25519 public key (RFC 8032): 32 bytes, written as 43 characters of base64url without padding, in its one
/// canonical encoding, as `urn:rutent:provider-role:common#/$defs/public_key` writes it.
///
/// Built only from canonical text: 43 characters of the base64url alphabet whose last character carries no unused
/// bits (one of `AEIMQUYcgkosw048`). Two keys are equal when their decoded bytes are, never by comparing text, so
/// an alias can't pass a "differs from" check.
///
/// This type checks the encoding only. The verifier (`M2`) must also refuse what this can't see: non-canonical
/// base64 from any decoder it uses (a last character with nonzero unused bits, padding, or the `+`/`/` alphabet),
/// and a non-canonical point: a `y` coordinate at or above the field prime 2^255 − 19, as RFC 8032 §5.1.3's strict
/// decoding does, with a small-order key refused as well.
#[derive(Clone)]
pub struct PublicKey {
    /// Canonical text, checked when built; the bytes are decoded from it when compared.
    text: String,
}

impl PublicKey {
    /// The key written by `text`, or `None` if `text` isn't a key in its canonical encoding.
    pub fn new(text: impl Into<String>) -> Option<Self> {
        let text = text.into();
        decode_public_key(&text)?;
        Some(Self { text })
    }

    /// The key as its canonical text.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The key's 32 bytes.
    pub fn bytes(&self) -> [u8; 32] {
        decode_public_key(&self.text).expect("checked when built")
    }
}

impl PartialEq for PublicKey {
    fn eq(&self, other: &Self) -> bool {
        self.bytes() == other.bytes()
    }
}

impl Eq for PublicKey {}

impl core::hash::Hash for PublicKey {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.bytes().hash(state);
    }
}

impl PartialOrd for PublicKey {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PublicKey {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.bytes().cmp(&other.bytes())
    }
}

impl core::fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("PublicKey").field(&self.text).finish()
    }
}

impl core::fmt::Display for PublicKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.text)
    }
}

impl TryFrom<String> for PublicKey {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::new(text).ok_or_else(|| {
            "a public key must be 43 base64url characters in their canonical encoding".to_owned()
        })
    }
}

impl From<PublicKey> for String {
    fn from(key: PublicKey) -> Self {
        key.text
    }
}

impl Serialize for PublicKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.text)
    }
}

impl<'de> Deserialize<'de> for PublicKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::try_from(text).map_err(serde::de::Error::custom)
    }
}

/// The 32 bytes `text` writes, if it is 43 characters of base64url without padding whose two unused bits are
/// zero, the one canonical encoding.
fn decode_public_key(text: &str) -> Option<[u8; 32]> {
    let digit = |c: u8| -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some(u32::from(c - b'A')),
            b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    };
    let chars = text.as_bytes();
    if chars.len() != 43 {
        return None;
    }
    let mut bytes = [0u8; 32];
    let (mut acc, mut held, mut out) = (0u32, 0u32, 0usize);
    for &c in chars {
        acc = (acc << 6) | digit(c)?;
        held += 6;
        if held >= 8 {
            held -= 8;
            bytes[out] = (acc >> held) as u8;
            out += 1;
            acc &= (1 << held) - 1;
        }
    }
    // 43 characters carry 258 bits: 32 bytes, and two left over that must be zero.
    (out == 32 && held == 2 && acc == 0).then_some(bytes)
}

/// A non-negative integer no larger than 2^53 - 1, so it survives RFC 8785 canonical JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct SafeInteger(u64);

impl SafeInteger {
    /// The largest value: 2^53 - 1.
    pub const MAX: u64 = (1 << 53) - 1;

    /// The value.
    pub fn get(self) -> u64 {
        self.0
    }
}

impl TryFrom<u64> for SafeInteger {
    type Error = String;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        if value <= Self::MAX {
            Ok(Self(value))
        } else {
            Err(format!("{value} is above 2^53 - 1"))
        }
    }
}

impl From<SafeInteger> for u64 {
    fn from(value: SafeInteger) -> Self {
        value.0
    }
}

/// The four categories (`REQ-CAT-8`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Where value is stored: connects an outer node and an inner node.
    Reserve,
    /// Where asset types are interchanged, within one node.
    Exchange,
    /// Moves one asset between two nodes.
    Ramp,
    /// Exposes judgements about a person and a class; moves no value.
    Verifier,
}

impl Category {
    /// Whether the category moves value: every category but the verifier.
    pub fn moves_value(self) -> bool {
        !matches!(self, Self::Verifier)
    }
}

/// What a provider advertises, keyed by category. At least one is present; a provider may advertise under
/// several (`REQ-CAT-8`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Offers {
    /// The reserve offer, if any.
    #[serde(
        default,
        deserialize_with = "crate::provider_role::types::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub reserve: Option<ReserveOffer>,
    /// The exchange offer, if any.
    #[serde(
        default,
        deserialize_with = "crate::provider_role::types::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub exchange: Option<ExchangeOffer>,
    /// The ramp offer, if any.
    #[serde(
        default,
        deserialize_with = "crate::provider_role::types::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub ramp: Option<RampOffer>,
    /// The verifier offer, if any.
    #[serde(
        default,
        deserialize_with = "crate::provider_role::types::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub verifier: Option<VerifierOffer>,
}

impl Offers {
    /// The categories present, in the order reserve, exchange, ramp, verifier.
    pub fn categories(&self) -> Vec<Category> {
        let mut out = Vec::new();
        if self.reserve.is_some() {
            out.push(Category::Reserve);
        }
        if self.exchange.is_some() {
            out.push(Category::Exchange);
        }
        if self.ramp.is_some() {
            out.push(Category::Ramp);
        }
        if self.verifier.is_some() {
            out.push(Category::Verifier);
        }
        out
    }
}

/// A reserve's tuples and its declaration (`REQ-CAT-6`, `REQ-CAT-13`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReserveOffer {
    /// The assets it accepts for deposits or redeems, as tuples between an outer and an inner node.
    pub tuples: Vec<ReserveTuple>,
    /// What the reserve declares at publishing (`REQ-CAT-6`).
    pub declaration: ReserveDeclaration,
}

/// An exchange's tuples.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExchangeOffer {
    /// The conversions it serves, each within one node.
    pub tuples: Vec<ExchangeTuple>,
}

/// A ramp's tuples.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RampOffer {
    /// The transfers it serves, each between two nodes, keeping its asset.
    pub tuples: Vec<RampTuple>,
}

/// A verifier advertises classes, not tuples (`REQ-CAT-13`, `DEC-16`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifierOffer {
    /// The classes it can judge a person against.
    pub classes: Vec<String>,
}

/// Where a reserve tuple starts or ends: an inner node, by identifier, or an outer node (`REQ-CAT-17`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NodeRef {
    /// An inner node: one on the operator's public list, or a declared private node.
    Inner(String),
    /// An outer node, outside the book.
    Outer(OuterNode),
}

/// An outer node, named for its network and its reserve (`REQ-CAT-17`). The reserve is the advertising
/// provider, so only the network is written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OuterNode {
    /// The network the funds come from or go to.
    pub network: String,
}

/// Which way a reserve tuple moves value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReserveDirection {
    /// From an outer node into an inner one.
    Withdrawal,
    /// From an inner node out to an outer one.
    Deposit,
}

/// The unit an asset is counted in at each end of a tuple (`REQ-CAT-11`): one whole asset is `10^decimals`
/// units. The two ends may differ, as one asset can have different decimals on different networks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndUnits {
    /// Decimals at the origin; the tuple's bands and increment are in this unit.
    pub origin: u8,
    /// Decimals at the destination.
    pub destination: u8,
}

/// A reserve tuple: one asset, between an outer node and an inner node, in either direction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReserveTuple {
    /// Where the value starts.
    pub origin: NodeRef,
    /// Where the value ends.
    pub destination: NodeRef,
    /// The one asset's identifier.
    pub asset: String,
    /// The unit at each end.
    pub decimals: EndUnits,
    /// The tuple's terms.
    pub terms: Terms,
}

impl ReserveTuple {
    /// The tuple's direction, or `None` if it doesn't connect an outer and an inner node (which the schema
    /// rejects).
    pub fn direction(&self) -> Option<ReserveDirection> {
        match (&self.origin, &self.destination) {
            (NodeRef::Outer(_), NodeRef::Inner(_)) => Some(ReserveDirection::Withdrawal),
            (NodeRef::Inner(_), NodeRef::Outer(_)) => Some(ReserveDirection::Deposit),
            _ => None,
        }
    }
}

/// An exchange tuple: a from asset and a to asset, at one inner node (`REQ-CAT-13`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExchangeTuple {
    /// The one node that is both origin and destination.
    pub node: String,
    /// The asset taken in.
    pub from: Denomination,
    /// The asset given out.
    pub to: Denomination,
    /// The tuple's terms; amounts are in `from`'s unit.
    pub terms: Terms,
}

/// A ramp tuple: one asset, between two different inner nodes (`REQ-CAT-13`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RampTuple {
    /// Where the value starts.
    pub origin: String,
    /// Where the value ends; different from `origin`.
    pub destination: String,
    /// The one asset's identifier.
    pub asset: String,
    /// The unit at each end.
    pub decimals: EndUnits,
    /// The tuple's terms.
    pub terms: Terms,
}

/// An asset and the unit the provider counts it in (`REQ-CAT-11`): one whole asset is `10^decimals` units.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Denomination {
    /// The asset's identifier.
    pub id: String,
    /// How many decimal places the provider's unit sits below one whole asset.
    pub decimals: u8,
}

/// A tuple's terms (`REQ-CAT-11`, `REQ-CAT-12`). Amounts are decimal integer strings in the tuple's input unit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Terms {
    /// The amount bands the provider serves; at least one.
    pub bands: Vec<Band>,
    /// The fill increment, if the provider fills in increments.
    #[serde(
        default,
        deserialize_with = "crate::provider_role::types::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub increment: Option<String>,
    /// Whether the provider's reports are gross or net of its charge.
    pub reporting: Reporting,
    /// The inputs a segment through this tuple needs from the customer; empty if none.
    pub required_inputs: Vec<String>,
}

/// An inclusive amount band.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Band {
    /// The smallest amount served.
    pub min: String,
    /// The largest amount served.
    pub max: String,
}

/// Whether reports are gross or net of the provider's charge (`REQ-CAT-11`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reporting {
    /// Reported amounts include the charge.
    Gross,
    /// Reported amounts exclude the charge.
    Net,
}

/// Provider-wide declarations (`urn:rutent:provider-role:declarations`). A provider that advertises a reserve,
/// exchange or ramp must declare `evidence` and `safe_retry`. A verifier-only provider may leave them out,
/// since it serves no segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declarations {
    /// How an outcome can be followed: at least one of an identifier and a status query (`REQ-CAT-16`).
    pub tracking: Vec<Tracking>,
    /// The evidence the provider gives of every segment outcome (`REQ-CAT-10`, `REQ-ORD-39`).
    #[serde(
        default,
        deserialize_with = "crate::provider_role::types::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub evidence: Option<Evidence>,
    /// Whether a repeat is safe (`REQ-CAT-10`). Where false, no retry is ever sent.
    #[serde(
        default,
        deserialize_with = "crate::provider_role::types::present",
        skip_serializing_if = "Option::is_none"
    )]
    pub safe_retry: Option<bool>,
}

/// A way to follow an outcome (`REQ-CAT-16`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tracking {
    /// Each transaction carries an identifier.
    Identifier,
    /// The provider answers a status query.
    StatusQuery,
}

/// The evidence a provider gives, and how long it stays available (`REQ-CAT-10`, `REQ-ORD-39`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    /// The grade the provider gives on every outcome, so its weakest (ADR-0007). An operator's per-tuple floor
    /// can rely on it.
    pub grade: EvidenceGrade,
    /// How long evidence of an outcome stays available.
    pub availability: Availability,
}

/// The four-grade evidence scale of ADR-0007, serialised as the integers 1 to 4. Lower is stronger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum EvidenceGrade {
    /// 1: checkable by anyone, such as a public chain record.
    PublicRecord = 1,
    /// 2: signed by another party, such as the next provider's receipt or the rail itself.
    SignedByAnotherParty = 2,
    /// 3: signed by the provider, with something external to compare later.
    SignedByProvider = 3,
    /// 4: the provider's bare statement.
    Statement = 4,
}

impl TryFrom<u8> for EvidenceGrade {
    type Error = String;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::PublicRecord),
            2 => Ok(Self::SignedByAnotherParty),
            3 => Ok(Self::SignedByProvider),
            4 => Ok(Self::Statement),
            other => Err(format!("evidence grade {other} is outside 1 to 4")),
        }
    }
}

impl From<EvidenceGrade> for u8 {
    fn from(grade: EvidenceGrade) -> Self {
        grade as u8
    }
}

/// How long evidence stays available: a number of seconds, or `"unbounded"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Availability {
    /// For a bounded time.
    Bounded(BoundedAvailability),
    /// For as long as the record exists, such as a public chain record.
    Unbounded(Unbounded),
}

/// A bounded availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundedAvailability {
    /// Seconds from the outcome; at least one.
    pub seconds: SafeInteger,
}

/// The `"unbounded"` keyword.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unbounded {
    /// `"unbounded"`.
    #[serde(rename = "unbounded")]
    Unbounded,
}

/// What a reserve declares at publishing (`urn:rutent:provider-role:reserve-declaration`, `REQ-CAT-6`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReserveDeclaration {
    /// Whether the reserve can require that the user initiate the withdrawal themselves.
    pub can_require_user_initiated_withdrawal: bool,
    /// Whether the reserve starts routes on automatic deposits (`REQ-ORD-13`).
    pub starts_routes_on_automatic_deposits: bool,
    /// The party the declaration is attributable to.
    pub maker: String,
    /// The provider version in which this declaration was made; a version that changes it declares again.
    pub declared_in_version: String,
}

/// Deserialises an optional field that, when present, holds a value: `null` is refused, as the schemas refuse it.
/// An absent field stays `None` through `#[serde(default)]`.
pub(crate) fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}
