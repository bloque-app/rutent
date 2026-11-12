# core-internal changelog

## 1.0.0
- The interfaces between `CMP-CORE-ORCH`, `CMP-CORE-AUTHZ` and `CMP-CORE-AUDIT`, settled as a sixth contract before the
  three parts are built side by side (`PLAN.md` §3, §4.4). From orchestration to policy: one decision call for every
  action that moves value, posts a route, changes the catalogue or moves a stuck order's value, answering permit, refuse
  with its class or not known, with a permit carrying its decision entry's receipt; the authorisation naming one order,
  its origin reserve, its destination, its to asset and any floor, which every value-moving effect stays inside; a stuck
  order's decide and move steps as two calls (ADR-0011), resolving it by delivering its value or handing it on to a
  linked order, the decider's party never the mover's; sessions; a charge of work to the caller's accountable party,
  answered within or over its bound; a question about a person, with its asker, the person's reference, its authority
  (with the shortlisted providers on a shortlisted route) and the providers already asked about its order, answered
  allowed, refused or past the bound; a read check of an order, a segment with its provider, the whole trail, a named
  resource or a kept statement, recorded either way, and read permissions granted and revoked through policy. From
  orchestration to the log's checks: a segment's pair of reports, a holding account's reading and the bulk measures'
  records, each answered with a receipt and nothing else. From orchestration and policy to the log: an append answering
  with a receipt before the effect it records, admitting an effect only on an unused permit for a permitting entry of
  the log that decided the same action, a stuck order's value only as its decide step's resolution names it, the
  provider that moves it included, a result only of an effect of the log and only once, and every other entry only on
  the earlier entries it cites, since a permit can be built anywhere, and each action on one path only (a stuck order's
  move never a decision request, an operator command one of a closed list that needs no decision, and the operator's; an
  arrival filed anew a refiling, by the operator or its reserve); an evidence store keeping each statement's bytes apart
  from the log and handing them, by permission, to a party or a checker with the steps to check them; two shapes of
  entry, stated and verified, each with who, what, when, the source of when and the outcome; a stated entry attributed
  only where its signature verified, holding the key it was signed with and a salted commitment to its bytes, which a
  call keeps in the evidence store apart from the log, so no statement's content can be confirmed from the log; a
  verified entry for every record Core's parts write, the state machine's recorded inputs and orchestration's own
  records included (the order's parties, a settlement-account beneficiary among them), each record a state-machine input
  cites joined to it by the input's id; the platform's events, the proxy's uses and each segment record handed over with
  their place in their drain. From the checks to the operator: a raise carrying a difference and nothing that can move
  value, answered only by whether it was delivered (ADR-0009). Every shape's check recurses, so a value built in memory
  is held to the rules a value read from the wire is. Twenty-six schemas, 116 valid fixtures and 115 invalid ones, each
  breaking one rule, and 311 cases of the log's admission of an entry, from one coherent log, against a citation table
  of 65 relations derived from SPEC.md, each with the ids that demand it. An acceptance given cites its origin reserve's
  yes and the user's session; a shortlist's decision record names its shortlisted providers. (DEC-2, DEC-4, DEC-20,
  T-020-01)
- Folded into 1.0.0 before its release: the log takes entries through two handles. `Log::append` takes a
  `GeneralEntry`, which holds no decision kind, and is orchestration's and the checks'; `PolicyLog::append_decision`
  takes a `DecisionEntry`, which holds only decision kinds, and is policy's alone. Neither type can be built or read
  holding the other's kinds, and `admit_entry` takes the path an entry came by and refuses a decision kind on the
  general one, so no part but the policy engine can write the decision an effect is admitted on. Twenty-two of the
  admission cases name the path. (DEC-2, DEC-4, DEC-20, REQ-AUZ-1, INV-7, T-020-01)
- Folded into 1.0.0 before its release: the log, not the writer, attributes a stated entry. A `GeneralEntry`
  refuses a stated entry attributed `verified`, so a statement reaches the general handle naming only its claimed
  maker and its bytes' commitment. `attribute` records it verified only where the bytes the evidence store keeps
  under that commitment are an envelope of this deployment, by that maker, of the entry's shape, time, key and
  transaction, whose signature verifies under a key that signs the maker's statements at its `made_at`, and
  otherwise unverified; it refuses an entry whose bytes aren't kept. The log holds three new traits for it:
  `KeptStatements`, the evidence store's read by commitment; `StatementKeys`, the deployment and the makers' keys
  from the catalogue, never from orchestration; and `Signatures`, the signature check. Thirty-five attribution cases,
  under `fixtures/attribution/`. (DEC-4, DEC-20, REQ-AUD-2, REQ-PRT-1, THR-3, T-020-01, T-022-01)
- Folded into 1.0.0 before its release: a reserve's yes is bound to its order through its signed transaction. A
  statement about an order or a segment is verified only where its `about` names the transaction its body names;
  `acceptance-rests-on-the-reserves-yes` requires that transaction; and a new none-other row over stated entries,
  `transaction-belongs-to-one-order` in `STATED_RELATIONS`, refuses a later entry naming a transaction under
  another order than the first. `Relation`, `Cites` and `Answers` take the citing entry's kind as a parameter,
  verified by default. (DEC-4, DEC-20, REQ-ORD-32, REQ-ORD-38, T-020-01)
- Folded into 1.0.0 before its release: a statement is cited only for what it is about, and a yes follows its
  question. Core appends each question it asks, an `operation` signed with the operator's key and about the order
  and transaction it asks under, before its answer; `answer-follows-its-question`, a second row of
  `STATED_RELATIONS`, admits a provider's result naming a transaction only after that question and about its
  order, so a genuine yes can't be bound to another order first. Every row that cites a statement (the sent and
  received reports, an arrival's report, graded evidence, what came after an order ended, a reliance's judgement)
  holds the order, segment and transaction its `about` names to the citing entry's, a transaction by the order it
  belongs to; a statement about nothing stays citable on its body. A judgement, which carries no transaction, is
  appended about no order and bound by its reliance. 206 admission cases and 19 attribution cases.
  (DEC-4, DEC-20, REQ-ORD-32, REQ-ORD-38, REQ-ORD-39, REQ-CPL-12, REQ-AUD-2, T-020-01)
