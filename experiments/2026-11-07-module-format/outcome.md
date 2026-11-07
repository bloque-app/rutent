---
experiment: EXP-2
status: written by T-008-04, 2026-11-07; awaiting the owner's answer to items exp2-adopt-format and licence-llvm-exception
---

# EXP-2 outcome

The one-page outcome of `EXP-2` (Narthex `REQ-EXP-4`). Every number names the run that produced it (Narthex
`REQ-CHN-17`), from the [scoring sheet](scoring.md) and the [run notes](runs/). Adopting it is one item on the
owner's form, `exp2-adopt-format`, with this page as its ref (Narthex `REQ-EXP-7`). The recommended option also needs
the licence item `licence-llvm-exception` answered.

## Result

| Candidate | Result | What it means |
|---|---|---|
| `wasm`, components on Wasmtime 48.0.8 | **adapt** | `M2`'s module admission and runtime (`F-013`) build on it, with the [follow-up rows](#follow-up-rows) closed before v0 reaches production. Its binding reaches `main` only once the licence ruling is recorded |
| `pvm`, PolkaVM 0.38.0 | **drop** for v0 | Not kept, not even as a second format. The module interface stays format-neutral (`DEC-4`), so a later experiment can score a PolkaVM release that its upstream holds fit for production |
| `ref`, the suite's reference double | the baseline | In process; never a candidate |

Both formats pass the whole module-boundary suite in Rust and C. PolkaVM's upstream says not to use it in
production (`G-13`, `EXP-2-pvm-20261107-1`), and rutent can't close that gap. Wasm's open gaps close in rutent's own
code and configuration. A second format adds a second binding, jail, review and dependency closure, and no
language Wasm lacks, so it does not earn its upkeep (`DEC-7`).

## Numbers

Both scored runs were on the same runner type: GitHub's `ubuntu-24.04`, AMD EPYC 7763, image `20261101.360.1`,
`x86_64-unknown-linux-gnu`. Sizes and times are measurements, not targets (`PLAN.md` §6.2, risk 9).

