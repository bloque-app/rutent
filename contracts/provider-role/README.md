# provider-role contract

This contract holds six things:

- what a provider advertises and declares to join the catalogue (`T-002-01`);
- the operations each role is asked, and the results it gives (`T-002-02`);
- the verifier's standalone judgement, and the reserve's reversal confirmation;
- a provider's signed terms for a tuple, and a reserve's signed hold of a quote (3.1.0);
- a reserve's signed notice of value arriving (3.1.0);
- the envelope every party statement travels in (`REQ-PRT-1`).

It is format-neutral and language-neutral (`DEC-4`): nothing here assumes a module format.

## Schemas

| `$id` | File | Holds |
|---|---|---|
| `urn:rutent:provider-role:statement-envelope` | `schemas/statement-envelope.schema.json` | The envelope of every party statement |
| `urn:rutent:provider-role:advertisement` | `schemas/advertisement.schema.json` | A provider's advertisement, in the envelope: identifier, publisher, version, signing keys with their purposes, offers by category, declarations, private nodes |
| `urn:rutent:provider-role:declarations` | `schemas/declarations.schema.json` | What a provider declares: tracking, and for value-moving providers evidence grade, availability and safe retry |
| `urn:rutent:provider-role:reserve-declaration` | `schemas/reserve-declaration.schema.json` | A reserve's declaration at publishing (`REQ-CAT-6`) |
| `urn:rutent:provider-role:operation` | `schemas/operation.schema.json` | An operation the operator asks of a provider, in the envelope |
| `urn:rutent:provider-role:result` | `schemas/result.schema.json` | A provider's result for one operation, in the envelope |
| `urn:rutent:provider-role:judgement` | `schemas/judgement.schema.json` | A verifier's judgement, in its own envelope |
| `urn:rutent:provider-role:reversal-confirmation` | `schemas/reversal-confirmation.schema.json` | A reserve's confirmation that one reversed credit arrived, in its own envelope (ADR-0008) |
| `urn:rutent:provider-role:terms` | `schemas/terms.schema.json` | A provider's terms for one of its tuples, in the envelope: the tuple by provider and both ends, its fee formula, and its slippage regime where the tuple sets a rate (3.1.0, `REQ-QTE-5`, `REQ-QTE-6`) |
| `urn:rutent:provider-role:hold` | `schemas/hold.schema.json` | A reserve's hold of a quote, in the envelope: the choice record by id and digest, the candidate, the order or session it is for, its validity (3.1.0, `REQ-QTE-15`) |
| `urn:rutent:provider-role:arrival-notice` | `schemas/arrival-notice.schema.json` | A reserve's notice of value arriving, in the envelope: the transaction it gives the credit, its per-credit reference, the outer node, the asset with its unit, the amount, the account and its holder (3.1.0, `REQ-ORD-14`, `REQ-ORD-23`) |
| `urn:rutent:provider-role:common` | `schemas/common.schema.json` | Shared atoms: identifier, version, protocol version, public key, amount, safe integer, timestamp, evidence grade, decimal |

