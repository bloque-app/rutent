//! The shapes that cross between Core's parts, each mirroring its schema under `contracts/core-internal/schemas/`.

use alloc::{borrow::ToOwned, string::String, vec::Vec};
use core::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use super::Violation;
use crate::module_interface::{
    CrossModuleCall, Digest, Name, OutboundCallRecord, SecretLookup, SecretRefusalReason,
};
use crate::provider_role::{
    Denomination, DepositDestination, EvidenceGrade, OuterNode, PublicKey, SafeInteger,
    SignatureScheme,
};
use crate::router_api::{Amount, Id, PositiveAmount, Timestamp};
use crate::state_machine::RecordedInput;

/// Deserialises an optional field that, when present, holds a value: `null` is refused, as the schemas refuse it.
/// An absent field stays `None` through `#[serde(default)]`.
fn some<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

macro_rules! checked_text {
    ($(#[$doc:meta])* $name:ident, $check:expr, $refusal:literal) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// The value, or `None` if the text breaks the schema's rule for it.
            pub fn new(text: impl Into<String>) -> Option<Self> {
                let text = text.into();
                let check: fn(&str) -> bool = $check;
                check(&text).then_some($name(text))
            }

            /// The value as text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = Violation;

            fn try_from(text: String) -> Result<Self, Self::Error> {
                $name::new(text).ok_or(Violation($refusal))
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

checked_text!(
    /// A reason or a bound's name, as the component that gives it names it: a lower-case word or words joined by
    /// `_`, up to 64 characters. Schema: `urn:rutent:core-internal:common#/$defs/token`.
    Token,
    |t| {
        t.len() <= 64
            && t.bytes().next().is_some_and(|c| c.is_ascii_lowercase())
            && t.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
    },
    "a token is 1 to 64 of a-z 0-9 _, starting with a letter"
);

checked_text!(
    /// The entry of a module's state that a read asked for: 1 to 256 characters. Schema:
    /// `urn:rutent:core-internal:handover#/$defs/state_read/properties/key`.
    StateKey,
    |t| !t.is_empty() && t.chars().count() <= 256,
    "a state key is 1 to 256 characters"
);

checked_text!(
    /// The `$id` of a statement's schema, as its envelope's `shape` names it: `urn:rutent:<contract>:<shape>`.
    /// Schema: `urn:rutent:provider-role:statement-envelope#/properties/shape`.
    ShapeId,
    |t| {
        let word = |w: &str| {
            w.split('-').all(|p| {
                !p.is_empty() && p.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            })
        };
        let parts: Vec<&str> = t.split(':').collect();
        t.len() <= 128
            && parts.len() == 4
            && parts[0] == "urn"
            && parts[1] == "rutent"
            && word(parts[2])
            && word(parts[3])
    },
    "a shape is urn:rutent:<contract>:<shape>, each words of a-z 0-9 joined by single -"
);

/// True when `text` is base64url without padding in its one canonical form: the last character carries no unused
/// bits.
fn is_base64url(text: &str) -> bool {
    let alphabet = |c: u8| c.is_ascii_alphanumeric() || c == b'-' || c == b'_';
    let last = text.bytes().last();
    !text.is_empty()
        && text.bytes().all(alphabet)
        && match text.len() % 4 {
            0 => true,
            2 => last.is_some_and(|c| b"AQgw".contains(&c)),
            3 => last.is_some_and(|c| b"AEIMQUYcgkosw048".contains(&c)),
            _ => false,
        }
}

checked_text!(
    /// The 32-byte salt the evidence store keeps beside a statement's bytes, base64url without padding. Schema:
    /// `urn:rutent:core-internal:kept-statement#/properties/salt`.
    Salt,
    |t| t.len() == 43 && is_base64url(t),
    "a salt is 32 bytes, base64url without padding, in its canonical form"
);

checked_text!(
    /// Bytes, base64url without padding, in their canonical form. Schema:
    /// `urn:rutent:core-internal:kept-statement#/properties/bytes`.
    Encoded,
    is_base64url,
    "bytes are base64url without padding, at least one, in their canonical form"
);

/// A count of one or more, no larger than 2^53 - 1. Schema: `urn:rutent:core-internal:common#/$defs/count`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct Count(u64);

impl Count {
    /// The count, or `None` if it is 0 or above 2^53 - 1.
    pub fn new(value: u64) -> Option<Self> {
        (1..=SafeInteger::MAX)
            .contains(&value)
            .then_some(Count(value))
    }

    /// The value.
    pub fn get(self) -> u64 {
        self.0
    }
}

impl TryFrom<u64> for Count {
    type Error = Violation;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Count::new(value).ok_or(Violation("a count is 1 to 2^53 - 1"))
    }
}

impl From<Count> for u64 {
    fn from(value: Count) -> Self {
        value.0
    }
}

// --- Who, and when ---------------------------------------------------------------------------------------------

/// The role an actor acts in. Each role is recorded apart, even where one party holds several (`REQ-AUD-8`). An
/// agent acts in its party's role, with [`Actor::kind`] saying it is an agent (`REQ-OPS-5`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// A customer: a requester, payer or beneficiary.
    Customer,
    /// The deployment's operator.
    Operator,
    /// A provider.
    Provider,
    /// A verifier giving a judgement.
    Verifier,
    /// A regulator, by an identified permission (`REQ-AUZ-7`, `REQ-AUD-12`).
    Regulator,
    /// A checker outside the operator, by an identified permission (`REQ-AUZ-7`, `REQ-AUD-10`).
    Checker,
    /// One of Core's own parts, acting on its own account: a recalculation, a timeout, a hand-over.
    Core,
}

/// What kind of actor acts: humans and agents use one contract, and the record says which acted (`REQ-OPS-5`,
/// `REQ-ORD-10`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    /// A person.
    Person,
    /// An agent acting for its accountable party, within the scope that party set (`REQ-AUZ-8`).
    Agent,
    /// A component: a provider's module, a reserve's system, or one of Core's parts.
    Component,
}

/// Who acts: the accountable party it acts for, the actor itself, the role it acts in, and what kind of actor it is
/// (`REQ-AUZ-8`, `REQ-AUD-8`, `REQ-OPS-5`). Both identifiers are references, never an identity itself (`REQ-AUD-6`).
/// Schema: `urn:rutent:core-internal:common#/$defs/actor`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Actor {
    /// The accountable party: the one limits bind at and actors are compared at (`REQ-AUZ-8`, ADR-0011).
    pub party: Id,
    /// The actor, as external authentication established it: a user, an agent, an operator's login, a provider,
    /// or one of Core's parts (`REQ-AUZ-1`).
    pub acts: Id,
    /// The role it acts in.
    pub role: Role,
    /// What kind of actor it is.
    pub kind: ActorKind,
}

/// Where the time of a verified entry comes from (`REQ-AUD-1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeSource {
    /// Core's own clock, the one it passes as `at` (`F-012`'s notes).
    CoreClock,
    /// The module platform's clock, as a record the platform handed over states it.
    PlatformClock,
}

/// When a verified entry's action happened, and by whose clock (`REQ-AUD-1`). Schema:
/// `urn:rutent:core-internal:common#/$defs/when`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct When {
    /// The time.
    pub at: Timestamp,
    /// Whose clock gave it.
    pub source: TimeSource,
}

/// The source of a stated entry's time: the maker's own statement, which rutent has not verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatedTimeSource {
    /// The time the maker states, its envelope's `made_at`.
    PartyStated,
}

/// When a party says it made a statement (`REQ-AUD-1`, `REQ-AUD-2`). Schema:
/// `urn:rutent:core-internal:common#/$defs/stated_when`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatedWhen {
    /// The time the maker states.
    pub at: Timestamp,
    /// Always the maker's statement.
    pub source: StatedTimeSource,
}

/// The log's answer to an append: where the entry sits, and the digest of the entry as appended. A receipt exists
/// only once the entry is durable, and an effect takes place only after its receipt (`INV-7`).
///
/// `entry` is the SHA-256 digest of the entry's RFC 8785 canonical JSON (provider-role's
/// [`canonical`](crate::provider_role::canonical) encoder): the [`Entry`] as this contract serialises it, with no
/// field added. Schema: `urn:rutent:core-internal:receipt`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    /// The log.
    pub log: Id,
    /// The entry's position in it, from 0.
    pub index: SafeInteger,
    /// The SHA-256 digest of the entry's canonical JSON.
    pub entry: Digest,
}

// --- Actions and decisions -------------------------------------------------------------------------------------

/// Where value goes: a reserve, the outer node it deposits to, and the account or address there, by reference
/// (`REQ-AUZ-3`, `REQ-ORD-35`, `REQ-AUD-6`). Schema: `urn:rutent:core-internal:common#/$defs/destination`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    /// The reserve: the provider of the route's last deposit.
    pub reserve: Id,
    /// The outer node it deposits to.
    pub node: OuterNode,
    /// The account or address at the reserve, by reference: one the requester provides is part of the
    /// destination too (`REQ-ORD-35`).
    pub account: Id,
}

/// What a route that moves value ends at, as the action states it, so policy can hold it against the
/// authorisation (`REQ-AUZ-3`). Schema: `urn:rutent:core-internal:common#/$defs/route_end`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteEnd {
    /// The route's destination.
    pub destination: Destination,
    /// The asset it delivers.
    pub to_asset: Denomination,
    /// The amount it is expected to deliver, in the to asset's unit, to hold against any floor.
    pub delivers: Amount,
}

/// A change to the catalogue, each a permission of its own (`REQ-AUZ-6`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogueChange {
    /// Granting a provider a tuple, or a verifier one of its classes.
    Grant,
    /// Revoking a grant.
    RevokeGrant,
    /// Accepting a module release.
    AcceptModule,
    /// Revoking a module release.
    RevokeRelease,
    /// Marking a release stale (`REQ-MDL-5`, `REQ-MDL-19`).
    MarkStale,
    /// Declaring a release actively exploited.
    DeclareExploited,
    /// Putting a release back in force.
    Reinstate,
    /// Withdrawing a release that waits for its acceptance.
    WithdrawRelease,
    /// Setting a tuple's slippage limit.
    SetSlippage,
    /// Putting a staged release in force, once its staging has run (`REQ-MDL-4`, ADR-0014). Its permit names the
    /// version decided on, but the platform promotes whatever is staged, so Core checks that the staged version is
    /// the permit's before it carries the promotion out (`T-020-08`).
    Promote,
    /// Ending a release's staging with the version in force kept (ADR-0014): the platform's `roll_back`, a decided
    /// change of its own apart from promotion, naming the staged version it ends (item `rollback-permission`,
    /// `T-020-39`). Policy decides it under a permission of its own from `T-021-12`; until then no rule answers it,
    /// and it is refused.
    RollBack,
    /// Setting one of the operator's settings: its fee formula, a tuple's slippage limit or evidence floor, a public
    /// node's settings, a ceiling on value in flight, a schedule's cadence or a protocol version's cutover date
    /// ([`OperatorSetting`]), each a decided change of its own whose action names the setting and its value, so it is
    /// rebuilt from the log (finding 4 of `M3`'s milestone review, `M2`'s item 6, `T-020-45`). Policy decides it under
    /// a permission of its own from `T-021-15`; until then no rule answers it.
    SetSetting,
    /// The operator's word that a provider's releases may come back into force after the platform's unclean end
    /// (`T-013-11`): the platform's `confirm`, a lifecycle action of its own (`T-020-49`). It names the provider's
    /// versions the confirmation puts back as the platform's record holds them, at least one, each once
    /// (`Action::ChangeCatalogue::versions`), and no single release, so a permit to confirm one set of versions
    /// confirms no other: Core checks the platform holds exactly those versions before it carries the confirmation
    /// out. Policy decides it under a permission of its own, held to the deployment operator's logins.
    ConfirmReleases,
    /// Listing a public network with the reader service that reads it (`DEC-28`, ADR-0007 as amended, `T-020-59`): it
    /// names the network and its listing ([`PublicNetwork`], [`NetworkListing`]): the network's identity, the reader's
    /// endpoint, its identity (the operator's CA, the host's subject name and the reader's statement key, never the
    /// host's leaf certificate), its version and its trust rule. A reader is the operator's own, so its `provider` is the
    /// caller's own party: the deployment operator's, or one of its desks' (policy refuses any other). Core keys the
    /// public list by the network, never by this provider (`T-020-60`). Policy decides it under a permission of its own,
    /// held to the deployment operator's logins.
    ListPublicNetwork,
    /// Delisting a public network (`DEC-28`, `T-020-59`): it names the network alone ([`PublicNetwork`] with no
    /// listing). Its `provider` is the caller's own party, as a listing's is. Policy decides it under a permission of its
    /// own, held to the deployment operator's logins.
    DelistPublicNetwork,
}

/// The platform's lifecycle actions on a module release, each a [`CatalogueChange`] of its own, which Core carries
/// out only on a permitted effect, as it does `promote` and `roll_back` (finding 4 and `M2`'s item 6 of `M3`'s milestone
/// review, `T-020-45`): the action's name on the platform and its change. Each is about one release.
pub const LIFECYCLE_ACTIONS: [(&str, CatalogueChange); 5] = [
    ("accept", CatalogueChange::AcceptModule),
    ("withdraw", CatalogueChange::WithdrawRelease),
    ("revoke", CatalogueChange::RevokeRelease),
    ("declare_exploited", CatalogueChange::DeclareExploited),
    ("mark_stale", CatalogueChange::MarkStale),
];

/// The name of Core's scheduled job that asks a segment left unknown its status again (`T-020-48`): the one job a
/// provider's own cadence is for (`OperatorSetting::Cadence::provider`, `T-020-49`).
pub const STATUS_ASKS_JOB: &str = "status_asks";

/// The platform's confirmation of a provider's releases after an unclean end (`T-013-11`, `T-020-49`): a sixth
/// lifecycle action, about a provider's versions as the platform's record holds them rather than one release, so it is
/// apart from [`LIFECYCLE_ACTIONS`]: the action's name on the platform and its change.
pub const CONFIRM_RELEASES: (&str, CatalogueChange) = ("confirm", CatalogueChange::ConfirmReleases);

impl CatalogueChange {
    /// Whether the change is one of the platform's lifecycle actions on a release that Core carries out only on a
    /// permitted effect: the five of [`LIFECYCLE_ACTIONS`], `promote`, `roll_back` and the confirmation of a provider's
    /// releases ([`CONFIRM_RELEASES`]).
    pub fn is_a_lifecycle_action(self) -> bool {
        matches!(
            self,
            CatalogueChange::Promote | CatalogueChange::RollBack | CatalogueChange::ConfirmReleases
        ) || LIFECYCLE_ACTIONS.iter().any(|(_, c)| *c == self)
    }

    /// Whether the change confirms a provider's releases, and so names the versions it confirms.
    pub fn confirms_releases(self) -> bool {
        self == CatalogueChange::ConfirmReleases
    }

    /// Whether the change sets one of the operator's settings, and so names it.
    pub fn is_a_setting(self) -> bool {
        self == CatalogueChange::SetSetting
    }

    /// Whether the change lists or delists a public network, and so names it (`T-020-59`).
    pub fn is_about_a_public_network(self) -> bool {
        matches!(
            self,
            CatalogueChange::ListPublicNetwork | CatalogueChange::DelistPublicNetwork
        )
    }

    /// Whether the change is about one module release, and so names its version.
    pub fn is_about_a_release(self) -> bool {
        matches!(
            self,
            CatalogueChange::AcceptModule
                | CatalogueChange::RevokeRelease
                | CatalogueChange::MarkStale
                | CatalogueChange::DeclareExploited
                | CatalogueChange::Reinstate
                | CatalogueChange::WithdrawRelease
                | CatalogueChange::Promote
                | CatalogueChange::RollBack
        )
    }

    /// Whether the change is about one tuple, and so names it.
    pub fn is_about_a_tuple(self) -> bool {
        matches!(
            self,
            CatalogueChange::Grant | CatalogueChange::RevokeGrant | CatalogueChange::SetSlippage
        )
    }
}

