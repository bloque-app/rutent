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
  exception: a `reversal_completed` on the starting segment (the part's first) of a part that is complete. That
  segment goes settled to reversal complete, and the part and so the order fail. The exception is read per part, so
  in a split order one complete part can fail while another is still open, and an order that ended can fail later.
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
  Core calls it with both orders after opening the second and before the first part's `handed_on`, or after; it holds
  both ways.
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
