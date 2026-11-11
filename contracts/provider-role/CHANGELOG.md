# provider-role changelog

## 3.0.0
- Breaking: a holding account's balance gets its own operation and result, `holding_balance`, so `CMP-CORE-AUDIT`
  reads the balance from the account's module and not from the host's own record of what it sent it. Before, no
  shape carried a balance (a `balance_verification` answers yes or no for a stated amount, `INV-9`), so the plain
  wallet's host side could report only the sum it had itself recorded, and a wallet that lost its record (a
  restarted provider process) still looked whole. The operation names the provider, the node and the asset, and no
  session: it is the operator's own question and changes nothing. The result is the balance in whole units of the
  asset, zero included, or a refusal with its class. `Operation` and `ResultBody` gain a variant and the schemas
  gain the kind, so a reader on 2.x rejects it on the wire and an exhaustive Rust match breaks: a major (`DEC-4`).
  Valid and invalid fixtures, and Rust tests. Fixtures keep naming protocol 2.0.1. (DEC-4, DEC-20, T-016-02)

## 2.4.0
- A verifier gets the statement's bytes as received: `signing_input(received)` gives the bytes a maker signs under
  the signing rule, the RFC 8785 canonical form of the statement's text with only the top-level `signature.value`
  removed, refusing text that isn't an envelope (`NotAnEnvelope`) or isn't I-JSON. `member(json, path)` gives a
  nested statement's own text exactly as it sits in the outer one, such as a judgement in a verifier's result.
  The role suite's `SignatureVerifier` is handed those bytes (`T-010-03`; F-002's notes: the verifier canonicalises
  the JSON as received, never a re-serialisation). Minor: two functions added, no schema, type or rule changes.
  (DEC-4, DEC-20, T-010-03)