| Criterion | `wasm` | `wasm` run | `pvm` | `pvm` run |
|---|---|---|---|---|
| Suite rows `M-01` to `M-16` | 16 of 16 pass, in `rs` and in `c`; four category reports, 82 of 82 exercised, `missing_languages` 0 | `EXP-2-wasm-20261107-1` | 16 of 16 pass, in `rs` and in `c`; four category reports, 82 of 82 exercised, `missing_languages` 0 | `EXP-2-pvm-20261107-1` |
| Binding rows `B-01` to `B-19` | 4 pass; 6 pass (binding); 3 fail (`B-01`, `B-14`, `B-19`); 1 measured; 5 not run (`B-09`, `B-11`, `B-16` to `B-18`) | `EXP-2-wasm-20261107-1` | 3 pass; 3 pass (binding); 3 fail (`B-01`, `B-14`, `B-19`); 1 measured; 9 not run (`B-03` to `B-05`, `B-09`, `B-13`, `B-15` to `B-18`) | `EXP-2-pvm-20261107-1`, `-2` |
| A guest's CPU, memory and trap (`B-03` to `B-05`) | pass by fuel; pass by `StoreLimits`; pass (binding) | `EXP-2-wasm-20261107-1` | not run | `EXP-2-pvm-20261107-1` |
| Re-derivation on another CPU model (`B-11`) | not run; all 16 parts compiled to identical machine code in two compiler processes on one machine | `EXP-2-wasm-20261107-1` | pass: the 16 parts' digests identical on an EPYC 7763 and an EPYC 9V74, both AMD | `EXP-2-pvm-20261107-1`, `-2` |
| Gap rows | 12: 4 pass; 4 pass (binding); 2 fail (`G-05`, `G-09`); 2 not checked (`G-02`, `G-04`) | `EXP-2-wasm-20261107-1` | 19: 12 pass; 1 pass (binding), `G-31`; 1 fail, `G-13`; 5 not run (`G-20`, `G-21`, `G-24` to `G-26`) | `EXP-2-pvm-20261107-1`, `-2` |
| Admission (`B-06`) | Compile of both parts, the compiler's process from spawn to exit: ~262 ms in `rs` (261.9 to 268.1), 54.4 to 55.6 ms in `c` | `EXP-2-wasm-20261107-1` | Admission round trip, the compiler's process start and isolation included: ~6.4 ms (medians 6.3 to 6.5), both languages | `EXP-2-pvm-20261107-1` |
| Start (`B-06`) | A provider's process, from the spawn of `unshare` to both parts started: median 5.5 to 6.0 ms | `EXP-2-wasm-20261107-1` | A first load into an already running worker: ~206 ms (medians 205.9 to 206.0). The worker's own spawn is left out | `EXP-2-pvm-20261107-1` |
| Size and memory (`B-06`) | Portable 90,710 to 90,763 + 84,619 B (`rs`), 23,714 to 23,718 + 22,031 to 22,035 B (`c`); machine code 265,720 + 248,576 to 248,584 B (`rs`), 73,952 + 70,760 to 70,768 B (`c`); resident memory median 7,004 to 7,132 KiB | `EXP-2-wasm-20261107-1` | Blobs 9,374 to 9,388 + 8,197 to 8,212 B (`rs`), 7,902 to 7,910 + 6,974 to 6,982 B (`c`). Machine-code size and resident memory not measured | `EXP-2-pvm-20261107-1` |
| Licence rows | `L-01`: 29 crates whose only expression is `Apache-2.0 WITH LLVM-exception`, Wasmtime's runtime and compiler, none in `licences/RECORD`. `L-02`: 105 of 136 host and 16 of 33 guest external packages have no line | `EXP-2-wasm-20261107-1` | `L-01`: one such crate, `regalloc2` 0.9.3, through `polkavm-linker` only. `L-04`: 17 of 41 and 5 of 9 external packages have no line. MPL-2.0 `option-ext` patched out (`G-31`) | `EXP-2-pvm-20261107-1` |

The admission row sets like beside like: both figures include the admitting compiler's process. PolkaVM's 0.40 ms
compile inside that process is not comparable with Wasm's spawn-to-exit figure. The start row is not like for like:
Wasm's figure includes the spawn, and PolkaVM's leaves out the worker's own spawn, so it is a lower bound.

## Why

1. **The suite.** Both pass `M-01` to `M-16` in both languages (`EXP-2-wasm-20261107-1`,
   `EXP-2-pvm-20261107-1`), so the suite does not choose.
2. **The binding's obligations.** A "pass (binding)" is work in rutent's own code, never a strength of the format:
   the import allow-list (`B-02`), the process per provider (`B-07`) and the jailed compiler (`B-08`) in both;
   for Wasm also trap containment (`B-05`), the sandboxed part's tested restrictions (`B-13`) and the refusal of
   precompiled code (`B-15`). `F-013` carries that code, under `DEC-20`'s review. Like with like, Wasm has evidence
   PolkaVM lacks: the format's own fuel and memory limits stop a looping and a growing guest (`B-03`, `B-04`,
   `EXP-2-wasm-20261107-1`). PolkaVM has evidence Wasm lacks: re-derivation on a second CPU model (`B-11`,
   `EXP-2-pvm-20261107-1`, `-2`). Both fail `B-01`, `B-14` and `B-19`, which are the same binding work whichever
   format is chosen.
