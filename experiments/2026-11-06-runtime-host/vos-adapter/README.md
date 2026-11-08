# VOS behind the host contract (`T-007-03`)

> **VOS's licence.** The code in this directory links VOS, whose crates are AGPL-3.0-or-later. On 2026-11-07 the
> owner approved recording them as permitted for this experiment (`licences/RECORD`, `DEC-6,counsel-licences`),
> under the `counsel-licences` ruling. The owner chose VOS as v0's runtime on 2026-11-08
> (`exp1-adopt-runtime`). Until the owner records the licence for that adoption, `licences/RECORD` keeps
> `vos not-adopted ... pending`.

`EXP-1`'s VOS candidate, two ways:

- **The first run** (`EXP-1-vos-20261107-1`, VOS `vos-core` at `1aadf44`): VOS as a durable store behind an
  adapter that ran the state machine itself. Its code is `journal/` and `adapter/`, kept buildable because the
  re-score's `R-07a` and `R-07b` cross from it. Its note and cells stay as history.
- **The re-score** (VOS `vos-core` at `5c78975`, items `exp1-rescore-*` in [`../decisions.json`](../decisions.json)):
  VOS as the whole runtime. The state machine is a VOS program (`program/`), and `rt/` is its thin `Host` client,
  run on one node (`vos-rt1`) and on three (`vos-rt3`), on Linux x86_64 with the JIT, by the workflow
  `.github/workflows/exp1-vos.yml`.

## Kept out of rutent's build

- `journal/`, `adapter/`, `program/` and `rt/` are cargo workspaces of their own, each with its own lockfile and
  toolchain. None is a member of rutent's workspace, so rutent's `Cargo.lock` and default build never see VOS.
  Rutent's licence check reads their lockfiles too, and `licences/RECORD` lists every package they lock.
- VOS is named by its repository URL at each commit. Nothing of VOS is copied here. `fetch-vos.sh pin|tip DIR`
  fetches a commit into a directory outside `experiments/` (or under a `target/` directory), and the build scripts
  take the checkout from a variable, `VOS_PIN` or `VOS_TIP`, never from a link: the licence check refuses symbolic
  links under `experiments/`, and VOS's own lockfiles never sit here.
- The flag that enables the candidate is each client crate's cargo feature `vos`. Without it the library is empty
  and nothing of VOS is compiled or linked.

## What is here

| Path | What it is |
|---|---|
| `program/` | The re-score's VOS program: one actor holding the journal and the state machine (`src/lib.rs`), built for the PVM with VOS's guest toolchain; features `next` and `next-format` build `R-06a`'s and `R-06b`'s versions |
| `program/contracts-nostd/`, `program/probe-nostd/` | Generated no_std copies of `contracts/src` and of conformance's probe, and the patches that make them; `program/SEPARATE` lists the files beside them |
| `nostd.sh` | `generate` writes the copies from the originals and the patches; `check` fails unless they are equal, every patch hunk is plumbing only, and the separate files are exactly the listed ones |
| `rt/` | `rutent-vos-rt`: the thin `Host` client (`host.rs`), the node processes (`cluster.rs`), the platform assertion (`platform.rs`), and `exp1-rt`, which runs the passes (`passes.rs`, `rows.rs`, and `pb.rs` for the purpose-built store) |
| `build-rt.sh`, `run-rt.sh`, `note-rt.sh` | Build the re-score on the machine that runs it, run its passes, write a run's note |
| `journal/`, `adapter/` | The first run's journal actor and adapter, at VOS `1aadf44`; `exp1-vos export-file` exports the stream and leaves the data directory `R-07` crosses |
| `fetch-vos.sh`, `build.sh`, `run.sh` | Fetch either commit; build and run the first run's passes |

## The re-score: how it meets the contract

**One handler step a submit.** `submit(record_id, input)` does the format check, the import and halted checks,
record-id dedup on byte-equal canonical JSON, the order's next sequence number, `open` or `transition` and the
record or the refusal, in one step, which VOS commits whole or not at all. It rests on VOS v1's dispatch rule, one
input at a time in batch order, which the program says in a comment.

**Reads** are queries: `orders`, `state`, `journal`, `refusals`, and the export's pages, each under VOS's 64 KiB
reply cap. The effect driver's cursor is the query `released_from`. **Import** is paged: one chunk of the snapshot
stream a call, at most `IMPORT_ENTRIES` (8) entries or `IMPORT_BYTES` (48 KiB), with a resume cursor; every run note
states the page.

**The state machine** is the suite's probe, built from the no_std copies, so the suite's assertions run unmodified
and only the state machine's build target differs. `H-17`'s fold is the original `contracts/` and `Probe`, natively.
The serialiser is not replaced: the guest uses serde_json built alloc-only, and a byte-equality corpus (the pass
`corpus`) runs floats, `-0`, exponents, large integers, non-ASCII keys and every recorded-input fixture through the
native build and the guest.

**Formats.** The actor's struct state is the same in every version; its `format` field names the rows' layout, and
a version of another format refuses every handler and query before it reads a row (`R-06b`).

**The platform, fail closed.** Before a pass: `uname -r` is 6.7 or later, and a userfaultfd probe with
`UFFD_USER_MODE_ONLY` and asynchronous write-protect succeeds. Every node runs with `VOS_PVM_BACKEND=recompiler`
and `VOS_PVM_PERF_MAP=1`; each node that answers a query of the program on its own replica must name the program
in `/tmp/perf-<pid>.map`, and no node's log may say the map is off. `R-09`'s interpreter leg asserts the opposite.
On three nodes each node's executors are floor(vCPUs ÷ 3), at least 1.

**Faults.** A kill is `kill -9` of a node's process. The suite's stop and reopen kill and restart every node; the
after-computed stop holds every node's commits (`hold_commits`, the faults build) and kills them once the leader's
`refine` stage counted the submit. `R-01` kills the leader alone.

## Build and run

```sh
./fetch-vos.sh tip <dir outside experiments>        # read it before building it
VOS_TIP=<dir> ./build-rt.sh <build dir>
./run-rt.sh <run id> <build dir> <short output dir> platform smoke corpus suite-1 ...
./note-rt.sh <run id> <candidate> <build dir> <output dir> <notes dir> <passes...>
```

`exp1-rt <id> list <dir>` lists every pass. Off Linux x86_64, `EXP1_DEV=1` runs a development run on the
interpreter, which is never scored.
