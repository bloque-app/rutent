---
status: Accepted (Pablo, 2026-11-06, decision-form item `module-sandbox-boundary`); amended by EXP-2 (Pablo, 2026-11-07, items `exp2-adopt-format` and `licence-llvm-exception`)
decision_item: module-sandbox-boundary, exp2-adopt-format
supersedes: adopts suggestions/ADR-002-module-format-sandbox.md
---

# ADR-0003: Module format and sandbox boundary

## Context

`REQ-MDL-15/16` require the platform to accept at least one common, verifiable, portable binary format from any
compiling language, with the platform producing the machine code, and require the outside-world-facing part of a
module to be split from its sandboxed part. `suggestions/ADR-002` proposed treating the *process boundary* and
the *binary format* as separate choices, and trialling both Wasm and PVM, two languages each, scored against
`AC-20` (multi-language sandboxing).

`research/2026-11-02-provider-modules.md` found Wasmtime published 62 advisories in 2026 against 4 in 2025 (5
Critical, 2 needing no non-default setting) — independently re-confirmed by this session's earlier research fork
— but the same report is explicit that raw advisory counts across differently-scrutinized projects aren't a
safety ranking. PVM (`polkavm`, whose manifest states `MIT/Apache-2.0`) has only one guest language documented today, which on its own fails
`AC-20`'s two-language bar until a second is built or found. No report found a measurement of sandbox overhead
under a provider-call-dominated (network-wait) workload for either.

Per ADR-0002's ruling, this ADR does not need to pick a format now — only the process boundary, which is firm
regardless of format.

## Options considered

1. **Sandbox-only boundary (Wasm/PVM in-process), no process separation.** Rejected: a compiler or runtime bug
   that escapes the sandbox escapes everything in that process, including every other provider's module.
2. **One OS process per provider, sandboxed within it**, with the admitting compiler itself isolated (it parses
   hostile binary input from an untrusted publisher). Defense in depth: a sandbox escape is contained to one
   provider's process, not the whole platform.
3. **microVM/gVisor per provider.** Strongest isolation; highest operational cost (a full VM or gVisor sandbox
   per provider, for potentially many low-volume providers) with no report finding this cost justified at
   rutent's expected provider count.
4. **V8 isolates.** JS/Wasm only; even V8's own maintainers describe isolates as needing an outer sandbox, so this
   doesn't reduce what option 2 already requires.

## Decision

Option 2: one OS process per provider, regardless of eventual binary format, with the admitting compiler run in
its own isolation. The binary format itself (Wasm, PVM, or both) is a milestone-1 experiment per ADR-0002's
ruling — one stand-in module per provider category, two languages, scored against `AC-20` — not a STEP-2
commitment. A second format is only adopted if it earns its own upkeep cost, not by default.

## Consequences

- `ARCHITECTURE.md` §5's deployment view states the process boundary as firm; no later format experiment can
  remove it without a new ADR.
- `THR-85` (unknown sandbox escape) is **not newly mitigated** by this ADR alone — it narrows the blast radius of
  an escape (one provider's process, not the platform) but doesn't close the escape class itself. This is the
  residual risk `ARCHITECTURE.md` §6 states explicitly, as re-checked by `EXP-2` (see the amendment below).
- `REQ-MDL-13` (one provider's module change doesn't reload the service) follows directly from the process
  boundary: restarting one provider's process is independent of every other.
- PolkaVM ran Rust and C guests in `EXP-2`. It was dropped for v0 for `G-13`, not for its languages.

## Amendment: EXP-2's outcome (2026-11-07)

Accepted by the owner on 2026-11-07 (item `exp2-adopt-format`: "Wasm only for M2; drop PolkaVM for v0"), and
promoted from `EXP-2`'s draft (Narthex `REQ-EXP-5`). It decides the format the Decision above left to the
experiment, and keeps the process boundary. The experiment's record stays under
[`experiments/2026-11-07-module-format/`](../experiments/2026-11-07-module-format/).

### Outcome of EXP-2

From the [outcome page](../experiments/2026-11-07-module-format/outcome.md):

- **Wasm: adapt.**
- **PolkaVM: drop for v0.**

The numbers, each with its run:

- **The suite.** Both pass `M-01` to `M-16` in Rust and in C. Each ran four category reports, with 82 of 82
  exercised and `missing_languages` 0 (`EXP-2-wasm-20261107-2`, `EXP-2-pvm-20261107-1`).
- **The binding's obligations.** Both fail `B-01`, `B-14` and `B-19`. Wasm's fuel and memory limits, the format's
  own, stop a looping and a growing guest (`B-03`, `B-04`); the per-operation CPU bound is the binding's, so `B-03`
  is a "pass (binding)". PolkaVM's were not run. Both pass `B-11` on two AMD CPU models (`EXP-2-wasm-20261107-2`,
  `-3`; `EXP-2-pvm-20261107-1`, `-2`). Every "pass (binding)" is rutent's own code: `B-02`, `B-03`, `B-05`, `B-07`,
  `B-08`, `B-13` and `B-15` for Wasm; `B-02`, `B-07` and `B-08` for PolkaVM.
