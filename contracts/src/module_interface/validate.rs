//! The rules JSON Schema can't express: they compare a name across lists or entries, or two fields. A value is
//! valid when it passes its schema and its check here returns `Ok`.

use alloc::{borrow::ToOwned, string::String, vec, vec::Vec};
use core::net::IpAddr;

use super::admission::PartName;
use super::types::{
    Addresses, CallOutcome, CallRefusalReason, CrossModuleCall, CrossModuleOutcome, Declaration,
    Hop, OperationKind, OutboundCallRecord, SecretLookup,
};
use crate::provider_role::{Advertisement, KeyPurpose, ResultBody, Statement};

/// A rule a module-interface value breaks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleViolation {
    /// An outside operation names a destination the outside part doesn't declare (`REQ-MDL-7`).
    UndeclaredDestination {
        /// Index into `outside.operations`.
        operation: usize,
        /// The name it gives.
        destination: String,
    },
    /// An outside operation names a secret the outside part doesn't declare (`REQ-MDL-6`).
    UndeclaredSecret {
        /// Index into `outside.operations`.
        operation: usize,
        /// The name it gives.
        secret: String,
    },
    /// Two entries of one list share a name, so a reference to it would be ambiguous.
    DeclaredTwice {
        /// The list.
        list: DeclaredList,
        /// Index of the repeat.
        index: usize,
        /// Index of the first entry with the name.
        first: usize,
    },
    /// A declared call names the module's own provider as callee: it isn't a call to another provider's module
    /// (`REQ-MDL-21`).
    DeclaredCallToOwnProvider {
        /// Index into `outside.calls`.
        call: usize,
    },
    /// A declared call lists `holding_balance` among its kinds. It is the operator's own question to a holding
    /// account, so a module may not ask it of another module: only the operator's audit reads an account's balance
    /// (`REQ-AUD-15`).
    DeclaredCallOfHoldingBalance {
        /// Index into `outside.calls`.
        call: usize,
    },
    /// A pinned address block that overlaps a special-purpose range ([`SPECIAL_PURPOSE`]), so a connection could
    /// land on the platform's own network or nowhere routable (`REQ-MDL-7`, ADR-0004).
    SpecialPurposeAddressBlock {
        /// Index into `outside.destinations`.
        destination: usize,
        /// Index into that destination's blocks.
        block: usize,
    },
    /// A secret lookup names an outside operation the module doesn't declare (`REQ-MDL-8`).
    LookupForUndeclaredOperation,
    /// A secret lookup names a secret its outside operation doesn't declare, though another may (`REQ-MDL-3`,
    /// `REQ-MDL-6`).
    SecretUndeclaredForOperation,
    /// An outbound call record checked against a declaration of another module: its provider or module
    /// version differs from the declaration's.
    RecordOfAnotherModule,
    /// An outbound call record names a secret its outside operation doesn't declare (`REQ-MDL-6`, `REQ-MDL-8`).
    RecordedSecretUndeclaredForOperation {
        /// Index into the record's `secrets`.
        secret: usize,
    },
    /// A refused cross-module call whose refusal names another callee or kind than its operation.
    RefusalNamesAnotherCall,
    /// A cross-module call checked against a declaration of another module: its caller or caller version
    /// differs from the declaration's.
    CallOfAnotherModule,
    /// A cross-module call recorded as passed on, delivered or unanswered, though its caller doesn't declare the
    /// callee and kind (`REQ-MDL-21`).
    UndeclaredCallDelivered,
    /// A cross-module call refused for a reason its caller's declaration contradicts: the first check the call
    /// fails names the reason.
    RefusalReasonUntrue,
    /// A cross-module call whose callee is its caller (`REQ-MDL-21`).
    CallToOwnProvider,
    /// A cross-module call whose operation names another transaction than the call (`REQ-MDL-8`).
    TransactionMismatch,
    /// A hop that didn't match a declared destination or didn't connect, before the last hop, or as the last hop
    /// of a call that was neither refused nor `unknown` after a failed dial to a declared destination (ADR-0004:
    /// every hop is checked, and a failed check ends the call).
    HopNotConnected {
        /// Index into `hops`.
        hop: usize,
    },
    /// The advertisement a statement-signing key is checked against is another provider's than the
    /// declaration's.
    AdvertisementOfAnotherProvider,
    /// The declared statement-signing key isn't one of the keys the provider's advertisement publishes
    /// (`REQ-CAT-2`).
    StatementKeyNotAdvertised,
    /// The declared statement-signing key is the deployment's `operator_key`, which signs only the operator's
    /// statements (`THR-8`).
    StatementKeyIsOperatorKey,
    /// The declared statement-signing key is listed in the advertisement for `advertising`, not `statements`:
    /// the outside part resolves it, so it could sign an advertisement or a release assertion (`THR-4`).
    StatementKeyNotForStatements,
    /// A signed result names another key than the declared statement-signing key (`REQ-MDL-3`).
    ResultNotSignedWithStatementKey,
    /// A signed result names another maker than the declaring provider, so the provider's key would vouch for
    /// another party's statement (`REQ-AUD-2`).
    ResultOfAnotherMaker,
    /// An admission artefact's assertion names another shape than a release assertion's.
    AssertionUnderAnotherShape,
    /// A part's received digest differs from the one the assertion states: the binary was altered
    /// (`REQ-MDL-1`).
    PartAltered {
        /// The part.
        part: PartName,
    },
    /// A history entry names a version an earlier entry names, or this release's (`REQ-MDL-1`).
    VersionAlreadyAdmitted {
        /// Index into `history`.
        entry: usize,
    },
    /// A first release lists widenings, though it compares with nothing.
    FirstReleaseWithWidenings,
    /// A first release doesn't require a fresh acceptance (`REQ-MDL-3`).
    FirstReleaseNotFresh,
    /// A release that widens its capabilities doesn't require a fresh acceptance (`REQ-MDL-3`, `AC-21`).
    WidenedReleaseNotFresh,
    /// A fresh acceptance given by no one named.
    FreshAcceptanceByNoOne,
    /// A release assertion made by another party than the releasing provider, so one publisher could release
    /// another's module (`REQ-MDL-1`).
    AssertionOfAnotherMaker,
    /// A release assertion of another deployment than the advertisement's (`REQ-PRT-1`).
    AssertionOfAnotherDeployment,
    /// A release assertion signed with a key the provider's advertisement doesn't publish (`REQ-MDL-1`).
    AssertionKeyNotAdvertised,
    /// A release assertion signed with the deployment's `operator_key` (`THR-8`).
    AssertionKeyIsOperatorKey,
    /// The assertion of a module the operator publishes isn't signed with the `operator_key` of the deployment
    /// the platform serves (`REQ-MDL-1`, 4.0.0).
    OperatorAssertionNotSignedWithOperatorKey,
    /// A release assertion signed with a key the advertisement lists for `statements`, which a module's outside
    /// part can resolve, so running code could sign a release (`THR-4`).
    AssertionKeyNotForAdvertising,
    /// A release assertion signed with the declaration's statement-signing key, which the running outside part
    /// can resolve, so running code could sign its own next release (`THR-4`). Since 2.0.0 it never fails alone:
    /// a statement key listed for `statements` also fails `AssertionKeyNotForAdvertising`, one listed for
    /// `advertising` fails `StatementKeyNotForStatements`, and an unlisted one fails `AssertionKeyNotAdvertised`.
    /// It stays as a second line.
    AssertionSignedWithStatementKey,
    /// A release that follows another provider's module (`REQ-MDL-1`).
    ReleaseOfAnotherModule,
    /// A release asserted in another deployment than the release it follows (`REQ-PRT-1`).
    ReleaseOfAnotherDeployment,
    /// A release asserted under another protocol version than the release it follows (`REQ-PRT-1`).
    ReleaseUnderAnotherProtocolVersion,
    /// A release asserted with a statement-signing key an earlier release declared, which that release's outside
    /// part could resolve (`THR-4`).
    AssertionSignedWithPreviousStatementKey,
    /// A release that follows one not yet accepted.
    PreviousNotAdmitted,
    /// A release whose history isn't the previous release's, extended by that release.
    HistoryNotExtended,
    /// An admission artefact that compares with another version than the last its history names, or with one
    /// though its history is empty, or with none though it has a history.
    ComparedWithNotLastAdmitted,
    /// A release whose recorded widenings aren't the ones its declaration and the previous one's give
    /// (`REQ-MDL-3`).
    WideningsMisrecorded,
    /// A signed result belongs to another deployment than the advertisement's, by name or operator key, so the
    /// statement-signing key wasn't checked against the deployment it was resolved for (`REQ-PRT-1`, `THR-8`).
    ResultOfAnotherDeployment,
}

