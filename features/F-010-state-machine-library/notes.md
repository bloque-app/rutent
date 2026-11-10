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
  Core must call it with both orders only after the first part's `handed_on` (corrected 2026-11-10, the review of
  `M2`'s pull request): before that, every part that waits for the customer or is stuck holds value for any order
  that names the first as `handed_on_from`, so the rule admits more than one second order (`hand_on.rs`,
  `holds_value_for`), which `INV-4` forbids. Either Core's own rule is that it calls this only after `handed_on`, or
  the rule is changed to require `HandedOn` with `linked_order == second`, and a two-second-orders test added. This is
  an `M3` item (`T-020-13`), listed in the `M2` retrospective.
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

## T-010-08: the production-host check, the build without native codegen, and the recovery drill

- **The check** is `components/stm/vos-program/host-check.sh BUILD_DIR` (with `VOS_DIR`). It reports and fails on each
  of: Linux x86_64 with a kernel of 6.7 or later; a userfaultfd that gives asynchronous write-protect to an
  unprivileged process (a Python 3 `ctypes` probe, standard library only, that makes the calls `platform.rs`'s
  `uffd_probe` makes: `userfaultfd` with `UFFD_USER_MODE_ONLY`, `UFFDIO_API` asking for `WP_ASYNC`, `/proc/self/pagemap`);
  and the JIT's perf map, by running the client's test `a_vos_node_on_the_jit_names_the_program_in_its_perf_map` (one
  node on the JIT, one record, `backend_proof`). It checks all three and does not stop at the first failure. With no
  build directory the perf-map check fails, since a skipped check is not a pass.
- **Hooks.** `HOST_CHECK_OS`, `HOST_CHECK_KERNEL`, `HOST_CHECK_UFFD` and `HOST_CHECK_PERF` replace a fact so the tests
  can stand in for a host that lacks it. They are in the production script, which is a small risk (anyone can fake a
  pass in their own shell); the alternative, a second script for the tests, would leave the tested script not the one
  that runs. `F-030` runs it with none set. Reading: the hooks stay.
- **Tests.** `client/tests/host_check.rs`: seven, none needing VOS or a node, led by
  `host_check_fails_on_a_host_without_userfaultfd_write_protect`. I saw them fail with the script absent, then pass.
  They were compiled and run with plain `rustc --test` here, since the client's cargo build resolves VOS's git
  sources, which this machine did not fetch. CI's step `The host check's own tests` runs them through `test.sh`.
  A shell test has no tag syntax in `tag-check`; a Rust test driving the script keeps `// verifies:` working.
- **Without native codegen.** `G-37`'s finding was VOS's `.cargo/config.toml` pinning `target-cpu=native` for
  `x86_64-unknown-linux-gnu`. `PORTABLE=1 STEP=node build.sh DIR` builds the node with `RUSTFLAGS="-C target-cpu=x86-64"`
  (an environment `RUSTFLAGS` replaces the config's rustflags), with `cargo build -v`, and fails unless the log shows
  rustc ran with `target-cpu=x86-64` and never with `target-cpu=native`. The guests are riscv builds and the client
  has no such config, so only the node changes. I read "native codegen" as that CPU pin; the JIT itself stays on, since
  the node's backend is a runtime choice. The workflow runs `the_host_contract_suite` (four tests, the probe and the
  program on one node and on three) on that node. If VOS's checkout keeps its `target-cpu=native` somewhere
  `RUSTFLAGS` does not reach, the build fails by its own check; I could not see VOS's config offline.
