//! The reader contract's shapes, each mirroring its schema under `contracts/reader/schemas/`.

use alloc::{format, string::String, vec::Vec};
use core::fmt;

use serde::{Deserialize, Deserializer, Serialize};

use super::Violation;
use crate::provider_role::{NoDetail, StatementShape};
use crate::router_api::{Amount, Id, Timestamp, Version};

/// The `shape` of a reader's answer: the `$id` of `contracts/reader/schemas/answer.schema.json`.
pub const ANSWER_SHAPE: &str = "urn:rutent:reader:answer";

/// Deserialises an optional field that, when present, holds a value: `null` is refused, as the schemas refuse it.
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

fn is_lower_hex(text: &str) -> bool {
    text.bytes()
        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

checked_text!(
    /// A question's request id: 128 bits Core draws at random for each question, never from a counter, as 32
    /// lower-case hexadecimal characters (`DEC-28`'s condition (b)). Its top 64 bits are never all zero, which is what
    /// a counter written as an id would give: so a host can't compute an answer before it is asked. Schema:
    /// `urn:rutent:reader:common#/$defs/request_id`.
    RequestId,
    |t| t.len() == 32 && is_lower_hex(t) && !t.starts_with("0000000000000000"),
    "a request id is 32 lower-case hexadecimal characters, 128 bits drawn at random, its top 64 bits not all zero"
);

impl RequestId {
    /// The id for 16 bytes Core drew from its random source for one question. `None` where the top 8 bytes are all
    /// zero, as a counter's would be: Core then draws again (the chance of it from a random source is 2^-64).
    pub fn from_random(bytes: [u8; 16]) -> Option<Self> {
        let text: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        Self::new(text)
    }
}

checked_text!(
    /// A Substrate network's genesis hash: `0x` and 64 lower-case hexadecimal characters. Schema:
    /// `urn:rutent:reader:common#/$defs/genesis_hash`.
    GenesisHash,
    |t| t.len() == 66 && t.starts_with("0x") && is_lower_hex(&t[2..]),
    "a genesis hash is 0x and 64 lower-case hexadecimal characters"
);

checked_text!(
    /// An EVM network's chain id (EIP-155), in decimal digits with no leading zero, above zero, up to 20 digits.
    /// Schema: `urn:rutent:reader:common#/$defs/chain_id`.
    ChainId,
    |t| {
        (1..=20).contains(&t.len())
            && t.bytes().all(|c| c.is_ascii_digit())
            && !t.starts_with('0')
    },
    "a chain id is 1 to 20 decimal digits with no leading zero"
);

checked_text!(
    /// Why a reader can't stand behind an answer, as a token: a lower-case word or words joined by `_`, up to 64
    /// characters. A label to read, which no decision branches on. Schema: `urn:rutent:reader:common#/$defs/reason`.
    Reason,
    |t| {
        t.len() <= 64
            && t.bytes().next().is_some_and(|c| c.is_ascii_lowercase())
            && t.bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
    },
    "a reason is 1 to 64 of a-z 0-9 _, starting with a letter"
);

/// True when `host` names one host by a DNS name: lower-case LDH labels, each 1 to 63 characters, neither starting nor
/// ending with a hyphen, at least two of them, up to 253 characters, with no trailing dot and no wildcard, and a last
/// label that starts with a letter, so it is never an IP address's literal, decimal or hexadecimal (`0x7f.0x1`), which a
/// resolver would read as an address (`T-020-59`'s review, L1 and L8). The reader's RPC endpoints and core-internal's
/// subject names are held to it. Names a resolver answers locally, such as `.local`, are the operator's to avoid.
pub fn is_host_name(host: &str) -> bool {
    let labels: Vec<&str> = host.split('.').collect();
    host.len() <= 253
        && labels.len() >= 2
        && labels.iter().all(|label| {
            (1..=63).contains(&label.len())
                && label
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        })
        && labels
            .last()
            .is_some_and(|last| last.bytes().next().is_some_and(|c| c.is_ascii_lowercase()))
}

/// The host and the port an endpoint names: `None` where it isn't an endpoint.
fn endpoint_parts(text: &str) -> Option<(&str, Option<&str>)> {
    let rest = text
        .strip_prefix("https://")
        .or_else(|| text.strip_prefix("wss://"))?;
    // No path beyond `/`: a node's key can sit in a path segment, and the contract can't tell one from a route, so
    // a provider that needs a path or a key reaches it through the egress layer's secret lookup, never the listing.
    let authority = rest.strip_suffix('/').unwrap_or(rest);
    let (host, port) = match authority.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    let port_ok = port.is_none_or(|p| {
        !p.is_empty()
            && p.len() <= 5
            && p.bytes().all(|c| c.is_ascii_digit())
            && !p.starts_with('0')
            && p.parse::<u32>().is_ok_and(|n| n <= 65_535)
    });
    (text.len() <= 256 && is_host_name(host) && port_ok).then_some((host, port))
}

fn is_rpc_endpoint(text: &str) -> bool {
    endpoint_parts(text).is_some()
}

checked_text!(
    /// One endpoint of an RPC pool: an `https` or `wss` URL to a host's DNS name (never an IP literal), with an
    /// optional port and at most a trailing `/`: no user, query, fragment or path, so nothing that could be a key is
    /// written in a listing (`T-020-59`'s review, L2). The reader host's egress reaches it by its name; a key or a path
    /// a node needs is resolved by name by the host through the egress layer's secret lookup (`REQ-MDL-6` applied to
    /// readers). Schema: `urn:rutent:reader:common#/$defs/rpc_endpoint`.
    RpcEndpoint,
    is_rpc_endpoint,
    "an rpc endpoint is an https or wss URL to a lower-case DNS host name, with an optional port and no user, path, query or fragment"
);

impl RpcEndpoint {
    /// The host it names: what a pool's quorum counts, whatever the scheme, port or trailing `/` (`T-020-59`'s review,
    /// M2).
    pub fn host(&self) -> &str {
        endpoint_parts(&self.0).map_or("", |(host, _)| host)
    }
}

/// A count of blocks: at least one. Schema: `urn:rutent:reader:common#/$defs/depth`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct Depth(u32);

impl Depth {
    /// The depth, or `None` for zero.
    pub fn new(blocks: u32) -> Option<Self> {
        (blocks > 0).then_some(Depth(blocks))
    }

    /// The number of blocks.
    pub fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<u32> for Depth {
    type Error = Violation;

    fn try_from(blocks: u32) -> Result<Self, Self::Error> {
        Depth::new(blocks).ok_or(Violation("a depth is at least one block"))
    }
}

impl From<Depth> for u32 {
    fn from(value: Depth) -> Self {
        value.0
    }
}

/// A network's identity, which the reader checks before it answers anything and Core holds to the listing (`DEC-28`).
/// Schema: `urn:rutent:reader:common#/$defs/network_identity`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NetworkIdentity {
    /// A Substrate network, by its genesis hash.
    GenesisHash {
        /// The hash.
        hash: GenesisHash,
    },
    /// An EVM network, by its chain id.
    ChainId {
        /// The chain id.
        id: ChainId,
    },
}

/// When a movement on the network counts as final (`DEC-13`), as `CMP-EVIDENCE`'s rules state it. Schema:
/// `urn:rutent:reader:common#/$defs/finality`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "finality", rename_all = "snake_case", deny_unknown_fields)]
pub enum Finality {
    /// Final as soon as it is recorded.
    Instant(NoDetail),
    /// Final after this many confirmations.
    Confirmations {
        /// The confirmations.
        count: Depth,
    },
    /// Final once a later checkpoint, such as a finality gadget's, covers it.
    Checkpoint(NoDetail),
}

/// What the network can do to a movement that is not yet final (`DEC-13`). Schema:
/// `urn:rutent:reader:common#/$defs/reorg_rule`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reorg", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReorgRule {
    /// A recorded movement is never undone.
    Never(NoDetail),
    /// A movement can be undone to this depth; beyond it, it is final.
    DepthBound {
        /// The depth.
        depth: Depth,
    },
    /// A movement can be undone until the finality rule says otherwise.
    UntilFinal(NoDetail),
}

