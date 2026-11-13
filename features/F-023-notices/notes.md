# F-023 notes

Readings taken where `SPEC.md` or `PLAN.md` is silent or ambiguous, recorded per Narthex `REQ-SKL-24`.

## T-023-01

Built in `components/core-orch/src/notice/` (`mod.rs`, `wire.rs`, `inbox.rs`); tests in
`components/core-orch/tests/notice.rs`, and over the assembly, with the policy engine itself, in
`conformance/scenarios/tests/notices.rs`. No contract change: core-internal's README leaves notices out of the contract
("Notices are not here: they are `F-023`'s module inside `CMP-CORE-ORCH`").

- **From the log, never from the caller.** `Notices::notify` reads the log's entries, each with its receipt, as the
  log holds them, from its first, and carries on from where it stopped, as `Stuck::follow` and policy's `charged_in`
  read them. A later read must hold the last entry read where it was, by its receipt (the log's name, the index and
  the entry's digest), so a shorter log, or another, is refused whole (`NoticeError::NotThisLog`). Only that one
  receipt is compared, so the check costs the same however long the log (on review; it compared every entry read
  before). Reading, reversible. Reading the log rather than hooking the segments, the stuck orders and the reversals
  keeps this row inside its own module, and makes the notices a function of the log: the same log gives the same
  notices, byte for byte, after a restart (what is signed holds each change's recorded time, never the clock's, and
  Ed25519 is deterministic), so a receiver's sequence still holds. Reading, reversible.
- **What is a change.** Every verified `input` entry the state machine applies (`Stm::open` on a `route_chosen`,
  `Stm::transition` after it, outcome `applied`) is one change, a failure included: a segment failed, a withdrawal
  cancelled, a part stuck, a part or order failed. That is wider than the published state list moving: a provider
  bound or a middle segment settled moves no `OrderState`, but is notified, so no failure that leaves the part's state
  as it was (a segment failed, its reversal owed) goes unnoticed (`AC-41`, "failure included"). An input the machine
  doesn't apply and the log takes (a repeat, a late report about a segment a recalculation retired) and an
  `input_not_taken` change nothing and are notified to no one. Also notified, each as the order's next change with its state as it was: an `afterwards` entry
  (`returned`, `late_approval`; `T-020-12`'s hand-off, `REQ-ORD-31`, `REQ-ORD-25`) and an `order_refusal` naming an
  order the notices follow. Reading, reversible.
- **One sequence per order.** Each change is the order's next sequence, counted from 1 in the log's order; both
  receivers get the same sequence for it.
- **Its receivers come from Core's records.** The user is the requester the order's `acceptance` entry names,
  receiving as `customer`; the operator is the deployment's operator party, set when `Notices` is built, receiving
  as `operator`. An order the log holds no acceptance for is notified to the operator alone; no customer is guessed.
  Before an order starts there is no state to notify: a refusal at intake is answered to its caller by intake's
  `Told`, and an `order_refusal` naming no started order is not notified. Reading, reversible.
- **What a notice carries.** The order's state as `Reads::OrderState` reads it (`REQ-ORD-5`, `T-021-02`): the
  order's state, each part's state, and when it reached them. Beside it, what changed: the change's kind, the part,
  a stuck part's reason, the linked order a part was handed on to, a refusal's class, an `afterwards` event; all of
  the customer's own order (corrected on review: the first notes said a notice carried only what `OrderState`
  covers). The time a change was recorded is carried unconverted (`wire::At`): an input's `recorded_at` in the
  state machine's grammar, an entry's `when.at` in core-internal's, so no change is dropped for a time that wouldn't
  convert. Never a provider, segment, amount, account, evidence id or provider's text (`REQ-AUZ-7`, `REQ-ORD-18`); a
  test holds a customer's notices to that.
- **A push asks policy nothing; the pull does.** The customer is the order's own requester, from the log, whom
  policy lets read its order's state. The operator's push goes to the deployment's configured operator party, not
  through policy's register: policy grants an operator's `OrderState` read only to a login its register lists for
  `ReadOrderState` (`session/mod.rs`), and that register governs the pull (`Notices::current`), not the push.
  Checking each push would ask policy again for every change of every order each time the notices are rebuilt from
  the log after a restart, and would make the notices depend on the log taking a read entry for each. Reading,
  reversible: a deployment wanting each push recorded as a read would check `Reads::check_read` per receiver.
- **Authenticated for its receiver.** A notice is sealed for one receiver: `{"notice": ..., "signature": hex}`, the
  signature Ed25519 by the deployment's notice key (`NoticeKey`) over `rutent-notice/1\n` followed by the notice's
  RFC 8785 canonical form. What is signed names the deployment, the receiver (party and role) and what the notice is
  about, at its sequence. The reference receiver (`Inbox::take`) checks, in order: it is a sealed notice
  (`Malformed`), the signature is the deployment's notice key's (`Forged`), it comes from this deployment
  (`OtherDeployment`), and it is addressed to this receiver (`ForAnother`). A copy re-addressed fails the signature;
  the operator's party receiving as a customer is another receiver. The domain prefix starts with a byte no canonical
  JSON object starts with, so a notice signature never verifies as a provider-role statement's, whose signing input
  is a canonical object; the assembly may therefore back `NoticeKey` with the operator's key. `NoticeKey` is its own
  trait, not `OperatorKey`, whose documented input is a provider-role statement (on the advisor's review). The
  receiver's signature check is a trait (`Verifier`), so `ed25519-dalek` stays a dev-dependency of `core-orch`.
- **Safe repeated or reordered.** An order's notice carries the order's whole state after its change, so the
  receiver keeps the latest sequence and takes only a later one (`Taken::Later`, else `Taken::Earlier`); the view never
  goes back and is always the state of the latest sequence taken. A late or lost notice below the latest changes
  nothing. A lost latest notice shows in no sequence: a receiver that must be sure before acting asks for the current
  state (`THR-103`: "a lost one is recovered only by asking for the state"). `Inbox::missing` says only that a
  sequence below the latest never arrived and no answer of the current state since covers it: an answer is marked
  `current`, and covers every sequence up to its own (on review: it never cleared). The property test runs any
  delivery, any repeats, any order, some never.
- **The current state on request** (`Notices::current`). Policy checks the read, `Reads::OrderState`, and records it
  either way. A refused read is told `CurrentError::Refused` and nothing more, whatever policy's reason: the engine
  refuses a stranger's read of an order that doesn't exist and of another party's order for different reasons, and
  passing the reason on told the stranger which orders exist (on review: high). The reason stays in the log. An
  allowed read is answered with the order's latest change as the log holds it to its end, sealed for the reader and
  marked `current`: for an operator's login, the deployment's configured operator, whatever party the login states;
  for anyone else, its party in its role. Asking sends nothing and moves nothing of the reading: the changes not
  notified yet go out on the next `notify`, each to its own receivers (on review it pushed every order's pending
  notices through the asker's transport). A log that doesn't extend the one read answers `NotThisLog`, never a stale
  state; a clock with no time answers `NoTime`, apart from a key that can't sign (`Unsigned`). An allowed read of
  an order the log holds no change of answers `Unknown`: only a reader policy allows learns that, the operator's
  listed login among them.
- **The operator's channel** (`Channel`, which implements core-internal's `Operator`). A raise is checked
  (`Raise::check`), sealed for the operator alone and sent; it answers `Delivered` only when the transport took it,
  so the raise's entry records `done` or `failed`. The operator's inbox holds each raise once, by its id, however
  often it comes. `notice::Unbuilt` stays, unused, for an assembly that has no channel yet.
- **Delivery** goes through the assembly's `Transport`. A notice it didn't take is kept, in order
  (`Notices::undelivered`); retrying it within a bound and recording it are `T-023-02`'s.

### Hand-offs to `T-023-01`

| From | Hand-off | How it is handled |
|---|---|---|
| `T-020-11` (F-020 notes, "The operator's warning") | the stuck order's warning, `stuck_declared` | Handled: `stuck_declared` is an input the machine applies, so it is a change notified to the operator and the customer, with its reason (`over_time`, `dispute`) and the part's state `stuck` (asserted in the `AC-41` test). The figures the declaration rests on (`stuck_call`: the p99, the tuple) are not in the notice, which carries only what both receivers read by their role; the operator reads them in the log by its permission. |
| `T-020-11` (F-020 notes, "A material change in a p99") | `Stuck::learn`'s raise | Handled on the channel's side: `Channel` is an `Operator`, so the raise is sealed for the operator alone and answered delivered or not (`raise_reaches_the_operator_alone_and_answers_its_delivery`, over core-internal's own p99 fixture). Gap: the assembly still hands no channel to `Stuck::learn`; owner, the assembly's row (`F-024`). |
| `T-020-12` (F-020 notes, "After the end") | `afterwards`: `returned`, `late_approval` | Handled: each is the order's next change, its state as it was (`what_comes_after_an_order_ended_is_notified`). |
| `T-021-05` (F-021 notes, "Who surfaces `ahead()`") | `Charged::ahead()`: party, request id, kind, count, counted time | Handled on the notices' side: `Notices::ahead` takes each charge with the moment it was counted at (`AheadCharge`, from the contract's `Charge`, since `core-orch` may not import `core-authz`) and sends the operator, alone, its party, request id, kind of work, count and counted time beside Core's now, once per party and request (`charge_counted_far_ahead_is_notified_to_the_operator_once`). Gap: calling `engine.charged().ahead()` after each charge and handing it over, mapping `Counted::at` to a timestamp through `Reading::of(at).timestamp()`, is the assembly's; owner, the assembly's row (`F-024`). |

### Defaults (reversible)

- Every applied input is a change, not only one that moves an `OrderState`.
- `afterwards` and an `order_refusal` naming a followed order are changes too; a refusal before an order starts is
  intake's to answer.
- Pushed notices ask policy nothing, the operator's included, sent to the configured operator party; the pull asks
  `Reads::OrderState`, where the operator's register governs.
- The user receiver is the acceptance's requester party, as `customer`; the operator's is the configured party, as
  `operator`.
- The notice key is its own trait; the assembly may back it with the operator's key.
- The wire form (`rutent-notice/1`, hex signature) is this module's, not a contract.

### Gaps

- **No published notice schema** (owner: the coordinator, to settle before the next contract freeze). Integrators
  have no schema under `contracts/` for a sealed notice: the wire form lives in `core-orch` (`notice::wire`). It
  doesn't block this row.
- **The assembly's wiring** (`F-024`): a `Notices` beside the log, read after each append (or on a schedule), a
  `Channel` where `Operator` is asked for (the checks, `Stuck::learn`), and `engine.charged().ahead()` handed to
  `Notices::ahead` after each charge.
- **Every rebuild sends the whole history again** (owner: `T-023-02`). The coordinator's ruling: keep the
  at-least-once re-send, since receivers drop each copy by its sequence and `REQ-ORD-20` holds. Built afresh, as
  after a restart, the notices read the log from its first entry and send every change of every order again. What
  `T-023-02` owns: a durable mark of what was delivered, and a rebuild that catches up to it without sending. Until
  then, only `undelivered` and the charges noticed ahead live in memory, and a charge still set apart after a restart
  is sent again, which the operator's inbox holds once.
- **The existence oracle on policy's side** (owners: `T-021-05`'s and `T-021-06`'s owner, for the engine; `T-020-08`'s,
  for `Segments::status`). The notices tell a refused reader one refusal, but the engine still answers a stranger's
  `OrderState` read of a missing order with another class than of another party's order (the assembly's test sees the
  two differ), and the log records the class. Should an unknown order answer "no" rather than "missing"? The same
  oracle is open in `Segments::status`, which answers `StatusError::Unknown` before it asks policy, and passes
  policy's `ReadAnswer`, its class with it, in `StatusError::Refused`.

