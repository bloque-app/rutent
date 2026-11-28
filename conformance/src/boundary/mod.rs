//! The module-boundary suite (`T-003-03`, `DEC-5`, `AC-19`, `AC-20`, `AC-21`, `REQ-MDL-6`, `REQ-MDL-7`,
//! `REQ-MDL-9`, `REQ-MDL-12`).
//!
//! It scores a **format binding**: the runtime that admits a module in some portable binary format, runs its
//! sandboxed part and its outside part, resolves its secrets, carries its outbound calls and checks its output.
//! `EXP-2` binds it to Wasm (`T-008-02`) and PVM (`T-008-03`). The in-process double it runs against here,
//! `reference::ReferenceDouble`, is built only with the `test-support` feature.
//!
//! **Format-neutral.** The suite describes each module it needs as a [`ModuleSpec`]: a module-interface
//! declaration, the [`Step`]s its parts take on each operation, and the [`Output`] its sandboxed part returns. A
//! binding builds that module in its own format and in each language it offers ([`BoundaryHarness::build`]); the
//! suite then wraps the binaries in a release assertion and admits them like any release (T-003-02).
//!
//! **Evidence, not silence.** A hostile module passes nothing by doing nothing. Each hostile fixture counts only when
//! the binding shows the refusal in contract terms, with the expected reason: a refused secret lookup, a refused
//! [`OutboundCallRecord`], a refused output, a refused [`CrossModuleCall`]. Each has an honest control that shares
//! its declaration and must succeed. A fixture or control not seen is [`Uncovered`], and the report doesn't pass.
//! Beside the refusals, the suite seeds another provider's secret and state with unique canaries, and looks for
//! them in every channel the module can write to. A finding names the channel, never the value (`THR-50`).
//!
//! **The harness is apart from the binding.** Building the suite's modules, the simulated world (the secrets in the
//! proxy, each provider's state, the endpoints and what they received) and the secret values the proxy delivered
//! are reached through [`BoundaryHarness`], a test-only trait. A production runtime implements [`ModuleBinding`]
//! alone, and that trait never hands out a secret's value: a lookup's outcome is its status only.
//!
//! **One process per provider** is expressible: [`ModuleBinding::process`] names the process a provider's module
//! runs in, and restarting one provider must leave every other one running, in the same process (`REQ-MDL-13`).

use std::collections::BTreeSet;
use std::fmt;

use rutent_contracts::module_interface::{
    AdmissionArtefact, CrossModuleCall, Declaration, OperationKind, OutboundCallRecord, PartName,
    Parts, ReleaseAssertion, Scheme, SecretLookup, SecretRefusalReason,
};
use rutent_contracts::provider_role::{Operation, ResultBody, Statement};

use crate::role::SuiteContext;

#[cfg(feature = "test-support")]
pub mod reference;
pub(crate) mod sha256;
mod suite;

pub use suite::run_boundary_suite;

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

impl Endpoint {
    /// An `https` endpoint on port 443.
    pub fn https(host: &str) -> Self {
        Self {
            scheme: Scheme::Https,
            host: host.to_owned(),
            port: 443,
        }
    }
}

impl Endpoint {
    fn key(&self) -> (u8, &str, u16) {
        let scheme = match self.scheme {
            Scheme::Https => 0,
            Scheme::Wss => 1,
        };
        (scheme, &self.host, self.port)
    }
}

impl PartialOrd for Endpoint {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Endpoint {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.key().cmp(&other.key())
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scheme = match self.scheme {
            Scheme::Https => "https",
            Scheme::Wss => "wss",
        };
        write!(f, "{scheme}://{}:{}", self.host, self.port)
    }
}

