//! Types mirroring the `urn:rutent:module-interface:*` schemas. Field names match the schemas, every struct
//! denies unknown fields as the schemas set `additionalProperties: false`, text the schemas require non-empty is a
//! non-empty newtype, and addresses are parsed.

use alloc::{string::String, string::ToString, vec::Vec};
use core::fmt;
use core::net::IpAddr;
use core::num::NonZeroU16;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::provider_role::Operation;
/// An Ed25519 public key in its one canonical encoding, compared by its decoded bytes: the provider-role
/// contract's [`PublicKey`](crate::provider_role::PublicKey), which every key in this contract is.
pub use crate::provider_role::PublicKey;

/// A value that breaks a rule of its type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalid(pub &'static str);

impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl core::error::Error for Invalid {}

macro_rules! checked_text {
    ($(#[$doc:meta])* $name:ident, $check:expr, $message:literal) => {
        checked_text!(@type $(#[$doc])* $name, $check, $message);

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_tuple(stringify!($name)).field(&self.0).finish()
            }
        }
    };
    ($(#[$doc:meta])* redacted $name:ident, $check:expr, $message:literal) => {
        checked_text!(@type $(#[$doc])* $name, $check, $message);

        /// Never prints the text (`THR-50`).
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($name), "(<redacted>)"))
            }
        }
    };
    (@type $(#[$doc:meta])* $name:ident, $check:expr, $message:literal) => {
        $(#[$doc])*
        #[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// The value, or `None` if the text breaks the type's rule.
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
            type Error = Invalid;

            fn try_from(text: String) -> Result<Self, Self::Error> {
                $name::new(text).ok_or(Invalid($message))
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

checked_text!(
    /// A non-empty identifier: a provider, transaction, decision record or segment, or a version, as
    /// `urn:rutent:provider-role:common` writes them.
    Identifier,
    |text| !text.is_empty(),
    "an identifier must not be empty"
);

checked_text!(
    /// A non-empty name a module declares for an outside operation, a destination or a secret.
    Name,
    |text| !text.is_empty(),
    "a name must not be empty"
);

checked_text!(
    /// A non-empty DNS host name.
    Host,
    |text| !text.is_empty(),
    "a host must not be empty"
);

checked_text!(
    /// A non-empty time, in RFC 3339 UTC as `urn:rutent:provider-role:common` writes it; the schema checks the
    /// form.
    Time,
    |text| !text.is_empty(),
    "a time must not be empty"
);

checked_text!(
    /// A secret's plaintext, never empty. It appears only in a [`SecretAnswer`] to the outside part. Its `Debug`
    /// prints `SecretValue(<redacted>)`, never the text, so a logged answer doesn't leak it (`THR-50`).
    redacted SecretValue,
    |text| !text.is_empty(),
    "a secret value must not be empty"
);

checked_text!(
    /// An IP address in its one text form: IPv4 in dotted decimal, IPv6 as RFC 5952 writes it, which is how Rust
    /// writes it. An IPv4-mapped IPv6 address is refused: it is written as the IPv4 address.
    Address,
    |text| canonical_ip(text).is_some(),
    "an address must be an IP address in its canonical text form, and not IPv4-mapped"
);

checked_text!(
    /// A block of IP addresses, `address/prefix length`, with at most 32 bits for IPv4 and 128 for IPv6. The
    /// address is in [`Address`]'s one text form and the length has no leading zero.
    AddressBlock,
    is_address_block,
    "an address block must be a canonical IP address and a prefix length that fits it"
);

checked_text!(
    /// A SHA-256 digest's value: 64 lower-case hexadecimal characters, its one text form.
    DigestValue,
    |text| text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
    "a digest value must be 64 lower-case hexadecimal characters"
);

checked_text!(
    /// A portable binary format's name, such as a format `EXP-2` binds: lower-case words joined by `-`. The
    /// contract names no format (`DEC-4`).
    BinaryFormat,
    |text| {
        let words: Vec<&str> = text.split('-').collect();
        text.len() <= 64
            && text.starts_with(|c: char| c.is_ascii_lowercase())
            && words.iter().all(|w| {
                !w.is_empty() && w.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            })
    },
    "a binary format must be lower-case words joined by '-'"
);

impl Address {
    /// The parsed address.
    pub fn ip(&self) -> IpAddr {
        self.0.parse().expect("checked when built")
    }
}

impl AddressBlock {
    /// The block's address and prefix length.
    pub fn parts(&self) -> (IpAddr, u8) {
        let (ip, len) = self.0.split_once('/').expect("checked when built");
        (
            ip.parse().expect("checked when built"),
            len.parse().expect("checked when built"),
        )
    }
}

/// The address `text` writes, if it is the one text form of an IP address that isn't IPv4-mapped.
fn canonical_ip(text: &str) -> Option<IpAddr> {
    let ip: IpAddr = text.parse().ok()?;
    let mapped = matches!(ip, IpAddr::V6(v6) if v6.to_ipv4_mapped().is_some());
    (!mapped && ip.to_string() == text).then_some(ip)
}

fn is_address_block(text: &str) -> bool {
    let Some((ip, len)) = text.split_once('/') else {
        return false;
    };
    let Some(ip) = canonical_ip(ip) else {
        return false;
    };
    let Ok(parsed) = len.parse::<u8>() else {
        return false;
    };
    let max = if ip.is_ipv4() { 32 } else { 128 };
    parsed.to_string() == len && parsed <= max
}

/// Deserialises a present optional field, refusing `null`: the schemas leave an absent field out, so an explicit
/// `null` is not one of its forms. Used with `#[serde(default)]`, so an absent field is `None`.
pub(super) fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// A provider-role operation kind, as `urn:rutent:provider-role:operation` names it (`REQ-CAT-9`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    /// A reserve's withdrawal.
    Withdrawal,
    /// A reserve's deposit.
    Deposit,
    /// A ramp's transfer.
    Transfer,
    /// An exchange's conversion.
    Conversion,
    /// A reserve's balance verification.
    BalanceVerification,
    /// A verifier's judgement.
    Judgement,
    /// An exchange's or ramp's reversal of a segment (`REQ-CAT-15`).
    Reversal,
    /// A reserve's confirmation that one reversed credit arrived (ADR-0008).
    ReversalConfirmation,
}

impl OperationKind {
    /// The kind of a provider-role operation.
    pub fn of(operation: &Operation) -> Self {
        match operation {
            Operation::Withdrawal(_) => Self::Withdrawal,
            Operation::Deposit(_) => Self::Deposit,
            Operation::Transfer(_) => Self::Transfer,
            Operation::Conversion(_) => Self::Conversion,
            Operation::BalanceVerification(_) => Self::BalanceVerification,
            Operation::Judgement(_) => Self::Judgement,
            Operation::Reversal(_) => Self::Reversal,
            Operation::ReversalConfirmation(_) => Self::ReversalConfirmation,
        }
    }
}

/// What a module declares about its two parts (`urn:rutent:module-interface:declaration`, `REQ-MDL-3`,
/// `REQ-MDL-16`). Whatever it doesn't declare is refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    /// The provider whose module this is.
    pub provider: Identifier,
    /// The module's version.
    pub version: Identifier,
    /// The version of this contract the module implements, the same across module versions (`REQ-MDL-19`).
    pub interface: Identifier,
    /// The sandboxed part.
    pub sandboxed: Sandboxed,
    /// The outside part.
    pub outside: Outside,
}

/// The sandboxed part's declaration: the operation kinds it handles, and nothing else. It has no field for a
/// destination, a secret or a call, so declaring one doesn't deserialise (`REQ-MDL-6`, `REQ-MDL-16`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sandboxed {
    /// The provider-role operation kinds the module answers.
    pub handles: Vec<OperationKind>,
}

/// The outside part's declaration: its operations, destination allow-list, secret names, calls and
/// statement-signing key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outside {
    /// The outside operations the sandboxed part may ask for (`REQ-MDL-8`).
    pub operations: Vec<OutsideOperation>,
    /// The destination allow-list (`REQ-MDL-7`, ADR-0004).
    pub destinations: Vec<Destination>,
    /// The names of the secrets it may resolve (`REQ-MDL-6`, `REQ-MDL-17`).
    pub secrets: Vec<Name>,
    /// The other providers' modules it may call (`REQ-MDL-21`).
    pub calls: Vec<CallDeclaration>,
    /// The public half of the provider's statement-signing key, Ed25519, base64url without padding: the one key
    /// of its advertisement's `keys` that the outside part signs results with (`DEC-4`, `T-003-01`).
    /// [`validate_statement_key`](super::validate_statement_key) checks it against the advertisement and a
    /// signed result.
    pub statement_key: PublicKey,
}

/// One outside operation, with the destinations and secrets it may use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutsideOperation {
    /// Its name.
    pub name: Name,
    /// Names of declared destinations it may reach; a redirect is followed only to one of them.
    pub destinations: Vec<Name>,
    /// Names of declared secrets it may use.
    pub secrets: Vec<Name>,
}

