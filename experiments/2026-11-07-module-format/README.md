---
id: EXP-2
start date: 2026-11-07
status: open
serves: DEC-7, ADR-0003, REQ-MDL-15, REQ-MDL-18, REQ-MDL-13, AC-20, THR-85
opened by: T-008-01
---

# EXP-2: module format

An experiment under Narthex's experiment mechanism (Narthex `REQ-EXP-1` to `REQ-EXP-7`), as `PLAN.md` `M1` and
`DEC-7` require. It binds each candidate format to the module-boundary suite (`T-003-03`, `DEC-5`), runs one stand-in
module per provider category in two languages, and scores each format and language, without loading the owner with
every question it raises. Only its definitive shape reaches the owner's form, as one adoption item (Narthex
`REQ-EXP-7`).

This page holds the record's fields in the order Narthex `REQ-EXP-1` names them: id, start date, question, scope,
decision log, draft changes, outcome and promotion record.

| Field | Where |
|---|---|
| Id | `EXP-2`, in the front matter and [below](#id) |
| Start date | 2026-11-07, in the front matter and [below](#start-date) |
| Question | [Question](#question) |
| Scope | [Scope](#scope), with the [candidates](#candidates) and the [scoring sheet](scoring.md) |
| Decision log | [`decisions.json`](decisions.json) |
| Draft changes | [`drafts/`](drafts/) |
| Outcome | [`outcome.md`](outcome.md), a placeholder until `T-008-04` |
| Promotion record | [Promotion record](#promotion-record) |

## Id

`EXP-2` (`PLAN.md` §1, Identifiers).

## Start date

2026-11-07, the day `T-008-01` opened this directory.

## Question

Which portable binary format, or formats, does the platform admit (`REQ-MDL-15`): Wasm, PVM, or both?

Whatever the answer, every provider's module runs in its own OS process and the admitting compiler runs in its own
isolation (ADR-0003, `DEC-7`); the format sits inside that boundary and does not replace it. A second format is kept
only if it earns its upkeep (`DEC-7`, ADR-0003). The answer is one of: Wasm only, PVM only, or both, with `AC-20`'s
two languages claimed for each kept format or the gap recorded (`PLAN.md` §6.2, risk 3). The experiment answers it
with the numbers of the [scoring sheet](scoring.md), each naming a run, and its outcome feeds the `THR-85` re-check
of `ARCHITECTURE.md` §6 (`T-008-04`).

## Scope

Scope is what the experiment may change (Narthex `REQ-EXP-1`).

### May change

| What | Where | Rule |
|---|---|---|
| This record | `experiments/2026-11-07-module-format/` | Merges at any time |
| The scoring sheet's results and run ids | [`scoring.md`](scoring.md) | One run id per filled cell |
| Measurement notes, one per run | `experiments/2026-11-07-module-format/runs/`, created by the first run | Merge at any time (Narthex `REQ-EXP-3`) |
| The experiment's own questions and defaults | [`decisions.json`](decisions.json) | Never the owner's inbox (Narthex `REQ-EXP-2`) |
| The ADR-0003 amendment, the plan decision and the `THR-85` re-check, as drafts | [`drafts/`](drafts/) | `Status: experimental` and `proposed` until promoted (Narthex `REQ-EXP-3`) |
| A Wasm binding of the suite, with its stand-ins | `T-008-02`'s code | Behind a flag (Narthex `REQ-EXP-3`); on `main` only once every package it brings has a line in `licences/RECORD` with a decision (`REQ-MOD-5`) |
| A PVM binding of the suite, with its stand-ins | `T-008-03`'s code | As the Wasm binding |

Where each binding sits is its task's design. When a task places one, it adds the placement as an item to
[`decisions.json`](decisions.json), as `T-007-02` did for `EXP-1`'s store.

### May not change

| What | Why |
|---|---|
| `contracts/`, the module-interface contract included | Frozen at `C1`; it is format-neutral and names no format (`DEC-4`). A binding that needs a field changed follows `DEC-4` and waits for the owner's click (`DEC-20`, `PLAN.md` §6.2, risk 1) |
| `conformance/`, the module-boundary suite | `T-003-03`'s; the experiment binds and runs the suite, it does not edit it |
| `SPEC.md` and `PLAN.md` | Only promotion changes them (Narthex `REQ-EXP-5`) |
| `adr/0003-module-format-sandbox.md` itself | The amendment stays a draft here until promoted (Narthex `REQ-EXP-3`) |
| The process boundary: one OS process per provider, the admitting compiler in its own isolation | Firm whatever the format (ADR-0003); removing it needs a new ADR |
| Module admission and signature checking, as merged code | A `DEC-20` path: a binding's admitting compiler and digest checks wait for the owner's click after a separate security review, whatever the scores say |
| The licence decisions in `licences/RECORD` | The owner's, case by case (`REQ-MOD-5`); the licence check keeps an undecided dependency off `main` |
| `ARCHITECTURE.md`'s `THR-85` row | `T-008-04` re-checks it from the outcome; until promotion the re-check is a draft here |
| The durable-store runtime | `EXP-1` (`DEC-6`). VOS's own PVM crates appear here only as a PVM implementation question, item `exp2-pvm-implementation` |
| Throughput or latency targets | None is claimed before `M4` (`PLAN.md` §6.2, risk 9). Sizes and timings may be recorded as measurements, never as a target |

## Candidates

| Code | Candidate | Task | Pinned at | Default by |
|---|---|---|---|---|
| `ref` | The module-boundary suite's in-process reference double, `conformance::boundary::reference::ReferenceDouble` (feature `test-support`), with no format and two nominal languages: the baseline, not a candidate | `T-003-03` | The suite's commit, named in each run note | |
| `wasm` | WebAssembly components on Wasmtime 48.0.8, the current LTS release, tag `v48.0.8`, commit `7b6c29ec3e9a49c97da231665cbd8eead9ccbda0` | `T-008-02` | 48.0.8 | items `exp2-wasm-runtime`, `exp2-wasm-shape` |
| `pvm` | PolkaVM 0.38.0, tag `v0.38.0`, commit `471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d` | `T-008-03` | 0.38.0 | item `exp2-pvm-implementation` |

The languages, scored apart for each format, are `rs` (Rust) and `c` (C), by default (item `exp2-languages`). Each
format has one stand-in module per provider category (reserve, exchange, ramp, verifier) in each language: sixteen
stand-ins across both formats (`DEC-7`). Scored runs are made on `x86_64-unknown-linux-gnu` (item
`exp2-run-platform`).

### What the candidates are, from primary sources

Read on 2026-11-07. Repository files were read at the pinned tag through GitHub's API, as raw text; the links below
are permalinks at the tag's commit. Where a fact was read through a fetch tool that returns a model-written digest
of a page, it says *(digest)*. Nothing was cloned, downloaded to build or run: every fact here is a claim for a run
to check, not a result.

#### Licence (fact only)

| Candidate | What the source states | Source |
|---|---|---|
| Wasmtime 48.0.8 | `license = "Apache-2.0 WITH LLVM-exception"` in the manifest of the `wasmtime` crate, the one a binding depends on, and the same in those of `cranelift-codegen`, `wasmtime-internal-cranelift` and `wasmtime-wasi`, read as a sample of what it pulls in; the workspace sets no shared licence. The `LICENSE` file carries the Apache 2.0 text and, from line 205, the LLVM exceptions | [`crates/wasmtime/Cargo.toml:7`][wt-cargo], [`cranelift/codegen/Cargo.toml:6`][wt-cargo-codegen], [`LICENSE:205`][wt-licence] |
| PolkaVM 0.38.0 | `license = "MIT/Apache-2.0"` in the workspace manifest, which every crate takes (`license.workspace = true`); the README says "either of" Apache-2.0 or MIT "at your option", with `LICENSE-APACHE` and `LICENSE-MIT`; GitHub's API reports the repository as `Apache-2.0`. ADR-0003 calls it Apache-2.0 | [`Cargo.toml:28`][pvm-cargo], [`crates/polkavm/Cargo.toml:5`][pvm-crate-cargo], [`README.md:36-43`][pvm-readme-licence] |

Whether these fall under the standing permission `licence-permissive-standing` is not recorded here: it is the
owner's, case by case (`REQ-MOD-5`), and the [licence rows](scoring.md#4-the-licence-question) score what it is about.

#### Sandboxing model

- **Wasmtime.** The guest runs in the host process. Linear-memory accesses are bounds-checked, the call stack is out
  of the guest's reach, control flow goes only to type-checked destinations, and all outside interaction is through
  imports and exports ([`docs/security.md:25-45`][wt-security]). Defence in depth: a 2 GB guard region before each
  linear memory ([`:54`][wt-security-guard]). Spectre mitigation is "ongoing research", and `csdb` is off by default
  on aarch64 ([`:123-140`][wt-security-spectre]). The page says nothing of a process boundary or an OS-level second
  layer: those are the binding's (ADR-0003).
- **PolkaVM.** Two backends, a compiler and an interpreter ([`config.rs:10-13`][pvm-config-backend]), and two
  sandboxes, `linux` and `generic` ([`:55-58`][pvm-config-sandbox]). The compiler, and with it either sandbox, is
  built only on x86_64, on Linux or, with the `generic-sandbox` feature, macOS or FreeBSD
  ([`lib.rs:11-19`][pvm-lib-gate]); the `generic` sandbox "is EXPERIMENTAL and is not meant for production use"
  ([`crates/polkavm/Cargo.toml:46-49`][pvm-crate-features]). Elsewhere the VM runs interpreted, in the host process.
  The Linux sandbox runs guest code in worker processes cloned into new cgroup, IPC, network, mount, PID, user and
  UTS namespaces ([`sandbox/linux.rs:324-330`][pvm-linux-flags]), needing unprivileged user namespaces
  ([`:441`][pvm-linux-userns]); it clears ambient capabilities, sets `no_new_privs` and sets resource limits, among
  them no new files, one process and no writes ([`:1200-1226`][pvm-linux-rlimits]); the worker, a prebuilt zygote
  binary included at build time ([`:799`][pvm-linux-zygote]), sets no open files, calls `pivot_root` and installs a
  seccomp filter ([`polkavm-zygote/src/main.rs:718-801`][pvm-zygote]). The README calls the project "a very heavy
  work-in-progress! Do not use it in production!" and lists its design goals under "not all of these are currently
  true" ([`README.md:5`, `:11`][pvm-readme]).

#### Metering and limits

- **Wasmtime.** CPU by fuel, which is deterministic, or by epochs, which are not and cost "around a 10% slowdown"
  ([`docs/examples-interrupting-wasm.md:25-51`][wt-interrupt]). Memory, tables and instance counts by
  `ResourceLimiter`, which does not limit Wasmtime's own structures, the embedder's allocations, or CPU
  ([`runtime/limits.rs:14-25`][wt-limits]).
- **PolkaVM.** Gas, synchronous or asynchronous; asynchronous may run "slightly longer" but the total is "strictly
  enforced and deterministic" ([`config.rs:482-497`][pvm-config-gas]). Dynamic paging, off by default, needs Linux 6.7
  and unprivileged `userfaultfd` ([`:377-384`][pvm-config-paging]). Settings that must never be on in production:
  cross-checking, experimental settings that "can introduce unsafety, break determinism", and sandboxing turned off
  ([`:317-345`, `:429-436`][pvm-config-unsafe]).

#### Deterministic compilation (ADR-0014's re-derivation)

ADR-0014 keeps every admitted binary with the compiler version that made its machine code, and re-derives it on
separate hardware; the admission artefact records both (`build.platform_version`, `build.machine_code`).

- **Wasmtime.** A test compiles small modules twice with one engine on one host and requires the same bytes
  ([`tests/all/module.rs:98`][wt-determinism-test]). That says nothing about another machine: by default the
  compiler infers CPU features from the host ([`config.rs:2930-2940`][wt-config-host]); setting `Config::target`
  turns inference off, and flags are then set explicitly ([`:390-404`][wt-config-target]). Deterministic execution
  needs `cranelift_nan_canonicalization`, `relaxed_simd_deterministic` and fuel rather than epochs
  ([`docs/examples-deterministic-wasm-execution.md:28-80`][wt-det-exec]).
- **PolkaVM.** Not determined. The README's "Fully deterministic" goal is about execution, and sits under "not all
  of these are currently true" ([`README.md:11`, `:19`][pvm-readme]). No test was found that compares machine-code
  bytes across two compilations; the test that compares `machine_code()` compares pointers, which is cache identity
  ([`tests.rs:7573-7578`][pvm-tests-cache]). The compiler requires BMI2 on AMD64 ([`api.rs:448-451`][pvm-api-bmi2]).
  `Module::machine_code()` is "mostly only useful for debugging", and returns `None` under the interpreter
  ([`api.rs:870-888`][pvm-api-machine-code]).

Both are scored by runs: [`B-11`](scoring.md#22-per-format), `G-03`, `G-04` and `G-19`.

#### Guest languages (`AC-20` needs two per format)

- **Wasmtime.** The component model guide lists C/C++, C#, Go, JavaScript, Python, Rust and MoonBit, and says "the
  level of support varies across languages" ([`language-support.md:4`, `:26-40`][cm-languages]); `wit-bindgen` has a
  C generator ([`crates/c`][wit-bindgen-c], release `v0.63.0`).
- **PolkaVM.** Rust: the tree's guest programs are Rust, and the linker ships custom RISC-V targets,
  `riscv32emac-unknown-none-polkavm` and `riscv64emac-unknown-none-polkavm`
  ([`crates/polkavm-linker/targets/`][pvm-targets]). C: a guest header exists, [`capi/polkavm_guest.h`][pvm-capi],
  with no C guest program or test found in the tree. Solidity reaches PolkaVM through `revive`, which targets
  `pallet-revive`'s host interface ([`revive` README][revive]); it is not a candidate stand-in language here. No
  floating point and no SIMD ([`README.md:33`][pvm-readme-nonfp]).

#### Version pinned, and why

| Candidate | Pin | Why |
|---|---|---|
| Wasmtime | 48.0.8, tag `v48.0.8` | An LTS release: a release whose number is a multiple of 12 "is supported for 24 months", others for 2 ([`docs/stability-release.md:9-12`][wt-release]); 48.0.0 was published on 2026-08-20. ADR-0014 keeps the compiler version with each binary, which favours the long support window. 50.0.1 (2026-10-29) is the newest stable release, and 51.0.0-rc.1 was tagged on 2026-11-05. Item `exp2-wasm-runtime` |
| PolkaVM | 0.38.0, tag `v0.38.0` | The newest release, published 2026-11-05; pre-1.0, so a minor release may break. Item `exp2-pvm-implementation` |

Tiers: Wasmtime lists `x86_64-unknown-linux-gnu` as Tier 1, and `aarch64-unknown-linux-gnu` and
`aarch64-apple-darwin` as Tier 2, missing continuous fuzzing ([`docs/stability-tiers.md:21-29`, `:81-86`][wt-tiers]).
Advisories: Wasmtime published 62 in 2026 by 2026-11-07, 5 of them Critical
([advisories][wt-advisories]); GitHub's API returned none for PolkaVM ([advisories][pvm-advisories]), which says how
it publishes, not how safe it is.

## Decision log

[`decisions.json`](decisions.json): the questions and defaults raised inside this experiment, in the decision
form's item format (Narthex `REQ-EXP-2`, Narthex `REQ-DEC-1`). It is committed with the experiment (Narthex
`ASM-48`) and never goes to the owner's inbox. A reserved item, such as a licence decision, is not filed here: it
needs the owner's explicit answer, and it reaches the owner's form with the adoption item at `T-008-04`.

## Draft changes

[`drafts/`](drafts/) holds the experiment's spec and plan changes, as drafts only (Narthex `REQ-EXP-3`):

- [`drafts/adr-0003-amendment.md`](drafts/adr-0003-amendment.md): the amendment to ADR-0003, `Status: experimental`;
- [`drafts/plan-decision.md`](drafts/plan-decision.md): the plan decision it would bring, `proposed`;
- [`drafts/thr-85-recheck.md`](drafts/thr-85-recheck.md): the re-check of `THR-85`'s row in `ARCHITECTURE.md` §6,
  `proposed`.

All three are placeholders until `T-008-04` drafts them from the outcome. The drafts sit here, not in `adr/`,
`ARCHITECTURE.md` or `PLAN.md`, so nothing reads them as accepted.

## Outcome

[`outcome.md`](outcome.md): one page, which format or formats are admitted, with the numbers behind it, each naming
a run (Narthex `REQ-EXP-4`, Narthex `REQ-CHN-17`). A placeholder until `T-008-04`.

## Runs

Every filled cell of the scoring sheet names the run that produced it. A run id is
`EXP-2-<candidate code>-<yyyymmdd>-<n>`, for example `EXP-2-wasm-20261112-1`, with a candidate code of
[the candidates](#candidates) (`ref`, `wasm`, `pvm`). One run may fill both languages' cells of its format. Each run
has a measurement note at `runs/<run id>.md` naming:

- the commit of this repository it ran at, and the flag that enabled the binding;
- the module-interface contract's version (`contracts/module-interface/CHANGELOG.md`), the frozen 1.0.0 for a
  scored run, and the suite's commit;
- the candidate's version and commit, and every engine setting the binding sets (for Wasmtime its `Config`,
  target and flags; for PolkaVM its backend, sandbox, gas metering and the settings of `G-30`);
- each language's toolchain and version, and each stand-in by category and language, with its binary's digest;
- the target triple, and the machine: CPU model, kernel version, and the sysctls the sandbox needs;
- the command that ran it;
- the suite's report as it returned it: its `run_id`, `format`, `languages`, `output_limit`, `findings` by check
  and language, `uncovered` by language and `missing_languages`; and for each other row how it was checked.

A run made before the contract freeze `C1` (`T-006-01`), against the contracts as they then stood, is a pre-freeze
development run. Its id is `EXP-2-<candidate code>-dev-<yyyymmdd>-<n>`, for example `EXP-2-pvm-dev-20261110-1`:
`dev` is a literal segment after the candidate code and before the date, and no candidate code contains it, so it
can't be taken for a scored run. Its note and every cell it fills say `pre-freeze`, and the scored run after `C1`
gets an id of the form above.

## Promotion record

Not promoted. The experiment is open.

When `T-008-04` writes the outcome, this section records one of:

- **adopted:** the adoption item on the owner's form, with [`outcome.md`](outcome.md) as its ref (Narthex
  `REQ-EXP-7`), naming the format or formats; once the owner answers, the one amendment to ADR-0003, the one plan
  decision, the `THR-85` re-check and the questions that still need the owner, at most three, that promotion
  consolidates from this record (Narthex `REQ-EXP-5`), with where each landed;
- **dropped:** that nothing is promoted, and that this record and its outcome stay as they are (Narthex
  `REQ-EXP-6`), with the report of the drop. Dropping both formats leaves `REQ-MDL-15` unmet; the report says what
  follows.

## Readings taken in opening it

- **The suite's rows.** The record was first drafted before `T-003-03` landed, with cell names assumed from
  `AC-19` to `AC-21` and the plan. Once the suite was on `m1` (`a443665`), the sheet's suite rows were rewritten
  from it: one row per `Check`, its 15, each listing the fixtures and controls of `Exercised::ALL`, its 41, that it
  counts, and one row for the whole report, which holds `AC-20`'s two-language count. No assumed name remains.
- **The restart.** `restarting one provider's process leaves the others running` is `T-008-02`'s test
  (`REQ-MDL-13`). The suite carries it under `ProvidersAreIsolated` (`M-15`), through the binding's `process` and
  `restart`; whether that process is a real OS process is `B-07`'s.
- **What the suite doesn't cover** is scored elsewhere: `REQ-MDL-11`'s limits other than output by `B-03`, `B-04`
  and `B-14`; DNS rebinding and TLS by `B-17` and `B-18`; the output schema's patterns and lengths by `B-19`
  (`conformance/README.md`, "Not covered").
- **The frozen contract.** The contracts freeze at 1.0.0 at `C1` (`T-006-01`). The sheet's rows are written against
  the frozen module-interface contract, not a version before it; each run note names the version it ran against.
- **Run ids and the binding legend.** The pre-freeze form `EXP-2-<candidate code>-dev-<yyyymmdd>-<n>` follows
  `EXP-1`'s (`EXP-1`'s README, Runs), and the legend's `pass (binding)` follows `EXP-1`'s `pass (adapter)`: a gap
  the binding closes is never the format's pass.
- **Per format and language.** `DEC-7` asks for one stand-in per provider category in two languages, and
  `T-008-03` for "the same stand-ins". This record reads that as per format: four categories, two languages, two
  formats. A language that one format can't take is scored as a `fail` with the gap recorded, not dropped
  (`PLAN.md` §6.2, risk 3).
- **The scoring platform.** PolkaVM's compiler and sandboxes exist only on x86_64; on the arm64 machine `EXP-1` ran
  on, PolkaVM would run interpreted, in-process, and have no machine code for the admission artefact's required
  `build.machine_code`. Scored runs are on `x86_64-unknown-linux-gnu`, also Wasmtime's Tier 1 (item
  `exp2-run-platform`); `G-14` and `G-18` score what this costs.
- **PolkaVM's licence.** ADR-0003 says Apache-2.0; the manifest says `MIT/Apache-2.0`. This record states both; the
  ADR-0003 amendment must reconcile the wording, and `L-03` records the expression the crates carry.
- **VOS's PVM.** `research/2026-11-02-vos.md` §2 finds VOS's own PVM crates under Apache-2.0, separable from its AGPL
  host. They are another PVM implementation, not PolkaVM; the default is PolkaVM (item `exp2-pvm-implementation`).

[wt-cargo]: https://github.com/bytecodealliance/wasmtime/blob/7b6c29ec3e9a49c97da231665cbd8eead9ccbda0/crates/wasmtime/Cargo.toml#L7
[wt-cargo-codegen]: https://github.com/bytecodealliance/wasmtime/blob/7b6c29ec3e9a49c97da231665cbd8eead9ccbda0/cranelift/codegen/Cargo.toml#L6
[wt-licence]: https://github.com/bytecodealliance/wasmtime/blob/7b6c29ec3e9a49c97da231665cbd8eead9ccbda0/LICENSE#L205
[wt-security]: https://github.com/bytecodealliance/wasmtime/blob/7b6c29ec3e9a49c97da231665cbd8eead9ccbda0/docs/security.md#L25-L45
[wt-security-guard]: https://github.com/bytecodealliance/wasmtime/blob/7b6c29ec3e9a49c97da231665cbd8eead9ccbda0/docs/security.md#L54
[wt-security-spectre]: https://github.com/bytecodealliance/wasmtime/blob/7b6c29ec3e9a49c97da231665cbd8eead9ccbda0/docs/security.md#L123-L140
[wt-interrupt]: https://github.com/bytecodealliance/wasmtime/blob/7b6c29ec3e9a49c97da231665cbd8eead9ccbda0/docs/examples-interrupting-wasm.md#L25-L51
[wt-limits]: https://github.com/bytecodealliance/wasmtime/blob/7b6c29ec3e9a49c97da231665cbd8eead9ccbda0/crates/wasmtime/src/runtime/limits.rs#L14-L25
[wt-determinism-test]: https://github.com/bytecodealliance/wasmtime/blob/7b6c29ec3e9a49c97da231665cbd8eead9ccbda0/tests/all/module.rs#L98
[wt-config-host]: https://github.com/bytecodealliance/wasmtime/blob/7b6c29ec3e9a49c97da231665cbd8eead9ccbda0/crates/wasmtime/src/config.rs#L2930-L2940
[wt-config-target]: https://github.com/bytecodealliance/wasmtime/blob/7b6c29ec3e9a49c97da231665cbd8eead9ccbda0/crates/wasmtime/src/config.rs#L390-L404
[wt-det-exec]: https://github.com/bytecodealliance/wasmtime/blob/7b6c29ec3e9a49c97da231665cbd8eead9ccbda0/docs/examples-deterministic-wasm-execution.md#L28-L80
[wt-release]: https://github.com/bytecodealliance/wasmtime/blob/7b6c29ec3e9a49c97da231665cbd8eead9ccbda0/docs/stability-release.md#L9-L12
[wt-tiers]: https://github.com/bytecodealliance/wasmtime/blob/7b6c29ec3e9a49c97da231665cbd8eead9ccbda0/docs/stability-tiers.md#L21-L86
[wt-advisories]: https://github.com/bytecodealliance/wasmtime/security/advisories
[cm-languages]: https://github.com/bytecodealliance/component-docs/blob/991acee434ea9fda717111253c03925dcefe9ee0/component-model/src/language-support.md
[wit-bindgen-c]: https://github.com/bytecodealliance/wit-bindgen/tree/29e03934154913417189a9bfb8e2fa6682d1d795/crates/c
[pvm-cargo]: https://github.com/paritytech/polkavm/blob/471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d/Cargo.toml#L28
[pvm-crate-cargo]: https://github.com/paritytech/polkavm/blob/471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d/crates/polkavm/Cargo.toml#L5
[pvm-crate-features]: https://github.com/paritytech/polkavm/blob/471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d/crates/polkavm/Cargo.toml#L46-L49
[pvm-readme]: https://github.com/paritytech/polkavm/blob/471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d/README.md#L5-L19
[pvm-readme-nonfp]: https://github.com/paritytech/polkavm/blob/471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d/README.md#L33-L34
[pvm-readme-licence]: https://github.com/paritytech/polkavm/blob/471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d/README.md#L36-L43
[pvm-config-backend]: https://github.com/paritytech/polkavm/blob/471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d/crates/polkavm/src/config.rs#L10-L13
[pvm-config-sandbox]: https://github.com/paritytech/polkavm/blob/471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d/crates/polkavm/src/config.rs#L55-L58
[pvm-config-paging]: https://github.com/paritytech/polkavm/blob/471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d/crates/polkavm/src/config.rs#L377-L384
[pvm-config-unsafe]: https://github.com/paritytech/polkavm/blob/471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d/crates/polkavm/src/config.rs#L317-L345
[pvm-config-gas]: https://github.com/paritytech/polkavm/blob/471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d/crates/polkavm/src/config.rs#L482-L497
[pvm-lib-gate]: https://github.com/paritytech/polkavm/blob/471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d/crates/polkavm/src/lib.rs#L11-L19
[pvm-linux-flags]: https://github.com/paritytech/polkavm/blob/471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d/crates/polkavm/src/sandbox/linux.rs#L324-L330
[pvm-linux-userns]: https://github.com/paritytech/polkavm/blob/471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d/crates/polkavm/src/sandbox/linux.rs#L441
[pvm-linux-zygote]: https://github.com/paritytech/polkavm/blob/471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d/crates/polkavm/src/sandbox/linux.rs#L799
[pvm-linux-rlimits]: https://github.com/paritytech/polkavm/blob/471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d/crates/polkavm/src/sandbox/linux.rs#L1200-L1226
[pvm-zygote]: https://github.com/paritytech/polkavm/blob/471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d/crates/polkavm-zygote/src/main.rs#L718-L801
[pvm-tests-cache]: https://github.com/paritytech/polkavm/blob/471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d/crates/polkavm/src/tests.rs#L7573-L7578
[pvm-api-bmi2]: https://github.com/paritytech/polkavm/blob/471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d/crates/polkavm/src/api.rs#L448-L451
[pvm-api-machine-code]: https://github.com/paritytech/polkavm/blob/471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d/crates/polkavm/src/api.rs#L870-L888
[pvm-targets]: https://github.com/paritytech/polkavm/tree/471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d/crates/polkavm-linker/targets
[pvm-capi]: https://github.com/paritytech/polkavm/blob/471a5b68f9dfa41e49b5d2fe80c394c1a8d68a3d/capi/polkavm_guest.h
[pvm-advisories]: https://github.com/paritytech/polkavm/security/advisories
[revive]: https://github.com/paritytech/revive/blob/v1.5.0/README.md
