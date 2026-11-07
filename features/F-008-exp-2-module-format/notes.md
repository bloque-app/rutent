# F-008 notes

Readings taken where `SPEC.md` or `PLAN.md` is silent or ambiguous, recorded per Narthex `REQ-SKL-24`. The
experiment's own questions and defaults are in `experiments/2026-11-07-module-format/decisions.json`; the record's
readings are in its README, "Readings taken in opening it".

## T-008-01

- **Drafted before its dependency landed, then reconciled.** `T-008-01` depends on `T-003-03`, the module-boundary
  suite. The record was first drafted with the suite's cell names assumed from `AC-19` to `AC-21` and the plan;
  once the suite was on `m1` (`a443665`), the sheet's suite rows were rewritten from its 15 checks and 41 fixtures
  and controls (`M-01` to `M-16`), and the record rebased onto it.
- **Stand-ins per format.** `DEC-7` asks for one stand-in per provider category in two languages, and `T-008-03`
  for "the same stand-ins". Read as per format: four categories, two languages, two formats.
- **The restart test.** `restarting one provider's process leaves the others running` (`REQ-MDL-13`) is in
  `T-008-02`'s row, not `T-003-03`'s, but the suite carries it under `ProvidersAreIsolated` (`M-15`). Whether the
  process it restarts is a real OS process is scored by `B-07`.
- **A plan point, not raised as an item.** `PLAN.md` §6.2, risk 3, reads PVM as having one guest language, after
  ADR-0003. At PolkaVM `v0.38.0` the tree also carries a C guest header, with no C guest program or test. This is
  what `T-008-03` is for; the sheet scores it as `G-22`, and the ADR-0003 amendment draft lists it to reconcile.

## T-008-03

- **A development run after `C1`.** The worktree is cut at `6cb6a67`, the `C1` freeze itself, so the run is
  against the frozen module-interface 1.0.0, which the README's Runs section would make a scored run. The brief asks
  for a `dev` id and the `pre-freeze` marking, and the run is off the scoring platform (`aarch64-apple-darwin`,
  interpreted; item `exp2-run-platform`), so it takes `EXP-2-pvm-dev-20261107-1` and marks its cells `pre-freeze`
  in the legend's sense: a scored run on `x86_64-unknown-linux-gnu` replaces every one, and no outcome rests on it.
- **`option-ext`, MPL-2.0.** `polkavm-linker` 0.38.0 brings it in through `dirs` 5.0.1 and `dirs-sys` 0.4.1. By the
  orchestrator's ruling (2026-11-07), the binding's workspace patches `dirs` with its own MIT stub and takes no
  MPL-2.0 code in; the scoring sheet records it as `G-31`, and the licence question stays the owner's, for the
  adoption item at `T-008-04`.
- **The sandboxed part's one channel.** The contract lets the sandboxed part ask the outside part to perform a
  declared outside operation, "and nothing else". Read strictly: its only host functions are the operation in, the
  outside request and the result out. A suite step that has the sandboxed part read state is an outside request the
  outside part serves, and its state read is recorded as the outside part's; one that has it look up a secret is an
  outside request naming a secret, which the contract's shape forbids, so the platform refuses it before the outside
  part, and records the lookup with no outcome.
- **The four categories.** The suite's modules name no category and all handle withdrawals, so each category's
  stand-in is the same step interpreter compiled with its category, which checks at `start` that the release was
  built for it. The suite runs once per category; item `exp2-pvm-placement`.
- **Refusal reasons at admission.** `AdmissionRefusal` has no reason for an import a part may not have; the binding
  refuses such a part as `UnreadableBinary`, "not a module the binding can read", and the run note gives the
  admitting compiler's own reason.
- **After review of `d272c15`** (`EXP-2-pvm-dev-20261107-2` supersedes `-1`):
  - **Isolation.** The worker parses and compiles hostile blobs too, so it starts isolated like the admitting
    compiler, and compiles only inside that isolation. On macOS both run under a `sandbox-exec` profile that
    denies by default. Beyond the program and the system's library paths, it allows only reading the root
    directory and the two page-size `sysctl`s, without which the program aborts before `main`. The Linux confinement (`jail.rs`) is written for the scored run and has not run.
  - **Host calls are the platform's.** The outside part's secret lookups, outbound calls and cross-module calls
    run under the transaction and operation of the outside request in flight, which the platform checked and
    recorded from the provider's pipe. A guest naming another operation is refused: a lookup as
    `undeclared_secret`, an outbound call as `undeclared_operation` at its first hop. Both are recorded under the
    operation in flight. `undeclared_operation` is the closest reason the contract has for "not the operation
    asked".
  - **A suite follow-up, not a change to the frozen suite.** The module-boundary suite's specs give each step one
    operation, which both parts use, and every declaration it writes has one outside operation. So no probe has an
    outside part act under an operation other than the request in flight, and a binding that took the guest's word
    passes it. A probe would need a declaration with two operations and a module whose outside part names the
    other one. The binding's own test does this (`binding/tests/boundary_suite.rs`); `T-003-03`'s suite could
    carry it at its next version.