/// How a reader stands behind what it answers (item `reader-answer-trust`, `DEC-28`): a light client, or a preset pool
/// of RPC endpoints of which at least the quorum must agree. Schema: `urn:rutent:reader:common#/$defs/trust_rule`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    remote = "Self",
    tag = "rule",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum TrustRule {
    /// An embedded light client, anchored on the network's genesis and answering only on finalised state: a
    /// Substrate network's, such as Kreivo's.
    LightClient(NoDetail),
    /// A preset pool of RPC endpoints, answered only where at least `quorum` of them agree: an EVM network's.
    RpcPool {
        /// The pool's endpoints, each once, at least as many as the quorum.
        endpoints: Vec<RpcEndpoint>,
        /// How many of them must agree: at least two.
        quorum: u32,
    },
}

/// The fewest endpoints of a pool that must agree: two (item `reader-answer-trust`).
pub const LEAST_QUORUM: u32 = 2;

impl TrustRule {
    /// Checks the rules of a trust rule the schema can't state, and the ones it does: a pool's quorum is at least
    /// [`LEAST_QUORUM`], its endpoints are distinct, and it has at least as many as its quorum.
    pub fn check(&self) -> Result<(), Violation> {
        // Each refusal through the contracts' rule switch, so a contract test sees an invalid listing break one rule
        // alone (`T-020-45`; the check of `T-020-59`'s invalid fixtures, routed to `T-020-60`).
        let broke = |rule: &'static str| {
            if crate::rule_switch::switched_off(rule) {
                Ok(())
            } else {
                Err(Violation(rule))
            }
        };
        if let TrustRule::RpcPool { endpoints, quorum } = self {
            if *quorum < LEAST_QUORUM {
                broke("an rpc pool's quorum is at least two")?;
            }
            if endpoints
                .iter()
                .enumerate()
                .any(|(i, e)| endpoints[..i].contains(e))
            {
                broke("an rpc pool names each endpoint once")?;
            }
            // A quorum is of nodes, not of spellings: two endpoints on one host, by another scheme, port or trailing
            // `/`, are one node agreeing with itself (`T-020-59`'s review, M2). Separate names that resolve to one
            // provider are beyond the contract.
            if endpoints
                .iter()
                .enumerate()
                .any(|(i, e)| endpoints[..i].iter().any(|o| o.host() == e.host()))
            {
                broke("an rpc pool names each host once")?;
            }
            if (endpoints.len() as u64) < u64::from(*quorum) {
                broke("an rpc pool has at least as many endpoints as its quorum")?;
            }
        }
        Ok(())
    }
}

