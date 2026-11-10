//! Module admission (`T-003-02`, `REQ-MDL-1`, `REQ-MDL-2`, `REQ-MDL-3`, `REQ-MDL-15`): the provider's signed release
//! assertion, the platform's admission artefact that binds it to what runs, and the widening of capabilities from
//! one release to the next.
//!
//! The contract states digests and signatures and checks their form; computing digests over bytes and verifying
//! signatures is the platform's (`CMP-PROV`, `M2`/`M3`), as in the provider-role contract.

use alloc::vec::Vec;
use core::net::IpAddr;

use serde::{Deserialize, Serialize};

use super::types::{
    AddressBlock, Addresses, BinaryFormat, Declaration, DigestValue, Identifier, Name, Nothing,
    OperationKind, PublicKey, Time, present,
};
use super::validate::{RuleViolation, done, validate_declaration};
use crate::provider_role::{Advertisement, Deployment, KeyPurpose, Statement, StatementShape};

/// The `shape` of a release assertion.
pub const RELEASE_ASSERTION_SHAPE: &str = "urn:rutent:module-interface:release-assertion";

/// The provider's signed assertion of one release (`urn:rutent:module-interface:release-assertion`): its
/// capability declaration and its two parts' portable binaries. It travels in the provider-role envelope, as a
/// [`Statement<ReleaseAssertion>`], made by the provider and signed with one of its advertised keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseAssertion {
    /// The release's capability declaration, whole (`REQ-MDL-3`).
    pub declaration: Declaration,
    /// The portable binary of each part (`REQ-MDL-1`).
    pub parts: Parts<BinaryPart>,
}

impl StatementShape for ReleaseAssertion {
    const SHAPE: &'static str = RELEASE_ASSERTION_SHAPE;
}

/// One value for each of a module's two parts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parts<T> {
    /// The sandboxed part's.
    pub sandboxed: T,
    /// The outside part's.
    pub outside: T,
}

/// A module's part, by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PartName {
    /// The sandboxed part.
    Sandboxed,
    /// The outside part.
    Outside,
}

impl<T> Parts<T> {
    /// Each part's value, with its name, sandboxed first.
    pub fn each(&self) -> [(PartName, &T); 2] {
        [
            (PartName::Sandboxed, &self.sandboxed),
            (PartName::Outside, &self.outside),
        ]
    }
}

/// One part's portable binary: its format and the digest of its bytes (`REQ-MDL-15`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BinaryPart {
    /// The format, by name; the contract names none (`DEC-4`).
    pub format: BinaryFormat,
    /// The digest of the binary's bytes.
    pub digest: Digest,
}

/// A digest of some bytes, in its one text form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Digest {
    /// The algorithm.
    pub algorithm: DigestAlgorithm,
    /// The value.
    pub value: DigestValue,
}

/// The digest algorithms allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DigestAlgorithm {
    /// SHA-256 (FIPS 180-4).
    #[serde(rename = "sha-256")]
    Sha256,
}

/// The platform's record of one admitted release (`urn:rutent:module-interface:admission-artefact`,
/// `REQ-MDL-1`, `REQ-MDL-2`, `REQ-MDL-3`). The platform writes it, not the module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionArtefact {
    /// The provider's signed release assertion, as received.
    pub assertion: Statement<ReleaseAssertion>,
    /// The platform's digest of the assertion as received, by which a later release's history names this one.
    pub assertion_digest: Digest,
    /// The digests the platform computed of the binaries it received.
    pub received: Parts<Digest>,
    /// The platform build that made the machine code (`REQ-MDL-2`).
    pub build: Build,
    /// The module's earlier admitted releases, oldest first.
    pub history: Vec<HistoryEntry>,
    /// How the capabilities changed since the release this one follows (`REQ-MDL-3`).
    pub capabilities: Capabilities,
    /// The acceptance the release requires, and whether it was given.
    pub acceptance: Acceptance,
}

impl AdmissionArtefact {
    /// The released declaration.
    pub fn declaration(&self) -> &Declaration {
        &self.assertion.statement.declaration
    }

    /// The history entry that names this release, once it is accepted; `None` until then.
    pub fn as_history_entry(&self) -> Option<HistoryEntry> {
        self.acceptance
            .accepted
            .as_ref()
            .map(|accepted| HistoryEntry {
                version: self.declaration().version.clone(),
                assertion_digest: self.assertion_digest.clone(),
                admitted_at: accepted.at.clone(),
                statement_key: self.declaration().outside.statement_key.clone(),
            })
    }
}