/// One allowed destination: the name a connection is made to, and the addresses it may land on (ADR-0004).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Destination {
    /// Its name in the declaration.
    pub name: Name,
    /// How it is reached.
    pub scheme: Scheme,
    /// The exact host name.
    pub host: Host,
    /// The port.
    pub port: NonZeroU16,
    /// The addresses a connection to it may land on.
    pub addresses: Addresses,
}

/// How a destination is reached: over TLS only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scheme {
    /// HTTPS.
    Https,
    /// WebSocket over TLS.
    Wss,
}

/// The addresses a connection to a destination may land on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Addresses {
    /// Any globally routable address the host resolves to at call time.
    Public(PublicAddresses),
    /// Only these blocks.
    Blocks(Vec<AddressBlock>),
}

/// The word `public`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PublicAddresses {
    /// Any globally routable address, never a loopback, private, link-local or special-purpose one.
    #[serde(rename = "public")]
    Public,
}

/// A declared call to another provider's module (`REQ-MDL-21`). It carries no credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallDeclaration {
    /// The provider whose module may be called.
    pub callee: Identifier,
    /// The operation kinds that may be asked of it.
    pub kinds: Vec<OperationKind>,
}

/// From the sandboxed part to the outside part: perform one declared outside operation for one transaction
/// (`urn:rutent:module-interface:outside-request`, `REQ-MDL-8`). It names no destination and no secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutsideRequest {
    /// The transaction.
    pub transaction: Identifier,
    /// The declared outside operation.
    pub operation: Name,
    /// Its arguments.
    pub arguments: Map<String, Value>,
}

