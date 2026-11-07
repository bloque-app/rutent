# rutent conformance suites

The shared conformance suites of `DEC-5`: each contract's implementations are scored by the same suite. This crate
depends on `rutent-contracts` alone.

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
