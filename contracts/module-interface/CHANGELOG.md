# module-interface changelog

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