- **The runbook** is `components/stm/vos-program/RECOVERY.md`: scheduled halted exports, the halt each costs and the
  restore's downtime at 1k, 10k and 100k records (`R-08`), the RPO as the export interval, and what it does not cover
  (power loss, `G-54`; upstream `vos node restore`; where streams live and who schedules them, which are `F-030`'s).
  No exporter program exists yet: the schedule is a procedure over `Host::halt`, `export` and `resume`, which
  `F-030` wires. Reading: the task asks for the runbook and the drill, not a scheduler.
- **The drill** is `halted_export_imported_onto_fresh_nodes_keeps_every_record_acknowledged_before_it` in
  `vos_node.rs`, on three nodes: submit two orders, halt, export, kill the cluster and delete its data, import onto
  three fresh nodes, check each acknowledged record is held once at its sequence number and each journal is equal.
  It and the perf-map test were written against the library's API and not compiled or run here.
- **Workflow.** Steps added to the `vos` job of `STM on VOS`, no new workflow: the check on the runner, then the
  same script on its fixture (`HOST_CHECK_UFFD=false`), which must fail; the check's tests; the portable node and the
  suite on it. The drill runs in the existing full-suite step. The branch is added to the push list. This is a
  `DEC-20` path.
- **Only CI can prove**: the check passing on the runner (it needs Linux x86_64, kernel 7.0 and the JIT); the node
  without native codegen building and the suite passing on it; the drill and the perf-map test compiling and passing.
  The run id goes here once the orchestrating session pushes the branch. The restore's time at 100k records is not
  re-measured here.

## T-010-08: answering the review of `5850eda`

- A `-rc` kernel of 6.7 now fails the check (`6.7-rc1`, `6.7.0-rc3-generic`), with tests.
- The userfaultfd probe now also registers an anonymous mapping in write-protect mode and write-protects one page
  (`UFFDIO_REGISTER`, `UFFDIO_WRITEPROTECT`), after `UFFDIO_API`. Written from the uapi's ioctl numbers and not run
  on Linux here; the node on the JIT (step 3) stays the real test, and CI's run is the first of this probe.
- `PORTABLE=1` replaces all of VOS's rustflags through `RUSTFLAGS`. Read at the pin through the GitHub contents API,
  `.cargo/config.toml` has one flag for x86_64 Linux, `-C target-cpu=native`, none under `[build]`, and an aarch64
  section with none; so nothing else is lost. The build now checks that before it builds and fails if the file gains
  another flag, so the substitution can't drop one unnoticed.

## T-010-08: the userfaultfd probe's struct sizes (CI runs 42694618063 and 42696178411)

- **Cause.** `ctypes.create_string_buffer(bytes)` allocates one byte more than it is given, for a NUL, so the
  24-byte `uffdio_api` came back as a 25-byte `.raw` and `struct.unpack("QQQ", ...)` raised. The runner's kernel was
  fine; the probe's own bug read as "not allowed". Runs `5850eda` and `403705f` both failed there.
- **Fix.** The probe is now `uffd-probe.py`; every ioctl buffer is built with its exact size (24, 32, 24) and the
  result read from that size. Exit 0 means write-protect works, 1 means the host does not allow it (EPERM, EACCES,
  ENOSYS, EINVAL or the feature bit missing), 2 means the probe itself failed on any other errno or any Python
  exception; `host-check.sh` fails on 2 and says "script error", so a bug can no longer pass for a verdict on the host.
- **Tests.** `UFFD_PROBE_FAKE` runs the same code against a fake kernel that asserts each ioctl's buffer has its
  struct's exact size, which the old code fails; four new tests cover success, a refusal and a missing feature bit
  as 1, an unexpected errno as 2, and `host-check.sh`'s message for a failed probe. Not run on Linux here.

## T-010-09: the refusal reasons `M2`'s tasks found missing

### What was built

- **One bump, module-interface 4.0.0.** Every type the reasons live in is module-interface's (`RefusalReason`,
  `SecretRefusalReason`, `CallRefusalReason`, `AdmissionRefusal`), so provider-role does not move. New Rust variants
  break an exhaustive match, as 3.0.0 ruled, so this is a major. `VERSION` is `4.0.0`; the conformance suites read
  the const, so no literal pin moved.
