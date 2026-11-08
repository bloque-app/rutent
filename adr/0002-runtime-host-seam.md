---
status: Accepted (Pablo, 2026-11-06, decision-form item `runtime-host-seam`); amended by EXP-1 (Pablo, 2026-11-08, item `exp1-adopt-runtime`)
decision_item: runtime-host-seam, exp1-adopt-runtime
supersedes: adopts and reframes suggestions/ADR-001-durable-runtime.md
---

# ADR-0002: Runtime / host seam for `CMP-STM`

## Context

`suggestions/ADR-001` proposed writing the state machine's transition rules as a host-independent library first,
then running a time-boxed spike of VOS (`virto-network/vos-core`) — the runtime Pablo's own brief named as his
preference (`PS-66`) — behind a thin wrapper, fault-injected against a gap list, falling back to a purpose-built
core if the gaps stayed open.

`research/2026-11-02-vos.md` (re-verified at a pinned commit by this session's own spot-check) found real,
present-tense gaps if VOS were adopted as-is: no publisher-signature check, a compacted log that keeps only a
tail of entries and drops inputs (`PS-21`'s who/what/when/outcome needs more than VOS's log gives today), no
second sandbox layer, no containment fuzzing, a Rust-only guest SDK, and — load-bearing for `REQ-MOD-5` — the
`vos-core`/`vos-sdk` workspace is AGPL-3.0-or-later, with no linking exception found in the report. Compatibility
with rutent's MIT licensing intent needs counsel before adoption either way, independent of whether VOS otherwise
performs. (Superseded: the licence is the owner's decision, recorded case by case, with no counsel step; see the
amendment below.)

The real design question is not *when* to run a spike, but *whether `CMP-STM`'s contract itself needs to assume a
specific runtime to be designed at all.* It doesn't: `REQ-STM-1/3` ("one current state... changes only by
recorded input... replay-deterministic") and `REQ-OPS-3` ("replaceable execution environment without losing
in-flight orders") are both expressible against an abstract durable-store/host contract, with no runtime named.

## Options considered

1. **Commit to VOS now**, writing `CMP-STM` directly against its APIs. Fastest if VOS works out; commits
   `ARCHITECTURE.md`'s deployment view to an unaudited, near-single-author, AGPL-licensed dependency before
   counsel has weighed in and before its sandbox/log gaps are closed — a real risk to reverse later if it doesn't.
2. **Write `CMP-STM` host-agnostic now**, against a small contract (durable transition storage; recorded-input
   delivery, exactly-once to the state machine; environment-swap without losing in-flight orders), and treat
   *which* runtime implements that contract — VOS, or a purpose-built store — as a milestone-1 experiment
   (`REQ-CHN-12`'s "first milestone" framing, Narthex's `experiments/` mechanism) rather than a STEP-2 commitment.
3. **Build a purpose-built runtime now**, skipping VOS entirely. Discards real, already-gathered evidence (VOS's
   gaps are specific and fixable in principle) before anyone has tried to close them; premature given `SPEC.md`
   itself left this choice open.

## Decision

Option 2, Pablo's ruling, 2026-11-06 (`runtime-host-seam`, "Host-agnostic now, experiment later").

`CMP-STM` is specified and built against a host-agnostic contract. The module sandbox gets the same treatment
(ADR-0003): process-per-provider is firm now, Wasm-vs-PVM is the experiment. Either runtime choice still needs
the durable-store/host contract this ADR defines, so `REQ-OPS-3`'s environment-swap requirement is satisfied by
the contract's existence, independent of which runtime eventually implements it.

VOS remains the leading experiment candidate given `PS-66`, but its AGPL-3.0 licensing and sandbox/log gaps must
be resolved — by counsel on the licence, by further evaluation on the gaps — before the experiment's outcome can
become an adoption decision, not before the experiment can run. (The counsel step is superseded by `DEC-6` and
`counsel-licences`; the amendment below records the outcome.)

## Consequences

- `CMP-STM`'s public contract (frozen at milestone 1 per `REQ-CHN-12`) is runtime-agnostic from day one; no later
  runtime swap requires rewriting the state machine's own logic, only reimplementing the host contract's
  adapter.
- The VOS AGPL-3.0 question is explicitly *not* resolved by this ADR. It gates adoption, not evaluation. It is no
  longer deferred to counsel: it is the owner's decision in `licences/RECORD` (`DEC-6`, `counsel-licences`; see the
  amendment below).
