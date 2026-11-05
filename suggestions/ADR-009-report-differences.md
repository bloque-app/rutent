# ADR-009: Detecting differences between adjacent providers' reports

Non-binding. See `README.md`.

## Context

rutent isn't a ledger, so conservation is read across what providers report (`INV-1`). What one provider reports sending is compared with what the next party reports receiving, and a difference raises the order to the operator (`REQ-ORD-9`).

Standing rule from the owner: reconciliation is detection only. It never completes an order, and it is never a patch for a gap in the primary path.

## Options

**Compare per order, when the second report is recorded.** Raises quickly. Needs both reports in comparable form.

**Periodic batch comparison against provider statements.** Works without cooperation at call time, and catches silent divergence. Delayed, and confirms only what a provider reports.

**Leave it to the operator's own review.** Nothing to build. Doesn't meet `REQ-ORD-9`.

## What we'd try first, and why

The per-order comparison, as a pure function of two reports and the providers' declarations (`REQ-CAT-11`), giving match, difference or not comparable. Its only outputs are a record and a raised flag. Give it no permission to change an order's state or move value, and test exactly that: a difference raises and changes nothing else.

A batch run could come later, as a second detector under the same limits.

Keep the primary path apart. A segment's outcome comes from `REQ-ORD-1`. The next amount comes from evidence at the node (`REQ-ORD-21`). A status query settling an unknown outcome belongs to that path, not to this one. If the comparison keeps finding the same gap, fix the primary path.

## Consequences

Costs normalising units, gross or net, and later identifiers (`THR-107`, `THR-108`). A rounding tolerance hides anything smaller than itself, so tie it to declared increments. Raised orders can become noise, so rank them by value and age (`REQ-OPS-2`, `THR-58`).

This doesn't catch two providers agreeing false reports (`THR-15`, accepted under R72). A final deposit has no next party to compare with (`THR-36`, `REQ-ORD-33`). Reports are statements, so a match shows agreement, not truth.

## Requirements served

`REQ-ORD-9`, `REQ-CAT-11`, `REQ-OPS-2`, `INV-1`, `THR-14`.

## Sources

- `research/2026-11-02-durable-state.md`: batch comparison is a listed candidate with inferred trade-offs, unchecked. It is used here for detection only.
- The owner's standing rule, as restated in `REQ-ORD-9`.
