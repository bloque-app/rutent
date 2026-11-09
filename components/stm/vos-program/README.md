# CMP-STM as a VOS program (`T-010-05`)

> **Licence.** This package links VOS's `vos-sdk`, which is AGPL-3.0-or-later, so the package and every binary built
> from it are AGPL-3.0-or-later (`license = "AGPL-3.0-or-later"` in both manifests). The owner ruled this on
> 2026-11-09 (item `vos-licence-v0`): AGPL covers only the package that links `vos-sdk`. `rutent-stm`, which the
> program runs, and every other package of this repository stay MIT and runtime-neutral behind the host contract, so
> the purpose-built store stays a drop-in fallback (`DEC-26`, ADR-0002). `licences/RECORD` lists the package.

`DEC-26`: `CMP-STM` runs as a VOS program on three replicas. The program is `EXP-1`'s re-score program
(`experiments/2026-11-06-runtime-host/vos-adapter/program/`) with the host-contract suite's probe replaced by
`rutent-stm`'s `Stm`, built `no_std` with `default-features = false`. The thin `Host` client is `client/`.

## Kept out of rutent's build

- This directory and `client/` are cargo workspaces of their own, each with its own `Cargo.lock` and toolchain. Neither
  is a member of rutent's workspace, so rutent's `Cargo.lock` and `cargo test --workspace` never see VOS and never
  fetch it. `licences/RECORD` lists every package they lock, and the licence check reads their locks.
- The client sits behind the `vos` feature of its own package, `rutent-stm-vos`, off by default. It is not a `vos`
  feature of `rutent-stm`: that would put VOS's crates into rutent's own `Cargo.lock`, make `rutent-stm` depend on
  the program that depends on it, and bring the AGPL into the MIT library.
- VOS's checkout and its lockfiles never sit in the tracked tree: `fetch-vos.sh DIR` fetches it into a directory
  outside it (or under a `target/` directory). Treat what it fetches as untrusted and read it before building it.

## What is here

| Path | What it is |
|---|---|
| `src/` | The program: one actor holding the journal and `Stm` (`lib.rs`), and its entry (`main.rs`), for VOS's guest target (`riscv64em-vos`, toolchain `nightly-2026-03-20`). Its feature `probe-machine` builds it with the host-contract suite's probe instead, for the suite (below) |
| `probe-nostd/`, `probe-nostd.patch`, `nostd.sh`, `SEPARATE` | The generated no_std copy of the suite's probe, the patch that makes it, the script that generates it and checks it (`nostd.sh check` runs in `build.sh` and CI), and the files beside it |
| `client/` | `rutent-stm-vos`: the `Host` client (`host.rs`), the node processes of one node or three (`cluster.rs`), the platform assertion (`platform.rs`), and the retry of an open refused while the platform spawns children (`retry.rs`, over `rutent_stm::open_retry`); toolchain `nightly-2025-07-14` |
| `client/tests/` | `vos_node.rs`: the host-contract suite on one node and on three, the program's states against `Stm`'s, and the leader killed mid-submit (they need the VOS toolchain); `retry.rs`: the open retry, which needs no node |
| `host-check.sh`, `RECOVERY.md` | The production-host check (`T-010-08`, `G-37`): kernel 6.7 or later, userfaultfd write-protect, the JIT's perf map; the total-loss recovery runbook, with its drill |
| `sources` | The list of sources, `G-48`: each source's name, SHA, upstream URL and fetch URL |
| `sources.sh`, `fetch-vos.sh`, `build.sh`, `test.sh` | Read the list; fetch VOS; build the node, the agent, the program and the client's tests on the machine that runs them; run the tests |

## Pins and the list of sources

| Source | Revision |
|---|---|
| `vos-core` (`vos`) | `5c7897582688ea93f8d02506bee8ea25da6b1458` |
| `lsm-tree` (fork) | `d0791931a96ff2be9b23af4e0d1ef17a330ed9f2` |
| `stwo` (fork) | `e14925a81606b4d68c6a5d3047d382a20bd953d3` |
| `s4` (fork) | `a11a57f806b944f62e16e63adc2f62eb81a28474` |

The manifests and lockfiles name each source by its upstream URL, at its revision. `sources` says where to fetch each
one from; the scripts keep cargo's URLs and rewrite them to the fetch URLs with git's `insteadOf` (VOS itself comes
from the checkout, never the network). A mirror therefore replaces a fetch URL in `sources`, or a whole other list
through `SOURCES=<file>`, and nothing else changes. `sources.sh check` fails if a revision the manifests or
lockfiles pin differs from the list's, and `build.sh` fails if VOS's own lockfile pins a fork at another revision.

**Fallback mirrors.** The owner had the four sources mirrored privately under the GitHub org `bloque-app`:
`bloque-app/vos-mirror`, `bloque-app/lsm-tree`, `bloque-app/stwo` and `bloque-app/s4`. The build fetches from
upstream for now. Pointing CI at the private mirrors needs a token that can read them, since a private repository
cannot be cloned anonymously.

## Build and test

```sh
./fetch-vos.sh <dir outside the tracked tree>             # read it before building it
VOS_DIR=<dir> ./build.sh <build dir>                       # STEP=guest builds only the agent and the program
VOS_DIR=<dir> ./test.sh <build dir>                        # the client's tests, against what build.sh made
```

`PORTABLE=1 STEP=node ./build.sh <dir>` builds only the node, without native codegen (VOS pins `target-cpu=native` for
x86_64 Linux; the build replaces it with the baseline and fails if rustc still ran with it), and CI runs the
host-contract suite on it. `VOS_DIR=<dir> ./host-check.sh <build dir>` is the production-host check.

A scored run is Linux x86_64 on kernel 6.7 or later with userfaultfd write-protect: the nodes run VOS's JIT, and the
tests fail closed on a machine that can't. `VOS_DEV=1 ./test.sh ...` runs the interpreter on any machine, a
development run whose result is not the task's. CI runs the real thing: `.github/workflows/stm-vos.yml`.

## Why two programs

The host-contract suite is typed on its own probe state machine and compares the states a host records with the
probe's, over shuffled inputs among others, where `Stm` rightly differs. So the suite scores the host's mechanics
(durability, exactly-once, swap, replay, import) on `elf/probe.elf`, the program built with `probe-machine`, and
`elf/stm.elf`, the program that ships, is scored by a test that compares each journal's states with a native fold of
`Stm`, and by the leader kill. The nodes are VOS's faults build, since the suite's stop after the program computed
holds every node's commits.

## How the program meets the contract

As `EXP-1`'s re-score program did: one handler step a submit (format, import and halted checks, record-id dedup on
byte-equal canonical JSON, the order's next sequence number, `open` or `transition`, the record or the refusal), which
VOS commits whole or not at all; reads are queries under VOS's 64 KiB reply cap; import is paged; the actor's
`format` field names the rows' layout, and a version of another format refuses every handler and query before it
reads a row. What changed: the state machine is `rutent_stm::Stm`, and the program's `next` and `next-format`
versions (`EXP-1`'s `R-06a` and `R-06b`) are not carried over, since nothing of this task upgrades a running program.
