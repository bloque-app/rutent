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
  is held to the rules a value read from the wire is. Twenty-six schemas, 130 valid fixtures and 125 invalid ones, each
  breaking one rule, and 442 cases of the log's admission of an entry, from one coherent log, against a citation table
  of 82 relations derived from SPEC.md, each with the ids that demand it. An acceptance given cites its origin reserve's
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
  312 admission cases, 65 relations. (DEC-4, DEC-20, INV-4, INV-8, REQ-ORD-2, REQ-ORD-27, REQ-ORD-28, REQ-AUZ-4,
  REQ-STM-6, T-020-01, T-020-10, T-020-11)
- Folded into 1.0.0 before its release: a stuck part is resolved on its resolution's done result. The row
  `stuck-resolved-follows-a-done-resolution` admits the state machine's `stuck_resolved` for a part only after a
  `done` result of a `resolve_stuck` effect whose decision names that part, and only to where the decision sent the
  value: the order's authorised destination, or its origin reserve. 317 admission cases, 66 relations. (DEC-4,
  DEC-20, REQ-AUZ-4, REQ-ORD-28, T-020-01, T-020-11)
- Folded into 1.0.0 before its release: an authorisation per part (`T-021-04`'s review). Destinations change per
  part, so the authorisation's terms, which `Asked::Authorise` and the logged `authorise` entry carry, gain
  `parts`, the parts it covers, at least one, each once; absent, every part. A replacement replaces the earlier
  authorisation for the parts it names alone. `move-rests-on-its-authorisation` holds a move to an authorisation
  that covers its segment's part (`part`) and that no later authorisation of the order covering that part replaced
  (`in_force`), so a split order's other parts go on under theirs; `move-rests-on-its-orders-acceptance` holds a
  later move to an acceptance with a yes for its part. One valid fixture, 117; 322 admission cases. (DEC-4,
  DEC-20, REQ-AUZ-3, REQ-ORD-34, T-020-01, T-021-04)
- Folded into 1.0.0 before its release: an authorisation naming no part, or one part twice, is refused by the
  schema, by the Rust type when it deserialises (`Authorisation::check`), and by the log: `admit_entry` now holds
  every entry to its own shape's rules (`Entry::check`) before any row, so one built in memory, which no
  deserialisation checked, is refused as one read from the wire is. Two invalid fixtures, 117; a test builds the
  world's `authorise` entry with each in Rust and sees the log refuse it. (DEC-4, DEC-20, REQ-AUZ-3, T-020-01,
  T-021-04)
- Folded into 1.0.0 before its release: only the customer sets new terms (`T-021-04`'s review). Two engines may
  each read one authorisation in force and permit one each, the customer's new destination and an operator's
  restatement of the old, and "last permitted wins" brought the old destination back. The row
  `only-the-customer-sets-new-terms` refuses a permitted `authorise` whose terms differ from those in force for a part
  it covers unless the order's customer gave it (its requester, or one acting for it as its party); and the terms in
  force for a part are the first authorisation's covering it, then each later one the customer gave, so anyone
  else's restatement sets nothing (`move-rests-on-its-authorisation`'s `in_force`). 328 admission cases, 67
  relations. (DEC-4, DEC-20, REQ-AUZ-3, REQ-AUZ-9, INV-10, T-020-01, T-021-04)
- Folded into 1.0.0 before its release: the world log and every case's history are ones the state machine takes.
  A differential oracle in `CMP-STM`'s tests replays each admission case's history, and each admitted case's entry,
  through the reference state machine, and found an illegal input in the world log in every round so far: a segment
  settled or reversed that never started, a destination changed on a part neither waiting nor stuck, a hand-on of a
  part whose value was still outside the book, inputs sharing one record id. Repaired: each input has its own record
  id, providers are bound before segments settle or fail, p1's exchange fails before its reversal, p3's withdrawal
  settles before it is declared stuck, p1 waits past its floor before its new destination, which starts where its
  value sits, the failed delivery's result precedes the hand-on decision, and the hand-on's done result precedes
  `handed_on`. Cases rebuilt on legal histories; the probe recalculating a handed-on part goes, since the state
  machine refuses its hand-on. 338 admission cases. (DEC-4, DEC-20, INV-4, REQ-STM-3, T-020-01)
