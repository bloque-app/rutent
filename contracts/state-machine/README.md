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
| A recalculation keeps the destination and to asset (`INV-10`) | can't be stated: needs the order | `CMP-STM`, as `not_applicable` / `changes_destination` |

## Repeats and record ids

Repeats are kept from moving anything twice (`REQ-STM-4`, `INV-8`) by a split of duties:

- **The host delivers each recorded input exactly once** to the state machine. That is the host contract's
  guarantee (`T-005-02`, ADR-0002).
- **One record id names exactly one content.** Recording a different input under a used id is a refusal by the
  record store, reported by the host contract, never a silent no-op.

The state machine is a second line only: an input whose record id is the order's `last_record` or its
`last_input.record_id` comes back with outcome `repeat`. It keeps no other history.

## Outcomes

`transition` keeps the SAD's signature and returns the order. The order's `last_input` names the last input
delivered and what it did (`REQ-AUD-1`): `applied`; `repeat`; `late`, when it names a retired segment and so
matches no current one; or `not_applicable`, with a reason (`other_order`, `unknown_subject`, `ended`,
`not_from_this_state`, `changes_destination`, `reuses_segment_id`).
