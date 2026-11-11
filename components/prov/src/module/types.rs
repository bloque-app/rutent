//! What the platform answers with: the shapes of admission, of an operation's outcome and of what it saw. They
//! name no format and no test harness; the module-boundary suite's binding maps them to the suite's own.

use rutent_contracts::module_interface::{
    AdmissionArtefact, CrossModuleCall, OutboundCallRecord, PartName, Scheme, SecretLookup,
    SecretRefusalReason,
};
use rutent_contracts::provider_role::{Deployment, ResultBody, Statement};

pub use crate::module::limits::{LimitHit, LimitKind, Metered, Scope};

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
    /// Refused: the assertion isn't one the platform admits on its word.
    Unverified(crate::module::admission::Unverified),
}

pub use rutent_contracts::module_interface::{AdmissionRefusal, PlatformRefusal};

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

/// One bound a module hit (`REQ-MDL-11`): the operation it was handling was refused as
/// [`PlatformRefusal::LimitReached`], and every later request of that operation was refused too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitEvent {
    /// The provider whose module hit it.
    pub provider: String,
    /// The transaction of the operation it was handling.
    pub transaction: String,
    /// Which bound, and which budget of it where it was a count or a total.
    pub hit: LimitHit,
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
    /// The records of the cross-module calls modules made, delivered, unanswered or refused, as the contract shapes
    /// them.
    pub calls: Vec<CrossModuleCall>,
    /// The bounds modules hit.
    pub limits: Vec<LimitEvent>,
}

/// An operating-system process, as the platform names it.
pub type ProcessId = u64;