/// From the outside part back to the sandboxed part: the outcome of one outside request
/// (`urn:rutent:module-interface:outside-response`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutsideResponse {
    /// The request's transaction.
    pub transaction: Identifier,
    /// The request's outside operation.
    pub operation: Name,
    /// The outcome.
    pub outcome: OutsideOutcome,
}

/// An outside request's outcome, by status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum OutsideOutcome {
    /// The destination answered with success.
    Completed(Answer),
    /// The destination answered with a failure.
    Failed(Answer),
    /// No answer came; the effect is not known.
    Unknown(Nothing),
    /// The platform refused the call.
    Refused(Refused),
}

/// What a destination answered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    /// The answer.
    pub body: Map<String, Value>,
}

/// An outcome that carries nothing but its status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Nothing {}

/// A refusal by the platform, with its reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Refused {
    /// Why.
    pub reason: RefusalReason,
}

/// Why the platform refused an outside call. A later minor version may add a reason, so a match needs a wildcard
/// arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RefusalReason {
    /// The operation isn't declared (`REQ-MDL-8`).
    UndeclaredOperation,
    /// The destination isn't declared, or not for this operation (`REQ-MDL-7`).
    UndeclaredDestination,
    /// The secret isn't declared, or not for this operation (`REQ-MDL-6`).
    UndeclaredSecret,
    /// The connected address is outside the destination's declared addresses (ADR-0004).
    AddressNotAllowed,
    /// A redirect led to a destination the operation doesn't declare (ADR-0004).
    RedirectToUndeclaredDestination,
    /// A limit was reached (`REQ-MDL-11`).
    LimitReached,
    /// The peer at the checked address didn't prove over TLS that it is the declared host: an untrusted, expired
    /// or mismatched certificate, none, or too low a version. Nothing of the request was sent (`REQ-MDL-7`,
    /// ADR-0004).
    PeerNotVerified,
}

