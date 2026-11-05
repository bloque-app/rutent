# ADR-007: Who confirms a reversal

Non-binding. See `README.md`.

## Context

A reversal counts as complete only on a statement from a party other than the provider that failed, showing the value back at the segment's origin node, in full and in the asset it left (`REQ-ORD-3`, `THR-13`). No route restarts before that (`REQ-ORD-2`, `INV-4`). `SPEC.md` doesn't name the party, except that a reserve at its own node may speak for itself, labelled as its word (R104).

## Options

**The origin node's own reserve.** It is where the value lands, and it is already a granted party.

**A public record, where the node is a chain.** Open to anyone. Needs a reader in the host, and it is a reading rather than a party's statement.

**The customer.** Knows their own account. Slow, leaves value waiting (`THR-60`), and has a stake in the answer.

**The failed provider.** Simple. Excluded by `REQ-ORD-3`, listed for comparison.

## What we'd try first, and why

A statement signed by the origin reserve, naming the segment's transaction, amount and asset. It speaks to that credit only, never a balance (`INV-9`). If it could reveal sufficiency it counts as a balance check (`REQ-CAT-4`). It stays recorded as the reserve's word (`REQ-AUD-2`). The reserve is the one party that sees the arrival first-hand, and asking it adds no new party.

## Consequences

Adds an operation that `REQ-CAT-9` doesn't list for reserves. A reserve that doesn't answer leaves the value unevidenced, so the order heads towards stuck rather than a reroute.

A failed withdrawal has no reversal to confirm: it cancels the order (`REQ-ORD-40`). A node with several reserves, when the value returns to a different one, is still unsettled.

This also doesn't catch a false statement, or two parties agreeing one (`THR-15`, accepted under R72). A signature catches forgery, not falsehood.

## Requirements served

`REQ-ORD-2`, `REQ-ORD-3`, `INV-2`, `INV-4`.

## Sources

- `notes/2026-11-03-threat-model-draft.md`, "For suggestions/": the proposal.
- `research/2026-11-02-durable-state.md`: treating an outcome as fact only when signed or matched by an independent record is inference.
- `research/2026-11-02-compliance-proofs.md`: W3C VC 2.0's line that verifying doesn't imply truth, checked.
- R39, R40, R61.
