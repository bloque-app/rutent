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