- **The gaps.**
  - PolkaVM's upstream says not to use it in production (`G-13`, `EXP-2-pvm-20261107-1`). Rutent can't close that
    row.
  - Wasm fails `G-05`, a bound on a provider process's whole memory, and `G-09`, Spectre settings left at the
    defaults (`EXP-2-wasm-20261107-2`). Both are rutent's configuration.
- **Measurements, not targets** (`B-06`, same runner type):
  - admission: Wasm's compile, spawn to exit, ~262 ms (`rs`) and 54.4 to 55.6 ms (`c`), against PolkaVM's
    admission round trip, ~6.4 ms;
  - start: a Wasm provider's process in 5.5 to 6.0 ms, against a PolkaVM first load into a running worker, ~206 ms.
- **Licence rows.**
  - Wasm brings 29 crates whose only expression is `Apache-2.0 WITH LLVM-exception`, the runtime and compiler
    themselves. At the run, 105 of its 136 host external packages had no line (`L-01`, `L-02`,
    `EXP-2-wasm-20261107-2`); all are recorded since.
  - PolkaVM's binding brings one such crate, `regalloc2` 0.9.3, through `polkavm-linker`, a build tool for the
    stand-ins. It patches out MPL-2.0 `option-ext` (`L-01`, `G-31`, `EXP-2-pvm-20261107-1`).

### Decision

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

**The licence ruling.** The owner confirmed on 2026-11-07 that the standing permission covers
`Apache-2.0 WITH LLVM-exception` (item `licence-llvm-exception`); `licences/RECORD` records it (`REQ-MOD-5`).

### Consequences

- **`F-013` builds on the binding** and moves it out of its experimental workspace into `CMP-PROV`. Its lines are
  already in `licences/RECORD`. Admission and its digest checks wait for the owner's
  click after a separate security review (`DEC-20`).
- **Before v0 reaches production**, these close (the outcome's [follow-up rows](../experiments/2026-11-07-module-format/outcome.md#follow-up-rows)):
  - `F-013`: limits on outbound calls, state and pending work (`B-14`); output checked against the schema's
    patterns and lengths (`B-19`); the pin read against 2026's advisories when it is set (`G-02`); a bound on a provider process's whole memory (`G-05`); re-derivation on a CPU
    of another vendor than the two AMD models shown (`B-11`, `G-04`, ADR-0014); Spectre settings chosen on purpose (`G-09`); fuzzed admission (`B-09`); and
    the role suite through the binding (`B-01`);
  - `F-014`: DNS rebinding and TLS at the egress layer (`B-17`, `B-18`);
  - `F-025`: the pin read again against the advisories (`G-02`); each jail layer probed alone; the seccomp allow-list
    pruned from a trace;
  - `F-030` and `F-033`: the operator obligation for Ubuntu 24.04's AppArmor restriction on unprivileged user
    namespaces.
- **`THR-85`** narrows and stays an accepted residual (`R72`). An escape from Wasmtime lands in one provider's jailed
  process, probed on the scoring platform. `ARCHITECTURE.md` §6 carries the re-checked row. For `THR-18`, re-derivation on separate hardware is shown for Wasm
  on two AMD models (`EXP-2-wasm-20261107-2`, `-3`).
- **`AC-20`** is claimed for Wasm in Rust and C (`M-02`, `M-03`, `M-16`, `EXP-2-wasm-20261107-2`). `PLAN.md` §6.2's
  risk 3 is retired: PolkaVM ran C too (`G-22`).
- **Only `x86_64-unknown-linux-gnu` was scored.** It is Wasmtime's Tier 1 target (`G-01`). Another deployment target
  needs its own scored run before any claim rests on it (`F-030`).