- A milestone-1 experiment (VOS vs. a purpose-built store, fault-injected against the gap list `research/2026-11-02-vos.md`
  names) produces its own one-page outcome (adopt/adapt/drop, per Narthex's experiment convention) and amends
  this ADR; it does not reopen `runtime-host-seam` itself.

## Amendment: EXP-1's outcome (2026-11-08)

Accepted by the owner on 2026-11-08 (item `exp1-adopt-runtime`: "VOS as the whole runtime, three replicas"), and
promoted from `EXP-1`'s draft (Narthex `REQ-EXP-5`). It decides the runtime the Decision above left to the
experiment, and does not reopen `runtime-host-seam` (`DEC-6`). The experiment's record stays under
[`experiments/2026-11-06-runtime-host/`](../experiments/2026-11-06-runtime-host/).

### Context

`EXP-1` scored both candidates behind the host contract, with the host-contract suite (`T-005-03`), the gap list of
`research/2026-11-02-vos.md`, and the licence rows of `DEC-6`
([scoring sheet](../experiments/2026-11-06-runtime-host/scoring.md)).

- **The first runs** (host contract 1.0.0) scored VOS at `vos-core` `1aadf44` as a store behind an adapter that ran
  the state machine itself: `EXP-1-vos-20261107-1`, and the store in `EXP-1-pb-20261107-1` to `-3`. They stay as
  history.
- **The re-score** (host contract 2.0.0) ran VOS the way it is meant to be run: the state machine a VOS program, at
  `vos-core` `5c7897582688ea93f8d02506bee8ea25da6b1458`, on Linux x86_64 with the JIT, on one node and on three,
  against new operational rows whose criteria were fixed before any run (`decisions.json`,
  `exp1-rescore-criteria`): `EXP-1-vos-rt1-20261107-2`, `EXP-1-vos-rt3-20261107-2` and `EXP-1-pb-20261107-5`.

### Outcome of EXP-1

From the [outcome page](../experiments/2026-11-06-runtime-host/outcome.md):

- **VOS, as the whole runtime: adapt.**
- **The purpose-built store: the ruled fallback.**

The numbers, each with its run:

- **Gate.** Both pass all 17 host-contract rows at host contract 2.0.0; VOS on one node and on three, its three
  after-computed rows on its faults build (`EXP-1-vos-rt1-20261107-2`, `EXP-1-vos-rt3-20261107-2`,
  `EXP-1-pb-20261107-5`).
- **Replication, one property** (`R-01` to `R-03`). On three VOS nodes no acknowledged record was lost across 21
  leader kills, a replica replaced, and a replica's disk lost (`EXP-1-vos-rt3-20261107-2`). The store has one node:
  its restarts after a `kill -9` lost nothing, and a restore from a copy lost the 28 records acknowledged after it
  (`EXP-1-pb-20261107-5`). Losing the host was not tested.
- **Damage** (`R-04`, `G-54`) was not delivered alike and decides nothing: on the store it hit an acknowledged
  frame, which was cut silently; on VOS it did not land on an acknowledged batch. `G-54` is open for both.
- **Upgrades and formats** (`R-06`). Both pass; for the store the upgrade is a redeploy of the binary, as the
  criterion says.
- **What VOS costs, on one host.** At 64 submitters the store writes about 4.5k records/s in-process behind a mutex,
  VOS 3.0k on one node and 2.0k on three through separate clients, with a far higher p50 and a lower p99 (`R-05`).
  A 100k-record halted export and import takes the store 3.0 s in-process in one call, VOS 59.1 s and 81.8 s in
  pages of 8 entries over the node API; both grow with history (`R-08`). A synced submit costs the store 222 µs;
  VOS's step costs 841 µs on the JIT and a whole single-input batch 1,348 µs, the guest's own work dominating
  (`R-09`).
- **Where they were measured.** Every scored run of the compared rows was one job on one GitHub runner. The three VOS nodes shared that host
  and replicated over loopback, with no network latency, loss or partition between them. The costs compare the
  candidates on one host only; they are not what a networked deployment would see.
- **Licence.** The owner permitted VOS's 12 AGPL-3.0-or-later crates for the experiment (`DEC-6,counsel-licences`);
  `licences/RECORD` lists every package the experiment's lockfiles lock.

### Decision

VOS implements the host contract for v0, as the whole runtime: `M2` builds `CMP-STM` as a VOS program on three
replicas (`F-010`).

- **The commit.** A `vos-core` commit pinned by SHA and mirrored on Bloque's side, with its forks (`lsm-tree`,
  `stwo`, `s4`) at their pinned revisions (`G-48`). A tag is followed, not required (`G-46`).
