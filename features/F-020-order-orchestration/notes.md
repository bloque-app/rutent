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

## T-020-04

Branch `f-020/t-020-04-arrivals`, from `m3` at `28038de`. The review of `b5142fa` changed several readings below;
"T-020-04, review answers" says how, and where a bullet here and an answer there differ, the answer stands.

- **Three calls on one `Arrivals`** (`components/core-orch/src/arrival/`). `arrive` takes value arriving at a reserve,
  `refile` files a differing arrival anew, and `file::<R>` files an order on an automatic deposit; `start` and `record`
  answer what the assembly reads of an order filed that way. They reach what intake reaches, through intake's `Desk`
  (the rounds' parts, the destination register, policy's authorisations, the register of actors, policy's session
  register, the bounds and prices, Core's and the operator's actors), and one port of their own, `OpenOrders`.
  `Arrivals` is Core's record (each notice taken, each arrival recorded, each refiling and filing, the transactions it
  asked under), in memory as intake's is; keeping it across a restart is `F-030`'s.
- **What arrives, the port's word.** provider-role has no statement a reserve makes of value arriving, and no question
  that asks it what arrived. So a `Report` carries what arrived (reserve, outer node, asset, amount, the account and
  its holder) as the assembly read it from the reserve's module, and the reserve's notice, its signed statement as
  received, which Core keeps and appends as the reserve's stated entry, about nothing, without reading its body. Core
  can't hold the figures to the signed bytes. **Gap**, owner a new `F-020` row with a provider-role bump: an arrival
  notice shape naming its node, asset, amount and account, and a question asking a reserve what arrived for an order;
  until then the figures are the port's word, as `T-020-03`'s `Destinations` answers a destination.
- **No order, no order** (`REQ-ORD-14`). Where no order is open at the arrival's reserve, outer node and asset for its
  account, the arrival is recorded as an `arrival` with no order, `no_order`, citing the notice, and nothing else:
  no order id is drawn, no one is asked anything, nothing is held, and no input is recorded, so nothing about it can
  count against the operator as a stuck order (`T-020-11` counts orders, and there is none). An order open elsewhere,
  at another reserve, node, asset or account, isn't this arrival's, whatever amount it states. Once an order is no
  longer open (its first segment settled, or it ended), an arrival for it is this case, as `REQ-ORD-23` says.
- **Held against an open order** (`REQ-ORD-23`). The port answers the orders open at the reserve for the account: an
  order accepted and started whose first segment hasn't settled, with the amount it states and the session its
  reserve is asked in. Of those at the arrival's node and asset, the first stating the amount that arrived is matched;
  otherwise the first is differed from. Reading, reversible: an arrival is held against one order, and a split
  arrival's pieces are each judged alone, never summed into a match.
- **Bound to its order before it is answered** (`REQ-ORD-32`, `REQ-ORD-38`; the row's last clause). An arrival naming
  an order rests on the reserve's answer to Core's own question about that order, never on its notice, which no
  order is bound to (`about_agrees` admits a report about nothing for any order, so the log alone wouldn't catch it).
  The question is a balance verification of the account at the amount that arrived, the nearest provider-role has
  (`INV-9`, yes or no; a reading, reversible). Its yes says only that the account's funds cover that amount, not that
  anything arrived: it binds the reserve's word to the order, and the figures stay the report's. It is asked under a fresh transaction, never one the rounds or arrivals asked
  under nor naming the order, in the order's session, as a check the catalogue lets out (`begin_check`, the holder's
  own order), signed with the operator's key and appended about the order and transaction before it is sent; the
  answer is appended with that transaction in its `about` and read through `answer_check`. Only on its yes is the
  `arrival` recorded; a no, a refusal, another answer or none leave the notice, the question and any answer in the log
  and the arrival waits (`Arrived::Waiting`, not kept: the same report again asks again; review answer 3). The
  contract's own world fixture models the same shape (`world--refiling`:
  the operator's question, the reserve's result, the `arrival` citing it).
- **Settles nothing by itself.** A matching or a differing arrival is a record and nothing more: no effect, no input,
  no evidence. The order's first segment settles only on its own result, judged by `T-020-08`'s segments.
- **Filing anew** (`REQ-ORD-23`). Only an arrival held against an order it differed from is filed anew, and only
  against that order (`refiling-files-an-arrival-anew`); one that met no order is `REQ-ORD-14`'s, and one that matched
  needs none. The filer is the operator, or the reserve it arrived at acting for itself (its party the reserve's id,
  as that row and policy's authorise rule read it), and in either case an actor the register says acts for the party it
  states, so an impostor of either files nothing. The `refiling` is recorded once, its `who` the filer, and moves
  nothing. **Gap**: no state-machine input takes a refiling, so what an order does with an arrival filed anew is no
  row's yet; a later `F-020` row with a state-machine bump, if it should move the order.
- **The automatic deposit** (`REQ-ORD-13`, `REQ-ORD-15`). *Superseded in its order of steps by review answer 2: policy
  is now asked before any order is opened, held, asked or routed.* A filing names the arrival it files on, one recorded with no
  order, and fills out the details: the parties, the session opened for the deposit's account, the destination, the
  route, the inputs and needs. In order, and nothing asked or recorded before each passes:
  1. The filer is the reserve it arrived at, or the operator, acting for the party it states; anyone else files
     nothing (`NotTheFiler`).
  2. The reserve's latest declaration in Core's catalogue says it starts routes on automatic deposits; one that says
     no, or never declared, starts nothing: no order, no question, no authorisation, nothing recorded beyond the
     arrival (`NotDeclared`). Core's own gate, before policy's, which holds the same declaration.
  3. The filing is for what arrived: from its outer node, in its asset, at its amount, for the account's holder as
     requester ("every order MUST be initiated by a user"), with no `follows`; and its session is one policy opened
     for that holder, still open. Otherwise an `order_refusal` naming no order, `malformed_request` or
     `refused_by_policy`, as intake refuses a request; such a filing holds nothing against the arrival, and the same
     filing again is told the same.
  4. One order per arrival: the same filing again carries it on; another filing on the same arrival opens nothing
     (`Conflicting`). The log doesn't stop two filers each opening an order on one deposit, so Core does.
  Then the order opens under a fresh id, its destination resolved (`destination_check`; not yet waits, filed again to
  carry on; no ends it); the deposit is held against it by its own question, as above, and recorded as an `arrival`
  naming the order, `matches`, citing the reserve's answer (not re-citing the notice); one routing through the rounds
  with no user present (`Presence::AutomaticDeposit`, in the filing's session, no yes asked); the quote recorded as
  `shown`; the authorisation, given by the filer, to policy; and on its permit the `acceptance`, `basis:
  automatic_deposit`, its `who` the filer, with no yes and no session, and the `route_chosen` input, the filer's
  statement (source `provider` for the reserve, `operator` for the operator, standing `stated`). Both are built once and
  appended as built; a refused one ends the order, `no_route_right_now`, as at intake. The log admits it on
  `automatic-deposit-rests-on-its-arrival`, `order-holds-one-acceptance-basis` and
  `chosen-route-rests-on-its-orders-acceptance`.
- **One deposit, more than one `arrival` entry.** The deposit is first recorded as it came, with no order; each
  permitted filing that holds it records another `arrival`, naming the order it opened, on the reserve's answer to that
  filing's own question. A filing that ends frees the deposit (review answer 2), so the next filing records one more.
  Nothing in the log links them: `arrival` has no field naming an earlier arrival. Reading, reversible. **Gap**, owner
  a new `F-020` row with a core-internal bump: an `arrival` naming the earlier arrival it holds against an order, so a
  checker sees one deposit, not several.
- **A held arrival whose order's session has ended**: see review answer 3.
- **Who is told what**: see review answer 9.
- **Readings, each reversible**: the routing's classes are intake's (`no_route_for_amount` where every candidate was
  dropped for its amount, `no_route_right_now` otherwise); a reserve that says the deposit's funds aren't there ends
  the order `no_route_right_now`; a quote is shown for an automatic deposit though no customer sees it, so the
  acceptance's `quote` digests a quote Core recorded. How policy's answers are taken is review answer 2.
- **Seams not taken.** None in other rows' modules. `rounds::Issued` has no public way to add a transaction, so
  `Arrivals` keeps its own and checks `Issued::contains` as well, as the segments do; a transaction the rounds draw
  later isn't checked against arrivals' (fresh identifiers are drawn at random, and `transaction-belongs-to-one-order`
  refuses one reused for another order). Hand-off to `T-020-06`, if it should: `Issued::issue`.
- **Wiring for the assembly** (`T-024-02`):
  - `Segments::follow` fed from `Arrivals::start`, beside `Intake::start`, for an order filed on an automatic deposit;
  - `Arrivals::record` added to policy's order records (`Records::orders`) once a filing opens an order, or policy's
    `OrderOrigin` and `Requester` inputs come up missing and the filer's authorisation is refused;
  - `OpenOrders` backed from intake's records and the orders the segments follow, with each order's session and
    account;
  - the session opened for the deposit's account handed to the catalogue's register (`Register::open_in`), as every
    session the engine opens is;
  - the `Report` built from what the reserve's module reports, its notice as received;
  - `Core::with_desk` hands its closure intake alone, so `Arrivals` is held beside it (it is `Default`, and can be
    taken out with `mem::take` for the call);
  - an operator filer needs `ActForUser` in the deployment's register (`Permission::ActForUser`), which the assembly
    lists only `Change` and `ReadOrderState` for today; a reserve filer needs an actor listed for its own id.
- **Tests** (`components/core-orch/tests/arrival.rs`, on intake's kit, unchanged, with policy's fake permitting every
  authorisation whose giver the register lists): the row's four, and six beyond it,
  `same_notice_again_is_recorded_once`, `arrival_held_against_an_order_rests_on_the_reserves_answer_to_cores_question`,
  `differing_arrival_is_filed_anew_only_by_the_operator_or_its_reserve`,
  `automatic_deposit_is_filed_once_by_its_reserve_or_the_operator_for_what_arrived`,
  `automatic_deposit_filing_is_carried_on_where_it_stopped_asking_no_one_twice` and
  `automatic_deposit_filing_ends_or_waits_as_its_destination_reserve_and_policy_answer`. The first eight were written
  first and seen red against the module's signatures with no body; the last two came after the code, on the advisor's
  review, so their rule-offs stand in for seeing them fail first. Every log is replayed through `attribute` and `admit_entry` on
  the path each entry came by. The real policy engine and the platform are `T-024-04`'s scenarios (`AC-10`, `AC-11`,
  `AC-26`).
- **Rule-off**, each rule switched off once, the file restored and touched. *Superseded by the review answers' table,
  which reruns every rule against the final code; this one is kept as it was run on `b5142fa`.*

  | Rule | Red |
  |---|---|
  | An open order holds an arrival only at its reserve, outer node, asset and account | `unattributed_arrival_creates_no_order` |
  | Matched only at the amount the order states | `short_excess_and_split_arrivals_settle_no_first_segment_by_themselves`, `differing_arrival_is_filed_anew_only_by_the_operator_or_its_reserve` |
  | A held arrival cites the reserve's answer to Core's question, not its notice | `arrival_held_against_an_order_rests_on_the_reserves_answer_to_cores_question`, `short_excess_and_split_arrivals_settle_no_first_segment_by_themselves` |
  | The question appended before it is sent | five tests: the log refuses the answer (`answer-follows-its-question`) |
  | Held only on the reserve's yes | `arrival_held_against_an_order_rests_on_the_reserves_answer_to_cores_question` |
  | Only a differing arrival filed anew | `differing_arrival_is_filed_anew_only_by_the_operator_or_its_reserve` |
  | The filer is the operator or the reserve | the refiling test and the filing test |
  | The reserve filer is the reserve itself | the refiling test and the filing test |
  | The filer acts for the party it states | the refiling test and the filing test |
  | One refiling per arrival | the refiling test |
  | The declaration gate | `automatic_deposit_at_a_reserve_that_never_declared_it_starts_nothing` |
  | The basis `automatic_deposit` | `automatic_deposit_is_recorded_as_acceptance_apart_from_an_explicit_one`, the filing test |
  | The acceptance names its filer | `automatic_deposit_is_recorded_as_acceptance_apart_from_an_explicit_one` |
  | The deposit held against the order before it is routed | the acceptance test and the filing test: the log refuses the acceptance (`automatic-deposit-rests-on-its-arrival`) |
  | Filed for the amount that arrived | the filing test |
  | The requester is the account's holder | the filing test |
  | The filing's session is the holder's | the filing test |
  | One order per arrival | the filing test |
  | Filed only on an arrival that met no order | the filing test |
  | The same notice taken once | `same_notice_again_is_recorded_once` |
  | The start is the filer's statement | the acceptance test |
  | The filer held before a filing on the arrival is told | the filing test |
  | The filer held before how the arrival met its order is told | the refiling test |
  | The acceptance and start kept across tries | `automatic_deposit_filing_is_carried_on_where_it_stopped_asking_no_one_twice` |
  | A refused destination ends the order | `automatic_deposit_filing_ends_or_waits_as_its_destination_reserve_and_policy_answer` |
  | A destination not yet resolved waits | the same |
  | Only `outside_authorisation` ends the order | the same |
  | A refusal of the filer leaves the order standing | the same |

  The rule "held only on the reserve's yes" is also red in the outcomes test (the deposit not confirmed). Not switched
  off alone: the fresh transaction's checks against `Issued`, arrivals' own and the order's name, since the tests'
  identifier source never repeats (as `T-020-03b` found for intake's); and the question kept across tries (appended
  once, sent once, its answer read once), since intake's kit fails only verified entries on demand, never a stated one:
  switched off, the retry test stays green, its retries stopping after the question was answered, where the built
  `arrival` entry is what is kept.
- **The entry-point scan** (`T-021-01`'s) watches outer methods by name (`accept`, `grant`, `promote` and the
  others in `CARRY_OUT`), so the filing's last step is `take_acceptance`, not `accept`: a step of that name read as
  carrying out `change_catalogue: accept_module` without its effect.
- **Touches**: `components/core-orch/src/arrival/`, `components/core-orch/tests/arrival.rs` and these notes. No other
  row's module, no manifest, no contract and no `DEC-20` path.

### T-020-04, review answers (of `b5142fa`)

Each finding's probe was written first, against `b5142fa`, and run once: nine were red there,
`an_actor_stating_the_operators_role_is_not_the_operator`, `a_filing_refused_or_ended_leaves_the_deposit_to_its_reserve`,
`a_silent_reserve_or_a_refused_check_makes_an_arrival_wait`, `one_notice_is_one_arrival_whatever_figures_it_comes_with`,
`an_arrival_is_held_against_its_holders_order_not_yet_arrived`,
`an_automatic_deposit_routed_to_anyone_but_its_holder_is_refused`, `a_question_retried_opens_one_check`,
`a_stranger_is_told_nothing_of_a_receipt` and `another_login_of_the_filers_party_is_told_the_filing_as_it_stands`.
The tenth, `a_reserve_with_no_declaration_on_record_starts_nothing`, was green there, as finding 8 expected: it is red
under the reviewer's mutation (rule-off below). The cases added later in the same tests (a question under way taking
its order, an unfinished filing carried on by its filer alone, the order's record handed over before policy) came with
the code, and their rule-offs stand in.

1. **The operator is the deployment's operator.** A filer in the operator's role is the operator only where its party
   is the deployment's operator party (`Desk::operator`), and the register says it acts for it; a customer stating the
   operator's role files nothing. The log's `refiling-files-an-arrival-anew` reads the role alone; that half's owner is
   `T-020-01f` (the contract branch), row `refiling-files-an-arrival-anew`, and `contracts/` is untouched here.
2. **Nothing opened before policy permits; nothing stopped holds the deposit.** A filing is now taken, before any
   order exists, through every check that can refuse it: the filing for what arrived, the holder's session, the
   destination (answer 6), then policy. The order is drawn, its record handed to the assembly through a new port,
   `FiledOrders::opened`, and only then is the filer's authorisation asked, so policy's `Requester` and `OrderOrigin`
   inputs answer for an order the assembly couldn't otherwise know yet (the real authorise rule asks `OrderOrigin`).
   A refusal about the filer is `ArrivalError::Refused`, recording nothing of Core's; `outside_authorisation` an
   `order_refusal` naming no order; not known `Filed::Waiting` with no order. None of them holds the deposit: another
   filer files on it. Only a permitted filing holds it, and one that ends after (the reserve's no, no route, a refused
   acceptance) is refused naming its order and moved out of the held runs, so the deposit is free again; the same
   filing again is told how it ended. Arrivals takes no exposures, so ending releases nothing else. Tested with a
   stated operator refused (`not_permitted`) and an operator without the permission to act for users
   (`missing_input`, as policy answers one the register doesn't list for `ActForUser`), then the reserve's own
   filing starting; and with `outside_authorisation` and the reserve's no, then the reserve's filing starting.
3. **Silence and a check that can't go yet wait.** At an arrival, a question answered no, not at all, or not as yes or
   no, or a check the catalogue won't let out (no session, over the bound, the reserve unusable), leaves the arrival
   `Arrived::Waiting`, with nothing kept but its notice: the same report again reads `OpenOrders` again and asks again,
   under a fresh transaction, each a check the catalogue logs (a reading, reversible), so once no order is open it
   meets none (`REQ-ORD-14`), and once the reserve answers yes it is held. A question stopped where the log couldn't
   take an entry is carried on for its own order, never re-targeted. At a filing, silence or a check that can't go
   waits (`FiledWait::ReserveNotYetConfirmed`); the reserve's no ends the order (a reading, reversible: a definite no
   says the deposit's funds aren't there). **Sessions:** the check goes in the session `OpenOrders` names for the
   order, read on every call, so a held arrival whose session ended waits until the assembly names a session open for
   the holder's account. Core can't open one itself: `Sessions::open_session` takes the holder or its delegate as
   requester, and Core is neither. **Hand-off to `F-021`**: a session opened on the holder's behalf for an arrival's
   check (Core as a delegate the register lists, bounded to balance verifications of that account), with a later
   `F-020` row to ask for it when the order's session has ended.
4. **One notice, one arrival.** Notices are kept by their bytes: the same bytes with other figures are refused
   (`OtherFigures`), and so is a notice the evidence store finds restates one taken before (`Kept::restates`) with
   other figures. Before anything is kept, the notice must be its reserve's (its envelope's maker the report's reserve)
   and name no transaction in its body (`NotItsNotice`). Since the re-review, it must also be of no shape a contract
   names for something else: a reserve's catalogue statements (its advertisement, its declarations, its reserve
   declaration) would otherwise pass, being its own and naming no transaction, and so would an operation, a judgement,
   a confirmation, a release assertion, a decision record or a result of a known kind. Nothing else of the body can be
   read. **Gap, exactly**: a
   provider-role shape for a notice of value arriving, naming its outer node, asset, amount, account and a per-credit
   reference the reserve assigns, so Core reads the figures from the signed bytes rather than the port, and two notices
   of one credit, signed at different times, are known as one. Owner: a new `F-020` row with a provider-role bump. A
   balance verification's yes says only that the funds suffice (corrected above and in `confirm.rs`).