The schemas refer to each other by `$id`. A validator registers all twelve and needs nothing from the network.
A fixture's file name starts with its shape: `advertisement-`, `statement-envelope-`, `operation-`, `result-`,
`terms-`, `hold-`, `arrival-notice-`, `successive-advertisements-`, `answered-operation-` or `arrival-notices-`. The
last three are bundles, not schemas: `arrival-notices` is an `earlier` and a `later` enveloped arrival notice, for
the rule that relates them (`check_one_credit`). Of the other two,
`successive-advertisements` is a `previous` and a `next` enveloped advertisement, each checked against the
advertisement schema, for the rules that relate them (`validate_successor`); `answered-operation` is an enveloped
`operation`, the enveloped `result` that answers it, and the `terms` of the tuple it runs through (the
advertisement's `$defs/terms`), for the rules that relate their amounts (`check_amounts`). The other schemas are
exercised through those. A judgement and a reversal confirmation are exercised inside the results that carry them.

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
- **The encoder** (2.3.0, 2.4.0): `provider_role::canonical` is the contracts' one RFC 8785 encoder, the one the
  host contract's `exactly-once` means too. `signing_input(received)` gives the signed bytes of a statement's text;
  `canonicalize` and `to_canonical` give any text's or value's canonical form, `to_canonical` refusing NaN and
  the infinities as RFC 8785 does; `member(json, path)` gives a nested statement's own text as it sits in the
  outer one. Its parser refuses what has no canonical form: text that isn't UTF-8, a member named twice, a lone
  surrogate, a noncharacter, a number beyond a double, nesting past 128. It is tested on RFC 8785's §3.2.2, §3.2.3
  and Appendix B vectors.
- **Scheme:** `ed25519` (RFC 8032), the only one allowed, verified strictly.
- **Key:** `key` is the signing public key. For a provider, it is one of the keys its advertisement publishes, made
  authoritative by the operator's grant (`REQ-CAT-2`), with the purpose the statement needs: `advertising` for an
  advertisement or a release assertion, `statements` for anything else (see "Key purposes"). For the operator, it is
  the deployment's `operator_key`.
- **Value:** the 64-byte signature, written as 86 characters of base64url without padding.
- **Canonical encodings:** a key's last character is one of `AEIMQUYcgkosw048`, and a signature's one of
  `AQgw`. The unused bits are then zero, so each value has exactly one encoding. In Rust every key is a `PublicKey`,
  which only canonical text builds, and two keys are equal when their decoded bytes are, so an alias with nonzero
  unused bits can't pass a "differs from" check.
- **What the verifier must still refuse** (`M2`): non-canonical base64 from whatever decoder it uses (nonzero unused
  bits, padding, the `+` and `/` alphabet), and a non-canonical point: a key whose `y` coordinate is at or above the
  field prime 2^255 − 19, refused as RFC 8032 §5.1.3's strict decoding does, and a small-order key. The contract checks
  the text only, since it has no curve arithmetic.
- **Integers:** every integer that can be signed is at most 2^53 − 1, or is carried as a string, since JCS writes
  numbers as IEEE doubles. Amounts are strings. `seconds` and `settlement_seconds` are capped.

The contract states the rule and computes its signing input, but doesn't verify signatures: it has no curve
arithmetic. The Ed25519 verifier is the catalogue's (`T-012-02`).

In Rust, a reader calls two checks after deserialising a `Statement<T>`, because serde enforces neither:

- `check_shape()` checks the statement's own shape, and any statement nested in it.
- `check_context(deployment, protocol_version)` refuses a statement replayed from another deployment or protocol
  version. It recurses into a nested judgement or reversal confirmation, which must also have the outer
  statement's maker.

Three more checks bind a statement's maker to its key:

- `validate_statement` rejects an advertisement whose maker isn't its `provider`, whose signing key isn't in its
  `keys`, or is there for `statements` only, and one that lists a key twice, by its bytes, whatever the purposes.
- `validate_successor(previous, next)` rejects a later advertisement that isn't the same provider's and deployment's
  as the accepted one, wasn't made after it, isn't signed with a key the accepted one lists for `advertising`, or
  lists for `advertising` a key the accepted one lists for `statements`.
- `check_signed_by_operator` rejects an operation not signed with its deployment's `operator_key`.

## Key purposes, and what the catalogue must hold

Added in 2.0.0, after the M1 security review (`THR-4`, `REQ-CAT-2`). Every advertised key states its `purpose`:

- **`advertising`** signs the provider's advertisements and its modules' release assertions
  (`urn:rutent:module-interface:release-assertion`). Only the provider holds it; no running module can resolve it.
- **`statements`** signs everything else the provider says: results, quotes, judgements and reversal confirmations.
  A module's outside part resolves this key through the credential proxy, as its declared `statement_key`, so a
  compromised outside part holds it.

**Why.** At 1.0.0 an advertisement needed only to be signed by a key it listed itself, and a release's
`statement_key` had to be one of those keys. A compromised outside part, which can resolve its statement key, could
therefore sign a new advertisement listing a fresh key, then sign its successor release with that fresh key, and
rewrite the provider's offers on the way. Every check passed, `AssertionSignedWith{,Previous}StatementKey` included.

**Why the purposes live here.** The module-interface contract imports this one, never the reverse, so this
contract's advertisement rules can't see which keys a release declared as `statement_key`. A rule such as "an
advertisement may not be signed with a key a release declared" could only be held by the catalogue, and this
contract would carry a hole it can't check. The purpose puts the distinction in the advertisement itself.

**What the contract checks:**

| Rule | Where |
|---|---|
| Every key states its purpose, `advertising` or `statements` | schema, and Rust (`KeyPurpose`) |
| An advertisement is signed with a key it lists for `advertising`, matching the signature's scheme and the key's bytes | Rust, `validate_statement`: `SignedWithUnpublishedKey`, `SignedWithStatementsKey` |
| A key is listed once, by its bytes, so it can't hold both purposes | Rust, `validate_statement`: `KeyListedTwice` (`uniqueItems` can't see it once the purposes differ) |
| A later advertisement is the same provider's and deployment's, made after the accepted one, signed with a key the accepted one lists for `advertising`, and promotes none of its `statements` keys | Rust, `validate_successor`: `SuccessorOfAnotherProvider`, `SuccessorOfAnotherDeployment`, `SuccessorNotNewer`, `SuccessorNotSignedWithPreviousAdvertisingKey`, `StatementsKeyPromoted` |
| A module's `statement_key` is listed for `statements`, at admission and for each result; a release assertion is signed with a key listed for `advertising` | module-interface: `validate_assertion_key`'s `StatementKeyNotForStatements` and `AssertionKeyNotForAdvertising`, and `validate_statement_key`'s `StatementKeyNotForStatements` |