/// One thing a module's part does while it handles an operation, in order. Whatever a step obtains, a secret's
/// value or a state entry, the module carries onwards: into the body of every later outbound call and into its
/// output, so that a leak is visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Resolve the secret `name` for the outside operation `operation`. From the sandboxed part this is hostile:
    /// it has no route to the proxy (`REQ-MDL-6`).
    LookUpSecret {
        /// The part that asks.
        part: PartName,
        /// The outside operation it asks for.
        operation: String,
        /// The secret's name.
        name: String,
    },
    /// Read the state entry `key` from the module's state.
    ReadState {
        /// The part that reads.
        part: PartName,
        /// The entry.
        key: String,
    },
    /// From the outside part: call `endpoint` under the outside operation `operation`.
    Call {
        /// The outside operation.
        operation: String,
        /// Where.
        endpoint: Endpoint,
    },
    /// From the outside part: call another provider's module, asking it `kind`.
    CallModule {
        /// The provider called.
        callee: String,
        /// The operation kind asked.
        kind: OperationKind,
    },
}

/// What a module's sandboxed part returns for an operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    /// A `withdrawal` result for the operation's transaction with outcome `succeeded`: `amount_in` and
    /// `amount_out` the operation's amount, a fee of `"0"` in its asset, and one evidence piece of grade 3 by the
    /// provider. That piece's `reference` is exactly `{provider}-{version}`, then `-{value}` for each value the
    /// module's steps obtained, in order. The suite compares it exactly: it is how the suite tells which version
    /// answered. The suite sends only withdrawals, so every spec's declaration handles `withdrawal`. It is emitted
    /// as compact JSON, with no insignificant whitespace, so its length is that of its re-serialisation.
    Honest,
    /// The honest result, its evidence reference padded with `x` so that its bytes as the module emits them are
    /// exactly this many.
    PaddedTo(usize),
    /// Bytes that aren't JSON.
    Unparseable,
    /// A valid result of another kind than the operation's.
    AnotherKind,
    /// A valid result naming another transaction.
    AnotherTransaction,
    /// The honest result with a field no result has.
    UnknownField,
}

/// A module the suite needs, described format-neutrally: what it declares, what its parts do, what it returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleSpec {
    /// Its declaration, which names its provider and version.
    pub declaration: Declaration,
    /// What its parts do on each operation, in order.
    pub steps: Vec<Step>,
    /// What its sandboxed part returns.
    pub output: Output,
    /// Whether it starts. A module that doesn't is admissible, and fails when it is started.
    pub starts: bool,
}

/// What admission gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admitted {
    /// The binding's admission artefact: accepted at once where only an automatic acceptance was required, and
    /// waiting for [`ModuleBinding::accept`] where a fresh one is.
    Artefact(Box<AdmissionArtefact>),
    /// Refused, with why.
    Refused(AdmissionRefusal),
}

/// Why a binding refused a release at admission, and why the platform refused to pass a module's answer on: the
/// module-interface contract's reasons, re-exported where the suite always had them.
pub use rutent_contracts::module_interface::{AdmissionRefusal, PlatformRefusal};

/// What handling one operation gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handled {
    /// The module's result, signed by its outside part, as the platform passes it on.
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
    /// part's must not. A resolved value is never handed out here; the harness checks values
    /// ([`BoundaryHarness::delivered_secrets`]).
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

/// What a binding saw since it was last asked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Events {
    /// The outbound call records its egress layer wrote.
    pub records: Vec<OutboundCallRecord>,
    /// The secret lookups modules made.
    pub lookups: Vec<LookupEvent>,
    /// The state reads modules made.
    pub state_reads: Vec<StateEvent>,
    /// The records of the cross-module calls modules made, delivered, unanswered or refused, as the contract
    /// shapes them.
    pub calls: Vec<CrossModuleCall>,
}

/// An operating-system process, as a binding names it.
pub type ProcessId = u64;

/// A format binding, as the module-boundary suite drives it. Nothing here names a format.
pub trait ModuleBinding {
    /// The portable binary format it binds, as release assertions name it.
    fn format(&self) -> String;

    /// The guest languages it can build modules in. `AC-20` asks for two.
    fn languages(&self) -> Vec<String>;