/// A list of a declaration's outside part whose entries are keyed by name or callee.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclaredList {
    /// `outside.operations`, by name.
    Operations,
    /// `outside.destinations`, by name.
    Destinations,
    /// `outside.calls`, by callee.
    Calls,
}

/// The special-purpose ranges no pinned address block may overlap, as `(address, prefix length)`. From the IANA
/// IPv4 and IPv6 special-purpose address registries:
///
/// - **unspecified and "this network":** `0.0.0.0/8`, `::/128`;
/// - **loopback:** `127.0.0.0/8`, `::1/128`;
/// - **private:** `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`;
/// - **shared address space (CGNAT):** `100.64.0.0/10`;
/// - **link-local:** `169.254.0.0/16`, `fe80::/10`, which hold cloud metadata endpoints such as
///   `169.254.169.254`;
/// - **IETF protocol assignments:** `192.0.0.0/24`;
/// - **documentation:** `192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24`, `2001:db8::/32`, `3fff::/20`;
/// - **benchmarking:** `198.18.0.0/15`;
/// - **multicast:** `224.0.0.0/4`, `ff00::/8`;
/// - **reserved, and the limited broadcast in it:** `240.0.0.0/4`;
/// - **IPv4-mapped and IPv4-compatible:** `::ffff:0:0/96`, `::/96`;
/// - **NAT64:** the well-known prefix `64:ff9b::/96`, which embeds an IPv4 address and so could reach
///   `169.254.169.254` as `64:ff9b::a9fe:a9fe`; RFC 6052 §3.1 forbids it for non-global IPv4, so nothing
///   legitimate is lost. Also the local-use prefix `64:ff9b:1::/48`;
/// - **discard:** `100::/64`;
/// - **in the IETF protocol assignments `2001::/23`:** Teredo `2001::/32` and benchmarking `2001:2::/48`. The
///   `/23` isn't listed whole, because some assignments in it are globally reachable: the anycast addresses
///   `2001:1::1/128` to `2001:1::3/128`, AMT `2001:3::/32`, AS112-v6 `2001:4:112::/48` and ORCHIDv2
///   `2001:20::/28`;
/// - **6to4:** `2002::/16`, and the deprecated 6to4 relay anycast prefix `192.88.99.0/24` (RFC 7526);
/// - **SRv6 SIDs:** `5f00::/16` (RFC 9602), segment identifiers that address functions inside a provider's network,
///   not hosts;
/// - **unique-local and the deprecated site-local:** `fc00::/7`, `fec0::/10`.
pub const SPECIAL_PURPOSE: &[(&str, u8)] = &[
    ("0.0.0.0", 8),
    ("10.0.0.0", 8),
    ("100.64.0.0", 10),
    ("127.0.0.0", 8),
    ("169.254.0.0", 16),
    ("172.16.0.0", 12),
    ("192.0.0.0", 24),
    ("192.0.2.0", 24),
    ("192.88.99.0", 24),
    ("192.168.0.0", 16),
    ("198.18.0.0", 15),
    ("198.51.100.0", 24),
    ("203.0.113.0", 24),
    ("224.0.0.0", 4),
    ("240.0.0.0", 4),
    ("::", 128),
    ("::1", 128),
    ("::", 96),
    ("::ffff:0:0", 96),
    ("64:ff9b::", 96),
    ("64:ff9b:1::", 48),
    ("100::", 64),
    ("2001::", 32),
    ("2001:2::", 48),
    ("2001:db8::", 32),
    ("2002::", 16),
    ("3fff::", 20),
    ("5f00::", 16),
    ("fc00::", 7),
    ("fe80::", 10),
    ("fec0::", 10),
    ("ff00::", 8),
];

