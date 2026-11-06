---
status: Accepted (Pablo, 2026-11-06, decision-form item `stuck-order-dual-approval`)
decision_item: stuck-order-dual-approval
supersedes: adopts suggestions/ADR-011-stuck-order-approval.md
---

# ADR-0011: Dual approval for resolving a stuck order

## Context

`REQ-AUZ-4` requires resolving a stuck order to be a policy action that names the relied-on evidence, with a
single actor forbidden from both deciding the outcome and moving value. The threat this guards against: a rogue
staff member, or a colluding customer and provider, both pushing a stuck order toward a chosen (wrong) outcome.
`suggestions/ADR-011` is, like ADR-0010, built on requirement text and inference — no report studied dual control
directly.

## Options considered

1. **Threshold approval scaling with order amount.** An order can be split below the threshold to dodge it, and
   it still needs the same decide/move separation underneath — doesn't remove the core requirement, just adds
   complexity on top.
2. **Single actor, reviewed after the fact.** Fails `REQ-AUZ-4` outright — the requirement is that a single actor
   must not both decide and move, not that the decision gets checked later.
3. **Two separate permissions under one policy decision**: one actor names the outcome and the relied-on
   evidence (order, amount, destination recorded); a different actor — compared at the accountable-person level,
   not the credential level, so one person holding two logins doesn't satisfy this — executes the value
   movement. Both actions logged.

## Decision

Option 3, allocated to `CMP-CORE-AUTHZ` (the decide step) acting on `CMP-CORE-ORCH` (the move step), with the
actor comparison enforced at the accountable-party level `REQ-AUZ-8` already establishes. This is the smallest
shape that meets `REQ-AUZ-4`'s text; it is explicitly extensible to amount-scaled thresholds later if operational
experience shows a need, but that extension is not adopted now absent such evidence.

## Consequences

- Two actors simply agreeing together defeats this control — it is not designed to catch collusion between the
  two approvers, only to require it where a single rogue actor previously sufficed.
- Where the operator and a provider are the same legal party (recorded under `REQ-CAT-20`), separate staff within
  that one party is all this control provides — there is no second organization to check against.
- A fully compromised operator account (one with both permissions somehow granted to it) defeats this entirely;
  `ARCHITECTURE.md` §6 already lists operator-compromise as accepted residual risk under `R119`, and this ADR
  doesn't change that.
- `AC-6` (stuck resolution with separation of duties) is this ADR's direct conformance test.
