# F-010 State machine library: notes

## T-010-02: the contract guard's three findings

- **Reading: "a contract's dependents".** The `M1` finding says contract dependents and the root `Cargo.lock` are not
  tied to bumps, and the task row says a change to them "that moves a contract's dependency" needs that contract's
  bump. Read as the dependency edges of the contracts crate: the guard follows `rutent-contracts` through the root
  `Cargo.lock` (its dependencies, theirs, and the dev-dependency) and fails when any reached package differs between
  the base and the head. A crate that depends on `contracts/` (`components/*`) is a consumer and does not trigger it.
  A change to `contracts/Cargo.toml` itself is a shared-file change, so the first rule covers it.
- **Reading: "that contract's bump".** All five contracts ship as one crate (`rutent-contracts`), and `CONTRACTS`
  does not say which contract uses which dependency, so a moved dependency counts as touching every contract: each
  needs a greater version and a new bullet naming a `DEC-*`. A contract new at the head still needs none.
- **Reading: the plan's decisions.** A decision is defined when `PLAN.md` has a table row starting with ``| `DEC-<n>` ``.
  The guard reads `PLAN.md` from the head working tree and exits 2 if it is missing. Every `DEC-<n>` a new bullet names
  must be defined, not just one. The shared changelog's new bullets are now held to the same rule as each contract's
  (the README already said "any changelog"; the code only checked contract changelogs).
- **Shared files.** A contract's version bump no longer covers a shared file. The old passing test for it is replaced
  by the required failing one.
- **DEC-20 paths touched.** `tools/contract-guard/` and `contracts/` (`README.md` states the rules; `CHANGELOG.md`
  holds the shared entry the new rule requires of this very change). No contract's files changed, so no bump.
- **Limit of "moved".** A move is any change to a reached package's identity: version, source, checksum and its
  dependency list. So another crate enabling a feature of a shared dependency (for example `preserve_order` on
  `serde_json`) changes that package's dependency list and forces every contract's bump and a `DEC-20` click for work
  that is not contract work. If it bites, narrow the identity to name, version and checksum.

## T-010-03: the contract changes `M1` routed to `M2`

- **One bump each.** Read as one version per change, not one per contract: provider-role 2.3.0 (the encoder) and
  2.4.0 (bytes as received for the verifier); module-interface 2.2.0 (`validate_call_against`), 2.3.0 (refusal
  reasons) and 3.0.0 (`delivered` only when answered), each its own `##` section, minors first so one major covers
  the breaking one. The guard reads only the top section, so one commit carries all five. Router-api,
  state-machine and host are unchanged: no file of theirs moved and no dependency of the crate moved.
- **Where the encoder lives.** In provider-role (`provider_role::canonical`), beside the signing rule, so the host
  contract's `exactly-once` wording ("the same canonical JSON under RFC 8785") is now true without a host bump; the
  host's README still names RFC 8785 and not the module. The VOS program and the native client still write
  `serde_json`'s form for their own dedup; moving them onto `to_canonical` is `T-010-05`'s, with the program.
- **Its own parser.** `serde_json::Value` keeps the last of two equal member names, and `serde_json`'s default float
  parsing is not always correctly rounded without `float_roundtrip`. Turning on a feature of `serde_json` from
  `contracts/` would move the root lock and force every contract's bump (T-010-02's note above), so the encoder
  parses I-JSON itself and reads numbers with `core`'s correctly rounded parser. No dependency added.
- **Numbers.** `core`'s `{:e}` gives the shortest digits that read back but does not break an exact tie to even, as
  ECMA-262's Note 2 asks (RFC 8785 Appendix B's `1424953923781206.25`, which `core` writes as `...206.3`). The
  encoder checks the exact expansion for a tie and takes the even neighbour where it reads back.
- **`to_canonical` and non-finite floats.** It goes through `serde_json::to_vec`, which writes NaN and infinities as
  `null`. No contract type holds a float, so this is documented on the function rather than guarded.
- **Refusal reasons.** `AdmissionRefusal` and `PlatformRefusal` were the module-boundary suite's, not the
  contract's. The plan calls them module-interface's, so they move into `contracts/src/module_interface/refusal.rs`
  and the suite re-exports them under the same paths. Added: `DisallowedImport(PartName)`,
  `VersionAlreadyAdmitted`, `LimitReached`, the last named after the outbound call's existing `limit_reached`.
  `UnreadableBinary` and `Unavailable` stay, for what they say. Rust-only types, no schema: nothing records them yet
  (`T-013-*` may). The reference double refuses a re-admitted version, and, given `with_step_limit`, a module past
  it; the suite has no probe for either, which stays `F-024`'s with the suite's next version.