    /// Sets the largest output, in bytes as a module emits them, the platform passes on. The suite sets it,
    /// so a binding can't pass by having no limit.
    fn set_output_limit(&mut self, bytes: usize);

    /// Sets the most steps one module's operation may take: host calls of its parts, which a binding counts as
    /// it does its per-operation bounds (`REQ-MDL-11`). The suite sets it, so a binding can't pass by having no
    /// limit, and probes a module at it and one past it, then lifts it with `usize::MAX`, so the limit bounds the
    /// probes and nothing else. False if the binding can't be given one, which leaves the probe uncovered and the
    /// report not passed: the default.
    fn set_step_limit(&mut self, steps: usize) -> bool {
        let _ = steps;
        false
    }

    /// Admits a release: the provider's signed assertion and the binaries it received. The binding keeps each
    /// provider's history, and starts a release that needs only an automatic acceptance.
    fn admit(
        &mut self,
        assertion: &Statement<ReleaseAssertion>,
        binaries: &Parts<Vec<u8>>,
    ) -> Admitted;

    /// Gives a fresh acceptance, by `by`, to a provider's release that waits for one, and starts it. The
    /// accepted artefact, or `None` if no such release waits.
    fn accept(&mut self, provider: &str, version: &str, by: &str) -> Option<AdmissionArtefact>;

    /// The version of the provider's module now running, if any.
    fn active_version(&self, provider: &str) -> Option<String>;

    /// The process the provider's module runs in, if it runs.
    fn process(&self, provider: &str) -> Option<ProcessId>;

    /// Restarts the provider's process. False if it has none.
    fn restart(&mut self, provider: &str) -> bool;

    /// Hands one provider-role operation to the provider's running module.
    fn handle(&mut self, provider: &str, operation: &Statement<Operation>) -> Handled;

    /// What the binding saw since it was last asked: records, lookups, state reads and cross-module calls.
    fn take_events(&mut self) -> Events;
}

/// The test hooks that set up the world a binding runs in. Test-only: a production runtime has no way to seed a
/// provider's secrets or to read what an endpoint received.
pub trait BoundaryHarness: ModuleBinding {
    /// Builds the module `spec` describes in `language`: each part's portable binary. `None` if it can't, and
    /// whatever the suite would have checked with it is then uncovered.
    fn build(&mut self, spec: &ModuleSpec, language: &str) -> Option<Parts<Vec<u8>>>;

    /// The secret values the proxy delivered to the provider's outside part, for the suite to look for another
    /// provider's canary. Only the harness sees values; [`ModuleBinding`] reports a lookup's status only.
    fn delivered_secrets(&self, provider: &str) -> Vec<String>;

    /// Stores `value` as the provider's secret `name` in the credential proxy.
    fn seed_secret(&mut self, provider: &str, name: &str, value: &str);

    /// Stores `value` as the provider's state entry `key`.
    fn seed_state(&mut self, provider: &str, key: &str, value: &str);

    /// Gives the binding a provider's signed advertisement, as the operator's grant and the provider's accepted
    /// advertisement would reach a platform: the keys it lists are the ones that vouch for the provider's release
    /// assertions (`THR-4`). False if the binding wouldn't take it.
    fn list_provider(&mut self, advertisement: &[u8]) -> bool;

    /// Serves `endpoint` at `address`, redirecting to `redirect` if given.
    fn serve(
        &mut self,
        endpoint: &Endpoint,
        address: std::net::IpAddr,
        redirect: Option<&Endpoint>,
    );

    /// The bodies of the requests `endpoint` received.
    fn received(&self, endpoint: &Endpoint) -> Vec<String>;

    /// How many operations the provider's module received, from the platform or from another module.
    fn operations_received(&self, provider: &str) -> usize;
}

/// The step limit the suite sets and probes, in steps of one module's operation (`REQ-MDL-11`). It is not part of
/// [`RunConfig`], whose fields a binding's own runner writes out.
pub const STEP_LIMIT: usize = 3;