/// The platform build that made a release's machine code, so what runs can be traced and re-derived
/// (`REQ-MDL-2`, `THR-18`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Build {
    /// The exact platform build version, its compiler included.
    pub platform_version: Identifier,
    /// The digest of each part's machine code.
    pub machine_code: Parts<Digest>,
}

/// One earlier admitted release of a module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryEntry {
    /// Its version.
    pub version: Identifier,
    /// The platform's digest of its assertion.
    pub assertion_digest: Digest,
    /// When it was admitted.
    pub admitted_at: Time,
    /// The statement-signing key it declared. Its outside part could resolve that key, so no later release may be
    /// asserted with it ([`validate_release`]).
    pub statement_key: PublicKey,
}

/// How a release's capabilities changed since the one it follows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    /// The version of the release it follows; absent for a first release, and `null` is refused.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub compared_with: Option<Identifier>,
    /// Every widening, as [`widening`] computes them.
    pub widenings: Vec<Widening>,
}

/// One way a release widens the previous release's capabilities (`REQ-MDL-3`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Widening {
    /// An operation kind the sandboxed part newly handles, such as a module that starts answering withdrawals.
    Handles(HandlesWidening),
    /// A new outside operation.
    Operation(OperationWidening),
    /// A destination an existing outside operation gains.
    OperationDestination(OperationDestinationWidening),
    /// A secret an existing outside operation gains.
    OperationSecret(OperationSecretWidening),
    /// A new secret.
    Secret(SecretWidening),
    /// A new destination, or one of the same name at another scheme, host or port.
    Destination(DestinationWidening),
    /// A destination's addresses grown: from blocks to `public`, or by a block no single old block contains.
    Addresses(DestinationWidening),
    /// A new callee.
    Call(CallWidening),
    /// A new kind asked of an existing callee.
    CallKind(CallKindWidening),
    /// A changed statement-signing key.
    StatementKey(Nothing),
}

/// An operation kind the sandboxed part newly handles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandlesWidening {
    /// The kind it now handles.
    pub operation_kind: OperationKind,
}

/// A widening that names an outside operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationWidening {
    /// The operation.
    pub operation: Name,
}

/// A destination an existing operation gains.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationDestinationWidening {
    /// The operation.
    pub operation: Name,
    /// The destination it gains.
    pub destination: Name,
}

/// A secret an existing operation gains.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationSecretWidening {
    /// The operation.
    pub operation: Name,
    /// The secret it gains.
    pub secret: Name,
}

/// A widening that names a secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretWidening {
    /// The secret.
    pub secret: Name,
}

/// A widening that names a destination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DestinationWidening {
    /// The destination.
    pub destination: Name,
}

/// A widening that names a callee.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallWidening {
    /// The callee.
    pub callee: Identifier,
}

/// A new kind asked of an existing callee.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallKindWidening {
    /// The callee.
    pub callee: Identifier,
    /// The kind now asked of it.
    pub operation_kind: OperationKind,
}

/// The acceptance a release requires, and whether it was given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Acceptance {
    /// Automatic, or fresh.
    pub required: AcceptanceMode,
    /// Absent until accepted; `null` is refused.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub accepted: Option<Accepted>,
}

/// Which acceptance a release requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptanceMode {
    /// Nothing widened: acceptance may be automated.
    Automatic,
    /// Something widened, or the release is the first: someone must accept it afresh (`REQ-MDL-3`).
    Fresh,
}

/// A given acceptance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Accepted {
    /// Who accepted it; required for a fresh acceptance, and `null` is refused.
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub by: Option<Identifier>,
    /// When.
    pub at: Time,
}

