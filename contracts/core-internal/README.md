# core-internal contract

The interfaces between Core's three parts, `CMP-CORE-ORCH`, `CMP-CORE-AUTHZ` and `CMP-CORE-AUDIT` (ADR-0001,
`DEC-2`, `DEC-4`, `T-020-01`). `C1` doesn't freeze them (`PLAN.md` §3); they are settled here, before `F-020`,
`F-021` and `F-022` run side by side, so no feature's agents edit another's interface. Each change after 1.0.0 is a
version bump with a changelog entry naming a `DEC-*`, and waits for the owner's click on the release into `main`
(`DEC-20`). The Rust traits and types in `contracts/src/core_internal/` are the API (a library API, `DEC-4`), and
every value that crosses between the parts serialises to a schema here. The version is the first heading of
`CHANGELOG.md`.

## The calls

Each call is a trait in `core_internal::calls`. Every method takes `&mut self` and plain references, so each trait is
object-safe and the assembly can hand a part another's as `Box<dyn ...>`. The traits are split so that each later
row implements its own in its own module, where its fail-closed stub sits until it does.

| Direction | Trait and method | Takes | Answers | Implemented in | Ids |
|---|---|---|---|---|---|
| orchestration → policy | `Decisions::decide` | `DecisionRequest` | `Decision` | `core-authz` `engine` (`T-021-01`) | `REQ-AUZ-1`, `REQ-AUZ-5` |
| orchestration → policy | `Authorisations::authorise` | `Authorisation` | `Decision` | `core-authz` `funds` (`T-021-04`) | `REQ-AUZ-3`, `REQ-AUZ-9` |
| orchestration → policy | `StuckOrders::decide_stuck` | `StuckDecide` | `Decision` | `core-authz` `stuck` (`T-021-06`) | `REQ-AUZ-4`, ADR-0011 |
| orchestration → policy | `StuckOrders::move_stuck` | `StuckMove` | `Decision` | `core-authz` `stuck` (`T-021-06`) | `REQ-AUZ-4`, ADR-0011 |
| orchestration → policy | `Sessions::open_session` | `SessionRequest` | `SessionAnswer` | `core-authz` `session` (`T-021-02`) | `REQ-CAT-3` |
| orchestration, log → policy | `Reads::check_read` | `ReadCheck` | `ReadAnswer` | `core-authz` `session` (`T-021-02`) | `REQ-AUZ-7`, `REQ-AUD-12` |
| operator → policy | `Reads::grant_permission`, `Reads::revoke_permission` | `PermissionChange` | `Decision` | `core-authz` `session` (`T-021-02`) | `REQ-AUZ-7`, `REQ-AUD-12` |
| orchestration → policy | `Charges::charge` | `Charge` | `ChargeAnswer` | `core-authz` `parties` (`T-021-05`) | `REQ-OPS-1`, `REQ-AUZ-8` |
| orchestration → policy | `PersonQuestions::ask_about_person` | `PersonQuestion` | `PersonAnswer` | `core-authz` `cpl` (`T-021-07`) | `REQ-CPL-2`, `REQ-CPL-5`, `REQ-CPL-6` |
| orchestration, checks → log | `Log::append` | `GeneralEntry`: an `Entry` of no decision kind, a stated one unverified | `Receipt` of the entry recorded | `core-audit` `log` (`T-022-01`) | `REQ-AUD-1`, `REQ-AUD-2`, `INV-7` |
| log → evidence store | `KeptStatements::kept` | a commitment | the `KeptStatement` under it, or none | `core-audit` `receipts` (`T-022-03`) | `REQ-AUD-2` |
| log → catalogue | `StatementKeys::deployment`, `StatementKeys::operator`, `StatementKeys::publisher`, `StatementKeys::signs_for` | a maker, a key and an instant | this deployment; the party its operator states as, which the log hands `admit_entry`; the party a maker states for; whether the key signs the maker's statements then | `core-audit` `log`, from advertisements the log recorded under permitted `change_catalogue` effects (`T-022-01`), never orchestration | `REQ-AUD-2`, `REQ-PRT-1`, `THR-3` |
| log → signature check | `Signatures::verifies` | a statement's bytes as received | whether its signature is its named key's | `core-audit` `log` (`T-022-01`) | `REQ-AUD-2` |
| policy → log | `PolicyLog::append_decision` | `DecisionEntry`: a `VerifiedEntry` of a decision kind | `Receipt` | `core-audit` `log` (`T-022-01`) | `REQ-AUZ-1`, `REQ-AUD-1`, `INV-7` |
| orchestration → evidence store | `Evidence::keep` | a statement's bytes as received | `Kept`: its salted commitment, and the statement it restates | `core-audit` `receipts` (`T-022-03`) | `REQ-AUD-2`, `REQ-AUD-5`, `REQ-AUD-6` |
| party, checker → evidence store | `Evidence::fetch` | `ReadCheck` of `reads: evidence` | `Fetched`: a `KeptStatement`, policy's refusal, or erased | `core-audit` `receipts` (`T-022-03`) | `REQ-AUD-5`, `REQ-AUD-9`, `REQ-AUD-10` |
| orchestration → checks | `Comparisons::report_pair` | `ReportPair` | `Receipt` | `core-audit` `compare` (`T-022-05`) | `REQ-ORD-9` |
| orchestration → checks | `Comparisons::holding_reading` | `HoldingReading` | `Receipt` | `core-audit` `compare` (`T-022-05`) | `REQ-AUD-15` |
| orchestration → checks | `Measures::measure` | `MeasureRecord` | `Receipt` | `core-audit` `measures` (`T-022-06`) | `REQ-AUD-7` |
| checks → operator | `Operator::raise` | `Raise` | `Delivery` | the operator's channel, `core-orch` `notice` (`F-023`) | `REQ-ORD-9`, `REQ-AUD-15`, ADR-0009 |

- **The callee checks what it is handed.** Every implementer runs the `check` of each value it is handed, which
  recurses into everything that value holds, and refuses one that fails: `Err(AppendFailure::Refused)`, with nothing
  recorded. A value built in memory is held to the same rules as one read from the wire.
- **The log answers first.** Every policy answer is appended to the log before it is given, and carries the receipt
  of its entry. Where the append fails, the call answers `Err(AppendFailure)` and nothing moves on it. Orchestration
  appends an effect's entry (`call: effect`, outcome `to_take_effect`) and holds its receipt before the action takes
  effect, so a failure between the record and the effect leaves no effect (`INV-7`). How it ended follows as a
  `result` entry citing that receipt: `done`, `failed` or `unanswered`.
- **Two handles on one log.** A decision entry is one whose `what` is a policy call's answer: `decide`,
  `authorise`, `stuck_decide`, `stuck_move`, `open_session`, `check_read`, `grant_permission`,
  `revoke_permission`, `charge` or `ask_about_person` (`calls::is_decision`). The log takes those through
  `PolicyLog` alone, as a `DecisionEntry`, and every other entry through `Log`, as a `GeneralEntry`. Each type's
  field is private, and it is built and deserialised only through `TryFrom`, which refuses the other's kinds, so a
  part that holds only `Log` can't write a permitting decide and cite it, however it builds or reads the entry.
  `admit_entry` takes the path an entry came by (`Through`) and refuses a decision kind on the general one, and
  anything else on policy's, before it reads a row. The assembly, `rutent-scenarios`, holds the log and hands
  `PolicyLog` to the policy engine and to no other part; `tools/boundary-check` reads the crate graph from
  `cargo metadata` and fails when any crate but the assembly reaches `core-audit`, which builds the log and its
  handles, or `core-authz`, which holds policy's.
