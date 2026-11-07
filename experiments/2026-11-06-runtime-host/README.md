---
id: EXP-1
start date: 2026-11-06
status: open
serves: DEC-6, ADR-0002, REQ-OPS-3, NFR-6, REQ-MOD-5, PS-66
opened by: T-007-01
---

# EXP-1: durable-store runtime

An experiment under Narthex's experiment mechanism (Narthex `REQ-EXP-1` to `REQ-EXP-7`), as `PLAN.md` `M1` and
`DEC-6` require. It tries each runtime behind the frozen host contract and scores it, without loading the owner with
every question it raises. Only its definitive shape reaches the owner's form, as one adoption item (Narthex
`REQ-EXP-7`).

This page holds the record's fields in the order Narthex `REQ-EXP-1` names them: id, start date, question, scope,
decision log, draft changes, outcome and promotion record.

| Field | Where |
|---|---|
| Id | `EXP-1`, in the front matter and [below](#id) |
| Start date | 2026-11-06, in the front matter and [below](#start-date) |
| Question | [Question](#question) |
| Scope | [Scope](#scope), with the [candidates](#candidates) and the [scoring sheet](scoring.md) |
| Decision log | [`decisions.json`](decisions.json) |
| Draft changes | [`drafts/`](drafts/) |
| Outcome | [`outcome.md`](outcome.md), written by `T-007-04` |
| Promotion record | [Promotion record](#promotion-record) |

## Id

`EXP-1` (`PLAN.md` §1, Identifiers).

## Start date

2026-11-06, the day `T-007-01` opened this directory.

## Question

Which runtime implements the frozen host contract (`contracts/state-machine/host/`, `DEC-3`, ADR-0002): VOS, or a
purpose-built store?

The answer is one of adopt, adapt or drop for VOS, with the purpose-built store as ADR-0002's ruled fallback and the
reference host of `T-005-04` as the floor (`PLAN.md` §6.2, risk 2). The experiment answers it with the numbers of the
[scoring sheet](scoring.md), each naming a run. It does not reopen `runtime-host-seam`: the host contract stays
runtime-neutral whatever the outcome (ADR-0002, Consequences).

## Scope

Scope is what the experiment may change (Narthex `REQ-EXP-1`).

### May change

| What | Where | Rule |
|---|---|---|
| This record | `experiments/2026-11-06-runtime-host/` | Merges at any time |
| The scoring sheet's results and run ids | [`scoring.md`](scoring.md) | One run id per filled cell |
| Measurement notes, one per run | `experiments/2026-11-06-runtime-host/runs/` | Merge at any time (Narthex `REQ-EXP-3`) |
| The experiment's own questions and defaults | [`decisions.json`](decisions.json) | Never the owner's inbox (Narthex `REQ-EXP-2`) |
| The ADR-0002 amendment and the plan decision, as drafts | [`drafts/`](drafts/) | `Status: experimental` and `proposed` until promoted (Narthex `REQ-EXP-3`) |
| A purpose-built store behind the host contract | `T-007-02`'s code | Behind a flag (Narthex `REQ-EXP-3`) |
| A VOS adapter behind the host contract, fault-injected | `T-007-03`'s code | Behind a flag, and on `main` only once VOS's licence decision is in `licences/RECORD` (`DEC-6`); until then only its measurement notes merge |

### May not change

| What | Why |
|---|---|
| `contracts/`, the host contract included | Frozen at `C1`; a change follows `DEC-4` and waits for the owner's click (`DEC-20`) |
| `conformance/`, the host-contract suite | `T-005-03`'s; the experiment runs the suite, it does not edit it |
| `SPEC.md` and `PLAN.md` | Only promotion changes them (Narthex `REQ-EXP-5`) |
| `adr/0002-runtime-host-seam.md` itself | The amendment stays a draft here until promoted (Narthex `REQ-EXP-3`) |
| The licence decision on VOS in `licences/RECORD` | The owner's, case by case (`REQ-MOD-5`, `DEC-6`); the licence check keeps an undecided dependency off `main` |
| The decision `runtime-host-seam` | ADR-0002: the outcome amends it, never reopens it |
| The module format and its sandbox | `EXP-2` (`DEC-7`, ADR-0003). Gaps of the VOS report about the sandbox are scored here because `DEC-6` names the whole gap list, but what they mean for the format is `EXP-2`'s |
| The effect driver's obligation | Not a host guarantee and not scored through `Host` (`contracts/state-machine/host/README.md`); `REQ-CAT-10`'s test scores it in a later milestone |
| Throughput or latency targets | None is claimed before `M4` (`PLAN.md` §6.2, risk 9). Timings may be recorded in measurement notes, never as a target |

## Candidates

| Code | Candidate | Task | Driven through | Faults reached by |
|---|---|---|---|---|
| `ref` | The in-memory reference host: the baseline and the floor, not a candidate | `T-005-04` | `testing::Harness`, `testing::FaultSeam` | The fault seam's named crash points |
| `pb` | A purpose-built store behind `Host` | `T-007-02` | `testing::Harness`; `testing::FaultSeam` if in-process | The fault seam's named crash points, if in-process |
| `vos` | VOS (`PS-66`), branch `vos-core` at commit `1aadf44fa2cb1eaedc015ee82343ac118cc89f1e`, the commit `research/2026-11-02-vos.md` read; the pin is a default, item `exp1-vos-commit` in [`decisions.json`](decisions.json) | `T-007-03` | `testing::Harness`, killing and restarting its process | A kill timed to each crash point, best-effort (`contracts/state-machine/host/README.md`) |
| `vos-rt1` | The re-score: VOS as the whole runtime, the state machine a VOS program, at `vos-core` `5c7897582688ea93f8d02506bee8ea25da6b1458` (item `exp1-rescore-commit`), on one node, Linux x86_64 with the JIT | `T-007-03` | `testing::Harness` and `testing::FaultSeam` through a thin `Host` client, each node its own OS process | `hold_commits` on every node for after computed; a `kill -9` of every node for the other stops |
| `vos-rt3` | The same, on three nodes | `T-007-03` | As `vos-rt1` | As `vos-rt1`; `R-01` kills the leader alone |

The re-score (`vos-rt1`, `vos-rt3`, and `pb` on the new rows) is scored in its own section of the
[scoring sheet](scoring.md), under its own run ids; the first run's cells stay as history.

Whether `pb` runs in-process or in another process is `T-007-02`'s design. Each run's note says which, and how each
fault was reached.

### Where `pb` sits

`pb` is the module `purpose_built_store` of `CMP-STM`'s crate, `rutent-stm` (`components/stm/`), beside the
reference host, built only with that crate's Cargo feature `purpose-built-store`, off by default (Narthex
`REQ-EXP-3`). It runs in-process: a store is a directory on disk, and the suite reaches it through
`testing::Harness` and `testing::FaultSeam`. The boundary check (`DEC-2`) allows it there and nowhere else without
a change: `rutent-stm` may depend on `rutent-contracts`, while a new crate of ours may depend on nothing of ours.
With the flag on, it brings no package into `Cargo.lock` that wasn't there: only `serde` and `serde_json`, which
`rutent-contracts` already uses, and so no new line in `licences/RECORD`. The default is item `exp1-pb-placement`
in [`decisions.json`](decisions.json).

## Decision log

[`decisions.json`](decisions.json): the questions and defaults raised inside this experiment, in the decision
form's item format (Narthex `REQ-EXP-2`, Narthex `REQ-DEC-1`). It is committed with the experiment (Narthex `ASM-48`) and
never goes to the owner's inbox. A reserved item, such as VOS's licence decision, is not filed here: it needs the
owner's explicit answer, and it reaches the owner's form with the adoption item at `T-007-04`.

## Draft changes

[`drafts/`](drafts/) holds the experiment's spec and plan changes, as drafts only (Narthex `REQ-EXP-3`):

- [`drafts/adr-0002-amendment.md`](drafts/adr-0002-amendment.md): the amendment to ADR-0002, `Status: experimental`;
- [`drafts/plan-decision.md`](drafts/plan-decision.md): the plan decision it would bring, `proposed`;
- [`drafts/adoption-item.json`](drafts/adoption-item.json): the adoption item `exp1-adopt-runtime`, in the decision
  form's item format, for the owner's form (Narthex `REQ-EXP-7`);
- [`drafts/licence-item.json`](drafts/licence-item.json): the licence item `licence-vos-agpl`, to publish only if
  the owner leans to VOS on the adoption item.

`T-007-04` drafted all four from the outcome. The drafts sit here, not in `adr/`, `PLAN.md` or the owner's form, so
nothing reads them as accepted.

## Outcome

[`outcome.md`](outcome.md): one page, adopt, adapt or drop, with the numbers behind it, each naming a run (Narthex
`REQ-EXP-4`, Narthex `REQ-CHN-17`). Written by `T-007-04`: VOS dropped for v0, the purpose-built store adapted.

## Runs

Every filled cell of the scoring sheet names the run that produced it. A run id is
`EXP-1-<candidate code>-<yyyymmdd>-<n>`, for example `EXP-1-pb-20261110-1`. Each run has a measurement note at
`runs/<run id>.md` naming:

- the commit of this repository it ran at, and the flag that enabled the candidate;
- the host contract's version (`contracts/state-machine/host/CHANGELOG.md`) and the suite's;
- the candidate's own version or commit;
- the command that ran it, and the machine it ran on;
- for each fault, how it was reached (a named crash point, or a timed kill);
- the suite's report, per fault, as the suite printed it.

A run made before the contract freeze `C1` (`T-006-01`), against the host contract as it then stood, is a
pre-freeze development run. Its id is `EXP-1-<candidate code>-dev-<yyyymmdd>-<n>`, for example
`EXP-1-vos-dev-20261106-1`, so it can't be taken for a scored run. Its note and every cell it fills say
`pre-freeze`, and the scored run after `C1` gets an id of the form above.

## Promotion record

Not promoted. The experiment is open until the owner answers the adoption item.

`T-007-04` wrote the outcome on 2026-11-07, recommending that VOS be dropped for v0 and the purpose-built store
adapted, and drafted the adoption item `exp1-adopt-runtime` in [`drafts/adoption-item.json`](drafts/adoption-item.json),
with [`outcome.md`](outcome.md) as its ref. It is on the owner's form once published there. When the owner answers,
this section records one of:

- **adopted or adapted:** the adoption item on the owner's form, with [`outcome.md`](outcome.md) as its ref (Narthex
  `REQ-EXP-7`); once the owner answers, the one amendment, the one plan decision and the questions that still need the
  owner, at most three, that promotion consolidates from this record (Narthex `REQ-EXP-5`), with where each landed;
- **dropped:** that nothing is promoted, and that this record and its outcome stay as they are (Narthex `REQ-EXP-6`),
  with the report of the drop.

## Readings taken in opening it

- **Counsel.** ADR-0002 says VOS's licence needs counsel before adoption. The owner's later ruling
  `counsel-licences` removes the counsel step: the licence is the owner's decision, recorded in `licences/RECORD`
  (`DEC-6`, `REQ-MOD-5`). This experiment follows `DEC-6`. The ADR-0002 amendment must reconcile the two; this
  record does not.
- **The gap list.** `DEC-6` names "the gap list" of `research/2026-11-02-vos.md`. The report has no single list, so
  the scoring sheet takes every gap, constraint and absence the report states against VOS at the pinned commit, each
  restated as a check. The report decides nothing and was read as text only, nothing built or run, so every gap is a
  claim to check, not a result.
- **The suite's cells.** `T-005-03` is not finished when this record opens. The sheet's host rows follow the
  guarantees and their faults as `GUARANTEES` states them at host contract 0.1.0; when the suite lands, each of its
  per-fault cells is mapped onto a row, and a cell with no row gets a new one.