/// Every way `next` widens `previous`'s capabilities, in the order of `next`'s lists (`REQ-MDL-3`):
///
/// 1. handled kinds: each operation kind the sandboxed part handles that it didn't, since a module that starts
///    answering, say, withdrawals reaches money it couldn't before;
/// 2. operations: a new operation; then, for an existing one, each destination and each secret it gains;
/// 3. secrets: each new secret;
/// 4. destinations: a new destination, or one of the same name at another scheme, host or port; then, for an
///    unchanged endpoint, addresses grown from blocks to `public`, or by a block no single old block contains;
/// 5. calls: a new callee; then, for an existing one, each new kind asked of it;
/// 6. a changed statement-signing key.
///
/// Narrowing is never listed: a kind no longer handled; a removed operation, secret, destination, call or kind; a
/// block within an old one;
/// `public` replaced by blocks, which can only be globally routable ones (`SpecialPurposeAddressBlock`). A block
/// covered only by the union of several old blocks is listed: containment is checked against single blocks,
/// which errs towards a fresh acceptance.
pub fn widening(previous: &Declaration, next: &Declaration) -> Vec<Widening> {
    let (old, new) = (&previous.outside, &next.outside);
    let mut out = Vec::new();
    for kind in &next.sandboxed.handles {
        if !previous.sandboxed.handles.contains(kind) {
            out.push(Widening::Handles(HandlesWidening {
                operation_kind: *kind,
            }));
        }
    }
    for operation in &new.operations {
        match old.operations.iter().find(|o| o.name == operation.name) {
            None => out.push(Widening::Operation(OperationWidening {
                operation: operation.name.clone(),
            })),
            Some(before) => {
                for destination in &operation.destinations {
                    if !before.destinations.contains(destination) {
                        out.push(Widening::OperationDestination(
                            OperationDestinationWidening {
                                operation: operation.name.clone(),
                                destination: destination.clone(),
                            },
                        ));
                    }
                }
                for secret in &operation.secrets {
                    if !before.secrets.contains(secret) {
                        out.push(Widening::OperationSecret(OperationSecretWidening {
                            operation: operation.name.clone(),
                            secret: secret.clone(),
                        }));
                    }
                }
            }
        }
    }
    for secret in &new.secrets {
        if !old.secrets.contains(secret) {
            out.push(Widening::Secret(SecretWidening {
                secret: secret.clone(),
            }));
        }
    }
    for destination in &new.destinations {
        let named = DestinationWidening {
            destination: destination.name.clone(),
        };
        match old.destinations.iter().find(|d| d.name == destination.name) {
            Some(before)
                if before.scheme == destination.scheme
                    && before.host == destination.host
                    && before.port == destination.port =>
            {
                if addresses_grow(&before.addresses, &destination.addresses) {
                    out.push(Widening::Addresses(named));
                }
            }
            _ => out.push(Widening::Destination(named)),
        }
    }
    for call in &new.calls {
        match old.calls.iter().find(|c| c.callee == call.callee) {
            None => out.push(Widening::Call(CallWidening {
                callee: call.callee.clone(),
            })),
            Some(before) => {
                for kind in &call.kinds {
                    if !before.kinds.contains(kind) {
                        out.push(Widening::CallKind(CallKindWidening {
                            callee: call.callee.clone(),
                            operation_kind: *kind,
                        }));
                    }
                }
            }
        }
    }
    if old.statement_key != new.statement_key {
        out.push(Widening::StatementKey(Nothing {}));
    }
    out
}

/// Whether `new` allows an address `old` doesn't.
fn addresses_grow(old: &Addresses, new: &Addresses) -> bool {
    match (old, new) {
        (Addresses::Public(_), _) => false,
        (Addresses::Blocks(_), Addresses::Public(_)) => true,
        (Addresses::Blocks(old), Addresses::Blocks(new)) => new
            .iter()
            .any(|block| !old.iter().any(|outer| contains(outer, block))),
    }
}

/// Whether `outer` contains every address of `inner`.
fn contains(outer: &AddressBlock, inner: &AddressBlock) -> bool {
    let bits = |ip: IpAddr| -> (u128, bool) {
        match ip {
            IpAddr::V4(v4) => (u128::from(u32::from(v4)) << 96, true),
            IpAddr::V6(v6) => (u128::from(v6), false),
        }
    };
    let ((outer_ip, outer_len), (inner_ip, inner_len)) = (outer.parts(), inner.parts());
    let ((outer_bits, outer_v4), (inner_bits, inner_v4)) = (bits(outer_ip), bits(inner_ip));
    if outer_v4 != inner_v4 || inner_len < outer_len {
        return false;
    }
    let mask = if outer_len == 0 {
        0
    } else {
        u128::MAX << (128 - u32::from(outer_len))
    };
    outer_bits & mask == inner_bits & mask
}

