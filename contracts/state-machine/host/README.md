# host contract

What any runtime that runs the state machine must guarantee (`DEC-3`, ADR-0002). It is runtime-neutral: a
purpose-built store, VOS, or anything else meets it through the `Host` trait in `contracts/src/state_machine/host/`.
The version is the first heading of `CHANGELOG.md`.

| Shape | Schema `$id` |
|---|---|
| Transition record | `urn:rutent:host:transition-record` |
| Refusal record | `urn:rutent:host:refusal-record` |
| Snapshot entry | `urn:rutent:host:snapshot-entry` |

## Production contract and the testing seam

`Host` is the production contract, the one that freezes. The host-contract suite (`T-005-03`) scores each candidate
through it (`DEC-5`, `DEC-6`), driving it through `testing::Harness`: a fresh store, start, kill, reopen, and import, whole or stopped part-way. The
`testing` module is in the same crate but not part of what a production host implements, and its `HarnessError` is
never returned by one.

- **An in-process candidate**: killing drops the host's memory and keeps its store. Its harness can also implement
  `testing::FaultSeam`, which stops a submit after a named `CrashPoint`. A stopped submit returns `Unavailable`,
  an unknown outcome.
- **A runtime in another process**, such as VOS: killing stops the process, and reopening starts it again on the
  same data. For it the named points are best-effort, reached by timing a kill rather than by a hook.

## The life of one input

1. **Received**: the host has the input. Nothing is recorded.
2. **Computed**: the host has called `open` (the order has no state yet) or `transition`, and holds the result in
   memory. Nothing is recorded.
3. **Durable**: the transition record, holding the input whole, its sequence number and the state it reached, is in
   the store. **A write is atomic**: after a crash a record, or a refusal, is whole or absent.
4. **Acknowledged**: `submit` returns the record. No record leaves the host, by `submit` or by any read, before it
   is durable; leaving the host is its *release*.

An *effect* is anything done because of a transition, such as calling a provider. It is driven only from a
released record, so every effect has its record (`INV-7`). What happens after that is the effect driver's, not the
host's.

## The effect driver's obligation

A record can be released more than once (a resubmit after a crash, a read), so whoever acts on released records
keeps a repeat from moving value twice (`INV-8`). This is `EFFECT_DRIVER` in the Rust module; a test keeps the two
equal. Driving effects from released records is part of it. It is not a host guarantee and is not scored through `Host`: `REQ-CAT-10`'s own test scores it in a later
milestone.

> An effect driver drives effects only from released records. It makes at least one attempt per record id,
> however often the record is released, and never repeats an attempt that was confirmed. Without safe_retry, it
> makes at most one attempt per record id; an unconfirmed attempt is never repeated automatically: the segment
> becomes outcome unknown (REQ-ORD-1) and waits for the operator.

`safe_retry` is the provider's declaration in `urn:rutent:provider-role:declarations` (`REQ-CAT-10`).

A different input under a used record id is refused, and the refusal is itself recorded, whole, in the host's
refusal log (`REQ-AUD-1`), durable before the refusal is returned.

## Swap

A swap is halt, export, import, resume. `halt` and `resume` are durable and can fail; a halted host reopens halted,
so a host that was halted and exported can't reopen and run an order a second time. `export` streams the snapshot
as JSON Lines of snapshot entries: a header naming both contracts' versions, every record and refusal, then an end
with the counts. `import` reads the same stream onto an empty store, checking it as it goes, and marks the import
complete only after the end entry. **A crash during an import** leaves a store that `open` refuses with
`ImportIncomplete`; the import is redone on a fresh store.

This version allows a swap that halts the host. `NFR-6` sets no time to replace yet; a later version may add a
live swap.

## Guarantees

The same list is `GUARANTEES` in the Rust module; a test keeps the two equal.