checked_text!(
    /// The subject name a reader host's server certificate carries (its SAN, a `dNSName`), which Core holds the host's
    /// certificate to under the operator's CA (`DEC-28`): a lower-case DNS name of LDH labels, each 1 to 63 characters,
    /// neither starting nor ending with a hyphen, at least two labels, up to 253 characters, with no wildcard, no
    /// trailing dot and a last label that starts with a letter, so it names one host by name and is never matched as an
    /// IP address, decimal or hexadecimal (`crate::reader::is_host_name`). Schema: `urn:rutent:core-internal:common#/$defs/subject_name`.
    SubjectName,
    crate::reader::is_host_name,
    "a subject name is a lower-case DNS host name of at least two labels, with no wildcard and no IP literal"
);

/// A public network a listing or a delisting is about (`DEC-28`, `T-020-59`): the network, as an outer node names it,
/// and, for a listing, what lists it. Schema: `urn:rutent:core-internal:common#/$defs/public_network`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicNetwork {
    /// The network.
    pub network: Id,
    /// What lists it: present exactly for `list_public_network`.
    #[serde(
        default,
        deserialize_with = "some",
        skip_serializing_if = "Option::is_none"
    )]
    pub listing: Option<NetworkListing>,
}

/// What lists a public network (`DEC-28`, ADR-0007 as amended): the network's identity, how to find its reader, who
/// the reader is, its version and its trust rule. Core checks each at start-up against the reader's own
/// `network_rules` answer, and a network whose reader is unreachable or fails the check doesn't start (`T-020-60`).
/// Schema: `urn:rutent:core-internal:common#/$defs/network_listing`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkListing {
    /// The network's identity: a genesis hash, or a chain id.
    pub identity: crate::reader::NetworkIdentity,
    /// Where its reader is.
    pub endpoint: ReaderEndpoint,
    /// Who its reader is.
    pub reader: ReaderIdentity,
    /// The reader's version, which Core holds each answer's to (`REQ-AUD-4`).
    pub version: crate::router_api::Version,
    /// The reader's trust rule: a light client, or a pool of RPC endpoints with a quorum of at least two.
    pub trust: crate::reader::TrustRule,
}

/// How Core finds a reader (item `reader-registration-path`): the host setting that says where its reader host is, and
/// the reader's name on that host, the maker of its answers. Schema:
/// `urn:rutent:core-internal:common#/$defs/reader_endpoint`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderEndpoint {
    /// The deployment's host setting, by name, that holds the reader host's address. The address itself stays the
    /// deployment's, so moving a host needs no listing change.
    pub host_setting: Id,
    /// The reader's name on its host.
    pub reader: Id,
}

/// Who a reader is, as Core holds it (`DEC-28`): the operator's CA that issues the host's server certificate, the
/// subject name that certificate carries, and the reader's statement key, which its answers are signed with. Never
/// the host's leaf certificate: rotating it under the same CA and name needs no listing change, and transport identity
/// alone is not authorisation, so Core checks both the certificate and the answer's signature. Schema:
/// `urn:rutent:core-internal:common#/$defs/reader_identity`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderIdentity {
    /// The SHA-256 digest of the operator's CA certificate (DER), the trust anchor Core checks the host's chain to:
    /// a CA's, never a leaf's. The contract can't tell a leaf's digest put here from a CA's; Core's link compares it
    /// with the CA the deployment hands it, so a leaf's digest fails closed and its network doesn't start (`T-020-60`).
    pub ca: Digest,
    /// The subject name the host's server certificate must carry.
    pub subject_name: SubjectName,
    /// The reader's statement key: the key each of its answers names and is signed with.
    pub statement_key: PublicKey,
}

/// One of the operator's settings and its value (finding 4 of `M3`'s milestone review, `T-020-45`): what a
/// `set_setting` change sets. The values stay the deployment's to choose (`T-020-49`); this names them so policy
/// decides each and the log holds it. Schema: `urn:rutent:core-internal:common#/$defs/operator_setting`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "setting", rename_all = "snake_case", deny_unknown_fields)]
pub enum OperatorSetting {
    /// The operator's own fee, taken at the destination (`REQ-QTE-2`, `REQ-QTE-5`), as the router's bounds carry it.
    OperatorFee {
        /// The formula.
        formula: crate::router_api::FeeFormula,
    },
    /// A tuple's slippage limit (`REQ-QTE-9`), as the router's bounds carry it.
    SlippageLimit {
        /// The limit, by its tuple.
        limit: crate::router_api::SlippageLimit,
    },
    /// The weakest evidence the operator accepts on a tuple (`REQ-ORD-39`).
    EvidenceFloor {
        /// The tuple.
        tuple: crate::router_api::TupleKey,
        /// The floor.
        grade: EvidenceGrade,
    },
    /// A public node's settings (`REQ-CAT-23`, `DEC-13`): whether the operator lists it public, and the confirmations
    /// a movement at it waits for before Core takes it as final, where the operator sets them. Each network's values
    /// are `F-031`'s to choose.
    PublicNode {
        /// The node.
        node: OuterNode,
        /// Whether the operator lists it public.
        listed: bool,
        /// The confirmations a movement waits for, where set.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        confirmations: Option<Count>,
    },
    /// A ceiling on value in flight at one provider (`REQ-ORD-8`), as the router's bounds carry it.
    ProviderCeiling {
        /// The ceiling.
        ceiling: crate::router_api::ProviderCeiling,
    },
    /// A ceiling on value in flight across one publisher's providers (`REQ-ORD-8`), as the router's bounds carry it.
    PublisherCeiling {
        /// The ceiling.
        ceiling: crate::router_api::PublisherCeiling,
    },
    /// How often one of Core's scheduled jobs runs (`T-020-48`), or, naming a provider, how often a segment left unknown
    /// at that provider is asked its status again (the status question's job, `status_asks`, alone: `T-020-49`), until a
    /// provider's declarations carry a cadence of their own.
    Cadence {
        /// The job, by name.
        job: Token,
        /// Its period, in whole seconds: zero runs it on every tick of Core's schedule (`T-020-49`).
        seconds: SafeInteger,
        /// The provider the cadence is for, where it is one provider's: named only for `status_asks`.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        provider: Option<Id>,
    },
    /// An older protocol version's cutover date, from which a statement under it is void (`REQ-PRT-4`, `T-020-54`).
    Cutover {
        /// The older version.
        version: crate::router_api::ProtocolVersion,
        /// The cutover date.
        cutover: Timestamp,
    },
    /// How long an order filed on an automatic deposit waits for its customer to accept a quote shown without a firm
    /// rate, counted from the first quote shown (`REQ-QTE-10`, `T-020-51`): past it the wait ends and its deposit is
    /// free again.
    DepositWait {
        /// The lifetime of a wait, in whole seconds.
        seconds: Count,
    },
}

/// Which of the operator's settings an [`OperatorSetting`] is: policy holds each under a permission of its own
/// (`T-021-15`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingKind {
    /// The operator's own fee.
    OperatorFee,
    /// A tuple's slippage limit.
    SlippageLimit,
    /// A tuple's evidence floor.
    EvidenceFloor,
    /// A public node's settings.
    PublicNode,
    /// A provider's ceiling.
    ProviderCeiling,
    /// A publisher's ceiling.
    PublisherCeiling,
    /// A scheduled job's cadence.
    Cadence,
    /// A protocol version's cutover date.
    Cutover,
    /// The lifetime of an order's wait for its customer on an automatic deposit.
    DepositWait,
}

impl OperatorSetting {
    /// Which setting it is.
    pub fn kind(&self) -> SettingKind {
        match self {
            OperatorSetting::OperatorFee { .. } => SettingKind::OperatorFee,
            OperatorSetting::SlippageLimit { .. } => SettingKind::SlippageLimit,
            OperatorSetting::EvidenceFloor { .. } => SettingKind::EvidenceFloor,
            OperatorSetting::PublicNode { .. } => SettingKind::PublicNode,
            OperatorSetting::ProviderCeiling { .. } => SettingKind::ProviderCeiling,
            OperatorSetting::PublisherCeiling { .. } => SettingKind::PublisherCeiling,
            OperatorSetting::Cadence { .. } => SettingKind::Cadence,
            OperatorSetting::Cutover { .. } => SettingKind::Cutover,
            OperatorSetting::DepositWait { .. } => SettingKind::DepositWait,
        }
    }

    /// The provider it is about, where it is about one: a tuple's, or a provider's ceiling's. `None` for a setting of
    /// the whole deployment.
    pub fn provider(&self) -> Option<&Id> {
        match self {
            OperatorSetting::SlippageLimit { limit } => Some(&limit.tuple.provider),
            OperatorSetting::EvidenceFloor { tuple, .. } => Some(&tuple.provider),
            OperatorSetting::ProviderCeiling { ceiling } => Some(&ceiling.provider),
            OperatorSetting::Cadence {
                provider: Some(provider),
                ..
            } => Some(provider),
            _ => None,
        }
    }
}

/// An action that policy decides: one that moves value, posts a route or changes the catalogue (`REQ-AUZ-1`).
/// Schema: `urn:rutent:core-internal:common#/$defs/action`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    remote = "Self",
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Action {
    /// Handing a segment's value-moving operation to its provider.
    MoveValue {
        /// The order.
        order: Id,
        /// The segment.
        segment: Id,
        /// The segment's provider.
        provider: Id,
        /// The order's origin reserve: the provider of its first withdrawal, or the holding account's provider
        /// on a route handed on, so policy knows whether the operator may act for the user there (`REQ-AUZ-9`).
        origin_reserve: Id,
        /// The authorisation it acts under (`REQ-AUZ-3`).
        authorisation: Id,
        /// What the route it belongs to ends at, as it stands now.
        route: RouteEnd,
    },
    /// Posting a route, which only the operator may do (`REQ-AUZ-2`).
    PostRoute {
        /// The order.
        order: Id,
        /// The digest of the recorded decision that chose the route.
        decision_record: Digest,
        /// Who suggested it, if anyone did: a suggestion has no standing until posted (`REQ-AUZ-2`).
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        suggested_by: Option<Actor>,
    },
    /// Changing the catalogue (`REQ-AUZ-6`).
    ChangeCatalogue {
        /// The change.
        change: CatalogueChange,
        /// The provider it is about.
        provider: Id,
        /// The module release it is about: present exactly when the change is about a release.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        version: Option<Id>,
        /// The tuple it is about: present exactly when the change is about a tuple, save a verifier's grant and
        /// its revocation, which name its class in its place.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        tuple: Option<Id>,
        /// The class a verifier's grant is for, in place of a tuple: a verifier advertises classes, not tuples
        /// (`REQ-CAT-13`, `DEC-16`). Named only by a `grant` or a `revoke_grant`, and never with a tuple. The keys a
        /// grant of a class alone trusts attribute only the maker's judgements of that class, in the verifier's role
        /// ([`attribute`](super::attribute)), never a statement about a tuple's segments.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        class: Option<Id>,
        /// The provider's advertising keys, at least one, each once: named exactly by a grant. The latest grant in
        /// the log replaces them, which is how a provider recovers a lost key, and they are the log's first trust
        /// in the provider's advertisements, never a deployment setting the log doesn't hold.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        advertising_keys: Vec<PublicKey>,
        /// The key the release's module signs its statements with: named exactly by `accept_module`, so the log
        /// verifies a module the operator publishes by the release it accepted.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        statement_key: Option<PublicKey>,
        /// The operator's setting it sets, and its value: named exactly by `set_setting` (`T-020-45`). For a setting
        /// about a provider (a tuple's, or a provider's ceiling), `provider` is that provider; for one about the whole
        /// deployment (the operator's fee, a public node, a publisher's ceiling, a cadence, a cutover), the
        /// deployment's operator.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        setting: Option<alloc::boxed::Box<OperatorSetting>>,
        /// The provider's versions a confirmation of its releases puts back in force, as the platform's record holds
        /// them, at least one, each once: named exactly by `confirm_releases` (`T-020-49`).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        versions: Vec<Id>,
        /// The public network a listing or a delisting is about: named exactly by `list_public_network`, with its
        /// listing, and by `delist_public_network`, without one (`DEC-28`, `T-020-59`).
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        public_network: Option<alloc::boxed::Box<PublicNetwork>>,
    },
    /// Asking the provider of a failed segment to reverse it, a decided action like any other (`REQ-ORD-3`,
    /// `REQ-ORD-11`): the segment owes its reversal, and the provider is the one that failed it.
    AskReversal {
        /// The order.
        order: Id,
        /// The failed segment.
        segment: Id,
        /// The provider that failed it, asked to reverse.
        provider: Id,
    },
    /// Filing an arrival anew against the order it differed from (`REQ-ORD-23`), a decided action like any other
    /// (`T-020-52`): the deployment's operator, or the reserve the arrival came to, files it, and policy decides that
    /// before the `refiling` entry is recorded, which cites the permit. It takes no effect: the refiling moves
    /// nothing. The filer is the caller that asks, named so a permit can't be read as another's.
    RefileArrival {
        /// The receipt of the arrival's entry.
        arrival: Receipt,
        /// The order it is filed against.
        order: Id,
        /// Who files it anew: the request's caller.
        filer: Actor,
    },
    /// Asking a segment's provider where the segment stands, late, once it failed or fell silent (`REQ-ORD-1`, `AC-5`'s
    /// late delivery): a decided action, taken as an effect under a fresh transaction of its order, which it names,
    /// and appended before the question is sent, so a late status is traceable from the log (`T-020-36`). What the
    /// provider answers is kept as its statement and moves nothing (`INV-4`). The provider is the one the segment's
    /// move went to (`status-is-asked-of-the-segments-provider`), and the transaction one no entry before it names
    /// (`status-is-asked-under-a-fresh-transaction`).
    AskStatus {
        /// The order.
        order: Id,
        /// The segment asked about.
        segment: Id,
        /// The segment's provider, asked.
        provider: Id,
        /// The fresh transaction it is asked under: never the segment's own, nor one any other entry named before.
        transaction: Id,
    },
    /// Reading a private node's record from its publisher's provider (`REQ-ORD-41`, `T-020-58`), for the segments of an
    /// order that pass the node: Core's closing takes it while it grades a segment, and Core's tick for an order still
    /// unsettled at the node. A decided action, taken as an effect under a fresh transaction it names, appended before the
    /// question is put, so the read is traceable from the log. What the provider answers is taken as its statement only as
    /// the question's answer, and moves nothing.
    ReadNodeRecord {
        /// The order.
        order: Id,
        /// A segment of the order that passes the node.
        segment: Id,
        /// The private node whose record is read.
        node: Id,
        /// The provider of the node's publisher that is asked.
        provider: Id,
        /// The fresh transaction the question is asked under.
        transaction: Id,
        /// The transactions the read is about: the segments whose record is asked for, at least one.
        about: Vec<Id>,
    },
    /// The customer accepting a provider's showing that a failed segment's failure was its fault (`REQ-ORD-11`): a
    /// decision of policy's on the policy handle, as an order's authorisation is, asked by the order's requester in its
    /// open session and naming the showing it accepts, so it can't be read as accepting another. It takes no effect:
    /// the `reversal_carrier` that rests on it records who carries the reversal. The operator accepts a showing by its
    /// `accept_fault` command instead.
    AcceptFault {
        /// The order.
        order: Id,
        /// The failed segment.
        segment: Id,
        /// The showing accepted.
        showing: ShowingNamed,
        /// The customer's session it is asked in (`REQ-ORD-32`).
        session: Id,
    },
    /// Erasing a kept statement's bytes from the evidence store, a decided action like any other (`REQ-AUD-13`): the
    /// operator asks it of policy, the log admits its effect on the permit, and the store erases only on the effect's
    /// `done` result, which the `erasure` entry then records. The commitment stays in the log.
    EraseStatement {
        /// The commitment the bytes are kept under.
        commitment: Digest,
        /// Why, as a token: a label to read, which no decision branches on.
        reason: Token,
        /// Whether the erasure goes ahead while later entries cite the statement: false unless set, and then the
        /// log refuses it while anything rests on the statement (`erasure-waits-while-anything-rests-on-the-statement`,
        /// the owner's default). Set, it is the operator's override, a decision of its own that policy holds under a
        /// permission apart from a plain erasure (`T-021-13`), and the log admits the erasure, its effect and its
        /// record only where the permitting decision's action sets it (item `erasure-of-cited-evidence`,
        /// `T-020-39`). The citing entries stay, naming a commitment whose bytes are gone.
        #[serde(default, skip_serializing_if = "core::ops::Not::not")]
        overrides_citations: bool,
    },
    /// Moving a stuck order's value as its decide step named, outside the order's authorisation, which a
    /// resolution back to the origin needn't keep to (`REQ-AUZ-4`, ADR-0011). It travels only as an effect on a
    /// `stuck_move` permit: a [`DecisionRequest`] never carries it.
    ResolveStuck {
        /// The stuck order.
        order: Id,
        /// The decide step's decision it carries out.
        carries_out: Decided,
        /// The outcome it moves, exactly as the decide step named it, the provider that moves it included: the log
        /// admits it only so (`admit_entry`).
        resolution: Resolution,
        /// The transaction the move goes under at that provider, exactly as the decide step named it: what its
        /// outcome's evidence is about, as a `move_value`'s is about its segment
        /// (`outcome-confirmation-cites-verified-evidence`).
        transaction: Id,
    },
}