/// How the suite runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunConfig {
    /// The run's identifier, carried into the report for `EXP-2`'s scoring sheet.
    pub run_id: String,
    /// The output limit the binding is configured with, in bytes.
    pub output_limit: usize,
    /// The deployment, protocol version and operator every statement names.
    pub context: SuiteContext,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            run_id: "conformance".to_owned(),
            output_limit: 4096,
            context: SuiteContext::default(),
        }
    }
}

/// One property of the module boundary the suite checks. Each groups every observation of one property, so a
/// binding that breaks it is flagged by this check alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Check {
    /// What a module declares works: its own secret resolves for its declared operation, a declared endpoint
    /// receives its call and the record says completed, a redirect to a destination the operation lists is
    /// followed, its own state reads back, a declared cross-module call is delivered, and its honest output is
    /// passed on. A binding that refuses everything fails here.
    DeclaredReachWorks,
    /// Neither part obtains another provider's secret: the proxy answers a lookup only from the caller's own
    /// secrets, and the other provider's canary appears in no channel (`REQ-MDL-9`, `AC-20`, `DEC-9`).
    OtherProvidersSecrets,
    /// Neither part reads another provider's state, and the other provider's canary appears in no channel
    /// (`REQ-MDL-9`, `AC-20`).
    OtherProvidersState,
    /// The sandboxed part never reaches the proxy, not even for its own provider's secret, and never sees a
    /// secret (`REQ-MDL-6`).
    SandboxedPartHoldsNoSecret,
    /// A lookup of a secret name the module doesn't declare is refused as `undeclared_secret`, even where the
    /// provider holds a secret by that name (`REQ-MDL-6`).
    UndeclaredSecretRefused,
    /// An outside part's call to an endpoint no declared destination of its operation names, another provider's
    /// included, is refused as `undeclared_destination`, and the endpoint receives nothing (`REQ-MDL-7`).
    UndeclaredDestinationRefused,
    /// A redirect to a destination the operation doesn't list is refused as `redirect_to_undeclared_destination`
    /// at that hop, however far along a chain of declared hops it comes, and its target receives nothing
    /// (`REQ-MDL-7`, ADR-0004).
    RedirectChecked,
    /// A declared host that resolves to an address outside its declared addresses is refused as
    /// `address_not_allowed`, on the first connection or after a redirect, over IPv4 or IPv6, and the address
    /// receives nothing (`REQ-MDL-7`, ADR-0004).
    ConnectedAddressChecked,
    /// Every outbound call record is valid under the contract, is about the call's transaction, provider, module
    /// version and operation, and names only secrets its operation declares (`REQ-MDL-8`).
    RecordsTellTheTruth,
    /// A cross-module call to a provider the caller doesn't declare, or of a kind it doesn't declare for it, is
    /// refused and recorded with that reason, and the running callee receives nothing (`REQ-MDL-9`,
    /// `REQ-MDL-21`).
    UndeclaredCallRefused,
    /// Output that isn't a result of the operation's shape is refused: unparseable, another kind, another
    /// transaction, an unknown field (`REQ-MDL-12`).
    OutputShapeChecked,
    /// Output exactly at the configured limit, measured on what the platform passed on, is passed on, and output one
    /// byte over it is refused (`REQ-MDL-12`).
    OutputSizeBounded,
    /// Admission records what it admits: a release whose assertion doesn't verify, signed by a key no
    /// advertisement lists or altered after signing, is refused as unverified (`REQ-MDL-1`); a release with either
    /// part's binary differing from its assertion is refused, and a
    /// first release's artefact is valid, received as asserted, and waits for a fresh acceptance (`REQ-MDL-1`,
    /// `REQ-MDL-2`).
    AdmissionRecordsWhatItAdmits,
    /// A release that widens its capabilities, by a destination, a secret, a callee or an address, waits for a
    /// fresh acceptance, and until then neither runs nor lends its reach; a narrowing one runs at once and its
    /// dropped reach is refused; every successive artefact is valid under the contract, its widenings the computed
    /// ones (`AC-21`, `REQ-MDL-3`).
    WideningNeedsFreshAcceptance,
    /// One provider's upgrade, failed upgrade or restart leaves every other provider running in its own process
    /// and answering as before, and a failed upgrade leaves the previous version in force (`AC-19`,
    /// `REQ-MDL-13`).
    ProvidersAreIsolated,
    /// A version a provider already has admitted, running or waiting for its acceptance, is never admitted again:
    /// it is refused as `version_already_admitted`, the reason that names it, and what runs is unchanged
    /// (`REQ-MDL-1`).
    ReadmittedVersionRefused,
    /// A module that takes more steps in one operation than the limit the suite set is refused as
    /// `limit_reached`, its module still admitted, and one that takes the limit's own number of steps answers
    /// (`REQ-MDL-11`).
    StepLimitEnforced,
}

