# F-005 notes

Readings taken where `SPEC.md` or `PLAN.md` is silent or ambiguous, recorded per Narthex `REQ-SKL-24`.

## T-005-01

- Two published lists, not one: order states and segment states, each its own schema (`order-state`,
  `segment-state`), because `REQ-STM-1` gives orders and segments each their own current state and `REQ-STM-2`'s
  table lists them apart. Names are snake_case and close to the table's wording; `REQ-STM-2` says names don't bind.
- "Ended without completing, including cancelled by its reserve" is one state, `ended_without_completing`; the
  cause (a reserve's `withdrawal_cancelled`, or an `end_requested`) stays in the recorded input that reached it
  (`REQ-STM-2`, `REQ-ORD-12`, `REQ-ORD-40`).
- Segments get one further state, `pending`, from when their route is chosen until their provider is bound, so that
  every segment of a chosen route has exactly one state (`REQ-STM-1`); `REQ-STM-2` allows further states. The
  table's "started" begins at binding, as written.
- A part of a split order carries a state from the order-state list, alongside the order's own state, because every
  rule about a route's failure, cancellation, return or stuck state applies to each part and each ends by itself
  (`REQ-ORD-42`). How the order's state follows from its parts' states, with N parts, is `CMP-STM`'s design (M2).
  v0 orders have one part (`DEC-8`, ADR-0006).
- A part is the unit that is handed on, since each part has its own final deposit (`REQ-ORD-43`, `REQ-ORD-42`). A
  handed-on part names its `linked_order`; the new order names `handed_on_from`, so both point to each other
  (`REQ-ORD-43`). An order in `handed_on` has at least one part in `handed_on`.
- An input is "recorded" when it carries a non-empty `record_id` (the id of the record entry written before any
  effect, `INV-7`; unique, so a repeat is recognisable, `INV-8`, `REQ-STM-4`), its `source` (a closed list of
  kinds plus an id) and `standing` (`stated` or `verified`, `REQ-AUD-2`), `recorded_at` with its `time_source`
  (`REQ-AUD-1`: the source of "when" is stated), a `subject`, and what was `observed`. In Rust the record id is a
  newtype that can't be empty, so an unrecorded input can't be built or deserialised (`REQ-STM-3`).
- Observed kinds are a closed list (`REQ-STM-8`: not a free-form engine). Each says whether it is about the order,
  a part or a segment, and the schema checks that the subject names exactly that. Adding a kind before `C1` is a
  minor version bump.
- Time enters only as recorded input. Being stuck "over time" (glossary: twice the p99) is decided outside the
  state machine and arrives as a `stuck_declared` input with reason `over_time`; `transition` reads no clock and
  elapsed time never counts as a segment's outcome (`REQ-STM-3`, `REQ-STM-5`, `INV-7`). A dispute also arrives
  as `stuck_declared` (`REQ-ORD-11`).
- Inputs name parts and segments by id, never by position, so a late report about a segment a recalculation
  replaced matches nothing current and changes nothing (`REQ-STM-4`). Ids being unique within an order is a rule
  the schema can't express; `CMP-STM` enforces it.
- `REQ-STM-7`'s automaton: a segment is a transition from one `(node, asset)` endpoint to another, at one provider,
  with its typed operation (`REQ-ORD-37`). "Changes no value" is read as a transition that keeps the asset (a
  route moving it between nodes); `from` may also equal `to`. Which transitions a route takes enters only as a
  `route_chosen` or `route_recalculated` input, carrying the router's decision record by id (`REQ-STM-3`, SAD §3).
- The route's own shape rules (it starts with a withdrawal and ends with a deposit, `REQ-ORD-16`; parts' amounts
  add up, `REQ-ORD-42`) are the router contract's (`F-004`), not checked here. The state machine tracks states,
  not amounts.
- The router's decision record, evidence and statements are referred to by an id string. `router-api` and
  `provider-role` have no published schemas yet, so a `$ref` to them could not resolve; the operation enum is
  defined here for the same reason. When those contracts land, a later version should refer to their shapes by
  `$id` instead (contracts README).
- The transition's signature takes and returns the whole order (`Order`), since a segment input changes a segment
  inside it: `fn transition(state: &Order, input: &RecordedInput) -> Order`, plus `fn open(input) ->
  Option<Order>` for the input that accepts the route (an order starts only once its quote is accepted,
  `REQ-ORD-34`). Neither takes `self`, so no hidden configuration enters. `transition` is total: an input that
  doesn't apply returns the state unchanged, and stays recorded (`REQ-STM-6`).
