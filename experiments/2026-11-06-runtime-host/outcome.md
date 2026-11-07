---
experiment: EXP-1
status: revised by T-007-04, 2026-11-07, from the re-score; awaiting the owner's answer to item exp1-adopt-runtime
---

# EXP-1 outcome

The one-page outcome of `EXP-1` (Narthex `REQ-EXP-4`). Every number names the run that produced it (Narthex
`REQ-CHN-17`), from the [scoring sheet](scoring.md) and the [run notes](runs/). Adopting it is one item on the
owner's form, `exp1-adopt-runtime`, with this page as its ref (Narthex `REQ-EXP-7`).

This revision replaces the first outcome (VOS dropped, the store adapted), which scored VOS at `1aadf44` as a remote
key-value store: the state machine outside VOS, one node, the interpreter on macOS. The re-score ran VOS the way it
is meant to be run: the state machine a VOS program, `vos-core` at `5c78975`, Linux x86_64 with the JIT, one node
and three ([scoring sheet §5](scoring.md#5-the-re-score-vos-as-the-whole-runtime)). Its criteria were fixed before
any run (`decisions.json`, `exp1-rescore-criteria`). The first run's cells stay as history.

## Result

| Candidate | Result | What it means |
|---|---|---|
| `vos`, VOS as the whole runtime at `vos-core` `5c78975` | **adapt** | `M2` builds `CMP-STM` as a VOS program on three replicas, at a pinned and mirrored commit, once the [preconditions](#preconditions) are met |
| `pb`, the purpose-built store | the ruled fallback | ADR-0002's fallback, kept behind its flag. It has no replication, and it loses acknowledged records on a restore and on a damaged last frame |
| `ref`, the reference host | the floor | In memory; never a candidate |

Both candidates pass the gate. On the operational rows the first run never reached, VOS on three nodes passes every
row it can run; `pb` fails four, two of them (replication) by construction. `pb` is faster on one disk. A VOS step costs about
a fifth of VOS's 10M default handler gas, about 2% of the 100M the program's `submit` declares.

## Numbers

Scored runs on GitHub's `ubuntu-latest` (AMD EPYC 9V45, 4 vCPUs, 16 GB, kernel 7.0.0-1019-azure), the JIT
forced and proved by each node's perf map: `EXP-1-vos-rt1-20261107-2`, `EXP-1-vos-rt3-20261107-2` and `EXP-1-pb-20261107-5`, in one job on one runner, and
the `R-07` job's own runner for `R-07a` and `R-07b`. Timings are measurements, not targets (`PLAN.md` §6.2, risk 9).

| Row | `vos-rt1`, one node | `vos-rt3`, three nodes | `pb` |
|---|---|---|---|
| `H-01` to `H-17`, host contract 2.0.0 | 17 of 17 pass | 17 of 17 pass | 17 of 17 pass |
| `R-01` the leader killed mid-submit | n/a: a three-node row | pass: 21 kills (one placed through `hold_commits`, 20 at random), a submit in flight at each as last seen; 0 of 497 acknowledged lost | **fail (by construction)**; its restart on the same disk, killed during a submit loop, opens in 11 to 24 ms in 5 rounds |
| `R-02` a replica replaced from the survivors | n/a | pass | **fail (by construction)** |
| `R-03` a replica's data lost, then replaced | n/a | pass, leader and follower | **fail**: restored from a copy, 28 of 28 records acknowledged after it are lost, each already released, so its effects may have run |
| `R-04a` the last batch damaged | pass, the damage's effect on the node not shown: opens, nothing acknowledged lost | pass: the follower rejoins and catches up | **fail**: opens one acknowledged record short, silently |
| `R-04b` the leader and a follower damaged together | n/a | pass, the damage's effect not shown: both reopen, nothing lost, all three agree | n/a: one node |
| `R-05` records/s at 1, 8, 64 submitters | 328, 1,538, 2,979 | 239, 1,387, 1,957 | 4,753, 4,568, 4,544 |
| `R-05` fsyncs a record at 1, 8, 64 | 1.02, 0.15, 0.08 | 3.06, 0.83, 0.20 (three nodes) | 1.00, 1.00, 1.00 |
| `R-06a` the program upgraded, one input in flight | pass | pass | pass, as a redeploy |
| `R-06b` a changed state format installed | pass: the program refuses it | pass | pass: the layout version refuses it |
| `R-07a` the 1aadf44 deployment swapped into the tip program | pass, as `H-13` | n/a: run on one node | n/a |
| `R-07b` 1aadf44's data directory opened at the tip | pass: refused cleanly | n/a: run on one node | n/a |
| `R-07c` the same program across two VOS commits | n/a: no second `vos-core` commit builds it yet | n/a | n/a |
| `R-08` 100,352 records: halt to export, import to ready (RTO) | 3.6 s + 55.5 s = 59.1 s | 4.8 s + 77.0 s = 81.8 s | 0.2 s + 2.8 s = 3.0 s |
| `R-09` a step: gas; µs on the JIT and the interpreter | 1.87M gas (18.7% of 10M); 841 and 1,551 µs | 1.87M gas; 941 µs | 222 µs a synced submit; 0.42 µs the step alone |

The `R-05` and `R-08` numbers compare the candidates on one noisy disk in one job, not either against production.

## Why

1. **The gate does not choose.** Both pass all 17 host rows at host contract 2.0.0, VOS on one node and on three.
   The after-computed rows (`H-02`, `H-06`, `H-11`) pass on the faults build, which holds commits; on the scored
   build the same stop lands after an unheld commit, and `stop-after-computed` fails there, as the first run's
   control did. That is the stop's test, not a VOS gap.
2. **Replication is one property, and only VOS has it.** `R-01` to `R-03` pass on three VOS nodes:
   no acknowledged record was lost when the leader was killed at 21 points, a submit in flight at each as the harness last saw it, or when a replica was replaced, with
   or without its data. `pb` has one node, so it fails `R-01` and `R-02` by construction, and its only recovery
   from a lost disk is a restore, which loses every record acknowledged after the copy (`R-03`), each already released
   to its submitter, so its effects may have run. Counted once, as the brief asks: `pb` lacks replication.
3. **Damage.** A damaged last frame cut `pb` one acknowledged record short without a refusal (`R-04a`, `G-54`).
   On VOS, damage to the last journal batch cost no acknowledged record. That is presumably VOS's layout more than proof:
   the last batch on disk after an acknowledged submit is likely the unsynced apply or commit-index batch that
   follows the synced append, which the log replays over. So the two results are not like for like: `pb`'s
   last frame is its acknowledged append, and VOS's damage probably never reached one. On one node the damage left no visible trace, consistent with that layout, and the harness
   could not place it on the append itself, so fjall's silent cut of a corrupt tail, the single-node residual of
   `G-54`, stands untested. On three nodes fjall cut a damaged follower's tail at the open, about 3 KB, leaving it one entry
   behind; it caught up from the leader once it saw the leader's next entry, and when two nodes were damaged together both reopened
   and all three agreed, with nothing lost, though that run recorded no evidence that the cut took effect
   (`R-04b`). No watermark patch exists at the pin, so that leg is n/a.
4. **Upgrades and formats.** Both survive an upgrade with an input in flight (`R-06a`), and both refuse a changed
   format (`R-06b`). On VOS the refusal is the program's own format check: `Admin::Install` checks nothing at the
   tip. Across VOS commits the tip refuses 1aadf44's store cleanly (`R-07b`), and the swap stream carried the
   first run's deployment, another program on another commit, into the tip program (`R-07a`): VOS upgrades can
   go by swap stream, as `G-40`'s fix says. The same program across two commits is not runnable yet (`R-07c`).
5. **Speed and recovery time.** `pb` is faster on one disk: about 4,500 to 4,800 records/s against VOS's 239 to
   2,979, and a 100k-record swap in 3.0 s against 59 to 82 s. VOS's group commit is visible: on one node one fsync per record
   alone, 0.08 at 64 submitters; on three, 3.06 and 0.20 across the nodes. No `NFR-1` target exists before `M4`. A VOS step costs 1.87M gas, the handler and the
   agent's work together; the runs did not break it down. The JIT is only about 1.8 times the interpreter on this step.
6. **What changed since the first outcome.** The six `pass (adapter)` rows of the first run (`G-01` to `G-03`,
   `G-07`, `G-08`, `G-31`; scoring sheet §2) are now the program's own code, as VOS intends. The first outcome's cost argument, that VOS would be a replicated store and not the runtime, no
   longer holds: the whole host contract runs inside VOS, through a `no_std` copy of `contracts/` whose patch
   `vos-adapter/nostd.sh check` holds to plumbing only (the workflow's `checks` job).
7. **A finding for the port.** The guest's canonical JSON is byte-equal to the native host's on every case of the
   corpus, but neither is RFC 8785, which the host contract names: serde_json sorts keys by UTF-8 bytes and
   writes numbers its own way. Dedup compares the guest's bytes with its own, so nothing fails here; the port
   should either write RFC 8785 or say what canonical JSON it means.

## Preconditions

What must be true for v0 to ship on VOS (the rethink's §8), with where each stands after the re-score:

| Precondition | Where it stands |
|---|---|
| The licence ruled, covering a state machine that links `vos-sdk` | The owner permitted VOS's 12 AGPL-3.0-or-later crates for this experiment (`DEC-6,counsel-licences`); adoption is this item. `licences/RECORD` gained 225 lines, counted from every experiment lockfile; the tip's lockfiles (`vos-adapter/rt`, `vos-adapter/program`) lock 274 packages from a registry or git, and 3 of the experiment's own path crates the licence check counts as external: 277 that need a line |
| A VOS commit pinned by SHA and mirrored, with its forks (`lsm-tree`, `stwo`, `s4`) | Pinned by SHA here; not mirrored yet (`G-48`) |
| The state machine inside VOS, passing host contract 2.0.0 on three nodes | Done here, as an experiment. For `M2`, `contracts/` goes `no_std` + `alloc` through a `DEC-4` bump and the owner's click (`DEC-20`); the patch this experiment applied shows it is plumbing only |
| `G-54` fixed upstream, or its residual accepted for both candidates | Not fixed at the tip. A damaged follower caught up from the leader in these runs; damage to a leader's append, and the single-node residual, are untested |
| A recovery story for total loss | Scheduled halted exports: about 59 to 82 s of downtime at 100k records, the RPO the export interval (`R-08`); or upstream `vos node restore` |
| The JIT verified on the production host | Verified on the runner (kernel 7.0, userfaultfd write-protect, perf maps); the production host is still to check, and VOS's node still builds on two pinned nightlies with native codegen (`G-37`) |
| Journal growth bounded | Open (`G-33`): archive terminal orders in the program |

Things to follow, not preconditions: a tag (`G-46`), the forks upstreamed (`G-48`), a second maintainer (`G-47`).

## The legal question

The owner approved recording VOS's AGPL crates as permitted for this experiment, under `counsel-licences`. The
adoption is the separate ruling this item asks; until then `licences/RECORD` keeps `vos not-adopted ... pending`.
This page draws no legal conclusion. `drafts/licence-item.json` stays the item to publish if the owner wants the
licence ruled apart from the adoption.

## The drafts

`drafts/adr-0002-amendment.md` and `drafts/plan-decision.md` were written for the first outcome. Their
[If the owner chooses VOS](drafts/adr-0002-amendment.md#if-the-owner-chooses-vos) section is the starting point if
the owner takes this recommendation; promotion rewrites them from the owner's answer.