3. **The gaps.** For a v0 in production with real customer value (`DEC-19`), `G-13` decides it: PolkaVM's README
   at the pin says "Do not use it in production!" (`EXP-2-pvm-20261107-1`). `G-14` to `G-16` and `G-30` pass, but
   they are settings inside that project, not readiness. Two rows that bear on keeping a pin alive weren't run:
   `G-25`, an advisory channel, and `G-26`, whether a pre-1.0 release keeps admitted binaries runnable. Its
   compiler and Linux sandbox exist only on x86_64 (`G-14`). Wasm fails two rows (`EXP-2-wasm-20261107-1`).
   Both are rutent's configuration: `G-05`, a bound on a provider process's whole memory, and `G-09`, a memory
   layout chosen on purpose for Spectre. Two rows weren't run and must be before production: `G-02`, the 62
   advisories of 2026 (5 Critical) against the pin, and `G-04`, compilation across machines.
4. **The licences.** Both closures carry `Apache-2.0 WITH LLVM-exception` as some crates' only expression. For Wasm
   that is 29 crates, the runtime and compiler themselves. For PolkaVM it is one, `regalloc2`, which only the
   stand-ins' linker brings. See [the legal question](#the-legal-question).
5. **The cost of closing the gaps.** Wasm's are rutent's own work: a resource limit or cgroup, a chosen memory
   configuration, a run on a second CPU model, an advisory read, fuzzing, and the binding rows both formats fail.
   Its pin is an LTS release, supported until 2028-08-20 (`G-12`). PolkaVM's `G-13` closes only upstream. Adapting
   it would also need `B-03` to `B-05`, `B-13`, `B-15`, `G-20` and `G-21` run. Its jail is weaker as built: the
   worker keeps every capability but `CAP_SYS_ADMIN`, and the admitting compiler drops none and has no Landlock
   (`EXP-2-pvm-20261107-1`).

**Speed decides nothing.** PolkaVM admits faster, Wasm starts a provider faster, and admission happens once per
release. Neither figure is a target before `M4` (risk 9).

**A second format doesn't earn its upkeep** (`DEC-7`). Wasm alone claims `AC-20`'s two languages. PolkaVM runs C too
(`G-22`), so ADR-0003's one-language concern no longer limits it, but it brings no language Wasm lacks. Keeping it
would mean a second binding, jail, `DEC-20` review, licence closure (41 and 9 packages) and pre-1.0 pin to follow.
In return it gives faster admission and a re-derivation already shown, and v0 requires neither.

## `AC-20`

Claimed for Wasm, in Rust and C. Both parts of each language's stand-ins run in the sandbox, and neither part reads
another provider's state or secrets (`M-02`, `M-03`, `M-16`, `missing_languages` 0, `EXP-2-wasm-20261107-1`). The role
suite through the binding (`B-01`) is a gap of the stand-ins, not of `AC-20`. PolkaVM would also claim it, in Rust
and C (`EXP-2-pvm-20261107-1`), which retires `PLAN.md` §6.2's risk 3 whichever format the owner chooses.

## `THR-85`

The row narrows but stays an accepted residual (`R72`). For Wasm, the scored run probed the jail on Linux
(`EXP-2-wasm-20261107-1`), and the development runs probed the jail on macOS. An escape from Wasmtime lands in one
provider's jailed process, with no network, file, other process or inherited descriptor. Still open:

- the layers were probed together, and seccomp refused first;
- Ubuntu 24.04 needs an operator setting for the namespaces;
- Spectre is left at the defaults (`G-09`), the advisories aren't read against the pin (`G-02`), and no fuzzing was
  run (`B-09`);
- for `THR-18`, re-derivation on separate hardware isn't yet shown for Wasm (`B-11`).

The re-checked row is drafted in [`drafts/thr-85-recheck.md`](drafts/thr-85-recheck.md).

## The legal question

Facts only. `licences/RECORD`'s standing permission names Apache-2.0, not its LLVM exception. Two existing lines
carry the exception as one choice of an `OR`: `wasip2` and `wit-bindgen`. No line carries it as the only
expression. The orchestrator read it as covered by the standing permission; the owner has not confirmed that. The
recommendation rests on the numbers above. What the owner's ruling changes:

