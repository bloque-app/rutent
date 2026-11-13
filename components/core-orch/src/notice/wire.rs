//! A notice as it travels: what it says, whom it is for, and the signature that binds the two (`REQ-ORD-20`).
//!
//! A sealed notice is the JSON object `{"notice": ..., "signature": "<128 lower-case hex digits>"}`. The signature is
//! Ed25519, by the deployment's notice key, over [`signing_input`]: [`DOMAIN`] followed by the RFC 8785 canonical
//! form of the `notice` member. The domain starts with a byte no canonical JSON object starts with, so a notice's
//! signature never verifies as a provider-role statement's (whose signing input is a canonical object), even where
//! the assembly signs both with the operator's key.
//!
//! What is signed names the deployment, the receiver (its party and the role it receives in), and what the notice is
//! about: an order's change, at its sequence, or an event for the operator alone. So a notice taken by anyone but
//! its receiver, or at another deployment, fails its check ([`super::Inbox::take`]).

use rutent_contracts::core_internal::{Afterwards, OrderRefusalClass, Raise, Role, Work};
use rutent_contracts::provider_role::to_canonical;
use rutent_contracts::router_api::{Id, Timestamp};
use rutent_contracts::state_machine as stm;
use rutent_contracts::state_machine::{OrderState, StuckReason};
use serde::{Deserialize, Serialize};

/// What every notice's signing input starts with: no RFC 8785 canonical object starts with `r`.
pub const DOMAIN: &[u8] = b"rutent-notice/1\n";

/// Whom a notice is for: a party, in the role it receives in. An order's customer receives as `customer`, the
/// deployment's operator as `operator` (`REQ-ORD-20`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receiver {
    /// The party.
    pub party: Id,
    /// The role it receives in.
    pub role: Role,
}

/// One part's state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartState {
    /// The part.
    pub part: stm::Id,
    /// Its state.
    pub state: OrderState,
}

/// An order's state, as `Reads::OrderState` reads it (`REQ-ORD-5`): the order's state, each part's, and when it
/// reached them. A notice adds what changed ([`Change`]): the change's kind, the part, a stuck part's reason, the
/// linked order of a hand-on, a refusal's class, an `afterwards` event; all of the customer's own order. Nothing
/// else: no provider, segment, amount, account or provider's text (`REQ-AUZ-7`, `REQ-ORD-18`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    /// The order's state.
    pub state: OrderState,
    /// Each part's state, in the route's order.
    pub parts: Vec<PartState>,
    /// When the order reached it: the `recorded_at` of the input that last moved it.
    pub since: stm::Timestamp,
}

/// When a change was recorded, as its record holds it, unconverted: an input's `recorded_at`, in the
/// state-machine contract's grammar (an offset from UTC allowed, any fraction of a second), or an entry's `when.at`,
/// in core-internal's (UTC, at most nine digits of a second). Nothing is converted, so no change is ever dropped
/// for a time that wouldn't convert.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct At(String);

impl At {
    /// The time as its record holds it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&stm::Timestamp> for At {
    fn from(at: &stm::Timestamp) -> Self {
        At(at.as_str().to_owned())
    }
}

impl From<&Timestamp> for At {
    fn from(at: &Timestamp) -> Self {
        At(at.as_str().to_owned())
    }
}

impl TryFrom<String> for At {
    type Error = &'static str;
    fn try_from(text: String) -> Result<Self, Self::Error> {
        if stm::Timestamp::new(text.as_str()).is_some() || Timestamp::new(text.as_str()).is_some() {
            Ok(At(text))
        } else {
            Err("a notice's time is a recorded input's or a log entry's")
        }
    }
}

impl From<At> for String {
    fn from(at: At) -> Self {
        at.0
    }
}

/// What changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change {
    /// An input the state machine applied: its kind, as the state-machine contract names it, the part it is about
    /// where it is about one, a stuck part's reason, and the order a part was handed on to.
    Input {
        /// The input's kind (`route_chosen`, `segment_failed`, `stuck_declared`, ...).
        input: String,
        /// The part it moved, where it is about one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        part: Option<stm::Id>,
        /// Why a part was declared stuck.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<StuckReason>,
        /// The linked order a part was handed on to (`handed_on`, `REQ-ORD-43`): the customer's own, which it
        /// follows from there.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        linked_order: Option<stm::Id>,
    },
    /// Something that came after the order ended, which leaves its state as it was (`REQ-ORD-31`, `REQ-ORD-25`).
    Afterwards {
        /// What came.
        event: Afterwards,
    },
    /// The order can't proceed, of this class: the class its caller is told (`REQ-ORD-18`).
    Refused {
        /// The class.
        class: OrderRefusalClass,
    },
}