/// Checks an admission artefact's rules on its own, and returns every one broken:
///
/// - its assertion names the release-assertion shape;
/// - the released declaration passes [`validate_declaration`];
/// - each part's received digest is the one the assertion states, or the part was altered (`REQ-MDL-1`);
/// - its history never names one version twice, nor this release's;
/// - it compares with the last version its history names, and with none when its history is empty;
/// - a first release, with no history, lists no widening, and it and a widening release require a fresh acceptance, which once
///   given names who gave it (`REQ-MDL-3`). The schema holds these too.
pub fn validate_artefact(artefact: &AdmissionArtefact) -> Result<(), Vec<RuleViolation>> {
    let mut violations = Vec::new();
    if artefact.assertion.check_shape().is_err() {
        violations.push(RuleViolation::AssertionUnderAnotherShape);
    }
    if let Err(found) = validate_declaration(artefact.declaration()) {
        violations.extend(found);
    }
    let asserted = &artefact.assertion.statement.parts;
    for ((part, received), (_, binary)) in artefact.received.each().into_iter().zip(asserted.each())
    {
        if received != &binary.digest {
            violations.push(RuleViolation::PartAltered { part });
        }
    }
    let version = &artefact.declaration().version;
    for (i, entry) in artefact.history.iter().enumerate() {
        let repeated = artefact.history[..i]
            .iter()
            .any(|e| e.version == entry.version);
        if repeated || &entry.version == version {
            violations.push(RuleViolation::VersionAlreadyAdmitted { entry: i });
        }
    }
    let last_admitted = artefact.history.last().map(|entry| &entry.version);
    if artefact.capabilities.compared_with.as_ref() != last_admitted {
        violations.push(RuleViolation::ComparedWithNotLastAdmitted);
    }
    let fresh = artefact.acceptance.required == AcceptanceMode::Fresh;
    let first = artefact.history.is_empty();
    if first && !artefact.capabilities.widenings.is_empty() {
        violations.push(RuleViolation::FirstReleaseWithWidenings);
    }
    if first && !fresh {
        violations.push(RuleViolation::FirstReleaseNotFresh);
    }
    if !artefact.capabilities.widenings.is_empty() && !fresh {
        violations.push(RuleViolation::WidenedReleaseNotFresh);
    }
    if fresh && matches!(&artefact.acceptance.accepted, Some(a) if a.by.is_none()) {
        violations.push(RuleViolation::FreshAcceptanceByNoOne);
    }
    done(violations)
}

/// Checks a release assertion's maker and key against the provider's enveloped advertisement, and returns every
/// rule broken (`REQ-MDL-1`):
///
/// - the advertisement is the releasing provider's;
/// - the assertion's maker is that provider, so a publisher can't release another's module;
/// - it belongs to the advertisement's deployment, by name and operator key;
/// - its key is one of the advertisement's `keys`, listed for `advertising`, never for `statements` (`THR-4`);
/// - it isn't the deployment's `operator_key`, and isn't the declaration's `statement_key`, which the running
///   outside part can resolve;
/// - the declaration's `statement_key` is listed for `statements`, never for `advertising`, and isn't the
///   `operator_key`, so a release can't hand its outside part a key that signs advertisements or releases. This is
///   checked here, at admission, since [`validate_statement_key`](super::validate_statement_key) needs a signed
///   result, which a release doesn't have yet.
///
/// `advertisement` must be the one the catalogue (`F-012`) accepted for the provider, never one that arrives
/// with the release: an advertisement vouches for its own keys only through
/// [`validate_successor`](crate::provider_role::validate_successor) against the one before it, back to the
/// operator's grant (`REQ-CAT-2`).
///
/// It compares keys by their bytes only; verifying the signature is `M2` and `M3` work.
pub fn validate_assertion_key(
    artefact: &AdmissionArtefact,
    advertisement: &Statement<Advertisement>,
) -> Result<(), Vec<RuleViolation>> {
    let mut violations = Vec::new();
    let assertion = &artefact.assertion;
    let provider = artefact.declaration().provider.as_str();
    let key = &assertion.signature.key;
    if advertisement.statement.provider != provider {
        violations.push(RuleViolation::AdvertisementOfAnotherProvider);
    }
    if assertion.maker != provider {
        violations.push(RuleViolation::AssertionOfAnotherMaker);
    }
    if assertion.deployment != advertisement.deployment {
        violations.push(RuleViolation::AssertionOfAnotherDeployment);
    }
    match advertisement
        .statement
        .purpose_of_signer(&assertion.signature)
    {
        None => violations.push(RuleViolation::AssertionKeyNotAdvertised),
        Some(KeyPurpose::Statements) => {
            violations.push(RuleViolation::AssertionKeyNotForAdvertising);
        }
        Some(KeyPurpose::Advertising) => {}
    }
    if key == &advertisement.deployment.operator_key {
        violations.push(RuleViolation::AssertionKeyIsOperatorKey);
    }
    if key == &artefact.declaration().outside.statement_key {
        violations.push(RuleViolation::AssertionSignedWithStatementKey);
    }
    let statement_key = &artefact.declaration().outside.statement_key;
    match advertisement.statement.purpose_of(statement_key) {
        None => violations.push(RuleViolation::StatementKeyNotAdvertised),
        Some(KeyPurpose::Advertising) => {
            violations.push(RuleViolation::StatementKeyNotForStatements);
        }
        Some(KeyPurpose::Statements) => {}
    }
    if statement_key == &advertisement.deployment.operator_key {
        violations.push(RuleViolation::StatementKeyIsOperatorKey);
    }
    done(violations)
}