- Folded into 1.0.0 before its release: a statement naming a transaction is bound through it. `attribute` verifies
  a statement whose body names a transaction only where its `about` names the same one, so no result is verified
  about nothing and none escapes `answer-follows-its-question` or the `about` checks; a judgement, which names no
  transaction, stays about nothing. `transaction-belongs-to-one-order` fixes the segment the first entry names as
  well as the order, and a cited statement's transaction must belong to the citing entry's segment where it names
  one. `answer-follows-its-question` compares the question's verified party with the deployment's operator party,
  which `admit_entry` now takes as a fourth argument and `StatementKeys::operator` answers; the stated rows read a
  `StatedCiting`. A customer's segment read names a segment its order's plan lists, and only a permitted
  revocation must revoke a grant, so a refused revocation of a permission never granted is recorded.
  (DEC-4, DEC-20, REQ-ORD-32, REQ-ORD-38, REQ-AUD-2, REQ-AUZ-7, T-020-01)
- Folded into 1.0.0 before its release: graded evidence claiming a signature rests on verified statements. Every
  statement a grade-2 or grade-3 evidence entry cites must be one the log verified; a grade-1 public record rests on
  the network. (DEC-4, DEC-20, REQ-ORD-39, REQ-AUD-2, T-020-01)
- Folded into 1.0.0 before its release: evidence of any grade is bound to its order and segment. A grade-4 bare
  statement is its provider's word, so it rests on a verified statement made by the provider this log's `move_value`
  names for the segment (`REQ-ORD-38`, `REQ-AUD-2`); and a statement that isn't verified, which only a grade-1 public
  record may rest on, must name a transaction bound to the evidence's order and segment.
  (DEC-4, DEC-20, REQ-ORD-38, REQ-ORD-39, REQ-AUD-2, T-020-01)
