---
status: promoted into PLAN.md 0.2.0 §2.1 as DEC-26 (2026-11-09)
experiment: EXP-1
decision_item: exp1-adopt-runtime (answered 2026-11-08)
---

# Draft: the plan decision EXP-1 would bring

A draft of `EXP-1` (Narthex `REQ-EXP-3`). The plan decision stays `proposed` until promotion (Narthex `REQ-EXP-5`).
It has no `DEC-*` number yet. `PLAN.md`'s identifiers are stable and never reused, so the number is given when the
decision is promoted into `PLAN.md` §2.1, as the row below. The draft is written for the recommended option of item
`exp1-adopt-runtime`, from the revised [outcome](../outcome.md). If the owner chooses the purpose-built store
instead, the row is rewritten from the amendment's
[If the owner chooses the purpose-built store](adr-0002-amendment.md#if-the-owner-chooses-the-purpose-built-store).

| ID | Decision | Source | Status |
|---|---|---|---|
| `DEC-<n>` | **VOS, as the whole runtime, is v0's runtime.** `CMP-STM` runs as a VOS program on three replicas, at a `vos-core` commit pinned by SHA and mirrored with its forks (`EXP-1` outcome: VOS adapted, the purpose-built store the fallback; `EXP-1-vos-rt1-20261107-2`, `EXP-1-vos-rt3-20261107-2`, `EXP-1-pb-20261107-5`). `F-010` builds it, after `contracts/` is ported to `no_std` + `alloc` under a `DEC-4` bump and the owner's click (`DEC-20`). The purpose-built store stays ADR-0002's ruled fallback behind its flag, and the host contract stays runtime-neutral. Before v0 ships on VOS: the licence ruled for a state machine that links `vos-sdk`; `G-54` tested by damaging the frame that holds the last acknowledged append, then fixed upstream or its residual accepted for both candidates; a total-loss recovery story (scheduled halted exports with their downtime and RPO written down, or upstream `vos node restore`); the JIT verified on the production host and a build without native codegen (`G-37`); journal growth bounded (`G-33`). `F-033` takes metrics (`G-41`) and load figures (`G-42`), measuring `R-05`, `R-08` and `R-09` again, and replaying `R-01` to `R-03`, over a synthetic network, since every scored run had all three replicas on one host; `F-022` the audit rows (`G-04`, `G-05`, `G-09` to `G-11`; ADR-0005); `F-025` `G-26`; `F-034` the release (`G-46`) | ADR-0002 as amended; `DEC-6`; `DEC-4`; `REQ-OPS-3`, `NFR-6`, `REQ-MOD-5`, `PS-66`, `DEC-19`; item `exp1-adopt-runtime` | accepted (Pablo, 2026-11-08) |

What promotion also changes in `PLAN.md`, as part of this decision:

- **The ADR row.** `F-007`'s goal is met. §7.2's row for ADR-0002 adds the new `DEC-<n>` beside `DEC-3` and
  `DEC-6`.
- **`M2`'s tasks.** When they are written (`PLAN.md` §3, `M2`), `F-010`'s include the `DEC-4` port of `contracts/`,
  the program and its thin `Host` client, the `G-54` test, the recovery runbook, and the production-host check, each
  with its test.
- **The licence record.** The `vos` line in `licences/RECORD` takes the owner's decision and the pinned commit.
