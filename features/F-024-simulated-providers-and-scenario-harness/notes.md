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

## T-024-02: the assembly and the scenario harness

Readings taken without asking, each reversible; the owner's form overrules any of them.

### What is wired, once (`conformance/scenarios/src/assembly/`)

- **The evidence store and the log.** The store is opened on its shelf, its log's read taken
  (`EvidenceStore::for_the_log`) and the log opened with it (`AuditLog::open`), each once; orchestration's `Keeper`
  is taken once, its reads checked by policy; and the store is bound to that same log (`EvidenceStore::bind`), once.
  Both sit on a `Disk` that keeps what was durable across a crash, so a Core read back after one opens the log and
  the store as a restart would (`read_back`).
- **The log's two handles.** The engine holds the policy handle (`PolicyHandle`), and nothing else does;
  orchestration holds `GeneralHandle` alone. The calls the engine doesn't build yet are answered beside it, on the
  engine's own handle (`PolicySide`), so its handle is still the only policy handle.
- **Policy's sources, from the records** (the table in `assembly/policy.rs`): `ActsFor` and `Permissions` from the
  deployment's registers (Core's actor holds `CarryOnward`; the operator's login holds the grant, promotion and
  `ReadOrderState`); `Account` from the account's register; `Requester` and `OrderOrigin` from `Intake::record`,
  copied once the order opens (intake is mutably borrowed while it asks policy, so the sources read the copy);
  `OrderParties` from the acceptance in the log; `SegmentOrder` from the `route_chosen` plan in the log;
  `SegmentProvider` from the `move_value` effect in the log; `Statement` from the stated entry naming the
  commitment; `Declarations` from the reserve's advertisement as listed in the log.
- **One clock** (`kit::SharedClock`): Core's snapshots and segments, policy's session bound (`Engine::set_clock`),
  the log's skew (`Settings::on`) and the simulated world (set before each platform call). It moves a millisecond on
  each reading, so two runs read the same times in the same order.
- **Sessions** opened by the engine are handed to the catalogue's register (`Register::open_in`).
- **Intake and the segments.** `Segments::follow` takes only `Intake::start`'s answer; one `Issued` is kept across
  routings; the route a part runs is the decision record Core signed, held to the digest the log names it by. The
  operator key is watched to keep each record Core signs, since intake hands none back.
