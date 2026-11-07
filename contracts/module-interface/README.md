# module-interface contract

This contract is the module boundary every provider module is written against (`REQ-MDL-16`, ADR-0003). It splits
a module into a **sandboxed part** and an **outside part**, and states:

- what each part may declare;
- what crosses between them, and what leaves the outside part;
- the record of every outbound call (`REQ-MDL-8`);
- the declared calls between modules (`REQ-MDL-21`).

It is format-neutral and language-neutral (`DEC-4`). Nothing here assumes Wasm, PVM or any module format, and
nothing names an import, an export or a host function. Bindings to a format belong to `EXP-2` (`T-008-*`). The
admission artefact that carries the declaration under a signed assertion is `T-003-02`; the module-boundary suite
that runs hostile modules against it is `T-003-03`.

## Schemas

| `$id` | File | Holds |
|---|---|---|
| `urn:rutent:module-interface:declaration` | `schemas/declaration.schema.json` | What a module declares: the sandboxed part's operation kinds; the outside part's operations, destinations, secret names and calls. This is the capability declaration (`REQ-MDL-3`) |
| `urn:rutent:module-interface:outside-request` | `schemas/outside-request.schema.json` | Sandboxed part to outside part: perform one declared outside operation for one transaction |
| `urn:rutent:module-interface:outside-response` | `schemas/outside-response.schema.json` | Outside part to sandboxed part: that request's outcome |
| `urn:rutent:module-interface:secret-lookup` | `schemas/secret-lookup.schema.json` | Outside part to credential proxy: resolve one declared secret by name, at call time, for one transaction and one outside operation |
| `urn:rutent:module-interface:secret-answer` | `schemas/secret-answer.schema.json` | Credential proxy to outside part: the value, or a refusal |
| `urn:rutent:module-interface:outbound-call-record` | `schemas/outbound-call-record.schema.json` | The platform's record of one outbound call, hop by hop |
| `urn:rutent:module-interface:cross-module-call` | `schemas/cross-module-call.schema.json` | One module's call to another provider's module, through the platform, and its record |
| `urn:rutent:module-interface:common` | `schemas/common.schema.json` | Shared atoms: declared name, scheme, host, port, address, address block, refusal reason |

The schemas refer to each other by `$id`, and to the provider-role contract's by its `$id`s: its identifiers,
versions, times, operation kinds and operations (`urn:rutent:provider-role:common`,
`urn:rutent:provider-role:operation`). A validator registers both contracts' schemas and needs nothing from the
network. A fixture's file name starts with its shape: `declaration-`, `outside-request-`, `outside-response-`,
`secret-lookup-`, `secret-answer-`, `outbound-call-record-`, `cross-module-call-`, `signed-result-`,
`declared-secret-lookup-` or `declared-outbound-call-record-`. The last three are bundles, not shapes of their own,
for rules that relate several values.
Each member is checked against its own schema and its own contract's rules, so an invalid bundle breaks only the
rule it is named for:

- a `signed-result-` fixture holds a module's `declaration`, its provider's enveloped `advertisement`
  (`urn:rutent:provider-role:advertisement`) and a `result` it signed (`urn:rutent:provider-role:result`), for the
  statement-signing key rules;
- a `declared-secret-lookup-` fixture holds a module's `declaration` and a secret `lookup` its outside part makes,
  for the rule that a lookup's operation declares its secret;
- a `declared-outbound-call-record-` fixture holds a module's `declaration` and an outbound call `record` of it,
  for the rule that a record's secrets are its operation's.

## The split

```
              provider-role operation                outside request
  platform  ------------------------->  sandboxed  ------------------>  outside  --- secret lookup --->  credential
            <-------------------------    part     <------------------   part    <-- secret answer ----   proxy
              provider-role result                  outside response          |
                                                                              |  outbound calls, recorded by the
                                                                              v  platform hop by hop
                                                                   declared destinations
```

