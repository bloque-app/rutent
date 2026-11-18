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
  engine's own handle (`PolicySide`), so its handle is still the only policy handle. This is structural (review of
  `712c3fb`, 2): the disk under the log, `read_back`, `assemble_on`, the sources' records and `PolicySide::engine`
  are crate-private, and four compile-fail doc tests in `lib.rs` hold that a scenario test reaches the log only to
  read it, through policy's calls or through Core's steps.
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
  routings; the route a part runs is the decision record Core signed, read only from the bytes the log's digest
  holds (review of `712c3fb`, 6). The operator key is watched to keep each record's signed envelope, since intake
  hands none back.
- **The providers in the log, then in the catalogue** (review of `712c3fb`, 3, and of `ebd6def`). Each tuple a
  provider advertises is granted on its own: decided as a catalogue change of the operator's login naming the tuple
  and the advertising key, its effect recorded, then that tuple granted in Core's catalogue at the effect's own time,
  by the decider, under the key, as `grant-<provider>-<the effect's index>`, then the effect's `done` result. So
  nothing is granted before its record. The catalogue's grant equals the log's on what the log's grant holds: the
  tuple, the advertising key, the decider and the effect's time. Its party, expiry, categories and reversal capacity
  are the deployment's settings (`kit::grant_terms`), which core-internal's `change_catalogue` doesn't carry, so the
  claim covers those four fields and no other. A tuple's id (`kit::tuple_id`) is `tuple-` and the first 32 hex
  digits of SHA-256 over the RFC 8785 canonical JSON of its provider, category, origin, destination, asset and
  `to_asset`, each in provider-role's own form, so an outside checker recomputes it from the advertisement (a test
  does). All of it happens before the advertisement was made (first trust is the grant's); then the advertisement is
  kept and appended as its stated entry, claimed for its publisher, and joins the catalogue at the time the log
  received it. Core's clock starts an hour before the earliest advertisement for the grants, then moves to the
  scenario's start (`kit::START`). Core's catalogue and its platform are crate-private, read through views
  (`Core::catalogue`, `Core::fleet`), held by two more compile-fail doc tests. The platform's own catalogue, which
  admits the releases (`Fleet`), is the platform's and not Core's, and is granted as `T-024-01` left it.
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
  machine's host, and Core is dropped before the disk is read back. A provider's silence is a scripted
  `Behaviour::silent`, which leaves its segment's result unanswered and moves nothing for it (its own test). The swap
  is the state machine halted, exported, imported into another environment and resumed, at the platform's seam
  mid-order (`Fault::SwapBeforeEffect`) or as a step; each swap keeps what both hosts held, and the check holds the
  new host's journals to the old one's right after the import.
- **Deliveries, both ways** (review of `712c3fb`, 7). To the state machine: each input twice (`Delivery::Repeated`),
  recorded once; or held to a step's end and those of different orders interleaved by the seed
  (`Delivery::Reordered`). `Stm` is per order, so the interleaving changes no order's states by itself; what the
  check holds is that each order's journal is in the log's own order, which a delivery that broke an order's sequence
  fails, and that the host's every state is the replay's. Between Core and a provider: every operation the segments
  carry out delivered to its provider twice under its transaction, Core taking the second answer
  (`Fault::DeliverTwice`), and the whole route run again (`Step::Rerun`): each transaction moves once, as many
  movements as with one delivery, every order ends where it ends with one, and the host's states are the replay's.
  A provider's answer reordered against Core, a result arriving before its effect's receipt or apart from the call
  that carried it out, needs a seam Core doesn't have: `Modules::carry_out` answers in the same call, and no port
  takes a provider's result delivered later. That port is the stuck and late-delivery path's (`T-020-11`), which
  `T-024-06` (`AC-5`) and `T-024-08` (`INV-4`, `INV-8`) test.
- **Seeds.** Core's identifiers are SplitMix64 over the seed (`kit::Seeded`), and the seeded scenario draws its
  amounts, its delivery fault, a swap and a silence from it. `RUTENT_SCENARIO_SEEDS` sets them; every check runs under
  each (the crash, swap and replay checks `1` by default, the seed check `1,2,3`), and `ci.yml`'s seeded step runs
  `11` to `15`. The same steps under two seeds project to two runs, so the seed reaches what Core draws (review of
  `712c3fb`, 8). The evidence store salts each statement from the operating system, which
  no seed reaches, so a run is compared by its projection, every digest and signature replaced by the order it first
  appeared in.
- **The crash test's claim is narrow**: the record of the effect it crashed on reads back from the disk, the
  provider moved nothing for it, the store binds with nothing missing, and every movement a provider's book holds
  has its `move_value` effect in the log. Carrying the order on after the restart is `F-030`'s (below).
- **The replay** compares whole `Order` values, the host's state after each record against the fold's after each
  input, and a fresh host's journals record for record (review of `712c3fb`, 5).
- **The swap.** The root workspace's run swaps the reference host to a fresh reference host. The twin's swap check
  moves the state machine from one cluster of three VOS replicas to another, mid-order, so an import into VOS runs;
  the seeded runs' planted swaps there move it out to the reference host.

### The VOS twin (`components/stm/vos-program/scenarios/`)

- **Two processes, not one.** The module platform the assembled Core runs needs rustc 1.95 or later (cranelift
  0.135's `rust-version`), and VOS's crates don't build on a newer toolchain than their own nightly-2025-07-14 (the
  `stwo` fork uses `array_chunks`, gone from later nightlies). So `rutent-scenarios-vos` builds with VOS's guest
  toolchain, nightly-2026-03-20, links nothing of VOS, and runs a host server, `rutent-stm-vos-serve`
  (`scenarios/serve/`, built with the client's toolchain, AGPL-3.0-or-later), as a process of its own, one per
  cluster, speaking to it over a JSON-lines wire (`rutent-stm-vos-wire`, `scenarios/wire/`). A call waits at most
  ten minutes for its answer (`ANSWER_WITHIN`). All three are members of the client's workspace, under its lock;
  `default-members` keeps a bare command in `client/` to the client. `scenarios.sh` builds the server and runs the
  scenarios.
- **Licence.** `rutent-scenarios-vos` links `rutent-scenarios`, the wire and rutent's crates, all MIT, and nothing
  AGPL: it is MIT (`vos-licence-v0`: AGPL covers what links VOS). The wire is its own MIT package so the twin links
  no AGPL library (review of `712c3fb`: the wire first sat in the server's AGPL package, and the twin linked it).
  `rutent-stm-vos-serve` links the client and is AGPL-3.0-or-later, and nothing links it. The client lock gains Core's
  tree at the root lock's versions, so no line of `licences/RECORD` changes; it moves `rustix` 1.1.3 to 1.1.6,
  `linux-raw-sys` 0.11.0 to 0.12.1 and `zmij` 1.0.21 to 1.0.23, the root lock's.
- **The simulated providers' deployment.** The guests built their stand-in in the suite's default context, whose
  operator key is a placeholder, and refused any other deployment's operation as foreign. They now read the
  deployment the platform's world names, under the state key `deployment` (`rutent_conformance::sim::DEPLOYMENT_KEY`),
  which `Fleet` seeds from its context, and serve the default where it names none. This is `T-024-01`'s code, F-024's
  own.
- **`disk.rs`** builds prov's executables in rutent's own workspace (`--manifest-path`, `--locked`), whichever
  workspace runs the test.

### Gaps, each named for its owner

- **Policy calls answered beside the engine** (`PolicySide`; review of `712c3fb`, 1): charges (`T-021-05`) and
  questions about people (`T-021-07`) are each decided as the engine decides a call: its
  own fixed rules (`core-authz`'s `Fixed`, the register of actors and the roles' rule among them) over the same
  records, then the deployment's policy, which `Core::refuse` reaches. The answer alone is the stand-in's: an
  authorisation recorded as the consultation's answer; a charge within a bound no scenario reaches where permitted,
  over a bound of 0 where not, so `AC-18`'s shared limit can't be shown on it; a question about a person refused
  where refused and otherwise not known. Authorisations were answered so too until `T-021-04` built them on the
  engine; its merge dropped that `impl` for the engine's, and gave the assembly's policy handle the read of
  `CoreLog` the engine's register needs (`F-021`'s notes). Each that remains still writes its answer outside the
  engine's `consult`, which the
  entry-point scan over Core's three crates can't see, so `T-025-05` should review them until each row builds its
  call on the engine; dropping one is deleting its `impl` and forwarding to the engine, as `Decisions` does.
- **Promotion.** Nothing in the assembly calls `Segments::promote`, so no scenario of this row promotes a staged
  release through a decided catalogue change; the path is `T-021-03`'s rule and `T-020-08`'s call, which `T-024-07`'s
  upgrade scenario (`AC-19`) first runs here.
- **For `T-025-05`: the stand-ins' consultation is a copy.** `PolicySide::consult` and `verdict`
  (`assembly/policy.rs`) copy the engine's `Engine::consult` and `verdict` (`core-authz`'s `engine/mod.rs`), which
  are crate-private there. Until each stand-in goes, they must be checked against the engine's on every change to
  it: the order of the fixed rules and the deployment's policy, a refusal beating not known, and each reply's
  answer.
- **For `T-025-05`: the log's skew checks can't trip here.** The log, policy and Core read one clock, so an effect
  or an advertisement is never dated outside the log's skew of its own clock; the rows that hold those rules are
  tested in `core-audit`'s own tests, not by the assembly.
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

The review of `712c3fb` added tests, each written first and seen red against the code it reviewed, and rules, each
switched off alone and seen red:

| Test or rule | Red |
|---|---|
| a stand-in's authorisation in a role the fixed rule refuses | `authorisation_given_in_a_role_the_fixed_rule_refuses_is_refused_beside_the_engine`, against `712c3fb`'s stand-in |
| a charge to an actor no register lists | `charge_to_an_actor_the_register_doesnt_list_is_over_its_bound`, against `712c3fb` |
| the deployment's refusal beside the engine | `deployment_refusing_reaches_the_calls_answered_beside_the_engine`, against `712c3fb` |
| the catalogue's grant on the log's terms | `catalogues_grant_is_the_one_the_log_records`, against `712c3fb` |
| the swap imported, not opened afresh and fed again | the swap check (the new host held none of the old one's journals; each input delivered twice) |
| the replay of whole states | the replay check (a fold that stops after each order's third input) |
| the register of actors, the session register, the destination's register | `request_by_an_actor_the_register_doesnt_list_is_refused`, `request_in_a_session_policy_never_opened_is_refused`, `destination_no_register_holds_is_never_quoted`, each against its adapter answering yes |
| the record read from the digest's bytes | `route_whose_signed_record_differs_from_the_logs_digest_is_not_run` |
| each order's journal in the log's order | the replay check (an interleave that reverses every input, an order's own among them) |
| a provider's operation delivered twice | the replay check (the second delivery never made) |
| two seeds, two runs | the seed check (Core's identifiers drawn from a seed of 7, whatever the scenario's) |
| a settlement account's reserve not a party | `reserve_a_settlement_account_is_at_is_not_one_of_the_orders_parties` |
| the silence fault | `silent_provider_records_its_deposit_failed_and_moves_nothing_for_it` |
| the crate's hold on the log | the compile-fail doc tests (Core's disk made public again) |
| the floor the authorisation names, handed to the segments (`Live::floor`, `T-020-10`) | `floor_the_authorisation_names_holds_the_part_at_the_segment_that_crosses_it` (the floor handed over as none) |
| the authorisation an order runs under: a permitted `authorise` of that order | `order_runs_under_its_permitted_authorisation_not_an_earlier_refusal_of_its_id`, against the first entry of the id |

Tests: `conformance/scenarios/tests/assembly.rs`, the row's four, on the reference host:
`crash_between_record_and_effect_leaves_no_effect_in_the_assembled_core` [`NFR-5`, `INV-7`],
`execution_environment_swapped_mid_order_loses_no_order` [`NFR-6`, `REQ-OPS-3`],
`replaying_a_scenarios_recorded_inputs_reaches_the_same_states` [`NFR-4`] and
`scenario_run_twice_with_one_seed_gives_one_run` [`DEC-18`]; `tests/assembly_wiring.rs` (eight),
`tests/assembly_policy.rs` (three), `tests/assembly_intake.rs` (three) and one unit test in `assembly/mod.rs`, tests
of the assembly, each tagged `DEC-18`, the row's, and none with the rows' whose rules they reach, so the evidence
table counts none of those (`REQ-CAT-5` stays partly met); and, in the twin,
`components/stm/vos-program/scenarios/tests/scenarios.rs`, the four on three VOS replicas, and `tests/wire.rs`, the
host server's wire (two).

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

**Runs.** At `m3`'s merge of this row (`970828a`), every workflow passed: `CI` run 43184990293 (the `ci` job,
with the seeded step), `STM on VOS` run 43184990196, `Contract guard` 43184990355, `licences` 43184990320 and
`records` 43184990233. On the branch, `STM on VOS` passed at `28b6353` (run 43176806840, the first to reach the
twin's scenarios on three VOS replicas) and at `304fa27` (run 43178711555). Run 43165714465 at `ebd6def` failed at
the twin's lint step before any scenario ran; `28b6353` fixed it.

**Polygon and BSC never turn final on their own** (a gap, from `T-020-09`). The fleet serves `polygon` and `bsc` with
`Confirmations(2)`, and the simulated world adds no blocks unless a scenario does (`SimWorld::network_mut`'s
`advance`). The assembly's grading reads them as the fleet serves them (`assembly/grading.rs`, through
`fleet::networks_of`), so a final deposit there stays pending, and its order waits for its public record, until a
scenario advances the network. No scenario routes a final deposit to either network yet; one that does needs a step
that adds blocks (for `T-024-04`'s and `T-024-05`'s scenarios).

## T-024-03: routing, quoting and acceptance scenarios

Readings taken without asking, each reversible; the owner's form overrules any of them.

**Tests** (`conformance/scenarios/tests/`): `routing.rs` (`AC-2`, `AC-3`, `AC-33`, `GUA-2`/`REQ-QTE-13`), `quoting.rs`
(`AC-13`, `AC-14`), `acceptance.rs` (`AC-15`, `AC-23`, `AC-24`, `AC-43`), with `routing_support/` for the shared world.
Each runs on the assembled Core through `Scenario::core` and drives Core's own `request`, `quote`, `accept` and `run`.

**The world.** Every route starts with a withdrawal and ends with a deposit (`REQ-ORD-16`), only the origin reserve is
asked for a withdrawal, and a private node is its publisher's alone (`LeftOutReason::NodeNotListed`), so the stand-in
exchange and ramp, which sit on public nodes no reserve reaches, can't be on a route here, and a ramp between two
reserves' ledgers is left out of the snapshot. Alternatives for one tuple therefore come from a second reserve
*beside* the first: `sim-reserve-a-quick` shares `sim-reserve-a`'s publisher, bank and ledger, so its deposit is a
second provider of the same tuple. A route across a reserve, an exchange and a ramp (`AC-2`'s wording, `AC-1`'s) needs
simulated providers that join the reserves, which is `T-024-04`'s to add; `AC-2` is shown on the one step that has two
providers.

**Added to the simulator** (`conformance/` only): `Shape` (`role/stand_in.rs`) and `sim::shape_of`, which a reserve takes
from its name, since the Wasm guests build a provider from its name alone: a fee quoted and charged, a charge beyond the
quote, the seconds it declares, the evidence it declares, and `beside`, the provider whose publisher and nodes it shares.
Names: `sim-reserve-a-quick`, `sim-reserve-slow` (four days), `sim-reserve-charging` (7 beyond the quote, the gap the
brief names) and `sim-reserve-dear` (6 000 000). A name with no shape is the stand-in as before. The ramp and the
exchange are unchanged.

**Added to the assembly** (`conformance/scenarios/src/`): `kit::prices_of`, Core's prices from a reserve's shape;
`Core::world` (script a provider), `Core::set_evidence_floor` (a deployment setting; `core-internal` has no catalogue
change for it, so it isn't in the log), `Core::check_balance` and `Core::check_by_dry_run` (a check outside an order,
through Core's catalogue and the platform), and `Core::check_charges` with `Core::channel` (`assembly/charges.rs`),
which feeds each settled segment's charge, read from its fee record, result and effect in the log, to
`quote::Charges::check` and records the operator's raises.

**Defaults taken.** A scenario's default `Objective` is the request's; `AC-2` reads "no provider being called" as the
router's decision being a function of the record alone: the router over the record's own snapshot gives the record
again, and under the other objective the other route, with no provider's books or rails touched. The holder's own
order's check is logged and never counted (`REQ-CAT-3`), so the counted check of `AC-15` is one outside an order. A
quote without a firm rate must be accepted like any other (Core starts no order before acceptance); the scenario holds
that nothing starts, and nothing reaches the reserve, before it.

**Gaps, with owners.**
- *Charges are checked by the assembly, not by Core.* Nothing in `core-orch`'s segments calls `quote::Charges::check`,
  and no operator channel exists in Core's wiring; `T-023-02` (the channel) and the segments' owner should call it where
  a segment settles. `check_charges` stands in until they do.
- *Signed terms and holds aren't wired into intake* (`T-021-02`'s hand-off): nothing calls `Terms::state` or redeems a
  hold, so `AC-13` and `AC-14` rest on the advertised price and no terms in force, and no scenario here shows a charge
  held to terms stated after the price (`quote.rs` tests do). `AC-15`'s hold-redeeming half and `REQ-QTE-15` stay
  untested at the assembly.
- *An evidence floor isn't a logged change*, so `AC-33` sets it on the catalogue directly.
- *`AC-24`'s verifier half* (a provider's rules naming a judgement about a party) isn't run: no simulated provider
  advertises a need; the reserve's rules name the payer's and the beneficiary's accounts, which is what is tested.

**Rule-off table** (each rule switched off alone, or with others in one run whose reds name their own causes, and
restored; the tests were green first and watched go red):

| Switched off | Red |
|---|---|
| intake keeping a request identifier's order | `repeated_order_request_leaves_one_order` |
| the stand-in reserve refusing a missing input | `requester_beneficiary_and_payer_are_recorded_apart_and_checked_by_their_providers_rules` |
| the catalogue counting a check | `balance_check_answers_yes_or_no_and_is_logged_and_counted` |
| the router's three-day line | `segment_over_three_days_has_no_firm_rate_and_waits_for_acceptance` |
| the charge check's comparison | `charge_above_the_formulas_maximum_is_flagged` |
| the stand-in's extra charge | `charge_above_the_formulas_maximum_is_flagged` |
| the snapshot's evidence floor | `provider_below_the_floor_is_not_routed_through` |
| `Core::set_evidence_floor` setting it | `provider_below_the_floor_is_not_routed_through` |
| the router dropping a refused dry run | `rejected_dry_run_falls_to_another_provider_of_the_tuple` |
| the router ranking by settlement | `changed_objective_can_change_the_route_and_the_record_explains_it_with_no_provider_call` |
| Core's prices from the shapes | `route_with_no_positive_arrival_is_not_offered`, the `AC-2` test |
| a dry run changing the reserve's books | `dry_run_changes_nothing_and_refuses_an_undepositable_destination` |

The router's `FeeAboveAmount` cut alone leaves `route_with_no_positive_arrival_is_not_offered` green: the zero-arrival
route is also dropped by the route's own arithmetic, so the test holds the outcome (nothing offered, nothing asked) and
the price rule above holds its cause.

## T-024-04: execution and closing scenarios

Readings taken without asking, each reversible; the owner's form overrules any of them.

**Tests** (`conformance/scenarios/tests/`): `execution.rs` (`AC-28`, `AC-32`, `AC-10`, `AC-11`, `AC-31`) over the base
reserve, and `execution_routes.rs` (`AC-1`, `AC-2`'s full route, `AC-30`, `AC-26`, `AC-27`) over the joined providers,
with `execution_support/` for the world. Each drives Core's own `request`, `quote`, `accept` and `run`, or its `arrive`
and `file`. Each carries exactly the `AC-*` of its plan bracket.

**Added to the simulator** (`conformance/` only; existing names and scenarios unchanged). `Shape` gains `publisher`,
`node`, `public_node`, `outer`, `asset`, `to_asset`, `deposit_input` and `internal_steps`, and the exchange and the
ramp are shaped by name as the reserve is (`Exchange::shaped`, `Ramp::shaped`, a fee and the seconds a ramp quotes).
Two groups, by name (`sim::shape_of`):
- `sim-join-in` (reserve, COP), `sim-join-exchange` (COP to USD), `sim-join-ramp` / `-quick` (5 000, 30 s) / `-steps`
  (internal steps), `sim-join-out` (reserve, USD): one publisher shares the private node `sim-join-ledger`, the ramp
  leaves it onto the public node `sim-join-chain`, and `sim-join-out` is a reserve whose inner node is that public node.
- `sim-chain-in` (reserve, on-chain `sim-chain-a`), `sim-chain-ramp-b`, `sim-chain-ramp-c`, `sim-chain-out-c`,
  `sim-chain-out-b` (USDC): no exchange, no local money; the reserves at the destination deposit to an address.
`Spec::networks`, `public_nodes` and `private_nodes` say what each provider's module declares and what the operator
lists; a conversion or transfer into a private node posts no public movement. A deposit's destination reads
`beneficiary.address` where there is no bank account. `RoleHarness::internal_steps` and `Simulated::internal_steps`
let a test read a module's internal steps back from its own journal.

**Added to the assembly:** public nodes on the operator's list with a wallet the operator publishes
(`kit::join`); prices for exchanges and ramps (`kit::prices_of`), an exchange's with its rate of one; the operator's
slippage limit on each exchange tuple (`kit::bounds_for`, `REQ-QTE-9`); `Books` reading every provider that serves in a
private node, in route order; `Core::exposures` (a view); `Core::report_price` (`assembly/price.rs`).

**Defaults taken.**
- `AC-1`'s "three providers" is read as a route over three kinds of provider: a reserve, an exchange and a ramp, with
  the reserve the value is deposited in (four providers, four segments).
- The executed price is reported by the assembly (`Core::report_price`): F-020's notes name "closing, `T-020-09`, or the
  assembly", closing doesn't, and nothing in Core's segments does. `F-030`'s assembly must call it where an order completes.
- `AC-28`'s "a transfer inside the reserve's book can't close" is a ledger-to-ledger request, which gets no route.
- `AC-31`: "shared" is the network tying the second order's movement to the first's (`Fault::OnePaymentForTwo`); the
  second order's records name the reserve as whose word, and its final deposit doesn't close.
- `AC-10`: "nothing counts against the operator" is the arrival adding no input, no value held and no stuck order.
- `AC-11`/`AC-26`: the reserve files as its own actor, listed in the deployment's register of actors (a deployment
  setting, `Registers::list`), its party the reserve (`files_at`).
- `AC-2`: the choice round confirms each route's own amounts, so a record can't be decided again under the other
  objective; each record is decided again from itself, and the other route is dropped as `outranked` or `unconfirmed`.
- `AC-30`: the refusal half is the catalogue's own check of a tuple (`check_tuple`, `Grant::new`) on the joined
  exchange's and ramp's grant terms; Core's catalogue can't be handed a malformed tuple through an advertisement.

**Gaps, with owners.** None blocks a scenario. `Core::report_price` is the assembly's (`F-030`); the slippage limits and
the public-node list are deployment settings the assembly holds (`F-030`); `AC-31` is shown on the network's tying, not
on a provider that states one payment for two orders itself (`T-024-06`, `T-024-08`).

**Rule-off table** (each switched off alone, with a hard timeout, restored; the tests were green first):

| Switched off | Red |
|---|---|
| public nodes on the operator's list | all five route tests |
| the deposit's address fallback | `AC-27`, `AC-26`, `AC-2` |
| `Books` reading every provider | the `AC-2` test (the faster route's replay) |
| the executed price's amounts | `AC-1` (and `AC-2`) |
| an exchange's rate / the slippage limits | every joined route (no price for the exchange, `NoPrice`), seen on the way |
| conversions and transfers into a private node posting no movement | none: the movement went to a host nobody serves and was dropped; no test holds it |

## T-024-14: simulated publishers emit the inner-segment record, and the assembly's books read it

**Built** (`conformance/` only). `rutent_conformance::sim::publishers_record` makes, from a provider's book, the
`inner-segments` statement `T-020-18` added: made by the provider, stating for its publisher (`Spec::publisher`),
naming its private node, each segment it ran there with its transaction, asset and amount in and out and fee,
labelled `publishers_word`, signed with the statements key its advertisement lists (behind the `signing` feature, as
the guests don't compile it). The assembly's `Readings` (`assembly/grading.rs`) is the books: `Books::recorded` reads
those statements, for each provider serving in the node, and no longer rebuilds the segments from the journal.
A record counts only if its key is one its maker's advertisement lists for statements, it verifies under it and
`check_inner_segments` holds it to that advertisement (maker, publisher, node declared); otherwise it is no record
(`NothingRecorded`, fails closed). No figure is refused: a record that doesn't add up still reaches the replay.
A transaction the closing asks about that has no accepted segment, because its provider's record was refused, makes the
whole node's reading `None`, so the closing records `NothingRecorded` and the rest of the node is never passed off as all
of it. `Core::forge_record_of` plants a record altered after it was signed. `Behaviour::misrecording` scripts one: the publisher records another amount out than the segment gave. Views for tests:
`Core::publishers_records(node)`, `Core::books_for(node, transactions)`.

**Tests** (`tests/publishers_record.rs`, and unit tests in `grading.rs`): the three the row names. Each asserts the
records' content (maker, publisher, node, label, transaction, assets, amounts, fee), not that one exists.

**Defaults taken** (reversible).
- The reader holds a record to the advertisement the installed provider carries, the one its catalogue joined; the
  assembly's catalogue isn't consulted.
- The record states what the book says now, as of Core's last reading; it is made on each read, not stored.
- A route across two private nodes **cannot be run on the platform**: the router joins segments by exactly equal
  endpoints (`Outer(n)` is not `Inner(n)`), a reserve enters a private node from an outer node, and a ramp, the only
  segment leaving a node for a public one, ends at an inner node. So the mismatch "at the second of two nodes" is shown
  on the platform at **one** node only (a scripted mismatch is recorded in the log's `replay` against the node; the
  test's name says "second of two nodes" because the plan's row does), and across two
  nodes as `CMP-EVIDENCE`'s replay over what the assembly's books read of two reserves' nodes. Owner of a platform
  route across two nodes: `T-024-05` (it needs the router or the catalogue to join an outer node to an inner one,
  which is not a conformance change).
- `PolicySide` answers `OutcomeDeclines::decline_outcome` with `AppendFailure::Unavailable`, recording nothing, because
  the engine has no `decline_outcome` on this base (`T-021-09` builds it). It is for `T-020-20`; when `T-021-09` merges
  the body is one forward to the engine, as its neighbours.

**Gaps, with owners.**
- **Binding a private node to its publisher is not conformance's to make** (`T-024-06`'s `AC-4`, `AC-6`). Owner:
  `T-020-32`, a new row in wave 29, which also lifts `T-024-06`'s two `#[ignore]`s; `T-020-30` makes `rests_on` a list. The log binds
  a node (`Reversals::follow`) from an `evidence` whose movement names the node, or a `replay` naming it, resting on a
  stated entry about the order that states for the publisher. Core never appends the publisher's record as a stated
  entry, and a `replay`'s `rests_on` is the exit segment's result, and no replay exists where value stays inside. The
  shape needed in `components/core-orch/src/closing/`: `Books` returns the signed `Statement<InnerSegments>` with the
  segments; closing appends it as a stated entry about the order (attribution verified, party the publisher) and
  cites its receipt where it binds the node (an `evidence` naming the node, or `replay`'s `rests_on` carrying each
  record's receipt: a contract change to `What::Replay` of one receipt to a list, which is also `REQ-AUD-9`). Owner:
  `T-020-32` (with `T-020-30` for the list); `T-020-18`'s hand-off of the receipts is the same work. The simulator already
  emits what that row reads. Un-ignoring `AC-4` and `AC-6` needs that row, and `T-024-06` merged into `m3`.
- **`REQ-AUD-9`'s receipts on `replay`** (`T-020-18`'s hand-off to this row): the same row; needs `components/`.

**Rule-off table** (each switched off alone, restored, tests green first).

| Switched off | Red |
|---|---|
| the record's publisher (the provider named in place of it) | all three tests (the record fails its advertisement and is no record) |
| `Behaviour::misrecording` | `mismatching record is replayed and recorded at the second of two private nodes` |
| the node filter of the records read | `publisher's emitted record is read by the assembly's books` |
| the transaction filter of the books | all three |
| the books read nothing | all three |
| a transaction without an accepted segment makes the node's reading `None` | `record_refused_for_one_provider_of_the_node_leaves_the_node_with_nothing_recorded` |
| a record's transactions (empty) | all three |
| the signature check on a record | `a_record_changed_after_it_was_signed_is_not_read` |
| the listed-key check | `a_record_signed_with_a_key_its_maker_does_not_list_is_not_read` |
| `check_inner_segments` | `a_record_of_another_publisher_or_another_node_is_not_read` |
| the label (`publishers_word`) | not switchable here: one value, `T-020-18`'s contract rule-off |

## T-024-05: private-node scenarios

Branch `f-024/t-024-05-private-node-scenarios`, from `m3` at `d0a7cf2c`. Edits `conformance/` only; `components/` and
`contracts/` are untouched. Readings taken without asking, each reversible; the owner's form overrules any of them.

**Tests** (`conformance/scenarios/tests/private_nodes.rs`, the eight the row names, each with exactly its plan tags).
- `AC-29`: the route `sim-join-in`, `-exchange`, `-ramp`, `-out`. The decision record's chosen operations are a
  withdrawal, an exchange, a route and a deposit; each operation's figures name its provider's publisher (one publisher
  for the reserve, exchange and ramp, another for the far reserve), the catalogue names the private node's publisher and
  none for the public node, and the log labels the exchange's evidence, inside the node, with the publisher.
- `AC-34`: a reserve (`sim-join-out-private`) out of the public node whose bank rail its publisher declares private and
  no network serves. The deposit closes on the public node's movement into the rail, naming the rail, and on the
  reserve's own statement, which is its word and no network's. The same reserve with the rail declared by no one
  (`sim-join-out-silent`) is quoted and refused at run (`NoPublicRecord`) before anything is handed over.
- `AC-35`: a match is `consistent_with_the_exit` and the segment inside the node stays its publisher's word; a scripted
  mismatch is recorded against the node, the order still completes, and the node gets no closing on the public movement
  out. A route ending in the private rail has one replay only (of the node it left for the public one), none for the
  rail, whose record is the publisher's word and is replayed against no exit.
- `AC-39`: the real plain wallet at a public node, three orders landed. A match raises nothing; a wallet asked directly
  for a withdrawal the host doesn't attribute reads short, one asked for a deposit reads over; each raises the operator
  with the direction, the amount and whose word, the attribution and the log's earlier entries unchanged. At a private
  node the same reading with the publisher's word: a match raises nothing, a short raises labelled so.
- `DEC-24`, `REQ-ORD-41`: **a route across two private nodes runs on the platform**, which `T-024-14` found it couldn't.
  It runs in four segments (the router takes at most four): `sim-pair-in` (publisher A, bank to A's node), `sim-pair-ramp-a`
  (A's node onto the public node `sim-pair-x`), `sim-pair-ramp-b` (publisher B, `sim-pair-x` into B's node) and
  `sim-pair-out` (B's reserve, B's node to its bank). The ramp's tuples join by inner node on both ends, and a ramp's
  destination may be its publisher's private node; the catalogue and router needed no change. The order completes, each
  node is replayed once, as the value leaves it (A at the ramp onto `sim-pair-x`, B at the final deposit onto the bank's
  rail), each against its own movement out and from its own publisher's records, which differ. A scripted mismatch in B's
  record of the deposit is recorded against B alone, the first node agreeing; one in A's record is recorded against A.
- `REQ-CAT-22`: every inner node on the chosen route of the join and pair scenarios is on one list or the other; a
  reserve whose node is on neither (`sim-ghost`) has both tuples left out of the snapshot as `NodeNotListed` and no
  route.

**Added to the simulator** (`conformance/src/`): `Shape` gains `outer_private`, `rail_private`, `rail_unserved` and
`undeclared_node`, with their names in `shape_of` (`sim-pair-*`, `sim-join-out-private`, `sim-join-out-silent`,
`sim-ghost`) and the matching answers of `Spec::public_nodes`, `private_nodes` and `networks`. A deposit out to a rail
its reserve declares private posts its movement on the public node it leaves (`Simulated::answer`), where
`movements_made` is unchanged. **Added to the assembly** (`src/assembly/audit.rs`): `Core::holding_reading` and
`Core::check_holding`, below.

**Defaults taken** (reversible, fail closed).
- `AC-29`'s "one conversion that changes no value" is the exchange at a rate of one (COP to USD, the amount out equal to
  the amount in); no second exchange was added.
- `AC-34`'s "public movement into [the private node]" is the movement of the deposit's transaction on the public node it
  starts at, since no public network records a rail declared private; the sim posts it there.
- The private node's balance at `AC-39` has no simulated holding module (`T-024-17` built the operator's plain wallet,
  which serves public nodes only), so the private reading is built from the wallet's, with the node, the account and
  the publisher's word changed, as its publisher would state it.
- The log keeps one `holding_check` record for equal checks (the entry is idempotent), so a check repeated with the
  same account and orders adds only what differs: the raise.

**Gaps, with owners.**
1. *Core never reads a holding account's balance or calls the comparison.* `core-audit`'s `Compare` is built and tested
   (`T-022-05`) and has no caller in `core-orch` or the assembly, so `AC-39` is shown over the pieces: the assembly's
   `holding_reading` and `check_holding` (the production caller's two steps, on Core's own log handle and operator
   channel) are the scenario's. Owner: `F-030`'s assembly (the periodic check), with `F-020` for the reading of a
   private node's balance from its publisher.
2. *Nothing lands value in a holding account where a segment settles at a public node* (`T-024-17`'s gap 1): the
   scenario lands it with `Core::land`. Same owners.
3. *`T-020-32`'s part.* No test of this row needs the publisher's record appended as a stated entry or cited by the
   replay, so none is ignored: the replays are shown by what they found and the movement they name. `T-020-32`'s
   `replay cites the receipt of each inner-segments record it read` will hold the cited receipts for both nodes of the
   pair route (`rests_on` as a list, `T-020-30`).
4. *The router's four hops* bound a route across two private nodes to the shape above (a public node between, the
   second node ending in a deposit). A route with a segment inside each node, or a third node, needs `max_hops` raised
   (`kit.rs`, a deployment setting).

**Rule-off table** (each switched off alone, file restored, the tests green first; run under a hard timeout):

| Switched off | Red |
|---|---|
| Closing's replay on the way out (`exits`) | the two-node close, the two-node mismatch, the match and mismatch test, the route ending inside a node |
| Closing's private rail closing (`can_close`) | `AC-34`, the route ending inside a node |
| The movement into the rail naming the node | `AC-34`, the route ending inside a node |
| The publisher label on segments inside a node | `AC-29`, the match and mismatch test |
| The catalogue leaving out a node on neither list | `REQ-CAT-22` |
| The holding difference raised (`core-audit`) | `AC-39` |
| The publisher's scripted misrecording | both mismatch tests |
| A ramp's private destination (`outer_private`) | the two-node close, the two-node mismatch, `REQ-CAT-22` |
| A deposit's movement into a private rail (`rail_private`) | `AC-34`, the route ending inside a node |
| A rail with no public record (`rail_unserved`) | `AC-34` (the silent reserve is routed to) |
| A node on neither list (`undeclared_node`) | `REQ-CAT-22` |

The four edits to `components/` were temporary, restored after each run; none hung (hard timeout 240 s).

## The toolchain is pinned (`T-020-11d`)

The root workspace builds on the toolchain `rust-toolchain.toml` names, Rust 1.98.0 with clippy, rustfmt and the
`riscv32imac-unknown-none-elf` and `wasm32-unknown-unknown` targets, and CI's `ci` job installs that same version
(`.github/workflows/ci.yml`, both `dtolnay/rust-toolchain` steps); the workflows that run cargo with no toolchain step
of their own (`licences`, `records`, `Contract guard`) take it from the file. CI used to install the floating
`stable`, and its move to 1.100.0 turned `m3` red twice with no change of ours: cargo's new build layout moved the
tests' executables (CI run 43287638222, fixed for both layouts by `T-020-11c`), and rustfmt 1.100 wraps long
`.all(...)` chains that 1.99 leaves alone (run 43305310485, `contracts/src/core_internal/types.rs` and
`contracts/src/module_interface/types.rs`). Reformatting for one version breaks `cargo fmt --check` on the others.
1.98.0 is the version every local chain of `M3` ran on, and CI passed on 1.99.1 with the same formatting.

**Locally**, rustup reads the file and looks for a toolchain named `1.98.0`, installing it if it can. Where the
`stable` toolchain installed is 1.98.0 already, with both targets, `RUSTUP_TOOLCHAIN=stable` overrides the file and
runs the same version with no download (`rustc --version` reads 1.98.0 under it).

**Moving the toolchain is a deliberate change of its own**: one commit that changes `rust-toolchain.toml` and the
two `toolchain:` inputs in `ci.yml` together (a `DEC-20` path), reformats with the new rustfmt, and runs the full
chain on the new version, so a toolchain release never turns a branch red by itself. The sub-workspaces under
`components/stm/vos-program/` and `experiments/` pin their own toolchains and move separately.


## T-024-09: the requirement-to-evidence table

Readings taken without asking, each reversible; the owner's form overrules any of them.

### What was built

- **`evidence-table`**, a binary of `rutent-records` (`tools/records/src/bin/evidence-table.rs`, over
  `tools/records/src/evidence.rs`), `std` only. It reads `conformance/evidence/v0-scope`, the `// verifies:` tags
  (the tag check's own reader, over every `.rs` file under the root) and a results file, and prints one line per
  row with its class, status and the tests, check or owner behind it, then the findings.
- **Three modes.** Report (the default) prints and exits 0. `--strict` fails on any finding, whatever the file's
  switch says. `--ci` is the one CI step that reads the switch: with `strict: on` it is `--strict`; with
  `strict: off` it reports the table's own findings and still fails on the comparison with `develop` (below).
  Exit 2 is "could not read the records", never a pass.
- **The scope file's format** (the header of the file says it too): `ID | class | citations | note`, one
  `strict: on|off` line, `#` comments. Classes: `m3` (a passing tagged test), `partly` (`form:<item>`), `inspect`
  (`check:documentation|boundary|licence`, or `review:F-0NN` whose `features/F-0NN-*/notes.md` exists), `m4` (`F-0NN`
  and `7.1:<family>`), `deferred` (`DEC-n`, or `ASM-n`, or `F-0NN` with `7.1:<family>`). An `m3` row may cite
  `later-part:F-0NN`, the owner of a half that comes later.
- **The 232 rows** are the identifiers `SPEC.md` defines in `REQ-`, `INV-`, `AC-`, `NFR-` and `GUA-`, less the
  withdrawn ones (the tag check's reading of withdrawal). Placed as the row says: `REQ-OPS-4`, `REQ-PRT-2`,
  `REQ-PRT-4` `m4` under `F-030`; `REQ-QTE-16`, `REQ-ORD-42`, `AC-38` deferred by `DEC-8`; `REQ-CPL-1`, `-4`, `-7`,
  `-10` and `AC-16` by `DEC-16`; `REQ-PRT-3` and `GUA-6` by `DEC-17`; `REQ-EXT-1` by `F-052` and §7.1; `NFR-1` by
  `ASM-1` and §6.2 risk 9; `REQ-CAT-5` partly met on `cat5-timing-residual`; `REQ-MOD-2` and `NFR-7` by the
  documentation check, `REQ-MOD-3` by the boundary check, `REQ-MOD-5` by the licence check, `REQ-EXT-2` by `F-011`'s
  notes. `REQ-AUD-10`, `REQ-AUD-12` and `AC-25` are `m3` with their `M4` owners (`F-033`, `F-033`, `F-030`) and
  `REQ-CPL-12` is `m3` with its verifier half's owner, `F-053`. Every other row is `m3`.

### How CI hands the table its results (the reading the plan leaves to the form)

- The Test step now runs `cargo test --workspace --no-fail-fast 2>&1 | tee $RUNNER_TEMP/test-results.txt`, with
  `shell: bash` so that the pipe keeps `-o pipefail` and a red test still fails the step (the default shell has no
  pipefail). `--no-fail-fast` makes every crate's tests run, so a verifier in a later crate is not left unrun by an
  earlier failure.
- **Results are read from libtest's lines** (`test <path> ... ok|FAILED|ignored`, with the reason after `ignored,`
  allowed; stable has no JSON output), each kept with the `Running <target> (...)` header above it, and the file is
  read lossily, since a jailed test can print bytes that are not UTF-8. A scanned test is joined to its **target and
  full path**: an integration test to its `tests/x.rs` and its exact path; a unit test to a `Running unittests`
  binary whose stem is its own crate's lib or bin name (from the nearest `Cargo.toml`) and the exact path
  `<file module>::<inline mods>::name` (`src/shape.rs` is `shape::`, a `mod tests { }` adds `tests::`, a top-level
  test of `src/lib.rs` is the bare name), so a crate that never compiled is vouched for by no other. It passes only if
  the run read at least as many lines for that place and name as the sources hold tests of it, and every one passed.
  So a test that failed, was ignored, never ran, or has a namesake in its own target that never ran does not pass,
  and a namesake in another target (a crate's test of the same name and path) vouches for nothing. Two crates that
  both have a `tests/boundary.rs` with a test of one name still share a place; they must both pass. The header
  doesn't name the package, so that is as fine as the join gets. Doc tests and tests in the separate workspaces print
  no line the root run joins, so a verifier there is not counted until a later row adds its own results.
- **The scan covers the root workspace's tests only**: it skips `experiments/` and every directory whose
  `Cargo.toml` has `[workspace]` (the module guests, the VOS twin, the wallet guest). The tag check still walks
  everything, since every test must be tagged.
- The two new steps are in the `ci` job after Test: report mode (`--results ... --base .evidence-base`, appended to
  the step summary) and the `--ci` run. Both read `develop`, checked out into `.evidence-base`, a hidden directory so
  that the walk over the tests skips it (its tests must never count as the head's verifiers).

### The ratchet against `develop` (the `evidence-scope-gate` item)

- **The default of the form item is applied: ratchet only.** The file is not added to `CODEOWNERS` as a path of its
  own (`.github/` is already guarded, and `PLAN.md` §4.4 gives `T-025-05` the review of the file), so no edit of
  the file needs the owner's click at the release into `main`. What guards it is the comparison below, which covers
  the moves the row names (the switch, and `m3` to `m4` or `deferred`) and no others; see the gaps.
- **The comparison fails whatever the switch says.** A head that reads `strict: off` against a `develop` that reads
  `on` fails; so does an identifier `develop` classes as `m3`, `partly` or `inspect` and the head classes as `m4` or
  `deferred`, unless its new entry cites a row of the plan that this change made to place it there. Were the
  comparison to wait on the switch, it could never fire, since it only matters while the head reads off.
- **"Changed to place it"** is read as: a clause (split at `;` and `|`) of the cited row names the identifier and,
  for `M4`, says `M4`, and no such clause was in `develop`'s row. For a deferral the cited decision is a `DEC-n` row;
  for `M4` both the §7.1 family row (`7.1:REQ-OPS`) and the feature's row (which must say `M4`) must be cited and
  must each do it. An edit elsewhere in a row that already placed the identifier excuses nothing; a row that names
  it only under `M3` places it nowhere new.
- **When `develop` has no scope file yet, the comparison is skipped and says so.** `--base` pointing at a checkout
  whose `SPEC.md` or `PLAN.md` can't be read is an error (exit 2), not a skip, so a failed checkout of `develop`
  fails the step.
- **A row removed from the file** is not a reclassification; the table reports it as `missing-id`.

### Defaults, each failing closed

- A scope file with no `strict:` line reads off, and the table reports it.
- With no results file nothing counts as passed, and the table says `no-results`.
- A review (`review:F-0NN`) counts only if the feature's notes exist; a check counts only if it is one of the two
  named. **The tool does not run those checks**; the documentation, boundary and licence checks are CI steps of their
  own, and an inspection row is "verified" by naming them, as the row says.
- A `deferred` row needs one `DEC-`, `ASM-` or `F-` identifier the plan or the specification defines. It does not
  check that the cited decision's text names the identifier (the ratchet checks that only on a move).
- `partly` rows are counted apart from verified ones, and need a `form:<item>`; the item's existence on the form is
  not checked (the form is outside this repository).

### Gaps (with owners)

- **`T-024-10`** sets `strict: on` and gets the first passing run: the real table, with no results, has every `m3`
  row unverified, since the scenarios that tag most `AC-*` and `INV-*` land in `T-024-03` to `T-024-08`. This row's
  test of the real file checks its structure only (every identifier placed, citations well formed).
- **`REQ-CAT-*` and `F-031`.** §7.1 puts `M4: F-031` in the `REQ-CAT` family and `F-031` serves `REQ-CAT-2`,
  `REQ-CAT-22`, `REQ-MDL-1`, but the row names only `REQ-OPS-4`, `REQ-PRT-2` and `REQ-PRT-4` as `M4`, and `M2`
  places `REQ-CAT-22` in `F-012`. They are `m3` here, so the table asks for a verifier of each. **Owner: the
  orchestrating session**, to say if any is `M4`'s.
- **`NFR-8`** is `m3` (§7.1: `F-012`), though `F-033` also serves it. Same owner.
- **The `M4` halves** (`REQ-AUD-10`, `REQ-AUD-12`, `AC-25`) are carried as `later-part:` citations, as the note for
  `F-032` the plan asks for; nothing reads them back. Owner: `F-032`.
- **A deferral excused by §7.1 alone** (`REQ-EXT-1`'s shape) isn't an excuse for a move: the ratchet takes only a
  `DEC-n` for a deferral, so moving an `m3` row to `deferred` by a §7.1 or `ASM-n` change fails until a decision
  carries it. Fail-closed; the owner of the plan can add the decision.
- **Moves into `partly` and `inspect` are compared too** (review of `cfce706`). An `m3` row that `develop` has and
  the head has as `inspect` or `partly` is `reclassified` unless a cited row (`DEC-`, `F-` or `T-`) newly places the
  id in a clause saying `inspect` or `partly`. Besides, **`PLAN.md`'s `T-024-09` row decides which ids may be in those
  classes**: inspection for the ids in the parentheses after "verified by inspection", partly met for those after
  "partly met with the form item it waits on". They are read from the row (a row reworded out of recognition allows
  nothing), and a test ties the real row to `REQ-MOD-2`, `REQ-MOD-3`, `REQ-MOD-5`, `NFR-7`, `REQ-EXT-2` and
  `REQ-CAT-5`. A `form:` item must be one of the ids in `decisions/questions.json` when the repository holds it; it
  doesn't (the form is untracked and lives outside this repository), so the check is skipped and the report says
  "form items not checked". A plan edit cannot widen those sets: every id the head's row allows and `develop`'s does
  not is `allowance-widened`, always, and the `T-024-09` row excuses no move (only `DEC-` and `F-` rows do). A
  `review:` notes file must not be empty. A deferral is excused only by a clause that
  says "defer".
- **The comparison starts once the scope file is on `develop`**, as the row says: until then `T-024-10`'s
  restriction to the `strict:` line and `T-025-05`'s review cover it.

### Run on this branch's tests

`evidence-table --ci --results <the chain's cargo test output>` reads 1509 passing test lines and prints 232 rows:
184 verified, 5 inspected, 1 partly met, 27 unverified (no test is tagged with them yet; the scenario rows
`T-024-03` to `T-024-08` tag most of them), 3 M4's and 12 deferred. The figures are the same after the join by target
and path (review of `cfce706`); the `N of M tests passed` details moved, since `M` no longer counts the
`experiments/` twins. It exits 0 with the switch off and 0 findings against the base. The binary's test of the switch runs on directories of its own, so it reads the same whether the
real file says `off` or `on` when `T-024-10` flips it.

### Rule-off table (each rule switched off once; the test that went red)

| Rule switched off | Red |
|---|---|
| identifier of `SPEC.md` in no row | `scope file missing a spec identifier is reported` |
| `m3` row with no passing verifier | `m3 row whose only tests failed is reported` |
| `m3` row with no tagged test | `m3 row with no tagged test is reported` |
| deferred row without its decision | `deferred row without its plan decision is reported` |
| partly met row without its form item | `partly met row without its form item is reported` |
| inspection row without a check or review | `inspection row naming no check or review is reported` |
| strict switch turned off against `develop` | `strict switch turned off after develop has it on fails`; the binary's test against a base |
| reclassification reported | `m3 row reclassified against develop is reported`, and the two excuse tests |
| the excuse needs a placement `develop`'s row lacked | `unrelated edit to a row naming the id does not excuse a reclassification` |
| an `M4` move needs both cited rows | `reclassification is excused only by a changed placement that it cites` |
| a tagged test's namesake that never ran | `tagged test with a namesake that never ran is not a verifier` |
| a namesake in another target vouches for nothing | `two tests of one name in different targets cannot vouch for each other`; `results are joined by target and full path and fail closed` |
| the scan skips `experiments/` and other workspaces | `only the tests the root workspace runs are scanned` |
| inspection and partly met only for the ids the plan names | `inspection and partly met are allowed only for the ids the plan names` |
| a move into inspect or partly met is a reclassification | `m3 row moved to inspected or partly met is reported unless a changed row places it` |
| a form item the decisions file lacks | `a form item the decisions file lacks is reported`; the binary's test of the decisions file |
| a review's notes must not be empty | the same binary test |
| unit tests joined by binary and exact module path | `a unit test of a crate that never compiled is vouched for by no test elsewhere`; `inline modules are part of a unit tests path` |
| the allowed sets only narrow | `a plan edit that widens the allowed sets is reported and excuses its own move` |
| a deferral needs a clause that defers | `a mention that does not defer the id does not excuse a deferral` |
| `--ci` with the switch on is strict | `the ci run is strict where the scope file reads on` |
| `ignored, <reason>` is ignored | `an ignored test with a reason is read as ignored` |
| a results file is read lossily | `results with bytes that are not utf8 are read lossily` |
| results fail closed on a count short of the sources' | `tagged test with a namesake that never ran is not a verifier`; `results are joined by target and full path and fail closed` |
| only `M3`'s classes are held to the ratchet | `m3 row reclassified against develop is reported` |
| `--ci` with the switch off fails on the table's findings | `the strict flag always runs strictly and the ci run follows the switch` |

## Rows added after review (`plan/m3-owed-rows`)

- `T-024-03`'s gaps now have owners: Core's own charge check, with the assembly's stand-in deleted, is `T-020-14`;
  signed terms and holds in intake is `T-020-15`; the evidence floor as a logged change stays on the owner's form,
  with no row; the verifier half of `AC-24` is `T-024-12`.
- New `F-024` rows: `T-024-11` (`REQ-CPL-9` through the assembly with an agent acting), `T-024-12` (the verifier
  half of `REQ-CPL-12`, and `AC-24`'s) and `T-024-13` (a gross, charged segment end to end, with the closing's
  public-movement figures at each end) and `T-024-14` (simulated publishers emit the inner-segment record that
  `T-020-18` adds, and the assembly's `Books` reads it).
- `AC-2`'s full route across a reserve, an exchange and a ramp is `T-024-04`'s: the simulated providers that join
  the reserves are added there (see the world paragraph of `T-024-03`).
- `T-024-10` and `T-025-05` now run in wave 27, after all of them.

## T-024-06: failure, stuck, reversal and ending scenarios

Readings taken without asking, each reversible; the owner's form overrules any of them.

**Tests** (`conformance/scenarios/tests/`, each carrying exactly the `AC-*` of its plan bracket): `failure.rs` (`AC-42`,
`AC-12`, `AC-40`, `AC-5`), `ending.rs` (`AC-7`, `AC-8`, `AC-9`, `AC-41`) and `reversing.rs` (`AC-4`, `AC-6`), with
`failure_support/` beside `execution_support/`. Each drives Core's own `request`, `quote`, `accept` and `run`, and the
methods below, over the simulated providers.

**Added to the assembly** (`conformance/scenarios/src/assembly/lifecycle.rs`, nothing in `components/`): Core's own
records of what goes wrong (`Recalculations`, `Stuck`, `Reversals`, `Notices`) held beside the segments, with `recalculate`,
`run_recalculated`, `watch`, `record_no_route`, `decide_stuck`, `carry_out_stuck`, `owe_reversal`, `ask_reversal`,
`confirm_reversal`, `end`, `returned`, `late_delivery`, `notify` and `notices_to`; `Core::run` split into `run_live`;
`Faults::operation_takes` (a segment takes seconds of Core's clock, so p99 has a history); `reprice_deposits` (a price that
moved after the quote) and `set_check_bound` (deployment settings). No simulator change was needed.

**Defaults taken.**
- `AC-5`: "stops answering" is the provider that took the deposit and never answered (`Behaviour::late`), so its outcome
  is unknown and its value held; the p99 is a real history of twenty settled orders at 10 s an operation. "The operator
  is warned" is the operator's notice of the `stuck_declared` change; "the provider can see it" is `seen_by_provider`.
  A late delivery is the provider's answer asked again under its own transaction, kept by `Reversals::later`.
- `AC-12`: the reserve's price moves after the quote (`reprice_deposits`), so the only route from the ledger falls below
  the floor.
- `AC-40`: the recalculation to the same destination is the same reserve again; with its dry run refused, the order
  waits. After `no_route` Core's recalculations answer `NoRouteLeft` (routed no more) where the kit's answered `Held`;
  both mean no route starts. The customer's new destination and the hand-on are not run here (below).
- `AC-7`: the second half runs on an order whose deposit was refused (first segment settled, order open) and on a complete one.
- `AC-9`: the destination's report is a signed deposit result that says the payment came back.
- `AC-41`: the expected changes are the inputs the state machine applies to the order, replayed from the log.
- Policy's stuck rules are the real engine's and the resolution's move is Core's own mover (`T-020-16`, merged).
- `AC-5`'s late delivery is folded into the plan-named test, so the name's promise holds.

**Gaps, with owners.**
- **`AC-4` and `AC-6` are written and `#[ignore]`d.** The reversal journal's receipt rule (`T-020-12`) is fixed on
  `m3-next` and `AC-8` and `AC-5`'s late delivery now run. What still blocks the two: the simulated world binds no
  private node to its publisher, so a failed exchange's own answer completes nothing (`FailedProvidersOwn`) and the
  reserve's confirmation is refused as the same publisher's. Owner: `T-024-14`, whose row now says so (the simulated world binds each
  private node to its publisher, the two `#[ignore]`s come off, and the rule-offs run). The "other party" half of `AC-4` has no simulated route at all: a private
  node is its publisher's alone, so no other publisher's provider starts where the origin reserve's node is; it is held
  in `components/core-orch/tests/reversal.rs`. Drop the `#[ignore]` when the node is bound. Evidence-table rows `AC-4`
  and `AC-6` stay unverified until then.
- **`AC-40`'s second half** (customer's new destination, hand-on to a linked order) runs on the kit only
  (`tests/hand_on.rs`): the assembly doesn't wire `hand_on`. Owner: a later `F-024` row.
- **`AC-5`'s `Behaviour::late`** waits the fleet's real deadline; the order is accepted before it is scripted, so the quote's calls don't run under it. The stale-guest builds behind the earlier `NoRouteRightNow` flake are fixed on `m3-next`.
- The customer's session lasts about a quarter hour of Core's clock, which bounds how many slow orders one scenario runs.

**Rule-off table** (each rule switched off in `components/` or `contracts/` for one run, with other scenarios' rules off in
the same run where each red names its own cause, and restored; the tests were green first). `AC-4` and `AC-6` could not be run.

| Scenario | Switched off | Red |
|---|---|---|
| `AC-42` | the floor check at a settled segment (`segments/mod.rs`, `holds_floor`) | the deposit is handed over after the short withdrawal |
| `AC-12` | the router's `BelowFloor` drop | the recalculation is refused by Core's own check instead of waiting |
| `AC-40` | the rule that places a part with no route at a failed deposit's node (`after_no_route`) | stuck, not waiting for the customer |
| `AC-5` | the stuck clock's twice (`over_time`, four times) | not stuck past twice the p99 |
| `AC-5` (late half) | recording the late statement (`Reversals::later` appends nothing) | the late delivery is not kept |
| `AC-7` | the state machine's end only before the first segment settles (`after_end_requested`) | the order past its first segment ends |
| `AC-8` | the state machine's failing a part whose start is proven undone | the order stays complete |
| `AC-9` | the destination check on a return (`makers.contains`) | another provider's report is recorded as the return |
| `AC-41` | the operator among an order's receivers (`receivers_of`) | `AC-41`, and the operator's half of `AC-7` and `AC-9` |

**Update (`T-020-32`):** the two `#[ignore]`s are gone and both tests pass; their rule-offs (`AC-4`: the node's binding, `SamePublisher`; `AC-6`: the two-actor rule in Core, policy and the log) are in `F-020`'s notes, `T-020-32`. `T-020-32` also edited `grading.rs` (`Books` returns the publisher's record whole), an `F-024` file.

`AC-6`'s second rule is held by two layers (Core's `SameParty`/`SameLogin` and policy's `one_actor`): both off together
is what the table would switch off; neither alone turns the scenario red. (Untested: `AC-6` is ignored.) Once un-ignored it is rule-offed in both layers together, then each alone. Both people hold both permissions, so
the refusals are the two-actor rule's (`SameParty` from Core's move step, `SameActor` from policy once Core's is off), not a missing permission.

## Owed rows (plan/m3-owed-rows-2)

- **`T-024-15`** owns the stand-in reserve's arrival-notice test, `stand in reserve signs an arrival notice that
  passes the role suite`, which `T-020-18`'s row named and nobody wrote (`conformance/src/role/stand_in.rs`,
  `suite.rs`). The test moved from that row's list to this one; issue #150, `T-020-18`'s, stays open until it
  lands. Wave 26, after `T-024-13`, which edits the same stand-in.
- **`T-024-16`** owns `T-024-09`'s two low gaps in `tools/records/`: a test whose attribute is not first on its
  line is not found by the tag check, and a fixture crate the root manifest excludes but that has no `[workspace]`
  line of its own would be counted as run by the evidence table.

## T-024-15: the stand-in reserve states what arrived

Branch `f-024/t-024-15-reserve-kind-held`, from `m3` at `fa7fc3f3`. Readings taken without asking, each reversible.
Refs #171.

**Tests** (`conformance/tests/role_arrivals.rs`): the three the row names.
- `stand_in_reserve_signs_an_arrival_notice_that_passes_the_role_suite` (`REQ-ORD-14`, `REQ-ORD-23`): the suite credits the
  reserve's bank, asks what arrived, and passes with every signature verified; the notice is in `ARRIVAL_NOTICE_SHAPE`, made
  by the reserve, with the credit's node, asset, amount, account and holder; four flawed reserves are flagged.
- `stand_in_reserve_answers_the_arrival_question_under_the_questions_transaction` (`REQ-ORD-1`): before any credit, nothing
  arrived; after one, its notice under the credit's reference inside an answer under the question's transaction;
  `check_answer` holds; asked of another account, nothing arrived.
- `two_notices_of_one_credit_from_the_stand_in_reserve_are_one_credit` (`REQ-ORD-23`): the notice of its own accord and the
  one the answer carries are `Credits::One`; another credit of the same figures is `Credits::Two`; two flawed reserves are
  flagged.

**Added** (`F-024`'s files only): `RoleProvider::arrival` and `arrival_notice` (default `None`) and `RoleHarness::credit_arrival`
(default false), with `BankCredit`, the credit at the simulated bank: reference, outer node, asset with unit, amount, account
and holder. The reserve keeps its credits and states each as a notice; node, asset, amount, account and holder are the
credit's, not the scenario's. The role suite gains `Check::ArrivalsStated` and `Exercised::{NothingArrived, ArrivalNoticed}`
and `probe_arrivals`, run for a reserve: nothing arrived before a credit; after one the answer's envelope, its maker and key,
each notice as text of its own in the answer received, `check_answer`, the notice's figures against the credit, one notice
per credit, two asks and the reserve's own notice are `Credits::One`; and no notice of another account's credit, to another
session, or to a question not signed with the operator's key. `Signing`, `WithoutHooks`, the scenarios' `InProcess` and
`Through` forward the new methods.

**Defaults taken.** The question is answered in the holder's session only, and a question not signed with the operator's key
is refused finally (as a balance check is, `REQ-CAT-3`, `REQ-CAT-4`). A credit's reference is the notice's transaction,
whenever signed. `NothingArrived` is required of a reserve only once it answers the question at all, so a provider that
cannot be asked is not made uncovered (the role suite's other tests pass unchanged); `ArrivalNoticed` is not required, since
only a hook provokes it. The arrival question's session fault is its own builder, so `ignoring_the_session` still flags
only `BalanceRevealsOnlySufficiency`.

**Gaps, with owners.**
- The bank's credits are left out of the stand-in's state fingerprint: they arrive by the harness's hook, which no journal holds,
  so `Through`'s check that the journal is the books would otherwise differ. Asking changes no state the suite can see
  there; the reserve's counter of notices is likewise outside it. Owner `F-024`.
- The platform's binding carries operations, and the arrival question is not one, so a simulated reserve through it is
  answered by its in-process twin, as its status is, and a reserve that does not answer the question is not flagged by the
  suite: owner `F-014` (the binding) and `F-031` (certification's first report). No Core bug found.
- `check_one_credit` and the suite's own comparison of a notice with its credit overlap (a fresh reference or another amount
  is caught by both); each rule-off alone stays red elsewhere, both together red.

**Rule-off table** (each switched off alone, `role_arrivals` run with `--no-fail-fast`, restored; tests were green first;
nothing hung; the stand-in was written before the test file compiled, so "red first" is the rows below).

| Switched off | Red |
|---|---|
| the answer's filter by account, node and asset | the answering test and the suite test |
| the answer under the question's transaction | the same two |
| a notice under the credit's reference | all three |
| the holder's-session check | the suite test |
| the operator-key check on the question | the suite test |
| the suite's `check_answer` | the suite test |
| `check_one_credit` and the suite's notice-against-credit check, together | the two-notices test (alone, each is caught by the other) |
| `credit_arrival` accepting a credit | all three |

## T-024-11: `REQ-CPL-9` through the assembly, with an agent acting

Readings taken without asking, each reversible; the owner's form overrules any of them.

**Tests** (`conformance/scenarios/tests/presence.rs`): the three the row names. In each, the customer's agent (its party
the customer's, its login `agent-1`, kind `Agent`) is listed in `Registers` and scoped to what the order takes
(`OpenSession`, `Authorise`, `Charge(Request)`, `Charge(FanOut)`, and no `AskAboutPerson`); it opens the session, requests,
quotes and accepts; the reserve is scripted to demand a person's presence on its withdrawal; Core runs the order.
- `presence demanded of an agent through the assembly is reported as the reserve's outcome`: the segment's outcome is
  `WaitingOnThePerson`; the statement is the reserve's (maker, and the log's attribution of the stated entry Core
  reported); its signed body states a refusal of class `waits_on_person`; the reserve moved nothing; the order is
  neither complete nor failed; and Core's only input after the route is the segment's start (`provider_bound`).
- `presence demanded of a person gives the same outcome as of an agent`: the same function with the customer's person
  acting, compared field by field on what matters: the reserve's statement (maker, signed body, attribution), the
  order's state, what the reserve's book holds, every input Core took after the route, and the number of decisions.
  The person's run is checked to be a real demand, so two runs that stopped short alike don't pass.
- `core takes no step after a reserve's demand of presence`, run with each actor: the segment's start is Core's last
  input; no decision follows the move's, whose decision comes before its effect; and no input and no decision is
  recorded after the reserve's statement Core reported.

**Added.** `Fault::WaitsOnPerson { provider, kind }` and `harness::plant(core, &fault)` (`conformance/scenarios/src/
harness.rs`), which `Step::Plant` now calls, so a test that drives Core's own calls plants a fault between them. The
behaviour itself (`Act::RefuseWaitingOnThePerson`) was in the simulator already (`conformance/src/sim/`), so nothing
there changed; nothing in `components/` changed.

**Defaults taken.**
- `Core::fleet()` stays a view. The row asks it to lend the fleet to the script; `Core::world()` (`T-024-04`) already
  lends the simulated world for scripting, and the fault goes through it, so the fleet's grants stay unreachable from
  a test (the two compile-fail doc tests hold it). `T-021-07`'s note that the fleet is "lent only shared" was about
  `fleet()` and is met by `world()`.
- The agent's scope is the four calls above. `AskAboutPerson` is left out: the order asks no one about a person, and
  an agent can't be told from a person on this path by anything Core passes the reserve.
- `AC-28` is the scenario's accepted criterion (the base order's shape); `presence.rs` tests `REQ-*` rows, which the
  harness's `Scenario::new` takes only through an `AC-*`.
- The statement compared is the signed body the reserve stated (`bytes`), not the log index, which differs by who asked
  (the agent's run records the same calls, but a log index is an artefact of the order of records).

**Gaps, with owners.**
- The decision clause ("no decision follows the move's") has no rule-off: the only way to make Core decide after a
  demand is a change in `components/core-orch`'s segments that no rule asks for, and the entries' order is asserted on
  the log's real indices. Owner: none needed; `T-024-06`'s stuck path is where a decision after a demand could arise.
- What an agent may then do at the reserve (approve, be refused) is the reserve's own and isn't Core's to test:
  `REQ-CPL-9` is closed by Core's reporting only. Owner: the reserve's own conformance (`F-053`).
- The comparison ignores the order's amount by design (the demand's statement holds none), so a run for another amount
  gives the same outcome and that is no finding.

**Rule-off table** (each switched off alone, under `alarm 400`, restored; the tests were green first, except that the
first two were written passing, the code being there):

| Switched off | Red |
|---|---|
| the segments' `WaitsOnPerson` arm (falls to the generic refusal, failed) | all three tests |
| Core takes a `failed` input after the demand (the arm also records one) | all three; the first version of the first test stayed green on it (its list of kinds named none that exists), so it now asserts the inputs equal `provider_bound` alone |
| the demand reported under another maker | the first and the second test (the third is about steps, and stays green) |
| the agent's `Authorise` scope | all three (a step of the agent's order is refused, so the order never runs) |
| the agent listed in `Registers` | all three (a step of the agent's order is refused, so the order never runs) |
| the order's amount differing between agent and person | none, by design (above) |

## T-024-16: two reads of `rutent-records` made exact

Readings taken without asking, each reversible; the owner's form overrules any of them.

**Tag check** (`tools/records/src/tags.rs`): `scan_tests` now finds a test attribute anywhere in a line's code (strings
and `//` comments cut out first), so `mod t { #[test] fn x() {} }`, `#[allow(..)] #[test] fn x`, `#[cfg(test)] mod t {
#[test] fn x }` and two tests on one line are found, each once per attribute, as the multi-line form already was. The
module path is read from the braces as the text is walked (one stack entry per `{`, the segment before it saying
whether it opens a `mod`, with leading attributes ignored), so a test on the line that opens its module is in that
module. A line is an "attribute line" (what may stand between a tag and its test) only when nothing but attributes is
on it: otherwise `// verifies: A` above `#[test] fn a() {}` would also tag the `#[test] fn b() {}` under it.

**Table scan** (`tools/records/src/evidence.rs`): `find_member_rust_files` also skips every directory the root
manifest's `[workspace]` table `exclude`s (single-line or multi-line list, a trailing `/` or `./` ignored, comments cut),
matched by the whole path under the root; basic and literal strings are both read. Defaults: only the `[workspace]`
table's own `exclude` counts (another table's does not); a name match elsewhere does not skip a member of the same
name; and an `exclude` the reader cannot fully parse (the inline form `workspace = { .. }`, a dotted `workspace.`
key, a `[workspace.exclude]` table, a value that is not an array of single-line strings, an escape, a list never
closed) is an `InvalidData` error from the walk, so the table's run fails loud rather than counting an excluded
crate as run. The tag check's walk (`find_rust_files`) is unchanged: an excluded crate's tests still need their tags.

**Tests**: `test whose attribute follows other code on its line is found and needs its tag` and `tests of an excluded
crate still need their tags` (`tests/records.rs`), `crate the root manifest excludes is not counted as run by the
table` (`tests/evidence.rs`). The repository still has no untagged test under the stricter read, and no tag needed
changing.

**Known noise, left**: block comments (`/* #[test] */`) and raw strings with an inner quote can still read as a
spurious test. That demands a tag nobody needs; it never hides a test.

**Rule-off table** (each switched off alone, restored):

| Switched off | Red |
|---|---|
| the whole same-line read (`tags.rs` as at `d4006ba4`) | the same-line test |
| an attribute line is any line starting `#[` | the same-line test (the tag leaks to the next test) |
| strings and comments cut from the code read | the same-line test, `inline modules are part of a unit tests path`, `the real workspace has no untagged test` |
| the module stack | the same-line test, `inline modules are part of a unit tests path` |
| the manifest's `exclude` list | `crate the root manifest excludes is not counted as run by the table` |
| exclusion matched by directory name, not whole path | the same table test |
| `exclude` read from any `[workspace*]` table | the same table test |
| the tag check's walk skipping excluded crates | `tests of an excluded crate still need their tags` |
| `after_attributes` reading the raw line (a trailing comment, a `]` in a string) | the same-line test, in its two new cases |
| the parse errors (an unreadable `exclude` read as none) | `a root manifest exclude the reader cannot parse is an error not silence` |
| literal strings and a `#` inside a quoted path | `crate the root manifest excludes is not counted as run by the table` |


## T-024-13: a gross, charged segment through the scenarios kit

Branch `f-024/t-024-13-gross-charged-segment`, from `m3` at `d4006ba4`. Readings taken without asking, each reversible.

**Tests** (`conformance/scenarios/tests/gross_charged.rs`, over the assembled Core and `sim-reserve-gross`, a reserve whose
tuples report gross and which charges 5 000 on each movement, quoted): the five the row names, and three that keep them
honest.
- `gross_charged_segment_hands_the_next_segment_what_arrives` (`REQ-ORD-21`, `REQ-CAT-11`): the reserve states each segment
  gross (1 000 000 in, 1 000 000 out, 5 000 charge; then 995 000, 995 000, 5 000); Core records the charge by value and hands
  on 995 000 then 990 000; the deposit the reserve was asked for is 995 000, not the 1 000 000 quoted or stated. Each charge
  is `Within` its formula (a charge read off the gross figures, which differ by nothing, would be none).
- `a_shortfall_of_a_gross_segment_is_what_the_next_segment_moves_and_not_its_failure` (same tags): the withdrawal states 1%
  short; the deposit moves 985 000 (990 000 less the charge), and the order completes.
- `operators_fee_is_figured_on_what_arrives_of_a_gross_segment` (`REQ-QTE-2`): a fee of 1 000 plus 1% comes to 10 900 on the
  990 000 that arrives, asserted in the quote Core shows (`providers_out` 995 000, the last charge as its own line, `shown_out`
  979 100, `difference` 15 900), in the price reported once it ran, and in the log's `fee` records (5 000, 5 000, operator
  10 900). Not 10 950 (on what the last segment states) nor 11 000 (on what left).
- `final_deposit_closes_on_the_public_movement_at_what_arrives_the_charge_staying_with_the_reserve` (`REQ-ORD-33`): the
  bank's network holds 990 000, which is what Core settled; and its negative,
  `final_deposit_that_reaches_the_network_at_its_stated_gross_figure_does_not_close` (a network at 995 000: the deposit
  doesn't settle, the order doesn't complete).
- `withdrawals_public_movement_is_checked_at_what_entered_the_charge_included` (`REQ-ORD-33`): the network holds 1 000 000,
  the result's `amount_in`; and its negative `withdrawal_that_reaches_the_network_at_what_arrived_is_no_public_record` (a
  network at 995 000: Core rests no public movement on it; the reserve's own word is kept, graded as its word).
- `charge_above_the_formulas_maximum_on_a_gross_segment_is_flagged_by_core` (`AC-14`): `sim-reserve-gross-charging` charges
  5 007 against 5 000; both charges are `Flagged`, delivered, raised with `charged` 5 007 and `maximum` 5 000, no terms, no
  no-formula mark, before the deposit is handed over; none passed.

**Added to the simulator** (`conformance/`): `Shape::gross`; `sim::GROSS_FEE`; the names `sim-reserve-gross` and
`sim-reserve-gross-charging` in `sim::shape_of`; `Behaviour::charge_misplaced` (the network records a movement with its charge on
the wrong side, the result unchanged), which only the two negative tests use.

**Added to the assembly** (`conformance/scenarios/src/`): `Core::set_operator_fee` (the kit's bounds leave it at nothing, so no
earlier scenario could show the fee figured on anything), `Core::charged_of` (what Core noted of a settled charge), and a
fix to `Core::report_price`: it fed Core's `Priced` the net amount a segment handed on as the segment's `amount_out`, where
`Executed` takes what the result states. Equal under net, which is all `T-024-04` ran; under gross it was refused
(`Undeliverable`). It now reads the stated figure from the log's kept statement of the result. Not a Core bug.

**Defaults taken.** The reserve states a gross `amount_out` equal to what it took in. Scenarios take the label of `AC-1` (hand-on,
fee) and `AC-32` (closing), the net cases this row adds to. The operator's fee is set on `Core`, before the request, as a
deployment setting. The negative tests use a new knob rather than editing the network's record in place.

**Gaps, with owners.** None blocks the row. A withdrawal whose network record disagrees still settles at a lower grade (the
provider's word), since only a final deposit needs a public record; that is Core's rule (`T-020-09`), not changed. `T-020-29`'s
marked raise (a fee in another asset) still has no scenario: no simulated provider states one (`T-024-14`'s sim, `F-024`).

**Rule-off table** (each switched off alone with `gross_charged` run, restored; Core-side switches were temporary edits to
`components/core-orch`, restored by `git checkout`, never committed; the tests were green first; nothing hung). The tests were
written before the shapes were wired but the shapes went in first, so "red first" is shown by the first four rows.

| Switched off | Red |
|---|---|
| the reserve's tuples reporting gross | all eight |
| the gross answer stating what it took in | five: hand-on, shortfall, fee, closing, `AC-14` |
| the network recording what arrives under a gross tuple | five: hand-on, shortfall, fee, closing, `AC-14` (the order never completes) |
| `Behaviour::charge_misplaced` | the two negative tests |
| `Core::set_operator_fee` | `operators_fee_is_figured_on_what_arrives_of_a_gross_segment` |
| `report_price` reading the stated figure | the same test (`Undeliverable`) |
| Core's closing checking a deposit's movement at what arrives | six: every test that runs an order to its close, and the negative |
| Core's closing checking a withdrawal's movement at what entered | the two withdrawal tests |
| Core's hand-on of a gross segment, less its charge | seven |
| Core's charge check reading the charge | `charge_above_the_formulas_maximum_on_a_gross_segment_is_flagged_by_core` |
| the operator's fee figured on what the last segment states, not what arrives | eight |

## T-024-17: the assembled Core and the real plain wallet

Branch `f-024/t-024-17-hand-on-real-wallet`, from `m3` at `fa7fc3f3`. Edits `conformance/scenarios/` only: `src/sim/disk.rs`
(`wallet_guests`), `src/sim/fleet.rs` (`Fleet::install_wallet`, `InstalledWallet`), `src/assembly/holding.rs` (the wallet's
installation, release acceptance, landing and questions), the new `src/assembly/linked.rs` (the linked order's acceptance) and
`tests/assembly_real_wallet.rs`. `components/` and `contracts/` are untouched.

**What was built.**
1. *The real wallet on the platform.* `disk::wallet_guests()` builds `components/prov/guests/plain-wallet/build.sh` the way the
   simulated providers are built: into a directory named by a hash of its sources, one build at a time, reusing the guests'
   tool `guests()` built (`RUTENT_PLAIN_WALLET` overrides). `Fleet::install_wallet` records the wallet as a module the operator
   publishes, signs its release (maker the operator's, key the assembly's operator key) and admits and accepts it; the world
   holds its statements key. `Core::install_wallet(node, asset)` adds the node to the operator's public list and registers the
   wallet as its holding account.
2. *The release accepted in the log* (`Core::accept_release`): a decided `change_catalogue` of kind `accept_module` naming the
   release's statements key, by the operator's login (permitted to it by the register), its effect and a `done` result. From it
   the log verifies the wallet's statements as a module the operator publishes (`REQ-CAT-23`, `REQ-AUD-2`).
3. *The wallet's questions and answers* (`Core::ask_wallet_stating`, `state_by`, `state_answer`): the operator's signed question
   as its stated entry, then the platform's answer as the wallet's stated entry, claimed for the operator's party; the log
   attributes it.
4. *A linked order's acceptance* (`Core::accept_linked`): the rounds route the order from the node, so the router's record is in
   the log; then the quote shown, and the customer's own acceptance (given, in the customer's session, the original customer
   its requester) on the yes of the provider the route starts with (its answer to the rounds' dry run); the route kept as the
   order's accepted route (`Core::accepted_linked`). It starts nothing: the `route_chosen` is the hand-on's.
5. *Value landed by hand* (`Core::land`): see gap 1.

**Tests** (`tests/assembly_real_wallet.rs`; the scenario is an order over `sim-chain-in`, `sim-chain-ramp-b` and
`sim-chain-out-b`, whose deposit out of the public node `sim-chain-b` is refused its start, so the part is stuck there, the
wallet being the node's account).
- `release of an operator-published wallet is accepted in the log and its answers are verified`: before the release is accepted
  the wallet's answer is recorded and attributed to no one; after, the same kind of answer is the wallet's, for the operator's
  party, under the key the release names.
- `linked order is accepted through the assembled Core and starts at an inner node`: the acceptance, its yes, the quote shown and
  the router's record are the log's and the log admitted them; the route starts at the node; nothing of the order reached the
  state machine.
- `real plain wallet's re-serialised answer verifies under its published key`: the answer `start_handed_on` returns verifies
  under the release's key, a changed figure doesn't, and the log attributes it (and the operator's question before it).
- `stuck order of the assembled Core is handed on through the real plain wallet`: **ignored**, blocked by Core (gap 2).
- `hand on through a public nodes wallet is refused whichever provider the decision names` (added): the gap as it stands today,
  fail closed: the decide step refuses the wallet, and a provider it takes is refused by the mover before the wallet is asked
  anything.

**Defaults taken** (reversible, fail closed). The wallet's provider is `operator-wallet`, the name the kit already registers at
public nodes. The linked order's request follows the original order's last request id, which is what makes the rounds ask no
reserve's yes (a hand-on has no withdrawal; `T-020-10`). The linked order's quote is shown under a digest of the order and its
request, since no quote is priced for it. Core's own parts (the log, policy, the mover) are the real ones; nothing in
`components/` was changed.

**Gaps, with owners.**
1. *Nothing lands value in a holding account.* Where a segment settles at a public node, Core never calls `Holdings::land`; the
   scenario lands it (`Core::land`). Owner: `F-020` (segments) with `T-020-26`'s successor; the order's value is in the wallet
   only by the scenario's word today.
2. *A hand-on can't name a plain wallet.* Three rules disagree for a node held by a wallet. `stuck::within` (decide)
   requires `by` to be a provider of the part's route (`components/core-orch/src/stuck/mod.rs`, `within`); `Mover::plan_hand`
   requires `by` to be the provider of the node's holding account and the provider of the linked route's first segment; the
   wallet is a provider of no route (it advertises no tuple, and an advertisement may not name a module the operator
   publishes), and no segment kind can stand for its own withdrawal in a plan (`Route` needs two nodes, `Deposit` and
   `Withdrawal` an outer end; the router can only choose a tuple's provider). So no decision names a provider that meets all
   three, and the real wallet is never asked. Checked by experiment: with only `within`'s membership rule switched off
   (temporary edit, restored), the mover asked the real wallet, which answered, its answer was kept as an attributed stated
   entry, and the move ended `done`; the next step, `HandOn::hand_on`, refused a plan led by a wallet segment as not a route
   handed on, as the state machine's `Route` needs two nodes. The fix is Core's and wants a decision: let `by` name the node's
   holding account and the route's first provider be the starting reserve that runs it (the record then proves the start
   by the holder, not by the first provider: `proves_start`, `recover_starts`, `plan_hand`), or give the wallet a
   segment kind. Owner: `T-020-17`/`T-020-13`'s successor in `F-020`; Pablo's call on the model.
3. *The linked order's first move* is therefore not run: the row's "linked order's first move in the log" waits on gap 2.
4. *The yes of a linked order's route is the starting reserve's dry run* (the provider of the route's first segment), not the
   holding account's; if gap 2 is fixed so that the holder is also asked, the acceptance's relation reads the provider of the
   first segment (`chosen-route-rests-on-its-orders-acceptance`), so nothing changes there.

**Rule-off table** (each switched off alone, file restored, the tests green first; none hung):

| Switched off | Red |
|---|---|
| the release's result `done` (`accept_release`) | the release test, the answer test |
| the answer claimed for the operator's party | the release test, the answer test |
| the release naming the wallet's statements key | the release test, the answer test |
| the customer as the acceptance's maker (Core writes it instead) | the acceptance test |
| the linked request following the original's | the acceptance test, the refusal test, the answer test |
| value landed in the account | the answer test |

**Updated by `T-020-33`** (edits to this row's files, `assembly_real_wallet.rs` and `src/assembly/holding.rs`, with a hunk
of `src/assembly/mod.rs`): gap 1 is closed, since Core lands the value where the segment settles (`Segments::land_settled`,
called by the assembly after each run of a part) and the scenario no longer lands it (`Core::land` stays for `AC-39`);
gap 2 is closed, since a hand-on's `by` names the node's holding account and the linked route's first provider runs the
move, so the ignored test runs and passes and the refusal test is rewritten (`hand on through a public node's wallet names
the wallet and refuses any other holder`); gaps 3 and 4 still wait on the log: `T-020-33`'s gap 1 (a contract rule holds
the first move's reserve to the record's holder). `AC-39`'s gap 2 in `T-024-14`'s notes (the scenario lands with
`Core::land`) holds for the private-node scenario, where no segment settles at the wallet's node.

## T-024-08: invariants over the assembled Core

Branch `f-024/t-024-08-invariants`, from `m3` at `d0a7cf2c`. Edits `conformance/scenarios/` only: `src/harness.rs` (two changes to the
shared kit, below), the new `tests/invariants.rs` and `tests/invariants_support/mod.rs`. `components/`, `contracts/` and the
workflows are untouched.

**What was built.** The ten tests the plan names, each tagged with its row's `INV-*`/`GUA-*`/`NFR-5`, each over the same set of
seeded runs of the assembled Core and each counted from what the run left on its disk, never from what a driver did:
the log's entries as they read back, the statements the evidence store kept for the commitments those entries hold (the
operator's operations and the providers' results, so the amounts a provider stated are read from what the log commits to),
the state machine's journals replayed from the log's own inputs, and the providers' own books where an invariant is about what
a provider did. Two additions to the shared kit, both in `harness.rs` and neither changing an existing name: `Run::read` is
public (a test that drives Core's calls reads its own run), and `Run` carries `statements` (the kept statements, parsed) and
`kept` (as kept: commitment, salt, bytes). Neither is in `projection`, which is salted.

**The runs** (`invariants_support::runs`, played once for the test binary). Each run is a `Plan { seed, shape, kind }`, and
`play(plan)` replays it exactly: the seed draws Core's identifiers, the amounts, how a repeated delivery reaches the state
machine and where a swap or a crash falls. Two routes (`Base`: one reserve's withdrawal and deposit; `Joined`: a reserve, an
exchange, a ramp and the reserve the value is deposited in, four segments) and eight kinds of fault:

| Kind | What is planted | Then |
|---|---|---|
| `Clean` | none | |
| `FailsAtTheEnd` | the destination reserve fails its deposit (`Act::Fail`) | the route is recalculated and run again (`AC-40`) |
| `FailsInTheMiddle` | the exchange fails its conversion (the base route's deposit) | recalculation is refused while the value is in motion |
| `SilentAtTheEnd` | the final deposit is taken and never answered (`Behaviour::late`) | the clock passes twice the advertised time, the part is declared stuck, a recalculation and a second run are asked, the late answer is delivered twice |
| `CrashAfterRecord(n)`, `CrashBeforeEffect(n)` | `T-024-02`'s crashes between a record and its effect | none: Core is not restarted (`F-030`) |
| `Repeated` | each input reaches the state machine twice or out of order, the state machine is swapped before an effect, and on the base route each operation reaches its provider twice | |
| `Rerun` | | the route is run again after it ended |

**Seed count, and why.** Twelve pinned runs (seeds 101 to 112, one or more of every kind on each route where it means something)
play whatever the seeds say, so no invariant is ever tested over runs that never met what it guards against; then the seeds of
`RUTENT_SCENARIO_SEEDS`, `1,2,3,4` by default and CI's `11` to `15` where it sets them, each drawing its own shape and kind.
That is 16 runs by default and 17 under CI's seeds, of 1 to 10 seconds each on a debug build: the whole binary, the ten tests
together, takes about a minute on one core, and none runs for hours. More seeds would add runs of the same eight kinds, so the
count is the pinned set (coverage) plus enough drawn seeds (variety in amounts, fault placement and delivery order) to keep the
binary in minutes. Every test also states what the runs must have reached (failed segments, stuck orders, crashed runs, repeated
deliveries, rewrites...) and fails if they reached less, so a green test is never an empty one. A failing check names the run
(`seed 104 (Joined FailsInTheMiddle)`), and a seeded run replays with `RUTENT_SCENARIO_SEEDS=<seed>`, a pinned one from `PINNED`.

**What each test counts** (`tests/invariants.rs`).
- `INV-1`: per order, over its settled segments (the log's `result` is `done` and the provider's kept result says `succeeded`):
  each took in what the one before gave out, each took in what it gave out plus its fee, and value in equals what the last
  deposit delivered plus every fee plus what rests at the last node; the log's own `fee` entries add to the statements' fees; and
  each figure is the provider's own book's. Across orders, for each private node and asset, what the publishers' books hold equals
  what the log evidences resting there plus the value in motion, which is exactly what a provider took and the log never settled.
- `INV-2`: for each segment that failed, what it was handed equals what the last settled segment gave out (in full); its transaction
  is in no provider's book (it moved nothing); nothing is handed over after it until a recalculation; and the failing provider's
  word alone frees nothing: no reversal counted complete, and no route recalculated from a failed middle segment, without evidence
  from another party. A final deposit's failure leaves the value where it is and Core recalculates (`AC-40`), which the check allows.
- `INV-3`: after the run, a complete part has every segment settled, a stuck one has one that isn't, an ended one has none in
  flight, an open one has none started or of unknown outcome, and what the log says of a segment is what the state machine reached.
- `INV-4`: replaying the log's effects and results in order, an order's value is taken out of a node when a segment is handed it and
  back only by a failure; no segment is handed more than rests there; each transaction is handed over once and moves once; a late
  answer is kept once.
- `INV-5`: every chosen or recalculated route starts at a reserve's outer node (or where a reserve's value sits) and ends in a deposit
  at an outer node, value rests only at a reserve's inner node, and no operation, fee or book names rutent.
- `INV-6`: a `Checkpointer` is built over each run's history, as read back from the disk; the untouched history passes the outside
  `Checker` against the checkpoints as published, and a record rewritten, one removed and one inserted are each found.
- `INV-7`, `NFR-5`: each action in a provider's own book has its `move_value` effect in the log, recorded before the provider's
  answer; each effect cites a permit recorded before it. Across all runs, crashes included, the count of actions with an effect and
  no record is 0.
- `INV-8`: no transaction identifier is stated twice, none moves twice in a book, each segment is handed over once, and every
  movement a book holds is of a transaction an effect of the log names.
- `INV-9`: for each of two reserves, over amounts the seed draws and the balance's edges, the check's answer is yes or no and
  nothing more, is monotone in the amount, is the same function for both reserves and agrees with the simulated balance; a dry run's
  answer is yes, no or a refusal by class with no detail; and neither the catalogue's check log nor the audit log holds the balance or
  what it leaves.
- `INV-10`: every route the log chose, every route it recalculated and every effect's route ends at the same reserve, outer node and
  asset.

**Findings, sorted.** Nothing in `components/` or `contracts/` was changed, and no invariant was weakened.
1. *A repeated delivery to a provider that declares no safe retry moves twice.* With `Fault::DeliverTwice` planted on the joined
   route (seed 110, first form), the simulated exchange's book holds its conversion twice (`id-dbc45aa91cb862f7` at the time), so
   `INV-1` (the book's node balance), `INV-4` and `INV-8` were red. The exchange advertises `safe_retry: false`, and `T-024-01`
   models it as changing its books on a repeat; Core never sends such a provider a retry (`REQ-CAT-10`), and the duplicate here is
   injected beneath Core, at the platform's seam. Sorted as a test artefact for this row (the fault is outside what Core
   guarantees): the joined route's repeats are the state machine's alone and `DeliverTwice` is planted on the base route's
   reserve, which answers a repeat as it did. **It is a gap, not a closed question:** nothing in the platform (`rutent-prov`)
   refuses a second operation under a transaction identifier it already carried out for a provider that declared no safe retry, so
   `INV-8`'s "repeating it MUST NOT move value twice" rests on the provider there. Owner: the platform's row in `F-020`/`F-013`,
   with `T-025-05` to review; the check is `R8c` in the table, red as soon as the fault is planted on the joined route again.
   **Closed by `T-013-09`:** the platform refuses that repeat (`DuplicateTransaction`) and answers a safe-retry provider's from its record; `DeliverTwice` is planted on both routes again.
2. No run broke an invariant otherwise.

**Defaults taken** (reversible, each failing closed).
- A delivery that comes short (`Delivery::Short`) is not drawn: a short delivery is the provider's difference to raise under
  `REQ-ORD-21`, which `T-022-05`'s comparison raises, and `INV-1` here balances an order whose providers' statements add up. Part
  delivery is `T-024-13`'s.
- The exchange's rate is 1 in the simulator, so a conversion balances as in equals out plus fee across assets; a run with another
  rate fails the check loudly (`the simulated exchange's rate is not 1`), never passes it.
- The joined route's amount is `AMOUNT`: the simulated tuples' bands refuse the other amounts tried (`NoRouteForAmount`), so only
  the base route's amount varies with the seed.
- A crashed run is read as it stands and not restarted (`F-030`), so its end-state checks are skipped; its effects, books and
  rewrites are checked.
- `INV-9`'s reserves are the two that take COP (`sim-reserve-a`, `sim-join-in`); the answers are measured against the simulated
  balance of 50 000 000.
- `INV-6` builds the `Checkpointer` over the run's history in the test: the assembly wires none. The ticks are daily from
  2026-11-05 and the clock is four days into the run's, so each run is checked against four or five checkpoints, all over the whole
  history; the rewrite moves one record's time.

**Gaps (with owners).**
- **Other-party confirmation is unreachable, so `INV-2`'s "freed only on another party's word" is held on the cases where nothing
  is freed.** The runs never complete a reversal: a private node is its publisher's alone, so no provider outside it can confirm
  (`T-020-32`, `T-024-06`'s two `#[ignore]`s). The check is written for the day one can (it fails a reversal counted complete or a
  middle failure recalculated from, with no evidence from a party other than the failing provider), and `R2b` below shows the
  freeing rules are held twice over today.
- **`INV-1`'s second half is the publishers' books, not holding-account readings.** The assembly wires no holding account for the
  nodes these runs use (`T-024-17`'s gap 1: nothing lands value in one), so the figure compared with what the log evidences is
  the publisher's own book, the word the audit labels a private node's. `T-022-05`'s pure comparison is tested in `core-audit`.
  Owner: `F-030`'s assembly, with `T-020-26`'s successor.
- **A balance check is recorded in the catalogue's check log, not in the audit log.** The audit log grows by no entry for a
  `check_balance`; only the catalogue's in-memory `check_log` holds it (`AC-15`'s "logged and counted" reads it). Nothing in it
  reveals the balance, but `GUA-3` ("every action has its record") and a restart would want it in the log. Owner: `F-021`
  (catalogue) and `T-025-05`.
- **The outside checker and the checkpointer are not in the assembled Core**: `INV-6` tests them over the Core's history. Owner:
  `F-030`, `T-025-05`.
- **CI's seeded step runs `--test assembly` only** (`.github/workflows/ci.yml`, "Scenarios, seeded"). For the invariants to run
  under CI's seeds as the row says, it needs `cargo test -p rutent-scenarios --test invariants` beside it; the full `cargo test`
  already runs them under the default seeds. I did not edit the workflow. Owner: the lead (one line).
- **Crash runs are not restarted** (`F-030`), and a restart's carrying on from the log is not tested here.

**Run id.** Not available locally; the CI run on this branch's pull request is the one the row's "Done when" cites, and its id
goes here when the lead has it.

**Rule-off table** (each rule switched off alone with a temporary edit, the file restored, the tests green first; every cargo run
under a 420 s hard alarm, none hung; the suite is the whole `invariants` binary, so a column lists every test that went red).

| Switched off | Red |
|---|---|
| the fee entries of a settled segment (`closing::fees` returns none) | `INV-1` |
| the stuck clock (`over_time` never true) | `INV-3` (a silent deposit's order stays open with a segment of unknown outcome; no stuck order reached) |
| the route stops at a segment that didn't settle (`run_segments` carries on with the same amount, `R2c`) | `INV-2` (handed value after the failed segment with no recalculation), `INV-4` (handed value from a node where none rests) |
| the checker's comparison of a checkpoint with the history (`Checker::check` counts every opening as holding) | `INV-6` |
| the effect's record (`Segments::step` returns an earlier step's receipt for `effect`, so none is appended) | `INV-7` (and, since each segment has no effect, `INV-1`, `INV-2`, `INV-3`, `INV-4`, `INV-8`, `INV-10`) |
| the balance verdict's `No` (an answer says not known) | `INV-9` |
| the reserve ends of a route (test side: no node is a reserve's) | `INV-5` |
| a segment handed over again once final, and an effect acted on again (`AlreadyHandedOver`, `AlreadyActed`) | none: `flight.carried` still holds a segment's single carry-out (`R8`) |
| a duplicate delivery planted on the joined route, so the exchange that declares no safe retry is sent one (test side, `R8c`: finding 1) | `INV-8` (the transaction moved twice in a book), `INV-1`, `INV-4` |
| the same two and `carried` (a segment is carried out again) | `INV-3` (the stuck order is no longer stuck); `INV-8` not red: the reserve answers a repeat as it did and moves once |
| the stuck part's refusal to hand over (`run_segments`' stuck guard) | none: `AlreadyHandedOver` and the flight's `carried` hold a stuck part's segments (`R4`) |
| the in-flight rule of recalculation and `value_at`'s failed middle segment (both) | none: the state machine refuses the recalculation of a part whose value is in motion by a third rule (`R2`, `R2b`) |
| every destination and to-asset guard of a recalculation (`Rejected::ChangesDestination`, `ChangesToAsset`, both uses of `recalculation_keeps_destination`) | none: the router plans to the request's own `to`, so no plan Core builds ends elsewhere (`R10`); `INV-10` is held by a property no rule-off of one guard reaches, and the order's own change of destination is `AC-40`'s second half, which the assembly doesn't wire |

**CI run.** The ten tests passed in CI's `ci` job, run `44255094117`, at `2988a310` (the branch merged up to `m3` at `f7c38c98`). CI's seeded step doesn't run this binary yet; `T-024-18` adds it.

## T-024-07: authorisation, audit and module scenarios

Branch `f-024/t-024-07-authz-audit-module-scenarios`, from `m3` at `d0a7cf2c`. Readings taken without asking, each
reversible and failing closed; the owner's form overrules any of them. Nothing in `components/` or `contracts/` changed.
Owner rulings that bind: Core is its own party; a decision is for its caller.

**Tests** (`conformance/scenarios/tests/`), each on the assembled Core through the assembly's own policy engine, log
and platform. Eight of the plan's nine are here; `AC-36`'s is not (gap 1).

- `authz_scenarios.rs`: `unreachable verifier gives a refusal and nothing moves` [`AC-17`] (the order's `move_value`
  decision needs the verifier stand-in's judgement of the payer; out of reach, every decision on the record is `not_known`
  with `verifier_unreachable`, no effect is recorded, the reserve's books are empty, the order doesn't complete; reached
  and silent it is refused `missing_input`, reached and saying no `not_permitted`, saying yes the same order moves, so the
  stop is the verifier's alone); `thousand agents under one party share one limit` [`AC-18`] (a thousand agents of one
  party, each listed and scoped, each charged a thousandth of the party's bound: 10 requests of 10 000, 1 question of 1 000;
  the thousand together reach the bound, and the next charge by the first agent or by one that never charged is over it, on
  the record; a second party's agent is within its own); `externally authenticated caller is still decided by policy`
  [`AC-25`] (a test-local `Gateway` turns a credential into the actor it names and holds no rule: the listed operator login
  is permitted by the register; the SSO "administrator" the register doesn't list, and a login stated for the customer's
  party that nobody listed, are refused, on the record; once the register lists the stranger it is let in, so the
  register's word and no authentication changes the answer; a scope-less agent is refused its work; and the listed login
  is refused while the deployment's policy refuses. Core's calls take an `Actor`, never a credential: none reached the log).
- `audit_scenarios.rs`: `rewrite with full operator access is detected by the outside checker` [`AC-22`];
  `busy and idle periods give indistinguishable checkpoints` [`AC-37`]; and `erasure is refused in the assembled Core until
  policy has a rule for it` (tagged `DEC-18` only: it doesn't verify `AC-36`).
- `module_scenarios.rs`: `upgrade of one provider interrupts no order at another and a failed one leaves the previous
  version` [`AC-19`]; `widened release needs fresh acceptance before it serves an order` [`AC-21`]; `modules in rust and
  in c run sandboxed and read no other provider's state or secrets` [`AC-20`, `GUA-5`].

**The kit added** (shared files touched only where a scenario needs them): `assembly/policy.rs` (a `Verifier` stand-in
for policy's `Source::Verifier`, default silent as before, and `Deployment::judge_before_moving`; the default behaviour
of every existing scenario is unchanged); `assembly/mod.rs` (two `mod` lines and one field); `assembly/audit.rs` (a
checkpointer over Core's log, signing with a log key the operator doesn't hold, on a schedule of the scenario's start and
an hour; the checker's inputs as bytes); `assembly/releases.rs` (`Core::publish_release`, `accept_waiting_release`,
`promote`, `segment_versions`, `install_probe`, `handle_probe`); `sim/fleet.rs` (`Fleet::release`, `Release`);
`sim/probes.rs` (the probe modules); `sim/world.rs` (`seed_secret`, the secrets the world handed out); `sim/disk.rs`
(`standins()`, the module guests' `build.sh` run once into the build directory). The checkpointer only reads the log, so
the crate's hold on it (the compile-fail doc tests) stands.

**Design.**
- *`AC-22`.* A run publishes a checkpoint at tick 0, runs an order, and publishes two ticks more; the checker is handed bytes
  only (the history's records, the checkpoints as they were received at publication, their openings) and holds the log's
  public key, the log's name and the schedule. Held against the copies, the untouched history is clean; one stated entry
  rewritten (its receipt time, canonical, still an entry) fails exactly the checkpoints that committed to a history
  holding it, `DoesNotHold` at those ticks and nowhere else; the last records removed give `HistoryShorter`; the whole
  history rewritten (the same run with another amount) and checkpointed again from its start with the log's own key checks
  out alone, which the test asserts as the limit, and fails every checkpoint received, the new ones sitting beside them as
  a second word for each tick (`Equivocation`); withheld openings are a finding (`NoOpening`).
- *`AC-37`.* Two idle ticks, a busy period (two orders, over twenty records), three ticks after it of which two idle: one
  checkpoint per tick on the schedule's own times, all of one length and one set of members, none naming a size, a root or
  a count; no two commitments alike, though the log didn't move in an idle period; and no unsalted commitment to a known
  size and root (the guesses an observer holding the history would make) equals one. Altered history fails the last,
  idle tick's checkpoint, in whose own period nothing was written.
- *`AC-19`.* Two reserves, the order at the first. At once: the second's module upgraded to 1.1.0 (nothing widened, so
  accepted automatically, in force, a new process) while the order waits; the first's process and version are unchanged,
  its order runs as a control run does (same books), each segment began and ran on 1.0.0 (Core's own `hand_over` record);
  a release whose sandboxed part is no component is refused at admission, the version in force stays in its process, and
  the next order runs on it. Staged: the same release is staged beside the version in force, a promotion naming another
  version is refused, the one naming the staged version is decided by policy for the operator's login, recorded as one
  effect before it is carried out and its result after (`Core::promote`, the call `T-024-02`'s notes said nothing made),
  and a refused release is never staged.
- *`AC-21`.* A release declaring one destination more is admitted and waits (`Fresh`, its widening named, compared with
  1.0.0); the version in force stays and no process of the waiting version runs; two orders run on 1.0.0; a second
  release can't be admitted over it; the fresh acceptance puts it in force and the next order began and ran on 1.1.0.
- *`AC-20`.* The module guests' stand-ins, in Rust and in C (`c-reserve-*`, built by clang for wasm32), carry a program in
  the slot of their component, as the prov tests' binding writes it: each part tries to look up a secret and to read a
  state entry under a name another provider, a simulated reserve, also uses and holds a canary in. Both modules run in the
  platform's sandbox and answer (their results signed by the platform); the sandboxed part's lookup never reaches the
  proxy and the outside part's is refused `not_found`; every state read finds nothing; neither canary is in the module's
  answer, the platform's records, calls or lookups, the secrets the world handed out (the owner alone was handed its own,
  as the control that it was there), the requests the world received, the reserve's books or any entry of Core's log; the
  four modules are four processes; and the order at the simulated reserve runs to its end meanwhile.
  **What every platform shows** is the WebAssembly sandbox in the platform's engine (no import gives a part another
  provider's state or secrets; the platform judges each host call), in a process per provider. **What only Linux shows**
  is the operating system's jail around that process (Landlock, a `seccomp` allow-list, no capabilities, `sandbox.rs`);
  `T-025-02`'s jail-probes job covers it and nothing here depends on it.

**Defaults taken.**
- `AC-17`: an unreachable verifier gives not known, never a refusal (the engine's, and `REQ-AUZ-5`'s; `AC-17`'s wording says
  refusal, and `F-021`'s notes keep `spec-ac17-not-known` on the owner's form). The test holds that nothing is permitted and
  nothing moves, and the exact record the engine gives; the plan's test name is kept. The verifier is the assembly's source
  stand-in under a deployment rule asking for the payer's judgement before any move, not the simulated verifier module
  through the platform, which is `T-024-12`'s.
- `AC-18`: the engine's default bounds, one thousand charges each, rather than a bound set low through a new hook.
- `AC-25`: the authentication stand-in is the test's own and the register is the deployment's, as `F-030` would run them.
- `AC-22`: the checkpoint store is in memory, the log's key is a fixed one the operator doesn't hold, tick 0 is the scenario's
  start and a tick is an hour.
- `AC-37`: one busy period, since each Core has one session of fourteen minutes (gap in `T-024-02`'s notes).
- `AC-19`: a failed upgrade is a release the compiler refuses; a release that compiles and whose process doesn't start
  has a stand-in only in the module guests' own probes, and is held by prov's `failed_upgrade_leaves_the_previous_version_in_force`.
- `AC-21`: a widening is a destination more; the acceptance is the platform's `accept`, by the test reviewer.
- `AC-20`: the probes are providers of the platform's own catalogue, beside the simulated providers, never of Core's
  catalogue or log; they take the first build of the stand-ins (about 80 s here), cached by a hash of their sources.

**Gaps, each with its owner.**
1. **Closed by `T-020-37`** (policy's rule by `T-021-10`, Core's carrying out by `T-020-37`; the scenario is
   `erasure_leaves_the_reference_and_history_checks` in `audit_scenarios.rs`). As found: **`AC-36` can't be run through Core: two missing pieces.** The plan's `erasure leaves the reference and history checks`
   [`AC-36`] isn't written, and `AC-36` stays unverified through the assembly (`T-022-03`'s own tests hold the store's half).
   (a) *Policy has no rule for `erase_statement`*: `roles/mod.rs` answers `None` for `Action::EraseStatement`, so every
   decision is refused (the test shows it, with the store's own `NO_DECIDED_ERASURE` behind it); the agent's `Scope::EraseStatement`
   exists, the operator's permission doesn't. Shape wanted: an operator login's `Permission` for it, held by the register,
   as `change_catalogue` has `Permission::Change`. Owner: `F-021`. (b) *Nothing in Core carries an erasure out*: no module
   of `core-orch` writes the effect, its `done` result, the `BoundStore::erase` call and the `erasure` entry (the order
   `T-022-03`'s notes give). Owner: `F-020`. With both, the scenario is small: decide, effect, result, erase, entry, then the
   outside checker over the history, the reference named by its entry and the statement answering `Erased`.
2. `AC-17`'s wording against `REQ-AUZ-5`: pending on the form (`spec-ac17-not-known`); no change here.
3. A provider's release and its acceptance aren't recorded in Core's log: the platform's `accept` is called by the kit, and
   the decided `accept_module` catalogue change exists only for the operator's plain wallet (`Core::accept_release`).
   `AC-21`'s fresh acceptance is shown at the platform, not as a record. Owner: `F-020` with `F-021` (`T-020-08`'s successor).
4. An upgrade in the middle of a segment is not driven: no fault hook at the platform's seam does it, and pinned segments are
   held by prov's lifecycle tests. Owner: `F-024`, if the form wants it.
5. The evidence store's own findings (`missing`, `refused_erasures`, `unsettled_erasures`) still don't reach the outside
   checker (`T-022-04`'s gap for `T-024-02`); nothing here reads them. Owner: `F-033` with the lead.

**Rule-off table** (each switched off once, the file restored; the tests passed first, since the code they hold was there,
so each rule-off stands in for seeing the test fail first; none hung; every one red on an assertion unless said):

| Switched off | Red |
|---|---|
| an unreachable input read as yes (`engine/mod.rs`, `verdict`) | `unreachable_verifier_gives_a_refusal_and_nothing_moves` |
| the deployment's judgement rule asking the verifier (kit) | the same |
| the counter keyed by the actor, not the party (`parties/mod.rs`) | `thousand_agents_under_one_party_share_one_limit` |
| the register's silence read as yes for who acts for whom (kit) | `externally_authenticated_caller_is_still_decided_by_policy` |
| the deployment's refusal (kit) | the same |
| the checker's `DoesNotHold` finding | `rewrite_with_full_operator_access_is_detected_by_the_outside_checker` |
| an idle tick not issued (`checkpoint/mod.rs`) | `busy_and_idle_periods_give_indistinguishable_checkpoints` (an index past the checkpoints issued, a panic) |
| the checkpoint's salt left zero | the same (an unsalted commitment equals a checkpoint) |
| a widened release not needing a fresh acceptance (`platform.rs`, `admit`) | `widened_release_needs_fresh_acceptance_before_it_serves_an_order` (the release isn't admitted: `validate_release`'s own `WidenedReleaseNotFresh` also stops it, a second guard) |
| a failed admission taking the version in force with it | `upgrade_of_one_provider_interrupts_no_order_at_another_and_a_failed_one_leaves_the_previous_version` |
| one provider's upgrade restarting every other's process | the same |
| the promotion's check that the version is the one staged (`segments/mod.rs`; a second check after the effect stays) | the same (two promotions recorded) |
| the world answering a lookup by name, whoever asks (kit) | `modules_in_rust_and_in_c_run_sandboxed_and_read_no_other_providers_state_or_secrets` |
| the world answering a state read by key, whoever asks (kit) | the same |
| a sandboxed lookup recorded as having reached the proxy (`platform.rs`) | the same |

## T-024-12: the verifier half of `REQ-CPL-12` and of `AC-24`

Readings taken without asking, each reversible; the owner's form overrules any of them. `T-020-23` (the writer of
`reliance` entries) is merged and this row runs on it: the scenarios declare which input rests on a judgement
(`Core::rests_on`, then `run`), and the `reliance` follows by itself.

**Tests** (`conformance/scenarios/tests/verifier.rs`, the four the row names, over `execution_support/` for the joined
route). Each drives Core's own `request`, `quote`, `accept` and `run`; the verifier is the stand-in (`DEC-16`), asked by
the order's needs (`rounds::Need`), since provider-role 3.0.0's tuples name no verifier.
- *provider relying on a verifier's judgement leaves a reliance entry naming the verifier and the provider*
  (`REQ-CPL-12`): one `reliance`, citing the receipt of the verifier's stated judgement, naming it as judge and the
  reserve as the one relying, written before the first move of value; no entry but the reliance cites the judgement, and
  neither the operator, the customer nor Core is judge or relier.
- *false judgement is recorded as its verifier's word and its relier's reliance* (`REQ-CPL-12`): `sim-verifier-lax`
  says yes without checking to a subject an honest verifier refuses (`unfit-`); the route cites that yes, the order runs,
  the reliance names the lax verifier and the reserve, and no reliance names the operator or the customer. The same
  subject put to `sim-verifier` is a no.
- *requester beneficiary and payer are checked by the judgements their providers' rules name* (`AC-24`, `REQ-AUZ-5`):
  over `sim-join-in`, `-exchange`, `-ramp` and `-out`, the payer is judged for the withdrawing reserve, the requester for
  the exchange and the beneficiary for the depositing reserve, each by the verifier that rule names; the acceptance
  records the three apart, policy was asked about each once, and each operation cites exactly its own party's judgement
  (the ramp's none).
- *tuple needing a judgement is not offered without one* (`REQ-AUZ-5`): a control offered with its judgement, then no
  offer where the verifier is unknown, judges another class, or one of two needs (or the deposit's) goes unmet.

**Added** (`conformance/` only; nothing in `components/` depends on it). The simulator: a verifier says no to a subject
beginning `unfit-` and `sim-verifier-lax` (the name chooses it) says yes to any (`Verifier::saying_yes_without_checking`).
The fleet (`sim/fleet.rs`): `Installed::released` (the accepted release) and `Fleet::judge` (a verifier answers in
process, signed for real). The assembly: `Core::rests_on`, `Core::judgement_of` (the receipt of a verifier's stated
judgement, read from the log: `T-020-23`'s gap 1), the verifier's logged grant and its release recorded, the `Asking`
seam handing the rounds the judgement a verifier's result carries, and policy's `OrderParties` falling back on intake's
record before the acceptance (`Records::order_parties`).

**Defaults taken.**
- A verifier is asked in process (`Fleet::judge`), not through the platform, and its answer reaches the rounds as the
  nested judgement (they read a judgement, not the result around it).
- A verifier's grant is logged naming a placeholder tuple (`verifier_tuple`) and the catalogue grant covers its category.
- Core's person question is answered before the acceptance from intake's record of the order's parties (the log holds
  them only from the acceptance, and routing happens before it); a read that used to find no parties before acceptance
  now finds them.
- One subject per relying provider is the snapshot's own rule (`all judgements about one relying provider's tuples are
  about one subject`), so three parties are judged by three providers.
- Only a verifier's accepted release is recorded in Core's releases; a reserve's would add its calls figure to every
  snapshot.

**Gaps, with owners.**
1. *A verifier's grant can't be logged* (contract). The log attributes a statement under the advertising keys a grant
   names, and `ChangeCatalogue` names a tuple exactly when it is about one, which a verifier's category grant is not
   (`check.rs`, "a catalogue change names a tuple exactly when it is about a tuple"); without a grant in the log the
   verifier's judgement is attributed to no one and `reliance-names-its-judges-statement` refuses a reliance on it. The
   exact shape: a `grant` with `tuple: None` for a category grant (and `granted_advertising_keys` reading it). Owner:
   core-internal's single writer, through the coordinator (a contract round of `F-020`/`F-021`), then `F-053`. Until it
   lands the assembly logs a placeholder tuple.
2. *The simulated verifier module refuses every question* (`components/prov/module-guests/sim/src/lib.rs`, `serve`): a
   judgement names no transaction, so the guest builds its provider with no deployment (`provider(held, provider,
   None)`) and refuses what the operator signed. Owner: `T-024-01`'s module guests; the fix reads `DEPLOYMENT_KEY` for a
   transaction-less operation. Until then `Fleet::judge` answers in process.
3. *No rule says which input rests on which judgement.* The scenarios declare it (`T-020-23`'s default); the
   production rule and the verifier's need in the advertisement are `F-053`'s (`M5`, `DEC-16`).
4. *`OrderParties` before the acceptance* is the assembly's reading of intake's record; `F-030`'s source must do the
   same, since routing precedes acceptance.
5. *A refused judgement doesn't stop a route*: `judged` asks for a judgement of the tuple's verifier, subject and
   class, not that its answer be yes; a no is cited like a yes. Not mine to change (`components/`), and in v0 a
   provider checks eligibility itself (`DEC-16`). Owner: `F-020`, if the answer is to count.

**Rule-off table** (each switched off alone, under a hard timeout, restored; green first):

| Switched off | Red |
|---|---|
| `Asking` handing the rounds the nested judgement | all four |
| policy's `OrderParties` fallback on intake's record | all four |
| the verifier's logged grant naming its keys | all four (the assembly refuses the grant) |
| `sim-verifier-lax` saying yes to everyone | the false-judgement test |
| the honest verifier's no to `unfit-` | the false-judgement test |
| the scenario's `rests_on` declaration | the reliance test, the false-judgement test |
| Core's `advance` writing the reliance (`T-020-23`) | the reliance test, the false-judgement test |
| `rounds`' `judged` check of a cited judgement | the not-offered test |

The three-party test stays red under the first two and is held by its own exact assertions (the questions put and each
operation's cited judgements), which no single rule-off isolates: the exact-set assertions were seen to fail while
the route kit was being built, when two needs on one provider were refused by the one-subject rule.

## T-024-18: the kit's owed items from T-024-12, T-024-15 and T-025-02

Branch `f-024/t-024-18-kit-owed-items`, from `m3` at `7bc8be82` (which has `T-020-30`'s class grant). Readings taken without
asking, each reversible; the owner's form overrules any of them. `T-024-08`'s invariants are not on `m3` yet, so item 5's
`--test invariants` needs that row merged first (see the gaps).

**Tests** (the six the row names, plus one for item 5):
- `simulated_verifier_module_answers_a_judgement_that_names_no_transaction` (`REQ-AUZ-5`,
  `conformance/scenarios/tests/verifier.rs`): in the assembly's deployment, through the platform and the module, a fit
  subject is judged yes and an `unfit-` one no, each the verifier's own statement; a question made in the suite's default
  deployment is refused. `T-024-12`'s four tests now run on the module: `Asking` no longer answers a verifier in process.
- `verifier_judged_through_the_assembly_is_attributed_by_its_class_grant` (`REQ-AUD-2`, same file): the log's one grant of
  the verifier names its class and one advertising key and no tuple; its judgement's entry is attributed `verified`, to the
  verifier, in the role of a verifier.
- `reserve_never_asked_the_arrival_question_is_reported_uncovered` and
  `arrival_question_under_another_key_or_in_another_session_must_be_refused_finally` (`REQ-ORD-23`,
  `conformance/tests/role_arrivals.rs`), beside `T-024-15`'s three, which pass unchanged.
- `evidence_table_counts_the_jail_probes_jobs_results` (`GUA-5`) and `seeded_ci_step_runs_the_invariants_beside_the_assembly`
  (`DEC-18`, `tools/records/tests/evidence.rs`).

**Item 1, the module.** The sandboxed part cannot read state, and its only way out is an outside request, which the platform
passes on only if it names the operation's own transaction; a judgement names none, so the platform's context for it held
the empty string, which no outside request can name (an `Identifier` is non-empty), and the guest could not ask for the
deployment at all. The guest alone could not fix it. Two changes, then:
- **`components/prov/src/module/platform.rs` (F-013's file, a `DEC-20` path):** the operation's context names it by the
  identifier the platform already knew it by for the world and the credential proxy, `rutent:untransacted:<sha256 of the
  operation's JSON>` (`F-014`'s notes), where it names no transaction. Records of its state reads, limits and outbound calls
  name that identifier too (an outbound call for an operation with no transaction used to be refused unrecorded). Nothing
  changes for an operation that names a transaction.
- **`components/prov/module-guests/sim/` (a `T-024-01` file):** for a verifier (an operation with no transaction) the guest asks
  the outside part for the deployment key under that same identifier, which it computes from the bytes it was handed (a
  `sha2` dependency, in the guests' own lock), and builds its provider in that deployment. It asks with `BEGIN`, the one
  outside operation that reads the deployment; its script read finds nothing, a verifier having no script.
- `Fleet::judge` and the fleet's in-process verifiers are gone, with the workaround they were.

**Item 2, the class grant.** The assembly logs a verifier's grant as `ChangeCatalogue { change: grant, tuple: None, class:
Some(c), advertising_keys }`, one grant for each class it advertises, in place of the placeholder tuple (`verifier_tuple`,
removed). `Core::relist` skips a done class grant (the category grant in the catalogue is made where the advertisement is
replayed, as before).

**Item 3, the role suite.** `NothingArrived` is required of every reserve before the question is first put, so a reserve never
asked it (the platform's binding carries operations and this is not one) is uncovered; a question under another session or
not signed with the operator's key must be refused, and finally (`REQ-CAT-3`, `REQ-CAT-4`), which a notice, "nothing arrived"
and a refusal that waits on the person each fail. These two are asked before the credit, so a reserve with no hook is held to
them too. The `Reformatting` stand-in in `conformance/tests/role.rs` and the `Through` and `InProcess` ones in
`components/prov/tests/module_role.rs` did not forward the arrival question, which the new rule showed; they forward it now
(test artefacts; `Through` hands it to its twin, signed for real). The doc comment of `other_protocol_versions` is back on it (it sat on
`elements_of`), and this file's `T-024-15` note reads `Refs #171` in place of "Closes issue #150" (the review's lows 4 and 5).

**Item 4, the evidence table and the jail probes.** The probes need `--features jail-layers`, which `cargo test --workspace`
leaves off, so `GUA-5` and `REQ-MDL-11` would read untested. A separate workflow can't hand its results to `ci.yml`'s table
except through an artefact of an earlier run, which a push races, so `ci.yml` runs the probes itself, once more, appended to the
results file the table reads (`tee -a`), before the table's steps. `jail-probes.yml` stays the job that records the run id for
`F-025`'s notes. The table itself needed no change: it joins a probe's line to its target (`tests/jail_layers.rs`) and its
name, as it does any test. The row `THR-85` and `THR-79` the probes also tag have no row in the scope file. Cost: the probes
(and the release build their scan test makes when `RUTENT_RELEASE_PROVIDER` is unset) run twice per push, a few minutes.

**Item 5, CI's seeded step.** `--test invariants` runs beside `--test assembly` under the step's seeds `11` to `15`. A `DEC-20`
path (`.github/workflows/`), as is item 4's step: both wait for the owner's click at the release into `main`.

**Defaults taken.** The verifier asks for the deployment with `BEGIN` (no new outside operation, so no declaration change);
a question for another deployment is refused, not judged, as every module is held to the world's deployment; a verifier that
advertises no class logs no grant.

**Gaps, with owners.**
1. `T-024-08` is not on `m3`, so `cargo test -p rutent-scenarios --test invariants` fails ("no test target") until it merges: this
   row must merge after it. Owner: the lead (merge order).
2. A platform record may now name `rutent:untransacted:...` as a transaction; a consumer that parses a record's transaction as an
   order's would meet it (none in the repository does). Owner: `F-013`, to confirm the reading.
3. `jail-probes` is still not a required check (`T-025-02`'s note); the table now reads a run of the probes in `ci`'s own job.
   Owner: Pablo.

**Rule-off table** (each switched off alone, run under a hard timeout, restored; green first, except that the tests were
written after the code, so "red first" is the rows below):

| Switched off | Red |
|---|---|
| the platform naming an untransacted operation's context | the module test (the guest's ask is refused) |
| the guest's read of the deployment | the module test and `T-024-12`'s four tests, now on the module |
| the logged class grant of a verifier | the attribution test and `T-024-12`'s four (the judgement is attributed to no one) |
| `require` of `NothingArrived` before the first ask | the uncovered test |
| the final refusal of the foreign questions, "nothing arrived" accepted | the foreign-question test and `T-024-15`'s suite test |
| the final refusal, a refusal that waits accepted | the foreign-question test |
| the jail probes' line in `ci.yml` | the jail probes test |
| the probes' output appended to the results file | the jail probes test |
| the seeded step's `--test invariants` | the seeded-step test |
| a probe's `GUA-5` tag | the jail probes test |

### T-024-18, after review of `abf1c85f`

- **The table read no result in CI** (`tools/records/src/evidence.rs`): `CARGO_TERM_COLOR: always` puts ANSI codes in cargo's
  `Running` and `Doc-tests` words, so no line joined its target ("232 rows: 0 verified" in run 44265372431, and on PR #206; the bug
  dates from `T-024-09`). `Results::parse` now strips escape sequences before it reads a line; the colour setting is left as it is.
  Test: `coloured_cargo_output_joins_each_test_to_its_target` (`DEC-17`). Run over the reviewer's saved log of that run (its
  runner timestamps cut): **232 rows: 210 verified, 5 inspected, 1 partly met, 1 unverified** (`AC-4`, whose only test is
  ignored), 3 M4's, 12 deferred. Rule-off: with the strip off the test is red.
- **`expect_final_refusal`** flags a reserve that answered the question in the session but gives no answer to a foreign one
  (`SilentToForeignQuestions` in `role_arrivals.rs`, which is red without the flag).
- `Through` (`module_role.rs`) says in a comment that the twin answers the arrival question, not the module (`F-014`, `F-031`).
- The guest no longer caches a provider built on a failed deployment ask: it answers that one operation as the default deployment
  does and asks again next time (`T-024-12`'s verifier tests, rerun, are green).

### T-024-19, three lows from `T-025-05`'s security review

- **A second `strict:` line** in the scope file is a `bad-row` finding (`tools/records/src/evidence.rs`); the first line stands as
  the one read, so a file showing `strict: on` at its top can no longer run off through a later line. Test:
  `scope file with a second strict line is refused` (`DEC-17`).
- **`decline_outcome`** of the assembly's policy side forwards to the engine, as the other calls do, instead of answering
  unavailable (`conformance/scenarios/src/assembly/policy.rs`). Test through the assembly:
  `assembled decline of a stuck outcome is decided by the policy engine` (`REQ-AUZ-4`): on an order with no report to decline,
  the answer is the log's refusal and nothing is recorded; the engine's own decline tests (`T-021-09`) cover the permitted path.
- **`ci.yml`'s jail step** sets `RUTENT_RELEASE_PROVIDER` as `jail-probes.yml` does, and a release build of the provider binary runs
  before it (the variable names that build; without it the probe would fail on a missing file). The scan no longer builds inside the
  test, so its result line stays its own and `GUA-5` counts it. Test: `evidence table counts the release scan probe run in ci`
  (`GUA-5`): the table reads a results file where the probe ran on a line of its own as verified, a shared line as not, and
  the workflow sets the variable and builds first. **`DEC-20` path:** the workflow edit waits for the owner's click at the release into
  main, and for `T-025-05`'s reviewer's addendum.
- **Default** (fails closed): a second `strict:` line is refused whatever it says, even a repeat of the first.

| Switched off | Red |
|---|---|
| the second-line refusal in `parse_scope` | the second-strict-line test |
| the assembly's forward to the engine (back to unavailable) | the assembled decline test |
| the probe's line left shared (the table's reading of the results) | the release-scan test's shared-line assertion |
| `RUTENT_RELEASE_PROVIDER` in the jail step | the release-scan test (the workflow assertion) |
| the release build step before the jail step | the release-scan test (the build-before-probes assertion) |
