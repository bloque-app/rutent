# state-machine changelog

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
