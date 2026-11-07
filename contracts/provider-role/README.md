# provider-role contract

This contract holds four things:

- what a provider advertises and declares to join the catalogue (`T-002-01`);
- the operations each role is asked, and the results it gives (`T-002-02`);
- the verifier's standalone judgement, and the reserve's reversal confirmation;
- the envelope every party statement travels in (`REQ-PRT-1`).

It is format-neutral and language-neutral (`DEC-4`): nothing here assumes a module format.

## Schemas

| `$id` | File | Holds |
|---|---|---|
| `urn:rutent:provider-role:statement-envelope` | `schemas/statement-envelope.schema.json` | The envelope of every party statement |
| `urn:rutent:provider-role:advertisement` | `schemas/advertisement.schema.json` | A provider's advertisement, in the envelope: identifier, publisher, version, signing keys, offers by category, declarations, private nodes |
| `urn:rutent:provider-role:declarations` | `schemas/declarations.schema.json` | What a provider declares: tracking, and for value-moving providers evidence grade, availability and safe retry |
| `urn:rutent:provider-role:reserve-declaration` | `schemas/reserve-declaration.schema.json` | A reserve's declaration at publishing (`REQ-CAT-6`) |
| `urn:rutent:provider-role:operation` | `schemas/operation.schema.json` | An operation the operator asks of a provider, in the envelope |
| `urn:rutent:provider-role:result` | `schemas/result.schema.json` | A provider's result for one operation, in the envelope |
| `urn:rutent:provider-role:judgement` | `schemas/judgement.schema.json` | A verifier's judgement, in its own envelope |
| `urn:rutent:provider-role:reversal-confirmation` | `schemas/reversal-confirmation.schema.json` | A reserve's confirmation that one reversed credit arrived, in its own envelope (ADR-0008) |
| `urn:rutent:provider-role:common` | `schemas/common.schema.json` | Shared atoms: identifier, version, protocol version, public key, amount, safe integer, timestamp, evidence grade, decimal |

The schemas refer to each other by `$id`. A validator registers all nine and needs nothing from the network.
A fixture's file name starts with its shape: `advertisement-`, `statement-envelope-`, `operation-` or `result-`.
The other schemas are exercised through those four. A judgement and a reversal confirmation are exercised inside the
results that carry them.

## The envelope and the signing rule

`urn:rutent:provider-role:statement-envelope` is shared: the router contract refers to it by `$id` for quotes and
decision records. All its fields are required:

- `deployment` `{name, operator_key}`: a globally unique deployment, bound to its operator by the operator's
  Ed25519 public key (`THR-8`).
- `protocol_version`: `major.minor.patch`. The statement is void at any other deployment or version
  (`REQ-PRT-1`).
- `shape`: the `$id` of the statement's schema. It is signed with the rest, so a result can't be presented as a
  quote. A schema for one kind of statement `$ref`s the envelope, fixes `shape` with `const`, and narrows
  `statement`.
- `maker` and `made_at`: who made the statement, and when, in RFC 3339 UTC (`REQ-AUD-2`).
- `signature` `{scheme, key, value}`: the maker's signature (`REQ-QTE-1`, `REQ-AUD-2`).
- `statement`: the statement itself.

**Signing rule** (ruled by the orchestrator, 2026-11-06):

- **What is signed:** the RFC 8785 (JCS) canonical JSON of the whole envelope, with only the top-level
  `signature.value` removed. So `scheme`, `key`, `shape`, `deployment` and `protocol_version` are all signed, and
  so is a nested statement's whole envelope, its own `signature.value` included.
- **How:** the verifier canonicalises the JSON as received, never a re-serialisation of parsed types. A statement
  with a duplicate member name is rejected.
- **Scheme:** `ed25519` (RFC 8032), the only one allowed, verified strictly.
- **Key:** `key` is the signing public key. For a provider, it is one of the keys its advertisement publishes, made
  authoritative by the operator's grant (`REQ-CAT-2`). For the operator, it is the deployment's `operator_key`.