- The order keeps `last_record`, the record id of the input that reached its state: the SAD §4 history pointer.
- Fixtures sit flat under `fixtures/valid/` and `fixtures/invalid/`, as the contracts README shows. A fixture's
  file name starts with its shape (`recorded-input-without-record-id.json`); the test takes the longest matching
  shape, so `order-state-...` is an order-state fixture, not an order one.
- The Rust module is a directory (`contracts/src/state_machine/mod.rs`), so the host contract's module can sit
  under it at `T-005-02`, as `contracts/CONTRACTS` maps by directory.
- `T-005-01` depends on `T-001-05` (the contract guard), which isn't on `m1` yet. The contract follows the guard's
  rules as the README states them: a new `## 0.1.0` heading with a bullet naming `DEC-3`, `DEC-4` and the task.
- `recorded-input-bare-observation-without-envelope.json` breaks more than one rule on purpose: it is an
  observation with no envelope at all, the case `REQ-STM-3`'s test names, so the contracts README's
  one-rule-per-invalid-fixture convention gives way here.

## T-005-01, after review (0.2.0)

- Rust is as strict as the schemas (`DEC-4`): non-empty `Id` and `RecordId`, a checked `Timestamp`,
  `deny_unknown_fields` everywhere (field-less observed kinds are empty struct variants so it applies), and
  `Order::check` and `RecordedInput::check`, run on deserialising. Every invalid fixture is rejected by Rust; the
  allow-list is empty. `contracts/state-machine/README.md` lists each rule and where it is enforced.
- The standalone `segment-plan` schema stays open, because the order's segment composes it and adds `state`; the
  shapes that embed it close it. The Rust `SegmentPlan` is closed. An extra field on a segment is tested on an
  order segment.
- Repeats are split (`REQ-STM-4`, `INV-8`, ADR-0002): the host delivers each recorded input exactly once
  (`T-005-02`), and one record id names one content, so a reused id with different content is the record store's
  refusal, never a silent no-op. The state machine reports a repeat only of `last_record` or `last_input`.
- The SAD's signature stays `transition(state, input) → state`; the returned order carries `last_input`, its
  record id and outcome: `applied`, `repeat`, `late` or `not_applicable` with a reason (`REQ-AUD-1`).
  `last_record` stays the input that reached the state.
- "Late" means the input names a segment a recalculation retired. An input after an ending is `not_applicable`
  with reason `ended` (`REQ-STM-6`), and an id the order never had is `unknown_subject`.
- Segment ids are unique for the order's whole life: each part keeps `retired_segments`, the ids a recalculation
  replaced, and `Order::check` refuses any id used twice across current and retired segments. This is the simplest
  enforceable rule, and it lets a late report be told apart from a report about nothing the order ever had.
  Deriving ids from the creating record was rejected because the router assigns them before any record exists.
- Recalculation to the same destination (`route_recalculated`) carries no acceptance: `REQ-ORD-22` makes it
  automatic at the user's cost, `REQ-AUZ-3` bounds it to the authorised destination, to asset and floor, and R49
  (`PS-99`, `PS-100`) closed `OQ-42`: the user covers recalculation costs and accepts that the final price may vary.
  A route past the customer's floor makes the part wait for the customer (`REQ-ORD-27`, `REQ-ORD-4`), so cost
  shows through the floor, not on the input. A route ending elsewhere is `not_applicable` with reason
  `changes_destination` (`INV-10`).
- Another destination is its own kind, `destination_changed`, and it and `handed_on` both require `acceptance`,
  the id of the customer's acceptance record (`REQ-ORD-43`, `REQ-ORD-34`, `REQ-AUZ-3`).
- Ruled since (`spec-no-route-left`, state-machine 0.4.0, below). Was raised to the owner: does "no route left" make an order stuck (glossary, `REQ-ORD-4`) or make it wait for the
  customer (`REQ-ORD-43`)? The contract keeps both open, as `no_route` and `stuck_declared` with reason
  `no_route_left`, until the owner rules.

## T-005-01, after re-review