- **The providers in the log.** Each provider is granted by a decided catalogue change (the operator's login), its
  effect and its `done` result, before its advertisement was made (first trust is the grant's, and an advertisement
  made before its grant rides on none), and then its advertisement is kept and appended as its stated entry, claimed
  for its publisher. Core's clock starts an hour before the earliest advertisement for the grants, then moves to
  the scenario's start (`kit::START`).
- **The operator's key is real.** The log verifies Core's questions by the deployment's `operator_key`, which a
  placeholder can't be, so the assembly's context names a real key (`RealKey`), and `Fleet::with_context` and
  `fleet::assertion_in` take it. The simulated providers serve the deployment the world names (below).
- **The state machine.** Every input the log takes is submitted to a host of the host contract, in the log's order
  (`machine::Feed`), on the reference host in the root workspace and on three VOS replicas in the twin.
- **Staging** is a required `Config` field (`Staged::AtOnce` or `Staged::Under(Staging)`), with no default.
- **The deployment's catalogue settings**: a balance check bound of 10 a day and 60 seconds to answer one
  (`CHECKS_PER_WINDOW`, `CHECK_ANSWERED_WITHIN`); without one the catalogue lets no check out.

### The harness (`harness.rs`) and the checks (`checks.rs`)

- **Given, when, then.** `Scenario::new` takes the `AC-*` the scenario accepts and refuses anything else. The four
  checks run the base order, `sim-reserve-a`'s withdrawal and deposit, `AC-28`'s shape; the AC scenarios themselves
  are `T-024-03` to `T-024-07`'s, over this harness.
- **Faults.** A crash after the log's store took a record and before it answered (`Fault::CrashAfterRecord`), or at
  the platform's seam right before an effect is carried out (`Fault::CrashBeforeEffect`): both between a record and
  its effect. A crash is a panic the harness catches; what survives is the disk, the providers and the state
  machine's host, and Core is dropped before the disk is read back. A repeated and a reordered delivery are
  deliveries to the state machine (the reading): each input twice, or held to the step's end and those of
  different orders interleaved by the seed, each order's kept in the log's order. A provider's silence is a scripted
  `Behaviour::silent`. The swap is the state machine halted, exported, imported into another environment and
  resumed, at the platform's seam mid-order (`Fault::SwapBeforeEffect`) or as a step.
- **Seeds.** Core's identifiers are SplitMix64 over the seed (`kit::Seeded`), and the seeded scenario draws its
  amounts, its delivery fault, a swap and a silence from it. `RUTENT_SCENARIO_SEEDS` sets them, `1,2,3` by default;
  `ci.yml`'s seeded step runs `11` to `15`. The evidence store salts each statement from the operating system, which
  no seed reaches, so a run is compared by its projection, every digest and signature replaced by the order it first
  appeared in.
- **The crash test's claim is narrow**: the record of the effect it crashed on reads back from the disk, the
  provider moved nothing for it, the store binds with nothing missing, and every movement a provider's book holds
  has its `move_value` effect in the log. Carrying the order on after the restart is `F-030`'s (below).
- **The swap.** The root workspace's run swaps the reference host to a fresh reference host; the twin's swaps three
  VOS replicas out to the reference host, mid-order.

### The VOS twin (`components/stm/vos-program/scenarios/`)

- **Two processes, not one.** The module platform the assembled Core runs needs rustc 1.95 or later (cranelift
  0.135's `rust-version`), and VOS's crates don't build on a newer toolchain than their own nightly-2025-07-14 (the
  `stwo` fork uses `array_chunks`, gone from later nightlies). So `rutent-scenarios-vos` builds with VOS's guest
  toolchain, nightly-2026-03-20, links nothing of VOS, and drives a host server, `rutent-stm-vos-serve`
  (`scenarios/serve/`, built with the client's toolchain, AGPL-3.0-or-later), over a JSON-lines wire, one server per
  cluster. Both are members of the client's workspace, under its lock; `default-members` keeps a bare command in
  `client/` to the client. `scenarios.sh` builds the server and runs the scenarios.
- **Licence.** `rutent-scenarios-vos` links no VOS crate, so it is MIT (`vos-licence-v0`: AGPL covers what links
  VOS); `rutent-stm-vos-serve` links the client and is AGPL-3.0-or-later. The client lock gains Core's tree at the
  root lock's versions, so no line of `licences/RECORD` changes; it moves `rustix` 1.1.3 to 1.1.6, `linux-raw-sys`
  0.11.0 to 0.12.1 and `zmij` 1.0.21 to 1.0.23, the root lock's.
- **The simulated providers' deployment.** The guests built their stand-in in the suite's default context, whose
  operator key is a placeholder, and refused any other deployment's operation as foreign. They now read the
  deployment the platform's world names, under the state key `deployment` (`rutent_conformance::sim::DEPLOYMENT_KEY`),
  which `Fleet` seeds from its context, and serve the default where it names none. This is `T-024-01`'s code, F-024's
  own.
- **`disk.rs`** builds prov's executables in rutent's own workspace (`--manifest-path`), whichever workspace runs the
  test.

### Gaps, each named for its owner

- **Policy calls answered beside the engine** (`PolicySide`): authorisations (`T-021-04`) are permitted where the
  giver acts for the party it states and that party requested the order, and refused otherwise; charges (`T-021-05`)
  are within a bound no scenario reaches; questions about people (`T-021-07`) are not known. Each writes a
  permitting answer outside `consult`, which the engine's entry-point scan, over Core's three crates, can't see;
  `T-021-03`'s fixed rule never runs on an authorisation here, and `AC-18`'s shared limit can't be shown on the charge
  stand-in. Each is replaced through the same trait when its row builds it on the engine; `T-025-05` should review
  them as written until then.
- **Sources not answered from a record yet.** `Verifier` and `Provider` (a judgement, a provider's answer) answer a
  missing input, so a decision that needs one is refused. `Holder` answers no: no call registers a delegation.
  `Account` and the destination's owner are the deployment's register of accounts, the nearest record to the
  reserve's own. `Statement` answers the maker's party only, not the party a statement was told to about its own
  part of an order (`REQ-AUD-5`), which no entry records yet.
- **One session per Core.** The simulated reserve holds a withdrawal to its context's session, and the engine opens
  one session per id, for at most 15 minutes, so each Core has one session of 14 minutes. A scenario that runs
  longer in scenario time (`T-024-06`'s stuck orders) needs the simulated reserve to take another session, which is
  `T-024-01`'s kit.
- **The rounds' platform events** reach the log only in the next segment's drain, which may be another order's.
- **The evidence checker** enters only through `Grading` once `T-020-09` is built; `ProvidersWord` stands in.
- **The credential proxy**: the simulated world resolves nothing through one, so the segments take uses from
  `NoProxy`, and the proxy's `take_uses` runs only in `F-030`'s production assembly.
- **The receipt issuer** (`receipts::Issuer`, `T-022-03`): only the assembly makes it, bound once to its log and to
  policy's read check, and this assembly doesn't yet, since no scenario of this row issues a party a receipt. The
  first row that issues one wires it, with the log's own key, never the operator's.
- **The twin's pipe** (`Remote`: open, import, export, stop) runs first in the `STM on VOS` workflow; the wire's
  round trip is tested here, the process plumbing only there.
- **Restarts** (`F-030`): Core's own records (intake's, the segments', `Issued`, the signed records) are in memory, so a
  Core read back after a crash has the log and the evidence store and nothing of what it acted on.

### Tests, and the order

The code came first, so rule-off checks stand in for seeing each test fail first. Each piece of wiring was switched
off alone, its test seen red for its own reason, and restored:

| Switched off | Red |
|---|---|
| the engine reading Core's clock | `policy_the_log_core_and_the_world_read_one_clock` (the session refused) |
| the log reading Core's clock | every check (the grant's effect unavailable: Core isn't put together) |
| sessions handed to the catalogue | the swap check (no balance check let out, so no quote) |
| the fleet in the assembly's context | the swap check (the catalogue refuses the advertisement's deployment) |
| the store bound to Core's log, not a scratch one | `evidence_store_is_bound_to_cores_own_log` |
| `OrderParties` from the acceptance | the swap check (the customer's read of the state refused) |
| Core's actor holding `CarryOnward` | the swap check (no move permitted) |
| the segments following intake's start | the swap check (no order runs) |
| one `Issued` across routings | `rounds_transactions_are_kept_across_routings_and_no_segment_runs_under_one` |
| staging as the scenario sets it | `staging_is_the_one_the_scenario_sets` |
| the crash after a record, and the crash before an effect | the crash check (no crash, every effect recorded and carried out) |
| the swap importing what it exported | the swap check (the new host holds no order) |
| a repeated delivery submitting twice | the replay check |
| a reordered delivery interleaving | the replay check |
| Core drawing its identifiers from the seed | the seed check |
| the projection normalising the salts | the seed check |
| the advertisements recorded in the log | the swap check (results unverified, no route) |

Tests: `conformance/scenarios/tests/assembly.rs`, the row's four, on the reference host:
`crash_between_record_and_effect_leaves_no_effect_in_the_assembled_core` [`NFR-5`, `INV-7`],
`execution_environment_swapped_mid_order_loses_no_order` [`NFR-6`, `REQ-OPS-3`],
`replaying_a_scenarios_recorded_inputs_reaches_the_same_states` [`NFR-4`] and
`scenario_run_twice_with_one_seed_gives_one_run` [`DEC-18`]; `tests/assembly_wiring.rs`, five tests of the wiring,
tagged with the assembly's own identifiers (`DEC-2`, `DEC-18`, `INV-7`) and not the rows' whose rules they reach, so
the evidence table counts none of those (`REQ-CAT-5` stays partly met); and, in the twin,
`components/stm/vos-program/scenarios/tests/scenarios.rs`, the four on three VOS replicas, and `tests/wire.rs`, the
host server's wire.

**Checks, run locally one at a time** (`CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=4`, the worktree's own build
directory): the documentation check with `-D warnings`; `cargo test --workspace`; clippy `-D warnings` with and
without `rutent-stm/purpose-built-store`; fmt of the workspace, the module guests and the plain wallet guest, and
the guests' clippy (each simulated role and the outside part); the contracts' riscv32 `no_std` build; tag-check and
plan-check `--strict` (0 findings); the licence check on a fresh build directory; the contract guard against
`origin/develop` (no violations); `sources.sh check` and `nostd.sh check`; and, in the VOS client's workspace, the
client's tests built and linted with `vos`, the host server and the scenarios formatted and linted with `vos`, each
on its own toolchain. The four checks also ran on the twin's toolchain, nightly-2026-03-20, with no VOS: the
guests, prov's executables and the scenarios build and pass on it. The VOS tests themselves weren't run locally:
CI runs them.

**Runs.** CI's run ids for the `ci` job and the `STM on VOS` workflow are pending the orchestrating session's push
of this branch (`stm-vos.yml` lists it as a trigger); none is recorded until then.