- **`undeclared_kind`.** Accepted only where the callee and the forwarded kind are both declared; an
  `undeclared_callee` refusal of a declared callee stays untrue, since the record's callee is the one judged. The
  record the wasm binding's `tests/calls.rs` writes for that case meets this rule as written; the binding was not
  rebuilt here.
- **`unanswered`.** A new outcome, not a refusal: the platform passed the call on, so the callee may have acted.
  `validate_call_against` treats it as passed on (`UndeclaredCallDelivered`, name kept, docs widened). The reference
  double still records `delivered`, since its callee is a counter that always "answers"; a binding-side test of a
  callee that never answers is `T-013-02`'s. Major, since a 2.x `delivered` for an unanswered call is untrue now and
  the Rust enum gained a variant. Fixtures keep naming interface 2.0.1, as `T-010-01` left them; nothing compares it.
- **Bytes as received.** `RoleProvider` now returns `Received<T>`, the JSON a provider sent and the statement read
  from it, so the suite never re-serialises. `SignatureVerifier::verify` takes `received`; a nested judgement or
  confirmation is handed over as its own text inside the result's (`provider_role::member`), so a verifier can
  canonicalise it as received. The suite also flags `EnvelopeWellFormed` when a statement's text has no signing
  input. The stand-ins implement a private typed trait and send through `Received::sent`.
- **Not run here.** The `EXP-2` bindings and the VOS adapter depend on `conformance/` and `contracts/` by path,
  outside the workspace, and were not rebuilt: the bindings implement `ModuleBinding`, whose refusal types keep their
  paths but gain variants, and implement no `RoleProvider`. Their workflows run only on their own branches.
- **DEC-20 path touched:** `contracts/`.

## T-010-04: the state machine library (`rutent-stm`)

Built against the contracts as they stand at `138281d` (state-machine 2.1.0, host 2.1.0). Nothing in `contracts/` was
needed or edited; nothing at `138281d` was missing. `T-010-03`'s row (canonical JSON encoder, module-interface
refusal reasons, the call-recording rules) names nothing the library reads. The library imports nothing of rutent's but
`contracts/` (`DEC-2`).

### How the library is shaped

- `Stm` implements the contract's `StateMachine`. `open` takes a `route_chosen` input and builds every part with its
  segments pending; `transition` is total, pure and reads no clock and no amount. Every order it returns passes
  `Order::check`: a change that would break it is not applied (`not_applicable`, `not_from_this_state`), and so is an
  input that fails its own `RecordedInput::check`.
- `rutent-stm` is `no_std` + `alloc` with a default-on `std` feature. The reference host (`std::collections`) and the
  purpose-built store (`std::fs`) are built only with `std`. `cargo build -p rutent-stm --no-default-features --target
  riscv32imac-unknown-none-elf` is the check.
- The contract is the published rules plus these functions, all read as published: `after_no_route`,
  `deposit_failed_at_after`, `recalculation_keeps_destination`, `destination_change_applies`,
  `recalculation_starts_where_value_sits`, `value_at`. The library adds no rule that reads an amount (F-005, finding
  2); which amount the next segment moves and the `exact_out` remainder stay `F-020`'s.

### Readings taken where `SPEC.md` or `PLAN.md` is silent

- **"Property test" is a seeded generator, not a framework.** `proptest` would put its whole dependency tree into
  `licences/RECORD` for no gain here: nothing needs shrinking, since a run is a pure function of its seed and every
  assertion names it. The generator (`components/stm/tests/common/generator.rs`) is biased towards the next valid input
  and mixes in repeats, inputs for unknown subjects, reports on retired segments and routes that change the
  destination, reuse an id or start away from the value. A coverage test fails unless every state, every
  not-applicable reason a host can reach, and every kind of applied change (recalculation, destination change,
  hand-on, complete order failed) occurs. `OtherOrder` can't be reached through a host, which finds an order by its
  subject; a scenario test holds it.
- **"The host-contract suite on the reference host".** The suite in `conformance/` is typed on its own `Probe`, so
  it can't score `Stm` as such, and `conformance/` is not this task's to edit. The reading: the existing reference-host
  test still runs the suite, and `tests/host_contract.rs` adds what the suite can't, with `Stm` as the machine: `Stm`
  reaches the probe's states over the suite's own scenarios, and a reference host running `Stm` keeps one record per
  record id, survives a kill and a swap, and replays to the states it recorded.
