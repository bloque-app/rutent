---
Status: promoted into adr/0002-runtime-host-seam.md, 2026-11-08
amends: adr/0002-runtime-host-seam.md
experiment: EXP-1
decision_item: exp1-adopt-runtime (answered 2026-11-08, VOS as the whole runtime on three replicas)
---

# Draft: amendment to ADR-0002, the runtime that implements the host contract

A draft of `EXP-1` (Narthex `REQ-EXP-3`). It is not accepted and not part of ADR-0002 until the owner answers item
`exp1-adopt-runtime` and promotion consolidates it (Narthex `REQ-EXP-5`). It amends ADR-0002 and does not reopen
`runtime-host-seam` (`DEC-6`). It is written for the item's recommended option, from the revised
[outcome](../outcome.md). [If the owner chooses the purpose-built store](#if-the-owner-chooses-the-purpose-built-store)
says what changes.

## Context

ADR-0002 made `CMP-STM` host-agnostic and left the runtime to a milestone-1 experiment: VOS (`PS-66`), or a
purpose-built store as the ruled fallback. `EXP-1` scored both behind the host contract, with the host-contract
suite (`T-005-03`), the gap list of `research/2026-11-02-vos.md`, and the licence rows of `DEC-6`
([scoring sheet](../scoring.md)).

- **The first runs** (host contract 1.0.0) scored VOS at `vos-core` `1aadf44` as a store behind an adapter that ran
  the state machine itself: `EXP-1-vos-20261107-1`, and the store in `EXP-1-pb-20261107-1` to `-3`. They stay as
  history.
- **The re-score** (host contract 2.0.0) ran VOS the way it is meant to be run: the state machine a VOS program, at
  `vos-core` `5c7897582688ea93f8d02506bee8ea25da6b1458`, on Linux x86_64 with the JIT, on one node and on three,
  against new operational rows whose criteria were fixed before any run (`decisions.json`,
  `exp1-rescore-criteria`): `EXP-1-vos-rt1-20261107-2`, `EXP-1-vos-rt3-20261107-2` and `EXP-1-pb-20261107-5`.

## Outcome of EXP-1

From the [outcome page](../outcome.md):

- **VOS, as the whole runtime: adapt.**
- **The purpose-built store: the ruled fallback.**

The numbers, each with its run:

- **Gate.** Both pass all 17 host-contract rows at host contract 2.0.0; VOS on one node and on three, its three
  after-computed rows on its faults build (`EXP-1-vos-rt1-20261107-2`, `EXP-1-vos-rt3-20261107-2`,
  `EXP-1-pb-20261107-5`).
- **Replication, one property** (`R-01` to `R-03`). On three VOS nodes, all on one runner, no acknowledged record
  was lost across 21 leader kills, a replica replaced, and a replica's disk lost (`EXP-1-vos-rt3-20261107-2`). The
  store has one node: its restarts after a `kill -9` lost nothing, and a restore from a copy lost the 28 records
  acknowledged after it (`EXP-1-pb-20261107-5`). Losing the host was not tested.
- **Damage** (`R-04`, `G-54`) was not delivered alike and decides nothing: on the store it hit an acknowledged
  frame, which was cut silently; on VOS it did not land on an acknowledged batch. `G-54` is open for both.
- **Upgrades and formats** (`R-06`). Both pass; for the store the upgrade is a redeploy of the binary, as the
  criterion says.
- **The costs of VOS.** At 64 submitters the store writes about 4.5k records/s in-process behind a mutex, VOS 3.0k
  on one node and 2.0k on three through separate clients, with a far higher p50 and a lower p99 (`R-05`). A 100k-record halted export
  and import takes the store 3.0 s in-process in one call, VOS 59.1 s and 81.8 s in pages of 8 entries over the
  node API; both grow with history (`R-08`). A synced submit costs
  the store 222 µs; VOS's step costs 841 µs on the JIT and a whole single-input batch 1,348 µs, the guest's own
  work dominating (`R-09`).
- **Licence.** The owner permitted VOS's 12 AGPL-3.0-or-later crates for the experiment (`DEC-6,counsel-licences`);
  `licences/RECORD` lists every package the experiment's lockfiles lock. Adoption is the item's ruling.

## Decision

VOS implements the host contract for v0, as the whole runtime: `M2` builds `CMP-STM` as a VOS program on three
replicas (`F-010`).

- **The commit.** A `vos-core` commit pinned by SHA and mirrored on Bloque's side, with its forks (`lsm-tree`,
  `stwo`, `s4`) at their pinned revisions (`G-48`). A tag is followed, not required (`G-46`).