/// A fixture or control the suite saw pass, in one language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Exercised {
    /// Its own secret resolved for its declared operation.
    OwnSecretResolved,
    /// Its own state read back.
    OwnStateRead,
    /// A declared endpoint received its call, recorded as completed.
    DeclaredCallCompleted,
    /// A redirect to a listed destination was followed.
    DeclaredRedirectFollowed,
    /// A declared cross-module call was delivered to the running callee.
    DeclaredCallDelivered,
    /// Its honest output was passed on.
    HonestOutputPassed,
    /// The outside part's lookup of another provider's secret was refused.
    ForeignSecretRefusedToOutside,
    /// The sandboxed part's lookup of another provider's secret never reached the proxy.
    ForeignSecretRefusedToSandboxed,
    /// The outside part's read of another provider's state found nothing.
    ForeignStateRefusedToOutside,
    /// The sandboxed part's read of another provider's state found nothing.
    ForeignStateRefusedToSandboxed,
    /// The sandboxed part's lookup of its own provider's secret never reached the proxy.
    SandboxedLookupRefused,
    /// A lookup of a secret name the module doesn't declare was refused as `undeclared_secret`.
    UndeclaredSecretNameRefused,
    /// A call to an undeclared endpoint was refused.
    UndeclaredDestinationRefused,
    /// A call to another provider's endpoint was refused.
    OtherProvidersEndpointRefused,
    /// A redirect to an undeclared destination was refused.
    UndeclaredRedirectRefused,
    /// A redirect to an undeclared destination at the end of a chain of declared hops was refused.
    RedirectChainRefused,
    /// A connection to an address outside the declared ones was refused.
    AddressRefused,
    /// A redirect to a declared host at a private address was refused.
    RedirectToPrivateAddressRefused,
    /// A declared host at the IPv6 loopback address was refused.
    Ipv6LoopbackRefused,
    /// A declared host at an IPv6 unique-local address, in `fd00::/8`, was refused.
    Ipv6UniqueLocalRefused,
    /// A declared host at an IPv4-mapped private address was refused.
    Ipv4MappedRefused,
    /// A cross-module call to an undeclared callee was refused.
    UndeclaredCallRefused,
    /// A cross-module call of a kind not declared for its callee was refused.
    UndeclaredKindRefused,
    /// Unparseable output was refused.
    UnparseableOutputRefused,
    /// Output of another kind was refused.
    OutputOfAnotherKindRefused,
    /// Output for another transaction was refused.
    OutputForAnotherTransactionRefused,
    /// Output with an unknown field was refused.
    OutputWithAnUnknownFieldRefused,
    /// Output exactly at the limit was passed on.
    OutputAtTheLimitPassed,
    /// Output one byte over the limit was refused.
    OversizedOutputRefused,
    /// A first release was admitted, waited for a fresh acceptance and ran once given it.
    FirstReleaseAdmitted,
    /// A release whose binaries differ from its assertion was refused.
    AlteredReleaseRefused,
    /// A release whose assertion was signed by another key, or altered after it was signed, was refused as
    /// unverified; the same release, signed right, was admitted.
    ForgedReleaseRefused,
    /// A widening release waited for a fresh acceptance and ran once given it.
    WideningReleaseHeld,
    /// While a widening release waited, its new destination stayed unreachable.
    PendingReleaseUnreachable,
    /// A narrowing release ran at once.
    NarrowingReleaseRan,
    /// After a narrowing release, the destination it dropped was refused.
    NarrowedReachRefused,
    /// A release widening by a secret waited for a fresh acceptance.
    WideningBySecretHeld,
    /// A release widening by a callee waited for a fresh acceptance.
    WideningByCalleeHeld,
    /// A release widening by an address waited for a fresh acceptance.
    WideningByAddressHeld,
    /// Another provider kept its process and its answers through an upgrade, a failed upgrade and a restart.
    OtherProviderUndisturbed,
    /// A failed upgrade left the previous version running.
    FailedUpgradeKeptThePrevious,
    /// A restart gave the provider a new process and its module answered again.
    RestartedProviderAnswered,
    /// The version running was admitted again, and refused as already admitted; what ran went on running.
    ReadmittedVersionRefused,
    /// The version waiting for its acceptance was admitted again, and refused as already admitted.
    ReadmittedWaitingVersionRefused,
    /// A module past the step limit was refused as `limit_reached`, and stayed admitted.
    StepLimitRefused,
    /// A module that took exactly the limit's steps answered.
    StepsWithinTheLimitAnswered,
}

