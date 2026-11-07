---
status: accepted by the owner on 2026-11-07; waits for its DEC number and row in PLAN.md §2.1, which only the owner edits
experiment: EXP-2
decision_item: exp2-adopt-format (answered 2026-11-07, Wasm only); licence-llvm-exception (answered 2026-11-07, covered)
---

# Draft: the plan decision EXP-2 would bring

A draft of `EXP-2` (Narthex `REQ-EXP-3`). The plan decision stays `proposed` until promotion (Narthex `REQ-EXP-5`).
It has no `DEC-*` number yet. `PLAN.md`'s identifiers are stable and never reused, so the number is given when the
decision is promoted into `PLAN.md` §2.1, as the row below. The draft is written for the recommended option of item
`exp2-adopt-format`. If the owner chooses otherwise, the row is rewritten from the amendment's [If the owner chooses
PolkaVM or both](adr-0003-amendment.md#if-the-owner-chooses-polkavm-or-both).

| ID | Decision | Source | Status |
|---|---|---|---|
| `DEC-<n>` | **Wasm components are v0's one module format.** `F-013` builds module admission and runtime on Wasmtime 48.0.8, the LTS release, from `T-008-02`'s binding, out of its experimental workspace. The binding brings components with its WIT world, the format name `wasm-component`, an explicit compile target, no WASI interface, fuel and a memory limit per part. Rust and C are `AC-20`'s two languages. One OS process per provider and the admitting compiler in its own isolation stand, as `DEC-7` has them. PolkaVM is not adopted for v0 (`EXP-2` outcome: PolkaVM dropped, Wasm adapted), and no second format is kept. The module interface stays format-neutral (`DEC-4`), so a later `EXP-n` may score another format. Wasmtime's licence, `Apache-2.0 WITH LLVM-exception`, is covered by the standing permission (item `licence-llvm-exception`) and recorded in `licences/RECORD`. Before v0 reaches production, `F-013` closes `B-14`, `B-19`, `G-05`, `G-09`, `B-09`, `B-01`, and re-derivation on a CPU of another vendor (`B-11`, `G-04`, shown on two AMD models), and reads the pin against the advisories when it sets it (`G-02`). `F-014` closes `B-17` and `B-18`, `F-025` reads `G-02` again and probes each jail layer alone, and `F-030` and `F-033` take the AppArmor obligation | ADR-0003 as amended; `DEC-7`; `REQ-MDL-15`, `REQ-MDL-18`, `AC-20`, `THR-85`, `REQ-MOD-5`, `DEC-19`; items `exp2-adopt-format`, `licence-llvm-exception` | proposed |

What promotion also changes in `PLAN.md`, as part of this decision:

- **The ADR row.** `F-008`'s goal is met. §7.2's row for ADR-0003 adds the new `DEC-<n>` beside `DEC-7`.
- **Risks.**
  - Risk 3 is retired: both formats ran Rust and C guests, and the format kept claims `AC-20` in both (`G-10`,
    `G-22`, `M-16`).
  - Risk 4's mitigation adds the re-checked `THR-85` row of `drafts/thr-85-recheck.md`, with `F-025` re-running it.
- **`M2`'s tasks.** When they are written (`PLAN.md` §3, `M2`), they carry each follow-up row of the outcome as a
  task with its test: `F-013`'s for its rows above, and `F-014`'s for `B-17` and `B-18`.
- **`E4`.** Its licence clause holds for Wasm: every package the binding brings has its line in `licences/RECORD`,
  the 29 `Apache-2.0 WITH LLVM-exception` crates among them.
