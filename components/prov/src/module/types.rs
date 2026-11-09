//! What the platform answers with: the shapes of admission, of an operation's outcome and of what it saw. They
//! name no format and no test harness; the module-boundary suite's binding maps them to the suite's own.

use rutent_contracts::module_interface::{
    AdmissionArtefact, CrossModuleCall, OutboundCallRecord, PartName, Scheme, SecretLookup,
    SecretRefusalReason,
};
use rutent_contracts::provider_role::{Deployment, ResultBody, Statement};

/// An endpoint a module's outside part may try to reach: a scheme, an exact host and a port.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Endpoint {
    /// The scheme.
    pub scheme: Scheme,
    /// The host name.
    pub host: String,
    /// The port.
    pub port: u16,
}

/// The deployment a platform serves and the protocol version it speaks: what every result it passes on names,
/// whatever the envelope of the operation it answers said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    /// The deployment.
    pub deployment: Deployment,
    /// The protocol version.
    pub protocol_version: String,
}

/// What admission gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admitted {
    /// The admission artefact: accepted at once where only an automatic acceptance was required, and waiting for
    /// [`Platform::accept`](crate::module::platform::Platform::accept) where a fresh one is.
    Artefact(Box<AdmissionArtefact>),
    /// Refused, with why.
    Refused(AdmissionRefusal),
}

/// Why a release was refused at admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionRefusal {
    /// This part's binary differs from the digest its assertion states (`REQ-MDL-1`).
    PartAltered(PartName),
    /// The binaries aren't a module the platform can read.
    UnreadableBinary,
    /// The platform judged at admission that the module won't start.
    WontStart,
}

/// Why the platform refused to pass a module's answer on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PlatformRefusal {
    /// The output isn't a result of the operation's shape: unparseable, of another kind, for another transaction,
    /// or with an unknown field (`REQ-MDL-12`).
    MalformedOutput,
    /// The output is longer than the limit the platform was configured with (`REQ-MDL-12`).
    OversizedOutput,
    /// No module of the provider is running.
    Unavailable,
    /// The module doesn't handle the operation's kind.
    NotHandled,
}

/// What handling one operation gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handled {
    /// The module's result, signed with the declaration's statement key, as the platform passes it on.
    Answered(Box<Statement<ResultBody>>),
    /// The platform refused it.
    Refused(PlatformRefusal),
}

/// One secret lookup a module's part made, and what came of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LookupEvent {
    /// The provider whose module made it.
    pub provider: String,
    /// The part that made it.
    pub part: PartName,
    /// The lookup.
    pub lookup: SecretLookup,
    /// The proxy's answer, by status only, or `None` where the lookup never reached the proxy, as a sandboxed
    /// part's must not. A resolved value is never recorded here.
    pub outcome: Option<LookupOutcome>,
}

/// A secret lookup's outcome, without its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LookupOutcome {
    /// The proxy resolved it.
    Resolved,
    /// The proxy refused it, for this reason.
    Refused(SecretRefusalReason),
}

/// One state read a module's part made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateEvent {
    /// The provider whose module made it.
    pub provider: String,
    /// The part that read.
    pub part: PartName,
    /// The entry asked for.
    pub key: String,
    /// Whether the read found a value.
    pub found: bool,
}

/// What the platform saw since it was last asked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Events {
    /// The outbound call records its egress layer wrote.
    pub records: Vec<OutboundCallRecord>,
    /// The secret lookups modules made.
    pub lookups: Vec<LookupEvent>,
    /// The state reads modules made.
    pub state_reads: Vec<StateEvent>,
    /// The records of the cross-module calls modules made, delivered or refused, as the contract shapes them.
    pub calls: Vec<CrossModuleCall>,
}

/// An operating-system process, as the platform names it.
pub type ProcessId = u64;