/// Why policy refused, as a stable class (`REQ-AUZ-1`, `REQ-AUZ-5`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusalClass {
    /// The actor's role or permissions don't allow it (`REQ-AUZ-2`, `REQ-AUZ-6`, `REQ-AUZ-7`).
    NotPermitted,
    /// An input the decision needs is missing (`REQ-AUZ-5`).
    MissingInput,
    /// It isn't inside the authorisation it names (`REQ-AUZ-3`).
    OutsideAuthorisation,
    /// The accountable party is past its bound (`REQ-OPS-1`, `REQ-AUZ-8`, `REQ-CPL-5`).
    OverBound,
    /// The actor acts for no identified accountable party (`REQ-AUZ-8`).
    NoAccountableParty,
    /// The same accountable party would both decide a stuck order's outcome and move its value (`REQ-AUZ-4`).
    SameActor,
}

/// Why policy could not decide (`REQ-AUZ-5`): nothing moves on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotKnownCause {
    /// The policy service couldn't be reached.
    PolicyUnreachable,
    /// A verifier couldn't be reached.
    VerifierUnreachable,
}

/// Policy's answer: permit, refuse with its class, or not known (`REQ-AUZ-1`, `REQ-AUZ-5`). Schema:
/// `urn:rutent:core-internal:decision#/$defs/answer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case", deny_unknown_fields)]
pub enum Answer {
    /// The action may take effect.
    Permit {},
    /// It may not.
    Refuse {
        /// Why.
        class: RefusalClass,
    },
    /// Policy couldn't decide, so it may not either.
    NotKnown {
        /// Why.
        cause: NotKnownCause,
    },
}

impl Answer {
    /// The outcome of the entry that records this answer.
    pub fn outcome(self) -> Outcome {
        match self {
            Answer::Permit {} => Outcome::Permitted {},
            Answer::Refuse { class } => Outcome::Refused { class },
            Answer::NotKnown { cause } => Outcome::NotKnown { cause },
        }
    }
}

/// The one call before any action that moves value, posts a route or changes the catalogue (`REQ-AUZ-1`). Schema:
/// `urn:rutent:core-internal:decision-request`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct DecisionRequest {
    /// The request's id.
    pub request: Id,
    /// Who asks for the action.
    pub caller: Actor,
    /// The action.
    pub action: Action,
    /// When, by Core's clock.
    pub at: Timestamp,
}

/// Policy's answer to a decision call, appended to the log before it is given. Schema:
/// `urn:rutent:core-internal:decision`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    /// The request it answers.
    pub request: Id,
    /// The decision's id, which its entry names.
    pub decision: Id,
    /// The answer.
    pub outcome: Answer,
    /// The receipt of the decision's entry in the log.
    pub receipt: Receipt,
}

impl Decision {
    /// The permit an effect cites, if this decision permitted. Nothing confines minting to this method across
    /// crates (a permit deserialises, and a decision's fields are public), so the guard is the log: its append
    /// refuses an effect whose permit isn't an unused one for a permitting entry of the log (`admit_entry`).
    pub fn permit(&self) -> Option<Permit> {
        matches!(self.outcome, Answer::Permit {}).then(|| Permit {
            request: self.request.clone(),
            decision: self.decision.clone(),
            answer: PermitAnswer::Permit,
            receipt: self.receipt.clone(),
        })
    }
}

/// The one answer an effect can cite: a permit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermitAnswer {
    /// A permit.
    Permit,
}

/// A decision an effect cites: its request, its id, that it permitted, and the receipt of the decision's entry, by
/// which the log finds the action decided (`REQ-AUZ-1`). [`Decision::permit`] makes one from a permitting answer;
/// whatever made it, the log admits an effect citing it only as [`admit_entry`](super::admit_entry) says. Its
/// fields are read through its methods. Schema: `urn:rutent:core-internal:effect#/$defs/permit`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Permit {
    request: Id,
    decision: Id,
    answer: PermitAnswer,
    receipt: Receipt,
}

impl Permit {
    /// The request the decision answered.
    pub fn request(&self) -> &Id {
        &self.request
    }

    /// The decision.
    pub fn decision(&self) -> &Id {
        &self.decision
    }

    /// The receipt of the decision's entry in the log.
    pub fn receipt(&self) -> &Receipt {
        &self.receipt
    }
}

/// An action about to take effect: what it is, the permit it rests on, and the receipt of its own entry in the
/// log, appended before it takes effect (`REQ-AUZ-1`, `INV-7`). Orchestration holds one before it acts. Schema:
/// `urn:rutent:core-internal:effect`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Effect {
    /// The action: the one its permit's decision entry decided.
    pub action: Action,
    /// The decision that permitted it.
    pub decision: Permit,
    /// The receipt of the entry that records it.
    pub receipt: Receipt,
}

/// The authorisation to move funds: one order, its destination, its to asset and any floor the customer set. A
/// recalculated route acts only within it, and anything else needs a new one (`REQ-AUZ-3`). Schema:
/// `urn:rutent:core-internal:authorisation`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct Authorisation {
    /// The authorisation's id.
    pub authorisation: Id,
    /// The one order it is for.
    pub order: Id,
    /// The order's origin reserve: where the user authorised, or the holding account's provider on a route handed
    /// on (`REQ-AUZ-9`). A split order's parts can start at different reserves, each named by its part's yes on the
    /// acceptance; this names the order's first part's reserve, the one policy holds against the order's origin
    /// when it asks whether the operator may act for the user there.
    pub origin_reserve: Id,
    /// The account at the origin reserve the value comes from, by reference: where a stuck resolution returns it
    /// (`delivery-goes-to-the-parts-ends`). Intake fills it (`T-020-03`); absent, no delivery back to the origin
    /// is admitted.
    #[serde(
        default,
        deserialize_with = "some",
        skip_serializing_if = "Option::is_none"
    )]
    pub origin_account: Option<Id>,
    /// The destination.
    pub destination: Destination,
    /// The asset delivered there.
    pub to_asset: Denomination,
    /// The least the customer will take at the destination, in the to asset's unit, if it set one.
    #[serde(
        default,
        deserialize_with = "some",
        skip_serializing_if = "Option::is_none"
    )]
    pub floor: Option<PositiveAmount>,
    /// The parts of the order it covers, at least one, each once; absent, it covers every part. Destinations change
    /// per part, so a replacement for one part's new destination names that part, and replaces the earlier
    /// authorisation for it alone: the order's other parts go on under theirs (`T-021-04`).
    #[serde(
        default,
        deserialize_with = "some",
        skip_serializing_if = "Option::is_none"
    )]
    pub parts: Option<Vec<Id>>,
    /// Who gave it: a user at the origin, its agent, or the operator acting for it where the origin reserve
    /// declared it can't require the user's own withdrawal (`REQ-AUZ-9`). The record says who acted.
    pub given_by: Actor,
    /// When, by Core's clock.
    pub at: Timestamp,
}

// --- Stuck orders (ADR-0011) -----------------------------------------------------------------------------------

/// The outcome a stuck order's decision names (ADR-0011): which provider moves how much of which asset, and either
/// where to, or on to which linked order. Schema: `urn:rutent:core-internal:stuck-decide#/$defs/resolution`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Resolution {
    /// The value moves to a destination: the order's own, or back to its origin.
    Deliver {
        /// The provider that moves the value: the decide step names it, so the move step can't choose another.
        by: Id,
        /// Where the value goes.
        to: Destination,
        /// The asset.
        asset: Denomination,
        /// The amount, in the asset's unit.
        amount: PositiveAmount,
    },
    /// The value is handed on to a linked order, which starts from the holding account where it sits as its origin
    /// reserve; the operator does this for a stuck order under `REQ-AUZ-4` (`REQ-ORD-13`, `REQ-ORD-43`).
    HandOn {
        /// The provider whose holding account holds the value: the decide step names it.
        by: Id,
        /// The linked order the value goes on to; its own records point back to this one.
        linked_order: Id,
        /// The asset.
        asset: Denomination,
        /// The amount, in the asset's unit.
        amount: PositiveAmount,
    },
}

/// The decide step of resolving a stuck order: one actor names the outcome and the evidence relied on
/// (`REQ-AUZ-4`, ADR-0011). Schema: `urn:rutent:core-internal:stuck-decide`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct StuckDecide {
    /// The request's id.
    pub request: Id,
    /// The stuck order.
    pub order: Id,
    /// The order's stuck part: a resolution resolves one part, and the log admits it only for a part that stands
    /// stuck (`only-a-declared-stuck-part-is-resolved`).
    pub part: Id,
    /// Who decides.
    pub decider: Actor,
    /// The outcome decided.
    pub resolution: Resolution,
    /// The transaction the resolution goes under at its provider: part of what is decided, so the move step can't
    /// choose another, and its outcome's evidence binds through it.
    pub transaction: Id,
    /// The evidence relied on, by digest, at least one, each once.
    pub evidence: Vec<Digest>,
    /// When, by Core's clock.
    pub at: Timestamp,
}

/// A decision a later call carries out. Schema: `urn:rutent:core-internal:common#/$defs/decided`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decided {
    /// The request the decision answered.
    pub request: Id,
    /// The decision.
    pub decision: Id,
}

/// The move step of resolving a stuck order: another actor, compared at the accountable party, moves the value the
/// decision named (`REQ-AUZ-4`, ADR-0011). Its permit is what a [`Action::ResolveStuck`] effect cites. Schema:
/// `urn:rutent:core-internal:stuck-move`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StuckMove {
    /// The request's id.
    pub request: Id,
    /// The stuck order.
    pub order: Id,
    /// The decide step's decision it carries out.
    pub carries_out: Decided,
    /// Who moves the value.
    pub mover: Actor,
    /// When, by Core's clock.
    pub at: Timestamp,
}

// --- Sessions and reads ----------------------------------------------------------------------------------------

/// A request for a session that speaks for one account, which the catalogue's balance checks read (`REQ-CAT-3`,
/// `F-012`'s notes). Schema: `urn:rutent:core-internal:session-request`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct SessionRequest {
    /// The request's id.
    pub request: Id,
    /// The account, by reference.
    pub account: Id,
    /// The party that holds the account.
    pub holder: Id,
    /// Who asks: the holder or its delegate.
    pub requester: Actor,
    /// The latest the session may end, by Core's clock: after `at`.
    pub until: Timestamp,
    /// When, by Core's clock.
    pub at: Timestamp,
}

/// A session policy opened. Schema: `urn:rutent:core-internal:common#/$defs/opened`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Opened {
    /// The session's id.
    pub session: Id,
    /// When it ends, by Core's clock: after the request's `at`, and no later than its `until`.
    pub expires_at: Timestamp,
}

/// Policy's answer to a session request. Schema: `urn:rutent:core-internal:session-answer#/$defs/outcome`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionOutcome {
    /// A session is open.
    Opened {
        /// The session's id.
        session: Id,
        /// When it ends, by Core's clock.
        expires_at: Timestamp,
    },
    /// No session.
    Refused {
        /// Why.
        class: RefusalClass,
    },
    /// Policy couldn't decide, so no session.
    NotKnown {
        /// Why.
        cause: NotKnownCause,
    },
}

impl SessionOutcome {
    /// The outcome of the entry that records this answer: an open session is permitted.
    pub fn outcome(&self) -> Outcome {
        match self {
            SessionOutcome::Opened { .. } => Outcome::Permitted {},
            SessionOutcome::Refused { class } => Outcome::Refused { class: *class },
            SessionOutcome::NotKnown { cause } => Outcome::NotKnown { cause: *cause },
        }
    }
}

/// The answer to a session request, appended to the log before it is given. Schema:
/// `urn:rutent:core-internal:session-answer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionAnswer {
    /// The request it answers.
    pub request: Id,
    /// The answer.
    pub outcome: SessionOutcome,
    /// The receipt of its entry in the log.
    pub receipt: Receipt,
}

/// What a read is of. Schema: `urn:rutent:core-internal:read-check#/$defs/reads`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Reads {
    /// One order: its customer's to read (`REQ-AUZ-7`).
    Order {
        /// The order.
        order: Id,
    },
    /// One order's recorded state and age, and nothing else: visible to its customer and the operator
    /// (`REQ-ORD-5`). The operator reads it without a permission; a full read of the order needs one.
    OrderState {
        /// The order.
        order: Id,
    },
    /// One segment of an order, with its provider: that provider's to read (`REQ-AUZ-7`).
    Segment {
        /// The order.
        order: Id,
        /// The segment.
        segment: Id,
        /// The segment's provider.
        provider: Id,
    },
    /// The whole audit trail: a regulator's, by its permission (`REQ-AUD-12`).
    Trail {},
    /// A named resource of the deployment, such as who its checker is (`REQ-AUD-10`).
    Resource {
        /// The resource, by name.
        resource: Token,
    },
    /// A statement kept in the evidence store, by its commitment: a party's own, or a checker's by its permission
    /// (`REQ-AUD-5`, `REQ-AUD-10`).
    Evidence {
        /// The commitment the stated entry names it by.
        commitment: Digest,
    },
    /// The status of an order whose parties the log doesn't hold yet, its acceptance not logged: read by its filed
    /// request's requester, which this names, so policy answers an agent's read from its scope and its party's being
    /// that requester (`REQ-AUZ-8`, `REQ-AUZ-7`; finding 6 of `M3`'s milestone review, `T-020-45`, `T-021-16`).
    FiledOrder {
        /// The order.
        order: Id,
        /// The party that filed the request: the order's requester.
        requester: Id,
    },
}

/// A read check: who reads and what. A customer reads only its own orders and a provider only its own segments; a
/// regulator's or a checker's read rests on an identified permission. Any actor may ask: policy answers, and each
/// read is recorded, allowed or refused (`REQ-AUZ-7`, `REQ-AUD-12`). Schema: `urn:rutent:core-internal:read-check`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadCheck {
    /// The request's id.
    pub request: Id,
    /// Who reads, in its role: an agent in its party's role, `kind` naming it an agent (`REQ-OPS-5`).
    pub reader: Actor,
    /// What it reads.
    pub reads: Reads,
    /// The identified permission a regulator's or checker's read rests on.
    #[serde(
        default,
        deserialize_with = "some",
        skip_serializing_if = "Option::is_none"
    )]
    pub permission: Option<Id>,
    /// When, by Core's clock.
    pub at: Timestamp,
}

/// A read permission granted or revoked: who may read what, by an identified permission (`REQ-AUZ-7`,
/// `REQ-AUD-12`). Policy answers it with a decision, and a read citing the permission rests on its grant. Schema:
/// `urn:rutent:core-internal:permission-change`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct PermissionChange {
    /// The request's id.
    pub request: Id,
    /// The permission's id, which a read check names.
    pub permission: Id,
    /// Who holds it: a regulator or a checker, which `check` holds it to.
    pub grantee: Actor,
    /// What it lets the grantee read.
    pub scope: Reads,
    /// Who grants or revokes it: the operator, which `check` holds it to.
    pub by: Actor,
    /// When, by Core's clock.
    pub at: Timestamp,
}

/// Policy's answer to a read check. Schema: `urn:rutent:core-internal:read-answer#/$defs/outcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReadOutcome {
    /// The read may go ahead.
    Allowed {},
    /// It may not.
    Refused {
        /// Why.
        class: RefusalClass,
    },
}

impl ReadOutcome {
    /// The outcome of the entry that records this answer: an allowed read is permitted.
    pub fn outcome(self) -> Outcome {
        match self {
            ReadOutcome::Allowed {} => Outcome::Permitted {},
            ReadOutcome::Refused { class } => Outcome::Refused { class },
        }
    }
}