/// Whether the block `ip/len` shares an address with a special-purpose range: two prefixes overlap when they
/// agree on the shorter one's bits, host bits ignored.
fn overlaps_special_purpose(ip: IpAddr, len: u8) -> bool {
    let bits = |ip: IpAddr| -> (u128, u32) {
        match ip {
            IpAddr::V4(v4) => (u128::from(u32::from(v4)) << 96, 32),
            IpAddr::V6(v6) => (u128::from(v6), 128),
        }
    };
    let (block, width) = bits(ip);
    SPECIAL_PURPOSE.iter().any(|(range, range_len)| {
        let range: IpAddr = range.parse().expect("a listed range parses");
        let (range, range_width) = bits(range);
        if range_width != width {
            return false;
        }
        let shorter = u32::from(len.min(*range_len));
        let mask = if shorter == 0 {
            0
        } else {
            u128::MAX << (128 - shorter)
        };
        block & mask == range & mask
    })
}

/// Checks a declaration's rules the schema can't express, and returns every one broken:
///
/// - every destination and secret an outside operation names is declared by the outside part;
/// - no two operations, destinations or calls share a name or callee;
/// - no declared call is to the module's own provider;
/// - no pinned address block overlaps a special-purpose range ([`SPECIAL_PURPOSE`]).
pub fn validate_declaration(declaration: &Declaration) -> Result<(), Vec<RuleViolation>> {
    let mut violations = Vec::new();
    let outside = &declaration.outside;
    for (d, destination) in outside.destinations.iter().enumerate() {
        if let Addresses::Blocks(blocks) = &destination.addresses {
            for (b, block) in blocks.iter().enumerate() {
                let (ip, len) = block.parts();
                if overlaps_special_purpose(ip, len) {
                    violations.push(RuleViolation::SpecialPurposeAddressBlock {
                        destination: d,
                        block: b,
                    });
                }
            }
        }
    }
    for (i, operation) in outside.operations.iter().enumerate() {
        for destination in &operation.destinations {
            if !outside.destinations.iter().any(|d| &d.name == destination) {
                violations.push(RuleViolation::UndeclaredDestination {
                    operation: i,
                    destination: destination.as_str().to_owned(),
                });
            }
        }
        for secret in &operation.secrets {
            if !outside.secrets.contains(secret) {
                violations.push(RuleViolation::UndeclaredSecret {
                    operation: i,
                    secret: secret.as_str().to_owned(),
                });
            }
        }
    }
    check_repeats(
        DeclaredList::Operations,
        outside.operations.iter().map(|o| o.name.as_str()),
        &mut violations,
    );
    check_repeats(
        DeclaredList::Destinations,
        outside.destinations.iter().map(|d| d.name.as_str()),
        &mut violations,
    );
    check_repeats(
        DeclaredList::Calls,
        outside.calls.iter().map(|c| c.callee.as_str()),
        &mut violations,
    );
    for (i, call) in outside.calls.iter().enumerate() {
        if call.callee == declaration.provider {
            violations.push(RuleViolation::DeclaredCallToOwnProvider { call: i });
        }
        if call.kinds.contains(&OperationKind::HoldingBalance) {
            violations.push(RuleViolation::DeclaredCallOfHoldingBalance { call: i });
        }
    }
    done(violations)
}