/// A question Core asks a reader, by kind, with what it is about. Each is read-only. Schema:
/// `urn:rutent:reader:common#/$defs/asked`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "question", rename_all = "snake_case", deny_unknown_fields)]
pub enum Asked {
    /// The network's identity, its finality and reorg rules, the reader's version and its trust rule: what Core's
    /// start-up check compares with the listing.
    NetworkRules(NoDetail),
    /// Where a movement stands, by its id on the network.
    MovementStatus {
        /// The movement asked about.
        movement: Id,
    },
    /// The movement the network ties to a segment's transaction, by the network's own matching.
    MovementOf {
        /// The segment's transaction.
        transaction: Id,
    },
}

impl Asked {
    /// The question's kind, as its `question` names it.
    pub fn kind(&self) -> &'static str {
        match self {
            Asked::NetworkRules(_) => "network_rules",
            Asked::MovementStatus { .. } => "movement_status",
            Asked::MovementOf { .. } => "movement_of",
        }
    }
}

/// A question from Core to a reader host (`DEC-28`): which reader it asks about which network, what it asks, under a
/// request id Core drew at random, and the deadline its answer must meet on Core's clock. It travels over the mTLS
/// link, whose client certificate the operator's CA issued for Core, and is not a signed statement: only Core asks.
/// Schema: `urn:rutent:reader:question`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReaderQuestion {
    /// The request id, drawn at random for this question alone ([`RequestId`]).
    pub request: RequestId,
    /// The reader asked, by its name on its host: the answer's maker.
    pub reader: Id,
    /// The network asked about, as an outer node names it.
    pub network: Id,
    /// The last instant, on Core's clock, at which Core takes its answer. The host answers under it and never draws
    /// its own; Core never reads the host's clock for it.
    pub deadline: Timestamp,
    /// What is asked.
    pub asked: Asked,
}

/// A network's rules, as its reader answers them. Schema: `urn:rutent:reader:answer#/$defs/network_rules`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkRules {
    /// The network's identity.
    pub identity: NetworkIdentity,
    /// Its finality definition.
    pub finality: Finality,
    /// Its reorg rule.
    pub reorg: ReorgRule,
    /// The reader's trust rule.
    pub trust: TrustRule,
}

impl NetworkRules {
    /// Checks the rules of a network's rules the schema states too: its identity and its trust rule are paired
    /// ([`check_pairing`]), and the trust rule holds to its own rules.
    pub fn check(&self) -> Result<(), Violation> {
        check_pairing(&self.identity, &self.trust)?;
        self.trust.check()
    }
}

/// Holds a network's identity to its trust rule (item `reader-answer-trust`, `T-020-59`'s review, M1): an EVM network,
/// by its chain id, is read from a pool of RPC endpoints with a quorum, and a Substrate network, by its genesis hash,
/// by a light client. Anything else is refused: an EVM chain read by a "light client" would skip the quorum the owner
/// required, and its reader's own code, which is not trusted, would hold it instead of the reader host.
pub fn check_pairing(identity: &NetworkIdentity, trust: &TrustRule) -> Result<(), Violation> {
    match (identity, trust) {
        (NetworkIdentity::ChainId { .. }, TrustRule::RpcPool { .. })
        | (NetworkIdentity::GenesisHash { .. }, TrustRule::LightClient(_)) => Ok(()),
        _ => Err(Violation(
            "a chain id is read from an rpc pool, and a genesis hash by a light client",
        )),
    }
}