## 2.3.0
- One canonical JSON encoder, `provider_role::canonical`, written to RFC 8785, the scheme the signing rule and the
  host contract's `exactly-once` name: `canonicalize(json)` and `to_canonical(value)`, which refuses a value holding
  NaN or an infinity (`NonFiniteNumber`) before `serde_json` could write it as `null`. Members sort by their names'
  UTF-16 code units; numbers are written as the doubles they denote, as ECMAScript writes them, ties to even; strings
  escape only `"`, `\` and the C0 controls. Its own parser refuses duplicate member names, lone surrogates,
  noncharacters (RFC 7493 §2.1), numbers beyond a double and nesting past 128, where `serde_json::Value` would keep
  the last of two members silently. It passes RFC 8785's §3.2.2, §3.2.3 and Appendix B vectors, the NaN and Infinity
  rows included, and expands a number's exact value only where an even neighbour of its shortest digits reads back, so
  a long array of short numbers costs no expansion. Before, both rules named RFC 8785 and no code in `contracts/`
  wrote it; the VOS program's dedup and the native client wrote `serde_json`'s own form (`EXP-1` outcome, finding 7).
  Minor: added, no schema, type or rule changes, and the host contract's wording now has the encoder it names. (DEC-4,
  DEC-20, DEC-26, T-010-03)

## 2.2.0
- The Rust code behind this contract (`contracts/src/provider_role`) is `no_std` with `alloc`:
  it uses `core` and `alloc` paths, and the crate's
  default `std` feature turns on `std` only for its dependencies and tests. Minor, not major: no schema, Rust type or
  behaviour changes, and every consumer that builds with default features builds unchanged. Fixtures keep naming
  their current version. (DEC-4, DEC-20, DEC-26, T-010-01)

## 2.1.0
- After review round 1 on the M1 pull request (findings 3 and 6): `check_amounts(operation, result, terms)`
  publishes the rules that tie a result's amounts to the operation it answers, and returns the segment's
  `Remainder`. A result's `amount_in` is what left the origin, the charge included, whether the tuple reports gross
  or net. Its fee is in the asset the segment takes in (a conversion's `from`), in the origin unit, and at most
  `amount_in`, as the router counts it. For `exact_in`, and always for withdrawals and deposits, `amount_in` is at
  most the stated amount; the difference is the remainder at the origin node, below the tuple's fill increment, or
  zero where it declares none (`INV-1`, `REQ-CAT-11`, `REQ-QTE-5`, `THR-108`). Dry-run answers are held to the same
  rules. `answered-operation-` bundle fixtures; the role suite applies the check. Minor, not major: no schema or
  Rust type changes, every valid fixture stays valid, and the fee rule asks of a result what the router already
  asks of every quote and operation. Fixtures keep naming 2.0.1. (DEC-4, DEC-20, T-002-03)

## 2.0.1
- The newer-than check for `made_at` now fails closed: a time that isn't in the schema's form (UTC `Z`, at most 9
  fraction digits, so no `+05:00` offset) is never newer, instead of being compared as text. The README adds that
  the catalogue refuses an advertisement made more than a bounded skew ahead of its own clock. Every fixture's
  `protocol_version` names 2.0.1. (DEC-4, DEC-20, T-002-03)

## 2.0.0
- Breaking, after the M1 security review: every advertised key states its `purpose`, required, `advertising` or
  `statements`. An advertisement is signed with an `advertising` key it lists (`SignedWithStatementsKey`), and a
  key is listed once by its bytes, whatever the purposes (`KeyListedTwice`). `validate_successor(previous, next)`
  checks a later advertisement against the accepted one: same provider and deployment, made after it
  (`SuccessorNotNewer`, so a replayed older advertisement can't undo a key's removal), signed with a key the
  accepted one lists for `advertising`, scheme and bytes, and promoting none of its `statements` keys. So a module's outside part,
  which resolves its statement key, can no longer sign an advertisement that adds a key and then sign its
  successor release with it, nor rewrite the offers. The README states what the catalogue (`F-012`) must hold:
  the first advertisement's keys by the operator's grant, no replay of an older advertisement or a dropped
  statements key, and a key's purpose across the whole history, every deployment and every provider; and that the
  credential proxy (`F-014`) resolves only a release's declared statements key.
  (DEC-4, DEC-20, T-002-03)
- Breaking: every key in the Rust envelope and types is a `PublicKey`: `Deployment.operator_key`,
  `Signature.key`, `SigningKey.public_key` and a judgement's `holder_key`. It is built only from the canonical
  encoding and compares by its decoded bytes, so an alias with nonzero unused bits can't pass a "differs from"
  check; the docs state that the verifier (`M2`) must refuse non-canonical base64 and non-canonical points.
  Fixtures name protocol version 2.0.0. (DEC-4, DEC-20, T-002-03)

## 1.0.0
- Frozen at C1: from here a change needs a version bump, a changelog entry naming a DEC-*, and the owner's click. (DEC-3, DEC-4, DEC-20, T-006-01)

## 0.4.0
- The reversal operation, added in T-002-03 after review, since T-002-02's operation list lacked it: an
  exchange's or ramp's `reversal` of a segment, answered by a true undo with evidence, outcome unknown, or a
  refusal that leaves it owed, never a failure (REQ-CAT-15, REQ-ORD-3); and the reserve's
  `reversal_confirmation`, answered by its own signed statement (`urn:rutent:provider-role:reversal-confirmation`)
  that one credit arrived, naming only the segment's transaction, amount and asset, or by `not_confirmed`.
  A nested confirmation shares its result's context and maker. (DEC-4, DEC-14, ADR-0008, T-002-03)
- No optional field accepts `null`: a judgement's `holder_key`, a withdrawal's `session`, a tuple's `increment`,
  the `evidence` and `safe_retry` declarations and each offer are either present with a value or absent, in Rust
  as in the schemas. (DEC-4, T-002-03)

## 0.3.0
- Operation and result schemas per role, with the statement envelope every party statement travels in:
  withdrawal, deposit, transfer, conversion with a detailed quote, balance verification and judgement; dry runs;
  outcome unknown as its own result; refusal classes. Fixtures are now named by shape. (DEC-4, DEC-16, T-002-02)
- Tightened after review: the signing rule (RFC 8785 canonical JSON, Ed25519, integers at most 2^53 - 1); a
  deployment bound to its operator's key; the advertisement enveloped, with the provider's signing keys; a
  standalone enveloped judgement with an optional holder key, and a judgement question without a transaction;
  evidence as a list naming whose each piece is; the signed quote's direction, slippage regime, what voids it and
  settlement time; a session on withdrawal dry runs; no lifting time on balance refusals. (DEC-4, T-002-02)
- Makers bound to keys: an advertisement is made by its provider and signed with a key it publishes; an
  operation is signed with its deployment's operator key; a nested judgement shares its result's context and
  maker. Keys and signatures have one canonical encoding. A reserve's dry-run answer states what voids it and
  its settlement time. The signing rule says the top-level signature.value, the JSON as received, and no
  duplicate member names. (DEC-4, T-002-02)

## 0.2.0
- Tightened after review: a tuple listed twice is invalid; the evidence grade is the provider's weakest; private
  nodes are unique identifiers and an outer node names only its network, both implied by the advertisement; ramp
  and reserve tuples declare their unit at each end; a verifier-only provider needn't declare evidence or safe
  retry; the node schema is removed. (DEC-4, DEC-13, DEC-16, T-002-01)
- Integers must be plain JSON integers, which the Rust types enforce and the README records; a reserve tuple
  listed twice gets its own invalid fixture. (DEC-4, T-002-01)

## 0.1.0
- Advertisement and declaration schemas: the four categories, tuples per role, private nodes and their
  publisher, per-tuple terms, evidence, safe retry, tracking, required inputs and a reserve's declaration.
  (DEC-4, DEC-13, DEC-16, T-002-01)
