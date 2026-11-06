---
status: Accepted (Pablo, 2026-11-06, decision-form item `report-difference-detection-scope`)
decision_item: report-difference-detection-scope
supersedes: adopts suggestions/ADR-009-report-differences.md
---

# ADR-0009: Detecting differences between adjacent providers' reports

## Context

`REQ-ORD-9` requires comparing what one provider reports sending against what the next party reports receiving,
raising a mismatch to the operator. Pablo's standing rule (recorded in `suggestions/ADR-009`, sourced from the
private threat-model draft): reconciliation is **detection only** — it must never complete an order or patch a
gap in the primary path itself.

## Options considered

1. **Leave this to the operator's manual review.** Rejected: doesn't meet `REQ-ORD-9`'s comparison requirement on
   its own; manual review alone has no systematic trigger.
2. **A pure per-order comparison function**: two providers' reports plus their declared units/gross-vs-net
   reporting conventions, in; `{match, difference, not comparable}`, out. Its only effects are a record and a
   raised flag — explicitly given no permission to change order state or move value, and tested as such.

A later, independent batch-comparison pass against provider statements (comparing many orders' worth of reports
at once) could be added as a second detector; not adopted now, since no gap in the per-order comparison has yet
shown a need for it.

## Decision

Option 2, allocated to `CMP-CORE-AUDIT` (`ARCHITECTURE.md` §7) rather than `CMP-CORE-ORCH`, precisely because its
only sanctioned effects are a record and a flag — the same shape as every other audit function, not an order-
state-changing one. This placement is itself part of the decision: keeping it out of `CMP-CORE-ORCH` is what
makes "detection-only, never completes an order" enforceable by construction (the comparison function has no
access to anything that could move value) rather than by convention alone.

## Consequences

- Rounding tolerance must be tied to each provider's own declared amount increments (`REQ-CAT-11`) — a fixed
  tolerance independent of that would hide real gaps below it.
- Raised mismatches need a ranking by value and age, or they become noise the operator stops reading; this ADR
  states the need but leaves the ranking rule to construction.
- This detector cannot catch two providers colluding on a matching but false report — the same residual every
  statement-based mechanism in this SAD carries (ADR-0007, ADR-0008).
- The final deposit has no next party to compare against by definition — this comparison only runs on segments
  with a following party, not the last one.
