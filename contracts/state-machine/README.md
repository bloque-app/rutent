# state-machine contract

The state-machine API (`DEC-3`, `DEC-4`, ADR-0002): the published state lists, the recorded-input envelope, the
order as one or more parts, and `transition(state, input) → state`. The Rust types in
`contracts/src/state_machine/` are the API; their records serialise to the schemas here. The version is the first
heading of `CHANGELOG.md`.

| Shape | Schema `$id` |
|---|---|
| Order state | `urn:rutent:state-machine:order-state` |
| Segment state | `urn:rutent:state-machine:segment-state` |
| Segment plan | `urn:rutent:state-machine:segment-plan` |
| Recorded input | `urn:rutent:state-machine:recorded-input` |
| Order | `urn:rutent:state-machine:order` |

## Rules and where they are enforced

Deserialising a `RecordedInput` or an `Order` runs its `check`, so Rust accepts no invalid fixture. A value built
field by field in Rust is checked only when `check` is called; a `StateMachine` returns only orders that pass it.

| Rule | Schema | Rust |
|---|---|---|
| Every id, name and record id is non-empty | `minLength` | `Id`, `RecordId` |
| `recorded_at` is an RFC 3339 date-time, seconds `00` to `59` | `format` and `pattern` | `Timestamp` |
| An optional id, when present, is an id, never `null` | `type` | `some_id` on every `Option<Id>` |
| No unknown fields, on any object or observed kind | `additionalProperties`, `unevaluatedProperties` | `deny_unknown_fields` |
| The subject names exactly what the observed kind is about (order, part or segment) | `if`/`then` | `RecordedInput::check` |
| A route has at least one part, and each part at least one segment | `minItems` | `RecordedInput::check`, `Order::check` |
| A new destination or a hand-on carries the customer's acceptance (`REQ-ORD-43`, `REQ-AUZ-3`) | `required` | field type |
| A part names a linked order exactly when it is handed on | `if`/`then`/`else` | `Order::check` |
| A handed-on order has a handed-on part | `if`/`contains` | `Order::check` |
| A part retires a segment id once | `uniqueItems` | `Order::check` |
| Part ids, and segment ids current or retired, are unique within the order for its whole life | can't be stated | `Order::check`, `RecordedInput::check` within one route |
| An `applied` last input is the input that reached the state | can't be stated | `Order::check` |
| A part's route ends with a deposit at its destination: the destination reserve, the outer node it deposits to, and the asset with its decimals (`INV-10`, `REQ-ORD-16`) | `destination` required; can't be stated that the route ends there | `Order::check`; for a `route_recalculated`, `recalculation_keeps_destination`, else `not_applicable` / `changes_destination`; for a `destination_changed`, `destination_change_applies`, else `not_applicable` / `keeps_destination` |
| A segment is the router's typed operation: its kind, and which end of each kind is outer and which inner | `$ref` to `urn:rutent:router-api:operation#/$defs/operation` | `OperationKind`, `Endpoint`, `SegmentPlan::check` |
| A withdrawal, route or deposit keeps its asset; a route runs between two different nodes; an exchange stays at one node | can't be stated | `SegmentPlan::check`, on every segment of an input and an order, as the router's `Operation::check` |
| Every node, network and asset is a provider-role identifier, and decimals are at most 36 | provider-role's `identifier` and `decimals` | `SegmentPlan::check`, with the router's own `Id` |
| Each segment starts where the one before it ended, in the same unit, and a route ends with a deposit | can't be stated | `RecordedInput::check`, `Order::check`, as the router's `Part::check` |
| A route starts with a withdrawal from an outer node; a route handed on starts at an inner node, with no withdrawal (`REQ-ORD-16`, `REQ-CAT-17`) | `if`/`then`/`else` on `handed_on_from`, `prefixItems` | `Order::check`, `RecordedInput::check` for `route_chosen` |
| A recalculation or change of destination restarts a route only from where the part's value is evidenced (`INV-4`) | can't be stated: a transition | `recalculation_starts_where_value_sits`, applied by `CMP-STM`, else `not_applicable` / `value_not_at_start` |
| `deposit_failed_at`, when present, is an inner node, never an outer one or `null` | `$ref` to provider-role's `inner_node` | `NodeId`, `deserialize_with` |
| While a part's last segment is a failed deposit, `deposit_failed_at` is that deposit's `from` node | can't be stated | `Order::check` |
| A part has a `deposit_failed_at` only where a final deposit failed: its last segment, or one a recalculation retired | can't be stated | `Order::check` |
| The destination reserve is a provider-role identifier | `identifier` | `Order::check`, with the router's own `Id` |
| `deposit_failed_at` is overwritten by each failed final deposit, cleared by `destination_changed`, kept by a recalculation | can't be stated: a transition | `deposit_failed_at_after`, applied by `CMP-STM` with every input |
| A part resolved to its destination is complete, or failed after; one resolved to its origin ended without completing | `if`/`then` | `Order::check` |
| A stuck part's resolution ends it: complete at its destination, ended without completing at its origin; a part that isn't stuck doesn't change (`REQ-AUZ-4`, `REQ-STM-6`) | can't be stated: a transition | `after_stuck_resolved`, applied by `CMP-STM` to a `stuck_resolved` input, else `not_applicable` / `not_from_this_state` |
| `waiting_since`, when present, is an RFC 3339 date-time, never `null` | `format` and `pattern` | `Timestamp`, `deserialize_with` |
| A part has `waiting_since` exactly while it waits for the customer | `if`/`then`/`else` | `Order::check` |
| `waiting_since` is the `recorded_at` of the input that made the part wait, kept while it goes on waiting, dropped when it stops | can't be stated: a transition | `waiting_since_after`, applied by `CMP-STM` with every input that changes a part |
| Past the customer's floor, a part whose value is evidenced at a node waits for the customer; one already waiting or stuck, or whose value is in flight or not yet withdrawn, doesn't change (`REQ-ORD-27`, `INV-4`) | can't be stated: a transition | `after_waiting_at_floor`, applied by `CMP-STM` to a `waiting_at_floor` input, else `not_applicable` / `not_from_this_state` |
| With no route left, a part becomes stuck unless its value sits at the node where the destination reserve lives, after a failed final deposit, when it waits for the customer (`REQ-ORD-28`, `REQ-ORD-43`, `spec-no-route-left`) | can't be stated: a transition | `after_no_route`, applied by `CMP-STM` to a `no_route` input |