- Every `Option` field in the contract is an `Option<Id>` (subject part and segment, `handed_on_from` on the
  input and the order, a part's `linked_order`); each deserialises through `some_id`, so `null` is refused and only
  an absent field is `None`. `retired_segments` is a `Vec` and already refuses `null`.
- No leap second: `recorded_at`'s seconds run `00` to `59`, in the `pattern` and in `Timestamp`. A recorded clock
  reading never needs one, and refusing it keeps replay free of a calendar table (`NFR-4`).
- A route that reuses a current or retired segment id is `not_applicable` with reason `reuses_segment_id`, so the
  order keeps passing `Order::check`.
- The null-id fixtures on the subject also break the subject's scope rule, since a `null` field is present in
  JSON; each is named for its null, and Rust's scope check alone would not have caught them.

## T-005-02

- The host is its own contract (`contracts/CONTRACTS`), at `0.1.0`, with `$id`s `urn:rutent:host:<shape>`. Its
  schemas refer to the state machine's `recorded-input` and `order` by `$id`, never copying them. Declaring its
  module and pointing to it from the state machine's docs changed state-machine text, so that contract is now
  `0.3.0`.
- Six guarantees, each with an id, the SPEC ids it serves and the check `T-005-03` runs: `durable`,
  `exactly-once`, `per-order-sequence`, `no-effect-without-record`, `swap`, `replay`. Each of `T-005-03`'s four
  plan tests maps to one, and a test keeps the Rust list, the README table and PLAN's row in step.
- The fault model for `durable` is abrupt termination of the host at any point, then reopening its store. Machine
  loss, disk loss and replication are outside this contract; a runtime may add them, but the suite doesn't score
  them (`NFR-5`, `NFR-6`).
- An *effect* is anything done because of a transition. The host releases a record (by `submit` returning it, or
  by a read) only once it is durable, and effects are driven only from released records, so "a crash between
  record and effect" is scored as: a crash before durability leaves no record and nothing released; a crash after
  it leaves the record, and submitting again returns it (`INV-7`).
- Exactly once is the host's: the state machine sees each record id at most once in the whole host, not per order
  (`INV-8`). `Halted` and `RecordIdReused` are definite refusals; every other error leaves the outcome unknown,
  and submitting the same input again is always safe. That is what makes a retry after a crash correct.
- Inputs for one order are applied in submit order, gapless; the host never sorts them by `recorded_at`, since
  equal times would make replay ambiguous. Reordered arrival is the state machine's to handle (`REQ-STM-4`); the
  host only records the order it applied.
- An input that opens no order and comes before any that does is still recorded, with no state (`REQ-STM-6`,
  `REQ-AUD-1`). Once a record has a state, every later one does.
- The swap is halt, export, import into another implementation, resume. A snapshot carries every journal whole, so
  states, journals and the set of record ids seen all survive, and a repeat from before the swap is still
  recognised. It names both contracts' versions, so an import can refuse what it can't read. Halting the host here
  serves only `REQ-OPS-3`'s second clause, replacing the environment; halting publishing and upgrades in an
  incident is not this contract's, and no test claims it.
- Each record keeps the state it reached, not only its input, so `replay` can be checked state by state (`NFR-4`).
- The fault seam is a trait, `FaultSeam`: `crash_at` a named point (after received, after computed, after
  durable) and `into_store`, then `Host::open` on the store. A runtime in another process meets it by killing that
  process at the point.
- The traits take `&mut self` for submits and are single-threaded; a runtime used from several threads serialises
  one order's submits itself, as `per-order-sequence` states.
- `research/2026-11-02-vos.md` (§3, PS-21 and PS-12) is read as evidence of what a runtime can miss, and each gap
  is stated as a guarantee without naming VOS: a compacted log (`replay`: never compacted); a log of results that
  names inputs by reference (`replay`: inputs whole); a caller's signature dropped at ingress (`replay`: the input
  whole, source included); the leader's clock as "when" (the host passes `recorded_at` unchanged and orders by
  submit, not by any clock); a store in another layout refused at open with no upgrade path (`swap`: a
  runtime-neutral snapshot).

## T-005-02, after review

- `exactly-once` is restated as at most one record per record id in the whole host, because the crash model
  allows a retry after a stop before durability to call the state machine again. That is harmless, since
  `transition` is pure; what can't happen is a second record. Two inputs are the same when their `RecordedInput`
  values are equal, the same canonical JSON.
- A record durable but unacknowledged at a stop is present after reopening; submitting the same input again
  returns it as already recorded, which releases it. That case is scored under `no-effect-without-record`, PLAN's
  "crash between record and effect".