- **The reasons.** `CallRefusalReason` gains `limit_reached` and `deadline_too_near`; `RefusalReason` gains
  `peer_not_verified`; `SecretRefusalReason` gains `store_unavailable`; `AdmissionRefusal` gains `AdmissionPending`
  and `AssertionUnverified` (the coordinator's addition from `T-013-04`). Each is in the schema's enum and the Rust
  type. Valid fixtures name each wire reason; invalid ones are an unknown outbound reason, an unknown secret reason,
  and a limit refusal of an undeclared callee (`RefusalReasonUntrue`).
- **The record rule.** `validate_record` accepts an `unknown` call whose last hop matched a declared destination and
  has no address: its dial connected to nothing. An unconnected hop before the last, and an `unknown` call ending at
  a hop that matched no destination, stay `HopNotConnected`, each with a fixture.
- **Call sites.** `module/platform.rs`: a call over a count or depth bound is `limit_reached` (unless an earlier
  check already refused it), a callee with too little of the caller's deadline left is `deadline_too_near`, and only
  a callee not running is `callee_unavailable`. `egress/layer.rs`: `Refusal::NotVerified` is `peer_not_verified`,
  and a hop names its address only when a connection was made (not after `NotVerified` or `Unreachable`).
  `proxy/shared.rs`: every store failure is `store_unavailable`, so `wire_reason` and `answer` are total.
- **Tests.** The row's three, in `components/prov`: `call_refused_by_a_limit_names_its_own_reason`,
  `outbound_call_that_failed_its_tls_check_names_its_own_reason` and
  `secret_store_that_gave_no_answer_is_not_reported_as_not_found`. In the contract: one per reason and one for the
  record rule. Each failed first (the prov ones at run time, on the old reasons; the contract ones on the missing
  variants). Existing tests that held the placeholders now hold the new reasons; one was renamed, since its name
  said "the address dialled" (`call_with_no_answer_is_recorded_as_unknown_and_one_that_connected_to_nothing_names_no_address`),
  and `refusals_map_onto_the_contracts_two_reasons` is now `..._three_reasons`.

### Readings taken where `PLAN.md` is silent

- **`#[non_exhaustive]` on all five reason enums, in this bump** (`PlatformRefusal` added after review). A Rust
  match on one now needs a wildcard arm, which spares Rust callers a broken build when a reason is added. It is not
  a promise that a later reason is minor: the contract is language-neutral (`DEC-4`), a reader on an older schema
  rejects a new wire value, so that stays a major. No workspace crate or experiment matched one exhaustively, so
  nothing else changed. The outcome enums (`CallOutcome`, `CrossModuleOutcome`, `SecretOutcome`) are left exhaustive: a new
  outcome changes what a record means, which stays a major.
- **Two call reasons, not one.** The row names "a limit, or too little of the caller's deadline left". They differ in
  the platform's own state: a count or depth bound ends the caller's operation as a limit (`LimitEvent`), a short
  deadline leaves it going and records no limit. One reason would contradict the platform's record, so each has
  its own.
- **The fourth item is a rule, not a reason.** An `unknown` call that connected to nothing keeps its status: the
  caller still checks rather than assuming nothing happened, as `T-014-03` read it. What changes is that the record
  now says truthfully that nothing connected.
- **`store_unavailable` covers every store failure,** denied token, unavailable or unexpected answer, malformed body,
  transport, an unaddressable name: one reason, so the module learns nothing about the store or the deployment.
- **Not fixtures for `AdmissionRefusal`.** It has no serialised shape in the contract (Rust only, since 2.3.0), so
  its two reasons have a Rust test and no JSON fixtures. Adding a schema for it is outside this row.
- **Left to other rows.** `World::secret` still returns `Option`, so the platform's own lookup path cannot yet tell
  a failed store from a missing secret; `T-014-05`'s row owns that hook change. The platform's `World::route` →
  `None` path still records `failed` with an unconnected hop, which `validate_record` refuses; the reference double
  in `conformance/` does the same. `T-014-05` replaces that loop with `Egress`, which records it as `unknown` with
  no address. The call sites of `AdmissionPending` and `AssertionUnverified` are `T-013-04`'s (branch
  `f-013/t-013-04-admission`, not yet in `m2`), which adopts them when it integrates, in place of
  `VersionAlreadyAdmitted` and its platform-only `Admitted::Unverified`.
- **No shared file changed,** so `contracts/CHANGELOG.md` has no entry; `T-011-05` edits router-api beside this.

## T-010-09: answering the review of `1689b51`

- `PlatformRefusal` is `#[non_exhaustive]` too, so every reason enum a platform gives is, as the README and the
  module docs say.
- The changelog, README and docs no longer claim a later reason is a minor version: `#[non_exhaustive]` spares
  Rust callers only, and a new wire value is still a major under `DEC-4`.
- The changelog says the two admission reasons are Rust only, with a Rust test and no fixtures, as 2.3.0's were.
- The admission test is renamed to what it checks
  (`admission_pending_and_assertion_unverified_are_distinct_reasons_that_print_their_names`) and tagged `DEC-4`.
- For `T-014-05`, in the F-014 notes: `platform.rs`'s lookup reply sends every reason but `not_found` as
  `"undeclared_secret"`, so `store_unavailable` needs its own arm when `World::secret` changes.

## T-010-10: the conformance suites sign for real

`DEC-20` paths: `conformance/` and, with it, module admission (`components/prov/src/module/admission.rs`, sealed).