/// The answer to a read check, recorded either way. Schema: `urn:rutent:core-internal:read-answer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadAnswer {
    /// The request it answers.
    pub request: Id,
    /// The answer.
    pub outcome: ReadOutcome,
    /// The receipt of its entry in the log.
    pub receipt: Receipt,
}

// --- Work and questions about people ---------------------------------------------------------------------------

/// The kind of work a charge counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Work {
    /// The caller's own requests to rutent.
    Request,
    /// Requests an order fans out to providers on the caller's behalf (`REQ-OPS-1`).
    FanOut,
    /// Questions asked about people (`REQ-AUZ-8`, `REQ-CPL-2`).
    PersonQuestion,
}

/// A charge of work to the accountable party the caller acts for, where limits bind (`REQ-OPS-1`, `REQ-AUZ-8`).
/// Schema: `urn:rutent:core-internal:charge`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct Charge {
    /// The request's id.
    pub request: Id,
    /// The accountable party charged: the caller's own.
    pub party: Id,
    /// The caller whose work it is.
    pub caller: Actor,
    /// What kind of work.
    pub work: Work,
    /// How much of it.
    pub count: Count,
    /// The order it is for, where it is for one.
    #[serde(
        default,
        deserialize_with = "some",
        skip_serializing_if = "Option::is_none"
    )]
    pub order: Option<Id>,
    /// When, by Core's clock.
    pub at: Timestamp,
}

/// Policy's answer to a charge. Schema: `urn:rutent:core-internal:charge-answer#/$defs/outcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChargeOutcome {
    /// Within the party's bound, with what is left of it.
    Within {
        /// What is left.
        remaining: SafeInteger,
    },
    /// Over the party's bound: the work is not done.
    Over {
        /// The bound.
        bound: SafeInteger,
    },
    /// Policy refused the work, for the class given: a scope it isn't in, say. Not a bound passed.
    Refused {
        /// Why.
        class: RefusalClass,
    },
    /// Policy couldn't decide, its counter or the party's scope register out of reach, so the work isn't done and
    /// the charge isn't refused (`REQ-AUZ-5`: not known is never a refusal).
    NotKnown {
        /// Why.
        cause: NotKnownCause,
    },
}

impl ChargeOutcome {
    /// The outcome of the entry that records this answer: within the bound is permitted, over it refused as
    /// `over_bound`, a refusal refused for its class, and not known not known, as [`PersonOutcome::outcome`] maps
    /// them.
    pub fn outcome(self) -> Outcome {
        match self {
            ChargeOutcome::Within { .. } => Outcome::Permitted {},
            ChargeOutcome::Over { .. } => Outcome::Refused {
                class: RefusalClass::OverBound,
            },
            ChargeOutcome::Refused { class } => Outcome::Refused { class },
            ChargeOutcome::NotKnown { cause } => Outcome::NotKnown { cause },
        }
    }
}

/// The answer to a charge, appended to the log before it is given. Schema: `urn:rutent:core-internal:charge-answer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChargeAnswer {
    /// The request it answers.
    pub request: Id,
    /// The answer.
    pub outcome: ChargeOutcome,
    /// The receipt of its entry in the log.
    pub receipt: Receipt,
}

/// The authority a question about a person rests on: the person's own, or a shortlisted route's (`REQ-CPL-2`).
/// Schema: `urn:rutent:core-internal:person-question#/$defs/authority`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Authority {
    /// The person's own consent, by the digest of its record.
    Person {
        /// The consent.
        consent: Digest,
    },
    /// A route the router shortlisted for the order: the digest of the shortlist round's recorded decision, the
    /// providers it shortlisted, and the verifiers its tuples need, which receive the subject, so policy can hold
    /// who is asked to them (`REQ-CPL-5`, `REQ-CPL-6`).
    ShortlistedRoute {
        /// The shortlist.
        shortlist: Digest,
        /// The shortlisted providers, at least one, each once.
        providers: Vec<Id>,
        /// The verifiers the shortlisted route's tuples need, each once: none where they need none.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        verifiers: Vec<Id>,
    },
}

/// A question about a person, before it is asked of any provider or verifier (`REQ-CPL-2`, `REQ-CPL-5`). Schema:
/// `urn:rutent:core-internal:person-question`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct PersonQuestion {
    /// The request's id.
    pub request: Id,
    /// Who asks, identified.
    pub asker: Actor,
    /// The person, by reference.
    pub person: Id,
    /// The order it is about.
    pub order: Id,
    /// The authority it rests on.
    pub authority: Authority,
    /// The providers and verifiers to be asked, at least one, each once.
    pub ask: Vec<Id>,
    /// The providers and verifiers already asked about the order, across its recalculations, each once: empty
    /// before the first question, never absent.
    pub asked_before: Vec<Id>,
    /// When, by Core's clock.
    pub at: Timestamp,
}

/// Policy's answer to a question about a person. Schema: `urn:rutent:core-internal:person-answer#/$defs/outcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case", deny_unknown_fields)]
pub enum PersonOutcome {
    /// It may be asked.
    Allowed {},
    /// It may not.
    Refused {
        /// Why.
        class: RefusalClass,
    },
    /// The set asked about the order would pass its bound: the order counts as having no route left
    /// (`REQ-CPL-5`, `REQ-ORD-28`).
    PastBound {
        /// The bound.
        bound: SafeInteger,
    },
    /// Policy couldn't decide, so it isn't asked.
    NotKnown {
        /// Why.
        cause: NotKnownCause,
    },
}

impl PersonOutcome {
    /// The outcome of the entry that records this answer: allowed is permitted, past the bound refused as
    /// `over_bound`.
    pub fn outcome(self) -> Outcome {
        match self {
            PersonOutcome::Allowed {} => Outcome::Permitted {},
            PersonOutcome::Refused { class } => Outcome::Refused { class },
            PersonOutcome::PastBound { .. } => Outcome::Refused {
                class: RefusalClass::OverBound,
            },
            PersonOutcome::NotKnown { cause } => Outcome::NotKnown { cause },
        }
    }
}

/// The answer to a question about a person, appended to the log before it is given. Schema:
/// `urn:rutent:core-internal:person-answer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersonAnswer {
    /// The request it answers.
    pub request: Id,
    /// The answer.
    pub outcome: PersonOutcome,
    /// The receipt of its entry in the log.
    pub receipt: Receipt,
}

// --- What the log's checks read --------------------------------------------------------------------------------

/// One provider's report about its own segment: how much of which asset it says it sent or received, and the
/// receipt of the stated entry that holds its statement, made by that provider. Schema:
/// `urn:rutent:core-internal:report-pair#/$defs/report`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    /// The provider that reports: the statement's maker, its `acts`, so two adjacent providers of one publisher
    /// report a pair (`REQ-ORD-9`), and the provider this log moved its segment's value with
    /// (`report-pair-is-by-its-segments-providers`).
    pub party: Id,
    /// The segment the report is about, which its statement's `about` names (`T-020-38`): at a segment's end Core
    /// holds that segment's provider's result, and the next provider's report of receiving is its own result, about
    /// the next segment.
    pub segment: Id,
    /// The amount, in the asset's unit.
    pub amount: Amount,
    /// The asset.
    pub asset: Denomination,
    /// The receipt of the stated entry that holds the party's statement.
    pub statement: Receipt,
}

/// Whether the sending party's figure holds its charge (`REQ-CAT-11`): its tuple's gross or net convention, as the
/// comparison reads it. Schema: `urn:rutent:core-internal:report-pair#/$defs/reported_as`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReportedAs {
    /// Net of its charge: what it states it sent is what it gave out.
    Net {},
    /// Gross of its charge: what it states it sent holds its charge, which the next party never receives.
    Gross {
        /// The charge the figure holds, in the reports' unit, which the comparison takes off it: no more than the
        /// amount sent (`ReportPair::check`). A conversion's charge is in an asset it doesn't give out, so `0`.
        charge: Amount,
    },
}

/// The terms of the sending party's tuple a pair is compared under (`REQ-CAT-11`, ADR-0009): its declared fill
/// increment and its gross or net convention, which orchestration fills from the tuple's terms where it builds the
/// pair. Schema: `urn:rutent:core-internal:report-pair#/$defs/terms`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportTerms {
    /// The tuple's fill increment, in the reports' unit: two reports less than one increment apart raise nothing.
    /// Absent where the tuple declares none, and the reports are then compared exactly.
    #[serde(
        default,
        deserialize_with = "some",
        skip_serializing_if = "Option::is_none"
    )]
    pub increment: Option<PositiveAmount>,
    /// Whether the sending party's figure is gross or net of its charge.
    pub reporting: ReportedAs,
}

/// A segment's pair of reports: what its provider says it sent, and what the provider of the route's next segment
/// says it received (`REQ-ORD-9`), each about its own segment, with the terms they are compared under: segment n's
/// `amount_out` against segment n+1's `amount_in`, under segment n's tuple's terms. The last segment of a route has
/// no pair. Schema: `urn:rutent:core-internal:report-pair`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct ReportPair {
    /// The order.
    pub order: Id,
    /// The segment whose end the pair compares: the sent report's (`sent.segment`, [`ReportPair::check`]).
    pub segment: Id,
    /// What the sending party reports, about the pair's segment.
    pub sent: Report,
    /// What the receiving party reports, about the route's next segment after the sent one
    /// (`received-report-is-about-the-next-segment`).
    pub received: Report,
    /// The sending party's tuple's increment and convention, so like is compared with like within one increment.
    pub terms: ReportTerms,
}

/// Whose word a holding account's balance is (`REQ-AUD-15`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Word {
    /// rutent read it at a public node.
    Checked,
    /// The node's publisher's word, at a private node.
    Publisher,
}

/// The value one order holds in a holding account. Schema: `urn:rutent:core-internal:holding-reading#/$defs/held`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Held {
    /// The order.
    pub order: Id,
    /// The amount it holds there, in the asset's unit.
    pub amount: PositiveAmount,
}

/// A holding account's balance, with the value each order holds there, for the check that they agree
/// (`REQ-AUD-15`). It goes to the check, never to the log, which holds no balance (`REQ-AUD-6`). Schema:
/// `urn:rutent:core-internal:holding-reading`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct HoldingReading {
    /// The holding account.
    pub account: Id,
    /// The provider whose module holds it.
    pub provider: Id,
    /// The inner node it is at.
    pub node: Id,
    /// The asset.
    pub asset: Denomination,
    /// Its balance, in the asset's unit.
    pub balance: Amount,
    /// Whose word the balance is.
    pub word: Word,
    /// What each order holds there, each order once: empty where no order holds anything, never absent.
    pub held_for: Vec<Held>,
    /// When it was read, by Core's clock.
    pub at: Timestamp,
}

/// A yes or a no.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Said {
    /// Yes.
    Yes,
    /// No.
    No,
}

/// How a segment ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentEnd {
    /// It completed.
    Done,
    /// It failed.
    Failed,
}

/// Which way the market moved over a failed segment, against its direction (`REQ-AUD-7`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Market {
    /// For the provider: failing let it keep a rate that moved its way.
    WithTheProvider,
    /// Against the provider.
    AgainstTheProvider,
    /// No material move.
    Flat,
}

/// What one measure record counts (`REQ-AUD-7`). Schema: `urn:rutent:core-internal:measure-record#/$defs/measure`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Measure {
    /// A quote against the price executed.
    Quote {
        /// The amount quoted, in the asset's unit.
        quoted: Amount,
        /// The amount executed.
        executed: Amount,
        /// The asset.
        asset: Denomination,
    },
    /// A dry run against the segment's outcome.
    DryRun {
        /// What the dry run said.
        said: Said,
        /// How the segment ended.
        outcome: SegmentEnd,
    },
    /// A failure, against the market's direction where a figure for it is held.
    Failure {
        /// The market's direction over the segment, absent until a market figure is held.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        market: Option<Market>,
    },
    /// A refusal.
    Refusal {},
    /// A reversal.
    Reversal {},
    /// An answer, counted towards the rate of no.
    Answer {
        /// What it said.
        said: Said,
    },
}

/// The role a measure record measures its party in (`REQ-AUD-7`, `REQ-AUD-8`): a party that is both a provider and a
/// verifier is measured apart in each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasuredRole {
    /// As a provider: its quotes, dry runs, failures, refusals, reversals and answers.
    Provider,
    /// As a verifier: its answers.
    Verifier,
}

/// One record the bulk measures read, per provider or verifier (`REQ-AUD-7`): rutent's record of what it measured,
/// citing the entries, stated or verified, it rests on. Schema: `urn:rutent:core-internal:measure-record`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct MeasureRecord {
    /// The provider or verifier it measures.
    pub provider: Id,
    /// The role it measures that party in, which the caller sets: the measures keep each role apart.
    pub role: MeasuredRole,
    /// The order.
    pub order: Id,
    /// The segment, where it is about one.
    #[serde(
        default,
        deserialize_with = "some",
        skip_serializing_if = "Option::is_none"
    )]
    pub segment: Option<Id>,
    /// What it counts.
    pub measure: Measure,
    /// The receipts of the entries it rests on, at least one, each once: a provider's word stays in its stated
    /// entry.
    pub cites: Vec<Receipt>,
    /// When, by Core's clock.
    pub at: Timestamp,
}

// --- What the platform and the proxy hand over -----------------------------------------------------------------

/// A module's part.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    /// The sandboxed part.
    Sandboxed,
    /// The outside part.
    Outside,
}

/// A secret lookup's outcome, without its value. Schema:
/// `urn:rutent:core-internal:handover#/$defs/lookup_outcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum LookupOutcome {
    /// The proxy resolved it.
    Resolved {},
    /// The proxy refused it.
    Refused {
        /// Why.
        reason: SecretRefusalReason,
    },
}

/// What a use of the proxy asked for. Schema: `urn:rutent:core-internal:handover#/$defs/use_of`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum UseOf {
    /// A declared secret, by name.
    Secret {
        /// The name.
        name: Name,
    },
    /// The statement key's private half.
    StatementKey {},
}

/// How a use of the proxy ended, without any value. Schema: `urn:rutent:core-internal:handover#/$defs/use_outcome`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum UseOutcome {
    /// Resolved.
    Resolved {},
    /// Refused.
    Refused {
        /// Why, as the proxy names it: a label to read, which no decision branches on.
        reason: Token,
    },
}

/// Why a segment moved to another version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveReason {
    /// Its version was marked stale (`REQ-MDL-19`).
    Stale,
}

/// One move of a segment from a version to another (`REQ-MDL-5`). Schema:
/// `urn:rutent:core-internal:handover#/$defs/move`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Move {
    /// The version it ran on.
    pub from: Id,
    /// The version it runs on from then on.
    pub to: Id,
    /// Why.
    pub reason: MoveReason,
    /// When.
    pub at: Timestamp,
}

/// What the module platform and the credential proxy reported, handed to the log whole, each item for its
/// transaction (`REQ-MDL-8`, `REQ-MDL-5`, `REQ-MDL-6`). Schema: `urn:rutent:core-internal:handover`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    remote = "Self",
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum HandedOver {
    /// An outbound call record, as the module interface shapes it.
    OutboundCall {
        /// The record.
        record: OutboundCallRecord,
    },
    /// A cross-module call's record, as the module interface shapes it.
    CrossModuleCall {
        /// The record.
        call: CrossModuleCall,
    },
    /// A secret lookup a module's part made, and its outcome without its value.
    SecretLookup {
        /// The provider whose module made it.
        provider: Id,
        /// The part that made it.
        part: Part,
        /// The lookup, which names its transaction.
        lookup: SecretLookup,
        /// The proxy's answer, or none where the lookup never reached the proxy, as a sandboxed part's must not.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        outcome: Option<LookupOutcome>,
    },
    /// A state read a module's part made.
    StateRead {
        /// The provider whose module made it.
        provider: Id,
        /// The part that read.
        part: Part,
        /// The transaction it was handling.
        transaction: Id,
        /// The entry asked for.
        key: StateKey,
        /// Whether the read found a value.
        found: bool,
    },
    /// A bound a module hit (`REQ-MDL-11`).
    Limit {
        /// The provider whose module hit it.
        provider: Id,
        /// The transaction it was handling.
        transaction: Id,
        /// Which bound, as the platform names it: a label to read, which no decision branches on.
        limit: Token,
        /// Which budget of it, where it was a count or a total.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        scope: Option<Token>,
    },
    /// One use of the credential proxy, resolved or refused, never with the value (`REQ-MDL-6`).
    SecretUse {
        /// The caller's provider.
        provider: Id,
        /// The version of the caller's admitted release, if one is admitted.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        module_version: Option<Id>,
        /// The transaction the use was for.
        transaction: Id,
        /// The outside operation it was for: present exactly when it asked for a secret, absent for the statement
        /// key, which serves the operation handled.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        operation: Option<Name>,
        /// What it asked for.
        of: UseOf,
        /// How it ended.
        outcome: UseOutcome,
    },
    /// An operation whose module passed something out and then ended with no answer the platform passes on: the
    /// platform's own record of it (`Events::unknown`), so an outcome unknown can cite it (`REQ-MDL-8`, `REQ-ORD-1`).
    OperationUnanswered {
        /// The provider whose module handled it.
        provider: Id,
        /// The operation's transaction.
        transaction: Id,
        /// What ended it, as the platform names it: the reason it would have been refused for had the module passed
        /// nothing out. A label to read, which no decision branches on.
        ended_by: Token,
    },
    /// A segment's record, as the platform hands it over when the segment ends (`REQ-MDL-5`).
    SegmentRecord {
        /// The segment's transaction.
        transaction: Id,
        /// The provider.
        provider: Id,
        /// The module version it began on.
        began_on: Id,
        /// The module version it ran on at its end.
        runs_on: Id,
        /// Its moves, in order, from the version it began on to the one it ran on.
        moves: Vec<Move>,
    },
}