- Effects run at least once per released record, keyed by record id; a repeat reaches a provider only where it
  declares `safe_retry` (`REQ-CAT-10`, `urn:rutent:provider-role:declarations`). The host can't make an effect
  exactly-once; the provider's declaration bounds what a repeat may do (`INV-8`).
- `halt` and `resume` return `Result` and are durable: a halted host reopens halted, so a halted and exported host
  can't reopen and run an order twice.
- Owner's ruling: `FaultSeam` is not in the frozen production contract. It lives in `host::testing`, in the
  contract crate; an in-process candidate implements it, and an out-of-process runtime such as VOS is stopped
  black-box by killing its process, with the named points best-effort. An import can't be crashed through
  `&mut self`, since it constructs the host, so the seam offers `import_crashing_after`, which returns the store
  as the stop leaves it.
- Owner's ruling: `export` streams. The stream is JSON Lines of `snapshot-entry` values: a header with both
  versions, every record and refusal, then an end with the counts, so a stream cut short is told apart. `import`
  reads the same stream and checks it with `StreamCheck`, which holds only each order's last sequence number and
  the record ids seen. Stream fixtures are `.jsonl`, a deviation from the README's plain JSON that a stream needs.
  `orders` lists a running host's orders.
- Owner's ruling: a `RecordIdReused` refusal is recorded whole in a host-wide refusal log, gapless from 1,
  pointing to the record that holds the id (`REQ-AUD-1`). A stream check confirms the pointer by record id; it
  doesn't hold inputs, so it doesn't re-check that the contents differ.
- A crash during an import leaves a store `open` refuses with `ImportIncomplete`; the import is redone on a fresh
  store, since the import marks itself complete only after the end entry.
- Owner's ruling: a swap may halt the host. `NFR-6` sets no time to replace yet (PLAN §6.2: the owner sets it when
  M4 is written); a later version may add a live swap.

## T-005-02, after re-review

- Effects are the effect driver's obligation, stated apart as `EFFECT_DRIVER` and in the README, not a host
  guarantee: the host can't observe effects, so the host suite can't score them. It is at least one attempt per
  record id, however often the record is released; without `safe_retry`, an unconfirmed attempt is never repeated
  automatically, and the segment becomes outcome unknown (`REQ-ORD-1`) and waits for the operator. That is what
  keeps `INV-8`. `REQ-CAT-10`'s own test scores it in a later milestone; no host guarantee claims `REQ-CAT-10`.
- "The same input" is equal under RFC 8785 (JCS). The rule is named here ahead of the provider-role statement
  envelope (`T-002-02`), which isn't on `m1` at the time of writing. A retry resends the input unchanged,
  `recorded_at` included; an input stamped again with another time is a different input and is refused under its
  used record id.
- `testing::Harness` (fresh store, start, kill, reopen) is how `T-005-03` drives every candidate, in-process or
  out of process; `FaultSeam` extends it with named stop points. A stopped submit returns `Unavailable`, the
  unknown-outcome error a production host already has, so `HostError` keeps no test-only variant; the harness's
  own failures are `HarnessError`.
- The effect driver's attempts are bounded both ways: at least one per record id, a confirmed attempt never
  repeated, and without `safe_retry` at most one, so re-releasing a record can't start a second attempt (`INV-8`).
  Driving effects only from released records is the driver's duty too, so it moved out of the host's
  `no-effect-without-record`, which now states only what the host does: release a record only once durable.
- `Harness` drives imports (`import`, `import_crashing_after`), so a stop during an import is scored for an
  out-of-process runtime as well, by a kill timed to the entry count, best-effort.
- Superseded: earlier bullets above that split effect duties between host and driver, say effects run "at least once per released record", or place `import_crashing_after` on `FaultSeam` are replaced by the later bullets: effects are the driver's alone, at most one attempt per record id without `safe_retry`, and imports run through `Harness`.

## T-005-03

