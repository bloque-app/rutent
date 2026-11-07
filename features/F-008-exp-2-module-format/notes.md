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