// --- Raises ----------------------------------------------------------------------------------------------------

/// Which way a holding account is off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// The balance is short of the value held for orders.
    Short,
    /// The balance is over it.
    Over,
}

/// One party's figure in a pair of reports that can't be compared: an amount, in the asset and unit it was stated in.
/// Schema: `urn:rutent:core-internal:raise#/$defs/reported`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reported {
    /// The amount, in the asset's unit.
    pub amount: Amount,
    /// The asset, and the unit it is counted in.
    pub asset: Denomination,
}

/// Why a holding account's balance can't be compared with the value held or evidenced there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HoldingIncomparable {
    /// Value is evidenced at the account in another asset, or another unit, than the account's (`INV-1`).
    EvidencedInAnotherAsset,
    /// Value is evidenced at an account no reading of its balance was given for (`INV-1`).
    NoReading,
    /// The value held or evidenced there is past the balance by more than an amount can state (78 digits): a short
    /// no `by` can state. A sum past the bound whose difference from the balance an amount states is a `holding`
    /// difference instead (`REQ-AUD-15`, `INV-1`).
    SumPastTheBound,
}

/// The difference a raise carries. Schema: `urn:rutent:core-internal:raise#/$defs/difference`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Difference {
    /// A segment's two reports disagree (`REQ-ORD-9`).
    Reports {
        /// The order raised.
        order: Id,
        /// The segment.
        segment: Id,
        /// What the sending party reports it sent.
        sent: Amount,
        /// What the receiving party reports it received.
        received: Amount,
        /// The asset.
        asset: Denomination,
    },
    /// A holding account's balance and the value held there for orders disagree (`REQ-AUD-15`).
    Holding {
        /// The holding account.
        account: Id,
        /// Which way it is off.
        direction: Direction,
        /// By how much, in the asset's unit.
        by: PositiveAmount,
        /// The asset.
        asset: Denomination,
        /// Whose word the balance was.
        word: Word,
    },
    /// A segment's two reports can't be compared: they state two assets, or one asset in two units (`REQ-ORD-9`,
    /// `THR-107`). Each figure is given as it was stated.
    ReportsNotComparable {
        /// The order raised.
        order: Id,
        /// The segment.
        segment: Id,
        /// What the sending party reports it sent.
        sent: Reported,
        /// What the receiving party reports it received.
        received: Reported,
    },
    /// A holding account's balance can't be compared with the value held or evidenced there (`REQ-AUD-15`,
    /// `INV-1`).
    HoldingNotComparable {
        /// The holding account.
        account: Id,
        /// Whose word the balance was: present exactly when a balance was read, so absent for `no_reading`.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        word: Option<Word>,
        /// The account's asset, in the unit its balance was read in: present exactly when a balance was read, so
        /// absent for `no_reading`. The log's check entry names no asset, so the raise states it.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        asset: Option<Denomination>,
        /// Why it can't be compared.
        reason: HoldingIncomparable,
    },
    /// A provider charged above its formula's maximum (`REQ-QTE-5`).
    Charge {
        /// The order.
        order: Id,
        /// The segment.
        segment: Id,
        /// The provider.
        provider: Id,
        /// What it charged, in the asset's unit.
        charged: Amount,
        /// Its formula's maximum.
        maximum: Amount,
        /// The asset.
        asset: Denomination,
        /// The receipt of the stated entry of the result whose charge this is: its provider's verified result about
        /// the order and segment.
        result: Receipt,
        /// The receipt of the stated entry of the provider's signed terms (provider-role's
        /// `urn:rutent:provider-role:terms`) whose formula the charge is above: in force at the segment's start, by
        /// [`StatedEntry::binds_from`]. Absent where the provider stated none by then, so the price its route was
        /// priced on bound it.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        terms: Option<Receipt>,
        /// Set where the charge is stated in an asset no formula covers: a formula carries no asset and counts in the
        /// segment's `from` asset only, so `maximum` is then no formula's and reads as nothing allowed. Only a charge
        /// in another asset than the segment's is so marked (`no-formula-marks-a-charge-in-another-asset`,
        /// `REQ-QTE-5`). Absent for a charge held to a formula.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        no_formula: Option<NoFormula>,
    },
    /// A non-final segment's public record contradicts what its provider said it moved (`REQ-ORD-33`): the
    /// difference of a [`What::PublicMismatch`], raised and changing nothing else (ADR-0009).
    PublicEnd {
        /// The order.
        order: Id,
        /// The segment.
        segment: Id,
        /// Which figure differs.
        found: PublicEndMismatch,
        /// What the network moved.
        moved: PublicFigures,
        /// What the provider said it moved.
        stated: PublicFigures,
    },
    /// A segment's pair of reports was never built, so the comparison of the two parties' words was never made
    /// (`REQ-ORD-9`, `ADR-0009`, `T-020-48`): a pair a restart dropped, whose sent report went with the stopped process,
    /// or one that couldn't be built or compared. A comparison is a detection, so the pair is parked and raised, never
    /// skipped silently. The raise may cite no receipt, as a pair dropped by a restart has none to cite; no other kind
    /// of raise may (`raise-of-a-pair-not-built-names-a-segment-its-plan-lists`).
    PairNotBuilt {
        /// The order.
        order: Id,
        /// The later segment of the two: the one whose end owed the pair.
        segment: Id,
        /// Why the pair was never built.
        why: PairNotBuilt,
    },
    /// A holding account whose balance Core asked and its module didn't give (`REQ-AUD-15`, `REQ-ORD-7`, `T-020-48`): the
    /// platform passed no answer, the module refused, or it answered another question, so no check could be made. It
    /// carries no balance (`REQ-AUD-6`): there is none. Raised once in a spell of failure, the spell ending at the
    /// account's next check; it may cite no receipt, since an account never read has no check to cite, and cites only
    /// the account's own last check where it has one (`raise-of-a-holding-not-read-cites-its-accounts-check`).
    HoldingNotRead {
        /// The holding account.
        account: Id,
        /// The inner node it is at.
        node: Id,
        /// The provider whose module holds it.
        provider: Id,
        /// Why it wasn't read.
        why: HoldingNotRead,
    },
    /// A material change in a provider's p99 settlement time (`REQ-ORD-6`).
    SettlementTime {
        /// The provider.
        provider: Id,
        /// The p99 before, in seconds.
        before_seconds: SafeInteger,
        /// The p99 now, in seconds.
        now_seconds: SafeInteger,
    },
}

/// Why a segment's pair of reports was never built. Schema: `urn:rutent:core-internal:raise#/$defs/pair_not_built`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PairNotBuilt {
    /// Core holds no sent report of the segment before: its result that counted isn't in hand, as after a restart.
    NoSentReport {
        /// The segment whose sent report is missing: the one before the pair's later segment.
        segment: Id,
    },
    /// The segment's flight isn't held.
    NoFlight {},
    /// A figure of one of the reports isn't an amount the contract carries.
    NotAnAmount {},
    /// The contract refuses the pair.
    Refused {},
}

/// Why a holding account's balance wasn't read. Schema: `urn:rutent:core-internal:raise#/$defs/holding_not_read`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum HoldingNotRead {
    /// The platform passed the module's answer on to no one: it refused the question, or the module gave none.
    NotAnswered {},
    /// The module answered and didn't give the balance.
    Refused {},
    /// The module answered another question, or of another transaction, than the one asked.
    AnotherAnswer {},
    /// The balance the module gave is no amount a reading carries.
    NotABalance {},
}

/// The mark of a charge stated in an asset no fee formula covers. Schema:
/// `urn:rutent:core-internal:raise#/$defs/difference_charge/properties/no_formula`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoFormula {
    /// No formula of the segment's tuple counts in the asset the charge is stated in.
    ForThisAsset,
}

/// Which figure of a non-final segment's public record contradicts its provider's, in the order the movement's check
/// reads them: its destination, then its asset, then its amount (`REQ-ORD-33`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublicEndMismatch {
    /// The movement went to another destination.
    WrongDestination,
    /// It moved another asset.
    WrongAsset,
    /// It moved another amount.
    WrongAmount,
}

/// A movement's figures at a public end: where it went, what asset and how much, as the network or the provider
/// states them. Schema: `urn:rutent:core-internal:raise#/$defs/public_figures`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicFigures {
    /// The destination on the network.
    pub destination: Id,
    /// The asset, by its id on the network.
    pub asset: Id,
    /// The amount, in the asset's unit.
    pub amount: Amount,
}

impl PublicEndMismatch {
    /// Whether `moved` and `stated` differ in the figure this names.
    pub fn differs(self, moved: &PublicFigures, stated: &PublicFigures) -> bool {
        match self {
            PublicEndMismatch::WrongDestination => moved.destination != stated.destination,
            PublicEndMismatch::WrongAsset => moved.asset != stated.asset,
            PublicEndMismatch::WrongAmount => moved.amount != stated.amount,
        }
    }
}

/// A raise to the operator: a difference and the entries it rests on, and nothing that can move value or change an
/// order (`REQ-ORD-9`, `REQ-AUD-15`, ADR-0009). Schema: `urn:rutent:core-internal:raise`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct Raise {
    /// The raise's id.
    pub raise: Id,
    /// The difference.
    pub difference: Difference,
    /// The receipts of the entries it rests on, at least one, each once.
    pub cites: Vec<Receipt>,
    /// When, by Core's clock.
    pub at: Timestamp,
}

// --- Orchestration's own records -------------------------------------------------------------------------------

/// How an order was accepted (`REQ-ORD-10`, `REQ-ORD-13`, `REQ-ORD-15`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptanceBasis {
    /// Its customer, or the customer's agent, accepted the quote shown; the entry's `who` says which.
    Given,
    /// A reserve that declared it started the route on an automatic deposit, which counts as acceptance.
    AutomaticDeposit,
    /// Its customer, or the customer's agent, accepted the quote shown for an order filed on an automatic deposit
    /// whose route has no firm rate, which the deposit alone can't accept (`REQ-QTE-10`): named with the session it
    /// was asked in, as `given` is, and resting on the reserve's automatic deposit, its arrival, in place of each
    /// part's yes, as `automatic_deposit` does (item `deposit-no-firm-rate`, `T-020-39`; `T-020-41` builds it).
    GivenOnDeposit,
}

impl AcceptanceBasis {
    /// Whether it rests on the reserve's automatic deposit, its arrival, in place of each part's yes:
    /// `automatic_deposit` and `given_on_deposit`.
    pub fn on_a_deposit(self) -> bool {
        matches!(
            self,
            AcceptanceBasis::AutomaticDeposit | AcceptanceBasis::GivenOnDeposit
        )
    }

    /// Whether its customer gave it in a session: `given` and `given_on_deposit`.
    pub fn in_a_session(self) -> bool {
        matches!(
            self,
            AcceptanceBasis::Given | AcceptanceBasis::GivenOnDeposit
        )
    }
}

/// An order's three parties, by reference, recorded apart (`REQ-ORD-17`). A third-party payer's details stay at
/// the provider: the record holds a reference at most (`REQ-CPL-11`). Schema:
/// `urn:rutent:core-internal:verified-entry#/$defs/parties`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parties {
    /// The requester.
    pub requester: Id,
    /// The beneficiary.
    pub beneficiary: Id,
    /// Where the beneficiary is a settlement account whose holder pays several payees from its book: the reserve
    /// that holds it, to which the payees' checks fall (`REQ-ORD-17`). Absent where the beneficiary is the payee.
    #[serde(
        default,
        deserialize_with = "some",
        skip_serializing_if = "Option::is_none"
    )]
    pub settlement_account_at: Option<Id>,
    /// The payer.
    pub payer: Id,
}

/// Why an order can't proceed, as the stable class its caller is told (`REQ-ORD-18`): what it asks of them, and
/// never a provider's own text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderRefusalClass {
    /// No route exists for this amount: another amount may have one.
    NoRouteForAmount,
    /// No route exists right now: the same request may have one later.
    NoRouteRightNow,
    /// The destination isn't resolved yet (`REQ-ORD-24`).
    DestinationNotYetResolved,
    /// The destination was resolved, and refused.
    DestinationRefused,
    /// Policy refused it.
    RefusedByPolicy,
    /// The caller's accountable party is past its bound.
    OverBound,
    /// The request breaks this deployment's rules for one.
    MalformedRequest,
    /// The destination's owner changed since it was resolved: the order waits for the customer (`REQ-ORD-24`).
    DestinationOwnerChanged,
    /// An input the decision rests on couldn't be read, so nothing is decided on it: policy's register of sessions
    /// unread, as policy itself answers `missing_input` (`REQ-AUZ-5`, `T-021-17`, `T-020-52`). The same request may
    /// get an answer once the register is read.
    MissingInput,
}

/// One part's yes on an acceptance: the part, its origin reserve, and the receipt of that reserve's stated yes to
/// the part's route (`REQ-ORD-32`). Schema: `urn:rutent:core-internal:verified-entry#/$defs/part_yes`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartYes {
    /// The part.
    pub part: Id,
    /// The part's origin reserve: the provider of its first withdrawal.
    pub origin_reserve: Id,
    /// The receipt of that reserve's stated yes.
    pub reserve_yes: Receipt,
}

/// How a destination came out when it was resolved or confirmed (`REQ-ORD-24`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DestinationOutcome {
    /// Resolved, to the same owner.
    Resolved,
    /// Not yet: no answer, which is not a no.
    NotYet,
    /// No.
    Refused,
    /// Its owner changed: the order waits for the customer.
    OwnerChanged,
}

/// How value arriving at a reserve met the orders open there (`REQ-ORD-14`, `REQ-ORD-23`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArrivalMatch {
    /// No order is open for it: it opens none.
    NoOrder,
    /// It matches the open order's amount.
    Matches,
    /// It differs from the open order's amount: it settles nothing by itself.
    Differs,
}

/// Which round of a routing a recorded decision is (`REQ-AUD-4`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingRound {
    /// Over advertised figures.
    Shortlist,
    /// Over refreshed figures for the shortlist.
    Choice,
}

/// How a figure was shown to the customer (`REQ-QTE-7`, `REQ-QTE-10`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShownAs {
    /// A quote, before execution: not a promise.
    Quote,
    /// A quote with no firm rate, for a route with a segment over three calendar days, naming it.
    QuoteWithoutFirmRate,
    /// The price executed.
    Price,
}

/// What a stuck call's clock ran on (`REQ-ORD-29`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StuckBasis {
    /// Twice the provider's p99 at the tuple.
    TwiceP99,
    /// Twice its advertised time, until it has a history.
    TwiceAdvertised,
}

/// Who a fee is attributed to: a provider or the operator, and no one else (`REQ-AUD-11`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeeTo {
    /// A provider.
    Provider,
    /// The operator.
    Operator,
}