- The suite lives in `rutent-conformance` as `conformance::host`, depending on `rutent-contracts` alone. It runs
  every candidate with its own state machine, `host::probe::Probe`, so a candidate is scored as
  `Harness<Probe>`: a real host is generic over its state machine (the host contract's `Host<M>`) and is
  instantiated with the probe here. The probe is not `CMP-STM`; it binds and settles segments and names every
  other input's outcome, and every order it returns passes `Order::check`.
- Expected states always come from folding the probe over a journal's inputs, never from the host's own reads,
  so a host can't vouch for itself (`NFR-4`).
- Fifteen cells, each with a stable kebab-case id, the guarantees it checks and the fault it injects, derived
  sentence by sentence from each guarantee's `suite` text. Each runs on a fresh store and inside `catch_unwind`,
  so a panicking candidate fails that cell, not the run.
- "Released" means acknowledged by `submit` or returned by a read. The crash between record and effect is scored
  as: every record released before a kill is there, whole, after the reopen; a stop before durability leaves no
  record and a resubmit applies the input once; a stop after durability leaves the record, and a resubmit returns
  it as already recorded. A stopped host is only killed, never read first.
- A black-box run (`run_black_box`, `Harness` alone) reports the three named-stop cells as not run, with the
  reason; not run never counts as a pass, and a guarantee with such a cell is incomplete. A run with `FaultSeam`
  runs all fifteen.
- The reordered delivery shuffles one order's seven inputs with a fixed-seed linear congruential generator (std
  only), and the seed is in the report. `recorded_at` values follow the recorded order, so a host that sorts by
  time is caught. The suite refuses a seed that leaves the order unchanged.
- The run id is the caller's; the suite reads no clock. Every Markdown row carries the run id, candidate, both
  contract versions and the seed, so a row pasted into `EXP-1`'s sheet keeps its provenance.
- The test double lives in `conformance/tests/support/`, test-only, and is not the reference host (`T-005-04`).
  Each flaw breaks one guarantee and keeps the double otherwise contract-valid: the stream it exports without an
  order is still a valid stream, the repeat it records twice keeps a gapless sequence, and the diverging state
  still passes the record checks. A flaw can fail more than its matching cell where the guarantees overlap:
  acknowledging before durability also loses acknowledged records under every stop.
- Not claimed: concurrent submits (`&mut self` makes them untestable here), the effect driver's obligation and
  `REQ-CAT-10`, the named stop points for an out-of-process runtime, and a swap between two different
  implementations. The suite takes a separate target harness for the swap, but with one double both sides are the
  same type; `EXP-1` pairs different ones.

## T-005-03, after review

- Every cell now checks each order's journal states and `state()` against the probe's fold of that journal, so a
  host that updates its state before its record fails the stop cells (`INV-7`).
- The reuse cell refuses three inputs under one used record id: one that differs in what was observed, one that
  differs only in `recorded_at`, and one that differs only in its order. It then kills the host and checks the
  refusal log again. A host that leaves `recorded_at` out of equality, looks ids up per order, or keeps refusals in
  memory fails it.
- The reordered delivery shuffles two orders' interleaved inputs, so a host with one sequence counter across orders
  fails `per-order-sequence`.
- A new cell, `import-refuses-a-stream-it-cannot-take`, feeds the target an export with a version it can't read,
  one with a record id twice under two orders, and a valid export onto a store that isn't empty; each must be
  refused. The stop during an import is tried at three points.
- `RunConfig` names the swap target, every report row carries it, and a swap cell's row names its direction.
- In the suite's own tests each flawed double is swapped onto another of the same kind, so a flaw in import is on
  the side that imports.
- Not covered, and stated in the suite's docs and README: power loss before an fsync and torn writes (a kill keeps
  the page cache, and no crash point falls inside a write; `EXP-1` inspects each candidate's durability code), and
  kills inside a submit for a black-box candidate, which is killed only between submits.

## T-005-03, after a second review

- The swap splits the scenario just after the input that opens nothing, so a record with no state crosses every
  swap; a host that exports only records with a state fails `swap-mid-order-loses-no-order`.
- After a submit is refused while halted, the halt cell resumes the host and checks nothing was applied, so a host
  that keeps refused submits and applies them on resume fails it.
- After each stopped import, the cell redoes the import onto a fresh store and checks the journals, refusals and
  states arrive whole, as the contract says the import is redone.
- The stop cells compare the record under the in-flight record id with the in-flight input itself, not only its id.
- The replay cell also checks each order's `state()` against the replay, and that `orders` lists exactly the orders
  with a record.
- `conformance/README.md` maps every cell to the rows of `EXP-1`'s scoring sheet (`H-01` to `H-17`).
- The halt cell, after resume, submits the refused input again and expects it recorded once, with the journals
  the acknowledged records plus that one; a host that keeps a refused input and applies it at the next submit
  fails it, and only it.
