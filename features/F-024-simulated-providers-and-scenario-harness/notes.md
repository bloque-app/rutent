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

- **Policy calls answered beside the engine** (`PolicySide`; review of `712c3fb`, 1): authorisations (`T-021-04`),
  charges (`T-021-05`) and questions about people (`T-021-07`) are each decided as the engine decides a call: its
  own fixed rules (`core-authz`'s `Fixed`, the register of actors and the roles' rule among them) over the same
  records, then the deployment's policy, which `Core::refuse` reaches. The answer alone is the stand-in's: an
  authorisation recorded as the consultation's answer; a charge within a bound no scenario reaches where permitted,
  over a bound of 0 where not, so `AC-18`'s shared limit can't be shown on it; a question about a person refused
  where refused and otherwise not known. Each still writes its answer outside the engine's `consult`, which the
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
| the silence fault | `silent_provider_leaves_its_deposit_unanswered_and_moves_nothing_for_it` |
| the crate's hold on the log | the compile-fail doc tests (Core's disk made public again) |

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

**Runs.** CI's run ids for the `ci` job and the `STM on VOS` workflow are pending the orchestrating session's push
of this branch (`stm-vos.yml` lists it as a trigger); none is recorded until then.