- **Value:** the 64-byte signature, written as 86 characters of base64url without padding.
- **Canonical encodings:** a key's last character is one of `AEIMQUYcgkosw048`, and a signature's one of
  `AQgw`. The unused bits are then zero, so each value has exactly one encoding.
- **Integers:** every integer that can be signed is at most 2^53 − 1, or is carried as a string, since JCS writes
  numbers as IEEE doubles. Amounts are strings. `seconds` and `settlement_seconds` are capped.

The contract states the rule but doesn't verify signatures. Verifying them is `M2` and `M3` work.

In Rust, a reader calls two checks after deserialising a `Statement<T>`, because serde enforces neither:

- `check_shape()` checks the statement's own shape, and any statement nested in it.
- `check_context(deployment, protocol_version)` refuses a statement replayed from another deployment or protocol
  version. It recurses into a nested judgement or reversal confirmation, which must also have the outer
  statement's maker.

Two more checks bind a statement's maker to its key:

- `validate_statement` rejects an advertisement whose maker isn't its `provider`, or whose signing key isn't in its
  `keys`.
- `check_signed_by_operator` rejects an operation not signed with its deployment's `operator_key`.

## The advertisement in brief

- An advertisement is the provider's own statement. Its envelope's `maker` is the provider, and it signs with one
  of the keys in `keys`.
- `offers` is keyed by category, `reserve`, `exchange`, `ramp` and `verifier`, with at least one (`REQ-CAT-8`).
- Each role has its own tuple shape (`REQ-CAT-13`):
  - **exchange:** `{node, from, to, terms}`, one inner node by construction. `from` and `to` each name an asset
    and its `decimals`.
  - **ramp:** `{origin, destination, asset, decimals, terms}`, between inner nodes, keeping one asset by
    construction.
  - **reserve:** `{origin, destination, asset, decimals, terms}`. One end is an outer node `{network}` and the
    other an inner node. Outer to inner is a withdrawal; inner to outer is a deposit (`REQ-CAT-17`).
  - **verifier:** `{classes}`, with no tuples (`DEC-16`).
- In ramp and reserve tuples, `decimals` is `{origin, destination}`, the unit at each end (`REQ-CAT-11`).
- The advertisement implies what it would otherwise repeat:
  - An outer node belongs to the advertising reserve.
  - `private_nodes` lists unique identifiers, published by the advertisement's `publisher` (`REQ-CAT-20`,
    `REQ-CAT-22`).
  - A reserve declaration's `maker` must be the statement's maker (`REQ-CAT-6`).
- `terms` holds:
  - inclusive amount `bands`, in the origin (or `from`) unit;
  - an optional fill `increment`;
  - `reporting`, `gross` or `net`;
  - `required_inputs` (`REQ-CAT-11`, `REQ-CAT-12`).
- `declarations` holds:
  - `tracking`, for every provider (`REQ-CAT-16`);
  - `evidence`: the weakest grade the provider gives, and its availability (`REQ-CAT-10`, `REQ-ORD-39`);
  - `safe_retry`.

  A provider that advertises a reserve, exchange or ramp declares `evidence` and `safe_retry`; a verifier-only
  provider may leave them out (`DEC-16`).
- A holding account doesn't advertise through this contract (`REQ-CAT-23`, `REQ-ORD-16`).
- No shape rules out an exchange resolving orders on a book (`REQ-EXT-2`).

## Operations and results in brief

