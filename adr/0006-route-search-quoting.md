---
status: Proposed
decision_item: route-search-algorithm (pending)
supersedes: adopts suggestions/ADR-005-route-search-quoting.md
---

# ADR-0006: Route search, quoting and splitting

## Context

`REQ-QTE-11/12` require the router to compute from catalogue advertisements within operator-set bounds,
optimizing the customer's objective, checking each segment's amount against that provider's own band **after
fees already taken upstream, in that provider's own asset** — a path-order-dependent, amount-dependent edge cost,
not a static graph weight. `REQ-QTE-16` requires split-route support with a decision record (`REQ-AUD-4`) that
covers the split. `research/2026-11-02-routing-quoting.md` found (abstracts only, not full papers, flagged as a
weakness) that fixed-fee, resource-constrained routing is NP-hard in the general case, and found no standalone,
well-documented multi-provider quoting library for fiat-plus-on-chain routing — a weak negative from one search,
not proof none exists.

## Options considered

1. **Bellman-Ford on negative log rates.** Rejected: assumes amount-independent rates and ignores provider bands
   entirely — incompatible with `REQ-QTE-12`'s stated cost model.
2. **Exhaustive path search under a hop limit, over a snapshot, each candidate evaluated at the actual amount and
   direction**, making the router a pure function of `(request, snapshot)` — reusable as a library, and fully
   recomputable from the decision record alone with no provider call (`AC-2`). Stale figures (known age/origin,
   `REQ-QTE-4`) drop their candidate with the reason recorded; a rate cycle implying free profit is flagged as
   possibly stale or false and never followed.
3. **Multi-criteria / round-based search.** NP-hard in the worst case per the report's own (unverified-in-full)
   citation; rejected as unjustified complexity until a concrete performance need is shown.
4. **Min-cost flow.** Fixed fees break the convexity min-cost flow assumes; how splitting composes with it is
   unruled by any source.
5. **An external solver.** No standing until posted (`REQ-AUZ-2`), and adds a new trusted party with no report
   finding this justified.

**Splitting:** start simple — split only above a threshold amount, into a small fixed number of parts, only when
the total (after fees) beats the best single route. More elaborate flow-based splitting is not ruled out later,
but isn't adopted now absent evidence it's needed.

## Decision

Option 2: hop-limited exhaustive search over a snapshot, each candidate evaluated at the actual amount/direction;
only the shortlist gets fresh quotes, dry-runs and eligibility checks afterward (`REQ-CAT-5`'s visibility limit
followed directly — non-shortlisted providers are never asked). Splitting starts with the simple
threshold-and-fixed-parts rule above.

## Consequences

- `CMP-ROUTER`'s public contract (`ARCHITECTURE.md` §3) is a pure function, satisfying `AC-2` by construction:
  an objective change re-derives the choice with no provider call.
- The hop limit can miss a real route; this is a stated, accepted limit, not a bug to be designed away later
  without revisiting this ADR.
- `THR-10`/`THR-135`/`THR-136` (route-computation tampering, split-gaming) are **not newly mitigated** — this
  design makes them detectable (the decision record, `REQ-AUD-7`'s bulk measures) but does not prevent them. This
  matches `ARCHITECTURE.md` §6's threat-revisit finding: restate as accepted residual risk.
- No performance number exists for this algorithm under real load (`NFR-1` is itself unbound, with no agreed
  figure) — this ADR does not claim a throughput target; one should be set once real provider-network-wait
  behaviour is observed.
- The simple splitting rule may under-split compared to a flow-based approach in some cases; this is accepted as
  a starting point, not a final answer, and can be revisited once `AC-38` (split-order independence) testing
  surfaces a concrete gap.
