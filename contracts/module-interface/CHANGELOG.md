# module-interface changelog

## 4.0.0
- Breaking: the refusal reasons `M2`'s tasks found missing, so no refusal stands in another's name. A cross-module
  call refused because its caller reached a limit on its calls is `limit_reached`, and one refused because too little
  of the caller's deadline is left is `deadline_too_near`, where both were `callee_unavailable`. An outbound call
  whose peer didn't prove over TLS that it is the declared host is refused as `peer_not_verified`, where it was
  `undeclared_destination`. A secret lookup the store couldn't answer is refused as `store_unavailable`, never
  `not_found`. `AdmissionRefusal` gains `AdmissionPending` (a new release while another waits for its acceptance)
  and `AssertionUnverified` (a signed assertion that doesn't verify). Each wire reason is in the schema's enum and
  the Rust type, with valid and invalid fixtures; the two admission reasons are Rust only, as 2.3.0's were, with a
  Rust test. (DEC-4, DEC-20, T-010-09)
- No outcome that is only unknown reaches anyone as a refusal (`REQ-ORD-1`). The calling module is told a
  cross-module call's outcome as it is recorded, `delivered`, `unanswered` or `refused`, so an unanswered call,
  whose callee may have acted, is never told to its caller as refused; the README says so, and a format binding
  gives its caller all three. `PlatformRefusal` gains `StatementKeyUnavailable` (the statement key the running
  release declares couldn't be had, or isn't that key's, judged before the module is handed the operation, so
  nothing of it ran; a key store that can't answer was `Unavailable`, judged after the module ran) and
  `OutcomeUnknown` (the module passed something out, an outbound or cross-module call, and then gave no answer the
  platform can pass on; this was `Unavailable`). `Unavailable` now says nothing took effect, and the README and the
  enum name the reasons that do. Both are Rust only, as the other platform reasons are, with a Rust test. New Rust
  variants of a `#[non_exhaustive]` enum, and 4.0.0 is not released, so they join this major. (DEC-4, DEC-20)
- The release lifecycle's refusals (`T-013-06`). `AdmissionRefusal` gains `ReleaseStaged` (a new release while
  another is staged on a share of the provider's traffic, ADR-0014) and `OlderRelease` (a release whose assertion
  was made before the provider's last accepted one, so publishing an older signed release never reinstates it,
  `THR-17`). `PlatformRefusal` gains `DuplicateTransaction` (a transaction identifier a segment at another provider
  holds, or an ended segment held, `REQ-MDL-14`, `INV-8`) and `VersionExploited` (a segment on a version declared
  actively exploited, `REQ-MDL-20`). `VersionAlreadyAdmitted` also names a version withdrawn while it waited. All
  four are Rust only, as the other platform reasons are, with a Rust test.
  Staging, stale marks and segment records stay `CMP-PROV`'s, not shaped here. New Rust variants of
  `#[non_exhaustive]` enums, and 4.0.0 is not released, so they join this major. (DEC-4, DEC-20, T-013-06)
- A new refusal reason, `request_not_allowed`: an outbound call whose request the platform won't send whatever the
  destination, since its method isn't allowed, its target isn't a path or a header names the host or frames the
  message, is recorded as refused with it, so such an attempt leaves an audit record (`REQ-MDL-8`). The record holds
  none of the request. In the schema's enum, `RefusalReason::RequestNotAllowed` (a variant, breaking an exhaustive
  match, which joins this major), and a valid fixture. (DEC-4, DEC-20, T-013-07)
- `OperationKind` gains `HoldingBalance`, the provider-role operation kind `holding_balance` (provider-role 3.0.0),
  which a holding account's module declares it handles. The declaration schema takes its kinds from the provider-role
  operation schema, so it accepts the new kind with no change of its own; the Rust enum gains a variant, which breaks an
  exhaustive match, and 4.0.0 is not released, so the variant joins this major. A declaration's `calls` may not list
  it: it is the operator's own question to a holding account, and a module asking it of another would read an
  account's total, which for a node holding one order is that order's balance. `validate_declaration` refuses it
  (`DeclaredCallOfHoldingBalance`, with an invalid fixture), and the platform's call judgement refuses it however it
  is named. (DEC-4, DEC-20, T-016-02)
- A module the operator publishes, such as a public node's holding account (`REQ-CAT-23`), is admitted on the
  operator's own release, with no provider advertisement: `validate_operator_assertion(artefact, deployment)` checks
  that its assertion belongs to the deployment the platform serves, is signed with that deployment's `operator_key`
  (`OperatorAssertionNotSignedWithOperatorKey`, a new `RuleViolation`), and declares a `statement_key` other than the
  `operator_key`. The deployment is the platform's own, from its settings, never the one the assertion names. A new
  `operator-artefact` bundle (the deployment and the artefact) has a valid fixture and three invalid ones. A new Rust
  variant breaks an exhaustive match, and 4.0.0 is not released, so it joins this major. (DEC-4, DEC-20, T-016-04)