So a key a statements key signs in can't sign anything that grants keys: a `next` signed with a key it adds itself,
or with the accepted one's `statements` key (which `next` must then list for `advertising`, promoting it), fails
`validate_successor`.

**What the catalogue (`F-012`) must enforce,** since it holds what these checks are given:

- **First trust.** A provider's first advertisement vouches for nothing by listing its own keys. Its `advertising`
  keys are trusted only because the operator grants them at admission (`REQ-CAT-2`), never because the
  advertisement lists the key it is signed with.
- **Every later advertisement** is accepted only after `validate_successor` against the one the catalogue accepted
  last, never against one that arrives alongside it, and never on `validate_statement` alone.
- **No replay.** An older advertisement stays public and validly signed. The catalogue never re-accepts an
  advertisement that isn't newer, by `made_at`, than the last it accepted (`validate_successor` checks this against
  the one it is given, `SuccessorNotNewer`), and never re-lists a `statements` key an accepted advertisement
  dropped. Otherwise resubmitting v1 {A, S1} after v2 {A, S2} would bring the retired S1 back.
- **No far-future time.** The catalogue refuses an advertisement whose `made_at` is more than a bounded skew ahead
  of its own clock. `validate_successor` only orders times, so one leaked advertising key could otherwise sign an
  advertisement made in the year 9999, and no later advertisement could ever be newer than it, so none could pass.
- **Releases are checked against the accepted advertisement.** At admission `validate_assertion_key` is called with
  the advertisement the catalogue holds, never one that arrives with the release; it checks both the assertion's key
  and the declared `statement_key`. `validate_statement_key` is called the same way for each signed result.
- **The whole history, everywhere.** The contract compares each advertisement with the one before it. The catalogue
  keeps every earlier advertisement and admitted release, and never accepts for `advertising` a key that any earlier
  advertisement listed for `statements`, or that any admitted release declared as its `statement_key`. A key's
  purpose holds across every deployment and every provider: a key used for `statements` anywhere, by any provider
  in any deployment, is never accepted for `advertising` anywhere, since whoever resolved it there holds it here.
- **The credential proxy (`F-014`)** resolves for a module's outside part only the `statement_key` its admitted
  release declares, which is a `statements` key, and never a key listed for `advertising`. A provider's advertising
  keys are never stored where the proxy can reach them for a module.
- **Losing every advertising key** is recovered by a fresh operator grant, not by a successor advertisement.
- Retiring an advertising key takes two advertisements: one that adds the new key, signed with the old; then one
  that drops the old, signed with either.

## The advertisement in brief