- **Keys and signing live in `conformance/src/signing.rs`** (feature `signing`; `test-support` implies it), built on
  `ed25519-dalek` 2.2.0 with default features off (BSD-3-Clause, already in `licences/RECORD`, so no new line; the
  licence check passes). Keys are derived from a label and a number through the suite's own SHA-256, so a run is
  the same everywhere and no random source is needed. The feature is off by default so the role stand-ins the
  guests compile for `wasm32` stay free of it; `boundary` and `signing` need it. Nothing imports `components/prov`
  (`DEC-2`).
- **Boundary suite.** Each provider's declaration names its real statements key; its release assertions are signed
  with its advertising key over `signing_input`; before a provider's first release the suite hands the binding its
  signed advertisement through a new `BoundaryHarness::list_provider`. `BoundaryHarness` gains a required method,
  so `experiments/` bindings (not in the workspace, and still on placeholders) need it if they are revived.
- **A forged assertion fails the suite.** New fixture `Exercised::ForgedReleaseRefused` (ordinal 41, 42 in all):
  an assertion signed by a key no advertisement lists, and one altered after it was signed, must each be refused
  as `AssertionUnverified`, and the same release signed right admitted. A binding that trusts every assertion is
  flagged by `AdmissionRecordsWhatItAdmits` alone (`ReferenceDouble::trusting_every_assertion`).
- **Role suite.** `signing::Signing` (moved from `components/prov/tests/role_suite.rs`) signs every statement a
  stand-in sends, nested ones first, with the key its placeholder stands for; `signing::Ed25519Verifier` verifies
  strictly. The suite passes with `signatures_deferred == 0` and nothing deferred, and flags a statement signed by
  another key (`MakerAndKey`) or altered after signing (`SignatureVerifies`).
- **Reading: what "role suite passes with every statement verified" covers.** The stand-ins sign for real
  in-process (through `Signing`) and the suite's own verifier checks all of them; through the platform
  (`module_role.rs`) the role modules' release assertions and advertisements are signed for real and admitted by the
  catalogue, but the results the platform sends still carry the platform's placeholder signatures, since the
  platform holds no private key for a statement key. That run keeps `SignatureVerifies` deferred. Result signing
  is outside this row.
- **Left alone.** The suites' operation statements still carry the operator's placeholder signature: no platform
  verifies an operator's statement yet, and the operator key is a deployment's, not a provider's.
- **Test-first.** The ordinal list, the forged fixture and the double's breaker were built together, so no test ran
  against a stub; the test that shows the suite failing is the one that runs the trusting double and asserts it is
  flagged, with both forgeries named. Before this task the same double could not be flagged, as the suite never
  submitted a signed assertion.
- **Tests.** `conformance/tests/boundary.rs`: `boundary_suite_refuses_a_release_whose_assertion_is_signed_by_another_key`,
  and the trusting double in `every_broken_boundary_is_flagged_by_its_own_check_only`.
  `conformance/tests/role_signed.rs`: `role_suite_passes_with_every_statement_verified` and
  `role_suite_flags_a_statement_signed_by_another_key_or_altered_after_signing`.
- **Also in `components/prov/tests/module_boundary.rs`:** the same-named test runs the forged assertion through the
  Wasm binding and its catalogue, and each category's run must exercise `ForgedReleaseRefused` in both languages.
  `signing::signed_inside` re-signs only the statements nested in a platform's answer (a verifier's judgement), as
  the platform signs its own result.

## T-010-10: answering the review

- The bindings under `experiments/` need the `signing` feature of `rutent-conformance` (the `boundary` module is
  behind it), as well as `list_provider`, if they are revived.
- CI's build and documentation steps pass `--features rutent-conformance/signing`, so `boundary` and `signing` are
  built and documented in the existing `ci` job. `signing::signature_bytes` now takes only canonical signatures
  (`role::is_canonical_signature`), so a final character with unused bits set is rejected.
- `ReleaseJudge`'s doctest is pinned to `E0277`.
## T-010-06: the `G-54` test, and the purpose-built store's two fixes

`G-54`: damage to the frame holding the last acknowledged append, which a store can mistake for a torn, unacknowledged
write and cut without a word. `T-010-06` tests VOS for it and fixes the purpose-built store, the fallback (`DEC-6`,
ADR-0002), for it and for the memory it held.

### The purpose-built store refuses a damaged acknowledged last frame

