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
    /// The provider already has this version admitted, or waiting for its acceptance: a version names one
    /// release (`REQ-MDL-1`).
    VersionAlreadyAdmitted,
    /// The binding judged at admission that the module won't start.
    WontStart,
    /// Another release of the provider waits for its fresh acceptance, so no new one is admitted until it is
    /// accepted or withdrawn (`REQ-MDL-3`).
    AdmissionPending,
    /// The release's signed assertion doesn't verify: its signature, its signing key, or the deployment or
    /// protocol version it names. Nothing of the release is judged further (`REQ-MDL-1`).
    AssertionUnverified,
}

/// Why a platform refused to pass a module's answer on. A Rust match on it needs a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum PlatformRefusal {
    /// The output isn't a result of the operation's shape: unparseable, of another kind, for another transaction,
    /// or with an unknown field (`REQ-MDL-12`).
    MalformedOutput,
    /// The output is longer than the limit the binding was configured with (`REQ-MDL-12`).
    OversizedOutput,
    /// A part was stopped by a limit before it answered: its CPU budget, its memory, its time, or a count or size
    /// of its host calls (`REQ-MDL-11`). The module stays admitted and its process keeps running.
    LimitReached,
    /// No module of the provider is running.
    Unavailable,
    /// The module doesn't handle the operation's kind.
    NotHandled,
}