- **Operations.** An operation is the operator's statement, signed with its `operator_key`.
  - Its `kind` is one of `withdrawal`, `deposit`, `balance_verification` and `reversal_confirmation` (reserve),
    `conversion` (exchange), `transfer` (ramp), `reversal` (exchange or ramp), or `judgement` (verifier)
    (`REQ-CAT-9`, `REQ-CAT-15`, ADR-0008).
  - The value-moving kinds carry a `transaction`, the `provider` asked, their segment, `inputs` (`REQ-CAT-12`) and
    `dry_run` (`REQ-CAT-14`). A conversion and a transfer also carry a `direction`, `exact_in` or `exact_out`,
    which says which amount is stated.
  - A withdrawal carries the holder's `session`, required on a dry run, since a dry run reveals sufficiency
    (`REQ-CAT-3`, `REQ-CAT-4`).
  - A balance verification names a session, an account, an asset and a stated amount, and is never a dry run.
  - A judgement question names the verifier, a subject reference and a class, and nothing else: no transaction
    (`REQ-CPL-4`).
  - A reversal names its own `transaction`, the `provider` asked, the transaction it `reverses`, and the segment's
    origin `node`, `asset` and full `amount`, where the value goes back. It is never a dry run. A reserve isn't
    asked to reverse (`REQ-CAT-15`).
  - A reversal confirmation asks the reserve at the reversed segment's origin node whether one credit arrived:
    its own `transaction`, the `provider` asked, the `segment`, `asset` and `amount`. It names no session or
    account, so it can't be used as a balance check (ADR-0008, `REQ-CAT-4`).
- **Results.** A result is the provider's signed statement. That envelope counts as a reserve's "signed statement
  for its own book" (`REQ-ORD-33`). A result has the operation's `kind`, the `transaction` it answers (not for a
  judgement), and an `outcome` by `status`:
  - **withdrawal, deposit, transfer:** `succeeded` (amounts in and out, fee, evidence), `failed` (evidence),
    `unknown` (nothing), `refused` (a refusal), or, for a dry run, `dry_run_accepted`.
  - **conversion:** the same, with the rate on success and in the quote.
  - **balance verification:** `answered` with `sufficient` true or false and nothing else (`INV-9`), or
    `refused`, whose class is `waits_on_person` or `final` but never says when it lifts.
  - **judgement:** `judged`, carrying the verifier's whole enveloped judgement (`urn:rutent:provider-role:judgement`:
    subject reference, class, `yes` or `no`, validity, optional `holder_key`), or `refused`.
  - **reversal:** `succeeded` (the origin `node`, `asset` and `amount` back, and evidence), `unknown`, or
    `refused`, which leaves the reversal owed (`REQ-CAT-7`). There is no `failed`: only a true undo counts
    (`REQ-CAT-15`). The provider's own word doesn't complete it (`REQ-ORD-3`).
  - **reversal confirmation:** `confirmed`, carrying the reserve's whole enveloped confirmation
    (`urn:rutent:provider-role:reversal-confirmation`: segment, asset and amount, and nothing else), `not_confirmed`
    (nothing more), or `refused`.
- **No `null`.** An optional field is present with a value or absent. Rust refuses `null` as the schemas do.
- **Evidence** is a non-empty list of `{grade, reference, by}`, where `by` says whose evidence each piece is. A
  public record and a reserve's statement can then both close an order (`REQ-ORD-33`, `REQ-ORD-39`, ADR-0007).
- **The quote.** A ramp's and an exchange's `dry_run_accepted` is the quote the provider signs (`REQ-CAT-9`,
  `REQ-QTE-1`, `REQ-QTE-3`). It has:
  - `amount_in`, `amount_out`, `fee`, and for an exchange `rate` and `slippage`;
  - `direction`;
  - `valid_until`;
  - `voided_by`;
  - `settlement_seconds`.

  A reserve's dry-run answer carries amounts, fee, validity, `voided_by` and `settlement_seconds`: `REQ-QTE-1` and
  `REQ-QTE-3` apply to every segment. Direction and slippage don't apply there, because a reserve states the
  origin amount and sets no rate.
- **Refusals.** A refusal's `class` is `lifts_at_known_time` (with `lifts_at`), `waits_on_person` or `final`
  (`REQ-CAT-18`).

## Which rule lives where

A valid advertisement passes the schemas and deserialises into
`rutent_contracts::provider_role::Statement<Advertisement>`. It also gets `Ok` from
`provider_role::validate_statement`. Two kinds of rule stay in Rust, because JSON Schema can't express them:
comparing two fields, and comparing a key across list entries.