/// What replaying private nodes' recorded segments against the public movement out found (`REQ-ORD-41`, `DEC-24`).
/// Schema: `urn:rutent:core-internal:verified-entry#/$defs/replay_found`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReplayFound {
    /// The records agree with themselves, with the public movement out and with the stated outcome: consistency with
    /// the public exit, never proof that the segments happened (`THR-131`).
    ConsistentWithTheExit {},
    /// Each way the records disagree, at least one, each once.
    Mismatch {
        /// The mismatches, in the order found.
        mismatches: Vec<ReplayMismatch>,
    },
    /// Nodes' publishers recorded nothing for the segments inside them, so nothing was compared: neither a match nor
    /// a mismatch, since recording is a SHOULD.
    NothingRecorded {
        /// Each node that recorded nothing, in route order, at least one, each once.
        nodes: Vec<Id>,
    },
}

/// A movement on a public network, as Core checked it (`REQ-ORD-33`, `REQ-ORD-38`, `DEC-13`). Schema:
/// `urn:rutent:core-internal:verified-entry#/$defs/public_movement`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicMovement {
    /// The public network.
    pub network: Id,
    /// The movement's identifier on it.
    pub id: Id,
    /// The private node it moved the value into or out of, where it evidences that node's segments rather than one
    /// segment alone: a part's final deposit to a rail declared private, closing on the movement into it, or a
    /// segment inside a private node, closing on the movements into and out of it (`REQ-ORD-33`). Absent otherwise.
    #[serde(
        default,
        deserialize_with = "some",
        skip_serializing_if = "Option::is_none"
    )]
    pub node: Option<Id>,
}

/// One way a replayed record disagrees, and the node it is against.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayMismatch {
    /// The node: one of those replayed. Where value passes between two nodes, the one it leaves.
    pub node: Id,
    /// How.
    pub kind: MismatchKind,
}

/// How a replayed record disagrees (`CMP-EVIDENCE`'s replay, `REQ-ORD-41`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MismatchKind {
    /// An amount or a fee isn't a whole number in its canonical spelling.
    Malformed,
    /// A segment doesn't take in what the one before it gave out.
    BrokenChain,
    /// A fee is more than what went in, or, where the asset stays the same, what went in isn't what came out plus
    /// the fee.
    FeeMismatch,
    /// The node doesn't hand on what the next one records taking in.
    HandoffBroken,
    /// The public movement out isn't a final record of the replayed value to its destination.
    PublicExit,
    /// The replayed value isn't the outcome the provider stated.
    StatedOutcome,
}

/// An operator command that needs no policy decision: none moves value, posts a route, changes the catalogue or grants
/// a permission (`REQ-AUZ-1`, `REQ-AUZ-6`, `REQ-AUZ-7`). A closed list. Filing an arrival anew is a `refiling`, and a
/// read permission goes through policy (`Reads::grant_permission`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorCommand {
    /// Appoint the checker outside the operator (`REQ-AUD-10`).
    AppointChecker,
    /// Accept a provider's showing that a failed segment's failure was the operator's, so the operator carries its
    /// reversal (`REQ-ORD-11`): about the order, its subject the segment.
    AcceptFault,
}

/// Something that came after an order ended (`REQ-ORD-25`, `REQ-ORD-31`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Afterwards {
    /// A complete order's destination reported the payment returned: the order stays complete.
    Returned,
    /// An origin approval arrived after the order ended: it starts nothing.
    LateApproval,
}

/// What a verified entry records: one of the calls between Core's parts, orchestration's own records, or what the
/// platform handed over. Schema: `urn:rutent:core-internal:verified-entry#/$defs/what`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "call", rename_all = "snake_case", deny_unknown_fields)]
pub enum What {
    /// A decision call, and the decision's id.
    Decide {
        /// The request.
        request: DecisionRequest,
        /// The decision.
        decision: Id,
    },
    /// An authorisation registered, and the decision's id.
    Authorise {
        /// The authorisation.
        authorisation: Authorisation,
        /// The decision.
        decision: Id,
    },
    /// An action about to take effect, before it does (`INV-7`).
    Effect {
        /// The action.
        action: Action,
        /// The permit it rests on.
        decision: Permit,
    },
    /// How an effect ended.
    Result {
        /// The receipt of the effect's entry.
        effect: Receipt,
    },
    /// A stuck order's decide step, and the decision's id.
    StuckDecide {
        /// The request.
        request: StuckDecide,
        /// The decision.
        decision: Id,
        /// The inner node whose holding account the deployment's register says the decision's mover
        /// (`resolution.by`) holds, as policy asked it (`T-021-16`): present exactly where nothing else this log holds
        /// ties that mover to the order, as for a hand-on through a public node's wallet (`T-020-33`).
        /// `stuck-decide-held-at-is-a-held-node` holds it to the inner node the stuck part's value rests at
        /// (`resting_node`), and only then is that mover a party to the order (`party_to_the_order`).
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        held_at: Option<Id>,
    },
    /// A stuck order's move step, and the decision's id.
    StuckMove {
        /// The request.
        request: StuckMove,
        /// The decision.
        decision: Id,
    },
    /// A session request, and the session opened where one was.
    OpenSession {
        /// The request.
        request: SessionRequest,
        /// The session: present exactly when the entry's outcome is permitted.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        opened: Option<Opened>,
    },
    /// A charge of work.
    Charge {
        /// The charge.
        charge: Charge,
        /// The engine's own clock when it counted the charge, beside the entry's `when.at`: present exactly where
        /// the charge was counted, `permitted` within the bound, so an engine started again rebuilds each party's
        /// count by window from the log (`T-021-05`).
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        counted_at: Option<Timestamp>,
    },
    /// A question about a person.
    AskAboutPerson {
        /// The question.
        question: PersonQuestion,
    },
    /// A read check.
    CheckRead {
        /// The check.
        check: ReadCheck,
    },
    /// A read permission granted, and the decision's id (`REQ-AUZ-7`, `REQ-AUD-12`).
    GrantPermission {
        /// The grant.
        change: PermissionChange,
        /// The decision.
        decision: Id,
    },
    /// A read permission revoked, and the decision's id.
    RevokePermission {
        /// The revocation.
        change: PermissionChange,
        /// The decision.
        decision: Id,
    },
    /// An arrival that differed from its open order filed anew against an order, by the operator or by the reserve
    /// it arrived at, which `who` names (`REQ-ORD-23`).
    Refiling {
        /// The receipt of the arrival's entry.
        arrival: Receipt,
        /// The order it is filed against.
        order: Id,
        /// The receipt of policy's permitted `decide` of `refile_arrival` it rests on (`T-020-52`): about this
        /// arrival and order, asked by `who`.
        decision: Receipt,
    },
    /// An item the platform or the proxy reported, at its place in the drain that took it, so two identical
    /// events of one drain are two entries and a retried append of one is still one (`REQ-MDL-8`).
    HandOver {
        /// The item.
        item: HandedOver,
        /// The drain that took it: one call of `take_events` or `take_uses`.
        drain: Id,
        /// Its place in that drain, from 0.
        position: SafeInteger,
    },
    /// A segment's pair of reports, compared.
    ReportPair {
        /// The pair.
        pair: ReportPair,
    },
    /// A holding account checked, without its balance (`REQ-AUD-6`).
    HoldingCheck {
        /// The holding account.
        account: Id,
        /// The provider whose module holds it.
        provider: Id,
        /// The inner node it is at.
        node: Id,
        /// Whose word its balance was.
        word: Word,
        /// The orders that held value there, each once.
        orders: Vec<Id>,
    },
    /// A measure record.
    Measure {
        /// The record.
        record: MeasureRecord,
    },
    /// A raise to the operator, and whether it reached the operator.
    Raise {
        /// The raise.
        raise: Raise,
    },
    /// A recorded input of the state machine, the only thing that changes an order's state: a route chosen or
    /// recalculated, a destination changed, no route, a provider bound, a segment settled, failed or of unknown
    /// outcome, a reversal completed, a withdrawal cancelled, an end requested, a part declared stuck or handed on
    /// (`REQ-STM-3`).
    Input {
        /// The input, as the state-machine contract shapes it.
        input: RecordedInput,
    },
    /// An order accepted (`REQ-ORD-10`, `REQ-ORD-13`, `REQ-ORD-34`).
    Acceptance {
        /// The acceptance's id: the one a state-machine input cites as its `acceptance` (`destination_changed`,
        /// `handed_on`), so the input and this record point to each other (`REQ-ORD-34`, `REQ-ORD-43`).
        acceptance: Id,
        /// The order.
        order: Id,
        /// The request identifier it was opened under (`REQ-ORD-19`).
        request: Id,
        /// The digest of the quote shown and accepted.
        quote: Digest,
        /// The authorisation it gave.
        authorisation: Id,
        /// How it was accepted.
        basis: AcceptanceBasis,
        /// Its parties, recorded apart.
        parties: Parties,
        /// The order's origin reserve: the one whose yes it rests on, or whose automatic deposit started it.
        origin_reserve: Id,
        /// Each part's yes, keyed by part: the receipt of its origin reserve's stated yes to the part's route, each
        /// part once. Present exactly when its basis is given; an automatic deposit carries none, nor an acceptance
        /// given on one (`REQ-ORD-32`, `REQ-QTE-10`). A split order has one per part, so no part moves on another's
        /// yes.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        yeses: Vec<PartYes>,
        /// The user's session it was asked in: present exactly when its customer gave it, `given` or
        /// `given_on_deposit` (`REQ-ORD-32`, `REQ-QTE-10`).
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        session: Option<Id>,
        /// The hold it redeems, where its route is a quote a reserve held (`REQ-QTE-15`): the receipt of the
        /// reserve's verified stated hold (provider-role's `urn:rutent:provider-role:hold`). Absent where it redeems
        /// none.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        held: Option<Receipt>,
    },
    /// A request or an order that can't proceed, with the class its caller is told (`REQ-ORD-18`, `REQ-ORD-19`).
    OrderRefusal {
        /// The request.
        request: Id,
        /// The order, where one exists: absent means "refused, nothing exists".
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        order: Option<Id>,
        /// The class.
        class: OrderRefusalClass,
    },
    /// A destination resolved or confirmed (`REQ-ORD-24`).
    DestinationCheck {
        /// The order.
        order: Id,
        /// The destination.
        destination: Destination,
        /// How it came out.
        outcome: DestinationOutcome,
    },
    /// Value arriving at a reserve, against the orders open there, with the reserve's report (`REQ-ORD-14`,
    /// `REQ-ORD-23`).
    Arrival {
        /// The reserve.
        reserve: Id,
        /// The outer node it arrived at.
        node: OuterNode,
        /// The asset.
        asset: Denomination,
        /// How much arrived, in the asset's unit.
        amount: Amount,
        /// The open order it was held against, where one is.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        order: Option<Id>,
        /// How it met the open orders.
        matched: ArrivalMatch,
        /// The receipt of the stated entry holding the reserve's report.
        report: Receipt,
        /// The deposit it is of, by the commitment of the reserve's notice of it, the statement the deposit was
        /// first reported in: on an arrival naming an order, required, whether it files an earlier arrival held
        /// against no order (its notice) or the deposit met the order at once (its own report's). One deposit
        /// opens one order (`one-order-per-deposit`).
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        notice: Option<Digest>,
        /// The part of the order it is for: required where the order's route has more than one part
        /// (`split-deposit-arrival-names-its-part`). The arrival's matcher fills it (`T-020-04`).
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        part: Option<Id>,
        /// The receipt of the earlier arrival of the same deposit this one holds against an order: the deposit as it
        /// was first recorded, against no order, or an earlier filing of it. So a checker sees one deposit, not
        /// several (`arrival-names-its-earlier-arrival`, `REQ-ORD-23`). Absent on a deposit's first arrival.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        earlier: Option<Receipt>,
        /// The party that holds the account the value reached, as the reserve's notice names it: whom a quote the
        /// order waits on its customer to accept waits for (`waiting-quote-is-for-its-arrivals-holder`, R-AWAITING,
        /// `T-020-45`). Absent where the filing names none.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        holder: Option<Id>,
        /// Where the reserve's notice names an automatic deposit of this credit to go, as the notice states it with
        /// who set it for whom (provider-role 4.0.0's `destination`, `T-020-50`): the order filed on the deposit is
        /// authorised to that destination and no other, for the holder the parameters were set for
        /// (`automatic-deposit-rests-on-its-arrival`'s `destination` and `set_for`). Absent, the notice names none and
        /// the deposit goes to its holder.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        destination: Option<alloc::boxed::Box<DepositDestination>>,
        /// The request identifier the filing that holds the deposit against its order was made under (`T-020-51`): so a
        /// Core that reopens knows the filing a wait belongs to. Absent on a deposit's first arrival and on any not
        /// held by a filing.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        request: Option<Id>,
        /// The salted commitment of the operator's statement of the filing's inputs and objective
        /// ([`FilingInputs`], shape [`FILING_INPUTS_SHAPE`]), kept in the evidence store and appended as a stated entry
        /// before this one (`filed-orders-inputs-are-a-kept-statement`), so the log holds no account (`REQ-AUD-6`) and
        /// a Core that reopens reads the filing back (`T-020-51`). Absent where the arrival isn't a filing's.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        inputs: Option<Digest>,
    },
    /// One round's recorded decision, by digest, chained to the order's history (`REQ-AUD-4`).
    DecisionRecord {
        /// The recorded decision's id: the one a state-machine input cites as its `decision_record`
        /// (`route_chosen`, `route_recalculated`, `destination_changed`, `no_route`).
        decision_record: Id,
        /// The order.
        order: Id,
        /// The round.
        round: RoutingRound,
        /// The digest of the recorded decision.
        record: Digest,
        /// The receipt of the order's previous decision record entry; absent on its first.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        previous: Option<Receipt>,
        /// The providers a shortlist round shortlisted, each once, and none where it kept none, so a `no_route`
        /// input has the record to cite (`REQ-AUD-4`): present exactly when the round is the shortlist, so a question
        /// on a shortlisted route is held to them (`REQ-CPL-2`, `REQ-CPL-6`).
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        shortlisted: Option<Vec<Id>>,
        /// The verifiers a shortlist round's tuples need, by its recorded decision, each once: only on a shortlist's
        /// record, and none where they need none, so a question on its route asks only them besides its providers
        /// (`REQ-CPL-5`, `REQ-CPL-6`).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        verifiers: Vec<Id>,
    },
    /// What was shown to the customer, by digest, and how (`REQ-QTE-2`, `REQ-QTE-7`, `REQ-QTE-10`).
    Shown {
        /// The order.
        order: Id,
        /// The digest of what was shown.
        shown: Digest,
        /// How it was shown.
        shown_as: ShownAs,
        /// Where the quote is one an order filed on an automatic deposit waits on its customer to accept: whom it
        /// waits for, until when the quote stands and which segments are slow, so the customer is told from the log
        /// alone (`REQ-QTE-10`, `REQ-ORD-20`, item `deposit-no-firm-rate`, `T-020-41`). Only on a quote shown without
        /// a firm rate.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        awaiting: Option<Awaiting>,
    },
    /// A segment's declared and actual settlement times (`REQ-QTE-3`).
    Settlement {
        /// The order.
        order: Id,
        /// The segment.
        segment: Id,
        /// The provider.
        provider: Id,
        /// The settlement time it declared with its quote, in seconds.
        declared_seconds: SafeInteger,
        /// The time it took, in seconds.
        actual_seconds: SafeInteger,
    },
    /// A segment's evidence graded through `CMP-EVIDENCE`, saying whose it is (`REQ-ORD-39`, `REQ-ORD-33`,
    /// `REQ-ORD-41`).
    Evidence {
        /// The evidence's id: the one a state-machine input cites as its `evidence` (`segment_settled`).
        evidence: Id,
        /// The order.
        order: Id,
        /// The segment.
        segment: Id,
        /// The grade: a statement alone grades as a statement.
        grade: EvidenceGrade,
        /// The party whose evidence it is.
        whose: Id,
        /// The receipts of the stated entries it rests on, at least one, each once.
        rests_on: Vec<Receipt>,
        /// The public movement Core checked for it, at whatever grade it earned: one movement evidences one segment
        /// of one order (`REQ-ORD-38`, `movement-evidences-one-segment`).
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        movement: Option<PublicMovement>,
        /// Inside a private node, the node's publisher whose word it is, labelled as such (`REQ-ORD-3`'s third
        /// sentence): the party its maker states for. Absent where it isn't a publisher's word.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        publisher: Option<Id>,
    },
    /// The fee captured on a settled segment, attributed to a provider or the operator, with no balance kept
    /// (`REQ-AUD-11`).
    Fee {
        /// The order.
        order: Id,
        /// The segment.
        segment: Id,
        /// Who it is attributed to.
        to: FeeTo,
        /// The provider or the operator, by id.
        party: Id,
        /// The fee, in the asset's unit; a formula may charge zero.
        amount: Amount,
        /// The asset.
        asset: Denomination,
        /// Whether it was billed separately, outside the amounts reported, as its provider stated it; absent where
        /// the provider didn't say.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        billed_separately: Option<bool>,
    },
    /// Private nodes' recorded segments replayed through `CMP-EVIDENCE` where the value leaves them onto a public
    /// node, against the public movement out and the stated outcome (`REQ-ORD-41`, `DEC-24`). A match is consistency
    /// with the public exit, not proof; a route ending inside a node has no such record.
    Replay {
        /// The order.
        order: Id,
        /// The segment the value left the private nodes by, onto the public node.
        segment: Id,
        /// The private nodes replayed, in route order, at least one, each once.
        nodes: Vec<Id>,
        /// What the replay found.
        found: ReplayFound,
        /// What the replay compared, at least one, each once: first the receipt of the exit segment's result, its
        /// stated entry, and then, for each private node replayed that recorded something, the receipt of its
        /// publisher's `inner-segments` record the replay read, at most one per such node (`REQ-AUD-9`), so a checker
        /// finds what was compared. `T-020-32` writes the records; until then a writer names the exit's alone.
        rests_on: Vec<Receipt>,
        /// The public movement out it was compared with, where the network ties one to the exit.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        movement: Option<PublicMovement>,
    },
    /// The figures behind a stuck call, recorded when used (`REQ-ORD-6`, `REQ-ORD-29`).
    StuckCall {
        /// The order.
        order: Id,
        /// The part.
        part: Id,
        /// The provider holding the funds.
        provider: Id,
        /// What the clock ran on.
        basis: StuckBasis,
        /// The figure it ran on, in seconds: the p99, or the advertised time.
        figure_seconds: SafeInteger,
        /// How long the funds had been held, in seconds.
        held_seconds: SafeInteger,
    },
    /// Something that came after the order ended, with the statement that reports it (`REQ-ORD-25`, `REQ-ORD-31`).
    Afterwards {
        /// The order.
        order: Id,
        /// What came.
        event: Afterwards,
        /// The receipt of the stated entry that reports it.
        statement: Receipt,
    },
    /// Whose judgement was relied on, and by whom (`REQ-CPL-12`).
    Reliance {
        /// The order.
        order: Id,
        /// The receipt of the stated entry holding the judgement.
        judgement: Receipt,
        /// Whose judgement it is: a verifier or a reserve.
        judge: Id,
        /// The provider that relied on it, which carries the loss of a false one.
        relied_on_by: Id,
        /// The order's inputs, by name, that rest on the judgement, as Core was told (`T-020-46`): what a Core that
        /// reopens rebuilds which input rests on which judgement from, so a segment after the reopening that carries
        /// one still writes its reliance (`REQ-CPL-12`). Left out where it names none.
        #[serde(default, skip_serializing_if = "alloc::vec::Vec::is_empty")]
        inputs: alloc::vec::Vec<Id>,
    },
    /// An operator command that needs no decision, naming its operator in the entry's `who` (`REQ-AUD-1`). The
    /// commands are a closed list: an action policy decides, a catalogue change or a hand-on travels only through
    /// its own call, never as a command.
    Command {
        /// The command: one of the few that need no decision.
        command: OperatorCommand,
        /// The order it is about, where it is about one.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        order: Option<Id>,
        /// What else it is about, by reference.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        subject: Option<Id>,
    },
    /// Evidence erased outside the log, by the reference that stays (`REQ-AUD-13`).
    Erasure {
        /// The commitment the erased bytes were kept under.
        evidence: Digest,
    },
    /// Who carries a failed segment's reversal (`REQ-ORD-11`): its provider, or the party at fault its provider
    /// showed, once that party accepted the showing.
    ReversalCarrier {
        /// The order.
        order: Id,
        /// The failed segment.
        segment: Id,
        /// Who carries the reversal.
        carrier: Carrier,
        /// The receipt of the provider's showing of fault, its verified stated entry: present exactly where the
        /// carrier isn't the provider.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        showing: Option<Receipt>,
        /// Who accepted the showing for the party at fault: present exactly where the carrier isn't the provider.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        accepted_by: Option<Actor>,
        /// The act the acceptance rests on, by receipt: for the customer, policy's permitted `decide` of its
        /// `accept_fault` (`Action::AcceptFault`), asked in its open session and naming the showing; for the operator,
        /// its `accept_fault` command. Present exactly where the carrier isn't the provider.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        accepted_with: Option<Receipt>,
    },
    /// A holding account's own record of the withdrawal that starts a route handed on (`REQ-ORD-40`, `INV-5`): the
    /// start proof, which settles the linked order's first segment. The linked order's first move rests on it
    /// (`handed-on-route-starts-on-its-holding-record`).
    HoldingWithdrawal {
        /// The linked order whose route it starts.
        order: Id,
        /// The order whose value the holding account held, the one the route was handed on from.
        handed_on_from: Id,
        /// The holding account's provider: the route's starting reserve, which made the record.
        holder: Id,
        /// The inner node the holding account is at, where the route handed on starts
        /// (`holding-withdrawal-starts-a-handed-on-route`).
        node: Id,
        /// The receipt of the record: the holder's verified stated entry of the withdrawal.
        record: Receipt,
        /// At a private node, the node's publisher whose word the record is, labelled as such (`REQ-ORD-40`): the
        /// party the record's maker states for. Absent where it isn't a publisher's word.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        publisher: Option<Id>,
    },
    /// An input recorded, but not taken: the state machine or the host refused it, so it moves no order, and the
    /// log keeps its trace (`REQ-AUD-1`) beside the inputs it took, which `standing` reads alone.
    InputNotTaken {
        /// The input.
        input: RecordedInput,
        /// Why it wasn't taken.
        reason: NotTaken,
    },
    /// A move whose outcome was unknown, a `move_value` or a stuck resolution's `resolve_stuck`, its outcome
    /// reported by its own mover: a decision on policy's handle, answered by policy for its reporter. It settles
    /// nothing until another actor confirms it ([`What::ConfirmOutcome`]).
    ReportOutcome {
        /// The request.
        request: OutcomeReport,
        /// The decision.
        decision: Id,
    },
    /// A move whose outcome was unknown, its mover's report of it confirmed by an actor of another party and login:
    /// a decision on policy's handle, answered by policy for its confirmer (`REQ-AUZ-4`, ADR-0011).
    ConfirmOutcome {
        /// The request.
        request: OutcomeConfirmation,
        /// The decision.
        decision: Id,
    },
    /// A mover's report of a move's outcome declined by an actor of another party and login, on its own evidence: a
    /// decision on policy's handle, answered by policy for its decliner as a confirmation is (`REQ-AUZ-4`,
    /// ADR-0011). A declined report settles nothing and frees nothing, and only after it may a new report of the same
    /// effect stand (`one-report-per-effect`); no confirmation confirms it.
    DeclineOutcome {
        /// The request.
        request: OutcomeDecline,
        /// The decision.
        decision: Id,
    },
    /// One notice's summary, once it was delivered or abandoned: what a restart needs to send it no more
    /// (`REQ-ORD-20`, `REQ-ORD-7`). A raise's summary closes the raise's own entry, written once (`done` or `failed`).
    NoticeSummary {
        /// Whom it was for.
        to: NoticeReceiver,
        /// What it was a notice of.
        of: NoticeOf,
        /// The number of its last try, counted from 1.
        last_try: Count,
        /// How it ended.
        ended: NoticeEnd,
    },
    /// A non-final segment whose public record contradicts what its provider said it moved: the segment still
    /// settles on its provider's declared grade, and the contradiction is recorded here and raised (`REQ-ORD-33`,
    /// `REQ-AUD-1`, ADR-0009).
    PublicMismatch {
        /// The order.
        order: Id,
        /// The segment.
        segment: Id,
        /// The public node its movement is at.
        node: OuterNode,
        /// The movement Core checked.
        movement: PublicMovement,
        /// Which figure differs.
        found: PublicEndMismatch,
        /// What the network moved.
        moved: PublicFigures,
        /// What the provider said it moved.
        stated: PublicFigures,
        /// The receipt of what the provider said: its verified result about the order and segment
        /// (`public-mismatch-rests-on-the-providers-result`).
        rests_on: Receipt,
    },
    /// A stuck part's evidence, graded, saying whose it is: the evidence a stuck resolution's `stuck_resolved` cites,
    /// of a part and no segment, since a resolution moves the part's value outside its plan (`REQ-ORD-39`,
    /// `REQ-AUZ-4`).
    PartEvidence {
        /// The evidence's id: the one `stuck_resolved` cites as its `evidence`.
        evidence: Id,
        /// The order.
        order: Id,
        /// The part.
        part: Id,
        /// The grade: a statement alone grades as a statement.
        grade: EvidenceGrade,
        /// The party whose evidence it is.
        whose: Id,
        /// The receipts of the stated entries it rests on, at least one, each once.
        rests_on: Vec<Receipt>,
    },
    /// The end of an order's wait for its customer's acceptance, unaccepted (finding 8 of `M3`'s milestone review,
    /// `T-020-45`, `T-020-51`): the order filed on a deposit that waited on a quote its customer never accepted. Once
    /// it is logged the order takes no acceptance, and its deposit is free for another order
    /// (`one-order-per-deposit`); nothing moved, so the funds stay in the holder's account (`GUA-1`).
    WaitEnded {
        /// The order whose wait ended.
        order: Id,
        /// Why it ended.
        why: WaitEnd,
    },
    /// An order as intake opened it, before any acceptance (`T-020-45`, for `T-020-46`'s rebuild on reopening): the
    /// request it was opened under, who asked, its parties, its origin reserve, the session the reserve's yes is asked
    /// in, its destination and the route asked for. The log holds nothing else of an order before its acceptance, so a
    /// Core that reopens reads its open orders from here. The tuples' inputs are not here: they may carry accounts, so
    /// the log holds only the salted commitment of the operator's statement of them, kept in the evidence store
    /// (`inputs`, `REQ-AUD-6`, `T-020-46`).
    OrderOpened {
        /// The order.
        order: Id,
        /// The request identifier it was opened under (`REQ-ORD-19`).
        request: Id,
        /// Who asked: the requester's person or agent, in the customer's role.
        caller: alloc::boxed::Box<Actor>,
        /// Its parties, recorded apart.
        parties: alloc::boxed::Box<Parties>,
        /// The reserve the value leaves from.
        origin_reserve: Id,
        /// The user's session at the origin reserve (`REQ-ORD-32`).
        session: Id,
        /// Where the value arrives.
        destination: alloc::boxed::Box<Destination>,
        /// The route asked for: from, to, amount, objective and any floor.
        route: alloc::boxed::Box<crate::router_api::Request>,
        /// The salted commitment of the operator's statement of the order's inputs ([`OrderInputs`], shape
        /// [`ORDER_INPUTS_SHAPE`]), kept in the evidence store and appended as a stated entry before this one
        /// (`opened-orders-inputs-are-a-kept-statement`): a Core that reopens reads the inputs back from the store, and
        /// the log holds no account (`REQ-AUD-6`, `T-020-46`). Absent where the order carries no input.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        inputs: Option<Digest>,
    },
}

