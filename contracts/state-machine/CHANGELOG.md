# state-machine changelog

## 3.0.0
- Breaking: a part waiting past its floor is a recorded state (`REQ-ORD-27`, `AC-12`, `AC-42`). A new observed kind,
  `waiting_at_floor {}`, about one part, records Core's judgement that the part is past the customer's floor; it
  carries no amount, since no transition reads one. The state is the existing `waiting_for_customer`, which fits:
  no new state is added. The published rule `after_waiting_at_floor` makes a part whose value is evidenced at a
  node wait; a part already waiting or stuck, or whose value is in flight or not yet withdrawn, doesn't change
  (`INV-4`). A part now carries `waiting_since` exactly while it waits, for any cause: the `recorded_at` of the
  input that made it wait, kept by the published rule `waiting_since_after`, so the order shows its age to a reader
  with a clock. An order waiting without `waiting_since` was valid at 2.1.0 and is refused now, and a new
  `Observed` variant breaks exhaustive matches, so this is a major bump. Fixtures: the waiting order gains
  `waiting_since`; new ones for the input, an order waiting at its floor, and three refusals. (DEC-4, DEC-20,
  T-020-01, T-020-10)

## 2.1.0
- The Rust code behind this contract (`contracts/src/state_machine`) is `no_std` with `alloc`:
  it uses `core` and `alloc` paths, and the crate's
  default `std` feature turns on `std` only for its dependencies and tests. Minor, not major: no schema, Rust type or
  behaviour changes, and every consumer that builds with default features builds unchanged. Fixtures keep naming
  their current version. (DEC-4, DEC-20, DEC-26, T-010-01)

## 2.0.0
- Breaking, after round 1 of M1's external review: a route starts with a withdrawal from an outer node, and a route
  handed on starts instead at an inner node, where the value sits, with no withdrawal (`REQ-ORD-16`,
  `REQ-CAT-17`). Orders and `route_chosen` inputs are checked, in the schemas and in Rust. The handed-on fixture
  that withdrew from an outer node invented for the holding account was valid at 1.0.0 and is refused now, so
  this is a major bump; the fixture is kept as an invalid one, and the valid one starts at the inner node.
  (DEC-4, DEC-20, T-005-01)
- The published rule of `INV-4`, `recalculation_starts_where_value_sits`: a `route_recalculated` or
  `destination_changed` restarts a route only from where the part's value is evidenced (`value_at`), and is
  otherwise not applicable, with the new reason `value_not_at_start`. When a route breaks more than one rule, the
  reason is the first of `changes_destination` or `keeps_destination`, `reuses_segment_id`, `value_not_at_start`.
  (DEC-4, DEC-20, T-005-01)

## 1.0.0
- Frozen at C1: from here a change needs a version bump, a changelog entry naming a DEC-*, and the owner's click. (DEC-3, DEC-4, DEC-20, T-006-01)

## 0.4.1
- A segment plan's typed-operation rules and a route's chaining are checked by the router contract's own
  `check_typed_operation` and `check_typed_chain`, not a copy, so the two contracts state each rule once. No
  shape, rule or message changes. (DEC-4, T-004-02)

## 0.4.0
- The segment plan is the router contract's typed operation (`urn:rutent:router-api:operation#/$defs/operation`):
  `kind`, and router endpoints, with `OperationKind` and `Endpoint` reused in Rust and the operation's other rules
  checked on every segment. (DEC-4, T-005-01)
- The owner's ruling `spec-no-route-left`: a `no_route` input makes a part stuck when its value can't be routed to
  the node where the destination reserve lives, and makes it wait for the customer when its value sits there after
  a failed final deposit, by the published rule `after_no_route`. A part keeps `deposit_failed_at`, and
  `stuck_declared` no longer takes `no_route_left`. (DEC-4, T-005-01, spec-no-route-left)
- After review: `deposit_failed_at` is an inner node, kept by the published update rule `deposit_failed_at_after`
  (each failed final deposit overwrites it, `destination_changed` clears it, a recalculation keeps it), and must
  match a failed last deposit; value at an outer node is not placed. A part keeps its `destination`, the last
  deposit's reserve, outer node and asset, and every route ends there (`INV-10`). Segments chain, and their
  identifiers and decimals are checked. (DEC-4, T-005-01, spec-no-route-left)
- After a second review: a `destination_changed` whose route keeps the destination is a recalculation and not
  applicable (`keeps_destination`, `destination_change_applies`), and keeps `deposit_failed_at`. A part has a
  `deposit_failed_at` only where a final deposit failed, and the destination reserve is an identifier. (DEC-4,
  T-005-01, spec-no-route-left)

## 0.3.0
- Points to the host contract for exactly-once delivery, and declares its module. (DEC-3, T-005-02)
- The pointer says what the host contract states: at most one record per record id. (DEC-4, T-005-02)

## 0.2.0
- Rust types as strict as the schemas: non-empty ids, a checked timestamp, no unknown fields, and cross-field
  checks on deserialising. The order carries its last input's outcome and its retired segment ids; a new
  destination or a hand-on carries the customer's acceptance; `recorded_at` has a pattern. (DEC-4, T-005-01)
- Optional ids refuse `null` in Rust as in the schemas; a malformed offset is refused without a panic; no leap
  second; a `reuses_segment_id` reason for a route that reuses a segment id. (DEC-4, T-005-01)

## 0.1.0
- The published order and segment state lists, the recorded-input envelope, the order as one or more parts, and
  the `transition(state, input) → state` signature, with schemas and fixtures. (DEC-3, DEC-4, T-005-01)