- Folded into 1.0.0 before its release: the log binds a statement's maker to its key and its publisher.
  `attribute` compared the envelope's `maker` with the claimed `party`, though the envelope names the provider,
  `acts`, and `party` is its publisher, so a provider whose publisher differs from it could never be verified, and
  `party` was the writer's word. It now verifies only where the envelope's `maker` is `acts`, the key signs for
  `acts`, and `party` is the one the new `StatementKeys::publisher` answers. The rows comparing a statement's maker
  with a provider read `acts`; those comparing it with a party read `party`. 224 admission cases and 24 attribution
  cases. (DEC-4, DEC-20, REQ-AUD-2, REQ-ORD-32, T-020-01)
- Folded into 1.0.0 before its release: a shortlist may keep no provider. A shortlist round's decision record names
  its shortlisted providers each once, and none where it kept none, so a `no_route` input has the record to cite;
  the schema drops `minItems` and the Rust check keeps distinctness. (DEC-4, DEC-20, REQ-AUD-4, REQ-CPL-6,
  T-020-01)
- Folded into 1.0.0 before its release: a person question names the verifiers it is sent to. A shortlisted route's
  authority and its shortlist's decision record name the verifiers the route's tuples need, which receive the
  subject; a question on the route asks only its providers and those verifiers, and records and bounds them with
  the providers (`REQ-CPL-5`). `shortlisted-authority-cites-its-shortlist` holds the authority's verifiers to the
  record's, and a choice round's record names none. (DEC-4, DEC-20, REQ-CPL-2, REQ-CPL-5, REQ-CPL-6, T-020-01)
- Folded into 1.0.0 before its release: rows that mean the signer read the actor. `answer-follows-its-question`
  compares the question's verified `acts` with the maker the operator signs as (`StatementKeys::operator`), so a
  module the operator publishes, stating for the operator's party, asks no question of Core's. `Report.party` names
  the reporting provider, and the two report rows read `acts`, so a sibling under one publisher can't stand in for
  it and two adjacent providers of one publisher can report a pair (`REQ-ORD-9`). A stated entry's relay is another
  actor than its maker, so a sibling of its publisher may relay it. (DEC-4, DEC-20, REQ-ORD-9, REQ-ORD-32, REQ-AUD-2,
  T-020-01)
- Folded into 1.0.0 before its release: a grant names its keys, and an accepted release names its statement key. A
  `change_catalogue` action names the provider's advertising keys, at least one, each once, exactly when it is a
  `grant`, and the release's `statement_key` exactly when it is `accept_module`. The log's first trust in a
  provider is the advertising keys of its latest grant in force (`granted_advertising_keys`), a new grant replacing
  them, never a deployment setting the log doesn't hold; `attribute` takes the log's history and verifies a module
  the operator publishes by the statement key of a release of it the log accepted and didn't revoke
  (`release_statement_keys`). (DEC-4, DEC-20, REQ-AUD-2, REQ-AUD-9, REQ-AUZ-6, T-020-01)
- Folded into 1.0.0 before its release: release keys follow exploited and revoked, and changes count once done. A
  catalogue change counts only once the log holds a `done` result for its effect. A release's statement key drops
  for every statement once the release is declared exploited; once revoked, it keeps verifying statements about a
  transaction whose segment began or ran on that release, by its handed-over segment record, and drops for any
  other (`REQ-MDL-20`); `reinstate` restores it. `release_statement_keys` takes the statement's transaction.
  35 attribution cases. (DEC-4, DEC-20, REQ-MDL-20, REQ-AUD-2, REQ-AUD-9, T-020-01)
- Folded into 1.0.0 before its release: an acceptance names each part's yes, and a change of owner has its class. An
  acceptance given names `yeses`, one per part, each with the part, its origin reserve and the receipt of that reserve's
  yes, in place of one `reserve_yes`; `acceptance-rests-on-the-reserves-yes` holds every part to its own yes, reserve
  and transaction, and the new `first-move-rests-on-its-parts-yes` holds a part's first move to its part's yes, an
  automatic deposit's order moving on none (`REQ-ORD-32`); `chosen-route-rests-on-its-orders-acceptance` holds the yeses
  to one per part of the plan, in its order, and each part cites a yes receipt of its own. `destination_owner_changed`
  joins the order refusal classes, and the new `owner-change-rests-on-its-destination-check` rests it on the destination
  check that found the change (`REQ-ORD-24`). 114 valid fixtures, 112 invalid, 244 admission cases. (DEC-4, DEC-20,
  REQ-ORD-24, REQ-ORD-32, T-020-01)