/// Why an order's wait for its customer ended (`T-020-45`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitEnd {
    /// The wait's own lifetime passed (`T-020-51`).
    PastItsLifetime,
    /// The session the customer could accept in is over.
    SessionOver,
    /// The order was refused while it waited, of the class its `order_refusal` names.
    OrderRefused,
}

/// What an order waiting for its customer's acceptance of a slow route tells the customer: the party it waits for, when
/// the quote stops standing and which of its segments are slow. No provider, amount or account (`REQ-AUZ-7`).
/// Schema: `urn:rutent:core-internal:verified-entry#/$defs/awaiting`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Awaiting {
    /// The party whose acceptance it waits for: the order's requester, receiving as its customer.
    pub requester: Id,
    /// When the quote stops standing, where any figure it rests on states a validity: the earliest of them, to the
    /// whole second below it. None where none does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<Timestamp>,
    /// The segments over three days, at least one, each by its place in the route, once.
    pub slow: Vec<SlowSegment>,
}

/// A slow segment of the route shown, by its place in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlowSegment {
    /// Its place in the route, counted from 1, part after part.
    pub position: Count,
    /// The time its provider declared, in seconds, where it declared one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_seconds: Option<Count>,
}

/// Whom a notice was for: a party, in the role it receives in. Schema:
/// `urn:rutent:core-internal:verified-entry#/$defs/notice_receiver`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoticeReceiver {
    /// The party.
    pub party: Id,
    /// The role it receives in.
    pub role: Role,
}

/// What a notice was of. Schema: `urn:rutent:core-internal:verified-entry#/$defs/notice_of`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NoticeOf {
    /// A change of an order: the `sequence`-th of its changes, counted from 1.
    Order {
        /// The order.
        order: Id,
        /// The change's sequence.
        sequence: Count,
    },
    /// A raise to the operator.
    Raise {
        /// The raise's id (`notice-summary-names-its-raise`).
        raise: Id,
    },
    /// An order's waiting for its customer's acceptance of a quote, told by the quote's digest: it has no sequence
    /// before it starts (`T-020-41`).
    Waiting {
        /// The order.
        order: Id,
        /// The quote's digest, as the `shown` entry holds it.
        quote: Digest,
    },
    /// The end of an order's wait for its customer (`wait_ended`), told once: a restart reads this and sends it no
    /// more (`T-023-04`, `T-020-45`).
    WaitEnded {
        /// The order.
        order: Id,
    },
}

/// How a notice ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeEnd {
    /// Its receiver's transport took it.
    Delivered,
    /// Its last try failed, and it is sent no more: its receiver recovers by asking for the current state.
    Abandoned,
}

/// Why a recorded input wasn't taken: the state machine's answer, or the host's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NotTaken {
    /// The state machine answered it not applicable, for this reason.
    NotApplicable {
        /// Why.
        reason: crate::state_machine::NotApplicableReason,
    },
    /// Its record id names another input this log holds: the host's exactly-once refuses it (`RecordIdReused`).
    RecordIdReused {},
}

/// A move's outcome, once unknown, as its own mover reports it (`REQ-AUZ-4`, ADR-0011): for a `move_value`, the
/// actor its `decide` step was for, or one of that actor's accountable party; for a stuck resolution's
/// `resolve_stuck`, its `stuck_move`'s mover. Its evidence is a verified stated entry of the log about the move's
/// order and part, by receipt. Policy decides it, the way it decides a stuck order's steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutcomeReport {
    /// The request's id.
    pub request: Id,
    /// The order.
    pub order: Id,
    /// The receipt of the effect whose outcome was unknown.
    pub effect: Receipt,
    /// What the move came to.
    pub outcome: MoveOutcome,
    /// Who reports it: the move's own mover.
    pub reporter: Actor,
    /// The report's evidence: a verified stated entry about the move's order and part, by receipt.
    pub evidence: Receipt,
    /// When, by Core's clock.
    pub at: Timestamp,
}

/// The confirmation of a move's outcome, once unknown, on two actors' word (`REQ-AUZ-4`, ADR-0011): it cites its
/// mover's permitted report by receipt, and an actor of another accountable party and another login than the
/// reporter confirms it with its own evidence, a verified stated entry about the move's order and part, another
/// entry than the report's, by another maker. Policy decides it, the way it decides a stuck order's steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutcomeConfirmation {
    /// The request's id.
    pub request: Id,
    /// The order.
    pub order: Id,
    /// The receipt of the effect whose outcome was unknown.
    pub effect: Receipt,
    /// What the move came to: the report's outcome.
    pub outcome: MoveOutcome,
    /// The receipt of the report it confirms: a permitted `report_outcome` of the same effect and outcome.
    pub report: Receipt,
    /// Who confirms it.
    pub confirmer: Actor,
    /// The confirmation's evidence: a verified stated entry about the move's order and part, by receipt.
    pub confirmed_with: Receipt,
    /// When, by Core's clock.
    pub at: Timestamp,
}