- **The log, not the writer, attributes a statement.** A stated entry reaches `Log` unverified: it names the maker it
  claims and the commitment of the bytes the evidence store keeps, and a `GeneralEntry` refuses one attributed
  `verified`, built or read. On append the log runs `attribute`: it fetches the kept bytes by the commitment
  (`KeptStatements`), refusing an entry whose bytes aren't kept, and records the entry `verified` to its claimed maker
  only where the bytes are an envelope of this deployment, made by that maker, of the entry's shape, time and key, about
  the transaction its body names exactly when its body names one, with a signature that verifies (`Signatures`) under a
  key that signs the maker's statements at its `made_at` (`StatementKeys`); otherwise it records it unverified,
  attributed to no one (`REQ-AUD-2`). The keys come from advertisements the log recorded under permitted
  `change_catalogue` effects and from the deployment, never from orchestration, which could otherwise call its own key a
  provider's. The receipt is of the entry recorded. A statement whose body names a transaction is verified only where
  its `about` names that transaction, so none is verified about nothing, and one about an order or a segment only
  through a transaction its body names: the order is Core's reading, which no provider signs, so the signed transaction
  is what binds the statement to it. Core appends the question it asks under that transaction first, signed with the
  operator's key and about the order, and the segment, it asks for; `answer-follows-its-question` holds the answer to
  it, made by the deployment's operator party, and `transaction-belongs-to-one-order` keeps the transaction to that
  order and segment (`REQ-ORD-32`, `REQ-ORD-38`). A judgement carries no transaction, so it is appended about no order,
  and the `reliance` entry that relies on it binds it to one. `fixtures/attribution/` holds a case for each part of the
  rule.