/// Checks an outbound call record's rule the schema can't express: every hop matched a declared destination
/// and connected, except the last hop of a refused call, and the last hop of an `unknown` call that matched a
/// declared destination and connected to nothing (its dial failed, so nothing was sent).
pub fn validate_record(record: &OutboundCallRecord) -> Result<(), Vec<RuleViolation>> {
    let last = record.hops.len().saturating_sub(1);
    let may_end_unconnected = |hop: &Hop| match record.outcome {
        CallOutcome::Refused(_) => true,
        CallOutcome::Unknown(_) => hop.destination.is_some(),
        _ => false,
    };
    let violations = record
        .hops
        .iter()
        .enumerate()
        .filter(|(i, hop)| !hop.connected() && !(*i == last && may_end_unconnected(hop)))
        .map(|(hop, _)| RuleViolation::HopNotConnected { hop })
        .collect();
    done(violations)
}

/// Checks a secret lookup against its module's declaration: the outside operation it names is declared, and
/// declares the secret it names. The proxy refuses a lookup that breaks either (`undeclared_secret`), so serving
/// one operation can't resolve a secret only another uses (`REQ-MDL-3`, `REQ-MDL-6`).
///
/// It assumes the declaration has passed [`validate_declaration`], so its operation names are unique.
pub fn validate_lookup(
    declaration: &Declaration,
    lookup: &SecretLookup,
) -> Result<(), Vec<RuleViolation>> {
    let operation = declaration
        .outside
        .operations
        .iter()
        .find(|o| o.name == lookup.operation);
    let violations = match operation {
        None => vec![RuleViolation::LookupForUndeclaredOperation],
        Some(o) if !o.secrets.contains(&lookup.name) => {
            vec![RuleViolation::SecretUndeclaredForOperation]
        }
        Some(_) => Vec::new(),
    };
    done(violations)
}