/// The outside part's request to resolve one declared secret by name, at call time
/// (`urn:rutent:module-interface:secret-lookup`, `REQ-MDL-17`). It names no provider: the proxy answers only for
/// the calling process's own (`DEC-9`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretLookup {
    /// The transaction the secret is used for (`REQ-MDL-6`).
    pub transaction: Identifier,
    /// The outside operation the secret is used for. The proxy resolves only a secret this operation declares
    /// (`REQ-MDL-3`, `REQ-MDL-6`).
    pub operation: Name,
    /// The secret's declared name.
    pub name: Name,
}

/// The credential proxy's answer to a [`SecretLookup`] (`urn:rutent:module-interface:secret-answer`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretAnswer {
    /// The lookup's transaction.
    pub transaction: Identifier,
    /// The lookup's outside operation.
    pub operation: Name,
    /// The lookup's name.
    pub name: Name,
    /// The outcome.
    pub outcome: SecretOutcome,
}

/// A secret lookup's outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SecretOutcome {
    /// The current value.
    Resolved(Resolved),
    /// Refused, with the reason.
    Refused(SecretRefused),
}

/// A resolved secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolved {
    /// Its current plaintext.
    pub value: SecretValue,
}

/// A refused secret lookup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretRefused {
    /// Why.
    pub reason: SecretRefusalReason,
}

/// Why a secret lookup was refused. A later minor version may add a reason, so a match needs a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SecretRefusalReason {
    /// The module doesn't declare the name, or the named operation doesn't.
    UndeclaredSecret,
    /// The caller's provider holds no secret by that name.
    NotFound,
    /// The secret store couldn't be asked or gave no usable answer, so whether the provider holds a secret by that
    /// name isn't known. It is never reported as `not_found`, and it tells the module nothing about the store
    /// (`REQ-MDL-6`, `REQ-MDL-9`).
    StoreUnavailable,
}

/// The platform's record of one outbound call (`urn:rutent:module-interface:outbound-call-record`,
/// `REQ-MDL-8`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutboundCallRecord {
    /// The transaction the call was made for.
    pub transaction: Identifier,
    /// The provider whose outside part made it.
    pub provider: Identifier,
    /// The module version that made it.
    pub module_version: Identifier,
    /// The outside operation it was made under.
    pub operation: Name,
    /// The names of the secrets resolved for it, never their values.
    pub secrets: Vec<Name>,
    /// When it started.
    pub started_at: Time,
    /// The connections in order: the first, then one per redirect.
    pub hops: Vec<Hop>,
    /// The outcome.
    pub outcome: CallOutcome,
}

/// One connection of an outbound call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hop {
    /// The scheme asked for.
    pub scheme: Scheme,
    /// The host name asked for.
    pub host: Host,
    /// The port asked for.
    pub port: NonZeroU16,
    /// The declared destination it matched by name, if any. Absent if none; `null` is refused.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub destination: Option<Name>,
    /// The address connected to, if a connection was made. Absent if none; `null` is refused.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub address: Option<Address>,
}

impl Hop {
    /// Whether the hop matched a declared destination and connected.
    pub fn connected(&self) -> bool {
        self.destination.is_some() && self.address.is_some()
    }
}

