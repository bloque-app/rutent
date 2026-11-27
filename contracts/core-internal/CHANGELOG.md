# core-internal changelog

## 1.0.0
- The contract round for `M3`'s milestone review of 2026-11-24 (`T-020-45`), folded into this unreleased version
  while `develop` holds none of it:
  - `T-020-48` (finding 4): `Difference::PairNotBuilt {order, segment, why}` (`pair_not_built`; `why`: `PairNotBuilt`,
    `no_sent_report {segment}`, `no_flight`, `not_an_amount`, `refused`), a segment's pair of reports that was never built, so
    the comparison was never made (`REQ-ORD-9`, `ADR-0009`); `Comparisons::pair_not_built`. The one raise that may cite no
    receipt, since a pair a restart dropped has none: `Raise::check` and the schema (`cites` needs one item for any other
    kind), with row `raise-of-a-pair-not-built-names-a-segment-its-plan-lists` instead. Fixtures: valid
    `raise-of-a-pair-a-restart-dropped`, `raise-of-a-pair-the-contract-refused`; invalid `raise-of-a-pair-not-built-without-the-segment-it-misses`,
    `--citing-one-receipt-twice`, `--for-a-reason-it-has-no-name-for`; admission `--its-plan-lists`, `--missing`. 118 relations.
  - `T-020-48` (the review of its first build, S2): `Difference::HoldingNotRead {account, node, provider, why}`
    (`holding_not_read`; `why`: `HoldingNotRead`, `not_answered`, `refused`, `another_answer`, `not_a_balance`), a holding
    account whose module didn't give its balance, raised to the operator once in a spell of failure, with no balance
    (`REQ-AUD-15`, `REQ-ORD-7`, `REQ-AUD-6`); `Comparisons::holding_not_read`. It may cite no receipt (an account never
    read has no check), as `pair_not_built` may (`Raise::check` and the schema's `cites` conditional), and cites only its
    account's own last check where it has one: row `raise-of-a-holding-not-read-cites-its-accounts-check` (fields
    `account`, `node`, `provider`). Fixtures: valid `raise-of-a-holding-account-not-read`, `--citing-its-last-check`;
    invalid `raise-of-a-holding-not-read-naming-a-balance`, `--for-a-reason-it-has-no-name-for`; admission
    `--its-accounts-check`, `--never-read`, `--missing`, `--account`, `--node`, `--provider`. 120 relations, with
    `T-020-52`'s merged beside it.
  - `T-020-49` (5): `CatalogueChange::ConfirmReleases` (`confirm_releases`, `CONFIRM_RELEASES`), the platform's
    confirmation of a provider's releases after an unclean end (`T-013-11`), a sixth lifecycle action, with
    `Action::ChangeCatalogue.versions`, the provider's versions it puts back in force, at least one, each once, named
    exactly by it (`CatalogueChange::confirms_releases`). And `OperatorSetting::Cadence.provider` (optional), a
    provider's own status cadence, named only for the status question's job (`STATUS_ASKS_JOB`), the change's provider
    then that provider; a cadence's `seconds` is a safe integer, zero running its job on every tick, as a job's
    default cadence does. Fixtures: valid `decision-request-to-confirm-a-providers-releases`,
    `decision-request-to-set-a-providers-status-cadence`; invalid `decision-request-to-confirm-releases-naming-no-versions`,
    `--naming-a-version-twice`, `decision-request-to-promote-naming-versions-to-confirm`,
    `decision-request-to-set-a-providers-cadence-of-another-job`. 120 relations, unchanged.
  - `F-025`'s L7: row `automatic-deposit-takes-a-firm-quote`, an `automatic_deposit` acceptance's quote is no `shown`
    without a firm rate or awaiting its customer.
  - R-AWAITING: `arrival.holder` (optional), and row `waiting-quote-is-for-its-arrivals-holder`, a `shown` with
    `awaiting` waits for the holder of its order's arrival. Its slow places stay orchestration's: the log holds the
    quote by digest alone (a gap in `F-020`'s notes).
  - `F-025`'s L5: `revoked_before_receipt` counts a revocation from its admitted `revoke_grant` effect until a `failed`
    result undoes it.
  - Finding 8: `wait_ended {order, why: WaitEnd}`, rows `wait-ends-a-wait` and `ended-wait-takes-no-acceptance`, and
    `one-order-per-deposit` freeing a deposit whose order's wait ended unaccepted (`GUA-1`); `NoticeOf::WaitEnded`, so a
    restart sends the notice once (`T-023-04`).
  - Finding 4 and `M2`'s item 6: `CatalogueChange::SetSetting` (`set_setting`) with `Action::ChangeCatalogue.setting`
    (`OperatorSetting`, `SettingKind`), a setting about a provider naming that provider as the change's `provider`
    (the review of `1bb5f4cb`; `decision-request-to-set-a-providers-ceiling-naming-another-provider`); the five
    lifecycle actions were already changes of their own (`LIFECYCLE_ACTIONS`, `is_a_lifecycle_action`).
  - `T-020-50` (finding 5): `arrival.destination` (optional, provider-role's `DepositDestination`), the destination the
    reserve's notice names an automatic deposit to go to with who set it for whom, and three fields of
    `automatic-deposit-rests-on-its-arrival`: the arrival's holder is the acceptance's requester, a destination it names
    is the one (and to asset) of the authorisation the acceptance names, and it was set for that requester.
  - `T-020-51` (finding 8): `OperatorSetting::DepositWait {seconds}` (`deposit_wait`, `SettingKind::DepositWait`), a ninth
    operator setting, the lifetime of an order's wait for its customer on an automatic deposit. One valid fixture
    (`decision-request-to-set-the-deposit-wait`) and one invalid (`--of-no-seconds`).
  - `T-020-51` (finding 8, gap 1): `arrival.request` and `arrival.inputs` (both optional), the request identifier of the
    filing that holds a deposit against its order and the salted commitment of the operator's statement of the filing's
    inputs and objective, a new shape `filing-inputs` (`FilingInputs`, `FILING_INPUTS_SHAPE`; a sibling of `order-inputs`,
    whose inputs are never empty and which states no objective), kept in the evidence store so a Core that reopens reads
    a waiting filing back and the log holds no account (`REQ-AUD-6`, `NFR-6`); row
    `filed-orders-inputs-are-a-kept-statement`, its stated entry verified, of that shape, the deployment operator's own
    and about nothing. Fixtures: valid `filing-inputs-of-a-filed-order`,
    `verified-entry-of-an-arrival-holding-a-deposit-with-its-filings-inputs`; invalid `filing-inputs-without-its-objective`;
    admission `--the-operators-statement` (admitted), `--missing`, `--verified`, `--inputs`, `--maker`, `--about`.
  - `Reads::FiledOrder {order, requester}`, a read of an order whose parties the log doesn't hold yet (`T-021-16`).
  - R-MOVER (`T-021-16`, `F-025`'s second pass): `stuck_decide.held_at` (optional), the inner node whose holding account
    the deployment's register says the decision's mover holds, which policy records where nothing else the log holds
    ties that mover to the order (a hand-on through a public node's wallet, `T-020-33`); row
    `stuck-decide-held-at-is-a-held-node`, the inner node the stuck part's value rests at (`resting_node`, public, the
    state machine's `value_at` of the part as the log leaves it), named only for such a mover;
    and `party_to_the_order` takes a permitted decide step's mover, or a resolution's, as a party only where it is a
    holder the order involves by the log's other entries or its decide step records `held_at`. Five admission cases
    (four of the new row, `--at` on a world with two inner nodes on the order's route; one confirmation on a mover held
    at a node); two earlier ones on an untied mover are now refused on `party`.
  - `T-020-55` (the milestone reviewer's Low and Info on `T-021-16`): `stuck-decide-held-at-is-a-held-node` takes a
    route's segment starting at `held_at` as well as one ending there, so a value resting at its route's first node,
    where `resting_node` places it, is held there and not refused an honest permit; admitted case
    `--a-wallet-held-at-its-routes-first-node`. `holder_of_the_order`, public, is the one reading of a holder the order
    involves, which the row, the party rule, policy and orchestration's port each call; it counts only entries held as
    `observed`, as every kind it reads is.
  - `order_opened`, an order as intake opened it, the one piece of Core's state the log held no summary of
    (`T-020-46`).
  - `order_opened.inputs` (`T-020-46`): the salted commitment of the operator's statement of an order's inputs, a new
    shape `order-inputs` (`OrderInputs`), kept in the evidence store, so a Core that reopens reads them back and the
    log holds no account (`REQ-AUD-6`, `NFR-6`); row `opened-orders-inputs-are-a-kept-statement`, its stated entry
    verified, of that shape, the deployment operator's own and about nothing.
  - `reliance.inputs` (`T-020-46`, optional): the order's inputs, by name, that rest on the judgement relied on, so a
    Core that reopens rebuilds which input rests on which judgement, and a segment after the reopening that carries
    one still writes its reliance (`REQ-CPL-12`, `NFR-6`). Valid fixture
    `verified-entry-of-reliance-naming-the-inputs-resting-on-it`.
  - The review's Lows: the README's citation table names every admission fixture that exists and none that doesn't; a
    fixture for the rule that a verifier's `class` is named only by a grant or its revocation
    (`decision-request-to-set-slippage-naming-a-verifiers-class`); each Rust-only invalid fixture is read with its rule
    switched off (`without_rule`, feature `rule-switches`) and must then be accepted, which found two fixtures breaking
    a second rule (`effect-granting-without-a-tuple`, now naming its advertising keys) and four named by a prefix of
    their rule (the raises', now named whole); the log's handling of provider-role's statements, which provider-role's
    README no longer says, is here.

  - `T-020-52` (the lead's authorisation, a `DEC-20` path folded into this unreleased version): `OrderRefusalClass::MissingInput`
    (`missing_input`), the class an order is refused with where policy's register of sessions can't be read, as policy
    answers it; and `Action::RefileArrival {arrival, order, filer}` (`refile_arrival`), a decided action that takes no
    effect, with `refiling.decision`, the receipt of policy's permitted `decide` of it, which the new row
    `refiling-rests-on-policys-permit` holds to the arrival, order and filer the refiling names, permitted, asked by the
    refiling's own actor. One valid fixture each (`decision-request-to-refile-an-arrival`,
    `verified-entry-of-a-refusal-for-an-input-policy-could-not-read`), an invalid one each
    (`decision-request-to-refile-an-arrival-without-its-filer`, `verified-entry-of-a-refiling-without-its-decision`),
    six admission cases, and every refiling in the admission cases now rests on a decision of its own. (REQ-ORD-23,
    REQ-AUZ-1, REQ-AUZ-5, T-020-52)
  - `T-020-57` (the lead's authorisation, a `DEC-20` path folded into this unreleased version): the reserve at a
    handed-on route's from-node. `reversal-rests-on-its-evidence`'s field `reserve` read the reserve at a segment's
    from-node as the provider of a plan's segment that delivered there, else the segment's own provider; a route handed
    on has no segment delivering to its first node, so for its first move the log read the failed provider itself as
    the reserve, refused the holding account's confirmation, and would have admitted the failed provider's own word,
    graded a statement, as completing its own reversal (`REQ-ORD-3`, `THR-13`). Now, for an order whose `route_chosen`
    names `handed_on_from`, the reserve at a from-node no segment delivered to is the `holder` of the order's verified
    `holding_withdrawal` at that node (`REQ-ORD-16`, `REQ-ORD-40`, `REQ-ORD-43`), and with no such record there is
    none. No shape changes. Three admission cases: `--a-handed-on-routes-holding-account-confirms-its-first-move`
    (admitted), `--a-handed-on-routes-failed-providers-own-word` (`whose`), `--a-handed-on-route-with-no-holding-record`
    (`reserve`). (REQ-ORD-3, THR-13, REQ-ORD-43, T-020-57)
  - `T-020-57`, round 3 (the lead's authorisation, a `DEC-20` path folded into this unreleased version): a route handed
    on whose first move was reversed can be recalculated. `recalculation-starts-from-evidenced-value` took only the
    part's latest `segment_settled`, and such a route has no settled segment before its first move, so its part could
    only be declared stuck. Now the `reversal_completed` of a route handed on's first move (one starting at an inner
    node no segment of the order's plans delivered to, no withdrawal), which this log took only on the holding
    account's word (`reserve_at_from`), evidences the value back at that move's `from`, and the recalculation starts
    there; a reversal still owed or refused evidences nothing. No shape changes. Three admission cases:
    `--a-handed-on-routes-reversed-first-move` (admitted), `--a-handed-on-routes-first-move-still-owed` (`missing`),
    `--a-handed-on-routes-reversal-from-another-node` (`node`). (REQ-ORD-2, INV-4, REQ-ORD-43, T-020-57)

  Valid fixtures for each new shape and setting, an invalid one per rule broken, and admission cases for each new
  row and field. With `T-020-44`'s, `T-020-50`'s, `T-020-46`'s (five rounds), `T-021-16`'s, `T-020-51`'s, `T-020-52`'s,
  `T-020-48`'s, `T-020-57`'s and `T-020-49`'s rounds, merged beside it: thirty schemas, 193 valid fixtures and 195 invalid,
  789 admission cases and 58 attribution cases (counted from the files); 120
  relations and 5 closing ones, three stated. (DEC-4, DEC-20, REQ-QTE-10, REQ-AUZ-7, REQ-AUZ-6, REQ-AUZ-8, THR-3, GUA-1, REQ-ORD-20, T-020-45)
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
  is held to the rules a value read from the wire is. Twenty-six schemas, 133 valid fixtures and 126 invalid ones, each
  breaking one rule, and 580 cases of the log's admission of an entry, from one coherent log, against a citation table
  of 103 relations derived from SPEC.md, each with the ids that demand it. An acceptance given cites its origin reserve's
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
- Folded into 1.0.0 before its release: an outcome's evidence is a provider's result, made after the move (round 7
  of `T-020-01`'s review). `outcome-confirmation-cites-verified-evidence` admitted Core's own `operation` question,
  re-appended after the move, as a report's evidence, and read "after the move" by log position alone. Each evidence
  is now a `result`-shaped statement (field `result`), not the deployment operator's own (field `maker`, against
  `DeploymentOperator.maker`), and made no earlier than the effect's time (field `made`, its `made_at` against the
  effect's `when.at`). Refused: Core's question re-appended, the operator's own result, a result made before the
  effect and appended after it. The world's evidence statements are made after the moves they report. 445 admission
  cases. (DEC-4, DEC-20, REQ-AUZ-4, REQ-AUD-2, T-020-01)
- Folded into 1.0.0 before its release: a stuck resolution names its own transaction (round 7 of `T-020-01`'s
  review). Its outcome's evidence could be about any segment of its part, so `p3`'s delivery rested on a statement
  about `p3-s1`'s withdrawal from before the part was stuck. `Action::ResolveStuck` gains `transaction`, the one the
  move goes under at its provider (schema and Rust), and its evidence binds through it as a `move_value`'s binds
  through its segment: a result about the order and that transaction, no segment, the transaction first bound to
  the order and no segment. A planless part may still bind through its arrival or acceptance yes. The world's
  delivery goes under `tx-d3` and its hand-on under `tx-h3`, and its report and confirmation rest on results about
  `tx-d3`. Refused: the delivery reported on a result about `p3-s1`. Admitted: a planless part through its
  resolution's transaction. 447 admission cases. (DEC-4, DEC-20, REQ-AUZ-4, ADR-0011, T-020-01, T-020-11)
- Folded into 1.0.0 before its release: a planless part's evidence comes through its own arrival (round 7 of
  `T-020-01`'s review). Any arrival of the order bound a planless part's evidence, another part's included. An
  arrival naming a part now counts for that part alone, and one naming none only where the log names no other part
  of the order (in an input, an acceptance's yes, an arrival or an authorisation's parts). Refused: an arrival naming
  `p9`, and an arrival naming no part of an order whose acceptance names `p4` beside `p3`. Admitted: an arrival
  naming `p3`. 450 admission cases. (DEC-4, DEC-20, REQ-AUZ-4, REQ-ORD-42, T-020-01)
- Folded into 1.0.0 before its release: an operator command is the deployment operator's by party (round 7 of
  `T-020-01`'s review). `VerifiedEntry::check` held an operator command to its actor's role alone, so any actor
  stating the operator's role could appoint a checker. The check moves into a row,
  `operator-command-is-the-deployment-operators`, which reads the actor's party against `DeploymentOperator.party`,
  as a refiling's filer is read; the shape-level rule and its invalid fixture
  (`verified-entry-of-a-command-by-a-provider`) give way to admission cases. The world appoints a checker. Refused:
  another operator party, a provider, a party named as the operator's maker. 124 invalid fixtures, 454 admission
  cases, 83 relations. (DEC-4, DEC-20, REQ-AUD-10, T-020-01)
- Folded into 1.0.0 before its release: a stuck resolution's transaction is decided (round 8 of `T-020-01`'s
  review). The `resolve_stuck` effect named its transaction on its own, so the move step chose it. `StuckDecide`
  gains `transaction` (schema and Rust), the one the resolution goes under at its provider, and
  `stuck-effect-moves-the-decided-resolution` holds the effect to it (field `transaction`). The world's delivery is
  decided under `tx-d3` and its hand-on under `tx-h3`. Refused: a hand-on effect under another transaction than its
  decide step's. 455 admission cases. (DEC-4, DEC-20, REQ-AUZ-4, ADR-0011, T-020-01, T-020-11)
- Folded into 1.0.0 before its release: a stuck resolution's transaction is its own (round 8 of `T-020-01`'s
  review). Nothing checked it, so the world's hand-on was admitted under the delivery's `tx-d3`, `p3`'s yes
  `tx-yes-1c` and `tx-f1`, which `p1-s1`'s move binds, and its outcome's evidence could then rest on those entries'
  statements. `stuck-resolution-transaction-is-its-own` refuses a decide step or effect under a transaction another
  stuck resolution, a move, a yes or a deposit's notice uses, its own decide step and effect and retries aside;
  `stuck-resolution-transaction-is-unbound-or-its-orders` refuses one first bound elsewhere than at order level to
  its own order. Refused: the three probe transactions, a deposit's notice's, a decide step under the delivery's,
  and one bound to another order. 461 admission cases, 85 relations. (DEC-4, DEC-20, REQ-AUZ-4, REQ-ORD-38, T-020-01)
- Folded into 1.0.0 before its release: every stuck resolution's evidence binds through its own transaction (round 8
  of `T-020-01`'s review, the reviewer's ruling). A planless part's evidence could also bind through its acceptance
  yes's or an arrival's transaction, a route round 6 opened for a part with no segment to name. Every resolution now
  names its transaction, decided, so that route goes: the planless cases through the order's entry are refused, and
  the one through the resolution's own transaction is admitted. (DEC-4, DEC-20, REQ-AUZ-4, T-020-01)
- Folded into 1.0.0 before its release: the README hands `T-022-01` the check on a statement's own time: its log
  refuses a stated entry whose `made_at` runs ahead of its append clock beyond a tolerance, since an outcome's
  evidence is held to no earlier than its move by that `made_at`. (DEC-4, REQ-AUD-2, T-020-01, T-022-01)
- Folded into 1.0.0 before its release: a stuck resolution doesn't take over another question's transaction (round 9
  of `T-020-01`'s review). `stuck-resolution-transaction-is-its-own` counted only statements binding the transaction
  to a segment, so a resolution could be decided under the transaction of an order-level question already asked and
  answered, whose answers would then settle it. Any statement naming the transaction before the resolution's own
  decision is now a use; after it, only Core's question and the answers to it are its own. Refused: the reviewer's
  probe, a question and its answer under `tx-z` before `p3`'s hand-on decision under `tx-z`. Admitted: the hand-on's
  own question and answer under `tx-h3`, after its decision. 463 admission cases. (DEC-4, DEC-20, REQ-AUZ-4,
  REQ-ORD-38, T-020-01)
- Folded into 1.0.0 before its release: the customer is in the customer's role, and the origin account is the
  payer's to move (the reviews since `T-020-01` landed). `by_the_customer` read only the actor's party, so one of the
  requester's party in another role set new terms; it now also requires the customer's role, as the engine and Core
  read it. A part's origin account was moved by any authorisation the customer gave, so a requester could redirect
  a payer's refund; `only-the-payer-moves-the-origin-account` holds each later authorisation to the account in force
  unless the order's payer gave it, and a delivery back to the origin goes to that account. Which accounts the payer
  holds is policy's register, handed to `T-021-04` and `T-021-06`. Refused: the requester's party in the operator's
  role setting a new destination; the requester, and the operator, redirecting the payer's refund. Admitted: the
  payer moving it, and a restatement before and after. 469 admission cases, 86 relations. (DEC-4, DEC-20,
  REQ-AUZ-3, REQ-AUZ-4, REQ-ORD-17, T-020-01)
- Folded into 1.0.0 before its release: a stuck resolution's question, decider and mover (the reviews since
  `T-020-01` landed). `one-question-per-resolution-transaction`, a stated-entry row, refuses a second question of
  Core's under a stuck resolution's transaction, an identical retry aside, so one set of answers settles it.
  `stuck-move-carries-out-its-decide-step` holds the mover to another login than the decider's too (field `login`),
  not its party alone. The world's stuck decisions are a desk's, `operator-desk-a`, a sibling of the mover's
  `operator-desk-b`, not the operator's own: under `T-021-06`'s rule, chains meeting below the operator are one
  actor. Refused: a second question under `tx-d3`; a mover of another party under the decider's login. 472 admission
  cases, 87 relations. (DEC-4, DEC-20, REQ-AUZ-4, ADR-0011, T-020-01, T-021-06)
- Folded into 1.0.0 before its release: a hand-on links an order its own route opens, and a hand-on of a part not
  stuck is the customer's choice (the reviews since `T-020-01` landed). `hand-on-links-an-unopened-order` refuses a
  `handed_on` whose linked order already has a route, and `linked-order-opens-on-its-hand-on` refuses a route
  opening a linked order that doesn't name the order it was handed on from. `hand-on-input-rests-on-the-linked-
  orders-acceptance` holds the hand-on of a part that isn't stuck to an acceptance with basis `given`, recorded by
  the customer (field `given`). Refused: p3's hand-on to ord-2 once ord-2 was opened on its own, ord-2 opened on its
  own route after the hand-on, a waiting part handed on on an automatic deposit's acceptance or on one the operator
  recorded. Admitted: a waiting part handed on on the customer's own. 477 admission cases, 89 relations. (DEC-4,
  DEC-20, REQ-ORD-43, REQ-ORD-13, T-020-01)
- Folded into 1.0.0 before its release: an end requested waits for value in motion, and a disputed exchange holds
  its part (the reviews since `T-020-01` landed). `part-waits-for-value-in-motion` now cites `end_requested` too,
  for every part of its order. A `failed` result released a `move_value` from motion even where its segment owed a
  reversal, so a failed exchange's disputed value let its part restart; it now releases only where the segment owes
  none. `--failed-exchange` carries the `failed` result Core writes and is still refused. Refused: an end requested
  with p1-s6's exchange moving. Admitted: one with nothing moving. 479 admission cases. (DEC-4, DEC-20, REQ-ORD-2,
  INV-3, INV-4, T-020-01)
- Folded into 1.0.0 before its release: a reversal's evidence is the from-node reserve's own word (the reviews since
  `T-020-01` landed). `reversal-rests-on-its-evidence` read `whose` as the evidence named it, so it could rest on the
  failed provider's statement under another's name. `whose` must now be the maker of a verified statement in
  `rests_on` (field `maker`), and the reserve at the segment's from-node (field `reserve`): the provider of the
  segment that delivered value there, or the withdrawal's own. The world's reversal of p1-s2 now rests on us-bank's
  statement, whose withdrawal put the value at ledger-in. Refused: us-bank named over fx-desk's statement; eu-bank's
  word. 482 admission cases. (DEC-4, DEC-20, REQ-ORD-3, T-020-01)
- Folded into 1.0.0 before its release: the log takes a new destination, or a waiting part's hand-on, only after a
  failed final deposit (`INV-10`, the reviews since `T-020-01` landed), as the state machine's rules now do. The
  world's p1 runs its recalculated route to its final deposit, which fails: p1-s6's exchange settles at ledger-in on
  fx-desk's signed result, p1-s7's deposit fails, and with no route left p1 waits, so the customer's new destination
  now starts from USDC at ledger-in. Refused: p1 waiting at its floor, p1-s6 settled and its final deposit not tried,
  sent to a new destination or handed on to ord-41. 491 admission cases. (DEC-4, DEC-20, INV-10, REQ-ORD-43,
  T-020-01)
- Folded into 1.0.0 before its release: a hand-on whose linked order was refused is taken back (the reviews since
  `T-020-01` landed), on the state machine's new `hand_on_taken_back`. `hand-on-taken-back-rests-on-the-linked-
  orders-refusal` holds it to an `order_refusal` of the linked order, which never opened (field `unopened`).
  Admitted: p3 taking back its hand-on to ord-2 on ord-2's refusal; refused: without the refusal, and once ord-2
  opened. 494 admission cases, 90 relations. (DEC-4, DEC-20, REQ-ORD-43, REQ-ORD-18, T-020-01, T-020-13)
- Folded into 1.0.0 before its release: a route handed on starts on its holding account's record (`REQ-ORD-40`, the
  reviews since `T-020-01` landed). A new entry kind, `holding_withdrawal {order, handed_on_from, holder, record}`,
  records the holding account's own record of the withdrawal, the start proof that settles a linked order's first
  segment. `holding-withdrawal-rests-on-its-record` holds the record to its holder's verified statement about the
  order whose value it held, `holding-withdrawal-starts-a-handed-on-route` to a route of the linked order handed on
  from that order, and `one-start-per-holding-record` to one start per record; the linked order's first move rests
  on it, by its holder (`handed-on-route-starts-on-its-holding-record`, ahead of the authorisation rows). The world
  records us-bank's withdrawal record starting ord-2. One valid fixture, 506 admission cases, 94 relations. (DEC-4,
  DEC-20, REQ-ORD-40, REQ-ORD-43, INV-5, T-020-01, T-020-13)
- Folded into 1.0.0 before its release: a publisher's word, labelled (`REQ-ORD-3`'s third sentence, the reviews since
  `T-020-01` landed). Inside a private node, its publisher's word counts for a reversal, labelled as such, and no
  field carried the label. `evidence` gains `publisher`, optional: the party the statement it rests on states for.
  `reversal-rests-on-its-evidence` admits the failed provider's own statement where it is so labelled and graded a
  statement, and refuses a label naming another publisher (field `publisher`). Admitted: p1-s2's reversal on
  fx-desk's word, fx-group's module, labelled fx-group; refused: labelled another group. 508 admission cases. (DEC-4,
  DEC-20, REQ-ORD-3, T-020-01, T-020-12)
- Folded into 1.0.0 before its release: who carries a reversal, and asking for one (`REQ-ORD-11`, `REQ-ORD-3`, the
  reviews since `T-020-01` landed). Nothing recorded who carries a failed segment's reversal, nor a provider's
  showing of fault and its acceptance, and Core couldn't ask a failed provider to reverse. A new entry kind,
  `reversal_carrier {order, segment, carrier, showing, accepted_by}`, records the carrier: the provider, or the
  customer or the operator at fault by the provider's showing, accepted (a carrier other than the provider names
  both, the provider neither). `reversal-carrier-rests-on-its-showing` holds the showing to the failed provider's
  verified statement about the segment, and `reversal-carrier-is-accepted-by-the-party-at-fault` the acceptance to
  that party. A new decided action, `ask_reversal {order, segment, provider}`, and
  `reversal-is-asked-of-its-failed-provider` hold it to a segment owing its reversal and the provider that failed
  it. Two valid fixtures, 519 admission cases, 97 relations. (DEC-4, DEC-20, REQ-ORD-3, REQ-ORD-11, T-020-01,
  T-020-12)
- Folded into 1.0.0 before its release: a publisher's label counts only on a private node the log binds (round 2 of
  the contract review). The label let any failed provider complete its own reversal: a verified statement by it,
  stating for the labelled party, sufficed, and it then skipped the from-node reserve. It now counts only where the
  log binds the segment's from-node as that publisher's private node: a `replay` of the order naming the node, or
  evidence of the order whose `movement.node` names it, resting on a verified statement stating for the publisher
  (field `private`). Refused: the review's reproduction, fx-desk's own word on p1-s2 at ledger-in labelled
  fx-group, with no node bound; admitted: the same with a replay of ord-1 naming ledger-in on fx-group's statement.
  520 admission cases. (DEC-4, DEC-20, REQ-ORD-3, T-020-01, T-020-12)
- Folded into 1.0.0 before its release: a hand-on taken back closes its linked order for good (round 2 of the
  contract review). After `handed_on`, the linked order's refusal and `hand_on_taken_back`, a `route_chosen` of the
  linked order handed on from the first was still admitted. `linked-order-opens-on-its-hand-on` now refuses any
  route opening an order a `hand_on_taken_back` names, and `hand-on-links-an-unopened-order` any new hand-on to it.
  Refused: the review's reproduction, and a new hand-on to ord-2 after the take-back. 522 admission cases. (DEC-4,
  DEC-20, REQ-ORD-43, T-020-01, T-020-13)
- Folded into 1.0.0 before its release: a holding record proves one start, keyed by its transaction (round 2 of the
  contract review). `one-start-per-holding-record` was keyed by the record's receipt, so the same record kept again
  under a new commitment was cited again; it is now keyed by the transaction the record names, as
  `one-order-per-deposit` is. `holding-withdrawal-rests-on-its-record` holds the record to a provider's result
  (field `result`), about the linked order whose route it starts (field `order`, not `handed_on_from`), naming its
  transaction (field `transaction`). The world's record is us-bank's result about ord-2 under tx-hold2. Refused: the
  review's dodge, an operation, a record about ord-1, one naming no transaction. 524 admission cases. (DEC-4, DEC-20,
  REQ-ORD-40, INV-5, T-020-01, T-020-13)
- Folded into 1.0.0 before its release: a reversal carrier's acceptance rests on an act (round 2 of the contract
  review). `accepted_by` was only a writer's label. `reversal_carrier` gains `accepted_with`, the act by receipt, and
  `session`; `OperatorCommand` gains `accept_fault`. The customer's acceptance rests on its own verified yes about
  the order and segment, given in its open session, as an acceptance rests on its yes and session; the operator's on
  its `accept_fault` command, by the deployment operator's party (`reversal-carrier-rests-on-its-acceptance`, in
  place of `reversal-carrier-is-accepted-by-the-party-at-fault`). Admitted: the customer's yes in sess-1, the
  operator's command. Refused: no act, the provider's showing as the act, a label naming the operator, a yes about
  another segment, a session never opened, one expired, an impostor's command. 531 admission cases. (DEC-4, DEC-20,
  REQ-ORD-11, REQ-ORD-32, T-020-01, T-020-12)
- Folded into 1.0.0 before its release: a showing of fault is its provider's result (round 2 of the contract
  review). `reversal-carrier-rests-on-its-showing` took any statement; it now requires a `result` (field `result`),
  as the outcome-evidence rows do. That the result's `fault` names the carrier stays with whoever holds the kept
  bytes, the log holding only their salted commitment. Refused: an operation as the showing. 532 admission cases.
  (DEC-4, DEC-20, REQ-ORD-11, REQ-AUD-2, T-020-01, T-020-12)
- Folded into 1.0.0 before its release: a reversal's carrier is of a reversal owed, and stands once the party at fault
  carries it (round 2 of the contract review). `reversal-carrier-is-of-a-reversal-owed` refuses a carrier for a
  segment not owing its reversal; `reversal-carrier-stands-once-the-party-at-fault-carries-it` refuses another
  carrier once the customer or the operator carries it, the provider then the party at fault staying the rule
  (`REQ-ORD-11`). Refused: a carrier for a settled segment, one after the reversal completed, the operator after the
  customer, the provider after the customer. Admitted: the provider then the customer, and the same carrier again.
  538 admission cases, 99 relations. (DEC-4, DEC-20, REQ-ORD-11, REQ-ORD-3, T-020-01, T-020-12)
- Folded into 1.0.0 before its release: an end requested waits only for the parts it ends (round 2 of the contract
  review). `part-waits-for-value-in-motion` held `end_requested` to every part of its order, so an end requested
  while a part past its start moved, which the state machine leaves going on, was refused. It now waits for value in
  motion only in the parts the state machine ends, those not past their first segment. Refused: an end requested
  while p2's withdrawal moves; admitted: one while p1, past its start, moves p1-s6. 539 admission cases. (DEC-4,
  DEC-20, REQ-ORD-12, REQ-ORD-2, T-020-01)
- Folded into 1.0.0 before its release: an open part's start undone waits for value in motion (the ruling on round 2
  of the contract review). A `reversal_completed` that undoes the start of a part still open fails it, so
  `part-waits-for-value-in-motion` now cites it, as it does a restart. Refused: p1's withdrawal undone while p1-s6's
  move is in motion; admitted: with nothing moving. 541 admission cases. (DEC-4, DEC-20, REQ-ORD-3, REQ-ORD-40,
  INV-4, T-020-01, T-020-12)
- Folded into 1.0.0 before its release: a customer's acceptance of fault is a statement of its own shape, after the
  showing (round 2's re-review of the contract). Any verified statement of the requester's about the segment
  counted as accepting the fault, a dispute or one made before the showing included. A new schema and type,
  `fault-acceptance` (`FaultAcceptance {order, segment, transaction, showing, fault}`, shape
  `urn:rutent:core-internal:fault-acceptance`), is the customer's own signed acceptance naming the showing it
  accepts, as an acceptance's yes is its reserve's. `reversal-carrier-rests-on-its-acceptance` holds the customer's
  act to that shape (field `shape`), about the showing's transaction, received and appended after it, and the
  operator's `accept_fault` to after it (field `showing`). Refused: another shape, one made before the showing, one
  received before it, one appended before it, one naming another showing, an `accept_fault` before the showing.
  One schema, one valid fixture, 547 admission cases. (DEC-4, DEC-20, REQ-ORD-11, T-020-01, T-020-12)
- Folded into 1.0.0 before its release: one hand-on per linked order (round 2's re-review of the contract, the
  reviewer's ruling). With a take-back closing the linked order, a sibling's hand-on to the same order would wait on
  it. `hand-on-links-an-unopened-order` now also refuses a `handed_on` naming a linked order another part or order
  already handed on to. Refused: p2 handed on to ord-2 after p3. Admitted: p1 handed on to a fresh ord-41. 548
  admission cases. (DEC-4, DEC-20, REQ-ORD-43, T-020-01, T-020-13)
- Folded into 1.0.0 before its release: a private node's binding is the order's own (round 2's re-review of the
  contract). The replay path of a publisher's label took a replay resting on any statement stating for the
  publisher; the statement it rests on, as the evidence path's, must now be about this order. Refused: a replay of
  ord-1 naming ledger-in resting on fx-group's statement about ord-2. Admitted: one resting on its statement about
  ord-1 (`--a-publishers-word-labelled`). 549 admission cases. (DEC-4, DEC-20, REQ-ORD-3, T-020-01, T-020-12)
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
- Folded into 1.0.0 before its release: a refused question about a person is recorded (round 3 of the contract,
  item t). `shortlisted-authority-cites-its-shortlist`, `provider-asked-before-was-asked` and
  `asked-before-is-complete` cited every `ask_about_person`, whatever its outcome, so a question policy refused
  could not be recorded even as a refusal, and the error told the caller whether a digest it named was a record of
  that order. Each now applies only to a permitted question, as `revocation-revokes-a-grant` does. Admitted: a
  refused question naming a digest no record has, one naming a provider never asked, one leaving out a provider asked
  before (`<row>--refused`). 552 admission cases. (DEC-4, DEC-20, REQ-CPL-2, REQ-CPL-5, REQ-AUD-1, T-020-01,
  T-021-07)
- Folded into 1.0.0 before its release: a fault acceptance accepts the customer's fault alone (round 3 of the
  contract, item u). Only the customer signs the `fault-acceptance` shape, since the operator accepts by its
  `accept_fault` command, so its `fault` is narrowed to `customer`: in the schema (`enum: ["customer"]`) and in Rust,
  where `FaultAcceptance.fault` is now `AcceptedFault`, a one-variant enum, in place of provider-role's `Fault`.
  `reversal-carrier-rests-on-its-acceptance` says who checks the body against the showing, since the log holds only
  salted commitments: Core on intake and a checker through `Evidence::fetch`, each holding the body's `fault` to
  `customer`, its `showing` to the commitment of the carrier's showing, and its `order`, `segment` and `transaction`
  to the showing's `about`. One invalid fixture, `fault-acceptance-of-the-operators-fault`: 125 invalid. (DEC-4,
  DEC-20, REQ-ORD-11, REQ-AUD-2, T-020-01, T-020-12)
- Folded into 1.0.0 before its release: the world holds a reversal carrier (round 3 of the contract, item w).
  `core-audit`'s `every_record_kind_core_writes_is_in_the_log` replays the world through `world--in_dest` and found
  no `reversal_carrier` in it. Between p1-s2's `segment_failed` and its `reversal_completed`, while its reversal is
  owed, the world now holds fx-desk's showing of fault for p1-s2 under `tx-f2`, the operator's `accept_fault`
  command after it, and the `reversal_carrier` naming the operator, resting on both (`world--showing12`,
  `world--accept_fault12`, `world--carrier12`); every later world case holds the three. The operator's path, not
  the customer's: the log `core-audit` builds knows the keys of the operator and of the providers it admitted, and
  no customer's, so a customer's fault acceptance there would be recorded unverified and its carrier refused. 555
  admission cases. (DEC-4, DEC-20, REQ-ORD-11, REQ-AUD-1, T-020-01, T-022-01)
- Folded into 1.0.0 before its release: a provider's terms and a reserve's hold rest on signed statements (round 3
  of the contract, item q, with provider-role 3.1.0's `terms` and `hold` shapes). Core kept fee terms and a held
  quote only in memory, so a charge raised against a formula and a hold redeemed rested on nothing signed. Type
  changes: `Difference::Charge` gains `result: Receipt`, the stated entry of the result whose charge it is, and
  `terms: Option<Receipt>`, the provider's signed terms it is above, absent where the price its route was priced on
  bound it; `What::Acceptance` gains `held: Option<Receipt>`, the reserve's hold it redeems; `StatedEntry::binds_from`
  is the later of a statement's `made_at` and Core's `received_at`, never `made_at` alone. Four rows:
  `charge-raise-rests-on-its-result` (verified, a result, its provider's, about its order and segment, the provider the segment's `move_value` named),
  `charge-raise-rests-on-terms-in-force` (verified, the terms shape, its provider's, binding no later than the
  segment's first `move_value` effect, so terms dated before a segment started but received after it bind nothing),
  `held-quote-rests-on-the-reserves-hold` (verified, the hold shape, made by the reserve each part starts at) and
  `hold-is-redeemed-by-one-order` (no acceptance of another order redeems the same hold). Which tuple terms are for,
  what a hold's body names, and its validity, are for whoever holds the kept bytes, the log holding only salted
  commitments. One invalid fixture, `raise-of-a-charge-without-its-result`; the charge raise fixtures name their
  result; 575 admission cases, 103 relations. (DEC-4, DEC-20, REQ-QTE-5, REQ-QTE-6, REQ-QTE-15, REQ-AUD-2,
  T-020-01, T-020-07)
- Folded into 1.0.0 before its release: a hold is keyed on its own transaction (the review of round 3, item 2).
  `hold-is-redeemed-by-one-order` was keyed on the hold's receipt, so a hold kept again under a new commitment and
  appended as a new entry could be redeemed by a second order at the held price (`THR-127`), and the log depended on
  the evidence store to see it (`REQ-AUD-9`). provider-role's `Hold` now names its `transaction`, its id; Core
  appends its stated entry about that transaction and no order, which attribution binds to the body's; the row is
  keyed on that transaction, as `one-order-per-deposit` keys a deposit on its notice's, and
  `held-quote-rests-on-the-reserves-hold` holds the entry to it (field `transaction`). Refused: the same hold kept
  again under a new commitment and redeemed by ord-2 after ord-9 (`--the-same-hold-kept-again`), a hold appended
  about nothing (`--transaction`). Admitted: the hold kept again, redeemed again by its own order. 578 admission
  cases. (DEC-4, DEC-20, REQ-QTE-15, REQ-AUD-9, T-020-01)
- Folded into 1.0.0 before its release: a customer's acceptance of fault is a policy decision (the review of round 3,
  item 3, the coordinator's ruling). No customer signs a statement the log can attribute (`StatementKeys::publisher`
  answers nothing for a customer), so the stated `FaultAcceptance` could never verify, and its admitted case passed
  only by making `cust-1` a provider component. Type changes: `Action` gains `AcceptFault {order, segment, showing:
  ShowingNamed, session}`, with a new struct `ShowingNamed {transaction: Id, commitment: Digest}` (schemas
  `common#/$defs/action_accept_fault` and `#/$defs/showing_named`), decided through `Decisions::decide` on the policy
  handle and never an effect (`Effect::check`, `What::check`); `What::ReversalCarrier` loses `session`;
  `FaultAcceptance`, `AcceptedFault`, `FAULT_ACCEPTANCE_SHAPE` and the `fault-acceptance` schema are gone.
  `reversal-carrier-rests-on-its-acceptance` reads the customer's act as a permitted `decide` of `accept_fault` by the
  order's requester in the customer's role, its own caller, about the order and segment, naming the showing's
  transaction and commitment, decided no earlier than the showing was received and appended after it, in a session
  still open when it was decided (fields `party`, `action`, `yes`, `session`, `showing`; `shape` is now `action`).
  The operator's path is unchanged. Admitted: cust-1, a person in the customer's role, in `sess-1`. Refused, new: a
  refused decision, another action, another showing's commitment, one decided before the showing was received. One
  valid fixture (`decision-request-to-accept-a-fault-in-the-customers-session`), one invalid
  (`verified-entry-of-an-effect-accepting-a-fault`); the fault-acceptance fixtures are gone. 580 admission cases.
  (DEC-4, DEC-20, REQ-ORD-11, REQ-ORD-32, REQ-AUZ-1, T-020-01, T-020-12)
- Folded into 1.0.0 before its release: a deposit's notice is the reserve's arrival notice (`T-020-18`'s review,
  item 1, high). `arrival-names-its-notice` held the cited stated entry to being verified, the arrival's reserve's
  and about a transaction, and never to its shape. Since round 3 a reserve's hold is about its own transaction, so a
  hold passed, and the operator, filing, could open an order on a deposit the reserve never stated. The row now
  requires provider-role's `urn:rutent:provider-role:arrival-notice` (new field `notice`, read last), so
  `one-order-per-deposit`'s deposit, read from an admitted arrival's notice, is always such a notice's. An arrival answer
  (`urn:rutent:provider-role:arrival-answer`) carries notices whole: each is appended as its own stated entry, about
  its credit's transaction, and that entry is the one an arrival cites, so a deposit is keyed on a notice and never on
  the answer that carried it. The world's deposit notice, `tx-k1`'s, is now of that shape, as is the notice kept again in
  `one-order-per-deposit--the-same-notice-kept-again`; `outcome-confirmation-cites-verified-evidence--about`, which
  cited that notice as a result about another order, cites a result of its own. Admitted:
  `--the-reserves-arrival-notice`. Refused: a hold cited as a deposit's notice, an arrival answer cited as one. 583
  admission cases. Outside `contracts/`, test-only: core-orch's arrival tests and the scenarios' arrival tests sign
  their notices under that shape (their bodies stay theirs until `T-020-19`), and the scenarios' seeded deposit
  (`segments_support::seed_arrival`), which cited the reserve's result as its notice, cites the reserve's own arrival
  notice; core-orch's `NOT_A_NOTICE` deny list
  stays `T-020-19`'s, and the log now refuses anything it lets through. (DEC-4, DEC-20, REQ-ORD-23, REQ-ORD-14,
  REQ-AUD-2, T-020-18)
- Folded into 1.0.0 before its release: a comparison that can't be made is raised (`T-022-05`'s review, item 1).
  `Difference` had no shape for two reports in two assets, or one asset in two units, which `THR-107` names, nor for
  a holding account whose balance can't be set against the value held or evidenced there, so the checks dropped
  them in silence and `REQ-ORD-9`, `REQ-AUD-15` and `INV-1` failed open. Two new kinds: `reports_not_comparable`,
  naming the order, the segment and each party's figure as it stated it (`Reported`: amount and asset), which
  `Raise::check` refuses where the two assets are one; and `holding_not_comparable`, naming the account, why
  (`HoldingIncomparable`: `evidenced_in_another_asset`, `no_reading`, or `sum_past_the_bound`, value held or
  evidenced past the balance by more than an amount states) and the balance's `word` and the account's `asset`
  exactly when a balance was read, since the check's entry names no asset; the schema and `Raise::check` both hold
  that. `Raise::check`'s rule names the new case. Five valid fixtures (reports in two units and in two assets) and
  five invalid ones. Outside `contracts/`, `core-audit`'s `compare` raises each. (DEC-4, DEC-20, REQ-ORD-9,
  REQ-AUD-15, INV-1, THR-107, T-022-05)
- Folded into 1.0.0 before its release: `core-internal`'s next round, the items the rows since gathered (`T-020-21`).
  Its row asks for one `DEC-4` bump; 1.0.0 isn't on `develop` yet, so `PLAN.md` §4.4 and retrospective lesson 7 fold
  it into the unreleased major, as `T-022-05` did, and the guard reads core-internal as a new contract. Each change,
  by the row's item:
  1. A notice's summary, `notice_summary {to: {party, role}, of: order {order, sequence} | raise {raise}, last_try,
     ended: delivered | abandoned}`, observed, for what a restart needs in place of the notice module's `Ledger`; a
     raise's closes the raise's own entry, written once. `notice-summary-names-its-raise` holds a raise's to a raise
     of the log, `one-summary-per-notice` keeps one per notice and receiver (`T-023-02`'s proposal; `T-023-03`).
     (REQ-ORD-20, REQ-ORD-7)
  2. router-api's `Operation::stated_out` documents its `None` (router-api's changelog; no shape).
  3. A mismatch at a non-final public end, `public_mismatch {order, segment, node, movement, found: wrong_destination
     | wrong_asset | wrong_amount, moved, stated, rests_on}`, and its raise, `difference: public_end {order, segment,
     found, moved, stated}` (`PublicFigures`: destination, asset, amount); the Rust types hold the figure `found`
     names to differ and the mismatch to its movement's network, and `public-mismatch-rests-on-the-providers-result`
     holds `rests_on` to the segment's provider's verified result about it, on a segment that isn't a part's final
     deposit (`T-020-22`). (REQ-ORD-33, REQ-AUD-1, ADR-0009)
  4. Two shapes for a checkpoint and its opening, `checkpoint` (`shape`, `log`, `tick` as 16 hexadecimal digits,
     `at`, `commitment`, `signature`) and `checkpoint-opening` (`shape`, `log`, `tick`, `salt`, `size`), so a party
     outside rutent's crates has a schema; a checkpoint carrying a size or a salt is invalid (`T-022-04`'s hand-off;
     `T-022-07`). (REQ-AUD-14, REQ-AUD-10)
  5. A decline of an outcome report, `decline_outcome {request: OutcomeDecline {request, order, effect, report,
     decliner, declined_with, at}, decision}`, a decision kind on policy's handle through a call of its own,
     `OutcomeDeclines::decline_outcome`. `outcome-decline-rests-on-its-report` holds it to a permitted report of the
     same move by another party and login, on another maker's statement, while the move and the report stand;
     `outcome-confirmation-cites-verified-evidence` holds its evidence as a confirmation's; `one-report-per-effect`
     admits a new report once the earlier one is declined, and `outcome-confirmation-rests-on-its-report` confirms no
     declined report (`T-020-20`). (REQ-AUZ-4, ADR-0011)
  6. `arrival.earlier`, the earlier arrival of the same deposit an arrival holds against an order;
     `arrival-names-its-earlier-arrival` holds it to an arrival at the same reserve, of the same deposit by its
     notice's signed transaction (`T-020-04`'s gap; `T-020-19`). (REQ-ORD-23, REQ-ORD-14)
  7. An empty shortlist's record was folded in already ("a shortlist may keep no provider", above); its test is
     named now, on the fixtures that held it (`T-020-25` writes it). (REQ-AUD-4)
  8. `HandedOver`'s `operation_unanswered {provider, transaction, ended_by}`, the platform's own record of an
     operation that ended with no answer (`T-020-08`'s gap; `T-020-25`). (REQ-MDL-8, REQ-ORD-1)
  9. The holding account's record and its relation were folded in already (`holding_withdrawal`,
     `handed-on-route-starts-on-its-holding-record`, above); it now names its inner `node`, which
     `holding-withdrawal-starts-a-handed-on-route` holds the route handed on to start at, and at a private node its
     `publisher`, the label `REQ-ORD-40` asks for, which `holding-withdrawal-rests-on-its-record` holds to the party
     the record's maker states for (`T-020-26`). (REQ-ORD-40, INV-5)
  10. The final-deposit relation was folded in already (`final-deposit-closes-on-a-public-record`, above, on
     `T-020-09`'s review); its test is named now (`T-020-22` writes Core's side). (REQ-ORD-33, AC-32)
  11. `Action::AskStatus {order, segment, provider}`, asking a provider for a late status, decided and taken as an
     effect, held to the segment's provider by `status-is-asked-of-the-segments-provider`; and
     `reversal-rests-on-its-evidence` reads the reserve at the segment's from-node, not the order's origin reserve,
     for the failed provider's own word. That widens admission in one case only: the provider that delivered to a
     later inner node fails out of it and vouches for itself as that node's reserve, as `REQ-ORD-3`'s second
     sentence allows; the origin reserve's word at a node not its own was refused before too (on `reserve`, now on
     `whose`), and no admitted history moved. A reversible reading, on the owner's form as ADR-0008's open question
     (`T-020-12`'s gaps; `T-020-27`). (REQ-ORD-1, REQ-ORD-3, INV-4)
  12. `decision-is-for-its-caller`: a `decide` entry whose actor isn't its request's caller is refused (`T-022-02`'s
     probe). (REQ-AUZ-1)
  13. `attribute` records a verified statement only in a role its maker holds, the operator's maker as the
     operator and any other as a provider or a verifier, and refuses one whose bytes bear its claim out in another
     (`IN_A_ROLE_ITS_MAKER_DOES_NOT_HOLD`; `T-022-02`'s probe). A module the operator publishes is a provider, so
     core-orch's `record_refused` claims it in the provider's role, not the operator's. (REQ-AUD-2)
  14. A charge's `no_formula: for_this_asset`, the mark of a fee in an asset no formula covers, beside its
     `maximum` of nothing, which `Raise::check` holds to `0`; `no-formula-marks-a-charge-in-another-asset` holds it
     to a charge in another asset than its segment's `from` (`T-020-14`'s closed gap 3; `T-020-29`). (REQ-QTE-5)
  15. A stuck part's evidence, `part_evidence {evidence, order, part, grade, whose, rests_on}`, of a part and no
     segment; `part-evidence-rests-on-statements` holds it to statements about its own part's resolution, through the
     transaction that part's `resolve_stuck` names, a bare statement to the provider the resolution names, and
     `stuck-resolved-cites-its-parts-evidence` refuses a `stuck_resolved` citing another order's or part's evidence,
     or a segment's (`T-020-16`'s gap 1; `T-020-20`). (REQ-ORD-39, REQ-AUZ-4)
  16. `answer-follows-its-question` holds a reserve's `arrival-answer` to the `arrival-question` Core asked under its
     transaction, as a `result` to its `operation` (`T-020-18`'s hand-off). (REQ-ORD-1, REQ-ORD-23)
  Twenty-eight schemas, 106 relations and 4 closing ones, three stated. (DEC-4, DEC-20, T-020-21)
- Folded into 1.0.0 before its release: a hand-on's outcome rests on the holding account's own answer (`T-020-26`).
  The holding account's wallet names its own withdrawal's transaction, which the operator's question binds to the
  linked order (`holding-withdrawal-rests-on-its-record` needs the record about the linked order), never the one the
  decision named, so no statement of the wallet could be the evidence a report or a confirmation of the hand-on's
  outcome cites: `outcome-confirmation-cites-verified-evidence`'s `about` read a stuck resolution's own transaction
  and no other. For a `resolve_stuck` whose resolution hands the value on, it now also accepts a statement about the
  linked order the decision named, no segment, through the transaction of this effect's own withdrawal: first bound
  to that linked order and to no segment by the operator's own `operation` question, appended after the effect and
  before its result, so a late answer of an earlier effect's withdrawal, which the log holds failed, is no evidence of
  another's. Its `maker` reads by kind: a report of a hand-on rests on the holding account's own statement (its maker
  the resolution's `by`), since a mover could otherwise report a failure on an unrelated maker's word and have it
  confirmed on the holder's own `succeeded`, and a new decision would pay again; a confirmation and a decline read no
  maker beyond the row's, since they need another maker's party than the report's. Two valid cases
  (`--a-hand-ons-report-on-the-holding-accounts-own-answer`, `--a-hand-ons-confirmation-on-another-makers-statement`)
  and eight invalid ones, each breaking one rule (`--a-hand-on-reported-on-another-makers-statement`, `maker`;
  `--a-hand-on-through-another-order`, `--a-hand-on-through-a-segment-of-its-linked-order`,
  `--a-hand-on-through-a-transaction-bound-to-another-order`, `--a-hand-on-through-another-effects-transaction`,
  `--a-hand-on-through-a-question-after-its-result`, `--a-hand-on-through-a-transaction-bound-by-no-operator-question`, `--a-hand-on-through-a-statement-naming-no-order`, each
  `about`). No shape changes; no consumer changes. (REQ-ORD-1, REQ-AUZ-4, DEC-4, DEC-20, T-020-26)
- Folded into 1.0.0 before its release: `core-internal`'s round after `T-020-20` (`T-020-30`). 1.0.0 isn't on
  `develop`, so `PLAN.md` §4.4 and retrospective lesson 7 fold it into the unreleased major, as `T-020-21` did. Each
  change, by the row's item:
  1. `stuck-resolved-cites-its-parts-evidence` is an exists row: a `stuck_resolved` cites a `part_evidence` of the
     log's with its id, of its own order (`order`) and part (`part`), appended before it, where it refused another's
     and admitted a bare id until `T-020-20` wrote the entry. A bare id, or a segment's `evidence` under the id, is
     refused (`--missing`, `--a-segments-evidence`); `--conflict` is now `--part`, beside `--order`. The admitted
     cases of `stuck-resolved-follows-a-done-resolution` hold their part's evidence. (REQ-AUZ-4, REQ-ORD-39)
  2. `ReportPair.terms`, the sending party's tuple's terms (`REQ-CAT-11`, ADR-0009, `T-022-05`'s gap 4): its fill
     `increment`, in the reports' unit, absent where the tuple declares none, and `reporting`, `{kind: net}` or
     `{kind: gross, charge}`, the charge the gross figure holds, which `ReportPair::check` holds to no more than the
     amount sent. Orchestration fills them from the tuple's terms where it builds the pair; `core-audit`'s `compare`
     takes the charge off a gross figure, raises nothing within one increment, and compares exactly with none. Two
     valid fixtures and five invalid ones; every pair the fixtures hold reads `net`, no increment, as before.
     (REQ-ORD-9, REQ-CAT-11)
  3. `MeasureRecord.role`, `provider` or `verifier` (`MeasuredRole`), the role the record measures its party in, set
     by its caller, so a party in both roles is measured apart in each (`T-022-06`'s review). The fixtures' answers
     are a verifier's, every other measure a provider's. One valid fixture and two invalid ones. (REQ-AUD-7,
     REQ-AUD-8)
  4. `What::Replay.rests_on` is a list (`T-020-18`'s hand-off of `REQ-AUD-9`): the exit segment's result first, then
     the receipt of each private node's publisher's `inner-segments` record the replay read, at most one per node it
     replayed that recorded something, which the Rust types hold. `replay-rests-on-its-exit` reads the first; the
     new closing row `replay-cites-its-nodes-records` holds each after it to a verified `inner-segments` statement
     about the replay's order (`verified`, `record`, `about`). A private node is still bound by the exit's result, the
     first receipt. `T-020-32` writes the records; every writer names the exit's alone until then. One valid fixture,
     three invalid ones, five admission cases. (REQ-AUD-9, REQ-ORD-41)
  5. `outcome-confirmation-cites-verified-evidence` reads a deliver's report by kind, as `T-020-26` reads a hand-on's:
     a report of a stuck resolution rests on the moving provider's own statement (its maker the `deliver`'s `by`), a
     confirmation or a decline on another maker's, so a mover can't report a failure on an unrelated maker's word and
     have it confirmed on the provider's own `succeeded`. The world's report now rests on bank-co's own result and its
     confirmation and decline on us-bank's, the two receipts swapped in every case that cites them; the old report is
     the refused `--a-deliver-reported-on-an-unrelated-makers-statement` (`maker`). (REQ-ORD-1, REQ-AUZ-4)
  6. A verifier's grant can be logged (`T-024-12`'s gap 1): `Action::ChangeCatalogue.class`, the class a verifier's
     `grant` or `revoke_grant` is for in a tuple's place, never with a tuple and on no other change, which the Rust
     types hold. `granted_advertising_keys` reads a grant of a class as of a tuple; `attribute` verifies a statement
     of a maker whose grants in force are of classes alone only where it is a judgement of one of those classes, and
     in the verifier's role alone, so a category grant never attributes a statement about a tuple's segments. The
     grants are read as they stood at the statement's own `made_at` (each change by its effect's time, done), as the
     key register trusts a key, so a revocation widens nothing backdated into the grant's window, a maker that has held
     classes alone attributes nothing once none is in force, and a tuple granted later widens only what is made after
     it (the review of `a2cde05d`). (REQ-AUD-2, REQ-CPL-12, REQ-CAT-13)
  Twenty-eight schemas, 157 valid fixtures, 159 invalid ones, 657 admission cases and 37 attribution cases; 106
  relations and 5 closing ones, three stated. Outside `contracts/`: `core-audit`'s `compare` and `measures` and its
  key register, `core-orch`'s closing writer and reversal reader, and the consumer lines named in the commit.
  (DEC-4, DEC-20, T-020-30)
- Folded into 1.0.0 before its release: `core-internal`'s round after `T-020-30` (`T-020-36`). 1.0.0 isn't on
  `develop`, so `PLAN.md` §4.4 and retrospective lesson 7 fold it into the unreleased major, as `T-020-30` did. Each
  change, by the row's item:
  1. A publisher's `inner-segments` record is verified about an order (`T-020-32`'s blocker). `attribute`'s step 6
     verified an entry about an order only through a transaction its body names at its top level, which a record
     names only inside `segments`, so the record was never verified about the order it binds and
     `replay-cites-its-nodes-records` and `private_node_of` refused it. It now verifies such a record about an order,
     no segment and no transaction, where its `about` names the node its body names, its body names the publisher its
     maker states for (the review of `d11e5a2b`), it lists at least one segment, and
     every segment's transaction is first bound to that order and asked under for it by the operator's own verified
     `operation` question: one naming another order's transaction, one the log binds to none, one Core never asked
     under, or one whose questions another maker asked, is attributed to no one about it. Thirteen attribution cases
     (three verified, ten unverified), and two tests on `core-audit`'s log. (REQ-ORD-41)
  2. A record names its node: `About.node`, named only by an `inner-segments` record, about no segment or
     transaction (the schema's `if`/`then` and `StatedEntry::check`), and verified against its body.
     `replay-cites-its-nodes-records` gains three fields: `node` (one the replay replayed and didn't find silent),
     `publisher` (stating for the party its exit's result states for, the publisher the replay binds its nodes to)
     and `once` (one record per node). `private_node_of` binds through a record only the node it names
     (`reversal-rests-on-its-evidence`'s `private`). One valid fixture, two invalid ones and nine admission cases.
     (REQ-AUD-9, REQ-ORD-3, REQ-ORD-41)
  3. `Action::AskStatus.transaction`, required: the fresh transaction a late status is asked under (`T-020-27`'s
     review), so it is traceable from the log; a new row, `status-is-asked-under-a-fresh-transaction` (none other),
     refuses one any earlier entry names: a stated entry's `about`, or a verified entry's action (`ask_status`,
     `resolve_stuck`, an `accept_fault`'s showing), `stuck_decide` or hand-over, but its own permitting decision, the entry at its permit's receipt deciding the
     same action (the reviews of `d11e5a2b` and `2b0f32b1`). `core-orch`'s `Reversals::ask_status` draws it before policy decides. One invalid fixture
     and eleven admission cases; the status fixtures name
     `tx-status-1`. (REQ-ORD-1, REQ-ORD-38)
  4. `handed-on-route-starts-on-its-holding-record` held a route handed on's first move to a holding record whose
     `holder` was the move's `origin_reserve`, so a plain wallet holding the value, with the linked route's first
     provider running the move (`T-020-33`, the owner's form `hand-on-wallet-holder`), was refused. The row now holds
     the record (still required) to the holding account the hand-on named (`holder`: an operator's `resolve_stuck`
     effect's `by`, or, for the customer's hand-on, which names none, the linked route's first provider), at the node the first segment moves from (`node`), and the move to the provider the linked route's plan
     names (`provider`). Two admitted cases and five refused ones; the old `--holder` stays refused, now on the hand-on's
     name. (REQ-ORD-40, REQ-ORD-43)
  Twenty-eight schemas, 158 valid fixtures, 162 invalid ones, 684 admission cases and 50 attribution cases; 107
  relations and 5 closing ones, three stated. Outside `contracts/`: `core-orch`'s `reversal/` (the late status's
  transaction) and every `About` literal Core and its tests build (`node: None`), `core-authz`'s two `AskStatus`
  literals, `core-audit`'s log tests, and the scenarios' two `About` literals. (DEC-4, DEC-20, T-020-36)
- Folded into 1.0.0 before its release: `core-internal`'s round after `T-020-36` (`T-020-38`), for the shape
  `T-020-35` found missing. 1.0.0 isn't on `develop`, so `PLAN.md` §4.4 and retrospective lesson 7 fold it into the
  unreleased major, as `T-020-30` and `T-020-36` did. A `ReportPair` held one `segment`, and both statement rows read
  each report's `about` against it, so a pair needed two parties' verified statements about one segment; at a
  segment's end Core holds only that segment's provider's own result, and the next provider's report of receiving is
  its own result, about the next segment (`T-020-35`'s finding, confirmed by its review). Each change:
  1. `Report.segment`, required: the segment each report is about. The pair's `segment` is the sent report's, and its
     received report is about another (`ReportPair::check`), so `core-audit`'s `compare_reports` and the
     `Difference` it raises name the segment whose end is compared, unchanged. Two invalid fixtures the Rust types
     refuse, one the schema refuses. (REQ-ORD-9)
  2. `sent-report-is-its-partys-statement` and `received-report-is-its-partys-statement` read each report's `about`
     against its own segment, and, after the review of `8a6854b1`, hold the statement to provider-role's `result`
     shape (a new field, `result`) and to an `about` naming the pair's order and the report's segment through a
     transaction first bound to it, so neither another shape nor a result about nothing backs a report. Six refused
     cases: a sent report about the received segment, a received report about the sent one (the pair before this
     round), and for each row another shape and a result about nothing. (REQ-ORD-9, REQ-AUD-2)
  3. A new row, `received-report-is-about-the-next-segment` (exists): the received segment is the one a plan of the
     pair's order lists right after the sent one (a `route_chosen` part, a `route_recalculated` or a
     `destination_changed`, the route a decision record backs), so a route's last segment has no pair and a pair
     spans one plan. Five refused cases: a segment no plan lists, a later one of the part, another part's, the one
     before, and a recalculated route's first. (REQ-ORD-9)
  4. A new row, `report-pair-is-by-its-segments-providers` (exists): each report's party is the provider this log's
     `move_value` effect moved its segment's value with, so a pair rests neither on a party that ran neither segment
     nor on a segment never run. Three refused cases. (REQ-ORD-9, REQ-AUD-2)
  The world's report pair is now us-bank's result about `p1-s1` (a new stated entry, receipt 440, in every history
  holding the pair) sent and fx-desk's about `p1-s2` received. Twenty-eight schemas, 158 valid fixtures, 165 invalid
  ones, 698 admission cases and 50 attribution cases; 109 relations and 5 closing ones, three stated. Outside
  `contracts/`: `core-audit`'s log tests (the pair on the real log). (DEC-4, DEC-20, T-020-38)
- Folded into 1.0.0 before its release: the contract round for the owner's decision-form answers of 2026-11-18
  (`T-020-39`). 1.0.0 isn't on `develop`, so it folds into the unreleased major, as `T-020-30`, `T-020-36` and
  `T-020-38` did. Each change, by its form item:
  1. Item `rollback-permission`: `CatalogueChange::RollBack` (`roll_back`), a release's staging ended with the version
     in force kept (ADR-0014), a decided change of its own about one release, naming the staged version. One valid
     fixture and one invalid (no version, which the Rust types refuse). Policy has no rule for it until `T-021-12`.
     (REQ-AUZ-6, REQ-AUZ-1)
  2. Item `confirmer-evidence-party`: `outcome-confirmation-cites-verified-evidence` (a report's, a confirmation's and
     a decline's evidence) and `part-evidence-rests-on-statements` (the stuck resolution's own evidence entry) gain a
     field, `party`: the statement's maker is a provider of a segment on any route this log gave the order or an
     order a hand-on joins it to, recalculated and changed routes included, or the one a move of it went to; the
     holder of a holding account its value rested at (its arrival's reserve, a stuck resolution's mover, a route
     handed on's holder); or it states for the publisher of a private node those routes crossed,
     as `private_node_of` binds the node. An outside provider's word is no evidence about the order. Four refused
     cases (a confirmation, a move's report, a decline and a part's evidence, each on an outside provider's result)
     and two admitted (another provider of the route; one only a recalculated route names). (REQ-AUZ-4)
  3. Item `erasure-of-cited-evidence`: `Action::EraseStatement.overrides_citations`, false unless set and written only
     when set. `erasure-waits-while-anything-rests-on-the-statement` cites nothing for an erasure whose permitting
     decision's action sets it: the decision's own, an effect's through its permit's receipt, an erasure record's
     through the erase effect of its commitment this log holds done. Three admitted cases (the decision, the effect
     and the record while cited) and one refused (an effect setting it on a decision that doesn't, refused as not
     the action decided). (REQ-AUD-13)
  4. Item `deposit-no-firm-rate`: `AcceptanceBasis::GivenOnDeposit` (`given_on_deposit`), the customer's acceptance of
     an order filed on an automatic deposit whose route has no firm rate: in a session, as `given`, and with no yes,
     resting on the deposit's arrival, as `automatic_deposit` (`on_a_deposit`, `in_a_session`). Its check, and the rows
     `automatic-deposit-rests-on-its-arrival`, `acceptance-is-of-the-quote-shown`, `move-rests-on-its-orders-acceptance`,
     `first-move-rests-on-its-parts-yes`, `chosen-route-rests-on-its-orders-acceptance` and the deposited test
     `split-deposit-arrival-names-its-part` reads, take it. One valid fixture, two invalid ones the Rust types refuse,
     two admitted cases and four refused. (REQ-QTE-10, REQ-ORD-32)
  5. Item `thr3-backdated-after-revocation`: `StatementKeys::signs_for` takes the statement's received time beside
     its `made_at`, and `attribute`'s step 8 passes the entry's `received_at`. The rule is the contract's,
     `revoked_before_receipt`: every grant of the maker in force at its `made_at` revoked, by an effect the log holds
     done, no later than its receipt, whatever the `made_at`; a key source answers no there. The trait's sentence
     calling the backdating a residual (`THR-3`) states the rule instead, and so does the README. Six attribution
     cases: before the revocation, with another grant standing, and made after the grant was made again, verified;
     after the revocation, at its instant, and made before it but received after the grant was made again,
     unverified. The log's register holds it from `T-022-09`. (THR-3, REQ-AUD-2)
  After the review of `27b0f6b9`: the `party` field reads no `holding_check` (no row ties one to the order) and a
  `stuck_decide` only where policy permitted it, in `party_to_the_order` and the hand-on closure alike; fourteen more
  admission cases, one per way a maker is made a party, alone.
  Twenty-eight schemas, 160 valid fixtures, 168 invalid ones, 728 admission cases and 56 attribution cases; 109
  relations and 5 closing ones, three stated. Outside `contracts/`: the consumer edits the shapes force, named in the
  round's commit. (DEC-4, DEC-20, T-020-39)
- Folded into 1.0.0 before its release: the quote an order filed on an automatic deposit waits on its customer to
  accept (`T-020-41`, item `deposit-no-firm-rate`, "Wait for the customer"). `shown` gains an optional `awaiting`
  (`Awaiting`: the party it waits for, until when the quote stands, and its slow segments, `SlowSegment`: place and
  declared seconds, no provider or amount), written only for such a quote, so the notices tell the customer from the
  log alone (`REQ-ORD-20`). Its check: on a quote shown without a firm rate it names at least one slow segment, and on
  a plain quote, which a quote asked again after the customer was asked may come back as, none; each place once;
  never on a price. Absent, a `shown` reads and writes as before. A notice's summary may be `of` an order waiting for
  its customer (`NoticeOf::Waiting`: the order and the quote's digest), since such a notice has no sequence. Two
  valid fixtures and a summary's, and three invalid ones, the Rust types refusing all three. Twenty-eight schemas, 163
  valid fixtures and 171 invalid ones. 1.0.0 isn't on `develop`, so it folds into the unreleased major and no version
  moves. (DEC-4, DEC-20, T-020-41)
- Folded into 1.0.0 before its release: a linked order starts once (`T-020-44`, M3's milestone review). The log held one
  start per record (`one-start-per-holding-record`), not one per linked order, so a second, different holding record
  of the same order was admitted. A new row, `one-start-per-linked-order`, refuses a `holding_withdrawal` of an order
  when another of that order cites a record of another transaction; the same record again stays the first row's and a
  retry. Admitted: the same start again. Refused: a second record of another transaction for ord-2. 730 admission
  cases and 56 attribution cases; 110 relations and 5 closing ones, three stated. 1.0.0 isn't on `develop`, so it folds into the unreleased major and no version moves. (DEC-4, DEC-20,
  REQ-ORD-40, INV-5, INV-4, T-020-44)