- Folded into 1.0.0 before its release: an automatic deposit rests on its arrival, and moves follow the plan's
  providers. An acceptance on an automatic deposit is admitted only on an arrival of its order at its origin
  reserve (`automatic-deposit-rests-on-its-arrival`, `REQ-ORD-15`), and an order holds one acceptance basis
  (`order-holds-one-acceptance-basis`). `move-segment-is-in-its-orders-plan` holds a move to the provider its plan
  names for the segment; a part's first move is made by its origin reserve; and
  `chosen-route-rests-on-its-orders-acceptance` holds each part's yes to the reserve its first segment runs at.
  `Authorisation.origin_reserve` names the order's first part's reserve. 253 admission cases. (DEC-4, DEC-20,
  REQ-ORD-15, REQ-ORD-32, REQ-ORD-38, REQ-AUZ-9, T-020-01)
- Folded into 1.0.0 before its release: a read of an order's state and age. `Reads::OrderState { order }`, schema
  `kind: order_state`, reads an order's recorded state and age and nothing else (`REQ-ORD-5`). The trail and the
  order cover it; an operator's permitted read without a permission is of an order's state or its own evidence; a
  provider's permitted read is never of an order's state; `customer-read-is-of-its-own-order` cites a customer's
  `order_state` reads. 115 valid fixtures, 114 invalid, 259 admission cases. (DEC-4, DEC-20, REQ-ORD-5, REQ-AUZ-7,
  T-020-01)
- Folded into 1.0.0 before its release: a permission-free evidence read is of the reader's own statement. The new
  row `own-evidence-read-is-of-the-readers-statement` holds a permitted evidence read that names no permission, by
  any reader, to a stated entry of this log holding that commitment and made (`acts`) by the reader, so no party
  reads another's kept bytes without a permission whose scope covers them (`REQ-AUD-5`). A person logged in as the
  operator reads the operator's own statements, made as the operator, as the operator's party; the rows over
  verified entries read a `VerifiedCiting`, which carries the maker the operator signs as. 265 admission cases.
  (DEC-4, DEC-20, REQ-AUD-5, REQ-AUZ-7, T-020-01)
- Folded into 1.0.0 before its release: erasure is a decided action, and nothing cites an erased statement. A new
  `Action::EraseStatement { commitment, reason }` goes through `Decisions::decide`; the log admits its effect on
  the permit and its result like any decided action, and the `erasure` entry follows its effect's `done` result
  (`erasure-is-carried-out-on-its-decision`). Once an erasure is done, no new entry cites the statement, by its
  stated entry's receipt or its commitment (`nothing-cites-an-erased-statement`). A permission-free evidence read
  is also of a statement the log verified. 116 valid fixtures, 115 invalid, 276 admission cases. (DEC-4, DEC-20,
  REQ-AUD-13, REQ-AUZ-1, REQ-AUD-5, T-020-01)
- Folded into 1.0.0 before its release: an erasure is refused while anything rests on the statement. The new
  none-other row `erasure-waits-while-anything-rests-on-the-statement` refuses an `erasure` entry, and an
  `erase_statement` decision, while an earlier entry cites the statement, so the log holds no done erasure the
  evidence store must refuse ("refuse while cited", the owner's default). 279 admission cases. (DEC-4, DEC-20,
  REQ-AUD-13, T-020-01)
- Folded into 1.0.0 before its release: no recalculation goes around a move in flight, and a retired segment takes
  no move. The none-other row `recalculation-waits-for-moves-in-flight` refuses a `route_recalculated` while a
  `move_value` effect on a segment of that part has no final result, unless the segment settled, its reversal
  completed, or it is a deposit that failed, so a move that may still deliver is never routed around and the same
  value never moves twice. The none-other row `retired-segment-takes-no-move` refuses a `move_value` effect on a
  segment a recalculation of its part left out of the new plan, unless it settled first. 288 admission cases, 59
  relations. (DEC-4, DEC-20, REQ-ORD-2, REQ-ORD-22, INV-4, T-020-01, T-020-10)
