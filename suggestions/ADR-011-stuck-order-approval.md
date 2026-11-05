# ADR-011: Dual approval for resolving a stuck order

Non-binding. See `README.md`.

## Context

The operator resolves and bears stuck orders (`OBL-2`, `REQ-ORD-28`, R42), and that work is mostly manual (R24). Resolving is an action under policy. It names the evidence relied on, and one actor can't both decide the outcome and move value (`REQ-AUZ-4`). A rogue staff member (`THR-77`) or a colluding customer and provider (`THR-70`) are the cases in mind.

## Options

**Two roles, one decision.** One actor names the outcome and evidence. A different actor moves value. Meets the requirement directly. Needs two people available.

**Threshold approval above an amount.** Several approvers for large orders, fewer for small. Scales effort to risk. Splitting into small orders can dodge it, and one approver still can't both decide and move (`REQ-AUZ-4`).

**One actor, reviewed afterwards.** Fast. Doesn't meet `REQ-AUZ-4`, listed for comparison.

## What we'd try first, and why

Two roles, as separate permissions under the same policy decision (`REQ-AUZ-1`, `REQ-AUZ-6`). The decision record names the order, the outcome, the amount, the destination and the evidence. Moving value is refused unless it matches that record and comes from a different actor, compared at the level of the accountable person and not the credential. Both actions enter the log (`REQ-AUD-1`).

It is the smallest shape that meets the requirement, and it can gain thresholds later.

## Consequences

Slower resolution while value waits, which is exposure the operator carries (`REQ-ORD-8`). A small team needs cover across hours. Show the queue by value and age, or a large order can hide among small ones (`REQ-OPS-2`, `THR-58`).

This doesn't stop two actors agreeing, or judge whether the evidence is true. Where one party holds every role, separate staff are all that remains (`THR-67`, accepted under R50). An intruder with full operator access can act as both (`THR-34`, `THR-66`).

No report studied dual control. This rests on the requirement and on inference.

## Requirements served

`REQ-AUZ-4`, `REQ-AUZ-1`, `REQ-AUZ-6`, `REQ-AUD-1`, `REQ-OPS-2`, `REQ-ORD-28`.

## Sources

- `notes/2026-11-03-threat-model-draft.md`, "For suggestions/": the proposal, unsourced.
- `research/2026-11-02-durable-state.md`: human intervention being authorised and recorded is a prompt from that report, not a finding.
- R24, R42, R50.