impl Exercised {
    /// Its position in [`Exercised::ALL`]. The match is exhaustive, so a variant added without an arm doesn't
    /// compile, and the suite's tests check that `ALL` holds each variant at its position.
    pub fn ordinal(self) -> usize {
        match self {
            Self::OwnSecretResolved => 0,
            Self::OwnStateRead => 1,
            Self::DeclaredCallCompleted => 2,
            Self::DeclaredRedirectFollowed => 3,
            Self::DeclaredCallDelivered => 4,
            Self::HonestOutputPassed => 5,
            Self::ForeignSecretRefusedToOutside => 6,
            Self::ForeignSecretRefusedToSandboxed => 7,
            Self::ForeignStateRefusedToOutside => 8,
            Self::ForeignStateRefusedToSandboxed => 9,
            Self::SandboxedLookupRefused => 10,
            Self::UndeclaredSecretNameRefused => 11,
            Self::UndeclaredDestinationRefused => 12,
            Self::OtherProvidersEndpointRefused => 13,
            Self::UndeclaredRedirectRefused => 14,
            Self::RedirectChainRefused => 15,
            Self::AddressRefused => 16,
            Self::RedirectToPrivateAddressRefused => 17,
            Self::Ipv6LoopbackRefused => 18,
            Self::Ipv6UniqueLocalRefused => 19,
            Self::Ipv4MappedRefused => 20,
            Self::UndeclaredCallRefused => 21,
            Self::UndeclaredKindRefused => 22,
            Self::UnparseableOutputRefused => 23,
            Self::OutputOfAnotherKindRefused => 24,
            Self::OutputForAnotherTransactionRefused => 25,
            Self::OutputWithAnUnknownFieldRefused => 26,
            Self::OutputAtTheLimitPassed => 27,
            Self::OversizedOutputRefused => 28,
            Self::FirstReleaseAdmitted => 29,
            Self::AlteredReleaseRefused => 30,
            Self::ForgedReleaseRefused => 31,
            Self::WideningReleaseHeld => 32,
            Self::PendingReleaseUnreachable => 33,
            Self::NarrowingReleaseRan => 34,
            Self::NarrowedReachRefused => 35,
            Self::WideningBySecretHeld => 36,
            Self::WideningByCalleeHeld => 37,
            Self::WideningByAddressHeld => 38,
            Self::OtherProviderUndisturbed => 39,
            Self::FailedUpgradeKeptThePrevious => 40,
            Self::RestartedProviderAnswered => 41,
            Self::ReadmittedVersionRefused => 42,
            Self::ReadmittedWaitingVersionRefused => 43,
            Self::StepLimitRefused => 44,
            Self::StepsWithinTheLimitAnswered => 45,
        }
    }