- **The contracts.** `contracts/` is ported to `no_std` + `alloc` through a `DEC-4` bump and the owner's click
  (`DEC-20`). `EXP-1`'s generated copy shows the port is plumbing only.
- **The purpose-built store** stays ADR-0002's ruled fallback, behind its flag, and the host contract stays
  runtime-neutral, so a later `EXP-n` may score another runtime without reopening `runtime-host-seam`.
- **The reasons.** Only VOS has replication: it turns losing a node or a node's disk into replacing a replica,
  where the store's only recovery from losing its disk is a restore that loses the records acknowledged after the
  copy. Losing every copy was not run on VOS; its path for that is a halted export, with the same shape of loss. The costs are throughput, a total-loss
  recovery time longer than the store's, and a slower step, none of them against a target before `M4`.

**Which wording stands on the licence.** ADR-0002 says VOS's licence needs counsel before adoption. The owner's
later ruling `counsel-licences` and `DEC-6` replace that step: a dependency's licence is the owner's decision,
recorded case by case in `licences/RECORD` (`REQ-MOD-5`). `DEC-6`'s wording stands, and ADR-0002's counsel sentences
read as superseded by it. This records which ruling applies. It draws no legal conclusion about any licence.

## Consequences

- **Preconditions before v0 ships on VOS** (the outcome's [preconditions](../outcome.md#preconditions)):
  - the licence ruled, covering a state machine that links `vos-sdk`;
  - the commit pinned and mirrored, with its forks;
  - the state machine inside VOS passing host contract 2.0.0 on three nodes, after the `DEC-4` port;
  - `G-54` fixed upstream or its residual accepted for both candidates, after the targeted test: damage to the
    frame holding the last acknowledged append, on one VOS node and on a leader of three;
  - a recovery story for total loss: scheduled halted exports, whose downtime and RPO are written down, or
    upstream `vos node restore`;
  - the JIT verified on the production host (kernel 6.7 or later, userfaultfd allowed), and a build without native
    codegen (`G-37`);
  - journal growth bounded: the program archives terminal orders (`G-33`);
  - power loss and the loss of the host tested, which no run reached on either candidate.
- **Owned elsewhere.** The audit trail to `F-022`, apart from the execution store (ADR-0005); metrics and load
  figures to `F-033`; `G-26` to `F-025`; the release to `F-034`.
- **The `vos` line** in `licences/RECORD` moves from `not-adopted` and `pending` to the owner's decision, with the
  pinned commit's crates already listed.
- **The module side.** `EXP-2` (ADR-0003) is unaffected. The module-runtime rows VOS failed are reported to it,
  not decided here.

## If the owner chooses the purpose-built store

Then the Decision is reversed:

- the store implements the host contract for v0, and `F-010` builds `CMP-STM`'s library on it, out of its
  experimental flag; it brings no new package;
- v0 has no replication: losing the disk means restoring a copy, which loses the records acknowledged after it, so
  the deployment's backup interval is its RPO;
- before production, `F-010` closes `G-54`, open for both (shown on the store, where a damaged acknowledged last
  frame is cut silently, and untested on VOS), and `G-33`, `G-40` and `G-51`, which both candidates fail, and tests power loss on the deployment
  platform, which no run reached on either;
- VOS is not adopted for v0, its code stays as `EXP-1`'s record, and the `vos` line stays `not-adopted`.
