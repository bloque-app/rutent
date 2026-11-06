---
status: Accepted (Pablo, 2026-11-06, decision-form item `reversal-bonding-model`)
decision_item: reversal-bonding-model
supersedes: adopts suggestions/ADR-006-reversal-backing.md
---

# ADR-0010: Standing behind a provider's reversal obligation

## Context

`REQ-CAT-7`/`OBL-3` require the operator to check a provider's demonstrated ability to pay reversals before
granting catalogue entry, and `REQ-ORD-8` lets the operator set a ceiling on value in flight per provider.
`suggestions/ADR-006` is explicitly **inference only** — no report studied this; its nearest precedent is one
unrelated auction design's solver bonding, which this session has not independently verified. It proposes a bond
held by a **third party**, sized to the provider's ceiling, so the ceiling follows the bond size.

This ADR's regulatory question — does a third party holding provider funds (rather than the operator holding
them) change how that arrangement is treated under the laws of wherever rutent operates — needs counsel, not
inference. It is recorded here as an open legal question attached to this specific ADR, to be routed to counsel
when this ADR comes up for ruling, not folded into the earlier blocking decision-form item (per the advisor's
guidance: handle each legal question where it actually bears, not as a bundled catch-all).

## Options considered

1. **Insurance sized to the ceiling.** No source found confirming such cover exists for this kind of exposure.
2. **Prefunding at the node itself.** Needs real capital locked at every node plus a custodian relationship — a
   much larger operational footprint than a bond.
3. **A declared-financials-plus-contract approach**, with nothing held anywhere. Cheapest, but leaves nothing
   recoverable if the provider simply vanishes — fails the spirit of `OBL-3`'s "ability to pay" check.
4. **A bond held by a third party** (not the operator — holding provider funds directly would keep rutent's own
   position non-empty, against `INV-5`), sized to that provider's ceiling.

## Decision

Option 4, accepted (Pablo, decision-form item `reversal-bonding-model`), with the custody question above still
routed to counsel before this is built for any roster beyond rutent's own. Ceiling-follows-bond-size is the model
`ARCHITECTURE.md` assumes for `CMP-CORE-AUTHZ`'s ceiling-setting logic.

Pablo's ruling adds a scope note: while rutent's own operator is also every provider on the catalogue, the bond
is not a third-party risk transfer in substance — it stands as a guarantee that idempotent reversals can be
made, not as cover against an unrelated party vanishing. The third-party-custody and regulatory questions this
ADR raises become load-bearing once a provider Bloque doesn't operate joins the catalogue. Pablo places that at
"the future (open protocol, V1 and further)", and v0 is a first build ahead of SPEC's v1 (`v0-before-v1`). So
counsel's read isn't needed for v0, and is due before v1, or before the first outside provider if one is admitted
sooner. That is earlier than open operation, which stays deferred past v1 (`DEF-1`): a provider the operator grants
entry to is closed operation, but no longer one the operator runs.

## Consequences

- This only recovers funds up to the bond's size — it never recalls value from a provider that takes it and runs
  beyond that amount.
- Where the operator and a provider are legally the same party, a bond checks nothing (the same entity would be
  bonding itself) — this is a known gap, not solved by this ADR.
- Collusion between a provider and a customer aiming specifically at exhausting the bond is deferred to
  ADR-0011's dual-approval process, not solved here.
- No cost figures exist for what bonding a realistic provider roster would actually require — this ADR commits
  to the mechanism, not to a number.