- Folded into 1.0.0 before its release: a part waiting past its floor is a recorded state. The state machine's new
  `waiting_at_floor` input (state-machine 3.0.0) is admitted as Core's own input, and the row
  `waiting-at-floor-rests-on-the-customers-floor` holds it to a permitted `authorise` of its order that sets a floor,
  so no part waits at a floor the customer never set (`REQ-ORD-27`, `AC-12`). 292 admission cases, 60 relations.
  (DEC-4, DEC-20, REQ-ORD-27, T-020-01, T-020-10)
- Folded into 1.0.0 before its release: a statement counts as erased from its erasure's `done` result. The store
  erases on the `done` result of the `erase_statement` effect, so `nothing-cites-an-erased-statement` refuses a
  citation from that result on, not only once the `erasure` entry is recorded; before, a citation in that gap was
  admitted, and `erasure-waits-while-anything-rests-on-the-statement` then refused the `erasure` for good. And
  `erasure-is-carried-out-on-its-decision` requires the `erasure` entry after a `done` result to be `done` itself
  (its new field `outcome`), so the log holds no failed erasure of bytes already gone. 294 admission cases. (DEC-4, DEC-20, REQ-AUD-13, T-020-01, T-022-03)
- Folded into 1.0.0 before its release: a change of destination waits for moves in flight, and an erase effect
  waits while cited. A `destination_changed` restarts a route and retires segments as a recalculation does, so
  `recalculation-waits-for-moves-in-flight` and `retired-segment-takes-no-move` hold it too. Erasure begins with the
  `erase_statement` effect: `erasure-waits-while-anything-rests-on-the-statement` refuses the effect while anything
  cites the statement, as it did the decision and the record, and `nothing-cites-an-erased-statement` refuses a
  citation from the effect on, unless its result failed. Refused cases for each; 298 admission cases. (DEC-4, DEC-20,
  REQ-ORD-2, INV-4, REQ-AUD-13, T-020-01, T-020-10, T-022-03)
- Folded into 1.0.0 before its release, after review: a part's standing, read once. One fold of the log's inputs
  for a part, over the state machine's published rules, tells whether the part is in flight, held or ended, and
  the part rows read it. A `done` result no longer releases a move that never settled: only its segment's
  settlement, reversal or failed deposit, or a `failed` result, does (`recalculation-waits-for-moves-in-flight`),
  and a recalculation starts from the part's latest settled segment. New rows: `floor-wait-waits-for-moves-in-flight`,
  `segment-is-moved-once` (no second move on a segment, an identical retry aside; the retired row's settled
  exemption goes), `ended-part-takes-no-more` (after a hand-on, a cancellation, an end on request, completion or a
  resolution, no new route, wait, no-route or move), `held-part-takes-no-move`, and, from `T-020-11`,
  `only-a-declared-stuck-part-is-resolved`, for which a stuck decision now names its `part`. The world log's
  hand-on moves to its stuck part p3, so p1 goes on routing. `REQ-ORD-22` leaves the in-flight row's citation.
  311 admission cases, 65 relations. (DEC-4, DEC-20, INV-4, INV-8, REQ-ORD-2, REQ-ORD-27, REQ-ORD-28, REQ-AUZ-4,
  REQ-STM-6, T-020-01, T-020-10, T-020-11)
- Folded into 1.0.0 before its release: promoting a staged release is a catalogue change of its own, `promote`,
  about one module release, so it names the version it puts in force. Before it, no decided action carried the
  platform's promotion, so policy could not decide who may promote, a permission `REQ-AUZ-6`'s list of separate
  permissions takes in (`REQ-MDL-4`, ADR-0014). One valid fixture added: 110 valid. (DEC-4, DEC-20, REQ-AUZ-6, T-021-03)