## No route left

The owner's ruling `spec-no-route-left` settles what `no_route` does, and the contract publishes it as a pure
function, `after_no_route`, that every `StateMachine` applies:

- a part whose value can't be routed to the node where the destination reserve lives becomes **stuck**
  (`REQ-ORD-28`);
- a part whose value already sits at that node, after a failed final deposit, **waits for the customer**
  (`REQ-ORD-43`).

That node is the `from` node of the failed deposit, an inner node. The part remembers it in `deposit_failed_at`, by
a second published rule, `deposit_failed_at_after`, so the first still holds after a recalculation from there fails
and is reversed:

- each failed final deposit overwrites it;
- `destination_changed` clears it, since the destination reserve is then another one (one that keeps the destination
  isn't a change, below, and keeps it);
- a recalculation keeps it.

The rule places nothing, and the input is not applicable, when the value is in flight (a segment started, owed a
reversal, or with its outcome unknown) or still outside the book (the withdrawal hasn't settled: nothing has moved,
`REQ-ORD-12`, `REQ-ORD-40`). `stuck_declared` no longer takes `no_route_left`: no route left has one path.

## Waiting past the floor

A customer may set a floor, a worst acceptable outcome; past it, the order waits for the customer, in a state
visible to them with its age (`REQ-ORD-27`, `AC-12`, `AC-42`). Core judges the floor, at a settled segment or at a
recalculation, against the value evidenced and the quotes still ahead, and records its judgement as a
`waiting_at_floor` input about the part. The state machine reads no amount, so the input carries none.

The state is the one the contract already had, `waiting_for_customer`: a part past its floor waits for the customer
as one after a failed final deposit with no route left does. A published rule, `after_waiting_at_floor`, places the
part:

- a part whose value is evidenced at a node (`value_at`) **waits for the customer**;
- a part already waiting or stuck doesn't change (a stuck part is the operator's), and nor does one whose value is in
  flight or not yet withdrawn: a floor is judged on value that sits somewhere (`INV-4`). The input is then
  `not_applicable` / `not_from_this_state`.

The age is `waiting_since`, which a part carries exactly while it waits, for any cause: the `recorded_at` of the
input that made it wait, kept while it goes on waiting, dropped when it stops (`waiting_since_after`). A reader with
a clock shows the age as its now less `waiting_since`; the state machine reads no clock (`REQ-STM-5`).

