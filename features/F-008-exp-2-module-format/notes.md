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
