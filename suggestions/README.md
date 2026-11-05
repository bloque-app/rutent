# suggestions

This folder holds non-binding engineering proposals. The implementing team may ignore any of it. `SPEC.md` decides what; these files only propose how. Where a file and `SPEC.md` differ, `SPEC.md` wins.

Each file takes one decision, and cites `SPEC.md` v0.3.1 Where a suggestion depends on a pending question, it names the `OQ-n` and doesn't assume the answer. Nothing here describes an existing system, and no legal claim here is stated as fact.

"Owner's preference" marks what the owner proposed in R22, R27, R28 or R38. He prefers it; nobody has decided it.

## How sure each source is

| Label | Meaning |
|---|---|
| Checked | The report's review note says this was checked at source |
| Report's tag | The report tagged it source or measured, and nobody rechecked it |
| Lead | Taken from a search excerpt only |
| Inference | Reasoning, stated by no source |
| Reading | An engineer's reading of a legal text, not legal advice |

## Files

| File | Proposes | Main requirements served |
|---|---|---|
| `ADR-001-durable-runtime.md` | Rules as a plain library, then a bounded spike under VOS, the owner's preference | `REQ-STM-3`, `REQ-MOD-1`, `REQ-MOD-5`, `NFR-6` |
| `ADR-002-module-format-sandbox.md` | One process per provider; test Wasm and PVM on two languages | `REQ-MDL-9`, `REQ-MDL-15`, `REQ-MDL-18` |
| `ADR-003-outside-part.md` | Per-provider credential proxy, secret resolvers, destination allow-lists | `REQ-MDL-6`, `REQ-MDL-7`, `REQ-MDL-17` |
| `ADR-004-authenticated-log.md` | Merkle log, checkpoints to an appointed checker, receipts to parties | `REQ-AUD-5`, `REQ-AUD-9`, `INV-6` |
| `ADR-005-route-search-quoting.md` | Amount-aware search under a hop limit; expired figures drop their candidate | `REQ-QTE-4`, `REQ-QTE-11`, `REQ-QTE-12` |
| `ADR-006-reversal-backing.md` | A bond held by a third party, with the ceiling following it | `REQ-CAT-7`, `REQ-ORD-8` |
| `ADR-007-reversal-confirmation.md` | A signed statement from the origin node's own reserve | `REQ-ORD-3`, `INV-4` |
| `ADR-008-judgements.md` | Short-lived judgements carried by the person; per-provider references if allowed | `REQ-CPL-1`, `REQ-CPL-4`, `REQ-CPL-10` |
| `ADR-009-report-differences.md` | Per-order comparison that only detects | `REQ-ORD-9`, `INV-1` |
| `ADR-010-balance-check-allowance.md` | Per-account allowance charging unmatched checks more | `REQ-CAT-3`, `REQ-CAT-4`, `INV-9` |
| `ADR-011-stuck-order-approval.md` | Two roles: one decides, another moves value | `REQ-AUZ-4` |
| `ADR-012-staged-rollout-rebuild.md` | Staged rollout, and re-derivation on separate hardware | `REQ-MDL-2`, `REQ-MDL-4` |
| `ADR-013-transition-evidence.md` | Grades of evidence, and how to validate a segment on a public network | `REQ-ORD-33`, `REQ-ORD-39` |
| `ADR-014-public-checkpoints-and-replay.md` | Public checkpoints that don't show activity, and replay at a private node's exit | `REQ-AUD-14`, `REQ-ORD-41` |

## Gaps these files ran into

Writing these turned up places where `SPEC.md` is silent:

- `ADR-003`: whether a shared credential proxy can satisfy `REQ-MDL-6`. That file proposes one proxy per provider, which avoids the question.
- `ADR-007`: a node with several reserves, when the value returns to a different one.
