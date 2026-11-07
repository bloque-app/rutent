# F-003 notes

These are the readings taken where `SPEC.md` is silent or ambiguous, recorded per Narthex `REQ-SKL-24`.

## T-003-01

- **What the sandboxed part declares.**
  - `REQ-MDL-16` splits "a module's interaction with the outside world" from the sandboxed part, and its test is
    "the sandboxed part has no outside reach".
  - So the sandboxed part declares one thing: the provider-role operation kinds it handles. It has no field for a
    destination, a secret or a call, and adding one is invalid, not ignored.
  - The kinds are referred to by the provider-role `$id`, not copied. A module can't invent a kind, and the
    interface stays the provider-role one across module versions (`REQ-MDL-19`).
- **What crosses the split.**
  - **Sandboxed to outside:** a request to perform a declared outside operation, for one transaction, with
    arguments.
  - **Outside to sandboxed:** that request's outcome.
  - The request names neither a destination nor a secret. `REQ-MDL-8` lets the outside part perform "only
    operations the provider's interface declares", so the declaration, not the request, fixes which destinations
    and secrets each operation may use. The sandboxed part then can't aim a call elsewhere (`REQ-MDL-7`) or ask for
    a secret (`REQ-MDL-6`, `THR-78`'s "asks the outside part for something beyond the declared operation").
- **The outside part's declaration is the capability declaration.**
  - `REQ-MDL-3`'s "capabilities it needs" is read as everything the outside part may do: operations, destinations,
    secret names and calls.
  - Every list is required, and empty means none. This follows the provider-role contract's `required_inputs`
    convention: a module can't leave a capability out and have it implied.
  - `T-003-02` carries this declaration in the admission artefact. Comparing two releases' declarations for
    widening (`REQ-MDL-3`, `AC-21`) is `T-003-02`'s.
- **The interface version.**
  - `REQ-MDL-19`'s "the provider interface MUST stay fixed across module versions" is read as: a declaration names
    the module's `version` and, separately, the `interface` it implements, which is this contract's version.
  - A schema sees one declaration, so "the same across versions" is a rule across values, for the platform and
    `T-003-02`.
- **Secrets.**
  - `REQ-MDL-17`'s "common interface to store and unseal secrets" is read, through ADR-0004 and `DEC-9`, as
    resolve-by-name at call time.
  - The lookup names the transaction, for `REQ-MDL-6`'s "attributable per transaction".
  - It names no provider. Under `DEC-9`, the shared proxy answers each lookup only for the calling process's own
    provider. The provider is known from the process (one OS process per provider, ADR-0003), so a field naming it
    could only be a way to ask for another provider's secret.
  - The answer carries the current value, so rotation needs no release.
  - Storing a secret is an operator action at deployment, not a module's. This contract has no shape for it; it
    belongs to `F-014`.
- **Destinations.** ADR-0004's "checked by name and connected address, re-checked on every redirect" is read as
  follows:
  - **Name:** the scheme, an exact host and a port. Wildcards and address literals are refused, so the name check
    is exact.
  - **Connected address:** the declaration states what the connected address may be, either `"public"` or pinned
    address blocks. The field is required, so the address check is never implied.
    - `"public"` is any globally routable address the host resolves to at call time, never a loopback, private,
      link-local or special-purpose one. That is what keeps an altered resolution (`REQ-MDL-7`'s test) from
      reaching the platform's own network.
    - Pinned blocks are there for providers that publish fixed ranges.
  - **TLS:** `https` and `wss` only. `THR-79` includes an attacker who "poses as the provider's endpoint". TLS
    binds the connection to the name, which a plain connection doesn't.
  - **Redirects:** each redirect is a new hop, checked as the first. It is followed only to a destination the same
    outside operation lists.
- **The outbound call record.**
  - **Who writes it.** The platform's egress layer, not the module. The outside part is the provider's own code
    (`THR-83`), so a record it wrote about itself would be the party's own statement. The record is therefore not
    a signed party statement in the provider-role envelope.
  - **What it holds.** `REQ-MDL-8` asks for "destination, version and transaction". It holds:
    - the transaction;
    - the provider and the module version;
    - the outside operation;
    - each hop, with the declared destination it matched and the address connected to;
    - the names of the secrets used, for `REQ-MDL-6`'s attribution;
    - the outcome.
  - **Refused calls** are recorded too, so an attempt to reach an undeclared destination leaves a trace.
  - **An unknown outcome** is its own status, as in the provider-role results: a call that got no answer may still
    have taken effect.
- **Cross-module calls.**
  - `REQ-MDL-21`: "a module MAY call another provider's module only where the route declares it, and each module
    MUST act on its own credentials".
  - **Where the declaration sits.** It is in the outside part's declaration, since one OS process per provider
    means a call to another module leaves the caller's process (ADR-0003).
  - **What is declared.** Each callee and the provider-role kinds that may be asked of it.
  - **The call:**
    - It names the route decision record and segment that declare it, so the platform can check the route names
      it (`THR-86`).
    - It carries the provider-role operation for the callee, so the callee answers it as any other operation.
      This keeps its interface fixed.
    - It carries no credentials. The callee acts on its own.
  - **Judgement questions.** A call asking a judgement names a transaction for the platform's record, but the
    judgement question itself still names none (`REQ-CPL-4`).
- **Where a result is signed (ruled by the orchestrator, 2026-11-06, `DEC-4`).**
  - **The question raised.** A provider-role result is the provider's signed statement (`REQ-QTE-1`,
    `REQ-AUD-2`), and SPEC doesn't say where the signature is made: in an outside-part step, or in a platform-held
    signing step (ADR-0004 rejects a host-side signing service, but for destination scoping).
  - **The ruling**, in its own words: "The module never holds a key. The outside part signs: it resolves the
    signing key by the deployment's name (operator_key in the enveloped advertisement) and signs over the JCS
    canonical envelope, with signature.value removed."
  - **Follow-up ruling** (the orchestrator, 2026-11-06), on the two readings below:
    - The first is confirmed. `operator_key` names which deployment the lookup is for; `signature.key` must be
      one of the advertisement's `keys`, never `operator_key`.
    - The second is reversed: signing is not an exception to the capability declaration. The declaration gets a
      required statement-signing key, and the suite checks a signed result against it, so `REQ-MDL-3` stays
      whole.
    - It is called the "statement-signing key" everywhere, apart from on-chain secrets such as
      `chain.signing_key`.
  - **What was stated.** The contract's README ("Signing a result"), the declaration and secret-lookup schema
    descriptions, the Rust types and module docs, and the changelog.
- **Readings taken while applying the signing rulings.**
  - **"Never holds a key"** is read as: no declaration, request, response or artefact of the module carries a
    private key, and the sandboxed part never sees one. The outside part resolves the private half at signing
    time and keeps it no longer, as for any other secret (`REQ-MDL-6`). The declaration carries only the public
    half.
  - **"(operator_key in the enveloped advertisement)"** names which deployment: the `deployment`
    `{name, operator_key}` that the enveloped advertisement carries. It is not the key signed with: under the
    provider-role contract a provider signs with one of its advertisement's `keys`, and the `operator_key` signs
    the operator's operations (`check_signed_by_operator`), so a provider holding it could forge operations.
    Confirmed by the follow-up ruling.
  - **The statement-signing key is declared.** It is `outside.statement_key`, in the outside part because the
    outside part signs. Required, like every outside list.
    - **"The advertisement key id".** The advertisement's `keys` entries are `{scheme, public_key}`, with no id.
      A key is therefore named by its public key, and `statement_key` is one (`urn:rutent:provider-role:common`'s
      `public_key`). A signed result's `signature.key` then equals it exactly.
    - It is not one of the declared `secrets`. Those are the provider's other credentials, which outside
      operations use; the statement-signing key signs rutent statements and no outside operation names it.
  - **The check spans three values**: the declaration, the provider's enveloped advertisement and a signed
    result. `validate_statement_key` checks:
    - the result's `signature.key` is the declared `statement_key`;
    - `statement_key` is one of the advertisement's `keys`;
    - `statement_key` isn't the `operator_key` of the advertisement's deployment.
    - **Beyond the first ruling, and confirmed:** it also checks that the advertisement is the declaring
      provider's. Without that, the second check could be met with another provider's advertisement.
    - **The result's deployment** (the orchestrator, 2026-11-06, the stricter option): it checks that the result
      belongs to the advertisement's deployment, by name and operator key both, with its own violation,
      `ResultOfAnotherDeployment`. The third check compares against the advertisement's `operator_key`, so
      without this a caller that skipped relating the result to the advertisement could accept a result whose
      own deployment's `operator_key` is the `statement_key`. The function now holds that step itself.
  - **The fixtures for it** are `signed-result-` bundles of the three values. Each member is checked against its
    own schema and its own contract's rules, so an invalid bundle breaks only the statement-key rule it names.
    - The operator-key case advertises the `operator_key` among the provider's `keys`. So it is the operator-key
      rule alone that rejects it, and an operator granting its own key to a provider is still refused.
  - **The lookup** has no wire shape. It is made by the deployment's name, at signing time, and the proxy's side
    is `F-014`'s.
  - **The version stays 0.1.0.** The contract is new on this branch, so the rulings are part of its first entry.

### T-003-01 review findings (the orchestrator, 2026-11-06)

An Opus review of the split found six defects and four nits, fixed in the same entry. The readings taken:

- **A secret is limited to its operation.** A lookup names the outside operation it serves, and the answer
  echoes it. `validate_lookup` relates a lookup to its declaration:
  - `SecretUndeclaredForOperation`: the operation doesn't declare the secret, though another may. This is the
    finding.
  - `LookupForUndeclaredOperation`: the operation isn't declared at all. This one is beyond the finding. It is
    added because the first check needs a declared operation to compare with.
  - The proxy refuses either with the existing `undeclared_secret` reason, broadened to "not declared, or not for
    the named operation". No new reason was added.
  - The rule left "Rules across values" for the Rust table, since a bundle fixture now tests it.
- **A result's maker** must be the declaring provider (`ResultOfAnotherMaker`). The other-provider bundle was
  re-isolated: its result's maker is now the declaring provider, so only the advertisement is wrong in it.
- **Redaction.** `SecretValue`'s `Debug` prints `SecretValue(<redacted>)`. The test checks `{:?}` and `{:#?}` of
  the answer, its outcome, the resolved value and the value itself. Its tag is `REQ-MDL-6`, since no test in the
  repository tags a `THR-*`; `THR-50` is named in its body.
- **`null` for an optional field.** The sweep found two `Option` fields, `Hop.destination` and `Hop.address`. Both
  now refuse `null` through a helper local to this module, the same idea as the state machine's `some_id` and
  `some_order`. The helper isn't shared, so this change stays inside one contract.
- **Special-purpose blocks.**
  - The ruling's categories are all there: loopback, link-local, private, CGNAT, multicast, unspecified,
    documentation, IPv4-mapped and unique-local.
  - Beyond the ruling, `SPECIAL_PURPOSE` also has IETF protocol assignments (`192.0.0.0/24`), benchmarking,
    reserved `240.0.0.0/4`, IPv4-compatible `::/96`, NAT64 local use, discard, 6to4 and the newer IPv6
    documentation block `3fff::/20`.
  - **After re-review**, it also has:
    - the NAT64 well-known prefix `64:ff9b::/96`, which embeds an IPv4 address: `64:ff9b::a9fe:a9fe` is
      `169.254.169.254`. RFC 6052 §3.1 forbids the prefix for non-global IPv4, so nothing legitimate is lost. The
      special-purpose fixture now pins that block too, and so breaks the rule four times;
    - Teredo `2001::/32`, IPv6 benchmarking `2001:2::/48` and the deprecated site-local `fec0::/10`.
  - **`2001::/23` isn't listed whole.** The registry marks the IETF protocol assignments block as a whole not
    globally reachable, but some assignments inside it are globally reachable: the anycast addresses
    `2001:1::1/128` to `2001:1::3/128`, AMT `2001:3::/32`, AS112-v6 `2001:4:112::/48` and ORCHIDv2 `2001:20::/28`.
    Listing the `/23` would refuse real endpoints, so only its non-global parts are listed.
- **Two more rules across values (after re-review).**
  - A secret lookup's transaction and operation must be those of an outside request the platform is serving.
    Only the platform knows which requests are in flight, so it is stated for `F-014` and not checked here.
  - An outbound call record's secrets must be a subset of its operation's declared secrets. That needs only the
    record and its declaration, so `validate_record_secrets` checks it, with `declared-outbound-call-record-`
    fixtures.
    - **Beyond the ruling:** it also checks that the record is the declared module's, by provider and module
      version (`RecordOfAnotherModule`). Without that, the subset check could be met with another module's
      declaration, as with the advertisement check.
    - A record of an undeclared operation, refused as `undeclared_operation`, then names no secret.
  - `validate_lookup` and `validate_record_secrets` both assume their declaration passed `validate_declaration`,
    so its operation names are unique, and their doc comments say so.
  - Overlap is decided on the shorter prefix's bits, so a host-bit form such as `8.8.8.8/0` is caught too.
- **The fixtures moved off the documentation ranges.** They had pinned `203.0.113.0/24`, `2001:db8::/32` and
  `198.51.100.16/28`, which are now invalid as blocks. The replacements, `185.12.64.0/24`, `2a0a:4cc0::/32` and
  `185.12.65.16/28`, are arbitrary, publicly routable allocations, chosen for the fixtures only. The outbound call
  records' connected addresses moved with them, so the fixture set stays coherent though no test relates a record
  to a declaration:
  - acme-bank's hops connect inside its pinned blocks (`185.12.64.5`, `185.12.65.20`), except the refused call
    whose address is outside them by design (`2001:db9::7`);
  - bridge-pay's destination is `"public"`, which excludes the documentation address `192.0.2.10` its hops used,
    so they connect to `185.12.66.10`.
- **Address forms.**
  - In Rust, an address must be the text Rust writes for it, which for IPv6 is RFC 5952. An IPv4-mapped address is
    refused. A block's address follows the same rule, and its length has no leading zero.
  - The schema's IPv6 pattern is now an anchored alternation that admits at most one `::`, so `:::` fails.
  - Only Rust checks the rest of RFC 5952: leading zeros, a single zero group compressed, an uncompressed run.
    The test's address-form tables list each form with both verdicts, and assert that Rust never accepts what the
    schema refuses.
- **Nits.**
  - The statement-key test is tagged `REQ-MDL-3`.
  - `statement_key` is a `PublicKey` in Rust, checked as provider-role writes a public key
    (`declaration-statement-key-malformed`).
  - `declaration-call-declared-twice` was added.
  - The README says `answered.body` can carry a secret inward at the provider's own risk.

## T-003-02

The admission artefact, in the module-interface contract at 0.3.0. It reuses what T-003-01 settled: the
provider-role envelope and its signing rule (JCS, Ed25519 only), and the declaration, `statement_key` included.

- **Two shapes, not one.** `REQ-MDL-1` asks for "the provider's signed assertion", and `REQ-MDL-2` for "the
  platform version that made its machine code". The provider can't sign the second, which comes after it.
  - The **release assertion** is the provider's statement.
  - The **admission artefact** is the platform's record around it, as `ARCHITECTURE.md`'s "module admission
    record" lists: admitted binary and platform build version, signature, capability declaration, version history.
- **The assertion's envelope.** The provider-role envelope's `shape` admits any `urn:rutent:<contract>:<name>`, so
  `urn:rutent:module-interface:release-assertion` needs no provider-role change. `ReleaseAssertion` implements
  `StatementShape`, so `check_shape` applies as to every party statement.
- **Who signs, and with which key.** The maker is the provider, as `REQ-MDL-1` words it. The key is one of the keys
  its advertisement publishes, which the operator's grant makes authoritative (`REQ-CAT-2`).
  - Not the `operator_key`, as for results.
  - **Not the `statement_key` either (beyond the brief).** The running outside part resolves that key to sign
    results, so a compromised outside part could otherwise sign its own next release (`THR-4`). The fixtures'
    advertisement therefore publishes two keys.
  - **The publisher.** `PS-84` speaks of a publisher's signed assertion, and the advertisement names the publisher
    behind a provider. Publisher keys aren't shaped in any contract, so the assertion is read as the provider's,
    signed by whoever holds its keys, the publisher in practice. "A publisher MUST NOT be able to replace
    another's module" then holds through the maker check and `ReleaseOfAnotherModule`.
- **Integrity: altered, unsigned and wrongly signed (`REQ-MDL-1`'s test).**
  - **Unsigned:** an artefact without an assertion, or with one carrying no signature, fails its schema.
  - **Wrongly signed:** `validate_assertion_key` covers the maker, deployment and key.
  - **Altered:** the artefact records the digest the platform computed of each received binary beside the asserted
    one, and `PartAltered` refuses a difference. Without `received`, the artefact couldn't show integrity was
    checked.
  - The contract checks digests' and signatures' form only. Computing them over bytes and verifying them is the
    platform's (`M2`, `M3`), so no hashing or signature dependency is added.
- **Digests.** SHA-256 only, as 64 lower-case hexadecimal characters, its one text form. The algorithm is named, so
  another can be added by a later version.
- **Formats.** A part's `format` is a free lower-case name, never an enum of Wasm or PVM (`DEC-4`); `EXP-2` binds
  formats.
- **The build.** `platform_version` names the exact platform build, its compiler included, that made the machine
  code, with each part's machine-code digest. Re-deriving and comparing (`REQ-MDL-2`, ADR-0014) then needs only the
  admitted binary and that version. Re-derivation results, staging, stale marks and the emergency bypass are
  `CMP-PROV`'s lifecycle (`REQ-MDL-4`, `REQ-MDL-5`, ADR-0014), so they aren't shaped here.
- **History.** Earlier admitted releases, oldest first, each by version, assertion digest and admission time.
  `assertion_digest` is the platform's digest of the assertion as received, so a history entry names exactly what
  was admitted, not only a version. A release extends the previous one's history by that release
  (`HistoryNotExtended`), and only an accepted release can be followed (`PreviousNotAdmitted`).
- **Widening.** It is computed from two declarations only, as the brief asks. The brief's list is a new operation,
  secret, destination, block or call, or a broader block. The readings taken:
  - **Beyond the list:** an existing operation gaining a destination or a secret, which reaches what it couldn't;
    a destination of the same name at another scheme, host or port, which is a new endpoint; a new kind asked of an
    existing callee.
  - **Also beyond the list:** a changed `statement_key`. A release that swaps its result-signing key is the
    `THR-4` pattern, so it takes a fresh acceptance.
  - A "broader block" is a destination's addresses grown: blocks to `public`, or a block no single old block
    contains. A block covered only by the union of two old blocks is marked: containment is checked against single
    blocks, which errs towards a fresh acceptance. The widening table tests that case.
  - `public` to blocks is narrowing only because T-003-01 refuses special-purpose blocks. Otherwise blocks could
    reach what `public` excludes.
  - Narrowing is never marked: anything removed, a sub-block, `public` replaced by blocks.
  - Handled kinds (`sandboxed.handles`) aren't capabilities: they are the provider-role interface, which reaches
    nothing outside. Call kinds are compared through `OperationKind`, so the kinds 0.2.0 added, `reversal` and
    `reversal_confirmation` (T-002-03), widen and narrow like any other. The widening table has a case for each:
    one asked of an existing callee, a new callee asked only for one, and both no longer asked.
  - The recorded widenings follow the order of the new declaration's lists, so comparing recorded with computed is
    exact (`WideningsMisrecorded`).
- **A first release** is one with no history. It compares with nothing, lists no widening, and requires a fresh
  acceptance by its own rule. Deriving it from "everything widens against nothing" would let a module with no
  outside reach be admitted automatically on its first release.
  - "First" is decided by the history, not by `compared_with` alone. Otherwise an artefact with no history could
    name a predecessor it doesn't have and pass as automatic, and with no previous artefact to compare,
    `validate_release` would never run. `compared_with` must be the last version the history names, and absent
    exactly when the history is empty (`ComparedWithNotLastAdmitted`). That makes `validate_release`'s own
    comparison with the previous version redundant, so it was dropped.
- **Acceptance.** `required` is `automatic` or `fresh`. `accepted` is absent until given, and a fresh one names who
  gave it. A refused release has no artefact, since it isn't admitted; recording refusals is `CMP-PROV`'s.
- **REQ-MDL-19 stays a rule across values.** Checking that `interface` is equal across successive releases in
  `validate_release` would stop a module from ever moving to a new interface version, so the rule stays with the
  platform.
- **Test-first.** The two plan tests were written first, and failed because no admission shape existed.

### T-003-02 review (the orchestrator, 2026-11-06)

- **The previous release's key.** Checking the assertion key against only the released declaration's
  `statement_key` missed the key the running outside part can actually reach, which is the previous release's. A
  compromised release could sign its successor's assertion with that key and declare a new one, and a fresh
  acceptance would show only "key rotated".
  - The ruling refuses any statement-signing key the module's history has ever declared, "if the history carries
    the declarations". The history carried versions and digests only, so each history entry now also records the
    `statement_key` its release declared. That is lighter than carrying whole declarations, and it is what the rule
    needs.
  - `validate_release` refuses an assertion signed with the previous declaration's key, or with any key a history
    entry names (`AssertionSignedWithPreviousStatementKey`).
- **Deployment and protocol version.** `validate_release` refuses a release of another deployment or protocol
  version than the one it follows. A statement is void elsewhere (`REQ-PRT-1`). Under a new protocol version, a
  module is therefore admitted again as a first release, with a fresh acceptance.
- **Machine code.** `build.machine_code` is tied to nothing the contract can check, by design: only re-deriving
  and comparing tests it (`REQ-MDL-2`). The build test now checks that both layers refuse an artefact without the
  platform version or the machine-code digests, instead of an assertion that couldn't fail.
- **Widening table.** It gained a secret moved between operations (one `operation_secret`), a renamed operation
  (one `operation`), and a call to another callee (one `call`).
- **History.** An earlier version listed twice has its own fixture.

## T-003-03

The module-boundary suite, in the conformance crate (`conformance/src/boundary/`), and module-interface 0.4.0's
cross-module call outcome, which the suite needed to check a refused call in contract terms.

- **What it scores.** A format binding, the runtime that admits a module in some portable binary format and runs
  it. The suite drives it through `ModuleBinding`, which names no format. `EXP-2` binds it per format; the in-process
  reference double runs it here.
- **Format-neutral modules.** The suite describes each module it needs as a `ModuleSpec`: a module-interface
  declaration, the steps its parts take, and the output its sandboxed part returns. A binding builds each spec in its
  own format and in each language it offers, and the suite admits the binaries like any release. The steps are a
  small fixed set, so a binding can write them as genuine guest code in each language.
- **Evidence, not silence.** A canary that never leaks proves nothing about a module that never tried. So each
  hostile fixture counts only when the binding shows the refusal in contract terms, with the expected reason. Each
  has an honest control on the same declaration, which must succeed, so a binding that refuses everything fails
  `DeclaredReachWorks`. Whatever isn't seen is uncovered, and the report doesn't pass.
- **The output limit is the suite's.** A binding reporting its own limit could pass `oversized output is refused`
  with an enormous one. The suite configures the limit, measures bytes as the module emitted them, and checks both
  edges: at the limit passes, one byte over is refused.
- **Both parts, secrets and state.** `AC-20` says "neither part", so another provider's secret and state are each
  probed from the sandboxed and from the outside part. The sandboxed part's lookup of its own provider's secret is
  probed too: it must never reach the proxy (`REQ-MDL-6`).
- **Another provider, three ways:** its secret and state; its endpoint, which this module doesn't declare, refused as
  `undeclared_destination`; and a cross-module call to it as an undeclared callee.
- **A refused cross-module call, in the contract** (ruled by the orchestrator, 2026-11-07, before the freeze).
  0.3.0's cross-module call had no outcome, so the suite first reported a refused call in a suite-side type.
  - The ruling: a refused call must be recordable in contract terms, for audit and so that `EXP-2` bindings write a
    contract shape.
  - The record now has a required `outcome`: `delivered`, or `refused` with the callee, the kind asked and the
    reason.
  - **Reasons.** The three ruled are `undeclared_callee`, `undeclared_kind` and `callee_unavailable`. The fourth,
    `not_named_by_route`, goes beyond the ruling: the contract already says the platform refuses a declared call
    the route doesn't name (`REQ-MDL-21`, `THR-86`), so that refusal needs a reason too.
  - The reason is the first check the call fails, in that order. `validate_call` checks that a refusal names the
    call's own callee and kind. `validate_call_against` checks the outcome against the caller's declaration. That
    moves "the caller declares the callee and kind" out of the rules across values; the route and the callee's
    running stay the platform's.
  - The suite checks the refused call through the record, and through both validators. Nothing was left that the
    contract can't hold, so the suite-side `CallEvent` is gone.
  - Required, not optional: every record of a call says what came of it, as an outbound call record does.
- **Checks group by property.** Each `Check` covers every observation of one property. Each of the double's 19
  breakers is flagged by its own check only, and every check has at least one breaker.
  - A proxy answering from another provider's secrets both resolves where it should refuse and leaks the canary.
    That is one check, `OtherProvidersSecrets`, not two.
- **Upgrades.** `AC-19`'s "a failed upgrade leaves the previous version in force" is read as a release that is
  admitted but fails to start. A refused admission is an admission failure, which `AdmissionRecordsWhatItAdmits`
  covers. The version in force is checked two ways: `active_version`, and the version the module's own output names.
- **Processes.** `ModuleBinding::process` names a provider's process, so one OS process per provider is checkable.
  Through an upgrade, a failed upgrade and a restart of one provider, another keeps its process and its answers
  (`REQ-MDL-13`).
- **Admission uses the contract's validators**: `validate_artefact`, `validate_release` and `widening`. The suite
  computes the digests its assertions state with a local SHA-256, checked against the standard vectors, since no
  hashing dependency is added. It is crate-private and for the suite and its double only.
- **The output shape** is checked with the contract's Rust types. The schema validator is a dev-dependency of the
  contracts, so patterns and lengths aren't checked here; the contracts' own suite holds them.
- **Not covered:** signatures, which are placeholders and unverified, as in the role suite (`M2`, `M3`);
  `REQ-MDL-11`'s limits other than output; TLS; and resolution changes other than to a private address.
- **`DEC-20`.** The suite and its double are neither the production admission path nor a signature check. The
  cross-module call outcome changes `contracts/`, so its commit names `DEC-20` and waits for the owner's click.

### T-003-03 review (the orchestrator, 2026-11-07)

An Opus review ran two altered doubles that passed, and listed probe gaps. The fixes, and the readings taken:

- **Both parts altered in turn.** The altered-release fixture alters the sandboxed part, then the outside part, and
  counts only when both are refused. A binding comparing one part's digest is flagged.
- **A failed upgrade must be admitted.** A failing release refused at admission proves nothing about a failed
  upgrade, so the fixture is then uncovered, not passed.
- **The other provider runs.** It is installed before the reach probes with a declaration naming its own
  endpoint. So a binding sharing one egress allowlist reaches it and is flagged, and "the callee receives nothing"
  is about a module that could receive.
- **Egress beyond simple paths:**
  - a redirect chain, declared to declared to undeclared, refused at the third hop;
  - a redirect to a declared host at a private address, refused at the second hop;
  - declared hosts at `::1`, in `fc00::/7` and at `::ffff:10.0.0.5`.

  A breaker checking only the first redirect, only the first hop's address, or only IPv4 is flagged.
- **Acceptance by reach, not only by version.** The release modules make calls.
  - While 1.1.0 waits for its fresh acceptance, its new destination `extra` is refused as undeclared. Once accepted,
    it is reached.
  - After the narrowing 1.2.0, the destination it dropped is refused. That is `archive`, not `internal` as the
    review named it: `internal` is at a private address and refused whatever the declaration says, so dropping it
    would test nothing.
  - Releases widening by a secret, a callee and an address each wait for a fresh acceptance. Breakers that don't
    count one of them as a widening are flagged.
  - A reach probe judges an acceptance only where the binding's record agrees with what the endpoint received. A
    call recorded as refused but delivered is a broken egress check. The egress probes judge that, and the reach
    probe's fixture is left uncovered. That keeps each breaker under one check.
- **No secret value on the production trait.** `ModuleBinding::take_events` reports a lookup's outcome status only
  (`LookupOutcome`). The values the proxy delivered are read through `BoundaryHarness::delivered_secrets`, where the
  suite looks for another provider's canary.
- **Nits:**
  - the cross-module probes have an honest control, a declared call delivered to the running callee, and an
    `undeclared_kind` probe;
  - the contract has fixtures for a delivered call of an undeclared kind, and for `undeclared_kind` given when the
    callee itself is undeclared;
  - the outside part's lookup of another provider's secret must be refused as `not_found`, since the module declares
    the name and holds no secret by it;
  - a record must name the secret its call used;
  - a lookup of a name the module doesn't declare must be refused as `undeclared_secret`, under its own check,
    `UndeclaredSecretRefused`;
  - output at the limit is measured on what the platform passed on, so a module padding short is flagged. Honest
    output is compact JSON, so its re-serialised length is its emitted length;
  - `build` moved from `ModuleBinding` to `BoundaryHarness`;
  - the reference double is built only with the `test-support` feature, which the crate enables for its own tests
    through a dev-dependency on itself. A production build can't link it, and `cargo test --workspace` still runs
    every test;
  - DNS rebinding isn't expressible yet: `serve` gives an endpoint one address.
- **Records name exactly the secrets used.** A record's `secrets` must equal, as a set, the names the outside part
  resolved for that operation in that transaction. `validate_record_secrets` checks only that they are declared, so
  a binding listing every declared secret on every record would otherwise pass. A breaker that does so is flagged.
- **The declared-call control can fail.** A breaker refusing every cross-module call, a declared one as
  `callee_unavailable`, is flagged by `DeclaredReachWorks` alone, since the contract allows that reason for a
  declared callee and kind.
- **Counts.** The suite has 15 checks and 41 fixtures and controls per language. The double has 38 breakers: 36
  flagged each by its own check only, and two that leave fixtures uncovered.

### T-003-03 delta review (the orchestrator, 2026-11-07)

Two fixes could still be slipped past; the reviewer built scratch doubles that passed.

- **A failed upgrade held as waiting was never started.** "Kept the previous" was then counted for free. Now:
  - the failing release, which widens nothing, must take an automatic acceptance;
  - if it is held as waiting anyway, the suite accepts it first, and only then checks the version in force.

  A breaker holding non-starting releases as waiting is flagged by `ProvidersAreIsolated` alone.
- **Three of the four widenings weren't checked by reach.** The release modules now:
  - look up `release.token`, seeded for the release provider;
  - call a new endpoint, `spread`, served at an address outside its pin until 1.5.0 opens it to `public`;
  - call the other provider.

  While each widening release waits, what it adds must be refused, as `undeclared_destination`,
  `undeclared_secret`, `undeclared_callee` and `address_not_allowed` respectively. Once accepted, it must work.
  `answers_as` allows the resolved value in the evidence reference.
- **Controls keep each breaker under one check.** Each widening is judged beside controls in the same operation:
  the module's own secret resolving, and a secret and a callee no release declares being refused. If a control
  fails, the binding's own secret or call enforcement is broken, which the reach probes judge, and the widening's
  fixture is left uncovered.
  - After acceptance, only the old declaration still being applied counts as a finding. A refusal any declared
    call can get, such as `callee_unavailable`, leaves the fixture uncovered.
  - A breaker honouring the waiting declaration in the proxy and the cross-module check is flagged by
    `WideningNeedsFreshAcceptance` alone.
- **Nits:**
  - the unique-local probe uses `fd00::5`;
  - `Admitted::Refused` carries an `AdmissionRefusal`, so an altered part must be refused as `PartAltered` of that
    part;
  - `Exercised::ordinal` is an exhaustive match, and a test checks `Exercised::ALL` against it;
  - each part is altered both by a byte appended and by a byte flipped in place.
- **Counts.** 15 checks, 41 fixtures and controls per language, and 40 breakers: 38 flagged each by its own check
  only, and two that leave fixtures uncovered.

## The M1 security review (module-interface 2.0.0, `T-002-03`)

Findings H1, M1 and M2 and two lows of the security review of the frozen contracts, ruled by the orchestrator.
A post-`C1` change, so a major bump under `DEC-4` and the owner's click under `DEC-20`.

- **H1, this contract's half.** Provider-role 2.0.0's key purposes (F-002 notes) give two rules here:
  `validate_statement_key` refuses a `statement_key` listed for `advertising` (`StatementKeyNotForStatements`), and
  `validate_assertion_key` an assertion signed with a key listed for `statements` (`AssertionKeyNotForAdvertising`).
  `AssertionSignedWith{,Previous}StatementKey` stay.
- **The statement key at admission.** `validate_statement_key` needs a signed result, which a release doesn't have,
  so on its own nothing at admission stopped a release declaring one of the provider's advertising keys as its
  `statement_key`; its outside part would then resolve a key that signs advertisements. `validate_assertion_key`
  now also checks the declared `statement_key` against the accepted advertisement, reusing
  `StatementKeyNotAdvertised`, `StatementKeyNotForStatements` and `StatementKeyIsOperatorKey`, with one
  `advertised-artefact-declaring-` fixture each.
- **One fixture breaks two rules.** `advertised-artefact-asserted-with-the-statement-key` lists the statement key
  for `statements`, as a valid advertisement must, so it trips `AssertionKeyNotForAdvertising` and
  `AssertionSignedWithStatementKey` together. The older rule can't fire alone any more: a statement key listed for
  `advertising` is refused as `StatementKeyNotForStatements`. It stays as a second line.
- **The advertisement given to these checks is the accepted one.** The docs say so: one that arrives with the
  release proves nothing, since a forged advertisement can list any key. Accepting a later advertisement is
  provider-role's `validate_successor`, held by the catalogue (`F-012`).
- **M1: newly handled kinds widen.** A release that starts handling a kind in `sandboxed.handles` was accepted
  automatically. `Widening::Handles { operation_kind }` (`kind: "handles"`) comes first in `widening`'s order, as
  the sandboxed part's list comes first in a declaration. A kind no longer handled is narrowing. The schema's
  widening is an `allOf` of `if`/`then` on `kind`, so adding one changed no other fixture's exact errors.
- **M2.** Keys are provider-role's `PublicKey`, re-exported as `module_interface::PublicKey`, so `as_str()` still
  works for the bindings. `advertised-artefact-asserted-with-a-non-canonical-alias-of-the-statement-key` signs with
  the statement key's last character moved from `w` to `x`, which differs only in the two unused bits: both layers
  refuse it.
- **Lows.** `SPECIAL_PURPOSE` adds `192.88.99.0/24`, the deprecated 6to4 relay anycast prefix (RFC 7526), and
  `5f00::/16`, the SRv6 SID block (RFC 9602). A new fixture holds both; the old one's expected violations are
  indexed by position, so it is left alone. No fixture or binding pins into either.
