# ADR-005: Route search and quoting

Non-binding. See `README.md`.

## Context

The engine computes routes from what the catalogue advertises, within what the operator allows, for the customer's objective (`REQ-QTE-11`, `REQ-CAT-13`, `REQ-CAT-1`). Figures past validity are not used (`REQ-QTE-4`). Bands are checked after earlier fees (`REQ-QTE-12`). Route calculation sits outside the deterministic part (R26), so its result enters as a recorded input (`REQ-STM-3`).

## Options

**Exhaustive search under a hop limit.** Exact, and easy to replay. Grows fast.

**Bellman-Ford on negative log rates.** Well known, finds cycles. Assumes rates don't depend on amount, and ignores limits.

**Multi-criteria labels or round-based search.** Gives cost and time trade-offs. NP-hard in the worst case.

**Min-cost flow.** Handles splitting, which nobody has ruled on. Fixed fees break convexity.

**Competing external solvers.** A suggestion has no standing until posted (`REQ-AUZ-2`), and it adds new trust.

## What we'd try first, and why

Search paths under a hop limit over a snapshot of advertised tuples. Evaluate each at the actual amount and direction, since a fixed fee's effect depends on both. The engine is then a pure function of request and snapshot, usable alone (`REQ-MOD-1`) and recomputable (`REQ-AUD-4`).

Stale figures: each carries origin, age and validity. An expired figure drops its candidate, with the reason recorded. Nothing gets decayed or estimated. Only the shortlist then gets fresh quotes, dry-runs and eligibility questions (`REQ-CAT-14`, `REQ-CPL-6`). A rate cycle implying free profit is flagged as possibly stale or false, never followed.

**Splitting (R127).** v1 ships it, and the owner left the algorithm for later. A simple start: split only above an amount, into a small fixed number of parts, and only when the total beats the best single route after fees.

## Consequences

A hop limit can miss a route, and whether this search is fast enough is unknown, since `NFR-1` has no number (R103). A dry-run that reveals sufficiency counts as a balance check (`REQ-CAT-4`). Graph libraries exist, but the report found no multi-provider routing library, a weak negative. Each licence is its own decision (`REQ-MOD-5`).

This doesn't solve unknown capacity, or providers answering dry-runs falsely (`THR-26`, `THR-27`), which only bulk measures show (`REQ-AUD-7`). A tampered input is seen only on recomputing (`THR-10`). Slippage compounds, and its limit is the operator's per tuple (R99). Hours and holidays can change a route after it is computed.

## Requirements served

`REQ-QTE-4`, `REQ-QTE-8`, `REQ-QTE-11`, `REQ-QTE-12`, `REQ-QTE-13`, `REQ-AUD-4`, `REQ-MOD-1`, `REQ-EXT-2`.

## Sources

- `research/2026-11-02-routing-quoting.md`: the algorithm facts are report's tags, unchecked; the complexity limits partly leads. Trade-offs are inference.
- R10, R15, R26.