- **The order's state from its parts (`REQ-ORD-42`, F-005's `T-005-01` note).** The order closes only when every part
  has ended (complete, failed, handed on, ended without completing). Then its state is the worst ending: failed, then
  handed on, then ended without completing, then complete. So an order of complete parts and one handed-on part is
  handed on, which the contract requires of any order that holds a handed-on part. While any part is open: stuck if
  any is stuck, else waiting for the customer if any waits, else first segment settled if any part is past its first
  segment, else not past it. An order of one part is in its part's state. It is `order_state_of`.
- **What counts as ended (`REQ-STM-6`).** Per part: complete, failed, handed on, ended without completing. Stuck and
  waiting for the customer are not endings. An ended part takes no input and the input is recorded as `ended`. The one
  exception, after review: a `reversal_completed` on the starting segment (the part's first) of a complete part, in
  an order that is itself complete. That segment goes settled to reversal complete, and the part and so the order
  fail (`AC-8`). A part that has not completed, a handed-on or failed order, and an order still open do not change:
  the same input is `not_from_this_state` or `ended`.
- **A part's segments move forward only,** along the edges in `machine.rs`'s header. This, not the host's dedup alone,
  is why a repeat of an older input finds nothing to move: the state machine remembers only its last input.
  `provider_bound` also needs every earlier segment of the part settled, so no segment starts while the value is
  elsewhere (`INV-4`).
- **`outcome_unknown` is left by `segment_settled`, `segment_failed` or `reversal_completed`** (`REQ-ORD-1`: only on
  stated evidence), never by a timer or by `stuck_declared`.
- **A deposit has no `reversal_completed`.** A failed deposit leaves the value at its `from` node (`INV-2`,
  `REQ-ORD-43`), so it owes nothing back and its segment stays `failed_reversal_owed`; `value_at` and `Order::check`
  both read it so. A `reversal_completed` on a deposit is `not_from_this_state`. Counts as "reversed in full" for
  `INV-3`: the value is still whole at that node, and the hand-on or the recalculation starts from it.
- **A stuck part still takes segment reports.** `stuck_declared` is a declaration, and elapsed time is never an
  outcome (`REQ-STM-5`), so a report that arrives later is evidence and moves the segment. A `provider_bound` is not
  a report: no new provider starts on a stuck or waiting part (`not_from_this_state`), since the operator or the
  customer resolves it (`REQ-ORD-28`, `REQ-ORD-43`). A part in a held state
  (stuck or waiting) goes back to complete if every segment then settled, and otherwise stays held.
- **`end_requested` ends each part that hasn't passed its first segment** (`REQ-ORD-12`), and leaves the others. It
  is `not_from_this_state` when no part ends. **`withdrawal_cancelled`** ends a part that hasn't passed its first
  segment, whose first segment is an unsettled withdrawal. No segment state means "cancelled", so the segments stay as
  they were; a later report on them is recorded as `ended` (`REQ-STM-6`).
- **`destination_changed` and `handed_on` need a held part** (waiting for the customer or stuck). `REQ-ORD-43` has the
  customer choose after a failed final deposit with no route left, and lets the operator process a stuck order the
  same way. `handed_on` also needs the value placed by `value_at` (`INV-4`) and a linked order other than the order
  itself. `route_recalculated` applies from any part that hasn't ended. After either route, the part is by its first
  segment again (not past it, or its first segment settled).
- **A `no_route` that would not change the state is not applicable.** A part already stuck that gets a `no_route`
  placed as stuck stays as it was.
- **The reasons a route is refused,** in the contract's order: the state first (a `destination_changed` outside a held
  part is `not_from_this_state`), then `changes_destination` or `keeps_destination`, then `reuses_segment_id`, then
  `value_not_at_start`. The ids "in use" are those of every part, current and retired.