| Rule | Where | Invalid fixture |
|---|---|---|
| An advertisement names its own shape and lists its signing keys | schema | `advertisement-under-another-shape`, `advertisement-without-signing-keys` |
| An exchange spans one inner node | schema (`exchange_tuple` has one `node`) | `advertisement-exchange-spanning-two-nodes`, `advertisement-exchange-outside-an-inner-node` |
| A ramp keeps its asset | schema (`ramp_tuple` has one `asset`) | `advertisement-ramp-changing-its-asset` |
| A ramp runs between inner nodes | schema | `advertisement-ramp-to-an-outer-node` |
| A ramp's two nodes differ | Rust, `RampWithinOneNode` | `advertisement-ramp-within-one-node` |
| A reserve connects an outer and an inner node | schema (`oneOf`) | `advertisement-reserve-between-two-inner-nodes`, `advertisement-reserve-between-two-outer-nodes` |
| An outer node names only its network | schema | `advertisement-outer-node-naming-its-reserve` |
| A reserve declares both answers, its maker and its version | schema | `advertisement-reserve-without-declaration`, `advertisement-reserve-declaration-without-maker`, `advertisement-reserve-declaration-without-automatic-deposit-answer` |
| A reserve declaration's maker is the statement's maker | Rust, `ReserveDeclarationOfAnotherMaker` | `advertisement-reserve-declaration-of-another-maker` |
| An advertisement is made by its provider, signed with a key it publishes | Rust, `AdvertisementOfAnotherMaker`, `SignedWithUnpublishedKey` | `advertisement-made-by-another-party`, `advertisement-signed-with-an-unpublished-key` |
| A verifier advertises classes, never tuples | schema (`additionalProperties: false`) | `advertisement-verifier-advertising-a-tuple`, `advertisement-verifier-without-classes` |
| At least one offer, only the four categories | schema | `advertisement-without-offers`, `advertisement-offer-under-an-unknown-category` |
| An offer has at least one tuple | schema | `advertisement-offer-without-tuples` |
| An offer lists each tuple once: ramp and reserve keyed on origin, destination and asset; exchange on node, from and to | Rust, `TupleDeclaredTwice` | `advertisement-ramp-tuple-declared-twice`, `advertisement-reserve-tuple-declared-twice`, `advertisement-exchange-tuple-declared-twice` |
| Each tuple has bands, gross or net, required inputs, and units at each end | schema | `advertisement-tuple-without-amount-bands`, `advertisement-tuple-without-gross-or-net`, `advertisement-tuple-without-required-inputs-declared`, `advertisement-tuple-without-units-at-its-destination`, `advertisement-exchange-asset-without-units` |
| Amounts are whole numbers; an increment is above zero | schema | `advertisement-band-amount-with-a-fraction`, `advertisement-zero-fill-increment` |
| A band's minimum is at or below its maximum | Rust, `BandMinAboveMax` | `advertisement-band-with-min-above-max` |
| An identifier or a status query, verifiers included | schema (`minItems: 1`) | `advertisement-provider-without-identifier-or-status-query`, `advertisement-verifier-without-identifier-or-status-query` |
| A value-moving provider declares evidence and safe retry | schema (`if`/`then`) | `advertisement-value-moving-provider-without-evidence-declaration`, `advertisement-provider-without-safe-retry-declaration` |
| The evidence grade is 1 to 4, its availability is declared, and its seconds are at most 2^53 − 1 | schema, and Rust (`EvidenceGrade`, `SafeInteger`) | `advertisement-evidence-grade-out-of-range`, `advertisement-evidence-without-availability`, `advertisement-evidence-availability-beyond-two-to-the-fifty-three` |
| The provider names its publisher | schema | `advertisement-provider-without-publisher` |
| A private node is declared once | schema (`uniqueItems`) | `advertisement-private-node-declared-twice` |
| Integers (`grade`, `seconds`, `decimals`, `max_bps`, `settlement_seconds`) are written as plain JSON integers, such as `2`, not `2.0` or `1e3` | Rust, when deserialising into the shape's type | none: JSON Schema's `integer` accepts whole-valued numbers such as `2.0`, and `multipleOf: 1` does too. Only a pattern on the text could reject them, and a schema sees numbers, not text |