5. **Matching.** `OpenOrder` names the account's holder, and an arrival is held only against its holder's order. An
   order that already has its arrival (`matches`, recorded) is left out, and so is one a question under way would
   match, so two arrivals never both match one order; with two equal orders at one account each gets one, and a
   stranger's deposit meets none. Ambiguity is still resolved as before, first match else first open, not by holding
   nothing.
6. **The deposit goes to its holder.** Until the holder's own standing instruction exists (the coordinator's default,
   on the owner's form), a filing's beneficiary and requester are the account's holder, and its destination must
   resolve to an owner that is the beneficiary; another owner, or a refusal, is `destination_refused`, naming no order,
   and not yet resolved waits with nothing recorded.
7. **A check is opened only once its question is kept.** The question is signed and its bytes kept before
   `begin_check`, and the ticket is kept with the question across tries. The kit now fails a keep, or a stated entry's
   append, once on demand (`Store::fail_after`, `Book::fail_stated_next` in `tests/intake_support/`, both off by
   default: a seam in `T-020-03`'s test kit, additive, intake's and the rounds' tests run unchanged). The probe fails
   the question's keep, then its append, and counts one balance check logged.
8. **No declaration on record.** `a_reserve_with_no_declaration_on_record_starts_nothing` files at a reserve the
   catalogue holds no declaration of, and policy isn't asked; `.is_some_and` → `.map_or(true, …)` turns it red.
9. **What a filer learns.** The filer is held first, before any lookup: an actor that isn't the deployment's operator
   or a listed reserve is told `NotTheFiler` whatever the receipt. A listed reserve is told `NoSuchArrival` alike for a
   receipt with no arrival and one with an arrival at another reserve, so it learns only that it can't file there. The
   operator, who may file at every reserve, learns whether an arrival is recorded at a receipt. Corrected from "nothing
   is told", which held for strangers only.
10. **Order ids.** A new order id is held against `Issued`, arrivals' own transactions and every filing's order. Intake's
    orders aren't reachable from the desk; in the assembly intake and arrivals draw from one `Fresh`, whose contract is
    an identifier never drawn before, so a repeat is that source's fault, not a case to handle.

- **Nits.** `open_run` returns the refusal it recorded. Refused filings are kept by the filer's party and identifier, so
  they are bounded by the filings made. Another login of the filer's party repeating a filing is told how it stands,
  once it started or ended. (One under way was carried on only by the filer policy permitted, `OtherFiler`; the
  re-review's answer 1 lets any listed login of its party carry it on.)
- **A permitted filing that waits holds the deposit** (a silent reserve, a check that can't go), but not for ever, since
  the re-review's answer 1: it ends, recorded, and frees the deposit once its session ends (the check refused
  `NoSession`), once its reserve is past its time to answer a question it left unanswered, or once no login of its
  filer's party that filed or carried it on is listed; and a permit it rests on holds no longer than its session.
- **F2's "operator without `ActForUser`"** is policy's fake answering `missing_input`; core-orch can't reach
  core-authz (the boundary check), so the real engine's answer is `T-024-04`'s (`AC-11`).
- **Changed meanings, deliberately.** Another reserve now gets `NoSuchArrival`, not `NotTheFiler` (answer 9); a
  filing refused as `outside_authorisation`, or with its destination refused, names no order; not yet resolved, or
  policy not known, waits with no order; a reserve no longer needs to act for itself (its party is held to the
  arrival's reserve, and the register to its party), which no test could tell apart.
- **Wiring for the assembly**, added to the list above: `Arrivals::file` takes a `FiledOrders`, which must put the
  record into policy's order records before the authorisation it precedes, and drop it again where no order opened
  (`Err(Refused)`, `Err(Log)`, `Filed::Waiting` or `Filed::CantProceed` with no order), since each new filing draws a
  new id and such records would pile up; the operator's login needs `ActForUser`
  listed, or AC-11 fails in `T-024-04` (the assembly lists only `Change` and `ReadOrderState` for it today);
  `OpenOrder` now carries the holder.
- **Rule-off**, every rule of the module switched off once against the final tests, the file restored and touched
  (48 rules, each red):

  | Rule | Red |
  |---|---|
  | Held only at its reserve, node, asset and account | `unattributed_arrival_creates_no_order` |
  | Held only against its holder's order | `an_arrival_is_held_against_its_holders_order_not_yet_arrived` |
  | An order with its arrival left out | the same |
  | A question under way takes its order | the same |
  | Matched only at the amount stated | the short, excess and split test, the bound-answer test, the refiling test, the waiting test |
  | A held arrival cites the reserve's answer | the bound-answer test, the short, excess and split test |
  | The question appended before it is sent | thirteen tests (the log refuses the answer) |
  | Held only on the reserve's yes | the bound-answer test, the release test, the outcomes test |
  | The check opened only once the question is kept | `a_question_retried_opens_one_check` |
  | A question under way kept across tries | the same |
  | An unanswered arrival waits, not kept | `a_silent_reserve_or_a_refused_check_makes_an_arrival_wait`, the matching test |
  | A check that can't go waits, not kept | the waiting test |
  | One notice, one set of figures | `one_notice_is_one_arrival_whatever_figures_it_comes_with` |
  | The notice is its reserve's own, about nothing | the same |
  | The same notice taken once | the same, `same_notice_again_is_recorded_once`, the matching test |
  | The operator is the deployment's operator | `an_actor_stating_the_operators_role_is_not_the_operator` |
  | The filer acts for its party | the stranger test, the filing test, the refiling test |
  | A reserve files only at itself | the same three |
  | `refile` holds the filer before the lookup | the stranger test, the refiling test |
  | `file` holds the filer before the lookup | the stranger test, the filing test |
  | Only a differing arrival filed anew | the refiling test |
  | One refiling per arrival | the refiling test |
  | No declaration on record is none (`map_or(true, …)`) | `a_reserve_with_no_declaration_on_record_starts_nothing` |
  | The declaration gate | the same, `automatic_deposit_at_a_reserve_that_never_declared_it_starts_nothing` |
  | Filed only on an arrival with no order | the filing test |
  | One permitted filing per arrival | the filing test |
  | Another login of the party told the filing | `another_login_of_the_filers_party_is_told_the_filing_as_it_stands` |
  | An unfinished filing carried on by its filer alone | the same |
  | A filing that ended frees the deposit | `a_filing_refused_or_ended_leaves_the_deposit_to_its_reserve` |
  | Filed for the amount that arrived | the filing test |
  | The requester is the holder | the filing test |
  | The beneficiary is the holder | `an_automatic_deposit_routed_to_anyone_but_its_holder_is_refused` |
  | The session is the holder's | the filing test |
  | The destination's owner is the beneficiary | the holder test |
  | A refused destination is refused | the outcomes test |
  | A destination not yet resolved waits | the outcomes test |
  | The order's record handed over before policy | the acceptance test |
  | Nothing before policy permits | the release test, the outcomes test |
  | `outside_authorisation` refused, naming no order | the release test, the outcomes test |
  | A refusal of the filer records nothing and holds nothing | the release test, the outcomes test |
  | Policy not known waits | the outcomes test |
  | The deposit held before it is routed | seven filing tests (the log refuses the acceptance) |
  | A silent reserve makes the filing wait | the waiting test, the login test |
  | The reserve's no ends the order | the release test, the outcomes test |
  | The basis `automatic_deposit` | seven filing tests |
  | The acceptance names its filer | the acceptance test |
  | The acceptance and start kept across tries | `automatic_deposit_filing_is_carried_on_where_it_stopped_asking_no_one_twice` |
  | The start is the filer's statement | the acceptance test |

  Not switched off alone: the restatement path (`Kept::restates`; the kit's store never answers one), the fresh order
  id's checks and the fresh transaction's (the tests' identifier source never repeats), and a colleague told a refused
  filing as it was (the refused filings are matched by the same function as the started ones, which is switched off).
- **Touches**: `components/core-orch/src/arrival/`, `components/core-orch/tests/arrival.rs`,
  `components/core-orch/tests/intake_support/mod.rs` (the two opt-in failures, answer 7) and these notes. No contract,
  manifest or `DEC-20` path.

### T-020-04, re-review answers (of `f307b2d`)

Each probe was written first and run against `738cbfe` (with the one new error variant, `NotFileable`, added so they
compile): eight red, `a_waiting_filing_whose_session_ends_ends_and_frees_the_deposit`,
`a_waiting_filing_whose_reserve_stays_silent_past_its_time_ends`,
`a_waiting_filing_is_carried_on_by_its_partys_logins_and_ends_with_none_left`,
`an_order_that_never_opened_is_withdrawn_from_the_assembly`, `a_deposit_its_reserve_said_no_to_is_filed_on_no_more`,
`a_deposit_with_an_acceptance_recorded_is_filed_on_no_more`, `a_filing_carried_on_after_its_session_ended_isnt_accepted`
and `a_reserves_advertisement_is_no_arrival_notice`. `a_restated_notice_is_the_same_arrival` was green there (the path
existed, untested); its rule-off stands in.

1. **A waiting filing ends** (the coordinator's defaults), each time recorded as an `order_refusal` naming its order,
   `no_route_right_now`, so the log holds no permitted order in limbo, and the deposit is free again:
   - on a check refusal that can't lift for that filing: `NoSession` (its session ended), `SessionNotForAccount` or
     `NotACheck`. Other refusals (the account's bound, the reserve unusable, the operator's settings) wait;
   - once a question it asked went unanswered and the catalogue now holds the reserve unavailable
     (`reserve_eligibility`, past the operator's time to answer, `REQ-CAT-19`): it ends before asking again;
   - once no login of the filer's party that filed or carried it on is listed. Any listed login of that party carries
     it on (no more `OtherFiler`), and the acceptance, where not yet built, names it. *Round 3 answer 1: such a login
     gives its own authorisation first.* Core knows a party's logins only by those that filed it, so "none left" is read from them: a
     reading, reversible. Another filer's filing ends it then, and goes on as its own.
2. **`FiledOrders::withdrawn`.** Called wherever `opened` was and no order opens: a policy refusal of either kind,
   policy's not known, and an authorise the log couldn't record (`Err(Log)`). Tested with policy unreachable; the
   `Err(Log)` path isn't switched off alone (the kit's policy can't fail its own append on demand).
3. **No re-asking without limit.** Once the reserve said a deposit's funds aren't there, the deposit is filed on no
   more (`ArrivalError::NotFileable`), asking no one and recording nothing; a new notice is a new arrival, and is filed
   on. Five filings after the no ask nothing more. This changes the meaning of the release test's "no" case,
   deliberately: the reserve's next filing on that deposit is refused, and one on a new notice starts.
- **Nits.**
  - The kit's store can now answer `restates` once on demand (`Store::restates_next`, off by default), and
    `a_restated_notice_is_the_same_arrival` takes the restatement path: the same figures are the same arrival, other
    figures are refused.
  - **One acceptance per deposit.** Where the log took the acceptance and refused the start, the order ends and the
    deposit is filed on no more (`NotFileable`), so no second `automatic_deposit` acceptance rests on it. Reusing the
    first acceptance for another order would need the arrival link core-internal lacks (the gap above).
  - **A permit doesn't expire**: `Decision` and `Permit` carry no time bound. Default, reversible: a permit holds no
    longer than the filing's session. Before the acceptance is built, the session must still be open for the holder;
    otherwise the order ends rather than be accepted, and a new filing, in a new session, asks policy again.
- **Records.** The waiting passage above is rewritten; the statements that would have passed as notices are named,
  and `its_own_notice` refuses them; the log half of finding 1 has its owner.
- **Rule-off**, every rule switched off once against the final tests (the 47 of the review answers, one retired with
  `OtherFiler`, and 15 new), each red. The review answers' table stands for the 47; the new:

  | Rule | Red |
  |---|---|
  | A check that can't go for the session ends the filing | `a_waiting_filing_whose_session_ends_ends_and_frees_the_deposit` |
  | Past the reserve's time to answer, the filing ends | `a_waiting_filing_whose_reserve_stays_silent_past_its_time_ends` |
  | An unanswered question is remembered | the same |
  | Any listed login of the party carries it on | `a_waiting_filing_is_carried_on_by_its_partys_logins_and_ends_with_none_left`, the login test |
  | The acceptance names the login that carried it on | `another_login_of_the_filers_party_is_told_the_filing_as_it_stands` |
  | With no listed login left, the filing ends | the carried-on test |
  | The logins held to the register | the carried-on test |
  | An order never opened is withdrawn | `an_order_that_never_opened_is_withdrawn_from_the_assembly` |
  | The reserve's no spends the deposit | `a_deposit_its_reserve_said_no_to_is_filed_on_no_more`, the release test |
  | A spent deposit is filed on no more | the same, `a_deposit_with_an_acceptance_recorded_is_filed_on_no_more` |
  | An acceptance recorded spends the deposit | `a_deposit_with_an_acceptance_recorded_is_filed_on_no_more` |
  | The session holds the permit to the acceptance | `a_filing_carried_on_after_its_session_ended_isnt_accepted` |
  | No catalogue statement is a notice | `a_reserves_advertisement_is_no_arrival_notice` |
  | No result of a known kind is a notice | the same |
  | A restated notice is the same notice | `a_restated_notice_is_the_same_arrival` |

- **Touches**: `components/core-orch/src/arrival/`, `components/core-orch/tests/arrival.rs`,
  `components/core-orch/tests/intake_support/mod.rs` (one more opt-in knob) and these notes. No contract, manifest or
  `DEC-20` path.

### T-020-04, round 3 answers (of `be17997`)

Both probes were written first and run against `be17997`, and both were red:
`a_login_carrying_a_filing_on_gives_its_own_authorisation` (the acceptance named a login policy never authorised) and
`a_quiet_filing_whose_session_ended_gives_way_to_another_filer` (`Conflicting`). Its second case, the reserve past its
time, came with the code; its rule-off stands in.

1. **A login carrying a filing on gives its own authorisation** (`REQ-AUZ-9`). When a login other than the one the
   acceptance would name carries a filing on, before its acceptance is built, policy is asked again: the same
   authorisation's terms, under a fresh id, given by that login. On a permit the login joins the filing's logins, the
   acceptance names it, and the acceptance names that authorisation, so its `who` and the authorisation's `given_by`
   are one login. A refusal is told as `ArrivalError::Refused`, and not known as `Filed::Waiting` with
   `PolicyNotKnown`; either way nothing moves on that call and the filing keeps the logins it had. Only logins policy
   permitted count towards "a listed login left". Once the acceptance is built, a login of the party carries on the
   appends as built, asking nothing.
2. **A quiet filing gives way.** Before another filer is told `Conflicting`, the filing holding the deposit is held to
   its end conditions, the same as when its own filer carries it on: its session over (policy's register no longer
   holds it open for the holder), its reserve unavailable after a question it left unanswered, or no login of its
   party that policy permitted still listed. If one holds, it ends, recorded (`order_refusal` naming its order), and
   the new filing goes on as its own. Read from the register and the catalogue without asking anything, so a check
   that could never go for its session (`SessionNotForAccount`, `NotACheck`) is found only when the filing asks again;
   its session ending is the case a quiet filing meets.
- **Readings recorded.**
  - **A no is for the deposit, not the credit.** After the reserve's no, the deposit is filed on no more, even if its
    credit was still pending at the reserve when it answered: once the credit clears, the reserve's new notice is a new
    arrival, filed on as any other. Reversible; a re-ask on the same notice would need the reserve's word that its
    no has lifted, which provider-role's balance answer doesn't carry.
  - **`no_route_right_now` as a final end.** Every end of a permitted filing (its session over, the reserve past its
    time, no login left, the reserve's no, a check that can't go, a refused acceptance or start) is recorded as an
    `order_refusal` of `no_route_right_now`, because `order_refusal`'s classes are a closed list of eight and none
    names these causes. **Item for the contract author** (`T-020-01f`): a core-internal class for an automatic deposit
    that couldn't be held or carried on, so a reader tells "the same request may route later" from "this filing ended".
- **Rule-off**, the new rules switched off once against the final tests, each red; the earlier rules rerun, each red
  (the re-review's "with no listed login left, the filing ends" now runs through answer 2's check and is switched off
  as its last row):

  | Rule | Red |
  |---|---|
  | A carrier asks policy for its own authorisation | `a_login_carrying_a_filing_on_gives_its_own_authorisation` |
  | A refused carrier carries nothing on | the same |
  | The acceptance names the carrier's authorisation | the same |
  | The end conditions checked before `Conflicting` | `a_quiet_filing_whose_session_ended_gives_way_to_another_filer`, the carried-on test |
  | A session over ends a quiet filing | `a_quiet_filing_whose_session_ended_gives_way_to_another_filer` |
  | A reserve past its time ends a quiet filing | the same |
  | No listed login left ends a quiet filing | `a_waiting_filing_is_carried_on_by_its_partys_logins_and_ends_with_none_left` |

- **Touches**: `components/core-orch/src/arrival/`, `components/core-orch/tests/arrival.rs` and these notes.

### T-020-04, round 4 answers (of `4457fcf`)

Both probes were written first and run red against `4457fcf`:
`a_filing_with_its_acceptance_written_is_finished_not_ended` (the reviewer's sequence: the start's answer lost, the
operator struck from the register, then the reserve's filing `Started`, two acceptances and two orders on one deposit,
both admitted by the log; and the same through the session ending) and
`logins_policy_permitted_carry_a_filing_on_without_asking_again` (policy asked four times for two logins).

- **A filing whose acceptance is built is finished, never ended.** Once its acceptance and start are built they may be
  in the log, so the filing is only finishing its last entries: its end conditions (session over, reserve past its
  time, no login left) don't apply to it, before `Conflicting` or anywhere. Another permitted filer of the deposit
  (the operator, or the reserve it arrived at) is told `Conflicting` naming its order, and its call first appends the
  entries the filing has left, as built, so Core finishes it on the next call of anyone who may file there, as well as
  on its own filer's.
- **Backstop.** Wherever a filing ends after its acceptance was built (a refused acceptance or start among them), the
  deposit is filed on no more (`NotFileable`), since that acceptance may be in the log. Stricter than the re-review's
  "the acceptance was appended": a refused acceptance after an earlier try whose answer was lost may be in the log too.
- **Every permitted login kept** (the nit). The filing keeps each login policy permitted with the authorisation it gave;
  any of them carries it on under its own authorisation without asking policy again, the acceptance naming that
  login and that authorisation. Only a login never permitted on it is asked about.
- **Rule-off**, each switched off once against the final tests:

  | Rule | Red |
  |---|---|
  | A finishing filing is finished on another filer's call, never ended | `a_filing_with_its_acceptance_written_is_finished_not_ended` |
  | Never ended once its acceptance is built, both guards off together | the same |
  | An acceptance built spends the deposit if the filing ends | `a_deposit_with_an_acceptance_recorded_is_filed_on_no_more` |
  | A permitted login carries it on without asking again | `logins_policy_permitted_carry_a_filing_on_without_asking_again`, the retry test |
  | A permitted login carries it on under its own authorisation | `logins_policy_permitted_carry_a_filing_on_without_asking_again` |

  The guard inside `should_end` alone stays green: `file()` takes a finishing filing before it asks `should_end`, so
  that guard is defence in depth, red only with the first switched off too (the second row). Every earlier rule was
  rerun and is red, but for the re-review's "any listed login carries it on" and "an acceptance recorded spends the
  deposit", which round 3's carrier rules and this round's backstop replaced.
- **Touches**: `components/core-orch/src/arrival/`, `components/core-orch/tests/arrival.rs` and these notes.

### T-020-04, round 5 (approved; the low before merge)

- **Spent only once the acceptance may be written.** Round 4's backstop spent the deposit once the acceptance was
  *built*, so an acceptance the log refused on its first append, never written, stranded the deposit. Now the filing
  notes its acceptance as possibly in the log once an append of it was taken or came back with the log unavailable
  (its answer lost); only then does ending the filing spend the deposit. A refusal on the first append leaves the
  deposit free for another filing. The reviewer's probe, `an_acceptance_refused_on_its_first_append_leaves_the_deposit_free`
  (`refuse_next = Some("acceptance")`, then the reserve files and starts), was written first and red on `8da7a6a`
  (`NotFileable`). `an_acceptance_whose_answer_was_lost_spends_the_deposit_if_its_filing_ends` (written, its answer
  lost, then refused on the retry) came with the change and was green before it, the wider rule covering it; its
  rule-off stands in.
- **Rule-off**, each red: a refused first append leaves the deposit free (`if true`: the first test); an acceptance
  taken spends it (`a_deposit_with_an_acceptance_recorded_is_filed_on_no_more`); an acceptance whose answer was lost
  spends it (the second test); the spend itself on ending (round 4's row, rerun on the new flag).

### T-020-04, merged with `m3` at `8f1067e` and wired into the assembly

- **Merge** (`eca97db`): no conflicts; `T-024-02`, `T-021-04`, `T-020-09` and `T-020-11` came in, and every test of
  both sides is kept and green.
- **Wired** (`conformance/scenarios/src/assembly/arrivals.rs`, and the assembly's `Core`): `Arrivals` held beside
  intake and run on intake's desk (taken out for the call, since `with_desk` hands its closure intake alone);
  `Core::arrive` and `Core::file`; `FiledOrders` on policy's order records, `opened` putting the record in and
  `withdrawn` taking it out; `OpenOrders` from intake's started orders whose first segment hasn't settled, each with its
  requester as the holder and the conformance account; `Segments::follow` fed from `Arrivals::start`, the filed
  order's origin, session and inputs from its filing and what it delivers from its chosen candidate; and
  `Permission::ActForUser` listed for the operator's login. `follow` was split so intake's orders and filed ones share
  `follow_on`. A smoke test, `assembly_arrivals.rs`, runs on the real log: a notice signed by the simulated reserve's
  key is verified and recorded as an arrival with no order, and the operator's filing on it is refused
  `NotDeclared`, by Core's gate over the simulated reserve's real declaration. It came with the wiring.
- **Merged `m3` at `ec9dbd2`** (`T-021-05`): `Actors::acts_for` now takes the actor's kind, so a filer is held to the
  register by its login, party and kind, as intake's callers are; the arrival tests strike a login by its three
  fields. The notes' conflict kept both sides.
- **Left for `T-024-04`** (`AC-10`, `AC-11`, `AC-26`):
  - a simulated reserve that declares it starts routes on automatic deposits: every simulated reserve (the stand-in's
    declaration) says no, so no automatic deposit can start in the assembly yet (`T-024-01`'s kit);
  - a simulated reserve that reports value arriving, with a notice, and answers a balance verification at an amount;
    today a scenario builds the `Report` and signs its notice with the reserve's key, as the smoke test does;
  - a reserve filer listed in the register (`Registers::list` of an actor whose party is the reserve's id), and the
    real engine's answer on `ActForUser` and on the reserve's own authorisation (`AC-11`);
  - filed orders in `OpenOrders`: only intake's orders are read as open, so an arrival after an automatic deposit's own
    order started meets no order.

### T-020-04, on the contract head (`acde1cf`, integration)

- **A deposit is known by its signed transaction.** A reserve's notice names the transaction the reserve gave the
  credit, and Core appends it about that transaction alone, bound to no order; a notice naming none, or of the `result`
  shape (which answers a question Core asked under its transaction, `answer-follows-its-question`), isn't taken. Core
  keys a deposit by its reserve and that transaction, whatever bytes carry it, the key `one-order-per-deposit` reads
  (`deposit_of`); the same deposit with other figures is refused. Every `arrival` names the deposit's notice by its
  commitment (`arrival-names-its-notice`), and one held against a part of a split order names the part
  (`OpenOrder.part`).
- **One deposit, one order, superseding round 5.** Once a filing held the deposit against its order (the `arrival`
  naming the order is in the log), the deposit is filed on no more, whatever comes of that order: the log refuses a
  second arrival naming an order on the same deposit. Round 5 left the deposit free when the acceptance's first append
  was refused; that history the contract now refuses, so the round-5 test is replaced by
  `one_deposit_opens_one_order_whatever_comes_of_it`.
- **An automatic deposit is routed as one part.** A split route of an order accepted on an automatic deposit rests on
  an arrival naming each part (`split-deposit-arrival-names-its-part`), and one deposit has one arrival naming an order
  (`one-order-per-deposit`), so one deposit can fund one part only: the filing routes under `max_parts` 1. A split
  automatic deposit is one deposit per part, each its own notice. The rule-off stays green on the kit, whose bounds are
  one part already.
- **The refiling row, handed to `T-020-01f`.** `refiling-files-an-arrival-anew` compares a filer's party with the
  operator `admit_entry` is handed, which the real log gives as `Settings.operator`, the operator's statement maker
  (`conformance-operator` in the assembly), not its party (`operator`): a genuine operator login's refiling would be
  refused by the log while Core takes it. The core-orch kit can't show it, its operator's maker and party being one.
  Sent by the coordinator to the contract author; `contracts/` is untouched here.

### T-020-03, tracked from `T-021-05`: intake's status reads bypass an agent's read scope

- **Open item for `T-020-03`'s owner, due before `M4`.** Intake answers an order's status (`Told`) from `open`,
  `quote`, `accept` and `Intake::told`, and finds an order for its caller through `kept_for`, holding the caller only
  to the register of actors (`Actors::acts_for`, which since `T-021-05` binds the login, the party and the kind). None
  of it asks policy, so an agent its party didn't scope for `check_read` (`T-021-05`'s `Scope::CheckRead`) still learns
  the status of its party's orders through intake. It can't move value, authorise or charge outside its scope: policy
  decides each of those.
- **Severity, the reviewer's call: low.** The agent's own party's orders only, and read only.
- **Why `T-021-05` didn't route it through `check_read`.** A customer's read of an order asks the log's record of the
  order's parties (`Source::OrderParties`), and intake answers status before the log holds the order's acceptance, so
  every customer would be refused its own new order. The fix needs either a read of an order's status that policy
  decides on intake's own record (a source the assembly answers from intake's `OrderRecord`), or intake answering
  status only once the order is accepted and in the log. `F-021`'s notes, under `T-021-05`'s re-review, hold the
  rest.

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
- **Gap, closed by `T-020-10b` (below): waiting past the floor had no state-machine input.** At state-machine
  2.1.0 a part held past its floor stayed `first_segment_settled` in the order Core follows, held only in Core's
  memory. Since state-machine 3.0.0 the wait is a recorded `waiting_at_floor` input.
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

### T-020-10b, waiting at the floor as a recorded state (state-machine 3.0.0, `m3` at `8cac73f`)

- **The wait is a state-machine input.** Where Core held a part past its floor in memory (`segments::Waits`, and
  `Recalculations`' own map), it now records `waiting_at_floor {}` about the part, through one call,
  `Segments::wait_at_floor`: Core's input on the value the part's last settled segment evidenced at its node, that
  segment's step, built once and appended until the log takes it, and taken by the order Core follows on append,
  so the part is `waiting_for_customer` with its `waiting_since` (`REQ-ORD-27`, `AC-12`, `AC-42`). Both places:
  - between segments, `Segments::run_part` records it when a settled segment's projection is below `Live::floor`,
    and hands nothing more over;
  - at a recalculation, a routing whose every candidate was dropped `below_floor` records it before answering
    `Recalculation::WaitsForCustomer`; a wait the log couldn't take is kept (the deciding record and the segment it
    rests on), and the next call records it as built, routing nothing again.
  `segments::Waits` is gone; `Segments::waiting` answers the part's `waiting_since`, from the order Core follows, and
  `Segments::status` shows the customer the order waiting with its age. `Recalculations::waiting` is gone.
- **A waiting part takes no move, in Core too.** `Segments::run_part` hands no segment over while the order Core
  follows has the part waiting for the customer, for any cause, mirroring the log's `held-part-takes-no-move`; a
  recalculation reads the part `Held` (`evidenced`).
- **The log's rows.** The input is admitted on `waiting-at-floor-rests-on-the-customers-floor` (the kit's
  authorisation names the floor under `Core::under`), `part-waits-for-value-in-motion` (the wait is recorded only on
  a settled segment, with nothing of the part in motion: the gate runs after a settlement, and a recalculation only
  on value evidenced with nothing handed over), and `input-is-one-the-state-machine-takes` (`after_waiting_at_floor`
  takes a part whose value is at a node). Every scenario's replayed log was admitted; no history was refused, so none
  needed classifying.
- **One reading corrected in a test, not in Core.** The new retry test first asserted the routing asked the reserve;
  under the operator's fee of 10 000 the shortlist already drops every candidate below the floor, so no question is
  sent. The test now counts the routing's decision records instead (once, not again on the retry). A test
  artefact.
- **Tests.** Rewritten to read the recorded state, not `Waits`: `settled_segment_that_crosses_the_floor_makes_the_order_wait`
  (one `waiting_at_floor` about the part, the part waiting since its `recorded_at`, the customer's status
  `waiting_for_customer` aged 3 600 seconds an hour on, what is evidenced read from `Segments::handed_on`),
  `part_run_through_the_segments_alone_waits_at_its_floor` (the wait recorded once over two runs),
  `recalculation_that_would_pass_the_floor_waits_for_the_customer`, and the assembly's
  `floor_the_authorisation_names_holds_the_part_at_the_segment_that_crosses_it`. New, each written first and seen
  red: `floor_wait_whose_append_failed_is_recorded_once` and
  `recalculated_floor_wait_whose_append_failed_is_recorded_once`. The kit gained `On::FloorWait`.
- **Rule-off, each red:** the wait recorded between segments (both floor tests and the retry test); the wait
  recorded at a recalculation (its two tests); the wait kept for a retry (the recalculated retry test); a waiting
  part handed nothing over (the two-runs test, whose last run names no floor: with Core's guard off, the log refused
  the deposit's move on `held-part-takes-no-move`, the row behind it).
- **Touches:** `components/core-orch/src/segments/mod.rs` (`T-020-11`'s last; `T-020-10`'s seam), `recalc/mod.rs`,
  the recalc tests, the kit's `On`, and the assembly's floor test (`F-024`'s file). No contract, manifest or
  `DEC-20` path.

### T-020-10b, review answers (of `ddae960`)

Each test was written first and seen red on `ddae960`, then fixed; each rule was then switched off once and its test
seen red again.

- **Medium, a second wait at one settled segment was swallowed.** The wait was the settled segment's step
  `"floor"`, so a part that waited at a node, was given a new route from it, and fell below its floor there again
  got the first wait's receipt back: `WaitsForCustomer`, nothing recorded, the part not waiting. Waits are now their
  own record (`Segments`' `floor_waits`, per part, in order): a wait recorded while the part still waits is the
  answer; once the part no longer waits, a new wait is built, a second input with its own record id and time. The
  customer's new route (`T-020-13`, not built) is recorded directly in the test. Test
  `second_wait_at_the_same_settled_segment_is_recorded_as_its_own`.
- **Low, a refused wait was tried for ever.** A wait the log refuses is dropped, as a refused `route_recalculated`
  is, and a recalculation drops what kept it (`Followed::waiting`), so the next try routes the part afresh. Before
  a retry, the part is read again: where the order Core follows no longer takes `waiting_at_floor` (the part is
  stuck, or moved on), the wait is dropped and never appended (`SegmentsError::WaitNotTaken`), and a recalculation
  answers why (`NotNow(Held(Stuck))`). Tests `floor_wait_the_log_refuses_is_dropped` (the next run's wait is judged
  then, its `recorded_at` the later time, not the refused one again),
  `recalculated_floor_wait_the_log_refuses_is_routed_afresh` (written after the fix; seen red by rule-off), and
  `floor_wait_retried_after_its_part_was_declared_stuck_is_dropped` (the dispute declaration, `T-020-12`'s, observed
  in Core's followed order only, so the test doesn't replay the log).
- **Low, a wait built and not recorded wasn't remembered.** `Segments::run_part` hands nothing over while the part
  has a wait built and not yet recorded (`Segments::floor_wait_pending`), whatever `Live::floor` says now: it records
  that wait, or drops it where it no longer applies, first. Test
  `floor_wait_built_and_not_recorded_holds_the_part_whatever_floor_a_rerun_names`, the rerun's floor lowered to
  985 000.
- **Rule-off, each red:** each wait its own (the second-wait test); the refused wait dropped (both refusal tests);
  the part read again (the stuck test); a pending wait holding the part (the rerun test); a recalculation's refusal
  clearing what kept it (the routed-afresh test).

### T-020-10b, the approval's two lows (of `539576f`)

Each test was written first and seen red on `539576f`; each rule was then switched off once and its test seen red.

- **A stale pending wait outlived a recalculation that found a route.** When the order Core follows takes a
  `route_recalculated` for a part (`Segments::observe`), the part's wait built and not yet recorded is dropped: it
  was judged on the route replaced, and the new route's floor is judged afresh. Test
  `pending_wait_goes_when_a_recalculation_finds_a_route_that_holds_the_floor`: the wait built under a run's 995 000,
  the log unavailable, the recalculation from the ledger under the authorisation's 985 000, and the new route's run
  hands its deposit over with no wait recorded.
- **A benign drop surfaced as an error.** Where `run_part` meets a pending wait the order Core follows no longer
  takes (`WaitNotTaken`), the wait is dropped and the same run goes on: the floor it judged at the segment before
  held. Test `pending_wait_no_longer_taken_lets_the_run_go_on`, with Core's copy of the order rebuilt from its route
  alone so it reads no value at the ledger to wait on (the one way a test reaches the case today).

## T-020-11

- **One record, `stuck::Stuck`** (`components/core-orch/src/stuck/`). Core's record of the parts it declared stuck,
  the decisions it holds for their resolution, the moves in progress and each tuple's settlement history, which the
  production assembly keeps durably (`F-030`), as `Segments`. What it reaches comes in `stuck::Parts` (Core's clock,
  the log, fresh ids, `Segments`, `Exposures`); policy's `StuckOrders`, the operator's `Operator` channel and the
  `Resolutions` that carry a resolution out are handed to the calls that use them.
- **No route left** (`Stuck::no_route`, `REQ-ORD-28`). Only a recalculation's `NoRouteLeft`, `NoRoute` or `Rejected`
  records the `no_route` input (`StuckError::RouteLeft` otherwise); it cites the routing's decision record, or, past
  the bound, where nothing was routed, the order's last decision record, which the caller names
  (`NoRouteLeft::last_record`). The state machine places the part: stuck, or waiting for the customer at a failed
  final deposit's node (`REQ-ORD-43`; the kit's single-reserve route always ends there, so the stuck test leaves the
  deposit never handed over, its begin refused, and the routing offering nothing from the ledger). Nothing is rolled
  back: no reversal, no hold released, nothing handed over. The value resting at the node is held there under the
  settled segment that took it there (`Exposures::hold_resting`, an additive seam in `T-020-05`'s module), so the
  ceilings in flight count it. Built once and appended until the log takes
  it; before it is appended the part is read again, and an input it no longer takes is dropped
  (`StuckError::NotApplicable`). Once taken, the order Core follows takes it (`Segments::observe`), and a stuck
  order is marked in `Exposures`. Reading, reversible.
- **`T-020-10`'s hand-offs.** `NoRouteLeft` is recorded as above. A stuck or waiting part isn't recalculated: the
  part's `evidenced` already refused one `Held`, and now holds, since the order Core follows takes `no_route` and
  `stuck_declared` (the no-route test sees `NotNow(Held(Stuck))`, no one asked). A retried recalculation's append
  re-reads the part since `T-020-10`'s review, so a part declared stuck in between reads `Held`; no test stages it,
  since this row's declarations can't reach a part whose recalculation waits on the log (one needs the value
  evidenced, the other a segment in flight or no route), and `dispute` is `T-020-12`'s.
- **`T-020-08`'s hand-off: what stays in flight.** A segment carried out with no final outcome (no result that
  counts, an outcome unknown, waiting on the person) is timed from when it was carried out
  (`Segments::carried_since`); past its time the part is declared stuck, and only its resolution moves the value.
- **Over time** (`Stuck::watch`, `REQ-ORD-29`). The clock runs on twice the p99 of the segment's tuple (provider,
  from, to) once that tuple has `HISTORY` (20) settlements, over its latest `WINDOW` (200), and on twice its
  advertised time (the router's `declared_times` over the chosen record) until then; keyed by tuple, so a reserve's
  withdrawal history doesn't time its deposit. The p99 is by nearest rank, so below a hundred samples it is the
  slowest; over the window it falls as well as rises. Both figures are the coordinator's defaults, reversible, on
  the owner's form. Whole seconds, Core's
  clock; strictly longer than twice: at exactly twice it isn't stuck. An advertised time the record can't give
  (`u64::MAX`) times nothing: no contract-valid record lacks one. The figures (`stuck_call`) are appended before the
  `stuck_declared` (`over_time`) that rests on them, each built once. Readings, reversible.
- **The operator's warning, a reading; hand-off to `T-023-01`.** No `Raise` difference fits a stuck order, so the
  warning is the declaration handed back (`Watch::Stuck(StuckCalled)`, with its figures and both receipts), the
  `stuck_declared` input, and the order on the operator's stuck view at once. Delivering it to the operator is
  `T-023-01`'s: `AC-41` notifies the operator of every state change, this one included.
- **A material change in a p99** (`Stuck::learn`, `REQ-ORD-6`, `THR-24`). Each settled segment's time
  (`Segments::settlements`, in the log's order) joins its tuple's history once. The first p99, formed at `HISTORY`
  settlements, is compared with the advertised time the settlements declared, and raised if it differs by
  `MATERIAL_BPS` (half) of it, citing the settlement that formed it; it is the baseline from then. Each later p99,
  over the window, is compared with the baseline, and raised if it moves by half of it, either way: up, or down
  once the slow settlements leave the window (unit test `p99_falls_after_a_long_slow_history_and_the_fall_is_raised`;
  before the review, with no window, it could never fall, so "either way" held only upward). A raise names the
  provider, cites the settlement the baseline rests on and the one that moved it, and is recorded `done` or `failed`
  as the channel answered; the new p99 is the baseline from then. A raise whose
  entry the log can't take now is appended as built, never raised twice; one the log refuses is dropped. Detection
  never holds up the clock: `watch` times the part whatever learning answered, and `learn` answers its failure
  when called on its own (`REQ-ORD-9`'s "only detects"; on the advisor's review). Readings, reversible.
- **By value and age** (`Stuck::stuck_values`, `REQ-OPS-2`, `THR-58`). A stuck part's value is what it holds,
  in flight or at rest at its node (`Exposures::stuck`), less what its resolutions moved. Ranked within each asset,
  largest first, then oldest; no rate converts across assets. Age from the first declaration. Like
  `Exposures::stuck`, the view is Core's own read, with no read check: the operator's console that shows it is a
  later row's. An exchange in flight is held in both its assets; moving the whole of it in either resolves the
  part, since it is one value. Reading, reversible.
- **Visible to the customer and the provider** (`REQ-ORD-6`). The customer reads its order's state through
  `Segments::status`; the provider reads its own segment through `stuck::seen_by_provider`, a `Reads::Segment` read
  naming the provider as the order Core follows has it, never the reader's word. Each read is recorded either way.
- **Resolution, in two steps** (`REQ-AUZ-4`, ADR-0011). `Stuck::decide` asks policy's `decide_stuck` only for a part
  Core declared stuck, still stuck in the order it follows and unresolved, with no move of it in progress or of
  unknown outcome, naming evidence, delivering to the order's destination or its origin exactly, moving no more than
  the part still holds in the asset named, through a provider of the part (`THR-77`'s "to a friend"). The two ends
  come from Core's records, never from the caller (`Stuck::follow`): the destination is the order's authorisation's,
  held to every part's destination in the order Core follows; the origin is the reserve and outer node the order's
  withdrawal took the value from, held to the authorisation's origin reserve. The destination's account is the
  log's own record: the order's latest permitted `authorise` entry (`Stuck::follow` reads the log's entries as the
  log holds them, as `T-021-04`'s engine does). The origin's account isn't in the log as such: the log holds the
  withdrawal Core signed only by its stated entry's commitment, its bytes in the evidence store. So the origin's
  account is checked at each decision against the accounts that operation names among its inputs, as Core kept it
  (`Segments::sent`); a residual: any account the withdrawal named passes, a beneficiary's included, where the
  tuple's terms make the withdrawal carry one. The assembly follows the order when it takes it up; the ends first
  followed stand, and others are refused. A permit
  answering Core's own request is kept for its move (`NotItsAnswer` otherwise). `Stuck::carry_out` moves only on a
  decision Core holds, of that order, by a mover of neither the decider's party nor its login, whatever policy
  answers, only while no other move of the part is in progress, and only for what the part still holds then; then
  `move_stuck`, and only its permit answering Core's request backs the `resolve_stuck` effect, built once, acted on
  once for its own receipt in the one register the segments keep too (`Segments::act_on`), carried out by
  `Resolutions`. A move that stopped after its effect was built goes on from there without asking policy again,
  only for its own mover and order. Before it acts the part is read again: one that left `stuck` while the move
  stalled (resolved, or completed by a result recorded since), or holds less than the move names now, is moved no
  more, the move dropped and its decision spent. Each move spends its decision, whatever came of it; an effect the log refuses drops its move and frees the part, its decision spent too. `done` records its result and counts what moved; the part is resolved, its holds released and every
  other decision for it spent, only once all of it moved. `failed` records its result and leaves the part held for a
  new decision. `unknown` records no result yet, and holds the part: nothing more is decided or moved for it until
  its outcome is known, on two actors' word: its own mover, for its own order, reports done or failed naming its
  evidence by an opaque reference (`Stuck::known_outcome`), which settles nothing, a later report replacing it and
  counting one more version; an actor of another party and login confirms it naming the report it confirms (its
  outcome, version and evidence, all the current report's, or nothing is recorded) and its own evidence
  (`Stuck::confirm_outcome`; the decider may, under ADR-0011), and only then is it recorded as the effect's result, once, with both actors and their evidence kept
  (`Stuck::outcome_of`). Done counts what moved and the part resolves once all of it moved; failed frees the part.
  The real answer is evidence Core checks, the provider's stated result for the effect's transaction, which the
  production `Resolutions` mover row (the coordinator's) or `T-020-12` owns. The order's stuck mark goes only once the order holds nothing more, so resolving one part of
  a split order keeps another part's live holds and the mark. Readings, reversible.
- **Hand-off to `T-021-04`: the order's authorisation.** `Stuck::follow` reads the order's permitted `authorise`
  entry from the entries it is handed, and takes them as the log's: that holds only where the assembly passes the
  log's own entries, never a caller's. Once policy registers authorisations, Core reads them from there. The rule
  on the assembly until then: it calls `follow` with the log's entries as the log holds them, when it takes the
  order up.
- **Gap for `T-021-06`: policy's stuck rules are unbuilt.** The engine's `StuckOrders` is only `Unbuilt`'s, which
  answers not known, so nothing moves through the real engine yet (the refusal test runs on it), and no fixed rule
  compares the decider and the mover: Core holds that rule itself, party and login, and the log's
  `stuck-move-carries-out-its-decide-step` holds the party after. The tests' `PermitsEveryStep` (kit) stands where
  `T-021-06`'s rules will, permitting every step, so Core's own rules are seen to hold where policy permits wrongly.
  For `T-021-06` to decide, since Core can't: which roles may decide, move and confirm a move's outcome (a customer
  as decider; the desk of the provider holding the funds as mover; and no customer, nor that desk, as the confirmer
  of an outcome, `THR-70`'s colluding customer and provider), and the evidence digests checked against the store.
- **Hand-off: the contract's half (`T-020-01f`, unmerged).** The contract author is adding a log row that admits a
  resolution only for a declared, unresolved stuck part, resolved once, and a state-machine input that ends a
  resolved part. Until then `stuck-decision-is-of-a-stuck-order` admits a `stuck_decide` after any `no_route` of the
  order, one that left the part waiting included (Core refuses it, `NotStuck`), and a resolved part stays `stuck` in
  the state machine while Core holds it resolved.
- **Hand-off: a late delivery** (`T-020-12`, `T-024-06`: `AC-5`, `THR-76`). A segment's answer is taken once, when
  it is carried out, so a provider's late outcome has no path into Core: that is `T-020-12`'s (reversals and late
  reports) and `T-024-06`'s scenario. What this row does: a stuck part's run records what a segment it carried out
  already gave back, each step once, and carries nothing out (test
  `stuck_part_records_what_its_carried_segment_gave_back`).
- **Gap: carrying a resolution out.** `Resolutions` is a seam; `stuck::Unbuilt` fails every resolution, so nothing
  moves and the value stays held. No row owns the production one: `hand_on` is `T-020-13`'s ("the operator hands
  the value on to a linked order"); `deliver`, to the destination or back to the origin, is in no row, an item for
  the plan (a new `F-020` row).
- **Hand-off to the contract's author: who reported and who confirmed an outcome.** The log records the confirmed
  outcome as the effect's `result`, but not who reported or confirmed it, nor their evidence: that needs a contract
  kind and a row, which the coordinator is queuing with the contract's author. Until then Core keeps them
  (`Stuck::outcome_of`).
- **Item for provider-role: the payer's account input.** provider-role names no input itself (a tuple's terms list
  what its operations require), so `PAYER_ACCOUNT` (`payer.bank_account`) is the name provider-role's fixtures and
  the simulated providers use, not a contract term. A contract-named key for the payer's account is an item for
  provider-role's author.
- **Durability, `F-030`'s.** A fresh `Stuck` holds no declaration, decision or move: after a restart the order Core
  follows is rebuilt from the log's inputs (`Segments::follow_recorded`), so it still reads stuck and `run_part`
  still refuses it, and a fresh `Stuck` decides nothing for it (test `restart_keeps_a_stuck_part_from_moving`); the
  kept decisions, what moved, the resolved mark and the timings come only from the durable copy. The segments' mark
  of a resolved part (`mark_resolved`) isn't rebuilt by `Segments::follow_recorded` either, so after a restart a
  stuck part's late settlement could take its rest hold back: `F-030`'s until `stuck_resolved` lands, when the
  state machine itself ends the part.
- **Seams in `T-020-08`'s module** (`segments/mod.rs`, additive; the coordinator's word not asked, reversible):
  `observe` takes a part's `no_route` and `stuck_declared` as well as `route_recalculated`, never a segment's input;
  `run_part` on a stuck part hands nothing over, carries nothing out and takes no hold, recording only what a
  carried-out segment gave back, a value it settled at an inner node held there at rest while the part is still
  stuck, and refuses it (`SegmentsError::Stuck`) when there is nothing to record; `sent` reads the operation Core
  signed for a segment; `act_on` marks an effect acted
  on in the one register; `follow_recorded` rebuilds an order's state from the log; `stuck`, `carried_since` and
  `settlements` (`Settlement`, with its declared time) read what the stuck clock runs on. In `T-020-05`'s
  `snapshot/exposure.rs`: `Exposures::hold_resting`. `T-020-12` and `T-020-13` share the wave and may touch the same file: each seam is its own
  function or one guard.
- **The kit** (`conformance/scenarios/tests/segments_support/`, additive, for `T-024-02` to fold): `On::Stuck`,
  `On::Raise` and `Fault::Refuse`;
  the register answers `SegmentProvider` (every kit segment is the reserve's) and lists `provider_desk` and
  `other_provider_desk`; `rerouting.rs` runs a recalculation of the kit's order and its recalculated route
  (with the rounds' ports, of which `T-020-10`'s `tests/recalc.rs` still keeps its own copy, left for `T-024-02` so as
  not to touch that file in parallel);
  `resolving.rs` holds `staff`, `destination`, `origin`, `Raises`, `Moves` (with scripted outcomes),
  `PermitsEveryStep` (which can answer another request), `on_parts`, `watch` and `follow`.
- **Tests.** Thirty in `conformance/scenarios/tests/stuck.rs`; all but
  `real_logs_receipt_passes_the_receipt_rule` run on the platform with the simulated providers and replay the log
  through `attribute` and `admit_entry`, the timing ones moving the kit's clock.
  - The row's: the no-route, twice-the-p99, no-history, figures, material-change and move-step tests.
  - This row's besides: `no_route_at_a_failed_final_deposit_waits_for_the_customer`,
    `stuck_state_is_visible_to_the_customer_and_the_provider`, `resolution_is_moved_once_by_another_party_on_its_decision`,
    `resolution_elsewhere_or_beyond_the_stuck_value_is_refused`, `raise_the_log_refuses_holds_up_no_stuck_call`.
  - The first review's: `resumed_move_never_pays_a_part_twice`, `partial_resolution_leaves_the_rest_to_decide`,
    `replayed_receipt_moves_nothing_without_its_record`, `failed_or_unknown_move_spends_its_decision`,
    `resolution_ends_come_from_cores_records`, `stuck_part_records_what_its_carried_segment_gave_back`,
    `no_route_the_log_couldnt_take_is_appended_by_the_next_watch`, `restart_keeps_a_stuck_part_from_moving`.
  - After `d3eabf2`: `resolved_part_takes_no_hold_back_when_run_again`, `ends_followed_at_take_up_survive_a_recalculation`,
    `effect_the_log_refuses_frees_the_part_and_spends_its_decision`, `real_logs_receipt_passes_the_receipt_rule`.
  - The re-review's: `stalled_move_pays_nothing_once_its_part_completed`,
    `settlement_recorded_late_keeps_a_stuck_parts_value_held`, `unknown_move_takes_its_known_outcome_once_from_its_mover`,
    `segment_awaiting_refused_evidence_is_declared_stuck_past_its_limit`; and the ends test, extended.
  - Round 3's: `resolved_part_takes_no_rest_hold_when_its_settlement_is_recorded_late`; the stalled-move test,
    extended.
  - Round 4's: `unknown_move_takes_its_known_outcome_once_on_two_actors_word` (the known-outcome test, renamed) and
    `confirmed_failure_frees_the_part_for_a_new_decision`.
  - Round 5's: `confirmation_names_the_report_it_confirms`.
  - Nine unit tests in the module: `one_large_stuck_order_is_seen_among_many_small`,
    `resolving_one_part_keeps_another_parts_live_hold`, `only_the_payers_account_is_the_origins`, and six over the
    clock's figures and history.
- **Rule-off, each red.** The code came first, so each rule was switched off once by a scratch script, its tests
  run, the file restored and touched: strictly over twice (the boundary unit test and the twice-the-p99 test); the
  p99 once a tuple has history (the twice-the-p99 test); history keyed by tuple (the no-history test); the figures
  before the declaration (the figures test); a material change raised, and recorded as delivered or not (the
  material-change test); the resting value kept at the declaration (the no-route test) and counted in the view (the
  large-among-small test); only a no-route answer recording `no_route` (the no-route test); `observe` taking
  `no_route` and `stuck_declared` (the no-route and visibility tests); a stuck part handing nothing over (the
  no-route and twice-the-p99 tests); the mover of another party, and a move only on a decision Core holds (the
  move-step test); delivering only to the order's ends, no more than the part holds through its provider, and the
  evidence named (the elsewhere test); carried out once with its outcome kept, and resolved once (the moved-once
  test); on the advisor's review, a resumed move only by its own mover and only for its own order (the moved-once
  test), learning never holding up the clock and a refused raise dropped (the refused-raise test). A decision only
  of a stuck part is held twice, by Core's declaration and by the order it follows: each
  switched off alone stays green, both together go red (the waiting test). Reading the part again before a kept
  declaration is appended is defence in depth that no test reaches alone. The review's rules are below.
- **Touches**: `components/core-orch/src/stuck/`, the seams above, `conformance/scenarios/tests/stuck.rs` and the
  kit's additions. No manifest, no dependency, no lock or `licences/RECORD` line, no `DEC-20` path.

### T-020-11, review answers (of `3cff030`)

The fresh reviewer found 1 critical, 2 high, 6 medium and 3 low. Each is answered here; the coordinator's defaults
are taken where it gave them. Each new rule was switched off by a scratch script and its test seen red, then restored
and touched (below).

| # | Finding | Answer | Test |
|---|---|---|---|
| 1 | Critical: a resumed move pays a part twice | One move of a part at a time: `decide` and a fresh `carry_out` are refused while one is in progress (`MoveInProgress`); `advance` reads the part again before acting; resolving a part spends every other decision and drops every move of it that never acted | `resumed_move_never_pays_a_part_twice`, the reviewer's sequence |
| 2 | High: resolving 1 unit closes a 1 000 000 part | Each part counts what its resolutions moved; it resolves only when nothing is left; a decision, and again its move, is held to what is left; the view shows what is left | `partial_resolution_leaves_the_rest_to_decide` |
| 3 | High: a replayed receipt moves value with no record | A receipt is taken only if its digest is the entry's own, otherwise the append is tried again; effects acted on are held in `Segments`' one register (`act_on`) | `replayed_receipt_moves_nothing_without_its_record`, with `Fault::Replay` |
| 4 | Medium: a failed decision stays usable for ever | `Resolved::Unknown` added: no result yet, the part held, nothing more decided or moved for it (`OutcomeUnknown`); every move, failed included, spends its decision | `failed_or_unknown_move_spends_its_decision` (carol can't rerun bob's) |
| 5 | Medium: one login under two stated parties passes | The login is compared too (`SameLogin`); the roles a decider and a mover may hold are named for `T-021-06` above | in `move_step_without_another_actors_decision_is_refused` |
| 6 | Medium: the order's ends come from the caller | `Stuck::follow` reads them from the authorisation, held to the order Core follows, and the order's withdrawal; `Deciding` carries none; the assembly rule and the `T-021-04` hand-off are named above | `resolution_ends_come_from_cores_records` |
| 7 | Medium: timing | `HISTORY` 20, `WINDOW` 200, the first p99 compared with the advertised time; the "either way" claim corrected above | `p99_falls_after_a_long_slow_history_and_the_fall_is_raised`, `slow_first_settlements_against_the_advertised_time_are_raised`, the material-change scenario |
| 8 | Medium: a late delivery on a stuck part can't be recorded | A stuck part's run records what a carried-out segment gave back, carrying nothing out; a provider's late outcome has no path into Core, handed to `T-020-12` and `T-024-06` above | `stuck_part_records_what_its_carried_segment_gave_back` |
| 9 | Medium: the split-order release has no test | Unit test over `Declared` and `Exposures` built directly | `resolving_one_part_keeps_another_parts_live_hold` |
| 10 | Low: `watch` overwrites a kept `no_route` | It appends it as built | `no_route_the_log_couldnt_take_is_appended_by_the_next_watch` |
| 11 | Low: kept decisions keyed by policy's echoed id | Keyed by Core's own request, and an answer to another request is refused (`NotItsAnswer`), at decide and at move | in the move-step test |
| 12 | Low: no restart test of the two-place check | `Segments::follow_recorded` rebuilds the order from the log | `restart_keeps_a_stuck_part_from_moving` |
| gap | Resting value not in the ceilings | Contained, so done: `Exposures::hold_resting` | in the no-route test (`in_flight` counts it) |
| gap | The warning, the contract's half, `Resolutions` | Hand-offs to `T-023-01`, `T-020-01f`, `T-020-13` and a new row, above | — |
| 13 | Found after `d3eabf2`: a resolved part took its hold back when run again, since the stuck path went through the segments' hold | A stuck part's segment takes no hold (`advance`'s `hold`) | `resolved_part_takes_no_hold_back_when_run_again` |
| 14 | Found after `d3eabf2`: the receipt rule was tested only against the kit's log | Pinned against `CMP-CORE-AUDIT`'s `AuditLog`: the same canonical JSON, SHA-256, lower-case hex (`stuck::is_receipt_of`, now public) | `real_logs_receipt_passes_the_receipt_rule` |
| 15 | Found after `d3eabf2`: a later `follow` replaced the order's ends | The ends first followed stand; the same again is nothing, others are refused. The assembly follows the order at take-up. A recalculation keeps the settled withdrawal, so the origin is still read after one | `ends_followed_at_take_up_survive_a_recalculation` |
| 16 | Found after `d3eabf2`: an effect the log refuses held the part for ever | A refused effect drops its move, frees the part and spends its decision; nothing moved and its permit backs nothing | `effect_the_log_refuses_frees_the_part_and_spends_its_decision` |

- **Red first.** The review's tests (1 to 12) use the reworked API (`follow`, scripted `Moves`), so they don't compile
  against `3cff030`: each rule they hold was shown red by switching it off alone, not by writing the test first
  (below). Rows 13 to 16 were written test-first against `d3eabf2` and seen red for the right reason before each
  fix (one hold taken back; a refused effect's move taken on again; another authorisation's ends accepted), then
  switched off once more after it.
- **Rule-off, the review's rules, each red:** decide waiting while a move is in progress, and no other move then (the
  pays-twice test); a part resolving only once all of it moved, and the move held to what is left (the partial
  test); a receipt only of its own entry (the replay test); a carried-out move spending its decision, and an unknown
  outcome holding the part (the failed-or-unknown test); the decider's login (the move-step test); a decision
  answering Core's own request (the move-step test); the ends from Core's records, and an authorisation that is the
  order's (the ends test); the value at rest held (the no-route test); a stuck part recording what it carried out
  (the carried-segment test); a kept `no_route` never overwritten (its test); another part's live hold kept (its unit
  test); the order rebuilt from the log (the restart test); twenty settlements before a p99, the window, and the
  first p99 against the advertised time (the timing unit tests and the material-change test). The one register of
  effects acted on is defence in depth behind the receipt rule, which the replay test holds, and no test reaches it
  alone. The part read again before a move acts is no longer defence in depth: the re-review's stalled-move test
  reaches it alone (below). After `d3eabf2`, each
  red too: a stuck part taking no hold (the resolved-part test), the ends first followed standing (the take-up
  test), a refused effect freeing the part (the refused-effect test).

### T-020-11, re-review answers (of `4d35507`)

The re-review held fixes 1 to 5, 7 and 9 to 16, and found four more, two of them high, all in the path where a stuck
part records what a segment gave back. Each test was written first and seen red for the right reason, then each rule
switched off once more after its fix.

| # | Finding | Answer | Test |
|---|---|---|---|
| N1 | High: a stalled move paid out after its part completed | Before a move acts, the part is read again, stuck and unresolved (`unresolved`) and holding what the move names (`within`); otherwise the move is dropped and its decision spent. Red first: the move answered `Done`, paying twice | `stalled_move_pays_nothing_once_its_part_completed` |
| N2 | High: a late settlement left a stuck part's value outside every hold | A segment settling on the stuck path, while the part is still stuck, rests its value held at the inner node it reached. Red first: nothing held | `settlement_recorded_late_keeps_a_stuck_parts_value_held` |
| N3 | Medium: the accounts in the ends could be steered | The destination, account included, from the log's permitted `authorise` entry, never one refused; the origin's account checked against the withdrawal Core signed, since the log holds it only by commitment (stated above). Red first: it didn't compile against the old `follow` | `resolution_ends_come_from_cores_records`, a friend's account at each end |
| N4 | Low: `Unknown` had no way out | `Stuck::known_outcome`: its own mover, its own order, once, done resolving and failed freeing. Learning the outcome is the production `Resolutions`'s (the coordinator's plan row). Red first: it didn't compile | `unknown_move_takes_its_known_outcome_once_from_its_mover` |
| nit | The repeated tests bullet | Rewritten above | — |
| E | From `T-020-09`'s review: evidence `CMP-EVIDENCE` refuses should be a stuck trigger | It is one already: a refusal from the grading seam leaves the segment carried out with no final outcome and its value held, so the stuck clock runs on it, and past its limit the part is declared stuck. Since `T-020-09` merged (`m3` at `31c3772`), the refusal is its `SegmentOutcome::AwaitsEvidence(Short::Refused)`: it stays out of the segments' `is_final`, so `carried_since` keeps timing it; adding it there turns the test red. The test passed on its first run, being of behaviour already there; switching off the timing of a segment that awaits evidence turned it red | `segment_awaiting_refused_evidence_is_declared_stuck_past_its_limit` |

- **Rule-off, each red:** the part re-read before a move acts, alone (the stalled-move test); a late settlement on
  a stuck part held at rest (its test); the destination only from a permitted authorisation, exactly, and the
  origin's account one the withdrawal named (the ends test); a known outcome only from its own mover, only of an
  unknown move of its order, never itself unknown, and a known done counting what moved (the known-outcome test); a
  segment awaiting evidence timed (the refused-evidence test).
- **The entry-point scan** (`T-021-03`'s) caught `follow` comparing an outcome by writing `Outcome::Permitted`; it
  reads it through a `matches!` pattern now, which the scan takes as read, not written.
- **Held for the contract branch** (`f-020/t-020-01f-erasure-decided`, not in `m3`): `StuckDecide.part`, and the
  `stuck_resolved { to, evidence }` input once a resolution's move is `done` and the part has nothing left. Core
  already doesn't produce the histories the new rows refuse (a resolution of a part not declared stuck or waiting,
  one resolved twice, another move on a held part, a second move on one segment); each is to be checked against
  the rows once they land.

### T-020-11, round 3 answers (of `f073a51`)

N1 to N3 held; two mediums and two lows remained. Each test was written first and seen red for the right reason
(R2 and R4b by not compiling against the old API), then each rule switched off once more after its fix.

| # | Finding | Answer | Test |
|---|---|---|---|
| R1 | Medium: a resolved part took a rest hold back, never released | Resolving a part marks it resolved in `Segments` (`mark_resolved`), and a stuck part's late settlement takes a rest hold only for a part not so marked: the state machine still reads it stuck until `stuck_resolved` lands. Red first: 1 000 000 held after resolution | `resolved_part_takes_no_rest_hold_when_its_settlement_is_recorded_late` (a settled withdrawal, not `Act::Unknown`) |
| R2 | Medium: the mover alone could make a part pay twice | The coordinator's default, reversible: a known `failed` is only reported (`Moved::AwaitsConfirmation`), and recorded, the part freed, once an actor of another party and login confirms it (then `Stuck::confirm_failed`, since round 4 `Stuck::confirm_outcome`; `SameParty`, `SameLogin`); a known `done` from the mover alone stays, since it frees nothing. The real fix is evidence, above | in `unknown_move_takes_its_known_outcome_once_from_its_mover`: the mover alone can't free the part, a same-party or same-login confirmer is refused |
| R3 | Low: a dropped move left an effect with no result | A move dropped before it acts, its effect recorded, gets its `failed` result: it never took effect. Red first: one effect left open | in `stalled_move_pays_nothing_once_its_part_completed` (no `to_take_effect` left open) |
| R4a | Low: `follow` trusts the slice it is handed | Stated in its docs and in the `T-021-04` hand-off above: it holds only where the assembly passes the log's own entries | — |
| R4b | Low: any account the withdrawal named passed as the origin's | Only the payer's input (`PAYER_ACCOUNT`, `payer.bank_account`, the name provider-role's fixtures and the simulated providers use; provider-role names no input itself, so a contract-named key is a provider-role item) | `only_the_payers_account_is_the_origins` (a withdrawal carrying the beneficiary's account too) |
| R4c | Low: per-part authorisation is coming (`T-020-01f`, `parts: Option<Vec<PartId>>`) | Work for its merge, each rule named below; it fails closed today | — |

- **Work for the per-part authorisation's merge**, each a rule of `Stuck::follow` that a per-part authorisation
  breaks: (1) the latest permitted authorisation is chosen per order, where it must be per part; (2) one destination
  is held to every part, where each part has its own; (3) the ends first followed stand per order, where they stand
  per part, and a new authorisation for one part (`REQ-ORD-43`'s new destination) must not be refused for the
  others; (4) the origin is read from the first part's first segment only, where each part has its own withdrawal
  and payer. Until then a split order whose parts are authorised apart fails closed: `follow` refuses it
  (`NotItsAuthorisation`), and nothing is decided for it.
- **Rule-off, each red:** a resolved part taking no rest hold (the late-settlement test; the first rule-off kept the
  check by mistake and stayed green, and the corrected one, inverting the guard, went red); a failure only reported
  until confirmed, a confirmer of another party, and of another login (the known-outcome test); a dropped move's
  effect getting its result (the stalled-move test); only the payer's account being the origin's (its unit test).

### T-020-11, round 4 answers (of `e0ca48a`)

R1 to R3 held; one medium and two lows remained. The tests were written first and seen red by not compiling
against the old API, then each rule switched off once more after its fix.

| # | Finding | Answer | Test |
|---|---|---|---|
| S1 | Medium: one actor's word settled a move's outcome, and the log doesn't record whose | Done is held to the rule failed was: the mover only reports, naming its evidence (`known_outcome`, `Moved::AwaitsConfirmation`), and a later report replaces it; an actor of another party and another login confirms, naming its own (`confirm_outcome`), and only then is the outcome recorded, the part resolved or freed, with both actors and their evidence kept (`outcome_of`). The confirmer's roles for `T-021-06`, and the log's record of who reported and confirmed for the contract's author, are named above | `unknown_move_takes_its_known_outcome_once_on_two_actors_word` (the mover's done alone resolves nothing; a confirmed done resolves; same-party and same-login confirmers refused; a switch from failed to done leaves only the last pending), `confirmed_failure_frees_the_part_for_a_new_decision` |
| S2 | Low: `mark_resolved` and `act_on` were public | `pub(crate)` | — |
| S3 | Low: the `PAYER_ACCOUNT` name | An item for provider-role, above | — |
| gap | `resolved` isn't rebuilt after a restart | `F-030`'s until `stuck_resolved` lands, above | — |

- **Rule-off, each red:** a confirmer of another party, and of another login; only the last report pending; nothing
  confirmed without a report (the two-actors test). That the mover's done alone resolves nothing is held by the
  report step and those checks together: `known_outcome` records nothing and takes no `Parts`.

### T-020-11, round 5 answer (of `66b5881`)

| # | Finding | Answer | Test |
|---|---|---|---|
| T1 | Medium: the confirmer didn't say which outcome it confirmed, so the mover could re-report after the confirmer committed | Each report of a move counts a version (`Moved::AwaitsConfirmation { report }`). `Confirming` names the outcome, the report's version and the report's evidence; a confirmation is refused (`NotTheReport`) and records nothing unless all three are the current report's | `confirmation_names_the_report_it_confirms`: failed then done, a confirmation of the first refused and nothing recorded; mixed ones refused; the same in reverse; the current report's confirmed |

- **Red first:** the test was written first and seen red by not compiling against the old `Confirming`.
- **Rule-off, red:** the report's outcome, version and evidence matched (the round 5 test).

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

### T-020-09, the assembly graded through `CMP-EVIDENCE` (after `m3` at `970828a`)

- **Merged `m3` at `970828a`** (`T-024-02`, the assembly and the scenario harness), a merge commit; it merged
  cleanly, the segments kit's additions on both sides kept.
- **The assembly's grading** (`conformance/scenarios/src/assembly/`, the grading wiring only, so `T-021-04`'s change to
  the authorise stand-in merges beside it): each part runs under `closing::Evidenced`. `grading.rs` holds the
  readings, a copy of the simulated networks (each reserve's bank, and `polygon` and `bsc` where an exchange or a
  ramp is installed) and the reserves' books, refreshed at the start of each run and after each `carry_out` (the
  operation delivered twice too), since the world lives inside the platform the segments hold as their modules. One
  `Closing` is kept on `Core` beside `Segments`, built by `Closing::recorded` from the log whenever Core is put
  together on a disk, so a Core opened again takes the movements its log names.
- **Every scenario still completes its orders**: the simulated reserve records each deposit's movement on its bank
  network (instant finality) to the destination account, so each order closes on that public record and the
  reserve's statement. Nothing in the closing rules was loosened. The assembly's four checks pass (the swap check
  asserts each order complete; the crash check stops Core, and nothing in the harness runs a Core on after it, so the
  rebuild runs at every assembly and is held by `one_payment_closes_one_order_across_a_restart` and
  `movements_core_checked_are_taken_live_and_after_a_restart` in core-orch).
- **Test** `assembled_order_closes_on_the_public_record_and_the_reserves_statement` (`assembly_wiring.rs`): the
  assembled order's deposit closes on grade-1 evidence of its movement on the bank network, naming it, and the
  reserve's statement.
- **Rule-off, each red:** `ProvidersWord` wired back (the wiring test, and the swap check's "each order completed");
  the readings not refreshed after an operation (the wiring test: the deposit's movement isn't seen and it waits).
  `Closing::recorded` at assembly, switched off, stays green at this level: no assembly scenario runs a reopened Core
  on.
- **The restart scenario (closing the gap above).** `checks::restart_keeps_what_closing_took`, run by
  `reopened_core_keeps_what_its_grading_took` (`tests/assembly.rs`): the first order completes; Core crashes before
  the second order's withdrawal (the crash check's fault); it is opened again on the same disk (`Step::Restart`,
  `Core::reopened`); with the reserve now paying two orders with one payment (`Fault::OnePaymentForTwo`: the network
  ties a movement by its destination and amount), a third order to the same account for the same amount is tied to
  the first order's movements. Checked: Core stopped mid-order, the first order completed, nothing stopped on a
  refusal of the log, the third order's withdrawal settles off the movement and its deposit waits, and no movement
  evidences two orders. Switched off, `Closing::recorded` at assembly turns it red: the reopened Core takes the first
  order's movements afresh and the log refuses it (`movement-evidences-one-segment`), so the third order stops; and
  without the planted tie the third order closes on its own payment.
- **What `Core::reopened` keeps and rebuilds.** What is outside Core goes on as it stands: the platform with the
  providers and their networks, the state machine's host, the clock, and the source of fresh identifiers (so no id
  repeats). Core's own parts are put together again over the disk: the log and the evidence store read back, the
  grading's `Closing` rebuilt by `Closing::recorded`, and the catalogue rebuilt from the log (each `done` grant at its
  effect's time and each advertisement at the time the log received it, `Core::relist`), since granting again on the
  reopened log would put each grant after the advertisement it trusts. Intake's, the segments' and `Issued` start
  empty (`F-030`), so an order Core was running when it stopped stays where it stopped. The harness's
  `Step::Restart` reopens Core after a crash or without one; after a crash with no restart, the steps after it are
  skipped, as before.

### T-020-09, the approval's low items (after `m3` at `85386b8`)

- **Merged `m3` at `85386b8`** (`T-021-04`: the assembly's authorise through the engine, the policy handle reading
  `CoreLog` through `Entries`): no conflict beside this row's grading wiring; both rows' tests pass on the merged tree.
- **`Core::relist` fails loudly** on a `done` catalogue change it doesn't replay (a revoke, a release accepted,
  revoked or promoted): the reopening is refused (`AssemblyError::Listing`) rather than rebuild a catalogue that grants
  what the log revoked. Test `reopened_core_refuses_a_catalogue_change_it_does_not_replay` (a promotion in the log).
- **`Core::relist` checks the log's commitment**: an advertisement is joined only where the bytes the evidence store
  kept under the stated entry's commitment (`bearing_out`) are the installed provider's. Test
  `reopened_core_joins_only_the_advertisement_its_log_committed_to` (the same statement in other bytes).
- **A restart between a grade-3 withdrawal and its deposit**: `restart_between_a_withdrawal_and_its_deposit_keeps_the_movement_into_the_node`
  (core-orch): rebuilt from the log, Core knows the withdrawal's movement into the node from the evidence naming it,
  and the exchange inside the node still gets its in and out records. It passed when written: the rebuild was there
  from the re-review round; switched off below.
- **`Readings::of`** takes the networks the fleet serves through `fleet::networks_of`; the polygon and bsc block gap
  is in `F-024`'s notes.
- **Rule-off, each red:** relist's refusal of a change it doesn't replay (its test); the commitment check (its test);
  `Closing::recorded` rebuilding a settled segment's own movement (the restart-between test).

### T-020-11, merged with `m3`

- **At `970828a`** (`T-024-02`'s assembly and shared kit, `T-020-10`, `T-022-03`): the segments kit's actors, keys,
  identifiers, route and bounds come from the shared kit (`rutent_scenarios::kit`); only `provider_desk` and
  `other_provider_desk` stay in the segments kit. The rounds ports in `rerouting.rs` stay: the assembly's are private.
- **At `31c3772`** (`T-021-04`, `T-020-09`): the grading seam answers `Graded::Settles` or `Waits`, and the kit's
  `ProvidersWord` no longer settles a part's final deposit. This row's helpers grade through the kit's `Recorded`,
  as its other platform tests do; the refused-evidence test's fake answers `Waits(Short::Refused(_))`, and its
  segment awaits evidence (`AwaitsEvidence`), timed by the stuck clock. Conflicts in `segments/mod.rs` (`Segments`'
  fields), the kit (`On`, the register) and these notes kept both sides.
- **Still for the contract branch's merge:** `StuckDecide.part`; `stuck_resolved { to, evidence }`; an unknown
  outcome's `unanswered` result, then on confirmation the `resolution_outcome` entry with both actors and both
  pieces of evidence (`Stuck::outcome_of`'s, into the log), then the final `done` or `failed` result.


## T-020-13

Branch `f-020/t-020-13-hand-on`, from `m3` at `066b2a7`, with `m3` at `5d9dcca` (`T-020-10b`) and at `d4da424`
(`T-020-11b`) merged in.

- **One record, `hand_on::HandOn`** (`components/core-orch/src/hand_on/`). Core's record of the parts it resolves
  on the customer's choice and the orders it hands value on to, which the production assembly keeps durably
  (`F-030`), as `Segments`. What it reaches comes in `hand_on::Parts` (Core's clock, the log, fresh ids, `Segments`,
  `Exposures`). Every input is built once and appended until the log takes it; one the log refuses is dropped. The
  rules are pure functions in `hand_on/rule.rs` (`new_terms`, `destination_change`, `proves_start`,
  `requester_of`, `acceptance_of`, `in_force`, `fresh_terms`), each held by a test of its own. Every pending
  input is appended as built while the log can't take it now, and dropped where the log refuses it; the
  exception is a linked order's `route_chosen` refused after its `handed_on` went in, which is recorded as refused
  (below). Pending inputs live in memory only (the durability gap below).
- **A failed final deposit never rolls the order back** (`REQ-ORD-43`). Nothing here asks for a reversal or
  releases the value: the deposit leaves it at its node, and the first answer is `T-020-10`'s recalculation to the
  same destination. Only a part the state machine has waiting for the customer (`T-020-11`'s `no_route` at a failed
  deposit's node, or `T-020-10b`'s `waiting_at_floor`) is resolved here; a part whose deposit failed but that still
  has a route is refused `NotWaiting`, nothing asked. Test `failed final deposit is recalculated to the same
  destination first`.
- **New terms only from a waiting part** (`hand_on::authorise`, `T-021-04`'s hand-off: policy holds who gives new
  terms, not when). Core asks policy's `authorise` for new terms only where every part they speak for is in the
  order and waits for the customer, the giver is the order's customer acting as such (role `customer`, the party
  the log holds as requester), the id is new, something is new (destination, to asset or floor), and the origin
  reserve is kept. A new destination or to asset needs each of those parts waiting at a failed final deposit
  (`INV-10`); a new floor alone resolves any wait (`T-021-04`'s default, `REQ-ORD-27`). An authorisation naming no
  part speaks for every part, so every part must wait: a split order's other part goes on under its own terms.
  Tests `with no route to it the order waits for the customer` and
  `new_terms_are_asked_only_for_a_part_waiting_for_the_customer` (unit).
- **Core's stuck ends followed again** (`T-020-11b`, the coordinator's word). `Stuck::follow` holds each part's
  authorisation to its destination as the order Core follows has it, so a new destination can't be followed before
  the part takes it. Core follows again at both points that change a part's terms: `hand_on::authorise`, once
  policy permits terms that keep the destination (a new floor), over the log's entries read then; and
  `HandOn::change_destination`, once the order Core follows takes the `destination_changed`. `hand_on::Parts` carries
  the `Stuck`, so neither can skip it; a follow that fails is `HandOnError::Follow` (after a change, its input stays
  recorded). In `with no route ...`, the ends Core holds stay the old ones after the customer's new destination is
  permitted and before it is taken; in the floor test, the ends are followed again on the new floor.
- **The customer's new destination** (`HandOn::change_destination`, `REQ-ORD-43`, `INV-4`, `INV-10`). The router's
  choice is taken from the assembly as its choice round's `DecisionRecord`; Core holds it to the customer's new
  authorisation, never the record (`recalc::check_route` with that authorisation: from where the value sits, all of
  it, ending at its destination reserve and node in its to asset, at least its floor after the operator's fee), then
  to the state machine's rules (`destination_change`: the part waits at a failed final deposit, the route changes the
  destination, it starts where the value sits), then the transition itself. Last, the customer's acceptance: of this
  order, accepting the new authorisation (`changed-destination-rests-on-its-acceptance`). Recorded as
  `destination_changed` (source `customer`, standing `stated`, Core's verified entry), citing the decision record and
  the acceptance; once taken, the order Core follows takes it and the value's holds go (the settled segment's rest
  hold and the retired ones); the new route's segments hold it as they run. Each move on the new route carries
  `Changed::route` (`recalc::route_end` read from the route) to policy and the log. Tests
  `customer's new destination starts from where the value sits` (unit, over the state machine, which also shows no
  recalculation reaches the new destination) and `customers_new_route_is_held_to_its_new_authorisation` (the kit:
  a route back to the kit's own destination refused as `ChangesDestination` against the new authorisation and as
  `KeepsDestination` under the old, nothing recorded).
- **A wait at the floor, resumed** (`HandOn::resume`, `T-020-10b`'s hand-off). After `5d9dcca` a part waiting at its
  floor reads `Held` in `recalc::evidenced`, so `Recalculations` never routes it again. The customer's new terms (a
  lower floor) resume it: the router's choice to the same destination is held to the new authorisation as above,
  to `recalculation_keeps_destination` and `recalculation_starts_where_value_sits`, and recorded as
  `route_recalculated` (source `orchestrator`, verified), citing its choice round's decision record, once the log
  holds the customer's acceptance of this order under the new authorisation. The order Core
  follows takes it, the part waits no more, and its route runs under the new authorisation. The customer's choice
  isn't a recalculation and counts against no bound (a reading, reversible); `Recalculations::followed_by` (a
  seam in `recalc/mod.rs`) lets the assembly have the next recalculation follow that round (`REQ-AUD-4`). Test
  `customers_new_floor_resumes_a_part_waiting_at_it`: the withdrawal delivers 1 % short, below the floor of
  995 000; the customer's floor of 900 000 resumes it, the deposit settles, the log admits every entry.
- **Handed on to a linked order** (`HandOn::hand_on`, `REQ-ORD-43`, `REQ-ORD-13`, `INV-4`). The linked order's
  route comes as its decision record's id and its parts' plans. Core builds its `route_chosen` naming
  `handed_on_from`, opens it with the state machine (`Stm::open`: a route handed on starts at an inner node, with no
  withdrawal; otherwise `NotAHandedOnRoute`), and calls the start rule across the two orders,
  `rutent_stm::handed_on_route_starts_where_value_sits(first, second)`, then holds every part of the linked order to
  this part's own value (`at_rest`), before anything is recorded. The linked order's acceptance must be of the
  linked order and name the first order's requester as its own (`REQ-ORD-13`; the log's
  `hand-on-input-rests-on-the-linked-orders-acceptance` holds the same). Then the first order's `handed_on`
  (`linked_order`, `acceptance`) is appended, then the linked order's `route_chosen`: `handed-on-route-points-back`
  cites the earlier one, so the two records point to each other. Once `handed_on` is taken the `route_chosen` is
  kept until the log takes it, so a part is never left handed on to an order not opened. The order Core follows
  takes `handed_on` (the part is `handed_on` and takes no more), the first order's holds for the part go, and Core
  follows the linked order. Tests `handed on route starting away from the value is refused` (from the other
  reserve's ledger: `StartsAwayFromTheValue`; a route starting with a withdrawal: `NotAHandedOnRoute`; nothing
  recorded, the linked order not opened) and `handed on route keeps the original customer as requester` (another
  requester: `NotTheRequester`; another order's acceptance: `NotItsAcceptance`; then both records, in order, the
  log admitting them, and the first order's part handed on, taking no more).
- **The holding account's record proves the start** (`HandOn::prove_start`, `REQ-ORD-40`, `INV-5`). No segment of a
  route handed on is handed over until the holding account's own record of its withdrawal is held for it:
  `proves_start` holds `prov`'s `WithdrawalRecord` (`T-016-03`) to the linked order's starting reserve (the
  holding account's provider, which counts as its origin reserve), the node and asset of its first segment, the
  first order's value (`handed_on_from`), all of what the part held (what its settled segment handed on), and a
  reference of its own. `Segments::run_part` refuses a route handed on until then
  (`SegmentsError::StartUnproven`), and runs it under the recalculated part's guard (from an inner node, by no
  withdrawal). Test `handed on route's withdrawal is proven by the holding account's record`: each wrong field
  refused and nothing handed over; the reserve's own record of the whole value accepted, and the linked route's
  deposit then settles from the ledger under the linked order's own authorisation, the log admitting it. The kit's
  ledger is a private node, so the record there is its publisher's word (`REQ-ORD-40`); the test builds it.
- **The operator's hand-on** (`Basis::Resolution`, `REQ-AUZ-4`). Only for a stuck part, and only once the log holds
  the `resolve_stuck` effect at the receipt named, of this order, handing the value on to this linked order, its
  decide step naming this part, and its result `done` (`stuck-resolved-follows-a-done-resolution`). The customer's
  path doesn't reach a stuck part. Test `operators_hand_on_follows_its_done_resolution`: refused with no done
  resolution, nothing recorded; decided by one desk, moved by another (the kit's `PermitsEveryStep` and `Moves`), then
  handed on, source `operator`, the log admitting every entry.
- **Which destinations the kit reaches.** Each simulated reserve routes only between its own bank network and its own
  ledger; no provider links two reserves, so the router can reach no other destination. The destination change's
  success path (below) runs on a choice record the kit writes for the other reserve's deposit.
- **The kit** (additive, for `T-024-02` to fold): `seed_linked` in `segments_support/mod.rs` (a linked order's
  authorisation through policy, its arrival, its acceptance by a named requester, its decision record, and no
  route), and `segments_support/handing.rs` (`hand_parts`, `recorded_with_receipts`, `linked_plan`,
  `from_the_ledger`, `linked_record`, `linked_route`, `route_from_the_ledger`, `seed_record`).
- **One history the log refused, classified a real Core bug.** The first version of `resume` took no acceptance.
  Its route's deposit, moving under the new authorisation, was refused by the log on
  `move-rests-on-its-orders-acceptance` ("a value-moving effect's acceptance names its authorisation"): the order's
  only acceptance named the authorisation the part waited under. New terms need the customer's acceptance
  (`REQ-ORD-34`, `REQ-AUZ-3`), so the row was right and Core wrong: `Resume` now names the acceptance, held to this
  order and the new authorisation before anything is recorded (`NotItsAcceptance` otherwise, the first assertion of
  the floor test). The kit's new acceptance is an automatic deposit's, as the order's first one is
  (`order-holds-one-acceptance-basis`).
- **A test artefact, named:** the kit's linked order's acceptance is an automatic deposit's, resting on a seeded
  arrival, since a customer's given acceptance names the origin reserve's signed yes, which the kit doesn't make.
  The log admits it; the row's `REQ-ORD-34` acceptance by the customer is intake's (`T-020-03`) to make for a
  linked order.
- **Red first, honestly.** The code came first, with each test written beside it, so for each rule the rule-off
  below stands in for seeing its test fail first, as `T-020-10`'s and `T-020-11`'s did. One test did see Core red on
  its own: the floor test's first run, the log refusing the resumed move (above), fixed in Core.
- **Rule-off, each switched off once by a scratch script, its tests run, the file restored and touched:**

  | Rule switched off | Tests that went red |
  |---|---|
  | new terms only for waiting parts | `new_terms_are_asked_only_for_a_part_waiting_for_the_customer` |
  | new terms only from the customer | the same |
  | a new destination only after a failed final deposit (new terms) | the same |
  | the new route starts where the value sits | `customers_new_destination_starts_from_where_the_value_sits` |
  | the new route changes the destination | the same |
  | a destination change only after a failed final deposit | the same |
  | the customer's hand-on only from a waiting part | `failed final deposit is recalculated ...`, `... keeps the original customer as requester`, `operators_hand_on_follows_its_done_resolution` |
  | the start rule across two orders (the stm call and the part's own value, together) | `handed on route starting away from the value is refused` |
  | the original customer as requester | `handed on route keeps the original customer as requester` |
  | the acceptance is the linked order's | the same |
  | `handed_on` appended before `route_chosen` | the four hand-on tests (the log refuses `handed-on-route-points-back`) |
  | `observe` taking `handed_on` | the four hand-on tests |
  | the run refused until the start is proven | `handed on route's withdrawal is proven ...` |
  | a route handed on runs under the inner-start guard | the same (refused `NoWithdrawalFirst`) |
  | the holding account's record held to the start | the same |
  | the operator's hand-on only after its done resolution | `operators_hand_on_follows_its_done_resolution` |
  | the new route held to the new authorisation (`check_route`) | `customers_new_route_is_held_to_its_new_authorisation` |
  | a resume rests on the customer's acceptance of the new terms | `customers_new_floor_resumes_a_part_waiting_at_it` |
  | `Stuck` followed again after new terms that keep the destination | `customers_new_floor_resumes_a_part_waiting_at_it` |

  Two stayed green, each defence in depth: a part handed on read as ended by `run_part` (its failed deposit's
  outcome is final, so nothing is handed over again, and the log's `ended-part-takes-no-more` would refuse a
  move), and a resume's own keeps-destination check (`check_route` against an authorisation of the same
  destination refuses first). `Stm::open` refusing a route that starts with a withdrawal can't be switched off
  alone: it is what builds the linked order. The follow after a destination change taken has no test of its own:
  no scenario takes one (the gap below).
- **Defaults taken** (reversible, none asked):
  - a new destination or to asset only for a part waiting at a failed final deposit; a floor wait takes a new floor
    alone. The contract's world fixture `world--in_dest` has a destination change after a floor wait, which the log
    admits; Core is stricter on `INV-10`'s "after a failed final deposit". For the owner's form.
  - the customer's path resolves only a part waiting for the customer; a stuck part is the operator's, as a stuck
    resolution (`Basis::Resolution`), though the state machine takes `destination_changed` and `handed_on` from a
    stuck part too.
  - new terms keep the origin reserve (`ChangesOrigin`): the value was withdrawn already.
  - the customer's own resumed route counts against no recalculation bound.
  - the linked order's route is handed in as plans and a decision record id, not routed by Core: its first round is
    the assembly's (`T-024-02`), which routes it from the node where the value sits.
  - a customer's destination change is recorded with source `customer`, standing `stated`; the linked order's route
    with the source of whoever stated it (customer or operator).
- **Gaps, each with its owner:**
  - **The customer's new destination run on the platform** (`T-024-06`, `AC-40`; the kit, `F-024`). The change is
    taken and admitted on the kit (below), on a record the kit writes; its deposit isn't run, since no simulated
    reserve deposits from another's ledger.
  - **The holding account's record isn't in the log** (the contract's author). `REQ-ORD-40` makes it the proof of a
    handed-on route's start, but core-internal has no kind for it, so Core holds it (`Segments`' proven starts) and
    the log admits the linked order's first move without it. A kind for the holding account's withdrawal record,
    and a row holding a route handed on's first move to it, would let the log hold `REQ-ORD-40` too. Until then a
    fresh `Segments` after a restart refuses the linked order's segments until the record is held again (`F-030`).
  - **The production `Resolutions` for a hand-on** (`T-020-11`'s gap, a new row). A `resolve_stuck` hand-on's move is
    the holding account's withdrawal for the linked order (`Holdings::start_handed_on`, `T-016-03`), but the effect
    names no route, which `start_handed_on` takes. Until a row joins them, the operator's hand-on is shown with the
    kit's `Moves`.
  - **The record at a private node** (`F-016`). The kit's ledger is private and has no holding module, so its record
    is built by the test as its publisher's word; `REQ-ORD-40`'s label for it isn't carried anywhere yet.
  - **A linked order's given acceptance** (`T-020-03`). Intake opens no order whose route starts at an inner node
    (`snapshot/route.rs`'s guard refuses it, `T-020-05`), so a customer's acceptance of a linked order, with its
    origin reserve's yes, has no path; the kit seeds an automatic deposit's.
  - **`T-020-08`'s open item, `judge` and a module the operator publishes**: not reached. The linked route's first
    segment runs at the reserve, not the holding module; it blocks only once a route handed on runs at the wallet.
  - **Policy's own hold on when** (`T-021-04`'s gap, the owner's item, default no): unchanged; Core's gate holds it.
- **Seams in other rows' modules** (named, each its own arm or guard):
  - `segments/mod.rs` (`T-020-08`): `observe` takes `destination_changed` and `handed_on`; a part handed on is
    ended for `run_part` (`ended_by_resolution` reads `handed_on` too); `run_part` refuses a route handed on until its
    start is proven (`SegmentsError::StartUnproven`, one variant) and runs it under the recalculated part's guard;
    `proven` and `prove_start`, `pub(crate)`.
  - `recalc/mod.rs` (`T-020-10`): `Recalculations::followed_by`.
- **Touches**: `components/core-orch/src/hand_on/`, the seams above, `components/core-orch/tests/hand_on.rs`,
  `conformance/scenarios/tests/hand_on.rs` and the kit's additions. No manifest, no dependency, no lock or
  `licences/RECORD` line, no contract, no `DEC-20` path.

### T-020-13, review answers (of `55bd1bc`)

The reviewer found 3 high, 5 medium and 2 low; the coordinator ruled on each, reversibly, on the owner's form. Each
test below was written first against `55bd1bc`'s API, compiled with only inert error variants added, and seen red
for its own reason before any fix (the run is in the branch's history: nine tests red, each on its assertion; the
two that write a record for the other reserve first failed on the kit's own record, fixed in the test). The two
unit assertions of finding 9 were written before their rules and are shown red by rule-off only. Then each new
rule was switched off once (the table below).

| # | Finding | Answer | Test |
|---|---|---|---|
| 1 | High: a hand-on stranded when the log refuses the linked route | Before the first append, Core checks what `route_chosen(linked, handed_on_from)` needs: the linked order not followed already (`AlreadyOpened`), its decision record of the linked order in the log (`NoDecisionRecord`), and its plan within its acceptance, one yes per part in order, each from the reserve its first segment runs at (`PlanOutsideItsAcceptance`, `chosen-route-rests-on-its-orders-acceptance`). Where the log still refuses the `route_chosen` outright after the `handed_on` went in, an `order_refusal` of the linked order (`malformed_request`, citing its decision record) is recorded once, the order Core follows takes the `handed_on` as the log holds it, and `LinkedRefused` is the answer from then. The state machine has no input that takes a hand-on back: the contract row is with the contract's author | `hand_on_is_checked_whole_before_its_first_record`, `linked_route_the_log_refuses_is_recorded_as_refused` |
| 2 | High: the customer's hand-on must be the customer's | `by` must be role `customer` with the requester's party (`NotTheCustomer`); the linked acceptance must be `given`, its entry's `who` the customer or its agent, by the requester's party (`NotTheCustomersAcceptance`); `source_kind` maps only `customer` to a customer source, `operator` to operator, `core` to orchestrator, and refuses any other role. The operator's path takes only an `operator` (`NotTheOperator`). Fail-closed, the coordinator's default: the customer's hand-on is unreachable in production until `T-020-03` gives a linked order an intake path (a hand-off). The kit now builds the linked order's given acceptance: the customer's permitted `open_session`, the quote `shown`, the reserve's signed yes on the operator's question, the acceptance in that session | `customers_hand_on_is_the_customers_own` |
| 3 | High: `INV-10` for the customer's hand-on | It needs a failed final deposit, as a new destination does (`NoFailedDeposit`) | `customers_hand_on_from_a_floor_wait_is_refused` |
| 4 | Medium: trusted inputs | `change_destination` and `resume` read the customer's terms from the log (`fresh_terms`: in force for the part by the log's rule, as `Stuck::follow` reads it, a later one of the customer's replacing the first, permitted after the part's last wait), never the caller's; `Change` and `Resume` carry no authorisation. A resume under the terms the part waited under is refused (`NotItsAuthorisation`). `authorise` reads the terms in force and the requester from the log reader; `Terms` is gone. Every part the new terms speak for must have the same terms in force | `customers_choice_rests_on_terms_the_log_holds` (a change under terms policy never permitted; a resume under terms never permitted and under the old ones; a new destination with a new floor from a floor wait refused, whatever a caller would have claimed in force) |
| 5 | Medium: the start proof | The starting reserve is derived when the value is handed on, from Core's records: the reserve the customer's linked route starts at (the yes's origin reserve), or the provider the operator's resolution names. `prove_start` takes no reserve; a linked order Core didn't hand on is `Unknown`. A record's reference, once spent on one linked order, proves no other (`ReferenceSpent`) | `holding_accounts_record_proves_one_start_from_its_own_reserve` (`NoReference`; another reserve's record refused, the caller's word taken for nothing; the same record refused for a second linked order in another world) |
| 6 | Medium: unfollowed orders | `run_part` refuses a route starting at an inner node of an order Core doesn't follow (`StartUnproven`), so a restart fails closed until Core follows it again. The real fix is a log kind for the holding account's record (with the contract's author) | `route_handed_on_isnt_run_by_a_core_that_doesnt_follow_it` (a fresh `Segments`, a linked record that claims to follow another round: nothing handed over) |
| 7 | Medium: `done_hand_on` | The resolution's asset is the one the value sits in, its amount the part's whole held value, and its `by` is the linked order's starting reserve | `operators_hand_on_needs_a_resolution_of_the_whole_value` (a done resolution of 1, written into the kit's log as Core never would) |
| 8 | Medium: the destination change's success path untested | Run on the kit: the customer's new destination through policy, the customer's acceptance of it, a choice record the kit writes for the other reserve's deposit from the ledger, `destination_changed` taken, the part's destination the customer's, `Stuck`'s ends followed to it, the log admitting every entry; an acceptance of the old terms refused first | `customers_new_destination_is_taken_from_where_the_value_sits` |
| 9 | Low: the origin account | `new_terms` refuses a change of origin account (`ChangesOrigin`), the default "payer only" | in `new_terms_are_asked_only_for_a_part_waiting_for_the_customer` |
| 10 | Low: durability | Named: `HandOn`'s pending changes, resumes and hand-ons, its stranded refusals, the linked orders' starts and the spent records are memory only; after a restart a linked order's start can't be proven again and its segments are refused (fail closed). `F-030`'s | — |
| nit | `NotItsAuthorisation` for terms covering no part; the "kept until the log takes it" and sim-routing claims | `Refused::NotItsAuthorisation`; the claims corrected in the module's docs and above | in the unit test |

- **Rule-off of the review's rules, each red** (`t-020-13-ruleoff2.py`, the file restored and touched each time):

  | Rule switched off | Tests that went red |
  |---|---|
  | the linked decision record in the log | `hand_on_is_checked_whole_before_its_first_record` |
  | the linked plan within its acceptance | the same |
  | the linked order opened once | the same |
  | the linked route's refusal recorded | `linked_route_the_log_refuses_is_recorded_as_refused` |
  | the customer's hand-on by the customer | `customers_hand_on_is_the_customers_own` |
  | the linked acceptance the customer's own | the same |
  | the customer's hand-on after a failed final deposit | `customers_hand_on_from_a_floor_wait_is_refused` |
  | fresh terms: a replacement, permitted after the wait | `customers_choice_rests_on_terms_the_log_holds` |
  | a destination change rests on the acceptance of the new terms | `customers_new_destination_is_taken_from_where_the_value_sits` |
  | `authorise` reads the terms in force from the log | the floor, held-route, recalculated-first, destination-taken and choice tests |
  | the starting reserve from Core's records | `holding_accounts_record_proves_one_start_from_its_own_reserve`, `handed on route's withdrawal is proven ...` |
  | one record proves one start | `holding_accounts_record_proves_one_start_from_its_own_reserve` |
  | an unfollowed order's inner-start route refused | `route_handed_on_isnt_run_by_a_core_that_doesnt_follow_it` |
  | the operator's resolution moves the whole value | `operators_hand_on_needs_a_resolution_of_the_whole_value` |
  | new terms keep the origin account | `new_terms_are_asked_only_for_a_part_waiting_for_the_customer` |
  | new terms cover a part | the same |

  `source_kind` refusing another role is reached by no test: every path refuses such an actor before it builds an
  input (the customer's `NotTheCustomer`, the operator's `NotTheOperator`), so it is defence in depth.
- **Seams, after the review:** `segments/mod.rs`'s guard now refuses an inner-start route of an unfollowed order too
  (one condition). The kit gained `On::Chosen`, `append_entry`, `seed_linked` (a given acceptance) beside
  `seed_linked_on_a_deposit`, and `stated_results`, which `results_after_the_move` now calls.

### T-020-13, round 2 of the review (of `74d1823`)

- **High: a race in "already opened".** The pre-check asked only whether Core follows the linked order, so a hand-on
  pending on the log (its `handed_on` unavailable) left the linked order looking free: another part could be
  handed on to it meanwhile, and the first, retried, had its `route_chosen` refused as `order-is-opened-once`,
  stranded. A restart reached the same state, the linked order in the log and not followed. Now, before the first
  record of a hand-on and again before each retry of it, the linked order must be free: Core doesn't follow it,
  the log holds no `route_chosen` of it and no `handed_on` naming it but this hand-on's own, and no other hand-on
  pending here names it (`AlreadyOpened`; a pending one whose linked order was taken meanwhile is dropped, nothing
  recorded). Tests, each written first and seen red (the second hand-on went in):
  `a_pending_hand_on_keeps_its_linked_order_from_another` (two orders over the kit's route, the second taken up as
  `ord-5`, standing for the reviewer's two parts: the race is the same, two hand-ons naming one linked order) and
  `hand_on_to_a_linked_order_the_log_opened_is_refused_after_a_restart` (Core's segments as they were before the
  first hand-on, the log holding it). Rule-off: the check narrowed to Core's own records, both red.
- **One history the log refused, classified a test artefact.** The second order's recalculation chained its record to
  the log's first decision record, the first order's, and the log refused it ("a decision record's previous is of
  its order"). The kit's `Rerouting::of` took the log's first decision record as the order's, true only for the
  kit's own order; it now takes the current order's own. No Core change.
- **Nit: the linked order's refusal.** It names the linked order's own request, the one its acceptance was opened
  under (`Accepted::request`), and class `no_route_right_now`, the closest true class: the log doesn't take its route
  now, and no class says the log refused it. A reading, reversible.
- **Merged with `m3` at `8f6196b` (`T-020-12`).** `Segments::observe` takes both rows' kinds (`reversal_completed`,
  `end_requested`; `destination_changed`, `handed_on`); `T-020-12`'s `sent`, `answered` and `ended` sit beside
  `prove_start`. `ended` (`stm::rules::is_ended`) counts a part handed on as ended too, so `ended_by_resolution`'s
  reading of `handed_on` is the same rule twice, kept.
- **The contract's half** is with the contract's author (the coordinator's word): a log row refusing a `handed_on`
  whose linked order is opened already, and a state-machine input taking back a hand-on whose linked order was
  refused.
- **Low, after the approval of `3f30c50`: a hand-on whose record the log took.** Where the log took this hand-on's
  `handed_on` and its answer was lost, the retry found the linked order taken by its own record, or by another opener
  meanwhile, and dropped the hand-on: the log held the part handed on and Core held it waiting. Now a hand-on whose
  own `handed_on` record id is in the log skips the free-order check and goes on to the `route_chosen`; a refusal of
  that is recorded as the linked order's, and Core takes the `handed_on` as the log holds it. Test
  `hand_on_whose_record_the_log_took_goes_on_whatever_opened_its_linked_order`, written first and seen red
  (`AlreadyOpened`); rule-off (the record in the log ignored), red.
## T-020-12

- **One record, `reversal::Reversals`** (`components/core-orch/src/reversal/`). Core's record of the reversals
  segments owe, the questions asked about them, who carries each, the orders ended on request and what came after an
  order ended, which the production assembly keeps durably (`F-030`), as `Segments`. What it reaches comes in
  `reversal::Parts` (Core's clock, the catalogue by `&mut`, the log, the evidence store, fresh ids, `Segments`,
  `Exposures`, Core and the operator); a question also takes `Asking` (the order's bounds, the operator key, the
  providers' modules). Every entry is built once and appended as built, its receipt taken only where it is the
  entry's own (`stuck::is_receipt_of`). Reading, reversible.
- **Owed, on the catalogue's list** (`Reversals::owe`, `REQ-CAT-7`, `F-012`'s hand-off). A segment the order Core
  follows holds `failed_reversal_owed`, not a deposit (a failed deposit owes nothing back, the state machine's
  rule), is owed by its provider, in the asset it took in, for what it took in, as Core signed it (`Segments::sent`),
  under the id `{order}/{segment}`. Only a completed reversal discharges it (`Observation::Confirmed`). `PLAN.md`
  §4.4's inherited "owed reversals" guard is read as feeding and discharging the list; refusing to route to a
  provider that owes is asked by no row. Reading, reversible.
- **Complete only on another party's statement** (`Reversals::confirm`, `REQ-ORD-3`, ADR-0008). Core asks the origin
  reserve, the provider of the part's withdrawal, a `reversal_confirmation` question (the reversed segment's
  transaction, the asset it took in, what it took in) under a fresh transaction drawn from the segments' register
  (`Segments::claim_transaction`), signed with the operator key and appended about the order, segment and
  transaction before it is sent; its answer is appended under that transaction, so `answer-follows-its-question`
  holds and the log can verify it. The answer counts, in this order (`reversal/judge.rs`): attributable; not the
  failed provider's own word unless the failed provider is the origin reserve; the origin reserve's; not of the failed
  provider's publisher; a confirmation answering Core's question, its inner statement the reserve's own, naming the
  segment's transaction, in its asset and in full. What counts is an `evidence` (whose: the reserve; grade 4 where
  the reserve is the segment's own provider, an undone start, so `reversal-rests-on-its-evidence`'s `--unless` and
  `evidence-rests-on-statements`' provider field both hold; grade 3 for its signed confirmation of another provider's
  reversal, since grade 4 must rest on the segment's provider's statement), then `reversal_completed` citing it, the
  order read again first. Once taken: `Segments::observe`, the hold released, the owed reversal discharged. The part's
  value is evidenced at the segment's start again, so `T-020-10`'s recalculation goes on from there (its hand-off,
  the 1449 note: asserted in the origin-reserve test with `recalc::evidenced`). An answer that doesn't count is kept,
  and the next call asks again under a fresh transaction. Only a segment starting at the node the withdrawal took
  the value to, where the origin reserve holds it, is asked about (`NoReserveAtNode` otherwise, nothing asked or
  recorded, the reversal owed and held): the reserve's word is about its own node, and taken for a later node's
  segment it would place the value where it isn't (`INV-4`); the log's `reversal_answers` keys on the order's
  origin reserve too, so it wouldn't catch it.
- **Defaults taken** (reversible, each for the form):
  - The **publisher half** (core-internal's README, "A reversal's evidence and the failed provider's publisher",
    left to `T-020-12`): the coordinator's ruling after the review, replacing the strict default (see the review
    answers). A reserve of the failed provider's publisher completes nothing, except inside that publisher's own
    private node, where its word counts, labelled as the publisher's (`REQ-ORD-3`'s third sentence).
  - **ADR-0008's origin reserve is the only confirming party**: any other party's statement is kept and completes
    nothing (`NotTheOriginReserve`).
  - **The grade** of the reserve's signed confirmation of another provider's reversal: 3 (signed by its maker).
  - **An undone start** is proven only by one confirmation of the whole withdrawal; pieces are kept, never summed:
    a confirmation names no credit, so two pieces can't be told from one credit reported twice.
  - **Ending while in motion**: `end` refuses while any segment of a part that would end is handed over with no
    final outcome (`InMotion`), though the state machine would end it; its value may still move.
  - **Who answers a showing of fault**: an actor in the role the showing names (customer or operator); that it is
    the order's own customer, or the deployment's operator, is policy's (`T-021-06`).
- **Kept as it arrives** (`Reversals::report`, `REQ-ORD-30`). A statement that answers no question of Core's (the
  failed provider's own `reversal` result, a piece of an undone start) is kept as its maker's stated entry about the
  order and segment, once per bytes, and answered with why it completes nothing (`FailedProvidersOwn`,
  `NotInFull`, or `Unasked` where it would count). The log verifies a statement whose body names a transaction only
  under that transaction, which Core didn't ask, so it is unverified there and nothing rests on it.
- **Who carries it** (`REQ-ORD-11`). `Reversals::carrier`: the failed provider by default; `show_fault` keeps the
  provider's showing that the fault was the customer's or the operator's, the kind read from its signed bytes (see
  the review answers); `accept_fault` by that party's role makes it the carrier, after which no other showing is
  taken; `dispute` by it declares the part stuck through `Stuck::dispute` (`stuck_declared`, `dispute`), so the stuck
  orders hold the declaration; the provider carries it meanwhile. `confirm` still completes the reversal on a stuck
  part (asserted in the dispute test). Today the stuck orders' `decide` takes a disputed part while its reversal is
  still owed: that rule is `T-020-11`'s, and the coordinator routes it (the log's `part-waits-for-value-in-motion`
  is the contract half). An undone start's carrier is the origin reserve (`REQ-ORD-30`).
- **Ending** (`Reversals::end`, `REQ-ORD-12`): `end_requested` once the state machine takes it (a part not past its
  first segment) and nothing of such a part is in motion; then no segment of an ended part is handed over
  (`SegmentsError::Ended`). After the first segment settles the state machine takes no end, and Core records none. A
  complete part's start proven undone (`confirm` on its withdrawal) is the one later ending: `reversal_completed`
  on the starting segment, and the part, so the order, fails (`REQ-STM-6`).
- **After the end** (`returned`, `late_approval`, `seen`; `REQ-ORD-31`, `REQ-ORD-25`). The destination reserve's
  report on a complete order, and the origin reserve's approval on an ended one, each verified by the catalogue as
  its maker's, kept as its stated entry about the order and recorded as `afterwards` resting on it, once; nothing
  moves and nothing starts. `seen` answers them on a `Reads::Order` check policy allows, recorded either way.
  Notifying them is `F-023`'s (`T-023-01`, `REQ-ORD-20`): `Afterward` is what it is handed.
- **Hand-offs named `T-020-12`**, each handled or named:
  - `T-020-08` (1196, 1212, segments' header): the failed segment's hold is released by its completed reversal.
  - `T-020-10` (1449, 1466): a failed middle segment is recalculated after `reversal_completed`; its return is in
    full, checked against what it took in.
  - `T-020-11` (1625): `dispute` is this row's, through `Stuck::dispute`.
  - `T-020-11` (1695): a stuck resolution's real outcome evidence stays the production `Resolutions` row's (the
    coordinator's); nothing here.
  - `T-020-11` (1715, 1814; `conformance/scenarios/tests/stuck.rs`): a provider's genuinely late answer now has a path
    into Core, `Reversals::later`: kept as its maker's statement about the segment and its own transaction (the
    question it answers, so the log verifies it and a stuck part's resolution may cite it), moving nothing. Taking
    it as the segment's result stays out: the segments are the one writer of a segment's results, and a stuck part's
    value moves only on its resolution. Asking a provider for a late status (`Modules::status`) is a gap for a new
    `F-020` row; `T-024-06` owns `AC-5`'s scenario.
  - core-internal's README (570): the publisher half, held by Core as above, not by `CMP-EVIDENCE`.
- **Gaps, each with its owner:**
  - core-internal has no record of who carries a reversal, nor of a showing of fault or its acceptance: Core keeps
    them. A contract item for core-internal's author (`T-020-01`, a bump), with a row for `T-020-12`'s follow-up.
  - core-internal has no action for asking a failed provider to reverse (provider-role's `reversal` operation moves
    value, so policy must decide it): Core can't ask, and takes the provider's own statement only as kept. Same
    owner.
  - state-machine 2.1.0 takes an undone start only of a complete part (`undoes_a_complete_part`), so `REQ-ORD-12`'s
    undone start of a part still open can't be recorded: `confirm` refuses it (`NotOwed`) and pieces are kept. A
    state-machine bump for `F-005`'s owner with core-internal's.
  - `REQ-ORD-3`'s third sentence (inside a private node, the publisher's word, labelled as such): Core takes it and
    holds the label (`Word::Publishers`); the log's `evidence` has no field for it yet, so the label waits on the
    contract round (core-internal's author, the coordinator's).
  - A multi-reserve node (ADR-0008's own open point) has no settlement path: the origin reserve asked is the
    withdrawal's.
  - A failed segment starting at an inner node after the first (a route through two inner nodes) has no party to
    confirm its reversal: ADR-0008's reserve "at the origin node" is read as the withdrawal's, and no entry names the
    reserve or holding account at a later node. Such a reversal stays owed and held, for the stuck orders. Item for
    core-internal's author and ADR-0008's follow-up (who confirms at a later inner node, its holding account's
    reserve perhaps), apart from the multi-reserve point; `reversal-rests-on-its-evidence` keys on the order's
    origin reserve and fails open there.
- **Seams in other rows' modules** (the coordinator's word not asked; reversible; each its own function or guard):
  - `segments/mod.rs` (`T-020-08`, last `T-020-11`'s): `observe` also takes `reversal_completed` and
    `end_requested`; `Segments::sent` (the transaction and the operation Core signed for a segment); a private
    `ended` read; one guard in `run_part`, before a segment is first handed over, refusing a part that has ended
    (`SegmentsError::Ended`, a new variant); and the header paragraph naming them.
  - `stuck/mod.rs` (`T-020-11`): `Stuck::dispute`, `no_route`'s twin for `stuck_declared { dispute }`, and a paragraph
    in its docs.
- **Tests.** Ten in `components/core-orch/tests/reversal.rs`, and one unit test in the module (the node rule), with fakes over the router-api fixture's route across
  three providers (withdrawal at `reserve-bank-co`, exchange at `exchange-a` inside `ledger-in`, deposit at
  `reserve-wallet`), run through `Segments::run_part`: the simulated providers' routes stay at one reserve, whose
  failed withdrawal is cancelled and whose failed deposit owes nothing, so no kit segment can owe a reversal. Each
  test replays everything this row appended through core-internal's `attribute` and `admit_entry` over the log
  before it (`World::admitted_from`: stated entries attributed from the store's bytes; keys answered true, the
  catalogue verifying the signatures, the operator's by its key). The final deposit settles on a fake grading of a
  public record (`PublicDeposit`); that grading is `T-020-09`'s. The row's nine, and
  `late_delivery_is_recorded_and_moves_nothing` for the late-answer hand-off.
- **Touches**: `components/core-orch/src/reversal/`, the seams above, `components/core-orch/tests/reversal.rs`. No
  manifest, no dependency, no lock or `licences/RECORD` line, no `DEC-20` path.
- **Merged `m3` at `5d9dcca`** (`T-021-04b`'s per-part authorisations, `T-020-10b`'s waits at the floor) and then
  at `d4da424` (`T-020-11b`'s per-part ends in `stuck/`): no conflict; the seams above sit beside the per-part waits
  in `segments/mod.rs` and the per-part ends in `stuck/mod.rs`, and every test passes on the merged tree. The
  rule-off below ran after the first merge, and the `Stuck::dispute` row again after the second.
- **Red first, and rule-off.** The module was written before its tests (the harness had to be built to reach a
  failed middle segment at all), so, as `T-020-08` and `T-020-11` did, each rule was switched off once by a scratch
  script (`t-020-12-ruleoff.py`, outside the tree), the reversal tests run, the file restored and touched:

  | Rule switched off | Red |
  |---|---|
  | not the failed provider's own word | the failed provider's test |
  | only the origin reserve's word (ADR-0008) | the origin reserve's test (another party's answer) |
  | not its publisher's | the failed provider's test (the shared publisher) |
  | the inner confirmation the reserve's own | the failed provider's test |
  | the reversed segment's transaction | the failed provider's test |
  | in the asset it left in | the failed provider's test |
  | in full | the failed provider's test (the short credit), the partial-evidence test |
  | no deposit owes a reversal | the owed test |
  | asked again after an answer that doesn't count | the failed provider's, origin reserve's and owed tests |
  | discharged only on a completed reversal | the owed test, the failed provider's test |
  | a statement kept once | the partial-evidence test (bytes kept once) |
  | a showing only the failed provider's | the dispute test |
  | no end while in motion | the end test |
  | a return only of a complete order | the return test |
  | a late approval only after the end | the late-approval test |
  | a late report about its own transaction | the late-delivery test |
  | an undone start only of a complete part | the undone-start test |
  | an undone start's loss the origin reserve's | the undone-start and partial-evidence tests |
  | the order Core follows takes the reversal (`observe` called) | the undone-start and origin reserve's tests |
  | the hold released | the origin reserve's test |
  | only a segment starting at the withdrawal's node (after the review) | its unit test, `origin_reserve_confirms_only_a_segment_starting_at_its_node`: no fixture route reaches a later inner node |
  | a statement only of the maker it must be (destination, origin reserve) | the return test |
  | a showing answered only in its party's role | the dispute test |
  | seam: `observe` takes `reversal_completed` | the undone-start and origin reserve's tests |
  | seam: `observe` takes `end_requested` | the end and late-approval tests |
  | seam: an ended part hands nothing over | the end and late-approval tests |
  | seam: `Stuck::dispute` declares the part | the dispute test |

  An input built only where the state machine takes it, and read again before its append (completion and end), is
  held twice: each switched off alone stays green, all three together go red (the end test), as `T-020-11`'s
  two-place check. Not switched off: the answer's signature (`Unattributable`), behind which the log's attribution
  refuses an unverified answer as evidence; `Unasked` in `judge`, since `report` never completes whatever `judge`
  answers; the question appended before it is sent; and `seen`'s read check, which its test counts.

### T-020-12, review answers (of `20d3105`)

The review found 2 high and 3 medium. Each test was written first and seen red for the reason it was written, the
module's API stubbed so it compiled: the malformed answer (`Err(Unrecordable)`), the reviewer's end sequence (the
kept end appended while the withdrawal moved), a showing naming no fault (taken), a late report before the answer
(kept), and an inner confirmation of another shape (completed). Assertions further down a test that a first one
masked are shown by the rule-off below. Then each new rule was switched off once.

| # | Finding | Answer | Test |
|---|---|---|---|
| 1 | High: `end_requested` recorded while the withdrawal moves | `record_end` reads the order's segments again before every append, a retry's included (`in_motion`), and drops a kept end that finds one in motion | the end test, the reviewer's sequence: the log down for the first `end`, the withdrawal handed over with its outcome unknown, the second `end` refused `InMotion` and nothing recorded |
| 2 | Medium: a malformed answer wedged its question | An answer nothing can be kept of drops the question, as silence does, answered `Unreadable` | `malformed_answer_drops_its_question_and_is_asked_again` |
| 3 | Medium: the notes and a test comment said a disputed part waits for its reversal | Corrected: today the stuck orders decide it while its reversal is owed; that rule is `T-020-11`'s, routed by the coordinator | — |
| 4 | Low-medium: the fault came from the caller, and a carrier could change again | The fault is read from the signed bytes: the failed provider's result for the segment's own transaction, an evidence piece's `reference` naming `fault:customer` or `fault:operator`, exactly one (`NoFaultShown` otherwise). provider-role names no field for it, so `FAULT_REFERENCE` is Core's name until it does: an item for provider-role's author. The showing is kept about the segment's transaction, so the log can verify it. Once a showing is accepted or disputed, no other is taken (`FaultSettled`) | the dispute test |
| 5 | Low: `dispute` marked the showing before the stuck orders recorded it | Marked only once `Stuck::dispute` succeeds | the dispute test: the log down for the dispute, the customer still accepts |
| 6 | Low: nothing killed disabling `AnotherQuestion` | An answer to an earlier question, replayed under a fresh one, completes nothing | in the failed provider's test |
| 7 | The publisher default | The coordinator's ruling: inside the failed segment's private start node, the publisher holding it (`register_private_holding`, `HoldingSource::Publisher`) being the publisher of both the origin reserve and the failed provider, the reserve's word counts, labelled the publisher's (`Completed::word`, `Word::Publishers`). The log's label waits on the contract round; until it lands the `evidence` says whose it is, the reserve, and Core holds the label. Correction: advertised providers, Bloque's included, are `Publisher::Party`, their advertisement's publisher; `Publisher::Operator` is for a module with no advertisement, such as a public node's holding account. The earlier note's "every provider is the operator's own, so no middle reversal completes in v0" was wrong on that reading too | in the failed provider's test: one publisher outside its node refused, inside its node counted as its word |
| nit | No publisher for the maker | Refused, `NoPublisher` | defence: a maker the catalogue verifies has one, so no test reaches it alone |
| nit | The inner statement's shape | Held to `reversal-confirmation`'s shape | in the failed provider's test |
| nit | `later` before the answer was taken | Refused, `NotAnswered`, read by a new `Segments::answered` | the late-delivery test |
| nit | A grade-4 own-word assertion | The undone start's `evidence` in the log: grade 4, whose the reserve, `Word::Reserves` | the undone-start test |

- **Seam**: `Segments::answered` (`segments/mod.rs`, read only), and its line in the header paragraph.
- **Rule-off, each new rule once** (`t-020-12-ruleoff2.py`, outside the tree; each file restored and touched):

  | Rule switched off | Red |
  |---|---|
  | the end read again before every append | the end test |
  | a malformed answer drops its question | the malformed-answer test |
  | the fault read from the signed bytes | the dispute test |
  | an accepted or disputed showing stands | the dispute test |
  | a showing disputed only once the stuck orders recorded it | the dispute test |
  | an answer only to Core's own question (`AnotherQuestion`) | the failed provider's test |
  | the publisher's word inside its own node counted | the failed provider's test |
  | only inside its own node | the failed provider's test |
  | the inner statement's shape | the failed provider's test |
  | a late report only once the answer was taken | the late-delivery test |
  | a showing kept about its segment's transaction | the dispute test |

  `NoPublisher` wasn't switched off: no maker the catalogue verifies lacks a publisher.


### T-020-12, round 2 answer (of `f4a232f`)

| # | Finding | Answer | Test |
|---|---|---|---|
| 1 | Low: an accepted fault could still end disputed. A dispute the log couldn't take now leaves `Stuck::dispute`'s built `stuck_declared { dispute }` kept, and a later watch or dispute appends it, so the part would go stuck over a fault the customer accepted since | A dispute answered `Unavailable` marks the showing pending: `accept_fault` is refused (`DisputePending`), and so is a new showing, until the dispute is retried to completion, after which the dispute stands. A dispute the log refused keeps nothing in the stuck orders, so the showing stays open | the dispute test: the reviewer's sequence (log down for the dispute, acceptance refused, the dispute retried, the part stuck), and a refused dispute after which the customer still accepts |
| nit | `fault_shown` took any result | Only a `failed` result shows a fault | the dispute test: a succeeded result naming a fault shows none |

- **Red first:** the tests were written before either fix and run: the succeeded result was taken as a showing; with
  that fixed alone, the acceptance after the kept dispute went through (`Ok(Customer)`). Then each fix, and each
  rule switched off once (`t-020-12-ruleoff3.py`, outside the tree):

  | Rule switched off | Red |
  |---|---|
  | no acceptance while a dispute is kept | the dispute test |
  | a dispute answered `Unavailable` kept as pending | the dispute test |
  | a refused dispute keeps nothing | the dispute test |
  | only a failed result shows a fault | the dispute test |

  A new showing refused while a dispute is kept is defence beside the refused acceptance; no test reaches it alone.