- **The start rule across two orders** (F-005, finding 4; `handed_on_route_starts_where_value_sits(first, second)`).
  Reading of "a handed-on route starting where the first order's deposit failed": the second order's every part starts
  at the endpoint `value_at` places for a part of the first order that holds value for it, in node and unit. A part
  holds value for it when it waits for the customer or is stuck, or was handed on to that very order. For a part that
  waits after a failed final deposit, `value_at` is the failed deposit's `from` node, which `Order::check` already
  keeps equal to `deposit_failed_at`. It is `value_at` and not `deposit_failed_at` alone because the operator may
  hand on a stuck order that never had a failed deposit, and because after a recalculation that moved the value on
  and failed there, the value sits at the new node, not where the deposit first failed. The refusals are
  `NotLinked` (the second doesn't name the first as `handed_on_from`), `NoValueToHandOn` and `StartsAwayFromTheValue`.
  Core calls it with both orders after opening the second and before the first part's `handed_on`, or after; it holds
  both ways.
- **No new contract need.** Nothing was missing from the contracts at `138281d`. If `T-010-03` publishes the start rule
  or an order-from-parts rule too, `rutent-stm`'s names (`handed_on_route_starts_where_value_sits`, `order_state_of`)
  should give way to the contract's.
- **How the properties were checked.** The implementation existed when the property tests were written, so they were
  not seen failing first; they were checked by mutation instead: dropping the `INV-4` start check, the ended-part
  check, and the segment edge rule each fails at least one property. The scenario tests were written first and seen
  failing against a stub. The "reordered" half of `REQ-STM-4` is exercised by the generator, which delivers segment
  reports out of their natural order (a settlement before its bind, a report on a later segment), and by the segment
  edge check.

## The VOS licence, adopted in part (owner, item `vos-licence-v0`, 2026-11-09)

The owner ruled a partial adoption: "There's nothing ruling that the entire repo must be on a single licence."
AGPL-3.0-or-later applies only to the package that links `vos-sdk`, the VOS program `T-010-05` builds, and to
binaries built from it. That package declares `license = "AGPL-3.0-or-later"`. `components/stm` and every other
package stay MIT and runtime-neutral behind the host contract, so the purpose-built store stays a drop-in fallback.
`licences/RECORD`'s `vos` line reads permitted under `DEC-26` and `vos-licence-v0`.

## T-010-05: the VOS program (`components/stm/vos-program/`)

Built on `T-010-04`'s library at `bf6502d`; nothing in `contracts/` was needed or edited.

### What was built

- **The program** (`components/stm/vos-program/`, package `rutent-stm-vos-program`): `EXP-1`'s re-score program with
  the suite's probe replaced by `rutent_stm::Stm`, which it builds `no_std` (`default-features = false`). The program's
  feature `probe-machine` swaps the probe back in, through the generated no_std copy of conformance's probe, its patch
  and `nostd.sh`, carried over from the adapter: see "The suite is typed on its probe" below. `EXP-1`'s `next` and
  `next-format` versions (`R-06a`, `R-06b`) are not carried over: nothing here upgrades a running program. It is a
  cargo workspace of its own with its own lock and the guest toolchain (`nightly-2026-03-20`).
- **The thin `Host` client** (`components/stm/vos-program/client/`, package `rutent-stm-vos`): the re-score's client,
  `Host<M>` for any `M` (see the readings), its node processes (one node or three) and its platform assertion, behind
  its own `vos` feature, off by default; a workspace of its own with the node toolchain (`nightly-2025-07-14`).
- **The retry** (`rutent_stm::open_retry`, MIT, `std`): `retry_open(policy, retryable, open)`, bounded (40 tries 10 ms
  apart, `OpenRetry::SPAWN_WINDOW`). The purpose-built store retries only "the store is open in another host"; the
  client retries a node that "exited before it started" when the cluster being opened already exists. Another failure returns at
  once, and a store held by a live second host is still refused after about 400 ms.
- **Sources** (`G-48`): `components/stm/vos-program/sources` lists `name revision upstream-url fetch-url` for `vos-core`
  and its three forks. The scripts read it, derive git's `insteadOf` rewrites from it, and `sources.sh check` fails
  when a manifest or lockfile pins another revision. Fetching is from upstream for now.
- **Licence check**: it now reads every lock under `components/` as well as `experiments/` (`LOCK_DIRS`, a test written
  first), since the program's locks live there; `licences/RECORD`'s header says so, and it has a line for
  `rutent-stm-vos-program` (AGPL-3.0-or-later, `DEC-26,vos-licence-v0`; the client's lock names it as a path
  dependency outside its own workspace). Every other package the two new locks name was already recorded.
- **CI**: `.github/workflows/stm-vos.yml`, modelled on `exp1-vos.yml` (a push to this branch under `components/stm/`,
  or a dispatch; never a pull request). It is a `DEC-20` path.

### Readings taken where `PLAN.md` is silent or the row's wording does not fit

- **"A `vos` feature of `rutent-stm`".** Read as a `vos` feature of the client's own package, `rutent-stm-vos`. A
  feature of `rutent-stm` that pulls the client or the program would (a) make `rutent-stm` depend on a package that
  depends on it, (b) put `vos-node`, `vos-wire` and their git sources into rutent's root `Cargo.lock`, even with the
  feature off, which breaks "`cargo test --workspace` never needs VOS" and moves the lock reached from
  `rutent-contracts` (`T-010-02`'s guard), and (c) link AGPL crates into the MIT library, outside the owner's ruling
  (AGPL covers only the packages that link VOS code and the binaries built from them). The client links `vos-node` and
  the program's types, so it is AGPL-3.0-or-later with the program: the owner's partial adoption covers the packages
  that link VOS code, the VOS program and its client, and the client's manifest declares it. Off by default holds: nothing of VOS builds
  without the flag, and neither package is in rutent's workspace. I did not run the scratch change that would show
  (b); it rests on cargo locking optional git dependencies.
- **A binary built from the AGPL package is AGPL.** A node or a client binary built here is under the ruling; shipping
  one is `F-034`'s question (`G-46`), not this task's.
- **The suite is typed on its probe, so a host running `Stm` can't pass it** (found by running it). The suite on the
  program with `Stm` failed three cells: `stop-after-computed` (it needs the faults node build, which holds commits),
  and `reordered-delivery-is-applied-in-submit-order` and `replay-reaches-the-same-states`, which compare the host's
  recorded states with the probe's over shuffled inputs, where `Stm` (forward-only segments, `INV-4`) rightly differs
  from the probe. `conformance/` is not this task's to edit, so the reading: the suite scores the host's mechanics on
  the program built with the probe (`probe-machine`, `elf/probe.elf`, the nodes the faults build), as `EXP-1` did, and
  `the_program_records_the_states_the_state_machine_library_computes` scores `Stm` on the nodes: one node, an order to
  completion, one with a failed final deposit and one shuffled, each journal's states equal to a native `Stm` fold.
  The client is `Host<M>` for any `M` so the suite can drive it as `Harness<Probe>`. A suite that takes its state
  machine as a parameter would remove the second program; that is an edit to `conformance/`, for the owner or a later
  task.
- **The retry's two predicates.** The store matches its own refusal text, exported as `STORE_OPEN_ELSEWHERE`. The VOS
  client has no refusal text of VOS's to match, so it retries a node that exited before it started (not one whose 60 s wait ran out) on a cluster that exists, and
  never on a fresh one. If a node fails to start for another reason, it costs about 400 ms per node open before the
  error returns.
- **Tests that need the toolchain.** `killing_the_leader_mid_submit_loses_no_acknowledged_record` is a smaller test than
  `EXP-1`'s `R-01`: three rounds, each killing the current leader once a submit of a stream is in flight (an in-flight
  marker is set before the call and cleared after it; the test requires that at least one round's kill landed in
  one, and all three did in the development run), resubmitting what was not acknowledged, restarting the node, and checking each acknowledged record is in a surviving node's journal
  once. `R-01`'s 20 random kill points and its `hold_commits` placement (a faults build) are not repeated here. The
  nodes are the faults build, for the suite's `AfterComputed` stop.
- **Test order.** The open-retry tests of `rutent-stm` were written first and failed to compile (no `open_retry`
  module); the store's retry test was seen failing on "the store is open in another host". The client's retry test was
  seen failing with its retry turned off. The suite and kill tests were written against the re-score's client and were
  not seen failing before it existed.

### What was run, and what was not

- Built here (macOS arm64): VOS's agent and the program (both ways) for the guest target, the faults node, and the
  client, all at `5c78975`. Run here, as a **development run** (`VOS_DEV=1`, the interpreter, which is not scored): all
  four tests of `vos_node.rs` passed (the suite on one node and on three, the states test, the leader kill), and so
  did the two retry tests. The nodes on this Mac are real processes of VOS's own node, but off Linux x86_64 with the
  JIT their result is not the task's.
- Not run: the host-contract suite on one node and on three, and the leader kill, **on Linux x86_64 with the JIT**, the
  only scored platform, and so the run id the `Done when` wants. They run in `stm-vos.yml` once the branch is pushed;
  the run id goes here then. The licence check is green on this branch, which `Done when` also names.
- **How the CI run happens.** As far as I know GitHub runs a `workflow_dispatch` only for a workflow file on the
  default branch (`main`), so before the release into `main` the only trigger of `stm-vos.yml` is a push of
  `f-010/t-010-05-vos-program`. A task agent does not push: the orchestrating session pushes that branch to get the
  run, and the run id goes here. I did not check this against GitHub's documentation.
- **Checks run locally** besides the brief's list: CI's `cargo build --workspace --all-targets`, the `no_std`
  builds of `rutent-contracts` and `rutent-stm` for `riscv32imac-unknown-none-elf`, the purpose-built-store docs and
  tests, and `contract-guard` against `bf6502d` (no violations; the root `Cargo.lock` is unchanged).
- **Private mirrors.** `bloque-app/vos-mirror`, `bloque-app/lsm-tree`, `bloque-app/stwo`, `bloque-app/s4` are the
  fallback, recorded in the program's README. Pointing CI at them needs a token that can read private repositories
  (a deploy key or a fine-grained token in a secret), which `stm-vos.yml` does not have; `sources` takes the mirror
  URLs, and the workflow would need the credential in git's configuration for the fetch.
- **Read before build.** The fetched VOS tree was skimmed, not audited: its five `build.rs` files (they read
  environment variables and write to `OUT_DIR`, and the phone tool's embeds two ELFs), its `.cargo/config.toml` files
  and `blobs.lock`. It sits under `target/` and its lockfiles stay out of the tracked tree.

## T-010-03: answering the review of `e1c7cf2`

- **The tie check's cost.** Expanding a double's exact value (`{:.780e}`) for every shortest form ending in an odd
  digit made a megabyte of `[1,1,…]` cost seconds inside `signing_input`, on untrusted text. The expansion now runs
  only where an even neighbour of the shortest digits reads back as the same double, which needs many digits; a
  unit test counts expansions and finds none over a megabyte of short numbers and one for Appendix B's tie.
- **Non-finite values.** `to_canonical` now refuses NaN and the infinities (`NonFiniteNumber`) with a serialiser
  that writes nothing and checks every float before `serde_json` writes the value. Folded into provider-role 2.3.0,
  which no release carries yet; the guard compares with `m2`, where provider-role is 2.2.0.
- **Noncharacters.** The parser refuses U+FDD0 to U+FDEF and the last two code points of every plane, written or
  escaped (RFC 7493 §2.1), so the "I-JSON" claim holds as written. Also folded into 2.3.0.
- **An unanswered call to a widened callee.** The module-boundary suite took it as unclear; it now counts as reached
  when the callee received it, since being passed on is what lends the widening. The reference double gains
  `with_callees_that_never_answer`, which records every call it passes on as `unanswered`, for the suite's test. No
  contract changes with it.
- **Ruling: edge-only lock changes (orchestrator's default, reversible).** `T-013-01` adds Wasmtime, whose `serde` and
  `allocator-api2` features give bumpalo, hashbrown, referencing and smallvec new dependency edges in `Cargo.lock`,
  with no contract text changed. The limit above had bitten, so the orchestrator ruled: a reached package whose name,
  version, source or checksum changes (or that appears or vanishes) still needs every contract's bump; one whose only
  change is its dependency list needs no bump, only a bullet in `contracts/CHANGELOG.md` naming a `DEC-*`, as a shared
  file does. Features can change behaviour (`serde_json`'s `preserve_order`), so the change stays visible and
  reviewed, without fake bumps. Reversible: restore the single identity in `lock_moves`.


## T-010-05: answering the review of `6b73945`

- The suite also runs on `STM_PROGRAM_ELF`, the real `Stm` program, on one node and on three, excusing only the two
  cells that compare recorded states with the probe's (`reordered-delivery-is-applied-in-submit-order`,
  `replay-reaches-the-same-states`); the states test runs on one node and on three. The probe-built program stays for
  the suite's full pass.
- `platform.rs` has the adapter's five `assert_jit` unit tests and its perf-map check back, as `backend_proof`, which
  reads `STARTED` and `PROBED` and ends each node test.
- The workflow also runs on pushes to `m*` and `develop`, and on `contracts/` and `conformance/`.
- `sources.sh check` fails on a missing pin. The client declares AGPL-3.0-or-later. Not run for these changes: the
  Linux/JIT run.

## T-010-05's scored run (2026-11-09)

The host-contract suite passed on one VOS node and on three, on Linux x86_64 with the JIT, in CI: workflow
`STM on VOS`, run 42675856453, at `dbf5222` (jobs `sources` and `vos` both green). The suite ran on the probe program
and on the real `Stm` program, excusing only the two cells that compare states against the probe; the states test
and `killing_the_leader_mid_submit_loses_no_acknowledged_record` passed on both setups. The first run, 42669881289 at
`6b73945`, was before the review's fixes.