- **After review of `37b9a97`** (`EXP-2-pvm-dev-20261107-3` supersedes `-2`):
  - **What a profile can't hold.** A deny-by-default `sandbox-exec` profile still let a child list and read other
    processes' information and read IOKit properties, until both were denied explicitly. It never mediates a read
    of a descriptor the child already holds, so every child now closes descriptors from 3 up before it execs. It
    couldn't be made to refuse a System V object created with `IPC_PRIVATE`: no profile rule tried refused it.
    That is recorded in the run note, not claimed closed.
  - **A child's standard error is evidence.** It is a pipe of the child's own. The platform keeps the first 64 KiB
    of each child's, labelled by provider or admission, and none of it reaches the platform's own stream.
  - **A version names one release.** A release of a version already admitted or waiting is refused. A waiting
    release's binaries are kept apart until its acceptance, so it can't replace what a restart brings back.
    `AdmissionRefusal` has no reason for "already admitted", so the binding refuses it as `UnreadableBinary` and
    records why.
  - **The C guest header is vendored** at PolkaVM's pinned commit, with its licence files and its upstream blob id
    and SHA-256 (`pvm-binding/guests/c/vendor/polkavm/UPSTREAM`), so the stand-ins build offline.
- **The scored run, on GitHub's `ubuntu-24.04` x86_64 runners** (`EXP-2-pvm-20261107-1`, workflow
  `exp2-pvm.yml` on branch `exp2/pvm-linux`):
  - **A runner setting, not the binding.** Ubuntu 24.04's AppArmor restricts unprivileged user namespaces, which
    both the binding's isolation and PolkaVM's sandbox need. The workflow lifts the restriction for the job only,
    and the run note records it as an operator obligation.
  - **The worker's capabilities.** Dropping every capability broke PolkaVM's sandbox on the runner, and so did
    keeping only `CAP_SETUID` and `CAP_SETGID`. The worker therefore locks its mounts in a further user namespace
    and drops `CAP_SYS_ADMIN` only. That meets the review's aim, that it can't unmount what hides the user's
    directories, which a probe checks. The capability set wasn't narrowed further.
  - **`B-11` across two workflow runs.** A runner can't be chosen by CPU model. Two runs of the same commit landed
    on two models (AMD EPYC 9V74 and 7763), built the same blobs and compiled them to the same machine code, which
    the run note reads as `B-11`'s two machines. Each run's own second runner had the same CPU model as its
    first.

## T-008-02

- **A development run after the freeze.** The README's Runs section defines a `dev` id for a run before `C1`.
  `EXP-2-wasm-dev-20261107-1` ran against the frozen 1.0.0, but on arm64 macOS, not on the scoring platform
  (`exp2-run-platform`). It takes the dev id, and its cells take the legend's `pre-freeze` marker, which here means
  "not on the scoring platform". The legend is left as it is; item `exp2-dev-run-off-platform` records the reading,
  for `T-008-04` to reword if it wants.
- **One stand-in per category, and the suite's modules.** The suite builds its own modules from `ModuleSpec`s, about
  forty per language, all handling withdrawals. Read as: each category's stand-in, in each language, is the compiled
  code every one of those modules runs, with the spec written into a program slot (`exp2-wasm-standins`); the suite
  runs once per category, and a row passes only if it passes in all four. Each stand-in also answers its own role's
  kind, which is what the binding's `B-01` test checks; the role suite through the binding is `B-01`'s open gap.
- **The sandboxed part's attempts.** The suite's steps have the sandboxed part try a secret and state, and count
  only an observed refusal. The contract gives it no route to either, so the binding gives it `denied`, which
  records the attempt and reaches nothing (`exp2-wasm-sandboxed-surface`).
- **Two flaws the PVM binding's review found, checked here.** Every secret lookup, outbound call, state read and
  cross-module call is judged against the outside request in flight, from the platform's own state, and a record
  names that operation, never the one the guest gave (`tests/egress.rs`, a hostile stand-in step and its control).
  A provider's process checks both parts' machine-code digests against the artefact's before it loads either. The
  macOS isolation profile denies by default, and is probed with a read in `$HOME`, a signal to another process, a
  network connect, a file write and a process start; `B-07` and `B-08` are scored on those probes only, on macOS.
  The Linux layer does not meet that bar: its seccomp filter is a deny list and nothing restricts file reads, and
  the probe test, which asserts the home-file read everywhere, fails there until it is rebuilt.