| Guarantee | Serves | Statement | How `T-005-03` tests it |
|---|---|---|---|
| `durable` | `INV-7`, `NFR-5` | Once `submit` returns a record or a refusal, its entry survives the host stopped abruptly at any point and its store reopened. A write is atomic. A record that was durable but not yet acknowledged when the host stopped is present after reopening, unacknowledged; submitting the same input again returns it as already recorded, which releases it. A halted host reopens halted. Nothing not yet durable can be read | Stop after each crash point, reopen, compare journals with what was acknowledged; the record in flight at after-durable is present. Halt, stop, reopen: still halted |
| `exactly-once` | `REQ-STM-4`, `INV-8`, `REQ-AUD-1` | At most one record per record id in the whole host. Two inputs are the same when their `RecordedInput` values are equal, the same canonical JSON under RFC 8785 (JCS). A retry resends the input unchanged, `recorded_at` included; an input stamped again with another time is a different input. An equal input again returns the recorded one and records nothing; a retry after a crash before durability may call the state machine again, harmless because it is pure, but never makes a second record. A different input under a used id is refused by name, and the refusal is recorded. `Halted` and `RecordIdReused` are definite; every other error leaves the outcome unknown, and submitting again is always safe | Repeat a delivery before and after a crash and after a swap: one record, no state moves twice. Reuse a record id with other content: refused, and in the refusal log |
| `per-order-sequence` | `REQ-STM-3`, `REQ-STM-4` | An order's records are numbered 1, 2, 3 with no gap, in the order applied, each to the state the previous one reached. Submits for one order are applied one at a time. The host never reorders inputs, by `recorded_at` or otherwise; nothing is promised between orders | Deliver one order's inputs out of their recorded order: gapless, in submit order, each state following from the one before |
| `no-effect-without-record` | `INV-7`, `NFR-5` | A record is released only once durable. A crash before the record is durable leaves no record and nothing released to act on | Stop after received and after computed: no record, no state change, nothing released; submitting again applies the input once. Stop after durable: the record is present and unacknowledged, and submitting again releases it once, as already recorded |
| `swap` | `REQ-OPS-3`, `NFR-6`, `REQ-STM-4` | Halt, export the stream, import it into another implementation on an empty store, resume. A submit is recorded before the halt or refused with `Halted`, never half applied. After the import every order's state and journal, the refusal log and the record ids seen are the same, and the same further inputs reach the same states. The stream names both versions; an import refuses one it can't read. A crash during an import leaves a store `open` refuses. A swap may halt the host | Swap mid-order between two implementations: no order lost or duplicated, a repeat from before the swap still recognised, the order runs to its end. Stop the new host during the import: `open` refuses its store |
| `replay` | `REQ-STM-3`, `NFR-4`, `REQ-AUD-1` | Each journal is complete and never compacted, holding every input whole, source and time included, and the state it reached; `orders` lists every order with a record. Folding `open` and `transition` over a journal's inputs, in sequence order, reaches every recorded state | Replay each journal of each listed order through the state machine alone and compare every state |

## Rules and where they are enforced

Deserialising a record runs its `check`, and `StreamCheck` checks a whole stream, so Rust accepts no invalid
fixture. Streams are fixtures too: `fixtures/*/snapshot-stream-*.jsonl`, one entry per line.

| Rule | Schema | Rust |
|---|---|---|
| Sequence numbers start at 1 | `minimum` | `TransitionRecord::check`, `RefusalRecord::check` |
| Sequence numbers are at most 2^53 − 1 (`MAX_SEQ`, since 2.0.0), so they survive RFC 8785 canonical JSON | `maximum` | `TransitionRecord::check`, `RefusalRecord::check` |
| A record or refusal holds its input whole, with its record id | `$ref` to `recorded-input` | `RecordedInput` |
| An absent state is absent, never `null` | `$ref` to `order` | `deserialize_with` |
| No unknown fields | `additionalProperties` | `deny_unknown_fields` |
| A stream names both versions as `MAJOR.MINOR.PATCH` | `pattern` | `Version` |
| An entry is a header, record, refusal or end | `oneOf` | `SnapshotEntry` |
| A record, its input's subject and its state name the same order | can't be stated | `TransitionRecord::check` |
| A record's state names the record's input as its last | can't be stated | `TransitionRecord::check` |
| A stream starts with one header and ends with an end that counts what came before; nothing follows the end | can't be stated | `StreamCheck` |
| Each order's records run from 1 with no gap; once a record has a state, every later one does | can't be stated | `StreamCheck` |
| A record id is recorded once in the whole host | can't be stated | `StreamCheck`; `exactly-once` for a live host |
| The refusal log runs from 1 with no gap, and each refusal points to the record holding its record id | can't be stated | `StreamCheck` |