/// A charge counted more than a window ahead of Core's now: what the operator sees of it (`T-021-05`'s hand-off).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AheadSeen {
    /// The party charged.
    pub party: Id,
    /// The charge's request id.
    pub request: Id,
    /// Its kind of work.
    pub work: Work,
    /// How much of it.
    pub count: u64,
    /// The moment it was counted at.
    pub counted_at: Timestamp,
    /// Core's now when it was noticed.
    pub now: Timestamp,
}

/// What a notice is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "about", rename_all = "snake_case", deny_unknown_fields)]
pub enum About {
    /// A change of an order: the `sequence`-th of its changes, counted from 1 in the log's order, with the order's
    /// state after it. A later sequence is a later state.
    Order {
        /// The order.
        order: stm::Id,
        /// The change's place among the order's changes.
        sequence: u64,
        /// When it was recorded.
        at: At,
        /// What changed.
        change: Change,
        /// The order's state after it.
        view: View,
    },
    /// A difference raised to the operator, and nothing that moves value (ADR-0009). The operator's alone.
    Raise {
        /// The raise.
        raise: Raise,
    },
    /// A charge counted far ahead of Core's clock. The operator's alone.
    Ahead {
        /// The charge.
        charge: AheadSeen,
    },
}

impl About {
    /// The order a notice is about, where it is about one.
    pub fn order(&self) -> Option<&str> {
        match self {
            About::Order { order, .. } => Some(order.as_str()),
            About::Raise { .. } | About::Ahead { .. } => None,
        }
    }
}

/// A notice: the deployment it comes from, its receiver, and what it is about. All of it is signed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Notice {
    /// The deployment.
    pub deployment: Id,
    /// Its receiver.
    pub to: Receiver,
    /// What it is about.
    pub about: About,
    /// Whether it answers a request for the order's current state: then every change of the order up to its
    /// sequence is in it, and a receiver missing one below it misses nothing ([`super::Inbox::missing`]).
    #[serde(default, skip_serializing_if = "is_false")]
    pub current: bool,
}

fn is_false(b: &bool) -> bool {
    !b
}

/// A notice and its signature, as sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Sealed {
    pub notice: Notice,
    pub signature: String,
}

/// The bytes a notice's signature is over: [`DOMAIN`], then the notice's canonical form. `None` where it has no
/// canonical form.
pub fn signing_input(notice: &Notice) -> Option<Vec<u8>> {
    let mut input = DOMAIN.to_vec();
    input.extend(to_canonical(notice).ok()?);
    Some(input)
}

/// The deployment's key for notices. Core holds no private key; the assembly holds this, and may back it with the
/// operator's key, since [`DOMAIN`] keeps the two kinds of signing input apart.
pub trait NoticeKey {
    /// Signs `input`, a notice's [`signing_input`], and answers the 64-byte Ed25519 signature, or `None` where it
    /// can't sign now. Nothing is sent unsigned.
    fn sign(&mut self, input: &[u8]) -> Option<[u8; 64]>;
}

/// Seals `notice`: its signature over its signing input. `None` where the key couldn't sign it.
pub(super) fn seal(key: &mut dyn NoticeKey, notice: Notice) -> Option<Vec<u8>> {
    let signature = key.sign(&signing_input(&notice)?)?;
    serde_json::to_vec(&Sealed {
        notice,
        signature: hex(&signature),
    })
    .ok()
}

/// Lower-case hex.
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|b| [DIGITS[usize::from(b >> 4)], DIGITS[usize::from(b & 0xf)]])
        .map(char::from)
        .collect()
}

/// A 64-byte signature from its 128 lower-case hex digits.
pub(super) fn unhex(text: &str) -> Option<[u8; 64]> {
    let digit = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    let bytes = text.as_bytes();
    if bytes.len() != 128 {
        return None;
    }
    let mut out = [0u8; 64];
    for (i, pair) in bytes.as_chunks::<2>().0.iter().enumerate() {
        out[i] = (digit(pair[0])? << 4) | digit(pair[1])?;
    }
    Some(out)
}
