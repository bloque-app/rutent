# provider-role changelog

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