- Folded into 1.0.0 before its release: a part's standing is the state machine's own. `standing` dispatches each of
  the order's inputs to the transition rules the state-machine contract now publishes, as `CMP-STM` does, in place of
  its own copy of the segment and route transitions, so `Failed`, the guards on `destination_changed`, the segment
  edges and every other rule are the machine's. The row `input-is-one-the-state-machine-takes`, last in the table,
  refuses an input those rules don't take (`state_machine_takes`, public). A property test in `CMP-STM`'s tests holds
  the log's verdict to the reference state machine's over 600 seeded runs, both ways; switching off one rule in
  `standing` turns it red. Tension, named: `REQ-AUD-1`'s "every input leaves a trace" let a not-applicable input be
  recorded; the log now refuses it, and Core records only inputs the machine takes. 341 admission cases, 68 relations.
  (DEC-4, DEC-18, DEC-20, INV-4, REQ-STM-3, T-020-01)
- Folded into 1.0.0 before its release: one predicate for a part's value in motion (the round-3 review's findings 1, 2
  and 4). An effect is in motion while it may still move value: a `move_value` until its segment settles, is reversed
  or fails as a deposit, a `resolve_stuck` of either kind until its `stuck_resolved` or `handed_on`, either until a
  `failed` result. One row, `part-waits-for-value-in-motion`, reads it for every input and decision that would move a
  part's value from where it sits: a recalculation, a change of destination, a floor wait, a hand-on, a stuck
  decision and a resolution's effect; it replaces `recalculation-waits-for-moves-in-flight` and
  `floor-wait-waits-for-moves-in-flight`, and `ended-part-takes-no-more` keeps only moves, the state machine's row
  refusing inputs on an ended part. A stuck part's `handed_on` also follows a done `hand_on` resolution to its linked
  order (`stuck-resolved-follows-a-done-resolution`). Refused: a stuck part resolved again while its delivery is in
  motion, a resolution moved while another is, a route recalculated around a delivery in motion, a waiting part
  handed on with a move in motion, a stuck part handed on before its hand-on is done. 345 admission cases, 67
  relations. (DEC-4, DEC-20, INV-4, REQ-ORD-2, REQ-AUZ-4, T-020-01, T-020-11)