### Tests and rule-offs

- **Red first.** The module was written first and its tests after, not test-first. Red against nothing built is
  shown, as `T-020-11`'s notes did, by the first rule-off, which stubs the module whole (the log read sends nothing, the inbox
  takes nothing, the pull answers nothing, no raise or charge is sent): every named test goes red. Two tests stayed
  green against it, vacuously (`same_log_gives_the_same_notices_after_a_restart` compared two empty runs,
  `order_with_no_recorded_acceptance_is_notified_to_the_operator_alone` checked every one of no notices); each now
  asserts how many notices it holds. The first run of the finished module had one red, a test artefact: the
  `input_not_taken` entry's reason was written in the wrong shape, refused by the contract's own deserialiser.
- **The entries are the log's shapes, built by hand,** replayed through `rutent-stm` in the test, so what each
  notice says is checked against the machine, and, since the review, put to the log (`held_to_the_log`). The first
  `AC-41` history held an input the log refuses (a `provider_bound` on the ended `p2`, a test artefact: the machine
  refuses it, so Core never records it); it is gone, and in its place two inputs the log takes and the machine
  doesn't apply: a settlement recorded again (a repeat) and a failure reported of a segment a recalculation retired
  (late). In every test log, each input is one the log takes (`state_machine_takes`, the row
  `input-is-one-the-state-machine-takes`, which every input is held to before any citation of its), and `admit_entry`
  refuses nothing but a citation of a record the test doesn't build and no notice reads (`UNBUILT_CITATIONS`): the
  quote shown, the reserve's yes and the user's session an acceptance cites, the router's decision record, the
  evidence a settlement or a reversal rests on, the stuck resolution's result, the statement an `afterwards` rests on,
  and the authorisation that sets the customer's floor. The notices read none of those: the acceptance only for its
  requester, the inputs only for what the machine does with them, `afterwards` and `order_refusal` only for their
  order, event and class. `admit_entry` stops at the first row an entry fails, so a refusal of an unbuilt citation
  could hide a later row's; for the inputs, the one row the notices rest on is asked on its own. Over the assembly,
  `conformance/scenarios/tests/notices.rs` reads an admitted log end to end.
