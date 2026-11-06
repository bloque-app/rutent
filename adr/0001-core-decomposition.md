---
status: Accepted (Pablo, 2026-11-06, decision-form item `core-decomposition`)
decision_item: core-decomposition
supersedes: none (no suggestions/ADR-0xx precedent — Core's internal shape is not addressed by any draft)
---

# ADR-0001: Core's internal decomposition

## Context

`REQ-MOD-1/2/3` fix the shape of the three outer libraries (router, state machine, provider system) and require
Core to bind them through their public contracts only. `SPEC.md` says nothing about Core's *own* internal shape.
Left undifferentiated, "Core" would be the single component `REQ-AUD-*` (15 requirements), `REQ-AUZ-*` (9),
and the cross-cutting half of `REQ-ORD-*` (43) all land on — too coarse to allocate meaningfully under
`REQ-CHN-8`, and too coarse to reason about independently (the audit log's design and the policy engine's design
are genuinely separable concerns with different failure modes).

## Options considered

1. **One undifferentiated Core.** Simplest to state; defers the real decomposition to construction, where it
   would happen informally and undocumented. Fails `REQ-CHN-8`'s allocation requirement in substance even if
   technically satisfied by naming "Core" once.
2. **Decompose Core into orchestration, authorization/policy, and audit/log sub-components**, each with its own
   contract, bound together the same way the three outer libraries are bound into Core itself.
3. **Decompose further** (e.g., separate notice-emission, separate decision-record capture from orchestration).
   Considered and rejected for now: `REQ-ORD-20` (notices) and `REQ-AUD-4` (decision records) are both tightly
   coupled to the order lifecycle itself; splitting them out doesn't reduce coupling, it just adds an interface
   crossing with no independent reuse case behind it.

## Decision

Option 2. Core is three sub-components:

- `CMP-CORE-ORCH` — order/segment orchestration, decision-record capture, notice emission.
- `CMP-CORE-AUTHZ` — the policy/authorization engine.
- `CMP-CORE-AUDIT` — the authenticated log, checkpoints, and the holding-account balance check.

None of the three is required to be standalone-reusable the way the outer libraries are (`REQ-MOD-1` doesn't
apply to them) — they exist to make Core's own requirement allocation and security reasoning tractable, not to
satisfy a reuse requirement.

## Consequences

- `ARCHITECTURE.md` §2/§7 allocates every `REQ-AUD-*`, `REQ-AUZ-*`, and the default/exception split of `REQ-ORD-*`
  against these three names, rather than a single "Core" bucket.
- Every later ADR that touches audit, policy, or order orchestration names which of the three it modifies,
  keeping each ADR's blast radius visible.
- This decomposition is itself provisional until Pablo rules on it; nothing downstream depends on the sub-component
  *names* surviving unchanged, only on the allocation being real and complete.