/// Checks the assertion of a module the operator publishes, such as a public node's holding account
/// (`REQ-CAT-23`), against the deployment the platform serves, and returns every rule broken (`REQ-MDL-1`, 4.0.0):
///
/// - it belongs to `deployment`, by name and operator key;
/// - it is signed with `deployment`'s `operator_key`: the operator's own release, which no provider advertisement
///   vouches for, so the key is the one the operator's deployment settings name;
/// - the declaration's `statement_key` isn't the `operator_key`, so the running outside part can't resolve the key
///   that signs the operator's statements and releases.
///
/// `deployment` must be the platform's own, from its deployment settings, never the one the assertion names. The
/// platform calls this only for a module its catalogue records as the operator's, and [`validate_assertion_key`]
/// for every other, which refuses an assertion signed with the `operator_key`. The `maker` is the operator's party,
/// which the deployment doesn't name, so the key is what binds the release to the operator, as
/// [`check_signed_by_operator`](crate::provider_role::Statement::check_signed_by_operator) binds an operation.
///
/// It compares keys by their bytes only; verifying the signature is the platform's.
pub fn validate_operator_assertion(
    artefact: &AdmissionArtefact,
    deployment: &Deployment,
) -> Result<(), Vec<RuleViolation>> {
    let mut violations = Vec::new();
    let assertion = &artefact.assertion;
    if &assertion.deployment != deployment {
        violations.push(RuleViolation::AssertionOfAnotherDeployment);
    }
    if assertion.signature.key != deployment.operator_key {
        violations.push(RuleViolation::OperatorAssertionNotSignedWithOperatorKey);
    }
    if artefact.declaration().outside.statement_key == deployment.operator_key {
        violations.push(RuleViolation::StatementKeyIsOperatorKey);
    }
    done(violations)
}

/// Checks a release against the admitted release it follows, and returns every rule broken (`REQ-MDL-1`,
/// `REQ-MDL-3`):
///
/// - both are one provider's module, of one deployment and protocol version;
/// - its assertion isn't signed with a statement-signing key any earlier release declared: the previous release's
///   outside part could resolve its key, so a compromised release could otherwise sign its successor and declare a
///   new key, which a fresh acceptance would show only as a rotation (`THR-4`);
/// - the previous release was accepted;
/// - the history is the previous one's, extended by the previous release, so the release compares with the
///   previous version ([`validate_artefact`]'s `ComparedWithNotLastAdmitted`);
/// - its recorded widenings are exactly those [`widening`] computes from the two declarations, so a widened
///   release can't be recorded as unchanged and pass as automatic.
///
/// It assumes both artefacts passed [`validate_artefact`].
pub fn validate_release(
    previous: &AdmissionArtefact,
    next: &AdmissionArtefact,
) -> Result<(), Vec<RuleViolation>> {
    let mut violations = Vec::new();
    let (before, after) = (previous.declaration(), next.declaration());
    if before.provider != after.provider {
        violations.push(RuleViolation::ReleaseOfAnotherModule);
    }
    if next.assertion.deployment != previous.assertion.deployment {
        violations.push(RuleViolation::ReleaseOfAnotherDeployment);
    }
    if next.assertion.protocol_version != previous.assertion.protocol_version {
        violations.push(RuleViolation::ReleaseUnderAnotherProtocolVersion);
    }
    let key = &next.assertion.signature.key;
    let declared_before = core::iter::once(&before.outside.statement_key)
        .chain(previous.history.iter().map(|entry| &entry.statement_key))
        .chain(next.history.iter().map(|entry| &entry.statement_key));
    if declared_before.into_iter().any(|k| k == key) {
        violations.push(RuleViolation::AssertionSignedWithPreviousStatementKey);
    }
    match previous.as_history_entry() {
        None => violations.push(RuleViolation::PreviousNotAdmitted),
        Some(entry) => {
            let mut expected = previous.history.clone();
            expected.push(entry);
            if next.history != expected {
                violations.push(RuleViolation::HistoryNotExtended);
            }
        }
    }
    if next.capabilities.widenings != widening(before, after) {
        violations.push(RuleViolation::WideningsMisrecorded);
    }
    done(violations)
}