- The swap's guard on the stateless record needs that order's journal to be non-empty, and checks the old host
  holds every acknowledged record before the swap.
- The three stop cells carry `exactly-once` too, since each repeats the stopped input after the reopen; the README
  maps them to `H-06`.
- Two more doubles show the in-flight input check and the redone-import refusal check can fail, each on its own
  cell alone.

## T-005-04

- The reference host lives in `rutent-stm` (`components/stm`), as `rutent_stm::reference_host`, not in
  `conformance`: it is the baseline `EXP-1` compares both candidates with, and the floor if neither passes
  (PLAN §6.2, risk 2), so it may run, not only test. The boundary check lets `rutent-stm` depend on
  `rutent-contracts` alone, and take `rutent-conformance` as a dev-dependency, which its test of the suite uses.
- It is generic over the state machine, as the host contract's `Host<M>` is, and runs the suite with the probe.
- The store, `MemoryStore`, is a log of records and refusals plus a halted flag and an unfinished-import flag.
  Each write is one push onto the log, so an entry is whole or absent. The host's indexes (journals, record ids,
  orders, the refusal log) are rebuilt from the log every time a store is opened, so what it answers can't differ
  from what the log holds.
- "Same input" is `RecordedInput` equality. For these types that is the same as equal canonical JSON under RFC
  8785: each field serialises one way, and no string is normalised either way.
- It accepts an import only of its own contract versions, exactly, onto an empty store, checked by `StreamCheck`
  entry by entry, and marks the store complete only after the end.
- What memory can't give: the store outlives the host value, not the process. In the suite's terms a kill is
  `Harness::kill`, which drops the host's indexes and keeps its store, and against that every guarantee holds; the
  suite passes all sixteen cells with `FaultSeam`, and every runnable cell black-box. Against a real process kill,
  power loss or a torn write nothing survives, since nothing is on disk; `EXP-1` scores durability across a real
  kill on the candidates only.

## State-machine 0.4.0 (a follow-up to T-005-01)

- The segment plan `$ref`s the router contract's typed operation, `urn:rutent:router-api:operation#/$defs/operation`:
  `operation` became `kind`, and the endpoints are the router's, an inner node by identifier or an outer node by its
  network, each with a denomination. The Rust types reuse `router_api::{OperationKind, Endpoint}`.
- The router's schema states only which end of each kind is outer and which inner. Its other rules (an asset kept,
  a route between two different nodes, an exchange at one node) live in its priced `Operation::check`, which the
  bare typed operation can't call. `SegmentPlan::check` repeats them, run on every segment of an input and an order.
  A shared function in `router_api` would remove the copy; that needs a router-api bump, so it is left as a
  follow-up for that contract.
- The owner's ruling `spec-no-route-left` is published as `after_no_route(part)`, the one transition rule the
  contract holds, so every `StateMachine` applies it alike: stuck when the part's value can't be routed to the node
  where the destination reserve lives (`REQ-ORD-28`); waiting for the customer when the value sits there after a
  failed final deposit (`REQ-ORD-43`). "At that node" compares nodes only, as the ruling words it.
- Where the value sits is read from the segments: the start of the first segment that hasn't settled, when it is
  pending, reversed, or a deposit that failed (a failed deposit leaves the value where it was). With a segment
  started, owed a reversal, or of unknown outcome, the value is in flight and the rule places nothing.
- The recorded inputs alone can tell the two paths apart, but the current state can't always: after the final
  deposit fails, a recalculation from that node replaces the deposit, and if its first segment fails and is
  reversed, the segments no longer show a failed deposit. So the part keeps `deposit_failed_at`, the endpoint where
  its final deposit failed, set by the state machine at that failure and kept across the recalculation. It is a
  state field, not an input field: the fact is already in an earlier recorded input, and `transition` needs it in
  the state it is given. No input field is added.
- `stuck_declared` no longer takes `no_route_left`, so no route left has one path, `no_route`, resolved by the rule.
  Past the recalculation bound (`REQ-ORD-4`), Core records `no_route` with the decision record of the last routing.
- The host contract's shapes don't change, but its fixtures embed segment plans and its tests resolve the router and
  provider-role schemas, so the host is bumped to 0.1.1. The suite's probe builds the new segment shape, with `bank`
  and `chain` as outer nodes; its provenance test now reads both versions from the contracts. The reference host
  needs no change.