- `validate_record` accepts an `unknown` call whose last hop matched a declared destination and connected to
  nothing: its dial failed, so the hop names no address. An unconnected hop before the last, and an `unknown` call
  ending at a hop that matched no destination, stay refused (`HopNotConnected`). (DEC-4, DEC-20, T-010-09)
- `RefusalReason`, `SecretRefusalReason`, `CallRefusalReason`, `AdmissionRefusal` and `PlatformRefusal` are
  `#[non_exhaustive]`, so a Rust caller's match has a wildcard arm and a later reason doesn't break its build. A new
  value on the wire is still a major (`DEC-4`): a reader on an older schema rejects it. New Rust variants and the
  attribute both break an exhaustive match, so this is a major. Fixtures keep naming interface 2.0.1: nothing
  compares it. (DEC-4, DEC-20, T-010-09)

## 3.0.0
- Breaking: a cross-module call is recorded as `delivered` only when its callee answered. A call the platform
  passed on and got no answer to, its callee stopped, killed at a deadline or answering unreadably, is the new
  outcome `unanswered`, whose effect is not known, in the schema and as `CrossModuleOutcome::Unanswered`. Before,
  `delivered` meant passed on, so a record overstated delivery (`EXP-2` outcome, follow-up rows; `REQ-MDL-21`). A
  2.x record of `delivered` for a call never answered is untrue now, and a new Rust variant breaks an exhaustive
  match, so this is a major. `validate_call_against` holds an unanswered call to its declaration as a delivered one
  (`UndeclaredCallDelivered`). Fixtures keep naming interface 2.0.1, as T-010-01's bump left them: nothing compares
  it. (DEC-4, DEC-20, DEC-27, T-010-03)

