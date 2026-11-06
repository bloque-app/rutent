# state-machine changelog

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