No provider starts on a waiting part. The customer's choice resolves it: a `route_recalculated` within a new
authorisation (a lower floor, say). A `destination_changed` or a `handed_on` of a waiting part comes only after a failed final deposit (`INV-10`): a part waiting at its floor with its final deposit not tried takes neither.

## A stuck part's resolution

A stuck part is resolved in two steps under policy (`REQ-AUZ-4`, ADR-0011): one party decides the outcome, naming
the evidence, and another moves the value. core-internal's `deliver` resolution sends the value to the order's own
destination or back to its origin; once the move's result is done, Core records `stuck_resolved`, with `to` naming
which, and the evidence it arrived on. ADR-0011 and `REQ-AUZ-4` don't name the outcomes; the names are core-internal's.
A published rule, `after_stuck_resolved`, ends the part:

- delivered to the **destination**, the part is **complete**;
- returned to the **origin**, the part **ended without completing**;
- a part that isn't stuck doesn't change; the input is `not_applicable` / `not_from_this_state`.

The value may have been in flight when the part was declared stuck, so the rule doesn't ask where it sits, and the
part's segments may not show it delivered: the part keeps `resolved_to`, which says so. Having ended, the part takes
no later report (`REQ-STM-6`): a late delivery on its segment moves nothing twice (`AC-5`). A resolution handing the
value on to a linked order is `handed_on`, as for a waiting part.

## The destination

A part's `destination` is its route's last deposit: the destination reserve (the deposit's provider), the outer node
it deposits to, and the asset with its decimals. `INV-10` keeps it: a recalculation ending anywhere else, another
reserve depositing to the same network included, is refused unless the customer accepts it as a
`destination_changed`.

The other way round, as the orchestrator ruled: a `destination_changed` whose route ends at the part's destination
is a recalculation, not a change of destination. It is not applicable (`keeps_destination`), and changes nothing,
`deposit_failed_at` included. `destination_change_applies` is the published test, `recalculation_keeps_destination`
negated.

## Where a route starts and restarts

A route starts with a withdrawal from an outer node (`REQ-ORD-16`). A route handed on (`REQ-ORD-43`) starts instead
at the inner node where the value sits, whose holding account counts as its origin reserve, "the one reserve at an
inner node" (`REQ-CAT-17`). A typed withdrawal goes from an outer node to an inner one, so a route handed on has no
withdrawal segment. The contract leaves two things open:

- how the holding account is named as that reserve: the catalogue's and the module interface's (`F-012`, `F-003`);
- that the node is where the first order's value sits: a rule across two orders, which one order can't check.

A route restarts, by `route_recalculated` or by the customer's `destination_changed`, only from where the part's value
is evidenced (`INV-4`): `recalculation_starts_where_value_sits` holds when `value_at` places the value and the route's
first segment starts there, in the same unit. While a segment has started, owes a reversal that isn't a failed
deposit's, or has an unknown outcome, the value is in flight; before the withdrawal settles nothing has moved. Either
way no route restarts, and the input is not applicable (`value_not_at_start`). So a recalculation never replaces a
settled withdrawal, and every part keeps the start above.

When a route breaks more than one rule, its reason is the first that applies: `changes_destination` or
`keeps_destination`, then `reuses_segment_id`, then `value_not_at_start`.

## Repeats and record ids

Repeats are kept from moving anything twice (`REQ-STM-4`, `INV-8`) by a split of duties:

- **The host keeps at most one record per record id**, and applies each recorded input once. That is the host
  contract's guarantee `exactly-once` (`host/README.md`, `T-005-02`, ADR-0002).
- **One record id names exactly one content.** Recording a different input under a used id is a refusal by the
  record store, reported by the host contract, never a silent no-op.

The state machine is a second line only: an input whose record id is the order's `last_record` or its
`last_input.record_id` comes back with outcome `repeat`. It keeps no other history.

## Outcomes

`transition` keeps the SAD's signature and returns the order. The order's `last_input` names the last input
delivered and what it did (`REQ-AUD-1`): `applied`; `repeat`; `late`, when it names a retired segment and so
matches no current one; or `not_applicable`, with a reason (`other_order`, `unknown_subject`, `ended`,
`not_from_this_state`, `changes_destination`, `keeps_destination`, `reuses_segment_id`, `value_not_at_start`).