- **The mark.** After the log's sync, an append writes `log.mark` in place and syncs it: where the frame ends, that
  frame's checksum and a checksum of the two, 16 bytes, before `submit` returns. It is a copy of the last acknowledged
  frame's identity kept apart from the log, the only thing the module's old note said could tell a torn write from
  later damage. On open, a log whose last whole frame ends before the mark's end, or whose frame at that end has another
  checksum, or that has no frame ending there, is refused with `HostError::Unavailable` naming the acknowledged append,
  and nothing is cut. A tail that starts at or after the mark's end is the append a crash cut short, and is cut as
  before. A mark that is behind the log (the crash fell between the log's sync and the mark's) leaves the frame
  present and the open moves the mark to it.
- **Format 2.** The store's format is now 2, to say it keeps a mark; format 1 never shipped and is not read. A log of
  more than its format frame whose mark is missing or empty is refused (answering the review: deleting the mark must
  not bring the silent cut back); a log of its format frame alone may lack one, and a finished import writes its own.
- **A missing mark** is refused, as above, once the log holds history. The residual is stated in the module's docs:
  damage to the log and the mark both, or a log restored beside a mark from an older copy, is not seen. An empty mark
  file is a new one only beside a log of its format frame alone; a mark of the wrong size or checksum refuses the store.