/// Checks an outbound call record against its module's declaration, and returns every rule broken:
///
/// - the record is the declared module's, by provider and module version;
/// - every secret it names is one its outside operation declares (`REQ-MDL-6`, `REQ-MDL-8`). A record of an
///   operation the module doesn't declare, refused as `undeclared_operation`, can therefore name no secret.
///
/// It assumes the declaration has passed [`validate_declaration`], so its operation names are unique.
pub fn validate_record_secrets(
    declaration: &Declaration,
    record: &OutboundCallRecord,
) -> Result<(), Vec<RuleViolation>> {
    let mut violations = Vec::new();
    if record.provider != declaration.provider || record.module_version != declaration.version {
        violations.push(RuleViolation::RecordOfAnotherModule);
    }
    let declared = declaration
        .outside
        .operations
        .iter()
        .find(|o| o.name == record.operation)
        .map(|o| o.secrets.as_slice())
        .unwrap_or_default();
    for (secret, name) in record.secrets.iter().enumerate() {
        if !declared.contains(name) {
            violations.push(RuleViolation::RecordedSecretUndeclaredForOperation { secret });
        }
    }
    done(violations)
}

/// Checks a cross-module call's rules the schema can't express: its callee isn't its caller, and its operation
/// names the call's transaction, unless it is a judgement question, which names none.
pub fn validate_call(call: &CrossModuleCall) -> Result<(), Vec<RuleViolation>> {
    let mut violations = Vec::new();
    if call.callee() == call.caller.as_str() {
        violations.push(RuleViolation::CallToOwnProvider);
    }
    if let Some(transaction) = call.operation_transaction()
        && transaction != call.transaction.as_str()
    {
        violations.push(RuleViolation::TransactionMismatch);
    }
    if let CrossModuleOutcome::Refused(refused) = &call.outcome
        && (refused.callee.as_str() != call.callee()
            || refused.kind != OperationKind::of(&call.operation))
    {
        violations.push(RuleViolation::RefusalNamesAnotherCall);
    }
    done(violations)
}