- Folded into 1.0.0 before its release: a stuck part's delivery goes to its ends, checked where it is decided and
  moved (the round-3 review's finding 5). The row `delivery-goes-to-the-parts-ends` holds a `deliver` decision and its
  `resolve_stuck` effect to the authorisation in force for the part, whole: its destination's reserve, node and
  account, or its origin, the in-force origin reserve at the node the part's first segment withdrew from; and
  `stuck_resolved` reads the same. Shape gap, named: no entry records the origin's account, so a delivery back to the
  origin isn't held to one. And without a plan, a declaration of stuck counts only for a part the order's acceptance
  names (low 7; an arrival names no part, a second shape gap). 353 admission cases, 68 relations. (DEC-4, DEC-20,
  REQ-AUZ-3, REQ-AUZ-4, INV-10, T-020-01, T-020-11)
- Folded into 1.0.0 before its release: a charge's answer says why it wasn't done (`T-021-05`'s review). A refused
  charge, or one policy couldn't decide, was logged as `over {bound: 0}`, so a scope refusal read as a rate limit and
  a charge whose scope register was out of reach as refused, against `REQ-AUZ-5`'s rule that not known is never a
  refusal. `ChargeOutcome` gains `refused {class}` and `not_known {cause}`, mapped as `PersonOutcome`'s are. A
  charge counted within its bound carries `counted_at`, its engine's own clock, beside `when.at`, so an engine
  started again rebuilds each party's count by window from the log; it is present exactly on a permitted charge.
  Four valid fixtures, two invalid. (DEC-4, DEC-20, REQ-AUZ-5, REQ-AUZ-8, T-020-01, T-021-05)
- Folded into 1.0.0 before its release: a repeat is the same input sent again. The state machine's guard reads a
  repeat by record id alone; the log now takes one only where the earlier input under that id is this same input,
  so another input under a used record id, which the host contract refuses, is refused here too. 355 admission
  cases. (DEC-4, DEC-20, REQ-STM-4, INV-8, T-020-01)
- Folded into 1.0.0 before its release: a stuck resolution's move whose outcome was unknown settles on two actors'
  word (`T-020-11`'s review). A new entry, `resolution_outcome {effect, outcome, reported_by, reported_with,
  confirmed_by, confirmed_with}`, records who reported the outcome and who confirmed it, each with its evidence.
  The row `unknown-resolution-settles-on-two-actors` admits a `resolve_stuck` effect's final `done` or `failed`
  result after an `unanswered` one only on such an entry with that outcome, its confirmer of another party and
  another login than its reporter; `effect-has-one-result` lets that one final result follow the `unanswered`
  one. The world's delivery is unknown, then failed on two actors' word. One valid fixture; 362 admission cases,
  69 relations. (DEC-4, DEC-20, REQ-AUZ-4, REQ-ORD-1, T-020-01, T-020-11)
- Folded into 1.0.0 before its release: a refiling is the deployment's operator's, and one deposit opens one order
  (`T-020-04`'s review). `refiling-files-an-arrival-anew` checked only the operator's role, so a customer stating
  it passed; it now asks for the deployment's operator, by the party the log knows it as, or the arrival's reserve.
  An `arrival` naming an order carries `notice`, the commitment of the reserve's notice of the deposit it is of
  (the statement it was first reported in), and `arrival-names-its-notice` holds it to a stated entry of the
  reserve's; `one-order-per-deposit` refuses a second arrival naming an order on the same notice, a second order
  or a second filing with other figures, the same arrival sent again aside. 371 admission cases, 71 relations.
  (DEC-4, DEC-20, REQ-ORD-14, REQ-ORD-23, INV-8, T-020-01, T-020-04)
- Folded into 1.0.0 before its release: one record id names one content in the whole log (the round-4 review's
  medium). The state machine's guard reads only the last record id; the log now refuses an input under any record id
  an earlier input holds, unless it is that input again, as the host's exactly-once (`RecordIdReused`) does. Both
  oracles hold the host's rule too. 372 admission cases. (DEC-4, DEC-20, REQ-STM-4, INV-8, T-020-01)
- Folded into 1.0.0 before its release: a refused input leaves a trace (`REQ-AUD-1`, the round-4 review). A new
  observed entry, `input_not_taken {input, reason}`, records an input the state machine answered not applicable or
  the host refused for its record id; `standing` reads only inputs, so no order moves on it. The row
  `input-not-taken-is-one-refused` holds the trace to the truth: an input refused after this log, for the reason it
  names. One valid fixture; 375 admission cases, 72 relations. (DEC-4, DEC-20, REQ-AUD-1, REQ-STM-3, T-020-01)
- Folded into 1.0.0 before its release, after round 4's review: a move's unknown outcome settles on policy's
  confirmation. `resolution_outcome`, on the general handle with free actors and opaque evidence, gives way to a
  decision kind on policy's handle, `confirm_outcome {request: OutcomeConfirmation, decision}`, which
  `StuckOrders::confirm_outcome` asks: its report and its confirmation each cite a verified stated entry, by receipt
  (`outcome-confirmation-cites-verified-evidence`); it is of a `move_value` or `resolve_stuck` effect whose result
  is `unanswered`, a resolution's report its `stuck_move`'s mover's, its confirmer of another party and login
  (`outcome-confirmation-rests-on-its-move`); and a final result after the `unanswered` one settles only on it, with
  its outcome (`unknown-outcome-settles-on-its-confirmation`). A permitted confirmation releases the move from
  motion, so AC-6's part, stuck on a silent move, is resolved once the move's outcome is confirmed; no later result
  of the move is admitted but through it. Policy's side is `T-021-06`'s; `T-020-11` calls it. 385 admission cases,
  74 relations. (DEC-4, DEC-20, REQ-AUZ-4, REQ-ORD-1, AC-6, T-020-01, T-020-11, T-021-06)
- Folded into 1.0.0 before its release: the origin account and an arrival's part (round 4). `Authorisation` names
  `origin_account`, and `delivery-goes-to-the-parts-ends` sends a delivery back to the origin only to it (intake
  fills it, `T-020-03`). An `arrival` names its `part`, and `split-deposit-arrival-names-its-part` holds a split route
  on an automatic deposit to an arrival naming each part (the arrival's matcher fills it, `T-020-04`). The README
  records that a bound's window reads the log's append clock or a per-engine forward-only `counted_at` (`T-021-05`,
  `T-022-01`). 388 admission cases, 75 relations. (DEC-4, DEC-20, REQ-AUZ-4, REQ-ORD-15, REQ-ORD-42, T-020-01,
  T-020-03, T-020-04)
- Merged with `T-020-09`'s closing (m3 at 31c3772): its `closing` rows, `replay` entry and fixtures stand beside
  these. Its final-deposit cases settled p1-s3 on a history where p1's earlier segments never started, which the
  state machine refuses (`input-is-one-the-state-machine-takes`); their histories now bind and settle p1-s1 and
  p1-s2 and bind p1-s3 first. `T-020-09`'s fixture generator needs the same steps. 129 valid fixtures, 125 invalid,
  403 admission cases, 78 relations with the closing table's three. (DEC-4, DEC-20, T-020-01, T-020-09)
- Folded into 1.0.0 before its release: a move's unknown outcome settles on two actors' word, each their own
  (round 5 of `T-020-01`'s review). The report was a field of the confirmer's request, so one actor wrote both words,
  and for a `move_value` any reporter passed; its evidence could be about anything, and one receipt served twice. The
  report is now the mover's own decision on policy's handle, `report_outcome {request: OutcomeReport, decision}`, asked
  through `StuckOrders::report_outcome`; `confirm_outcome`'s `OutcomeConfirmation` cites it by receipt (`report`) in
  place of `reported_by` and `reported_with`. `outcome-report-rests-on-its-move` holds the report to an unanswered move
  and its reporter to the move's own mover: a `move_value`'s decided-for actor or one of that actor's accountable
  party, a `resolve_stuck`'s `stuck_move` mover. `outcome-confirmation-rests-on-its-report` holds the confirmation to a
  permitted report of the same effect, order and outcome, its confirmer to another party and login than the
  reporter, and its evidence to another statement than the report's, by another maker.
  `outcome-confirmation-cites-verified-evidence` holds each evidence to a verified statement about the move's order
  and part, through the transaction it names. `one-confirmation-per-effect` refuses a second confirmation of an
  effect, an identical retry aside. The world's delivery is reported failed by its mover and confirmed by the
  operator on two statements about `p3`, by two makers; AC-6's silent move is reported by Core, its decide step's
  actor. 130 valid fixtures, 420 admission cases, 81 relations. (DEC-4, DEC-20, REQ-AUZ-4, ADR-0011, T-020-01)
- Folded into 1.0.0 before its release: one deposit opens one order, keyed by its signed transaction (round 5 of
  `T-020-01`'s review). `one-order-per-deposit` keyed an arrival by its notice's commitment, which is salted, so the
  reserve's same notice kept twice had two commitments and opened two orders. It now keys each arrival by the
  transaction its notice's stated entry names (`about.transaction`), and `arrival-names-its-notice` requires the
  notice to name one (field `transaction`). Refused: a second order on the same notice kept again under another
  salt, and a notice naming no transaction. 422 admission cases. (DEC-4, DEC-20, REQ-ORD-23, INV-8, T-020-01)
- Folded into 1.0.0 before its release: a refused input's record id is used (round 5 of `T-020-01`'s review). The
  log's record-id check read only `input` entries, so an input under the record id of one kept as `input_not_taken`
  was admitted, which the host's exactly-once refuses. `state_machine_takes` and the not-taken verdict now read
  `input_not_taken` inputs too: another input under a refused input's record id is refused, and a trace of one is
  admitted only naming `record_id_reused`; the same input as a refused one is judged again. `CMP-STM`'s fixture
  oracle records a trace's record id as the host does. 425 admission cases. (DEC-4, DEC-20, REQ-AUD-1, REQ-STM-3,
  T-020-01)
- Folded into 1.0.0 before its release: an arrival's notice is a verified statement (round 6 of `T-020-01`'s
  review). `arrival-names-its-notice` and `one-order-per-deposit`'s key read the notice's stated entry whatever its
  signature, so the reserve's notice appended again unverified, claiming another transaction, keyed a second order
  apart. The notice must now be verified (field `verified`), and the key reads only a verified one. 426 admission
  cases. (DEC-4, DEC-20, REQ-ORD-23, REQ-AUD-2, INV-8, T-020-01)
- Folded into 1.0.0 before its release: an outcome's evidence is newer than its move (round 6 of `T-020-01`'s
  review). `outcome-confirmation-cites-verified-evidence` admitted a statement made before the move, which can't say
  what the move came to, and a `move_value`'s evidence about any segment of its part. Each evidence now follows the
  move's effect in the log (field `after`), and a `move_value`'s is about the moved segment itself, through a
  transaction first bound to it. Refused: the probe, a report of `p3`'s delivery citing the statement about `p3-s1`
  at position 60, before the effect at 69; a `move_value`'s evidence about another segment of its part. 429 admission
  cases. (DEC-4, DEC-20, REQ-AUZ-4, REQ-AUD-2, T-020-01)
- Folded into 1.0.0 before its release: a move's outcome is reported once, while it is unsettled (round 6 of
  `T-020-01`'s review). `outcome-report-rests-on-its-move` admitted a report after the move's confirmation or final
  result, and any number of reports. A report is now admitted only while the move has neither (field `open`), and
  `one-report-per-effect` refuses a second permitted report of an effect, an identical retry aside, so the
  confirmer confirms the one report there is. 433 admission cases, 82 relations. (DEC-4, DEC-20, REQ-AUZ-4, ADR-0011,
  T-020-01)
- Folded into 1.0.0 before its release: a refused input's refusal is handed back (round 6 of `T-020-01`'s review).
  The same input sent again under a refused input's record id was judged again by the state machine, so once its
  order had moved on the log took it, where the host hands back the refusal it recorded. Under a refused input's
  record id, `state_machine_takes` now takes nothing, the same input included, and the not-taken verdict for the same
  input is the recorded refusal, so only an identical `input_not_taken` follows it. Refused: the reviewer's sequence,
  a destination change refused, then the order waiting, then the same input under the same id; the same input's
  trace naming another reason. Admitted: the same trace again, also after its order has moved on. `CMP-STM`'s
  fixture oracle refuses a refused input taken again. 437 admission cases. (DEC-4, DEC-20, REQ-AUD-1, REQ-STM-3,
  T-020-01)
- Folded into 1.0.0 before its release: a planless stuck part's outcome evidence binds through how its order came
  in (round 6 of `T-020-01`'s review, the follow-up row added while the contract is unreleased). A stuck part with no
  plan has no segment for its evidence to name, so `outcome-confirmation-cites-verified-evidence` could admit none,
  and its silent resolution would deadlock. Its evidence now names the order and no segment, through a transaction
  first bound to the order that is the deposit of an arrival naming the order or the one the part's acceptance yes
  names. Admitted: `ord-21`'s planless `p3`, through its acceptance yes's transaction and through its arrival's;
  refused: through another transaction of the order. 440 admission cases. (DEC-4, DEC-20, REQ-AUZ-4, REQ-AUD-2,
  T-020-01)
- Folded into 1.0.0 before its release: the README records that for a silent `move_value` the two actors may be
  Core, the decide actor's party, and one person, and hands `T-021-06` the check the log can't make: policy's answer
  to `confirm_outcome` refuses where the confirmer's and the reporter's `ActsFor` chains reach a common accountable
  party. (DEC-4, REQ-AUZ-4, ADR-0011, T-020-01, T-021-06)
- Folded into 1.0.0 before its release: `admit_entry` reads the deployment's operator by its maker and by its party
  (int-arrivals' report on `T-020-01`). It took one `operator` id, the maker the operator signs its statements as
  (`StatementKeys::operator`), and `refiling-files-an-arrival-anew` compared a refiler's party with it, so where the
  maker and the party differ (a deployment signing as `conformance-operator`, its people acting for `operator`) the
  operator's own login was refused, and a party named as the maker passed. `admit_entry` now takes a
  `DeploymentOperator {maker, party}`: the stated rows and the operator's own statements read the maker, the refiling's
  filer the party, which the log knows as the one `StatementKeys::publisher` answers for the operator's maker. The
  world's operator is maker and party `operator`; a case may name another pair. Admitted: the operator's login under
  a maker of another name; refused: a party named as the maker. 442 admission cases. (DEC-4, DEC-20, REQ-ORD-23,
  T-020-01)
- Folded into 1.0.0 before its release: promoting a staged release is a catalogue change of its own, `promote`,
  about one module release, so it names the version it puts in force. Before it, no decided action carried the
  platform's promotion, so policy could not decide who may promote, a permission `REQ-AUZ-6`'s list of separate
  permissions takes in (`REQ-MDL-4`, ADR-0014). One valid fixture added: 110 valid. (DEC-4, DEC-20, REQ-AUZ-6, T-021-03)
- Folded into 1.0.0 before its release: a home for a private node's replay. `What::Replay { order, segment, nodes,
  found }` records where the value left one or more private nodes onto a public node, the nodes replayed in route
  order, and what `CMP-EVIDENCE` found: `consistent_with_the_exit` (consistency, never proof, `THR-131`), each
  `mismatch` with the node it is against, or a node that recorded `nothing_recorded`. Before it, `T-020-09` had no
  record for `REQ-ORD-41`'s replay. It cites nothing earlier and is admitted on its own checks. Three valid
  fixtures and four invalid: 119 valid, 119 invalid. (DEC-4, DEC-20, DEC-24, REQ-ORD-41, T-020-09)
- Folded into 1.0.0 before its release: closing rests on what the log can check (review of `T-020-09`). `evidence`
  names the public movement it relied on (`movement`: network, id, and `node`, the private node it moved value into or
  out of where it evidences that node's segments), only as a public record; `replay` rests on its exit's result (`rests_on`) and names the movement
  out, and `nothing_recorded` names every silent node; `fee.billed_separately` is what the provider stated, absent
  where it said nothing. Three rows, kept in their own module (`relations/closing.rs`):
  `final-deposit-closes-on-a-public-record` (a part's final deposit settles only on grade-1 evidence naming a movement
  on its authorised destination's network, resting on its reserve's verified statement, never a statement alone),
  `movement-evidences-one-segment` (no second segment, of any order, relies on the same movement) and
  `replay-rests-on-its-exit`. 121 valid fixtures, 122 invalid, 293 admission cases. (DEC-4, DEC-20, DEC-24,
  REQ-ORD-33, REQ-ORD-38, REQ-ORD-41, REQ-AUD-11, T-020-09)
- Folded into 1.0.0 before its release: what Core took, the log takes (re-review of `T-020-09`). `evidence` names the
  movement Core checked at whatever grade it earned, so `movement-evidences-one-segment` holds every movement Core
  relied on, and a node's movements in and out are reused only within its one order; a final deposit into a private
  rail names that rail, the authorised destination's network, as the movement's `node`
  (`final-deposit-closes-on-a-public-record--conflict-naming-another-node`). One invalid fixture is valid now
  (`verified-entry-of-signed-evidence-naming-the-movement-core-checked`). (DEC-4, DEC-20, REQ-ORD-33, REQ-ORD-38,
  T-020-09)