- **Covered by the standing permission:** the 29 Wasmtime crates take `permitted` lines when `F-013` brings them
  into the root `Cargo.lock`. The other 76 unlisted host packages, and the 16 guest ones, state only licences the
  standing ruling names. `E4`'s licence clause can then hold for Wasm.
- **Ruled case by case:** the owner rules each of the 29 crates, and each later version, one at a time.
- **Declined:** Wasmtime can't reach `main` at this pin, since the 29 are its runtime and compiler. PolkaVM's
  platform need not carry `regalloc2`: its binding takes `polkavm-linker` only to build stand-ins, never on
  admission's path (`pvm-binding/binding/Cargo.toml`). But `G-13` stands, so no format fit for production would
  remain, `REQ-MDL-15` would be unmet for v0, and this outcome would be reopened.
- **Not ruled:** no Wasmtime line is recorded, the licence check keeps the binding off `main`, and `F-013` waits.

The C stand-ins of both formats are built with clang and LLD, which state the same expression. They are tools, not
packages in a `Cargo.lock` (`L-05`). PolkaVM's own crates state `MIT/Apache-2.0` (`L-03`). This page draws no legal
conclusion. The licence question is the owner's own item, [`drafts/licence-item.json`](drafts/licence-item.json).

## Follow-up rows

The gaps the recommendation leaves, each with the plan feature that would own it. Each closes before v0 reaches
production.

| Gap | Rows | Why it matters for v0 | Owner |
|---|---|---|---|
| No limit on outbound calls, state or pending work, per module or per provider | `B-14` | `REQ-MDL-11`, `THR-57` | `F-013` |
| Output not checked against the schema's patterns and lengths | `B-19` | `REQ-MDL-12` | `F-013` |
| Nothing bounds a provider process's whole memory | `G-05` | `REQ-MDL-11`; Wasmtime reserves address space per memory | `F-013`; the cgroup in `F-030` |
| Re-derivation on separate hardware not shown for Wasm | `B-11`, `G-04` | ADR-0014, `REQ-MDL-2`, `THR-18` | `F-013` |
| Spectre settings are Wasmtime's defaults, not a choice | `G-09` | `THR-85` | `F-013`, reviewed in `F-025` |
| No fuzzer-made binary or fuzz corpus admitted | `B-09` | The admitting compiler parses hostile input (`THR-85`) | `F-013`, under `DEC-20`'s review |
| 2026's advisories not read against the pin; upkeep not measured | `G-02`, `B-16` | A known flaw in the pinned runtime | `F-013` reads `G-02` when it pins, and `F-025` reads it again; `F-033` and `F-034` for the pin's upkeep |
| The role suite not run through the binding | `B-01` | Stand-ins carry no role behaviour | `F-013`, with `F-024`'s simulated providers |
| DNS rebinding and TLS not exercised | `B-17`, `B-18` | `REQ-MDL-7`, `THR-79` | `F-014` |
| Jail layers probed only together; seccomp allow-list not pruned by a trace | `EXP-2-wasm-20261107-1` note | Each layer's own strength is unshown | `F-025` |
| Ubuntu 24.04's AppArmor restricts the user namespaces the jail needs | `G-16`, both run notes | An operator setting or an AppArmor profile, on every host | `F-030`; recorded with the obligations in `F-033` |
| The suite has no probe of an outside part naming another operation than the request in flight | F-008 notes, `T-008-03` | The binding's own test covers it; the suite doesn't | `F-013` keeps the test; the suite's next version, `F-024` |
| No closer refusal reason for a limit hit, a disallowed import or a re-admitted version | both run notes | `PlatformRefusal::Unavailable` and `UnreadableBinary` stand in | `F-013`; a change in `contracts/` follows `DEC-4` and the owner's click |

Not this outcome's: signing and routes, which the suite leaves as placeholders (`REQ-MDL-1` in `F-013`,
`REQ-MDL-21` in `F-014`).