/// Checks a cross-module call's outcome against its caller's declaration, and returns every rule broken:
///
/// - the call is the declared module's, by provider and version;
/// - a call passed on, delivered or unanswered, has its callee and kind declared (`REQ-MDL-21`);
/// - a refused call's reason is the first check it fails, in the order the platform checks: an undeclared
///   callee, then an undeclared kind. A refusal because the route doesn't name the call, a limit was reached,
///   the callee isn't running or too little of the caller's deadline is left is for a call whose callee and kind
///   are declared.
/// - a call whose callee and kind are declared may still be refused as `undeclared_kind`: the record names the
///   kind of the operation forwarded, and the caller's own code may have named another, undeclared one, which
///   the platform refuses without passing anything on (`EXP-2`'s bindings). An `undeclared_callee` refusal of a
///   declared callee stays untrue, since the record names the callee the platform judged.
///
/// Whether the route names the call, and whether the callee runs, the platform holds at run time. It assumes the
/// declaration has passed [`validate_declaration`].
pub fn validate_call_against(
    declaration: &Declaration,
    call: &CrossModuleCall,
) -> Result<(), Vec<RuleViolation>> {
    let mut violations = Vec::new();
    if call.caller != declaration.provider || call.caller_version != declaration.version {
        violations.push(RuleViolation::CallOfAnotherModule);
    }
    let declared = declaration
        .outside
        .calls
        .iter()
        .find(|c| c.callee.as_str() == call.callee());
    let kind = OperationKind::of(&call.operation);
    let first_failure = match declared {
        None => Some(CallRefusalReason::UndeclaredCallee),
        Some(c) if !c.kinds.contains(&kind) => Some(CallRefusalReason::UndeclaredKind),
        Some(_) => None,
    };
    match (&call.outcome, first_failure) {
        (CrossModuleOutcome::Delivered(_) | CrossModuleOutcome::Unanswered(_), Some(_)) => {
            violations.push(RuleViolation::UndeclaredCallDelivered);
        }
        (CrossModuleOutcome::Refused(refused), Some(reason)) if refused.reason != reason => {
            violations.push(RuleViolation::RefusalReasonUntrue);
        }
        (CrossModuleOutcome::Refused(refused), None)
            if refused.reason == CallRefusalReason::UndeclaredCallee =>
        {
            violations.push(RuleViolation::RefusalReasonUntrue);
        }
        _ => {}
    }
    done(violations)
}

/// Checks a module's statement-signing key across three values, and returns every rule broken (`DEC-4`,
/// `T-003-01`):
///
/// - the advertisement is the declaring provider's own;
/// - the declared `statement_key` is one of the advertisement's `keys`, listed for `statements`, never for
///   `advertising`: the outside part resolves it, so it must not sign advertisements or release assertions
///   (`THR-4`);
/// - it isn't the `operator_key` of the deployment the enveloped advertisement names;
/// - the signed result belongs to that same deployment, its name and operator key both, so a caller needn't
///   relate the result to the advertisement first;
/// - the signed result's maker is the declaring provider;
/// - the signed result names it as its `signature.key`.
///
/// `advertisement` must be the one the catalogue (`F-012`) accepted for the provider. Keys compare by their
/// bytes. It compares keys and deployments only; verifying the signature is `M2` and `M3` work.
pub fn validate_statement_key(
    declaration: &Declaration,
    advertisement: &Statement<Advertisement>,
    result: &Statement<ResultBody>,
) -> Result<(), Vec<RuleViolation>> {
    let mut violations = Vec::new();
    let key = &declaration.outside.statement_key;
    if advertisement.statement.provider != declaration.provider.as_str() {
        violations.push(RuleViolation::AdvertisementOfAnotherProvider);
    }
    match advertisement.statement.purpose_of(key) {
        None => violations.push(RuleViolation::StatementKeyNotAdvertised),
        Some(KeyPurpose::Advertising) => {
            violations.push(RuleViolation::StatementKeyNotForStatements);
        }
        Some(KeyPurpose::Statements) => {}
    }
    if key == &advertisement.deployment.operator_key {
        violations.push(RuleViolation::StatementKeyIsOperatorKey);
    }
    if result.deployment != advertisement.deployment {
        violations.push(RuleViolation::ResultOfAnotherDeployment);
    }
    if result.maker != declaration.provider.as_str() {
        violations.push(RuleViolation::ResultOfAnotherMaker);
    }
    if &result.signature.key != key {
        violations.push(RuleViolation::ResultNotSignedWithStatementKey);
    }
    done(violations)
}

/// Reports each entry whose key an earlier entry of the same list already has.
fn check_repeats<'a>(
    list: DeclaredList,
    keys: impl Iterator<Item = &'a str>,
    violations: &mut Vec<RuleViolation>,
) {
    let keys: Vec<&str> = keys.collect();
    for (index, key) in keys.iter().enumerate() {
        if let Some(first) = keys[..index].iter().position(|earlier| earlier == key) {
            violations.push(RuleViolation::DeclaredTwice { list, index, first });
        }
    }
}

pub(super) fn done(violations: Vec<RuleViolation>) -> Result<(), Vec<RuleViolation>> {
    if violations.is_empty() {
        Ok(())
    } else {
        Err(violations)
    }
}
