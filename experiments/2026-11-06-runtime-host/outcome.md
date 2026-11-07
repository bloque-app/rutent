---
experiment: EXP-1
status: written by T-007-04, 2026-11-07; awaiting the owner's answer to item exp1-adopt-runtime
---

# EXP-1 outcome

The one-page outcome of `EXP-1` (Narthex `REQ-EXP-4`). Every number names the run that produced it (Narthex
`REQ-CHN-17`), from the [scoring sheet](scoring.md) and the [run notes](runs/). Adopting it is one item on the
owner's form, `exp1-adopt-runtime`, with this page as its ref (Narthex `REQ-EXP-7`).

## Result

| Candidate | Result | What it means |
|---|---|---|
| `pb`, the purpose-built store | **adapt** | `M2` builds `CMP-STM` on it (`F-010`), with the [follow-up rows](#follow-up-rows) closed before v0 reaches production |
| `vos`, VOS at `vos-core` `1aadf44` | **drop** for v0 | Not adopted. The host contract stays runtime-neutral (ADR-0002), so a later experiment can score a tagged VOS release |
| `ref`, the reference host | the floor | In memory; never a candidate |

Both candidates pass the gate. Outside the module-runtime rows, every gap row `pb` fails VOS fails too, and VOS
fails nine more (`EXP-1-pb-20261107-1`, `EXP-1-vos-20261107-1`). `pb` brings no new package and needs no licence
ruling. Choosing VOS would need its licence ruled first, and 246 more packages recorded (`EXP-1-vos-20261107-1`).

## Numbers

All runs on one Apple M2 Pro. Timings are measurements, not targets (`PLAN.md` §6.2, risk 9).

| Criterion | `pb` | `pb` run | `vos` | `vos` run |
|---|---|---|---|---|
| Host rows `H-01` to `H-17` | 17 of 17 pass, in 3 swap directions | `EXP-1-pb-20261107-1` | 17 of 17 pass, in `vos-to-ref`, `ref-to-vos` and `compacted` | `EXP-1-vos-20261107-1` |
| The 29 gap rows scored for both, outside the module rows | 12 pass, 17 fail | `EXP-1-pb-20261107-1`, `-2`, `-3` | 2 pass, 6 pass (adapter), 21 fail | `EXP-1-vos-20261107-1` |
| The 7 gap rows on VOS's own parts (proof stack, JIT, node key, `cipher-clerk`, docs) | do not apply | `EXP-1-pb-20261107-1` | 2 pass, 5 fail | `EXP-1-vos-20261107-1` |
| Gap rows only this candidate fails | 0 | `EXP-1-pb-20261107-1` | 9: `G-13`, `G-14`, `G-36` to `G-39`, `G-48`, `G-49`, `G-53` | `EXP-1-vos-20261107-1` |
| The 16 module-runtime rows (`G-06`, `G-15` to `G-25`, `G-27` to `G-30`) | 16 fail: no module runtime. Not counted | `EXP-1-pb-20261107-1` | 14 fail, 2 not run | `EXP-1-vos-20261107-1` |
| `G-01`, a journal past the compaction defaults | 1,201 entries, 93 MiB, replayed whole; reopen 339.6 ms | `EXP-1-pb-20261107-2` | 40 of 40 journals whole under `compact 16 4`, through the adapter | `EXP-1-vos-20261107-1` |
| `G-32`, the largest fixture's record | 4,818 bytes; frame payload 4,844 of 67,108,864 | `EXP-1-pb-20261107-3` | value about 4,782 of 65,536; request about 4,814 of 65,536 | `EXP-1-vos-20261107-1` |
| A synced submit, end to end | median 4.81 to 4.89 ms, p99 7.35 to 8.04 ms; 202 to 209 submits/s. Release build, in-process | `EXP-1-pb-20261107-2` | `batch` mean 7.6 ms, `flush_synced` mean 6.2 ms, 281 single-call batches. `faults` build, interpreter, `compact 16 4`. The `refine` mean, 831 µs, is one stage and excludes commit and sync | `EXP-1-vos-20261107-1` |
| `G-45`, own tests | 19 passed, 0 failed | `EXP-1-pb-20261107-1` | 473 passed, 0 failed, 35 not run or not finished, in 6 of 38 crates | `EXP-1-vos-20261107-1` |
| Licence rows | `L-04` pass: no new package | `EXP-1-pb-20261107-1` | `L-01` pending; `L-02` 25 VOS crates unlisted (11 AGPL-3.0-or-later, 14 Apache-2.0); `L-03` 221 of 240 packages with no line | `EXP-1-vos-20261107-1` |

The timing row compares a median with means, and the two setups differ. It isn't a ranking.

## Why

1. **The gate.** Both pass all 17 host rows, so the gate does not choose.
2. **The gaps.** `pb` fails all 16 module-runtime rows, as any store would: `pb` is a host, not a module
   runtime. That is `EXP-2`'s question, so those rows are left out of the count. VOS's 14 fails there go to
   `EXP-2`. Outside them, `pb`'s 17 fails are all VOS fails too (all counts in this section: `EXP-1-pb-20261107-1`,
   `-2`, `-3` and `EXP-1-vos-20261107-1`). Of VOS's 9 extra fails, four matter for a v0 in
   production with real customer value (`DEC-19`): two pinned nightlies (`G-37`), a restored backup counted as a
   compromised node (`G-39`), forks under a personal account (`G-48`), and no CI (`G-49`). `G-13`, `G-14` and
   `G-53` don't, because rutent rides no value on a VOS proof. `G-36` misses VOS's own invocation budget, not a
   rutent target, and `G-38` makes key rotation a node replacement. VOS's six `pass (adapter)` rows are open VOS
   gaps that `T-007-03`'s adapter code closes. `pb` passes all six on its own.
