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
    Denomination, EvidenceGrade, OuterNode, PublicKey, SafeInteger, SignatureScheme,
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
    /// Granting a provider a tuple.
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
}

impl CatalogueChange {
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
        /// The tuple it is about: present exactly when the change is about a tuple.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        tuple: Option<Id>,
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
#[serde(deny_unknown_fields)]
pub struct Authorisation {
    /// The authorisation's id.
    pub authorisation: Id,
    /// The one order it is for.
    pub order: Id,
    /// The order's origin reserve: where the user authorised, or the holding account's provider on a route handed
    /// on (`REQ-AUZ-9`).
    pub origin_reserve: Id,
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
    /// Who decides.
    pub decider: Actor,
    /// The outcome decided.
    pub resolution: Resolution,
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
}

impl ChargeOutcome {
    /// The outcome of the entry that records this answer: within the bound is permitted, over it refused as
    /// `over_bound`.
    pub fn outcome(self) -> Outcome {
        match self {
            ChargeOutcome::Within { .. } => Outcome::Permitted {},
            ChargeOutcome::Over { .. } => Outcome::Refused {
                class: RefusalClass::OverBound,
            },
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
    /// A route the router shortlisted for the order: the digest of the shortlist round's recorded decision, and the
    /// providers it shortlisted, so policy can hold who is asked to them (`REQ-CPL-6`).
    ShortlistedRoute {
        /// The shortlist.
        shortlist: Digest,
        /// The shortlisted providers, at least one, each once.
        providers: Vec<Id>,
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

/// One party's report about a segment: how much of which asset it says it sent or received, and the receipt of the
/// stated entry that holds its statement. Schema: `urn:rutent:core-internal:report-pair#/$defs/report`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    /// The party that reports.
    pub party: Id,
    /// The amount, in the asset's unit.
    pub amount: Amount,
    /// The asset.
    pub asset: Denomination,
    /// The receipt of the stated entry that holds the party's statement.
    pub statement: Receipt,
}

/// A segment's pair of reports: what one party says it sent, and what the next says it received (`REQ-ORD-9`).
/// Schema: `urn:rutent:core-internal:report-pair`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct ReportPair {
    /// The order.
    pub order: Id,
    /// The segment.
    pub segment: Id,
    /// What the sending party reports.
    pub sent: Report,
    /// What the receiving party reports.
    pub received: Report,
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

/// One record the bulk measures read, per provider or verifier (`REQ-AUD-7`): rutent's record of what it measured,
/// citing the entries, stated or verified, it rests on. Schema: `urn:rutent:core-internal:measure-record`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct MeasureRecord {
    /// The provider or verifier it measures.
    pub provider: Id,
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

/// An operator command that needs no policy decision: none moves value, posts a route, changes the catalogue or grants
/// a permission (`REQ-AUZ-1`, `REQ-AUZ-6`, `REQ-AUZ-7`). A closed list. Filing an arrival anew is a `refiling`, and a
/// read permission goes through policy (`Reads::grant_permission`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperatorCommand {
    /// Appoint the checker outside the operator (`REQ-AUD-10`).
    AppointChecker,
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
        /// The receipt of the origin reserve's stated yes to the route: present exactly when its basis is given; an
        /// automatic deposit carries none (`REQ-ORD-32`).
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        reserve_yes: Option<Receipt>,
        /// The user's session it was asked in: present exactly when its basis is given (`REQ-ORD-32`).
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        session: Option<Id>,
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
    },
    /// What was shown to the customer, by digest, and how (`REQ-QTE-2`, `REQ-QTE-7`, `REQ-QTE-10`).
    Shown {
        /// The order.
        order: Id,
        /// The digest of what was shown.
        shown: Digest,
        /// How it was shown.
        shown_as: ShownAs,
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
        /// Whether it was billed separately, outside the amounts reported.
        billed_separately: bool,
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

/// What a stated entry is about, where Core knows it: at least one of an order, a segment or a transaction. Schema:
/// `urn:rutent:core-internal:stated-entry#/$defs/about`.
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
