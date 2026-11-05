# ADR-006: Standing behind reversals

Non-binding. See `README.md`.

## Context

A grant records that a provider has shown it can pay for its reversals, and going silent doesn't discharge one (`REQ-CAT-7`, `OBL-3`, R56). The operator can set a ceiling on value in flight at a provider (`REQ-ORD-8`), and bears stuck orders (`OBL-2`, R42). `SPEC.md` leaves "can pay" open.

No report studied provider bonds or insurance. The nearest precedent is solver bonding in one auction design. The threat-model draft proposes the idea, and everything below is inference.

## Options

**Bond, sized to the ceiling.** One figure to reason about, and cover exists when the provider is gone. Ties up the provider's capital.

**Insurance, sized to the ceiling.** No capital held. We don't know whether such cover exists for this, or what it excludes.

**Prefunding at the node.** A reversal can pay out at once. Needs capital per node, and someone holding it.

**Declared financials plus a contract.** Cheapest. Nothing is there if the provider vanishes.

## What we'd try first, and why

A bond held by a third party, with the ceiling following the bond: a provider posting less gets a smaller ceiling. Then `REQ-CAT-7`'s evidence and `REQ-ORD-8`'s limit are one number, which is easy to audit and to explain at entry.

Keep it outside rutent, whose own position stays empty (`INV-5`). A third party rather than the operator, because whether holding provider funds changes how an operator is treated is a legal question nobody here has answered.

## Consequences

Capital cost may keep small providers out, and capacity falls with the bond. Size says nothing about how fast a claim pays. Claim terms are contract, outside `SPEC.md` (`OBL-2`). We have no cost figures.

This doesn't recall value from a provider that takes it and goes (`THR-73`); it only funds recovery up to the bond. Customers still rely on the operator's solvency (`THR-75`). A provider and customer in collusion can aim at the bond, so resolution stays under `REQ-AUZ-4` (`THR-70`). An origin undone after delivery is a separate matter (`THR-31`, R97). Where operator and provider are one party (R50), the bond checks nothing (`THR-67`).

## Requirements served

`REQ-CAT-7`, `REQ-ORD-8`, `REQ-OPS-2`, `OBL-3`, `OBL-5`, `INV-5`.

## Sources

- `notes/2026-11-03-threat-model-draft.md`, "For suggestions/": the proposal, unsourced.
- `research/2026-11-02-routing-quoting.md`: solver bonding, report's tag, unchecked.
- R42, R50, R56.
