//! Types mirroring the `urn:rutent:provider-role:*` schemas. Field names match the schemas, and every struct
//! denies unknown fields, as the schemas set `additionalProperties: false`.

use serde::{Deserialize, Serialize};

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

/// One of a provider's public signing keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigningKey {
    /// The scheme.
    pub scheme: super::envelope::SignatureScheme,
    /// The Ed25519 public key, base64url without padding.
    pub public_key: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserve: Option<ReserveOffer>,
    /// The exchange offer, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exchange: Option<ExchangeOffer>,
    /// The ramp offer, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ramp: Option<RampOffer>,
    /// The verifier offer, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<Evidence>,
    /// Whether a repeat is safe (`REQ-CAT-10`). Where false, no retry is ever sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