/// A mover's report of a move's outcome declined, on another actor's word (`REQ-AUZ-4`, ADR-0011): it cites the report
/// by receipt, and an actor of another accountable party and another login than the reporter declines it with its
/// own evidence, a verified stated entry about the move's order and part, another entry than the report's, by another
/// maker. Policy decides it, the way it decides a confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutcomeDecline {
    /// The request's id.
    pub request: Id,
    /// The order.
    pub order: Id,
    /// The receipt of the effect whose outcome was unknown.
    pub effect: Receipt,
    /// The receipt of the report it declines: a permitted `report_outcome` of the same effect, still open.
    pub report: Receipt,
    /// Who declines it.
    pub decliner: Actor,
    /// The decline's evidence: a verified stated entry about the move's order and part, by receipt.
    pub declined_with: Receipt,
    /// When, by Core's clock.
    pub at: Timestamp,
}

/// A provider's showing of fault as an acceptance names it: the transaction its result is about, and the commitment
/// its stated entry names its bytes by, so the acceptance is of that showing alone. Schema:
/// `urn:rutent:core-internal:common#/$defs/showing_named`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShowingNamed {
    /// The transaction the showing, its provider's failed result, is about.
    pub transaction: Id,
    /// The commitment of the showing's stated entry.
    pub commitment: Digest,
}

/// Who carries a failed segment's reversal (`REQ-ORD-11`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Carrier {
    /// The provider that failed it: until a showing of another's fault is accepted.
    Provider,
    /// The customer, at fault by the provider's showing, accepted.
    Customer,
    /// The operator, at fault by the provider's showing, accepted.
    Operator,
}

/// What a move whose outcome was unknown came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MoveOutcome {
    /// It moved the value.
    Done,
    /// It didn't.
    Failed,
}

/// How what a verified entry records came out. Schema: `urn:rutent:core-internal:verified-entry#/$defs/outcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Outcome {
    /// Policy permitted it: a permit, an open session, a charge within its bound, an allowed question or read.
    Permitted {},
    /// Policy refused it.
    Refused {
        /// Why.
        class: RefusalClass,
    },
    /// Policy couldn't decide.
    NotKnown {
        /// Why.
        cause: NotKnownCause,
    },
    /// An effect, recorded before it takes place.
    ToTakeEffect {},
    /// It completed.
    Done {},
    /// It failed.
    Failed {},
    /// The request went unanswered.
    Unanswered {},
    /// Something rutent saw or did on its own: an item handed over, a comparison, a measure, a raise, a record.
    Observed {},
}

/// The kind of a verified entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifiedKind {
    /// Verified: what rutent itself did or saw.
    Verified,
}

/// What rutent itself did or saw: who, what, when by whose clock, and the outcome (`REQ-AUD-1`). Never a party's
/// statement, which is a [`StatedEntry`] (`REQ-AUD-2`). Schema: `urn:rutent:core-internal:verified-entry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct VerifiedEntry {
    /// Always `verified`.
    pub kind: VerifiedKind,
    /// Who acted.
    pub who: Actor,
    /// What.
    pub what: What,
    /// When, and by whose clock.
    pub when: When,
    /// The outcome: one the call can have.
    pub outcome: Outcome,
}

/// The kind of a stated entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatedKind {
    /// Stated: what a party said.
    Stated,
}

/// What a stated entry records: a statement of the shape its envelope names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatedWhat {
    /// The statement's shape.
    pub statement: ShapeId,
}

/// The outcome of a stated entry: it was stated, and nothing more is claimed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatedStatus {
    /// Stated.
    Stated,
}

/// The outcome of a stated entry. Schema: `urn:rutent:core-internal:stated-entry#/$defs/outcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatedOutcome {
    /// Always `stated`.
    pub status: StatedStatus,
}

/// Who a statement is attributed to: its maker where its signature verified against a key of the maker's, and
/// otherwise only the party it claims as maker, attributed to no one (`REQ-AUD-2`). A verified maker's `acts` is the
/// envelope's maker and its `party` the one that maker states for, both bound by the log; its `role` is the writer's
/// label, which no relation reads. Schema:
/// `urn:rutent:core-internal:stated-entry#/$defs/attribution`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "signature", rename_all = "snake_case", deny_unknown_fields)]
pub enum Attribution {
    /// The signature verified: the entry is the maker's.
    Verified {
        /// The maker.
        who: Actor,
    },
    /// It didn't, or couldn't be checked: the entry names only who the statement claims made it.
    Unverified {
        /// Who the statement claims made it.
        claimed_by: Actor,
    },
}

impl Attribution {
    /// The maker the statement names, attributed or claimed.
    pub fn maker(&self) -> &Actor {
        match self {
            Attribution::Verified { who } => who,
            Attribution::Unverified { claimed_by } => claimed_by,
        }
    }
}

/// The key a statement says signed it, and the scheme, without the signature's value, which stays beside the
/// statement's bytes in the evidence store. Schema: `urn:rutent:core-internal:stated-entry#/$defs/signed_with`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedWith {
    /// The signature scheme.
    pub scheme: SignatureScheme,
    /// The public key.
    pub key: PublicKey,
}

/// What a stated entry is about, where Core knows it: at least one of an order, a segment, a transaction or a private
/// node. Schema: `urn:rutent:core-internal:stated-entry#/$defs/about`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct About {
    /// The order.
    #[serde(
        default,
        deserialize_with = "some",
        skip_serializing_if = "Option::is_none"
    )]
    pub order: Option<Id>,
    /// The segment.
    #[serde(
        default,
        deserialize_with = "some",
        skip_serializing_if = "Option::is_none"
    )]
    pub segment: Option<Id>,
    /// The transaction.
    #[serde(
        default,
        deserialize_with = "some",
        skip_serializing_if = "Option::is_none"
    )]
    pub transaction: Option<Id>,
    /// The private node a publisher's record of the segments inside it is of (provider-role's `inner-segments`), as
    /// its body names it: named by that shape alone, so the log reads which node a record is of
    /// (`replay-cites-its-nodes-records`, `T-020-36`). [`attribute`](super::attribute) verifies it against the body.
    #[serde(
        default,
        deserialize_with = "some",
        skip_serializing_if = "Option::is_none"
    )]
    pub node: Option<Id>,
}

/// What a party stated, apart from anything rutent verified (`REQ-AUD-2`): who it is attributed to, its shape, the
/// time the maker gives it, the key it says signed it, and a salted commitment to the statement's bytes as received.
/// The bytes, the signature's value and the salt stay apart from the log, in the store
/// [`calls::Evidence`](super::calls::Evidence) answers for, so the signature stays checkable against exactly what
/// was signed, and nothing in the log lets anyone confirm a guess of a statement's content, such as a holding
/// account's balance (`REQ-AUD-6`). Its time is the maker's own, labelled so. Schema:
/// `urn:rutent:core-internal:stated-entry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct StatedEntry {
    /// Always `stated`.
    pub kind: StatedKind,
    /// Who it is attributed to, by whether its signature verified.
    pub attribution: Attribution,
    /// The statement's shape.
    pub what: StatedWhat,
    /// When the maker says it made it, its envelope's `made_at`.
    pub when: StatedWhen,
    /// When Core received it, by Core's clock.
    pub received_at: Timestamp,
    /// Always `stated`.
    pub outcome: StatedOutcome,
    /// The key and scheme the statement says signed it.
    pub signed_with: SignedWith,
    /// The salted commitment to the statement's bytes as received, which [`calls::Evidence::keep`] answered.
    ///
    /// [`calls::Evidence::keep`]: super::calls::Evidence::keep
    pub commitment: Digest,
    /// What it is about, where Core knows it.
    #[serde(
        default,
        deserialize_with = "some",
        skip_serializing_if = "Option::is_none"
    )]
    pub about: Option<About>,
    /// Who passed it on, where someone other than its maker did: a restatement stays the maker's, and who passed
    /// it on is recorded apart (`REQ-AUD-2`).
    #[serde(
        default,
        deserialize_with = "some",
        skip_serializing_if = "Option::is_none"
    )]
    pub relayed_by: Option<Actor>,
}

/// A statement as the evidence store keeps it, for a party to keep its own evidence (`REQ-AUD-5`) and for a checker
/// outside the operator to check the log against (`REQ-AUD-9`, `REQ-AUD-10`, `T-022-04`). Schema:
/// `urn:rutent:core-internal:kept-statement`.
///
/// To check it against the stated entry that names it:
///
/// 1. Recompute SHA-256 of the decoded salt followed by the decoded bytes, and compare it with the entry's
///    `commitment` (and this shape's).
/// 2. Read the bytes as the statement's envelope, check that its signature's scheme and key equal the entry's
///    `signed_with`, and verify the signature over provider-role's `signing_input` of the bytes: the RFC 8785
///    canonical JSON of the envelope as received, without `signature.value`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeptStatement {
    /// The commitment the stated entry names it by.
    pub commitment: Digest,
    /// The salt it was committed with.
    pub salt: Salt,
    /// The statement's bytes exactly as received.
    pub bytes: Encoded,
}

/// An entry of the log: a party's statement, or what rutent itself did or saw (`REQ-AUD-1`, `REQ-AUD-2`). Schema:
/// `urn:rutent:core-internal:entry`, with its `kind` choosing between the two shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// What a party stated.
    Stated(StatedEntry),
    /// What rutent did or saw.
    Verified(VerifiedEntry),
}

impl Serialize for Entry {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Entry::Stated(entry) => entry.serialize(serializer),
            Entry::Verified(entry) => entry.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Entry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let value = Value::deserialize(deserializer)?;
        let kind = value
            .get("kind")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        match kind.as_deref() {
            Some("stated") => serde_json::from_value(value)
                .map(Entry::Stated)
                .map_err(D::Error::custom),
            Some("verified") => serde_json::from_value(value)
                .map(Entry::Verified)
                .map_err(D::Error::custom),
            _ => Err(D::Error::custom("an entry's kind is stated or verified")),
        }
    }
}

// --- Checkpoints (ADR-0005) ------------------------------------------------------------------------------------

/// A checkpoint's `shape`.
pub const CHECKPOINT_SHAPE: &str = "urn:rutent:core-internal:checkpoint";

/// The shape of the operator's statement of an order's inputs.
pub const ORDER_INPUTS_SHAPE: &str = "urn:rutent:core-internal:order-inputs";

/// The inputs an order carries for the tuples that require them, as the operator states them when intake opens the
/// order (`T-020-46`): the body of a statement of shape [`ORDER_INPUTS_SHAPE`], kept in the evidence store behind a
/// salted commitment that `order_opened` names, so a Core that reopens reads them back and the log holds no account
/// (`REQ-AUD-6`). It names no transaction, and is appended about nothing: the order it is of is the body's, which
/// only whoever holds the kept bytes reads. Schema: `urn:rutent:core-internal:order-inputs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct OrderInputs {
    /// The order.
    pub order: Id,
    /// Each input, by name: never empty, since an order with no input states none.
    pub inputs: alloc::collections::BTreeMap<String, String>,
}

/// The shape of the operator's statement of a filing's inputs and objective.
pub const FILING_INPUTS_SHAPE: &str = "urn:rutent:core-internal:filing-inputs";

/// The inputs and objective an order filed on an automatic deposit carries, as the operator states them when the
/// filing holds the deposit against its order (`T-020-51`): the body of a statement of shape [`FILING_INPUTS_SHAPE`],
/// kept in the evidence store behind a salted commitment that the `arrival` holding the deposit names, so a Core that
/// reopens reads them back and the log holds no account (`REQ-AUD-6`). It names no transaction and is appended about
/// nothing. Unlike an order's, its inputs may be empty: the objective is always stated. Schema:
/// `urn:rutent:core-internal:filing-inputs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilingInputs {
    /// The order.
    pub order: Id,
    /// Each input, by name; empty where the filing carries none.
    pub inputs: alloc::collections::BTreeMap<String, String>,
    /// What the router optimises the order's route for.
    pub objective: crate::router_api::Objective,
    /// The judgements the route's tuples need of a person, as the filing named them; absent where it names none. The
    /// kept statement fixes them with the rest of the filing, so a Core that reopens asks the route as it was asked.
    #[serde(default, skip_serializing_if = "alloc::vec::Vec::is_empty")]
    pub needs: alloc::vec::Vec<FilingNeed>,
}

/// A judgement a tuple's provider needs before it may be on a filed order's route: of which verifier, about which
/// person and class, under the relying provider's subject reference (`REQ-CPL-6`, `T-020-51`). Schema:
/// `urn:rutent:core-internal:filing-inputs#/$defs/need`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilingNeed {
    /// The relying provider's tuple.
    pub tuple: crate::router_api::TupleKey,
    /// The person, by reference.
    pub person: Id,
    /// The verifier the provider's rules name.
    pub verifier: Id,
    /// The person's subject reference, the relying provider's.
    pub subject: String,
    /// The class.
    pub class: String,
}

/// A checkpoint opening's `shape`.
pub const CHECKPOINT_OPENING_SHAPE: &str = "urn:rutent:core-internal:checkpoint-opening";

checked_text!(
    /// A checkpoint's tick on its schedule, as 16 lower-case hexadecimal digits, so a checkpoint's size never tells how
    /// far along the schedule it is. Schema: `urn:rutent:core-internal:checkpoint#/$defs/tick`.
    Tick,
    |t| t.len() == 16 && t.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
    "a tick is 16 lower-case hexadecimal digits"
);

checked_text!(
    /// A 64-byte Ed25519 signature, base64url without padding, in its canonical form. Schema:
    /// `urn:rutent:core-internal:checkpoint#/$defs/signature/properties/value`.
    SignatureValue,
    |t| t.len() == 86 && is_base64url(t),
    "a signature is 64 bytes, base64url without padding, in its canonical form"
);

/// The one shape a checkpoint has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CheckpointShape {
    /// [`CHECKPOINT_SHAPE`].
    #[serde(rename = "urn:rutent:core-internal:checkpoint")]
    Checkpoint,
}

/// The one shape a checkpoint's opening has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CheckpointOpeningShape {
    /// [`CHECKPOINT_OPENING_SHAPE`].
    #[serde(rename = "urn:rutent:core-internal:checkpoint-opening")]
    CheckpointOpening,
}

/// The log's signature on a checkpoint. Schema: `urn:rutent:core-internal:checkpoint#/$defs/signature`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointSignature {
    /// Always Ed25519, verified strictly.
    pub scheme: SignatureScheme,
    /// The log's own key, never the operator's statement key.
    pub key: PublicKey,
    /// The signature over the RFC 8785 canonical JSON of the checkpoint without `signature.value`.
    pub value: SignatureValue,
}

/// What the operator publishes for one tick of a schedule fixed in advance, whether or not the log moved: a short,
/// fixed-size, signed commitment to the log's history and nothing else, no size, no root, no count and no salt
/// (`REQ-AUD-14`, ADR-0005). Schema: `urn:rutent:core-internal:checkpoint`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    /// Always [`CHECKPOINT_SHAPE`].
    pub shape: CheckpointShape,
    /// The log.
    pub log: Id,
    /// The tick.
    pub tick: Tick,
    /// When the tick fell on the schedule, never the time of writing.
    pub at: Timestamp,
    /// The commitment: SHA-256 over a domain, the log, the tick, a fresh salt, the log's size and its root, as
    /// `core-audit`'s checkpointer computes it; only an opening's holder can open it.
    pub commitment: Digest,
    /// The log's signature.
    pub signature: CheckpointSignature,
}

/// What a checkpoint commits to, beside its root, and the salt: kept in `CMP-CORE-AUDIT`'s own checkpoint store,
/// outside the log, and handed to the checker with the history, never published (`REQ-AUD-14`, `REQ-AUD-10`). Schema:
/// `urn:rutent:core-internal:checkpoint-opening`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointOpening {
    /// Always [`CHECKPOINT_OPENING_SHAPE`].
    pub shape: CheckpointOpeningShape,
    /// The log.
    pub log: Id,
    /// The tick, as its checkpoint has it.
    pub tick: Tick,
    /// The salt, 32 fresh bytes.
    pub salt: Salt,
    /// The number of entries the log held, whose root the checkpoint commits to.
    pub size: SafeInteger,
}