    /// Every fixture and control, in order.
    pub const ALL: [Self; 46] = [
        Self::OwnSecretResolved,
        Self::OwnStateRead,
        Self::DeclaredCallCompleted,
        Self::DeclaredRedirectFollowed,
        Self::DeclaredCallDelivered,
        Self::HonestOutputPassed,
        Self::ForeignSecretRefusedToOutside,
        Self::ForeignSecretRefusedToSandboxed,
        Self::ForeignStateRefusedToOutside,
        Self::ForeignStateRefusedToSandboxed,
        Self::SandboxedLookupRefused,
        Self::UndeclaredSecretNameRefused,
        Self::UndeclaredDestinationRefused,
        Self::OtherProvidersEndpointRefused,
        Self::UndeclaredRedirectRefused,
        Self::RedirectChainRefused,
        Self::AddressRefused,
        Self::RedirectToPrivateAddressRefused,
        Self::Ipv6LoopbackRefused,
        Self::Ipv6UniqueLocalRefused,
        Self::Ipv4MappedRefused,
        Self::UndeclaredCallRefused,
        Self::UndeclaredKindRefused,
        Self::UnparseableOutputRefused,
        Self::OutputOfAnotherKindRefused,
        Self::OutputForAnotherTransactionRefused,
        Self::OutputWithAnUnknownFieldRefused,
        Self::OutputAtTheLimitPassed,
        Self::OversizedOutputRefused,
        Self::FirstReleaseAdmitted,
        Self::AlteredReleaseRefused,
        Self::ForgedReleaseRefused,
        Self::WideningReleaseHeld,
        Self::PendingReleaseUnreachable,
        Self::NarrowingReleaseRan,
        Self::NarrowedReachRefused,
        Self::WideningBySecretHeld,
        Self::WideningByCalleeHeld,
        Self::WideningByAddressHeld,
        Self::OtherProviderUndisturbed,
        Self::FailedUpgradeKeptThePrevious,
        Self::RestartedProviderAnswered,
        Self::ReadmittedVersionRefused,
        Self::ReadmittedWaitingVersionRefused,
        Self::StepLimitRefused,
        Self::StepsWithinTheLimitAnswered,
    ];
}

/// A fixture or control the suite didn't see pass in a language.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Uncovered {
    /// The language.
    pub language: String,
    /// The fixture or control.
    pub answer: Exercised,
}

/// One broken rule, in one language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// The rule.
    pub check: Check,
    /// The language of the module it was seen with.
    pub language: String,
    /// What the suite saw. It names a channel, never a secret's value.
    pub detail: String,
}

/// What a run found and exercised, for one format.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// The run's identifier.
    pub run_id: String,
    /// The format the binding binds.
    pub format: String,
    /// The languages it offered.
    pub languages: Vec<String>,
    /// The output limit it was configured with.
    pub output_limit: usize,
    /// Every broken rule.
    pub findings: Vec<Finding>,
    /// What passed, by language.
    pub exercised: BTreeSet<(String, Exercised)>,
    /// What didn't, by language.
    pub uncovered: BTreeSet<Uncovered>,
    /// How many languages short of `AC-20`'s two the binding is.
    pub missing_languages: usize,
}

impl Report {
    /// Whether no rule was broken, every fixture and control passed in every language, and there were at least
    /// two languages.
    pub fn passed(&self) -> bool {
        self.findings.is_empty() && self.uncovered.is_empty() && self.missing_languages == 0
    }

    /// The rules broken, each once.
    pub fn flagged(&self) -> BTreeSet<Check> {
        self.findings.iter().map(|f| f.check).collect()
    }
}