- **Rule-offs**, each rule switched off alone (`notice/`), the tests run, the rule restored and the file touched;
  all thirteen pass after:

  | Rule switched off | Tests that went red |
  |---|---|
  | nothing built (the whole module stubbed) | every test but the two noted above |
  | the inbox checks the signature | `notice_for_another_receiver_fails_its_check` |
  | the inbox checks the deployment | `notice_for_another_receiver_fails_its_check` |
  | the inbox checks the receiver | `notice_for_another_receiver_fails_its_check` |
  | the inbox takes only a later sequence | `repeated_or_reordered_notices_leave_the_receivers_view_right` |
  | only an applied input is a change | `each_state_change_is_notified_to_the_user_and_the_operator`, `each_customer_receives_only_its_own_orders_notices`, `receiver_can_ask_for_the_current_state`, `repeated_or_reordered_notices_leave_the_receivers_view_right`, `what_comes_after_an_order_ended_is_notified` |
  | the customer is the order's own requester | `each_customer_receives_only_its_own_orders_notices` |
  | `afterwards` is a change | `what_comes_after_an_order_ended_is_notified` |
  | a later read extends the entries read | `another_log_or_a_locked_key_sends_nothing_amiss` |
  | the pull answers only an allowed read | `receiver_can_ask_for_the_current_state` |
  | a charge counted ahead is noticed once | `charge_counted_far_ahead_is_notified_to_the_operator_once` |
  | a raise that fails its check isn't sent | `raise_reaches_the_operator_alone_and_answers_its_delivery` |
  | an applied input's ids and time are carried unconverted (the router-api grammar put back on `recorded_at`) | `input_recorded_with_an_offset_is_notified_and_the_order_followed_on` |

