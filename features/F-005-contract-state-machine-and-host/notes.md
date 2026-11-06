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
- Raised to the owner: does "no route left" make an order stuck (glossary, `REQ-ORD-4`) or make it wait for the
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
