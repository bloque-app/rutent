---
status: proposed
experiment: EXP-1
decision_item: exp1-adopt-runtime (pending)
---

# Draft: the plan decision EXP-1 would bring

A draft of `EXP-1` (Narthex `REQ-EXP-3`). The plan decision stays `proposed` until promotion (Narthex `REQ-EXP-5`).
It has no `DEC-*` number yet. `PLAN.md`'s identifiers are stable and never reused, so the number is given when the
decision is promoted into `PLAN.md` §2.1, as the row below. The draft is written for the recommended option of item
`exp1-adopt-runtime`. If the owner chooses VOS instead, the row is rewritten from the amendment's
[If the owner chooses VOS](adr-0002-amendment.md#if-the-owner-chooses-vos).

| ID | Decision | Source | Status |
|---|---|---|---|
| `DEC-<n>` | **The purpose-built store is v0's runtime.** It implements the host contract (`rutent_stm::purpose_built_store`, store format 1), and `F-010` builds `CMP-STM`'s library on it, out of its experimental flag. VOS is not adopted for v0 (`EXP-1` outcome: VOS dropped, the store adapted), and the host contract stays runtime-neutral, so a later `EXP-n` may score another runtime. Before v0 reaches production, `F-010` closes the store's gaps `G-54`, `G-33`, `G-40` and `G-51`, and tests durability across power loss on the deployment platform (`F-030`). `F-033` takes metrics (`G-41`) and load figures (`G-42`). `F-022` takes the audit rows (`G-04`, `G-05`, `G-09` to `G-11`; ADR-0005), `F-025` takes `G-26`, and `F-034` takes the release (`G-46`). The store brings no dependency, so `licences/RECORD` gains no line; the `vos` line stays `not-adopted` | ADR-0002 as amended; `DEC-6`; `REQ-OPS-3`, `NFR-6`, `REQ-MOD-5`, `PS-66`, `DEC-19`; item `exp1-adopt-runtime` | proposed |

What promotion also changes in `PLAN.md`, as part of this decision:

- **The ADR row.** `F-007`'s goal is met. §7.2's row for ADR-0002 adds the new `DEC-<n>` beside `DEC-3` and
  `DEC-6`.
- **`M2`'s tasks.** When they are written (`PLAN.md` §3, `M2`), `F-010`'s include one task for each of the four
  store gaps above, each with its test.