- **A statement's maker is bound to its key and its publisher.** A stated entry's actor has two names: `acts`, the
  provider, operator or module whose name the envelope carries and whose key signs, and `party`, the one it states
  for, its publisher or the operator's party. The log verifies a statement only where the envelope's `maker` is its
  `acts`, the key signs for that `acts`, and its `party` is the one `StatementKeys::publisher` answers for it, so
  both names of a verified maker are bound. Its `role` stays the writer's label, and no relation reads it. A row
  comparing a statement's maker with a provider (an origin reserve, an arrival's reserve, a judge, a segment's
  provider under grade-4 evidence) reads `acts`; a row comparing it with a party (a report pair's reporting party,
  the operator's party for a question) reads `party`.
- **The log is the guard of every citation.** Permits, receipts and ids can be built anywhere: `Decision::permit`
  makes a `Permit` from a permitting answer, but a permit deserialises, and a decision's fields are public. So the
  log's append runs `admit_entry` on every entry, which reads one table, `RELATIONS` for a verified entry and
  `STATED_RELATIONS` for a stated one: each row is a reference an entry makes to earlier entries, derived from
  `SPEC.md`, with the kind of entry it must point to and the outcome required, the fields that must match, and the
  ids that demand it. An `exists` row is refused when no earlier entry answers, naming the field that failed on the
  closest one; a `none other` row when one does. A relation with no `SPEC.md` id is not in the table.

### The citation table

| Row | Citing entry: field | Must point to | Fields that must match | `SPEC.md` | Fixtures (`fixtures/admission/<row>…`) |
|---|---|---|---|---|---|
| `effect-permits-a-decided-action` | `effect` (not `resolve_stuck`): its permit's receipt | a `decide`, `permitted`, at that receipt | request, decision, action | `REQ-AUZ-1` | admitted: `world--*`; refused: `--missing`, `--request`, `--decision`, `--action` |
| `stuck-effect-permits-a-stuck-move` | `effect` of `resolve_stuck`: its permit's receipt | a `stuck_move`, `permitted`, at that receipt (never a `decide`) | request, decision, order, carries_out | `REQ-AUZ-4` | admitted: `world--*`; refused: `--missing`, `--request`, `--decision`, `--order`, `--carries_out` |
| `permit-backs-one-effect` | `effect`: its permit | none other: no other `effect` citing the same permit (the same effect retried is admitted) | — | `INV-8` | admitted: `world--*`; refused: `--conflict` |
| `move-rests-on-its-authorisation` | `effect` of `move_value`: its `authorisation` | an `authorise`, `permitted`, of that authorisation | order, destination (`route.destination`), to_asset (`route.to_asset`), floor (`route.delivers` at least the floor, where one is set) | `REQ-AUZ-3`, `INV-10` | admitted: `world--*`; refused: `--missing`, `--order`, `--destination`, `--to_asset`, `--floor`, `--where` |
| `move-rests-on-its-orders-acceptance` | `effect` of `move_value`: its `order` | an `acceptance` of that order (before or after its `authorise`) | authorisation | `REQ-ORD-34` | admitted: `world--*`; refused: `--missing`, `--authorisation` |
| `stuck-effect-moves-the-decided-resolution` | `effect` of `resolve_stuck`: its `carries_out` | a `stuck_decide`, `permitted`, with that request and decision | order, resolution | `REQ-AUZ-4` | admitted: `world--*`; refused: `--missing`, `--order`, `--resolution` |
| `hand-on-rests-on-the-linked-orders-acceptance` | `effect` of `resolve_stuck` handing on: its `linked_order` | an `acceptance` of the linked order | requester (the order handed on's) | `REQ-ORD-13`, `REQ-ORD-43` | admitted: `world--*`; refused: `--missing`, `--requester` |
| `result-ends-an-effect` | `result`: its `effect` | an `effect` at that receipt | — | `REQ-ORD-1`, `INV-7` | admitted: `world--*`; refused: `--missing` |
| `effect-has-one-result` | `result`: its `effect` | none other: no other `result` of that effect (the same result retried is admitted) | — | `REQ-ORD-1`, `INV-7` | admitted: `world--*`; refused: `--conflict` |
| `stuck-move-carries-out-its-decide-step` | `stuck_move`: its `carries_out` | a `stuck_decide`, `permitted`, with that request and decision | order, mover (its party isn't the decider's) | `REQ-AUZ-4` | admitted: `world--*`; refused: `--missing`, `--order`, `--mover` |
| `stuck-decision-is-of-a-stuck-order` | `stuck_decide`: its `order` | an `input` of that order | observed (`stuck_declared` or `no_route`) | `REQ-ORD-28`, `REQ-AUZ-4` | admitted: `world--*`; refused: `--missing`, `--observed` |
| `input-cites-its-decision-record` | `input` (`route_chosen`, `route_recalculated`, `destination_changed`, `no_route`): its `decision_record` | a `decision_record` with that id | order | `REQ-AUD-4` | admitted: `world--*`; refused: `--missing`, `--order` |
| `chosen-route-rests-on-its-orders-acceptance` | `input` (`route_chosen`): its order | an `acceptance` of that order | — | `REQ-ORD-34` | admitted: `world--*`; refused: `--missing` |
| `changed-destination-rests-on-its-acceptance` | `input` (`destination_changed`): its `acceptance` | an `acceptance` with that id | order | `REQ-ORD-43`, `REQ-ORD-34` | admitted: `world--*`; refused: `--missing`, `--order` |
| `hand-on-input-rests-on-the-linked-orders-acceptance` | `input` (`handed_on`): its `acceptance` | an `acceptance` with that id | order (the `linked_order`), requester (the order handed on's) | `REQ-ORD-43`, `REQ-ORD-13` | admitted: `world--*`; refused: `--missing`, `--order`, `--requester` |
| `handed-on-route-points-back` | `input` (`route_chosen` with `handed_on_from`): its `handed_on_from` | an `input` (`handed_on`) of that order | linked_order (this order) | `REQ-ORD-43` | admitted: `world--*`; refused: `--missing`, `--linked_order` |
| `settlement-rests-on-its-evidence` | `input` (`segment_settled`): its `evidence` | an `evidence` with that id | order, segment | `REQ-ORD-33`, `REQ-ORD-39` | admitted: `world--*`; refused: `--missing`, `--order`, `--segment` |
| `reversal-rests-on-its-evidence` | `input` (`reversal_completed`): its `evidence` | an `evidence` with that id | order, segment, whose (not the failed segment's provider, unless the origin reserve labelled its word) | `REQ-ORD-3` | admitted: `world--*`; refused: `--missing`, `--order`, `--segment`, `--whose`, `--unless` |
| `decision-record-chains-to-its-orders-previous` | `decision_record`: its `previous` | a `decision_record` at that receipt | order | `REQ-AUD-4` | admitted: `world--*`; refused: `--missing`, `--order` |
| `posted-route-cites-its-decision-record` | `decide` of `post_route`: its `decision_record` | a `decision_record` with that digest | order | `REQ-AUD-4` | admitted: `world--*`; refused: `--missing`, `--order` |
| `sent-report-is-its-partys-statement` | `report_pair`: `sent.statement` | a stated entry at that receipt | verified (as the log attributed it on append, from the kept bytes), maker (`sent.party`), about (the pair's order and segment) | `REQ-ORD-9`, `REQ-AUD-2` | admitted: `world--*`; refused: `--missing`, `--verified`, `--maker`, `--about` |
| `received-report-is-its-partys-statement` | `report_pair`: `received.statement` | a stated entry at that receipt | verified (as the log attributed it on append, from the kept bytes), maker (`received.party`), about (the pair's order and segment) | `REQ-ORD-9`, `REQ-AUD-2` | admitted: `world--*`; refused: `--missing`, `--verified`, `--maker`, `--about` |
| `arrival-rests-on-the-reserves-report` | `arrival`: its `report` | a stated entry at that receipt | verified (as the log attributed it on append, from the kept bytes), maker (the `reserve`), about (the order it arrived for) | `REQ-ORD-23` | admitted: `world--*`; refused: `--missing`, `--verified`, `--maker`, `--about` |
| `refiling-files-an-arrival-anew` | `refiling`: its `arrival` | an `arrival` at that receipt | order, filer (the operator, or the arrival's reserve) | `REQ-ORD-23` | admitted: `world--*`; refused: `--missing`, `--order`, `--filer`, `--or` |
| `evidence-rests-on-statements` | `evidence`: each of `rests_on` | a stated entry at that receipt | verified (each statement under grades 2 to 4: 2 and 3 claim a signature, and 4 is its provider's word, which an unverified statement is no one's; a public record, 1, rests on the network), provider (under grade 4, the statement's maker is the provider this log's `move_value` names for the segment), bound (an unverified statement names a transaction bound to the evidence's order and segment), about (the evidence's order and segment, a transaction by the order and segment it belongs to) | `REQ-ORD-39`, `REQ-ORD-38`, `REQ-AUD-2` | admitted: `world--*`, `--signed-and-verified`, `--provider-word`, `--public-record-unverified-bound`; refused: `--missing`, `--verified`, `--provider`, `--bound`, `--about`, `--about-segment`, `--about-grade-1`, `--about-grade-4` |
| `afterwards-rests-on-a-statement` | `afterwards`: its `statement` | a stated entry at that receipt | about (the order) | `REQ-ORD-25`, `REQ-ORD-31` | admitted: `world--*`; refused: `--missing`, `--about` |
| `reliance-names-its-judges-statement` | `reliance`: its `judgement` | a stated entry at that receipt | verified (as the log attributed it on append, from the kept bytes), maker (the `judge`), about (nothing but the relying order: a judgement is appended about no order) | `REQ-CPL-12` | admitted: `world--*`; refused: `--missing`, `--verified`, `--maker`, `--about` |
| `measure-cites-entries-of-the-log` | `measure`: each of `cites` | an entry at that receipt | — | `REQ-AUD-2` | admitted: `world--*`; refused: `--missing` |
| `raise-cites-entries-of-the-log` | `raise`: each of `cites` | an entry at that receipt | — | `REQ-ORD-9`, `REQ-AUD-15` | admitted: `world--*`; refused: `--missing` |
| `erasure-names-a-kept-statement` | `erasure`: its `evidence` | a stated entry holding that commitment | — | `REQ-AUD-13` | admitted: `world--*`; refused: `--missing` |
| `acceptance-is-of-the-quote-shown` | `acceptance` (`given`): its `quote` | a `shown` with that digest | order, shown_as (a quote, never a price) | `REQ-ORD-34`, `REQ-QTE-7` | admitted: `world--*`; refused: `--missing`, `--order`, `--shown_as`, `--never` |
| `shortlisted-authority-cites-its-shortlist` | `ask_about_person` on a shortlisted route: its `shortlist` | a `decision_record` with that digest | order, round (`shortlist`), providers (among its `shortlisted`) | `REQ-CPL-2`, `REQ-CPL-6` | admitted: `world--*`; refused: `--missing`, `--order`, `--round`, `--providers` |
| `provider-asked-before-was-asked` | `ask_about_person`: each of `asked_before` | an `ask_about_person`, `permitted`, asking that provider | order | `REQ-CPL-5` | admitted: `world--*`; refused: `--missing`, `--order` |
| `asked-before-is-complete` | `ask_about_person`: its order | none other: no `ask_about_person`, `permitted`, of that order asking a provider missing from `asked_before` | — | `REQ-CPL-5` | admitted: `world--*`; refused: `--conflict` |
| `revocation-revokes-a-grant` | `revoke_permission`, `permitted`: its `permission` (a refused one cites nothing) | a `grant_permission`, `permitted`, of that permission | — | `REQ-AUZ-7` | admitted: `world--*`, `--refused`; refused: `--missing` |
| `permitted-read-rests-on-its-grant` | `check_read`, `permitted`, naming a `permission` | a `grant_permission`, `permitted`, of that permission | grantee (the reader's party), scope (covers what it reads) | `REQ-AUZ-7`, `REQ-AUD-12` | admitted: `world--*`; refused: `--missing`, `--grantee`, `--scope` |
| `permitted-read-is-not-revoked` | `check_read`, `permitted`, naming a `permission` | none other: no `revoke_permission`, `permitted`, of that permission | — | `REQ-AUZ-7` | admitted: `world--*`; refused: `--conflict` |
| `customer-read-is-of-its-own-order` | `check_read`, `permitted`, by a customer, of an order or segment: its order | an `acceptance` of that order | party (the reader is its requester, beneficiary or payer), segment (a segment read's segment is one its order's plan lists) | `REQ-AUZ-7` | admitted: `world--*`, `--segment-of-its-order`; refused: `--missing`, `--party`, `--beneficiary`, `--segment` |
| `provider-read-is-of-its-own-segment` | `check_read`, `permitted`, by a provider, of a segment: its order and segment | an `effect` of `move_value` on that segment of that order | provider (the reader, as the effect and the read name it) | `REQ-AUZ-7` | admitted: `world--*`; refused: `--missing`, `--provider`, `--as` |
| `segment-belongs-to-one-order` | `effect` of `move_value`: its segment | none other: no `effect` of `move_value` on that segment under another order | — | `REQ-ORD-38` | admitted: `world--*`; refused: `--conflict` |
| `route-starts-on-the-withdrawal-proof` | `effect` of `move_value` on a segment after its part's first, by the `route_chosen` plan | an `input` (`segment_settled`) of the part's first segment, for the order | — | `REQ-ORD-40` | admitted: `world--*`; refused: `--missing` |
| `fee-rests-on-a-settled-segment` | `fee`: its segment | an `input` (`segment_settled`) of that segment | order | `REQ-AUD-11` | admitted: `world--*`; refused: `--missing`, `--order` |
| `acceptance-rests-on-the-reserves-yes` | `acceptance` (`given`): its `reserve_yes` | a stated entry at that receipt | verified (as the log attributed it on append, from the kept bytes), maker (the `origin_reserve`), order (its `about`), transaction (its `about` names the one the reserve signed) | `REQ-ORD-32` | admitted: `world--*`; refused: `--missing`, `--verified`, `--maker`, `--order`, `--transaction` |
| `acceptance-is-asked-in-the-users-session` | `acceptance` (`given`): its `session` | an `open_session`, `permitted`, that opened that session | holder (the requester), expires_at (not before the acceptance) | `REQ-ORD-32` | admitted: `world--*`; refused: `--missing`, `--holder`, `--expires_at` |
| `move-segment-is-in-its-orders-plan` | `effect` of `move_value`: its segment | an `input` (`route_chosen`, `route_recalculated` or `destination_changed`) whose plan lists that segment | order | `REQ-ORD-38`, `REQ-ORD-40` | admitted: `world--*`; refused: `--missing`, `--unplanned-segment`, `--order` |
| `recalculation-starts-from-evidenced-value` | `input` (`route_recalculated`): its order and part | an `input` (`segment_settled`) of that order | part, node (the settled segment's `to` in its plan is the new route's first `from`) | `REQ-ORD-2`, `INV-4` | admitted: `world--*`; refused: `--missing`, `--part`, `--node` |
| `order-is-opened-once` | `input` (`route_chosen`): its order | none other: no other `route_chosen` of that order (the same input retried is admitted; a linked order opened by `handed_on_from` is its own order) | — | `REQ-ORD-19` | admitted: `world--*`, `--retried`; refused: `--conflict` |
| `answer-follows-its-question` | a stated entry of a provider's `result` naming a transaction: its `about.transaction` | an earlier stated entry of an `operation` under that transaction | verified, maker (its verified party is the deployment's operator party, which `admit_entry` is handed), order (the answer's, where it names one) | `REQ-ORD-32`, `REQ-AUD-2` | admitted: `world--*`; refused: `--missing`, `--verified`, `--maker`, `--order` |
| `transaction-belongs-to-one-order` | a stated entry about an order: its `about.transaction` | none other: no earlier stated entry naming the same transaction under another order or another segment (the first fixes both; the same entry retried is admitted) | — | `REQ-ORD-32`, `REQ-ORD-38` | admitted: `world--*`, `--retried`; refused: `--conflict`, `--segment` |

Every row has a refused case for its target missing (or, for `none other`, present) and one for each field it
matches, each named `<row>--<field>`; `world--<entry>` cases admit each entry of one coherent log on what came
before it, and `--retried` cases admit an identical effect or result again. `tests/core-internal.rs` reads the
expected refusal from `RELATIONS` itself, and checks that every row and field has its case and every row is in
this table with its ids.

Entries that cite nothing earlier are admitted on their own checks: `authorise`, `open_session`, `charge`,
`hand_over`, `holding_check`, `order_refusal`, `destination_check`, `settlement`, `stuck_call`, `command`,
`grant_permission`, and stated entries.

- **Only a verified statement answers for its maker.** Every row that names a statement's maker requires its
  attribution to be `verified`: an unverified statement is attributed to no one (`REQ-AUD-2`).
- **A grant's scope covers what lies under it.** A permission over the trail covers every order and segment; one
  over an order covers its segments; any other scope covers only itself (`covers`). A `stuck_decide`'s evidence is named by digest in the evidence store, not
the log. An `acceptance` names its authorisation, which may be registered after it; the value-moving effect holds
the two together.

- **Each action travels one path.** A stuck order's value moves only on a `stuck_move` permit: a
  `DecisionRequest` never carries `resolve_stuck` (its schema and its `check` refuse it), and the log refuses one
  resting on a `decide` permit. An operator command is one of a closed list that needs no decision
  (`appoint_checker`), made by the operator only, so no decided action, catalogue change, hand-on or permission,
  under any name, is recorded as a command with no decision behind it. A read permission goes through policy
  (`Reads::grant_permission`, `Reads::revoke_permission`), so policy learns of each grant; an arrival filed anew is a
  `refiling`, by the operator or the reserve it arrived at. Only `decide` and `stuck_move` entries back an effect;
  `authorise` and `stuck_decide` permits back none. A kept statement leaves the store only through
  `Evidence::fetch`, which runs its own read check.
- **Not known moves nothing.** `not_known` is its own answer, never a refusal and never a permit (`REQ-AUZ-5`).
- **Detection only reaches the operator.** The checks answer orchestration with a receipt and nothing else, so
  nothing orchestration holds can act on what a comparison found. A difference goes to `Operator::raise`, which
  answers only whether it was delivered; the raise's entry records that (`done` or `failed`). A `Raise` holds its id,
  the difference, the receipts it rests on and its time, and no field that could change an order or move value
  (ADR-0009).
- **The handed-over items** (the platform's drained events, the proxy's uses, each `SegmentRecord`) are appended as
  verified entries, `call: hand_over`, each with the drain that took it and its place in that drain. An append that
  fails leaves them with orchestration, which appends them again: an equal entry appended again is one entry, and
  two identical events of one drain differ by their place, so neither is lost nor merged (`REQ-MDL-8`).
- **Notices** are not here: they are `F-023`'s module inside `CMP-CORE-ORCH`, one crate.
- `AppendFailure`, `Kept`, `Fetched` and `Delivery` are library API results with no wire form.

### How each answer is recorded

| Answer | Its entry's outcome |
|---|---|
| `Decision`: `permit`, `refuse {class}`, `not_known {cause}` | `permitted`, `refused {class}`, `not_known {cause}` |
| `SessionAnswer`: `opened`, `refused`, `not_known` | `permitted` (the entry names the session), `refused`, `not_known` |
| `ReadAnswer`: `allowed`, `refused` | `permitted`, `refused` |
| `ChargeAnswer`: `within`, `over` | `permitted`, `refused {class: over_bound}` |
| `PersonAnswer`: `allowed`, `refused`, `past_bound`, `not_known` | `permitted`, `refused`, `refused {class: over_bound}`, `not_known` |
| `Delivery`: `Delivered`, `Undelivered` | the raise's `done`, `failed` |

The Rust types carry the mapping as `Answer::outcome`, `SessionOutcome::outcome`, `ReadOutcome::outcome`,
`ChargeOutcome::outcome` and `PersonOutcome::outcome`.

## The shapes

| Shape | Schema `$id` | Rust |
|---|---|---|
| Shared atoms | `urn:rutent:core-internal:common` | `Actor`, `Role`, `ActorKind`, `When`, `StatedWhen`, `Destination`, `RouteEnd`, `Action`, `CatalogueChange`, `Decided`, `Opened`, `RefusalClass`, `NotKnownCause`, `Token`, `Count` |
| Receipt | `urn:rutent:core-internal:receipt` | `Receipt` |
| Decision request | `urn:rutent:core-internal:decision-request` | `DecisionRequest` |
| Decision | `urn:rutent:core-internal:decision` | `Decision`, `Answer` |
| Effect | `urn:rutent:core-internal:effect` | `Effect`, `Permit` |
| Authorisation | `urn:rutent:core-internal:authorisation` | `Authorisation` |
| Stuck order: decide | `urn:rutent:core-internal:stuck-decide` | `StuckDecide`, `Resolution` (`deliver` or `hand_on`) |
| Stuck order: move | `urn:rutent:core-internal:stuck-move` | `StuckMove` |
| Session request and answer | `urn:rutent:core-internal:session-request`, `…:session-answer` | `SessionRequest`, `SessionAnswer` |
| Read check and answer | `urn:rutent:core-internal:read-check`, `…:read-answer` | `ReadCheck`, `Reads`, `ReadAnswer` |
| Charge and answer | `urn:rutent:core-internal:charge`, `…:charge-answer` | `Charge`, `ChargeAnswer` |
| Question about a person and answer | `urn:rutent:core-internal:person-question`, `…:person-answer` | `PersonQuestion`, `Authority`, `PersonAnswer` |
| Segment report pair | `urn:rutent:core-internal:report-pair` | `ReportPair`, `Report` |
| Holding account reading | `urn:rutent:core-internal:holding-reading` | `HoldingReading`, `Held` |
| Measure record | `urn:rutent:core-internal:measure-record` | `MeasureRecord`, `Measure` |
| Handed over to the log | `urn:rutent:core-internal:handover` | `HandedOver` |
| Raise | `urn:rutent:core-internal:raise` | `Raise`, `Difference` |
| Verified entry | `urn:rutent:core-internal:verified-entry` | `VerifiedEntry`, `What`, `Outcome`, `OperatorCommand` |
| Stated entry | `urn:rutent:core-internal:stated-entry` | `StatedEntry`, `Attribution`, `SignedWith`, `About` |
| Log entry, either kind | `urn:rutent:core-internal:entry` | `Entry` |
| Kept statement | `urn:rutent:core-internal:kept-statement` | `KeptStatement`, `Salt`, `Encoded` |
| Permission change | `urn:rutent:core-internal:permission-change` | `PermissionChange` |

Every request carries its own id, which its answer names, and an `at`: Core's clock, the one it passes as `at`
(`F-012`'s notes). Discriminated unions name their branch in one field (`kind`, `answer`, `call`, `status` or
`signature`) and close each branch with `additionalProperties: false`, the module interface's form.

### Who, and when

- **An actor** is `{party, acts, role, kind}`: the accountable party it acts for, where limits bind and where a stuck
  order's two actors are compared (`REQ-AUZ-8`, ADR-0011); the actor itself, as external authentication established
  it and no more (`REQ-AUZ-1`); the role it acts in, recorded apart for each role a party holds (`REQ-AUD-8`); and
  whether it is a `person`, an `agent` or a `component`. Humans and agents use one contract (`REQ-OPS-5`): an agent
  acts in its party's role, `acts` naming the agent and `kind` saying it is one, so an agent's acceptance is
  recorded as an agent's (`REQ-ORD-10`). Both identifiers are references, never an identity (`REQ-AUD-6`).
- **A receipt's digest** is SHA-256 of the entry's RFC 8785 canonical JSON (provider-role's `canonical` encoder), the
  `Entry` exactly as this contract serialises it, with no field added.
- **Two kinds of entry, two shapes** (`REQ-AUD-2`). A *verified entry* is what rutent did or saw: `who`, `what`,
  `when` with its source, `core_clock` or `platform_clock`, and an `outcome`. A *stated entry* is what a party said:
  its `attribution` (`{signature: verified, who}` where the signature verified against a key of its maker's, and
  `{signature: unverified, claimed_by}` otherwise, attributed to no one), `what` its shape, `when` its `made_at` with
  the source `party_stated`, Core's `received_at`, the outcome `stated`, `signed_with` (the scheme and key it says
  signed it), a salted `commitment` to its bytes as received, optionally `about` (its order, segment or transaction)
  and who relayed it where someone did. Neither validates as the other, and `entry` chooses between them by `kind`
  (`REQ-AUD-1`).
- **The bytes a signature covers stay apart from the log.** A statement's signature covers the RFC 8785 canonical
  JSON of the envelope as received, without `signature.value` (provider-role's signing rule, `signing_input`), so
  the bytes as received are what must be kept. Orchestration hands them to `Evidence::keep` before it appends the
  stated entry, and the store keeps them, with the signature's value inside them, beside a fresh random salt of 32
  bytes; it answers with the commitment, SHA-256 of the salt followed by the bytes. The log holds neither the bytes,
  nor the signature's value, nor the salt, so nothing in it lets anyone confirm a guess of what a statement says: a
  `holding_balance` result's balance, low in entropy, would otherwise be confirmable by trying values against an
  unsalted digest or the signature (`REQ-AUD-6`).
- **A kept statement is checked against its entry** by anyone `Evidence::fetch` hands it to: a party for its own
  evidence (`REQ-AUD-5`) or a checker by its permission (`REQ-AUD-9`, `REQ-AUD-10`, `T-022-04`). The store asks
  policy through `Reads` (`reads: evidence`) first. The steps:
  1. recompute SHA-256 of the decoded `salt` followed by the decoded `bytes`, and compare it with the stated entry's
     `commitment`;
  2. read the bytes as the statement's envelope, check that its signature's scheme and key equal the entry's
     `signed_with`, and verify the signature over `signing_input` of the bytes.
- **Restatements are found in the store, not the log.** The salt that hides a statement's content also hides that
  two entries hold one statement. The store keeps an unsalted index, the SHA-256 digest of each statement's signed
  content, which never enters the log, and `keep` answers with the commitment of the statement a new one restates,
  so a relay's copy of another party's statement is detected and stays that party's (`REQ-AUD-2`, `T-022-02`).
- **The log holds no balance** (`REQ-AUD-6`). A `HoldingReading` goes to the check, not to the log; its entry is
  `holding_check`, which names the account, its node, whose word the balance was and the orders, without the
  balance. A raise carries the difference, not the balance. A stated entry holds no statement's content.

### Where each record Core writes lives

Every record a row of `F-020`, `F-021` or `F-022` writes has a home in a verified entry's `what`, a stated entry or a
raise. A record not listed here needs a bump of this contract.

| Row | Record | Home |
|---|---|---|
| `T-020-02` | the fee formula and slippage limits a decision ran under | `decision_record`, by the record's digest |
| `T-020-03` | requester, beneficiary and payer apart; the acceptance, an agent's as an agent's | `acceptance` (`parties`, `basis: given`, `who.kind`) |
| `T-020-03` | a settlement-account beneficiary, its payees' checks resting with its reserve | `acceptance.parties.settlement_account_at` |
| `T-020-03`, `T-020-13` | the acceptance a state-machine input cites, both pointing to each other | `acceptance.acceptance`, the id `destination_changed` and `handed_on` cite |
| `T-020-03` | one order per request identifier; refused, nothing exists | `acceptance.request`; `order_refusal` without `order` |
| `T-020-03` | a stable class of cause, no provider text | `order_refusal.class`, closed to anything else |
| `T-020-03` | the destination resolved, not yet, no, or its owner changed | `destination_check` |
| `T-020-03` | a deposit to an address the requester gives | the authorisation's `destination.account` |
| `T-020-04` | an arrival with no order, or differing from one, and the reserve's report | `arrival` citing the reserve's stated entry |
| `T-020-04` | an automatic deposit counting as acceptance | `acceptance` with `basis: automatic_deposit` |
| `T-020-04` | an arrival filed anew by the operator or the reserve it arrived at | `refiling`, by `who` |
| `T-020-05` | a statement refused before the record, a route refused | stated entry `unverified`; `order_refusal` |
| `T-020-06` | each round's decision record, chained to the order's history | `decision_record` with `previous`, its `decision_record` the id the state machine's inputs cite |
| `T-020-06` | questions fanned out, counted against the caller | `charge`; `ask_about_person` |
| `T-020-07` | the quote shown, labelled a quote, without a firm rate, or the price | `shown` |
| `T-020-07` | stated terms with their times; a held quote | stated entries |
| `T-020-07` | a charge above its formula's maximum, flagged | raise, `difference: charge` |
| `T-020-08` | segment records, platform events, proxy uses | `hand_over` |
| `T-020-08` | a result matched to its operation; one final outcome per segment | `result`; `input` (`segment_settled`, `segment_failed`, `segment_outcome_unknown`) |
| `T-020-08` | a place a provider sends a person to approve | stated entry |
| `T-020-08` | actual settlement time beside the declared one | `settlement` |
| `T-020-09` | graded evidence, a statement alone as a statement, whose it is | `evidence`, its `evidence` the id `segment_settled` cites |
| `T-020-09` | a fee record per settled segment, to a provider or the operator | `fee` |
| `T-020-10` | a recalculation; a provider passed again; waiting past the floor | `input` (`route_recalculated`); `decision_record` |
| `T-020-11` | stuck, and its processing | `input` (`stuck_declared`, `no_route`); `stuck_decide`, `stuck_move`, `effect` of `resolve_stuck` |
| `T-020-11` | the figures behind a stuck call; a change in a provider's p99 | `stuck_call`; raise, `difference: settlement_time` |
| `T-020-12` | a reversal complete, on another party's statement | `input` (`reversal_completed`) citing a stated entry |
| `T-020-12` | a complete order returned; a late origin approval | `afterwards` |
| `T-020-13` | a failed final deposit; a route handed on; records pointing to each other | `input` (`segment_failed`, `destination_changed`, `handed_on` naming `linked_order`, `route_chosen` naming `handed_on_from`) |
| `T-020-13`, `T-020-11` | the operator handing a stuck order's value on, under `REQ-AUZ-4` | `stuck_decide` with `resolution: hand_on`, `stuck_move`, `effect` of `resolve_stuck` |
| `T-021-01` | each decision before it answers | `decide` |
| `T-021-02` | sessions; each read, allowed or refused; a read permission granted or revoked | `open_session`; `check_read`; `grant_permission`, `revoke_permission` |
| `T-021-03` | a route posted, its suggester; each permission with its actor; who acted for the user | `decide` (`post_route.suggested_by`, `change_catalogue`, `who`); `authorise.given_by` |
| `T-021-04` | an authorisation; one given anew | `authorise` |
| `T-021-05` | work charged to the party | `charge` |
| `T-021-06` | decide and move, the evidence relied on | `stuck_decide` (`evidence`), `stuck_move` |
| `T-021-07` | questions about people, the set asked; a reserve's proof; presence; a payer's reference | `ask_about_person`; stated entries; `acceptance.parties.payer` |
| `T-021-07` | whose judgement was relied on, and by whom | `reliance` |
| `T-022-01` | every action, refusal and unanswered request | the entries above; `result` with `unanswered` |
| `T-022-02` | statements apart from what rutent verified; an operator command naming its operator | stated entries; `command` with the operator in `who` |
| `T-022-03` | evidence apart from the log, by reference; a party's or checker's copy; an erasure | `Evidence::keep`; `Evidence::fetch` and `check_read` of `reads: evidence`; `erasure` |
| `T-022-04` | checkpoints | outside the log, in `CMP-CORE-AUDIT`'s own store (ADR-0005) |
| `T-022-05` | a report pair compared; a holding account checked; a difference raised | `report_pair`; `holding_check`; `raise` |
| `T-022-06` | the bulk measures; the regulator's read | `measure`; `check_read` with `reads: trail` |

### The rules, and where each is enforced

Every rule of this contract's own schemas is enforced by the Rust types too; the shapes of other contracts it
carries are as strict in Rust as those contracts make them (module-interface's `Name`, for one, checks only that it
isn't empty, and provider-role's `Denomination` and `OuterNode` hold plain text). These are the rules JSON Schema
can't state, each checked by the shape's `check`, which every enclosing shape's `check` runs too:

| Shape | Rule |
|---|---|
| `DecisionRequest` | it never asks for `resolve_stuck`, which travels only as a stuck move (the schema states it too) |
| `Action` | a catalogue change names a version exactly when it is about a module release (`accept_module`, `revoke_release`, `mark_stale`, `declare_exploited`, `reinstate`, `withdraw_release`), and a tuple exactly when it is about a tuple (`grant`, `revoke_grant`, `set_slippage`) |
| `SessionRequest` | a session ends after the time it is asked for |
| `Charge` | work is charged to the accountable party its caller acts for |
| `StuckDecide`, `PersonQuestion`, `MeasureRecord`, `Raise`, `What::Evidence` | the lists the schemas require non-empty and distinct are (the schemas state it too) |
| `ReportPair` | a segment's two reports are by two different parties |
| `HoldingReading` | a holding account reading names each order once |
| `HandedOver` | a segment record's moves lead from the version it began on to the one it runs on, each from the version the one before moved to; a proxy use names its operation exactly when it asked for a secret |
| `Raise` | its difference is a real one: two amounts or times that differ, a charge above its maximum |
| `VerifiedEntry` | an entry's outcome is one its call can have: a policy call's `permitted`, `refused` or `not_known`; an effect's `to_take_effect`; a result's `done`, `failed` or `unanswered`; a raise's, a command's and an erasure's `done` or `failed`; any other record's `observed`. A session entry names the session exactly when it was opened, ending after it was asked for and no later than its `until` |
| `What::Input` | the recorded input holds to the state-machine contract's own `check` |
| `StatedEntry` | its relay is a party other than its maker; its `about` names something |
| `VerifiedEntry` of a `command` | its `who` is the operator |
| `GeneralEntry` | it holds no decision kind (`calls::is_decision`), and no stated entry attributed `verified`, however it is built or read |
| `DecisionEntry` | it holds a decision kind and nothing else, however it is built or read |
| `VerifiedEntry` of a permitted `check_read` | a regulator's or a checker's names its permission; a provider's is of a segment, never an order or the trail; a customer's is never of the trail |
| `PersonQuestion` | on a shortlisted route, it asks only the shortlisted providers |
| `PermissionChange` | its grantee is a regulator or a checker, and `by` is the operator |
| `What::Acceptance` | it names the reserve's yes and the session exactly when its basis is given |
| `What::DecisionRecord` | it names its shortlisted providers, at least one, each once, exactly when its round is the shortlist |

Each fixture under `fixtures/invalid/` breaks exactly one rule, named in its file name. `tests/core-internal.rs`
lists, for each, the exact schema errors and the Rust refusal it gives, or, for a rule only Rust states, that the
schema accepts it and the Rust refusal.

## What it refers to, never copies

- From provider-role: identifiers, amounts, positive amounts, safe integers, timestamps, public keys and evidence
  grades (`common`); inner and outer nodes and denominations (`advertisement`); the statement envelope's `shape` and
  its signature's `scheme` (`statement-envelope`). In Rust: `router_api`'s checked `Id`, `Amount`, `PositiveAmount`
  and `Timestamp`, and provider-role's `SafeInteger`, `PublicKey`, `SignatureScheme`, `EvidenceGrade`, `OuterNode`
  and `Denomination`.
- From the module interface: the digest and the declared name (`common`); the outbound call record, the cross-module
  call and the secret lookup, whole; and the proxy's refusal reasons for a secret (`secret-answer`). In Rust, its own
  types: `Digest`, `Name`, `OutboundCallRecord`, `CrossModuleCall`, `SecretLookup`, `SecretRefusalReason`.
- From the state machine: the recorded input, whole (`recorded-input`), in Rust `RecordedInput`.
- The state machine's part destination is `{provider, to}`, an endpoint; the authorisation's destination is
  `{reserve, node, account}` with the to asset apart, because `REQ-AUZ-3` names the destination and the to asset
  separately and `REQ-ORD-35` makes an account or address the requester gives part of the destination. Its
  `reserve` is the part destination's `provider`, its `node` the part's `to.node`, and `to_asset` its `to.asset`.

## Visible to every crate, named by Core alone

The contract is in `rutent-contracts` (`PLAN.md` §4.4's reading): the one crate all three parts already reach, so it
adds no edge between them and the contract guard keeps every change a reviewed bump. Its cost: the outer libraries
reach the same crate, and a crate-level check can't stop them naming these types. `tools/boundary-check` therefore
also reads source, and fails:

- when the router, the state machine, the provider system or the evidence checker names `core_internal` anywhere in
  its package's tree (guests and nested workspaces included, the package's own `target` aside), comments included;
- when any contract module other than this one names it, the crate root's one `pub mod core_internal;` aside, so no
  re-export carries it out under another name;
- when a public item of the conformance suites, which may use it, names it: a `pub use`, `pub type`, `pub const` or
  `pub static`, a public function's signature, a public type's fields or variants, a public trait's items, a trait
  impl, an exported macro (comments and literals aside);
- when an attribute with a `path` key (`#[path = ..]`, `# [path = ..]`, `#[cfg_attr(.., path = ..)]`, over any
  number of lines) or an `include!` in any of those trees mounts a file from outside its package, or can't be
  resolved;
- when this module carries a `#[macro_export]`, which would export a macro at the crate root;
- and, as an error, when a checked tree holds a symbolic link.

The scenario assembly, `rutent-scenarios`, is another member and may import every crate of ours.

## Hand-offs and readings

These are defaults taken where the row left a choice open. Each is reversible by a later `F-020` row's bump.

- **State reads carry their transaction.** This row added the transaction to `rutent_prov::module::StateEvent`,
  which the platform already held for the operation in flight, so every handed-over item names one (`REQ-MDL-8`).
- **The proxy's and the platform's reasons are tokens.** A refused proxy use and a bound hit name their reason as
  the component gives it (`no_release`, `outbound_calls`), a lower-case token, so a
  reason the proxy or the platform adds needs no bump. They are labels to read: no decision may branch on them.
  `T-020-08` maps prov's enums to these tokens with an exhaustive match, so a new prov variant fails its build until
  it is named. A secret lookup's refusal reason is the module interface's enum, since that contract owns it.
- **Measures.** All six of `REQ-AUD-7`'s: quotes against executed prices, dry runs against outcomes, failures against
  market direction, refusals, reversals and answers towards the rate of no. A failure's `market` is optional: no
  part holds a market figure in `M3`, so `T-022-06` records failures without it until one does, with no bump. Each
  record cites the entries it rests on, so a provider's word stays in its stated entry.
- **The evidence store holds statements as received**, a holding account's balance among them where a
  `holding_balance` result is one; the log holds only a salted commitment. Whether those bytes may be kept at all is
  on the owner's form; until he rules, they are kept only in the evidence store, never the log.
- **The time sources** are `core_clock` and `platform_clock` for verified entries and `party_stated` for stated
  ones. A network's block time reaches the log inside a party's statement, as that party's word.
- **A stuck order's move** names the decision it carries out, not the decider: policy holds the decision and compares
  the two actors at their accountable party (`T-021-06`). Its value moves as `resolve_stuck`, outside the order's
  authorisation, since a resolution back to the origin isn't inside it. The decide step names the whole outcome,
  the provider that moves it included (`resolution.by`), and the log admits the effect only on that outcome.
- **Handing value on.** The operator hands a stuck order's value on to a linked order only as a stuck resolution,
  `resolution: hand_on {by, linked_order, asset, amount}`, decided and moved through `stuck_decide` and
  `stuck_move` and admitted like any `resolve_stuck`, acting under `REQ-AUZ-4` (`REQ-ORD-13`, `REQ-ORD-43`). A
  customer's own hand-on, the other path `REQ-ORD-43` gives, is its acceptance of the new order (`acceptance`, and
  the `handed_on` input citing it); it moves nothing until the linked order's own `move_value` under its own
  authorisation, so it needs no decided action of its own. `REQ-ORD-13`'s operator hands on only under `REQ-ORD-43`,
  that is, for a stuck order, so no other operator hand-on exists.
- **Acting once on an effect is orchestration's.** The log admits an identical effect entry again, as a retry of an
  append that failed, so it can't stop orchestration from acting twice on one receipt. `T-020-08` owns an effect's
  idempotency: it acts on each effect's receipt once, whatever retries the append had (`INV-8`).
- **A question on a shortlisted route** carries the shortlisted providers, asks only among them (`REQ-CPL-6`, a
  shape rule), and its shortlist's decision record names them (`shortlisted-authority-cites-its-shortlist`). A read
  check is taken from every role; policy answers it, and a permitted one is held to who may read
  (`customer-read-is-of-its-own-order`, `provider-read-is-of-its-own-segment`, the read rules above).
- **Refusal classes for orders** are the seven `order_refusal` names; one `REQ-ORD-18` needs beyond them is a bump.
- **The conformance suites** (`rutent-conformance`) may name this contract inside their own code, as `F-024` may
  write a suite against it, but no public item of theirs may name it, so a suite's public functions can't take or
  return its types. A suite that needs them in its interface lives in the scenario assembly, which may.
- **A reversal's evidence and the failed provider's publisher.** `REQ-ORD-3` wants the statement that a reversal
  is complete to come from a party other than the failed provider and its publisher. The table checks the provider
  half (`reversal-rests-on-its-evidence`, `whose`); no entry names a provider's publisher, so the publisher half is a
  residual for `CMP-EVIDENCE` to hold when it grades the evidence (`T-020-12`).
- **The reserve's yes and the user's session** (`REQ-ORD-32`). An acceptance given names its origin reserve, the
  receipt of that reserve's stated yes about the order, and the session it was asked in; an automatic deposit names
  neither, as `REQ-ORD-32` exempts it.
- **Reads and sessions.** No row ties a read to a session: `SPEC.md` asks no session of a reader (`REQ-CAT-3`'s
  sessions bound a reserve's balance checks), and a customer authenticated outside rutent must still read its
  order's state (`REQ-AUZ-1`, `REQ-ORD-5`, `AC-12`). A rule tying reads to sessions needs a `SPEC.md` ruling first.
- **Policy's handle** (amending `T-020-01`'s row in `PLAN.md`, after it merged). Until then any part holding the
  log could append a permitting decide of its own, and the log admitted an effect on it, since it held a decide to
  any author. The fix is in the types and the crate graph rather than a new row of the citation table: a decide
  written by policy and one written by orchestration look alike to the table. `T-020-05` and `T-021-01` hold the log
  through these handles, orchestration through `Log` and the engine through `PolicyLog`, and `T-022-01` implements
  both on its log.
- **Attribution by the log** (amending `T-022-01`'s row in `PLAN.md`, after `T-020-01` merged).
  Until then a stated entry's attribution was its writer's word, and the rows that require a verified statement
  (`sent-report-is-its-partys-statement`, `received-report-is-its-partys-statement`,
  `arrival-rests-on-the-reserves-report`, `reliance-names-its-judges-statement`,
  `acceptance-rests-on-the-reserves-yes`) trusted it: orchestration could log a verified yes the origin reserve
  never gave. `T-020-05` appends each statement unverified, naming its claimed maker, after keeping its bytes;
  `T-022-01`'s log implements `attribute` over `KeptStatements`, `StatementKeys` and `Signatures`, and hands
  `admit_entry` the operator party `StatementKeys::operator` answers; `T-022-03`'s store answers `KeptStatements`.
  `core-audit`'s own checks (`T-022-05`, `T-022-06`) append through a general handle, never the log's own type, so
  they write no decision and attribute no statement.
- **Knowing a statement verified** (`T-020-03`, `T-020-05`). `Log::append` answers a receipt, not the attribution
  it recorded. Orchestration ties a statement to its maker's key before it relies on it (`T-020-05`), so it knows
  beforehand what the log will find; where the two differ, the log's entry is the one a relation reads, and an
  acceptance resting on a yes the log recorded unverified is refused.
- **A statement is bound to its order before it is answered** (`REQ-ORD-32`, `REQ-ORD-38`). No provider signs an
  order; it signs the transaction Core asked it under. Core appends its question first, an `operation` signed with
  the operator's key, about the order it asks for and the transaction it asks under, so the binding exists before
  any answer can. A provider's result naming a transaction is admitted only after that question
  (`answer-follows-its-question`), about the order the question asked for; is verified about an order or a segment
  only where its `about` names the transaction its body names; and `transaction-belongs-to-one-order` refuses any
  later entry naming that transaction under another order. Every row that cites a statement holds its `about` to
  the citing entry's. A genuine yes about one order can't be cited for another, whether its transaction is left
  out or kept, and whether it is appended before or after the genuine one.
- **A question before each answer** (`T-020-03`, `T-020-04`, `T-020-06`). Core asks each question under a fresh
  transaction for its order, never one an earlier order or question used, and appends the question before it sends
  it, so its answer, appended with that transaction in its `about`, is admitted.
- **Asking under a false label** (a residual for `T-025-05`). The log can't tell that the order a question names is
  the one Core asked for: orchestration could ask a question for one order and label it another before any answer.
  It can be caught, because the question's signed body names the account and the amount asked about, which a
  checker compares with the labelled order's; the log does not compare them.
- **A report's party** (`T-020-07`, `T-022-05`). `Report.party` is "the party that reports", and
  `sent-report-is-its-partys-statement` and `received-report-is-its-partys-statement` compare it with the
  statement's `party`, the publisher it states for. Should a report name the provider instead, those two rows move
  to `acts` with it.
- **Whose keys** (a residual for `T-025-05`). The catalogue the state machine holds is state orchestration drives,
  so `StatementKeys` takes a maker's keys only from advertisements the log itself recorded under permitted
  `change_catalogue` effects (`T-022-01`). A key is checked as current at the statement's own `made_at`, so a key
  revoked since can still sign a statement backdated to before its revocation (`THR-3`).
- **One process** (a residual for `T-025-05`). The handles keep decision entries to the policy engine while Core's
  parts share one process, where only the assembly can reach the log. Were the parts split across processes, a
  decision entry would need the policy engine's signature, which the log checks before it admits one; no such
  signature exists in this contract.
- **The join ids** are the state machine's own: an `acceptance` entry's `acceptance`, a `decision_record` entry's
  `decision_record` and an `evidence` entry's `evidence` are the ids its inputs cite, and
  `tests/core-internal.rs` joins each input fixture to its records by them (`REQ-ORD-34`, `REQ-ORD-43`).
