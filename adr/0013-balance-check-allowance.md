---
status: Accepted (Pablo, 2026-11-06, decision-form item `balance-check-allowance-priority`)
decision_item: balance-check-allowance-priority
supersedes: adopts suggestions/ADR-010-balance-check-allowance.md
---

# ADR-0013: Balance-check allowance (low priority)

## Context

`INV-9`/`GUA-4` require a balance verification to reveal only whether funds suffice for a stated amount, nothing
else. `suggestions/ADR-010` proposed a platform-kept allowance bucket per account, debiting a higher rate for
unmatched checks and refunding the difference if a matching order follows within a stated window, so that probing
sufficiency repeatedly stays slower than honest use while `R20`'s "thresholds are free" rule holds. Its own text
already notes that ruling `R96` — restricting balance checks to inside a user/delegate session — makes most of
this moot for v1: the abuse case this ADR defends against (an outside party probing sufficiency without being the
account holder) mostly can't happen once checks are session-scoped.

## Options considered

1. **A flat allowance**, same rate whichever outcome. Rejected: a patient prober costs the same as an honest
   user, defeating the point.
2. **Checks only for real orders, or fixed coarse thresholds.** Both conflict directly with `R20`'s ruling that
   thresholds themselves must stay free to check.
3. **A platform-kept allowance bucket per account**, higher debit rate for unmatched checks, refunded if a
   matching order follows within a window; dry-runs or refusals that reveal sufficiency debit identically.

## Decision

Option 3 is kept as the design of record, but **deprioritized**: not built for v1 given `R96` already closes most
of the exposure this ADR addresses. It is retained for a possible future open-protocol scenario (`REQ-EXT-1`,
deferred past v1 per `OVERVIEW.md`), where a check might no longer be session-scoped.

## Consequences

- No construction work is scheduled against this ADR for the current milestone plan; it exists so the design
  isn't lost if open operation is ever built.
- Rate and window values have no data behind them in any report — if this is ever built, those numbers need
  real measurement, not a guess carried over from this ADR.
- An asker could still open-and-drop orders purely to earn refunds under this design — a new, minor abuse surface
  this ADR itself would introduce, worth noting for whoever eventually implements it.
- Timing-based leaks (inferring sufficiency from response latency rather than the yes/no answer itself) remain
  explicitly out of scope, as `suggestions/ADR-010` already stated.
