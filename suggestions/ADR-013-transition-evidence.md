# ADR-013: Proving a transition

Non-binding. See `README.md`.

## Context

The owner asked how transitions get verified: a provider's outcome must be "more than just a statement", provable "as much as possible" (R100). Evidence is graded, and the operator sets a floor per tuple (`REQ-ORD-39`, R113). A final deposit closes on a public record of the movement plus the reserve's signed statement (`REQ-ORD-33`, R114). He also asked, at plan level, how to verify a segment's outcome on a public network, and whether another provider should do it. Checks run in the outer layer (R114).

## Options

**Grades of evidence, recorded per segment.** From strongest:
1. Checkable by anyone: a public chain's record.
2. Signed by another party: the next provider's receipt, or the rail itself.
3. Signed by the provider, with something outside it to compare later.
4. A provider's bare statement.

**One rule for everything.** Third-party proof on every segment. Simple, and it excludes private nodes.

**Evidence only at the endpoints.** Cheap. A false mid-route success moves value on.

## What we'd try first, and why

Grades, with a declaration per provider of the grade it can give (`REQ-CAT-10`). Route choice can then prefer stronger segments. The next provider's receipt is the cheapest grade 2 evidence, because it already has to confirm what arrived (`REQ-ORD-21`), so long as it's a different publisher.

**Validating on a public network.** The owner's instinct holds up: a segment's claim is a tuple of provable inputs, shaped by its role. A ramp shows a transfer in and a transfer out. An exchange shows asset A out and asset B in. A reserve shows the withdrawal or the deposit. So the role's contract can name the evidence it owes.

Who checks: start with rutent's own read-only checker per public node, in the outer layer, run apart from any provider's module. A provider's module checking itself is marking its own work. A validating provider adds a party to trust, and a new role means a protocol change (R90). Keep that for networks too costly to read directly.

The checker needs three things settled per network: what counts as final, what to do on a reorganisation, and how a movement ties to a segment. The last is the hard one, since a transfer may carry no order reference. One destination per route is the plainest answer, and matching on amount is a guess (F-23).

## Consequences

Grade 2 fails where one publisher runs both sides (`THR-124`). A private node then rests on grades 3 or 4, so a low floor there is a choice somebody records (`THR-126`).

For private APIs, proofs of a TLS session are a possible route to grade 2 evidence. No report studied them, so treat this as a lead only. A signature catches forgery, not falsehood, and evidence shows what a source says, not that it is true. Nothing here settles shared movements (R88), where each order's outcome stays a provider's word.

## Requirements served

`REQ-ORD-33`, `REQ-ORD-39`, `REQ-CAT-10`, `REQ-CAT-21`, `REQ-ORD-21`, `THR-36`, `THR-126`.

## Sources

- `research/2026-11-02-durable-state.md`: treating an outcome as fact only when signed or matched by an independent record is inference.
- `research/2026-11-02-compliance-proofs.md`: W3C VC 2.0's line that verifying doesn't imply truth, checked.
- `notes/2026-11-03-aledgent-evidence.md`: one reserve's evidence, as its session reports it, unchecked.
- R68, R88, R90, R100, R113, R114. The grades, the checker and the TLS lead are inference.
