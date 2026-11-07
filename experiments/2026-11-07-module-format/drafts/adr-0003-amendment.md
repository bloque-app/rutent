---
Status: experimental
amends: adr/0003-module-format-sandbox.md
experiment: EXP-2
decision_item: exp2-adopt-format (pending); licence-llvm-exception (pending)
---

# Draft: amendment to ADR-0003, the admitted module format

A draft of `EXP-2` (Narthex `REQ-EXP-3`). It is not accepted and not part of ADR-0003 until the owner answers item
`exp2-adopt-format` and promotion consolidates it (Narthex `REQ-EXP-5`). It decides the format ADR-0003 left to the
experiment. It keeps the process boundary ADR-0003 made firm (`DEC-7`). It is written for the item's recommended
option, Wasm only. [If the owner chooses PolkaVM or both](#if-the-owner-chooses-polkavm-or-both) says what changes.

## Context

ADR-0003 fixed one OS process per provider and the admitting compiler in its own isolation. It left the binary
format, Wasm, PVM or both, to a milestone-1 experiment, and kept a second format only if it earns its upkeep.
`EXP-2` bound each candidate to the module-boundary suite (`T-003-03`) against the module-interface contract frozen at
`C1` (1.0.0). Each binding had four stand-ins, reserve, exchange, ramp and verifier, in Rust and in C. It scored the
obligations `DEC-7`, ADR-0003, ADR-0004 and ADR-0014 put on a binding, each candidate's own gaps, and the licence
rows ([scoring sheet](../scoring.md)). The scored runs were on GitHub's `ubuntu-24.04`, `x86_64-unknown-linux-gnu`,
an AMD EPYC 7763:

- `EXP-2-wasm-20261107-1`: components on Wasmtime 48.0.8, tag `v48.0.8`, commit `7b6c29e`, through `T-008-02`'s
  binding;
- `EXP-2-pvm-20261107-1`: PolkaVM 0.38.0, tag `v0.38.0`, commit `471a5b6`, its compiler and Linux sandbox, through
  `T-008-03`'s binding; and `EXP-2-pvm-20261107-2`, the same commit on an AMD EPYC 9V74, for re-derivation.

## Outcome of EXP-2

From the [outcome page](../outcome.md):

- **Wasm: adapt.**
- **PolkaVM: drop for v0.**

The numbers, each with its run:

- **The suite.** Both pass `M-01` to `M-16` in Rust and in C. Each ran four category reports, with 82 of 82
  exercised and `missing_languages` 0 (`EXP-2-wasm-20261107-1`, `EXP-2-pvm-20261107-1`).
- **The binding's obligations.** Both fail `B-01`, `B-14` and `B-19`. Wasm passes `B-03` and `B-04` by the format's
  own fuel and memory limits; PolkaVM's were not run. PolkaVM passes `B-11` on two CPU models
  (`EXP-2-pvm-20261107-1`, `-2`); Wasm's was not run. Every "pass (binding)" is rutent's own code: `B-02`, `B-05`,
  `B-07`, `B-08`, `B-13` and `B-15` for Wasm; `B-02`, `B-07` and `B-08` for PolkaVM.
- **The gaps.**
  - PolkaVM's upstream says not to use it in production (`G-13`, `EXP-2-pvm-20261107-1`). Rutent can't close that
    row.
  - Wasm fails `G-05`, a bound on a provider process's whole memory, and `G-09`, Spectre settings left at the
    defaults (`EXP-2-wasm-20261107-1`). Both are rutent's configuration.
- **Measurements, not targets** (`B-06`, same runner type):
  - admission: Wasm's compile, spawn to exit, ~262 ms (`rs`) and 54.4 to 55.6 ms (`c`), against PolkaVM's
    admission round trip, ~6.4 ms;
  - start: a Wasm provider's process in 5.5 to 6.0 ms, against a PolkaVM first load into a running worker, ~206 ms.
- **Licence rows.**
  - Wasm brings 29 crates whose only expression is `Apache-2.0 WITH LLVM-exception`, the runtime and compiler
    themselves. 105 of its 136 host external packages have no line (`L-01`, `L-02`, `EXP-2-wasm-20261107-1`).
  - PolkaVM's binding brings one such crate, `regalloc2` 0.9.3, through `polkavm-linker`, a build tool for the
    stand-ins. It patches out MPL-2.0 `option-ext` (`L-01`, `G-31`, `EXP-2-pvm-20261107-1`).

## Decision

**Wasm components are the one admitted format** (`REQ-MDL-15`). Wasmtime 48.0.8, an LTS release, is the runtime,
and `M2`'s module admission and runtime (`F-013`) build on it.

- **The binding's design** is kept from `T-008-02`:
  - components, with the binding's WIT world, outside the frozen contract (`DEC-4`);
  - the format named `wasm-component` in release assertions;
  - an explicit compile target with no CPU feature inferred from the host (`G-03`);
  - no WASI interface, and only the WebAssembly features the modules need (`G-11`);
  - fuel for CPU, and a memory limit per part;
  - machine code made only by the platform's own compiler, and loaded only after its digest matches the artefact's
    (`B-15`, `G-08`).
- **PolkaVM is not adopted** at 0.38.0. Its binding and run notes stay as `EXP-2`'s record, and its code does not
  merge. The module interface stays format-neutral (`DEC-4`), so a later experiment may score a PolkaVM release that
  its upstream holds fit for production, as its own `EXP-n`.
- **No second format.** Wasm alone claims `AC-20`'s two languages, Rust and C. PolkaVM would bring no language Wasm
  lacks. It would add a second binding, jail, `DEC-20` review, licence closure and pre-1.0 pin, so it doesn't earn
  its upkeep.
- **The reasons.** The suite ties. `G-13` makes PolkaVM unfit for a v0 in production with real customer value
  (`DEC-19`), and only its upstream could change that. Wasm's open gaps close in rutent's own code and
  configuration, and its pin is supported until 2028-08-20 (`G-12`).
- **The boundary is unchanged.** One OS process per provider, and the admitting compiler in its own isolation,
  whatever the format. Removing either still needs a new ADR.

**The licence ruling first.** The decision takes effect in `main` only once the owner's decision on
`Apache-2.0 WITH LLVM-exception` is in `licences/RECORD` (item `licence-llvm-exception`, `REQ-MOD-5`). Until then the
licence check keeps Wasmtime off `main`. If the owner declines it, Wasmtime can't be adopted at this pin, and this
amendment is withdrawn.

## Consequences

- **`F-013` builds on the binding** and moves it out of its experimental workspace into `CMP-PROV`. Its lines enter
  `licences/RECORD` as Wasmtime enters the root `Cargo.lock`. Admission and its digest checks wait for the owner's
  click after a separate security review (`DEC-20`).
- **Before v0 reaches production**, these close (the outcome's [follow-up rows](../outcome.md#follow-up-rows)):
  - `F-013`: limits on outbound calls, state and pending work (`B-14`); output checked against the schema's
    patterns and lengths (`B-19`); the pin read against 2026's advisories when it is set (`G-02`); a bound on a provider process's whole memory (`G-05`); re-derivation on separate
    hardware (`B-11`, `G-04`, ADR-0014); Spectre settings chosen on purpose (`G-09`); fuzzed admission (`B-09`); and
    the role suite through the binding (`B-01`);
  - `F-014`: DNS rebinding and TLS at the egress layer (`B-17`, `B-18`);
  - `F-025`: the pin read again against the advisories (`G-02`); each jail layer probed alone; the seccomp allow-list
    pruned from a trace;
  - `F-030` and `F-033`: the operator obligation for Ubuntu 24.04's AppArmor restriction on unprivileged user
    namespaces.
- **`THR-85`** narrows and stays an accepted residual (`R72`). An escape from Wasmtime lands in one provider's jailed
  process, probed on the scoring platform. The re-checked row of `ARCHITECTURE.md` §6 is
  [`thr-85-recheck.md`](thr-85-recheck.md). For `THR-18`, re-derivation on separate hardware is still to be shown for
  Wasm.
- **`AC-20`** is claimed for Wasm in Rust and C (`M-02`, `M-03`, `M-16`, `EXP-2-wasm-20261107-1`). `PLAN.md` §6.2's
  risk 3 is retired: PolkaVM ran C too (`G-22`).
- **Only `x86_64-unknown-linux-gnu` was scored.** It is Wasmtime's Tier 1 target (`G-01`). Another deployment target
  needs its own scored run before any claim rests on it (`F-030`).

## Points reconciled

- **PolkaVM's licence.** ADR-0003 says PolkaVM is Apache-2.0. At `v0.38.0` its workspace manifest states
  `MIT/Apache-2.0`, which every crate takes, and its README offers either licence at the user's option. GitHub's API
  reports the repository as `Apache-2.0` (README, Licence). The amendment reads: "PolkaVM (`polkavm`, whose manifest
  states `MIT/Apache-2.0`)". This states the source; it is no licence decision.
- **PVM's guest languages.** ADR-0003 says PVM has one documented guest language, and its last consequence makes
  that a condition for choosing PVM. At `v0.38.0` the tree carries a C guest header with no C program or test.
  `T-008-03` built C stand-ins with Ubuntu's clang 18.1.3, the header and `polkavm-linker`, and they passed the whole
  suite (`G-22`, `EXP-2-pvm-20261107-1`). The amendment replaces that consequence with: "PolkaVM ran Rust and C
  guests in `EXP-2`. It was dropped for v0 for `G-13`, not for its languages."

## If the owner chooses PolkaVM or both

**PolkaVM only.** The Decision is reversed:

- `F-013` builds on `T-008-03`'s binding, with PolkaVM's compiler and Linux sandbox, on x86_64 Linux only (`G-14`);
- `G-13` is recorded as an accepted risk for v0's production, the owner's;
- before production, `F-013` also runs `B-03` to `B-05`, `B-09`, `B-13`, `B-15`, `G-20`, `G-21` and `G-24` to
  `G-26`;
- the jail is strengthened where it is weaker than Wasm's: the worker keeps every capability but `CAP_SYS_ADMIN`,
  and the admitting compiler drops none and has no Landlock;
- `regalloc2` stays off the platform's closure if `polkavm-linker` stays a publisher-side tool;
- the `THR-85` row takes the variant in [`thr-85-recheck.md`](thr-85-recheck.md#if-the-owner-chooses-polkavm).

**Both.** Each binding is carried into `CMP-PROV`, each with its own closure, jail and `DEC-20` review. Each row of
the outcome's follow-up table, and each of PolkaVM's above, is closed for both formats before production. The
amendment then records what earns the second format its upkeep, which `EXP-2` did not find.