- An advertisement is the provider's own statement. Its envelope's `maker` is the provider, and it signs with one
  of the keys in `keys` listed for `advertising`. Each key in `keys` states its `purpose`.
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
    `holding_balance` (a holding account's module, 3.0.0), `conversion` (exchange), `transfer` (ramp), `reversal` (exchange or ramp), or `judgement` (verifier)
    (`REQ-CAT-9`, `REQ-CAT-15`, ADR-0008).
  - The value-moving kinds carry a `transaction`, the `provider` asked, their segment, `inputs` (`REQ-CAT-12`) and
    `dry_run` (`REQ-CAT-14`). A conversion and a transfer also carry a `direction`, `exact_in` or `exact_out`,
    which says which amount is stated.
  - A withdrawal carries the holder's `session`, required on a dry run, since a dry run reveals sufficiency
    (`REQ-CAT-3`, `REQ-CAT-4`).
  - A balance verification names a session, an account, an asset and a stated amount, and is never a dry run.
  - A holding balance (3.0.0) is the operator's own question to a node's holding account: the provider, the node and
    the asset, with no session. Its result is the balance in whole units of the asset, zero included, or a refusal
    with its class. It is the module's own figure, which `CMP-CORE-AUDIT` checks against the value held for each
    order (`REQ-AUD-15`, `INV-1`); a balance verification stays a yes or no (`INV-9`).
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
- **Amounts** (2.1.0; see "Amounts against the operation"). `amount_in` is what left the segment's origin, the
  provider's charge included, whether the tuple reports gross or net: `reporting` says only whether `amount_out`
  includes the charge. The fee is in the asset the segment takes in, in the origin unit.
- **Less the charge** (3.1.0). Under `gross`, a segment that keeps its asset (a withdrawal, a deposit or a transfer)
  delivers its `amount_out` less its charge. `amount_out` is in the out unit, the operation's out asset at its
  destination decimals, and the fee in the origin unit, which differ where a reserve's two ends count in different
  decimals. So the charge subtracted is the fee restated in the out unit: multiplied out exactly where that unit is
  finer, divided and rounded up where it is coarser, so the charge is never counted below what was charged and
  what arrives is never overstated (`charge_in_out_unit`, `less_the_charge`). A charge above `amount_out` delivers
  nothing that can be stated (`None`). Under `net`, what it delivers is its `amount_out`. A conversion's output is as
  its answer states it: its fee is in its `from` asset, which it doesn't give out.
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

## Terms and holds (3.1.0)

Before 3.1.0 Core kept a provider's fee terms and a held quote only in memory, with nothing signed behind them,
though core-internal places "stated terms with their times; a held quote" among stated entries. Each is now its
maker's own statement, which Core keeps (`Evidence::keep`) and appends as a stated entry: terms about nothing, since
their body names no transaction, and a hold about its own transaction, its id.

- **Terms** (`TupleTerms`, shape `urn:rutent:provider-role:terms`). The maker is the tuple's provider. The body
  names the tuple, by its provider and both ends (an inner node by identifier or an outer node by its network, and
  the asset with its unit), the fee formula (`flat`, `proportional`, optional `maximum`, in the tuple's input
  unit), and the slippage regime, present exactly where the tuple sets a rate: an exchange's, whose ends hold
  different assets. Terms bind from the later of their `made_at` and Core's `received_at`, never `made_at` alone,
  so terms added after a segment started but dated before it never bind it (router-api's
  `StatedTerms::from_statement` builds the router's history entry so). A charge raised against a formula cites both
  the result's entry and the terms' entry (core-internal).
- **A hold** (`Hold`, shape `urn:rutent:provider-role:hold`). Made by the reserve that starts each part of the
  route. The body names its own `transaction`, its id, which its stated entry is about, so the log keys the hold
  on it; the choice round's decision record by id and digest (`record`), the candidate held, what it
  is `for`, an order or a session before its order exists, and its `valid_until`, at most the earliest validity
  among the figures the candidate cites (router-api's `HeldQuote::check_hold`, which has the record). Redeeming is
  refused for any order but the one it names, or one accepted in the session it names (`check_redemption`). The
  acceptance that redeems it names its entry (core-internal).

| Rule | Where | Invalid fixture |
|---|---|---|
| A terms statement names its shape, its tuple, its fee formula; a proportion's denominator is above zero | schema | `terms-under-another-shape`, `terms-with-a-zero-denominator` |
| Terms are made by their tuple's provider | Rust, `check_terms`: `OfAnotherMaker` | `terms-made-by-another-party` |
| Terms state a slippage regime exactly where their tuple sets a rate | Rust, `check_terms`: `NoSlippageWhereARate`, `SlippageWhereNoRate` | `terms-of-an-exchange-without-a-slippage-regime`, `terms-of-a-reserve-with-a-slippage-regime` |
| A hold names its transaction, its record by id and digest, its candidate, an order or a session, and its validity | schema | `hold-without-its-transaction`, `hold-of-a-record-without-its-digest`, `hold-for-no-order-or-session` |
| A hold is redeemed only for its order, or an order accepted in its session | Rust, `check_redemption`: `ForAnotherOrder`, `ForAnotherSession` | none: it relates a hold to its redemption; a test covers it |
| A hold names the record it holds by id and digest, which must be its held quote's | router-api, `HeldQuote::check_hold` | none here: `hold-of-another-records-candidate-of-the-same-id` is a valid statement, refused against another record in router-api's tests |

## Arrivals (3.1.0)

Before 3.1.0 provider-role had no statement a reserve makes of value arriving. Core took what arrived from the
assembly's port and kept the reserve's notice as bytes it read only for the transaction its body names, so nothing
held the figures to the signed bytes (`T-020-04`'s gap). A notice is now the reserve's own statement, which Core
keeps and appends as a stated entry.

- **The notice** (`ArrivalNotice`, shape `urn:rutent:provider-role:arrival-notice`). Made by the reserve the value
  arrived at. The body names the `transaction` the reserve gives the credit, the reserve's outer `node`, by its
  network, the `asset` with the unit the amount is counted in, the `amount`, above zero, the `account` it reached and
  the `holder`, the party that holds it (`REQ-ORD-14`, `REQ-ORD-23`).
- **The transaction is the per-credit reference.** It is the reserve's own, the same in every notice of the credit,
  whenever signed. It is the deposit's signed transaction core-internal's log keys a deposit on
  (`arrival-names-its-notice`, `one-order-per-deposit`), so the notice's stated entry is about it, as attribution
  binds an entry's transaction to the body's.
- **One credit, one arrival.** `check_one_credit(earlier, later)` says two notices of the same maker and transaction
  are one credit where they state the same figures (`Credits::One`), refuses the same transaction with another node,
  asset, unit, amount, account or holder (`CreditRestated::OtherFigures`), and takes another maker's or another
  transaction's notice as another credit (`Credits::Two`). Times and signatures are left aside.

| Rule | Where | Invalid fixture |
|---|---|---|
| A notice names its shape, its transaction, its outer node, its asset with its unit, a positive amount, its account and the account's holder | schema | `arrival-notice-under-another-shape`, `arrival-notice-without-its-transaction`, `arrival-notice-without-its-outer-node`, `arrival-notice-at-an-inner-node`, `arrival-notice-without-its-asset`, `arrival-notice-asset-without-its-unit`, `arrival-notice-without-its-amount`, `arrival-notice-of-a-zero-amount`, `arrival-notice-without-its-account`, `arrival-notice-without-its-holder` |
| Two notices of one credit state the same figures | Rust, `check_one_credit`: `OtherFigures` | `arrival-notices-of-one-credit-with-another-amount` |

What the contract leaves to its reader: that the notice's maker is a reserve whose advertisement lists that outer
node and asset (the catalogue's, `F-012`), and that a credit taken once is taken once (Core's, `T-020-19`, holding
each notice it took against the next by `check_one_credit`).

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
| Every key states its purpose, `advertising` or `statements` | schema | `advertisement-key-without-a-purpose`, `advertisement-key-of-an-unknown-purpose` |
| An advertisement is signed with a key it lists for `advertising`, and lists each key once, by its bytes | Rust, `SignedWithStatementsKey`, `KeyListedTwice` | `advertisement-signed-with-its-own-statements-key`, `advertisement-listing-one-key-for-both-purposes` |
| A later advertisement is the accepted one's provider's and deployment's, made after it, signed with a key the accepted one lists for `advertising`, promoting none of its `statements` keys | Rust, `validate_successor`: `SuccessorOfAnotherProvider`, `SuccessorOfAnotherDeployment`, `SuccessorNotNewer`, `SuccessorNotSignedWithPreviousAdvertisingKey`, `StatementsKeyPromoted` | `successive-advertisements-of-another-provider`, `successive-advertisements-of-another-deployment`, `successive-advertisements-replaying-an-older-advertisement`, `successive-advertisements-signed-with-a-key-the-next-one-adds`, `successive-advertisements-promoting-the-previous-statements-key`, `successive-advertisements-signed-with-the-previous-statements-key` (which breaks both of the last two: a statements key signs `next` only once `next` promotes it) |
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
| Keys and signatures are in their one canonical encoding | schema, and Rust for keys (`PublicKey`) | `statement-envelope-key-in-a-non-canonical-encoding`, `statement-envelope-signature-in-a-non-canonical-encoding`, `operation-signed-with-a-non-canonical-alias-of-the-operator-key` |
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
  published for `statements`, doesn't count. Keys compare by their bytes.

The contract's own Rust checks cover what one statement can show about itself: `check_context`,
`check_signed_by_operator`, and `validate_statement`'s maker and key bindings. `validate_successor` relates two
advertisements, and the catalogue holds the rest (see "Key purposes"). `check_amounts` relates a result's amounts
to its operation; `CMP-CORE-ORCH` calls it when it matches the two, and the role suite does on every answer.

### Amounts against the operation

Added in 2.1.0, after review round 1 on the M1 pull request (`INV-1`, `REQ-CAT-11`, `REQ-QTE-5`, `THR-108`).
`check_amounts(operation, result, terms)` takes an operation, the result that answers it, and the terms of the
tuple it runs through. It returns the segment's `Remainder`, or every rule broken:

- **`amount_in` is gross.** It is what left the origin, the charge included, whatever `reporting` declares. So a
  provider that counts its fee on top of the amount (`THR-108`) shows it in `amount_in`.
- **The fee is in the asset the segment takes in**: a withdrawal's, deposit's or transfer's `asset`, a conversion's
  `from`. It is in the origin unit (a conversion's `from` unit), and at most `amount_in`. The router asks the same of
  every quote and operation, so a charge can be counted against the tuple's formula (`REQ-QTE-5`).
- **`exact_in` takes in at most what it was asked.** Where the operation fixes what is taken in (`exact_in`; a
  withdrawal and a deposit always state their origin amount), `amount_in` is at most the stated `amount`. The
  difference never left the origin node: it is the segment's **remainder**, which `INV-1` counts as the reported
  remainder, at that node. At an inner node `REQ-AUD-15` counts it at the node's holding account; a withdrawal's
  remainder never left its outer node, so it never entered the book.
- **A remainder is declared.** It is below the tuple's fill `increment`, or zero where the tuple declares none
  (`REQ-CAT-11`: a report in undeclared form is flagged). Nothing requires what is taken in to be a multiple of the
  increment, since SPEC sets no such rule.
- **Dry-run answers** are held to the same rules, since their figures price the execution, and name no remainder.
- **`exact_out` names none either.** Its operation states what is given out, so what stays at the origin follows
  from the quote it was priced on, not from the operation. That is the state machine's (`F-010`, `F-020`), with the
  question of which amount a following segment moves (review round 1, finding 2).

The check assumes both statements passed their schemas, and is about amounts only: the other rules across
statements above still apply.

| Rule | `AmountMismatch` | Invalid fixture |
|---|---|---|
| The result is of the operation's kind and names its transaction | `AnswersAnotherOperation` | `answered-operation-for-another-transaction` |
| The fee is in the asset the segment takes in, for a settlement and a quote | `FeeInAnotherAsset` | `answered-operation-charging-its-fee-in-another-asset`, `answered-operation-quoting-its-fee-in-another-asset` |
| The fee is at most `amount_in` | `FeeAboveAmountIn` | `answered-operation-charging-more-than-it-took-in` |
| `exact_in` takes in at most the stated amount | `TookInMoreThanStated` | `answered-operation-taking-its-fee-on-top-of-the-stated-amount` |
| A remainder needs a declared increment | `UndeclaredRemainder` | `answered-operation-leaving-an-undeclared-remainder` |
| A remainder is below the increment | `RemainderOfAWholeIncrement` | `answered-operation-leaving-a-whole-increment-behind` |
| Amounts are whole numbers | `NotAnAmount` | none: the schemas refuse them first; a unit test covers it |

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

Every fixture names `protocol_version` `2.0.1`, the contract's version when the fixtures were last rewritten (it was
`1.0.0` at `C1`, `2.0.0` before the final security review's fix; 2.1.0 added rules and left them as they were),
except `v3` in
`statement-envelope-protocol-version-not-major-minor-patch`, which is malformed on purpose (the module-interface
fixture `successive-releases-under-another-protocol-version` also keeps old versions, for a mismatch). No check compares a fixture's protocol version with
`VERSION`: the tests run `check_context` against each fixture's own deployment and protocol version, since a fixture stands
for a statement made at some deployment. The role suite's `SuiteContext` defaults to `VERSION`, so the stand-ins'
statements carry `2.0.1`. The stand-ins sign their advertisements with an `advertising` key and everything else with
a `statements` key, and the suite counts a result only if its key is published for `statements`.
