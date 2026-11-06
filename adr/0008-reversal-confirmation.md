---
status: Accepted (Pablo, 2026-11-06, decision-form item `reversal-confirmation-source`)
decision_item: reversal-confirmation-source
supersedes: adopts suggestions/ADR-007-reversal-confirmation.md
---

# ADR-0008: Who confirms a reversal

## Context

`REQ-ORD-3` requires a reversal to count as complete only on a statement from a party other than the provider
that failed the segment. `suggestions/ADR-007` proposes the origin node's own reserve as that party, since it is
the party that sees the arrival first-hand and asking it adds no new party to the system.

## Options considered

1. **Public-record reading.** Only works where a public record exists; needs a host-side reader (this is what
   `CMP-EVIDENCE`, ADR-0007, already provides for public nodes) — not available at a private node.
2. **Customer confirmation.** Slow, and the customer has a stake in the answer.
3. **The failed provider's own statement.** Excluded outright by `REQ-ORD-3`'s own text.
4. **The origin node's own reserve's signed statement**, naming the segment's transaction, amount and asset only
   — never a general sufficiency claim, which would make it a balance check under `REQ-CAT-4` instead.

## Decision

Option 4. The statement must speak only to that specific credit's arrival, never to sufficiency in general.
Recorded as "the reserve's word," graded per ADR-0007's scale (typically grade 3 or 4, depending on whether the
reserve itself operates on a public or private node) — not elevated to a fact rutent independently verifies.

## Consequences

- A failed *withdrawal* (the very first segment) has no reversal to confirm by this mechanism — `REQ-ORD-40`
  already handles that case by cancelling the order outright, with no downstream effect.
- **Unresolved by this ADR, carried forward as an open design point**: a node with several reserves, where value
  returns to a *different* reserve than the one that saw it leave, has no settlement path yet. Flagged in
  `suggestions/README.md`'s own gap list; not blocking this ADR's adoption, since the common case (single reserve
  per node) is fully covered, but should become its own decision-form item before a multi-reserve node is ever
  admitted to the catalogue.
- This mechanism catches *forgery* (a false statement from a party that would need to lie outright), not
  *falsehood* (two colluding parties agreeing on a false statement) — the same caveat ADR-0007 and ADR-0009 both
  carry; it is not re-derived independently each time, but it is real in all three places.