The envelope, operation and result rules live in the schemas, except the four bindings Rust checks. Kind, status,
class and regime each pick one closed shape, so a field that belongs to another case is rejected where it sits.

| Rule | Where | Invalid fixture |
|---|---|---|
| A statement names its deployment, protocol version, maker, time and signature | schema | `statement-envelope-without-deployment`, `statement-envelope-without-protocol-version`, `statement-envelope-without-signature` |
| A deployment names its operator's key | schema | `statement-envelope-deployment-without-operator-key` |
| Its time is RFC 3339 UTC, its protocol version `major.minor.patch`, its shape a rutent URN | schema | `statement-envelope-made-at-not-a-utc-time`, `statement-envelope-protocol-version-not-major-minor-patch`, `statement-envelope-naming-no-shape` |
| It is signed with Ed25519, in unpadded base64url | schema, and Rust for the scheme | `statement-envelope-signed-with-another-scheme`, `statement-envelope-signature-with-padding` |
| Keys and signatures are in their one canonical encoding | schema | `statement-envelope-key-in-a-non-canonical-encoding`, `statement-envelope-signature-in-a-non-canonical-encoding` |
| An operation is signed with its deployment's operator key | Rust, `check_signed_by_operator` | `operation-signed-with-another-key` |
| A nested judgement belongs to the same deployment and protocol version, and has the outer maker | Rust, `check_context` | `result-judgement-from-another-deployment`, `result-judgement-made-by-another-verifier` |
| A nested reversal confirmation does too | Rust, `check_context` | `result-reversal-confirmation-from-another-deployment`, `result-reversal-confirmation-made-by-another-reserve` |
| A statement names its own shape | schema (`const`), and Rust `check_shape` | `advertisement-under-another-shape`, `operation-under-another-shape`, `result-under-another-shape`, `result-judgement-under-another-shape`, `result-reversal-confirmation-under-another-shape` |
| An operation is one of the eight kinds | schema | `operation-of-an-unknown-kind` |
| A reversal names the segment it reverses, a positive amount, and is never a dry run | schema | `operation-reversal-without-the-segment-it-reverses`, `operation-reversal-of-a-zero-amount`, `operation-reversal-as-a-dry-run` |
| A reversal confirmation names no account | schema | `operation-reversal-confirmation-asking-about-an-account` |
| An optional field is never `null` | schema, and Rust (`present`) | `operation-withdrawal-with-a-null-session`, `result-judgement-with-a-null-holder-key`, `advertisement-tuple-with-a-null-increment`, `advertisement-with-a-null-evidence-declaration`, `advertisement-with-a-null-safe-retry-declaration`, `advertisement-with-a-null-offer` |
| An operation names its transaction, and a withdrawal goes into an inner node | schema | `operation-without-a-transaction`, `operation-withdrawal-into-an-outer-node` |
| A withdrawal dry run names its session | schema (`if`/`then`) | `operation-withdrawal-dry-run-without-a-session` |
| A conversion or transfer states its direction | schema | `operation-conversion-without-a-direction` |
| A balance verification states an amount and is never a dry run | schema | `operation-balance-verification-without-an-amount`, `operation-balance-verification-as-a-dry-run` |
| Amounts are whole numbers; inputs are named as tuples declare them | schema | `operation-with-a-fractional-amount`, `operation-with-an-input-of-a-bad-name` |
| A judgement question carries a subject reference and a class only | schema | `operation-judgement-question-carrying-personal-data`, `operation-judgement-question-carrying-a-transaction` |
| A balance answer is yes or no and nothing else, and a balance refusal never says when it lifts | schema | `result-balance-answer-carrying-an-amount`, `result-balance-answer-neither-yes-nor-no`, `result-balance-refusal-saying-when-it-lifts` |
| A refusal states one of three classes, with a time only when it lifts at one | schema | `result-refusal-without-its-class`, `result-refusal-of-an-unknown-class`, `result-refusal-lifting-at-no-stated-time`, `result-final-refusal-carrying-a-time` |
| Outcome unknown carries nothing; success and failure carry at least one piece of evidence, each saying whose it is | schema | `result-outcome-unknown-carrying-amounts`, `result-success-without-evidence`, `result-success-with-no-evidence-listed`, `result-failure-without-evidence`, `result-evidence-not-saying-whose-it-is`, `result-evidence-graded-five` |
| A conversion states its rate; other kinds don't | schema | `result-conversion-success-without-a-rate`, `result-transfer-success-carrying-a-rate` |
| A reserve's dry-run answer states what voids it and its settlement time | schema | `result-reserve-dry-run-answer-without-what-voids-it` |
| A quote states its validity, direction, what voids it and its settlement time; an exchange's states its slippage regime, a ramp's has none | schema | `result-dry-run-quote-without-validity`, `result-quote-without-a-direction`, `result-quote-without-what-voids-it`, `result-transfer-quote-without-a-settlement-time`, `result-conversion-quote-without-a-slippage-regime`, `result-transfer-quote-with-a-slippage-regime` |
| A settlement time is at most 2^53 − 1 seconds | schema, and Rust (`SafeInteger`) | `result-settlement-time-beyond-two-to-the-fifty-three` |
| An outcome has one of its kind's statuses | schema | `result-of-an-unknown-status` |
| A judgement answers yes or no, with its validity, and its result names no transaction | schema | `result-judgement-answering-not-known`, `result-judgement-without-validity`, `result-judgement-carrying-a-transaction` |
| A reversal succeeds with something back and evidence, and never fails | schema | `result-reversal-succeeded-without-evidence`, `result-reversal-succeeded-with-nothing-back`, `result-reversal-failed` |
| A reversal confirmation speaks only to its credit, never to sufficiency | schema | `result-reversal-confirmation-saying-whether-funds-suffice`, `result-reversal-confirmation-naming-a-balance` |

