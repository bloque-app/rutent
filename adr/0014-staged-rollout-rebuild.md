---
status: Accepted (Pablo, 2026-11-06, decision-form item `staged-rollout-rebuild`)
decision_item: staged-rollout-rebuild
supersedes: adopts suggestions/ADR-012-staged-rollout-rebuild.md
---

# ADR-0014: Staged rollout and independent rebuild

## Context

`REQ-MDL-4/5` require module upgrades to be stageable, observable and rollback-able, with in-flight segments
moving only to an admitted version and marking a version stale being the operator's own permission, never
automatic from publishing. The threat this guards against: a signed-but-malicious release, live because the
owner's acceptance model (`R27`) trusts the provider list, not every byte it ever publishes.

## Options considered

1. **Activate the new version at once, with rollback available if something goes wrong.** Every order hits the
   new release first — the worst case for a bad release is the default case under this option.
2. **Rebuild from reviewed source** before accepting a release. Better coverage in principle, but cross-language
   review is hard, publishers may not share source at all, and this exceeds what `REQ-MDL-2` actually requires
   (traceability to the admitted binary, not to its source).
3. **Catalogue metadata via TUF** (rollback/freeze primitives). Handles rollback and freeze mechanics but says
   nothing about whether a release is honest — doesn't address the actual threat.
4. **Route a small share of new segments to the new version for a stated observation period**, comparing
   `REQ-AUD-7`'s bulk measures against the old version before widening. Never mark the old version stale until
   staging ends — a stale mark would force in-flight segments onto the new version, skipping staging entirely
   (`THR-16`). Keep every admitted binary plus the compiler version that produced it; re-derive machine code on
   separate hardware after each compiler fix and on a schedule.

## Decision

Option 4, allocated to `CMP-PROV` (module admission and lifecycle). An emergency bypass path (to force a known-bad
version offline immediately) needs its own separately recorded permission (`REQ-AUZ-6`), since the bypass path
itself becomes a target precisely because it skips staging.

## Consequences

- Low-volume providers may never see enough traffic within the staging window to judge a new release reliably —
  this ADR doesn't solve that; a minimum observation volume, not just a time window, may need adding at
  construction time.
- Re-derivation only detects a compromised build once someone actually runs the comparison — it is not a
  continuous guarantee.
- Operator-run re-derivation is weaker than an outside party's would be, but requiring an outside party conflicts
  with `REQ-OPS-4`'s independent-operation requirement — the same tension `CMP-EVIDENCE`'s placement already
  accepts (ADR-0007), restated here rather than resolved differently.
- This design doesn't catch an attack that stays fully within a module's already-granted capabilities, or one
  that stays quiet through the entire observation window — both remain accepted residual risk.