3. **The licence.** `pb` adds no line to `licences/RECORD` (`L-04`), so `E4`'s licence clause already holds for it.
   For VOS, see [below](#the-legal-question).
4. **The cost of closing the gaps.** `pb`'s gaps are rutent's own MIT code in `rutent-stm`. VOS's shared gaps sit
   upstream:
   - `G-54` in fjall's recovery mode, which offers no other choice;
   - `G-40` and `G-51` in VOS's store-layout rule;
   - `G-33` in VOS's agent.

   That project has one maintainer (`G-47`) and no tagged release (`G-46`), so rutent would depend on work it
   doesn't control. Its other costs are the nine extra fails, keeping the adapter for the six adapter rows, and
   adapter merges that wait on the licence ruling (`DEC-6`). The state machine also runs in the adapter, not in
   VOS, so VOS would be a replicated store here, not the whole runtime `PS-66` describes.

## The legal question

The recommendation doesn't depend on VOS's licence: it holds on the numbers alone. What the owner's ruling would
change:

- **VOS's licence permitted:** VOS could be chosen, but first the 25 VOS crates and 221 other packages need 246
  more lines in `licences/RECORD` (`REQ-MOD-5`). Permissive ones, the 14 Apache-2.0 VOS crates among them, are
  permitted by the standing ruling `licence-permissive-standing`; copyleft ones come to the owner one by one. The
  run didn't count the copyleft among the 221. The gaps above would still stand.
- **Declined:** VOS can't be chosen at this pin. This outcome doesn't change.
- **Not ruled:** the `vos` line stays `not-adopted` and `pending`, which the licence check never fails on. No VOS
  code reaches `main`.

This page draws no legal conclusion. If the owner leans to VOS, `drafts/licence-item.json` is the item to publish
first.

## Follow-up rows

The gaps the recommendation leaves, each with the plan feature that would own it.

| Gap | Rows | Why it matters for v0 | Owner |
|---|---|---|---|
| A damaged, acknowledged last frame is cut silently | `G-54` | At most one acknowledged entry lost without a refusal (`EXP-1-pb-20261107-1`) | `F-010` |
| Every record is held in memory, and the whole log is read on open | `G-33` | Grows with production volume. Reopening a 93 MiB journal took 339.6 ms (`EXP-1-pb-20261107-2`) | `F-010`; the bound's figure with `NFR-1` in `M4` (`F-033`) |
| No upgrade path for the store format | `G-40`, `G-51` | The first format change after production data exists (`NFR-6`, `REQ-OPS-3`) | `F-010`; cutovers in `F-030` |
| Power loss not reached; no scored run or timing on Linux, only macOS `F_FULLFSYNC` | `EXP-1-pb-20261107-1` note | Production durability rests on the deployment platform's sync | `F-010` tests; `F-030` on the platform |
| No metrics | `G-41` | `STEP-5` signals | `F-033` |
| No figure beyond one host and one disk | `G-42` | `NFR-1`, risk 9 | `F-033`, bounds set when `M4`'s tasks are written |
| No signature, operator name, check, side store or tamper evidence in the store | `G-04`, `G-05`, `G-09` to `G-11` | `PS-21`, `REQ-AUD-1`. ADR-0005 puts these in the authenticated log, apart from the execution store | `F-022`; `F-021` for `G-05`'s operator |
| The state in the log is readable by its operators | `G-26` | `THR-79`. Credentials stay in the proxy (ADR-0004), never in the store | `F-025` |
| No tagged release; one author | `G-46`, `G-47` | The release pins the store; reviews follow `DEC-20` | `F-034`; `DEC-20` |

Not v0's: `G-12` (ADR-0005 rejected an outside anchor; checkpoints are `F-022`'s), `G-43` and `G-44` (`PS-23`, one
deployment under `REQ-OPS-4`; protocol work for v1, `F-052`). The module rows are `EXP-2`'s (`F-008`) and `F-013`'s.
