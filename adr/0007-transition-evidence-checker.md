---
status: Accepted (Pablo, 2026-11-06, decision-form item `evidence-grading-checker`)
decision_item: evidence-grading-checker
supersedes: adopts suggestions/ADR-013-transition-evidence.md; introduces `CMP-EVIDENCE`
---

# ADR-0007: Transition evidence grading and the evidence checker

## Context

`REQ-ORD-39` requires every segment's outcome to carry the best evidence its provider can give, graded, with the
operator setting a per-tuple evidence floor. `SPEC.md` requires the grading scheme to exist but leaves its shape
open. `suggestions/ADR-013` — explicitly flagged by its own source confidence as **inference, not sourced**, no
report having studied evidence grading directly — proposes a four-grade scale and a read-only checker apart from
any provider's module, since a module validating itself "marks its own work."

## Options considered

1. **One evidentiary rule for everything**, requiring third-party proof universally. Rejected: excludes private
   nodes entirely, where no third party exists to ask.
2. **Evidence only at route endpoints.** Cheap, but a false mid-route success would move value onward
   undetected — fails the intent behind `REQ-ORD-39` requiring *every* segment to carry graded evidence.
3. **A four-grade scale** — (1) checkable by anyone (a public chain record), (2) signed by another party (the
   next provider's own receipt, or the rail itself), (3) signed by the provider with something external to
   compare later, (4) the provider's bare statement — each provider declaring which grade it can give, with route
   choice able to prefer stronger-evidence segments. Grade 2's cheapest form is the next provider's own receipt,
   since it already must confirm arrival; valid only when that next provider is a genuinely different publisher
   (fails when one publisher runs both sides of a hop, `THR-124`).
4. **Validation by the provider itself.** Rejected: a module checking its own work adds no new assurance and
   would require a protocol change for every new validating-provider role.

For public-network validation specifically: model each segment's claim as a role-shaped tuple of provable inputs
(ramp = transfer-in + transfer-out; exchange = asset-A-out + asset-B-in; reserve = withdrawal or deposit), checked
by a read-only checker run apart from any provider's module. That checker needs three network-specific things
settled per network: a finality definition, reorg handling, and how a movement ties back to a specific segment —
the hard part, since transfers often carry no order reference. One-destination-per-route is this ADR's answer;
matching by amount alone is called a guess (flagged `F-23` in the source draft) and rejected.

## Decision

The four-grade scale, declared per provider. `CMP-EVIDENCE` (named in `ARCHITECTURE.md` §2) is the read-only,
per-public-node checker that validates grade-1/2 claims against public records, run apart from any provider's
module. It is the same component ADR-0005 uses for private-node replay — one checker, not two, so a provider's
own module is never the thing marking its segment's work.

## Consequences

- Grade 2 (next-provider's receipt) fails by design when one publisher runs both sides of a hop (`THR-124`,
  unchanged, accepted) — route choice that prefers grade 2 must know the publisher behind each hop, not just the
  provider identifier, to avoid false reliance.
- Private nodes rest on grade 3/4 by necessity; a low evidence floor there is an explicit, recorded operator
  choice (`REQ-ORD-39`'s per-tuple floor), not a gap this ADR hides.
- `CMP-EVIDENCE` needs, per public network it checks, a finality definition and reorg-handling rule settled before
  it can run — this is construction-time work (`STEP-3`), not resolved by this ADR, and should be tracked per
  network as it's added to the catalogue.
- One-destination-per-route is now a load-bearing assumption for movement-to-segment matching; a future route
  shape that violates it (if ever proposed) would need this ADR revisited, not just `CMP-ROUTER` changed.
- This whole ADR rests on inference, not a sourced precedent (`suggestions/ADR-013`'s own confidence label) — it
  should be the first place to revisit if real operational evidence later contradicts the grading scheme's
  assumptions.