/// A movement as the network records it. Schema: `urn:rutent:reader:common#/$defs/movement`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Movement {
    /// Where the value went.
    pub destination: Id,
    /// The asset.
    pub asset: Id,
    /// The amount, in the asset's unit on the network.
    pub amount: Amount,
}

/// Where a movement stands now, by `status`. Schema: `urn:rutent:reader:answer#/$defs/standing`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Standing {
    /// No such movement.
    Unknown(NoDetail),
    /// Recorded, not yet final.
    Pending(Movement),
    /// Recorded and final under the network's rules.
    Final(Movement),
    /// Recorded once, since undone by a reorg.
    ReorgedOut(NoDetail),
}

/// The movement a network ties to a transaction. Schema: `urn:rutent:reader:answer#/$defs/tied`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tied {
    /// The movement's id on the network.
    pub id: Id,
    /// Where it went.
    pub destination: Id,
}

/// What a reader answers, by `answer`: the kind of the question asked, or that it can't stand behind an answer.
/// Schema: `urn:rutent:reader:answer#/$defs/answered`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case", deny_unknown_fields)]
pub enum Answered {
    /// The network's rules, to a `network_rules` question.
    NetworkRules {
        /// The rules.
        rules: NetworkRules,
    },
    /// Where the movement asked about stands, to a `movement_status` question.
    MovementStatus {
        /// Where it stands.
        standing: Standing,
    },
    /// The movement tied to the transaction asked about, to a `movement_of` question: absent where it ties none.
    MovementOf {
        /// The movement, where there is one.
        #[serde(
            default,
            deserialize_with = "some",
            skip_serializing_if = "Option::is_none"
        )]
        movement: Option<Tied>,
    },
    /// The reader can't stand behind an answer: its trust rule is not met, its start-up checks have not passed, or it
    /// failed. Settles nothing and grades nothing down (ADR-0007 as amended). The host gives it, for any question, for
    /// a reader that crashed, hung or hit a limit.
    Unreadable {
        /// Why, as a label.
        why: Reason,
    },
}

impl Answered {
    /// The kind of question it answers: `None` for an unreadable answer, which answers any.
    pub fn kind(&self) -> Option<&'static str> {
        match self {
            Answered::NetworkRules { .. } => Some("network_rules"),
            Answered::MovementStatus { .. } => Some("movement_status"),
            Answered::MovementOf { .. } => Some("movement_of"),
            Answered::Unreadable { .. } => None,
        }
    }
}

/// A reader's answer, the statement in the contracts' envelope (`urn:rutent:provider-role:statement-envelope`) whose
/// maker is the reader, by its name on its host, signed with the reader's statement key, which the reader host holds
/// for it (`DEC-28`). It names the request and what was asked, the network and the reader's version, so
/// [`check_reader_answer`](super::check_reader_answer) can hold it to the question Core sent. Schema:
/// `urn:rutent:reader:answer#/$defs/answer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(remote = "Self", deny_unknown_fields)]
pub struct ReaderAnswer {
    /// The request it answers.
    pub request: RequestId,
    /// The network it is about.
    pub network: Id,
    /// The reader's version, which Core holds to the listing's (`REQ-AUD-4`).
    pub version: Version,
    /// The question it answers, as asked.
    pub asked: Asked,
    /// The answer.
    pub answer: Answered,
}

impl ReaderAnswer {
    /// Checks the rules of an answer the schema can't state: it answers the kind of question it names, or is
    /// unreadable; and the trust rule it carries holds to its own rules.
    pub fn check(&self) -> Result<(), Violation> {
        if self
            .answer
            .kind()
            .is_some_and(|kind| kind != self.asked.kind())
        {
            return Err(Violation(
                "an answer answers the kind of question it names, or is unreadable",
            ));
        }
        if let Answered::NetworkRules { rules } = &self.answer {
            rules.check()?;
        }
        Ok(())
    }
}

impl StatementShape for ReaderAnswer {
    const SHAPE: &'static str = ANSWER_SHAPE;
}

macro_rules! checked_serde {
    ($($name:ident),*) => {$(
        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                $name::serialize(self, serializer)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                use serde::de::Error as _;
                let value = $name::deserialize(deserializer)?;
                value.check().map_err(D::Error::custom)?;
                Ok(value)
            }
        }
    )*};
}

checked_serde!(TrustRule, ReaderAnswer);