## 2.3.0
- A platform's refusals are the contract's, with closer reasons: `AdmissionRefusal` gains `DisallowedImport` (a
  part importing what the binding doesn't offer) and `VersionAlreadyAdmitted` (a version admitted or waiting),
  and `PlatformRefusal` gains `LimitReached` (a part stopped by a limit before it answered, its module still
  running), where `EXP-2`'s bindings gave `UnreadableBinary` and `Unavailable`. Both enums move here from the
  module-boundary suite, which re-exports them unchanged in name. Minor: types added to the contract, no schema
  or rule changes. (DEC-4, DEC-20, DEC-27, T-010-03)

## 2.2.0
- `validate_call_against` accepts the platform's `undeclared_kind` refusal of a call whose callee and forwarded
  kind are declared: the caller's code named another kind, which the platform refuses without passing anything
  on, and the record names the kind forwarded (`EXP-2` outcome, follow-up rows; F-008's notes). An
  `undeclared_callee` refusal of a declared callee stays untrue. A valid
  `declared-cross-module-call-refused-as-an-undeclared-kind-its-caller-named` fixture. Minor: it accepts a record
  it refused, and refuses nothing it accepted. (DEC-4, DEC-20, DEC-27, T-010-03)

## 2.1.0
- The Rust code behind this contract (`contracts/src/module_interface`) is `no_std` with `alloc`:
  it uses `core` and `alloc` paths, and the crate's
  default `std` feature turns on `std` only for its dependencies and tests. Minor, not major: no schema, Rust type or
  behaviour changes, and every consumer that builds with default features builds unchanged. Fixtures keep naming
  their current version. (DEC-4, DEC-20, DEC-26, T-010-01)

## 2.0.1
- No change to this contract's shapes or types: its fixtures name provider-role 2.0.1 as their `protocol_version` and
  this version as their `interface`, after provider-role's fix to its newer-than check. (DEC-4, DEC-20, T-002-03)

## 2.0.0
- Breaking, after the M1 security review, following provider-role 2.0.0's key purposes: `validate_statement_key`
  refuses a `statement_key` the advertisement lists for `advertising` (`StatementKeyNotForStatements`), and
  `validate_assertion_key` a release assertion signed with a key it lists for `statements`
  (`AssertionKeyNotForAdvertising`). At admission `validate_assertion_key` also checks the declared `statement_key`
  against the advertisement (`StatementKeyNotAdvertised`, `StatementKeyNotForStatements`,
  `StatementKeyIsOperatorKey`), since a release has no signed result yet. Both checks are called with the
  advertisement the catalogue accepted, never one that arrives with the release. Keys are provider-role's `PublicKey`, compared by their bytes; a
  non-canonical alias of the statement key has its own fixture. (DEC-4, DEC-20, T-002-03)
- Breaking: a kind the sandboxed part newly handles is a widening, `handles` with its `operation_kind`, so a
  release that starts answering, say, withdrawals takes a fresh acceptance; a release recorded without it is
  `WideningsMisrecorded`. (DEC-4, DEC-20, T-002-03)
- Breaking: the special-purpose ranges add the 6to4 relay anycast prefix `192.88.99.0/24` and the SRv6 SID
  block `5f00::/16`. Fixtures name interface and protocol version 2.0.0. (DEC-4, DEC-20, T-002-03)

## 1.0.0
- Frozen at C1: from here a change needs a version bump, a changelog entry naming a DEC-*, and the owner's click. (DEC-3, DEC-4, DEC-20, T-006-01)

## 0.4.0
- A cross-module call's record now states its outcome, required: `delivered`, or `refused` with the callee, the
  kind asked and the reason, `undeclared_callee`, `undeclared_kind`, `not_named_by_route` or `callee_unavailable`,
  so a refused call is recordable in contract terms. (DEC-4, T-003-03)
- `validate_call` checks that a refusal names the call's own callee and kind, and `validate_call_against` checks a
  call's outcome against its caller's declaration, with `declared-cross-module-call-` fixtures. (DEC-4, T-003-03)

## 0.3.0
- Module admission: the provider's signed release assertion, in the provider-role envelope under the JCS and
  Ed25519 signing rule, with the release's declaration and each part's portable binary by format and SHA-256
  digest; and the platform's admission artefact, with the received digests, the platform build version and
  machine-code digests, the version history, the widenings and the acceptance. (DEC-4, T-003-02)
- `widening` computes a release's widenings from two declarations, narrowing never marked; a widening release and
  a first release, one with no history, require a fresh acceptance, which names who gave it; a release compares
  with the last version its history names. (DEC-4, T-003-02)
- `validate_artefact`, `validate_assertion_key` and `validate_release` refuse an altered part, an assertion not the
  provider's or signed with a key it doesn't advertise, the operator's key or the statement-signing key, and a
  release that doesn't follow an accepted release of the same module or misrecords its widenings, with
  `advertised-artefact-` and `successive-releases-` fixtures. (DEC-4, T-003-02)
- After review: a history entry records the statement-signing key its release declared, and `validate_release`
  refuses an assertion signed with any statement-signing key an earlier release declared, and a release of
  another deployment or protocol version than the one it follows. (DEC-4, T-003-02)

## 0.2.0
- A module may handle, and a call may ask, provider-role 0.4.0's two new kinds: `reversal`, an exchange's or
  ramp's true undo of a segment (REQ-CAT-15), and `reversal_confirmation`, a reserve's signed word that one
  reversed credit arrived (ADR-0008). The schemas take the kind list from provider-role by `$ref`; the Rust
  `OperationKind` and the cross-module call's callee and transaction now cover both. (DEC-4, DEC-14, T-002-03)

## 0.1.0
- The module interface split: the sandboxed part's and the outside part's declarations, what crosses between
  them, destination declarations, secret lookups by name from the outside part only, the outbound call record and
  declared cross-module calls. (DEC-4, DEC-9, T-003-01)
- Signing a result, as ruled: the module never holds a key; the outside part signs the sandboxed part's result
  with the statement-signing key it declares in the required `outside.statement_key`, resolving the private half
  through the credential proxy by the deployment's name (the deployment `{name, operator_key}` of the enveloped
  advertisement), over the RFC 8785 (JCS) canonical envelope with only the top-level `signature.value` removed.
  `validate_statement_key` checks that a signed result's `signature.key` is the declared key, that the result
  belongs to the advertisement's deployment, that the key is one of the advertisement's `keys`, and that it isn't
  the `operator_key`; `signed-result-` fixtures bundle the three values. (DEC-4, T-003-01)
- After review: a secret lookup names the outside operation it serves, and its answer echoes it;
  `validate_lookup` refuses a lookup whose operation is undeclared or doesn't declare the secret, with
  `declared-secret-lookup-` fixtures bundling a declaration and a lookup. A resolved secret's value never prints
  through `Debug`. (DEC-4, DEC-9, T-003-01)
- After review: `validate_statement_key` also refuses a result made by another party than the declaring provider,
  and `statement_key` is checked in Rust as a canonical Ed25519 public key. (DEC-4, T-003-01)
- After review: a pinned address block may not overlap a special-purpose range; an address is in its one text
  form (RFC 5952 for IPv6, never IPv4-mapped), the IPv6 patterns admit at most one `::`, and a hop's optional
  fields refuse `null`. The fixtures' pinned blocks moved off the documentation ranges. A call declared twice has
  its own fixture. (DEC-4, T-003-01)
- After re-review: the special-purpose ranges add the NAT64 well-known prefix `64:ff9b::/96`, Teredo, IPv6
  benchmarking and site-local; `validate_record_secrets` refuses an outbound call record naming a secret its
  operation doesn't declare, with `declared-outbound-call-record-` fixtures. (DEC-4, T-003-01)
