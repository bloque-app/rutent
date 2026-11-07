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
