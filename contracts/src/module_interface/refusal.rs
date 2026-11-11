//! Why a platform refuses a release at admission, or refuses to pass a module's answer on: the reasons a format
//! binding gives (`REQ-MDL-1`, `REQ-MDL-11`, `REQ-MDL-12`). They name no module format (`DEC-4`). The
//! module-boundary suite scores a binding by them, and `EXP-2`'s bindings stood in `UnreadableBinary` and
//! `Unavailable` for the three reasons that had none, a disallowed import, a re-admitted version and a limit hit.

use super::admission::PartName;

/// Why a platform refused a release at admission. A Rust match on it needs a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AdmissionRefusal {
    /// This part's binary differs from the digest its assertion states (`REQ-MDL-1`).
    PartAltered(PartName),
    /// The binaries aren't a module the binding can read: not its format, or malformed in it.
    UnreadableBinary,
    /// This part imports something the binding doesn't offer a part of its kind: a host function, a system
    /// interface, or a feature turned off. The binding's own reason goes to its admission log, never a guest's
    /// string (`REQ-MDL-2`).
    DisallowedImport(PartName),
    /// The provider already has this version admitted, waiting for its acceptance, or withdrawn while it waited:
    /// a version names one release (`REQ-MDL-1`).
    VersionAlreadyAdmitted,
    /// The binding judged at admission that the module won't start.
    WontStart,
    /// Another release of the provider waits for its fresh acceptance, so no new one is admitted until it is
    /// accepted or withdrawn (`REQ-MDL-3`).
    AdmissionPending,
    /// The release's signed assertion doesn't verify: its signature, its signing key, or the deployment or
    /// protocol version it names. Nothing of the release is judged further (`REQ-MDL-1`).
    AssertionUnverified,
    /// Another release of the provider is staged on a share of its traffic, so no new one is admitted until that
    /// staging ends, promoted or rolled back (`REQ-MDL-4`, ADR-0014).
    ReleaseStaged,
    /// The release's assertion was made before that of the provider's last accepted release: publishing an older
    /// signed release never reinstates it, which only a recorded decision does (`REQ-MDL-4`, `THR-17`).
    OlderRelease,
}

/// Why a platform refused to pass a module's answer on. A Rust match on it needs a wildcard arm.
///
/// Every reason but [`OutcomeUnknown`](Self::OutcomeUnknown) says that nothing of the operation took effect: the
/// platform handed it to no module, or the module passed nothing out, no outbound call and no cross-module call,
/// before it ended. Once the module passed something out, whatever ends the operation without an answer, a limit,
/// a bad output, a stop or a result that can't be signed, is [`OutcomeUnknown`](Self::OutcomeUnknown), and what
/// ended it is recorded beside it (`REQ-ORD-1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum PlatformRefusal {
    /// The output isn't a result of the operation's shape: unparseable, of another kind, for another transaction,
    /// with an unknown field, or nesting a statement in the provider's name that isn't what the operation asked
    /// (`REQ-MDL-12`). Only where the module passed nothing out: after, it is
    /// [`OutcomeUnknown`](Self::OutcomeUnknown).
    MalformedOutput,
    /// The output is longer than the limit the binding was configured with (`REQ-MDL-12`). Only where the module
    /// passed nothing out: after, it is [`OutcomeUnknown`](Self::OutcomeUnknown).
    OversizedOutput,
    /// A part was stopped by a limit before it answered: its CPU budget, its memory, its time, or a count or size
    /// of its host calls (`REQ-MDL-11`). The module stays admitted and its process keeps running, or is started
    /// again. Only where the module passed nothing out: after, it is [`OutcomeUnknown`](Self::OutcomeUnknown), and
    /// the limit is recorded all the same.
    LimitReached,
    /// No module of the provider is running, or the one handed the operation stopped before it passed anything
    /// out: nothing of the operation took effect.
    Unavailable,
    /// The module doesn't handle the operation's kind.
    NotHandled,
    /// The operation's transaction is one a segment already began at another provider, or one whose segment
    /// ended: a transaction is identified once, and repeating its identifier moves nothing (`REQ-MDL-14`,
    /// `INV-8`).
    DuplicateTransaction,
    /// The version the operation's segment runs on was declared actively exploited: it runs nothing more, and the
    /// segment isn't moved to another (`REQ-MDL-20`).
    VersionExploited,
    /// The store holding the private half of the provider's statement key couldn't answer, so no answer could be
    /// signed. The platform asks before it hands the operation to the module, so nothing of the operation ran. The
    /// store may answer later: the operation can be asked again (`REQ-MDL-6`, `REQ-MDL-17`).
    StatementKeyUnavailable,
    /// The provider's statement key can't sign: the store holds none, or another key, under the declared one's
    /// name, or the key isn't one the platform may resolve (not advertised for statements, listed for
    /// advertising, or the operator's key). The platform judges it before it hands the operation to the module,
    /// so nothing of the operation ran. It is the deployment's or the provider's misconfiguration, and asking
    /// again changes nothing until it is fixed (`REQ-MDL-6`, `THR-4`).
    StatementKeyMisconfigured,
    /// The module took the operation and passed something out, an outbound call or a cross-module call, and then
    /// gave no answer the platform passes on: a limit stopped it, its output was malformed or oversized, its process
    /// stopped, it failed, or its answer couldn't be signed. What it did may have taken effect, so this is never
    /// read as nothing having happened, and the operation is not asked again as if it had not run: what became of it
    /// is read from its outbound and cross-module call records (`REQ-ORD-1`, `REQ-MDL-8`, `REQ-MDL-21`).
    OutcomeUnknown,
}