- **The sandboxed part** computes. It receives provider-role operations of the kinds it declares in
  `sandboxed.handles` (`urn:rutent:provider-role:operation`, in the operator's envelope), and returns each one's
  result body (`urn:rutent:provider-role:result#/$defs/result`). It may ask the outside part to perform a declared
  outside operation, and nothing else.
  - **It doesn't sign, and never holds a key.** A result is the provider's signed statement (`REQ-QTE-1`,
    `REQ-AUD-2`), and the sandboxed part returns only its body. The outside part signs it (see "Signing a result"
    below; ruled by the orchestrator, 2026-11-06, `DEC-4`, `T-003-01`).
  - **It declares** only the operation kinds it handles.
  - **It has no field for** a destination, a secret or a call. Declaring one is invalid (`REQ-MDL-6`,
    `REQ-MDL-16`).
  - **Its outside request** names a transaction, a declared outside operation and its arguments. It names no
    destination and no secret, so it can't aim a call elsewhere or ask for a secret: the declaration fixes which
    destinations and secrets each outside operation uses (`REQ-MDL-6`, `REQ-MDL-7`).
  - **It never sees secret plaintext** through this contract: no shape that reaches it has a field for a secret.
    The outside part can still copy one into a response's `answered.body` (see below).
- **The outside part** is the provider's own code, admitted with the module (`REQ-MDL-1`, `REQ-MDL-16`). It does
  all of the module's outside interaction.
  - **It declares**:
    - `operations`, each with the destinations and secrets it may use (`REQ-MDL-8`);
    - `destinations`, the allow-list (`REQ-MDL-7`);
    - `secrets`, by name only (`REQ-MDL-6`, `REQ-MDL-17`);
    - `calls` to other providers' modules (`REQ-MDL-21`);
    - `statement_key`, the public half of its statement-signing key (see "Signing a result").
  - **Every list is required**, and so is `statement_key`. An empty list declares none, so nothing is implied, and whatever isn't declared is
    refused.
  - **It resolves a secret** by name at call time, with a secret lookup naming the transaction (`REQ-MDL-6`'s
    "attributable per transaction") and the outside operation it serves. The proxy resolves only a secret that
    operation declares, so while serving one operation the outside part can't resolve a secret only another uses
    (`REQ-MDL-3`, `REQ-MDL-6`); the answer echoes the operation. The lookup names no provider: under `DEC-9`, the proxy answers only for the
    calling process's own provider, which it knows from the process (ADR-0003), never from the request. So a lookup
    can't ask for another provider's secret (`REQ-MDL-9`). The answer carries the current value, so rotating a
    secret needs no release (`REQ-MDL-6`). The secret answer is the only shape here that carries plaintext, and only
    the outside part receives it.
  - **It signs the sandboxed part's results** with its declared statement-signing key, as "Signing a result"
    below states.
  - **What the outside part does with plaintext** is not bounded by this contract: its author reads it by design
    (`THR-83`, accepted).
    - In particular, an outside response's `answered.body` is free-form, so an outside part that copies a secret
      into it passes the plaintext inward to the sandboxed part. No schema can prevent that. It is the provider's
      own risk, taken in its own code; the contract keeps only the platform from handing the sandboxed part a
      secret.
- **The platform** sits between the parts and around them.
  - It refuses whatever isn't declared, with a reason (`refusal_reason`).
  - It records every outbound call, refused ones included (`REQ-MDL-8`).
  - It makes a cross-module call only where the route names it (`REQ-MDL-21`).
- **The interface is fixed** across module versions (`REQ-MDL-19`):
  - A declaration names the module's `version` and, separately, the `interface` version it implements, this
    contract's.
  - The kinds a module handles or calls are provider-role kinds, referred to by `$id`, so a module can't invent
    one.

## Signing a result

Ruled by the orchestrator, 2026-11-06 (`DEC-4`, `T-003-01`):

- **The module never holds a key.** No declaration, request, response or artefact of the module carries a private
  key, and the sandboxed part never sees one.
- **The outside part signs.** It takes the result body the sandboxed part returns and wraps it in the provider-role
  envelope (`urn:rutent:provider-role:statement-envelope`), with the provider as `maker`.
- **It declares the key it signs with.** `outside.statement_key` is the public half of the provider's
  statement-signing key, and is required. Signing is then part of the capability declaration like everything else
  the outside part does (`REQ-MDL-3`), not an exception to it.
  - The advertisement's `keys` have no id, so a key is named by its public key, and `statement_key` is one.
  - It is not one of the declared `secrets`, which are the provider's other credentials. The statement-signing key
    signs rutent statements, unlike, say, an on-chain `chain.signing_key` secret an outside operation uses.
- **It resolves the private half by the deployment's name** at signing time, through the credential proxy. The
  deployment is the one the enveloped advertisement names, `{name, operator_key}`, so a key resolved for one
  deployment can't sign a statement for another (`REQ-PRT-1`, `THR-8`). As for every lookup, the proxy answers only
  for the calling process's own provider (`DEC-9`). The lookup has no wire shape here; the proxy's side is
  `F-014`'s.
- **The key is the provider's, not the operator's.** The deployment's `operator_key` only names the deployment; it
  signs the operator's own statements, never a provider's.
- **What it signs** is the provider-role signing rule: the RFC 8785 (JCS) canonical JSON of the whole envelope,
  with only the top-level `signature.value` removed, under `ed25519` (RFC 8032).
- **What is checked.** `validate_statement_key(declaration, advertisement, result)` relates the three values:
  - the advertisement is the declaring provider's (`AdvertisementOfAnotherProvider`);
  - `statement_key` is one of the advertisement's `keys` (`StatementKeyNotAdvertised`);
  - it isn't the `operator_key` of the advertisement's deployment (`StatementKeyIsOperatorKey`);
  - the result belongs to the advertisement's deployment, by name and operator key, so no caller has to relate
    the two first (`ResultOfAnotherDeployment`);
  - the result's `maker` is the declaring provider, so the provider's key never vouches for a statement in
    another party's name (`ResultOfAnotherMaker`);
  - the result's `signature.key` equals it (`ResultNotSignedWithStatementKey`).

  It compares keys, deployments and makers only. Verifying the signature itself is `M2` and `M3` work, as in the provider-role contract.

## Destinations and ADR-0004's checks

ADR-0004 asks for destination allow-lists that are deny by default and checked by name and connected address,
again on every redirect. A destination declaration carries what those checks need:

- `name`, the declared name an outside operation refers to;
- `scheme`, `https` or `wss`: TLS only, so a connection is to the endpoint the name certifies (`THR-79`);
- `host`, an exact DNS name with no wildcard and no address literal;
- `port`;
- `addresses`, what the connected address may be:
  - `"public"`: any globally routable address the host resolves to at call time, never a loopback, private,
    link-local or other special-purpose one;
  - or a non-empty list of address blocks the connection is pinned to. Each block is globally routable: one that
    overlaps a special-purpose range is invalid, such as `0.0.0.0/0`, `::/0` or the cloud metadata address
    `169.254.169.254/32`. This is checked in Rust (`SpecialPurposeAddressBlock`), against `SPECIAL_PURPOSE`, which
    follows the IANA special-purpose registries:
    - unspecified and "this network", `0.0.0.0/8`, `::/128`;
    - loopback, `127.0.0.0/8`, `::1/128`;
    - private, `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`;
    - shared address space (CGNAT), `100.64.0.0/10`;
    - link-local, `169.254.0.0/16`, `fe80::/10`;
    - documentation, `192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24`, `2001:db8::/32`, `3fff::/20`;
    - multicast, `224.0.0.0/4`, `ff00::/8`;
    - IPv4-mapped, `::ffff:0:0/96`;
    - NAT64, the well-known prefix `64:ff9b::/96`, which embeds an IPv4 address and could otherwise reach the
      metadata address as `64:ff9b::a9fe:a9fe`. RFC 6052 §3.1 forbids it for non-global IPv4, so nothing
      legitimate is lost. Also the local-use prefix `64:ff9b:1::/48`;
    - unique-local, `fc00::/7`, and the deprecated site-local `fec0::/10`;
    - in the IETF protocol assignments `2001::/23`: Teredo `2001::/32` and benchmarking `2001:2::/48`. The `/23`
      isn't listed whole: some assignments in it are globally reachable, such as the anycast addresses
      `2001:1::1/128` to `2001:1::3/128`, AMT `2001:3::/32`, AS112-v6 `2001:4:112::/48` and ORCHIDv2
      `2001:20::/28`;
    - and also IETF protocol assignments `192.0.0.0/24`, benchmarking `198.18.0.0/15`, reserved `240.0.0.0/4`,
      IPv4-compatible `::/96`, discard `100::/64` and 6to4 `2002::/16`.

An address is IPv4 in dotted decimal or IPv6 in its RFC 5952 text form, and an IPv4-mapped IPv6 address is refused:
it is written as the IPv4 address. The schema checks the shape only (lower-case groups, at most one `::`); Rust also
requires the one text form, so `2001:0db8::1` passes the schema and fails in Rust. The test's address-form tables
list each such form.

The checks run per hop: the first connection, then each redirect.

- **Name:** the hop's scheme, host and port must be one of the declared destinations that the outside operation
  lists. A redirect elsewhere is refused (`redirect_to_undeclared_destination`).
- **Connected address:** the address actually connected to, not one resolved earlier, must satisfy that
  destination's `addresses`. Otherwise the call is refused (`address_not_allowed`). This is what "altered
  resolution" in `REQ-MDL-7` is tested against.

The outbound call record shows each check was made. Each hop records the scheme, host and port asked for, the
declared destination it matched, and the address connected to. Every hop but the last of a refused call matched
and connected.

## The outbound call record

The platform's egress layer writes it, not the module, since the outside part is the provider's own code
(`THR-83`). It holds:

- the `transaction`, the `provider`, the `module_version` and the outside `operation` (`REQ-MDL-8`, `REQ-MDL-10`);
- the names of the secrets resolved for the call, never their values (`REQ-MDL-6`);
- `started_at`;
- the `hops`;
- an `outcome`: `completed`, `failed` or `unknown` as the destination answered or didn't, or `refused` with the
  reason.

## Cross-module calls

- **The declaration.** A module declares each callee and the provider-role operation kinds it may ask of it, in
  `outside.calls`. The declaration carries no credentials.
- **The call** names:
  - the transaction, and the caller and its version;
  - the route decision record and the segment that name the call;
  - the provider-role operation asked of the callee, whose `provider` is the callee.
- **What the platform does.** It makes the call only when the caller declares it and the route names it
  (`REQ-MDL-21`, `THR-86`). The callee answers the operation as any other, on its own credentials. The call carries
  none.

## Which rule lives where

A valid value passes its schema and deserialises into its type in `rutent_contracts::module_interface`. A
declaration, an outbound call record and a cross-module call also get `Ok` from `validate_declaration`,
`validate_record` and `validate_call`. A `signed-result-` bundle also gets `Ok` from `validate_statement_key`, and a
`declared-secret-lookup-` bundle from `validate_lookup` and a `declared-outbound-call-record-` bundle from
`validate_record_secrets`, after each member passes its own contract's rules. Both assume their declaration
passed `validate_declaration`. The Rust
rules are the ones JSON Schema can't express: a name compared across lists or entries, two fields compared, an
address block compared with the special-purpose ranges, or values compared across a declaration and the values it
governs.

| Rule | Where | Invalid fixture |
|---|---|---|
| The sandboxed part declares only the operation kinds it handles: no secret, no destination | schema (`additionalProperties: false`), and Rust | `declaration-sandboxed-part-declaring-a-secret-lookup`, `declaration-sandboxed-part-declaring-a-destination` |
| It handles at least one kind, each a provider-role kind | schema | `declaration-sandboxed-part-handling-nothing`, `declaration-sandboxed-part-handling-an-unknown-kind` |
| A declaration names the interface version it implements, and lists every outside declaration, its statement-signing key included | schema | `declaration-without-its-interface-version`, `declaration-without-calls`, `declaration-without-its-statement-key` |
| A destination is an exact host name over TLS, with its addresses declared as `public` or as address blocks | schema, and Rust except the host pattern | `declaration-destination-with-a-wildcard-host`, `declaration-destination-at-an-address-literal`, `declaration-destination-over-plain-http`, `declaration-destination-without-addresses`, `declaration-destination-pinned-to-a-host-name` |
| An outside operation reaches at least one destination | schema | `declaration-operation-reaching-no-destination` |
| A secret name is declared once | schema (`uniqueItems`) | `declaration-secret-declared-twice` |
| An operation's destinations and secrets are declared by the outside part | Rust, `UndeclaredDestination`, `UndeclaredSecret` | `declaration-operation-naming-an-undeclared-destination`, `declaration-operation-using-an-undeclared-secret` |
| An operation, destination or call is declared once by name or callee | Rust, `DeclaredTwice` | `declaration-operation-declared-twice`, `declaration-destination-declared-twice`, `declaration-call-declared-twice` |
| A pinned address block overlaps no special-purpose range | Rust, `SpecialPurposeAddressBlock` | `declaration-destination-pinned-to-special-purpose-blocks` |
| The statement-signing key is a canonical Ed25519 public key | schema, and Rust (`PublicKey`) | `declaration-statement-key-malformed` |
| A declared call names kinds, carries no credentials, and isn't to the module's own provider | schema; Rust, `DeclaredCallToOwnProvider` | `declaration-call-without-kinds`, `declaration-call-carrying-credentials`, `declaration-call-to-its-own-provider` |
| An outside request names its transaction, and neither a secret nor a destination | schema | `outside-request-without-a-transaction`, `outside-request-naming-a-secret`, `outside-request-naming-a-destination` |
| An outside response's refusal states its reason; an unknown outcome carries nothing | schema | `outside-response-refused-without-a-reason`, `outside-response-unknown-carrying-a-body` |
| A secret lookup names its transaction and its outside operation, and no provider | schema | `secret-lookup-without-a-transaction`, `secret-lookup-without-an-operation`, `secret-lookup-naming-a-provider` |
| An outbound call record's secrets are ones its operation declares, and the record is the declared module's | Rust, `validate_record_secrets`: `RecordedSecretUndeclaredForOperation`, `RecordOfAnotherModule` | `declared-outbound-call-record-using-a-secret-its-operation-does-not-declare`, `declared-outbound-call-record-of-another-module-version` |
| A secret lookup's operation is declared, and declares the secret | Rust, `validate_lookup`: `LookupForUndeclaredOperation`, `SecretUndeclaredForOperation` | `declared-secret-lookup-for-an-undeclared-operation`, `declared-secret-lookup-of-a-secret-its-operation-does-not-declare` |
| A resolved secret carries a non-empty value | schema, and Rust | `secret-answer-resolved-without-a-value`, `secret-answer-resolved-to-an-empty-value` |
| An outbound call record names its transaction and module version, and has at least one hop | schema | `outbound-call-record-without-a-transaction`, `outbound-call-record-without-a-module-version`, `outbound-call-record-without-hops` |
| It names the secrets used, never their values; a connected address is an IP address | schema, and Rust | `outbound-call-record-carrying-a-secret-value`, `outbound-call-record-connected-to-a-host-name-as-its-address`, `outbound-call-record-connected-to-a-malformed-ipv6-address` |
| A hop's optional destination and address are absent or present, never `null` | schema, and Rust | `outbound-call-record-hop-with-a-null-destination`, `outbound-call-record-hop-with-a-null-address` |
| A refused call states its reason; any other outcome carries none | schema | `outbound-call-record-refused-without-a-reason`, `outbound-call-record-completed-carrying-a-reason` |
| Every hop matched a declared destination and connected, except the last hop of a refused call | Rust, `HopNotConnected` | `outbound-call-record-completed-through-an-unconnected-hop` |
| A cross-module call names its transaction and its route segment, and carries no credentials | schema | `cross-module-call-without-a-transaction`, `cross-module-call-without-a-route`, `cross-module-call-route-without-its-segment`, `cross-module-call-carrying-credentials` |
| A cross-module call's callee isn't its caller, and its operation names the call's transaction | Rust, `CallToOwnProvider`, `TransactionMismatch` | `cross-module-call-to-its-own-provider`, `cross-module-call-for-another-transaction` |
| A signed result's `signature.key` is the declared `statement_key` | Rust, `ResultNotSignedWithStatementKey` | `signed-result-signed-with-another-advertised-key` |
| The `statement_key` is one of the provider's advertised `keys`, and isn't the deployment's `operator_key` | Rust, `StatementKeyNotAdvertised`, `StatementKeyIsOperatorKey` | `signed-result-statement-key-not-advertised`, `signed-result-statement-key-is-the-operator-key` |
| The advertisement checked is the declaring provider's own | Rust, `AdvertisementOfAnotherProvider` | `signed-result-checked-against-another-providers-advertisement` |
| The signed result belongs to the advertisement's deployment | Rust, `ResultOfAnotherDeployment` | `signed-result-from-another-deployment` |
| The signed result's maker is the declaring provider | Rust, `ResultOfAnotherMaker` | `signed-result-made-by-another-provider` |
| Ports are plain JSON integers from 1 to 65535 | schema, and Rust (`NonZeroU16`) when deserialising | none, as in the provider-role contract: JSON Schema's `integer` accepts `443.0` |

### Rules across values

These rules relate one value to another, so no schema or single-value check can hold them. The platform holds them
at run time (`CMP-PROV`, `F-013`, `F-014`), and the module-boundary suite (`T-003-03`) tests them with hostile
modules:

- an outside request names an operation its module declares;
- a secret lookup's transaction and operation are those of an outside request the platform is serving for that
  module, so a lookup can't be made for an operation no one asked for (`F-014` enforces it at the proxy);
- an outbound call record's secrets are a subset of its operation's declared secrets. Given the record and its
  module's declaration, `validate_record_secrets` checks it, and the platform holds it as it records the call;
- a hop matches a destination the call's operation declares, and connects within its `addresses`;
- a cross-module call's caller declares the callee and kind, and the named route segment declares the call;
- a provider-role operation reaches a module only if its kind is in `sandboxed.handles`;
- a module's `interface` is the same across its versions (`REQ-MDL-19`).

### Agreement with the Rust types

Every invalid fixture is also run through its Rust type. Most fail there too. The ones that deserialise, because
only a schema can reject them (a pattern on a host, `minItems`, `uniqueItems`), are listed by name with the reason
in `rust_accepts`. The test checks that list both ways.

## Tests

`contracts/tests/module-interface.rs` does these things:

- It checks that every valid fixture passes both layers and round-trips through the Rust types unchanged.
- It checks that every invalid fixture is rejected by the layer the table above names:
  - A schema rejection must give exactly the expected set of (location, keyword) errors.
  - A Rust rejection must give exactly the named violation.
  - The Rust types are checked against `rust_accepts`.
- It checks the address and address-block forms against both layers, from a table of each form's verdicts.
- It checks that a resolved secret's `Debug` output, at every level, never contains its value (`THR-50`).
- It checks that `VERSION` matches this changelog.