- **Found on the advisor's review, after the first commit: an applied input could be dropped.** The first version
  converted an applied input's `recorded_at` to router-api's timestamp, whose grammar takes only `Z` and at most nine
  digits of a second, after the machine applied it. An input the state-machine contract allows at an offset from UTC
  was then no change, the order followed stayed behind the log, and each later input was judged from the stale state
  and dropped. The notice now carries the state machine's own ids and times, unconverted;
  `input_recorded_with_an_offset_is_notified_and_the_order_followed_on` holds it, red with the conversion put back. Its first run failed for a test artefact: the test's helper gave the
  log entry itself the input's offset time, which core-internal's entry refuses (Core's log stamps its own UTC clock);
  with that corrected, it went red with the conversion put back, on the count of notices, and green without it.

### T-023-01, the review of `685b985`

| # | Finding | What changed | Held by |
|---|---|---|---|
| 1 | High: `current` is an existence oracle, passing policy's refusal class through | `CurrentError::Refused` carries nothing; the class stays in the log. The policy-side question is handed to the engine's and `T-020-08`'s owners (Gaps) | `stranger_learns_nothing_of_whether_an_order_exists` (a stand-in giving the engine's two classes); `stranger_is_told_one_refusal_whether_the_order_exists_or_not` (the assembly, the engine's own reads, which refuse the two for different reasons) |
| 2 | Medium: every restart re-sends the whole history | Kept, on the coordinator's ruling: at least once. Named as a gap owned by `T-023-02`, the durability gap corrected | — |
| 3 | Medium: the `AC-41` history holds an input the log refuses | The inapplicable input dropped; a repeat and a late report added; every test log put to `admit_entry` and `state_machine_takes` (Tests, above) | `each_state_change_is_notified_to_the_user_and_the_operator`, and `held_to_the_log` in each test that builds a log of its own |
| 4 | Low: kinds untested | One test notifies `route_recalculated`, `destination_changed`, `handed_on`, `end_requested`, `waiting_at_floor`, `no_route`, `reversal_completed` and `segment_outcome_unknown`, in a log the log takes; a hand-on carries its `linked_order`, and the linked order is notified to the same customer | `every_kind_of_input_is_notified` |
| 5 | Low: what a notice carries misstated | Notes and module docs corrected | — |
| nit | `noted` drops a change on a time that won't parse | A notice's time is `wire::At`, carried unconverted from the input or the entry: nothing is parsed, so nothing is dropped. No test can make core-internal's `when.at` fail, since its grammar is the narrower | — |
| nit | A clock failure becomes `Unsigned` | `NoticeError::NoTime`, `CurrentError::NoTime` | `clock_without_a_time_is_told_apart_from_a_locked_key` |
| nit | `current` answers stale state silently | A log that doesn't extend the one read answers `CurrentError::NotThisLog` | `receiver_can_ask_for_the_current_state` |
| nit | `current` pushes every order's pending notices through the reader's transport | `current` takes no transport and `&self`: it reads the asked order to the log's end and sends nothing; the next `notify` sends the rest to each receiver | `receiver_can_ask_for_the_current_state` |
| nit | The pull answer goes to the login's party, not the configured operator | An operator's login is answered as the deployment's operator | `receiver_can_ask_for_the_current_state` |
| nit | `missing` never clears | An answer of the current state is marked `current` and covers every sequence up to its own | `receiver_can_ask_for_the_current_state` |
| nit | The set of charges noticed ahead only grows | Kept to what policy still sets apart; one set apart again is sent again, held once by the inbox | `charge_counted_far_ahead_is_notified_to_the_operator_once` |
| nit | `starts_with` compares every entry read | The last entry read is compared by its receipt alone; `notify` and `current` take the log's entries with their receipts | `another_log_or_a_locked_key_sends_nothing_amiss` |

- **Red first.** This round's tests were written before the code they hold, against the API the fixes give
  (`current` without a transport, entries with receipts, `CurrentError::Refused` bare), so they failed first to
  build; the first run of the built code had three reds, each a test artefact (an acceptance with no yes, refused by
  the contract's own deserialiser; two counts miscounted against the new history), and one refusal of the log's,
  `waiting-at-floor-rests-on-the-customers-floor`, a record the test doesn't build, named in `UNBUILT_CITATIONS`.
  Each new rule was then switched off alone:

  | Rule switched off | Tests that went red |
  |---|---|
  | a refused read is told one refusal, whatever policy's reason (a missing order answered `Unknown`) | `stranger_learns_nothing_of_whether_an_order_exists`, `stranger_is_told_one_refusal_whether_the_order_exists_or_not` |
  | the pull answers the log to its end | `receiver_can_ask_for_the_current_state` |
  | the pull answers nothing from a log that isn't this one | `receiver_can_ask_for_the_current_state` |
  | an operator's pull is addressed to the deployment's operator | `receiver_can_ask_for_the_current_state` |
  | an answer of the current state closes the gap | `receiver_can_ask_for_the_current_state` |
  | a clock without a time is told apart from a locked key | `clock_without_a_time_is_told_apart_from_a_locked_key` |
  | the charges kept are those policy still sets apart | `charge_counted_far_ahead_is_notified_to_the_operator_once` |
  | the last entry read is where it was, by its receipt | `another_log_or_a_locked_key_sends_nothing_amiss` |
  | a hand-on carries its linked order | `every_kind_of_input_is_notified` |

  All sixteen of `core-orch`'s notice tests and the assembly's pass after.