- **Reading: "acknowledged".** An append is acknowledged when `submit` returns it, which is after the mark's sync; so
  a write that fails at the mark is undone like one that fails at the log's sync, and the old mark goes back with the
  cut (a unit test writes the new mark's bytes, then fails the sync, and fails if the restore is deleted), or a healthy
  log would be refused. A first append that fails leaves an empty log and an empty mark.
- **Cost.** Two syncs per append where it was one. `measurements.rs` has no latency threshold on the store and its
  throughput row (`G-42`) is `#[ignore]`d; it was not re-run here, so the extra sync's price is unmeasured.
- **Tests.** `purpose built store refuses a damaged acknowledged last frame` (six kinds of damage to the last frame,
  nothing cut each time, and the undamaged log opens) and `purpose built store still cuts an append that was never
  acknowledged` (a torn append beside the earlier mark is cut and can be made again; a whole frame ahead of the mark
  is kept; damage after that is refused). The existing torn-tail test is now a log with several frames and
  no mark, which is refused (with a test that deletes or empties a real store's mark). Both new tests were seen failing against the unchanged store first (the store opened and cut the frame).

### The purpose-built store's memory is an index, not the log

- **Reading: "bounds its memory".** The `M1` retrospective names only the line "the store ... bounding its memory". The
  store kept every record it had written, so `G-01`'s 90 MiB journal cost 90 MiB or more, twice that while opening
  (it read the whole log into one buffer and then parsed it). The reading taken: what an open host retains must not
  follow the size of the records or of the log, and opening must not hold the log. It is not constant in the number
  of records, which an index that dedups record ids can't be; each record costs one 16-byte place and two 128-bit keys.
- **What it holds.** Where each record's frame is (offset and length), by order and by record id key, and the
  refusals' places. Every read goes to the log by position and checks the frame's checksum again, so damage after
  an open is an error, never a wrong answer. Keys are two hashes under the process's random keys, so a record id
  of any length costs the same, and each read checks the id it finds against the one asked for (a collision is an
  `Unavailable`, which no log is expected to hold). Opening reads a frame at a time; only a bad tail is read whole.
  An export reads each record as it reaches it.
- **Not bounded.** The index grows by one small entry per record; the contract's `StreamCheck`, which an open runs
  over the log, holds each record id and order id until the open ends (it is the contract's, not this task's); a
  frame is as large as its entry, up to 64 MiB; a journal read returns one order's records at once.
- **Test.** `purpose built store's memory stays bounded as its history grows`, in its own test binary
  (`tests/purpose_built_store_memory.rs`) so that its counting allocator sees nothing else. Three histories, 2 KiB
  and 64 KiB records, and 400 of the large ones (log 0.7, 13 and 27 MB): what an open host retains was 14, 16 and 28
  KB, the same for records thirty times larger (a limit of 16 KB more), under 2 % of the log (a limit), and the heap
  never rose more than 1 MiB over what the host kept while the log was opened (0.1 to 0.3 MB seen). Against the
  unchanged store the same test failed: it retained 0.7, 13.4 and 26.9 MB, and peaked at twice that.
- **DEC-20 path touched:** `.github/workflows/stm-vos.yml` (a step, below).

### `G-54` on VOS: what the test found (scored: Linux x86_64, the JIT, CI run 42758365383)

> **Corrected 2026-11-10, the review of `M2`'s pull request.** The run below showed the cut at `20567be`, before
> `T-010-07`. At the head, `a3d241d`, run 42772739790 gave `recovered` on one node for the same damage, so the cut
> is not shown at the head. See "At the head" below; the table is what run 42758365383 recorded and stays as it was.

`G-54` (`scoring.md`): damage to the last frame of a store that returned its acknowledgement, **its length intact**,
and a refusal or the entry kept. The test (`components/stm/vos-program/client/tests/damaged_frame.rs`, run by a
`continue-on-error` step of `stm-vos.yml`, its lines in `g54.txt` of the run's artifact) writes one order's inputs
through the real program, measures the journal's length when the node is quiet before the last submit and after it, so
the bytes that submit wrote are a range (`store/0.jnl`, in which the last record id, `rec-ord-g54-7`, sits 14 times:
the append's batch and what is derived from it), kills every node, and damages the range, on one node and on the
leader of three. Each damage is followed by starting the damaged node alone and asking its own replica (an
applied-consistency query), with its log kept.

The scored run: workflow `STM on VOS`, run 42758365383, at `20567be`, branch `m2-ci-t01006`; green, the `G-54` step
being allowed to fail. On one node the last submit wrote bytes 40582..46840 of `store/0.jnl`.

| Damage, inside the last submit's bytes | One node | Leader of three (started alone) |
|---|---|---|
| One byte flipped at the id's first place (length intact; later frames of that submit behind it) | **refused**: the node does not start, `Store(JournalRecovery(ChecksumMismatch))` | **refused**, the same |
| The range's last byte flipped | recovered, 7 of 7 records, nothing in the log | undetermined: the lone node answered `Rejected(Behind)`; with the others up it held all 7, so a loss, if any, was repaired by replication |
| The journal cut just after the id's first place (a torn tail) | **cut silently**: 6 of 7 records, no log line naming damage | **cut silently** locally (6 of 7, no log line), then repaired by replication: it caught up from the others |

- **What the test can and cannot target.** It does not read fjall's frame boundaries, so it cannot isolate "the frame
  that holds the append" or put length-intact damage on a final frame with nothing behind it. The first place of the id
  is taken as inside the append's batch (the id's first byte in the range), not proven to be. The last-byte row
  recovered all 7 records, so that byte belongs to a frame after the one that holds the record, or the tail was cut
  without a word and the record sat earlier; the run cannot tell which.
- **Reading.** A byte flipped inside the append's batch with frames behind it is refused (the journal's recovery reports
  a checksum mismatch and the node does not start). A torn tail is cut and the node starts without a word, losing an
  acknowledged record. The case `G-54` names, length-intact damage to the acknowledged batch with nothing behind it, is
  **not shown** by the run: for it there is only the code reading that `EXP-1-vos-20261107-1` recorded (fjall's
  `TolerateCorruptTail` recovery discards a corrupt tail without an error).
- **Not tried.** Power loss (a real torn write); a cut at a frame boundary. The VOS tests (two then, four now) fail on a
  silent cut by design (they need not pass); they did in this run, and the job stayed green.

#### At the head (run 42772739790 at `a3d241d`; the test since changed)

- **Run 42758365383 (`20567be`) showed the cut:** on one node the torn tail lost 1 of 7 records with nothing in the
  log, the table above.
- **Run 42772739790 (`a3d241d`) gave `recovered`** for the same damage on one node. The likely reason, read and not
  traced: `T-010-07` makes an order's last input seal the journal, and the test's last submit ended the order, so the
  heuristic cut (just after the id's first place) no longer cost the record. Nothing in the program handles a torn
  tail, so the residual is probably still there; the test no longer shows it.
- **The test changed** (`damaged_frame.rs`): the last submit is now of two kinds, each its own test: one that ends the
  order (seven inputs) and one that leaves it running (six, the last binding the order's last segment), so the
  record is the journal's live tail. A cut that leaves every record is now reported `not reproduced` and not
  `recovered`, and the test fails on it, since the run then says nothing about a torn tail; a flipped byte that
  leaves every record is still `recovered`. As before, the result is recorded and the step need not pass.
- **The new test's run will be recorded here when it lands.** Until then the torn tail is shown only by run
  42758365383, before `T-010-07`.

### Owner's item: `vos-g54-silent-tail-cut` (confirmed by the scored run)

Evidence: torn tail, **shown** in run 42758365383 at `20567be` (not reproduced at the head, above): a journal cut inside the acknowledged append's bytes is cut silently
by VOS on one node (6 of 7 records, nothing in the log), and on three the leader cut it and the others repaired it.
Length-intact damage to the acknowledged batch with nothing behind it: **code reading only** (`EXP-1-vos-20261107-1`).
Damage with frames behind it is refused (shown). Choose:

1. **Fix it upstream** (recommended): ask VOS's maintainers for a refusal or a log line and a node status flag when
   journal recovery cuts a tail, and for an option to fail instead of cut; or, as a program-side guard, a watermark the
   program writes outside the journal that a restart compares with what recovered.
2. **Accept the residual for both candidates**: on three replicas a node that cut its tail is repaired by the others,
   so only a loss of every replica's tail together is unprotected; `pb` now refuses (this task) and VOS does not.
3. Other (in notes).

Reversible default: option 1 (the recommendation), asked upstream while `M2` goes on with option 2's residual in
force. The item waits on the form; nothing else of F-010 waits on it.


## T-010-07: journal growth bounded (`G-33`)

- **What it does.** The VOS program archives an order when a record ends it (`rutent_stm::archive::is_archivable`:
  complete, failed, handed on, or ended without completing, which is `is_ended`). The step that writes that record
  packs the order's whole journal into rows of a new `chunks` table, each at most 32 KiB, and deletes its live `records` rows. A
  record written to an archived order afterwards (a repeat, a late input, the undo of a complete part, which
  `machine.rs` still allows) is appended to the row. Stuck and waiting orders stay live: someone can still resolve them.
- **Library.** `rutent_stm::archive` (`no_std` + `alloc`, no new dependency): `Entry`, `pack`, `unpack` (refuses a
  wrong header, a cut, a count the bytes cannot hold, trailing bytes), `is_archivable`, and `replay`, the host's own
  fold of `open` and `transition` over archived inputs. The format (`RSA1`, length-prefixed entries) is the library's;
  entries are the canonical JSON the program already stores, byte for byte.
- **The host contract is not touched.** The host contract's `replay` guarantee says a journal is complete and never
  compacted, so archiving is invisible through `journal`, `state`, `orders`, `export`, a refusal's pointer to its
  record and the effect driver's cursor: all of them read through `row_of`, which falls back to the archive. No
  `DEC-4` bump and no `DEC-20` path (`git diff` of `contracts/` is empty). The program gains a query, `archived`,
  that serves an order's packed row so it can be replayed outside the host.
- **What is bounded, and what is not (reading, reversible).** Bounded: the live `records` table, the payload, which
  holds only orders still open (`Status::live_records`, `archived_orders`). Not bounded: the index rows `written`
  (write order, which the export and the effect driver's cursor need), `ids` (the record-id check across the whole
  host), `orders` and `places`, each small and fixed-size, plus the archive rows themselves, one per ended order. So
  total storage still grows with the stream; it grows by the compact archive, and the hot tables the program reads
  and writes on every submit stay small. Bounding the rest needs a cold store outside the actor (copy archive
  rows out, then drop them), which changes what `journal` can answer for a dropped order. That is a host-contract
  question for the owner, not done here. `G-33` is not closed: the live journal is bounded, and total storage awaits the owner's item `vos-storage-bound`.
- **Format.** The program's `FORMAT` is 2 and `PROGRAM` 2: the rows changed (new tables). The struct itself is
  the same as format 1's: the new tables are skipped fields, and the two counters (`live_records`,
  `archived_orders`) live in a table row, so a format-1 instance still decodes in `state::load` and every handler
  and query refuses it by the format check, instead of trapping on load. A test decodes a format-1 struct row.
  No running instance is upgraded by this task (v0; the `next` versions were not carried over, as `T-010-05` recorded).
- **Rows of bounded size (review).** VOS caps a stored value at 64 KiB and one state is a few KB, so one row for
  a whole order could not be written once its journal passed ten or twenty records. An archive is now several rows
  of at most `CHUNK_BYTES` (32 KiB, `rutent_stm::archive::split`), found through a head of each row's first sequence
  number; a late record goes into the last row or a new one. The `archived` query serves one row at a time. A test
  ends an order after forty filler records and appends twenty more.
- **A damaged archive is an error.** `row_in` returns an error for a missing, damaged or short row, which the
  queries refuse as `Refused::Archive` and a submit as `Invalid`: never a record that reads as absent. A page of
  reads unpacks each row once. The `archived` reply is compared with 60 KiB, under the 64 KiB reply cap. The
  damage cases of the lookup (`chunk_of`, `entry_in`) are native tests; the node-level refusal is not exercised
  because the program offers no way to damage a row.
- **Cost.** A late input rewrites the last archive row (at most 32 KiB). The archive step moves one order's rows
  inside one step under the 100M-gas cap.
- **Tests.** `components/stm/tests/archive.rs` (native, in `cargo test --workspace`): an archive replays to the
  journal's recorded states over 600 seeded orders; inputs after the ending are appended and still replay; a damaged
  archive is refused (every cut, trailing byte, wrong header); arbitrary entries round-trip; `is_archivable` is
  `is_ended`. `vos-program/client/tests/journal_bounded.rs` (needs the `vos` feature): `live_journal_stays_bounded_under_a_stream_of_completed_orders` and `archived_order_replays_to_the_same_states` on three nodes, each again on one, the long-journal order, and the format-1 struct row. `stm-vos.yml` runs the file in its own step (a `DEC-20` path). The tests were written first; the native ones failed to
  compile until the module existed.
- **Not proved here.** The VOS tests pass on three nodes with the JIT only in CI (`stm-vos.yml`); the run id goes
  here once the branch is pushed.

### T-010-07: answering the second review

- **Sealing as records arrive.** VOS caps a step's writes at 256 KiB, and ending an order wrote its whole journal.
  Now an order's live records are sealed into archive rows (at most 32 KiB each) as soon as they weigh 32 KiB
  (`archive::seal_due`), and the record that ends the order seals what is left, so no step writes more than a few
  rows whatever the journal's length. `OrderRow` gains `live_bytes`; `ArchiveHead` gains `covered` and `ended`. An
  order that is still open can therefore have archive rows; `archived` serves the sealed ones, and `Status::archived_orders`
  counts ended orders only. The live journal now holds less than 32 KiB of an open order plus one record.
- **A record too large to archive is refused.** Alone in a row, a record must pack under `archive::MAX_ENTRY`
  (60 KiB; the stored cap is 64 KiB less framing). A larger record is refused when it arrives (`Submitted::Invalid`,
  "the record is larger than an archive row holds, so it cannot be archived"), in any order, rather than capping
  records below the cap: a record that near the cap would already be most of VOS's reply. A refused record is
  never written. An import with such a record is refused the same way.
- **Tests.** Native: the packing rule over a one-megabyte journal (the worst step writes under 128 KiB; seen failing
  with sealing at the end only: 1.2 MB in one step) and the record-size edge. Gated: an order of about half a megabyte
  ends and replays (written first; not seen failing, as no node was run here). The format-1 decode test asserts the
  stored format is 1.

### T-010-07: the third review

- **An import step has a write budget.** An import chunk of eight entries could seal eight different orders, each
  writing its live records again as archive rows, and pass VOS's 256 KiB cap on a step's writes: such an export
  could never be imported, and import is the total-loss path. `import_chunk` now charges each entry
  (`archive::import_cost`: its row, and the order's live bytes again if it seals) and stops before the step's total
  would pass `STEP_BUDGET` (200 KiB), always taking at least one. It returns a partial `next`; the client's chunk
  sender resends what was not taken from there and fails if a step takes nothing. No contract changes: the chunk
  call's shape is the same, and a chunk it takes whole behaves as before.
- **The long-order test now reaches the cap.** It submits 260 filler records (about 1.3 KB each in the journal) and
  asserts the journal weighs over 256 KiB before it ends the order. A new gated test exports fourteen orders that
  each seal on consecutive records and imports the export. Neither gated test was seen failing here: no node was run.
  The native budget test was seen failing with `import_cost` stubbed to zero.

### T-010-07: the fourth review

- **The import budget charges a late record for rewriting an ended order's last row.** `archive::import_cost` now
  takes what the order holds (`Held`): live records (bytes and count) or an ended order's last archive row (its
  stored length). A record to an ended order rewrites that row, up to 32 KiB, and its head, so eight of them in
  one chunk could pass 256 KiB while the old charge said a few KB. Deletes the seal makes, and each index put,
  are charged `INDEX_PUT` (48 bytes) instead of a flat 128. The native test fails with the ended charge removed
  (cost 1240 bytes against a 32 KiB row). No gated test was added for this: building ended orders whose last rows
  are nearly full needs the packed length, which a node test cannot steer.