### After review

- `deposit_failed_at` has a published update rule, `deposit_failed_at_after`, applied with every input: each failed
  final deposit overwrites it; `destination_changed` clears it; a recalculation keeps it. Without it, a part whose
  customer accepted a new destination from the same node, and whose first new segment was reversed, would wait
  where it should be stuck, and implementations could disagree on a second failure. `Order::check` requires it to be
  the failed deposit's `from` node while that deposit is the part's last segment, so `after_no_route` reads the field
  alone. The suite's probe applies the rule, and the replay cell runs an order whose final deposit fails.
- `deposit_failed_at` is an inner node only, and its asset is dropped: the ruling says "at that node". `value_at`
  places nothing at an outer node, so with every segment pending nothing has moved, and `no_route` there is not
  applicable rather than stuck (`REQ-ORD-12`, `REQ-ORD-40`).
- `INV-10`'s destination, as the orchestrator ruled: the last deposit's provider (the destination reserve), its
  `to` node and its to-asset, decimals included. A part keeps it as `destination`; `Order::check` requires every
  route to end with a deposit there, and `recalculation_keeps_destination` is the published test a
  `route_recalculated` must pass, else `changes_destination`.
- Rules JSON Schema can't state get invalid fixtures too, listed in the test as rejected by Rust only, each with
  its rule: another reserve depositing to the same network, segments that don't chain, and a failed final deposit
  without `deposit_failed_at`.