/// An outbound call's outcome, by status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CallOutcome {
    /// The destination answered with success.
    Completed(Nothing),
    /// The destination answered with a failure.
    Failed(Nothing),
    /// No answer came; the effect is not known.
    Unknown(Nothing),
    /// The platform refused the call.
    Refused(Refused),
}

/// One module's call to another provider's module, through the platform, and its record
/// (`urn:rutent:module-interface:cross-module-call`, `REQ-MDL-21`). It carries no credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrossModuleCall {
    /// The transaction it is made for.
    pub transaction: Identifier,
    /// The calling module's provider.
    pub caller: Identifier,
    /// The calling module's version.
    pub caller_version: Identifier,
    /// The route segment that names the call.
    pub route: RouteReference,
    /// What is asked of the callee.
    pub operation: Operation,
    /// Whether the callee answered it, the platform passed it on and no answer came, or the platform refused it.
    pub outcome: CrossModuleOutcome,
}

/// A cross-module call's outcome, by status (`REQ-MDL-21`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CrossModuleOutcome {
    /// The platform passed it on and the callee answered. A call is recorded as delivered only then.
    Delivered(Nothing),
    /// The platform passed it on and no answer came: the callee stopped, was stopped by a limit or a deadline, or
    /// gave no answer the platform could read. What the callee did is not known.
    Unanswered(Nothing),
    /// The platform refused it, and passed nothing on.
    Refused(CallRefused),
}

/// A refused cross-module call: the callee, the kind asked and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallRefused {
    /// The provider called.
    pub callee: Identifier,
    /// The operation kind asked.
    pub kind: OperationKind,
    /// Why.
    pub reason: CallRefusalReason,
}

/// Why the platform refused a cross-module call, in the order it checks. A later minor version may add a reason,
/// so a match needs a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CallRefusalReason {
    /// The caller doesn't declare the callee (`REQ-MDL-21`).
    UndeclaredCallee,
    /// The caller declares the callee but not the kind asked.
    UndeclaredKind,
    /// The route doesn't name the call (`REQ-MDL-21`, `THR-86`).
    NotNamedByRoute,
    /// The caller reached a limit on its cross-module calls, their count or their depth (`REQ-MDL-11`). The
    /// caller's operation ends as a limit.
    LimitReached,
    /// The callee's module isn't running.
    CalleeUnavailable,
    /// Too little of the caller's deadline is left for the callee to answer inside it, so the callee isn't started
    /// (`REQ-MDL-11`). The caller's operation goes on.
    DeadlineTooNear,
}

impl CrossModuleCall {
    /// The callee: the provider the operation asks.
    pub fn callee(&self) -> &str {
        match &self.operation {
            Operation::Withdrawal(o) => &o.provider,
            Operation::Deposit(o) => &o.provider,
            Operation::Transfer(o) => &o.provider,
            Operation::Conversion(o) => &o.provider,
            Operation::BalanceVerification(o) => &o.provider,
            Operation::Judgement(o) => &o.provider,
            Operation::Reversal(o) => &o.provider,
            Operation::ReversalConfirmation(o) => &o.provider,
        }
    }

    /// The transaction the operation names, or `None` for a judgement question, which names none.
    pub fn operation_transaction(&self) -> Option<&str> {
        match &self.operation {
            Operation::Withdrawal(o) => Some(&o.transaction),
            Operation::Deposit(o) => Some(&o.transaction),
            Operation::Transfer(o) => Some(&o.transaction),
            Operation::Conversion(o) => Some(&o.transaction),
            Operation::BalanceVerification(o) => Some(&o.transaction),
            Operation::Judgement(_) => None,
            Operation::Reversal(o) => Some(&o.transaction),
            Operation::ReversalConfirmation(o) => Some(&o.transaction),
        }
    }
}

/// The route decision record, and the segment in it, that names a call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteReference {
    /// The decision record.
    pub decision_record: Identifier,
    /// The segment.
    pub segment: Identifier,
}
