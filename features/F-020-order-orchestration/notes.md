# F-020 notes

Readings taken where `SPEC.md` or `PLAN.md` is silent or ambiguous, recorded per Narthex `REQ-SKL-24`.

## T-020-02

- **One bump: 4.0.0.** This row first folded its change into router-api 3.0.0, unreleased then, as a third bullet
  of its changelog entry. `M2` was then merged into `develop` (pull request #60, `fb883e0`), so the contract guard
  read 3.0.0 as released and refused the change at 3.0.0 ("version not bumped"). The row's Work names that case:
  the new required `PolicyBounds` fields make router-api 4.0.0. The bullet moved, unchanged, into a 4.0.0 entry
  that says why, and the guard against `origin/develop` passes at 4.0.0.
- **What `PolicyBounds` gained.** `operator_fee`, a `FeeFormula`, and `slippage_limits`, both required. The fee is
  taken at the destination in the request's `to` unit from what the providers deliver, capped by its maximum and
  never raised to it, as `quote::quoted_price` already took it. Its proportion is at most one (`PolicyBounds::check`):
  a fee taking more than what arrives in proportion can't be shown, and the ranking reading below needs it.
- **Slippage limits keyed by tuple, not figure id.** `quote::SlippageLimit` named a tuple figure by id, which is
  local to one snapshot; the shortlist and choice rounds read different ones. A limit now names its tuple by
  provider and ends, as `excluded_tuples` does, and `commitments` matches it on the operation's provider and ends.
- **"Policy bounds without a slippage limit … are invalid" needs the snapshot.** Whether a tuple settles within
  three days is known only from its price or quotes, so the rule is `PolicyBounds::check_against(snapshot)`, and
  `DecisionRecord::check` runs it; the `Router` trait's precondition names it. It holds for exchange tuples only,
  since reserves and ramps set no rate (`commitments` treats them so already). A tuple counts as within three days
  if its price or any accepted fresh quote about it declares at most 259,200 seconds, so the rule fails closed.
  `THREE_DAYS_SECONDS` moved into router-api; `quote` re-exports it.
- **The floor after the fee, and what the contract checks.** The floor is "the least the customer accepts at `to`",
  so the router now drops a candidate as `below_floor` when what is left after the operator's fee is below it
  (the review of `M2`'s pull request, the example 99,000 / 99,500 / 1,000). The contract checks it exactly, both
  ways (after review: a first pass left it to the suite and relaxed the contract, which accepted a false
  `below_floor` even at a zero fee). No division is needed: `ceil(a × n / d) ≤ k` exactly when `a × n ≤ k × d`, and
  the cap is a second branch, `maximum ≤ a − floor`; a `no_std` long multiplication of amounts and
  `Amount::checked_sub` do it. The double judges the floor after the fee, and a flaw `FLOOR_BEFORE_THE_FEE` is now
  refused by the contract itself on a new case. `judge.rs`, `quote::shown` and the public
  `quote::received_after_operator_fee` share one function, so the floor, the price shown and Core's checks at
  each settled segment (`AC-42`) can't disagree.
- **"The record says its ranking is before the operator's fee."** A wire field on the record, `ranking`, with one
  value, `before_operator_fee`, rather than prose alone, so ranking net of the fee would be a new value a reader
  sees. On the record, not the bounds: it says what the router did, not what the operator chose.
- **Item for the form, `m3-rank-before-fee`.** `REQ-QTE-2` has the operator's formula follow `REQ-QTE-5`,
  which lets it depend on the tuple. With a formula per tuple, ranking before the fee may not give `REQ-QTE-11`'s
  lowest total cost. The bounds hold one formula, so the case can't arise yet; allowing it is a major. The property
  `one_operator_formula_at_the_destination_ranks_candidates_the_same_before_and_after_its_fee` pins the reading,
  and fails at seed 9 when one candidate's fee is computed by another formula.
- **Types moved, checks stayed.** `StatedTerms`, `SettlementTimes` (the declared and actual pair) and `HeldQuote`
  are router-api's, with schemas and fixtures. `HeldQuote::hold`, `redeem` and `label` became the free functions
  `quote::hold`, `quote::redeem` and `quote::held_label`, since the router can't add methods to a contract type.
  `quote::measure` reads a `SettlementTimes`; `SegmentTime::measure` builds one and calls it. `StatedTerms` names
  its tuple (after review), since nothing else would key a tuple's history. `hold` runs `HeldQuote::check`, the
  record's own included, and `redeem` refuses a hold whose record chose nothing as `NothingChosen`.
- **The quote functions keep their explicit arguments.** `quoted_price` and `commitments` still take the formula and
  the limits as arguments, so their tests can vary them; Core passes the record's own
  `bounds.operator_fee` and `bounds.slippage_limits`, which the docs say.
- **`rutent_router::VERSION` is `rutent-router-0.4.0`**: the same inputs may now drop as below the floor a candidate
  0.3.0 kept. The fixture the router's own record must equal names it.
- **Tests' bounds.** Every existing bounds gets a zero operator's fee, so no expected route moved. Tests and cases
  with exchanges get a limit of 50 basis points for every exchange (`common::limited`, `cases::limits_for`), drawn
  after the other draws so the property networks are unchanged.
- **Seen failing first.** `floor_is_judged_on_what_the_customer_receives_after_the_operators_fee` failed against
  the router judging the floor before the fee (kept, not `below_floor`), then passed with `judge.rs` restored. The
  ranking property failed under the mutation above. The contract's four "… is invalid" tests were written with the types
  they test, so they weren't seen failing first; each rejects its fixture with the named rule. After review, the contract's floor rules
  were each switched off once: judging the floor before the fee let the kept-below fixture through (three tests
  failed), and dropping the `below_floor` rule let all three false-`below_floor` fixtures through (three failed).
- **A limit is 1 to 10,000 basis points**, as provider-role's bounded slippage is; a limit of 0 isn't allowed, and
  an exchange held to no move states a firm rate in its quote.
- **Touches `DEC-20` paths**: `contracts/`. Also edits the router's `quote` and `judge` modules and `lib.rs`, and the
  router suite in `conformance/src/router/`, as the row's Work and `PLAN.md` §4.4 allow.

## T-020-03

- **Three calls on one `Intake`** (`components/core-orch/src/intake/`). `open` takes an `OrderRequest` under the
  caller's request identifier, `quote::<R>` runs one routing through `rounds::route` and shows its quote, and
  `accept` takes the requester's acceptance of the quote shown, and confirms a destination's new owner (since the
  review, below). What they reach comes in a `Desk`: the rounds' `Parts`, the destination register, policy's
  `Authorisations`, the bounds and prices the router runs under, and Core's and the operator's actors. Humans and
  agents call the same functions with the same types (`REQ-OPS-5`); only the `Actor`'s `kind` and `acts` differ.
- **One order per request identifier** (`REQ-ORD-19`, `INV-8`). The same request again under its identifier is told
  what the first was told and records nothing. Another request under it opens nothing and records nothing:
  `IntakeError::Conflicting`, naming the order the identifier opened. Reading, reversible: it could instead be told
  the first request's answer. A repeated acceptance of an accepted order is told the same and records nothing. An
  acceptance whose append, or whose start's, failed is carried on by accepting again: the authorisation policy
  permitted isn't asked again, the acceptance isn't appended twice, the start is appended as first built, and the
  order is told accepted only once it has started; a quote isn't run again meanwhile. The
  register is in memory; keeping it across a restart is the deployment's (`F-030`), as with the rounds' `Issued`.
- **Refused, nothing exists.** Only a request that breaks the deployment's rules is refused before an order exists:
  an `order_refusal` naming no order, class `malformed_request`. The rules are intake's reading: the caller acts for
  the requester in the customer's role; the route asked ends at the destination's node; a settlement account is held
  at the destination's reserve; and a deposit input names no other address than the destination's. Every other
  request opens an order, told `exists` with its outcome to come, and every later refusal names it.
- **Classes of cause** (`REQ-ORD-18`). A routing that offers nothing is told "no route for this amount" where the
  deciding record (the last choice round's, or the shortlist's where none ran) considered a candidate and dropped
  every one for its amount (`outside_band`, `beyond_levels`, `fee_above_amount`, `nothing_delivered`,
  `no_positive_outcome`, `below_floor`), and "no route right now" otherwise. A drop reason router-api adds later
  counts as now until it is named. The rounds' other answers: `OverBound` is `over_bound`; `PersonQuestionsPastBound`
  is `refused_by_policy`, since it is policy's bound for the order, not the caller's party's; `NoReservesYes` and
  `NotEligible` are `no_route_right_now`, since the reserve or the verifier may answer later. A policy refusal of
  the authorisation is `over_bound` for its own class, and `refused_by_policy` for every other. A chosen part
  `snapshot::check_part` refuses is `no_route_right_now`, recorded before anything is shown. Readings, reversible.
- **No provider text reaches the customer.** What a caller is told (`Told`, `Status`) holds identifiers, a class
  from a closed list and the quote Core built from the router's record. A provider's words stay in its stated entry,
  kept by the evidence store, and the log holds them by commitment only. The rule is the types', so no rule-off
  stands for it beyond the test, which feeds refusals carrying markup and third parties' names.
- **The quote shown, a minimal one.** `Quote` names the order, the choice round's decision record, the chosen
  candidate, both ends, the amount and what the route delivers before the operator's fee, labelled a quote. Its
  digest is SHA-256 of its RFC 8785 canonical JSON, the recorded decision's rule, and the `shown` entry, by Core,
  names it. `T-020-07` adds the operator's fee shown apart, the slippage limits and a quote without a firm rate,
  which changes the digest and nothing else here. A routing run again replaces the quote; only the latest is
  accepted (`IntakeError::NotTheQuoteShown`).
- **The acceptance** (`REQ-ORD-10`, `REQ-ORD-34`). Only the order's requester accepts: an actor for its party, in
  the customer's role, a person or an agent; anyone else is refused (`NotTheRequester`) with nothing recorded, so a
  stranger can't end another's order. The destination is resolved first; then the authorisation goes to policy, and
  only on its permit is the `acceptance` recorded, its `who` the actor that accepted, then the `route_chosen` input.
  Policy's not known leaves the order waiting, with nothing recorded by intake; a refusal ends it, as an
  `order_refusal`. Reading: the authorisation is asked before the acceptance is recorded, so a refused one leaves no
  acceptance; core-internal's README lets it come after. The operator acting for the user (`REQ-AUZ-9`) gives an
  authorisation, never the customer's acceptance of a quote, so it isn't a caller of `accept`.
- **The reserve's yes and the session** (`REQ-ORD-32`). The acceptance cites the receipt the rounds' `Offer::Offered`
  names, and the session the order was routed in. Intake asks no provider anything itself: every question, the
  reserve's request for its yes among them, is the rounds', asked under a fresh transaction for the order, signed
  with the operator's key, appended about the order and transaction before it is sent, and its answer appended
  with that transaction in its `about`. The tests show it end to end: the replayed log admits the acceptance's rows
  (`acceptance-rests-on-the-reserves-yes`, `acceptance-is-asked-in-the-users-session`,
  `acceptance-is-of-the-quote-shown`) and `answer-follows-its-question` on every answer.
- **One yes for a split order** (a contract gap at first, closed by `T-020-01e`; see `T-020-03b` below).
- **The session comes with the request.** The user's session is opened through policy (`Sessions::open_session`)
  by whoever takes the user's login before it reaches intake, and handed to the catalogue by the assembly, as
  `F-021`'s notes have the register's hand-off. Intake doesn't open it; the log holds the acceptance to a permitted
  `open_session` of the requester's that hasn't ended.
- **The destination** (`REQ-ORD-24`). No provider-role operation resolves a destination or names its owner, so a port,
  `intake::Destinations`, answers it: an owner, not yet, or no. The assembly backs it from the destination reserve's
  register; Core records each answer as a `destination_check`. It is resolved when the order opens, and that owner is
  kept; at acceptance it is resolved again. Not yet waits (`destination_not_yet_resolved`, accept again later; once it
  resolves, to the owner first resolved, it is accepted), no ends the order (`destination_refused`), and another owner
  makes the order wait for the customer (`destination_check` `owner_changed`, then, since `T-020-01e`, an
  `order_refusal` of `destination_owner_changed`): the requester's next acceptance names that owner and goes through
  policy.
  Confirming it again before the last segment is `T-020-08`'s and `T-020-13`'s. Reading, reversible.
- **The requester's address** (`REQ-ORD-35`). The destination's account is the one place the address lives: intake
  sets it as the deposit's input `beneficiary.address` (provider-role's name, `intake::DESTINATION_INPUT`), so the
  deposit dry run asked is to the address the authorisation names as `destination.account`. A request naming
  another address there, or a destination on another network than the route's, is refused before an order exists.
  A reserve that can't deposit to an address refuses in its dry run, and the order has no route right now.
- **The order's start.** `route_chosen` carries the chosen candidate's parts as the state machine plans them, each
  part and segment under a fresh identifier, citing the choice round's decision record, stated by the customer who
  accepted (source `customer`, the acceptor's `acts`, standing `stated`, as core-internal's `world--in_route` has
  it), with time source `core_clock`. Feeding the input to the state machine is the assembly's (`T-024-02`) and
  `T-020-08`'s, from the log.
- **What policy's sources read.** `Intake::record` answers an order's requester, parties and origin reserve, for the
  assembly to answer `F-021`'s `Requester`, `OrderOrigin` and `OrderParties` sources before the acceptance exists.
- **Tests, and the order.** The code came first, so rule-off checks stand in for seeing each test fail first. The log
  is the real core-internal reading: no `AuditLog` is merged, so `tests/intake_support/` holds one log both handles
  write to, and replays it through `attribute` and `admit_entry` on the path each entry came by (policy's
  `open_session` and `authorise` on `Through::Policy`, the rest on `Through::General`), as `T-020-06`'s
  `Env::admitted` does. Each rule was switched off once and its test seen red: the replay of the same request; a
  different request under a used identifier; a repeated acceptance; a refused request given an order; the amount rule
  both ways; acceptance of another quote; acceptance by anyone; not known taken as a permit; the start at the quote;
  the start before its acceptance; the acceptance recorded as Core's; the authorisation given by Core; the beneficiary
  recorded as the requester; a settlement account at any reserve; the owner not compared; not yet taken as no;
  confirmation by anyone; the deposit input not set from the destination; another address kept; another network kept;
  an agent refused where a person is taken; the acceptance citing another entry as the yes, another session, or
  another quote than shown (each refused by the replayed log); and, after review, the order told accepted before its
  start was appended, policy asked again on a retry, the acceptance appended again on a retry, and a late destination
  left refused. An eleventh test, beyond the row's list, `acceptance_whose_append_failed_is_carried_on_once`, covers
  the retry; the test log fails one append on demand. The property test found one bug as written: a request repeated
  was compared after intake had set its deposit input, so it read as another request; intake now keeps the request as
  asked.
- **Touches**: only `components/core-orch/src/intake/`, its tests (`tests/intake.rs`, `tests/intake_support/`,
  which reuses `snapshot_support` and `rounds_support` unchanged), these notes, and `proptest` as a dev-dependency
  of core-orch, already locked, so `Cargo.lock` gains one dependency line. No `DEC-20` path.

### T-020-03, review answers (of `1cc10a0`)

Merged `m3` at `bd9e1d5` first (grants naming their keys; `attribute` taking the history). Each test below was
written first and seen red on the code under review, then the fix made it green; each rule was then switched off
once and its test seen red again.

- **The test kit holds actors to the register.** The log now attributes each stated entry and admits each entry as
  it is appended, through `attribute` (with the history) and `admit_entry` on the path it came by, as the real log
  does, so a refused append is a refused append. It can fail an append, or write it and lose its answer, once, on
  demand. Policy's stub answers `ActsFor` first: an authorisation whose giver doesn't act for its stated party is
  refused `no_accountable_party`. The kit has an impostor, `someone-else` stating the customer's party, whom the
  register lists for no one; its absence is why B1 and B2 weren't caught.
- **B1, a carry-on recorded an actor policy never checked.** `Accepting` keeps the actor policy permitted, and only
  it carries the acceptance on: any other, even one acting for the same party, is refused `OtherActor`. Every call
  that reads an order (`told`, `quote`, `accept`) is held to the register of actors, through a port onto it
  (`intake::Actors`), so an actor that only states the party is told the order doesn't exist. Test
  `acceptance_is_carried_on_only_by_the_actor_policy_permitted`; red as reviewed, accepted as `someone-else`.
- **B2, the owner's confirmation went round policy.** `confirm_owner` is gone. While an order waits on a new owner,
  only an acceptance naming that owner (`accept`'s `owner`) goes on: it resolves the destination again, goes to
  policy as an authorisation given by the confirming actor, and is recorded as the acceptance, its `who` that actor,
  after a `destination_check` that resolved to the owner confirmed. A refusal about the caller (any class but
  `outside_authorisation`) answers `IntakeError::Refused` and leaves the order as it stood, waiting included; only
  `outside_authorisation`, a refusal about the order, ends it, as `refused_by_policy`. Reading, reversible: which
  classes are about the order. Tests: `destinations_change_of_owner_makes_the_order_wait` and
  `order_is_not_started_without_acceptance`.
- **Should-fix 1, a retry built a new entry.** The acceptance and the start are built once, when policy permits, and
  appended as built from `Accepting`, so a retry after a lost answer appends the same entry, which the log admits,
  and the order isn't wedged by `order-is-opened-once`. The test moves the clock on before the retry and loses the
  answer of each append in turn. Red as reviewed: the start refused as a second `route_chosen`.
- **Should-fix 2, a refused append wedged the order.** Only `Unavailable` is carried on. Where the log refuses the
  acceptance or the start (the session the yes was asked in has ended, say), the order ends, `no_route_right_now`,
  recorded as an `order_refusal`, and is quoted and accepted no more. Reading, reversible: the class. Tested by
  ending the session before the acceptance.
- **Should-fix 3, the session wasn't tied to the requester before the reserve was asked.** `open` reads the session
  from policy's register through a port (`intake::SessionRegister`): a session the register doesn't hold, or holds
  for another party, or that has ended by Core's clock, is refused before an order exists, `refused_by_policy`, and
  no provider is asked anything. Test `session_that_isnt_the_requesters_is_refused_before_the_reserve_is_asked`.
- **Should-fix 4, the request register was global.** Keyed by the requester's party and the request identifier, on
  the coordinator's ruling: another party's request under the same identifier opens its own order, and nothing
  anyone is told names another party's order. A request from an actor that doesn't act for the party it states
  opens nothing and records nothing (`NotTheRequester`). The party a request is kept under is its caller's, which a
  well-formed request has as its requester, so a malformed request can't occupy another party's identifier. Test
  `request_identifier_is_the_requesters_own`.
- **Should-fix 5, `open` appended before it kept the order.** The request's record, its order id and the entries its
  opening appends are kept first, and appended as built; the same request again carries them on under the same
  order. Test `open_whose_append_failed_keeps_its_order`; red as reviewed, a second order.
- **Should-fix 6, a split order's yes.** A `// T-020-01e:` note where the acceptance took the first part's yes,
  until core-internal's acceptance named each part's; closed in `T-020-03b`, below.
- **Nit.** The carry-on path doesn't resolve the destination again: policy permitted on the resolution made then.
- **A repeat by another actor of the party** (the coordinator's ruling, `REQ-OPS-5`). A repeated request is compared
  leaving out who acts for the party (the caller's `acts` and `kind`; its party and role are the key's and the
  gate's): the customer's agent repeating its person's request under the same identifier is told the same, and
  nothing is recorded. Another body is `Conflicting`, whoever sends it. Test
  `agent_repeating_its_persons_request_gets_the_same_answer`, red first (`Conflicting`); rule-off red.
- **Rule-off, each red:** a carry-on by any actor of the party; a stated party trusted, without `ActsFor` (on the
  carry-on and on the confirmation); an owner change released without naming the owner; a refusal of the caller
  ending the order; a retry rebuilding its entries; a refused append waited on; any session taken; one register for
  every party; told to anyone stating the party; and `open` appending before it keeps the order. The earlier
  rule-offs were run again on the new code, each red.

### T-020-03b, each part's yes (after `T-020-01e`)

Branch `f-020/t-020-03b-per-part-yes`, from `m3` at `2219de2` with `T-020-01e`'s contract change merged
(`d6f131c`).

- **The acceptance cites each part's yes.** core-internal's acceptance now names `yeses`, one `PartYes { part,
  origin_reserve, reserve_yes }` per part of the chosen route, in the plan's order, with distinct receipts. Intake
  builds the start's plan first, then the acceptance: each part of the plan, by its id, with the yes the rounds
  answered for it (`Offer::Offered`'s `reserve_yes`, in part order) and its origin reserve, the provider of the part's
  first move. The `// T-020-01e:` note is gone. A route the rounds offer with fewer yeses than parts, or with one yes
  for two parts (an even split at one reserve, answered by one question at one amount), would cite one statement
  twice, so it isn't shown: `no_route_right_now`. Asking each part its own question in that case is the rounds'
  (`T-020-06`), a hand-off. Test `split_orders_acceptance_names_each_parts_yes`, over a test router that splits the
  route `RouteFinder` finds into two parts of 600 and 400 (v0's router doesn't split, `DEC-8`), admitted by the real
  `admit_entry`; switched off (only the first part's yes, or the first part's for each), red.
- **A change of owner has its class.** A destination whose owner changed is recorded as a `destination_check` of
  `owner_changed` and then an `order_refusal` of `destination_owner_changed`, naming the order, the eighth class. The
  order still waits for the customer (`Wait::OwnerChanged`), whose acceptance naming the owner goes on as before.
  Test `change_of_owner_is_refused_with_its_class`; the refusal left out, red.
- **A half-accepted order is never started** (review nit). Where the log takes the acceptance and refuses the start,
  the order ends, and nothing acts on it: `T-020-08`'s segments follow only an order's recorded `route_chosen`
  (`Segments::follow`), and intake's new `Intake::start` answers the start the log took, none for such an order. The
  assembly is to feed `Segments::follow` from `Intake::start`, never from an acceptance (`T-024-02`). Test
  `half_accepted_order_is_never_started`, with a log that refuses the start once; the start counted before the log
  took it, red.
- **The ended order's class stays `no_route_right_now`** (review nit). Of the eight classes it is the least wrong for
  an order whose acceptance or start the log refused: the route as quoted can't run now, and a new request, with a
  fresh session and yes, may route. `malformed_request` would blame the request, `refused_by_policy` a decision
  policy never made, and the destination classes a destination that resolved. A class of its own is a core-internal
  bump, an item for `F-020`.
- **The fresh-identifier check restored in `open`** (review nit): an order id drawn fresh that an order already has
  is refused (`IntakeError::Fresh`) before anything is kept. No test draws a repeat: the tests' identifier source
  never repeats, and `rounds_support`'s `SameId` would repeat the transaction ids the rounds refuse first.
- **What the merge broke elsewhere, fixed.**
  - `T-020-08`'s scenario kit (`conformance/scenarios/tests/segments_support/`) built an automatic deposit's
    acceptance with no arrival, which `automatic-deposit-rests-on-its-arrival` now refuses. The seed appends the
    deposit's arrival first: the operator's question under its own transaction, the origin reserve's signed report
    answering it, and the `arrival` citing that report. The test whose operator states for the reserve's publisher
    (`segment_on_a_release_revoked_mid_flight_settles_on_its_result`) needs the release's acceptance in the log before
    that report, so the kit gains `Core::over_with`, which appends it first and signs as that operator; and the count
    of results kept once counts segments' results only, apart from the arrival's report.
  - `T-022-01`'s log tests (`components/core-audit/tests/log.rs`) replay the contract's world, which gained a second
    part's yes from `eu-bank`: `eu-bank` joins the providers the run admits, and the tests name the world's
    acceptance, its yes and its second effect by what they are, not by places the fixture moved.
- **Touches**: `components/core-orch/src/intake/`, its tests, the scenario kit and one scenario test of `T-020-08`'s,
  `T-022-01`'s log test, and these notes. No `DEC-20` path and no shared manifest.
- **Review of `e6682d1`.** The guard against a route with fewer yeses than parts, or one yes for two, has its test:
  `even_split_answered_by_one_yes_is_not_shown`, over `Splitter` at 500 and 500, where the rounds answer both parts'
  withdrawals with one question. The quote is refused `no_route_right_now`, nothing is shown, and policy is never
  asked; switched off, or with only its count half, red. And the acceptance and the start are built and checked
  before policy is asked, the authorisation's id drawn first, so an acceptance that fails its own shape costs no
  permit.

## T-020-05

- **Core's clock.** `snapshot::Clock` is Core's clock, read as a `Reading` to the nanosecond; `SystemClock` reads
  the host's, and the assembly builds `Snapshots::new` with it (its fields are private). Nothing in the module takes
  a time from its caller: `form` reads the clock once per snapshot, and `bind_statement`, `bind_operator_statement`,
  `record_refused` and `record_route_refusal` each read the clock they are handed. A grant is honoured only when in
  force at both whole seconds around the reading (the catalogue's snapshot at the second after, and a tuple kept
  only if it is also in the snapshot at the second before), so never before `granted_at` and never at or after
  `expires_at`; a verifier's grant is checked at both. Key checks use the second before (the skew rule is then the
  stricter); `taken_at` and record stamps the second after. The clock lives in this module because this row carries
  the inherited item; other rows reach it as `snapshot::Clock`.
- **Prices are an input.** The catalogue makes no price figure and no row of `M3` names one, so `form` takes
  Core's prices (`snapshot::Price`: tuple, fee formula, rate, settlement seconds, when stated) and writes each with
  its tuple's origin. A price whose tuple is out, that is dated after `taken_at`, or that the snapshot's contract
  or the bounds' check refuses stays out, with why. Reading: where prices come from is `T-020-07`'s stated terms;
  reversible.
- **Exposures (`REQ-ORD-8`).** `snapshot::Exposures` is Core's record of value in flight, per order and segment.
  A held operation counts as the router counts one against a ceiling: `amount_in` in its `from` asset, and an
  exchange's `amount_out` in its `to` asset too. Every segment of every part of every order adds up per provider
  and asset, so a split order's parts count together; the router sums a publisher's providers. Every provider with
  value in flight gets an exposure figure, whether or not it has a tuple in the snapshot, so an unroutable provider
  still counts against its publisher. Its publisher is the catalogue's, and a module the operator publishes (a
  holding account) counts under the publisher `operator`. Value at a provider no publisher stands behind is listed
  in `Formed::unattributed` and formation goes on: no route of the snapshot can reach that provider, and its order
  is the caller's to settle (after review: failing every formation let one hold stop all routing). A stuck order's
  holds stay until `release_order`, and `Exposures::stuck` reads them with no snapshot, clock or grant. Who holds
  and releases is `T-020-08`'s (a segment handed over, a segment accounted for); the exposure figure's source is
  `core-exposures` and its age 0.
- **The `calls` figure.** `snapshot::Releases` records each accepted release from its admission artefact; one not
  accepted is refused. A provider's calls figure is that of the release its tuple figures name by
  `module_version`, with their origin, aged from the release assertion's `made_at` (0 if after `taken_at`, as the
  catalogue does). A call for a holding balance, a call to the provider itself and a repeated callee are left out;
  a call left out is one no route names, so it fails closed.
- **Statements tied to keys (router-api's "Unchecked by either").** Statements enter as received bytes
  (`snapshot::Received`), never a re-serialised type. `bind_statement` runs `Catalogue::verify_statement` (the
  catalogue's deployment and protocol version, a key listed for its purpose by the advertisement in force at its
  `made_at` and by the one held now) on the statement and on every statement nested in it, at any depth, each by its
  own maker, over its own text exactly as it appears in the outer bytes. `snapshot/span.rs` finds that text by
  member names and list places (provider-role's `member` takes names only, and a decision record carries its
  provider statements in `snapshot.figures`, a list); it lives in core-orch, so no contract changes. A text it
  can't find refuses the statement (`NestedOutOfReach`), and one it misreads verifies as nothing. A maker the
  operator publishes, and `bind_operator_statement` (the recorded decision among the operator's own), go to
  `verify_operator_statement`, the deployment's `operator_key`. The snapshot's own figures nest no statement today;
  a decision record does, every dry run, fresh quote and judgement its snapshot carries, and a real `RouteFinder`
  record signed with the operator key is tied whole (after review: the first version refused every real record).
- **What else keeps a statement out.** A quote's or dry run's maker that isn't its tuple's provider; a tuple
  not in the snapshot; a verifier whose grant isn't in force for judgements at Core's clock, or whose origin is
  unknown (its accepted advertisement and its release accepted last give it, since a verifier has no tuple figure);
  a statement dated after `taken_at` though within the clock skew, whose age the contract can't state; and a
  figure the contract or the bounds' check against the snapshot refuses. Each statement is added on trial, so one
  bad statement is refused alone rather than failing the formation.
- **Refusals recorded.** Core-internal's table gives this row "a statement refused before the record, a route
  refused → stated entry `unverified`; `order_refusal`". `record_refused` keeps the bytes in the evidence store and
  appends a stated entry naming the maker its envelope names: `signature: verified` where its own signature tied
  it to that maker and it was refused for something else (not its tuple's provider, no tuple, dated after the
  snapshot, a contract rule; `RefusedStatement::verified`), `unverified` where it didn't (core-internal's README,
  "Two kinds of entry"). The maker acts for its publisher, for `operator` where the operator publishes it, or for
  itself where no publisher is known; a judgement in the verifier's role. `record_route_refusal` appends an
  `order_refusal`. `form` itself appends nothing, so a statement stays out even if the append fails. Who calls them
  is `T-020-06` (refused statements, before each round's record) and `T-020-08` (`check_part` before any segment is
  handed over, and the holds), whose `PLAN.md` rows say so since `T-020-05`'s review. `Formed` and `RouteRefused`
  are `#[must_use]`.
- **One adapter to the log.** Per the coordinator (the log's append surface split, `T-020-01b`), every append of
  this module goes through `snapshot/journal.rs`, and it writes no decision kind. Since m3 took the split, the
  adapter builds a `calls::GeneralEntry` with its `TryFrom`, which refuses the decision kinds, and appends it through
  orchestration's `calls::Log`.
- **The route guard (`REQ-ORD-16`, `REQ-ORD-36`, `REQ-ORD-37`).** `check_route` reads a route as received JSON,
  one part, so a contract type's own check on the way in can't mask Core's rule. Each rule has one owning check:
  typed and ordered (a non-empty list of the four kinds with two ends each, each starting where the one before
  ended), an asset changed only at an exchange, by its id as `check_typed_operation` compares it (a ramp or reserve
  may count it in other decimals at each end, `REQ-CAT-11`; after review), and a withdrawal from an outer node
  first and a deposit to one last; `check_typed_operation`'s node rules follow. A route received is refused as
  `malformed_request`; a part the router chose, through `check_part`, as `no_route_right_now`, since the router's
  output isn't the caller's request (after review). A route handed on under
  `REQ-ORD-43`, which starts at a holding account, is `T-020-13`'s; this guard refuses it until then.
- **Tests, and the order.** The tests were not written first: the code came first, and rule-off checks stand in
  for seeing each test fail first. Each key rule was switched off in turn and its tests seen red: the exposure figures (all three exposure tests), the clock read once per formation replaced by a
  fixed reading (the clock test), the key binding (both statement tests), the nested walk (both), the operator
  path (the operator test), parts summed per provider (the provider ceiling test), the asset rule, the withdrawal
  first, the deposit last, and the order (each its route test). A tenth test, beyond the row's list,
  `judgement_is_carried_only_while_its_verifiers_grant_is_in_force_at_cores_clock`, covers the eligibility path
  (the verifier's origin, its grant at Core's clock), and went red with the verifier's grant check switched off. Every snapshot a test forms is checked against
  router-api's `catalogue-snapshot` schema and round-trips. The ceiling tests run through `RouteFinder`.
- **Touches**: `Cargo.lock` and `components/core-orch/Cargo.toml` (this row's lines: `serde`, `serde_json`; dev
  `ed25519-dalek`, `jsonschema`, all in the lock already, so no new package and no `licences/RECORD` line).
  Signature checking is a `DEC-20` concern: this row's binding of statements to keys is in `T-025-05`'s scope.

### T-020-05, review answers

- **Six should-fix items and the nits** of the fresh reviewer, answered in the readings above: the asset compared
  by id; nested statements found in lists, and a real decision record tied; the guards' callers named in
  `T-020-06`'s and `T-020-08`'s rows (Work and Tests, `PLAN.md` edited on the coordinator's word) and `#[must_use]`;
  every time read from a `Clock`; the exchange's output held and counted; a refused but verified statement
  recorded as its maker's. Nits: value at an unknown publisher's provider stops no routing, the operator's modules
  count under `operator`; a grant honoured neither before its start nor at its expiry; `check_part`'s class.
- **New tests**: `decision_record_signed_by_the_operator_is_tied_with_every_statement_it_carries`,
  `exchange_held_in_flight_counts_in_what_it_gives_out_against_a_ceiling`,
  `value_at_a_provider_of_no_known_publisher_stops_no_other_route`,
  `grant_is_honoured_neither_before_its_start_nor_at_its_expiry`,
  `a_reading_inside_a_second_counts_as_both_whole_seconds_around_it`, and `span`'s unit test; existing tests gained
  a nested statement in a list, a verified refusal, units changing outside an exchange (allowed) and the chosen
  part's class.
- **Rule-off, each red**: decimals compared again (the asset test); lists not walked (the key test and the record
  test); the verified flag forced off, and the attribution forced unverified (the key test); the second-before check
  dropped, and the catalogue read at the second before (the start-and-expiry test); a fixed clock reading (both
  grant tests); the key binding (both statement tests); the exchange's output not held (the exchange test); the
  operator's modules unmapped (the unknown-publisher test); the chosen part's class as `malformed_request` (the
  ordered-list test).

### T-020-05, second review answers

- **`record_refused` checks the signature itself.** It verifies the refused statement's own signature again against
  the catalogue at Core's clock and attributes it by that alone; `RefusedStatement::verified` is informational and
  never read there, so no Core code can log `signature: verified` by setting a flag. Test:
  `refused_statement_claiming_its_signature_verified_is_recorded_by_its_own_signature`, red with the flag trusted.
  (`T-020-01c` is to have the log set a stated entry's attribution itself; this closes it here too.)
- **A member named twice** anywhere in a statement, nested ones included, is refused as `DuplicateMember` with its
  byte offset, before anything in the text is read; any other text the signing rule can't make canonical is
  `NotIJson`. Before, it surfaced as the catalogue's `Unreadable("deployment")`. Test:
  `statement_naming_a_member_twice_is_refused_before_the_record`, red with the check switched off.
- **The publisher name `operator`** is reserved and documented on `OPERATOR_PUBLISHER`: a catalogue publisher so
  named would be counted with the operator's modules against an `operator` ceiling, an over-count that only refuses
  more. The catalogue is `F-012`'s, so no refusal is added there.
- **`T-020-08`'s Serves** gains `REQ-ORD-36`, which its Work cites.

## T-020-06

- **One call, `rounds::route`.** It runs one routing of one order: the shortlist round, the questions, the choice
  round, and up to `LATER_ROUNDS` (3) later choice rounds, each formed through `snapshot::Snapshots` and each record
  appended as a `decision_record` entry chained to the order's previous one. It is generic over the `Router`, which
  the assembly gives as `RouteFinder`. What it reaches comes in `rounds::Parts`: the catalogue (by `&mut`, since balance
  checks go through it; a `Snapshots` is built from it for each formation), Core's clock, exposures and releases, the
  log, the evidence store, policy's `Charges` and `PersonQuestions`, and three seams of its own (`rounds/ports.rs`):
  `OperatorKey` (the deployment's operator key signs each question and each recorded decision; core-orch holds no
  private key and gains no dependency), `Providers` (a signed operation in, the answer's bytes out; the assembly backs
  it with `Platform::handle`), and `Fresh` (identifiers drawn at random by the assembly).
- **The order as an input, a reading.** Intake (`T-020-03`) isn't built, so the order comes as `rounds::Order`: its
  id, the caller whose work it is, its origin reserve, whether a user is present (`Presence::User { session }` or
  `Presence::AutomaticDeposit { session }`), the inputs it carries, and the judgements its tuples need. `History`
  (the last decision record's receipt, and who was asked about a person) and `Issued` (every transaction Core asked
  under) are kept by the caller across routings: `Issued` by the assembly, `rutent-scenarios` in `M3`
  (`T-024-02`) and the production assembly in `M4` (`F-030`), which must keep it durably, since a transaction reused
  after a restart would let two questions be linked. Reversible.
- **One adapter to the log** (`rounds/journal.rs`). Every stated entry the rounds write, each question and each answer,
  is appended `unverified { claimed_by }` with `about { order, transaction }`, its bytes kept first; the rounds never
  write `verified`. Every `decision_record` entry goes through it too. The switch to the log attributing a stated
  entry (`T-020-01c`) is a change there alone.
- **A question before its answer.** Each question is drawn under a fresh transaction, never one in `Issued` and never
  naming the order (a source that repeats one stops the routing as `RoutingError::Transaction`), signed with the
  operator key, appended about its order and transaction, and only then sent. Answers are appended once the round's
  snapshot is formed and before its record is: a statement the snapshot refused through `snapshot::record_refused`,
  with `about` naming the order and the transaction Core asked it under; every other through the adapter. A
  judgement, which names no transaction, goes about no order, as the in-flight core-internal README has it.
- **The questions.** Exactly `shortlist::asks` on the shortlist record: each shortlisted provider about each of its
  tuples once, at one amount. A question carries only what its step needs: its tuple's ends and amount, and of the
  order's inputs only those its tuple's terms require. All of a batch's questions, eligibility's included, are
  charged to the caller's accountable party as one `fan_out` charge before any is sent; over the bound none is, and
  the routing offers nothing (`NotOffered::OverBound`). Later batches are charged the same way.
- **The reserve's yes (`REQ-ORD-32`).** A withdrawal's dry run is a balance check: it goes out only once
  `Catalogue::begin_check` lets it, in the order's session, and its answer is read through `answer_check`. A yes is a
  `Verdict::Yes` to Core's own question, under the transaction Core asked it, with the user present. An answer that
  names another transaction (a yes the user carries, relayed in place of the reserve's answer) never enters a
  snapshot, and is kept on the record about the order and the transaction Core asked under. A route is offered only
  with a yes for each part's withdrawal, and `Offer::Offered` names each yes's receipt, which `T-020-03`'s
  acceptance cites, and the session. On an automatic deposit no yes is needed; its withdrawal's dry run still goes
  as a check, in the session Core opened for the deposit (the dry run's schema requires one). Reading, reversible.
- **`own_order` (`REQ-CAT-3`, `F-012`'s notes).** Set only for a withdrawal's dry run at the order's origin reserve,
  in the order's session; since the review, that is the only withdrawal asked, so every check a routing makes is
  the holder's own. Core computes it; nothing the caller passes sets it.
- **Eligibility, a reading.** No catalogue API says which verifier and class a tuple needs, so they come as
  `rounds::Need`s on the order. A need is asked only when its tuple is shortlisted: first to policy, as a question
  about a person on the shortlisted route (`ask` the relying provider, the authority the shortlist record's digest
  and its shortlisted providers, `asked_before` from `History`), and on `allowed` to the verifier, as a judgement
  request naming the subject reference and the class only. `past_bound` stops the routing with nothing offered
  (`REQ-CPL-5`); a refusal or `not_known` asks no verifier. A chosen operation whose tuple has a need and cites no
  judgement is not offered (`NotOffered::NotEligible`, `REQ-AUZ-5`). Reversible.
- **`refines`** is copied from the shortlist record Core keeps: its request id and its snapshot's `taken_at`, never
  read from the clock. Every later round refines the same shortlist.
- **`quote_voided`, emitted by the router** (since the review; see below). The first version kept the router's
  record verbatim and held the case as Core's own `rounds::Voided`, since `RouteFinder` emitted no `quote_voided` and
  Core may not rewrite the router's record (`AC-2`, `REQ-AUD-4`, `THR-10`). The router now emits it and
  `rounds::Voided` is gone.
- **Later rounds.** Where a choice round chooses nothing, every operation of a candidate it dropped as unconfirmed
  or for a voided quote, at
  a tuple the shortlist asked about, at an amount that tuple wasn't asked at, is asked again at its own amount, each
  its own transaction, and another choice round refines the same shortlist. A provider off the shortlist, or a
  tuple the shortlist didn't ask about, is never asked, whatever a choice round lists. At most `LATER_ROUNDS` later
  rounds, each charged.
- **A shortlist that keeps nothing can't be recorded, a contract gap.** core-internal's `What::DecisionRecord` names
  at least one shortlisted provider exactly when its round is the shortlist, so an empty shortlist has no entry: its
  record is returned signed with `receipt: None`, and the order's chain doesn't grow. A `no_route` input citing it
  would find no `decision_record` (`input-cites-its-decision-record`). A gap for a new `F-020` row with a
  core-internal bump.
- **The recorded decision** is router-api's `RecordedDecision`: the record in its envelope, made by the operator's
  party, signed with the operator key. Its digest is SHA-256 of its RFC 8785 canonical JSON, one function for the
  entry and for a question's shortlisted authority; `rutent_prov::module::wire::sha256` computes it, so no dependency
  is added. The signed bytes are returned in `rounds::Recorded`, for the caller to keep; where they are stored is
  `F-022`'s.
- **Timing correlation** stays a residual against `REQ-CAT-5`, as `PLAN.md` §4.4 has it: questions share no
  transaction and name no order, but a provider asked twice in close succession may line the two up.
- **For the switch to `T-020-01c` (hand-off).** `snapshot::record_refused`, `T-020-05`'s, writes
  `signature: verified` where a refused statement's own signature verifies; once a `GeneralEntry` refuses a stated
  entry its writer attributed, such a refusal fails to append and stops the routing. The rounds' own entries are
  already unverified. `record_refused` needs the same change as `rounds/journal.rs` when the branch lands.
- **Tests, and the order.** The code came first, so rule-off checks stand in for seeing each test fail first. Each
  key rule was switched off once and its test seen red: the check gate on a withdrawal (sent outside a session); the
  transaction filter on answers (the carried yes entered the choice snapshot); `own_order` set for every check;
  the voided reading; the asking again at the new amount; one transaction reused for every question; `Issued` not
  consulted; every candidate's providers asked, dropped ones included; needs not filtered by the shortlist (the
  person question's own shape rule then refuses it); no charge; no chaining; `refines.taken_at` from the clock; and
  refusals recorded after the record. The yes requirement backs the check gate: off alone, the gate still holds the
  offer; off with the gate, the route was offered without a yes. An eleventh test, beyond the row's list,
  `automatic_deposit_is_routed_without_a_users_yes`, covers `REQ-ORD-32`'s exception.
- **Touches**: no shared manifest and no dependency; only `components/core-orch/src/rounds/`, its tests
  (`tests/rounds.rs`, `tests/rounds_support/`, which reuses `T-020-05`'s `tests/snapshot_support/` unchanged), and
  these notes. No `DEC-20` path.

### T-020-06, review answers

- **B1, a route offered on a non-origin reserve's yes.** The yes must be the order's origin reserve's under
  `Presence::User`: `offer` requires each part's withdrawal to be at the origin reserve. And only the origin is
  asked (should-fix 1): every other reserve's withdrawal tuple is added to the routing's `excluded_tuples`, read from
  the shortlist's snapshot (`origin_only`), so the record shows the exclusion and recomputes; `ask` also skips any
  withdrawal question not at the origin. The reviewer's probe (two reserves, the origin refusing) is now part of
  `route_is_not_offered_without_the_reserves_yes_in_the_users_session`: no route, and the other reserve asked no
  withdrawal. Three guards back each other: the exclusion, `ask`'s skip of a withdrawal not at the origin, and the
  offer's origin check. Off alone, the offer's check stays green, since the exclusion keeps the other reserve off
  every route; the probe is offered on the other reserve's yes only with all three off (the exclusion and the skip
  off and the origin check on, the offer is still refused).
- **B2 and should-fix 3, judgements.** A judgement answer is admitted only if its maker is the need's verifier and
  its subject and class are the need's; any other never reaches a snapshot. A tuple with needs is offered only where
  its operation cites, for each need, a judgement by that verifier about that subject and class. The test kit gains
  verifiers (`Env::add_verifier`, from provider-role's fixture) that answer judgements; new test
  `tuple_needing_two_judgements_is_refused_while_one_verifier_is_silent`. Binding a judgement to a transaction stays
  a residual: a judgement request carries none.
- **Should-fix 2, who a person question asks.** A `// T-020-01d:` note at the call: `ask` and `asked_before` name
  the relying provider until core-internal's `PersonQuestion` may name the verifier on a shortlisted route.
- **Should-fix 4, the router's record checked.** A record whose request, snapshot or bounds aren't the round's own is
  refused as `RoutingError::NotItsRecord` before it is signed, recorded or acted on. New test
  `record_of_another_routing_is_refused_before_it_is_recorded`, with a router that echoes another order's shortlist.
- **Should-fix 5, `quote_voided` in the router.** `RouteFinder` now drops a choice-round candidate whose first
  unconfirmed operation has a usable fresh quote for its tuple, accepted for another amount and listing
  `amount_changes`, as `quote_voided { figure, condition: amount_changes }`, and that operation cites the quote (the
  contract's one exception to citing only quotes at an operation's amount). Edited: the router's `judge.rs`,
  `search.rs` (`Search::voided_quote`), `lib.rs` (`cite_voided`), `shortlist.rs`'s docs, and its shortlist test
  `provider_reached_at_two_amounts_is_asked_at_one_only`, which pinned `unconfirmed` for this case. `PLAN.md`'s
  `T-020-06` row names the router change, on the coordinator's word. No contract change: router-api 4.0.0 holds the
  reason. The same inputs now give another record, so `rutent_router::VERSION` is `rutent-router-0.5.0`, and the
  router-api fixtures naming the router's version (the valid record the router's own test compares with, and the five
  invalid ones made from it) name `0.5.0`, folded into router-api 4.0.0, still unreleased on `m3`, on the
  coordinator's word. As at `T-020-02`'s bump, the fixture's envelope signature is left as it was: no test verifies
  a fixture's signature. Touches a `DEC-20` path, `contracts/router-api/`.
- **Should-fix 6, `record_refused` at the attribution switch.** Waits for the coordinator's word that it has landed
  in `m3`; then m3 is merged here.
- **Nits.** A refusal is recorded once per answer, however many later rounds refuse it again. A judgement question is
  logged under a fresh transaction of its own, which the verifier's answer can't name (a residual for `T-025-05`). A
  count a charge can't hold is refused as `RoutingError::Uncountable`, never taken as within. Who keeps `Issued` is
  above.
- **Rule-off, each red:** the exclusion with `ask`'s skip (the probe's other reserve asked a withdrawal); all three
  origin guards together, the exclusion, the skip and the offer's origin check (the probe offered); the verifier, subject and class match at admission (the
  foreign judgement entered the snapshot); the per-need check at offer (any judgement stood for both); the record
  check (another order's record taken); the router's `quote_voided` (the record said `unconfirmed`); refusals
  recorded once (the refused statement kept twice).

### T-020-06, second review answers

- **A no for one class beside a yes for another.** router-api's agreement rule compared judgements by tuple and
  subject only, so a sanctions no beside a KYC yes for one person was refused from the snapshot, the router chose the
  candidate, and Core refused it at the offer: the record named a route never offered and never cited the no
  (`REQ-AUD-4`, `REQ-CPL-6`). The rule now compares tuple, subject and class (`CatalogueSnapshot::check`), folded
  into the unreleased router-api 4.0.0 with a changelog entry and a valid fixture,
  `catalogue-snapshot-judgements-of-two-classes-disagreeing`; the disagreeing fixture of one class stays invalid.
  The rule was the orchestrator's reading, changed on the coordinator's word. New test
  `no_for_one_class_beside_a_yes_for_another_drops_the_candidate_citing_the_no`, red with the class left out of the
  comparison. Touches a `DEC-20` path, `contracts/router-api/` and `contracts/src/router_api/`.
- **A judgement question is logged about nothing**, not under a transaction its body doesn't name, so the log can
  verify what it attributes; `Sent::transaction` is `None` for it.
- **The rule-off account above** now says that the probe is offered only with all three origin guards off.
- **`T-020-01d`** (a person question names its verifiers, the shortlist record carries them) waits for the merge of
  the attribution branch into `m3`; then `ask`, `asked_before` and the shortlist record's `verifiers` come from the
  tuples' needs, and the `// T-020-01d:` note goes.

### T-020-06, after the attribution branch (`m3` at `b5f64e5`)

- **Merged** `m3` at `b5f64e5`. `T-020-06`'s `PLAN.md` row took the merge's wording of the question-before-answer
  rule and of `record_refused` (appended unverified, the log setting its attribution), with this row's review
  additions kept. Every rounds test passed on the new `record_refused`.
- **`T-020-01d`.** A person question on a shortlisted route asks the verifier its need names, which receives the
  subject; `asked_before` lists the verifiers already asked for the order (`History`); the authority and the
  shortlist's `decision_record` entry name the verifiers the shortlisted tuples need, from the order's needs. The
  `// T-020-01d:` note is gone. Each of the three was switched off once and its test seen red.
- **A shortlist that keeps nothing is recorded now**, its entry naming no provider, so a `no_route` input has the
  record to cite; `Recorded::receipt` is no longer optional. The gap above is closed.
- **Questions and answers bind through the log.** Core signs its questions and records as the operator's `acts`,
  the maker `StatementKeys::operator` names, and the stated entry claims that actor. Each answer is appended with
  the transaction Core asked it under in its `about`. The tests now replay the log as the log would
  (`Env::admitted`): each stated entry through core-internal's `attribute` over the kept bytes and the catalogue's
  keys, and every entry through `admit_entry`, so `answer-follows-its-question`, `transaction-belongs-to-one-order`
  and the decision records' chain hold on what the rounds write. There, a question is verified as the operator's, the
  reserve's yes as the reserve's, and a carried yes, whose bytes sign another transaction, as no one's. Switched off,
  a question signed under the operator's party, and answers appended without their transaction, each went red.
- **Superseded above:** the hand-off on `record_refused` (it appends unverified since the merge) and the empty
  shortlist's gap.

### T-020-06, third review answers

- **An empty shortlist, pinned.** `shortlist_that_keeps_nothing_is_recorded_and_asks_no_one`: with the only reserve
  excluded, one shortlist entry names no provider and no verifier, `History::previous` is its receipt, no one is
  asked or charged, and the replayed log admits it.
- **The record's maker, pinned.** `recorded_decision_is_the_operators_own_statement`: each recorded decision's
  envelope is made by the operator's `acts` and signed with the operator key, and the catalogue ties it to the
  deployment's `operator_key`. The test names Core's own party apart from the operator's maker, since the kit's
  Core acts for `operator`, the same text, which left the reviewer's switch to `routing.core.party` green; with them
  apart, that switch goes red.
- **What `Env::admitted` leaves out**, said in its doc too: policy's question and charge entries go to the fakes, so
  the rows over them (`provider-asked-before-was-asked`, `asked-before-is-complete`,
  `shortlisted-authority-cites-its-shortlist`) aren't exercised; every receipt the tests' log hands out shares one
  digest; and `signs_for` always answers true, the key's tie to its maker resting on the catalogue's verification.

## T-020-08

- **One call per part, `Segments::run_part`.** It runs one part of an order's chosen route, segment after segment,
  while each settles: the route guard, then for each segment policy's decision, its effect, the platform, the drains,
  the result and the segment's one outcome. `Segments::hand_over` runs one segment at an explicit amount. What it
  reaches comes in `segments::Parts`: Core's clock, the catalogue (results are attributed by its keys), the log, the
  evidence store, policy's `Decisions`, and three seams of its own (`segments/ports.rs`): `Modules` (the platform:
  begin and end a segment, name and end its calls, carry an operation out, drain events, the staged version and
  promotion), implemented for `Platform<W>`; `Uses` (the proxy's `take_uses`), implemented for `CredentialProxy`,
  with `NoProxy` for a world that resolves nothing through one, as the simulated providers' does; and `Grading`
  (below). It reuses the rounds' `OperatorKey`, `Fresh` and `Issued` through their public API. `Segments` is Core's
  record across orders (effects acted on, segments handed over, transactions assigned, items not yet appended,
  segments in flight, orders followed), which the production assembly keeps durably (`F-030`), as `Issued`.
- **The platform only on an effect.** `Modules::carry_out` and `Modules::promote` take the `Effect` they carry out,
  so the entry-point scan holds them to a decision; neither is named after a method the scan watches, and the
  `Platform` implementation hands its effect to nothing: holding it is the rule.
- **The order, for each segment** (`segments/mod.rs`): the hand-over refused if the segment was handed over before;
  a fresh transaction, never one the rounds issued nor one a segment ran under; the provider-role operation,
  `exact_in` at the amount moved, carrying only the inputs its tuple's terms require, a withdrawal in the user's
  session; `decide` of a `move_value`, and nothing more unless it permits; the operation signed with the operator key
  and appended about its order, segment and transaction (so `answer-follows-its-question` holds the result to it);
  the hold; the `effect` entry and its receipt; acted on once for that receipt; `begin_segment`; the calls named;
  `provider_bound`; the operation carried out; the events and uses drained and handed over; the result kept and
  appended as the provider's stated entry; judged; the outcome.
- **Acting once (`INV-8`).** An append the log can't take now is tried again with the same entry, `APPEND_TRIES` (3)
  times in all, and the log's answer to an equal entry is its first receipt. Core acts on each effect's receipt once
  (`Segments::acted_on`), and hands each segment over once (`AlreadyHandedOver`), so a retried append, or a caller
  that runs a part again, moves nothing twice. A hold already there is taken as this segment's, from a try that
  stopped before its effect.
- **What counts (`judge`, `REQ-ORD-1`, `INV-1`)**, in order: the catalogue's `verify_statement` over the bytes (keys
  the maker listed for statements at its `made_at` and now, this deployment and protocol version); the maker is the
  segment's provider; the result is of the operation's kind and transaction and isn't a dry-run answer;
  `check_amounts` against the tuple's terms, read from the figures the chosen operation cites. A result that doesn't
  count is still kept and appended as its maker's stated entry, unverified (the log attributes it), and is no result:
  no `result` entry, no input, the hold kept and the segment in flight, waiting for one that counts.
- **The outcomes, a reading.** Settled: the evidence `Grading` records, `segment_settled` citing it, `result: done`,
  the settlement time, the segment ended and released. Failed: `result: failed`, `segment_failed`, the hold kept for
  the reversal (`T-020-12`). The withdrawal failed or refused (final or lifting at a known time): `result: failed`,
  `withdrawal_cancelled` for the part, the hold released, no later segment handed over (`REQ-ORD-40`). A later
  segment refused that way: as failed. Outcome unknown: `segment_outcome_unknown`, no `result` yet (an effect has one
  result, and this one isn't final), the hold kept and the segment in flight. Waits on the person: no outcome yet,
  the provider's statement handed back labelled as its word. The platform refusing before the module passed anything
  out: `result: unanswered`, the segment ended and released, nothing moved. Reversible.
- **Outcome unknown only on stated evidence.** The provider's own `unknown`, in a result that counts, or the
  platform's `OutcomeUnknown`, which it gives only once the module passed something out: what it passed out is in
  the drain handed over with it. A module silent before passing anything out, a forged or mismatched result, and a
  result whose amounts break leave no outcome unknown.
- **The amount the next segment moves (`REQ-ORD-21`)** is what the one before gave out by its result that counted,
  `amount_out`, never the quote. Until `T-020-09` grades evidence, that result is the evidence at the node; a
  shortfall is the earlier segment's, and the next is handed what arrived. The first segment moves the chosen
  route's `amount_in`. The `exact_out` remainder needs no rule here yet: Core writes every operation `exact_in`.
- **Holds (`REQ-ORD-8`).** Taken just before the effect, from the chosen operation as the router counted it (its
  quoted amounts, which over-count a short delivery and so only refuse more), released when the segment is settled,
  its withdrawal cancelled, or it was never handed over; kept otherwise, for `T-020-11` and `T-020-12` to release.
- **Hand-over (`REQ-MDL-8`, `REQ-MDL-6`, `REQ-MDL-5`).** One drain per `take_events`, one per `take_uses`, one per
  segment record, each a fresh id, each item at its place. Items go to a queue first and are appended in order; an
  append that fails keeps the rest whole for the next drain or `Segments::flush`, never drained again. prov's enums
  map to tokens with exhaustive matches (`segments/drain.rs`). A segment's record is handed over when it ends, on a
  final outcome.
- **Gap: `Events::unknown`.** core-internal's `HandedOver` has no item for an operation that ended with no answer, so
  it is carried into the segment's outcome (`UnknownBy::Platform`) and not appended. A new `F-020` row with a
  core-internal bump; with it, an outcome unknown could cite the platform's own record.
- **Gap: no approval place in provider-role.** provider-role 3.0.0 has no field naming a place a provider sends a
  person to approve; the nearest is a `waits_on_person` refusal, with no detail. Core hands it back as
  `ProvidersStatement` (its maker, its stated entry's receipt, its bytes as the platform checked them against the
  result schema, `REQ-MDL-12`), and an altered one counts as no one's. A place with an address is a provider-role
  bump; this label carries it unchanged.
- **Evidence until `T-020-09` (`Grading`, a seam).** `ProvidersWord` writes an `evidence` entry of grade 4 by the
  provider, resting on the result's stated entry, which `segment_settled` cites, so `route-starts-on-the-withdrawal-proof`
  and `evidence-rests-on-statements` hold. A statement alone closes no order (`REQ-ORD-33`): `T-020-09` replaces it
  through the same trait.
- **The calls (`REQ-MDL-21`).** Named from the chosen record at hand-over (`RouteCalls::name_segment`), against the
  segment's id, for its transaction; a record that can't name them names none, which fails closed; ended with the
  segment.
- **State and age (`REQ-ORD-5`).** `Segments::follow` takes the order's `route_chosen` as recorded and opens the
  order in `rutent-stm`; each input the segments record moves it, and the time it reached its state is kept.
  `Segments::status` answers its state, each part's, when it reached it and its age, only on a read check policy
  allows, recorded either way. Age is the time in the current state. Reversible.
- **Promotion (`REQ-AUZ-6`, `REQ-MDL-4`; `T-021-03`'s hand-off).** `Segments::promote` takes the effect and the
  decision request it rests on: the effect must be the request's, a `change_catalogue: promote` about one release,
  naming the version the platform has staged now, and not acted on before; the platform's `by` is the permitting
  decision's caller. Written before `T-021-03` reached `m3` with no change promoting, and opened on `promote` when
  it did.
- **The caller.** `Live::caller` asks policy for each move: Core carrying the user's authorisation onward, or the
  operator where the operator starts the move (`T-021-03`'s hand-off). The tests' policy is a fake that appends
  each decision through the policy path, so `T-021-03`'s register inputs (`ActsFor`, `CarryOnward`) aren't asked
  here; the assembly's engine asks them (`T-024-02`).
- **Tests.** Fifteen of the row's seventeen on the running platform with the simulated providers
  (`conformance/scenarios/tests/segments.rs`, kit in `segments_support/`), one beyond it
  (`move_policy_does_not_permit_hands_nothing_over`), and the other two in `components/core-orch/tests/segments.rs` with fakes, beside a second record test,
  `segment_record_reaches_the_log_with_each_move_when_it_ends`, whose segment moved off a version marked stale (the
  simulated providers are admitted at one version, so the platform test's records carry no move):
  `call_the_live_segment_doesnt_name_is_refused`, since no simulated release declares a call and no simulated
  provider's nodes admit a route across two providers, and `promotion_whose_staged_version_differs_from_its_permit_is_refused`.
  The route is the router's (`RouteFinder`) over a snapshot formed through `snapshot::Snapshots` from a catalogue of
  the simulated reserve's advertisement and its own dry runs. Results the providers can't be scripted to give
  (amounts that break, another provider's, another transaction's, a failed withdrawal) are the platform's result
  changed on its way back and signed again with the key the module signs with. The kit's log replays every entry
  through core-internal's `attribute` and `admit_entry` on the path it came by; what it leaves out is in
  `Core::admitted`'s doc (a receipt is of the entry as handed in; `signs_for` answers true; the operator's
  statements verify under the real key the deployment's placeholder `operator_key` stands for, the kit's
  convention). It seeds the authorisation, an automatic deposit's acceptance, the decision record and `route_chosen`.
- **Touches**: `conformance/scenarios/Cargo.toml` (dev-dependencies `rutent-core-orch`, `rutent-router`,
  `ed25519-dalek`, all in the lock already) and `Cargo.lock`; no `licences/RECORD` line, no new package. No `DEC-20`
  path.
- **Rule-off, each red.** The code came first, so each rule was switched off once and its test seen red, then
  restored: the route guard (the guard test); a segment handed over once (the retried-effect test); the result's
  signature, its maker and its amounts (the approval test's altered response, the other-provider test, the amounts
  test); the withdrawal cancelling the part (the failed-withdrawal test); the next amount from the result (the
  next-segment test); undelivered items kept (the outbound-call test); the live segment's calls named (the calls
  test); the platform's outcome unknown (the outcome-unknown test); the proxy's uses drained (the secret-use test);
  the segment record handed over (the record test); its moves carried (the record-with-a-move test); the read checked by policy (the state-and-age test); the hold
  (the held-in-flight test); no effect without its record (the retried-effect test's log that can't take the
  effect); the actual settlement time (the settlement test); policy's permit required (the not-permitted test); and,
  for promotion, the staged version compared and the effect acted on once (the promotion test). The explicit
  transaction check in `judge` wasn't switched off alone: `check_amounts` refuses another transaction too.

### T-020-08, review answers

- **Merged** `m3` at 74bb7b1 (fast-forward, before `ab48413`) and at bd9e1d5 (a merge commit): `T-021-03`'s roles,
  and core-internal's grant and release keys, with `attribute` taking the history.
- **B1, what a result gives out.** `check_amounts` holds what a segment takes in, never what it gives out, so a
  result signed by its provider could claim more out than in and hand the next segment the surplus. `judge` now
  refuses it (`NotCounted::GivesOutMore`): a segment that keeps its asset gives out at most what it took in; a
  conversion at most what the fresh quote its operation cites gives for what it took in, moved by no more than that
  quote's slippage (0 for a firm rate); a conversion that cites no such quote, or a floating one, has no bound and
  doesn't count. The hold is taken at the amount the segment moves (its operation's `amount_in`; an exchange's out
  side by the route's own rate, rounded up), not the quote. Test `result_giving_out_more_than_it_took_in_does_not_count`
  (`REQ-ORD-21`, `INV-1`); the held-in-flight test now holds the deposit at what arrived after a short withdrawal.
- **B2, nothing that came back is lost.** Each segment is kept in `Segments` from its permit to its final outcome,
  with everything it needs to go on: its signed operation, its effect, whether it began and was carried out, the
  answer as the platform gave it, when it started, and each of its entries, built once and appended unchanged until
  the log gives its receipt. The drains are taken and kept whole, as taken (`Events`, the uses, the segment record),
  before any step that can fail, and are written as entries only when they are appended, from the first item not yet
  appended. A step that fails returns its error with all of that kept; running the part again resumes the segment
  where it stopped, carrying nothing out twice and appending each entry once. A segment whose outcome is final is
  refused as handed over; one whose outcome isn't (no result, unknown, waiting on the person) is taken on from what
  it kept. Test `what_comes_back_is_kept_when_a_later_step_fails`, over the result's stated entry, the
  provider-bound input and the segment record; `drain_the_log_cant_take_is_kept_whole_and_answered`.
- **Should-fix answered.**
  1. `hand_over` is private.
  2. Promotion: `Segments::promote` asks the decision itself, from a `Promotion` (provider, version, caller,
     Core's actor), so the permitting decision's caller is the request Core sent and the platform's `by`; it
     refuses a version that isn't the staged one before policy is asked, appends the effect, compares again, calls
     the platform, appends the `result` (`done` or `failed`), and marks the effect acted on only once the platform
     put the release in force.
  3. The receipt rule alone: `effect_receipt_acted_on_before_is_not_acted_on_again`, a log answering the deposit's
     effect with the withdrawal's receipt. The restart gap is `F-030`'s: a fresh `Segments` knows nothing of the
     effects acted on before it, so only the production assembly's durable copy stops a second `move_value` on a
     segment after a restart.
  4. An answer that can't be serialised after `Handled::Answered` is `OutcomeUnknown` (no test: serde can't be made
     to fail on a statement the platform built).
  5. A failed `provider_bound` append leaves the segment begun and in flight with its operation kept; the next run
     appends it and goes on (in the B2 test).
  6. "Done when": a second release of the simulated reserve now runs on the platform. The promotion test promotes
     it, staged beside 1.0.0; the record test moves the withdrawal off 1.0.0, marked stale mid-flight after 1.1.0 is
     put in force, so its record carries the move. The calls test stays with fakes: no simulated provider's nodes admit
     a route across two providers, which a call needs a callee on, and adding one is `T-024-01`'s kit; the module
     scenarios that run cross-module calls are `T-024-07`'s (`AC-20`). `impl Uses for CredentialProxy` isn't run for
     real: the simulated world resolves nothing through a proxy, and the assembly with the production world and its
     proxy is `T-024-02`'s; the secret-use test feeds `Use` values through the seam.
  7. `Events::unknown` is kept with the segment in flight (`Segments::unknown_outcomes`), and core-internal's README
     hand-offs say an item for it needs a bump.
- **Nits.** `flush` answers a drain the contract can't carry, or an entry the log refuses, as the error it is, every
  time, holding back only the drains after it. A settled segment that gave out nothing ends the part with an
  `order_refusal` of `no_route_for_amount` (test `segment_settling_with_nothing_given_out_ends_the_part_with_its_refusal`).
  An effect whose answers were all lost is appended again unchanged on the next run, not a second effect (test
  `effect_whose_answers_were_lost_is_appended_again_unchanged`).
- **The record before the result (`REQ-MDL-20`, on the coordinator's word).** A final outcome ends the segment and
  hands its record over before its result's stated entry is appended, so the log attributes the result knowing the
  release the segment ran on: a release revoked mid-flight keeps its key only for segments whose record shows they
  ran on it. `PLAN.md`'s row says so. Test `segment_on_a_release_revoked_mid_flight_settles_on_its_result`: the kit
  makes the operator state for the reserve's publisher, so the log verifies the reserve's results by the releases it
  accepted, as for a module the operator publishes; its 1.0.0 is accepted, then revoked mid-flight, and the
  withdrawal's result is still verified. The catalogue refuses to publish a provider with a publisher as the
  operator's, so the kit's log view does it instead. Judging a module the operator publishes by its release's key,
  in Core's own `judge`, which goes through the catalogue's advertisements, is open: no route segment runs through one
  yet (`T-020-13`'s hand-on is the first that will).
- **Policy in the tests.** The tests' policy is now `CMP-CORE-AUTHZ`'s engine itself, its fixed rules first, over a
  deployment that permits and registers that answer `ActsFor` for Core's actor, the customer and the operator,
  `CarryOnward` for Core's actor and promotion for the operator. So every move is decided as the assembly will decide
  it. One consequence: under `T-021-02`'s default the operator reads an order only by a permission policy holds for
  it, and core-internal grants permissions only to regulators and checkers, so the operator's half of `REQ-ORD-5` is
  refused, recorded, until the owner's form rules otherwise; `Segments::status` takes the reader's permission for
  when it does. The state-and-age test pins that.
- **Rule-off for the review's rules, each red:** the surplus rule (the surplus test); the hold at the amount moved
  (the held test); the answer kept and not carried out again, and the result's stated entry built once (the B2 test,
  whose log takes each entry and loses its answer); the segment resumed rather than decided again (the
  lost-answers test); the receipt rule (the replayed-receipt test); the record before the result (the
  revoked-release test); nothing given out recorded (its test); the staged version compared before policy (the
  promotion test); `Events::unknown` kept (the outcome-unknown test); drain errors answered (the drain test).

### T-020-08, after the advisor's review of 7d16098

- **One drain holds up no other.** `flush` tries every kept drain, each from its first item not yet appended,
  keeps each that fails where it is, and answers the first failure; it no longer stops at the first. A final result
  waits only for its own segment's record drain, and a segment's accounting is stopped only by an error in its own
  drains (the log not taking them now stops nothing). Test `drain_the_log_cant_take_holds_up_no_other_segment`: one
  order's bad drain stays kept and answered while a second order's segments settle, each record before its result.
  The kit now runs more than one order over the route (`Named`, `Core::take_up`).
- **The conversion bound includes the operator's limit.** `sign::cited` folds the operator's slippage limit for the
  tuple (from the record's bounds, matched on provider and ends as the router's `commitments` does) into the cited
  quote's slippage, the tighter of the two; with a floating rate the operator's limit alone bounds it. The bound is a
  pure function (`judge::conversion_bound`, `judge::converts_within`), unit-tested over a firm rate, a bounded one,
  the operator's tighter limit, a floating one, no quote, at the bound and one over. No simulated route has an
  exchange, so the platform tests don't reach it.
- **Rule-off, each red:** a record waiting for every drain, not its own (the no-stall test); any drain's error
  stopping a segment (the no-stall test); the conversion bound (its unit test); the operator's limit where tighter
  (`sign`'s unit test).
- **Open, for the coordinator:** `REQ-ORD-5`'s operator half against `T-021-02`'s default that the operator reads
  only by a permission, which core-internal grants only to regulators and checkers (the owner's form); Core's
  `judge` counting a result from a module the operator publishes, which has no advertisement (first needed by
  `T-020-13`'s hand-on).

### T-020-08, review answers on 00f4599

- **N1, a part resumes past a settled segment.** `run_part` takes a segment whose outcome is final as it stands
  (`Segments::standing`) and goes on from the next with what it gave out, so a run that stopped on the deposit, at
  its effect or its result, resumes there. A part run again once all is final answers its outcomes as they stand,
  asking and moving nothing, rather than refusing the first segment as handed over; `hand_over` still refuses one
  that is, should anything reach it so. The zero-amount refusal is recorded only when the settlement is fresh. Test
  `part_resumes_after_its_first_segment_settled`, over a fault on the deposit's effect and on its result, written
  first and seen red (`AlreadyHandedOver("p1-s1")`).
- **N2, a refused begin stays refused.** The platform's refusal to begin a segment is kept with the segment before
  its `unanswered` result is appended, so a run that stopped there finishes it as refused, never begins it again,
  and never settles it under a result built as `unanswered`. Each result step is keyed to its outcome
  (`result: done`, `result: failed`, `result: unanswered`), so an entry built for one outcome is never appended for
  another. Test `segment_the_platform_wouldnt_begin_is_finished_as_refused`, a platform refusing the deposit's begin
  once and the log refusing its result every try; written first, seen red.
- **The first verdict stands.** The verdict on what a segment's operation gave back is kept with the segment, so a
  later try reads it, not a new judgement at a later clock under keys that may have changed. Test
  `first_verdict_on_a_result_stands_when_its_key_is_retired_between_tries`.
- **The charge as the tuple reports it** (`REQ-CAT-11`). For a segment that keeps its asset, `judge::handed_on`
  reads the tuple's `reporting`: under `net`, what it gave out and its charge together are at most what it took in,
  and what it gave out is handed on; under `gross`, what it gave out is at most what it took in, and what it gave out
  less its charge is handed on. `Counted::Succeeded` carries both, and `Settled::amount_out` is what is handed on. A
  conversion's charge is in the asset it takes in, so its output is handed on as it is. Tests:
  `judge`'s unit test, and `next_segment_is_handed_what_arrived_net_of_the_charge_as_the_tuple_reports_it`, with a
  charge of 1 000 under each reporting. **Hand-off to `T-020-07`:** the router doesn't read `reporting` either, so
  what a route prices for the next segment under `gross` terms includes the charge; pricing should take it off.
- **`REQ-ORD-5`, split until `T-021-02b`.** `orders_state_and_age_are_readable_by_its_customer` asserts the
  customer's read; `operators_read_of_state_and_age_waits_on_its_read_kind` asserts today's refusal of the
  operator's, recorded, with a `// T-021-02b:` note to fold it back once policy has a read kind the operator holds.
- **Nits.** The zero-amount refusal's reason names the next segment's place and that nothing arrived for it to move
  (`RouteRefusal::Malformed`, which carries a place and a reason; the log records the class alone). The effect is
  marked acted on after Core's clock is read, so a clock that can't be read leaves it unacted.
- **The README exception.** `PLAN.md` §4.4 gives `contracts/` to `T-020-01` and `T-020-02` alone in `M3`. The
  hand-off line in `contracts/core-internal/README.md` (an operation that ended with no answer has no `HandedOver`
  item) is this row's one exception, made on the coordinator's word because the gap is this row's to report and
  the contract's hand-offs are where a later row reads it; it changes no type, schema or fixture, and the item itself
  is a new `F-020` row with a core-internal bump.
- **Rule-off, each red:** the standing segments skipped (the resume test); the begin refusal kept (the refused-begin
  test); the first verdict kept (the verdict test); `net`'s charge counted within what came in, and `gross`'s
  charge taken off what is handed on (the reporting test). Keying each result step to its outcome is defence in
  depth behind the kept begin refusal, which no test reaches alone.

### T-020-08, the reviewer's last two nits on 73ff352

- **A refusal owed after a settled segment is recorded on resume.** The `no_route_for_amount` refusal a part owes
  when a segment settles with nothing given out is now that segment's own step, built once and appended until the
  log takes it, and a run that takes the settled segment as it stands appends it if it is still owed. Before, a
  re-run skipped the settled segment and the refusal was never recorded. Test
  `refusal_owed_after_a_settled_segment_is_recorded_on_resume`, the refusal's append failing every try of the first
  run; written first, seen red. The refusal is built in this module now, not through `record_route_refusal`, so it
  carries no `RouteRefusal` reason: the log records the class alone, as before.
- **A conversion under `gross` hands on its output as it is: the reading kept, not a refusal.** A conversion's charge
  is in the asset it takes in, never the one it gives out, so under `gross` or `net` alike it can't be taken off
  what the conversion gives out, an amount in another asset. What it gives out is already limited, for what it took
  in, by its cited quote and the tighter of the quote's slippage and the operator's limit, which hold whatever the
  terms report. Refusing `gross` conversions would refuse every exchange that reports gross with nothing gained.
  Reversible.

## T-020-10

- **One call, `Recalculations::recalculate`** (`components/core-orch/src/recalc/`). It recalculates one part of an
  order: where the value is evidenced, the bound, a routing through `rounds::route` with the request's `follows`
  naming the choice round it follows, Core's check of the router's choice, and the `route_recalculated` input. What it
  reaches comes in `recalc::Parts` (the rounds' parts, the exposures by `&mut`, and `Segments`); what it routes, in an
  `Ask` (the order as the rounds take it, the part, the authorisation, the objective, bounds and prices, Core's and
  the operator's actors). `Recalculations` is Core's record across orders (the choice round each order follows, its
  count, what was handed over and never settled, a recalculation not yet appended, the parts waiting), which the
  production assembly keeps durably (`F-030`), as `Segments`.
- **Only from evidenced value** (`REQ-ORD-2`, `INV-4`). `recalc::evidenced` reads the order Core follows
  (`Segments::order`): the value is evidenced at the start of the part's first segment that hasn't settled, where
  that segment hasn't started, was reversed, or is a deposit that failed, the segment before it settled there, and
  the state machine's `value_at` agrees. A segment started (silent, no result that counts, waiting on the person),
  failed with its reversal owed, or of unknown outcome holds it in flight, and a stopped advertisement or a stale
  version changes none of that, so none of them releases a route: nothing is asked and nothing recorded. The amount
  is what the settled segment handed on (`Segments::handed_on`), all of which the new route moves. A part waiting
  for the customer or stuck isn't recalculated: the customer or the operator resolves it. A failed middle segment is
  recalculated only after its `reversal_completed` (`T-020-12`). Reading, reversible.
- **The bound** (`REQ-ORD-4`). `RECALCULATIONS` (3) per order, counted on recalculations recorded. Past it,
  `Recalculation::NoRouteLeft`, with nothing asked or recorded: core-internal's README puts the `no_route` input
  and the stuck state under `T-020-11`, which takes this outcome. A routing that offers nothing for another reason
  than the floor is `Recalculation::NoRoute`, for the same row, and a route Core refuses is `Rejected`; after
  either, the part is routed no more (`NoRouteLeft`), so providers that refuse their dry runs in turn can't make
  Core ask without end (`THR-28`; on the advisor's review). Readings, reversible.
- **A provider passed again without progress** (`REQ-ORD-4`, `THR-28`): a provider on the new route that had a
  segment of the part handed over (`provider_bound`) from the node the value still sits at, which never settled.
  `Recalculated::passed_again` names it with those segments, and `recalc::passed_again` reads the same from the
  order's recorded inputs alone (its plans, `provider_bound` and `segment_settled`), so it is visible from the log,
  which the platform test compares. No `Raise` difference fits it and core-internal's home for it is the
  `route_recalculated` input and the `decision_record`, so it is raised to no one: whether it should reach the
  operator is a question for a later row.
- **At the user's cost; the failed provider covers only the return** (`REQ-ORD-22`, `INV-2`). The new route moves
  everything evidenced at the node, so nothing of the failed segment's value is charged to anyone but by the new
  route's own charges, which come off what it delivers. A failed deposit leaves the value at its node; a failed
  middle segment's return is its reversal, in full (`T-020-12`). The failed segment's hold is released once the
  new route is recorded, so the value counts once in flight (`REQ-ORD-8`).
- **Inside the authorisation** (`REQ-AUZ-3`, `INV-10`). `recalc::check_route` holds the router's choice against
  the authorisation, never its own record: one part (the routing runs with `max_parts` 1), from where the value is
  evidenced, moving all of it, ending in a deposit at the authorisation's destination reserve and node, in its to
  asset, leaving the floor after the operator's fee. Then the state machine's own rules run on the plan, and the
  transition is tried before anything is appended. Each move on the new route carries a `RouteEnd` read from the
  route itself (`recalc::route_end`: its last deposit's reserve and outer node, the asset it delivers, `delivers`
  after the operator's fee; only the account is the authorisation's, since a route names none) to policy's engine
  (`F-021`), so the log's `move-rests-on-its-authorisation` holds the route's own ends against the authorisation, a
  third guard (on the advisor's review: built from the authorisation, it could never refuse). The simulated
  providers can't route to another destination, so that third guard is shown by core-internal's own fixtures
  (`move-rests-on-its-authorisation--destination`, `--to_asset`) and the `INV-10` property's check of `route_end`,
  not by a platform scenario. A recalculation names no new authorisation and no acceptance.
- **The floor** (`REQ-ORD-27`, `AC-42`, `REQ-ORD-4`). At a recalculation, the request carries the authorisation's
  floor, the router drops what falls below it, and a routing whose every candidate was dropped `below_floor` makes
  the part wait (`Recalculation::WaitsForCustomer`), the routing's decision record showing why. At each settled
  segment of any part, `Segments::run_part` checks the floor `Live::floor` carries (since the review: the one way a
  part runs, so the check can't be skipped): `recalc::left_after` projects what the quotes still ahead leave the customer after the operator's fee, from what
  the settled segment handed on (an operation keeping its asset and unit takes off what its quote charged; one
  converting or counting in another unit gives out in its quote's proportion, rounded down; the operator's fee by
  `quote::received_after_operator_fee`, the router's own), and below the floor nothing more is handed over and the
  part waits (`Segments::waiting`), however often it is run. A projection Core can't count holds no floor.
  Readings, reversible. The assembly (`T-024-02`) fills `Live::floor` from the order's authorisation; the log can't
  hold the floor between segments, since a chosen route's `Live::route.delivers` is its quote.
- **Gap: waiting past the floor has no state-machine input.** state-machine 2.1.0 puts a part in
  `waiting_for_customer` only by `no_route` at a failed final deposit's node (`after_no_route`). A part held past
  the floor stays `first_segment_settled` (or `not_past_first_segment`) in the order Core follows, and
  `Segments::waiting` (or `Recalculations::waiting`, at a routing) is the only place it shows; `AC-12`'s "visible to them with its age" needs a new
  `F-020` row with a state-machine and core-internal bump (an input that makes the part wait for the customer). The
  decision record (at recalculation) and the settled segment's evidence with the chosen record (between segments)
  are what the log holds of it.
- **Recorded once** (`INV-8`). The `route_recalculated` input (source `orchestrator`, Core's `acts`, standing
  `verified`, time source `core_clock`), citing the deciding choice round's decision record, is built once and
  appended until the log takes it; a log that can't take it now keeps it, and the next call appends it as built,
  asking no one again. One the log refuses is dropped. A retried append doesn't read the part again: a part
  declared stuck in between would still take it (hand-off to `T-020-11`, which declares it). Only once it is taken does the order Core follows move to the
  new route (`Segments::observe`), the old route's holds go, and the count and the round followed move on. The log
  admits it on `recalculation-starts-from-evidenced-value` and `input-cites-its-decision-record`; each move on the
  new route on `move-segment-is-in-its-orders-plan`. Each new segment's plan names the provider its operation
  runs at, so that row's provider field (`T-020-01e`, in `m3` since `dfe8a45`) holds: the paying test admits the
  new route's move through the real `admit_entry` and sees the same move refused on the provider field once the
  recalculated plan names another provider. `first-move-rests-on-its-parts-yes` reads a part's first segment by its
  `route_chosen` plan, so a recalculated route's first move isn't held to a yes.
- **Merged `m3` at `dfe8a45`** (fold `01e`, the intake update): the kit's seed now takes the evidence store and the
  operator and seeds the automatic deposit's arrival, so `seed_order_under` and `Core::under` take them too, and the
  recalculation tests find the seeded decision record by its kind, not its place.
- **Seams in other rows' modules** (the coordinator's word not asked; reversible, each named here and in the commit):
  - `snapshot/route.rs` (`T-020-05`): `check_recalculated_part`, the route guard with the start "at an inner node,
    by no withdrawal", and a refusal `NotFromAnInnerNode`. Every other rule is the same function's.
  - `segments/mod.rs` (`T-020-08`): `run_part` checks a part whose record's request `follows` another with
    `check_recalculated_part`; only a withdrawal's failure cancels a part (`place == 0` was taken as the withdrawal,
    so a recalculated deposit's failure would have cancelled it); the floor checked in `run_part` from
    `Live::floor`, and `waiting`; a segment the followed order retired never handed over or taken on again
    (`SegmentsError::Retired`); `observe` (a `route_recalculated` only: every other input the segments record
    themselves), `order`, `handed_on`, `handed_over` and `retired`. `Live` gained `floor`, so `T-020-08`'s own test
    and the kit set it.
    `T-020-09`, in the same wave, works in the segments' grading path: the lead sequences the two (`PLAN.md`
    §4.4).
  - `rounds/mod.rs` (`T-020-06`): a part of a routing that `follows` another and starts with no withdrawal asks no
    yes (otherwise every user-present recalculation was `NoReservesYes`); `History::recorded`, an order's history
    as the log holds it, so a recalculation chains to the order's last decision record (`REQ-AUD-4`).
  - `intake/mod.rs` (`T-020-03`), on the review: a request whose route `follows` another routing is refused as
    `malformed_request` before an order exists. Only Core sets `follows`; from a customer it would have run the
    recalculated route's guard, from an inner node, past the withdrawal and its yes. Test
    `request_claiming_to_follow_another_routing_is_refused`, in intake's test file.
- **Tests.** Three property tests in `components/core-orch/tests/recalc.rs`, over every sequence of reports a part
  can get: `silence_never_releases_a_route` (256 cases; only evidence releases the value, never silence, a bound
  provider or an unknown outcome), `late_delivery_after_a_recalculation_moves_nothing_twice` (a report about a
  retired segment is late or after the end, moving nothing; never two segments started at once, nor one twice) and
  `no_recalculation_changes_destination_or_to_asset` (Core's check, `route_end` and the state machine's), and five
  unit tests (passed again from the inputs, the floor in a route, the recalculated guard, the quotes still ahead
  with a charge and an exchange). Fourteen on the running platform in `conformance/scenarios/tests/recalc.rs`, on `T-020-08`'s kit with policy's engine
  deciding each move and the log replayed through `attribute` and `admit_entry`, the routing through the rounds and
  `RouteFinder` with the user present: the failed provider paying only the return, a late report after a
  recalculation, silence (silent, unknown, too late, the grant revoked, the version marked stale), the bound, a
  provider passed again, the floor at a settled segment and at a recalculation, an append retried, and a routing
  that offered no route not asked again, and the review's five below. The paying test runs the recalculation under an operator's fee of 10 000:
  the new route moves all 1 000 000 and delivers 990 000. The kit
  gained `Core::under`, `Core::split` with `route_chosen_split` and the second part's ids, `Core::floor`,
  `seed_order_under`, `authorisation_of`, `TestLog::receipt_at`, `On::Recalculated` and a public `price`, all
  additive.
- **Not the real `AuditLog`.** The brief asked for the real log in the platform tests "as `T-020-08`'s kit does",
  but the kit's log is its own `TestLog`, replayed through core-internal's `attribute` and `admit_entry`
  (`Core::admitted`). Moving the kit onto `AuditLog` needs its register seeded from recorded grants and
  advertisements, which is the kit's (`F-024`) to do for every scenario at once; these tests stay on the kit as it
  is. The real policy engine decides every move.
- **Rule-off, each red.** The code came first, so each rule was switched off once, its tests run, the file restored:
  the in-flight check (the silence property and the platform silence test; the state machine's `value_at` still
  refuses there, as `NoSettledEnd`); the in-flight check and the `value_at` agreement together (both silence tests:
  the rounds are asked, and the state machine's start rule rejects the route, `StartsElsewhere`, before anything is
  recorded; the log's `recalculation-starts-from-evidenced-value` would have admitted it, the withdrawal's settled end
  being the route's start); the bound (the bound test); the destination and to-asset checks, and the whole-amount
  check (the `INV-10` property); the floor in the route (its unit test); the floor gate at a settled segment and the
  waiting part held (the floor test); a routing below the floor read as waiting (its test); the old route's holds
  released (the paying test); a provider passed again (its test); a built recalculation appended as built (the
  append test); and the seams: the recalculated guard (the paying test, refused `NoWithdrawalFirst`), only a
  withdrawal cancelling a part (the bound test, the recalculated deposit cancelled), `observe` (the paying test, the
  followed order left on the old route), the yes a recalculated part doesn't ask (the paying test,
  `NoReservesYes`), and the guard's inner start (its unit test). After the advisor's review: the part held after no
  route (its test), `observe` taking a recalculation only (the late-report test's `provider_bound`), and the route's
  ends read from the route (the `INV-10` property).
- **Gap: silence is guarded only inside Core, not by the log.** With Core's evidence rule off, the rounds ask the
  providers before the state machine's start rule rejects the route; the log would have admitted the input, since a
  settled segment ends where the route starts, whatever came after it. The contract half is for core-internal's
  author (`T-020-01`, a bump): `recalculation-starts-from-evidenced-value` should also refuse a recalculation while a
  later segment of the part has a move (an `effect` of `move_value`) with no final `result`, and the log should
  refuse any further move on a segment a recalculation retired. Until then the log doesn't hold `INV-4` across a
  recalculation; Core and the state machine do.
- **The review of `3a95a8e`** (merged `m3` at `2233f33` first). Each test below was written first and seen red,
  then fixed:
  - **B1, the same value moved twice.** A deposit whose effect the log took but whose answers were lost stayed
    `pending` in the state machine, so a recalculation went around it, and running the old route again carried the
    retired deposit out: the platform handled two deposits and the log admitted both. Now a segment handed over
    with no final outcome counts as in flight (`NotRecalculable::HandedOver`, from `Segments::handed_over`), and
    `Segments` refuses to hand over or take on a segment the followed order retired (`SegmentsError::Retired`).
    Tests `deposit_whose_effect_may_be_recorded_is_not_recalculated_around` (the reviewer's probe) and
    `retired_segment_is_never_handed_over`.
  - **The bound, with parts and an outage.** Recalculations waiting on the log count against the bound, the count
    moves by one when one is recorded, and `Recalculations::follow_recorded` rebuilds it from the log's inputs.
    Test `recalculations_of_a_split_order_waiting_on_the_log_count_against_its_bound`, a split order seeded by the
    kit (`Core::split`; the router never splits).
  - **A retried append checks the part again.** Before a recalculation waiting on the log is appended, the part is
    read again: evidenced, nothing handed over, the state machine taking the input; otherwise it is dropped and
    answered `NotNow`. If the followed order still doesn't take it once recorded, `Recalculation::NotFollowed`:
    no hold released, nothing counted. Test `recalculation_retried_after_its_part_moved_on_is_not_recorded`.
  - **The floor can't be skipped.** See the floor above. Test `part_run_through_the_segments_alone_waits_at_its_floor`.
  - **`follows` from a customer**, refused at intake (above).
  - Each new rule was then switched off once and its test seen red: the handed-over check, the retired check, the
    intake refusal, the pending count, the count rebuilt, the re-check, the floor in `run_part` (both floor tests) and
    the waiting part held.
- **Touches**: `components/core-orch/src/recalc/`, the three seams above, `components/core-orch/tests/recalc.rs`,
  `conformance/scenarios/tests/recalc.rs` and the kit's additions in `conformance/scenarios/tests/segments_support/`.
  No manifest, no dependency, no `DEC-20` path.

## T-020-09

- **The grading, `closing::Evidenced`** (`components/core-orch/src/closing/`). It implements the segments' `Grading`
  seam, which now answers `Graded::Settles { cites, entries }` (the entries `segment_settled` rests on, the first the
  evidence it cites) or `Graded::Waits(Short)`, and appends nothing itself: the segments append what it answers, each
  entry once, kept with the segment like every other step (`INV-8`). What it reads comes in the struct: the catalogue
  (declared grades, which nodes are private or public, holding accounts), `PublicRecords` (each public network's rules
  and movements, and the network's own tie of a movement to a segment's transaction, `DEC-13`), `Books` (each private
  node's publisher's record of the segments inside it) and `Closing`, Core's record across orders of which segment
  each public movement evidences and what each settled segment's grading found, which the production assembly keeps
  durably (`F-030`), as `Segments`.
- **Graded through `CMP-EVIDENCE`** (`REQ-ORD-39`, `DEC-13`). The result's pieces go through `grade_outcome` against
  the grade the provider declared. A claim of grade 1 or 2 is backed only by `public_record_check` of the segment's
  own public movement (at its public end, its destination's first), on the piece whose reference is that movement; an
  unbacked claim is a statement. The `evidence` entry says whose: the piece's `by` where a backed proof is the best
  piece (the network, or the receipt's maker), otherwise the provider. Reading, reversible.
- **Closing at a part's final deposit** (`REQ-ORD-33`). Core checks the deposit's movement itself, held to the route's
  one destination (its account, its to asset and what the deposit gave out), and records two `evidence` entries, both
  resting on the reserve's verified result: the cited one, grade 1, whose the network; and `{cites}.reserve`, the
  reserve's signed statement, at the grade the reserve's own pieces earn (never the network record it names), whose
  the reserve. The result's envelope is the reserve's "signed statement
  for its own book" (provider-role's README). Without a final public record the deposit waits
  (`SegmentOutcome::AwaitsEvidence(Short::NoPublicRecord)`): no `segment_settled`, no `result`, the value held, the
  segment in flight and graded again on the next run, so a movement that becomes final later closes it. Nothing is
  recorded of a waiting deposit's grade beyond its result's stated entry (default, reversible: an evidence entry
  built while waiting would change once the record turns final, and one id can't carry two entries). A deposit to a
  rail declared private closes on the public movement into the private node instead, plus the reserve's statement,
  and records no replay.
- **Inside a private node.** A segment whose two ends are private settles on its provider's evidence (grade 3 or 4 by
  necessity, ADR-0007). Where the value leaves the node on a final public record, each such segment gets two more
  `evidence` entries, `{cites}.in.{place}` and `{cites}.out.{place}`, grade 1, whose the network of the movement into
  the node and of the movement out, resting on that segment's own result. Reading, reversible.
- **Replay** (`REQ-ORD-41`, `DEC-24`). At a segment from a private node onto a public one, the private nodes passed
  since the value last left a public node are replayed with `replay_route` against the movement out and the stated
  outcome (the segment's to asset and what it gave out), each node's record asked of `Books` for the transactions of
  the segments that touched it. A new core-internal entry, `replay`, records it: `consistent_with_the_exit` (never
  proof), each `mismatch` with its node, or `nothing_recorded`. Two private nodes with a public node between are
  replayed in turn, each as the value leaves it; two adjacent ones together, as one hands to the other. A mismatch
  stops nothing: it is recorded (raising it is `T-022-05`'s). An exit with no public movement to compare is a
  `public_exit` mismatch. The replay entry cites nothing earlier (admitted on its own checks, like `holding_check`).
- **One movement, one segment** (`REQ-ORD-38`). The first segment whose grading relies on a movement holds it in
  `Closing`; for any other order's segment that movement backs nothing, so it is graded on its other pieces, the
  provider's word unless they grade higher, and each order keeps its own segment and its own entry. A final deposit
  on a movement another deposit closed on waits (`Short::MovementShared`).
- **Before a segment is handed over** (`closing::admits`, called from `run_part`): its provider's declared grade is at
  or above the floor the operator set on its tuple now, and a tuple with no floor or a provider with no declaration
  is refused (`REQ-ORD-39`); a segment that isn't its part's last leaves the value at an inner node with a holding
  account (`INV-5`, `REQ-CAT-23`). The snapshot left such providers and nodes out when the route was chosen; this
  holds a segment to the catalogue as it stands, so a floor raised since stops it. A refusal is recorded as
  `order_refusal` `no_route_right_now` (`RouteRefusal::BelowEvidenceFloor`, `NoHoldingAccount`) and the part answers
  `Ran::Refused`, also where earlier segments settled (they stand). A result graded below its tuple's floor still
  settles, recorded as graded: the floor is a routing rule. Readings, reversible.
- **Fees** (`REQ-AUD-11`, `closing::fees`). After `segment_settled`, built once and appended as the segment's step:
  the fee its result states, to its provider (only where the fee is in the asset the segment takes in, which `judge`
  holds), and on a part's final deposit the operator's, `rutent_router::quote::operator_fee` over what the deposit
  gave out, attributed to the operator's `acts`, recorded at zero too. `billed_separately` is always false: provider-role
  has no field for a fee billed apart (gap below). Fees are recorded whatever the grading, `ProvidersWord` included.
  Nothing sums them.
- **Seams in other rows' files** (each named in the commit):
  - `segments/ports.rs` and `segments/mod.rs` (`T-020-08`): `ToGrade` carries what the grading reads (the part, the
    transaction, the result's bytes, the part's operations, the operator's fee formula, the place, the segments, the
    route's end, what the result says it gave out, the operator); `Grading::grade` answers `Graded`; `Short`;
    `SegmentOutcome::AwaitsEvidence`; the graded entries and the fee records kept with the segment; the guard before a
    first hand-over. `ProvidersWord` stays, answering the same grade-4 entry as before, documented as a stand-in that
    closes on a statement and so is no grading for an order that closes.
  - `snapshot/route.rs` (`T-020-05`): the two `RouteRefusal` variants.
  - `components/core-orch/tests/segments.rs` (`T-020-08`): its catalogue was empty, so the guard refused the
    withdrawal; it now joins the withdrawal's reserve with a floor and a holding account (`admitting`).
  - `conformance/scenarios/tests/segments.rs` (`T-020-08`): `first_verdict_on_a_result_stands_when_its_key_is_retired_between_tries`
    re-runs under an empty catalogue; the withdrawal still settles on its first verdict, and the deposit, never handed
    over, is now refused as below its floor rather than run.
  - `components/core-audit/tests/log.rs` (`T-022-01`): the new kind in its exhaustive list and its standalone fixtures.
- **Contract: core-internal 1.0.0, folded** (unreleased, no bump). `What::Replay`, `ReplayFound`, `ReplayMismatch`,
  `MismatchKind`, the schema's `what_replay`, three valid and four invalid fixtures, the README's home row and the
  standalone list, the CHANGELOG line. `contracts/` is a `DEC-20` path. `PLAN.md` §4.4 gives `contracts/` to `T-020-01`
  and `T-020-02` in `M3`; this is made on the brief's word, since `REQ-ORD-41`'s replay had no home and the home table
  says a record not listed needs the contract changed.
- **Tests.** On the running platform with the simulated providers (`conformance/scenarios/tests/closing.rs`, over
  `T-020-08`'s kit, a `Recording` wrapper keeping the bank network and the reserve's book as Core reads them):
  `statement_alone_does_not_close_an_order` (the reserve's `unrecorded` deposit; then the same route on the record,
  closing, the ledger replayed), `provider_below_the_tuples_floor_is_not_routed_through` (the floor raised on the
  catalogue the part runs against, on each of the two tuples), `value_between_segments_rests_only_in_a_holding_account`
  (the same catalogue with no holding account registered) and `every_settled_segment_has_its_fee_record_and_no_balance_is_kept`
  (two orders), and beyond the row, `deposit_waiting_for_its_public_record_closes_once_it_is_recorded` (`REQ-ORD-33`,
  `INV-8`: the waiting deposit run again appends nothing, then the movement recorded, the log refusing its evidence and
  then its fee record for a run each, and the next run settles it once). With fakes in `components/core-orch/tests/closing.rs`, over the real `CMP-EVIDENCE` and
  `SimulatedNetwork`, since no simulated route has a segment inside a private node, crosses two providers' private
  nodes, or shares a movement: `segment_within_a_private_node_closes_on_the_public_movements_in_and_out` (the
  router-api fixture's route through `ledger-in`), `route_across_two_private_nodes_replays_each_on_its_way_out` (five
  segments through `ledger-in`, the public `polygon` and `ledger-b`, matching and with a mismatch at the second node)
  and `shared_movement_keeps_one_segment_per_order_as_the_providers_word`, and beyond the row
  `exchange_and_ramp_operations_are_held_to_their_own_tuples_floors` (the guard's tuple keys against provider-role's
  exchange and ramp fixtures). The segment-within test also checks the fee records under an operator's fee of 10 000.
  Three of the row's seven aren't on the platform, though its done-when says they are: a kit route across two
  providers' private nodes is `T-024-01`'s, and the platform scenarios `T-024-05`'s. The end-to-end scenarios are `T-024-04`'s
  and `T-024-05`'s.
- **Written after the code, then each rule switched off.** The code came first (the seam's shape decided the tests'),
  so each rule was switched off once, its test run red, the file restored and touched (scratch helper
  `t-020-09-ruleoff.py`, outside the tree):

  | Rule switched off | Red |
  |---|---|
  | a final deposit closes only on a final public record | `statement_alone_does_not_close_an_order`; `segment_within_a_private_node_closes_on_the_public_movements_in_and_out` |
  | the tuple's floor before hand-over | `provider_below_the_tuples_floor_is_not_routed_through` |
  | a holding account at the node between segments | `value_between_segments_rests_only_in_a_holding_account` |
  | the guard called in `run_part` | `value_between_segments_rests_only_in_a_holding_account` |
  | the reserve's statement graded by its own pieces | `segment_within_a_private_node_closes_on_the_public_movements_in_and_out` |
  | a waiting segment isn't final, so a later run grades it again | `deposit_waiting_for_its_public_record_closes_once_it_is_recorded` |
  | an evidence or fee append that fails stops the settlement | `deposit_waiting_for_its_public_record_closes_once_it_is_recorded` |
  | a ramp's tuple keyed as a ramp | `exchange_and_ramp_operations_are_held_to_their_own_tuples_floors` |
  | an exchange's tuple keyed by its to asset | `exchange_and_ramp_operations_are_held_to_their_own_tuples_floors` |
  | the provider's fee record | `every_settled_segment_has_its_fee_record_and_no_balance_is_kept` |
  | the operator's fee record on the final deposit | `every_settled_segment_has_its_fee_record_and_no_balance_is_kept` |
  | a movement relied on by another order backs nothing | `shared_movement_keeps_one_segment_per_order_as_the_providers_word` |
  | whose the evidence is, from the backed piece | `shared_movement_keeps_one_segment_per_order_as_the_providers_word` |
  | the replay on the way out | `route_across_two_private_nodes_replays_each_on_its_way_out` |
  | each node replayed in turn (the run stops at a public node) | `route_across_two_private_nodes_replays_each_on_its_way_out` |
  | the inner segment's public movements in and out | `segment_within_a_private_node_closes_on_the_public_movements_in_and_out` |
  | contract: a mismatch is against a node replayed | `every_valid_fixture_validates_and_every_invalid_one_is_rejected` |
  | contract: a replay names its nodes | `every_valid_fixture_validates_and_every_invalid_one_is_rejected` |

- **Hand-off to `T-024-02`** (the assembly, in review). Swap `grading: &mut ProvidersWord` for
  `&mut closing::Evidenced { catalogue, records, books, closing }`, keeping one `Closing` beside `Segments`. In the
  simulated world the networks and the books live inside the platform's world, which `Parts` already borrows as the
  modules, so the assembly copies them into what implements `PublicRecords` and `Books` before every grading: at the
  start of each run and after each `carry_out`, as the test's `run` and `Recording` do. A copy after `carry_out` alone
  isn't enough: a resumed run carries nothing out, and a deposit on `polygon` or `bsc` (`Confirmations(2)`) is
  pending at its first grading and final only once blocks are added, so it would wait for ever. `Closing` must live
  as long as `Segments` (and as durably, `F-030`): a fresh one silently loses the private-rail close (the movement
  into the node), the replay's earlier transactions and the inner segments' in-and-out records, and the
  shared-movement guard; none of them errs. No field of `Parts` or `Live` changed. The kit gained `On::Evidence` and
  `On::Fee` (additive); `T-024-02`'s rewritten kit keeps every name `closing.rs` uses.
- **Gaps, with their owners.**
  - The log doesn't hold a final deposit's settlement to a public record and the reserve's statement: only Core does.
    A core-internal relation (the `segment_settled` of a part's last deposit cites grade-1 evidence and rests on the
    reserve's verified result) would make it the log's too. `T-020-01`'s contract, foldable while 1.0.0 is
    unreleased; not added here.
  - A fee billed separately: provider-role 3.0.0's `Fee` has no flag, so every record says `false`. A provider-role
    change (released): reported, not made.
  - A private node's record of its inner segments has no provider-role shape, so `Books` is a seam the assembly fills
    from each publisher (`F-015`'s notes): a provider-role change, for the owner.
  - Each real network's movement-to-segment matching, finality and reorg rules (`PublicRecords`): `F-031`, one task per
    network (`DEC-13`).
  - A final deposit made in pieces (several movements) can't close: its one destination's movement is one. The
    simulation's `Delivery::Pieces` makes one; the rule for pieces (`REQ-ORD-30`) is open, for the coordinator.
  - Routing away from a reserve that can't give a public record ("A reserve that can't give them MUST NOT end a
    route"): the router and the catalogue's (`F-011`, `F-012`); here such a deposit only waits.
  - A replay mismatch is recorded, not raised to the operator: `T-022-05`'s raise.
  - Evidence `CMP-EVIDENCE` refuses (a claim above the declared grade, a reused movement) leaves the segment waiting,
    held: the stuck path (`T-020-11`) resolves it.
- **Touches**: `components/core-orch/src/closing/`, the seams above, the two new test files, the contract's files. No
  manifest, no dependency, no `licences/RECORD` line. `DEC-20` path: `contracts/` (core-internal).

### T-020-09, review answers (of `8b272a9`)

Each answer's test was written first and seen red, except where the table says the code came first; then each new
rule was switched off once, its test seen red, the file restored and touched.

| Finding | Answer | Test |
|---|---|---|
| H1, a deposit from a public node into a private rail never closed | `movement_into` takes the deposit's own movement where the deposit is the segment that enters the private node; a final deposit into a private rail is held to the network's matching, not the route's account, and its record names the rail (`movement.node`) | `final_deposit_into_a_private_rail_closes_on_the_public_movement_into_it` (core-orch) |
| H2a, `ProvidersWord` closed a part on grade 4 | it answers `Waits(NoPublicRecord)` for a part's final deposit | `providers_word_never_closes_a_part` |
| H2b, the log let a statement alone close | core-internal row `final-deposit-closes-on-a-public-record` (none other): a part's last segment, a deposit, settles only on grade-1 evidence naming its movement on the network its `move_value` route names (or naming the private `node` it went into), resting on a verified statement of the segment's provider | the contract's cases; `providers_word_never_closes_a_part` (a grading that settles on the word anyway is refused) |
| H3, no entry named the movement | `evidence.movement { network, id, node }`, only at grade 1; `replay.movement`, the movement out; row `movement-evidences-one-segment`: no second segment relies on a movement, of another order, or of its order where neither names a node (a node's movements in and out evidence each segment inside it); `Closing::recorded` rebuilds Core's record from the log | `one_payment_closes_one_order_across_a_restart`: two orders to one account for one amount, one payment, a restart between the gradings: rebuilt, the second waits; forgotten, the log refuses it |
| M4, a node caught misreporting still got grade-1 in/out entries | the in and out entries are written only where the node's replay found its records consistent with the exit | `route_across_two_private_nodes_replays_each_on_its_way_out` (mismatching run) |
| M5, whose was the piece's `by` | a public record is the network's Core checked, whatever the piece names; the module doc says a non-final segment's destination tie rests on each network's matching (`F-031`), and that the amount compared is what the provider says it gave out, gross or net being the network's matching too (`F-031`) | `public_record_is_the_networks_whatever_the_piece_names` (`by` an auditor of the reserve's choosing) |
| M6, `replay` cited nothing | `replay.rests_on`, the exit's result; row `replay-rests-on-its-exit` (order, segment); invalid fixtures for a silent node not replayed and a replay resting on nothing; refused cases for a missing result, another order's and another segment's | the contract's cases |
| M7, the final-deposit guards untested | a wrong account, a wrong asset, a movement not yet final | `evidence_for_another_segment_does_not_close_an_order` (passed when written: the guards were there; each switched off below) |
| L8, a mid-route refusal appended on every run | kept per order and part, answered as recorded while the guard refuses for the same reason | `refusal_mid_route_is_recorded_once_and_leaves_the_order_to_be_recalculated`, which also reads `recalc::evidenced` at the ledger |
| L9, `PublishersWord` recorded a check that didn't run | it records no replay entry | none reachable: Core replays only towards a public exit (reading kept) |
| L10, shared movement | default kept; the reading is on the owner's form | — |
| L11, no public record possible | `Grading::can_close`: a part whose final deposit's destination Core reads no public record of, and which no publisher declares a private rail, isn't routed (`RouteRefusal::NoPublicRecord`), before anything is handed over; `ProvidersWord` answers that every part can | `reserve_that_cant_give_a_public_record_is_not_routed_to` (the code came first; switched off below) |
| Billed separately | provider-role 3.1.0: `fee.billed_separately`, optional; core-internal's `fee.billed_separately` is what the provider stated, absent where it said nothing; the operator's fee record says nothing | `segment_within_…` (absent, then `true`); `every_settled_segment_has_its_fee_record_…` (the simulated reserve says nothing) |
| The fakes and the log | the fake-backed tests record the order's plan and moves, each result's stated entry, and admit every entry the grading answers, and each `segment_settled`, through the real `admit_entry` | every test in `components/core-orch/tests/closing.rs` |
| Nits | an unlisted provider is `ProviderNotListed`; core-audit appends the replay after its world, resting on a world result, rather than standalone; `nothing_recorded` names every silent node; the fake's operator fee is the operator's | `exchange_and_ramp_…`, `every_node_that_recorded_nothing_is_named`, `segment_within_…` |

- **Rule-off, each red:**

  | Rule switched off | Red |
  |---|---|
  | a deposit entering a private rail uses its own movement | `final_deposit_into_a_private_rail_…` |
  | `ProvidersWord` waits at a final deposit | `providers_word_never_closes_a_part` |
  | the close names its movement | `segment_within_…` |
  | `Closing` rebuilt from the log | `one_payment_closes_one_order_across_a_restart` |
  | in and out entries only where the replay is consistent | `route_across_two_private_nodes_…` |
  | a public record is the network's | `public_record_is_the_networks_…` |
  | the final deposit held to the destination's account | `evidence_for_another_segment_…` |
  | the asset compared (switching off the to-asset hold alone can't go red: a deposit's own asset is the route's to asset, so the network adapter was made to pass any asset) | `evidence_for_another_segment_…` |
  | only a final movement | `evidence_for_another_segment_…` |
  | a part that can't close isn't routed | `reserve_that_cant_give_a_public_record_…` |
  | billing as the provider stated | `segment_within_…` |
  | an unlisted provider named as such | `exchange_and_ramp_…` |
  | every silent node named | `every_node_that_recorded_nothing_is_named` |
  | a mid-route refusal recorded once | `refusal_mid_route_…` |
  | row `final-deposit-closes-on-a-public-record` | the contract's citation test; `providers_word_never_closes_a_part` |
  | row `movement-evidences-one-segment` | `one_payment_closes_one_order_across_a_restart` |
  | the row's node-level exemption within one order | `segment_within_…` |
  | row `replay-rests-on-its-exit` | the contract's citation test |
  | a movement named only on a public record (contract check) | the contract's fixture test |
  | a silent node is one replayed (contract check) | the contract's fixture test |

- **Readings, reversible.** A node's movements in and out evidence every segment inside it, of its one order, so the
  movement row lets them share a movement within the order when the entry names the node; another order never. A
  final deposit naming a private `node` is let off the network comparison: the log can't tell a node declared private
  (a residual for `T-025-05`, beside the other catalogue residuals). The reserve's own entry at a close is graded by
  its own pieces, at best grade 3. `Closing` relies on a movement only where an entry names it for its segment alone.
- **The kit's stand-in.** The platform tests that are about something else (`segments.rs`, `recalc.rs` and the kit's
  runners) now grade with `segments_support::Recorded`: a segment on its provider's word, and a part's final deposit
  on the movement the simulated reserve records, by the segment's transaction, taken as found. Under `ProvidersWord`
  their deposits now wait. The closing tests grade through `closing::Evidenced`.
- **Hand-off to `T-024-02`, sharper.** The assembly on `m3` wires `ProvidersWord`, so once this merges every
  scenario's final deposit waits and no order completes: it must switch to `closing::Evidenced`, refreshing what it
  reads of the networks and books before every grading, and keep `Closing` beside `Segments`, rebuilt with
  `Closing::recorded` from the log after a restart.
- **The three fake-backed row tests** (inside a private node, across two, one shared movement) need simulated
  shapes the kit doesn't have: a route through a provider's segment inside its private node, one across two
  providers' private nodes, and a reserve answering two orders with one movement. Their done-when ("on the running
  platform") goes on the owner's form; the shapes are `T-024-01`'s, the scenarios `T-024-05`'s.
- **Contracts, `DEC-20` paths.** core-internal 1.0.0, folded (unreleased): `evidence.movement`, `replay.rests_on` and
  `replay.movement`, `nothing_recorded.nodes`, an optional `fee.billed_separately`, `PublicMovement`, the three rows
  in `contracts/src/core_internal/relations/closing.rs` (registered by one line each in `relations.rs`, `mod.rs` and
  the contract's test), fixtures, README and CHANGELOG. provider-role 3.1.0 (released, minor, authorised by the
  coordinator): an optional `fee.billed_separately`, a valid and an invalid fixture; every `Fee` literal in the
  workspace and the plain-wallet guest gained `billed_separately: None`.

### T-020-09, re-review answers (of `bc99b4f`)

| Finding | Answer | Test |
|---|---|---|
| 1, high: a deposit into a private rail from another private node closed on the customer's payment into the first node | the coordinator's default, on the owner's form: a private rail closes only on the deposit's own movement, which only a deposit from a public node has; otherwise it waits, and `Grading::can_close` refuses a part ending in a deposit from a private node into a private rail, so it isn't routed | `deposit_into_a_private_rail_from_a_private_node_does_not_close` (the reviewer's route) |
| 2a, restart: `Closing::recorded` rebuilt only grade-1 movements | `evidence.movement` names every movement Core checked, at whatever grade it earned, and `Closing::recorded` takes every movement the log names, its own and a node's | `movements_core_checked_are_taken_live_and_after_a_restart` (restart half) |
| 2b, live: node-level movements never entered Core's record | Core takes every movement it checked, its own and each node's in and out, keyed by order and segment, a node's reused within its order (`Closing::free_for`), exactly as the log's row does | the same test (live half): order B, tied to order A's movements, is graded off them and waits on the deposit, with nothing refused |
| 3, reading 2 too wide | the row requires the `node` a final deposit names to be its authorised destination's network; whether the rail is declared private stays a residual for `T-025-05` | refused case `final-deposit-closes-on-a-public-record--conflict-naming-another-node` |
| 4, router-api's operation fee changed with no record | a line under router-api 4.0.0 (unreleased, no bump): its `fee` is provider-role's by reference and may carry `billed_separately`, which the router never sets | the guard |
| 5, 3.0.0 statements | provider-role 3.1.0's changelog says statements made under 3.0.0 are refused under 3.1.0 by `check_context` and must be re-signed | — |
| 6, the movement id's encoding | `PublicRecords`' docs: one label per chain and one canonical spelling per movement id, the matching `F-031`'s; grade-2 evidence a public-record check backs names and takes its movement, as every grade does now | covered by 2 |
| 7, README | the movement row names a node's reuse within its one order | — |

- **Rule-off, each red:**

  | Rule switched off | Red |
  |---|---|
  | a private rail closes on no earlier movement (the old reach back restored) | `deposit_into_a_private_rail_from_a_private_node_does_not_close` |
  | `can_close` refuses a private rail from a private node | the same |
  | every movement Core checked named in its evidence | `movements_core_checked_are_taken_live_and_after_a_restart` |
  | every movement Core checked taken live, its own and the node's | the same (switching off either alone stays green: each node's movements in and out are also the own movements of the segments that entered and left it, so either take covers the other) |
  | `Closing::recorded` takes every movement the log names | the same |
  | the movement row at any grade | the same (its forgetful-Core half) |
  | the rail a final deposit names is its authorised network | the contract's citation test |

  The filter that a private rail's own movement be from a public node, switched off alone, stays green: a deposit from
  a private node into a private rail has no public end, so it checks no movement of its own; the filter stays as
  defence in depth.
- **Contracts.** core-internal 1.0.0, folded: the movement row at any grade, a node's reuse within its order, the rail
  named as the authorised network; the invalid fixture `verified-entry-of-signed-evidence-naming-a-public-movement` is
  valid now, renamed `verified-entry-of-signed-evidence-naming-the-movement-core-checked`; README and CHANGELOG.
  router-api 4.0.0 and provider-role 3.1.0 changelog lines. `DEC-20` path: `contracts/`.