- `SegmentPlan::check` also checks the provider-role atoms, every node, network and asset an identifier (with the
  router's own `Id`) and decimals at most 36; and each route chains, every segment starting where the one before
  it ended, as the router's `Part::check` requires.
- A `destination_changed` whose route ends at the part's destination is a recalculation, not a change of
  destination, as the orchestrator ruled: not applicable, with the new reason `keeps_destination`, and it keeps
  `deposit_failed_at`. Otherwise it would clear the field and send the part to stuck where a `route_recalculated`
  would leave it waiting. `destination_change_applies` is the published test.
- `Order::check` refuses a `deposit_failed_at` on a part where no final deposit failed. It reads "failed" as the
  last segment being a failed deposit, or the part having retired segments, since only a recalculation keeps the
  field after the deposit is gone; the state can't show more than that.
- The rules only Rust checks now cover recorded inputs too: a recalculated route that doesn't chain, and one that
  doesn't end with a deposit. A test checks the violation each Rust-only fixture is refused with.

## The M1 security review (host 2.0.0, `T-002-03`)

- A low finding: `seq` was an unbounded integer. A transition record's `seq`, a refusal record's `seq` and the
  `existing.seq` it points to are now at most 2^53 − 1 (`MAX_SEQ`), in the schemas (`maximum`) and in Rust (the
  records' `check`, which deserialising runs). The Rust fields stay `u64`.
- **Why major.** A record with a `seq` above 2^53 − 1 was valid at 1.0.0 and is refused now, which the contracts
  README's rule calls breaking, even though no host could reach it. Host 2.0.0; the fixtures' `host_version` follow,
  except the deliberate `snapshot-entry-header-with-a-version-not-semver`, which keeps `0.1.1`.
- **A reading: the brief names no task for the host.** The change rides in `T-002-03`'s commit with the other
  security-review fixes, and its changelog bullet names `T-002-03`.
- An end entry's counts are not sequence numbers and stay uncapped; a stream whose counts exceed what came before
  fails `StreamCheck` already.

## Review round 1 of M1 (state-machine 2.0.0, `T-005-01`)

The external review of M1's pull request raised three findings on this feature: which amount the next segment
moves (2), how a handed-on route starts (4), and `INV-4`'s rule (5).

### Finding 4: where a route starts (addressed)

- `REQ-ORD-16`'s start is now checked, on orders and on `route_chosen` inputs, in the schemas and in Rust: a part's
  first segment is a withdrawal, which the router's typed operation already makes outer to inner. A route handed
  on starts instead at an inner node, with no withdrawal.
- Why no withdrawal segment. `REQ-CAT-17` makes the holding account "the one reserve at an inner node", and a typed
  withdrawal starts at an outer node (`check_typed_operation`). `F-002` keeps inner-to-inner reserve tuples invalid
  (its notes, "A holding account doesn't advertise through this contract"). So a handed-on route can't open with a
  typed withdrawal, and the old valid fixture had to invent an outer node `{"network": "ledger-in"}` for the
  holding account. It now sits under `invalid/` as `order-handed-on-starting-with-a-withdrawal`.
- `REQ-ORD-40` says the holding account's own record proves a handed-on route's withdrawal. That record isn't a
  segment here. It is what `route_chosen` with `handed_on_from` rests on, and how it is named is `F-012`'s and
  `F-003`'s, with the holding account's shape. When a handed-on order counts as past its first segment
  (`REQ-ORD-12`) is `F-020`'s; in this contract the first segment is whatever moves the value first.
- Not checked: that a handed-on route starts at the first order's `deposit_failed_at`. That is a rule across two
  orders, which one order's check can't see; it is `CMP-STM`'s and Core's (`F-010`, `F-020`).
- The router already allows it: a request's `from` is any endpoint, inner included, as recalculations need. The
  router checks that a part starts at the request's origin, and doesn't check `REQ-ORD-16`'s withdrawal, so a
  route chosen for a fresh order is held to it only here. Whether the router should refuse a fresh request from an
  inner node is `F-004`'s; noted for it, not changed.
- Superseded: the `T-005-01` bullet that leaves "the route's own shape rules (it starts with a withdrawal and ends
  with a deposit)" to the router contract. The end has been checked here since 0.4.0, and the start is checked
  since 2.0.0. Amounts still aren't.
- Why major. An order and an input valid at 1.0.0 are refused now, which the contracts README calls breaking, as
  for host 2.0.0's `seq` cap. No host file changes: its fixtures' orders all start with a withdrawal, and its
  headers' `state_machine_version` isn't compared with this contract's version by any test.

### Finding 5: `INV-4`'s rule (addressed)

- Published as `recalculation_starts_where_value_sits(part, route)`: `value_at` places the value, and the route's
  first segment starts there, node and unit. It applies to `destination_changed` too: `REQ-ORD-43`'s other route
  starts "from the node where the value sits", and `INV-4` is about any restart.
- It reads `value_at` as `after_no_route` does. No route restarts while the value is in flight (a segment started,
  owing a reversal that isn't a failed deposit's, or of unknown outcome), nor before the withdrawal settles. In
  that second case the part can still end (`REQ-ORD-12`) or recalculate once the withdrawal settles, so nothing is
  lost.
- With it, a recalculation never replaces a settled withdrawal, so finding 4's start holds through recalculations.
  In a route handed on, a first segment that hasn't started may be replaced by a route from the same inner node.
- The reasons' precedence is stated, since `last_input` is part of the state and replay must agree across
  implementations (`REQ-STM-3`): `changes_destination` or `keeps_destination`, then `reuses_segment_id`, then
  `value_not_at_start`. The route's fit to the part comes before its timing. This keeps the existing
  `not_applicable` fixtures consistent: both have a segment started, so a timing-first order would have named them
  `value_not_at_start`.
- The probe doesn't apply part-scope inputs, so the host suite is unchanged.

### Finding 2: which amount the next segment moves (routed to `F-020` and `F-010`)

- Not changed here. No published rule reads an amount: `after_no_route`, `deposit_failed_at_after` and the
  destination and start tests read endpoints only. A settled amount added now would have no rule to pin it, and
  its unit and gross-or-net reading depend on the result shapes `F-002` is changing in the same round (findings 3
  and 6).
- The settled amount is already recorded. `segment_settled` names its evidence by id, and `REQ-ORD-39` has that
  evidence recorded, as `route_chosen` names its priced decision record by id.
- The open question, for `F-020` (`CMP-CORE-ORCH`, which moves value between segments) and `F-010` (`CMP-STM`):
  - Does segment k+1 move its plan's amount, or segment k's settled `amount_out`?
  - If the settled amount, what happens when it falls outside segment k+1's band or quote: a re-quote, so in effect
    a recalculation?
  - If the plan's amount, where does the difference rest, so that `INV-1` and `REQ-AUD-15` attribute it to the
    order at that holding account (`F-024`'s property tests)?
- What the answer costs this contract. If any transition comes to read the settled amount, it must be a field of
  `segment_settled`, because `transition` is pure and can't read the evidence behind an id (`REQ-STM-3`). Adding
  it as a required field is a state-machine major, with the owner's click. If no transition reads it, the state
  machine stays amount-free and Core reads the amount from the evidence record.
