# F-024 notes

## T-024-01: the simulated providers

Readings taken without asking, each reversible; the owner's form overrules any of them.

- **The guests sit in `components/prov/module-guests/sim/`**, a member of the module guests' workspace, not in
  `conformance/`. The licence check reads the `Cargo.lock`s under `experiments/` and `components/` only
  (`tools/licence-check`, a `DEC-20` path this row doesn't change), so a lock of its own under `conformance/`
  would never be checked. `sim/build.sh` builds them, and `rutent-scenarios` builds them the first time a test needs
  them, as the prov tests do for the stand-ins.
- **The kit is in `rutent-scenarios`** (`conformance/scenarios/src/sim/`), since it joins the platform to
  `rutent-evidence` and `rutent-stm`, which only the assembly may import (`DEC-2`). What the providers are is in
  `rutent-conformance`'s `sim`, which imports only the contracts.
- **The books are a journal in the platform's shared state**, written by the provider's outside part to its own rail
  and read back through the state read. The WIT world has no state write, and adding one means a change to
  `components/prov/src/module/platform.rs`, a `DEC-20` path, with its metering under `REQ-MDL-11` and its
  attribution by transaction and version; that stays the row's own question for a later change if the owner wants a
  write. Until then the world applies a rail's request as the backend would.
- **A journal entry per first sight of a real movement**, and per change in the books after it. A repeat of a
  movement that moved nothing makes none, so the inner segments a private node records are each segment once
  (`REQ-ORD-41`). A refusal the stand-in itself made, for want of funds, is an entry too: the stand-in remembers the
  answer it gave, and a repeat must give it again.
- **"Answer late"** is a module that has acted and then doesn't answer within its deadline. A repeat is answered as
  it was. A module that answers slowly but in time is not a behaviour; wall time isn't something a scenario that
  replays can lean on.
- **"In pieces"** is one movement and one piece of evidence for each of two to eight pieces, which add up to what it
  gave out (`REQ-ORD-30`); "short" is `amount_out` below what it priced, with `amount_in` as stated, so the result
  still passes `check_amounts` and a shortfall is a thing a later row tests (`REQ-ORD-21`).
- **The verifier** is the same guest with no script and no book, for the role only (`DEC-16`).
- **The step limit probe** counts a sandboxed part's reads of its state, which the platform bounds per operation, so
  a module past it passes nothing out. The suite sets the limit, `STEP_LIMIT`, and lifts it after its probes. It is a
  constant of the suite and not a field of `RunConfig`, so the PolkaVM binding's runner, which writes a `RunConfig`
  out field by field and sits in a `DEC-20` path, still compiles. A binding that can't be given a limit leaves the
  probe uncovered, so the two `EXP-2` runners now report the four new probes (re-admitted running and waiting
  versions, past and within the step limit) as uncovered. Their scored runs were of the suite as it was.
- **The seeds of placeholder keys** of a simulated provider come from its name (40 to 59), so a module, its twin and
  its advertisement agree without being told. `Fleet::install` refuses two providers with the same seed, naming both;
  the five of the scenarios (`sim-reserve-a`, `sim-reserve-b`, `sim-exchange`, `sim-ramp`, `sim-verifier`) differ.
- **An untargeted behaviour** (`SimWorld::script`) is taken by the next operation that isn't a dry run and names a
  transaction, a balance verification and a reversal confirmation among them; `script_for` targets a kind.
