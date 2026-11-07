# rutent conformance suites

The shared conformance suites of `DEC-5`: each contract's implementations are scored by the same suite. This crate
depends on `rutent-contracts` alone.

## Role suite (`role`)

`T-002-03`'s suite for the provider-role contract (`contracts/provider-role/`), the one every provider module is
checked against, Bloque's own included (`DEC-5`, `DEC-23`). `run_role_suite(harness, context, verifier)` derives
every probe from the provider's advertisement and returns a `Report`.

- **What it drives.** A provider implements `RoleProvider`: its advertisement, `handle` for one operation
  statement, a `state` fingerprint, and an optional status query. The test hooks that provoke what can't be
  provoked from outside (`force_outcome_unknown`, `force_failure`, and a reserve's `credit_reversal`) live in the
  separate `RoleHarness` trait, so a production adapter can't fabricate a reserve's word. `WithoutHooks` runs a
  provider with none.
- **What it probes.** Per value-moving tuple: a dry run, an execution compared with its quote, a repeat where
  `safe_retry` is declared, the bands' edges, a real operation above them, an unadvertised node and asset, and
  missing inputs. For an exchange or ramp: the reversal of the settled segment, a failed segment and its owed
  reversal, and the reversal of a segment never served. For a reserve: balance checks in and out of the holder's
  session, a withdrawal dry run outside it, and reversal confirmations. For a verifier: each class and an
  unadvertised one. For every provider: outcome unknown and its status query, and the operations outside its
  role.
- **Operations from elsewhere.** These are each sent several ways:
  - a dry run and a real execution per tuple, and a replay of the executed transaction itself;
  - the reversals of a settled and of a failed segment;
  - a reserve's in-session balance check and its confirmation of a credit that arrived;
  - a verifier's judgement question for each class.

  The ways are: under this deployment's name with another operator key (signed with it); under another
  deployment's name; naming another operator key while signed with the operator's; under a higher major, a
  higher patch and a lower protocol version; and signed with a key other than the operator's. Each must be refused finally and change nothing. Where an honest answer would be yes, a
  provider that served the foreign question would really give one.
- **The report.** `findings` lists every broken rule under one of 21 `Check`s. `uncovered` lists the answers the
  advertisement requires that the suite didn't see. `deferred` lists checks it couldn't conclude: until `M2`,
  `SignatureVerifies` with `DeferredVerifier`. `passed()` means no finding, nothing uncovered and nothing
  deferred; `passed_but_for_deferred()` leaves the deferred checks aside.
- **Certification takes two reports on the same adapter build** (ruled by the orchestrator, for `F-031` to
  apply in `M4`):
  1. a run through `WithoutHooks`, where `Report::passed_without_hooks()` holds: no hook took effect
     (`hooks_used` false), no finding, nothing deferred, and only hook-only answers uncovered
     (`Exercised::needs_a_hook`: outcome unknown and its status query, a failure and its reversal, a confirmed
     reversed credit);
  2. a hooked run against the provider's test mode or sandbox, where it has one, where `Report::passed()` holds.

  `WithoutHooks` keeps a test hook from provoking an answer the adapter wouldn't give, and `hooks_used` keeps a
  hooked run from being passed off as the first report.
- **What a reversal check rests on.** A reversal is judged a true undo from what the provider reports: the
  node, asset and amount it says are back, its evidence, and its `state` fingerprint returning to where it was.
  The suite has no independent view of the origin node. Completing a reversal still needs the origin reserve's
  confirmation (`REQ-ORD-3`, ADR-0008), which Core asks for.
- **The stand-ins** (`role::stand_in`) are one in-memory provider per category, plus a ramp with internal steps.
  Each check has a builder that breaks it alone, and `tests/role.rs` shows each one flagged by that check only.

`features/F-002-contract-provider-roles/notes.md` records the readings behind each check.

## Host-contract suite (`host`)

`T-005-03`'s suite for the host contract (`contracts/state-machine/host/`), the one `EXP-1` scores its candidates
with (`DEC-6`). A candidate is driven through the contract's `testing::Harness`, and through `testing::FaultSeam`
when it runs in-process. It runs the suite's own state machine, `host::probe::Probe`, and every expected state comes
from folding the probe over a journal.

- `run_with_fault_seam(config, harness, target)` runs every cell on an in-process candidate.
- `run_black_box(config, harness, target)` runs a candidate driven through `Harness` alone, such as a runtime in
  another process, killed between submits. The cells that stop a submit at a named point are reported not run.

`RunConfig` names the run, the candidate, the swap target and the seed. A `Report` gives each cell's outcome (pass,
fail with a detail, or not run) and a verdict per fault and per guarantee. `Report::to_markdown` renders both tables;
every row carries the run id, candidate, target, both contract versions and the seed, and a swap cell's row names
its direction.

### Cells and the `EXP-1` scoring rows

Each cell, and the rows of `experiments/2026-11-06-runtime-host/scoring.md` it scores. A cell marked *black-box form*
scores its rows with a kill between submits rather than a stop at a named point, so it also scores a candidate in
another process.

| Cell | Guarantees | Scoring rows |
|---|---|---|
| `acknowledged-records-survive-a-kill` | `durable` | `H-01` to `H-03`, black-box form |
| `halt-survives-a-kill` | `durable` | `H-04` |
| `stop-after-received` | `durable`, `no-effect-without-record`, `exactly-once` | `H-01`, `H-06`, `H-10` |
| `stop-after-computed` | `durable`, `no-effect-without-record`, `exactly-once` | `H-02`, `H-06`, `H-11` |
| `stop-after-durable` | `durable`, `no-effect-without-record`, `exactly-once` | `H-03`, `H-06`, `H-12` |
| `released-records-survive-a-kill` | `no-effect-without-record` | `H-10` to `H-12`, black-box form |
| `repeat-returns-the-recorded-record` | `exactly-once` | `H-05` |
| `repeat-after-a-kill` | `exactly-once` | `H-06`, black-box form |
| `reused-record-id-is-refused-and-logged` | `exactly-once` | `H-08` |
| `reordered-delivery-is-applied-in-submit-order` | `per-order-sequence` | `H-09` |
| `swap-mid-order-loses-no-order` | `swap` | `H-13` |
| `repeat-after-a-swap-is-recognised` | `swap`, `exactly-once` | `H-07` |
| `halted-host-refuses-and-only-a-halted-host-exports` | `swap` | `H-14` |
| `stop-during-import-is-refused-at-open` | `swap` | `H-16` |
| `import-refuses-a-stream-it-cannot-take` | `swap` | `H-15` |
| `replay-reaches-the-same-states` | `replay` | `H-17` |

### Not covered

- **Power loss before an fsync**, and **torn writes**. A process kill keeps the operating system's page cache, and
  no crash point falls inside a write, so the suite can't produce either. `EXP-1` checks them by inspecting each
  candidate's durability code.
- **Kills inside a submit, for a black-box candidate**: it is killed only between submits.
- **Concurrent submits**: `Host::submit` takes `&mut self`.
- **The effect driver's obligation** and `REQ-CAT-10`, which the host contract states apart from its guarantees.

The suite's own tests (`tests/host_suite.rs`) run it against an in-memory test double (`tests/support/`), correct
and with one flaw at a time, to show each cell can fail.