- **Not done here.** No Linux run: the Linux isolation code is type-checked only, and `run.sh` is yet to be run on
  x86_64 Linux for the scored run. `B-11` needs two machines. The outcome page and the adoption item are
  `T-008-04`'s.
- **After the review of `7af940e`.** Run 2, `EXP-2-wasm-dev-20261107-2`, replaces run 1 in every cell. Its commit
  closes inherited file descriptors before `exec` (a descriptor the platform held open reached a child, where the
  profile's file-read denial didn't stop it), turns every WebAssembly feature off but the five the stand-ins need,
  marks `B-07`, `B-08`, `B-13` and `G-07` macOS only, and lets `denied` name no operation. From the PVM binding's
  review it adds explicit `process-info*` and `iokit-get-properties` denials with probes, and refuses re-admitting a
  version already admitted, with a test that a pending release leaves the running one in force across a restart.
  Run 1's note is kept, marked superseded.

## Audit corrections to the EXP-2 scores (T-008-02, T-008-03)

Applied from an independent audit of the two scored runs, to the sheet, the record's README and the run notes. No code
and no binding source changed.

- **Run ids on the platform.** The README's Runs section now says any run on the scoring platform after `C1` takes a
  scored id and its own note. Actions run 42359164555, the push run that re-derived PolkaVM's machine code on an EPYC
  9V74, is `EXP-2-pvm-20261107-2`, with a note written from its logs and artifacts; `B-11` and `G-19` cite it with `-1`.
  Two readings: a workflow's own label (`EXP-2-pvm-ci-7`, `EXP-2-wasm-ci-3`) is not an id; and the workflow runs that
  only developed a binding, citing no cell and carrying no scored claim, are listed in one line each in the scored
  run's note and take no id of their own.
- **Scores.** pvm `B-01`, `B-14` and `B-19` are `fail`; `B-02` is `pass (binding)` for both formats; wasm `B-15` is
  `pass (binding)`; `G-10` and `G-22` share one criterion, C stand-ins written for this task with upstream tooling,
  and are `pass`.
- **`G-14`.** Its criterion is the compiler backend and the non-experimental Linux sandbox existing, and the pvm
  result is `pass`. The sentence that `G-14`, `G-15`, `G-16` and `G-30` are settings inside a project upstream calls
  unfit for production is in that result cell, as the other notes on a result are in theirs.
- **`L-01` and `L-04` for pvm.** `regalloc2` 0.9.3 is licensed only as `Apache-2.0 WITH LLVM-exception` and has no
  line in `licences/RECORD`. Of the binding's 41 external packages, 24 have a line at the same version and 17 have
  none; of the guests' 9, 4 have one and 5 have none. The counts exclude path packages (the `dirs` stub among them,
  which reuses upstream's version number 5.0.1); the wasm column's counts exclude them too. Checked by comparing each
  `Cargo.lock` with `licences/RECORD`.
- **`B-06`.** Wasm's spawn-to-exit compile (about 262 ms) is set beside PolkaVM's admission round trip (6.4 ms). The
  in-process 0.40 ms is kept but marked not comparable. PolkaVM's resident memory and machine-code size are not
  measured: the scored-run artifacts hold only the machine code's digests.
- **`B-08` for pvm, in the note only.** The `/proc` read was refused by `seccomp`, not the empty root; the layers were
  probed together; the admitting compiler drops no capabilities and has no Landlock.
- **Tree at `dfeb1c2`.** The Wasm binding's tree there differs from the scored run's (`eab957c`) by one comment.


## T-008-04

- **"Adapt", not "adopt", for Wasm.** As `T-007-04` read it for `EXP-1`'s store: the format is taken for `M2`, and
  the gaps the outcome lists close before v0 reaches production, each owned by a plan feature.
- **Two items, not one.** The recommended option needs a licence ruling first: Wasmtime's runtime and compiler are 29
  crates whose only expression is `Apache-2.0 WITH LLVM-exception`. `licences/RECORD`'s standing permission names
  Apache-2.0 but not the exception. The orchestrator read it as covered; the owner hasn't confirmed it. So the
  question is its own reserved item, `licence-llvm-exception`, with no recommendation, beside `exp2-adopt-format`.
- **`regalloc2` and PolkaVM.** PolkaVM's only crate with that expression enters through `polkavm-linker`, which the
  binding's manifest says is for building stand-ins, never on admission's path. Read as: a PolkaVM platform need
  not carry it. Nothing was built to show it.
- **macOS evidence.** The macOS probes come from development runs only, and the re-check of `THR-85` rests no
  status on them.