### Rules across statements

These rules relate one statement to another, so no schema or single-statement check can hold them. The reader
that matches a result to its operation holds them: `CMP-CORE-ORCH`, under `REQ-ORD-1`, which counts a result only
if it is attributable to that segment's provider and matches that transaction.

- A `dry_run_accepted` answering an operation that wasn't a dry run doesn't count.
- A `succeeded`, `failed` or `unknown` answering a dry run doesn't count.
- A result whose envelope `maker` isn't the operation's `provider`, or whose signing key isn't one that provider
  published, doesn't count.

The contract's own Rust checks cover what one statement can show about itself: `check_context`,
`check_signed_by_operator`, and `validate_statement`'s maker and key bindings.

### Agreement with the Rust types

Every invalid fixture is also run through its Rust type, with `check_shape`. Most fail there too. Some deserialise,
because only a schema can reject them. These use a pattern, `minItems`, `uniqueItems`, `minProperties`,
`propertyNames`, a `oneOf` between two node kinds, or an `if`/`then`. The test lists them by name, with the reason,
in `rust_accepts`. It checks the list both ways: everything on it deserialises, and nothing off it does.

Some rules this contract doesn't check, because they need the catalogue (`F-012`):

- that every inner node in a tuple is on the operator's public list or is a declared private node (`REQ-CAT-22`);
- that a declaration is recorded once and shown before acceptance (`REQ-CAT-6`);
- the operator's evidence floor per tuple (`REQ-ORD-39`).

## Tests

`contracts/tests/provider-role.rs` does three things:

- It checks that every valid fixture passes both layers and round-trips through the Rust types unchanged.
- It checks that every invalid fixture is rejected by the layer the tables above name:
  - A schema rejection must give exactly the expected set of (location, keyword) errors.
  - A Rust rejection must give exactly the named violation.
  - The Rust types are checked against `rust_accepts`.
- It checks that `VERSION` matches this changelog.

The fixtures keep `protocol_version` `0.3.0`, the version they were written at, on purpose. No check compares a
fixture's protocol version with `VERSION`: the tests run `check_context` against each fixture's own deployment and
protocol version, since a fixture stands for a statement made at some deployment. The role suite's `SuiteContext`
defaults to `VERSION`, so the stand-ins' statements carry `0.4.0`. `T-006-01` moves both to `1.0.0` at the freeze.
