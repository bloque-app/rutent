# VOS behind the host contract (`T-007-03`)

> **Not mergeable until the owner rules on `DEC-6`.** The code in this directory links VOS, whose crates are
> AGPL-3.0-or-later. `licences/RECORD` lists `vos not-adopted ... pending`, and the owner has not ruled. Under
> `PLAN.md` `T-007-03`, only the measurement notes merge until VOS's licence decision is in the record. They live in
> [`../runs/`](../runs/) and in [`../scoring.md`](../scoring.md), in commits of their own, so they can be picked
> alone.

`EXP-1`'s VOS candidate (`vos` in [the record](../README.md#candidates)): rutent's host contract
(`contracts/state-machine/host/`) met by a VOS node, fault-injected, and scored by the host-contract suite
(`conformance/src/host/`).

## Kept out of rutent's build

- `journal/` and `adapter/` are cargo workspaces of their own, each with its own lockfile and toolchain. Neither is
  a member of rutent's workspace, so rutent's `Cargo.lock`, its licence check and its default build never see VOS.
- VOS is named by its repository URL at the pinned commit, `1aadf44fa2cb1eaedc015ee82343ac118cc89f1e` (item
  `exp1-vos-commit` in [`../decisions.json`](../decisions.json)). Nothing of VOS is copied here. The guest
  build reads VOS's target and linker script from the checkout at `.vos`. Each lockfile started as a copy of VOS's
  own lockfile at that commit (`agent/test-actors/Cargo.lock` for `journal/`, `Cargo.lock` for `adapter/`), which
  cargo then re-resolved. The adapter repeats VOS's `[patch.crates-io]` for `lsm-tree`.
- The flag that enables the candidate is `rutent-vos-adapter`'s cargo feature `vos`. Without it the library is
  empty and nothing of VOS is compiled or linked; `build.sh` builds with `--features vos`.
- The manifests declare no licence. How VOS-linked code is handled is the owner's `DEC-6` ruling.

## What is here

| Path | What it is |
|---|---|
| `journal/` | The journal actor, a VOS guest (`no_std`, RV64EM), holding the host's durable state in its rows |
| `adapter/` | `rutent-vos-adapter`: `VosHost` (`Host`), `VosHarness` (`Harness`, `FaultSeam`), and `exp1-vos`, which runs the suite and prints its report |
| `fetch-vos.sh` | Fetches VOS at the pinned commit into an empty directory, and links it as `.vos` |
| `build.sh` | Builds the `vos` binary with VOS's `faults` feature, VOS's default agent program, the journal and `exp1-vos` |
| `run.sh` | Runs every pass of one run and writes each pass's report |

## How it meets the contract

**The state machine runs in the adapter, not in VOS.** rutent's state-machine contract needs `std` and `serde`. A
VOS guest is `no_std` on a custom RV64EM target with no atomics. So `VosHost::submit` reads the order's latest
state from VOS, calls `open` or `transition`, and sends the record to the journal.

**Every rule that needs state is VOS's.** The journal decides, in one handler step, which VOS commits whole or not
at all:

- a halted journal refuses;
- a record id seen before returns its record if the input is byte-equal canonical JSON, and otherwise is refused,
  with the refusal written to the refusal log;
- the order's next sequence number must be the one sent, a compare-and-set;
- an open import refuses appends, and `open` refuses a store whose import never finished.

The adapter keeps nothing authoritative in memory, so the suite's kill drops nothing the contract needs. Reads are
linearizable queries of the journal.

**Faults.** A kill is a `SIGKILL` of the `vos node run` process; reopening starts it again on the same data
directory. The harness implements `FaultSeam` by timing a kill to each crash point:

| Crash point | How it is reached |
|---|---|
| After received | The adapter holds the input; nothing is computed or sent. The node is killed |
| After computed | The adapter has computed the transition. VOS's fault file holds every commit (`hold_commits`), the append is sent, and after 1 s the node is killed; held, VOS cannot have made it durable. That VOS had received the append by then is inferred, not observed: a control pass (`control-no-hold`) sends the same append without the hold, and the record is then durable before the kill, so the append reached VOS within the 1 s and the hold, not the timing, is what keeps it out |
| After durable | VOS answered the append as run, so it is committed. The node is killed before the adapter acknowledges it |
| Stop during an import | The node is killed between two entries' calls |

A refused import leaves the new store marked as importing, so it can't be opened; the import is redone on a fresh
store. The reference host does the same (`components/stm/src/reference_host.rs`, `import_onto`).

## Build and run

```sh
./fetch-vos.sh <empty dir>      # VOS at the pin, linked as .vos; read it before building it
./build.sh <build dir>
./run.sh <run id> <build dir> <short output dir>
```

`run.sh`'s passes are those of `exp1-vos`: `sizes`, `compaction`, `session-expiry`, `vos-to-ref`, `ref-to-vos`,
`compacted` and `control-no-hold` (see `adapter/src/main.rs`). The scripts set `CARGO_TARGET_DIR` for each build, so a global
setting does not move the builds.
