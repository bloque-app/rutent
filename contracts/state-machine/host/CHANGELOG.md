# host changelog

## 1.0.0
- Frozen at C1: from here a change needs a version bump, a changelog entry naming a DEC-*, and the owner's click. (DEC-3, DEC-4, DEC-20, T-006-01)

## 0.1.1
- No change to the host's shapes: its fixtures carry the state machine's 0.4.0 segment plans, the router's typed
  operation, and its tests resolve the router and provider-role schemas those refer to. (DEC-4, T-005-01)

## 0.1.0
- The host contract: the `Host` and `FaultSeam` traits, the six guarantees (durable, exactly-once,
  per-order-sequence, no-effect-without-record, swap, replay) with how the suite tests each, and schemas and
  fixtures for the transition record and the snapshot. (DEC-3, DEC-4, T-005-02)
- After review: `exactly-once` is at most one record per record id, with equal inputs the same canonical JSON;
  the crash after durability is scored and the in-flight record's fate stated; `halt` and `resume` are durable and
  can fail; effects get at least one attempt per record id, and without `safe_retry` at most one; `orders` lists the
  host's orders. (DEC-4, T-005-02)
- After review: `FaultSeam` moves to a `testing` module outside the production contract; `export` and `import`
  stream JSON Lines of snapshot entries, with a refusal record schema and a stream check; a reused record id's
  refusal is recorded; writes are atomic and a crash during an import leaves a store `open` refuses. (DEC-4,
  T-005-02)
- After re-review: what happens to effects is the effect driver's obligation (`EFFECT_DRIVER`), not a host
  guarantee: at least one attempt per record id, and without `safe_retry` no automatic repeat, the segment left
  outcome unknown for the operator. Equal inputs are equal under RFC 8785 (JCS), and a retry resends the input
  unchanged. `testing::Harness` (fresh store, start, kill, reopen) drives in-process and out-of-process
  candidates alike; `FaultSeam` extends it, and `Crashed` leaves `HostError` for the harness-only
  `HarnessError`. (DEC-4, T-005-02)
- After a third review: the effect driver makes at most one attempt per record id without `safe_retry`, never
  repeats a confirmed attempt, and drives effects only from released records, a duty moved out of
  `no-effect-without-record`. `Harness` drives imports (`import`, `import_crashing_after`), so a stop during an
  import is scored out of process too. `StreamCheck::push` runs each entry's own check. (DEC-4, T-005-02)