- **The contracts.** `contracts/` is ported to `no_std` + `alloc` through a `DEC-4` bump and the owner's click
  (`DEC-20`). `EXP-1`'s generated copy shows the port is plumbing only.
- **The purpose-built store** stays this ADR's ruled fallback, behind its flag, and the host contract stays
  runtime-neutral, so a later `EXP-n` may score another runtime without reopening `runtime-host-seam`. Its code
  stays as `EXP-1`'s record.
- **The reasons.** Only VOS has replication: it turns losing a node or a node's disk into replacing a replica,
  where the store's only recovery from losing its disk is a restore that loses the records acknowledged after the
  copy. Losing every copy was not run on VOS; its path for that is a halted export, with the same shape of loss.
  The costs are throughput, a total-loss recovery time longer than the store's, and a slower step, none of them
  against a target before `M4`, and all of them measured on one host.

**Which wording stands on the licence.** This ADR's first text said VOS's licence needs counsel before adoption. The
owner's later ruling `counsel-licences` and `DEC-6` replace that step: a dependency's licence is the owner's
decision, recorded case by case in `licences/RECORD` (`REQ-MOD-5`). `DEC-6`'s wording stands. This records which
ruling applies. It draws no legal conclusion about any licence.

### Consequences

- **Preconditions before v0 ships on VOS** (the outcome's
  [preconditions](../experiments/2026-11-06-runtime-host/outcome.md#preconditions)):
  - the licence recorded for adoption, covering a state machine that links `vos-sdk`: the `vos` line in
    `licences/RECORD` moves from `not-adopted` and `pending` to the owner's decision, with the pinned commit's
    crates already listed;
  - the commit pinned and mirrored, with its forks;
  - the state machine inside VOS passing host contract 2.0.0 on three nodes, after the `DEC-4` port;
  - `G-54` fixed upstream or its residual accepted, after the targeted test: damage to the frame holding the last
    acknowledged append, on one VOS node and on a leader of three;
  - a recovery story for total loss: scheduled halted exports, whose downtime and RPO are written down, or
    upstream `vos node restore`;
  - the JIT verified on the production host (kernel 6.7 or later, userfaultfd allowed), and a build without native
    codegen (`G-37`);
  - journal growth bounded: the program archives terminal orders (`G-33`);
  - power loss and the loss of the host tested, which no run reached on either candidate.
- **The costs measured again over a network.** Before any claim rests on `R-05`, `R-08` or `R-09`, `F-033` measures
  them, and replays `R-01` to `R-03`, with the three replicas on separate hosts or behind a synthetic network that
  adds latency, loss and partitions (`G-42`). A replicated commit pays a network round trip there that loopback
  hides.
- **Owned elsewhere.** The audit trail to `F-022`, apart from the execution store (ADR-0005); metrics and load
  figures to `F-033`; `G-26` to `F-025`; the release to `F-034`.
- **The module side.** `EXP-2` (the amended ADR-0003) is unaffected. The module-runtime rows VOS failed are
  reported to it, not decided here.
- **A finding for the port.** `contracts/` names RFC 8785 for canonical JSON, but its serialiser writes
  serde_json's form; the guest's bytes equal the native host's on every case of the corpus, so nothing fails
  here. The `DEC-4` port either writes RFC 8785 or says which canonical JSON it means (`DEC-20`).
