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
  gave out, attributed to the operator's `acts`, recorded at zero too. `billed_separately` is what the provider stated, absent where it said
  nothing (provider-role 3.1.0 added the field, below); the operator's fee record says nothing. Fees are recorded whatever the grading, `ProvidersWord` included.
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
  - A fee billed separately: closed by provider-role 3.1.0's optional `fee.billed_separately`, which the fee record
    carries as stated; no row.
  - A private node's record of its inner segments has no provider-role shape, so `Books` is a seam the assembly fills
    from each publisher (`F-015`'s notes): `T-020-18` carries the shape, `T-024-14` is its consumer.
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

### T-020-13, integration round 2 (the contract round at `0f93569`, with `int2-policy` at `7caf118`)

- **The start proof is the log's record** (`holding_withdrawal`, `handed-on-route-starts-on-its-holding-record`).
  `HandOn::prove_start` takes `Parts` and a `Start`: the linked order, the holding account's `WithdrawalRecord`, the
  receipt of the holder's stated entry of the withdrawal, and Core. Core holds the record to the start as before
  (`proves_start`), then records `holding_withdrawal { order, handed_on_from, holder, record }`, built once and
  appended as built while the log can't take it; the segments hand the linked order's segments over only once the
  log takes it. This is the fix for the history the policy and closing owners found the log refusing (entry 50 of
  `handed on route's withdrawal is proven ...`, `handed-on-route-starts-on-its-holding-record`): a real Core bug, since
  nothing wrote the start proof to the log. The holder's statement is a result about the linked order naming its
  transaction (`holding-withdrawal-rests-on-its-record`); the kit's `holding_statement` makes one under a transaction
  of its own each time, so the log's one start per record, keyed by that transaction
  (`one-start-per-holding-record`), holds. Core's own spent check stays keyed by the record's reference, which names
  the same transaction.
- **A refused linked route takes the hand-on back** (`hand_on_taken_back`,
  `hand-on-taken-back-rests-on-the-linked-orders-refusal`). Where the log refuses the linked order's `route_chosen`
  after the `handed_on` went in, Core records the linked order's refusal, then the part's `hand_on_taken_back`
  (source `orchestrator`), appended as built while the log can't take it; the order Core follows takes it and the
  part waits again where its final deposit failed (or is stuck). The answer is `LinkedRefused`, once; the part is no
  longer stranded. A take-back closes the linked order for good (`linked-order-opens-on-its-hand-on`): Core never
  appends its `route_chosen` afterwards, and a later hand-on to it is refused `AlreadyOpened`, since the log holds the
  earlier `handed_on` naming it.
- **One hand-on per linked order** (`hand-on-links-an-unopened-order`). Core's free-order check already refused a
  second hand-on to a linked order named in the log or pending. The lost-reply test's other opener, another order's
  `handed_on` naming the same linked order, is a history the log now refuses, classified as such and removed: the
  test keeps the lost answer and the retry, and the log admits every entry. The skip of the free-order check for a
  hand-on whose own record the log holds stays, as defence in depth: no other hand-on can name the linked order once
  this one is in.
- **A given acceptance** for a part that isn't stuck, and **`INV-10`** in the log: Core already required both
  (a given acceptance for every hand-on, a failed final deposit for the customer's).
- **The `publisher` label** at `segments/ports.rs` (`ProvidersWord`) and the kit's `Recorded`: `None`, with the reason:
  the label of a private node's publisher counts only for a reversal's evidence (`reversal-rests-on-its-evidence`),
  so a settlement carries none.
- **Seam** in `segments/mod.rs`: `observe` takes `hand_on_taken_back` (one arm).
- **Red first:** the start-proof and refusal tests were changed first and seen red on the inert `prove_start`
  (no `holding_withdrawal`; no take-back). **Rule-off, each red:** the `holding_withdrawal` recorded (the start-proof
  test); the take-back recorded (the refusal test); `observe` taking `hand_on_taken_back` (the refusal test).
- **`Segments::status` no longer tells a refused reader whether an order exists** (the coordinator's item, on
  `T-020-08`'s read, as `T-023-01` did for notices). It answered `Unknown` before asking policy, and passed policy's
  whole answer, its reason included, inside `Refused`. Now policy is asked first, whether or not Core follows the
  order, and a refused reader is told `StatusError::NotReadable` alone, whatever the reason, which stays in the log;
  only an allowed reader is told `Unknown`. `StatusError::Refused(ReadAnswer)` stays for `stuck::seen_by_provider`
  (`T-020-11`) and the reversal's afterward read (`T-020-12`), which have the same leak (each answers `Unknown`
  before asking policy, and passes policy's answer): a hand-off to their owners. Test
  `stranger_is_refused_alike_for_a_real_order_and_a_missing_one` (`conformance/scenarios/tests/segments.rs`), written
  first and seen red (the real order refused with policy's answer, the missing one `Unknown` before policy was
  asked): a stand-in for policy refuses the two for different reasons and records each, since the engine now refuses
  both `not_permitted` (`4415700`); the stranger gets one bare answer for both, and the log keeps both reasons.
  Rule-off, each red: policy asked first (the new test); the refusal opaque (the new test and the state-and-age
  test). The kit gained `append_entry_to`.

## T-020-07

Branch `f-020/t-020-07-quote-shown`, from `m3` at `d950ecd`.

- **One module, `quote`, and two call sites.** The rules are the router's pure functions (`rutent_router::quote`,
  `T-011-03`), over router-api's types (`T-020-02`). This row is Core's use of them, in
  `components/core-orch/src/quote/`: `show` and `executed` (the price shown), `Prices` (the price reported), `Terms`
  (stated terms with their times), `Holds` (a reserve's held quote) and `Charges` (a charge above its formula,
  raised). Intake's `Quote` gains `price: quote::Shown`, as `T-020-03`'s notes left for this row ("changes the digest
  and nothing else here"). Its `shown_as` comes from the price, and so does the `shown` entry's. Arrival's filing
  (`T-020-04`) builds the same `Quote` for an automatic deposit, so the new field made it take the price too. The
  edit there is the price, its label, and one behaviour change: a route `show` won't show ends the filing's order
  with the `NotShown` class (`self.end`), as intake refuses it. No arrival test reaches that branch: an untested
  change to `T-020-04`'s module, for its owner or the coordinator to take or test. What arrival does with a route
  without a firm rate is a gap (below).
- **The quote shown (`REQ-QTE-2`).** Read as composition, on the owner's ruling `spec-route-price-composition`.
  `Shown` names:
  - each segment's provider fee and amounts;
  - `providers_out`, what the providers' terms deliver as they state it: the sum of the quotes, each part's last
    `amount_out`, summed;
  - `within`, each charge a part's last segment reports within what it gives out (below);
  - `arrives`;
  - the operator's fee, its formula from the bounds the chosen record ran under (`record.bounds.operator_fee`,
    never the caller's), and its amount on what arrives;
  - `shown_out` and `difference`.

  `difference` must equal the named lines, the operator's fee plus each charge within, or nothing is shown
  (`Unexplained`). The router's own `quoted_price` (or `executed_price`) is run over the candidate with each part's
  last `amount_out` set to what arrives, and its `check_shown` must accept `shown_out`. So the router's floor, its
  price and Core's lines are one rule. Ranking stays before the operator's fee (`m3-rank-before-fee`): this module
  ranks nothing.
- **A gross charge is taken off (`T-020-08`'s hand-off).** The router takes a confirming answer's `amount_out` as it
  stands. Under `gross` reporting that amount holds the charge, so the next segment is priced on more than arrives.
  `segments::judge` hands on `amount_out` less the charge, and `show` reads each tuple's `reporting` from the
  record's snapshot and does the same:
  - a conversion hands on its output as it is;
  - a part's last segment that reports gross has its charge named in `within` and taken off before the operator's
    fee;
  - a segment before another that hands on less than the next was quoted to take in can't deliver as composed, so
    nothing is shown (`Undeliverable`).

  Seen empirically: a gross withdrawal answering 1 000 out with a charge of 4 is refused, and a gross deposit shows
  1 000, less 4 within, less the operator's 10. The router pricing and ranking on what arrives is `F-011`'s (a gap).
  Since the review, a segment that keeps its asset hands on exactly what `segments::judge::handed_on` gives (below).
- **Never a price the route can't deliver.** `NotShown`, and intake's class for it (reversible):
  - `no_route_for_amount` for the operator's fee above what arrives, `shown_out` below the request's floor once a
    charge within is taken off, and a sum past an amount;
  - `no_route_right_now` for undeliverable, no terms for a segment, no slippage limit, and unexplained.

  An `order_refusal` of that class is recorded, and nothing is shown. Through intake this is
  `route_intake_cant_deliver_is_refused_and_never_shown`: the kit's reserve reports `gross`, and a charge of 4 on
  each segment leaves its withdrawal handing on less than its deposit was quoted at. The customer is told
  `no_route_right_now`, one refusal is recorded, no `shown`, and the log admits it all.
- **Slippage (`REQ-QTE-9`).** Each segment carries `held_to`, from the router's `commitments` over the record's own
  limits:
  - `committed`, with `rate_bps` (the tighter of the quote's regime and the operator's limit) and
    `operator_limit_bps` (the bounds' limit for the tuple, matched on provider and ends);
  - `free` past three days.

  A segment that sets no rate is committed with neither. "Recorded": the decision record's bounds name the limit
  (`REQ-AUD-4`), and the `shown` entry names the digest of the quote that shows it. A segment the bounds give no
  limit for (`NoSlippageLimit`) is never shown. `check_against` makes it unreachable for a record read from the wire,
  so the test builds one field by field.
- **No firm rate (`REQ-QTE-10`, `AC-13`).** Where `firmness` names segments over three days, the quote is
  `quote_without_firm_rate`. `no_firm_rate` names each segment: part, operation, provider, tuple figure and declared
  seconds. The `shown` entry carries the label too, and core-internal's `acceptance-is-of-the-quote-shown` admits an
  acceptance of it. Intake starts an order only on its customer's acceptance of the quote shown, so a slow route
  starts only then.
- **Quote, then price (`REQ-QTE-7`).** `executed` composes the same lines from what each segment did (router-api's
  `Executed`, as the results that counted state it), labelled `price`. It refuses (`NotExecuted`) until every segment
  has executed, so until then what is shown stays the quote. `Prices::report` records an order's price once, as a
  `shown` entry of `price` naming the digest of a `Priced` (order, record, candidate, price). The entry is built
  once and appended until the log takes it; anything not labelled the price is refused (`NotThePrice`), and a second,
  different price for the order too (`AnotherPrice`).
- **Stated terms (`REQ-QTE-6`).** `Terms` keeps each tuple's history of router-api `StatedTerms`, keyed by provider
  and ends; the same terms twice are kept once. `binding` is the router's: the last stated at or before the segment's
  start, whatever order they were kept in, and a quote before it. It is in memory, as `Issued` is; keeping it is
  `F-030`'s. The history rests on no stated entry: provider-role has no statement of a provider's terms (a gap).
- **Held quotes (`REQ-QTE-15`).** `Holds::hold` runs the router's `hold`: a choice record that chose, and a validity
  at or after its snapshot. Only the reserve every part of the route starts at may hold it (reading, reversible). The
  quote is shown as `show` shows it, and the hold is refused if anything would label it the price. `redeem` is the
  router's: usable at its validity, refused past it, changing nothing. `drop_hold` has no effect beyond the hold. No
  stated entry: provider-role has no statement by which a reserve asks for a hold (a gap). Whoever starts a route on
  a held quote redeems it first; that caller is a gap.
- **Charges (`REQ-QTE-5`, `AC-14`).** `Charges::check` takes a segment's charge as its result that counted states
  it, with the receipt of the result's stated entry. The formula is the terms that bound the segment at its start
  (`Terms::binding`, so `REQ-QTE-6` binds the check), or else the formula the route was priced on. With neither it is
  `NoFormula`, never passed. `check_charge` is the router's: against the maximum where the formula gives one, and
  else against the fee it gives. A charge above is a `Raise` of `difference: charge` (`maximum` is the bound,
  reading: for a formula with no maximum, the fee it gives), citing the result's stated entry. It is raised once,
  and its `raise` entry is recorded `done` where it reached the operator and `failed` where it didn't, as
  `stuck::learn` records its raises. Each segment is checked once. A raise made but not yet recorded is appended as
  built on the next check, never raised again.
- **Tests, and the order.** The seven tests of the row are in `components/core-orch/tests/quote.rs`, with a kit,
  `tests/quote_support/`, that routes `RouteFinder` over choice-round snapshots written as router-api's JSON: a
  reserve's withdrawal and deposit, an exchange between them where asked, and answers that are gross or net. Three
  tests beyond the row's list pin the gross reading:
  - `charge_reported_within_what_a_segment_gives_out_is_a_line_of_its_own`;
  - `route_that_cant_hand_on_what_its_next_segment_was_quoted_at_is_not_shown`, with the floor case;
  - `route_intake_cant_deliver_is_refused_and_never_shown`, through intake, written after the advisor's review,
    so seen red only by its rule-offs (the last two rows below).

  `operators_fee_is_shown_apart_from_the_quotes_sum` and
  `route_with_a_segment_over_three_days_starts_only_after_acceptance` also run through intake, on `T-020-03`'s kit
  unchanged, and replay its log as the log would admit it. The tests were written first, over a stubbed API (`show`
  answering zeros labelled a quote, `executed` and the rest refusing or doing nothing), and each was seen red on its
  assertions (the nine of `quote.rs`) before the bodies were written.
- **Rule-off, each red** (`t-020-07-ruleoff.py`, outside the tree: each rule switched off once, its test run, the
  file restored and touched):

  | Rule switched off | Red |
  |---|---|
  | the operator's fee shown apart, its amount on the line | `operators_fee_is_shown_apart_from_the_quotes_sum` |
  | a gross charge taken off what is handed on | `route_that_cant_hand_on_…` |
  | the next segment's amount held to what the one before hands on | `route_that_cant_hand_on_…` |
  | a charge within named as its own line | `charge_reported_within_…` |
  | never below the floor | `route_that_cant_hand_on_…` |
  | the tighter of the quote's slippage and the operator's limit shown | `segments_slippage_limit_is_recorded_and_shown` |
  | no slippage limit, not shown | `segments_slippage_limit_is_recorded_and_shown` |
  | a segment over three days leaves no firm rate | `route_with_a_segment_over_three_days_…` |
  | intake records the label the price has | `route_with_a_segment_over_three_days_…` |
  | no price until every segment has executed | `price_before_execution_is_labelled_…` |
  | the executed price labelled the price | `price_before_execution_is_labelled_…` |
  | only the price reported as the price | `price_before_execution_is_labelled_…` |
  | an order's price reported once | `price_before_execution_is_labelled_…` |
  | terms bind from the segment's start | `terms_stated_after_a_segment_started_do_not_apply_to_it` |
  | a charge checked against the terms that bound it | `terms_stated_after_a_segment_started_do_not_apply_to_it` |
  | a held quote past its validity refused | `held_quote_past_its_validity_starts_nothing` |
  | only the reserve the route starts at holds its quote | `held_quote_past_its_validity_starts_nothing` |
  | the charge compared with its formula | `charge_above_the_formulas_maximum_is_flagged_to_the_operator` |
  | a segment's charge raised once | `charge_above_the_formulas_maximum_is_flagged_to_the_operator` |
  | an undelivered raise recorded `failed` | `charge_above_the_formulas_maximum_is_flagged_to_the_operator` |
  | through intake: the next segment's amount held to what the one before hands on | `route_intake_cant_deliver_…` |
  | through intake: a gross charge taken off what is handed on | `route_intake_cant_deliver_…` |

  Two checks are defence in depth, and no test reaches either alone. The check that the lines add up stayed green
  when switched off: `shown_out`, `arrives` and `difference` are computed from the lines, so they can't disagree. The
  cross-check against the router's own price is the same identity. Arrival's label isn't switched off: no arrival
  test routes a slow route.
- **Gaps, with their owners.**
  1. **No statement of terms or of a hold** (a contract change, for the coordinator's `F-020` contract row).
     core-internal's README homes `T-020-07`'s "stated terms with their times; a held quote" in stated entries, and
     provider-role 3.x has no shape that carries either: an advertisement's terms hold bands, reporting and inputs,
     and a quote holds an amount, not a formula. The change is provider-role (a minor), with two statement shapes:
     - `urn:rutent:provider-role:terms`, made by a tuple's provider, its `made_at` the time stated: the tuple by
       its two ends, its fee formula (flat, proportion, optional maximum) and, for an exchange, its slippage regime;
     - `urn:rutent:provider-role:hold`, made by the reserve: the choice record's digest and the hold's validity.

     Then `Terms::state` and `Holds::hold` would take the signed bytes, keep them and append their stated entries,
     and a raise would cite the terms' entry beside the result's. Until then both registers rest on Core's inputs. No
     `contracts/` file was edited.
  2. **The router prices and ranks on gross amounts** (`F-011`). `search::step` takes a confirming answer's
     `amount_out` as it stands, so under `gross` reporting the next segment is priced, and candidates are ranked, on
     more than arrives. Core refuses a route it can't deliver as composed, and names a last segment's charge within.
     The router taking the charge off would make such routes deliverable and rank them on what arrives.
  3. **A route without a firm rate on an automatic deposit** (`T-020-04`'s module, for the coordinator). Arrival's
     filing now labels such a quote `quote_without_firm_rate`. It still starts the route on the automatic deposit,
     which counts as acceptance (`REQ-ORD-15`), where `REQ-QTE-10` wants the customer's. The reading proposed is to
     fail closed: the filing ends the order, or waits for the customer, when `price.no_firm_rate` isn't empty.
  4. **The callers** (`T-024-02` for the scenarios' assembly, `F-030` for production):
     - `Charges::check`, fed from each settled segment. `Segments` exposes no settled result's charge, amount in,
       start time or result receipt (`settlements` carries times only), so it needs an accessor on `T-020-08`'s
       module or the assembly's reading of the log.
     - `Prices::report` once every segment has executed (closing, `T-020-09`, or the assembly).
     - `Holds::redeem` before any route starts on a held quote (arrival or the assembly).
     - `Terms`, fed from wherever Core's prices come from (`snapshot::Price`, still an input).
- **Defaults taken, reversible:**
  - the refusal classes above;
  - a charge within read as `segments::judge` reads it, with no regard to `billed_separately`;
  - a next segment quoted at less than the one before hands on is still shown (it moves more, never less);
  - only the reserve each part starts at holds its quote;
  - `difference: charge`'s `maximum` is the formula's fee where the formula gives no maximum;
  - an executed price is reported once per order;
  - the price of a held quote is shown under the hold's own record's bounds and floor.
- **Touches**:
  - `components/core-orch/src/quote/` (the module, five files);
  - intake's `Quote` and its build site (`intake/mod.rs`);
  - arrival's filing (`arrival/filing.rs`): the price and label the new field forced, and the untested end on a
    route not shown (above);
  - the tests `tests/quote.rs` and `tests/quote_support/`, which reuse `intake_support`, `rounds_support` and
    `snapshot_support` unchanged;
  - these notes.

  No `DEC-20` path, no shared manifest, no dependency.

### T-020-07, review answers (round 1)

One commit per finding; the map is at the end. Each new test was written first and seen red on its assertion, on
the code under review or on the rule stubbed off, before the fix.

- **1 (high), `show` handed on more than the judge allows.** For a segment that keeps its asset, `show` now reads
  what it hands on through `segments::judge::handed_on`, re-exported `pub(crate)` from `segments` (one line in
  `T-020-08`'s module, on the coordinator's word). `None` is `Undeliverable`. Under `net`, what it gives out plus its
  charge must be at most what it takes in; under `gross`, what it gives out must be at most that. The judge reads
  amounts as `u128`, so a larger amount isn't shown either. A conversion still hands on its output as it is: its
  quote and slippage bound it. Test `segment_claiming_more_out_than_it_takes_in_is_never_shown`, the reviewer's
  sequence: USDC to EURC through an exchange at a floor of 950 000, and an EURC deposit taking in 900 000, net, free,
  whose dry run claims 950 000. It was shown before the fix.
- **2 (high), an automatic deposit started a route with no firm rate.** Arrival's filing ends the order
  `no_route_right_now` when the price names a segment over three days. The edit is in `T-020-04`'s module, on the
  coordinator's word; ending or waiting for the customer is on the owner's form. One correction to the brief: the
  deposit isn't left free to be filed again. The filing holds the deposit against its order before it routes, and
  `one-order-per-deposit` lets a deposit open one order, so every end at routing, this one included, leaves it
  `NotFileable`, and only a new notice is a new arrival. Waiting for the customer would keep the order open instead;
  that is the form's choice. Test `automatic_deposit_never_starts_a_route_with_no_firm_rate`: the arrival kit with
  the deposit priced at four days. Before the fix it started; now it ends, and filing again is `NotFileable`.
- **3 (medium), a stale or foreign held quote.** `Holds::hold` takes the order, and caps the stated validity at the
  earliest expiry of any figure the chosen candidate cites. A figure's expiry is the snapshot's time plus what is
  left of its validity, rounded down. `redeem` takes the reserve and the order and refuses anyone else
  (`NotTheHolder`). It isn't single use, as the brief allows. `HeldQuote::check`, router-api's, is not edited: the
  brief keeps `contracts/` for the contract round. The change there is: "`HeldQuote::check` refuses a `valid_until`
  later than the earliest expiry of the figures the chosen candidate cites", with an invalid fixture, folded into the
  unreleased router-api 4.0.0, for the contract round. Tests:
  - `held_quote_past_its_validity_starts_nothing`, which now holds within the dry runs' validity (it held past it);
  - `long_hold_is_capped_at_its_figures_validity`, a hold asked for 15:30 and held to 15:03:40;
  - `held_quote_is_redeemed_only_by_its_reserve_for_its_order`.
- **6 (medium-low), a price with nothing executed.** `executed` takes each segment's counted amounts with its
  result's receipt (`ExecutedSegment`), and refuses one result cited for two segments (`ResultCitedTwice`).
  `Priced` has no public field and no `Deserialize`. `Priced::new` makes it from `executed`, citing the results.
  `Prices::report` checks again that it is labelled the price and cites one result per segment. Tests: the price
  test, rebuilt on cited results, and `price_rests_on_each_segments_own_result` (nothing executed; a result cited
  twice).
- **7 (low), the charge cache.** `Charges` keeps the result's receipt, the amount in and the charge each segment was
  checked on. The same segment on another result or charge is refused (`AnotherCharge`) and raises nothing. Test
  `segment_checked_on_one_result_refuses_another`, the 10-then-9 000 probe.
- **8 (low), `Quote.delivers`.** In intake and in arrival, it is now `price.arrives`: before the operator's fee, net
  of any charge reported within. Its readers, the scenarios' assembly and recalc's `RouteEnd`, read it as before the
  operator's fee. `Quote` is intake's, not a contract shape. The intake kit can't make `arrives` differ from the
  quotes' sum (its reserve reports gross, and any charge on it is undeliverable), so the test asserts the equality.
- **9 (low), arrival's untested end.** The branch is kept, with
  `automatic_deposit_whose_route_cant_be_delivered_ends_unshown`: the reserve charges 4, gross. The order ends
  `no_route_right_now`, with its refusal and no `shown`.
- **10, tests.** `route_that_cant_hand_on_…` drops its `REQ-ORD-21` tag. `split_route_is_shown_and_held_whole`
  covers a split: the kit's `split_route` gives 600 and 400 under bounds of two parts, made as intake's `Splitter`
  makes one. It checks `amount_in` over both parts, each part's segments, and a hold refused where a part starts at
  another reserve. `AnotherPrice` is exercised in the price test.
- **Nits.**
  - `Charged.advertised` is required, since every operation cites a price, and `NoFormula` is gone.
  - A raise's `done` and `failed` entries are built before it is raised.
  - `Charged.provider` must be its tuple's (`NotTheTuplesProvider`).
  - `provider_at` refuses (`NotTheRecords`) where no segment sits.
  - The AC-14 test's formula gives 1 000 before use, with a maximum of 2 000.
- **Out of this round** (coordinator): signing `Terms` and `Holds` (finding 4, contract round 3) and the router's
  gross ranking (finding 5, its own task). No caller is wired to the in-memory registers.
- **Rule-off after the round, each red** (`t-020-07-ruleoff2.py`, outside the tree, over every rule the row now
  has, old and new: each switched off once, its test run, the file restored and touched):

  | Rule switched off | Red |
  |---|---|
  | the operator's fee shown apart | `operators_fee_is_shown_apart_from_the_quotes_sum` |
  | a kept-asset segment hands on what the judge allows (1) | `segment_claiming_more_out_…`, `route_intake_cant_deliver_…`, `route_that_cant_hand_on_…` |
  | the next segment's amount held to what is handed on | `route_that_cant_hand_on_…`, `route_intake_cant_deliver_…` |
  | a charge within named as its own line | `charge_reported_within_…` |
  | never below the floor | `route_that_cant_hand_on_…` |
  | amount in summed over every part (10) | `split_route_is_shown_and_held_whole` |
  | the tighter slippage shown; no slippage limit, not shown | `segments_slippage_limit_is_recorded_and_shown` |
  | a segment over three days leaves no firm rate; intake records the price's label | `route_with_a_segment_over_three_days_…` |
  | an automatic deposit never starts a route with no firm rate (2) | `automatic_deposit_never_starts_a_route_with_no_firm_rate` |
  | arrival ends a route not shown with its class (9) | `automatic_deposit_whose_route_cant_be_delivered_ends_unshown` |
  | no price until every segment executed; labelled the price; reported once; another refused | `price_before_execution_…` |
  | one result per segment (6) | `price_rests_on_each_segments_own_result` |
  | terms bind from the segment's start; a charge checked against them | `terms_stated_after_a_segment_started_…` |
  | a held quote past its validity refused | `held_quote_past_its_validity_starts_nothing` |
  | a hold capped at its figures' validity (3) | `long_hold_is_capped_at_its_figures_validity` |
  | redeemed only by its reserve for its order (3) | `held_quote_is_redeemed_only_by_its_reserve_for_its_order` |
  | only the reserve every part starts at holds (10) | `split_route_is_shown_and_held_whole` |
  | the charge compared; raised once; an undelivered raise `failed`; the tuple's provider (nit) | `charge_above_the_formulas_maximum_…` |
  | a segment checked on one result (7) | `segment_checked_on_one_result_refuses_another` |

  The lines-add-up identity stays defence in depth, as before. So does building both raise entries before the
  raise: no test can make a checked entry fail to build.
- **Commits, by finding:**
  - `596249f`: 1;
  - `3ab29cb`: 2;
  - `7fa54d0`: 3;
  - `cb8cbf0`: 6, and 10's `AnotherPrice`;
  - `c9379c8`: 7;
  - `71f89ef`: 8;
  - `10b4a8c`: 9;
  - `3f528a4`: 10;
  - `c94bb09`: the nits;
  - the commit that adds these notes.

### T-020-01, contract round 3 integration: notes for whoever wires the quote module

The quote module's `Terms`, `Holds` and `Charges` are libraries no part of Core calls yet, and intake's acceptance
sets `What::Acceptance.held` to `None`. Whoever wires them (T-020-07's quote owner) must close these, which the
integration left open:

- **The statement must match the entry's commitment.** `Terms::state` and `Holds::hold` take a stated entry, its
  receipt and the signed statement, and check the entry's shape, attribution, maker and times against the statement,
  never the entry's commitment against the statement's bytes: the log holds only a salted commitment, so the caller
  must have the evidence store's kept bytes behind that commitment be the statement it hands in. Wired to a real
  intake, the call fetches the kept statement and compares before it keeps terms or a hold.
- **`redeem` does not verify the caller's session.** `Holds::redeem` takes the session an order was accepted in as the
  caller states it, and `check_redemption` compares it with a session hold's. The caller must pass the session
  intake's acceptance actually names, which intake verifies; `redeem` holds the hold to that and nothing more.
- **`redeemed_by` lives in memory.** The first order to redeem a hold binds it for this process only. Across a
  restart the log's `hold-is-redeemed-by-one-order` is the guard; the deployment's store (`F-030`) must keep it, or a
  restarted Core relies on the log refusing a second order's acceptance after the fact.
- **No caller redeems a hold during acceptance yet.** `Redeemed::hold` is the receipt to put in `held`.
- **No module hands a provider's signed terms to `Terms::state` yet.** A charge raised on `Charges::check` names the
  terms in force only from what was kept.

Other things this integration settled, for the record:

- A record's operation states what arrives (router-api 4.0.0). `show` adds a gross charge back only where an accepted
  answer set the amount, so `handed_on` and the lines shown read what a result would state; a priced operation is
  checked as it states.
- The close figures the operator's fee on what arrives by the segment (`ToGrade::arrives`), as the quote figures it.
  A public movement is checked against what the network actually moved at that end: onto a public node (a final
  deposit, and the exit replay's `PublicExit`), what arrives, the charge staying with the reserve; from a public node
  (a withdrawal into a reserve), what entered, the charge included (the result's `amount_in`). The simulated network
  (`conformance/src/sim`, `movements_made`) records the same.

## T-020-18

Branch `f-020/t-020-18-arrival-notice-contract`, from `m3` at `34c700e`. A provider-role round, folded into 3.1.0
(unreleased on `develop`, 3.0.0 there), and, after its review, one row of core-internal 1.0.0 (unreleased). Touches
`contracts/` only, and two test constants (below).

- **The notice** (`urn:rutent:provider-role:arrival-notice`, `ArrivalNotice`): the reserve's own statement of the
  transaction it gives the credit, its per-credit reference, with the outer node, the asset and unit, the amount, the
  account and its holder. The transaction is the deposit's signed transaction the log already keys on
  (`arrival-names-its-notice`, `one-order-per-deposit`), so `notice_transaction` reads it as it stands.
  `check_one_credit` knows two notices of one credit as one. Two notices under two references are two claims, the
  reserve's loss if false (`REQ-AUD-2`).
- **The question and its answer** (`arrival-question`, `arrival-answer`): shapes of their own, not an operation and
  a result kind, which would have been a major carrying module-interface with it. The question names its own
  transaction, the holder's session (a balance check, `REQ-CAT-4`), the account, node and asset with its unit; the
  answer names the question's transaction and carries each notice whole, or says nothing arrived, or refuses
  without saying when it lifts. `check_answer` holds one to the other.
- **The inner-segment record** (`inner-segments`): made by a provider stating for its publisher, which holds no key;
  `check_inner_segments` takes the maker's advertisement (its publisher, the node among its private nodes). Labelled
  `publishers_word`; no figure refused, so a mismatch reaches the replay.
- **The review's high finding** (live on `m3` before this branch): `arrival-names-its-notice` didn't read the cited
  entry's shape, so a reserve's hold, about its own transaction, passed as a deposit's notice. The row now requires
  the arrival notice's shape; an answer's notices are each kept as their own stated entry, and an arrival cites
  those. core-orch's `tests/arrival.rs` and the scenarios' `tests/assembly_arrivals.rs` sign their notices under that
  shape now (test-only, their bodies their own until `T-020-19`), and the scenarios' seeded deposit
  (`segments_support::seed_arrival`) cites the reserve's own arrival notice, not its result.
- **Defaults** (reversible): folded into 3.1.0; standalone shapes; the notice's `transaction` as the per-credit
  reference; the question's session required, and no order named; three outcomes for an answer, a refusal by the
  balance-refusal classes; a constant label on the record, a transaction per segment, no arithmetic rule.
- **Hand-offs:**
  - `T-020-19`: core-orch's `NOT_A_NOTICE` deny list admits a hold and an arrival answer (each the reserve's, naming
    a body `transaction`); replace it with `ARRIVAL_NOTICE_SHAPE` and read the body as `ArrivalNotice`. The log now
    refuses anything else, so the gap is closed fail-closed until then. Append each notice an answer carries as its
    own stated entry, about its credit's transaction, and call `check_answer` and `check_one_credit`.
  - `T-020-21`: `answer-follows-its-question` keys on `result` and `operation` only; extend it to `arrival-answer`
    and `arrival-question`.
  - `T-024-14`: the simulated publishers emit `InnerSegments`, `Books` reads it, and `Replay` carries the receipts of
    the inner-segment records it read (`REQ-AUD-9`), so a checker finds what the replay compared.
  - `F-024`: the test `stand in reserve signs an arrival notice that passes the role suite` [`REQ-ORD-14`,
    `REQ-ORD-23`] is not written (`conformance/src/role/stand_in.rs`, `suite.rs`). `T-020-18` can't close until it
    lands.
  - `F-021`: the question's session, open on the holder's behalf where the order's has ended (`T-020-04`'s
    hand-off).
- **Rule off**, each once, its case red: every required notice field; `check_one_credit`'s figures and maker; the
  question's transaction and session; the answer's transaction; the question's operator key; the answer's nested
  shapes and context; each of `check_answer`'s comparisons, the unit included; the record's amounts in and out, fee
  and label; `check_inner_segments`' advertisement, publisher and node; `arrival-names-its-notice`'s shape.

## T-020-19

Branch `f-020/t-020-19-arrival-notice-link`, from `m3` at `d4006ba4`. Edits `components/core-orch/src/arrival/`, its
test (`tests/arrival.rs`) and, as the compiler forced, three F-024 test-support files of the scenarios
(`assembly_arrivals.rs`, `execution_support/mod.rs`, `segments_support/mod.rs`, which the commit body names).

- **`Report` is the reserve and its signed notice** (`arrival::Report { reserve, notice }`). The port's figures are
  gone: node, asset, amount, account and holder are the notice's, which Core reads from the signed bytes and uses for
  matching, the question, the `arrival` entry and a filing's checks (`read_notice`, `Credit`). `NOT_A_NOTICE` and
  `NoticeEnvelope` are deleted.
- **The shape is required positively.** `read_notice` reads the bytes as `Statement<ArrivalNotice>` (the envelope and
  the body both `deny_unknown_fields`), calls `check_shape` (the notice's `$id`), requires the maker to be the reserve
  the report names, the transaction, account and holder to be identifiers and the amount a positive decimal integer.
  Anything else is `NotItsNotice` before anything is kept or appended; the read-back refusal (next bullet) comes after the keep, so there the bytes may be kept and nothing is appended. The probe list (`signed hold or arrival answer
  is not taken for an arrival notice`): a hold, an arrival answer, an arrival question; a hold's and an answer's body
  under the notice's shape; the notice's body under a result's and a hold's shape; a body without its holder, with an
  extra field, with an amount of zero, a negative one, a fraction or a JSON number, or with an empty transaction;
  another reserve's notice; bytes that are no statement. Each with and without an order open, each leaving the log and
  the evidence store as they were.
- **Read back.** After the evidence store keeps the bytes, the log's own read (`Desk::kept`) must give behind the
  commitment the statement Core read its figures from, or the notice is refused and nothing is appended
  (`kept_as_read`; semantic equality, so a pretty-printed copy is the same statement).
- **One credit, one arrival** (`check_one_credit`). A deposit is known by `(reserve, the notice's transaction)`: two
  notices of one credit, signed at other times or in other bytes, are the first arrival's outcome again, kept and
  appended once (the second notice is not appended: a default); the same reference with another node, asset, unit,
  amount, account or holder is `OtherFigures`, nothing kept. Two references are two deposits, each citing its own
  notice (the contract's own two-claims reading, `T-020-18`).
- **A notice the log refused holds no reference** (a default, found by the test below). The log cannot be asked
  whether it attributed a notice, so an arrival whose notice it could not attribute is refused at the arrival's append
  (`arrival-names-its-notice`, `arrival-rests-on-the-reserves-report`). Kept as it was, that refusal made the deposit
  known to the forged notice: the reserve's genuine notice of the same credit, sent after it, read as the same arrival
  and was refused for ever. Now a refused notice or arrival append forgets the notice, so the genuine one is taken
  anew. Nothing was ever recorded as an arrival.
- **A parked notice is replaceable** (review of `976f9fef`). A forged notice parked by a silent reserve (`Waiting`,
  no outcome) wedged the genuine notice of the credit when their figures differed (`OtherFigures` on every try). Now a
  later notice of the same reserve and reference with other figures replaces one with no outcome and no recorded
  arrival, and the log's attribution decides which survives; an arrival recorded under the reference stands.
  Test: `a_parked_forged_notice_does_not_wedge_the_genuine_one`.
- **`arrival.earlier`** (`T-020-21` item 6): a filing's arrival, which holds the deposit against the order the filing
  opened, names the arrival that met no order (`filing.rs`). The arrivals `arrive` writes name none: a deposit's first
  arrival has no earlier, and the notice outcome is kept, so `arrive` never writes a second.
- **The seeded deposit** (`segments_support::seed_arrival`): the notice's transaction is the reserve's own per-credit
  reference (`credit-<order>[-<part>]`), no longer the `tx-deposit-<order>` Core's question and the reserve's report
  go under. No simulated reserve issues a reference yet, so the seed gives each deposit its own (`T-024-14`).
- **Owner ruling, 2026-11-16** (automatic-deposit providers set the destination from the customer's configured
  parameters; the operator never chooses it). The notice carries no destination and `Report` names none, so no step of
  this row lets a notice or a port set one. `Filing::destination` is still the filer's, the operator included, held to
  resolve to the holder (`T-020-04`'s answer 6); that is *not* the ruling and is a gap (below).
- **Defaults** (reversible, each failing closed): the second notice of a credit is not appended; the question to the
  reserve stays a balance verification (the arrival question and answer are not used, below); a refused notice or
  arrival append forgets the notice; `ArrivalError::NotItsNotice` covers every refusal of the bytes, so no variant is
  added; the maker is the report's reserve exactly (the id the catalogue knows the reserve by).
- **Histories the log refused, sorted.** None new: the existing tests' notices carried a body of their own
  (`kind`, `transaction`, `amount`), refused by the positive check, so their helpers now sign the contract's body; each
  test function reads as before. The scenarios' three helpers built a `Report` with figures and now build the notice.
  A first draft of the unattributed test failed on a real Core fault, the sticky forged notice above.
- **Gaps, with owners.**
  1. *Core can't read the log's attribution of the notice it appended*, so with an order open the reserve is still
     sent Core's balance question before the log refuses the arrival of an unattributed notice. Nothing moves, no
     arrival or order is recorded, and the question is no operation; but it is a question sent on an unverified word.
     Closing it needs the log's recorded entry by receipt on `Desk` (`quote::Recorded` is the shape), which is
     `intake/`'s, edited by `T-020-24`. Owner: the lead, a row after `T-020-24`; `Arrivals` would read the notice's
     entry back and ask only on `Attribution::Verified`.
  2. *The destination.* The ruling says the operator never chooses it; `Arrivals::file` takes the destination in the
     `Filing` from either filer and checks only that it resolves to the holder. Enforcing the ruling needs the
     customer's configured parameters at the reserve, which no port in Core's reach answers (`Destinations::resolve`
     answers an owner, not a configured destination). Owner: the lead, a row naming the port (an `intake` or
     `arrival` seam, and `F-021` if policy holds the parameters). Until then the filing is as `T-020-04` left it.
  3. *The question and its answer are still a balance verification* (`confirm.rs`); `T-020-18`'s hand-off to append
     each notice an answer carries as its own stated entry and call `check_answer` goes with moving the question to
     `arrival-question` and `arrival-answer`, which needs the catalogue's balance-check gate (`REQ-CAT-4`) to treat the
     new question as a check. Owner: the lead, a row of `F-020` with `F-012`.
  4. *No simulated reserve emits an arrival notice* (`T-020-18`'s `F-024` hand-off; `T-024-14`); the scenarios sign
     one with the reserve's statements key.
- **Rule-off table** (each switched off alone, `cargo test -p rutent-core-orch --test arrival`, 150 s alarm, tree
  restored; none hung):

  | Rule switched off | Tests that went red |
  |---|---|
  | the notice's shape check (`check_shape`) | `signed_hold_or_arrival_answer_is_not_taken_for_an_arrival_notice` |
  | the maker is the report's reserve | the same |
  | the holder read from the notice (the reserve's id in its place) | 32 tests, `arrivals_figures_are_read_from_the_reserves_signed_notice` among them |
  | the account read from the notice | `arrivals_figures_are_read_from_the_reserves_signed_notice` |
  | the amount read from the notice (a constant) | six, `arrivals_figures_are_read_from_the_reserves_signed_notice` and `signed_hold_or_arrival_answer_is_not_taken_for_an_arrival_notice` among them |
  | the node read from the notice | `arrivals_figures_are_read_from_the_reserves_signed_notice` |
  | the asset read from the notice | the same |
  | `check_one_credit` (every restatement one credit) | `two_notices_of_one_credit_are_one_arrival`, `one_notice_is_one_arrival_whatever_figures_it_comes_with`, `a_restated_notice_is_the_same_arrival`, `a_deposit_is_known_by_its_transaction_and_every_arrival_names_its_notice` |
  | a deposit known by its transaction (never matched) | six, `two_notices_of_one_credit_are_one_arrival` among them |
  | the read back of the kept statement | `notice_the_store_keeps_nothing_behind_is_refused` |
  | a refused notice forgets its reference | `notice_the_log_could_not_attribute_to_its_reserve_creates_no_arrival` |
  | a parked notice replaceable by other figures | `a_parked_forged_notice_does_not_wedge_the_genuine_one` |
  | `earlier` set by the filing | `deposit_filed_again_against_another_order_names_its_earlier_arrival` |
  | the log's `verified` on `arrival-names-its-notice` (contracts, restored) | `notice_the_log_could_not_attribute_to_its_reserve_creates_no_arrival` |
  | that and the report's `verified` in `arrival-rests-on-the-reserves-report` | the same and `unattributed_arrival_still_creates_no_order` |

  Two tests were green before the code and are held by their rule-offs alone: `two notices of one credit are one
  arrival` (the key was already the transaction) and `unattributed arrival still creates no order` (the log already
  refused). `arrival's figures are read from the reserve's signed notice`, `deposit filed again against another
  order names its earlier arrival` and `signed hold or arrival answer is not taken for an arrival notice` failed on an
  assertion first; so did the sticky-notice case of `notice the log could not attribute to its reserve creates no
  arrival`.

## Rows added for the gaps the reviews found (`plan/m3-owed-rows`)

Each gap below named "a new `F-020` row" or "no row owns it" now has one. Where a gap sits in another feature's
module, the row is that feature's.

| Gap, and the note it is in | Row |
|---|---|
| Core's segments don't call `Charges::check`; no operator channel; `Terms` not fed from signed terms (`T-020-07`, round 3 integration; `F-024`'s `T-024-03`) | `T-020-14` |
| Signed holds not redeemed in intake; statement vs commitment; the session; `redeemed_by` (`T-020-01`, round 3 integration) | `T-020-15` |
| Production `Resolutions` for a `deliver` (`T-020-11`'s gap) | `T-020-16` |
| Production `Resolutions` for a hand-on (`T-020-13`) | `T-020-17`, after `T-020-16` |
| Provider-role statement of what arrived, a question of what arrived (`T-020-04`); the publisher's inner-segment record (`T-020-09`, `F-015`) | `T-020-18`; consumer of the record `T-024-14` |
| The arrival linked to that notice, and to the earlier arrival it holds against an order (`T-020-04`) | `T-020-19` |
| A wrong stuck report can't be corrected; a decline then a re-report; a segment's move confirmed done not taken (`T-020-11`, `stuck/mod.rs` Gaps) | `T-020-20` |
| A mismatch at a non-final public end isn't recorded; the final deposit's settlement not held by the log (`T-020-09`) | `T-020-22` |
| Next `core-internal` round: notice summary entry, `Operation::stated_out` doc, the mismatch record, checkpoint and opening shapes, a decline, an earlier-arrival field, an empty shortlist (`T-020-06`), a `HandedOver` item (`T-020-08`), the holding account's withdrawal record, the final-deposit relation, a late-status action and a confirming party at a later inner node (`T-020-12`) | `T-020-21`; writers `T-020-25` (shortlist, `HandedOver`), `T-020-26` (holding account record), `T-020-22`, `T-020-27` (late status, later inner node) |
| No writer of `reliance` entries, reserve half (`F-021`'s `T-021-07` notes) | `T-020-23`; the verifier half at the assembly is `T-024-12`; the production verifier stays `F-053`'s |
| Carry-on refused as `OutsideAuthorisation`; `StatusError::Refused`; `AcceptFault` read from the log | `T-020-24` |

The evidence floor as a logged catalogue change has no row: it stays on the owner's form. The stuck rows (`T-020-16`,
`T-020-17`, `T-020-20`) edit `stuck/` in turn across waves; `T-020-20`, `T-020-25` and `T-020-27` share wave 26 and
edit `stuck/`, `segments/` and `reversal/`, so the lead runs them in that order.

## T-020-14

Branch `f-020/t-020-14-charge-check`, from `m3` at `34c700e`.

- **Core's segments make the charge check.** `Segments::run_part` holds each settled segment's charge to the formula
  that bound it, in `segments/charges.rs`, and raises one above it to the operator.
  - *When a segment settles*, `account` keeps what the check needs (`Segments::note_charge`) and checks at once
    (`check_charge`), before the next segment of the part is handed over, so the operator is warned while the part
    is still running. The log shows it: the first segment's `raise` precedes the second segment's `move_value`.
  - *On every run*, `run_part` checks the part's settled segments again (`check_charges`, also after an error in the
    run). A check or a raise the log couldn't take is made when it can, and one made is not made twice
    (`Charges::check` keeps each segment's result, amount and charge, and its built entries until the log takes them).
  - The check is made *whatever `billed_separately` says*: the field is never read. It is the charge the formula
    bounds, not how it is billed (`F-020`'s reading, kept).
- **What the check is given** (`Segments::charged`, the accessor `T-020-07`'s notes asked for):
  - the **charge**: the result's `fee.amount`, which `judge` now carries in `Counted::Succeeded.fee`. A fee stated in
    another asset than the one the segment takes in is held to no formula of its tuple and is not noted (gap below);
  - **`amount_in`**: the result's own `amount_in`, charge included. The assembly's stand-in summed `amount_out` and
    the charge, which holds under `net` reporting only;
  - **`started_at`**: the `effect` step's entry time, the segment's first `move_value` effect, which is where
    `charge-raise-rests-on-terms-in-force` counts a segment's start (not the carry-out time `flight.started` keeps);
  - **`result`**: the receipt of the result's stated entry;
  - **`advertised`**: the fee formula of the price the operation cites (`sign::Cited.price`), never the assembly's
    own price list. An operation that cites no price is a `Plan` error: a chosen candidate always cites one.
- **The operator's channel.** `segments::Parts` gains `channel: &mut dyn Operator`. The assembly hands it the notices'
  own `notice::Channel` (`T-023-02`): the raise is sealed for the operator, signed with a notice key of the
  assembly's own, and sent on a transport that takes everything. The tests take the sealed notices through the
  reference receiver (`Inbox::take`, a real Ed25519 verification) and read the raises from its events, so "the
  channel carries the raise" is checked from the receiver's side. A channel that delivers nothing (`notice::Unbuilt`)
  leaves each raise recorded `failed`, never unmade. The `Charged`/`Charges`/`Terms` registers are `Segments`' own,
  kept as long as it is (`F-030` keeps them with it).
- **Terms from a provider's signed bytes** (`Terms::take`, `Segments::take_terms`). The call keeps the bytes in the
  evidence store, appends them as a stated entry about no order (the log attributes it), reads the entry back as the
  log recorded it (`quote::Recorded`), fetches the statement kept behind its commitment (`KeptStatements`, the log's
  own read, no policy read) and compares it with the statement the bytes read as, and only then calls
  `Terms::state`, which checks the entry's shape, attribution, maker and times. A mismatch, or nothing kept, is
  `NotTerms::NotWhatIsKept`. `Terms::state` stays public for callers that hold the kept bytes themselves; Core's own
  path goes through `take`. **Nothing delivers a provider's terms to `take` yet**: no module hands them to Core, so
  the API stops there. In the assembly, `Core::state_terms` is how a scenario hands them over, signed with the
  provider's statements key (`Core::statements_signing_key`).
- **A charge is held to the terms stated before its segment started** (`REQ-QTE-6`): `Terms::in_force` over the
  segment's `started_at`, the router's `binding`. Terms stated later bind the segments after them.
- **Restart** (`Charges::recorded`, `Segments::recorded_charges`). A register rebuilt from the log's entries knows the
  charges the log holds a raise of, by order and segment with the result and charge they name; such a segment is
  `Checked::AlreadyRaised`, never raised again, and a different result or charge for it is refused
  (`AnotherCharge`). The assembly's `Core::charges_rebuilt_from_log` loses the register and rebuilds it, with the
  segments kept as a durable `Segments` would be, and a rerun re-checks every settled segment. A reopened assembly
  starts `Segments` empty (`F-030`), so nothing there runs an order again.
- **The assembly's stand-in is deleted** (`Core::check_charges`, `charges::Channel`, the `charges` and `terms`
  fields). This is the row's `F-024` exception: `conformance/scenarios/src/assembly/{charges,mod}.rs` and
  `conformance/scenarios/tests/quoting.rs` (`T-024-03`'s `charge_above_the_formulas_maximum_is_flagged`, which now
  calls `run` and reads Core's own checks). The assembly gains `Notices`, a notice key, a transport, the log's
  `KeptStatements` read and the readers a scenario needs (`raised`, `operator_notices`, `charge_checked`,
  `charged_segments`, `state_terms`, `route_operations`).
- **Files outside the row's directories, for the same reason**: `Parts` gains a field, so every literal that builds a
  `segments::Parts` gains `channel: &mut notice::Unbuilt`, in `core-orch/tests/{segments,reversal}.rs` and
  `scenarios/tests/{hand_on,recalc}.rs` and `segments_support/{mod,rerouting}.rs`. A compile-only line each.
  `quote/journal.rs` gains `built_stated`; `segments/journal.rs`'s `stated_entry` takes an optional `About`, and
  `segments::maker` and `segments::journal` are `pub(crate)`, so `Terms::take` builds the stated entry as the
  segments do.
- **Defaults taken, reversible, failing closed:**
  - a segment that settles is checked even when the operator's channel delivers nothing (the raise is then recorded
    `failed`);
  - a charge the log can't record stops the part with `SegmentsError::Append`, to be checked on the next run;
  - an unreadable or unattributed terms statement is not kept, and a charge is then held to the price;
  - `Terms::take` reads the kept bytes through `KeptStatements`, not `Evidence::fetch`, so it writes no read entry and
    asks no policy: the bytes never leave Core's audit part. The commitment itself is compared as the store
    answers it; Core doesn't recompute the salted hash (no hash in `core-orch`), which is the log's job when it
    attributes the entry.
- **Gaps, with owners:**
  1. **No module delivers a provider's terms** (`F-013`/`F-030`, whoever wires a module's statements to Core): `take`
     has no production caller.
  2. **A raise delivered but not recorded before a stop is raised again** (`F-030`): `Charges::recorded` reads the
     log, and a raise the transport took but whose entry the log never took is not in it. The operator's inbox holds
     a raise once by its id, so it shows once; the log may then hold a second `raise` for the segment's charge.
  3. ~~A fee in another asset is skipped.~~ Closed after review: a charge in another asset is noted with the asset it
     is stated in (`Charged::charged_in`) and raised by `Charges::check` as a `charge` difference with a maximum of
     nothing, in that asset, resting on the result alone; nothing charged raises nothing; an asset the operation names
     at neither end stops the part (`Plan`). The existing raise and difference shapes carry it, so no contract change.
     `check_amounts` already refuses such a result before it counts, so only the tests reach it
     (`a_charge_in_another_asset_is_raised_and_never_passed`).
  4. **`billed_separately`** has no scenario: no simulated provider states it. The rule is that nothing reads it, and
     nothing does.
  5. **`amount_in` for a gross charge** is the result's, not summed (`T-024-13`): the row has no gross scenario until
     the stand-in reserve gains one.
- **Tests** (`conformance/scenarios/tests/charge_check.rs`, over the assembled Core and the simulated providers; each
  was seen red on its assertions with the check stubbed off, before the rule-offs below):
  `charge_above_the_formulas_maximum_is_raised_to_the_operator_by_cores_own_segments` (`AC-14`, `REQ-QTE-5`),
  `charge_within_the_formulas_maximum_raises_nothing` (`REQ-QTE-5`),
  `terms_stated_after_a_segment_started_do_not_bind_it` (`REQ-QTE-6`),
  `terms_whose_kept_statement_does_not_match_the_entrys_commitment_are_not_kept` (`REQ-QTE-6`, `REQ-AUD-2`),
  `each_segments_charge_is_checked_once_also_after_a_restart` (`REQ-QTE-5`),
  `the_assembly_no_longer_checks_charges_itself` (`AC-14`). The last reads the assembly's source for the stand-in's
  names. `T-024-03`'s `charge_above_the_formulas_maximum_is_flagged` runs on Core's own call.
- **Rule-off, each red** (each rule switched off alone, `charge_check` and `quoting` run, the file restored and
  touched; the baseline is green, and no run hung):

  | Rule switched off | Red |
  |---|---|
  | the charge is checked as soon as its segment settles | `charge_above_the_formulas_maximum_is_raised_to_the_operator_by_cores_own_segments` (the raise comes after the next segment's effect) |
  | a charge is held to the terms in force at its segment's first effect | `terms_stated_after_a_segment_started_do_not_bind_it` |
  | a register rebuilt from the log knows what was raised | `each_segments_charge_is_checked_once_also_after_a_restart` |
  | the statement kept behind the commitment is compared with the one handed in | `terms_whose_kept_statement_does_not_match_the_entrys_commitment_are_not_kept` |
  | the raise goes through the notices' own channel (`Unbuilt` instead) | all five behavioural tests of `charge_check`: nothing reaches the operator's inbox |
  | the assembly holds no check of its own (a stand-in name put back) | `the_assembly_no_longer_checks_charges_itself` |

  After review, three more tests (`charge_check.rs`) hold what was defence in depth, each red with its rule off:

  | Rule switched off | Red |
  |---|---|
  | every run re-checks the settled segments | `a_raise_the_log_could_not_take_is_recorded_by_the_same_run_before_the_next_segment`, `a_raise_retried_by_a_later_run_is_made_once_and_before_the_next_segment` |
  | the settling's own check, before the next segment | the same two, and `charge_above_the_formulas_maximum_is_raised_to_the_operator_by_cores_own_segments` |
  | the commitment equality in `Terms::take` | `terms_whose_kept_statement_names_another_commitment_are_not_kept` |
  | a charge in another asset held to no formula | `a_charge_in_another_asset_is_raised_and_never_passed` (`reversal.rs`) |

  The retry order was right already: the settling's check comes before `account` returns the segment as settled, so
  a retried run records a refused raise before it hands the next segment over, and `Charges` keeps the raise made so
  the operator is warned once. The log's refusals are planted on the assembly's store (`Disk::refuse`), since
  `segments_support`'s test log routes one reserve that can't charge above its price. Still without a test of its own:
  the refusal of a store that keeps nothing (the `Nothing` read is refused by `ok_or`, which no single switch-off
  isolates from the compare).

## T-020-15: signed holds wired into intake

- **Design.** An acceptance may name the hold of the quote it accepts (`Intake::accept_holding`; `accept` is
  `accept_holding(.., None)`). Intake redeems it through `Holds::redeem`, once the destination checks pass and just
  before the acceptance is built, and cites the receipt of the hold's stated entry as `What::Acceptance.held`. `redeem`
  now takes a `Redemption` (every field intake verified: the request's origin reserve, the order, the session the
  reserve's yes was asked in (`offered.session`, the same value the acceptance records), the quote's decision record
  and candidate, Core's clock) and the log's read of the evidence store (`KeptStatements`). It refuses, in order: no
  such hold; another reserve than the holder; another order or session (`check_redemption`); another order having
  redeemed it; a hold of another quote (`NotThisQuote`); a kept statement that is not the hold (`NotWhatIsKept`:
  nothing kept, SHA-256 of salt and bytes not equal to the entry's commitment, or bytes that don't read as the signed
  hold Core holds); past validity. Nothing is redeemed unless every check passes.
- **`redeemed_by` rebuilt.** It moved out of `Held` into `Holds.redeemed` (hold transaction to order), and
  `Holds::recorded(history)` rebuilds it from the log's acceptances (an acceptance's `held` receipt names a stated entry
  whose `about.transaction` is the hold), as `Closing::recorded` does. A hold kept again after a restart meets the
  order that redeemed it, so the log's `hold-is-redeemed-by-one-order` is not what refuses the second order.
- **Also added.** `Holds::take` (the hold's own taking, as `Terms::take`: keep, append as a stated entry about the
  transaction, read back, `hold`); the assembly's `Core::state_hold`, `accept_holding`, `holds_rebuilt_from_log`, and
  the Desk's `holds` and `kept` fields (`conformance/scenarios/src/assembly/`, an F-024 file, edited for this row as
  the plan says).
- **Defaults (reversible, fail closed).**
  1. The acceptance **names** the hold (a transaction id); intake never picks one by matching the quote.
  2. A hold that isn't redeemed refuses the acceptance (`IntakeError::Hold`), the order stays quoted; it is never
     accepted without the hold. The destination's check, resolved first, is already in the log by then.
  3. A hold is redeemed only for the quote it holds (same decision record id and candidate). So two orders can
     only meet on one hold if the second names a hold of the first's quote: it is refused as `RedeemedByAnother`,
     checked before `NotThisQuote` so that the one-order rule is seen on its own.
  4. The commitment is recomputed (salt then bytes, SHA-256), stronger than `Terms::take`'s equality.
  5. A redemption is not released if the acceptance is then refused by the log: the hold stays bound to that order
     (rebuilt state after a restart would show it unbound).
- **Gaps.**
  1. No module delivers a reserve's signed hold: `Holds::take` has no production caller (`F-013`/`F-030`).
  2. The held quotes themselves are in memory; keeping them across a restart is `F-030`'s (a reading: only
     `redeemed_by` is rebuilt here).
  3. A session hold made before an order exists can't be redeemed through intake: an order's quote is a record of its
     own routing, and a hold of another record is `NotThisQuote`. Owner: a new `F-020` row if the product wants
     pre-order holds (needs a way to route on a held record).
  4. `intake_support`'s evidence store now makes real salted commitments (so a kept hold can be held to one).
- **Tests.** `components/core-orch/tests/intake_holds.rs` (six named plus two more: no hold named, and another quote's
  or no hold) and `conformance/scenarios/tests/holds.rs` (the assembled Core's acceptance names the receipt; one order
  only, also after `holds_rebuilt_from_log`). `tests/quote.rs` redeem calls go through the new signature.
- **Rule-off, each red** (`intake_holds` and `quote` run; a rule switched off alone, file restored):

  | Rule switched off | Red |
  |---|---|
  | SHA-256 of salt and bytes equals the commitment | `hold_whose_kept_statement_does_not_match_its_entrys_commitment_is_not_redeemed` |
  | kept bytes read as the signed hold Core holds | the same |
  | one order per hold | `a_hold_is_redeemed_by_one_order_only`, `redemption_is_rebuilt_from_the_log_when_core_is_reopened` |
  | hold is for this order or session | `hold_redeemed_in_another_session_than_the_one_it_was_made_for_is_refused` |
  | hold is of the quote accepted | `a_hold_of_another_quote_or_none_kept_is_not_redeemed` |
  | session is the one intake verified (none passed) | four, among them the session test |
  | validity (Core's clock replaced by an old time) | six (crude: it also breaks every redemption) |
  | `Holds::recorded` reads nothing | `redemption_is_rebuilt_from_the_log_when_core_is_reopened` |
  | the acceptance cites the receipt | three, among them `accepting_a_quote_held_by_a_reserve_redeems_the_hold_into_the_acceptance` |

  Not isolated by a single switch-off: the store keeping nothing (`ok_or`, covered by the forgetting store in the kept
  test but the same refusal as the others), and "an acceptance naming no hold redeems none" (`None => None`; the test
  asserts the outcome, no mutation of it was tried).

## T-020-16

Branch `f-020/t-020-16-stuck-mover`, from `m3` at `20ed319`. Edits `components/core-orch/src/stuck/` and `segments/`
(visibility seams only), `conformance/scenarios/` (the kit, the assembly, two test files); no contract, manifest or
`DEC-20` path.

- **The production mover** (`stuck/mover.rs`: `Mover`, `Ports`, `Delivering`). A `Resolutions` for a `deliver`. It
  takes everything from the effect's `Action::ResolveStuck` (provider `by`, destination, asset, amount, the decide
  step's transaction), never from the caller. The inner node the value rests at is read from Core's record of the
  operation it signed for the part's last settled segment at that provider (`Segments::sent`). The operation is a
  `deposit` from that node to the destination's outer node, the destination's account under the one input the
  provider's advertised deposit tuple requires, signed with the operator key. Order of steps, each once: the
  operator's question (stated entry about the order and transaction, no segment) is appended; the platform begins the
  provider's segment of that transaction; the operation is handed over once; events, uses and the segment's record are
  kept and appended as hand-over entries; the answer is appended as the provider's stated entry; the effect's result
  follows (written by `Stuck`).
- **The trait is fallible and resumable.** `Resolutions::resolve(parts, carrying, effect) -> Result<Resolved,
  AppendFailure>`; an `Err` means the log couldn't take a step and the same call is made again. `Stuck` marks the
  effect acted on once (`InMotion::acted`) and never drops an acted move; the mover keeps its own per-effect flight, so
  a retry never hands the provider the operation again (`INV-8`). Stated steps use `Step::append_stated`: the log
  attributes a statement, so its receipt is of the recorded entry and the receipt rule can't hold it (the kit's log
  hid this; the assembly's real log showed it).
- **Outcome mapping, defaults (reversible, fail closed; reviewed).** `Done` only for an answer that counts under
  `judge` and says settled. `Failed` only for what Core itself saw: nothing passed out (a platform refusal on the list
  that proves it: `Unavailable`, `NotHandled`, `VersionExploited`, the statement-key refusals, and the output refusals
  that apply only where the module passed nothing out), a begin refused so, the question refused by the log or
  recorded unattributed, no resting node, no advertised deposit tuple, a unit differing from the tuple's, more than one
  required input, a hand-on. `Unknown` for everything else, so a part is never freed on a word that may hide a
  movement: the platform's `OutcomeUnknown`, `DuplicateTransaction` or any refusal not on the list, the provider's own
  unknown, waits-on-person, **and the provider's own "failed" or final refusal**. A provider's failure frees the part
  only through `known_outcome` and `confirm_outcome` by another party (the owner's ruling of 2026-11-14): a provider
  that paid could otherwise say "failed" and be paid twice.
- **Re-entry** (`Carrying::again`, `Resolutions::may_have_handed_over`). `Stuck` marks an effect acted on at the first
  call and tells every later call it is a re-entry. A mover holding no record of an effect acted on (a restart) answers
  `Unknown` and hands over nothing. A mover that knows it handed nothing over lets `Stuck` read the part's ends again
  (the customer may have replaced them), dropping the move as before if they changed. The question must be recorded
  attributed (its receipt isn't the digest of the entry as built) before anything is handed over.
- **Known outcome** (`Delivering::statement`). A provider's later statement for the move's transaction is checked
  (attributable under a published statements key, a result, naming the transaction; the move's own provider's judged
  against the operation Core signed), kept once as a stated entry about the order and transaction, and returned with
  the outcome it states (`Stated`). A reporter names it in `Stuck::known_outcome`, a confirmer another maker's in
  `confirm_outcome`. Nothing is handed to the platform.
- **Assembly** (`assembly/mod.rs`): `Core::leave_no_route`, `decide_stuck`, `move_stuck`, `Faults::refuse_begin`.
  Policy is the real engine (`PolicySide`).
- **Tests.** `conformance/scenarios/tests/stuck_mover.rs`: `decided_delivery_to_the_destination_is_carried_out_on_the_platform`,
  `decided_delivery_back_to_the_origin_account_is_carried_out_on_the_platform`,
  `resolution_is_carried_out_only_on_a_move_policy_permitted`,
  `move_left_unknown_takes_its_known_outcome_from_the_providers_stated_result`,
  `resolution_acted_on_once_is_not_acted_on_again_after_a_retried_append`,
  `failed_move_leaves_the_value_held_and_the_part_stuck`; after review:
  `provider_stated_failure_does_not_free_the_part_until_another_party_confirms_it`,
  `move_taken_on_by_a_mover_with_no_memory_of_it_is_never_handed_over_again`,
  `platform_refusals_that_prove_nothing_passed_out_are_the_only_failures`,
  `move_the_provider_cant_be_asked_fails_before_anything_is_handed_over`,
  `stalled_move_before_any_hand_over_is_dropped_when_the_customer_replaced_the_destination`, and two unit tests in
  `stuck/mover.rs`; each of the scenario ones ends by replaying the log through `admit_entry`.
  `tests/assembly_stuck.rs`: `stuck_order_of_the_assembled_core_is_resolved_through_the_mover_and_the_real_engine`
  (the assembled Core, the real engine and log: `same_actor` by chain, no permission, and the provider's own desk each
  refused with the reserve handed nothing; the permitted move done once). All were seen red with the mover stubbed to
  `Failed`.
- **Rule-off** (each alone, both files run, file restored; none hung):

  | Rule switched off | Red |
  |---|---|
  | mover stubbed (`Failed`) | all seven |
  | question appended before the hand-over | all seven (the log's `answer-follows-its-question`) |
  | unknown answers kept `Unknown` | `move_left_unknown_takes_its_known_outcome_from_the_providers_stated_result` |
  | effect marked acted on once | `resolution_acted_on_once_is_not_acted_on_again_after_a_retried_append` |
  | a refused begin fails the move | `failed_move_leaves_the_value_held_and_the_part_stuck` |
  | the mover's `carried` flag | none red: defence in depth, the kept verdict already stops a second hand-over |
  | a provider's own failure answers `Failed` (review B1) | `provider_stated_failure_does_not_free_the_part_until_another_party_confirms_it` |
  | any platform refusal read as nothing passed out (B2) | `platform_refusals_that_prove_nothing_passed_out_are_the_only_failures` |
  | a mover with no record of an acted-on effect carries it out (B2; the platform forgets its begins too) | `move_taken_on_by_a_mover_with_no_memory_of_it_is_never_handed_over_again` |
  | the platform's `OutcomeUnknown` answers `Failed` (S1) | `platform_refusals_that_prove_nothing_passed_out_are_the_only_failures` |
  | the unit check against the tuple (S1) | `move_the_provider_cant_be_asked_fails_before_anything_is_handed_over` |
  | more than one required input (S1) | `only_a_tuple_whose_inputs_the_account_can_fill_is_asked` (unit test, `cargo test -p rutent-core-orch --lib`) |
  | the question's receipt must be an attributed one (S2) | `move_the_provider_cant_be_asked_fails_before_anything_is_handed_over` |
  | the ends re-read when nothing was handed over (S3) | `stalled_move_before_any_hand_over_is_dropped_when_the_customer_replaced_the_destination` |

  The kit's log gained `attribute_from_now` and `stop_attributing`: from then on its receipts for stated entries are of
  the entry as the real log records it, attributed, which is how the question's check is seen here. The refusal
  fallback arm of `Answered::Refused` (a refusal not on the list, on the hand-over) has no test of its own.

- **Gaps, with owners.**
  1. `What::Evidence` requires a segment, so `stuck_resolved`'s evidence stays an id of Core's with no entry behind it
     (a contract shape; owner `T-020-21`).
  2. The mover's state is in memory, as `Stuck`'s and `Segments`' are (`F-030`); after a restart a fresh `Stuck`
     declares nothing and decides nothing, and the log's `part-waits-for-value-in-motion` holds an open effect.
  3. The provider module's own calls are named for no route (`REQ-MDL-21`): a module that needs one would be refused,
     failing closed; none of the simulated providers does.
  4. A hand-on is refused by this mover (`Failed`); `T-020-17`.
  5. `Stuck::known_outcome` doesn't itself check that the evidence it is given states the outcome reported; the check
     is in `Delivering::statement`, and the assembly passes its result. Binding the two is `T-020-20`'s.
  6. The confirmer's evidence must be another maker's statement: this row takes it through `statement` with
     `outcome: None`; a second provider's status path is not built.

## T-020-17

Branch `f-020/t-020-17-hand-on`, from `m3` at `b77fa932`. Edits `components/core-orch/src/stuck/` (`mover.rs`, the new
`handing.rs`), one `pub(crate)` seam in `hand_on/mod.rs` (`resting`), and the kit (`segments_support/resolving.rs`,
the new `tests/stuck_hand_on.rs`). No contract, manifest or `DEC-20` path.

- **The arm** (`Delivering::resolve_hand_on`). `resolve` dispatches on the resolution; a `HandOn` goes to the
  withdrawal, a `Deliver` as before. Policy is asked nothing more. Steps, each once: (1) the route is held to Core's
  own records before the wallet is asked anything: the linked order's own accepted route (`LinkedRoutes::accepted`),
  one part, starting at the inner node where `hand_on::resting` says the part's value rests, in its asset, with no
  withdrawal first, of an order Core doesn't follow, the node's holding account the provider the decision names,
  all of what the part holds; (2) the operator's signed withdrawal statement is shown to the mover before the wallet
  is handed it (`Withdrawals::withdraw`'s `asked`), checked against what was decided, and recorded as the operator's
  question about the linked order and the wallet's transaction, attributed; (3) the wallet is asked once; (4) what
  the platform and proxy handed over is kept and appended; (5) the answer.
- **Answer.** `Done` only for the holding account's own record, held by `proves_start` to the decided provider, node,
  asset, order and amount and to the route's first segment. `Failed` only where Core saw nothing pass out: the route
  refused, the question not recorded attributed, the statement not the one decided, a platform refusal on
  `nothing_passed_out`. `Unknown` for everything else once the wallet was handed the statement: the wallet's own
  failure, `DuplicateTransaction`, `OutcomeUnknown`, a wrong result, no record, a record proving nothing of this start.
  A re-entry with no record of the effect answers `Unknown` and hands over nothing.
- **Ports.** `Withdrawals` (holder of a node; withdraw with the show-first rule), `LinkedRoutes`, `HandingOn`,
  `Delivering::handing_on`; without them a hand-on fails. `Wallets` is `Withdrawals` over `Holdings` and the platform:
  a statement the mover refuses is replaced by a reversal, which no plain wallet handles, so the platform refuses it
  as not handled before running anything. `OperatorSigns` is the operator key as the wallet's `Operator`.
  `Mover::withdrawn(effect)` gives, once done, the route, the record and the question's receipt: the hand-on takes
  its plan from it and `prove_start` its record.
- **Tests** (`tests/stuck_hand_on.rs`, nine, each ending or replaying through `admit_entry` where a history is built):
  the five named, plus no-memory re-entry, wallet failures classified, question attributed and exact, and no ports.
  The holding account is a stand-in driven through the same rule as `Holdings::start_handed_on`.
- **Rule-off** (each alone, file run, restored; none hung):

  | Rule switched off | Red |
  |---|---|
  | arm stubbed to `Failed` | six of nine |
  | route starts where the value rests | `hand_on_of_a_route_other_than_the_linked_orders_accepted_one_is_refused` |
  | route is the linked order's | the same |
  | holder is the decided provider | the same |
  | one part only | the same |
  | no withdrawal first | the same |
  | question must be attributed | `hand_on_question_must_be_recorded_attributed_and_be_the_withdrawal_decided` |
  | statement is the decided withdrawal | the same |
  | `Done` only on a proving record | `hand_on_with_an_unproven_start_is_refused` |
  | only the proven refusals are `Failed` / the platform's refusal list | `a_wallets_failure_or_a_refusal_that_could_follow_a_hand_over_is_unknown_not_failed` |
  | no memory means `Lost` | `hand_on_taken_on_by_a_mover_with_no_memory_of_it_is_never_handed_over_again` |
  | linked order not already followed; all of what the part holds | none red: `hand_on`'s `AlreadyOpened` and decide's `within` hold them first |

- **Review round (of `e4065786`).**
  1. *Blocker, fixed.* The mover now holds the linked order to what the log records before the wallet is asked:
     `hand_on::linked_accepted` (the checks `build_hand_on` made, now one function both call) runs in `plan_hand` over
     the log's entries (`LogRead`, `HandingOn::log`): the decision record is in the log, the acceptance is the
     customer's own, of the linked order, keeps the original requester, and the plan fits its yeses; the route's first
     segment is the decided provider's. `AcceptedRoute` gains `acceptance`. A linked order accepted on an automatic
     deposit is refused with the wallet shown nothing (`hand_on_to_a_linked_order_the_customer_did_not_accept_is_refused_before_the_wallet_is_asked`).
  2. *Re-ask, fixed.* The verdict is set from the wallet's answer before anything that can fail; a drain that can't
     be kept makes the outcome `Unknown` (`uncarried`), never an error to retry; and a flight that was asked with no
     verdict answers `Unknown` without calling the wallet. Test `a_call_that_stopped_after_the_wallet_was_asked_never_asks_it_again`
     (the clock made unreadable once the wallet was handed the statement).
  3. *Green rule-offs, pinned.* `hand_on_to_a_linked_order_core_already_follows_is_refused_before_the_wallet_is_asked`
     and `a_hand_on_resolved_directly_for_less_than_the_part_holds_is_refused` (a direct `Resolutions::resolve`, past `within`).
  4. *Owners.* `PLAN.md`'s `T-020-26` row now carries the known-outcome path for an unknown hand-on (with the one
     prov seam: the wallet's signed answer and transaction on `HandedOnStart`), the assembly calling `handing_on`, and
     the real plain wallet's scenario, with named tests. `T-024-06`'s row is not edited.
  5. The `Effect` parameter of `withdraw` is read only by a `debug_assert!`, for `core-authz`'s entry-point scan;
     said in a comment.

  Rule-off of the round (each alone, file restored): the acceptance and decision-record check red on the customer's-acceptance
  test and the route-refused test; the already-followed guard red on its test; the whole-value check red on the
  direct-resolve test. Still green, each defence in depth: the first segment's provider (the acceptance's yeses refuse a
  plan from another reserve first), the re-ask guard and the drain rule (the verdict is set before either can matter).

- **Defaults** (reversible, fail closed): one-part linked orders only; a hand-on only where the node's holder is the
  operator's plain wallet (`Holdings`), so a private node's holder fails closed; the route comes from the assembly's
  `LinkedRoutes`, Core's log holds only its decision record's id and acceptance; the question is about the linked
  order, as `holding-withdrawal-rests-on-its-record` needs.
- **Gaps, with owners.**
  1. The wallet's signed answer isn't handed out by `start_handed_on` (`HandedOnStart` carries the record, not the
     statement), so the `holding_withdrawal`'s statement receipt can't be written from the mover's result, and an
     `Unknown` hand-on has no known-outcome path (`Mover::statement` answers `NoMove`): `T-020-26`, with a prov seam
     (the row now names it).
  2. The real plain wallet isn't run in these tests (the kit's reserves' nodes are private, the wallet's binaries are
     prov's build): `T-020-26`'s scenario runs it with the assembled Core; the `Wallets` adapter's refusal path is untested.
  3. `HandOn::hand_on` isn't held to the plan the mover withdrew for; the assembly passes `Withdrawn::route`: `T-020-26`.
  4. The assembly doesn't call `handing_on` yet: `T-020-26` (its row now names it; `T-024-06` is not edited).
  5. A private node's holder (a provider's module) isn't driven: `T-020-26`.

## Owners added by `plan/m3-owed-rows-2`

| Gap, and the note it is in | Owner |
|---|---|
| `relayed_by: None` at five sites of core-orch (`T-022-02`'s finding 2) | `T-020-28`, wave 26, after `T-020-23` and `T-020-24` (dependencies) and, run in turn, `T-020-19`, `T-020-25` and `T-020-27` |
| `decision-is-for-its-caller`, the role check in `attribute()`, a "no formula in this asset" marker on a charge raise (`T-020-14`'s closed gap 3; `T-020-21` the shape, `T-020-29` the writer), `What::Evidence` without a segment (`T-020-16`'s gap 1), and `answer-follows-its-question` for the arrival shapes (`T-020-18`'s hand-off) | `T-020-21`; the evidence entry for `stuck_resolved` is written by `T-020-20` (3) |
| `NOT_A_NOTICE` replaced by a positive check of `ARRIVAL_NOTICE_SHAPE`, and the seeded deposit's per-credit reference (`T-020-18`'s hand-offs) | `T-020-19`, done (its notes) |
| `Prices::report` has no caller in Core's own completion (`T-020-14`, gap 4): only the assembly calls it (`T-024-04`) | `M4`, `F-030`, in §4.4's inherited table |
| `Terms::take` has no production caller (`T-020-14`, gap 1) | `M4`, `F-030` and `F-031`, in the same table |
| The stand-in reserve's arrival-notice test (`T-020-18`'s hand-off to `F-024`) | `T-024-15` |

## T-020-21: `core-internal`'s next round

Branch `f-020/t-020-21-core-internal-round`, from `m3` at `c99e130a`. Edits `contracts/` (core-internal, and one doc of
router-api), a `DEC-20` path, and only the consumer lines that compiling needs (named below).

- **Versioning: folded into 1.0.0, no bump** (reversible default). The row asks for one `DEC-4` bump whose changelog
  names each change. core-internal 1.0.0 is on no `develop` (`git show origin/develop:contracts/CONTRACTS` lists no
  core-internal), so `PLAN.md` §4.4 and retrospective lesson 7 make folding into the unreleased major the rule, as
  `T-022-05` did. The changelog's last entry names each item, by number. router-api's doc change folds into its own
  unreleased 4.0.0 (`develop` has 3.0.0), with a changelog line. The contract guard against `origin/develop` reads
  core-internal as a new contract.
- **Item by item** (what was built, or why not):
  1. *Notice summary*: `What::NoticeSummary {to: NoticeReceiver {party, role}, of: NoticeOf (order {order,
     sequence} | raise {raise}), last_try: Count, ended: NoticeEnd (delivered | abandoned)}`, observed. Rows
     `notice-summary-names-its-raise` (exists) and `one-summary-per-notice` (none other).
  2. *`Operation::stated_out`*: doc only, router-api.
  3. *Public-end mismatch*: `What::PublicMismatch {order, segment, node: OuterNode, movement, found:
     PublicEndMismatch, moved, stated: PublicFigures, rests_on}`; the raise's `Difference::PublicEnd {order, segment,
     found, moved, stated}`; Rust holds the named figure to differ, in both; row
     `public-mismatch-rests-on-the-providers-result` (closing rows).
  4. *Checkpoint shapes*: schemas `checkpoint` and `checkpoint-opening`; `Checkpoint`, `CheckpointOpening`, `Tick`,
     `SignatureValue`, `CheckpointSignature`, the two shape enums and constants.
  5. *Decline*: `What::DeclineOutcome {request: OutcomeDecline, decision}`, a decision kind (`is_decision`), on its
     own trait `OutcomeDeclines`; row `outcome-decline-rests-on-its-report`; `one-report-per-effect` lets a new report
     stand after a decline; `outcome-confirmation-rests-on-its-report` gains `declined`;
     `outcome-confirmation-cites-verified-evidence` cites the decline's evidence.
  6. *Earlier arrival*: `What::Arrival.earlier`; row `arrival-names-its-earlier-arrival` (reserve, deposit).
  7. *Empty shortlist*: **already met** at `c99e130a`: "a shortlist may keep no provider" was folded into 1.0.0
     (`T-020-01`), with `verified-entry-of-a-shortlist-record-keeping-no-provider` and the admission cases
     `input-cites-its-decision-record--no-route-on-a-shortlist-keeping-none` (admitted) and
     `--no-route-on-another-orders-empty-shortlist` (refused). Not built again; the named test runs those fixtures.
     The writer is `T-020-25`'s.
  8. *`HandedOver` item*: `HandedOver::OperationUnanswered {provider, transaction, ended_by: Token}`.
  9. *Holding account's record*: the kind and the relation were **already met** (`holding_withdrawal` and
     `handed-on-route-starts-on-its-holding-record`, folded in by `T-020-13`'s integration round 2, after the gap was
     written). What was missing: its `node` (the row's test) and `REQ-ORD-40`'s label at a private node. Built:
     `node` (required) and `publisher` (optional); `holding-withdrawal-starts-a-handed-on-route` gains `node`,
     `holding-withdrawal-rests-on-its-record` gains `publisher`. Every fixture holding one was patched to name
     `ledger-in` (insertions only).
  10. *Final-deposit relation*: **already met**: `final-deposit-closes-on-a-public-record` (closing rows, `T-020-09`'s
     review, H2b) holds a part's last deposit to grade 1 naming its movement and a verified statement of the
     segment's provider. Not built again; the named test runs its cases. That the reserve's statement is a `result`
     shape the row doesn't read; a verified statement bound to that segment is in practice one (residual, below).
  11. *Late status and a later inner node*: `Action::AskStatus {order, segment, provider}`, decided and taken as an
     effect, row `status-is-asked-of-the-segments-provider`. `reversal-rests-on-its-evidence` reads the reserve at
     the segment's from-node (its `reserve` reading) instead of the order's origin reserve for the failed provider's
     own word. Corrected on review: this widens admission and closes no fail-open. The base row already refused the
     origin reserve's word at a node not its own, through `reserve`; the change admits one case more, the provider
     that delivered to a later inner node failing out of it and vouching for itself as that node's reserve
     (`REQ-ORD-3`'s second sentence), which a split order's later part needs. At the first node the two readings are
     one reserve, and no admitted history moved; `--the-origin-reserves-own-word-at-a-later-inner-node` was refused
     before too, its field now `whose` where it was `reserve`. A reversible reading, on the owner's form (ADR-0008's
     open question).
  12. *`decision-is-for-its-caller`*: row (none to point to) on `decide`.
  13. *Role check*: `attribute` refuses (`IN_A_ROLE_ITS_MAKER_DOES_NOT_HOLD`) a statement whose bytes bear its
     claim out in a role its maker doesn't hold: the operator's maker as `operator`, any other as `provider` or
     `verifier`.
  14. *No-formula mark*: `Difference::Charge.no_formula: Option<NoFormula>` (`for_this_asset`); row
     `no-formula-marks-a-charge-in-another-asset`. `maximum` stays required, `0` beside the mark.
  15. *A stuck part's evidence*: `What::PartEvidence {evidence, order, part, grade, whose, rests_on}`; rows
     `part-evidence-rests-on-statements` and `stuck-resolved-cites-its-parts-evidence`.
  16. *Arrival answers*: `answer-follows-its-question` holds an `arrival-answer` to an `arrival-question`.
- **Defaults** (reversible, each failing closed):
  - fold into 1.0.0 (above);
  - the decline on a trait of its own, `OutcomeDeclines`, so no `StuckOrders` implementer changes; nothing
    implements it yet, so no report is declined and a wrong report holds its part;
  - `Action::AskStatus` decided by policy, as `ask_reversal` is, so its question is appended before it is sent;
    policy's role table answers `None` for it, so it is refused, as `erase_statement` is;
  - `part_evidence` a kind of its own rather than an optional segment on `evidence`, so no reader of a segment's
    evidence meets one without a segment;
  - `stuck-resolved-cites-its-parts-evidence` as none other: it refuses a citation of another order's or part's
    evidence, or of a segment's, and admits one with no entry, since Core appends none before `T-020-20`;
  - the role a maker holds read from the log alone: the operator's maker, or any other as provider or verifier;
    a mislabelled genuine statement refused rather than recorded unverified;
  - the checkpoint shapes carry the contract's `$id`s as their `shape`; the commitment's computation stays
    `core-audit`'s;
  - a public end's figures as identifiers and an amount (`PublicFigures`), the destination an `Id` (128 characters,
    the identifier pattern);
  - the holding record's `node` the route's first segment's inner node; core-orch's `hand_on` fills it from the
    linked route it recorded, refusing an outer start (`NotAHandedOnRoute`, unreachable after `proves_start`).
- **Consumer lines changed to compile** (no behaviour): `core-orch` `arrival/mod.rs`, `arrival/filing.rs`
  (`earlier: None`), `quote/charge.rs` (`no_formula: None`, as the row says), `hand_on/mod.rs` (`node`,
  `publisher: None`; `T-020-17` edits the same module: a merge to watch); `core-authz` `roles/mod.rs`
  (`AskStatus => None`), `parties/mod.rs` (`Scope::AskStatus`), `engine/mod.rs` (a test's exhaustive arm, and
  its guard's table marking `OutcomeDeclines` unbuilt, `false`, until policy implements it);
  `core-audit` `tests/log.rs` (two name maps); `conformance/scenarios/tests/segments_support/mod.rs`
  (`earlier: None`).
- **Red first.** The fixtures (by a generator in the scratch folder, `f020t/gen.py`) and the nineteen named tests in
  `contracts/tests/core-internal.rs` came first, against the contract at `c99e130a`: all nineteen failed, on an
  assertion or a panic reading a fixture, none on a compile error (the tests name fixtures, rows and one constant
  only). Two of them, for the items already met (7, and the row's `route handed on whose first move rests on no
  holding account record is refused`), failed only through the `node` the round patched into their histories; their
  cases were in the table already and pass beside the shape. The shapes, schemas and rows followed.
- **Histories the log refused, sorted.** After the shapes, `core-authz`'s unit tests went red on two test artefacts,
  none a Core bug: three of them read `RELATIONS[0]` as the effect row, which the new `decision-is-for-its-caller`
  had displaced (the row moved after `permit-backs-one-effect`, so `RELATIONS[0]` is the effect row again); and its
  guard that every policy trait is built met `OutcomeDeclines` (marked unbuilt, above). No workspace history was
  refused by a new row: Core's `decide` entries name their caller, its statements claim the role their maker holds,
  and nothing appends an arrival answer, a decline, a part's evidence or a status ask yet.
- **Gaps, with owners.**
  1. Policy's answer to `decline_outcome` (`OutcomeDeclines` for the engine, as `confirm_outcome` is, `T-021-06`'s
     roles): no row owns it; `T-020-20` needs it, and edits only `stuck/`. Owner: the lead, a row of `F-021`'s (or
     `T-020-20` widened to `core-authz`'s `stuck`), which also flips the engine guard's `OutcomeDeclines` to `true`.
  2. Policy's rule for `ask_status` (`roles` answers `None`): `T-020-27` needs a permitted ask. Owner: the lead, a
     policy row (as for `erase_statement`).
  3. `stuck-resolved-cites-its-parts-evidence` admits a `stuck_resolved` whose evidence has no entry, since Core
     writes none until `T-020-20`; tightening it to "an entry exists" is a contract change after `T-020-20` lands.
     Owner: the next `F-020` contract row.
  4. The holding record's and a reversal's private-node label: the log can't tell a node declared private from the
     catalogue. Residual, as before (`T-025-05` reads it).
  5. `final-deposit-closes-on-a-public-record` holds the reserve's statement verified and the segment's, not of
     `result` shape. A verified statement bound to the segment is in practice its result; residual, noted for
     `T-020-22`.
  6. A provider and a verifier are one role class to the log: a provider claiming `verifier` is not caught. Owner:
     a later round with a catalogue read in `StatementKeys` (none in `M3`).
  7. `T-022-05`'s gap 4 (a report's tolerance by increment and gross or net) was named for "`T-020-21`'s round" in
     `F-022`'s notes but isn't in this row's list; not built. Owner: the lead.
- **Rule-off table** (each switched off alone, `rutent-contracts`' tests run under a 600 s alarm, tree restored; none
  hung):

  | Rule switched off | Tests that went red |
  |---|---|
  | `decision-is-for-its-caller` | `decision_whose_actor_is_not_the_requests_caller_is_refused`, the citation table's |
  | `outcome-decline-rests-on-its-report`, and each of its fields `party`, `login`, `evidence`, `open` | `decline_of_a_report_naming_no_report_is_invalid`, the citation table's |
  | `one-report-per-effect`'s "a declined report stands no more" | the same two |
  | `outcome-confirmation-rests-on-its-report`'s `declined` | the same two |
  | `outcome-confirmation-cites-verified-evidence` citing a decline's evidence | the citation table's |
  | `stuck-resolved-cites-its-parts-evidence` | `stuck_resolved_citing_an_evidence_entry_that_names_another_part_is_refused`, the citation table's |
  | `status-is-asked-of-the-segments-provider` | `request_for_a_providers_late_status_without_its_segment_is_invalid`, the citation table's |
  | `arrival-names-its-earlier-arrival`, and its `reserve`, `deposit` | `arrival_naming_an_earlier_arrival_of_another_reserve_is_invalid`, the citation table's |
  | `part-evidence-rests-on-statements` | `evidence_entry_of_a_stuck_resolution_without_its_part_is_invalid`, the citation table's |
  | its `verified` | the citation table's |
  | its `about`, the "no segment" conjunct alone | none: the transaction-binding conjunct refuses the case too (kept for an unverified statement naming no transaction) |
  | its `about`, whole | `evidence_entry_of_a_stuck_resolution_without_its_part_is_invalid`, the citation table's |
  | `notice-summary-names-its-raise`; `one-summary-per-notice` | `notice_summary_entry_without_its_outcome_is_invalid`, the citation table's |
  | `no-formula-marks-a-charge-in-another-asset`, and its `asset` | `charge_raise_marked_as_having_no_formula_whose_fee_is_in_the_segments_own_asset_is_refused`, the citation table's |
  | `holding-withdrawal-rests-on-its-record`'s `publisher`; `holding-withdrawal-starts-a-handed-on-route`'s `node` | `holding_account_withdrawal_record_without_its_node_is_invalid`, the citation table's |
  | `reversal-rests-on-its-evidence` keyed back on the failed provider (not the from-node's reserve) | `reversal_at_a_later_inner_node_rests_on_that_nodes_confirming_party`, the citation table's |
  | `answer-follows-its-question`'s arrival shapes; an operation taken for an arrival question | `arrival_answer_with_no_arrival_question_before_it_is_refused`, the citation table's |
  | `public-mismatch-rests-on-the-providers-result`, and its `maker` | `mismatch_record_of_a_non_final_public_end_without_its_segment_is_invalid`, the citation table's |
  | its `verified`, `result`, `about` | the citation table's |
  | `What::PublicMismatch`'s figure differs; a `public_end` raise is real | the fixture tests, `mismatch_record_of_a_non_final_public_end_without_its_segment_is_invalid` |
  | `What::PartEvidence` rests on statements, at least one | the fixture tests, `evidence_entry_of_a_stuck_resolution_without_its_part_is_invalid` |
  | `attribute`'s role a maker holds | `the_log_not_the_writer_attributes_a_stated_entry`, `verified_statement_recorded_in_a_role_its_maker_does_not_hold_is_refused` |
  | schema: a summary's `ended`, a try from 1; a mismatch's `segment`; a decline's `report`; a holding record's `node`; a part's evidence's `part`; a status ask's `segment`; an unanswered operation's `transaction`; an opening's `salt`; a checkpoint's closed properties, its tick's pattern | `every_valid_fixture_validates_and_every_invalid_one_is_rejected` and each item's named test |

### T-020-21, the review of `457ecc62`

The review (Opus, fresh): changes, no blocker. Each item, as built on this branch:

1. *A stuck part's evidence tied to its own part* (should-fix). `part-evidence-rests-on-statements`'s `about` now
   holds each statement to the part's own resolution: about the order and the transaction a `resolve_stuck` effect
   whose decide step names the part goes under, no segment, first bound to the order alone (`about_the_resolution`,
   as an outcome's evidence about a stuck resolution is). The reviewer's probes, the evidence named for p1 or for a
   part with no resolution on p3's result under `tx-d3`, are refused cases (`--another-part`,
   `--a-part-with-no-resolution`). **Default, reversible**: a bare statement (grade 4) rests on the provider the
   resolution names to move the value (`resolution.by`), a new field `provider`, as a segment's rests on its
   provider's (`--provider` refused, `--the-moving-providers-word` admitted).
2. *Item 11's write-up corrected* (above, the CHANGELOG and the README): a widening, not a closed fail-open. The
   reviewer's cases as fixtures: D, the node's reserve failing out of it on its own word, is
   `--at-a-later-inner-node-its-reserves-own-word` (admitted, the round's own); A, the node's reserve confirming
   another's failure, `--at-a-later-inner-node-the-nodes-reserve-on-anothers-failure` (admitted); B, another failed
   provider's own word there, `--at-a-later-inner-node-the-failed-providers-own-word` (refused, `whose`).
3. *Rows for the gaps*: `T-021-09` (policy's answer to `decline_outcome`, which flips `core-authz`'s guard back,
   and its rule for `ask_status`; wave 26, before `T-020-20` and `T-020-27`, which now wait on it and move to wave
   27 with `T-020-25` and `T-020-28`); `T-020-30` (the next contract round: gap 3, a stuck resolution's evidence
   entry required, gap 7, `T-022-05`'s report tolerance by increment and gross or net, and, at the coordinator's word,
   `T-022-06`'s review's role on a measure record, per provider and per verifier; wave 28; `T-020-31` is left for
   `T-022-06`'s author's measure-caller row). `T-024-10` and
   `T-025-05` wait on both, in wave 29.
4. *An operator's module's role* (should-fix). `core-orch`'s `snapshot::record_refused` claimed a module the
   operator publishes in the operator's role, which the round's `attribute` refuses once the module's key verifies
   it. SPEC has a module as a provider (`REQ-MDL-1`; the catalogue's `Publisher::Operator` is "a module it
   publishes"), and every other writer claims it so, so the writer changed: the provider's role, for the operator's
   party. Test `operators_module_statement_is_claimed_in_the_providers_role` (`core-orch`'s
   `tests/snapshot_statements.rs`), and the contract's attribution case
   `refused--a-module-the-operator-publishes-in-the-operators-role`.

Low items, each done: the round's entry in `contracts/CHANGELOG.md`; `entry_at`'s doc comment restored above it,
`receipt_of`'s its own; a charge marked `no_formula` names a `maximum` of `0` (`Raise::check`, invalid fixture
`raise-of-a-charge-marked-with-no-formula-naming-a-maximum`), and an unmarked charge in another asset stays admitted
until `T-020-29` writes the mark (`T-020-29`'s gap); a public end's mismatch is at its movement's network
(`Rust`, invalid fixture `verified-entry-of-a-public-mismatch-at-another-network`) and of a segment that isn't a
part's final deposit (`public-mismatch-rests-on-the-providers-result`'s `non_final`, case `--non_final`); that its
node is the segment's planned public end stays Core's to hold (`T-020-22`), named in the README; the README says
`earlier` is never required and a checker links one deposit's arrivals by its notices' signed transaction.

- **Order of work.** The review's fixes came before their fixtures this time; each new rule was switched off once
  and a test went red (below).
- **Gaps, updated.** 1 and 2 are `T-021-09`'s, 3 and 7 `T-020-30`'s. 4, 5 and 6 stand as written.
- **Rule-offs of the review round:**

  | Rule switched off | Tests that went red |
  |---|---|
  | `part-evidence-rests-on-statements`'s `provider` (grade 4 on the moving provider's word) | `evidence_entry_of_a_stuck_resolution_without_its_part_is_invalid`, the citation table's |
  | its `about` on the part's own resolution transaction | the same two |
  | `public-mismatch-rests-on-the-providers-result`'s `non_final` | `mismatch_record_of_a_non_final_public_end_without_its_segment_is_invalid`, the citation table's |
  | a public mismatch at its movement's network (`What::check`) | the fixture tests, the mismatch test |
  | a marked charge's maximum of nothing (`Raise::check`) | the fixture tests, the no-formula test |
  | `record_refused` claiming an operator's module as a provider | `operators_module_statement_is_claimed_in_the_providers_role` |

## Journal receipts come from the log (`f-020/journal-receipts-from-the-log`)

**The bug.** `reversal/journal.rs::append` took a receipt only where its digest was the SHA-256 of the entry as Core
built it (`is_receipt_of`). The audit log attributes a stated entry from its kept bytes before it records it
(`AuditLog::append_through`, step 2): an entry Core handed in unverified, and the log could bear out, is recorded
`verified`, so its receipt is of the entry recorded, not of the one handed in. In the assembled Core every reversal
step that appended a statement the log could attribute (Core's own question to the failed provider, its question to
the origin reserve, the providers' signed answers and late words) got a receipt the check refused, was tried three
times and ended `Append(Unavailable)`: ask, confirm and every later step. The kit's log hid it, since it answers the
receipt rule's digest; `T-024-06` found it with the real log, and `T-020-16` had met the same class in `stuck/` and
added `Step::append_stated`.

**The journals, one verdict each** (every append of core-orch's journals, and whether it can be handed a stated entry
the log re-attributes, and checks the receipt against the entry as built):

| Append | Stated entry? | Checks the receipt against the entry as built? | Verdict |
|---|---|---|---|
| `quote/journal.rs:64` (`Step::append`) | yes (the quote's statements) | no: takes the log's receipt | fine |
| `quote/journal.rs:90` (`built_stated`) | yes | no | fine |
| `reversal/journal.rs:150` (`append`) and `:178` (`Step::append`) | yes (questions, answers, late words, a return's statement) and verified | **yes, for both** | **affected, fixed** |
| `arrival/journal.rs:100` | yes | no | fine |
| `rounds/journal.rs:45` (`append_stated`), `:97`, `:130` | yes, and verified records | no | fine |
| `segments/journal.rs:31` and `:38` (`append_general`) | yes | no: the model the fix follows | fine |
| `snapshot/journal.rs:15` | yes (a refused statement's stated entry) | no | fine |
| `stuck/journal.rs:55` (`append`) and `:84` (`Step::append`) | no: only `built()`'s verified entries reach them | yes, and rightly | fine: verified entries only |
| `stuck/journal.rs:98` (`append_stated`) | yes | no (`T-020-16`) | fine |

Also read, outside the list: `hand_on/mod.rs:406` checks the receipt too, but its steps are verified entries only
(`T-020-17`'s file; not touched), `recalc/mod.rs:809` and `intake/record.rs:45` take the log's receipt. `stuck/mover.rs`
:557 deliberately compares an attributed question's receipt with the entry as built, to find a question the log
recorded **un**attributed; that is a check of the attribution, not a receipt rule, and stays.

**The fix.** `reversal/journal.rs::append` takes the log's receipt for a stated entry, as `segments/journal.rs::
append_general` does, and holds a verified entry to the receipt rule as before. One function serves both
(`Step::append` goes through it), so there is no second copy. What the check protected stays: a verified entry's
receipt must still be its own digest (an answer for another entry is no answer and is tried again, then
`Unavailable`), and a refusal, a statement's included, is returned at once and never retried or swallowed. The log's
`admit_entry` is the gate for a statement: a question recorded unattributed cannot be cited by the effect that follows
it, and the log refuses that effect.

**Core's own questions are guarded** (the review of `08ec786b`). Taking any receipt for a stated entry would let a
question the log recorded **un**attributed go through: its receipt is the digest of the entry as built, the log can't
tie it to the operator, and `admit_entry` refuses the provider's answer after the value moved. `Step::append_question`
(used for `ask_reversal`'s operation and the confirm question, nothing else) refuses such a receipt before anything
is handed over, as `stuck/mover.rs` does for its question, and refuses again on a retry, the log answering its equal
entry with the same receipt. Answers and late words stay unguarded: an unattributed provider's answer is legitimate.

**Tests.** The file's `World` is built over a log that attributes (`World::attributing`; the charge test opts out, as it
cites the statements it kept by the receipts of the entries as handed in): on a stated entry it runs
core-internal's `attribute` over the evidence store's kept bytes and the catalogue's keys, as `AuditLog` does, records
the entry attributed, and answers an equal entry with its own receipt; the receipt it answers is of the entry
recorded. It is not `AuditLog` itself: the audit log's key register is fed only by granted advertisements recorded in
it, and the `World`'s policy writes its decisions raw, so the audit log's `admit_entry` is not run on append: every
test replays it over what the log holds (`admitted_from`), as the others of this file do. Core's assembly doesn't hold
a reversal yet (`T-024-06` wires it): its own test with the audit log itself is that row's `AC-8`, which passed with a
one-line local fix of this function.

**Rule-off table.**

| Rule switched off | Tests that go red |
|---|---|
| `append` takes the log's receipt for a stated entry (switched back to the receipt rule's check for every entry; before the fix, the same: `Append(Unavailable)`) | `a_reversal_is_asked_over_a_log_that_attributes_its_statements`, `a_reversal_is_confirmed_over_a_log_that_attributes_its_statements`, `a_late_word_and_a_return_are_kept_over_a_log_that_attributes_its_statements` |
| a verified entry's receipt must be its own digest (check removed) | `a_verified_entrys_receipt_is_still_its_own_digest` |
| a refusal is returned, never retried or swallowed (turned into a retry) | `a_refusal_of_the_log_is_never_swallowed` |
| a question recorded unattributed backs no move (`append_question`'s digest check off) | `a_question_the_log_leaves_unattributed_backs_no_move` |

Run once per rule, under a hard timeout, nothing hung. The three journals of the list that check no receipt
(`quote`, `arrival`, `rounds`, `segments`, `snapshot`) and `stuck`'s, which handles its statements by `append_stated`,
have no rule of this kind to switch off: no test is added to them.

## T-020-29: the charge raise says when a fee is in an asset no formula covers

Branch `f-020/t-020-29-no-formula-marker`, from `m3` at `7ac373aa`.

- **The writer.** `Charges::check` (`quote/charge.rs`) sets `Difference::Charge.no_formula` to `for_this_asset` exactly
  where it raises a charge whose `charged_in` is some other asset than its tuple's `from` asset, the one branch that
  already raised against a `maximum` of `0` with no terms. Every other raise leaves it absent: a charge held to a
  formula, including one whose `charged_in` is stated as the segment's own asset (which goes through the formula, as
  before). A charge of nothing in another asset still raises nothing.
- **Tests** (`components/core-orch/tests/reversal.rs`, beside `T-020-14`'s, which pass unchanged):
  `charge_in_another_asset_than_the_segments_is_raised_marked_as_having_no_formula` [`REQ-QTE-5`] and
  `charge_in_the_segments_own_asset_is_not_marked` [`REQ-QTE-5`, `AC-14`]. Each raise goes through the log's
  `admit_entry` (`admitted_from`), so the contract's `no-formula-marks-a-charge-in-another-asset` row admits the
  marked raise (the plan's `from` asset differs from EURC) and `Raise::check` holds `maximum` at `0` beside it. The
  second test covers the own-asset charge unstated and stated, both above the formula, naming the formula's own maximum.
- **Red first.** Before the change the first test failed on its assertion (`left: None`, `right: Some(ForThisAsset)`);
  the second passed from the start, as a guard against over-marking, and was watched red by the rule-off below. A first
  version of the second test was refused by the log for a test artefact (a charge of the exchange's segment named the
  wallet as provider, so the result's maker was not the provider); the test now uses the reserve's own segment, as
  `T-020-14`'s does. No Core bug.
- **Defaults** (reversible, fail closed): the mark is set from `charged_in` against the tuple's `from` asset in Core, not
  inferred from `maximum == 0`; a charge in another asset raises with no terms named, as `T-020-14` already did.
- **Gaps.** The simulated providers cannot state a fee in their segment's out asset, so no conformance scenario runs a
  marked raise through the assembled Core; the case is held by `core-orch`'s `World` and the log's replay. Owner: the
  sim is `T-024-14`'s (edits `conformance/src/sim`); a scenario is `F-024`'s to add once the sim can.

| Rule switched off | Tests that go red |
|---|---|
| the mark is not set on a charge in another asset (`no_formula: None`) | `charge_in_another_asset_than_the_segments_is_raised_marked_as_having_no_formula` |
| the mark is set on every raise, the own-asset ones too | `charge_in_the_segments_own_asset_is_not_marked`, and `T-020-14`'s `a_charge_is_raised_on_its_result_and_the_terms_that_bound_its_segment` (the log refuses the mark on an own-asset charge) |

Each run once, nothing hung.

## T-020-24

Branch `f-020/t-020-24-integration-items`, from `m3` at `7ac373aa`.

- **Intake ends the order on a carry-on refused as `OutsideAuthorisation`.** Where another login of the requester's
  party carries on a logged acceptance and policy refuses its authorisation as `OutsideAuthorisation`, the order
  ends: its `order_refusal` is recorded, of the class policy's refusal gives (`refused_by_policy`), nothing starts and
  the customer is told that class (`REQ-AUZ-3`, `REQ-ORD-18`). Every other refusal of that login still answers
  `IntakeError::OtherActor` and leaves the order as it was, as `T-020-03` ruled; the same-actor carry-on already
  ended on any refusal. A stranger is not asked: `kept_for` answers `NoSuchOrder` before policy, alike for a real
  order and a missing one (`REQ-AUZ-7`), and the test holds it with policy set to refuse as outside the
  authorisation.
- **`StatusError::Refused` is gone**, with the `ReadAnswer` import it needed. Nothing built it since `T-020-08`.
- **Core reads the customer's `AcceptFault` decision from its own history.** `Accepting::Customer` carries only `by`.
  `Reversals::follow` keeps, per order, each permitted `decide` of an `accept_fault` for it with the receipt it has in
  the log, and `accept_fault` carries the reversal on the latest of those whose caller is `by` and whose request names
  this order, segment and the showing's transaction and commitment. No such decision is `NotAnAcceptance`, a refused
  one included (`NotPermitted` stays for `ask`). The carrier cites that receipt. The caller's copy of a decision is
  no longer an input, so a stale or made-up one carries nothing and blocks nothing (`REQ-ORD-11`).
- **Defaults (reversible, fail closed).**
  - The decision is found as of the last `follow`: the assembly follows the order from the log before it accepts a
    fault, as it already does as the log grows. A decision the log holds but `follow` has not seen is no acceptance.
  - Where the log holds several matching permitted decisions, the latest is cited.
  - A refused decision reads as `NotAnAcceptance`, not `NotPermitted`: Core no longer holds policy's answer.
- **Gaps.**
  - The assembly (`F-024`) must call `Reversals::follow` before `accept_fault`; `Accepting::Customer` lost its
    `request` and `decision` fields, so its call sites change (the tests' were updated).
- **Rule off**, each once, its test red: the carry-on's `OutsideAuthorisation` branch; a stranger told another
  refusal than `NoSuchOrder`; `follow` keeping decides whatever their outcome (the log's refusal then carries);
  and the same with the first matching decide taken (the earlier refused decision then is cited).

## T-020-26

Branch `f-020/t-020-26-holding-withdrawal-record`, from `m3` at `4b306ca9`. Edits `core-orch` `hand_on/` and `stuck/`,
the prov seam (`holding/wallet/handed_on.rs`, and the private `movement` in `holding/wallet.rs`, which now hands the
wallet's signed answer to it), `conformance/scenarios/src/assembly/` (`holding.rs`, small hunks in `mod.rs`), the kit
(`segments_support`: `prove_labelled`, `wallet_answer`, `Core::admitted_skipping`) and `contracts/core-internal/` (a
`DEC-20` path).

- **Design.**
  1. *The record.* `prove_start` already wrote the `holding_withdrawal`; the row adds its label (`Start::publisher`,
     read by the assembly from the catalogue's `publisher_of_node`; `None` at a public node) and
     `HandOn::recover_starts(segments, entries)`, which proves again, after a restart, each start whose
     `holding_withdrawal` the log holds: Core's own entry, for a linked order opened by a `route_chosen` naming the
     order it was handed on from, with a `handed_on` of that order naming it, every part starting at the record's node
     with a segment of its holder; once per order. Anything else proves nothing. The assembly calls it on `reopened`.
  2. *The wallet's answer.* `HandedOnStart` gains `transaction` and `answer` (the signed result as the platform passed
     it on). The mover keeps the answer as the holder's stated entry about the linked order under the wallet's own
     transaction (the question's), before the effect's result, and answers `Done` only when the record proves the
     start, the answer says the same (`said`), and the log recorded the answer attributed. `Mover::withdrawn` gives the
     answer's receipt (`Withdrawn::statement`), which the `holding_withdrawal` cites, and works for a hand-on left
     unknown once `Delivering::statement` took the holder's answer as `Done`. `Delivering::statement` for a hand-on
     checks the statement (attributable, a result, the wallet's transaction; the holder's judged: `Done` only for a
     record that proves the start, `Failed` for its own failure or final refusal, otherwise `Unknown`; a succeeded
     answer that proves nothing of the start is `NotCounted`; a wallet never asked is `NoMove`).
  3. *The assembly.* `Core::move_stuck` hands the mover `HandingOn {HeldWallets over Holdings, Accepted, LogHandle}`.
     `LogHandle` is the log's own `Rc`, read live. The platform is shared by the mover and the wallets through a
     `RefCell<Running>` (`Shared`). `Core::hand_on` writes the hand-on and the start from `Mover::withdrawn`.
  4. *The contract arm* (found by this row; decided by the coordinator): the wallet names its own transaction, which
     the question binds to the linked order, but `outcome-confirmation-cites-verified-evidence` read a stuck
     resolution's own transaction only, so no statement of the wallet could be the evidence of a hand-on's outcome.
     The log can't take the same bytes about two orders (a verified statement's `about` transaction must be the
     body's, and a transaction belongs to one order). The row's `about` now also accepts a statement about the linked
     order the decision named, no segment, through a transaction first bound to it and to no segment. Folded into
     core-internal's unreleased 1.0.0. It reads no maker: a confirmation needs another maker's statement, so holding
     the arm to the holding account's maker would make it impossible; that the record is the holder's own is
     `holding-withdrawal-rests-on-its-record`'s and Core's (`said`).
- **Tests.** `tests/hand_on_record.rs` (four), `tests/stuck_hand_on.rs` (five added: known outcome Done, through
  report, confirmation, hand-on, record and the linked order's first move, and admitted whole; a holder's failure frees
  the part only on a confirmation, admitted; a wallet never asked; an answer that doesn't bear out its record; an
  answer the log couldn't attribute), `tests/assembly_hand_on.rs` (two), `contracts/tests/core-internal.rs`
  (`hand_on_outcome_report_rests_on_a_statement_about_its_linked_order`, five fixtures). The two T-020-17 assertions
  that counted one stated entry now count two (question, answer). Tests were written beside the code, not seen red
  first; the rule-offs below were the check.
- **Rule-off** (each alone, file restored, none hung):

  | Rule switched off | Red |
  |---|---|
  | the hand-on arm of the evidence row | the contract test, and the citation table's cases |
  | evidence about the linked order only (`about.order == Some(linked)`) | none red in the first round, which the review corrected (S3, below): the binding rule refuses another order's statement, but not one that names no order |
  | no segment | the contract test and the table's cases |
  | transaction first bound to the linked order | the same |
  | the record labelled its publisher | `record_at_a_private_node_is_labelled_its_publishers_word` |
  | a recovered record must be Core's | `proven_start_is_rebuilt_from_the_log_after_a_restart` |
  | its holder starts the route | the same |
  | it needs the hand-on | the same |
  | the answer kept as the holder's stated entry | eight of `stuck_hand_on` |
  | the answer bears out the record | `hand_on_whose_answer_does_not_bear_out_its_record_is_left_unknown` |
  | an unattributed answer backs no done | `hand_on_whose_answer_the_log_could_not_attribute_is_left_unknown` |
  | the statement verifies under a published key | the known-outcome test |
  | a holder's failure is read as failed | the failure-confirmation test |
  | a record that proves nothing bears out nothing | the known-outcome test |
  | a known outcome starts the route | the known-outcome test |

- **Defaults** (reversible, fail closed): a hand-on stays a one-part, plain-wallet-at-a-public-node matter; the label is
  given only where the catalogue names a publisher for the node; a success with no record of the wallet's own is kept
  as `Unknown`; an answer the log recorded unattributed, or one Core can't keep, leaves the outcome `Unknown`; the
  contract arm reads no maker (above).
- **Gaps, with owners.**
  1. The scenario that runs the real plain wallet with the assembled Core moved to `T-024-17` (a public node on a
     route's path: the reserve's ledger is private and can't also be public; an accepted release for the
     operator-published wallet in the log; a linked-order intake path). This row's assembled-Core test is the
     fail-closed one: the log refuses a hand-on to a linked order it holds no acceptance of.
  2. The mover's and the hand-on's state is in memory (`F-030`); after a restart a fresh mover has no flight, so the
     known-outcome path needs the flight kept. `recover_starts` rebuilds only the proven starts.
  3. The assembly's `reopened` carries neither the holdings nor the accepted routes (`F-030`).
  4. A private node's holder (a provider's module) isn't driven (`T-020-17`'s gap 5), nor is the label at a node
     the catalogue declares private tested with the real catalogue's `publisher_of_node` (the kit labels by hand).

### T-020-26, the review of `66e821d4` (Opus): changes

- **S1, fixed.** `about_the_linked_order` took any transaction first bound to the linked order. A late "succeeded" of an
  earlier effect's withdrawal (confirmed failed) could be the evidence of a later effect, and one route be funded on a
  withdrawal the log holds failed (probe E). The transaction's binding entry must now be the operator's own
  `operation` question, appended after the cited effect and before that effect's result. Fixtures
  `--a-hand-on-through-another-effects-transaction` (probe E) and `--a-hand-on-through-a-question-after-its-result`.
- **S2, fixed.** A report of a hand-on rests on the holding account's own statement: the evidence's maker is the
  resolution's `by`, read in the row's `maker` field (probe H: a mover reporting failed on an unrelated maker's
  word, confirmed on the holder's own `succeeded`, then a new decision: the pay-twice path). A confirmation or a
  decline keeps the row's own rule, since `outcome-confirmation-rests-on-its-report` needs another maker's party.
  `--a-hand-ons-linked-order-through-another-makers-statement` is now the refused
  `--a-hand-on-reported-on-another-makers-statement`; the valid other-maker case is a `confirm_outcome`
  (`--a-hand-ons-confirmation-on-another-makers-statement`). The same looseness on the deliver arm is `T-020-30`'s
  new item (4), not changed here.
- **S3, fixed.** A stated result naming only a transaction, no order, is admitted at append; only the conjunct
  `about.order == Some(linked)` refuses the report. The first round's note that the binding rule refuses it first was
  wrong, and is corrected in the rule-off table above. Fixture `--a-hand-on-through-a-statement-naming-no-order`.
- **L1, fixed.** `Mover::withdrawn` answers `None` once the holding account has stated, in a statement Core took, that
  the withdrawal failed (`a_known_start_is_given_to_no_hand_on_once_the_holder_has_stated_a_failure`).
- **L2.** `T-020-16`'s gap 5 (`known_outcome` and `confirm_outcome` don't check that the evidence states the outcome
  claimed) was routed to `T-020-20` but its row didn't carry it: it does now, as an item with the test `report whose
  evidence states another outcome than the one reported is refused`.
- **L3.** `prov/src/holding/wallet/handed_on.rs`: if `to_vec(&signed)` fails after the wallet moved value, the
  account's `self.ledger = next` is skipped. It is the same shape as the existing `proof_of_withdrawal(..)?`, and it
  can't fail in practice (a signed result is JSON). Noted as a known shape; the mover answers `Unknown` for it.
- **L4.** No test here checks that the re-serialised real wallet's answer verifies under its key. If it didn't, the
  outcome would fail closed to `Unknown`. `T-024-17` runs that path with the real wallet and owns the test.
- **L5.** The unreachable fallback in the shown transaction (`unwrap_or_else(|| linked.clone())`) is removed: the
  transaction parsed for the question's entry is the one kept. Left as they are: the mover's state is in memory
  (gap 2 above), and the label at a private node is tested with a hand-given publisher.
- **Owner's form, with its default.** SPEC doesn't say whether a confirmer's evidence maker must be a party to the
  order. Default: not required beyond the existing "another party" rule.
- **Merge.** `origin/m3` (`7ac373aa`) merged with a plain signed merge; `PLAN.md` and these notes kept both sides (the
  row count is 73 with `T-024-17`, whose wave is 29, beside `T-020-31`).

- **Rule-offs of the review round** (each alone, file restored, none hung): a report of a hand-on resting on the holder's
  own, the question after the effect, the question before the result, and the statement naming the linked order: each
  red on the contract test and the citation table's cases. The binding entry being the operator's own question was
  green on the first fixtures (every question there was the operator's), so `--a-hand-on-through-a-transaction-bound-by-no-operator-question`
  was added (its first version sat after the result, so the result rule masked the question rule; moved before it) and the rule is now red on the contract test and the table's cases. The holder's stated failure withdrawing the start (L1) is green:
  `flight.known` is cleared when the failure is taken, which already withdraws it; the flag is defence in depth.

## T-020-22

A mismatch at a non-final public end is recorded and raised; the final-deposit settlement relation is held on both
sides. Code: `components/core-orch/src/closing/` (`mod.rs` for the grading, the new `public_end.rs` for the raise).
Two additive hooks on the `Grading` seam in `segments/` (below). Tests: `tests/closing.rs` (the row's seven, and two
more), one probe in `tests/reversal.rs`.

- **What is recorded.** In `Evidenced::graded`, a segment that is not a part's final deposit, whose movement's check
  (`verify_movement`) answers `WrongDestination`, `WrongAsset` or `WrongAmount` against a final record, gets a
  `public_mismatch` entry among the entries it settles on, after its evidence and any replay: its order, segment, the
  planned public end as `node`, the movement (network and id), the figure that differs, what the network moved (its
  record) and what the provider said (the figures the check was held to), resting on the segment's result. The
  segment settles on the grade it earns as before: the evidence entry is unchanged, still naming no movement, since
  the check is `unchecked`; no input, no state change. Not a mismatch: a movement the network can't vouch for
  (unknown, not final, reorged out) or on another network.
- **The planned public end (`T-020-21`'s README item).** `public_end::planned_end` takes the node from the segment's
  operation, the outer node at either end on the movement's network, and never from the network's record, which the
  log can't check. For a withdrawal out of a public rail that is its `from`; for a non-final deposit out to a public
  one, its `to`.
- **The raise.** A `public_end` difference citing the mismatch's entry, delivered through the operator's channel and
  recorded `done` or `failed` by the delivery, as the charge raise is. It needs the mismatch's receipt, which only the
  segments' append makes known, so the `Grading` trait gets two defaulted methods: `recorded(settled, receipts)`
  (called after the graded entries are appended, again on a re-run of the settle) and `raise(desk)` (called on every
  run of a part, after the charges are checked, with a `RaiseDesk` of the log, clock, fresh ids, the operator's
  channel and Core's actor). `Closing` keeps the mismatches not yet raised and recorded: made once, its two entries
  built before it is raised, appended until the log takes it. `Closing::recorded` rebuilds them from the log: a
  mismatch with no raise citing it is raised on the next run, a raised one is not.
- **The final-deposit relation.** `final-deposit-closes-on-a-public-record` already holds a part's last deposit to
  grade 1 naming its movement on the authorised network and a verified statement of the segment's provider
  (`T-020-21` item 10, already met). This row writes Core's side and the two tests: Core waits for the record
  (`Waits(NoPublicRecord)`, a contradicting record included, which records no mismatch), the log refuses a deposit
  settled on a statement, and a deposit on a public record and the reserve's statement is admitted.
- **Defaults** (reversible, fail closed):
  1. A raise whose record the log can't take fails the run of the part, as a charge's does; the raise made is kept,
     and the next run appends it as built and raises nothing again.
  2. A mismatch whose figures the log's shapes can't hold (an id or an amount in a spelling they refuse) waits the
     segment as unreadable, as every other entry of this function does, rather than settling with the contradiction
     unrecorded.
  3. A raise cites the mismatch's entry alone; the result is reachable through it.
  4. A raise delivered but not recorded before a stop is raised again after a restart (the charge raise's own gap).
- **Gaps, with owners.**
  1. A non-final public end at an **inner** public node (one on the operator's public list) can't be recorded:
     `public_mismatch.node` is an outer node. Core records none there, though the segment's check ran. Owner: the next
     `core-internal` round (`T-020-30`), which would give the node a shape for an inner end; the lead.
  2. `Grading::raise` runs on every run of every part and raises mismatches of any order, since `Closing` is Core's
     record across orders. Harmless, but a per-order raise would be tidier. Owner: `T-024-02` (assembly).
  3. The mismatch's figures that the provider "said" are those the movement's check was held to (the result's
     amounts and the operation's asset); that they are the result's is for whoever holds the kept bytes, as the
     contract says. Owner: none (stated residual).
  4. The raise's ranking and the operator's channel are `T-023-02`'s.
- **Seam edits outside `closing/`** (additive, defaulted, so no other grading changes): `segments/ports.rs` (the two
  methods and `RaiseDesk`), `segments/mod.rs` (two call sites and `graded_receipts`). Said here so the lead can
  sequence them against rows that edit `segments/`.
- **Order of work.** Each test was written first and seen red on an assertion (after the two hooks existed as
  no-ops), then the code.
- **Rule-off table** (each switched off alone, file restored; none hung; `closing` and `reversal` test binaries):

  | Rule switched off | Tests that went red |
  |---|---|
  | a mismatch never recorded | the recorded-and-still-settles test, the each-figure test, the raise test, the retry test, the planned-end test |
  | figures swapped (what the network moved and what the provider said) | the recorded test, the each-figure test |
  | `raise` does nothing | the raise test, the retry test |
  | raised again on every call | the retry test |
  | delivery ignored (always `done`) | the retry test |
  | no rebuild from the log after a restart | the retry test (restart half) |
  | the rebuild ignores a raise the log holds | the raise test (restart half) |
  | receipts not bound to the mismatch | the raise test, the retry test |
  | planned end taken from the `to` side only | the recorded test, each-figure, raise, retry, planned-end tests |
  | segments don't hand the grading its receipts | `the_grading_is_told_what_it_answered_and_asked_to_raise_on_every_run` |
  | segments never ask the grading to raise | the same test |
  | Core settles a final deposit on a statement | the statement-alone test, the public-record test and eleven older ones |
  | the log lets a statement close a deposit (contract relation, restored) | the statement-alone test, `providers_word_never_closes_a_part` |
  | the log refuses every close, a public record's included (contract relation, restored) | the public-record-admitted test and nine older ones |
  | a final deposit also records a mismatch | none: not observable. Core's check returns `NoPublicRecord` first, so the guard is defence in depth; the log's `non_final` row (the contract's cases) is the second |

### T-020-22, review answers (of `cd515045`)

- **1. A regrade after a restart raised twice.** `Unraised::expect` now skips a segment whose mismatch is pending
  already (it only fills in the entry's place, so the receipts bind) or raised; `rebuilt` also records the segments the
  log holds a raise of, so a segment graded again after its raise isn't raised again. Test
  `a_segment_graded_again_after_a_restart_raises_its_mismatch_once` (the reviewer's probe, plus a regrade after the
  raise). It also covers the pile of receipt-less pendings after a failed hand-over (item 3).
- **2. A refused raise stalls the part.** Default 1 is narrowed: only `Unavailable` fails the run. A `Refused` raise
  entry (or one that fails its own check) is kept readable in `Closing::refused_raises()` (order, segment, why), the
  mismatch is not tried again, and the part runs on. Test `a_raise_the_log_refuses_is_kept_readable_and_stops_nothing`.
- **Rule-offs:** the dedupe off, and the raised-skip off alone: `a_segment_graded_again_after_a_restart_raises_its_mismatch_once`;
  a refusal failing the run: `a_raise_the_log_refuses_is_kept_readable_and_stops_nothing`. None hung.

## T-020-23

Branch `f-020/t-020-23-reliance-writer`, from `m3` at `ea6c290c`. Edits `components/core-orch/` (`reliance/`, new;
`segments/mod.rs` where an operation's inputs are handed over; `segments/journal.rs`, two helpers made `pub(crate)`;
`lib.rs`) and `conformance/scenarios/tests/` (`reliance.rs`, new; the kit `segments_support/mod.rs`, additive). No
contract is changed: `What::Reliance` and its relation (`reliance-names-its-judges-statement`) were `T-021-07`'s.

- **Design.**
  1. *The register.* `reliance::Reliances` holds which of an order's inputs rest on which judgement
     (`Judgement { receipt, judge }`: the receipt of the stated entry holding it, and its maker) and the reliance
     entries written. Core reads a name and a value, never where a value came from, so declaring that an input rests on
     a judgement is the caller's (`Segments::rests_on(order, input, judgement)`).
  2. *The writer.* `Reliances::write(log, core, at, order, provider, judgements)` builds one `reliance` entry (Core's
     own, `outcome: observed`) per order, judgement and relying provider, once, and appends it unchanged however often
     it is called. The log, not Core, says whether the judgement is the judge's: it refuses a judgement that is no
     stated entry of the log, is not verified, was made by another than the judge named, or is about another order. Core
     asks it and sends nothing on a refusal.
  3. *The hook.* When a segment is handed over, the judgements its operation's inputs rest on are taken from the
     register and kept with the segment (`InFlight::relies`, beside `carries`, the names of the inputs). `advance`
     writes them first, before the segment's hold, question, effect and the operation, so a refusal leaves nothing of the
     segment recorded but policy's decision, and a retry (a segment resumed from where it stopped) finds them written
     and appends nothing again. A segment whose inputs rest on nothing writes nothing.
  4. *The refusal.* A reliance the log refuses or can't take comes back as `SegmentsError::Append`, as every other
     append of a segment does, so the callers that already map it (`stuck`, `recalc`) need no change; the segment stays
     handed over in Core's register and is asked again on the next run, which the log refuses again.
- **Defaults taken (reversible, failing closed).**
  - *Declared by the caller.* A declaration is made before the segment that carries the input is handed over:
    afterwards it is `Rests::TooLate`, since its reliance would come after the send. An input resting on one judgement
    and declared again on another is `Rests::Another`, not replaced; the same one again is a no-op.
  - *One entry per order, judgement and provider*, not per segment or input: two segments of one provider relying on one
    judgement leave one entry; another provider relying on it, or the same provider for another order, one each.
  - *Time.* The entry's time is Core's clock at its first try and doesn't change on a retry, so a retry is an equal
    entry the log keeps once.
  - *Judge and relier may be one party.* The log's row doesn't forbid it, so Core doesn't.
  - *Only what an operation carries.* An input no tuple of the route takes is handed to no one, so nothing relies on
    what rests on it (`required_inputs` stays the one filter).
- **For `T-024-12` (the verifier half at the assembly).** Drive it through `Segments`: after the order's route is
  chosen and before `run_part`, for each input that rests on a judgement call
  `core.segments.rests_on(&order, "<input name>", Judgement { receipt, judge })`, `receipt` being the log's receipt of the
  stated entry of the judgement and `judge` its maker's id (the verifier, or the reserve). Then run as usual. The
  `reliance` entries follow by themselves, one for each provider handed such an input. Standalone, without
  `Segments`, `Reliances::write` takes the same. The receipt of a verifier's judgement isn't handed back by the
  routing (`Offer::Offered` carries the reserve's yes only, in `reserve_yes`, and a judgement's `Answer::receipt` is
  private to `rounds`): the assembly reads it from the log, the stated entry of shape `JUDGEMENT_SHAPE` made by the
  verifier, with the receipt the log gave it (gap 1). A false judgement is the same: the stand-in's wrong yes is a
  stated entry like any other, and the reliance names its maker and the provider that relied on it.
- **Review round 1.** (1) The stuck mover (`stuck/mover.rs`) fills the tuple's required input with the destination's
  account and sends it with `carry_out`; it now consults `Segments::resting` when it writes its flight and, before the
  question and the send, writes the reliance through `Segments::write_reliance`: one the log refuses fails the move
  (nothing sent, `Resolved::Failed`), one it can't take now is tried again. (2) A declaration refused as `TooLate` is
  still stored, so later segments and the mover write their reliance; only the segment already sent goes without, and the
  caller is told. Tests: `stuck delivery of an input resting on a judgement leaves a reliance before the send` and
  `stuck delivery whose reliance the log refuses sends nothing` (`stuck_mover.rs`), the first of which declares late
  (`TooLate`) and relies on its being kept. Rule-offs: the mover's write switched off, and the late declaration not
  stored: both red on both tests. (4) The `REQ-ORD-28` / `REQ-CPL-12` residual below goes on the owner's form.
- **Review of Core's loss paths** (the row's Done when). Inspected: every place in `components/*/src` that names a cost,
  a bearer or a charge. Core writes no entry or call that debits, bills or charges the operator or a customer for a
  judgement's falsity: a `reliance` names the judge and the relying provider and nothing else, and no other path reads
  it. The cost allocations Core does carry are `REQ-ORD-28` (the operator resolves and bears a *stuck* order, in
  `core-authz`'s `stuck`, unrelated to judgements), `REQ-ORD-30` (an undone start's loss is the origin reserve's,
  `reversal`), and the charge check (`REQ-QTE-5`, a raise to the operator that moves nothing). Residual, a decision of
  the owner's and not a Core defect: where a false judgement is what leaves an order with no route, `REQ-ORD-28` still
  puts the stuck order on the operator, and the `reliance` entry is what shows the operator whom to recover from
  under contract (outside this document, `REQ-CPL-12`); Core doesn't tell such a stuck order from another.
- **Gaps, with owners.**
  1. The verifier's judgement receipt isn't returned by the routing (above). Owner: `T-024-12` reads it from the log;
     a field on `Routed` is a small row of `F-020` if a second caller wants it.
  2. The register of what rests on what is in memory, like the flights it serves: a restarted Core declares again and
     appends an equal entry only if its clock reading is the same, which it isn't. Owner: `F-030` (restart), which
     rebuilds `Segments` as a whole.
  3. Hand-overs this row does not cover: a routing question's dry run (`rounds/ask.rs::inputs_for` fills the inputs a
     tuple requires into every question), a reversal's `carry_out` (`reversal/`, which asks the failed provider to
     reverse), and a hand-on's start. None of them carries a judgement-backed input today, so no reliance is written
     there. Owner: `F-053` (`M5`, `DEC-16`), with the production verifier, if a tuple's input comes to rest on a
     verifier's word on those paths. Covered after review: the stuck mover (below).
  4. The real log's row is proved here on the finished history (`Core::admitted`); the scenario log (`TestLog`) refuses
     a reliance only by its guard (`guards_reliance`: the cited entry is a stated entry claimed by the judge). The
     attribution part, a statement the log attributes to no one, is the contract's `--verified` fixture.
- **Histories the log refused.** None in this row's tests. One kit note: `results_after_the_move` appends a question
  and two results under a transaction; the judge's result is the judgement, about the order through that transaction,
  which the relation's `about` row takes as agreeing with the relying order.
- **Rule-off table**, each once, file restored, run under a timeout, none hung:

  | Rule switched off | Red |
  |---|---|
  | `advance` never writes the reliance | the four scenario tests |
  | the reliance written only once the operation was carried (after the send) | the four scenario tests |
  | the log's refusal of the entry swallowed (the segment goes on) | `reliance on a judgement the log did not verify as its judge's is not written`; `reliance is written before the operation that relies on it is sent` |
  | the writer keeps no entry (each call builds and offers anew) | `one judgement relied on by one provider is written once`; unit `a retry is the same entry at the same time` |
  | an input may be declared on another judgement (`Rests::Another` off) | `one judgement relied on by one provider is written once`; unit `inputs name each judgement they rest on once` |
  | a declaration after the send accepted (`Rests::TooLate` off) | `one judgement relied on by one provider is written once` |
  | the entry names the provider as judge | the four scenario tests |
  | every judgement declared for the order relied on, whatever the inputs carried | `provider relying on a reserve's judgement leaves a reliance entry naming both` |

## T-020-20

Branch `f-020/t-020-20-stuck-report-correction`, from `m3` at `ea6c290c`. Edits `components/core-orch` `stuck/`
(`mod.rs`, `mover.rs`) and, as the row's seam, `segments/mod.rs`; the kit (`segments_support`: `on_settling`,
`Core::late_statement`, `PermitsEveryStep`'s decline) and four test files. No contract, manifest or `DEC-20` path.

- **A decline, and a new report after it** (`Stuck::decline_outcome`, `Stuck::decline_segment_outcome`, over
  `OutcomeDeclines`, `T-021-09`). The decliner is of another party and login than the reporter's (`two_actors`: the
  mover cannot withdraw its own report; policy holds the chain and the customer and holder checks). It names the
  report (outcome, version, evidence), and its own evidence, another statement following the move. A permitted decline
  settles and frees nothing: the report no longer stands, the part stays held (`OutcomeUnknown`, `ValueInMotion`), and
  a new report is admitted, one version beyond every one made, which a confirmation names (`ConfirmingSegment` gains
  `report`). A confirmation or a decline of a declined version, and the declined report made again (same outcome and
  evidence), are `ResolveError::ReportDeclined`. `Moved::Declined { report }`.
- **A refusal is not a crash.** A decline or a confirmation the log's rules refuse (a report settled or declined)
  comes back from policy as `Err(AppendFailure::Refused)`, and `Stuck` returns it as `ResolveError::Append(Refused)` before
  it has changed anything: the report still stands and is declined or confirmed once the log takes it.
- **The evidence states the outcome claimed** (`T-020-16`'s gap 5). `known_outcome`, `report_segment_outcome`,
  `Confirming.evidence` and `ConfirmingSegment.evidence` take a `Stated` (the receipt and what Core's own reading of
  the statement says of the move, `Delivering::statement`, `Segments::late_statement`), not a bare receipt. A report
  rests on a statement stating the outcome it reports (`EvidenceStatesAnother` for another, or for none: another maker's
  states none of the move's). A confirmation or a decline is held to the same only where the move's own provider made
  it; another maker's states none and backs the outcome the report named.
- **A segment's move confirmed done** (`Stuck::confirm_segment_done`, `Segments::late_statement`, `grade_late`,
  `confirmed_done`). The provider's late result is taken as the segments take a result (attributable, a result, the
  move's transaction, judged against the operation Core signed), kept once as its maker's stated entry about the
  order, the segment and the transaction, and is what a report names. Done is confirmed only through
  `confirm_segment_done`, which grades first (`Segments::grade_late` over `Grading`, nothing recorded), asks policy only
  on a result that grades, and then records the final `result` after the `unanswered` one, the entries it graded to
  and the segment's `segment_settled` (`settle_on`, the settlement the segments' `account` carries out too). A done
  that grades to nothing yet, or rests on no late result Core took as the segment's own, is
  `ResolveError::Ungraded(..)` and nothing is recorded. A confirmation policy permitted whose record the log couldn't
  take is recorded once: a retry records the outcome and asks policy nothing more (`segment_confirmed`, as `INV-8`).
  `report_segment_outcome` now takes a done too (it settles nothing); `confirm_segment_outcome` still takes failed only
  (`DoneNeedsEvidence`). `Settling` carries the ports the stuck orders' `Parts` doesn't hold.
- **A stuck resolution's evidence has an entry behind it** (`Resolutions::evidence_of`, `Stuck::resolve`). Where a
  `deliver` is done, `stuck_resolved`'s evidence id is the id of a `part_evidence` of the order and the part, grade 4,
  `whose` the provider the decision names, resting on the statement the done rests on: the provider's own answer, as
  the log holds it attributed (`Delivering::evidence_of`), or the report the confirmer confirmed. Appended before the
  input. `Delivering::statement` now refuses a statement the log recorded unattributed (`NotTaken::Unattributable`),
  as `hand_statement` did: the log refuses a `part_evidence` resting on one.
- **Tests** (written beside the code, then each rule switched off below). `tests/stuck_mover.rs`: the ten named by the
  row for a stuck resolution's move (`report declined by its confirmer is followed by a new report that stands until it is confirmed`,
  `second report with no decline between is refused`, `mover cannot withdraw its own report`,
  `confirmation of a declined report is refused`, `decline by the reporter's own party is refused`,
  `part stays held while its report is declined`, `report whose evidence states another outcome than the one reported
  is refused`, `stuck resolved cites an evidence entry the log holds`), plus
  `statement_the_log_recorded_unattributed_is_not_taken_as_evidence` and
  `decline_or_confirmation_the_log_refuses_is_a_refusal_that_changes_nothing`. `tests/stuck.rs`, for a segment's silent
  move: `declined_report_of_a_silent_move_is_followed_by_a_new_one_that_stands_until_it_is_confirmed`,
  `silent_move_reported_on_evidence_stating_another_outcome_is_refused`,
  `segments_move_confirmed_done_is_graded_and_then_settles_the_segment`,
  `segments_move_confirmed_done_with_no_graded_evidence_settles_nothing`,
  `segments_move_confirmed_done_whose_record_the_log_could_not_take_is_confirmed_once`. Each replays its history through
  `admit_entry`. The two earlier stuck tests' calls changed to pass a `Stated` (no behaviour), and
  `silent_move_on_a_stuck_part_...` no longer asserts that a report of done is refused (it is taken, and graded on
  its confirmation).
- **Rule-off** (each alone, through a temporary environment switch in the code, removed before the commit; the named
  tests of each file run; none hung; every one red):

  | Rule switched off | Red |
  |---|---|
  | a report rests on a statement stating its outcome (`states`) | `report_whose_evidence_states_another_outcome_than_the_one_reported_is_refused`, `silent_move_reported_on_evidence_stating_another_outcome_is_refused` |
  | a confirmation's evidence bears the report's outcome out (`bears_out`) | the same two |
  | the declined report is not made again (`made_again`) | `report_declined_by_its_confirmer_is_followed_by_a_new_report_that_stands_until_it_is_confirmed` |
  | the decline is of another party and login (`two_actors`, both arms) | `mover_cannot_withdraw_its_own_report`, `decline_by_the_reporters_own_party_is_refused`, `declined_report_of_a_silent_move_...` |
  | one report while it stands (`AlreadyReported`, both arms) | `second_report_with_no_decline_between_is_refused`, `declined_report_of_a_silent_move_...` |
  | a declined version is not confirmed (`ReportDeclined` on a confirmation, both arms) | `confirmation_of_a_declined_report_is_refused`, `declined_report_of_a_silent_move_...` (red on the error named; the refusal itself still holds: no report stands, or the version differs) |
  | a decline frees nothing (the part is released) | `part_stays_held_while_its_report_is_declined` |
  | the decline's evidence is its own (`on_its_own_statement`, segment arm) | `declined_report_of_a_silent_move_...` |
  | a done is graded before policy is asked | `segments_move_confirmed_done_with_no_graded_evidence_settles_nothing` |
  | a permitted confirmation is not asked again on a retry | `segments_move_confirmed_done_whose_record_the_log_could_not_take_is_confirmed_once` |
  | `stuck_resolved` cites a `part_evidence` | `stuck_resolved_cites_an_evidence_entry_the_log_holds` |
  | the evidence entry reaches the log before the input | the same |
  | a statement the log recorded unattributed is not taken | `statement_the_log_recorded_unattributed_is_not_taken_as_evidence` |

  Not run, each defence in depth or covered by an assertion of a test above only: `confirm_segment_outcome` refusing a done
  (`DoneNeedsEvidence`, asserted in the done test), `late_figures` taking only a late result judged settled,
  `evidence_of`'s attributed filter (the statement is refused unattributed before it, at `statement`).
- **Probed for paying twice** (none found). A failed outcome frees a part only on a confirmation by another party:
  a decline frees nothing, a new report counts one version more and still needs one, and a declined report is never
  confirmed; the mover can't decline its own. A done confirmed on a silent segment settles that segment, which the part's
  stuck record keeps stuck: a stuck part is handed over no more (`SegmentsError::Stuck`), so the late settlement hands
  nothing on; a part's final deposit confirmed done waits for its public record, since grading refuses a statement
  alone (`REQ-ORD-33`). A late statement tried again is one entry (kept by its bytes), a retried confirmation is one
  entry on the log, and a statement of another transaction, one forged or one a key never published signed is not taken.
- **Defaults** (reversible, each failing closed):
  - the evidence trust of the deliver arm is the caller's `Stated`, which only Core's own readers (`Delivering::statement`,
    `Segments::late_statement`) make; a done on a silent segment rests besides on the segments' own record of the late
    result, so a `Stated` made by hand can't settle one (`Ungraded(None)`);
  - a confirmer's evidence maker need not be a party to the order (the form's default, 2026-11-17): Core reads no maker;
  - a declined report is not made again as it stands (same outcome and evidence), beyond the row;
  - a late settlement records no settlement time and keeps the segment's hold (a stuck part's resolution releases it): a
    time the confirmation stretched says nothing of the provider's p99, and the hold over-reports rather than drops;
  - a `Resolutions` that names no evidence leaves `stuck_resolved` citing a bare id, as before;
  - a statement the log recorded unattributed is not taken as evidence on the deliver arm either (`hand_statement`'s
    rule), so no `part_evidence` rests on one.
- **Gaps, with owners.**
  1. `PolicySide` of the assembly doesn't forward `decline_outcome` yet (`T-021-09`'s gap 1; an `F-024` file the row
     doesn't edit): a decline can't be asked through the assembled Core; the tests use the kit's policy. Owner: the
     lead, with a row of `F-024`'s.
  2. The `Resolutions` stand-ins of `T-020-11`'s tests (`Moves`) name no evidence, so their `stuck_resolved`s cite a bare
     id, which `T-020-30` (1) will hold to an entry: they must name one then. Owner: `T-020-30`.
  3. After a segment's move is confirmed done the part's resting hold is not re-formed at the node the value went to
     (the segment's hold stays, as above); a hand-on or delivery of such a part reads the last settled segment. No
     test drives a resolution after a late settlement. Owner: the lead (a later `F-020` row after `T-020-28`).
  4. The assembly calls none of the new calls yet (`decline_outcome`, `confirm_segment_done`, `late_statement`): a
     `Core::` entry for each is `F-024`'s. Owner: the lead.
  5. The mover's and `Stuck`'s new state (declined reports, versions, `segment_confirmed`, late statements) is in memory
     (`F-030`).
  6. `contracts/core-internal/README.md`'s hand-off on `stuck-resolved-cites-its-parts-evidence` (a citation of none is
     admitted until `T-020-20` writes one) is now stale in its second half: the writer exists. A `DEC-20` path; the
     round that tightens the row (`T-020-30`) brings it up to date.

### T-020-20, the review of `258475fb`: changes

- **S1, fixed.** `segment_confirmed` held only (order, segment), so a retry by another confirmer skipped policy
  (`THR-70`). It now stores the confirmer, the report version and the evidence receipt with the permit, and a retry skips
  policy only on an exact match; any other is asked of policy. `probe_retry_by_another_confirmer_skips_policy` is in
  `segments_move_confirmed_done_whose_record_the_log_could_not_take_is_confirmed_once` (another confirmer, and bob with
  other evidence, are refused by policy; bob's own retry is not asked). The stuck-resolution path has no such bypass:
  once policy permits, `motion.resolved` is set, so no further confirmation is open to anyone
  (`confirmation_whose_result_the_log_could_not_take_is_not_open_to_another_confirmer`).
- **S2, fixed.** `Stated`'s fields are private (accessors `receipt()`, `outcome()`); only Core's readers build one. The
  kit builds its own through `Stated::for_tests`, behind the `test-support` feature, enabled by the scenarios'
  dev-dependency only. A `compile_fail` doctest shows outside code can't build one by hand.
- **Low 3, 4, added.** The late settlement keeps the hold (asserted in the done test); a segment decline after a permitted
  confirmation is refused (`NoDecision`, in the retry test).
- **Rule-off** (each alone, switched in code and removed): the permit bound to its confirmer, version and evidence: red on
  the retry test; the decline refused after a permit: red on the same test; `if !on.late` around the release: red on
  `segments_move_confirmed_done_is_graded_and_then_settles_the_segment`. The seal is held by the doctest.

## T-020-25

Branch `f-020/t-020-25-empty-shortlist-handed-over`, from `0eaffd68` (`m3` plus `T-020-20`). Edits `components/core-orch/`
(`segments/drain.rs`, `segments/mod.rs`, `stuck/mod.rs`: `seen_by_provider`) and `conformance/scenarios/` (`tests/recorded_ends.rs`,
new; `tests/segments_support/rerouting.rs`, one additive field; `tests/segments.rs`, one pattern). No contract is changed:
`HandedOver::OperationUnanswered` and the empty shortlist's fixtures were `T-020-21`'s.

- **Design.**
  1. *Empty shortlist: already built.* `rounds::round` has appended the shortlist's `decision_record` for every
     shortlist round since `T-020-06`'s third review (`Recorded::receipt` stopped being optional; `shortlist_that_keeps_nothing_is_recorded_and_asks_no_one`
     pins it in `core-orch`). The row's wording ("returned with `receipt: None`") described the code before that, so
     part (1) needed no writer. What was missing was a test on the running platform: `empty_shortlist_is_appended_and_the_orders_chain_grows`
     and `no_route_input_citing_an_empty_shortlist_is_admitted` recalculate the kit's order with every provider excluded,
     so the shortlist keeps nothing, and check the order's chain grows by that record (chained to the last, and the next
     round chains from it), that a `no_route` cites it, and that the real log's rows admit the history
     (`input-cites-its-decision-record`).
  2. *The hand-over item.* `drain::of_events` now emits, after the other items, one `HandedOver::OperationUnanswered
     {provider, transaction, ended_by}` per `Events::unknown`; `ended_by` is the refusal's label, an exhaustive match
     with a refusal prov adds later refused whole (the drain is kept, loud), as the other maps are. An events drain holding
     only such items is no longer empty. The stuck mover's drains use the same function, so its operations that end
     with no answer reach the log as well.
  3. *The citation.* `Segments` keeps the receipt of each such record by transaction, collected as drains are appended.
     `UnknownBy::Platform` became `{refusal, record: Option<Receipt>}`; `account` writes the `segment_outcome_unknown`
     input and the `unanswered` result only once the record is on the log, and the outcome carries its receipt.
     `Segments::platform_record(order, segment)` reads it, and `ProviderView::platform_record` (`stuck::seen_by_provider`)
     shows it to the segment's own provider, on the read policy already allows.
- **Defaults (reversible, failing closed).**
  - A platform record the log can't take yet holds the segment's unknown outcome back (`SegmentsError::Append(Unavailable)`,
    retried on the next run) rather than recording the outcome without its citation. A refusal ends the same way as any
    drain's.
  - `record` is `None` only where the platform reported no such record of the operation (an answer Core couldn't keep,
    `ports::carry_out`'s serialisation failure): there is nothing to cite, and the outcome is recorded as before.
  - The view gives the receipt, not the entry's content, and only to the provider that reads its own segment.
- **Gaps, with owners.**
  1. No row of core-internal ties the `segment_outcome_unknown` input to the record: the input has no field to cite
     with (`Observed::SegmentOutcomeUnknown {}`), so "cites" is Core's own outcome and the log's order (the record before
     the input), not a relation the log refuses on. Owner: the next contract round, `T-020-30`, if the owner wants the log to hold it.
  2. The fail-closed hold (a record the log can't take yet) has no test of its own: the kit's fault queue targets an
     append by kind, and none targets a `hand_over`. Owner: a follow-up of `F-020`, with the kit's `On` gaining the kind.
  3. `contracts/core-internal/README.md`'s hand-off for `T-020-25` still says "hands it over in place of carrying it only
     into the segment's outcome" (future tense); this row doesn't edit `contracts/`. Owner: the next contract round.
- **Histories the log refused.** None: every history is replayed through `Core::admitted`.
- **Rule-off table**, each once, file restored, run under a timeout, none hung:

  | Rule switched off | Red |
  |---|---|
  | an empty shortlist is not appended (the receipt of the one before is reused) | `empty shortlist is appended and the order's chain grows`; `no route input citing an empty shortlist is admitted` |
  | `of_events` emits no `OperationUnanswered` item | `operation that ended with no answer reaches the log as a handed over item`; `outcome unknown cites the platform's own record` |
  | the outcome does not carry the record's receipt (and the segment is then held back, failing closed) | the same two |
  | `seen_by_provider` shows no platform record | `outcome unknown cites the platform's own record` |

## T-020-27

Branch `f-020/t-020-27-late-status-reversal-party`, from `m3` at `f7c38c98`. Edits `components/core-orch/` (`reversal/`,
and in `segments/` one port, `Modules::status`, and one read, `Segments::statement_of`) and `tests/reversal.rs`. No
contract, manifest or `DEC-20` path.

- **Design.**
  1. *A late status asked* (`Reversals::ask_status`, `Modules::status`, `StatusAsked`). Once a segment's answer was
     taken (`NotAnswered` otherwise) and it hasn't settled or had its reversal completed (`StatusNotNeeded`), Core asks
     policy for `Action::AskStatus` (caller Core, as `ask` does), draws a fresh transaction from the segments' register,
     appends the `ask_status` effect before anything is sent, acts on it once (`act_on`), and asks the platform, which
     answers the provider's own result about the segment's transaction. The answer goes through `Reversals::later`, so it
     is kept as its maker's statement about the order, segment and the segment's own transaction (which is how the log
     verifies it), once by its bytes, and the effect's one result is recorded: done where an answer was kept, failed
     where the platform refused before the module passed anything out, unanswered otherwise. Nothing else is written: no
     input, no evidence, no hold released, the order as Core follows it unchanged (`INV-4`). A call that stopped carries
     on from where it stopped (`INV-8`).
  2. *A reversal at a later inner node* (`later_confirmer`, `Target.confirmer`). A segment that doesn't start where the
     withdrawal took the value (node and asset, as before) is confirmed by the provider of the segment that delivered
     the value there (what the log's `reversal-rests-on-its-evidence` reads as the node's `reserve`), where the node has
     a registered holding account and that provider isn't the failed one. A holding account's `reference` is an
     interface string (a private node's publisher interface, a public node's operator module), never a provider's id,
     so it names no party; it only gates the node. Core asks that party the same `reversal_confirmation` question; the judge holds the answer to its maker, and the evidence says its word
     (`Word::Reserves`, grade 3). Inside a private node the failed provider's own publisher holds, the publisher's word
     through `Reversals::ask` counts as before, labelled: its guard on the withdrawal's node is lifted, so `ask` is now
     taken at a later node too (its operation names the segment's from-node). With no such party `confirm` is refused
     `NoReserveAtNode` before anything is asked or recorded: the reversal stays owed (the assembly owes it from the
     failure) and held, for the stuck orders.
- **Defaults (reversible, each failing closed).**
  - The confirming party at a later node is the provider that delivered the value there, the reserve the log reads
    (`reversal-rests-on-its-evidence`: `whose` is the reserve at the from-node or a labelled publisher), gated on the
    node having a registered holding account and never the failed provider. The row's "holding account" is read as
    that node's reserve; the register's interface string names no one.
  - `Modules::status` has a default that refuses (`NotHandled`), so no platform that implements nothing is asked and
    every existing implementer compiles; the production `Platform<W>` leaves the default.
  - The ask is for a segment whose answer was taken and that hasn't settled or been reversed; it is not limited to a
    failed one, so an unknown outcome (silent provider) is covered.
  - The late status is asked under a fresh transaction of Core's own, passed to the platform; the answer stays about
    the segment's transaction, because the log verifies a result only under the transaction it names. The fresh
    transaction appears in the platform call and `StatusAsked`, not in a log entry (`What::Effect` has no field for it).
- **Gaps, with owners.**
  1. The production platform has no status query: `Platform<W>` doesn't reach a module's status (`RoleProvider::status` is
     the conformance kit's), so `Modules::status` answers nothing there. Owner: the lead, a platform row (`F-014`'s).
  2. No simulated route in the assembled world crosses two inner nodes (`sim-join-*` has one ledger), so these four tests
     run in `core-orch`'s fakes, as `T-020-12`'s do, over the router-api fixture's second candidate; "on the running
     platform with simulated providers" waits on a route of that shape in the sim. Owner: `T-024-14`'s sim work.
  3. Where the delivering provider is the failed one, no party confirms and the reversal waits for the stuck orders.
     Owner: ADR-0008's open point (who confirms at a node whose only reserve failed), a reading for the owner's form.
  4. A route can't run through an inner node with no holding account (`NoHoldingAccount`), so the gate on a registered
     holding account is never what refuses; the no-party test uses a route one provider delivers and fails
     (`Act::FailConversion` in the fake settles a provider's other operations).
  6. The fresh transaction of `ask_status` is in no log entry (`What::Effect` has no field for it). Held as a contract
     item by the coordinator.
  5. `ask_status` isn't called by anything yet: the owner of the stuck mover's watch (`T-024-06`'s scenario) calls it.
- **Histories the log refused.** None: the tests replay every entry through `attribute` and `admit_entry`
  (`World::admitted_from`), and the effect's result and the kept statement are admitted.
- **Red first.** The two status tests were written against a stub (`ask_status` answering `Unknown`) and failed on that
  assertion. The two later-node tests failed first on the route fixture they build (the record's validation), then, with
  the fixture right, on the first run of the implementation their guard was already in place; the rule-off below shows
  each rule's test goes red.
- **Rule-off table** (each once, file restored, `cargo test -p rutent-core-orch --test reversal` under a 120 s alarm; none
  hung):

  | Rule switched off | Red |
  |---|---|
  | the confirmer is the provider that delivered there | `failed segment at a later inner node is confirmed by that node's holding account` (nothing asked) |
  | the confirmer isn't the failed provider | `reversal at a later inner node with no confirming party stays owed and held` |
  | an answer counts only from the party asked | `failed segment at a later inner node is confirmed by that node's holding account`, `origin_reserves_statement_completes_a_reversal_as_its_word` |
  | the question goes to the confirmer, not the origin reserve | `failed segment at a later inner node is confirmed by that node's holding account` |
  | `confirm` refuses with no confirmer up front | none: `question` refuses the same case; kept as the first guard |
  | a late status only after the answer was taken | `late status of a segment is asked under a fresh transaction and kept as the provider's statement` |
  | not asked of a settled segment | the same |
  | the fresh transaction | the same |
  | the answer kept as the provider's statement | the same, and `late status moves nothing` |
  | the result says how it came out | `late status moves nothing` |
  | a late status releases no hold | `late status moves nothing` |

## T-020-30: `core-internal`'s round after `T-020-20`

Branch `f-020/t-020-30-core-internal-round`, from `0eaffd68` (`m3`'s tip with `T-020-20`). Edits `contracts/`
(core-internal, a `DEC-20` path: it waits for the owner's click at the release into `main`), and the consumer files
named below.

- **Versioning: folded into 1.0.0, no bump** (lesson 7): core-internal 1.0.0 and router-api 4.0.0 aren't on
  `develop`; the guard reads core-internal as a new contract. The changelog's last entry names each item by number,
  and `contracts/CHANGELOG.md` takes a line.
- **Item by item.**
  1. *A stuck resolution's evidence exists.* `stuck-resolved-cites-its-parts-evidence` is an exists row (fields
     `order`, `part`): a `part_evidence` with the cited id, of the input's order and part, before it. Cases:
     `--its-parts-evidence` admitted; `--missing` (a bare id, the row's test), `--a-segments-evidence`, `--order`,
     `--part` (was `--conflict`) refused. The admitted `stuck-resolved-follows-a-done-resolution--delivered` now holds
     its part's evidence. Core: `T-020-20` writes the entry where a `deliver`'s done rests on a statement it took; a
     `Resolutions` that names none leaves a bare id, which the log now refuses (stuck/'s docs say so; no code changed).
  2. *A report pair's tuple terms.* `ReportPair.terms: ReportTerms {increment?: PositiveAmount, reporting:
     ReportedAs (net | gross {charge})}`; `ReportPair::check` holds a gross charge to no more than the amount sent.
     `core-audit`'s `compare_reports` (an `F-022` file) takes the charge off a gross figure, raises nothing within one
     increment (strictly less), compares exactly with none, and names the compared (net) figure as `sent` in a
     difference. Orchestration fills the terms "where it builds the pair": **no orchestration path builds a pair**
     (no caller of `Comparisons::report_pair` in `core-orch` or the assembly), so `segments/` is untouched; a gap
     below.
  3. *A measure record's role.* `MeasureRecord.role: MeasuredRole (provider | verifier)`, required. `core-audit`'s
     `measures::compute` (an `F-022` file) keeps each role apart: `Report.providers` and a new `Report.verifiers`.
     Orchestration has no measure caller yet (`T-020-31` sets it).
  4. *A replay's receipts.* `What::Replay.rests_on: Vec<Receipt>`: the exit's result first, then at most one
     `inner-segments` record per node replayed that recorded something (Rust holds the count, the schema `minItems`
     and `uniqueItems`). `replay-rests-on-its-exit` reads the first; a new closing row,
     `replay-cites-its-nodes-records`, holds each after it to a verified `inner-segments` statement about the order.
     The node binding (`private_node_of` in the log, `Reversals::follow` in `core-orch`'s `reversal/`) reads the
     first receipt only, as before. `core-orch`'s closing writes a one-item list.
  5. *A deliver's report on its provider's own word.* `holders_own` reads `Resolution::Deliver { by }` as it read a
     hand-on's: a report's evidence is the moving provider's statement; confirmations and declines unchanged. The
     world's report now cites bank-co's own result (176) and its confirmation and decline us-bank's (175), swapped in
     every case that cites them, so each case keeps what it breaks.
  6. *A verifier's grant of a class* (added by the coordinator from `T-024-12`'s gap 1). `Action::ChangeCatalogue`
     gains `class: Option<Id>`: a `grant` or `revoke_grant` names a class in a tuple's place, never both, and no other
     change names one. `granted_advertising_keys` reads class grants; `attribute` verifies a statement of a maker
     whose grants in force are all of classes only where it is a judgement of one of them (else unverified), and
     refuses such a maker's statement claimed in any role but the verifier's. `core-audit`'s key register
     (`log/keys.rs`, an `F-022` file) keys a class grant apart from every tuple, so the real log trusts the verifier's
     advertisement from it. `T-024-12`'s placeholder tuple (`verifier_tuple`, in `conformance/scenarios/`, its files)
     is untouched and can move to the class grant once both merge.
- **Defaults** (reversible, each failing closed):
  - fold into 1.0.0 (above);
  - a pair's increment is in the reports' unit; a caller restates the tuple's origin-unit increment into it,
    rounding down; a gross pair carries its charge (`0` for a conversion); the receiving party's convention isn't
    read, since its `amount_in` holds the charge whatever its tuple declares;
  - a difference's `sent` is the compared figure, net of the charge; the pair's record keeps both as stated;
  - `role` is required and the caller's word: the log can't tell a provider from a verifier;
  - a replay's node records are not yet required one per node, and a node stays bound by the exit's result;
  - a class grant's maker is a verifier alone only while every grant of it in force is of a class; one granted a
    tuple too is attributed as before (any statement, provider or verifier role);
  - the class grant needs no advertisement check in the log (which classes a verifier may be granted is policy's
    and the catalogue's).
- **Consumer lines** (to compile or to keep what they held): `components/core-orch/src/closing/mod.rs` (the
  one-item list), `reversal/mod.rs` (the first receipt), `stuck/mod.rs` (docs), `segments/mod.rs` (`class: None` in
  the one catalogue change Core builds; `T-020-25` edits the same file); `components/core-orch/tests/closing.rs`;
  `components/core-authz/src/engine/mod.rs` and `roles/tests.rs` (`class: None`); `components/core-audit/src/
  compare/report.rs`, `measures/compute.rs`, `log/keys.rs`, and its tests `compare.rs` (two tests), `measures.rs`
  (the record helper's role, the field list, a test), `log.rs` (a replay's list, a test);
  `conformance/scenarios/src/assembly/mod.rs` (`class: None` twice; `T-024-12` edits the same literal),
  `tests/bulk_measures.rs` (role), `tests/segments_support/{mod.rs,resolving.rs}` (`class: None`; the kit's store is
  shared; `answered_under` split out of `stated_results_asking`; `Moves::stating`), `tests/stuck.rs`,
  `tests/stuck_mover.rs`, `tests/person_records.rs` (the shared store, `Moves::stating`).
- **Histories the log refused, sorted.** All test artefacts, no Core bug:
  - ten of `T-020-11`'s stuck tests (`conformance/scenarios/tests/stuck.rs`) recorded a `stuck_resolved` citing a
    bare id: their `Moves` stand-in named no evidence (`T-020-20`'s gap 2, routed here). `Moves::stating` now states
    the moving provider's word (the operator's question and its result through the resolution's transaction, signed
    and kept) for each `deliver` it answers done, and `evidence_of` names it, as the production mover does;
  - the contract's own cases that the new rows reached otherwise than they meant (each patched in place, its
    expectation unchanged): the world's report (the swap above), three planless-part cases (the resolution now
    delivered by us-bank, where a planless part's value sits), two timing cases and `--evidence-by-the-same-maker`
    (their own statement now bank-co's), and `stuck-resolved-follows-a-done-resolution--delivered`.
- **Order of work.** Fixtures (`scratchpad/f020zd/gen.py`, `gen2.py`) and the named tests first; with the shapes in
  and no rule, the contract's tests went red on assertions (the bare id admitted, Rust accepting the charge above
  what was sent and the replay's extra receipts, no row for the replay's records). Item 6's test was written after its
  code, and the rule-offs below are its check, as are `core-audit`'s.
- **Gaps, with owners.**
  1. No orchestration path builds a `ReportPair` (`T-022-05`'s gap 6: no caller of `Comparisons`), so nothing fills
     the terms. Owner: the lead (a row of `F-020`'s that calls `Comparisons::report_pair` from `segments/`).
  2. A replay's node records aren't required one per node that recorded something, and the log can't read which node
     a record is of. Owner: `T-020-32` (writes them; may tighten the row after, as this round did for item 1).
  3. A `move_value`'s report is held to its segment's transaction, not by maker. Owner: the lead, if a review finds
     a path; none found here.
  4. A party granted both a tuple and a class is attributed as a provider for everything. Residual for `T-025-05`.
  5. `T-024-12`'s placeholder tuple moves to the class grant once both merge. Owner: `T-024-12`'s author or the lead.
  6. The scenario suite beyond the binaries run here (the kit changed: shared store, `Moves`) runs in CI.
- **Rule-off table** (each switched off alone from `bc89ce68`, its tests run under a 600 s alarm through the lock, the
  file restored with `git checkout`; script `scratchpad/f020zd/ruleoff.py`; none hung, every one red):

  | Rule switched off | Red |
  |---|---|
  | `stuck-resolved-cites-its-parts-evidence`'s `order` | `stuck_resolved_citing_an_evidence_id_the_log_holds_no_entry_for_is_refused`, the citation table's |
  | its `part` | the same two, `stuck_resolved_citing_an_evidence_entry_that_names_another_part_is_refused` |
  | the row whole (cites nothing) | the same three |
  | `ReportPair::check`'s gross charge no more than sent | `report_pair_without_its_tuples_increment_and_convention_is_invalid`, the two fixture tests |
  | `What::Replay`'s receipts listed (at least one, each once) | `replays_receipts_that_name_no_record_for_a_node_it_replayed_are_invalid`, the two fixture tests |
  | its at most one record per node that recorded something | the same three |
  | `replay-cites-its-nodes-records` whole | `replays_receipts_that_name_no_record_for_a_node_it_replayed_are_invalid`, the citation table's |
  | its `verified`; its `record`; its `about` (each alone) | the same two, each time |
  | `outcome-confirmation-cites-verified-evidence`'s deliver arm of `maker` | `deliver_reported_on_an_unrelated_makers_statement_is_refused`, the citation table's |
  | `Action::check`'s class only on a grant or its revocation, never with a tuple | `verifiers_category_grant_naming_no_tuple_attributes_its_statements_for_that_category_only` |
  | `attribute`'s judgements of the granted class alone | the same |
  | `attribute`'s verifier's role alone for a class grant's maker | the same |
  | `granted_advertising_keys` reading a class grant | the same |
  | `core-audit` measures keyed by role | `measures_of_one_party_in_two_roles_are_kept_apart` |
  | `compare_reports` within one increment (exact instead) | `reports_differing_by_less_than_one_increment_raise_nothing`, `gross_and_net_reports_of_one_segment_are_compared_like_with_like` |
  | `compare_reports` taking a gross figure's charge off | `gross_and_net_reports_of_one_segment_are_compared_like_with_like` |
  | `core-audit`'s key register reading a class grant | `a_verifiers_class_grant_attributes_its_judgements_of_that_class_alone` |
- **Local checks** (one lock hold, at `bc89ce68`): `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  -- -D warnings`, the contracts' `no_std` build (`riscv32imac-unknown-none-elf`), `cargo test -p
  rutent-contract-guard`, the guard against `origin/develop` ("1605 changed path(s), no violations"), plan-check and
  tag-check `--strict`, `cargo test` of `rutent-contracts`, `rutent-core-audit`, `rutent-core-authz`,
  `rutent-core-orch` and `rutent-scenarios`, and `cargo doc -D warnings` of the five: all green.

### T-020-30, the review of `a2cde05d`: changes

- **Blocker, fixed: a revocation widened what a verifier's keys attribute.** `classes_alone` read the grants in force
  when a statement was appended, and with none left it answered "as before"; `core-audit`'s key register trusts a key
  where a grant was in force at the statement's own `made_at`, which the maker chooses. So a statement backdated into
  a revoked class grant's window (another class's judgement, a provider's `result`, a judgement claimed as a
  provider) came out verified, and a class-only maker later granted a tuple had its earlier statements attributed
  with no class limit. `classes_alone` now reads the grants as they stood at the claim's `when.at` (each tuple's or
  class's latest change at or before it by its effect's time, done, as the register reads them), and a maker that has
  held a class grant and has none in force then attributes nothing. Cases added, seen red on the reviewer's probe
  before the fix: in `verifiers_category_grant_naming_no_tuple_attributes_its_statements_for_that_category_only`
  (revoked then backdated, made after the revocation, class then tuple) and in `core-audit`'s new
  `a_verifiers_class_grant_is_read_at_its_statements_own_time` (the same on the real log).
- **Low, fixed:** `holders_own`'s doc names the deliver arm.
- **Low, noted for `T-020-32`** (gap 2 above, widened): `replay-cites-its-nodes-records` admits an `inner-segments`
  record by any maker; holding it to the node's publisher's maker is `T-020-32`'s with the records it writes.

### T-020-30, the re-review of `c9941d98`

- **Merged `m3` at `f7c38c98`** (`T-024-12`, `T-024-17`, `T-020-25`, `T-024-15`) with a plain signed merge.
  `conformance/scenarios/src/assembly/mod.rs`: `T-024-12`'s rewritten grant literal kept, with `class: None`
  (its placeholder tuple untouched); `assembly/holding.rs` (`T-024-17`'s release acceptance) takes `class: None`;
  these notes keep both sections, `m3`'s first.
- **Low, fixed:** `core-audit`'s `Register::admits` looked up a change's last date by tuple alone, so a class's grant
  or revocation dated before that class's last change was admitted where a tuple's is refused
  (`CHANGE_BEFORE_ITS_TUPLES_LAST`). It now looks it up by `granted(tuple, class)`, as the write side keys it. Test
  `a_class_grants_changes_are_dated_in_the_order_the_log_records_them` (`core-audit`'s `tests/log.rs`): red before the
  fix on the reviewer's probe (a revocation dated the 4th behind the grant of the 5th admitted).

## T-020-28

Run in wave 27 after `T-020-19`, `T-020-23`, `T-020-24`, `T-020-25` and `T-020-27`, each merged first; `T-020-31`,
`T-020-33`, `T-020-34` and `T-020-35` edit these modules after it.

- **What is built.** `components/core-orch/src/relay.rs`, the one place `relayed_by` is set: `relayed_by(maker,
  presented_by)` answers the presenter where it isn't the claimed maker (compared by `acts`, as `core-audit`'s
  `Statements` does) and nothing where the maker presented its own statement. `claimed_by` reads the maker an envelope
  names; `presenter` makes the actor of a provider (or verifier) that handed Core a statement. Each of the five writers
  (`snapshot/statements.rs::record_refused`, `reversal/journal.rs::stated`, `segments/journal.rs::stated_entry`,
  `arrival/journal.rs::stated`, `rounds/journal.rs::append_stated`) takes `presented_by` as a required argument and calls
  the helper before building the entry; none writes `relayed_by: None`, and `tests/relay_helper.rs` scans `src/` so a
  sixth site that builds a `StatedEntry` without the helper, or writes `None`, fails.
- **Who presented, per caller.** Core's own questions: the operator, who is also their maker. A statement a provider
  hands back to a call Core made to it (a segment's result, a stuck move's statement or late status, a reversal's
  answer, a reserve's answer to a balance question, a round's answers): the provider asked, whatever the envelope
  names as its maker. The round's answers and the reversal's and arrival's answers now claim the maker the envelope
  names (the log verifies a mismatch between a claim and the kept bytes, so this is the honest claim); where the
  envelope names none Core can read, the provider asked. A snapshot's refused statement: the answer's provider
  (`record_refused` takes the presenter; rounds passes the answer's provider, or the envelope's maker where no answer
  is found). `Terms::take` and the hold's take: a new required `presented_by` on `TermsDesk` and `HoldDesk`.
- **Defaults taken (reversible).** (1) `Reversals::report` and `later` take `presented_by` from their caller (review
  of `04f0756c`): a default of the target provider would record "no relay" when that provider is the maker and another
  party handed the statement over, the loss this row closes. The one internal caller of `later` (the late status Core
  asks a provider for) passes that provider, who answered the platform call. (2) `show_fault` and an `afterwards` statement are accepted only from the party named
  (`NotTheProvider`, `makers`), so their presenter is that maker and no relay can be named there. (3) The scenario
  harness hands over each provider's own terms or hold, so its desks name the envelope's maker as the presenter
  (`assembly::handed_over_by`, in `conformance/scenarios`, an `F-024` file).
- **Gaps, with owners.**
  1. `T-024-02`'s assembly doesn't route Core's statements through `core-audit`'s `Statements`: Core appends through
     orchestration's `Log` handle (`GeneralHandle`), and `Statements` is the intake of an outside presenter with its own
     `Evidence`/`Log`; routing would put a second writer between Core and the log. The five sites set the field as
     `Statements` does instead. Owner: the lead, if the owner wants one writer.
  2. `Arrivals::arrive` takes only a notice its reporting reserve made (`read_notice` requires the envelope's maker to
     be `report.reserve`), so no relayed notice reaches that site by that door; the named test drives the arrival
     site through the reserve's answer, where a relay does arrive. Owner: `T-020-19`'s notice read, if a relayed
     notice should ever be taken.
  3. `show_fault` and the `afterwards` statements still name no presenter: they are accepted only from the party
     named, so no relay is possible there. `report`'s and `later`'s external callers (none outside `core-orch`'s
     tests today) must supply the authenticated presenter: the owner of the route that delivers a provider's late or
     unasked statement (`T-024-06`/`T-024-14`'s assembly work). `T-020-12`'s tests changed only to pass the maker as
     presenter to those two calls.
  5. `rounds` passes the envelope's maker to `record_refused` where no answer is found for a refused statement; every
     refused statement comes from an answer Core holds, so that case does not arise today.
  4. `conformance/scenarios` (`assembly/mod.rs`, `assembly/charges.rs`) changed by the new desk field only.
- **Histories the log refused.** None: the reversal, rounds and arrival tests replay the log through `attribute` and
  `admit_entry`, and admit the relayed entries.
- **Red first.** The six tests were written with the sites; each goes red with the helper switched off (R1 below). I
  wrote the code before the tests, so the red was seen by rule-off and not before the code.
- **Rule-off table** (each once, file restored; test binaries `rounds`, `reversal`, `segments`, `arrival`,
  `snapshot_statements`, `relay_helper` under a 500 s alarm; none hung):

  | Rule switched off | Red |
  |---|---|
  | R1: the helper names no relay | all five relay tests: `quote`, `segment result`, `arrival notice`, `reversal statement`, `statement the snapshot takes` |
  | R2: rounds claims the provider asked, not the envelope's maker | `quote restated by a relay is recorded naming the relay` |
  | R3: reversal's answer claims the presenter, not the envelope's maker | `reversal statement restated by a relay is recorded naming the relay` |
  | R4: a segment's result is handed over by its claimed maker | `segment result restated by a relay is recorded naming the relay` |
  | R5: a writer writes `relayed_by: None` | `arrival notice restated by a relay is recorded naming the relay`, `every stated entry core builds takes its relay from the one helper` |


## T-020-37: Core carries out an erasure

Branch `f-020/t-020-37-erasure`, from `m3` at `b1771c2b`. Edits a new module `components/core-orch/src/erasure.rs`
(`pub mod erasure`), the assembly's wiring (`conformance/scenarios/src/assembly/erasure.rs`, one `mod` line, one
register line) and two scenario files. Nothing in `contracts/`, `core-audit/` or `core-authz/` changed.

- **The order, each step once** (`erasure::erase_statement`): (1) the store's `erasable` check, before anything is asked
  of policy, so no erasure is decided that the store would refuse (`NotErasable`, nothing recorded); (2) policy's
  `decide` of `erase_statement`, the caller the operator's login; no permit ends it (`NotPermitted`: no effect, nothing
  erased); (3) the effect, recorded before it takes place (`INV-7`); (4) `erasable` again, since a citation may have
  landed since (1): a store that now refuses gets the effect's result `failed` and the statement stays (`Failed`);
  (5) the effect's `done` result, which is what `BoundStore::erase` takes and reads the decision back through; (6) the
  store's erasure, bytes and salt gone, commitment and reference staying; (7) the `erasure` entry, by Core, `done`.
  Core reaches the store through a port, `erasure::Statements` (`erasable`, `erase`), which the assembly implements over
  the bound store: `core-orch` never imports `core-audit` (`DEC-2`).
- **After the done result the erasure can't be called a failure.** The log counts a statement erased from its done
  result (`nothing-cites-an-erased-statement`) and refuses an `erasure` entry that says `failed` after one
  (`erasure-is-carried-out-on-its-decision`). So a store unreachable now, or an `erasure` entry the log can't take, gives
  `NotErased::Unsettled`, which `Unsettled::settle` finishes: the store's erase of one erased already is `Ok`, the entry
  is appended after. Until then `BoundStore::unsettled_erasures` reports it (`T-022-03`).
- **Defaults, each reversible and failing closed.**
  1. The effect's `who` and the `erasure` entry's are Core's own actor, as a promotion's effect is (`T-020-08`); the
     decision's is the caller's, as the log requires (`a decision is recorded for its request's caller`).
  2. A failed erase is recorded as the effect's result `failed`, only where the store refuses before the done result.
     The log has no record for a failure after it, so there is none; it is a finding for the checker.
  3. Erasing what is erased already, asked a second time, is decided and recorded again (the store's `erasable` says
     yes to an erased statement); no rule says it shouldn't. Not held by a test.
  4. The assembly's register lists `Permission::EraseStatement` for the operator's login (`kit::operator_login`); a
     deployment lists its own (`T-021-10`, gap 1).
  5. The `reason` is a label (`retention` here); nothing branches on it.
- **Tests.** `components/core-orch/tests/erasure.rs`, over a log that admits every entry through the real `admit_entry`:
  `erasure without a permitting decision erases nothing` [`DEC-18`]; `erase that fails leaves the statement and records
  the failure` [`REQ-AUD-14`], as the plan tags it; and three more: the permitted path in order, a statement the store
  would refuse is never decided, and an erasure the store can't finish now settles later. The assembled Core:
  `erasure leaves the reference and history checks` [`AC-36`, `REQ-AUD-13`] in `conformance/scenarios/tests/audit_scenarios.rs`,
  **an `F-024` file** (`T-024-07`'s). The outside checker, handed bytes, finds nothing before the erasure, after it
  against the checkpoints published before, and after a later one; the history up to the erasure is byte for byte what it
  was, the statement's entry is still in it, the four new entries are the decision, the effect, its done result and the
  `erasure` entry naming the commitment, and the store's `missing`, `refused_erasures` and `unsettled_erasures` are
  empty. `T-024-07`'s `erasure_is_refused_in_the_assembled_core_until_policy_has_a_rule_for_it`, which pinned today's
  refusal, is rewritten for the fixed rules as `erasure_without_a_permitting_decision_is_refused_in_the_assembled_core`
  [`DEC-18`]: a caller the register doesn't list is refused (only the refusal is recorded; the store refuses to erase on
  its receipt) and a statement an entry rests on is refused before policy is asked. The review of `T-021-10`'s low, in
  `authz_scenarios.rs`: `operator agent is refused an erasure without the erase scope or when its login is unlisted`
  [`REQ-AUZ-8`, `DEC-18`], with the listed, scoped, permitted agent as its control.
- **Red first.** The module and its tests were written together and the tests passed on the first full run, so each rule
  is shown by its rule-off below instead; the log's own refusal (a decision's actor is its caller) was the one history it
  refused, a test artefact in the policy stand-in, fixed there.
- **Gaps, with owners.**
  1. `T-024-07`'s gap 1 is closed. The evidence store's findings (`missing`, `refused_erasures`, `unsettled_erasures`)
     still don't reach the outside checker: `F-033` with the lead (unchanged).
  2. A crashed erasure between the done result and the `erasure` entry is settled by the caller calling
     `Unsettled::settle`, which Core keeps nowhere: after a restart, binding the store reports it and the operator asks
     again. A durable resume is `F-030`'s (production assembly), with the rest of what Core keeps in memory.
  3. Nothing here calls `erase_statement` from an operator channel: the operator's surface is `F-030`'s.
  4. The login-unlisted half of the agent test is held by the engine's actors source (`T-021-05`'s rows); it is not
     switched off here on its own (see the table).

**Rule-off table** (the file restored each time; the core-orch ones under `perl -e 'alarm 120'`, none hung):

| Rule switched off | Red |
|---|---|
| `erasable` before the decision (`erasure.rs`) | `erasure_of_what_the_store_would_refuse_is_not_even_decided`, `erase_that_fails_leaves_the_statement_and_records_the_failure` |
| the permit required (a permit minted from the answer) | `erasure_without_a_permitting_decision_erases_nothing` (the log refuses the effect) |
| `erasable` again after the effect | `erase_that_fails_leaves_the_statement_and_records_the_failure` |
| the `erasure` entry not appended | `a_permitted_erasure_is_decided_recorded_carried_out_and_recorded_again`, `an_erasure_the_store_cannot_finish_now_settles_later` |
| the store asked on the effect's receipt, not the done result's | the same two |
| the store tried once, not `APPEND_TRIES` times | `an_erasure_the_store_cannot_finish_now_settles_later` |
| the assembly's register without `Permission::EraseStatement` for the operator's login | `erasure_leaves_the_reference_and_history_checks` |
| `core-authz` `roles`: no permission asked of the erasing caller | `erasure_without_a_permitting_decision_is_refused_in_the_assembled_core` |
| `core-authz` `parties`: an agent's scope refusal read as yes | `operator_agent_is_refused_an_erasure_without_the_erase_scope_or_when_its_login_is_unlisted` (and `unreachable_verifier_gives_a_refusal_and_nothing_moves`, which also meets it) |

## T-020-31: orchestration calls `Measures::measure`

Branch `f-020/t-020-31-measures-called`, from `81be3583` (`m3` with `T-020-27`, `T-020-28` and `T-020-30`). Edits
`components/core-orch/src/segments/` (a new `measured.rs`, and the measure calls in `mod.rs`: its hooks only),
`reversal/mod.rs`, and `conformance/scenarios/src/assembly/measures.rs` (an `F-024` file: the commit body says so).
`contracts/` is untouched. `T-020-28`'s one relay helper is untouched: no stated entry is written here.

- **Design.** A measure is a detection (`ADR-0009`), so it is built where the ending happens and written apart from it.
  `Segments` keeps a queue of `MeasureRecord`s, each built once, with the time of its ending by Core's clock;
  `Segments::write_measures(&mut dyn Measures) -> usize` is orchestration's one call of `Measures::measure`, oldest
  first. A record the log can't take now is tried up to `APPEND_TRIES` times and then waits, with those after it, for the
  caller's next call ("retried by the caller as any other append"); one the log refuses is dropped and counted
  (`measures_refused`). `unmeasured()` reads what waits. The hooks build the record from what the segment holds
  (`measure_end`, infallible: a record that can't be built is none), so the segment's outcome, its entries and what it
  moved do not depend on the writer or its answer.
- **What each ending gives**, for the provider the segment ran at, role `provider`: settled, a quote (the operation's
  `amount_out` against what the segment gave out, in the operation's asset); failed or a withdrawal cancelled, a
  failure with `market` absent; refused (final or until a known time), a refusal. For the answers the operation was
  evaluated with: each dry run of its own provider, a `dry_run` against how the segment ended and an `answer`, role
  `provider`; each verifier's judgement the operation cites, an `answer` by the judgement's maker, role `verifier`.
  Each answer counts once for its order, whichever segments cite it. A completed reversal gives a `reversal` for the
  provider whose segment was reversed (`reversal/` records it in `complete`, the one place a reversal completes).
  The endings hooked are the run's (`advance`), `confirmed_done` and `confirmed_failed`.
- **Cites.** A segment's records cite its effect and its result as the provider stated it (a late result, for
  `confirmed_done`; the failed `result` entry, for `confirmed_failed`); a reversal's cite the confirmation and the input
  that recorded it.
- **The assembly's one handle.** `Core::write_measures()` (`assembly/measures.rs`) hands `Segments` the writer
  `Core::measure_writer` already gives. It is not called by the assembly's own run, so `T-022-06`'s counting test,
  which writes its own records, still counts what it writes and no more (a call from `run` would double them: the
  entry's time differs, so the log would not dedupe).
- **Defaults taken** (reversible, each failing closed).
  1. A segment the platform wouldn't begin (`NotHandedOver`) and every outcome that isn't final measure nothing: neither
     says anything of the provider's conduct.
  2. A refusal counts as a segment that ended failed, for its dry runs; a settled one as done.
  3. Only `dry_run` figures of the operation's own provider count as dry runs; a fresh quote is a quote, which the
     route's price already measures.
  4. The writer is called by the assembly's caller, not by `run_part`: orchestration holds the records and the caller
     writes them, so no `Parts` of any caller changed.
  5. A record the log refuses is dropped, never retried; one it can't take now is kept without bound until written.
- **Gaps, with owners.**
  1. The queue is in `Segments`, which the production assembly keeps durably (`F-030`): a record queued and not yet
     written when it is lost isn't rebuilt from the log. Owner: `F-030`'s durable store. The production assembly also
     owns calling `write_measures` after each of its entry points that can end a segment or complete a reversal; the
     scenario assembly calls it from the tests that read measures back.
  2. Closed in the review round: `Live.record_entry` (the receipt of the log's `decision_record` entry, which holds the
     dry runs and judgements the route was evaluated with whole) is kept in the flight, and the `dry_run` and `answer`
     records cite it besides the segment's own entries. The assembly finds it in the log (`Core::record_entry_of`, in
     `assembly/mod.rs`, an `F-024` file); a caller that passes `None` gets records citing the segment's entries alone.
     The log holds no stated entry of a dry run or judgement tied to the order's segment through the record, so the
     record entry is the one that carries them. Rule-off: the answers' extra cite dropped turns the unit test and the
     bulk scenario red. Lows noted: the end-to-end scenario produces no verifier record (the verifier route is
     `verifier.rs`'s), and the dedupe sets grow without bound (gap 1).
  3. `failure.market` stays absent, as `T-022-06` left it: no part holds a market figure in `M3`.
- **Histories the log refused.** None: the scenario tests read the records back through the real log, which admits each
  (`measure-cites-entries-of-the-log`).
- **Red first.** The four scenario tests (`conformance/scenarios/tests/measures_called.rs`) were written against a
  no-op `write_measures` and failed on their assertions; the fifth (`measured.rs`' unit test) after the code, its fixture
  needing the contract's rule that an operation cites every usable judgement about its tuple.
- **Rule-off table** (each once, file restored; none could hang, so no alarm):

  | Rule switched off | Red |
  |---|---|
  | R1: a verifier's answer is measured in the provider's role | `a verifier's answer and the same party's quote as provider are measured apart` |
  | R2: an answer counts once per order | the same test (its second segment) |
  | R3: the queue drops a record the log can't take now | `a measure that cannot be written leaves the segment's outcome unchanged` (the retry) |
  | R4: a failed segment gives no record | `bulk measures over assembled runs ...` |
  | R5: a completed reversal queues no record | `completed reversal is measured` |
  | R6: the run's ending queues no record (the caller's hook) | `segment ending leaves no measure record when its writer is not called`, the bulk test and the unwritable one |

## T-020-34

Branch `f-020/t-020-34-verifier-no`, from `m3` at `81be3583`. Edits `components/core-orch/src/rounds/mod.rs` only.

- **Finding, as the row states it, did not reproduce through `route`.** `judged` indeed never read a judgement's answer,
  but `rounds::route` runs `DecisionRecord::check` on every round's record, and the contract refuses a kept candidate
  that cites a judgement of no ("a kept candidate cites no refused dry run and no judgement of no"), as the real router
  also drops it (`Ineligible`). Probed at the assembly (four-provider route, `sim-verifier-b`, `unfit-x`, the need on the
  route's real first tuple): not offered, the candidate dropped `Ineligible`. The offered order `T-024-12`'s review saw
  had a need whose tuple is no tuple of the route (`kit::tuple(...)` instead of the route's own), and `offer` only
  tests the needs whose tuple the chosen route uses; such a need is ignored, not a no. A router returning a record that
  keeps a no is refused as `RoutingError::Router` (tried with a router that un-drops the candidate).
- **Design (defence in depth, fail closed).** (1) `judged` now requires the cited judgement's answer to be yes. (2)
  `offer` refuses a chosen route any of whose operations cites a judgement of no (`cites_a_refusal`), needed or not:
  `NotOffered::NotEligible { tuple }`. Nothing a reliance rests on can then be a refusal, since a reliance is declared
  on a judgement of an offered, accepted route.
- **Tests** (unit, in `rounds/mod.rs`, over the contract's own fixture record with its judgement's answer set after
  reading, since the contract refuses it earlier): `tuple whose verifier answered no is not offered`,
  `route citing a verifier's refusal as its judgement is refused`. `T-024-12`'s tests are unchanged.
- **Gaps, with owners.**
  1. *The reliance writer can't see an answer.* `Reliances::write` takes `Judgement { receipt, judge }` and an
     append-only `Log`, and `reliance-names-its-judges-statement` checks stated, verified, maker and about, never the
     answer. A reliance on a refusal is therefore not refused at the writer or the log. Exact shape if wanted: the
     relation reads the stated entry's judgement body and requires `answer: yes` (a new refused fixture `--answer`), or
     `Judgement` gains the answer and the caller (the assembly's `judgement_of`) supplies it. Owner: core-internal's
     single writer through the coordinator (contract round), then `F-020`. Not done here (`contracts/` untouched).
  2. *A need on a tuple the chosen route doesn't use is ignored.* Reasonable, but a caller mistaking the tuple sees an
     offered order with no judgement. Owner: `F-020` (intake could refuse a need whose tuple is no tuple of the catalogue).
- **Review round 1.** (1) `offer`'s check is now `eligible(record, chosen, needs)`, a free function holding both
  calls (`cites_a_refusal`, `judged`); the two unit tests drive it, so a removed call is red (a `Run` needs a whole
  `Parts`, and `route` refuses the no before `offer`). (2) F-024's `verifier.rs` gains two tests (additive; T-024-12's
  four are untouched): `honest verifier's no to a need on the route's tuple leaves no offer and no reliance` (need on
  the tuple of a bare quote's route: not offered, no round chose, the candidate dropped `Ineligible` naming the
  verifier's no, no reliance, nothing accepted, no move of value) and `need on a tuple off the route is ignored and
  the order is offered with no judgement` (gap 2 pinned; today's behaviour, not a requirement).
- **Reliances can't reach a no in practice:** a reliance is declared on a judgement of an offered, accepted route, and no
  such route cites a no (the contract refuses it, and `eligible` again). The reliance half of the row's "refused" stays
  gap 1's contract item.
- **Rule-off table** (each alone, 120 s timeout, restored, none hung):

  | Rule switched off | Red |
  |---|---|
  | `eligible`'s call to `judged` | `tuple whose verifier answered no is not offered` |
  | `eligible`'s call to `cites_a_refusal` | `route citing a verifier's refusal as its judgement is refused` |
  | `judged` ignoring the answer | `tuple whose verifier answered no is not offered` |
  | `offer` ignoring `eligible`'s result | T-024-12's `tuple needing a judgement is not offered without one` |

## T-020-33: a hand-on through a holding account that runs no route

Branch `f-020/t-020-33-hand-on-holding-account`, from `m3` at `17f98832`. Edits `components/core-orch/src/`: `stuck/`
(`within`, `Mover::plan_hand`), `hand_on/` (`recover_starts`) and `segments/` (a new `landing.rs`, and `land_settled` in
`mod.rs`); `conformance/scenarios/` (`F-024` files: `src/assembly/holding.rs`, a hunk of `src/assembly/mod.rs`,
`tests/assembly_real_wallet.rs`; and `T-020-17`'s `tests/stuck_hand_on.rs`, whose stand-in wallet gains an `account`). The
commit body says so. `contracts/` is untouched; its one conflicting rule is gap 1.

- **Design.** The form's choice `hand-on-wallet-holder`, built as its applied default: **the decision's `by` names the
  node's holding account, and the linked route's first provider runs the first move.**
  1. *Decide* (`stuck::within`): `by` of a hand-on is no longer held to be a provider of the part's route (the account of
     a public node, the operator's plain wallet, advertises no tuple and runs no segment); a deliver still is. The
     amount and asset are held to what the part holds, as before. What a hand-on's `by` must be is the mover's to hold,
     where Core's records of the nodes' accounts are.
  2. *Move* (`Mover::plan_hand`): the check that `by` is the first segment's provider is gone; the check that `by` is
     the node's registered holding account (`Withdrawals::holder`) stays, and is the only rule on the holder: a decision
     naming the reserve that put the value there, or the route's first provider, is refused with the account shown
     nothing. The first move's provider is the one the linked route names, which the customer's acceptance names too
     (`linked_accepted`: one yes per part, from the first segment's provider).
  3. *The start* is the holding account's own `holding_withdrawal` record (`proves_start`, unchanged: `record.by` is the
     holder, the starting reserve the hand-on recorded from the done resolution's `by`), and `recover_starts` reads it
     the same way: for a linked order opened by an operator's resolution, the record's holder must be the `by` of that
     resolution's `resolve_stuck` effect and no other, and every part must start at the record's node, whoever runs
     the first segment; for a customer's hand-on (no such effect) the holder is the route's first provider, as before.
  4. *Landing* (`segments/landing.rs`): `Segments::land_settled(&mut dyn Landings) -> usize`, orchestration's one call
     of `Holdings::land`, like `write_measures`: for each settled segment whose destination is an inner node the
     `Landings` port says has a holding account, once per segment, the wallet is asked to place `amount_out` for the
     order; `holds` false takes none. `Landed::NotNow` (the platform says nothing took effect) is asked again by the
     next call; `Landed::Unconfirmed` (anything else) never again and is counted (`landings_unconfirmed`), because
     asking twice could attribute twice, and the account's own rules refuse a withdrawal of more than it holds, so no
     value leaves the account that was never confirmed in it (`INV-5`). The assembly implements the port over
     `Holdings` and the platform, in the operator's signed statement, and calls it after each run of a part (`run_live`);
     `Core::land_settled` is public for a scenario to show nothing lands twice. `Core::land` (by hand) stays for
     `AC-39`, which lands where no segment settles.
- **Tests.** `stuck_hand_on.rs` (two added): `hand on names the node's holding account while the linked route's first
  provider runs the move`, `hand on whose start no record of the holding account proves stays refused`; the stand-in is
  `OTHER_RESERVE`'s wallet while the linked route starts with `RESERVE`. `assembly_real_wallet.rs`: `value settled at a
  public node held by a holding account lands in it` (added, over the real wallet, with no `Core::land`); `stuck order of
  the assembled Core is handed on through the real plain wallet` runs with no `#[ignore]` and passes; its refusal test is
  rewritten as `hand on through a public node's wallet names the wallet and refuses any other holder` (the wallet is
  taken, the ramp's provider and the route's first provider are refused before the wallet is asked anything).
  `T-020-17`'s and `T-020-26`'s tests pass unchanged (only the stand-in wallet's helper gained the `account` field,
  defaulting to the reserve).
- **Histories the log refused.** None in the tests that run. The one the log would refuse is gap 1.
- **Red first.** The two `stuck_hand_on` tests failed on `decided: NotItsValue` (the old `within`) before the code. The
  three `assembly_real_wallet` tests were written with the code (the landing test needs the new `Core::land_settled`, so
  it could not compile before): the rule-offs below are their red.
- **Defaults taken** (reversible, fail closed). The holder is only ever the node's registered holding account; the decide
  step does not know the registry, so a decision naming another provider is taken and then fails at the move with the
  wallet asked nothing, the part still held (its failure frees nothing without another party's confirmation). A landing
  that isn't confirmed is never retried. A segment a recalculation retired lands nothing. The first move's provider is
  whoever the route says.
- **Gaps, with owners.**
  1. **The log refuses the linked order's first move for a holder that runs no segment** (needs a contract change, which
     this row doesn't make). `contracts/src/core_internal/relations.rs`, relation
     `handed-on-route-starts-on-its-holding-record`, field `holder`, requires the `holding_withdrawal`'s `holder` to
     equal the first `move_value`'s `origin_reserve`, which is the first segment's provider. With the holder the wallet
     and the provider another, Core's mover hands the segment over (the experiment: the effect is built and the segment
     settles in the kit) and the real log would not admit its effect (`core.admitted` names entry 54 and that field).
     The shape wanted: that field's check (`&[holder == origin_reserve]`) dropped, or replaced by the record's node
     being the first segment's `from` (which `holding-withdrawal-starts-a-handed-on-route` already holds); the
     `holding_withdrawal`'s `holder` is then the account the record proves. Owner: `F-006`/`F-020` contract round, Pablo's
     call; until then, `T-024-17`'s "linked order's first move in the log" stays unproven for a wallet holder, and the
     tests here stop at the proven start.
  2. The landing is called after `run_live` only; a segment that settles later (a late statement confirmed done,
     `confirmed_done`) lands on the next call of `land_settled`, which the assembly doesn't make by itself yet. Owner:
     the production assembly (`F-030`), with `write_measures`.
  3. A segment that settled at the node and is later reversed leaves the account's attribution as landed: the reversal
     doesn't release it. Owner: `T-020-12`'s successor.
  4. The landing's `NotNow` and `Unconfirmed` paths are not exercised by a test (the real wallet in the kit always
     answers). Owner: `T-020-35`'s neighbour in `segments/`, or a fake `Landings` unit test in the next `segments/` row.
  5. `Landing` events: the wallet's deposit may add platform events that the next drain takes; none was seen in the
     scenarios (the log admits every entry). Owner: `F-030`.
- **Rule-off table** (each switched off alone, file restored; the tests of `stuck_hand_on.rs` and
  `assembly_real_wallet.rs` run; none hung, hard timeout 250 s):

  | Rule switched off | Red |
  |---|---|
  | a hand-on's `by` held to be a provider of the part (`within`) | `hand on names the node's holding account...`, `hand on whose start no record...`, `stuck order of the assembled Core is handed on through the real plain wallet` |
  | the holder must be the node's registered holding account (`plan_hand`) | `hand on through a public node's wallet names the wallet and refuses any other holder`, `hand on whose start no record...`, and `T-020-17`'s `hand on of a route other than the linked order's accepted one is refused` |
  | the recovered holder is the one the resolution named (`recover_starts`) | `hand on whose start no record of the holding account proves stays refused` |
  | the record is the holder's own (`proves_start`'s `by`) | the same |
  | the assembly lands what settled (`run_live`) | `value settled at a public node...`, `real plain wallet's re-serialised answer verifies...`, `stuck order of the assembled Core is handed on...` |
  | one landing per segment (`closed`) | `value settled at a public node held by a holding account lands in it` |

## T-020-36: `core-internal`'s round after `T-020-30`

Branch `f-020/t-020-36-core-internal-round`, from `m3` at `430b73dd` (with `T-020-30` and `T-020-27` merged). Edits
`contracts/` (core-internal, a `DEC-20` path: it waits for the owner's click at the release into `main`), and the
consumer files named below.

- **Versioning: folded into 1.0.0, no bump** (lesson 7): core-internal 1.0.0 isn't on `develop`. The changelog's last
  entry names each item by number; `contracts/CHANGELOG.md` takes a line.
- **Item by item.**
  1. *A node's record verified about an order* (`T-020-32`'s blocker). `attribute`'s step 6 verified an entry about an
     order only through a transaction its body names at top level; an `inner-segments` body names its transactions
     only inside `segments[]` (and denies unknown fields), so the record was never verified about the order it binds.
     Step 6 now reads such a body (`inner_segments_of`, parsed as provider-role's `InnerSegments`): about an order, no
     segment, no transaction, it is verified only where `about.node` is the body's `node`, `segments` isn't empty, and
     every segment's transaction is first bound to that order (`binding_of`, the binding `transaction-belongs-to-one-
     order` keeps) **and** asked under for it by the operator's own verified `operation` question (`asked_under`).
     About its node alone, or about nothing, it is verified as before and binds no order. Every other shape is read as
     before, and names no node.
  2. *A record names its node; the replay holds it to the node's publisher, once.* `About.node: Option<Id>`, named only
     by an `inner-segments` record, about no segment or transaction (`StatedEntry::check`, and the schema's `if`/`then`
     on `stated-entry`), and verified against the body (item 1). `replay-cites-its-nodes-records` gains `node` (one of
     the replay's `nodes`, not one its `found` says recorded nothing), `publisher` (its maker states for the party the
     replay's exit result states for) and `once` (no other cited record names the node). `private_node_of`, which
     binds a node for `reversal-rests-on-its-evidence`'s `private`, binds through an `inner-segments` record only the
     node its `about` names.
  3. *A late status names its transaction.* `Action::AskStatus.transaction: Id`, required. New none-other row
     `status-is-asked-under-a-fresh-transaction`: no earlier stated entry's `about` names it, nor another `ask_status`
     effect. `core-orch`'s `Reversals::ask_status` (`reversal/mod.rs`) now claims the transaction before policy
     decides, so the request, the decision and the effect name it; `tests/reversal.rs`'s late-status test asserts the
     effect names `asked.transaction`.
- **Security reading** (the brief's weight). The widened step 6 can't verify a record about an order through another
  order's transaction (`binding_of` fixes a transaction to the first order named, and `transaction-belongs-to-one-
  order` refuses a later other), through one no entry binds, through one only a provider's own statement binds (Core
  never asked under it), or vacuously (no segments). The node is the body's own, so a writer can't relabel a record of
  one node as another's. A non-publisher's record binds no node: the replay row holds it to the exit's publisher, and
  `private_node_of` lets a record bind only the node it names, under the publisher its key states for (attribution's
  step 3).
- **Defaults** (reversible, each failing closed):
  - fold into 1.0.0;
  - "of that order" is first bound to it and asked under for it by the operator's own verified `operation`, stricter
    than the binding alone: a record of a publisher's internal transaction, or appended before Core's questions, is
    attributed to no one about the order;
  - a node a replay replayed belongs to the publisher its exit's result states for, as `private_node_of` already binds
    it; a sibling provider of that publisher may sign the record;
  - a record of a node the replay found silent is refused (`node`), beside the Rust count;
  - an `inner-segments` record about its node alone, or about nothing, stays verifiable and binds nothing;
  - the late status's transaction is drawn before policy decides; a refused ask leaves it unused, never drawn again;
  - the freshness row reads stated entries' `about.transaction` and other `ask_status` effects, not hand-overs
    (which follow the effect) nor decisions (which name the same action).
- **Consumer lines** (to compile): `node: None` in every `About` literal: `components/core-orch/src/{arrival/confirm.rs,
  arrival/mod.rs, quote/held.rs, reversal/mod.rs, rounds/mod.rs, segments/mod.rs, snapshot/statements.rs,
  stuck/mover.rs}`, `components/core-orch/tests/{intake_holds.rs, quote.rs, rounds.rs}`,
  `conformance/scenarios/tests/{assembly_real_wallet.rs, segments_support/mod.rs}` (`F-024`'s); a transaction in
  the two `AskStatus` literals of `components/core-authz/src/{engine/mod.rs, roles/tests.rs}` (`F-021`'s). New tests in
  `components/core-audit/tests/log.rs` (`F-022`'s).
- **Real log's path.** `core-audit`'s `a_publishers_record_of_a_nodes_segments_is_verified_about_its_order_on_the_log`
  and `a_replay_on_the_log_cites_one_record_per_node_by_its_publisher` run the contract's world on `AuditLog`, sign each
  record for real with its maker's statements key, keep its bytes, and append it through the general handle: fx-desk's
  record of `ledger-in` under `tx-f1` and `tx-f2` (Core's questions for `ord-1`) is verified, one naming `ord-2`'s
  `tx-yes-2` or an unbound transaction isn't; a replay citing the verified record is admitted, one citing the
  unverified one, us-bank's (another publisher than the exit's fx-desk), two of one node or one of a node not
  replayed is refused, and one record of each of two nodes is admitted.
- **Histories the log refused, sorted.** None new: every world case, `T-020-30`'s and `T-020-27`'s tests pass with only
  the shape's literals changed. `T-020-30`'s three `inner-segments` admission cases now name their node.
- **Order of work.** Shapes, then the rules, then the fixtures (`scratchpad/f020zh/gen.py`) and the named tests, which
  passed on their first run; their red is shown by the rule-offs below, each run from the committed round. The first
  run left two rules green; each got a case (`6ba57ed0`) and went red on the second run.
- **Gaps, with owners.**
  1. `T-020-32` appends each record about its order and node after the operations it lists, one per order: a book
     spanning orders, or a segment under a transaction Core never asked under, stays unverified about the order (by
     design). Owner: `T-020-32`.
  2. A replay's nodes are bound to its exit's publisher. A run of private nodes of two publishers, or an exit made by
     another publisher's provider, would need each node's publisher from the catalogue's private-node declarations,
     which the log doesn't hold. Owner: the lead, a later `F-020` round if `T-020-32` meets one; residual for
     `T-025-05`.
  3. `private_node_of`'s replay branch still binds through the exit's result alone; the records past it add nothing to
     the binding while they must state for the same publisher. Owner: `T-020-32`, if its scenarios need otherwise.
  4. The scenario suite beyond the binaries run here runs in CI.
  5. `once` holds within one replay, not across replays of one order: two replays may each cite a record of the same
     node. Owner: the lead, a later none-other row ("one record of a node per order") if a reader needs it (the
     review of `d11e5a2b`).
  6. A record's body names its maker's publisher (the review's fix), but each segment it lists isn't held to a provider
     stating for that publisher: a provider may list another publisher's segment of the same order. Residual for
     `T-025-05`.
- **Rule-off table** (each switched off alone from the committed tree, its tests run under a 600 s alarm through the
  lock, the file restored with `git checkout`; script `scratchpad/f020zh/ruleoff.py`; none hung). "Table" is
  `every_relation_of_the_citation_table_holds_and_breaks_as_its_cases_say`, "attribution" is
  `the_log_not_the_writer_attributes_a_stated_entry`.

  | Rule switched off | Red |
  |---|---|
  | `attribute`: a record's transactions first bound to its order (`binding_of`) | green on the first run (the operator's question covered every case); with `--naming-a-transaction-first-bound-to-another-order` (`6ba57ed0`): `inner_segments_record_naming_a_transaction_of_another_order_is_not_verified_about_this_one`, attribution |
  | `attribute`: asked under by the operator's own question | the same two |
  | `attribute`: no segment transaction read at all | the same two |
  | the same, on `core-audit`'s log | `a_publishers_record_of_a_nodes_segments_is_verified_about_its_order_on_the_log` |
  | `attribute`: at least one segment | the same two contract tests |
  | `attribute`: `about.node` is the body's, about an order | the same two |
  | `attribute`: `about.node` is the body's, about its node alone | the same two |
  | `attribute`: no node on another shape | green on the first run (`StatedEntry::check` refuses first); with the in-memory probe (`6ba57ed0`): `inner_segments_record_naming_a_transaction_of_another_order_is_not_verified_about_this_one` |
  | `attribute`: the widening whole (records read as before) | `inner_segments_record_about_an_order_whose_transactions_are_all_of_that_order_is_verified`, attribution |
  | `StatedEntry::check`: a node only on a record | `every_valid_fixture_validates_and_every_invalid_one_is_rejected`, `every_invalid_fixture_is_rejected_by_the_rust_types_too`, `inner_segments_record_naming_a_transaction_of_another_order_is_not_verified_about_this_one` |
  | `replay-cites-its-nodes-records`'s `node` | table, `replay_citing_a_node_record_by_another_than_that_nodes_publisher_is_refused` |
  | its `node`, the silent nodes alone | the same two |
  | its `publisher` | the same two |
  | its `once` | table, `replay_citing_two_records_of_one_node_is_refused` |
  | its `publisher`, on `core-audit`'s log | `a_replay_on_the_log_cites_one_record_per_node_by_its_publisher` |
  | its `once`, on `core-audit`'s log | the same |
  | `private_node_of`: a record binds only its node | table, `replay_citing_a_node_record_by_another_than_that_nodes_publisher_is_refused` |
  | `status-is-asked-under-a-fresh-transaction` whole (cites none) | table, `late_status_asked_without_its_transaction_is_invalid` |
  | its stated entry naming the transaction | the same two |
  | its other late status's effect (before the review) | the same two |
  | the schema's `transaction` required on `ask_status` | `every_valid_fixture_validates_and_every_invalid_one_is_rejected`, `late_status_asked_without_its_transaction_is_invalid` |
  | `core-orch`: the action names the transaction drawn (another fresh id instead) | `late_status_of_a_segment_is_asked_under_a_fresh_transaction_and_kept_as_the_providers_statement` |
- **Local checks of the first round** (one lock hold, at `6ba57ed0` with these notes): `cargo fmt --all -- --check`, `cargo clippy
  --workspace --all-targets -- -D warnings`, the contracts' `no_std` build (`riscv32imac-unknown-none-elf`), `cargo test
  -p rutent-contract-guard`, the guard against `origin/develop` ("1671 changed path(s), no violations"), plan-check
  and tag-check `--strict`, `cargo test` of `rutent-contracts`, `rutent-core-authz` (and its `--lib`),
  `rutent-core-orch` and `rutent-scenarios`, and `cargo doc -D warnings` of the five: all green. `rutent-core-audit`:
  all green but one run of `reopen.rs`'s `store_holding_a_verified_entry_no_kept_statement_bears_out_is_refused_on_opening`,
  which failed opening the evidence store's files (`Unavailable`, at the test's `EvidenceStore::open`) and passed three
  runs of its own binary after; it reads no file this row touched, so a flake for CI to confirm.

### T-020-36, the review of `d11e5a2b`: changes

- **Should-fix, fixed: a late status's transaction could be one a verified entry named.** The row counted stated
  entries' `about` and other `ask_status` effects only, where the action's doc says no other entry may name it: the
  reviewer's probe asked under `tx-h3`, named before only by a `stuck_decide` and its `resolve_stuck` effect, and was
  admitted. The row now reads every verified shape that names a transaction (`transactions_named`: a decision's or an
  effect's action, `ask_status`, `resolve_stuck` or an `accept_fault`'s showing; a `stuck_decide`; any hand-over item),
  excepting the effect's own permitting decision (by its request and decision ids). Checked across `types.rs`: no
  other verified shape names a transaction. Six refused cases: `--a-stuck-resolutions-transaction` (the probe),
  `--a-transaction-only-a-stuck-decide-step-names`, `--a-transaction-only-a-resolve-stuck-effect-names`,
  `--a-transaction-another-late-status-decision-names` (decided, never taken), `--a-transaction-a-hand-over-names`,
  `--a-transaction-an-accepted-showing-names`.
- **Low, fixed: a record's body names its maker's publisher.** `inner_segments_of` holds the body's `publisher` to the
  party the entry claims (step 3 binds it to the key), as its `node` to `about.node`. Case
  `unverified--an-inner-segments-record-naming-another-publisher-than-its-maker-states-for`. Going further (each listed
  segment's provider stating for that publisher) is gap 6, a residual.
- **Low, noted:** `once` across replays of one order, gap 5.
- **Rule-offs of the review's rules** (from `e4c51b1e`, same script and alarm; none hung, every one red):

  | Rule switched off | Red |
  |---|---|
  | the row whole; its stated arm; its verified arm (each alone) | table, `late_status_asked_without_its_transaction_is_invalid`, each time |
  | its own permitting decision excepted (counted instead) | `every_entry_of_a_complete_log_is_admitted_on_what_came_before_it`, table, `late_status_asked_without_its_transaction_is_invalid`, `request_for_a_providers_late_status_without_its_segment_is_invalid` |
  | a decision's action; an effect's action; a `stuck_decide`; a hand-over (each alone) | table, `late_status_asked_without_its_transaction_is_invalid`, each time |
  | `ask_status`'s and `resolve_stuck`'s transaction; `resolve_stuck`'s alone; an accepted showing's (each alone) | the same two, each time |
  | `attribute`: a record's publisher is its maker's | `inner_segments_record_naming_a_transaction_of_another_order_is_not_verified_about_this_one`, attribution |
- **Local checks** (one lock hold, at `e4c51b1e` with these notes): the same list as the first round's, all green,
  `rutent-core-audit` included; the guard against `origin/develop`: "1678 changed path(s), no violations".

### T-020-36, the re-review of `2b0f32b1`: changes

- **Should-fix, fixed: the own-decision exception matched by ids alone.** A decision copying the permit's request and
  decision ids, asking about p1-s1 (us-bank) under `tx-status-2`, was read as the effect's own and admitted. The
  exception is now the entry at the permit's receipt (the same entry, `core::ptr::eq`), deciding the same action.
  Cases `--a-decision-copying-its-own-ids-for-another-action` (the reviewer's) and
  `--a-copy-of-its-own-decision-at-another-receipt` (pins the receipt half), both refused.
- **Rule-offs** (from `d12dd92a` and `c0b58b6e`; none hung):

  | Rule switched off | Red |
  |---|---|
  | the fix whole (ids alone, as before) | table, `late_status_asked_without_its_transaction_is_invalid` |
  | the receipt half alone | green from `d12dd92a` (the action half refused the reviewer's case); from `c0b58b6e`, with the copy case: table, `late_status_asked_without_its_transaction_is_invalid` |
  | the action half alone | none: a decision at the permit's receipt for another action is refused first by the effect's own permit rows, so this half is kept as defence and no case of this row reaches it |
- **Local checks** (one lock hold, at `c0b58b6e` with these notes): the first round's list, all green; the guard against
  `origin/develop`: "1680 changed path(s), no violations".

### T-020-36, item 4 (added by the lead): a route handed on, whoever runs its first move

- **Finding** (`T-020-33`, a hand-on through a plain wallet, the owner's form `hand-on-wallet-holder` default):
  `handed-on-route-starts-on-its-holding-record` held the holding record's `holder` to the move's `origin_reserve`,
  so the wallet holding the value while the linked route's first provider runs the first segment was refused on that
  field.
- **Design.** The record stays required. Its `holder` is the holding account the hand-on named (field `holder`): for
  the operator's hand-on of a stuck order, the `by` of its `resolve_stuck` effect onto this order; for the customer's
  hand-on after a failed final deposit, which names no holder, the linked route's first provider, which holds the value
  there (`core-orch`'s `hand_on/` reads the two bases the same way). Its `node` is the inner
  node the linked route's own plan moves this first segment from (field `node`, as
  `holding-withdrawal-starts-a-handed-on-route` holds the node). The move's `provider` is the one that plan names for
  the segment (field `provider`); it needn't be the holder.
- **Default** (reversible, failing closed): the log holds no register of a node's holding accounts, so "that node's
  holding account" is the one the hand-on named and its own record places at the node. `origin_reserve` is no longer
  read by this row (the authorisation's rows still read it).
- **Cases.** The contract's world had no admitted move of a route handed on (its hand-on names fx-desk, its holding
  record us-bank's), so the cases are built from `--missing`'s history, with ord-2's own authorisation (`auth-2`, which
  its acceptance names and the history lacked) and its own segment id (`o2-s1`: `p1-s1` is ord-1's, which
  `segment-belongs-to-one-order` keeps): `--a-wallet-holds-and-the-plans-provider-runs-the-first-move` admitted
  (plain-wallet named and holding at `ledger-in`, us-bank running the move); `--a-holder-the-hand-on-did-not-name`
  (`holder`), `--node` (the record at `ledger-out`), `--a-first-move-from-another-node` (`node`), `--provider` (fx-desk
  runs what the plan gives us-bank, decided so) refused, beside `--missing` and `--holder` (now refused on the
  hand-on's name); for the customer's basis, `--a-customers-hand-on-held-by-its-first-provider` admitted and
  `--a-customers-hand-on-held-by-another-than-its-first-provider` refused (`holder`).
- **Histories the log refused, sorted.** The first version read the operator's basis alone and the end checks'
  scenarios refused four of `rutent-scenarios`' real flows (`hand_on.rs`'s
  `handed_on_routes_withdrawal_is_proven_by_the_holding_accounts_record`, and three of `hand_on_record.rs`), each a
  customer's hand-on with no `resolve_stuck`: a gap in the new rule, not a Core bug; the customer's basis fixed it,
  and both binaries pass. Test `handed_on_route_whose_first_move_another_provider_runs_from_the_holding_records_node_is_admitted`.
- **Hand-off to `T-020-33`:** its hand-on's `resolve_stuck` names the wallet as `by`, the wallet's `holding_withdrawal`
  record names it as `holder` at the node the linked plan's first segment starts from, and the plan names the provider
  that runs it.
- **Rule-offs** (from the item's second commit; none hung):

  | Rule switched off | Red |
  |---|---|
  | `holder`: the holding account the hand-on named | table, `handed_on_route_whose_first_move_another_provider_runs_from_the_holding_records_node_is_admitted`, `route_handed_on_whose_first_move_rests_on_no_holding_account_record_is_refused` |
  | `node`: the first segment's from-node | table, `handed_on_route_whose_first_move_another_provider_runs_from_the_holding_records_node_is_admitted` |
  | `provider`: the plan's | the same two |
  | `holder`, the customer's basis alone (its first provider) | the same two |
  | `holder`, the operator's basis alone (its `by`) | table, `handed_on_route_whose_first_move_another_provider_runs_from_the_holding_records_node_is_admitted`, `route_handed_on_whose_first_move_rests_on_no_holding_account_record_is_refused` |
- **Local checks** (one lock hold, at `e3650fe9` with these notes): the first round's list, all green, `rutent-scenarios`
  included; the guard against `origin/develop`: "1687 changed path(s), no violations"; tag-check 1984 tests.



### T-020-33, after the merge of `T-020-36`'s item 4 (`e2681780`)

Gap 1 above is closed by the contract: `handed-on-route-starts-on-its-holding-record` holds the first move to the
holding record of the account the hand-on named, at the plan's first node, run by the plan's provider. The hand-on tests
now go through the linked order's first move: `stuck_hand_on.rs`' `hand on names the node's holding account...` runs the
linked route (settled, the first move by `RESERVE` in the log, which the kit's log admits), and `assembly_real_wallet.rs`'
`stuck order of the assembled Core is handed on through the real plain wallet` asserts the `holding_withdrawal`
(holder the wallet, node `sim-chain-b`) and then runs the linked order with the new `Core::run_linked` (`assembly/linked.rs`,
an `F-024` file): the customer's authorisation of the linked order through policy (the holding account its origin reserve;
the linked order is entered in intake's records, which policy's `Requester` source reads), then its route; the first move
is by `sim-chain-out-b`, the wallet is asked nothing by it, and the real log admits each entry as it is appended.
- **Rule-off:** the first move run with the wallet as its origin reserve (`Started.origin`) turns the T-024-17 scenario
  red: the log refuses it ("a part's first move is from the origin reserve its part's yes names").

### T-020-33, review round (Opus, `16c9529e`)

1. *Only value still resting is held.* `Segments::land_settled` lands a settled segment's value at a held node only
   while it rests there: where the segment out of the node settled first the value is never landed, and a landing is
   released (`Landings::release`, `Holdings::release`) once that segment settles or the segment that brought the value is
   reversed (`Segments::note_reversed`, called where a reversal completes). The probe (the `AC-6` order through
   `sim-chain-b`, no fault) is `value that passes through the wallet's node is not held for the order`. A release is asked as
   a landing is (again where nothing took effect, never where unconfirmed). Not done: a reversal of the segment *out of* the
   node returns the value to the node, which should land it again; owner, `T-020-12`'s successor. The reversal hook has no
   end-to-end test (no kit scenario completes a reversal of a segment that settled at a held node); `Landing::reverse` is
   unit-tested, and switching the hook off turns nothing red.
2. *`recover_starts`* takes the last `resolve_stuck` effect for the linked order before the `handed_on` input, the one
   whose result was done (a part is resolved once, so none follows it), not the first: `recovered start follows the hand-on
   that succeeded, not an earlier refused one` (the probe: the first provider named and refused, then the holding account).
3. The stray `notes.md.bak` is deleted.
4. *Tests of the retry rules and the error mapping:* `segments/landing.rs` unit tests, `tests/landing.rs` (a fake
   `Landings` at the kit's ledger: landed once, passed-through never landed, asked again only where nothing took effect),
   `assembly/holding.rs`' `answered` and its unit test.
- Lows noted, not changed (kit code): `run_linked` authorises with the original order's destination rather than the linked
  request's, and `given_by` isn't checked against the linked order; production refuses an unproven start.
- **Rule-off** (each alone, file restored, none hung):

  | Rule switched off | Red |
  |---|---|
  | value that moved on is not landed (`moved_on`) | `value that passed through a held node is never landed`, `value that passes through the wallet's node is not held for the order` |
  | the recovered holder is the done effect's, not the first | `recovered start follows the hand-on that succeeded...` |
  | an unconfirmed landing is never asked again | the `landing.rs` unit test, `a landing is asked again only where nothing took effect` |
  | a landing that took no effect is asked again | the same two |
  | the wallet's error mapping (an unknown outcome is never asked again) | `only a refusal that moved nothing is asked again` |
  | a landed, moved-on value is released | `only value still resting is held and what left is released` |
  | the reversal hook | none (see 1) |


## T-020-35: the report pair, partly built; `T-020-25`'s two lows closed

Branch `f-020/t-020-35-report-pair-built`, from `933a7ad9`. Edits `components/core-orch/src/segments/drain.rs` and
`conformance/scenarios/tests/recorded_ends.rs`. No contract is edited.

- **The report pair is not built: a shape is missing (stopped, as the row asks).** `ReportPair` and the log's rows
  (`sent-report-is-its-partys-statement`, `received-report-is-its-partys-statement`) need two verified statements
  about one order **and one segment**, by two different parties, each citable with `about_agrees` (a statement about
  another segment, or through a transaction bound to another, is refused). Orchestration holds, at a segment's end,
  only these statements about it: the provider's own result and the operator's operation. The next party's report of
  receiving is its own result, about the **next** segment (its own transaction); a cross-module callee's result never
  reaches Core (`Events::calls` records the call, not the answer; `prov`'s `Platform` hands Core only the top-level
  result); a stuck move's confirming statement is another maker's, of the move's transaction, and in the kit states no
  figures (a `balance_verification`); an arrival notice is about the order and the reserve's own transaction, which
  `about_agrees` refuses beside a segment. So no pair of statements that the log would admit exists to be built on
  the running platform.
  Exact shape wanted, for the contract round (`core-internal`, `DEC-20`): `ReportPair` (and its two rows) take the
  receiving report as about the **next** segment of the route (e.g. `received.segment: Id`, with
  `received-report-is-its-partys-statement` reading `about` against it and holding it to be the segment after
  `pair.segment` in the order's route), so the provider's `amount_out` of segment n is compared with the next party's
  `amount_in` of segment n+1, both of which Core already holds and has judged. Orchestration then builds it in
  `segments/` where segment n+1's result counts, with segment n's tuple's terms (`quote::Terms`: increment and
  gross/net). Owner: the lead, as the next contract round; `T-020-35`'s pair part follows it.
  The two pair tests of the row (`segment ending with two reports hands core audit a pair with its tuple's terms`,
  `reports differing by less than one increment on the running platform raise nothing`) are therefore not written.
- **Closed: the wait while the log can't take the platform's record** (`T-020-25`'s gap 2). `outcome unknown waits while
  the log can't take the platform's record and is recorded after` (`recorded_ends.rs`): the withdrawal's module passes
  a call out and answers too late; the kit's fault queue makes a `HandOver` append unavailable for one drain's tries
  (`On::HandOver` already existed in the kit and is unchanged); the run answers `SegmentsError::Append(Unavailable)`,
  with no `unanswered` item and no `segment_outcome_unknown` input on the log; the next run appends the platform's
  record, then the input citing it, and the log admits every entry.
- **Closed: an operation with no answer and no transaction.** `prov`'s platform pushes an `UnknownOutcome` with an
  empty transaction for an operation it ran under none (`unwrap_or_default`). `drain::of_events` now refuses it by
  name, `Uncarried { what: "an operation that ended with no answer under no transaction" }`, in place of the bare
  "an identifier"; the drain is kept whole and fails on every try, as any uncarried drain does. Two unit tests
  (`drain.rs`): the refusal's name, and an operation under a transaction still carried.
- **Defaults (reversible, failing closed).** The refusal keeps the drain whole and loud (no item is skipped); the
  wait test uses the existing fail-closed hold unchanged.
- **Gaps, with owners.**
  1. The report pair (above). Owner: the lead (a contract round for the shape, then this row's pair part).
  2. The empty transaction itself comes from `prov`'s platform (`platform.rs`, `refused_after`); whether the platform
     should refuse an operation that has no transaction before it runs is `F-013`'s. Not edited here.
- **Rule-off table** (each once, run under a 300 s alarm through the lock, file restored with `git checkout`; none hung):

  | Rule switched off | Red |
  |---|---|
  | `account`'s hold while the platform's record isn't on the log (`reported && record.is_none()`) | `outcome unknown waits while the log can't take the platform's record and is recorded after` |
  | `of_events`' refusal of an empty transaction | `operation that ended with no answer and no transaction is refused by name` |

  The two tests were written after their rules, which already existed (the hold, `T-020-25`) or were one line (the
  name); the rule-offs are their check.

### T-020-35, part two: the report pair built (after `T-020-38`, merged at `bc27b878`)

Edits `components/core-orch/src/segments/` (a new `pairs.rs`, and `mod.rs`: its hooks, one error, one field in each of
`Segments` and `InFlight`) and, in `conformance/scenarios/` (`F-024`'s files: the commit body says so), the assembly's
`audit.rs` (`Core::write_pairs`, `write_pairs_as`, `unpaired_reports`) and a new `tests/report_pairs.rs`. No contract.
The closed gap 1 above is this part's; the earlier text of the section stays as the record of the stop.

- **Design.** When segment n+1 settles (`settle_on`, late settlements included, never a grade-only call), `segments/`
  keeps what segment n reported (`note_report`: n's provider, its stated `amount_out`, the out asset, the receipt of
  its result that counted, and n's tuple's terms) and builds the `ReportPair` of n's report and n+1's (n+1's provider,
  its stated `amount_in`, its from-asset, the receipt of its result): `pair.segment` and `sent.segment` are n's,
  `received.segment` n+1's. The terms are n's tuple's: its fill increment restated from the origin's unit to the out
  unit, rounded down (one that comes to nothing is none: compared exactly, which fails closed), and its convention,
  `net`, or `gross` with the charge the figure holds restated in the out unit (`0` for a conversion, whose charge is
  in the asset it doesn't give out). The pair is queued once for its two segments and handed to `core-audit`'s
  comparison by `Segments::write_pairs(&mut dyn Comparisons)`, the caller's call, as the measures' (`T-020-31`): the
  segment's outcome, its entries and what it moved depend on neither the writer nor its answer. A pair the log can't
  take waits, with those after it; one it refuses stays and is the first failure answered (loud, never dropped).
- **A pair that can't be built is raised, never skipped.** The segment's end answers `SegmentsError::Pair(Unbuilt)`
  (no sent report held for the segment before; a figure that is no amount; the contract's `ReportPair::check` refusing
  it), and is built again on the next run. "Raised" here is the run's error, as `SegmentsError::Charge`'s: the
  assembly's caller sees it; no `Raise` to the operator is written for it (`Difference` has no shape for a pair never
  built). The segments that make no pair are those with none to make, by the contract's rule: a route's first
  segment (a recalculated route's included: a pair spans one plan), and one run by the provider that ran the segment
  before it (the log's two-parties rule).
- **Defaults** (reversible, each failing closed): the writer is the caller's, not `run_part`'s (no `Parts` changed);
  the increment restated rounds down, a zero one means exact comparison; a refused pair is kept and answered every
  call.
- **Gaps, with owners.**
  1. *The simulated providers can't make reports differ.* The stand-in tuples declare no increment and Core hands
     segment n+1 exactly what n gave out, so an honest platform's two reports agree. The two tests that need a
     difference restate the pair's figures and increment on its way to the comparison (`Core::write_pairs_as`), the
     statements and receipts as Core built them: a sim that fills in increments (`Delivery`/`Behaviour`) is `F-024`'s.
  2. *No test of a pair that can't be built.* The only causes are a segment whose flight Core lost (a restart: flights
     are in memory, `F-030`'s durable store, as `T-020-31`'s gap 1) or a contract refusal no honest result gives. After
     a restart, `segments/` would answer `Unbuilt::NoSentReport` for the first segment settled in the new session
     whose predecessor settled in the old one. Owner: `F-030`.
  3. *A pair across a re-plan* is `T-038`'s gap 1 (the lead).
  4. *The production assembly* calls `write_pairs` after the entry points that can settle a segment, as it calls
     `write_measures` (`T-020-31`'s gap 1); the scenario assembly's callers are the tests.
- **Tests.** Written after the code (the shape was settled by the reading above, then each test run once green):
  `segment ending with two reports hands core audit a pair with its tuple's terms` and `reports differing by less than
  one increment on the running platform raise nothing` (the row's), and `reports differing by one increment or more
  on the running platform raise the difference` (`tests/report_pairs.rs`, the four-provider route of `AC-1`, in the
  real log: it admits each pair on the citation table of `T-038`); unit tests of the terms in `pairs.rs`. `T-022-05`'s
  and `T-020-25`'s tests pass unchanged.
- **Rule-off table** (each alone, run under a 400 s alarm through the lock, file restored with `git checkout`; none hung):

  | Rule switched off | Red |
  |---|---|
  | no pair is built (the build returns early) | the three `report_pairs` tests |
  | a gross tuple read as net | `segment ending with two reports hands core audit a pair with its tuple's terms` |
  | one provider's adjacent segments are not skipped | seven of the kit's `segments` tests (their route runs both segments at one reserve) |

## T-020-32: the simulated world binds each private node to its publisher

Branch `f-020/t-020-32-publisher-binds-node`, from `m3` at `7bc8be82`, with `T-020-36` (`728a8d72`) merged: the first
attempt stopped at a contract blocker (a record couldn't be verified about an order), which `T-020-36` item 1 lifted.
Edits `components/core-orch/src/closing/` and, for the hook below, `segments/ports.rs` and `segments/mod.rs`; the
assembly's `grading.rs` and the `Books` impls of the two closing test files (`F-024`'s); `T-024-06`'s `reversing.rs`
only to remove its two `#[ignore]`s.

- **Design.**
  - `Books::recorded` returns `Recorded { segments, records }`: the segments `CMP-EVIDENCE` reads, and the publishers'
    signed `inner-segments` records they came from, whole (one per provider of the node that recorded any).
  - A receipt is the log's to give, so an entry citing a record can only follow it. `Grading` gains
    `statements(&ToGrade) -> Vec<ToKeep>`; the segments keep and append each once (cached in the flight, so a retry
    appends only what isn't) before grading, and hand the receipts back in `ToGrade::kept`. The default asks for none.
  - **Entering a node.** At a segment that takes the value from a public node into a private one, closing asks for the
    record of that node, its provider's where it lists the segment's transaction, and cites it in an extra `evidence`
    (`<id>.node`, the public record, naming the movement into the node *with the node*, resting on the segment's result
    and the record). That is what `Reversals::follow` and `private_node_of` bind on. The binding is made here, not at
    the failed segment, which never reaches closing.
  - **Leaving a node.** At an exit, each replayed node's record is asked for, and the replay's `rests_on` is the exit's
    result then the record of each node it read, one per node (`replay-cites-its-nodes-records`).
  - **Only what the log verifies.** A record is about its order and node alone (`About.node`) and is appended only
    where every segment it lists is a transaction this order ran (the segments Core has seen settle, and the one being
    graded); otherwise the log would leave it unverified about the order, and an entry citing it would be refused and
    stall the segment. A book that spans orders therefore binds nothing for the later order (fail closed), shown by
    `a_book_spanning_two_orders_binds_nothing_for_the_second_and_stops_nothing`.
- **Tests** (`conformance/scenarios/tests/publishers_binding.rs`): the plan's four, each with its tags, plus the
  spanning-books case (`REQ-ORD-41`). `T-024-06`'s two now run and pass.
- **Defaults** (reversible, each failing closed).
  - A node gets **two** records about one order where a replay follows: the early one (the entering provider's book,
    just the withdrawal) binds it, a later one (as of the exit) is what the replay read and cites. `T-020-36` gap 1
    says "one per order"; a single early record would be stale for the replay, a checker finding what was compared
    would read the wrong figures. The log's `once` holds within one replay. Overrule if one per node per order is wanted.
  - Where several providers of one publisher recorded in a node, the replay cites one record per node, the exit
    provider's where it served the node, else the first; the other providers' segments are read but their records not
    cited (the contract's `once`).
  - Records that list a transaction this order didn't run are skipped; so is one listing a reversal's transaction
    (reversals run after the entering and exit reads, so none is seen there).
  - A segment entering a private node with no public movement of its own binds nothing.
- **Histories the log refused, sorted.** None: the sim's records were verified about their orders on the real log at
  first use; the one refusal risk, a book spanning orders, is a test artefact the pre-check turns into "nothing bound".
- **Order of work.** Hook and closing first; the two ignored tests were run with their ignores removed and passed, then
  the four named tests were written and passed on their first run; their red is the rule-offs below (I did not watch
  the four red before the code).
- **Gaps, with owners.**
  1. **`replay-cites-its-nodes-records` admits an entry of any provider of the publisher, and one record of a node a
     replay didn't cite none is not required** (`T-020-30`'s low): closing now cites only the records it keeps, one per
     node, made by the exit's provider where it served the node; the log's side (a record per node that recorded
     something, the exit's provider) needs a contract change (`T-020-36` gap 2/5): the lead.
  2. `Closing::recorded`'s rebuild after a restart doesn't read the records; the evidence names them. Owner: `F-030`.
  3. The record is taken as of Core's last reading of the book, so a node's early record lists only what the book held
     then. By design (default 1).
  4. Reversal operations, which Core runs and the provider books, aren't listed in a record Core keeps; a record
     listing one is skipped. Owner: none yet; the lead if a scenario needs a record after a reversal.
  5. `Evidenced` still has to be given the hook by any other `Grading` that wants records; the production assembly's
     (`F-030`) must wire `Books` to the publishers' signed records.
- **Rule-off table** (each switched off alone, file restored with `git checkout`, run under a 900 s alarm through the
  lock, script `scratchpad/f020zg/ruleoff.py`; none hung). `binding` is `publishers_binding`; `reversing` is
  `T-024-06`'s two tests: `true_undo_…` is AC-4, `failed_segment_…` AC-6.

  | Rule switched off | Red |
  |---|---|
  | closing: the evidence naming the node at the entering segment | binding: `publishers_inner_segments_record_…cited_by_what_binds_its_node`, `failed_providers_own_answer_…`; reversing: both |
  | closing: asking for the record at the entering segment | the same, and `a_book_spanning_two_orders…` |
  | closing: the replay's cited records | binding: `replay_cites_the_receipt_of_each_inner_segments_record_it_read` |
  | closing: records of this order's transactions alone | binding: `a_book_spanning_two_orders_binds_nothing_for_the_second_and_stops_nothing` |
  | `grading.rs`: `Books` returns no record | binding: the four others (not the `SamePublisher` one); reversing: both |
  | contracts' `private_node_of`, the evidence branch (the log binds the node by evidence) | binding: `failed_providers_own_answer_…`; reversing: both |
  | `judge`: `SamePublisher` | binding: `reserves_confirmation_of_its_own_publishers_providers_reversal_stays_refused`; reversing: AC-4 |
  | `AC-6`'s two-actor rule: Core's `SameParty`/`SameLogin` | reversing: AC-6 |
  | the same and policy's `one_actor` | reversing: AC-6 |
  | the same, policy's and the log's `stuck-move-carries-out-its-decide-step` `mover`/`login` (all three layers) | reversing: AC-6 |

  The last three are red on the test's first refusal, `Err(SameParty)` from Core's move step, which the test asserts by
  variant; with Core's off, the refusal that surfaces is policy's `SameActor` and then the log's, so each layer's own
  protection isn't shown separately beyond that.

### T-020-32, the review of `8e6802cd`: changes

- **Should-fix, fixed: an empty answer was cached as asked.** `keep_asked` asked the grading once per flight and cached
  the answer, so a book empty (or a record failing the orders filter) at the first pass left `kept` as an empty `Some`,
  and a segment graded again after it waited with a `Short` was never asked again: the node stayed unbound and
  `ask_reversal` never completed. It now asks at every grading until the segment settles, caches only the steps it
  built (each key once) and appends what isn't appended; fail-closed holds (nothing binds until a record is there).
  Test `a_record_missing_at_the_first_ask_is_asked_for_again_when_the_grading_waited`
  (`conformance/scenarios/tests/publishers_record_retry.rs`, `REQ-ORD-3`): a grading whose book is empty at the first
  ask and which waits, then holds the record and settles; the record is kept at the second grading and appended once.
  Switched off once (the old caching restored): red (`[0, 0]` kept at the two gradings, not `[0, 1]`).
- **Not rebuilt after a restart:** in-flight segments, and so `kept`, aren't rebuilt from the log, as `graded` isn't
  (`F-030`'s durable `Segments`); a segment mid-flight at a restart is asked again on its next grading, and a record
  already appended is appended a second time (a second stated entry of the same bytes' commitment is the log's to
  accept).
- **Residual for `T-025-05` (low):** the early record that binds a node and the later one the replay cites are never
  compared, so a publisher's two records of one node can disagree unnoticed.

## T-020-38: `core-internal`'s round after `T-020-36`, a report pair's two segments

Branch `f-020/t-020-38-report-pair-segments`, from `m3` at `c874f7aa` (with `T-020-30` and `T-020-36` merged). Edits
`contracts/` (core-internal, a `DEC-20` path: it waits for the owner's click at the release into `main`), and
`core-audit`'s `tests/log.rs` (`F-022`'s), for the real log's path.

- **The finding** (`T-020-35`, confirmed by its review). A `ReportPair` held one `segment`, and both statement rows read
  each report's `about` against it, so a pair needed two parties' verified statements about one segment. At a
  segment's end Core holds only that segment's provider's own result; the next provider's report of receiving is its
  own result, about the next segment. So no caller could build a pair the log admits.
- **Versioning: folded into 1.0.0, no bump** (lesson 7): core-internal 1.0.0 isn't on `develop`. The changelog's last
  entry names each item; `contracts/CHANGELOG.md` takes a line.
- **Design** (the reviewer's suggestion, taken whole).
  1. `Report.segment: Id`, required (schema and type). `ReportPair.segment` stays, and is the sent report's
     (`ReportPair::check`: "a report pair's segment is the one its sent report is about"); the received report's is
     another ("… is about another segment than its sent report"). The old two-parties check stands.
  2. `sent-report-is-its-partys-statement` and `received-report-is-its-partys-statement` read `about` against the
     report's own segment (`sent.segment`, `received.segment`).
  3. New exists row `received-report-is-about-the-next-segment` (`REQ-ORD-9`): a verified plan input of the pair's
     order (a `route_chosen` part, a `route_recalculated`, a `destination_changed`: each the route a decision record
     backs, `input-cites-its-decision-record`) lists the received segment, and lists the sent segment right before
     it (field `next`). A route's last segment is followed by nothing, so it has no pair.
  4. New exists row `report-pair-is-by-its-segments-providers` (`REQ-ORD-9`, `REQ-AUD-2`): for each report (two
     citations, sent and received), a `move_value` effect of the pair's order on that report's segment, whose
     `provider` is the report's `party` (field `provider`).
  - The terms are segment n's: the pair compares n's `amount_out` with n+1's `amount_in` under n's tuple's increment
    and gross or net convention (`T-020-30`). A gross `amount_in` is no matter, the charge taken off being n's.
- **Security reading.** The received report must be about the route's actual next segment (row 3, read from the plan,
  never from the pair) and be its own party's (the maker check, now on its own segment, and row 4, which ties the
  party to the provider the log moved that segment with). One party can't report both sides: the check refuses equal
  parties, so a provider running two adjacent segments has no pair, and row 4 refuses any party that didn't move its
  segment. A report of another order is refused by the statement rows (`about`'s order, and its transaction's
  binding); one of another route of the same order by row 3 (a plan that lists the two segments consecutively) and
  row 4 (a segment no `move_value` ran: a superseded plan's, or a never-started one). Rule-off 3 shows the two rows
  overlap: with row 3 off, row 4 still refuses the recalculated route's first segment, which the world never moved.
- **`core-audit`'s `compare_reports`** (`F-022`) is untouched: it reads `pair.segment`, still the segment whose end is
  compared, and names it in the `Difference`; `tests/compare.rs` passes on the new shape, its world pair the new one.
- **Fixtures.** `scratchpad/f020zo/migrate.py` rewrote every fixture's pair (437 files): each report names its
  segment (`seg-2`/`seg-3` for the hand-built pairs), and the world's pair is now us-bank's result about `p1-s1` as
  sent and fx-desk's about `p1-s2` (134) as received. us-bank had no result about `p1-s1` in the world, so a new
  stated entry (receipt 440, under Core's question `tx-b1`, 110) is inserted before the pair in every history that
  holds it. `scratchpad/f020zo/gen.py` regenerated the ten statement-row cases on that base and wrote the new ones:
  two `--about-*` cases, five for row 3, three for row 4, and three invalid pairs (two the Rust types refuse, one the
  schema).
- **Histories the log refused, sorted.** One, a test artefact: the world's old pair (bridge-pay's result about `p1-s1`
  sent, fx-desk's about `p1-s1` received) under the new rows, refused for the received report's segment and, past
  that, for bridge-pay, which never moved `p1-s1` (us-bank did). It predates the shape; rewritten as above. No Core
  bug: no orchestration path builds a pair yet (`T-020-35`'s).
- **Real log's path.** `core-audit`'s `a_report_pair_of_a_segment_and_the_next_is_admitted_on_the_log_and_any_other_refused`
  replays the world on `AuditLog` (the new statement 440 signed for real and attributed by the log), finds the
  world's pair admitted, admits it again, and refuses, built in memory: fx-desk's `p1-s6` (the recalculated route's
  first) as received (`next`), fx-desk's result about `p1-s1` as received for `p1-s2` (`about`), bridge-pay's
  `p1-s1` as sent (`provider`), and us-bank on both sides (the two-parties check).
- **Named tests** (`contracts/tests/core-internal.rs`, each `// verifies: REQ-ORD-9`):
  `report_pair_whose_received_report_is_about_the_next_segment_of_the_route_is_admitted`,
  `report_pair_whose_received_report_is_about_a_segment_not_next_on_the_route_is_refused`,
  `report_pair_whose_two_reports_are_one_partys_is_invalid`. Written before the rows: the first failed on its
  assertion (the old received row refused the world's pair, "about its pair's order and segment"), the second on its
  missing row; the check was written with its shape, and its red is rule-offs 1 and 2. `T-020-36`'s and `T-022-05`'s
  tests pass unchanged beside them.
- **Defaults** (reversible, each failing closed):
  - fold into 1.0.0;
  - `pair.segment` kept, equal to `sent.segment`, so the comparison and its `Difference` read as before;
  - "next" is the immediate successor in one plan that lists both; a pair spans one plan, so a recalculated or changed
    route's first segment makes no pair with the settled segment it starts from;
  - a report's party is the provider of the `move_value` effect on its segment (not the plan's provider), so a
    segment never run has no report;
  - a provider running two adjacent segments has no pair (one party's word compares nothing).
- **Gaps, with owners.**
  1. *Across a re-plan.* The settled segment a `route_recalculated` or `destination_changed` starts from and the new
     route's first segment make no pair; if that end needs comparing, a later round can admit it, read from the part's
     latest settled segment before the re-plan, as `recalculation-starts-from-evidenced-value` reads it. Owner: the
     lead (a later `F-020` round), raised by `T-020-35` if its scenarios meet it.
  2. *Hand-off to `T-020-35`*, which builds the pair in `segments/` after this round merges: the sent report from
     segment n's result and its provider, about n; the received one from n+1's, about n+1; the terms from n's tuple;
     no pair after a route's last segment, nor where one provider runs n and n+1. Owner: `T-020-35`.
  3. *A report's shape.* Closed by the review of `8a6854b1` (below): both statement rows hold the statement to a
     `result` about the report's own segment.
  4. *A report's figures.* That a report's `amount` and `asset` are what its statement's bytes state, the log can't
     read from a commitment (as before this round): Core's, on taking the result, and a checker's on
     `Evidence::fetch`. Owner: `T-020-35` (Core builds the pair from the results it took).
  5. The workspace suite, the scenarios beyond `person_records`, and `cargo doc --workspace` run in CI.
- **Rule-off table** (each switched off alone from the committed round `2f91c3d4`, its tests run under a 300 s alarm
  through the lock, the file restored with `git checkout`; scripts `scratchpad/f020zo/ruleoff.py` and `run_off.sh`;
  none hung). "Table" is `every_relation_of_the_citation_table_holds_and_breaks_as_its_cases_say`, "fixtures"
  `every_valid_fixture_validates_and_every_invalid_one_is_rejected`, "log" `core-audit`'s
  `a_report_pair_of_a_segment_and_the_next_is_admitted_on_the_log_and_any_other_refused`.

  | Rule switched off | Red |
  |---|---|
  | `ReportPair::check`: the pair's segment is the sent report's | `report_pair_whose_two_reports_are_one_partys_is_invalid`, fixtures |
  | `ReportPair::check`: the two reports about two segments | the same two |
  | `received-report-is-about-the-next-segment` whole (cites none) | `report_pair_whose_received_report_is_about_a_segment_not_next_on_the_route_is_refused`, table, log (row 4 then refuses the recalculated route's first segment, which the world never moved) |
  | `report-pair-is-by-its-segments-providers` whole (cites none) | `report_pair_whose_received_report_is_about_the_next_segment_of_the_route_is_admitted`, table, log (bridge-pay's sent report admitted) |
  | `received-report-is-its-partys-statement`: `about` against the pair's segment again | both admitted and refused named tests, table, `every_entry_of_a_complete_log_is_admitted_on_what_came_before_it`, log (the world's replay refuses its pair) |
  | the schema's `segment` required on a report | `report_pair_whose_two_reports_are_one_partys_is_invalid`, fixtures |

  The sent row's re-pointing (`sent.segment` for `pair.segment`) is no rule of its own: the check makes them equal.
- **Local checks** (one lock hold, at `2f91c3d4`): `cargo fmt --all -- --check`, `cargo clippy --workspace
  --all-targets -- -D warnings`, the contracts' `no_std` build (`riscv32imac-unknown-none-elf`), `cargo test -p
  rutent-contract-guard`, the guard against `origin/develop` ("1730 changed path(s) … no violations"), `cargo test` of
  `rutent-contracts` and `rutent-core-audit`, and, since their tests read the admission fixtures, `rutent-stm`'s
  `log_oracle`, `rutent-core-authz --lib` and `rutent-scenarios`' `person_records`; plan-check and tag-check
  `--strict` (0 findings, 2078 tests tagged); `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` of the two crates:
  all green. One earlier run of `core-audit`'s `reopen.rs` failed `store_whose_verified_entrys_statement_was_erased_still_opens`
  at `EvidenceStore::open` (`Unavailable`), the flake `T-020-36` saw; it passed alone and in the final run.

### T-020-38, the review of `8a6854b1`: changes

- **Should-fix (gap 3, worse than noted).** Both statement rows went through `statement_by` and `about_agrees`, which
  takes a statement with no `about` on its body, so any verified statement of the right provider backed a report: on
  the real log the reviewer had admitted, as sent, us-bank's judgement or result about nothing, and a hold or terms
  about `tx-b1`/`p1-s1`; as received, fx-desk's judgement about nothing and an arrival notice about `tx-f2`/`p1-s2`.
  Both rows now read `report_statement`: a new field `result` (provider-role's `result` shape) between `verified` and
  `maker`, and `about` requiring an `about` naming the pair's order and the report's own segment, through a
  transaction first bound to that segment (`about_the_segment`), as the showing-of-fault and charge-raise rows hold
  theirs. Four refused cases: `--result` (a hold for the sent row, an arrival notice for the received) and
  `--a-statement-about-nothing` (the provider's result, about nothing) on each row (`scratchpad/f020zo/gen2.py`).
  `core-audit`'s `a_report_pair_on_the_log_rests_only_on_its_partys_results_about_its_own_segments` runs the
  reviewer's eight shapes on the real log, each refused (a judgement about the segment the log leaves unverified, a
  judgement naming no transaction, so `verified` refuses it first). Gap 3 above is closed.
- **Low.** `core-audit`'s report-pair test now refuses bridge-pay's result about `p1-s2` (appended and signed on the
  real log) as the received report: the received side of `report-pair-is-by-its-segments-providers`.
- **Rule-offs** (as before, from `2a7b1bca`; none hung):

  | Rule switched off | Red |
  |---|---|
  | the statement rows' `result` (any shape) | `report_pair_whose_received_report_is_about_the_next_segment_of_the_route_is_admitted`, table, `a_report_pair_on_the_log_rests_only_on_its_partys_results_about_its_own_segments` |
  | their `about` required and bound (back to `about_agrees`) | the same three (on the log, us-bank's result about nothing admitted as the sent report) |
  | `report-pair-is-by-its-segments-providers` on the received side alone | the admitted named test, table, `a_report_pair_of_a_segment_and_the_next_is_admitted_on_the_log_and_any_other_refused` (bridge-pay's received report admitted) |
- **Local checks** (one lock hold, at `2a7b1bca`): the same set as the first round, all green; the guard against
  `origin/develop` "1734 changed path(s) … no violations", plan-check and tag-check `--strict` 0 findings (2079 tests
  tagged).


## T-020-35: the report pair, partly built; `T-020-25`'s two lows closed

Branch `f-020/t-020-35-report-pair-built`, from `933a7ad9`. Edits `components/core-orch/src/segments/drain.rs` and
`conformance/scenarios/tests/recorded_ends.rs`. No contract is edited.

- **The report pair is not built: a shape is missing (stopped, as the row asks).** `ReportPair` and the log's rows
  (`sent-report-is-its-partys-statement`, `received-report-is-its-partys-statement`) need two verified statements
  about one order **and one segment**, by two different parties, each citable with `about_agrees` (a statement about
  another segment, or through a transaction bound to another, is refused). Orchestration holds, at a segment's end,
  only these statements about it: the provider's own result and the operator's operation. The next party's report of
  receiving is its own result, about the **next** segment (its own transaction); a cross-module callee's result never
  reaches Core (`Events::calls` records the call, not the answer; `prov`'s `Platform` hands Core only the top-level
  result); a stuck move's confirming statement is another maker's, of the move's transaction, and in the kit states no
  figures (a `balance_verification`); an arrival notice is about the order and the reserve's own transaction, which
  `about_agrees` refuses beside a segment. So no pair of statements that the log would admit exists to be built on
  the running platform.
  Exact shape wanted, for the contract round (`core-internal`, `DEC-20`): `ReportPair` (and its two rows) take the
  receiving report as about the **next** segment of the route (e.g. `received.segment: Id`, with
  `received-report-is-its-partys-statement` reading `about` against it and holding it to be the segment after
  `pair.segment` in the order's route), so the provider's `amount_out` of segment n is compared with the next party's
  `amount_in` of segment n+1, both of which Core already holds and has judged. Orchestration then builds it in
  `segments/` where segment n+1's result counts, with segment n's tuple's terms (`quote::Terms`: increment and
  gross/net). Owner: the lead, as the next contract round; `T-020-35`'s pair part follows it.
  The two pair tests of the row (`segment ending with two reports hands core audit a pair with its tuple's terms`,
  `reports differing by less than one increment on the running platform raise nothing`) are therefore not written.
- **Closed: the wait while the log can't take the platform's record** (`T-020-25`'s gap 2). `outcome unknown waits while
  the log can't take the platform's record and is recorded after` (`recorded_ends.rs`): the withdrawal's module passes
  a call out and answers too late; the kit's fault queue makes a `HandOver` append unavailable for one drain's tries
  (`On::HandOver` already existed in the kit and is unchanged); the run answers `SegmentsError::Append(Unavailable)`,
  with no `unanswered` item and no `segment_outcome_unknown` input on the log; the next run appends the platform's
  record, then the input citing it, and the log admits every entry.
- **Closed: an operation with no answer and no transaction.** `prov`'s platform pushes an `UnknownOutcome` with an
  empty transaction for an operation it ran under none (`unwrap_or_default`). `drain::of_events` now refuses it by
  name, `Uncarried { what: "an operation that ended with no answer under no transaction" }`, in place of the bare
  "an identifier"; the drain is kept whole and fails on every try, as any uncarried drain does. Two unit tests
  (`drain.rs`): the refusal's name, and an operation under a transaction still carried.
- **Defaults (reversible, failing closed).** The refusal keeps the drain whole and loud (no item is skipped); the
  wait test uses the existing fail-closed hold unchanged.
- **Gaps, with owners.**
  1. The report pair (above). Owner: the lead (a contract round for the shape, then this row's pair part).
  2. The empty transaction itself comes from `prov`'s platform (`platform.rs`, `refused_after`); whether the platform
     should refuse an operation that has no transaction before it runs is `F-013`'s. Not edited here.
- **Rule-off table** (each once, run under a 300 s alarm through the lock, file restored with `git checkout`; none hung):

  | Rule switched off | Red |
  |---|---|
  | `account`'s hold while the platform's record isn't on the log (`reported && record.is_none()`) | `outcome unknown waits while the log can't take the platform's record and is recorded after` |
  | `of_events`' refusal of an empty transaction | `operation that ended with no answer and no transaction is refused by name` |

  The two tests were written after their rules, which already existed (the hold, `T-020-25`) or were one line (the
  name); the rule-offs are their check.


### T-020-35, part two, the review of `53c32685`: changes

- **Merged** `origin/m3` (`5915aa63`) and `T-020-38`'s merged-up head `89ca70e1`; notes kept both sides.
- **Should-fix: a pair never fails a settlement.** `pair_with_previous(...)?` returned from `settle_on` after the result,
  fees and exposure release were on the log, so `finish` and `measure_end` never ran and the part stopped; a
  deterministic refusal would have halted it for good (ADR-0009: a comparison is a detection). `settle_on` now calls
  `Segments::report_end`, which fails nothing: what can't be built (no sent report held, a figure that is no amount,
  the contract refusing the pair) is parked in `Segments::unbuilt_pairs()` (`UnbuiltPair {order, segment, why}`;
  `SegmentsError::Pair` is gone), and the settlement goes on. `Core::unbuilt_reports()` and `unpaired_reports()` (waiting
  plus parked) surface it. Test: `pair that can't be built leaves the segment settled and the part running to its end`
  (the sent reports lost, as a restart would: `Segments::lose_sent_reports`, doc-hidden; four settled segments,
  the order complete, three parked pairs naming `NoSentReport`, none written) and `a pair without the sent report or
  holding a charge above it is unbuilt` (unit: a gross charge rounded above what was sent is `Refused`).
- **Low: a refused pair blocked the queue.** `write_pairs` parks a pair the log refuses (with the violation) and goes on to
  the ones after it; only unavailability stops, leaving the pair and those after it waiting. Test: `a pair the log
  refuses is parked and the pairs after it still go`.
- **Low: the increment is carried.** `Shape.increment` (`conformance/src/role/stand_in.rs`, an `F-024` file; the commit
  body says so); `sim-join-in` declares 10. The first test asserts pair 0's `terms.increment` is 10 in the reports'
  unit and the others' none, so a tuple's increment goes through `report_terms` to a built pair on the running
  platform. The two difference tests no longer set the increment; they restate only the receiving figure (nine, then
  ten, less than the sending one), and their doc comments say so.
- **Rule-offs** (each alone, under a 300 or 400 s alarm, restored with `git checkout`; none hung):

  | Rule switched off | Red |
  |---|---|
  | an unbuilt pair is not parked | `pair that can't be built leaves the segment settled and the part running to its end` |
  | a refused pair stays at the queue's head | `a pair the log refuses is parked and the pairs after it still go` |
  | the tuple's increment dropped from the pair's terms | the pair-with-terms test and the within-one-increment test |

## The owner's answers of 2026-11-18, in the plan (2026-11-23)

`PLAN.md` gains `T-020-39` to `T-020-43` (waves 33 and 34, and `T-020-42` in 35) for the items `rollback-permission`,
`confirmer-evidence-party`, `erasure-of-cited-evidence`, `deposit-no-firm-rate` and `shared-movement-reading`, with
`T-020-39` the contract round that carries their shapes. The router's half of `shared-movement-reading` is
`T-020-43`'s, not an `F-011` row: an `M2` feature's row can't wait on an `M3` one (`plan-check`), so the router's search
is an outer library's code that `T-020-43`'s Work names, as `T-020-02` edited the router's `quote` module.

### The origin check of 2026-11-23

The owner answered `origin-account-change` "Payer only" and `third-party-payer-authorises` "Requester only", read
together as: in v0 the origin stays as the first authorisation set it. Read against `m3` at `e63afbeb`:

- **Orchestration holds to it.** Core builds an authorisation in three places, each the order's first or a carry-on
  of it: intake (`components/core-orch/src/intake/mod.rs:863`, the origin account from the order's inputs at `:870`),
  a carry-on by another login of the requester's party, which reuses the accepted authorisation whole
  (`intake/mod.rs:691`), and arrival's filing (`components/core-orch/src/arrival/filing.rs:283`, the arrival's own
  account at `:287`; its carry-on at `:383`, `..self.authorisation.clone()` at `:387`). The one path for new terms,
  the customer's choice after a failed final deposit (`hand_on/mod.rs:232`), refuses any change of the origin reserve
  or account (`components/core-orch/src/hand_on/rule.rs:119` to `:121`, `Refused::ChangesOrigin`). No path of
  `core-orch` moves an origin account.
- **Policy is wider.** `core-authz` lets the order's payer move a part's origin account after the first authorisation:
  `origin_in_force` takes each later authorisation the payer gave (`components/core-authz/src/funds/mod.rs:306`), and
  a move is refused only where the giver isn't the payer (`funds/mod.rs:466` to `:470`, `by_the_payer` at `:201`).
  The log's row `only-the-payer-moves-the-origin-account` is the same rule
  (`contracts/src/core_internal/relations.rs:4988` and `:4999`). "Requester only" holds: a customer-role actor
  authorises only for the order's requester (`components/core-authz/src/roles/mod.rs:270`; `funds/mod.rs:68`), so a
  third-party payer gives no authorisation. But where the payer is the requester, the common case, policy and the
  log would permit a move of the origin account, and only orchestration's own guard stops it.
- **So the code holds to the reading only through orchestration**, not through the decision call. `T-021-11` makes
  policy refuse any move of an origin account once one is in force, the payer's included, so the decision call holds
  what orchestration already does. A part whose authorisation in force names no account yet may have one named once
  (`origin_in_force` answers `Some(None)` there, `funds/mod.rs:303`; `sets_account` at `:476`), and it is held from
  then. The reading is the owner's own, not one for the form: the notes on both items say the origin is fixed at the
  first authorisation and nobody changes it in v0, and to widen both together later. That is also why the contract's
  `only-the-payer-moves-the-origin-account` stays as it is: it is the widened rule, kept for then, and policy holds
  within it until both are widened.
- **`false-judgement-stuck-bearer`'s open half.** `SPEC.md` 0.4.2 has `REQ-ORD-28` govern a stuck order a false
  judgement left: the operator bears it, then recovers under contract, the reliance record (`T-020-23`) naming the
  judge and the provider that relied on it. From whom it recovers is not decided: form item
  `false-judgement-recovery`.

## T-020-39: the contract round for the owner's answers of 2026-11-18

Branch `f-020/t-020-39-contract-round-1118`, from `m3` at `391f92c4`. Edits `contracts/` (core-internal,
provider-role, router-api) and `components/core-authz/` and `components/core-audit/` (`DEC-20` paths: they wait for
the owner's click at the release into `main`), and the consumer files below.

- **Versioning: folded, no bump.** core-internal 1.0.0 isn't on `develop`; provider-role's 3.1.0 and router-api's
  4.0.0 are unreleased (`develop` holds 3.0.0 of each). Each changelog's entry names its items; `contracts/CHANGELOG.md`
  takes a line.
- **Item by item.**
  1. `rollback-permission`: `CatalogueChange::RollBack` (`roll_back`), about one release (`is_about_a_release`), naming
     the staged version it ends. `core-authz`'s `roles/mod.rs` answers it with no rule (`None`), so the fixed rule
     refuses it whoever asks, before the deployment's policy, until `T-021-12`. In `engine/mod.rs`'s
     `every_decided_action` and `roles/tests.rs`'s `every_permission` it sits in the match as `unreachable!`, not in
     the arrays, so their counts stay; `T-021-12` lists it and moves them by one.
  2. `confirmer-evidence-party`: `outcome-confirmation-cites-verified-evidence` (a report's, confirmation's and
     decline's evidence) and `part-evidence-rests-on-statements` gain a field `party`, read by `party_to_the_order`:
     the statement's maker is a provider of a segment on any plan this log holds for the order or an order a hand-on
     joins it to (`route_chosen`, `route_recalculated`, `destination_changed`; `linked_orders` closes over hand-on
     inputs, `resolve_stuck` hand-ons and their decide steps, holding withdrawals and routes handed on, both ways), or
     the provider a `move_value` of those orders went to; a holder of a holding account its value rested at (the
     reserve of an `arrival` of the order, a stuck resolution's `by`, a `holding_withdrawal`'s holder;
     after the review of `27b0f6b9`, a decide step's `by` only where policy permitted it, and no `holding_check`); or it states for the publisher of a private node those plans cross, as
     `private_node_of` binds it, or the publisher a `holding_withdrawal` names.
  3. `erasure-of-cited-evidence`: `Action::EraseStatement.overrides_citations: bool`, `#[serde(default)]` and skipped
     when false, so every earlier request, entry and digest reads and writes as before. The schema's property is a
     boolean. `erasure-waits-while-anything-rests-on-the-statement` cites nothing where `overrides_citations` (the
     helper) holds: a decide entry's own action, an effect's permitting decision through its permit's receipt (never
     the effect's own word), an erasure record's through an erase effect of its commitment this log holds `done`,
     whose decision sets it. `nothing-cites-an-erased-statement` is unchanged: once erased, nothing new cites it.
  4. `deposit-no-firm-rate`: `AcceptanceBasis::GivenOnDeposit` (`given_on_deposit`), with `on_a_deposit()` and
     `in_a_session()`. Its check: a session, no yes (its own message). The deposit rows read `on_a_deposit`
     (`automatic-deposit-rests-on-its-arrival`, `move-rests-on-its-orders-acceptance`, `first-move-rests-on-its-parts-yes`,
     `chosen-route-rests-on-its-orders-acceptance`, `deposited` for `split-deposit-arrival-names-its-part`); the quote
     row reads `in_a_session` (`acceptance-is-of-the-quote-shown`), and the session row already cites any session.
  5. `shared-movement-reading`, provider-role: `ReserveTuple.payouts: Option<Payouts {PerOrder, Batched}>`,
     `ReserveTuple::payouts()` reading absent as per order; `RuleViolation::PayoutsOnAWithdrawal` in `validate`
     (the schema states the enum, the Rust validation the direction, as for a ramp within one node).
  6. router-api: `DropReason::BatchedPayout {part, operation}`; `Candidate::check` holds it to the part's last
     operation, the record's check to a deposit tuple of the snapshot whose `payouts()` is batched. The snapshot's
     `reserve_tuple` figure is provider-role's shape by reference, so it carries `payouts` with no field of
     router-api's own.
  7. `thr3-backdated-after-revocation`: `StatementKeys::signs_for(maker, key, at, received)`; `attribute`'s step 8
     passes the entry's `received_at`. The rule is the contract's function `revoked_before_receipt(history, maker,
     made_at, received)`: each grant (tuple or class) of the maker in force at `made_at`, by effect times of changes
     held `done` as `classes_alone` reads them, revoked after `made_at` and no later than `received`. The trait's doc,
     `calls.rs`'s sentence that called it a residual and the README's "Whose keys" state the rule.
- **Differences from the row, reported.**
  - The row's "and the decision's own" is read as `part-evidence-rests-on-statements`, the stuck resolution's own
    evidence entry that `stuck_resolved` cites. `StuckDecide.evidence` is a list of digests no row holds to any
    statement, so there was no row of the decide step's own to widen (gap 1).
  - router-api's snapshot needed no field and `core-orch`'s `snapshot/` no edit: the tuple figure already is
    provider-role's tuple, so `payouts` rides on it.
  - No exhaustive match on drop reasons exists outside `contracts/` (`DropReason` is `#[non_exhaustive]`):
    `intake/class.rs`, `arrival/filing.rs` and the router's are `matches!`, where `batched_payout` reads as "no route
    right now", which is right for it; `rounds/`, `recalc/`, `components/router/` and the router suite hold none. No
    edit there.
  - No exhaustive match on `AcceptanceBasis` exists in `core-orch` or the assembly (only `==`), so none was edited.
    `hand_on/rule.rs`'s `customers_own` and `hand-on-input-rests-on-the-linked-orders-acceptance`'s `given` still mean
    `given` alone.
  - No `ReserveTuple` literal exists in the guests; the workspace's are `conformance/src/role/stand_in.rs`'s two.
  - The rule of item 7 is a contract function the key sources call, not a step `attribute` applies itself:
    applied in `attribute`, `core-audit`'s `a_verifiers_class_grant_is_read_at_its_statements_own_time` ("its class,
    made inside the window", received five seconds after the revocation, asserted verified) would go red at this
    round's commit, which the row forbids. `core-audit`'s register ignores the new argument until `T-022-09` (gap 2).
- **Consumer edits** (to compile, no behaviour changed): `overrides_citations: false` in
  `components/core-orch/src/erasure.rs` and `conformance/scenarios/tests/authz_scenarios.rs`; `RollBack` in
  `components/core-authz/src/engine/mod.rs` and `roles/tests.rs` (`unreachable!`) and `roles/mod.rs` (no rule);
  `payouts: None` in `conformance/src/role/stand_in.rs`; `signs_for`'s fourth argument in
  `components/core-audit/src/log/keys.rs` (ignored) and the fakes of `components/core-orch/tests/{intake_support/mod.rs,
  reversal.rs, rounds_support/mod.rs, snapshot_statements.rs}` and `conformance/scenarios/tests/segments_support/mod.rs`.
- **Named tests.** `contracts/tests/core-internal.rs`: `roll_back_is_a_catalogue_change_of_its_own` [`REQ-AUZ-6`],
  `stuck_resolution_evidence_made_outside_the_order_is_refused_by_the_log` [`REQ-AUZ-4`],
  `erasure_of_a_cited_statement_is_admitted_only_on_a_decision_that_overrides_the_citations` [`REQ-AUD-13`],
  `customers_acceptance_of_a_filed_orders_slow_route_has_a_basis_of_its_own` [`REQ-QTE-10`],
  `statement_received_after_its_grants_revocation_is_unattributed_by_the_contracts_rule` [`THR-3`];
  `contracts/tests/provider-role.rs`: `deposit_tuples_payouts_are_read_as_per_order_where_undeclared`,
  `deposit_tuple_declaring_batched_payouts_is_valid` [`REQ-ORD-33`]; `contracts/tests/router-api.rs`:
  `candidate_dropped_for_a_batched_payout_names_its_reason` [`REQ-AUD-4`]; `components/core-authz/src/roles/tests.rs`:
  `roll_back_is_refused_while_no_rule_answers_it` [`REQ-AUZ-1`]. Written before the rules, on the shapes: the
  erasure, acceptance and evidence tests and the citation table failed on their assertions (an override refused while
  cited; a deposit acceptance with no arrival admitted; no field `party`), router-api's three on Rust accepting both
  invalid fixtures. The shape-only tests (roll back, the payouts pair, the revocation rule written with its helper)
  show their red in the rule-offs.
- **Fixtures** (`scratchpad/f020zp/gen.py`, `gen2.py`, from the world's cases): core-internal 2 valid, 3 invalid, 16
  admission cases (4 refused and 2 admitted for `party`; 3 admitted and 1 refused for the override; 2 admitted and 4
  refused for the new basis), 6 attribution cases; provider-role 1 valid, 2 invalid; router-api 1 valid, 2 patched
  invalid.
- **Histories the log refused, sorted.** None: every world case and earlier admission case passed under the new rows
  unchanged (the world's outcome evidence is us-bank's, fx-desk's, eu-bank's or bank-co's, each on a route of `ord-1`
  or the resolution's mover).
- **Defaults** (reversible, each failing closed):
  - fold into each contract's unreleased version;
  - a decline's evidence is held to a party too (the row's own field), so an outside provider's word can't decline
    either;
  - "a party the order involves" includes the reserve of its arrival, a stuck resolution's mover (`by`, effected, or
    decided by a permitted decide step), and a holding withdrawal's holder and publisher (no holding check, after the
    review of `27b0f6b9`), and the orders a hand-on
    joins, both ways and transitively; and nothing else: not the authorisation's destination reserve unless a plan
    or a move names it, never the customer;
  - a node's publisher is read only where the log binds it (`private_node_of`: a replay or evidence of the order);
  - `overrides_citations` is written only when true; an explicit `false` on the wire reads as unset;
  - the override is read from the permitting decision, an erasure record's from an erase effect held `done`;
  - `given_on_deposit` is cited by the quote row (the customer accepts a quote shown, `T-020-07`'s slow segments
    named), and is no customer's `given` for a hand-on;
  - a revocation at the very instant of receipt counts as before it (unattributed); a grant made again after the
    revocation attributes only what is made under it;
  - `payouts` on a withdrawal tuple is a Rust rule, the schema only stating the two values;
  - router-api's record check holds a `batched_payout` reason to its tuple but doesn't yet refuse a kept candidate
    ending at a batched tuple (the router doesn't drop one until `T-020-43`).
- **Gaps, with owners.**
  1. `StuckDecide.evidence` digests are held to no statement by any row; orchestration's `stuck/` is to hold them to
     a party (`T-020-40`), and a row for the decide step's digests is the lead's to add if a reader needs it.
  2. `core-audit`'s register holds the rule from `T-022-09`; that row must turn
     `a_verifiers_class_grant_is_read_at_its_statements_own_time`'s "its class, made inside the window" case
     (received after the revocation) to expect it unattributed. Owner: `T-022-09`.
  3. A kept candidate ending at a batched tuple: the router's search and Core's route guard are `T-020-43`'s; a
     router-api rule refusing such a kept candidate could follow there.
  4. The log can't read that a `given_on_deposit` acceptance's route has no firm rate; `T-020-41` holds it in
     `arrival/`.
  5. A private node crossed with no replay or evidence yet binds no publisher, so its publisher's word is refused
     until one does. Owner: `T-020-40` if its scenarios meet it, else the lead.
  6. Policy's separate permission for the override is `T-021-13`'s, the store's `T-022-10`'s, orchestration's
     `T-020-42`'s; roll back's permission `T-021-12`'s.
  7. The workspace suite runs in CI.
  8. No case moves a later segment, chooses a route or splits an order accepted given on a deposit, so the three rows
     that widen for it there are shown by no red. `T-020-41` builds that flow and its scenarios exercise them.
- **Contract guard.** Against `origin/develop` (`fb883e01`): "1778 changed path(s) … no violations". CI's guard runs
  against the pull request's base, `m3`, where the three folded versions equal the base's and it fails, as it did on
  `T-020-38`'s #237: the fold is lesson 7's, read against `develop`, the milestone's base.
- **Local checks** (through the lock): `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -D
  warnings`, `cargo test -p rutent-contracts`, `cargo test -p rutent-core-authz --lib`, plan-check and tag-check
  `--strict` (0 findings, 2105 tests tagged), the guard as above. The suites of `rutent-core-orch`,
  `rutent-core-audit` and `rutent-scenarios` run in CI on #256.
- **Rule-off table** (each switched off alone from a committed checkpoint, its tests run through the lock, the file
  restored with `git checkout`; script `scratchpad/f020zp/ruleoff.py`; none hung). "Table" is
  `every_relation_of_the_citation_table_holds_and_breaks_as_its_cases_say`, "fixtures" the two
  `every_valid_fixture_…`/`every_invalid_fixture_…` tests of the contract, "evidence"
  `stuck_resolution_evidence_made_outside_the_order_is_refused_by_the_log`, "erasure", "basis", "revocation",
  "per order", "batched" and "dropped" the other named tests.

  | Rule switched off | Red |
  |---|---|
  | `roles/mod.rs`: `roll_back` answered by no rule (falls to its change's permission) | `roll_back_is_refused_while_no_rule_answers_it` |
  | `outcome-confirmation-cites-verified-evidence`'s `party` | evidence, table |
  | `part-evidence-rests-on-statements`'s `party` | evidence, table |
  | `party_to_the_order`: providers of the order's plans | evidence, table |
  | `party_to_the_order`: providers its moves went to | none: every move's provider is a plan's (`move-segment-is-in-its-orders-plan`); kept as defence |
  | `party_to_the_order`: a resolution's mover, its effect's arm alone | none: its decide step names the same mover |
  | the same, the effect's and the decide step's arms | evidence ("the resolution's mover", with the arrival taken out), table |
  | `party_to_the_order`: publishers of a node crossed (`private_node_of`) | evidence |
  | `linked_orders`: the hand-on closure | evidence ("a provider of the linked order's route") |
  | the erasure's override whole (cited as before) | erasure, table |
  | an effect's override read from its own action, not its decision's | none: `effect-permits-a-decided-action` makes the two equal first; kept as the row's own reading |
  | an erasure record's override from any erase effect, not its decision's | erasure (its decision unset, its effect still claiming it) |
  | `check.rs`: `given_on_deposit`'s session and no yes | basis, fixtures |
  | `automatic-deposit-rests-on-its-arrival` citing `automatic_deposit` alone | basis, table |
  | `acceptance-is-of-the-quote-shown` citing `given` alone | basis, table |
  | `first-move-rests-on-its-parts-yes`: the deposit arm `automatic_deposit` alone | basis, table |
  | `move-rests-on-its-orders-acceptance`, `chosen-route-rests-on-its-orders-acceptance`, `deposited`: `automatic_deposit` alone (each alone) | none: no case yet moves a later segment, chooses a route or splits an order accepted given on a deposit; each is a widening, so off it refuses more (gap 8) |
  | `revoked_before_receipt` never holds | revocation, `the_log_not_the_writer_attributes_a_stated_entry` |
  | `attribute` asking with `made_at` as the receipt | the same two |
  | a revocation counted before the making too | green on the first run; with `verified--a-statement-made-after-its-grant-was-made-again` (checkpoint 6): the same two |
  | `validate`: payouts on a withdrawal tuple | per order, fixtures |
  | `payouts()` reading absent as batched | per order, batched |
  | `Candidate::check`: a batched payout names the part's last operation | none: the record's half refuses the earlier operation's tuple first; kept as defence |
  | the record's check: the deposit tuple pays out in batches | dropped, fixtures |

### T-020-39, the review of `27b0f6b9`: changes

- **Should-fix 1, fixed: a holding check made any provider a party.** No row ties a `holding_check` to the order, so
  whoever writes one could name an outside provider. The arm is dropped from `party_to_the_order`; the holders the
  log binds stay (an arrival's reserve, a resolution's mover, a holding withdrawal's holder and publisher). Refused
  case `outcome-confirmation-cites-verified-evidence--a-confirmation-on-a-provider-a-holding-check-names`, the
  reviewer's probe.
- **Should-fix 2, fixed: a refused decide step named a party and joined orders.** `party_to_the_order` and
  `linked_orders` read a `stuck_decide` only where its outcome is `permitted`. Cases: `--a-confirmation-on-the-mover-a-
  permitted-decide-names` (admitted), `--…-a-refused-decide-names` (refused), `--a-confirmation-on-a-provider-of-an-order-
  a-permitted-decide-hands-on-to` (admitted), `--…-a-refused-decide-hands-on-to` (refused).
- **Should-fix 3, fixed: the erasure override was permitted on the plain erase permission.** `core-authz`'s
  `roles/mod.rs` answers `EraseStatement { overrides_citations: true, .. }` with no rule, so the fixed rule refuses it
  whoever asks until `T-021-13`, as `roll_back` until `T-021-12`. Test
  `erasure_overriding_citations_is_refused_while_no_rule_answers_it` [`REQ-AUZ-1`, `REQ-AUD-13`]; `T-021-13` replaces
  it when it gives the override its own permission (gap 6). Default: refused, failing closed.
- **Low 4, fixed: the two halves of the `batched_payout` check.** The candidate's half refuses "a batched-payout reason
  names its part's last operation", the record's "a batched-payout reason names a deposit at a reserve tuple that pays
  out in batches". New invalid fixture (patched), `decision-record-batched-payout-naming-an-earlier-deposit-at-a-
  batching-reserve`: the dropped part deposits at a batching reserve's tuple mid-route (`f-dep-co`, co-ach) and
  withdraws again; only the candidate's half refuses it.
- **Low 5, fixed: every arm in the rule-off table.** One admission case per way the log makes a maker a party, alone
  (`scratchpad/f020zp/gen3.py`): the resolution's effect, a permitted decide, an arrival's reserve, a holding
  withdrawal's holder and its publisher; and per way it joins an order through a hand-on: a hand-on input, a route
  handed on from the order, a holding withdrawal, a resolve effect, a permitted decide; and an order no hand-on joins
  (refused). Fourteen new admission cases (728 in all); `stuck_resolution_evidence_made_outside_the_order_is_refused_by_the_log`
  asserts each.
- **CI's failures at `115c97fc` (run `45333150064`), sorted.** 21 tests.
  - `conformance/tests/router_suite.rs`'s `the_suite_knows_every_drop_reason_the_contract_schema_names`: fixed,
    `batched_payout` in the suite's `DROP_REASONS` (`conformance/src/router/mod.rs`), which the contract's own record
    check recomputes; the router's cases for it are `T-020-43`'s.
  - `tools/records/tests/evidence.rs`'s `the_binary_reads_the_decisions_file_and_a_review_needs_notes_that_are_not_empty`:
    `#243`'s, fixed on `m3` at `bb0cd23e`, not on this base; not this row's.
  - 19 scenarios, each refused by the new `party` field and still refused after the fixes (run locally through the
    lock): `stuck.rs`'s `confirmation_names_the_report_it_confirms`, `confirmed_failure_frees_the_part_for_a_new_decision`,
    `declined_report_of_a_silent_move_is_followed_by_a_new_one_that_stands_until_it_is_confirmed`,
    `segments_move_confirmed_done_is_graded_and_then_settles_the_segment`,
    `segments_move_confirmed_done_whose_record_the_log_could_not_take_is_confirmed_once`,
    `segments_move_confirmed_done_with_no_graded_evidence_settles_nothing`,
    `silent_move_on_a_stuck_part_settles_on_two_actors_word_and_frees_the_part`,
    `unknown_move_takes_its_known_outcome_once_on_two_actors_word`; `stuck_mover.rs`'s
    `confirmation_of_a_declined_report_is_refused`, `decline_by_the_reporters_own_party_is_refused`,
    `decline_or_confirmation_the_log_refuses_is_a_refusal_that_changes_nothing`,
    `move_left_unknown_takes_its_known_outcome_from_the_providers_stated_result`,
    `part_stays_held_while_its_report_is_declined`,
    `provider_stated_failure_does_not_free_the_part_until_another_party_confirms_it`,
    `report_declined_by_its_confirmer_is_followed_by_a_new_report_that_stands_until_it_is_confirmed`,
    `report_whose_evidence_states_another_outcome_than_the_one_reported_is_refused`,
    `stuck_resolved_cites_an_evidence_entry_the_log_holds`; `stuck_hand_on.rs`'s
    `hand_on_failure_stated_by_the_holding_account_frees_the_part_only_on_another_partys_confirmation`,
    `hand_on_left_unknown_takes_its_known_outcome_from_the_holding_accounts_stated_answer`. **Test artefacts, not a
    Core bug:** each confirms or declines on `OTHER_RESERVE`'s (`sim-reserve-b`) result, a reserve the kit installs
    beside the order's but which runs no segment of any route the order was given, holds none of its value and
    is joined to it by no hand-on: the outside provider the owner's ruling refuses, put there under the overruled
    default of 2026-11-17 ("a confirmer's evidence maker need not be a party to the order"). Core passes the caller's
    evidence through as built (`T-020-40` refuses it in `stuck/` first). **Not fixed here, and the rule not relaxed:**
    the kit's order runs one reserve (withdrawal and deposit at `RESERVE`, on its own private ledger), so it has no
    second party; the confirmer can't be swapped for `RESERVE` either, since a confirmation needs another party than
    the report's (`outcome-confirmation-rests-on-its-report`). The fix is a kit world whose order involves a second
    provider (a route, or a recalculated route, through it), which is what `T-020-40`'s "statement by a provider on an
    earlier route of the order backs a resolution" needs. Owner: `T-020-40`, or the lead for a kit row. So `ci` stays
    red on these 19 until then (gap 9).
- **Gap 9 (new).** Beside the kit: under the ruling, a stuck part of an order whose routes involved one party only
  has no second party to confirm a silent resolution, so it stays held. Recorded for the lead, as a reading the
  owner may want to see (`REQ-AUZ-4`, `REQ-ORD-28`).
- **Rule-offs of the review's round** (from `79994eb2`, same script; none hung). "Evidence" and "table" as above:

  | Rule switched off | Red |
  |---|---|
  | `party_to_the_order`: plans' providers | evidence, table |
  | its moves' providers | none: a strict subset of the plans' (`move-segment-is-in-its-orders-plan`), kept as defence (the reviewer agreed) |
  | its resolve effect's mover | evidence, table |
  | its decide step's mover | evidence, table |
  | its decide step read whatever policy answered | evidence, table |
  | its arrival's reserve | evidence, table |
  | its holding withdrawal's holder | evidence, table |
  | its holding withdrawal's publisher | evidence, table |
  | its node publishers (`private_node_of`) | evidence |
  | `linked_orders` whole | evidence, table |
  | its hand-on input | evidence, table |
  | its route handed on from the order | evidence, table |
  | its holding withdrawal | evidence, table |
  | its resolve effect | evidence, table |
  | its decide step | evidence, table |
  | its decide step read whatever policy answered | evidence, table |
  | `roles/mod.rs`: the override answered by the plain erase permission | `erasure_overriding_citations_is_refused_while_no_rule_answers_it` |
  | `Candidate::check`: a batched payout names its part's last operation | `candidate_dropped_for_a_batched_payout_names_its_reason`, fixtures (the earlier deposit admitted) |
  | the record's check: the deposit tuple pays out in batches | the same three |
- **Local checks** (through the lock, at the fix commit): fmt, clippy `--workspace --all-targets -D warnings`,
  `rutent-contracts`, `rutent-core-authz --lib`, the three `stuck*` scenario binaries (19 failing as above, the rest
  passing), plan-check and tag-check `--strict`, the guard against `origin/develop`.

### T-020-39, fix round 2: the scenario kit's second party

`m3` takes only a green `ci`, so the 19 stuck scenarios of round 1's gap 9 are ported in this row (the coordinator,
fix round 2). The log rule is unchanged.

- **The world** (`conformance/scenarios/tests/segments_support/mod.rs`, `World::two_parties`; `T-020-40` reuses it
  for "statement by a provider on an earlier route of the order backs a resolution"). Two simulated reserves of two
  publishers on one public ledger (`conformance/src/sim/mod.rs`): `sim-duo-in`, between its bank and
  `sim-duo-in-ledger`, which the operator lists public, and `sim-duo-out`, between that ledger and its own bank. The
  kit's route withdraws at the first and deposits at the second, so the order's route was given through two
  providers of two parties, and the second's result about the order is a party's word however the route ends: in the
  no-route worlds the order is recalculated after `sim-duo-out` refuses, so it is a provider on an earlier route.
  The kit reads the world's two ends from a thread-local every `World` sets (`origin_reserve()`, `last_reserve()`,
  `second_reserve()`, the confirming party: the last reserve where it isn't the origin, `OTHER_RESERVE` otherwise),
  so every other world reads as before: the route's withdrawal, the arrival, the authorisation's origin and the
  reserve's yes at the origin; its deposit, the destination, a linked order's route and its yes at the last
  (`chosen_route`, `route_end`, `seed`, `seed_linked`, `handing::route_from_the_ledger`, `rerouting`, `resolving`).
- **Ported, each keeping its intent and its assertions** (its confirmer or decliner now `sim-duo-out`'s result, a
  provider of a segment on a route the order was given, whose party isn't the reporter's):
  - `stuck.rs`: the no-route world (`left_no_route`, `stuck_by_no_route`) and the silent-move worlds are two-party:
    `confirmation_names_the_report_it_confirms`, `confirmed_failure_frees_the_part_for_a_new_decision`,
    `unknown_move_takes_its_known_outcome_once_on_two_actors_word` (no route), and
    `declined_report_of_a_silent_move_is_followed_by_a_new_one_that_stands_until_it_is_confirmed`,
    `segments_move_confirmed_done_is_graded_and_then_settles_the_segment`,
    `segments_move_confirmed_done_whose_record_the_log_could_not_take_is_confirmed_once`,
    `segments_move_confirmed_done_with_no_graded_evidence_settles_nothing`,
    `silent_move_on_a_stuck_part_settles_on_two_actors_word_and_frees_the_part` (a silent withdrawal, the other party
    the route's deposit provider). The file's other 32 scenarios run in the same worlds, unchanged in intent.
  - `stuck_mover.rs`: `stuck_by_no_route` is two-party: `confirmation_of_a_declined_report_is_refused`,
    `decline_by_the_reporters_own_party_is_refused`, `decline_or_confirmation_the_log_refuses_is_a_refusal_that_changes_nothing`,
    `move_left_unknown_takes_its_known_outcome_from_the_providers_stated_result`,
    `part_stays_held_while_its_report_is_declined`,
    `provider_stated_failure_does_not_free_the_part_until_another_party_confirms_it`,
    `report_declined_by_its_confirmer_is_followed_by_a_new_report_that_stands_until_it_is_confirmed`,
    `report_whose_evidence_states_another_outcome_than_the_one_reported_is_refused`,
    `stuck_resolved_cites_an_evidence_entry_the_log_holds`. The four that deliver on to the destination
    (`decided_delivery_to_the_destination_is_carried_out_on_the_platform`, `failed_move_leaves_the_value_held_and_the_part_stuck`,
    `stalled_move_before_any_hand_over_is_dropped_when_the_customer_replaced_the_destination`,
    `stuck_delivery_of_an_input_resting_on_a_judgement_leaves_a_reliance_before_the_send`) stay at one reserve
    (`stuck_by_no_route_at_one_reserve`): only the reserve holding the value can pay it out, the simulated
    `sim-duo-out` holds none of it, and none of the four needs a second party's word.
  - `stuck_hand_on.rs`: `hand_on_failure_stated_by_the_holding_account_frees_the_part_only_on_another_partys_confirmation`
    and `hand_on_left_unknown_takes_its_known_outcome_from_the_holding_accounts_stated_answer` run
    `left_unknown_of_two_parties`: the linked order's route starts with the kit route's deposit, `sim-duo-out`'s,
    which says its yes and stands as its origin; the holding account stays `sim-duo-in`. The file's other scenarios
    keep the one-reserve world.
  - None of the 19 was about a confirmer from outside the order, so none became a refused case; the refused outside
    confirmation is pinned in the new scenario below.
- **The consequence, pinned** (`stuck.rs`, `silent_move_of_a_one_party_order_stays_held_for_want_of_a_confirming_party`,
  [`REQ-AUZ-4`, `INV-2`]): an order whose whole route one reserve ran, its resolution silent and reported done on that
  reserve's word. A confirmation on `OTHER_RESERVE`'s word, installed beside the route and running nothing of it, is
  the entry the log refuses (`Core::admitted` fails at it with the `party` message); with the log refusing it on
  append, it changes nothing; one on the reporter's own maker's word is refused; the part stays held and can't be
  decided again. The coordinator puts the consequence on the owner's form; until he rules, the test pins it.
- **`PLAN.md`**: the row's Work says it also gives the kit a second party and ports the stuck scenarios.

## T-020-43: routing ends no route on a reserve that batches payouts

Branch `f-020/t-020-43-batched-reserves`, from `m3` at `a3bc16b4`. Item `shared-movement-reading`, "First keeps it". No
`contracts/` edit: provider-role's `payouts` and router-api's `batched_payout` came with `T-020-39`.

- **What was built.**
  - *Router* (`components/router/`): `Segment::batches_payouts` (a deposit tuple whose `payouts()` is `batched`) and a
    step in `judge.rs`'s `fault`, right after the four cut-short reasons and before `excluded_by_policy` (the order
    of router-api's reasons): a path whose last segment batches is dropped as `BatchedPayout {part: 0, operation:
    last}`. Both rounds. A tuple declaring `per_order`, or nothing, is routed as before.
  - *Snapshot* (`snapshot/`): no edit. The catalogue's snapshot carries each reserve tuple as the advertisement
    states it (`prov/src/catalogue/snapshot.rs` clones the tuple), so `payouts` reaches the router on the figure
    (`T-020-39`'s note). `snapshot_batched.rs` pins that, through Core's own snapshot and the router.
  - *Route guard* (`closing/`, `segments/`): `closing::pays_in_batches(catalogue, deposit)` reads the provider's
    accepted advertisement for the reserve tuple with the deposit's ends and asset; `Unadmitted::BatchedPayout` and
    `RouteRefusal::BatchedPayout(place)` (`snapshot/route.rs`). The segments' `closable` check, which already ran at
    every place before the first segment is handed over (as `NoPublicRecord` does), refuses it first, naming the
    deposit's place. The refusal is a `no_route_right_now` one, recorded once like the others.
  - *Router suite* (`conformance/src/router/`): the policy case gains
    `f-dep-batched` and its expected drop, and a choice case, `batching-deposit-would-win-choice`, a free deposit at a
    batching reserve that would win the slow route and the round; the double (`tests/router_double/`) drops it and
    has two flaws, `ENDS_ON_BATCHING_RESERVE` and `BATCHING_DROPPED_ONLY_WHEN_SHORTLISTING`. Edits to `components/router/` and the suite are named
    here because the row makes them the Work.
- **Defaults** (reversible, failing closed where they can):
  - a reserve that declares no `payouts` is per order, as built (`T-020-39`); the owner's answer of 2026-11-18 stands
    as the most conservative reading and is on the form;
  - the guard reads the provider's *accepted advertisement*, not the snapshot, so a re-advertised tuple takes effect at
    once for any segment not yet handed over. `closable` runs at every segment not yet in flight and reads the
    deposit at the part's last place, so a reserve that turns `batched` mid-route is refused at the part's next
    segment (`reserve_that_turns_batched_mid_route_is_refused_at_the_next_segment`); only a part whose own deposit is
    already handed over escapes it (gap 1);
  - a deposit whose provider or tuple the catalogue doesn't hold reads as per order here: it is refused on other
    grounds (`ProviderNotListed`, `BelowEvidenceFloor`) at its own place;
  - `pays_in_batches` matches *any* reserve tuple of the provider with the deposit's origin, destination and asset,
    where the router drops by the one tuple figure its operation cites. A provider advertising two tuples with the
    same ends, one batched and one per order, has a route the router kept refused by Core. This fails closed and is
    left as it is (review of `269ccee8`, finding 4); nothing forbids two such tuples in an advertisement;
  - the router drops by the destination tuple's own declaration, per tuple, not per provider: a reserve may batch one
    asset or direction and pay another per order.
- **Gaps.**
  1. A part whose own deposit is already handed over when its reserve starts batching is not stopped (owner:
     `F-020`; the guard runs before a segment is first handed over, and at the deposit's own place it would be too
     late). The log's `movement-evidences-one-segment` row keeps it safe: the second order waits, as in
     `second_order_on_a_shared_final_movement_waits_while_the_first_keeps_it`.
  2. Ramps and exchanges carry no `payouts`; only a reserve's deposit can batch (provider-role's rule).
  3. Split routes (several parts) are read part by part by the contract's check, but the search builds one part per
     candidate (`DEC-8`), so the multi-part case is the contract's alone.
  4. A deposit at a batching reserve in the middle of a route (a deposit, then a withdrawal again), which
     `T-020-39`'s fixture "earlier deposit at a batching reserve" admits as a candidate-half case, is not read: only a
     part's last operation is (the owner's ruling is about a route's *end*, where the order's close rests on the
     movement). Its movement is relied on by the segment it settles, so `movement-evidences-one-segment` can hold a
     second order there too, as it would on any shared movement; it leaves the part waiting on evidence, not closing
     wrongly. Owner: `F-020`, with the owner's word on whether a middle deposit is in the ruling's scope; left as it
     is until he rules.
- **Named tests.** `components/router/tests/batched.rs`: `route_is_never_ended_on_a_reserve_that_batches_payouts`
  [`REQ-ORD-33`], `candidate_ending_on_a_batching_reserve_is_recorded_with_its_drop_reason` [`REQ-AUD-4`],
  `route_ended_on_a_reserve_that_pays_per_order_is_still_chosen` [`REQ-QTE-11`];
  `components/core-orch/tests/segments.rs`: `route_chosen_to_end_on_a_reserve_that_batches_payouts_is_refused_before_anything_is_handed_over`
  [`REQ-ORD-33`] and `route_ending_on_a_reserve_that_pays_per_order_is_handed_over`;
  `components/core-orch/tests/closing.rs`: `second_order_on_a_shared_final_movement_waits_while_the_first_keeps_it`
  [`REQ-ORD-33`] (pins behaviour that was there, so it passed at once; its rule-off is below);
  `components/core-orch/tests/snapshot_batched.rs`: `snapshot_hands_each_deposit_tuples_payouts_to_the_router`;
  the suite: `a_route_ending_on_a_reserve_that_batches_payouts_is_dropped_with_its_reason`,
  `a_kept_route_ending_on_a_reserve_that_batches_payouts_is_refused`,
  `a_router_that_ends_routes_on_a_batching_reserve_fails_only_where_one_is_advertised`.
  Added in fix round 1: `a_batching_deposit_that_would_win_the_choice_round_is_dropped_and_not_chosen`,
  `a_router_that_drops_a_batching_route_only_when_it_shortlists_fails_the_choice_case`,
  `reserve_that_turns_batched_mid_route_is_refused_at_the_next_segment`, and the contract's
  `candidate_dropped_for_a_batched_payout_names_its_reason` with its new invalid fixture.
- **Seen failing first.** The router's three on assertions (a kept batching route; no `BatchedPayout`; the batching
  reserve chosen), the guard's refusal with its check stubbed (the part was handed over), the suite's tests on the
  double and the cases before they dropped it (`left: None`, `findings: []`, the shortlist listing the batching
  route).
- **Histories the log refused, sorted.** None.
- **Rule-offs.** See the table below.

| Rule switched off | Went red |
|---|---|
| router: the `batches_payouts` step in `fault` | `route_is_never_ended_on_a_reserve_that_batches_payouts`, `candidate_ending_on_a_batching_reserve_is_recorded_with_its_drop_reason`, `route_ended_on_a_reserve_that_pays_per_order_is_still_chosen` |
| guard: `pays_in_batches` in the segments' `closable` | `route_chosen_to_end_on_a_reserve_that_batches_payouts_is_refused_before_anything_is_handed_over` |
| `Closing::free_for` always free (first keeps it) | `second_order_on_a_shared_final_movement_waits_while_the_first_keeps_it`, and three older closing tests |
| snapshot: the catalogue's snapshot strips `payouts` from a reserve tuple (`prov/src/catalogue/snapshot.rs`, put back) | `snapshot_hands_each_deposit_tuples_payouts_to_the_router` |
| double: its drop of a batching deposit (`ENDS_ON_BATCHING_RESERVE`) | `a_router_that_ends_routes_on_a_batching_reserve_fails_only_where_one_is_advertised`, `every_flaw_fails_the_suite_run` |

The router's own suite run (`router_suite`) stays green with every rule on: 30 tests.

### T-020-43, fix round 1 (review of `269ccee8`)

1. *Choice-round case* (should-fix): `batching-deposit-would-win-choice` (`conformance/src/router/cases.rs`): the AC-2
   choice network plus a free deposit at a reserve that batches, answered at the slow route's amount, so it would
   win the route and the round (253.963518 USDC against 253.836536); expected `batched_payout`, route through
   `f-dep` chosen. The double's flaw `BATCHING_DROPPED_ONLY_WHEN_SHORTLISTING` fails only that case.
2. *Record half* (low): router-api's `DecisionRecord::check` now refuses a kept or outranked candidate whose part
   ends at a batching deposit (the same unreleased 4.0.0; changelog entry naming the item, the invalid fixture
   `decision-record-batching-deposit-ending-a-kept-candidate`, a test). This row edited `contracts/` on the
   coordinator's instruction, router-api only. With it the suite's own check (`NoRouteEndsOnABatchingReserve`)
   became unreachable behind `Check::Contract`, which runs first, so it is removed; the double's flaws now produce
   records the contract refuses, and the suite catches them as contract failures.
3. *Gap 1 corrected* above, and pinned by a test whose withdrawal settles, the reserve then batches, and the next run
   is refused at the exchange's place with `BatchedPayout`.
4. *Middle deposit*: gap 4.
5. *Wider tuple match*: a default above.

| Rule switched off (fix round 1) | Went red |
|---|---|
| contract: the record's refusal of a kept batching ending | `candidate_dropped_for_a_batched_payout_names_its_reason` (seen red on the stub before the rule) |
| double: `BATCHING_DROPPED_ONLY_WHEN_SHORTLISTING`'s condition removed (it drops in both rounds) | `a_router_that_drops_a_batching_route_only_when_it_shortlists_fails_the_choice_case` |
| guard: `closable`'s `pays_in_batches` | `reserve_that_turns_batched_mid_route_is_refused_at_the_next_segment` |
## T-020-41: an automatic deposit whose only route has no firm rate waits for its customer

Branch `f-020/t-020-41-deposit-waits`, from `m3` at `a3bc16b4`. Edits `components/core-orch/src/arrival/` and the
assembly's `arrivals.rs`; no `contracts/` or `DEC-20` path.

- **The flow.** `arrival/filing.rs`'s routing no longer ends an order whose price names slow segments
  (`price.no_firm_rate`). It records the quote as `shown` (`quote_without_firm_rate`, the slow segments named by
  `T-020-07`) and the run stops at a new step, `Customer`, answering `Filed::Waiting { wait:
  FiledWait::CustomerAcceptance }`. The quote and its digest are read through `Arrivals::awaiting(order)`. Nothing
  is accepted, started or authorised to move, and the funds stay in the holder's account at the reserve (`GUA-1`):
  the deposit is held against the order, as for any filing, so it is spent and no other filing opens on it.
- **The customer's acceptance** is `Arrivals::accept::<R>(desk, order, by, quote)`, given as intake's is. It is
  refused to anyone but the order's own customer (`NotTheFiler`: the customer role, the order's requester's party,
  listed in the register of actors) and for any quote but the one shown (`NotTheQuoteShown`, a new `ArrivalError`).
  Policy is asked the customer's own authorisation (`REQ-AUZ-9`, as a carry-on by another login is), and on its
  permit the acceptance is recorded on `AcceptanceBasis::GivenOnDeposit`: `who` the customer, the filing's session,
  no yes, no hold, naming the authorisation the customer gave; then the `route_chosen` start, stated by the customer
  (`SourceKind::Customer`, as intake's is). It is built once and appended as built, as the deposit's own is, so an
  append that fails is carried on by the same customer accepting again, and the same acceptance again is told the
  same.
- **Validity.** The offered route keeps `DecisionRecord::earliest_expiry` of its candidate. Accepting past it records
  nothing, asks the route again (a fresh `shown`, a new digest), and the order waits on that quote; the old digest is
  `NotTheQuoteShown`.
- **A session over ends the wait.** The filing's session is the one the acceptance is given in, so a wait whose
  session is over ends as every other filing's does (`no_route_right_now`, recorded), at the next filing or
  acceptance; nothing moved.
- **Named tests** (`components/core-orch/tests/arrival.rs`): `automatic_deposit_whose_only_route_has_no_firm_rate_waits_for_its_customer`
  [`REQ-QTE-10`, `AC-13`], `customers_acceptance_of_the_slow_route_starts_it` [`REQ-QTE-10`, `REQ-ORD-15`],
  `nothing_moves_while_the_filed_order_waits_and_the_funds_stay_with_the_holder` [`GUA-1`],
  `slow_routes_quote_past_its_validity_is_asked_again_before_it_is_accepted` [`REQ-QTE-15`]. The first replaces
  `automatic_deposit_never_starts_a_route_with_no_firm_rate`, which pinned the ending this row replaces.
- **Gap 8 of `T-020-39`**, the three widenings for `given_on_deposit`: the scenario
  `conformance/scenarios/tests/deposit_waits.rs` (the F-024 file of this row), `a_deposit_with_no_firm_rate_waits_and_runs_on_its_customers_acceptance`,
  runs a slow reserve's order to its end through the assembly's real log: the customer's acceptance, the start
  (`chosen-route-rests-on-its-orders-acceptance`), and the deposit, a later segment that moves
  (`move-rests-on-its-orders-acceptance`). The third, `split-deposit-arrival-names-its-part`'s `deposited`, is shown
  by `a_split_start_of_an_order_accepted_on_a_deposit_rests_on_an_arrival_naming_each_part` (core-orch): the filing
  never splits (one deposit funds one part), so the case offers the log the order's start with a second part.
- **Defaults** (reversible, each failing closed):
  - the customer's acceptance asks policy for the customer's own authorisation (a new one, as a carry-on by another
    login does), in place of reusing the filer's; policy not knowing waits as `policy_not_known`, a refusal is
    `ArrivalError::Refused` and the order keeps waiting;
  - a quote with a slow segment is asked of the customer for good: once asked, a requoted route waits for its
    customer too, even where it is now firm, so a customer never accepts a quote and then finds another started;
  - a quote with no figure stating a validity doesn't expire (`earliest_expiry` is `None`), as a hold's doesn't;
  - the customer accepts in the session the deposit was filed in (the acceptance cites it); there is no second
    session parameter, and a wait whose session is over ends, with nothing moved;
  - the customer's acceptance comes only from the order's requester's party, in the customer's role, listed in the
    register of actors (a person or an agent); anyone else is `NotTheFiler`, as a stranger is told of a receipt;
  - one deposit still funds one part, so the log's split row for the new basis is shown by an offered history, not a
    filing.
- **Gaps, with owners.**
  1. Closed by the fix round below: the waiting order is told to its customer through its notice.
  2. `T-020-39`'s gap 4, that the log can't read that a `given_on_deposit` acceptance's route has no firm rate, stays:
     `arrival/` holds it (the filing asks the customer only for a route with a slow segment and `Arrivals::accept`
     only answers an order waiting for it), and no log row can check the route's price.
  3. An acceptance the customer gives while policy doesn't know waits as `policy_not_known` with nothing recorded, and
     the customer accepts again; no timer carries it (as intake's).
- **Rule-off table** (each switched off alone from the committed checkpoint `dfb57a75`, its tests run through the lock,
  the file restored with `git checkout`; script `scratchpad/ruleoff.py`). "Waits" is
  `automatic_deposit_whose_only_route_has_no_firm_rate_waits_for_its_customer`, "accepts"
  `customers_acceptance_of_the_slow_route_starts_it`, "nothing moves"
  `nothing_moves_while_the_filed_order_waits_and_the_funds_stay_with_the_holder`, "validity"
  `slow_routes_quote_past_its_validity_is_asked_again_before_it_is_accepted`, "split"
  `a_split_start_of_an_order_accepted_on_a_deposit_rests_on_an_arrival_naming_each_part`, "scenario"
  `deposit_waits.rs`'s one.

  | Rule switched off | Red |
  |---|---|
  | `filing.rs`: a slow route never asks the customer (it is accepted on the deposit) | waits, accepts, nothing moves, validity, split, scenario |
  | `filing.rs`: the digest isn't held to the quote shown | accepts, validity |
  | `filing.rs`: the acceptance's caller isn't held to the order's customer | accepts |
  | `filing.rs`: the quote's validity isn't read | validity |
  | `filing.rs`: a wait ends no more with its session | nothing moves |
  | `filing.rs`: the customer's acceptance recorded on `automatic_deposit` | accepts, scenario |
  | `relations.rs`, `move-rests-on-its-orders-acceptance`: `automatic_deposit` alone (gap 8) | scenario |
  | `relations.rs`, `first-move-rests-on-its-parts-yes`'s deposit arm: `automatic_deposit` alone | scenario |
  | `relations.rs`, `chosen-route-rests-on-its-orders-acceptance`: `automatic_deposit` alone (gap 8) | accepts, validity, split, scenario |
  | `relations.rs`, `deposited` (`split-deposit-arrival-names-its-part`): `automatic_deposit` alone (gap 8) | split |

  The three rows of gap 8 now each show a red; `contracts/` was edited only in these runs, and restored.

### T-020-41, fix round 1 (the review of `a610cb93`)

- **Finding 1, tell the customer** (fixed, not waived; the owner's standing rule is to fix contract gaps now).
  - *Contract* (`DEC-20` path; core-internal, folded into its unreleased 1.0.0, so **no version moved**; the
    guard is read against `origin/develop`, where 1.0.0 isn't released): `What::Shown` gains an optional `awaiting`
    (`Awaiting { requester, valid_until, slow }`, `SlowSegment { position, declared_seconds }`), written only for the
    quote an order filed on a deposit waits on its customer to accept. The notices are built from the log alone, and
    the log's `shown` held only the digest, so the shape rides on it: no new entry kind and no new citation row.
    Its check: only on a quote shown without a firm rate, at least one slow segment, each place once; absent, a
    `shown` reads and writes as before. Schema, README and changelogs (the contract's and the crate's) say so. One valid
    fixture (`verified-entry-of-a-quote-awaiting-its-customer`) and two invalid (shown as a firm quote; a segment named
    twice), both refused by the Rust types.
  - *Notices* (`F-023`): `About::Waiting { order, at, quote, valid_until, slow }`, no provider, amount or account
    (`REQ-AUZ-7`), sent by `Notices::notify` to the customer the entry names and to the operator, once for each
    `shown` that carries `awaiting`, so a requote is told again. The order has no state before its acceptance, so the
    notice has no sequence and no key in the ledger or the log's summaries: a restart tells it again, and an
    `Inbox` holds it once (`Taken::Event` then `Repeat`; `Inbox::waiting_on` reads it). The order's changes are
    counted from its start as ever.
  - *Filing*: the slow quote is recorded with `awaiting` (requester, the whole-second validity, the slow segments by
    place), again on every requote.
  - *Tests*: contract `quote_an_order_waits_on_its_customer_to_accept_is_told_from_the_log` [`REQ-QTE-10`,
    `REQ-ORD-20`]; notices `an_order_waiting_for_its_customers_acceptance_is_told_to_the_customer_and_the_operator`
    [`REQ-QTE-10`, `REQ-ORD-20`]; the scenario `deposit_waits.rs` now reads the customer's notice off the real log.
- **Finding 2, the validity shown** (fixed): `Arrivals::awaiting(order, now)` returns `Awaited { quote, digest,
  valid_until, expired }`; an expired quote is said to be, and accepting it still asks the route again (the requote
  stays on acceptance, nothing is requoted on a read). The validity is the earliest figure's, to the whole second
  below.
- **Finding 3, the session** (fixed): `nothing_moves_while_the_filed_order_waits_and_the_funds_stay_with_the_holder`
  now runs both ends of a wait whose session is over, the filing filed again and the customer accepting: each ends the
  order, records no acceptance and moves nothing. The refusal loop of
  `customers_acceptance_of_the_slow_route_starts_it` now also tries a listed customer of another party, beside the
  reserve filing it and an unlisted login of the right party.
  - **What the session's end costs** (said, and an owner choice for the form: *re-offer on the same deposit in a later
    session?*): the wait ends only when the next filing or acceptance looks at it (no timer), and its end leaves
    the deposit spent, since the deposit was held against the order (`one-order-per-deposit`). The funds stay with
    the holder, as the ruling asks, but a customer who leaves the session can't come back to the same deposit: only a
    new arrival notice from the reserve is a new arrival. Default: no re-offer.
- **Finding 5, what policy decides** (said): in `customer_accepts` the requote (a router call and a fresh `shown`) and
  the end for a session that is over run before policy is asked of the customer. Both are bounded to the order's own
  customer, and the filer's permit already covers the order; policy decides the acceptance and the start, not the
  requote.
- **Rule-offs of this round** (each alone, from the committed fix; script `scratchpad/ruleoff2.py`):

  | Rule switched off | Red |
  |---|---|
  | the contract's check: `awaiting` only on a quote without a firm rate | the contract test (`--shown-with-a-firm-rate`) |
  | the contract's check: each slow place once | the contract test (`--naming-a-segment-twice`) |
  | `Notices::notify` sends no waiting notice | the notices test, the scenario |
  | the filing writes no `awaiting` on a requote | `slow_routes_quote_past_its_validity_is_asked_again_before_it_is_accepted` |
  | `awaiting` never says expired | the same |

### T-020-41, fix round 2 (the re-review of `1bc97afa`)

- **Finding 1, a requote that comes back firm** (blocker, fixed). Asked once, a customer is asked of every requote of
  the order, firm now or not, and the contract refused `awaiting` there. The cleanest fix keeps `awaiting` on every
  such quote, since the notice carries the new quote's validity, and tightens the check to say what it means: `awaiting`
  is on a quote shown without a firm rate naming at least one slow segment, or on a plain quote naming none (a
  requote that came back firm), each place once, never on a price. The other option, `awaiting` only where `slow` is
  non-empty, would have left a firm requote with no validity and no notice to carry it. The schema's `minItems` is gone
  (the Rust check holds the pairing); fixtures: one valid (`--asked-again-firm`), one more invalid
  (`--without-a-firm-rate-naming-no-slow-segment`). The reviewer's probe is
  `a_requote_that_comes_back_firm_is_shown_and_accepted_by_the_customer`: the old digest is refused, the new quote is
  plain, names no slow segment, is told as still waiting, and its acceptance starts the order.
- **Finding 2, a restart** (fixed). The waiting notice is named in the ledger (`Key::Waiting { to, order, quote }`),
  so a restart sends none the ledger shows delivered and an undelivered one is tried again by `retry`; and it is
  summarised in the log as the order's notices are, for which core-internal gains `NoticeOf::Waiting { order, quote }`
  (folded into the unreleased 1.0.0, one valid fixture; `NoticeOf` no longer derives `Hash`, since provider's
  `Digest` doesn't and nothing hashed it). A wait is told only while the log shows no acceptance, refusal or start of
  the order after it: the read skips an order its own unread entries settle. Its end is told by `About::WaitEnded`,
  named in the ledger alone (no summary), so an `Inbox` can drop the wait; a restart with the ledger sends it no more.
  Test: `a_waiting_notice_survives_a_restart_is_retried_and_is_sent_only_while_the_order_waits`.
- **Finding 3, the inbox** (fixed). The waiting notice carries the place of its `shown` entry in the log (`index`);
  the `Inbox` keeps the latest by it, whatever order the notices come in, and drops the wait when the order's first
  notice arrives (it started) or `WaitEnded` does; a late older notice doesn't bring it back. Test:
  `an_inbox_holds_the_latest_waiting_quote_and_none_once_the_order_started_or_the_wait_ended`.
- **A limit, said**: a restart without a durable ledger tells again what the log still shows waiting (at least
  once); only the ledger and the log's summaries make a restart quiet, as for every other notice.
- **Rule-offs of this round** (each alone, from the committed fix; script `scratchpad/ruleoff3.py`): see the table the
  commit's body holds and below.

  | Rule switched off | Red |
  |---|---|
  | the contract's firm pairing: a plain quote may carry `awaiting` | the contract test (the firm fixture) |
  | the contract's pairing check whole | the contract test (the invalid fixtures) |
  | the filing writes no `awaiting` where `slow` is empty | the firm-requote test |
  | the waiting notice's ledger mark is not read | the restart and retry test |
  | an order the log shows settled is still told as waiting | the restart test |
  | the end of a wait is not told | the restart test |
  | the inbox keeps the first notice, not the latest | the inbox test |
  | the inbox keeps the wait after the order starts | the inbox test |

## T-020-40: a stuck order's evidence comes from a party to the order

Branch `f-020/t-020-40-confirmer-party`, from `m3` at `a3bc16b4`. Edits `components/core-orch/src/stuck/` and the
scenario kit; no `contracts/` edit.

- **Design.** `stuck/party.rs` reads, from the log's history, whether a statement's maker is a party to an order, as
  `core-internal`'s `party_to_the_order` does (a port, since the contract's function is private): a provider of a
  segment on any route the order, or an order a hand-on joins it to, was given (chosen, recalculated, destination
  changed), the provider a move or a resolution went to, the holder of a holding account its value rested at (an
  arrival's reserve, a permitted decide step's or an effect's mover, a holding withdrawal's holder and publisher),
  or the publisher of a node crossed, as a replay or evidence of the order binds it. Never a holding check, a refused
  decide step, or an order no hand-on joins.
  Every step that names a statement now takes the history (`Recorded`, `&[(Receipt, Entry)]`, as the log holds it) as
  a trailing argument: `decide`, `known_outcome`, `confirm_outcome`, `decline_outcome`, `report_segment_outcome`,
  `confirm_segment_outcome`, `confirm_segment_done`, `decline_segment_outcome`. A report, confirmation or decline
  whose evidence is by a maker the order doesn't involve is `ResolveError::OutsideTheOrder`; a confirmation or decline
  on another statement of the party that made the report's own is `ReportersOwnParty`. Both come before policy is
  asked or anything is built, so nothing is recorded. The log's rule stays the guard.
- **Decide step's digests** (T-020-39 gap 1): a digest of `Deciding::evidence` that names a stated entry the history
  holds (its commitment) must be by a party, else `OutsideTheOrder`.
- **Kit low finding** (`segments_support/mod.rs`): the ends stay on the thread, but a live `World` holds a guard;
  building a world with other ends while one lives panics, and `World::two_parties` sets its ends in one step.
  The first guard hit was my own test, fixed.
- **Consumer edits:** `conformance/scenarios/src/assembly/mod.rs` (`decide_stuck` passes the log's entries), and the
  scenarios `stuck.rs`, `stuck_mover.rs`, `stuck_hand_on.rs`, `hand_on.rs` (the history argument, `Core::recorded`).
- **Named tests** (`stuck.rs`): `stuck_resolution_resting_on_a_statement_by_a_provider_outside_the_order_is_refused`
  [`REQ-AUZ-4`], `statement_by_a_provider_on_an_earlier_route_of_the_order_backs_a_resolution` [`REQ-AUZ-4`,
  `REQ-ORD-1`], `confirmation_naming_an_outside_providers_word_frees_nothing` [`INV-2`, `AC-6`] (T-020-39's pin of
  the one-party order, now refused by Core). Added: `silent_move_reported_confirmed_or_declined_on_an_outside_providers_word_is_refused`,
  `decline_resting_on_an_outside_providers_word_or_the_reporters_own_party_is_refused` (`stuck_mover.rs`), the kit's
  two world tests, and `party.rs`'s three differential tests over the contract's admission cases.
- **Red first:** with the three checks off, the four scenario tests and the party tests failed on assertions
  (outside word accepted, `Declined`, `AwaitsConfirmation`); the earlier-route test passes then, red only in its rule-off.
- **Defaults** (reversible): a digest naming no statement the history holds is a document and passes; evidence the
  history doesn't hold is refused; the reporter's own party is read by the statements' makers' parties, as the log does.
- **Gaps.** (1) A private node crossed with no replay or evidence binds no publisher, so its publisher's word is
  refused (T-020-39 gap 5): left to the lead. (2) `party.rs` ports the log's rule; drift is guarded by the differential
  tests over the contract's cases only. (3) The part with one party only stays held (T-020-39 gap 9), on the owner's form.
- **Rule-off table** below.

Rule-offs (each switched off alone from commit `833f27fa`, its tests run through the lock, the file restored with
`git checkout`; script `scratchpad/ruleoff.py`). "Scenarios" is `stuck` and `stuck_mover`; "lib" `core-orch`'s `party`
tests. The four scenario tests are named by their first words: `confirmation_naming…`, `decline_resting…`,
`silent_move_reported…`, `stuck_resolution_resting…`.

| Rule switched off | Red |
|---|---|
| `from_a_party`: the party check | all four |
| `from_a_party`: the reporter's own party | `confirmation_naming…`, `decline_resting…` |
| `from_a_party`: a statement the history lacks refused | all four |
| `digests_from_a_party` whole | `stuck_resolution_resting…` |
| the digests' limit to provider results | none in scenarios: `assembly_stuck` and `assembly_hand_on` go red on it (the first statement about their order is Core's question), as seen before the limit was added |
| the kit's live-world guard | `second_world_with_other_ends_is_refused_while_the_first_lives` (a `should_panic` failure, not matched by the script's pattern) |
| `involves`: plans' providers | lib `the_makers_the_log_admits…`; scenarios: five silent-move tests (`silent_move_on_a_stuck_part_settles…`, `segments_move_confirmed_done_*`, `declined_report_of_a_silent_move…`) |
| `involves`: moves' providers | none: a subset of the plans' (`move-segment-is-in-its-orders-plan`), kept as defence |
| `involves`: resolve effect's mover, permitted decide's mover, arrival's reserve, holding withdrawal's holder, its publisher, hand-on closure | lib `the_makers_the_log_admits…` |
| `involves`: node publishers | lib `the_publisher_of_a_node…` |
| `involves`: a decide step read whatever policy answered | lib `the_makers_the_log_refuses…` |

### T-020-40, fix round 1 (the review of `66cf48df`)

- **1. Every kind of statement a decision's digests name is held to a party** (`stuck/mod.rs`
  `digests_from_a_party`): the limit to result shapes is gone. Only Core's own questions (`OPERATION_SHAPE`,
  `ARRIVAL_QUESTION_SHAPE`) are skipped, explicitly, so `assembly_stuck` and `assembly_hand_on`, which decide on
  Core's question, are unchanged. The probe is `decision_evidence_digests_are_held_to_a_verified_party_whatever_their_kind`
  (`party.rs`): the contract's outside result, relabelled a judgement, is refused; a party's judgement passes.
- **2. The differential test walks the contract's folder** (`orchestrations_reading_of_a_party_follows_the_logs_over_every_admission_case`):
  every admission case whose entry cites evidence (report, confirmation, decline, part evidence; both rows and the
  worlds), the log's verdict computed with `admit_entry` through either handle. Admitted: each cited maker reads as a
  party. Refused on `party` (the two rows): one does not. Floors on the number of cases read, so a silently skipped
  folder is red. A new or changed contract case reaches it with no edit here.
- **3. Verified attribution is required** for the digest check, and for the evidence of a report, confirmation or
  decline (`party::verified_party`). The kit's history (`Core::recorded`) reads each statement verified, as the real
  log's does.
- **4. Defaults that fail open, named.** A digest naming no statement the history holds passed: it failed open.
  Closed by `T-020-53` (the section at the end of these notes).
- **5. Doc lines** on the eight entry points: `history` is taken as the log's own entries.
- **Rule-offs of the round** (from the fix commit, as above; "walk" is the differential test, "probe" the digest
  probe, "assembly" `assembly_stuck` and `assembly_hand_on`):

  | Rule switched off | Red |
  |---|---|
  | digests: Core's questions not skipped | `assembly_stuck`, `assembly_hand_on` |
  | digests: held to results only (the old limit) | probe |
  | digests: verified attribution not required | probe |
  | `involves`: always a party | walk, probe, the publisher test |
  | `involves`: never a party | walk, probe, the publisher test |
  | `involves`: arrival's reserve arm | walk (the round-1 test had it on a named case only) |
  | `involves`: node publishers | the publisher test |

  The verified-attribution requirement on a report's, confirmation's or decline's evidence has no rule-off red:
  `Core::recorded` reads every kit statement verified, and no scenario builds an unverified one; the probe covers the
  digests' half only.

### T-020-40, fix round 2 (the re-review of `9bb5de85`)

- **1. No statement is exempt by its shape.** No log rule ties an `operation` or `arrival-question` statement to the
  operator, so the skip is gone (`digests_from_a_party` holds every named statement to a verified party). The two
  assembly tests' `evidence_about` now names a provider's result about the order. The probe at `party.rs` expects an
  outsider's verified statement in question shape refused.
- **2. The walk takes the log's own verdict both ways.** Admitted: each cited maker reads as a party. Refused with
  the party rule's message, whatever the fixture says it breaks: some cited maker reads as no party. Every case
  without `breaks` must be admitted live, so a dropped case fails.
- **3. `evidence_of_an_outcome_must_be_a_verified_statement_of_another_party_than_the_reporters`** calls
  `from_a_party` with an unverified statement and with the report's own evidence.
- Left as is: "a digest naming nothing in the log passes" (on the owner's form).
- **Rule-offs** (from the fix commit):

  | Rule switched off | Red |
  |---|---|
  | the contract's arrival-reserve arm of `party_to_the_order` (`relations.rs`, restored) | the walk |
  | question shapes skipped again | the digest probe |
  | `from_a_party`: verified attribution not required | the unverified-evidence test |

## T-020-42

Branch `f-020/t-020-42-erasure-override`, from `9af9746c`. Edits `components/core-orch/src/erasure.rs` and, in the `F-024`
conformance assembly (`conformance/scenarios/`), the register, `erasure.rs` and `releases.rs`.

- **Design.** `erase_statement_overriding` runs `T-020-37`'s steps with the override: the first look and the look after
  the effect are `Statements::erasable_overriding` (the store's `BoundStore::erasable_overriding`, `T-022-10`), and the
  `EraseStatement` policy is asked for sets `overrides_citations`. `erase_statement` is unchanged and never sets it, so
  the default (`EVIDENCE_RESTED_ON`, nothing decided or recorded) holds; `Erasure` gained no field, so the callers and
  `T-020-37`'s tests are untouched. Core asks for the override only where its caller calls the overriding function: it
  never infers one from a refusal. Policy decides who may (`T-021-13`: an operator's login holding `EraseStatement` and
  `EraseOverridingCitations`); the store honours the flag only from the permitting decision. The entries citing the
  statement stay as they are.
- **Roll back.** `Core::roll_back` / `roll_back_as` in the assembly's `releases.rs` call `Segments::roll_back` over
  the platform, as `promote` is wired, and the operator's login now holds `Change(RollBack)` in the register.
- **Defaults, reversible, each failing closed.** (1) `Statements::erasable_overriding` has a default that is the refusing
  `erasable`: a store that says nothing of an override never lifts one. (2) The assembly's operator login holds no
  `EraseOverridingCitations`: the scenarios that need it grant it through `Core::registers`, so nothing erases a cited
  statement unless a scenario says the login may (`T-021-13`'s gap 1 is the deployment's register). (3) The refused
  roll-back scenario uses the customer as a caller no register lists, and a version that isn't the one staged.
- **Tests.** In `components/core-orch/tests/erasure.rs`: `cited_statement_is_not_erased_without_an_override`
  [`REQ-AUD-13`], `operators_decided_override_erases_a_cited_statement_and_history_still_checks` [`REQ-AUD-13`, `AC-36`],
  `override_without_a_permitting_decision_erases_nothing` [`REQ-AUZ-1`]; the fake store there models "cited" by flag, since
  the fake log's `admit_entry` has no citing entry, so the real log's refusal and the history check are the scenarios'.
  In `conformance/scenarios/tests/audit_scenarios.rs` (the assembled Core, the real log, store and outside checker):
  `an_erasure_asked_without_the_override_keeps_a_cited_statement`,
  `an_operator_holding_both_permissions_erases_a_cited_statement_and_history_checks`,
  `an_operator_holding_only_the_erase_permission_is_refused_the_override`; and in `module_scenarios.rs`
  `a_permitted_roll_back_ends_the_staging_and_a_refused_one_moves_nothing`. `T-020-37`'s tests pass unchanged.
- **Red first.** With `erase_statement_overriding` a stub that behaved as the plain one, the override and refusal tests
  failed on the store's refusal (`NotErasable`); the cited-by-default test passed first (it pins today's behaviour). The
  assembly's override scenarios failed on the same refusal and the roll back scenario on `NotPermitted` for `Ok` (the
  register not yet listing the permission).
- **Gaps, with owners.** None found. The deployment's register listing `EraseOverridingCitations` and `Change(RollBack)`
  for its operator logins is configuration (`T-021-13`'s gap 1, and `T-021-12`'s gap 2). This row closes `T-021-12`'s gap 1 (nothing called `Segments::roll_back`).
- **Rule-offs** (each restored):

  | Rule switched off | Red |
  |---|---|
  | `overrides_citations` always false | core-orch override and refusal tests; the assembly's two override scenarios |
  | first look is the default check | core-orch override and refusal tests |
  | look after the effect is the default check | core-orch override test |
  | the overriding check is the refusing one | core-orch override and refusal tests; the assembly's override scenario (`Bound`) |
  | the operator login listed for the override by default | the assembly's refused-override scenario |
  | the operator login not listed for `Change(RollBack)` | the roll back scenario |

### T-020-42, fix round 1 (the review of `91250014`)

1. **A citation between the decision and the effect.** `BoundStore::erasable_overriding_since(commitment, decision)`
   (core-audit) runs the override's check and refuses a citation after the permitting decision's index, as `reconcile`
   counts it. `Statements::erasable_overriding_since` (refusing default) is what the look after the effect now asks on
   the override path, with the decision's receipt, so such a citation makes the erasure `Failed` and nothing is erased.
   Tests: `override_is_refused_for_a_citation_made_after_its_decision` (core-orch) and
   `override_check_since_its_decision_refuses_a_later_citation` (core-audit).
2. **Rebound store.** `Core::rebound_store` reads the disk back and binds again; both erasure scenarios (`T-020-37`'s
   and the override's) assert the findings on that, not only on the store bound at assembly.
3. Notes: gap owners corrected above, and `T-021-12`'s gap 1 recorded closed in F-021's notes.
4. The roll back scenario asserts the effect's `done` result. 5. The core-orch override test's doc points at the scenario
   that carries the history check.
- **Rule-offs.**

  | Rule switched off | Red |
  |---|---|
  | `erasable_overriding_since` counts no citation | the core-audit test |
  | the look after the effect on an override is the plain override check | the core-orch late-citation test |
  | `reconcile` reports every citation of an overridden statement | the override scenario (on the rebound store) |
  | the look after the effect is given the effect's receipt in place of the decision's | both core-orch override tests (the fake store records the receipt it is given) |

## T-020-44: a hand-on retried under another linked order

Branch `f-020/t-020-44-hand-on-retry`, from `2d62f1b9`. Fixes finding 2 of M3's milestone review (PR #143, review
6043265441) and two Lows beside it. Edits `components/core-orch/src/hand_on/` (`mod.rs`, `rule.rs`),
`components/stm/src/hand_on.rs`, `contracts/src/core_internal/relations.rs` and `contracts/core-internal/`.

- **Design.**
  1. *The retry.* A pending hand-on is keyed by the first order's part and built for one linked order. Once its
     `handed_on` is in the log, a retry skipped `linked_is_free`, appended the pending `route_chosen`, and inserted
     `self.linked[handing.linked]` with that pending order's data: the log opened `ord-2`, Core's map said `ord-3` held
     `ord-2`'s start, and the caller was told `ord-3` was handed on. `HandOn::hand_on` now compares the retry's linked
     order with the one the pending hand-on was built for, before anything is appended, and refuses a different one
     with `HandOnError::PendingForAnother { pending }`, naming the pending order. Refusing, not answering with the
     pending order, because an answer naming `ord-2` to a caller who asked for `ord-3` could be taken for `ord-3`'s
     success; the refusal names the one to retry and records nothing. The same holds before the `handed_on` is in the
     log (a retry then could be rebuilt, but one rule is simpler and fails closed). A hand-on stranded on a refusal
     keeps its own answer (`LinkedRefused`, naming the order the log refused).
  2. *One start per linked order.* `prove_start` keeps what the log took, by linked order (`Started`: record, statement,
     publisher, receipt). A second call for the order passes the very record, statement and publisher and is answered
     with the receipt it had, nothing appended; any other is refused with `Unproven::AlreadyStarted`, nothing
     recorded. A start built and kept while the log was unavailable becomes the taken one when its retry lands. The
     log gets the matching row, `one-start-per-linked-order` (core-internal): a `holding_withdrawal` of an order is
     refused when another of that order cites a record of another transaction. It is disjoint from
     `one-start-per-holding-record` (same transaction, another entry), and a record naming no transaction conflicts with
     none. Folded into the unreleased 1.0.0: README row, changelog line, two admission fixtures
     (`--conflict`, `--the-same-start-again`), no version moves. The contract guard against `origin/develop` is green.
  3. *`holds_value_for`.* It accepted a part waiting for the customer whatever the cause; `after_handed_on` takes a
     waiting part only after a failed final deposit (`deposit_failed_at` set, `INV-10`). It now matches, and its doc and
     the rule's say a floor wait holds none. Core never relied on it for that: `build_hand_on` runs the transition
     itself and refuses on its answer; the fix keeps a caller that reads the rule alone from being told otherwise.
- **Tests (red first).** In `conformance/scenarios/tests/hand_on.rs`:
  `a_pending_hand_on_retried_for_another_linked_order_is_refused` (the log took `ord-2`'s `handed_on`, its
  `route_chosen` unavailable; a retry for `ord-3` was answered `Ok`, now refused, nothing recorded; the retry for `ord-2`
  completes, `ord-3` stays unfollowed, the log admits every entry) and `a_started_linked_order_takes_no_second_start`
  (another record, another statement of the same record: refused; the same entry again: the one receipt, one
  `holding_withdrawal`). In `components/stm/tests/transitions.rs`:
  `the_rule_across_two_orders_finds_no_value_to_hand_on_at_a_part_waiting_at_its_floor` (also pins the part waiting after
  a failed deposit as still holding value). In `contracts/tests/core-internal.rs`:
  `a_second_holding_record_of_another_transaction_for_the_same_order_is_refused_by_the_log`. The first three were
  watched failing on their assertions before the code (`Ok` for `PendingForAnother`, `Ok` for `AlreadyStarted`, `Ok(())`
  for `NoValueToHandOn`).
- **Defaults taken (reversible, fail closed).** A retry for another linked order is refused, not answered with the
  pending one. An identical start retried after it was taken is answered with the first receipt rather than appended
  again. The log row is keyed by the order and the transaction the record names.
- **Gaps, with owners.**
  1. `HandOn`'s `started`, `spent` and pending maps are in memory, like the rest of it: after a restart a second record
     for the order is refused by the log's new row, not by Core. The durable record is `F-030`'s. Owner: `F-030`.
  2. The contract row waits for the owner's `DEC-20` click with M3's other contract changes. Owner: the owner.
  3. A pending hand-on whose `handed_on` is not in the log yet could be dropped and rebuilt for the new linked order;
     it is refused as well. Operator choice, if wanted: a row, not a defect.
- **Rule-offs** (each restored):

  | Rule switched off | Red |
  |---|---|
  | the retry's linked order compared with the pending one | `a_pending_hand_on_retried_for_another_linked_order_is_refused` |
  | the start taken answered from `started` (second record or statement refused, same entry answered) | `a_started_linked_order_takes_no_second_start` |
  | `holds_value_for` takes any waiting part | `the_rule_across_two_orders_finds_no_value_to_hand_on_at_a_part_waiting_at_its_floor` |
  | the log row `one-start-per-linked-order` cites nothing | the contract's `a_second_holding_record_...` test and `every_relation_of_the_citation_table_holds_and_breaks_as_its_cases_say` |

## T-020-45: the contract round for `M3`'s milestone review of 2026-11-24

Branch `f-020/t-020-45-contract-round-r1`, from `2ae0425c` (the PLAN PR #267 on `m3`'s tip). Edits `contracts/`
(provider-role, module-interface, router-api, core-internal, the shared files) and `tools/contract-guard/` (`DEC-20`
paths: they wait for the owner's click at the release into `main`), and the consumer files below. Wave 38's rows
(`T-021-14`, `T-020-44`) were not on this base: `T-020-44` (`4b895b88`) is on its own branch, `T-021-14` has no commit
yet, so this round starts from `m3`'s shapes and its merge with them is the lead's (gap 1).

- **Versioning.** provider-role's unreleased 3.1.0 is 4.0.0 (finding 9; `develop` holds 3.0.0). module-interface,
  released at 4.0.0 on `develop`, moves to 5.0.0: its `handles` and call kinds take provider-role's operation kinds
  by reference, and `OperationKind` gains two variants. router-api (unreleased 4.0.0) and core-internal (unreleased
  1.0.0) fold. state-machine refers to provider-role (`order.schema.json`'s nodes and amounts, the re-exported `NodeRef`), so its unreleased 3.0.0 takes a line that it follows provider-role 4.0.0 (fix round 1); host refers only to state-machine, already at an unreleased 3.0.0. Fixtures keep naming protocol
  2.0.1, as every major since 2.0.1 has (router's `calls.rs` pins it "so a provider-role bump leaves the fixture
  standing"): the row's "every `protocol` literal" is read as the literals that name the protocol's current version,
  which were `contracts/src/provider_role/mod.rs`'s `VERSION` and `conformance/src/role/suite.rs`'s pin; every other
  reader uses `VERSION`. No guest names a protocol version.
- **Item by item.**
  1. provider-role 4.0.0, with a top entry naming finding 9.
  2. `ArrivalNotice.destination: Option<DepositDestination {reserve, node, account, asset, set_by, set_for}>`; two
     notices of one credit with other destinations restate it.
  3. Read first: `TupleTerms` and `Hold` were statements of their own (3.1.0) and `Tracking::StatusQuery` was a
     declaration, with no operation carrying any of them to or from a module (the commit body says so). New:
     `Operation::StatusQuery {transaction, provider, about}` / `ResultBody::StatusQuery {transaction, about, outcome:
     answered(result, nested) | not_known | refused}` with `check_status_answer`; `Operation::Hold(HoldRequest)` /
     `ResultBody::Hold {transaction, outcome: held(hold, nested) | refused}` with `check_hold_answer`; `terms`
     (optional, nested) on the three dry-run answers (`ReserveDryRunAnswer`, `TransferQuote`, `ConversionQuote`), so
     terms come back on the round's question, before a segment starts (`REQ-QTE-6`); `ResultBody::transaction()` and
     `ResultBody::terms()`. module-interface's `OperationKind::{StatusQuery, Hold}`, and a declaration's `calls` may
     list neither (`DeclaredCallOfAnOperatorsQuestion`); router-api's `call_kind` and its checks refuse them too.
  4. `check_context(deployment, &ProtocolVersions, at)`: `ProtocolVersions::only(v).with_older(old, cutover)`; a
     statement under an older version is void from its cutover on (`ContextMismatch::PastCutover`), an unorderable time
     voids it. The guard refuses a new entry of provider-role's changelog from 4.0.0 on that says neither or both of
     `Ignorable: yes` / `Ignorable: no`, or that `Adds a protection` and says `Ignorable: yes`; 3.1.0's thirteen
     entries, folded into 4.0.0, now say it.
  5. core-internal, folded: row `automatic-deposit-takes-a-firm-quote` (L7); `arrival.holder` and row
     `waiting-quote-is-for-its-arrivals-holder` (R-AWAITING); `revoked_before_receipt` counting a revocation from its
     effect until a `failed` result (L5); `wait_ended {order, why}`, rows `wait-ends-a-wait` and
     `ended-wait-takes-no-acceptance`, `one-order-per-deposit` freeing an ended wait's deposit (finding 8);
     `NoticeOf::WaitEnded` (`T-023-04`); `CatalogueChange::SetSetting` with `Action::ChangeCatalogue.setting:
     OperatorSetting` (eight kinds, `SettingKind`), the five lifecycle actions already changes of their own
     (`LIFECYCLE_ACTIONS`, `is_a_lifecycle_action`); `Reads::FiledOrder {order, requester}`; `order_opened`.
  6. Lows: the README's citation table repaired (seven stale names gone, twenty-five unlisted named, the handle
     families named); `decision-request-to-set-slippage-naming-a-verifiers-class` for the class rule;
     `without_rule` (feature `rule-switches`, the crate its own dev-dependency) and
     `each_invalid_fixture_breaks_only_the_rule_it_names`, which found `effect-granting-without-a-tuple` breaking the
     advertising-keys rule too (fixed: it names its keys) and four raises named by a prefix of their rule (now whole);
     provider-role's README and five schema descriptions say what a provider states, core-internal's README the log's
     side ("Provider-role's statements in the log"); router-api's six router-version decision records signed for real
     over their current text by `rutent-core`'s statements key as the suites derive it, the five patched ones by a
     patch op on `/signature/value`, verified by a conformance test.
- **The rebuild's kinds (`T-020-46`).** Read piece by piece against the log: `Segments` (the started route is the
  `route_chosen` input, each segment's state the inputs and results, the effects acted on every `move_value` effect,
  which the platform's durable record answers once on a repeat, `T-013-10`); `Stuck` (`stuck_decide`, `stuck_move`,
  their effects and results); `HandOn` (every pending piece is an entry built and not yet appended, so nothing to
  rebuild, and what was appended is in the log: inputs, decides, holding withdrawals); `Issued` (every transaction
  Core asked under that the log names; one asked with no answer logged is lost, and the fresh source makes reuse
  improbable, not impossible: gap 4); the held quotes (`Holds::recorded`); the unsettled erasures (their effects and
  results); `Exposures` (route and results). One piece had no summary: intake's order before its acceptance (its
  request, parties, destination, session and route asked), now `order_opened`. Its tuples' inputs stay out of the log
  (`REQ-AUD-6`): gap 5.
- **Consumer edits** (to compile, no behaviour changed but where said): `check_context` with
  `ProtocolVersions::only` in `components/core-audit/src/log/keys.rs`, `components/prov/src/catalogue/advertisements.rs`,
  `components/prov/src/module/admission.rs` (a `DEC-20` file), `components/router/src/search.rs`,
  `conformance/src/role/stand_in.rs`, `conformance/src/role/suite.rs`, `conformance/src/router/mod.rs` and
  `conformance/tests/router_double/mod.rs`; `components/prov/src/module/output.rs` embeds `hold` and `terms`, which the
  result schema now references; the two new kinds in `components/prov/src/proxy/tests/calls.rs`, the stand-ins'
  `refuse_outside_role` and `transaction_of` and the suite's views (`conformance/src/role/`); `terms: None` and
  `destination: None` in the stand-ins (the four literals of `stand_in.rs`, kept minimal beside `T-024-22`'s edits
  there); `setting: None` in every `ChangeCatalogue` literal (core-authz's engine and roles tests, core-orch's
  `segments/` and `tests/roll_back.rs`, the assembly's `holding.rs` and `mod.rs`, `authz_scenarios.rs`,
  `segments_support/mod.rs`); `holder` on the two arrivals core-orch builds: `None` in `arrival/mod.rs`, and the
  account's holder in `arrival/filing.rs`, the one edit that adds what the log records, since the waiting quote's row
  reads it (without it the filing's waiting `shown` would be refused); `NoticeOf::WaitEnded` keyed as `wait_ended`
  is in `notice/summary.rs`; `Read::FiledOrder` covers no entry (core-audit's `receipts/receipt.rs`) and is refused by
  policy whoever reads (core-authz's `session/mod.rs`) until `T-021-16`; `set_setting` is answered by no rule
  (core-authz's `roles/mod.rs`) until `T-021-15`, its test `operator_setting_is_refused_while_no_rule_answers_it`
  [`REQ-AUZ-1`]; the two new kinds in core-audit's exhaustive `kind`; `tests/notice.rs` names the waiting quote's
  arrival among the records its log doesn't build (a test artefact: the test logs a `shown` alone).
- **Named tests.** provider-role: `statement_under_provider_role_3_0_0_is_taken_under_4_0_0_only_while_3_0_0_is_in_force`,
  `statement_under_a_version_past_its_cutover_date_is_void`, `arrival_notice_naming_its_destination_and_who_set_it_is_valid`,
  and beside them `status_question_is_answered_by_the_providers_own_result_for_the_transaction_asked_about`
  [`REQ-ORD-1`], `hold_request_is_answered_by_the_reserves_own_hold_of_what_was_asked` [`REQ-QTE-15`],
  `terms_handed_back_with_a_result_are_its_makers_own` [`REQ-QTE-6`]; the guard:
  `changelog_entry_that_says_neither_ignorable_nor_not_fails_the_contract_guard`,
  `change_adding_a_protection_marked_ignorable_fails_the_contract_guard`; core-internal:
  `automatic_deposit_acceptance_of_a_quote_shown_without_a_firm_rate_is_refused_by_the_log`,
  `waiting_notice_whose_requester_is_not_the_arrivals_holder_is_refused_by_the_log`,
  `statement_received_between_a_revocations_effect_and_its_result_is_unattributed_by_the_contracts_rule`,
  `second_order_on_a_deposit_whose_wait_ended_unaccepted_is_admitted`, `operator_setting_is_a_catalogue_change_of_its_own`,
  `each_lifecycle_action_is_a_catalogue_change_of_its_own`, `each_invalid_fixture_breaks_only_the_rule_it_names`,
  `every_fixture_core_internals_readme_names_exists_and_every_fixture_is_named`, and beside them
  `read_of_a_filed_order_names_its_requester` [`REQ-AUZ-8`, `REQ-AUZ-7`]; conformance:
  `decision_record_fixtures_naming_the_routers_version_carry_signatures_that_verify` [`DEC-4`]. Written on the shapes
  before their rules; the guard's pair failed on their assertions first, and the rest show their red in the rule-offs.
- **Fixtures.** provider-role 10 valid, 15 invalid (two bundles, `answered-status-query-` and `answered-hold-`);
  module-interface 2 invalid; router-api 2 invalid (one patched) and the six re-signed; core-internal 14 valid, 8
  invalid, 16 admission cases, 2 attribution cases. Scripts in the row's scratch folder (`gen_pr.py`, `gen_ci.py`,
  `gen_adm.py`, `resign.py`).
- **Histories the log refused, sorted.** None of the earlier admission or world cases changed verdict under the new
  rows. One test artefact: core-orch's `an_order_waiting_for_its_customers_acceptance_is_told_to_the_customer_and_the_operator`
  logs a waiting `shown` with no arrival; named as an unbuilt citation, as its other unbuilt records are. No Core bug.
- **Defaults** (reversible, each failing closed):
  - fixtures keep naming protocol 2.0.1;
  - a statement under an older version is void from the cutover instant on, and at a time that can't be ordered;
  - the protocol's changelog rule binds provider-role alone, from 4.0.0 on, by its markers;
  - terms ride on the dry-run answers, not on every result: stated before a segment starts;
  - a hold is asked for by an operation of its own, since a hold names the choice round's record, which no dry run
    can;
  - neither new kind is a cross-module call (module-interface and router-api);
  - `set_setting` for every setting, `provider` the deployment's operator for one about the whole deployment;
    `set_slippage` stays;
  - a filed order's read is refused by policy, and a `set_setting` by the fixed rule, until their owners;
  - an operator reads a filed order without a permission as it reads an order's state, and a provider never;
  - a wait ends once, only unaccepted, and its order takes no acceptance after it;
  - the arrival's holder is optional, required only by the waiting quote's row;
  - `order_opened` carries no tuple input.
- **Gaps, with owners.**
  1. Closed by the merge below: wave 38's `T-020-44` (`32d0e379`, its `one-start-per-linked-order` row beside this
     round's in the changelog, the README table and `relations.rs`) and `T-021-14` (`d1c25a72`, core-authz) are merged
     into this branch, with `PLAN.md`'s `2ea04091`.
  2. A waiting quote's slow places stay orchestration's: the log holds the quote by digest alone and no figure of its
     segments' settlement times, so no row can check them (R-AWAITING's second half). Owner: the lead (a later round,
     if the decision record's settlement figures are to enter the log).
  3. Which protocol versions are in force, their cutover dates and the per-segment reading across a cutover are
     `T-020-54`'s; router-api's bounds name one version (`ProtocolVersions::only`), so routing takes no older one.
     Every caller judges a statement at its receiving side's time (fix rounds 1 and 2): module admission's `read` now
     takes the platform's admission time and the versions in force. Which versions the platform holds in force is
     `T-020-54`'s. Owner: `T-020-54`.
  4. A question Core asked under a transaction with no answer logged leaves that transaction out of a rebuilt
     `Issued`. Owner: `T-020-46`.
  5. A reopened Core holds an open order's request from `order_opened` but not its tuples' inputs. Owner: `T-020-46`.
  6. The platform's call judgement refuses `holding_balance` however it is named; `status_query` and `hold` named as a
     call are refused by the contracts, not yet by the platform. Owner: `T-013-11`.
  7. `set_setting`'s permissions are `T-021-15`'s, a filed order's read `T-021-16`'s, the wait's end `T-020-51`'s,
     its notice summary `T-023-04`'s, the register's L5 reading `T-022-11`'s, the destination `T-020-50`'s, the
     carriers' callers `T-020-47`'s and `T-013-11`'s.
  8. The rest of the fixtures' signatures are the contracts' placeholders, as `conformance/src/signing.rs` says; only
     the six router-version records are real.
  9. The workspace suite runs in CI.
- **Contract guard.** Against `origin/develop` (`fb883e01`): "1900 changed path(s) … no violations". CI's guard runs
  against `m3`, where the folded versions equal the base's and it fails by design, as `T-020-39`'s did.
- **Local checks** (through the lock): `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D
  warnings`; `cargo test -p` `rutent-contracts`, `rutent-contract-guard`, `rutent-core-authz --lib`,
  `rutent-core-audit`, `rutent-router`, `rutent-conformance`, `rutent-core-orch`, and the scenarios `deposit_waits`,
  `assembly_arrivals`, `authz_scenarios`, `holds`, `notices`; the doc check; plan-check and tag-check `--strict` (0
  findings, 2197 tests tagged). core-audit's `reopen` failed twice on one run at `EvidenceStore::open` and passed 11/11
  on the rerun, nothing of this row in its path.
- **Rule-off table** (each switched off alone from the committed checkpoint, its tests run through the lock, the file
  restored with `git checkout`; script `ruleoff.py` in the row's scratch folder; none hung). "Fixtures" is the
  contract's `every_valid_fixture_validates_and_every_invalid_one_is_rejected`, "table"
  `every_relation_of_the_citation_table_holds_and_breaks_as_its_cases_say`.

  | Rule switched off | Red |
  |---|---|
  | `in_force`: an older version never past its cutover | `statement_under_a_version_past_its_cutover_date_is_void` |
  | `in_force`: no older version taken | the same, and `statement_under_provider_role_3_0_0_is_taken_under_4_0_0_only_while_3_0_0_is_in_force` |
  | `same_figures`: the destination left aside | `arrival_notice_naming_its_destination_and_who_set_it_is_valid` |
  | `check_status_answer`: each of its four mismatches, alone | fixtures, `status_question_is_answered_by_the_providers_own_result_for_the_transaction_asked_about` |
  | `check_hold_answer`: each of its three mismatches, alone | fixtures, `hold_request_is_answered_by_the_reserves_own_hold_of_what_was_asked` |
  | the nested status result's, hold's and terms' maker, each alone | fixtures, and the status, hold or terms test |
  | module-interface: a call of an operator's question | module-interface's fixtures |
  | router-api: `call_kind` excluding `holding_balance` alone (schema) | router-api's fixtures |
  | the guard: neither or both markers | `changelog_entry_that_says_neither_ignorable_nor_not_fails_the_contract_guard` |
  | the guard: a protection marked ignorable | `change_adding_a_protection_marked_ignorable_fails_the_contract_guard` |
  | the guard: protocol entries not read at all | both guard tests |
  | `automatic-deposit-takes-a-firm-quote` cites nothing | `automatic_deposit_acceptance_of_a_quote_shown_without_a_firm_rate_is_refused_by_the_log`, table |
  | the same, a quote awaiting its customer read as firm | the same two |
  | `waiting-quote-is-for-its-arrivals-holder`: its holder field | `waiting_notice_whose_requester_is_not_the_arrivals_holder_is_refused_by_the_log`, table |
  | L5: a revocation counted from its `done` result only | `statement_received_between_a_revocations_effect_and_its_result_is_unattributed_by_the_contracts_rule`, `statement_received_after_its_grants_revocation_is_unattributed_by_the_contracts_rule`, `the_log_not_the_writer_attributes_a_stated_entry` |
  | L5: a failed revocation still counted | the first and the third |
  | `wait-ends-a-wait` cites nothing | `second_order_on_a_deposit_whose_wait_ended_unaccepted_is_admitted`, table |
  | `ended-wait-takes-no-acceptance`: an acceptance after the end | the same two |
  | `ended-wait-takes-no-acceptance`: a second end | the same two |
  | `one-order-per-deposit`: an ended wait frees nothing | the same two |
  | `check`: a setting named exactly by `set_setting` | `operator_setting_is_a_catalogue_change_of_its_own`, `each_invalid_fixture_breaks_only_the_rule_it_names`, fixtures, `every_invalid_fixture_is_rejected_by_the_rust_types_too` |
  | the rule switch never switching off | `each_invalid_fixture_breaks_only_the_rule_it_names` |
  | `is_a_lifecycle_action` leaving out `promote` and `roll_back` | `each_lifecycle_action_is_a_catalogue_change_of_its_own` |

### T-020-45, fix round 1 (the review of `1bb5f4cb`, PR #294)

- **1, should-fix: an answer by another provider.** `check_status_answer` and `check_hold_answer` now take the signed
  statements (`&Statement<Operation>`, `&Statement<ResultBody>`), as `check_answer` does, and refuse an answer whose
  maker isn't the provider asked (`StatusMismatch::AnsweredByAnotherProvider`, `HoldMismatch::OfAnotherReserve`), and
  a question or answer of another kind (`NotAStatusQuestion`, `NotAHoldRequest`). Invalid bundles
  `answered-status-query-by-another-provider` and `answered-hold-by-another-reserve`. Folded into 4.0.0.
- **2, should-fix: a setting naming another provider.** `Action::check` refuses a `set_setting` whose setting is about
  a provider (a tuple's slippage limit or evidence floor, a provider's ceiling) and whose `provider` is another ("an
  operator's setting about a provider names that provider"). Invalid fixture
  `decision-request-to-set-a-providers-ceiling-naming-another-provider`.
- **3, should-fix: the merge's duplicate wave 38.** `PLAN.md`'s diagram keeps one `W38` subgraph, naming both of
  wave 38's rows, and drops `T --> RG`, so it matches the wave table.
- **4, low: the reader's time.** Every caller in the workspace now judges a statement's version at its receiving
  side's time, never the statement's own `made_at`: core-audit's key register at the stated entry's `received_at`
  (`log/keys.rs`, `join` now takes it); the catalogue at the caller's clock `at` (`advertisements.rs`, refused as
  `BadTime` where it can't be written); the stand-ins and the role suite at their own clocks (`MADE_AT`, `TAKEN_AT`);
  module-interface's contract test at its own time. Test `statement_dated_before_its_cutover_but_received_after_it_is_void`
  [`REQ-PRT-4`, `THR-64`]. One caller holds no received time: module admission's `read` (`prov/src/module/admission.rs`),
  which judges a release assertion at its own `made_at`; added to gap 3 for `T-020-54`, which passes the platform's
  clock when older versions are in force.
- **5, low: the holder and `set_for`.** Recorded as gaps, with owners: `arrival.holder` is Core's word, which no row
  ties to the holder its reserve's notice names (the log reads a stated entry's commitment, maker and shape, never its
  body), so R-AWAITING rests on `filing.rs` copying the notice's holder (gap 10, owner `T-020-50`, which reads the
  notice in the filing: a receipt-side check of the holder against the kept notice); and `DepositDestination.set_for`
  isn't held to the notice's `holder` (gap 11, owner `T-020-50`, whose filing takes the destination). The destination's
  changelog entry is now `Ignorable: no`: it decides where a deposit goes.
- **6, low: the guard's markers.** `check_ignorable` reads `Ignorable:` followed by the whole word `yes` or `no`,
  exactly once, so `Ignorable: none`, `Ignorable: not sure` and a marker said twice say neither; `adds a protection` is
  read whatever its case. The guard test gains those cases.
- **7, low: docs.** `wait_ended`'s doc and `deposit_of`'s, and `check_ignorable`'s and `new_bullets`'s, each back on
  their own item; the core-internal test's comment says what the two-rule case changes; the note on state-machine
  above corrected, and state-machine's unreleased 3.0.0 says it follows provider-role 4.0.0.
- **8, low: terms passed unsigned.** The platform's output schema now admits `terms`, holds and status answers, so a
  dry-run answer carrying `terms` made by another party is passed on unsigned inside the signed result, where it was
  `MalformedOutput`; terms made by the provider itself are still refused (`NestedOfAnotherShape`). Today the router's
  `is_void` refuses such a figure (`NestedMaker`), so the harm falls on the module alone. Gap 12, owner `T-020-47`
  (Core taking terms from what the platform hands back): the signer refuses a nested envelope not made by the
  provider, or signs the provider's own.
- **Gaps added:** 3 now also names module admission's `read`; 10, 11 and 12 above.

- **Rule-offs of the fix round** (from `6cc50683`'s tree, script `ruleoff-fix1.py` in the row's scratch folder; none hung):

  | Rule switched off | Red |
  |---|---|
  | `check_status_answer`: the answer's maker is the provider asked | fixtures, `status_question_is_answered_by_the_providers_own_result_for_the_transaction_asked_about` |
  | `check_hold_answer`: the answer's maker is the reserve asked | fixtures, `hold_request_is_answered_by_the_reserves_own_hold_of_what_was_asked` |
  | `check_hold_answer`: a request of another kind | the hold test |
  | `Action::check`: a setting about a provider names it | `each_invalid_fixture_breaks_only_the_rule_it_names`, `every_invalid_fixture_is_rejected_by_the_rust_types_too`, fixtures |
  | the guard: `Ignorable:` read as a prefix (`none`, `not sure`) | `changelog_entry_that_says_neither_ignorable_nor_not_fails_the_contract_guard` |
  | the guard: a marker said twice taken | the same |
  | the guard: `adds a protection` read in its case | `change_adding_a_protection_marked_ignorable_fails_the_contract_guard` |
  | a caller judging at the statement's `made_at` again | none in the workspace: every caller holds one version in force, so the time isn't read yet; the contract's rule is shown by `statement_dated_before_its_cutover_but_received_after_it_is_void`, and the callers' is `T-020-54`'s to test once older versions are in force |
- **The guard on its own changelog.** Read whatever its case, `adds a protection` also matched the rule's own entry,
  which explains the rule and is `Ignorable: yes`; that entry now says "bringing a new protection". Against
  `origin/develop`: "1906 changed path(s) … no violations".
- **Local checks** (through the lock, at the fix commit): fmt, workspace clippy `-D warnings`, `rutent-contracts`,
  `rutent-contract-guard`, `rutent-core-authz`, `rutent-core-orch`, `rutent-core-audit`, `rutent-router`,
  `rutent-conformance`, the scenarios `hand_on`, `authz_scenarios` and `deposit_waits`, the doc check, plan-check and
  tag-check `--strict` (0 findings, 2211 tests tagged), and the guard against `origin/develop` as above.

### T-020-45, fix round 2 (the approval of `6cc50683`)

- **Module admission's time.** `Platform::admit` computes `admitted_at` before reading the assertion, so
  `admission::read(received, deployment, versions, admitted_at)` now takes the deployment, the versions in force and
  that time, and judges the version there, never at the assertion's own `made_at`; the platform passes its one
  version (`ProtocolVersions::only`). Test `assertion_dated_before_its_cutover_but_admitted_after_it_is_refused`
  [`REQ-PRT-4`, `THR-64`] (`components/prov/tests/module_admission_signed.rs`): an assertion under 3.0.0 made before
  its cutover and read after it is `WrongContext(PastCutover)`, and read before it is read. Edits
  `components/prov/src/module/admission.rs` and `platform.rs` (`DEC-20` paths). Gap 3's admission part is closed.
- **The fix round's rule-off table** names `6cc50683`'s tree, on the branch, in place of `accd6812`, an amended
  commit that isn't.
- **Gap 13 (new): the guard's protection check reads free text.** "adds a protection" is matched anywhere in an entry,
  so an entry that only mentions the words (as the rule's own did) is held to `Ignorable: no`. Owner: a later guard
  change (a marker of its own, such as `Adds a protection.` as a whole sentence, the lead's).
- **Rule-off.** `read` judging at the assertion's `made_at` again turns
  `assertion_dated_before_its_cutover_but_admitted_after_it_is_refused` red.


### T-020-45, fix round 3 (CI on `m3-next` at `f01bc669`, run 45484769309)

- **The boundary check and the contracts crate's own dev-dependency.** The `rule-switches` feature reaches the
  contract tests through the crate's dev-dependency on itself, which `tools/boundary-check` read as an edge
  `rutent-contracts → rutent-contracts`, failing `core_reaches_libraries_only_through_their_public_contracts` and
  `only_the_scenario_assembly_may_import_core_crates`. `violations` now takes a crate's dev-dependency on itself, by
  its own package name, as no edge between two crates; a crate depending on itself outside its dev-dependencies, and
  every real edge, are judged as before. The switch stays out of every shipped build. Test
  `a_crates_dev_dependency_on_itself_is_no_boundary_crossing` [`REQ-MOD-1`, `REQ-MOD-3`]: the self dev-dependency
  passes, a dev-dependency on another crate of ours and a normal self-dependency are each reported.
  `tools/boundary-check/` is not a `DEC-20` path.


## T-020-50: an automatic deposit's destination as its reserve names it (Core's half)

Branch `f-020/t-020-50-deposit-destination`, from `f01bc669` (the review round's batch with `T-020-45`'s `destination`
field). Finding 5 of the milestone review of 2026-11-24; the owner's answer `automatic-deposit-destination`: the
destination comes from the user's standing instruction, which the reserve sets from parameters the customer configured
through the integrator or the operator. Edits `components/core-orch/src/arrival/` (`mod.rs`, `filing.rs`),
`core-internal` (a `DEC-20` path, folded into its unreleased 1.0.0), the stand-in reserve
(`conformance/src/role/stand_in.rs`, an `F-024` file, kept to one optional instruction) and a scenario
(`conformance/scenarios/tests/deposit_destination.rs`).

- **What a filing does.** `Recorded` (an arrival Core took) keeps the destination its notice names, read from the signed
  bytes with the other figures. `open_run`, before any order opens:
  1. the entry Core is about to build agrees with the kept notice (`kept_notice_agrees`: the evidence store's bytes
     behind the notice's commitment, read as the log's own read gives them, state the holder, the destination and the
     figures `Recorded` holds; a notice no longer kept, or kept as another statement, agrees with nothing): else an
     `order_refusal` of `malformed_request`, naming no order (gap 10 of `T-020-45`, closed on Core's side);
  2. a destination `set_for` another party than the holder is `destination_refused` (gap 11, Core's side);
  3. `well_formed` holds the filing's destination to the notice's where it names one: its reserve, outer node and
     account, and the route's to asset, no other: else `malformed_request`. Where the notice names none, the filing is
     for what it names as before and the holder rule below stands;
  4. the destination still resolves through `REQ-ORD-24` to an owner, and that owner is the beneficiary, who is the
     holder (the holder rule stays as the floor); the authorisation, given by the filer, carries that destination, so
     policy and the log hold every move to it as for any destination (`move-rests-on-its-authorisation`).
  The `arrival` entry naming the order carries the notice's `destination` beside its `notice` commitment and its
  `holder`: the order's record names the reserve's statement the destination came from (`REQ-ORD-15`).
- **A reserve that never declared automatic deposits.** `Arrivals::file` already refuses with `NotDeclared`, before an
  order, an entry or a question, whatever the notice names; the scenario shows it for a notice that names a destination.
- **The log's rows** (`core-internal`, three fields appended to `automatic-deposit-rests-on-its-arrival`; the log reads
  no notice's body, so they hold Core's entry to the order, not the entry to the notice): `requester` (an arrival naming
  a holder names the acceptance's requester), `destination` (a destination the arrival names is the destination and to
  asset of the permitted `authorise` the acceptance names), `set_for` (that destination was set for the acceptance's
  requester). New field `arrival.destination` (provider-role's `DepositDestination`, optional), schema, README row and
  changelog. Seven admission fixtures (`--naming-its-destination`, `--naming-its-holder` admitted; `--destination`,
  `--destination-in-another-asset`, `--destination-with-no-authorisation`, `--set_for`, `--requester` refused).
- **The stand-in reserve** takes the customer's parameters with `Reserve::with_deposit_instruction(account, set_by,
  set_for)` and names that destination (its own node, the credit's asset) in the notices of `set_for`'s credits, never
  another holder's. The scenarios hand-sign their notices as `assembly_arrivals.rs` does, so they don't read it; its own
  test is in `role_arrivals.rs`, and the role suite holds what it signs.
- **Defaults taken (reversible, fail closed).** (a) The destination's owner must still be the holder (on the owner's
  form as `standing-instruction-third-party`): a standing instruction naming an account another party owns is
  `destination_refused`, until an owner choice says an instruction may pay a third party. (b) A filing naming another
  destination than the notice's is `malformed_request`, not silently replaced by the notice's: the filer states what it
  files, and Core holds it to the reserve's word. (c) A notice whose bytes the store no longer keeps (erased,
  `REQ-AUD-13`) can't be filed on: refused, permanently for that request identifier, free again under another.
- **Gaps.**
  - 10 and 11 of `T-020-45` are closed as far as the log can: its rows can't read a notice's body, so `arrival.holder`
    and `.destination` remain Core's entry, tied to the order's requester and authorisation by the rows above and to the
    kept notice by `kept_notice_agrees`, before the entry is built. A log that read stated bodies would close the rest;
    owner `T-025-05`'s review of the residual.
  - The kept notice is read once per filing step 1 (an extra read of the store per filing, not per call carried on).
  - A real reserve's side (`T-031-01`, outside this repository) reads the customer's parameters; the stand-in's
    instruction is one account per holder, with no history of who changed it (`REQ-AUZ-9` records one setter).
  - An erased notice (default c) ends the filing permanently; whether the reserve should be told is `T-031-01`'s.
- **Tests** (`conformance/scenarios/tests/deposit_destination.rs`, against the simulated reserve that declares automatic
  deposits): the row's four, `destination_set_for_another_party_or_not_the_filings_is_refused`; `core-orch`'s
  `filing_on_a_deposit_whose_kept_notice_is_erased_or_replaced_is_refused_before_an_order_opens`; `core-internal`'s
  `arrival_naming_a_destination_holds_the_orders_authorisation_to_it`; the stand-in's
  `stand_in_reserve_names_the_destination_its_customer_set_in_its_arrival_notice`. The four named tests' first lines
  of defence were already in place when written (the holder rule admits a destination the holder owns, and the
  arrival's plumbing had landed), so they were seen red under rule-offs below rather than before the code.
- **Rule-offs** (each rule switched off alone, the named test turned red; script `ruleoff.py` in the row's scratch
  folder; none hung):

  | Rule switched off | Red |
  |---|---|
  | log: an arrival's holder is the acceptance's requester | `arrival_naming_a_destination_holds_the_orders_authorisation_to_it` (`--requester`) |
  | log: the arrival's destination is the named authorisation's (reserve, node, account, to asset) | the same (`--destination`, `--destination-in-another-asset`, `--destination-with-no-authorisation`) |
  | log: the destination was set for the requester | the same (`--set_for`) |
  | `well_formed`: the filing's destination is the notice's | `destination_set_for_another_party_or_not_the_filings_is_refused` |
  | `open_run`: a destination set for another party is refused | the same |
  | `open_run`: the kept notice agrees with the entry | `filing_on_a_deposit_whose_kept_notice_is_erased_or_replaced_is_refused_before_an_order_opens` |
  | the `arrival` entry carries the notice's destination | `orders_record_names_the_reserves_statement_its_destination_came_from` |
  | the notice read keeps its destination | the same, and `automatic_deposit_goes_where_its_reserve_sets_it_from_the_customers_parameters` |
  | the stand-in names the instruction's destination | `stand_in_reserve_names_the_destination_its_customer_set_in_its_arrival_notice` |

### T-020-50, fix round 1 (the review of `0f45cab1`, PR #297)

- **1, should-fix: the `REQ-ORD-13` test now needs the notice's destination.** It files to another account the holder
  owns first and asserts that refused (it stayed green with the filing's-destination check off before), then to the
  reserve's. Its notice is the simulated reserve's own: `Spec::with_deposit_instruction` (the stand-in's instruction,
  set by `integrator-1` for the customer) makes the reserve sign it, taken through `Spec::make`, `credit_arrival` and
  `arrival_notice`, and not hand-signed; the arrival entry's `set_by` is asserted to be the integrator's. The other
  scenarios keep hand-signed notices where the stand-in can't be made to say it (a destination set for another party,
  a reserve naming none).
- **2, low:** `role_arrivals.rs` sets its role-suite instruction for `conformance-holder`, whose credits the suite
  makes, so the suite now checks a notice that names a destination.
- **3, low (gap): Core checks `set_for`, not `set_by`.** Who set the parameters (the customer, an integrator, the
  operator, an agent) is recorded in the arrival entry and read by nothing: whether that party may set them for the
  holder is the real reserve's to check before it names a destination. Owner `T-031-01`.
- **4, low:** the `core-orch` test also replaces the kept notice (`SharedKept::replaces`, new in `intake_support`: the
  store answers another statement, another holder, under the commitment of the one kept; `alters`, one byte more, is the
  same statement and agrees) and expects the same refusal before an order opens; renamed
  `..._is_erased_or_replaced_is_refused_...`.
- **5, low:** default (a) is named above as `standing-instruction-third-party`.
- **6, low:** the fixture `--holder` is `--requester`, as the field it breaks.
- **Rule-offs of the round.**

  | Rule switched off | Red |
  |---|---|
  | `well_formed`: the filing's destination is the notice's | `automatic_deposit_goes_where_its_reserve_sets_it_from_the_customers_parameters` |
  | the stand-in names the instruction's destination (`Reserve::notice_of`) | the same, and `stand_in_reserve_names_the_destination_its_customer_set_in_its_arrival_notice` |
  | `open_run`: the kept notice agrees | `filing_on_a_deposit_whose_kept_notice_is_erased_or_replaced_is_refused_before_an_order_opens` |
  | `kept_notice_agrees`: the holder compared | the same, at its replaced step |
- **Correction found after the first push of this round** (`61e6ea10`): its replaced-notice step used `alters`, which
  keeps the statement and so was green; the follow-up commit carries `replaces` and the rule-off above.

## T-020-46: Core rebuilds its state from the log when it reopens

Branch `f-020/t-020-46-core-rebuilds`, from `f01bc669` (finding 3 of the milestone review of 2026-11-24, issue #269). A
first, partial round (`6c6b532b`, `3800e8ba`) stopped on a missing contract shape; the lead authorised the shape
(`DEC-20`, the owner's click at the release; the owner's rule to fix contract gaps pre-production) and this round
finishes the row. Edits `contracts/` (core-internal), `components/core-orch/` (`segments/`, `stuck/`, `hand_on/`,
`rounds/`, `intake/`), `components/prov/src/holding/` (the host's re-landing) and the `F-024` files the commit bodies
name: the assembly's `reopened` (`conformance/scenarios/src/assembly/mod.rs`, `holding.rs`, the new `reopen.rs`),
`harness.rs`'s `Scenario::reopen`, and the support kit's `reopen_segments`.

- **The contract (core-internal, folded into the unreleased 1.0.0).** `order_opened.inputs`: the salted commitment of
  the operator's statement of the order's inputs, a new shape `urn:rutent:core-internal:order-inputs` (`OrderInputs
  {order, inputs}`, `ORDER_INPUTS_SHAPE`), kept in the evidence store and appended as a stated entry before the
  `order_opened`, about nothing (the statement names no transaction, so attribution verifies it only about nothing).
  The log holds no account (`REQ-AUD-6`). Row `opened-orders-inputs-are-a-kept-statement` (`REQ-AUD-6`, `NFR-6`): a
  stated entry holding that commitment, verified, of that shape, the deployment operator's own, about nothing; that
  its body names the order and its inputs is for whoever holds the kept bytes. Fixtures: valid
  `order-inputs-of-an-order`, `verified-entry-of-an-order-opened-with-its-inputs`; invalid
  `order-inputs-without-its-order` (schema and Rust); admission `--the-operators-statement` (admitted), `--missing`,
  `--verified`, `--inputs`, `--maker`, `--about`. Changelog and README (citation table, shapes table) updated; 29
  schemas, 115 relations. Script `gen_inputs.py` in the row's scratch folder.
- **The rebuild.** The log is Core's durable record; a Core that reopens reads its own state back from it, beside
  `Closing::recorded`, `Holds::recorded` and `recover_starts`:
  - `Issued::recorded`: every transaction any entry names under a `transaction` key, at any depth. A round appends its
    question about its transaction before it sends it, so every transaction a provider was asked under is there,
    answered or not: `T-020-45`'s gap 4 is closed.
  - Intake signs the order's inputs as the operator's `order-inputs` statement, keeps it and names its commitment in
    `order_opened`. `Intake::reopened(history, &mut dyn Reopening)` rebuilds each order under its requester's request
    identifier with its inputs read back from the kept statement (held to the order it names): accepted and started
    where the log holds its acceptance and `route_chosen`, refused where it holds its refusal, its outcome to come
    otherwise; a repeat is held to the fields the log holds (`same_logged_request`). `T-020-45`'s gap 5 is closed.
  - `Segments::reopened` (`segments/reopen.rs`): the orders followed, every effect acted on (an effect is acted on
    only after its entry is in the log, so no second `move_value` or `resolve_stuck` follows a restart), the
    transactions, the resolved parts, the platform's unanswered records, the charges, and each segment handed over
    from its `move_value` effect and the operator's question (bytes read back from the evidence store): settled on what
    its provider's kept result gave out, judged again at the result's `received_at`; failed; withdrawal cancelled;
    unknown; refused before anything passed out where its one result is `failed` with no outcome input. One with no
    final input is rebuilt carried and never carried out again (`InFlight::reopened`, `SegmentsError::AlreadyActed`),
    in flight and held; one whose operation can't be read back is held as acted on (`unread`).
  - `Exposures`: every segment rebuilt in flight held at the amount its signed operation moved; every unresolved stuck
    order marked and its resting value held (`Stuck::reopened`).
  - `Stuck::reopened` (`stuck/reopen.rs`): declarations, ends, what moved (every `resolve_stuck` effect whose result is
    `done`), kept decisions (permitted `stuck_decide`s no effect carries out), moves in motion (acted; a fresh mover
    answers them unknown, `Kept::Lost`).
  - `HandOn::reopened` (`hand_on/mod.rs`): each part whose `handed_on` the log took with no `route_chosen` of its linked
    order and no `hand_on_taken_back` is pending, its `handed_on` as the log holds it and the first order as it stood
    before it. The linked order's segment ids are the caller's (its accepted plan), so a retry naming the same linked
    order builds the `route_chosen` from its route on that earlier state (`build_on`) and appends it alone; one naming
    another is `PendingForAnother`.
  - The holding accounts (`components/prov/src/holding/wallet.rs`): `WalletHolding::held_by_wallet` asks the wallet
    what it holds for an order (`balance_verification`, per order, a search over the amount, each question under a
    transaction derived from Core's), never the host's attribution and never a repeat landing's `succeeded`;
    `WalletHolding::restore` lands again only what the wallet lacks, under a transaction of Core's own (the host's
    counter would meet `DuplicateTransaction` from the platform's durable record), and caps the host's attribution at
    the log's amount, so nothing is attributed twice; `WalletHolding::restarted` compares the wallet's process with
    the one that last confirmed a movement, as the platform reports it. Core: `Wallets::withdraw` restores the value
    the log holds before a hand-on's withdrawal where the account's module restarted (a fresh transaction drawn by the
    assembly's `move_stuck` only then); `Segments::reconcile_landings`, run on reopening, asks each account what it
    holds for each segment the log settled at its node and restores only what it lacks, never landing blind (the first
    round's known risk is closed).
  - The assembly's hand-on after a reopening: the linked order's segment ids are derived from its part,
    `{part}-s{n}` (`linked::segment_id`), which its logged acceptance names, so the accepted route is rebuilt from the
    log and the decision record; `Core::hand_on` reads the holding account's withdrawal from the log where the mover
    holds none (`withdrawn_in_the_log`: the hand-on's effect and `done` result, the wallet's verified answer about the
    linked order, its record from the kept bytes).
  - The assembly: `Core::reopened` carries the operator key with the decision records it signed and the host's
    holding accounts (beside the platform), lists the accounts again (`relist_holdings`), replays an operator-published
    wallet's `AcceptModule`, and `rebuild_from_log` puts the pieces together and rebuilds each started order from the
    log, its inputs from intake's. Test hooks: `Core::set_provider_ceiling`, `Core::restart_module`.
- **Named tests.** `core_reopened_mid_order_carries_on_as_the_core_that_ran_on` [`NFR-6`],
  `no_value_moves_twice_after_core_reopens` [`INV-8`],
  `exposures_are_rebuilt_from_the_log_so_routes_respect_the_ceiling_after_a_restart` [`REQ-ORD-8`, `THR-74`],
  `stuck_parts_kept_decision_and_move_are_rebuilt_from_the_log` [`INV-4`] (`tests/reopened.rs`);
  `pending_hand_on_is_rebuilt_from_the_log` [`REQ-ORD-43`] (`tests/hand_on.rs`);
  `holding_account_whose_module_restarted_is_re_landed_from_the_log_before_a_release` [`REQ-ORD-43`, `INV-1`],
  `stuck_order_is_handed_on_after_the_holding_accounts_module_restarts` [`REQ-ORD-43`] (`tests/assembly_real_wallet.rs`).
  Beside the pending hand-on's own test, which runs on the scenario kit, the assembly's wiring has one of its own (the
  lead's ask, a new name for `PLAN.md`): `pending_hand_on_is_rebuilt_from_the_log_through_the_assembly` [`REQ-ORD-43`,
  `INV-4`], a crash right after the first order's `handed_on`, the assembly reopened, the hand-on asked again: the
  linked `route_chosen` and the holding account's start each once, then the linked order's first move runs on the
  route it logged and settles.
  Beside them: `reopened_segments_move_nothing_twice_where_the_log_would_take_a_second_move` [`INV-8`]
  (`tests/segments.rs`, against the kit's log, which admits a second move the real log refuses, so Core's own rule is
  what holds), `done_move_is_counted_as_moved_after_a_restart` [`INV-8`, `INV-4`] (a crash right after a resolution's
  `done` result, before its `stuck_resolved`) and `transactions_the_log_names_are_held_as_used_after_a_restart`
  [`INV-8`] (`tests/reopened.rs`).
- **Rule-off table** (each switched off alone from the committed checkpoint, the binaries run through the lock with
  the row's private wallet build, the file restored with `git checkout`; scripts `ruleoff.py`, `ruleoff2.py` in the
  row's scratch folder). "Scenarios" are `reopened`, `hand_on`, `segments` and `assembly_real_wallet`; "table" is
  core-internal's `every_relation_of_the_citation_table_holds_and_breaks_as_its_cases_say`.

  | Rule switched off | Red |
  |---|---|
  | Core's flights neither rebuilt nor held unread | `core_reopened_mid_order_carries_on_as_the_core_that_ran_on`, `exposures_are_rebuilt_from_the_log_so_routes_respect_the_ceiling_after_a_restart`, `stuck_parts_kept_decision_and_move_are_rebuilt_from_the_log`, `pending_hand_on_is_rebuilt_from_the_log`, `holding_account_whose_module_restarted_is_re_landed_from_the_log_before_a_release`, `reopened_segments_move_nothing_twice_where_the_log_would_take_a_second_move` |
  | no flight rebuilt (each held unread) (first round) | the stuck and exposures tests |
  | no hold for a segment rebuilt in flight (first round) | the exposures test |
  | an effect failed alone read as in motion (first round) | the stuck test |
  | no resting hold for a stuck part (first round) | the stuck test |
  | no decision kept (first round) | the stuck test |
  | `Issued::recorded` holds nothing | `transactions_the_log_names_are_held_as_used_after_a_restart` |
  | a `done` move not counted as moved | `done_move_is_counted_as_moved_after_a_restart` |
  | no inputs statement written | `core_reopened_mid_order_carries_on_as_the_core_that_ran_on` |
  | intake reads no inputs back | the same |
  | the assembly doesn't rebuild the hand-ons | `pending_hand_on_is_rebuilt_from_the_log_through_the_assembly` |
  | `HandOn::reopened` finds nothing | `pending_hand_on_is_rebuilt_from_the_log` |
  | a rebuilt hand-on built on the order as it stands | the same |
  | no withdrawal read from the log | `pending_hand_on_is_rebuilt_from_the_log_through_the_assembly` |
  | linked segment ids drawn fresh, not derived | the same |
  | no restore before a hand-on's withdrawal | `stuck_order_is_handed_on_after_the_holding_accounts_module_restarts` |
  | no landings reconciled on reopening | `holding_account_whose_module_restarted_is_re_landed_from_the_log_before_a_release` |
  | a restore lands all, the wallet not asked | the same (the wallet would hold the log's amount twice) |
  | a restore attributes beyond the log's amount | both wallet tests |
  | row `opened-orders-inputs-are-a-kept-statement`: cites nothing; verified, shape, maker, about each unread | table, each |

  `no_value_moves_twice_after_core_reopens` stays green with Core's rules off: the real log's
  `part-waits-for-value-in-motion` refuses a second move too. Core's own rule is shown by
  `reopened_segments_move_nothing_twice_where_the_log_would_take_a_second_move`, against the kit's log, which admits it.
- **Local checks** (through the lock, the plain wallet built privately into the row's scratch folder and named by
  `RUTENT_PLAIN_WALLET`): `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings`
  (after a `needless_borrows_for_generic_args` fix in `linked::segment_id`); `cargo test -p` `rutent-core-orch`,
  `rutent-contracts`, `rutent-prov`, `rutent-core-authz --lib`; `rutent-scenarios`' `reopened`, `hand_on`, `segments`,
  `assembly_real_wallet`, `assembly`, `assembly_stuck`, `assembly_hand_on`, `assembly_intake`, `assembly_wiring`,
  `charge_check`, `holds`, `stuck_hand_on`, `stuck`, `closing`, `invariants`, `hand_on_record`, `landing` (all pass);
  the doc check; plan-check and tag-check `--strict` (0 findings, 2263 tests tagged); the contract guard against
  `origin/develop` ("1934 changed path(s) … no violations"). The full suite runs in CI.
- **Tests first.** Not kept for the first round's rebuild, written before its tests; every test was then watched red on
  an assertion by switching its rule off (the table below), which for the first round is the base's behaviour.
- **Defaults** (reversible, failing closed): a segment logged with no final outcome is never carried out again after a
  reopen (asking the platform again under its transaction is a later choice); one that can't be read back is never
  handed over again; an effect whose one result is `failed` reads as the platform's refusal to begin
  (`PlatformRefusal::Unavailable`); a settled amount is judged again at its result's `received_at`; the decision records
  go on with the operator key, and the host's holding accounts with the platform; a reopened quote is shown again on a
  new routing; a reopened flight's stuck clock starts at the reopening; a restore asks the wallet by
  `balance_verification` over a binary search of the amount (at most about 20 questions for six digits); a segment
  whose value moved on before a reopen is read as passed, its release not asked again; the deployment's registers and
  settings are set again on each Core put together.
- **Owner's form:** the alternative to the inputs commitment, a reopened order asking its customer again for its
  tuples' inputs; built as the commitment, recommended.
- **Fix round 2** (the reviewer's CHANGES at `a079df9d`, PR #300; `b3092367` and then `m3`'s `98eae9b1` merged first,
  the second for `ad648667`'s `Source::OfOperator` fix, without which `segments`'
  `promotion_whose_staged_version_differs_from_its_permit_is_refused` fails at the base too). Each finding fixed:
  1. *Re-landing per order.* `landing::reconcile` groups what the log holds by node, asset and order and restores the
     sum once (`landing::sum`); `Wallets::withdraw` restores the order's whole at the node (`restored_whole`, from
     `Segments::resting_totals`), never the part withdrawn alone.
  2. *A release after a restart.* `Segments::land_settled` asks `Landings::restarted(node)` and, before a release,
     restores per order what the log holds there (what rests and what is about to leave); a restore that can't be made
     now (`NotNow`) leaves the releases for the next call, one not confirmed asks none.
  3. *The missing `stuck_resolved`.* `Stuck::reopened` keeps each part whose resolution's `done` the log holds without
     its `stuck_resolved` (`Finishing`), and `Stuck::finish_reopened` appends it once on reopening, as a Core that ran
     on would.
  4. *What the reopened Core rebuilds besides.* `HandOn::reopened` now also rebuilds the stranded refusals (a linked
     order's refusal in the log, its hand-on not taken back: a retry takes it back once), the linked orders opened (the
     first order's value at rest, the starting reserve, the route's first segment) and the starts and spent records the
     log took (`started_in_the_log`, each record read back from the holder's kept answer; a start whose record can't be
  read back refuses every record, `unread_starts`, where before it took another record of the started order). The assembly restarts a
     linked order from its `handed_on_from` (`restarted_linked`) and rebuilds its accepted route
     (`reaccept_linked`). `Prices::recorded` holds each order's executed price as reported; the segments rebuild each
     settled segment's charge (`charged_of`, from the kept result) and read the providers' terms back; a segment whose
     answer the log holds and nothing of its grading goes on from that answer (`resumed_answer`), never asked again;
     `Unsettled::reopened` finds the erasures decided and done but not recorded, settled once. HandOn's changes and
     resumes need no rebuild: once the log takes one, the state machine rebuilt from the log moves the part out of
     `WaitingForCustomer` (`after_restart`), so a retry is refused `NotWaiting` before anything is built; one built and
     not taken is not in the log, and is built again as the first time.
  5. *Failing closed.* (a) A segment handed over whose operation can't be read back is held at the value it states
     (`Hold::Stated`, from the question's kept bytes or else `order_opened`'s amount), unless the log says its value
     moved on. (b) Each decision record Core signs is kept in the evidence store and appended as the operator's stated
     entry of router-api's decision-record shape, about nothing, before its `decision_record` entry; a reopened Core
     reads its records from the store (`records_in_the_store`), never from memory: the signer is no longer carried
     across a reopening. No contract shape was needed beyond the authorised core-internal fold.
  6. *An unreadable inputs statement.* `Intake::reopened` holds the order (`Intake::held`, `request_of` answers
     `None`): it is never run again with its inputs missing, and a statement naming another order counts as unreadable.
  7. *An empty inputs map.* `OrderInputs::check` refuses it ("an order's inputs name at least one input"); invalid
     fixture `order-inputs-naming-no-input`.
- **Fix round 2's tests.** The seven named in `PLAN.md`:
  `pending_hand_on_is_rebuilt_from_the_log_through_the_assembly` (`tests/assembly_real_wallet.rs`),
  `charge_raise_recorded_before_core_reopens_is_not_raised_again` (`tests/charge_check.rs`),
  `private_node_segment_record_queued_before_core_reopens_is_written_once`,
  `private_rail_closed_before_core_reopens_stays_closed` (`tests/private_nodes.rs`),
  `holdings_and_accepted_routes_of_a_reopened_order_are_rebuilt_from_the_log` (`tests/assembly_real_wallet.rs`),
  `executed_price_reported_before_core_reopens_is_not_reported_again` (`components/core-orch/tests/quote.rs`),
  `wallets_balance_answer_attributes_every_order_it_holds` (`components/prov/tests/holding_wallet.rs`). New names for
  `PLAN.md`, each a rule of this round: `release_after_a_holding_accounts_module_restarts_restores_first`
  (`tests/landing.rs`) [`INV-1`, `REQ-ORD-43`]; `two_values_of_one_order_at_one_node_are_restored_as_their_sum`
  (core-orch, `segments::landing`) [`INV-1`]; `a_hand_ons_restore_is_the_orders_whole_at_its_node` (core-orch,
  `stuck::handing`) [`INV-1`]; `in_flight_segment_whose_decision_record_is_unreadable_is_held_at_its_stated_value`
  [`REQ-ORD-8`, `THR-74`], `erasure_done_before_core_reopens_is_settled_once` [`NFR-6`],
  `order_whose_inputs_statement_names_another_order_is_held_on_reopening` [`NFR-6`],
  `segment_answered_before_core_reopens_goes_on_from_its_logged_answer` [`NFR-6`, `INV-8`] (`tests/reopened.rs`);
  `charge_raise_cut_short_before_core_reopens_is_raised_once` [`NFR-6`, `REQ-QTE-5`],
  `terms_stated_before_core_reopens_bind_the_segments_after_it` [`REQ-QTE-6`, `NFR-6`] (`tests/charge_check.rs`);
  `executed_price_reported_through_the_assembly_before_core_reopens_is_not_reported_again` [`REQ-QTE-7`, `NFR-6`]
  (`tests/charge_check.rs`); `stranded_hand_on_is_taken_back_after_a_restart` [`REQ-ORD-43`] (`tests/hand_on.rs`);
  `linked_start_and_its_spent_record_are_rebuilt_from_the_log_after_a_restart` [`REQ-ORD-43`, `REQ-ORD-40`],
  `linked_start_whose_record_cannot_be_read_back_is_proven_by_no_record_after_a_restart` [`REQ-ORD-43`]
  (`tests/hand_on_record.rs`; the kit states the holding account's withdrawal answer as the wallet does,
  `holding_withdrawal_statement`). `done_move_is_counted_as_moved_after_a_restart` now also asks for the
  `stuck_resolved` once after the reopening. The assembly's disk crashes after a stated entry by its shape too.
- **Tests first, fix round 2.** The probe tests of findings 1 and 2 (`two_values_…`, `release_after_…`) were written
  first and watched red on their assertions before the fix; the others were written beside their rules and each
  watched red by switching its rule off (the table below).
- **Fix round 2's rule-off table** (script `ruleoff3.py` in the row's scratch folder; "wide" is the scenario binaries
  `reopened`, `hand_on`, `hand_on_record`, `segments`, `landing`, `assembly_real_wallet` and `charge_check`). Several
  wide runs also showed `segments`' promotion test red: that is the base's failure fixed by `ad648667`, merged since.

  | Rule switched off | Red |
  |---|---|
  | restores not grouped per order | `two_values_of_one_order_at_one_node_are_restored_as_their_sum` |
  | no restore before a release | `release_after_a_holding_accounts_module_restarts_restores_first` |
  | a hand-on's restore lands its own value, not the order's whole | `a_hand_ons_restore_is_the_orders_whole_at_its_node` |
  | a resolution done not finished on reopening | `done_move_is_counted_as_moved_after_a_restart` |
  | decision records not read from the store | the reopened NFR-6, stuck, re-landing, assembly hand-on, holdings and both reopened raise tests |
  | decision record not kept as a stated entry | the same seven |
  | an unread segment not held at its stated value | `in_flight_segment_whose_decision_record_is_unreadable_is_held_at_its_stated_value` |
  | `OrderInputs` admits an empty map | core-internal's fixture tests (`every_valid_fixture_…`, `every_invalid_fixture_…`) |
  | `Prices::recorded` holds nothing | `executed_price_reported_before_core_reopens_is_not_reported_again` |
  | the assembly reports prices afresh | `executed_price_reported_through_the_assembly_before_core_reopens_is_not_reported_again` |
  | a settled segment's charge not rebuilt | both reopened raise tests |
  | terms not read back | `terms_stated_before_core_reopens_bind_the_segments_after_it` |
  | an answered segment not resumed from its logged answer | `segment_answered_before_core_reopens_goes_on_from_its_logged_answer` |
  | a refused hand-on not rebuilt | `stranded_hand_on_is_taken_back_after_a_restart` |
  | an opened linked order not rebuilt | `linked_start_and_its_spent_record_are_rebuilt_from_the_log_after_a_restart` |
  | starts and spent records not rebuilt | both linked start tests |
  | an unreadable start proven by any record | `linked_start_whose_record_cannot_be_read_back_is_proven_by_no_record_after_a_restart` |
  | a linked order not restarted | `holdings_and_accepted_routes_of_a_reopened_order_are_rebuilt_from_the_log` |
  | linked acceptances not rebuilt | the same |
  | unsettled erasures not found | `erasure_done_before_core_reopens_is_settled_once` |
  | an unreadable inputs statement not held | `order_whose_inputs_statement_names_another_order_is_held_on_reopening` |
  | an inputs statement's order not checked | the same |
- **Fix round 2's local checks** (through the lock, the row's private plain-wallet build named by
  `RUTENT_PLAIN_WALLET`): `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets -- -D warnings` (after
  boxing `Hold::Route`'s operation and naming `landing::reconcile`'s group type); `cargo test -p` `rutent-core-orch`,
  `rutent-core-authz --lib`, `rutent-contracts`, `rutent-prov`; `rutent-scenarios`' `reopened`, `hand_on`,
  `hand_on_record`, `segments`, `landing`, `private_nodes`, `charge_check`, `assembly_real_wallet`, `assembly`,
  `assembly_stuck`, `assembly_hand_on`, `assembly_intake`, `assembly_wiring`, `holds`, `stuck_hand_on`, `stuck`,
  `closing`, `invariants`, `audit_scenarios`, `stuck_mover` (all pass); the doc check; plan-check and tag-check
  `--strict` (0 findings, 2322 tests tagged); the contract guard against `origin/develop` ("1950 changed path(s) …
  no violations"). Against `origin/m3`, the pull request's own base, the guard flags core-internal's 1.0.0 as not
  bumped, as it does for every row folding into an unreleased major on `m3` (`T-021-16`'s too); the fold is the
  lead's authorisation. The full suite runs in CI.
- **Fix round 2's defaults** (reversible, failing closed): an order whose inputs statement can't be read back, or names
  another order, is held on reopening, never run; a segment whose operation can't be read back is held at its stated
  value; decision records live in the evidence store as the operator's stated entries; a restore after a module
  restart is per order and of its whole at the node; a segment answered before the stop goes on from the logged answer
  and is never asked again; a linked start the log holds whose record can't be read back refuses every record, the one
  the log took included, the caller reading its receipt from the log; a change or resume retried after a reopening is refused `NotWaiting` where a Core that ran
  on would answer its receipt (nothing is appended twice either way).
- **Fix round 3** (the reviewer's CHANGES at `935f7d2c`, PR #300 comment 7153083056; `m3`'s `7b9d41c5` merged
  first; the lead's decisions on its open points):
  - *A, `person_records.rs:201`.* `order_opened.parties` names the payer only as a party identifier (`Parties.payer`,
    an `Id`), so no contract change: `records_after_a_third_party_payment_hold_a_reference_at_most` admits the
    payer's reference in the acceptance and in `order_opened`, each in `parties.payer` alone, an identity as a
    reference as `REQ-AUD-6` allows, and says so in its doc.
  - *B, retired holds.* `hold_in_flight` holds no segment the state machine rebuilt from the log retired
    (`retired_in_the_log`, the part's retired segments through `Segments::retired`): one a recalculation, a resume or
    a destination change replaced; the unread segments held at their stated value skip them too. The reviewer's probe was written first and seen red
    (`[("sim-reserve-a", "1000000")]` against `[]`): `segment_a_destination_change_retired_is_not_held_again_after_a_restart`
    (`tests/hand_on.rs`, an unread variant inside it, the kit's `reopen_segments_reading`), with
    `segment_a_hand_on_retired_is_not_held_again_after_a_restart` (`tests/hand_on_record.rs`) beside it.
  - *C, the whole-order restore through its call site.* The assembly's `Core::withdraw_for_a_hand_on` (an `F-024`
    file) runs a hand-on's withdrawal through `Wallets::withdraw` with the log's resting value of the order at the
    node given: `hand_on_after_a_module_restart_restores_the_orders_whole_value_at_the_node`
    (`tests/assembly_real_wallet.rs`): two values of the order at the plain wallet's node, the module restarted,
    one part withdrawn; the wallet still holds the other value and the host attributes it.
  - *Gap 2, the measures' queue.* `Segments::reopened` rebuilds the writer's queue a restart drops
    (`measure_what_the_log_lacks`): each rebuilt segment ending and each completed reversal gives its records again,
    citing what the Core that ran on cites (the effect, the provider's answer as the log took it, a failed result;
    the answers' records the log's `decision_record` entry, which rebuilt flights now name), timed at the log's
    record of the ending, and every record the log already holds is dropped (`Measuring::drop_written`), so a writer
    cut short keeps the rest. Tests: `measure_records_a_restart_dropped_are_written_once_after_core_reopens`
    (`tests/charge_check.rs`, written first and seen red, 0 against 6) and
    `reversal_completed_before_core_reopens_is_measured_once` (`tests/measures_called.rs`).
  - *Low, a change or resume retried after a reopening.* Left as it is, named: answering the log's receipt would mean
    rebuilding a `Changed`'s route end and the value's place from the state before the change; a Core that ran on
    answers `NotWaiting` too once the log took its change, so the order carries on either way.
  - *Low D.* `pending_hand_on_is_rebuilt_from_the_log_through_the_assembly` carries `REQ-ORD-43, INV-4` where PLAN
    lists `REQ-ORD-43`; for the lead with the new names at merge.
- **Fix round 3's new test names for `PLAN.md`:**
  `segment_a_destination_change_retired_is_not_held_again_after_a_restart` [`REQ-ORD-8`],
  `segment_a_hand_on_retired_is_not_held_again_after_a_restart` [`REQ-ORD-8`],
  `hand_on_after_a_module_restart_restores_the_orders_whole_value_at_the_node` [`INV-1`, `REQ-ORD-43`],
  `measure_records_a_restart_dropped_are_written_once_after_core_reopens` [`REQ-AUD-7`, `NFR-6`],
  `reversal_completed_before_core_reopens_is_measured_once` [`REQ-AUD-7`, `NFR-6`].
- **Fix round 3's rule-off table** (script `ruleoff4.py` in the row's scratch folder):

  | Rule switched off | Red |
  |---|---|
  | a retired segment held again (`hold_in_flight`) | `segment_a_destination_change_retired_is_not_held_again_after_a_restart` |
  | an unread retired segment held again | the same (its unread variant) |
  | a hand-on's restore of the part alone at its call site (`stuck/handing.rs:178`) | `hand_on_after_a_module_restart_restores_the_orders_whole_value_at_the_node` |
  | no measure records rebuilt | `measure_records_a_restart_dropped_are_written_once_after_core_reopens`, `reversal_completed_before_core_reopens_is_measured_once` |
  | records the log holds kept again (`drop_written`) | both |
  | no reversal record rebuilt | `reversal_completed_before_core_reopens_is_measured_once` |
  | a rebuilt ending cites no answer | `measure_records_a_restart_dropped_are_written_once_after_core_reopens` |
  | rebuilt answers cite no decision record entry | the same |

  Two further branches first written into the retired check (a segment in no current plan, a part handed on)
  turned no test red: `Segments::retired` covers the change, and the hand-on's retired deposit was held by nothing
  already. They were taken out, and `segment_a_hand_on_retired_is_not_held_again_after_a_restart` stays as the
  hand-on's guard.

- **Fix round 3's local checks** (through the lock, `contracts/src/lib.rs` touched first, the row's private
  plain-wallet build): fmt; workspace clippy `-D warnings`; `cargo test -p` `rutent-core-orch`, `rutent-core-authz
  --lib`, `rutent-contracts`, `rutent-prov`; `rutent-scenarios`' `reopened`, `hand_on`, `hand_on_record`,
  `segments`, `landing`, `private_nodes`, `charge_check`, `assembly_real_wallet`, `assembly`, `assembly_stuck`,
  `assembly_hand_on`, `assembly_intake`, `assembly_wiring`, `holds`, `stuck_hand_on`, `stuck`, `closing`,
  `invariants`, `audit_scenarios`, `stuck_mover`, `person_records`, `measures_called`; CI's seeded step
  (`RUTENT_SCENARIO_SEEDS=11,12,13,14,15`, `assembly` and `invariants`); the doc check; plan-check and tag-check
  `--strict` (0 findings, 2335 tests tagged); the contract guard against `origin/develop` (1951 paths, no
  violations). All pass.
- **Round 4** (issue #269 reopened; the milestone reviewer's High on T-020-46 at `1bc91b21`, its notes in the review
  chat of 2026-11-25; branch `f-020/t-020-46-r4`, cut from `1bc91b21`; the lead's decisions):
  - *High: a completed hand-on was landed again after a reopening (`INV-1`, `INV-8`).* `Segments::land_settled` and
    `Segments::reconcile_landings` now read a settled segment's value through one predicate,
    `Segments::settled_at_node`: a segment the state machine retired is skipped; one whose next segment settled, or
    that was reversed (`ReversalComplete`), moved on; one whose part a resolution or a hand-on ended
    (`ended_by_resolution`) took its value with it, so nothing is landed, released or restored for it. A reopened
    Core marks resolved each part a done `resolve_stuck` moved, as the Core that ran on marked it once the move was
    done, before its `stuck_resolved` or `handed_on`. The reviewer's two probes were written first and seen red (the
    wallet attributing the handed-on order's 1,000,000 again):
    `handed_on_value_is_not_landed_again_after_core_reopens` and
    `value_withdrawn_for_a_hand_on_is_not_landed_again_when_core_reopens_before_the_hand_on`
    (`tests/assembly_real_wallet.rs`, the attributions empty and the wallet holding nothing for the order). The
    reversed case: `reversed_segments_landing_is_not_restored_after_core_reopens` (`tests/landing.rs`, the kit's
    ledger account, the withdrawal proven undone: the Core that ran on releases the landing, the reopened one
    restores nothing). Through the assembly the reversal of the landed first segment isn't applicable in the
    chain route the wallet's node is in (the part isn't open with only pending segments after it), so the kit carries
    it.
  - *Medium: what `Segments` and the stuck record lost, each rebuilt or said why.*
    - `reliances`: rebuilt (`Reliances::recorded`). Contract: core-internal 1.0.0's `reliance` entry gains
      optional `inputs`, the order's inputs by name declared on the judgement relied on (folded into the unreleased
      1.0.0 on the lead's authorisation, a `DEC-20` path; valid fixture
      `verified-entry-of-reliance-naming-the-inputs-resting-on-it`, README row, CHANGELOG line). Core writes it; a
      reopened Core rebuilds which input rests on which judgement and the entries written, so a later segment writes
      its reliance and none is written twice. A declaration no segment relied on yet left no entry: it is the
      caller's word, given again. Test: `which_input_rests_on_which_judgement_is_rebuilt_from_the_log_after_a_restart`
      (`tests/reliance.rs`).
    - `refused` (a guard refusal): rebuilt by order and the refused route's decision record (`refused_in_log`); the
      `order_refusal` names no reason, so while the guard refuses that route the logged entry is the answer. Test:
      `guard_refusal_recorded_before_core_reopens_is_not_recorded_again` (`tests/closing.rs`).
    - `floor_waits`: rebuilt from the log's `waiting_at_floor` inputs. Test:
      `floor_wait_recorded_before_core_reopens_is_not_recorded_again` (`tests/recalc.rs`).
    - Stuck's `segment_reports`, `segment_history`, `segment_confirmed`, and a stuck move's own reports: rebuilt
      from the log's `report_outcome`, `decline_outcome` and `confirm_outcome` decisions (`Stuck::rebuild_reports`):
      each report's version, the declined ones, the one that stands, a permitted confirmation not yet recorded. Not
      safe to lose: the log takes one standing report per effect (`one-report-per-effect`), so a reopened Core
      without it could neither take a second report nor confirm the first, and the move would stay unknown. Test:
      `outcome_reported_before_core_reopens_is_confirmed_after_it` (`tests/stuck.rs`).
    - `pairing`: a pair built and not written, or one owed, rests on reports that went with the stopped process. A
      reopened Core parks each such pair unbuilt with why (`NoSentReport`), surfaced as any pair that can't be built
      is (`REQ-ORD-9`, `ADR-0009`), never skipped silently; pairs the log took are neither surfaced nor written again.
      Test: `pairs_a_restart_dropped_are_surfaced_unbuilt_after_core_reopens` (`tests/report_pairs.rs`).
    - `Mover.kept`: not rebuilt, safe. A move in motion when Core stopped is never carried out again (the effect is
      acted on, `Kept::Lost`): its outcome is unknown and is settled by its mover's report and another actor's
      confirmation (`AC-6`), whose state is now rebuilt; `stuck_parts_kept_decision_and_move_are_rebuilt_from_the_log`
      and `outcome_reported_before_core_reopens_is_confirmed_after_it` cover the path.
  - *Low: what `Scenario::reopen` carries.* The restore's transaction and its `balance_verification` questions were
    drawn from Core's fresh ids and named in no log entry. They are now derived
    (`Holdings::restore_transaction`): from the node, the order and the process the wallet's module runs in now, so a
    restore after the module restarted goes under a transaction no earlier process took, whichever Core process asks,
    and a retry in the same process is a repeat; the questions derive from it. Test:
    `restore_transaction_is_new_for_each_process_of_the_wallets_module` (`components/prov/tests/holding_wallet.rs`).
    What a reopen still carries, and why: the fleet (the platform and the providers, processes apart from Core's,
    which a Core restart doesn't stop); the clock (time goes on); the installed modules and the feed (the platform's);
    the host's `holdings` (the platform's host register of each account and its attribution, beside the platform;
    `T-024-21` reopens the platform and its host from their record); and `fresh`, Core's seeded id source, so a
    scenario replays: a real restart draws from new entropy, every id Core draws is either in the log (and
    `Issued::recorded` keeps a round's transactions off) or now derived, so none depends on it being carried. A
    platform reopened from its record must keep its process ids new (`T-024-21`), on which the derivation rests.
  - *Low: a held order.* Intake holds an order whose inputs statement can't be read back in `quote` and `accept` as
    well as `request_of` (`IntakeError::NotNow`). Test:
    `order_whose_inputs_cannot_be_read_back_is_neither_quoted_nor_accepted_after_core_reopens`
    (`tests/assembly_intake.rs`, the assembly's `intake_reopened_without` hook: a disk that lost a verified statement
    isn't opened at all, since the log refuses it, so the hook reads the statement as unreadable).
  - *Holdings sites touched* (for the merge with `T-021-16`): prov `holding/wallet.rs` (`restore_transaction`, new);
    core-orch `stuck/handing.rs` (`Wallets` loses `restore`, derives the transaction); the assembly's `holding.rs`
    (`HeldWallets` loses `restore`, `Landing` loses `fresh` and derives the transaction, `withdraw_for_a_hand_on`)
    and `mod.rs` (`move_stuck` draws no restore id).
- **Round 4's new test names for `PLAN.md`:** the ten above, with their tags in their `// verifies:` lines.
- **Round 4's rule-off table** (script `ruleoff5.py` in the row's scratch folder):

  | Rule switched off | Red |
  |---|---|
  | the shared predicate reads every value as resting (`settled_at_node`) | both hand-on probes, `reversed_segments_landing_is_not_restored_after_core_reopens`, `value_that_passes_through_the_wallets_node_is_not_held_for_the_order`, `value_that_passed_through_a_held_node_is_never_landed`, `release_after_a_holding_accounts_module_restarts_restores_first` |
  | a reversed segment's value read as resting | `reversed_segments_landing_is_not_restored_after_core_reopens` |
  | a part a resolution ended read as holding its value | both hand-on probes |
  | a done stuck move's part not marked resolved on reopening | `value_withdrawn_for_a_hand_on_is_not_landed_again_when_core_reopens_before_the_hand_on` |
  | a held order quoted | `order_whose_inputs_cannot_be_read_back_is_neither_quoted_nor_accepted_after_core_reopens` |
  | a held order accepted | none: a reopened Core shows no quote until the order is quoted again, which the quote's check refuses, so the acceptance is refused before the check; kept as the second line |
  | a restore transaction drawn alike for every process | `restore_transaction_is_new_for_each_process_of_the_wallets_module` |
  | a reliance entry names no input | `which_input_rests_on_which_judgement_is_rebuilt_from_the_log_after_a_restart` |
  | no reliance register rebuilt | the same |
  | no guard refusal rebuilt | `guard_refusal_recorded_before_core_reopens_is_not_recorded_again` |
  | no floor wait rebuilt | `floor_wait_recorded_before_core_reopens_is_not_recorded_again` |
  | no reports rebuilt | `outcome_reported_before_core_reopens_is_confirmed_after_it` |
  | no dropped pairs surfaced | `pairs_a_restart_dropped_are_surfaced_unbuilt_after_core_reopens` |

- **Round 4's local checks** (through the lock, `contracts/src/lib.rs` touched first, the row's private plain-wallet
  build): fmt; workspace clippy `-D warnings`; `cargo test -p` `rutent-core-orch`, `rutent-core-authz --lib` (after
  a `matches!` in `reports_of` for the entry-points scan), `rutent-contracts`, `rutent-prov`; `rutent-scenarios`'
  `reopened`, `hand_on`, `hand_on_record`, `segments`, `landing`, `private_nodes`, `charge_check`,
  `assembly_real_wallet`, `assembly`, `assembly_stuck`, `assembly_hand_on`, `assembly_intake`, `assembly_wiring`,
  `holds`, `stuck_hand_on`, `stuck`, `closing`, `invariants`, `audit_scenarios`, `stuck_mover`, `person_records`,
  `measures_called`, `reliance`, `recalc`, `report_pairs`; CI's seeded step (`RUTENT_SCENARIO_SEEDS=11,12,13,14,15`,
  `assembly` and `invariants`); the doc check; plan-check and tag-check `--strict` (0 findings, 2346 tests tagged);
  the contract guard against `origin/develop` (1952 paths, no violations). All pass.
- **Round 5** (#309's CHANGES at `45d13fbe`, comment 7159760767):
  - *Should-fix A: `Segments::resting_totals` bypassed the predicate.* It feeds what a hand-on's restore restores
    after a module restart (`Wallets::withdraw`, `restored_whole`), and counted retired segments, reversed ones and
    parts a resolution or a hand-on ended: the High's class through a third path. It now counts only the values
    `settled_at_node` reads as still resting. Tests: the reviewer's probe in
    `reversed_segments_landing_is_not_restored_after_core_reopens` (`tests/landing.rs`: after the reversal,
    `resting_totals` holds nothing for the order at the ledger, on the Core that ran on and on the reopened one), and
    `handed_on_part_is_not_counted_resting_beside_the_part_still_at_the_node` (`tests/hand_on.rs`, a new name for
    `PLAN.md` [`INV-1`, `INV-8`]: a split order, both parts' withdrawals settled at the ledger, the first part handed
    on; `resting_totals` counts the second part's value alone, before and after a reopening, so the second part's
    hand-on after a module restart restores only it). The support kit's `Core` runs a named part (`part`) for it.
  - *Low B: the rebuilt reliance entries.* `reliance_written_before_core_reopens_is_one_entry_per_provider_after_it`
    (`tests/reliance.rs`, a new name for `PLAN.md` [`REQ-CPL-12`, `INV-8`]): two providers relying on one judgement,
    the log turning away the withdrawal's effect so its reliance is written and nothing sent, Core reopened and the
    part run again: one entry per provider.
  - The held check in `Intake::accept` stays: a real second guard, which `T-020-51` reaches.
  - *Rule-offs* (script `ruleoff6.py`): `resting_totals` counting every settled value turns
    `handed_on_part_is_not_counted_resting_beside_the_part_still_at_the_node` and
    `reversed_segments_landing_is_not_restored_after_core_reopens` red; the reliance entries written not rebuilt turns
    `reliance_written_before_core_reopens_is_one_entry_per_provider_after_it` red (once the run after the reopening
    is later, so an entry built afresh is another entry).
  - *Local checks* (through the lock, `contracts/src/lib.rs` touched first, the private plain-wallet build): as round
    4's, all pass: fmt; workspace clippy `-D warnings`; core-orch, `core-authz --lib`, contracts, prov; the 25
    scenario binaries; CI's seeded `assembly` and `invariants` (seeds 11 to 15); the doc check; plan-check and
    tag-check `--strict` (0 findings, 2348 tests tagged); the contract guard against `origin/develop` (no violations).
- **Gaps, with owners.**
  1. The arrivals' state after a reopening (filings, waits): `T-020-51`. The notices' state is `T-023-03`'s and
     `T-023-04`'s (merged: the notices' summary is read on reopening, so `wait_ended` is sent once).
  2. A segment left unknown is asked again at its provider's cadence: `T-020-47` (`Modules::status`) and `T-020-48`
     (the cadence). A stuck move in motion when Core stopped (the mover's `Kept::Lost`) is settled by the existing
     report and confirmation (`AC-6`), which fails closed and needs no new row.
  3. `Platform::open` with a file record exists, but no assembly opens one; a restarted platform holds no release
     until each is admitted again (`T-013-10`'s carry-over): `T-024-21`, which reopens the platform from its record.
  4. `stated_value` holds nothing where neither the question's bytes nor `order_opened`'s amount in that asset can be
     read: two losses at once, noted, not built for.

## T-020-51: a deposit waiting for its customer outlives the filer's session, and its wait ends

Branch `f-020/t-020-51-deposit-wait`, from `1bc91b21` (the batch holding `T-020-46`'s rebuild and `T-020-50`'s deposit
destination). Finding 8 of the milestone review of 2026-11-24, issue #274; the owner's answer on the form,
`deposit-wait-end`: no re-offer by Core. Edits `components/core-orch/src/arrival/` (`mod.rs`, `filing.rs`, the new
`reopen.rs`), `core-internal` (a `DEC-20` path, folded into its unreleased 1.0.0: one setting), `core-authz`'s role tests
(no rule code: `Permission::Setting` takes its kind by itself), and the `F-024` files the commit bodies name: the
assembly's `arrivals.rs` and `reopen.rs`, the scenario `conformance/scenarios/tests/deposit_waits.rs`.

- **The wait's lifetime.** The first quote an order waits on its customer to accept is shown at a time (the `shown`'s);
  the wait lasts the operator's deposit-wait setting from it (`Run::waiting_since`), and no requote renews it. A quote
  shown for the wait stands no longer than the wait (`Offered::expires` is the earlier of the figures' validity and the
  wait's end, the whole second below), so the validity the customer is told (`awaiting.valid_until`) never outlasts it.
  Past the lifetime the wait ends (`end_wait`): the log's `wait_ended` (`why: past_its_lifetime`) and then the order's
  `order_refusal` (`no_route_right_now`), and the deposit is free (`spent` unset, so `Arrivals::settle` doesn't keep it
  and the log's `one-order-per-deposit` admits the next order on it). The refusal is what tells the customer, once
  (`T-023-04`: `Notices::notify` reads `order_refusal`, not `wait_ended`, and keys the notice by receiver and order, so a
  restart sends it no more); the new entry adds no notice of its own.
- **Who reads it.** `Arrivals::end_expired(desk)`, for Core's schedule (`T-020-48`: a tick calls it; the assembly's
  `end_expired_waits` stands in until then), ends every wait past its lifetime, answering the orders ended, and retries
  an end the log couldn't take; the next call that reads a wait ends it too: the filer filing the deposit again
  (`should_end` at `Step::Customer` is the lifetime alone), the same filing carried on (`go_on`), and the customer's
  acceptance (`customer_accepts` checks the lifetime before the digest). `Arrivals::wait_ends(order)` says when a wait
  ends; `awaiting` says `expired` once its quote can no longer stand.
- **The customer's session.** `Arrivals::accept(desk, order, by, session, quote)` takes the session the customer
  accepts in, one policy opened for the order's requester that is still open by Core's clock (the session register,
  `held_by`), else `ArrivalError::NoSession` and nothing recorded. The acceptance cites it
  (`acceptance-is-asked-in-the-users-session` holds it to an `open_session` in the log). The filing's session is no more
  the wait's: a wait whose filing session is over is held (`Run::should_end` at the customer step ignores the session and
  the filer's logins), and `T-020-41`'s `nothing_moves_while_the_filed_order_waits_…` now expects that, ending the wait at
  its lifetime instead.
- **The setting.** `OperatorSetting::DepositWait { seconds }` (`deposit_wait`, `SettingKind::DepositWait`), added to
  `core-internal`'s unreleased 1.0.0 (schema, README line, changelog entry, one valid and one invalid fixture, no version
  moved), so policy holds it under `Permission::Setting(DepositWait)` with no change to core-authz's rules: its role
  tests list the new fixture (and the counts moved by one). Core's decided path for setting it is `T-020-49`'s. Until
  then `Arrivals` holds it (`with_deposit_wait`, `set_deposit_wait`) and defaults it to one day
  (`DEFAULT_DEPOSIT_WAIT_SECONDS`): the snapshot's `PolicyBounds` (router-api) has no such field and no contract is
  edited for it. `T-020-49` calls `set_deposit_wait` from its decided path and rebuilds it from the log.
- **The rebuild** (`reopen.rs`, `Arrivals::reopened(history, kept)`): the arrivals (`Recorded`) and the notices taken
  (`Noted`, so a notice reported again is the same arrival), read back from the evidence store as `arrive` reads one; the
  deposits held (`spent`) by every order an `arrival` holds, unless the log shows its `wait_ended`; and the waits, each
  order the log shows `shown` awaiting its customer with no acceptance, start, refusal or end, at the customer step, its
  lifetime counted from its first `shown`, rebuilt by `Run::rebuilt`. The assembly's `rebuild_from_log` calls it, hands
  policy's sources the order's record again and keeps the filing for the order's start.
- **Defaults** (reversible, each failing closed):
  1. The wait lasts one day, until the operator's setting says otherwise (`T-020-49`).
  2. A wait is the customer's, not the filer's: it outlives the filing session and the filer's logins; only its lifetime
     ends it (`SessionOver`, a `WaitEnd` the contract names, is never written: no wait ends with its session now).
  3. The end writes `wait_ended` and then `order_refusal` (`no_route_right_now`); the refusal keeps the order's end and
     its notice where they were. Only the lifetime's end is a `wait_ended` (and frees the deposit): a requote that finds
     no route still ends the order as before, its deposit spent (`OrderRefused` is never written).
  4. The quote shown on a rebuilt wait is gone (the log holds its digest alone): the customer's acceptance of the digest
     the log last shows asks the route again, shown anew, as a quote past its validity does; any other digest is
     `NotTheQuoteShown`. The filing is read back from the log and the operator's kept statement of its inputs and
     objective (the lead's ruling, below): its request identifier, inputs and objective come from there, so the route is
     asked again with no restated filing. A wait whose statement can't be read, or isn't the filing's, is **held**:
     told it waits, never accepted, never asked the route again, ended at its lifetime. Its session is the customer's,
     the one it accepts in.
  5. The session a rebuilt wait asks its route in is the customer's, the one it accepts in.
- **Gap 1, closed** (the lead's ruling, 2026-11-25: fix now, pre-production). The `arrival` entry that holds a deposit
  against a filed order gains `request` and `inputs`, the salted commitment of the operator's signed statement of the
  filing's inputs and objective (`FilingInputs`, shape `filing-inputs`, a sibling of `order-inputs`: a filing's inputs
  may be empty and it states an objective), kept in the evidence store and appended as a stated entry about nothing
  before the arrival (no account in the log, `REQ-AUD-6`). Row `filed-orders-inputs-are-a-kept-statement`, in the style
  of `opened-orders-inputs-are-a-kept-statement`: verified, of that shape, the deployment operator's own, about
  nothing. `core-internal`, folded into the unreleased 1.0.0 (a `DEC-20` path): schema, README, changelog, fixtures
  (valid `filing-inputs-of-a-filed-order` and an arrival naming them, invalid `filing-inputs-without-its-objective`, six
  admission cases). `Arrivals::reopened` reads the statement back, held to its shape and to the order. The
  statement also holds the filing's judgements (`needs`, `FilingNeed`), so the whole filing but the session is fixed by
  the log, the statement and the authorisation. A filer restating the filing after a reopening (same request, party and
  deposit) is told the wait as it stands and **changes nothing**: there is no `adopt` (review of `9e2214ee`, blocker 1:
  the restated destination, payer account and objective had started the order). Tests:
  `waiting_deposit_is_accepted_after_core_reopens_with_no_new_filing` [`REQ-QTE-10`, `REQ-AUD-6`],
  `reopened_waiting_deposit_whose_statement_cannot_be_read_stays_held` [`REQ-QTE-10`, `REQ-AUD-6`] (the store keeps
  nothing behind the commitment, the arrival names none, the statement is another order's), and the scenario, which no
  longer files again after the reopening. Rule-offs, each alone from the commit of that change:

  | Rule switched off | Red |
  |---|---|
  | the arrival names no statement | `waiting_deposit_is_accepted_after_core_reopens_with_no_new_filing` |
  | the arrival names no request | the same |
  | the statement's order isn't checked on reading | `reopened_waiting_deposit_whose_statement_cannot_be_read_stays_held` |
  | a wait whose filing wasn't read back is accepted | the same |
  | the statement isn't appended before its arrival | 35 arrival tests (the log refuses the arrival) |
  | the log row cites nothing (`--missing`) | `every_relation_of_the_citation_table_holds_and_breaks_as_its_cases_say` |
  | the log row takes a statement about something (`--about`) | the same |
  | a restated filing replaces the wait's filing (the old `adopt`) | `filing_restated_after_a_reopen_changes_nothing_the_statement_fixed` |

- **Gaps, with owners.**
  1. What a reopening doesn't rebuild, by case (the review of `9e2214ee` probed each): an order started or ended after
     the deposit was held keeps the deposit held (`one-order-per-deposit`, rebuilt as `spent`) and a repeat of its filing
     is told `NotFileable` or that the deposit is held, not how it ended; a filing refused before an order opened leaves
     no arrival naming an order, so the deposit is free after a reopening and the same filing is evaluated afresh (it may
     be refused again, a second `order_refusal`), and another filing may start; a deposit its reserve said no to is
     filed on again after a reopening, the reserve is asked once more and its second order ends before the deposit is
     `NotFileable`, which fails open by one question, harmless for funds. All three are `T-024-21`'s (restarted runs).
  2. The setting's decided path and its rebuild from the log: `T-020-49`. Its schedule's call to `end_expired`: `T-020-48`.
- **Named tests** (`components/core-orch/tests/arrival.rs`): `customer_accepts_a_waiting_deposit_in_a_later_session_of_its_own`
  [`REQ-QTE-10`, `AC-13`], `wait_past_its_lifetime_ends_and_frees_its_deposit` [`GUA-1`],
  `freed_deposit_is_filed_again_and_routed_once` [`REQ-ORD-15`],
  `filed_order_waiting_for_its_customer_still_waits_after_core_reopens` [`REQ-QTE-10`],
  `acceptance_on_a_waiting_quote_past_its_validity_asks_again` [`REQ-QTE-15`]; `contracts/tests/core-internal.rs`:
  `deposit_wait_setting_is_an_operator_setting_of_its_own` [`REQ-AUZ-6`]. Scenarios (`deposit_waits.rs`):
  `a_waiting_deposit_is_accepted_in_a_later_session_after_core_reopens`,
  `a_waiting_deposit_past_its_lifetime_is_ended_and_nothing_moved`.
- **Red first, honestly.** The named tests were written beside the code, not before it: the first run of the arrival
  tests failed on assertions (a wait ended by the old session rule, the clock's two-second drift against the shown
  quote's time, an acceptance the log refused for a session it never opened) and the code and tests were fixed from
  there. Each rule's red is shown by its rule-off below.
- **Rule-offs** (each rule switched off alone from the committed checkpoint `387f5bfd`, the file restored with
  `git checkout`; script `ruleoff.py` in the row's scratch folder). "Arrival" is `core-orch`'s `arrival` test file,
  "scenario" `deposit_waits.rs`; the tests are named by their first words: `customer_accepts…`,
  `wait_past_its_lifetime…`, `freed_deposit…`, `filed_order_waiting…` (reopens), `acceptance_on_a_waiting_quote…`,
  `nothing_moves…` (`T-020-41`'s, changed).

  | Rule switched off | Red |
  |---|---|
  | the wait ends again with the session it was filed in (`go_on`) | `customer_accepts…`, `filed_order_waiting…`, `nothing_moves…` |
  | the customer's session isn't checked (`NoSession`) | `customer_accepts…` |
  | a wait never passes its lifetime | `wait_past_its_lifetime…`, `freed_deposit…`, `filed_order_waiting…`, `nothing_moves…` |
  | the end writes no `wait_ended` | `wait_past_its_lifetime…`, `freed_deposit…` (the log refuses the second order), `filed_order_waiting…` |
  | the end leaves the deposit spent | `freed_deposit…` |
  | a quote may outlast its wait | `wait_past_its_lifetime…`, `filed_order_waiting…` |
  | a requote renews the wait | `acceptance_on_a_waiting_quote…`, `filed_order_waiting…` |
  | `end_expired` ends nothing | `wait_past_its_lifetime…`, `freed_deposit…`, `filed_order_waiting…` |
  | the waits aren't rebuilt from the log | `filed_order_waiting…` (and the scenario's reopening) |
  | a reopened wait takes any digest | `filed_order_waiting…` |
  | a deposit freed by a wait's end is held again after reopening | `filed_order_waiting…` |
  | a reopened wait that routes nowhere ends the order | the scenario `a_waiting_deposit_is_accepted_in_a_later_session_after_core_reopens` |
  | `SettingKind` of `DepositWait` read as another kind | `deposit_wait_setting_is_an_operator_setting_of_its_own`; core-authz's `another_partys_login_listed_in_the_operator_role_is_refused_every_setting_and_change` |

## T-020-53: a stuck decision's evidence fails closed (and finding 4's evidence-store reads)

The Low of `M3`'s milestone review of 2026-11-24. It supersedes `R-DIGEST` (`F-025`) and the form item
`stuck-decision-digest-scope`.

- **The rule.** `digests_from_a_party` (`stuck/mod.rs`) holds every digest of a stuck decision to the log: each must
  name a stated entry the history holds (else `ResolveError::NoEvidence`), and that statement must be a verified
  statement by a party the order involves (else `OutsideTheOrder`, as before). Nothing is asked of policy before it
  (the check sits before `decide_stuck`). The evidence is never empty (`StuckDecide::check`), and an empty slice is
  refused here too, so a decision rests on at least one statement of a party, and on no digest the log doesn't hold.
  The rule holds whatever a production assembly answers for policy's `Source::Kept`. Who counts as a party stays in
  one place, `party::verified_party` (`T-020-40`'s parties, as on this base): `T-021-16`'s narrowing of who counts as a
  party reaches this check through it, with no edit here.
- **Default (named).** A digest the log holds no statement under is `NoEvidence`, not `OutsideTheOrder`: it names no
  maker to be outside of. Both refuse before policy is asked; only the error differs. Reversible.
- **The scenarios.** No scenario decides a stuck order on a placeholder digest now: `stuck.rs` (46 tests),
  `stuck_mover.rs`, `stuck_hand_on.rs` and `hand_on.rs` decide on a statement the kit logs where the decision is asked
  (`segments_support::own_evidence`: the order's own provider's result, signed with its statements key, appended after
  the operator's question it answers). `assembly_stuck`, `assembly_hand_on`, `assembly_real_wallet` and `reopened`
  already named statements the log holds. `hand_on.rs`'s forged `stuck_decide` entry (a placeholder digest inside an
  entry the log is shown to refuse) decides nothing and stays.
- **The kit's history.** `Core::recorded` reads every statement verified, as the real log's would; `TestLog::
  claim_only_from_now` leaves the statements appended after it as claimed only, which the second test below uses (the
  `attributed` range moves only the receipts).
- **The only guard.** The log admits a `stuck_decide` whose digests name no logged statement, so orchestration's
  `digests_from_a_party` is the one check that each names a logged, verified statement by a party to the order. A path
  that decided a stuck part without core-orch would pass it.
- **Tests.** `stuck decision whose digest names no logged statement is refused` [`REQ-AUZ-5`]
  (`stuck_decision_whose_digest_names_no_logged_statement_is_refused`, in `stuck.rs`): a placeholder alone, or beside or
  before a party's own statement, is refused with policy asked nothing and nothing recorded; the party's own is taken.
  `stuck decision needs one digest naming a verified statement by a party to the order` [`REQ-AUZ-4`]
  (`stuck_decision_needs_one_digest_naming_a_verified_statement_by_a_party_to_the_order`): no digest, an outside
  provider's, a claimed-only statement (one whose key was not current at its making, `THR-3`), and a claimed-only beside a party's, each refused; one verified statement of a
  party is enough. A unit test, `decision_evidence_that_names_no_logged_statement_fails_closed` (`stuck/party.rs`,
  [`REQ-AUZ-5`]), holds the function itself, the empty slice included.
- **Rule-offs.**

  | Rule switched off | Red |
  |---|---|
  | A digest naming no logged statement passes (`ok_or(NoEvidence)` dropped for a skip) | `decision_evidence_that_names_no_logged_statement_fails_closed`; `stuck_decision_whose_digest_names_no_logged_statement_is_refused` |
  | The verified-party check dropped | `decision_evidence_digests_are_held_to_a_verified_party_whatever_their_kind`; `stuck_decision_needs_one_digest_naming_a_verified_statement_by_a_party_to_the_order` |
  | Only the first entry naming a digest held to the party check (`break`) | `a_statement_entered_twice_with_another_attribution_is_refused_in_either_order` (`stuck/party.rs`, [`REQ-AUZ-4`]): the result no longer depends on log order |
  | The empty-evidence refusal dropped | `decision_evidence_that_names_no_logged_statement_fails_closed` only: the scenario stays green, since `StuckDecide::check` refuses the empty evidence first in `decide` (`NoEvidence`); the guard holds the function itself |

- **Finding 4: the evidence store's reads, in `F-022`'s notes.** Every read Core makes of the evidence store is listed
  there with its caller in Core; the one read with no caller (`Evidence::fetch`) is named out of `v0` with its reason.


## T-020-55: Core's own answers to policy's questions, and a regulator's read of the trail

Finding 4 of `M3`'s milestone review of 2026-11-24, as the milestone reviewer read `T-021-16` and `T-021-17` (issue
`#308`). Branch `f-020/t-020-55-core-sources`, from m3's `adb73186`. Three parts, each in commits of its own. `DEC-20`
paths: `components/core-authz/` (a new module, `core_sources`, and `stuck/`), `contracts/core-internal` (folded into
the unreleased 1.0.0) and `components/core-orch/` (`stuck/party.rs`), each waiting for the owner's click at the release
into `main`; `components/core-audit/` (`measures/trail.rs`), Core's crate. `F-024` files: the assembly and its tests.

### (1) Core answers its own sources; the assembly supplies only the deployment's registers

- **The shape** (`core-authz/src/core_sources/`). `CoreSources<L, E, R>` implements policy's `Sources`. It answers
  `Requester`, `OrderOrigin`, `OrderParties`, `SegmentOrder`, `SegmentProvider`, `Statement`, `Showing`,
  `Declarations`, `Kept` and `KeptAbout`, and `order_record`, itself, over Core's log (`LogEntries`, read without a copy)
  and evidence store (`KeptBytes`, a statement's bytes borne out against its commitment). The deployment implements
  `DeploymentRegisters` for what is its own: `ActsFor`, `AgentScope`, `Permissions`, `Account`, `Holder`, `Reaches`,
  `MeetsBelowOperator`, `OfOperator`, `Verifier`, `Provider`, `Consent` and `HoldingAccount`. Core asks it only through
  `Registered`, which only `CoreSources` builds, and only for a source `is_the_deployments` names; that match names
  every source, so a new one is placed by whoever adds it. No answer is a permit: each is an input the fixed rules and
  the deployment's policy rule over.
- **Readings, each the assembly's `Records` before this row.**
  - An order's record: the one a filing handed over before the log holds the order (`OpenedOrders`, which core-orch's
    `FiledOrders` port forwards to, `withdrawn` dropping it), else the log's `order_opened`, else its acceptance (a
    linked order, which intake never opened). `OrderParties`: the order's acceptance, else that record.
  - `SegmentOrder`, `SegmentProvider`, `Statement`, `Showing`, `Kept`, `KeptAbout`: the log, read as `Records` read it.
  - `Declarations`: since the review of `8f7ddefd` (below), the advertisement the log's key register accepted last for
    the reserve, where the log holds its stated entry. At `8f7ddefd` it read the latest stated advertisement whose
    claimed key its maker's grant named, which failed open.
- **The differential, before anything moved** (`1657cc7c`). The assembly's engine went on answering from `Records`,
  and `Compared` asked `CoreSources` beside it on every input, recording each disagreement
  (`RUTENT_SOURCES_DIFF`, `RUTENT_SOURCES_COUNT`). `tests/sources_compared.rs` ran the seeded steps (seeds 11 to 15,
  restarts among them) and asserted none. Over the whole `rutent-scenarios` crate it first found two classes, both
  where Core's answer was a refusal's (missing) and `Records`' a yes, so failing closed, and both fixed before the
  move:
  1. `Declarations` (15 answers, the automatic-deposit filings): a reserve's first advertisement is held stated, not
     verified, since the log records the maker's publisher only from it; requiring verified refused it. Now trusted
     as the log first trusts it (above).
  2. A waiting filed order after a reopen (`Requester`, `order_record`, `deposit_waits`): the log holds no
     `order_opened` or acceptance of it, and `OpenedOrders` starts empty. The assembly's reopen hands each record
     arrivals rebuilt from the log (`Arrivals::record`) to `OpenedOrders` again, as `Records` was refilled.
  Then: 772 answers compared across the crate (`Requester` 417, `OrderParties` 284, `Declarations` 15, `Kept` 15,
  `KeptAbout` 15, `order_record` 12, `OrderOrigin` 10, `Showing` 3, `SegmentProvider` 1), none disagreeing, every test
  green. No scenario asks `SegmentOrder` or `Statement`; the first named test asks both over a run order.
- **The move** (`303517f2`). The engine asks `CoreSources`; `Records`, its order and declaration maps and every copy
  into them (at intake, on a linked order's start, on a reopen, at advertising and relisting) are gone, and the
  differential with them. The assembly's `Deployed` holds the registers and the verifier stand-in;
  `Core::sources()` hands out `CoreSources`, `Core::verifier_is` sets the stand-in.
- **`HoldingAccount` from the deployment's register.** `Registers::holding_accounts` (node to provider), written where
  the operator installs a node's wallet (`install_wallet`) and carried across a reopen in `Carried` as the
  deployment's own configuration; the host's `Holdings`, which Core carries beside the platform, is never read for
  it.
- **Tests.** `policy's log backed answers are core's and the assembly supplies only the deployment's registers`
  [`REQ-AUZ-5`] (`assembly_policy.rs`): an order opened, accepted and run; Core answers its requester, origin, parties,
  a segment's order and provider, a statement's maker, its keeping about the order, the origin reserve's two
  declarations and the order's record; with the deployment's registers emptied and its verifier saying yes to
  anything, every one of those answers stands and the register's own (`ActsFor`) goes. `holding account answer reads
  the deployment's register not holdings carried across a reopen` [`REQ-AUZ-4`, `NFR-6`] (`assembly_real_wallet.rs`):
  the wallet named before and after a reopen; the register naming another holder, or none, moves the answer though the
  carried holdings still name the wallet, and the hand-on through the wallet is then not permitted. core-authz's unit
  tests (`core_sources/tests.rs`): the deployment is asked only its own sources, and a deployment saying yes to all of
  them moves none of Core's; an unreadable log, an unreadable store or a held register gives not known.
- **Red first.** The differential was red before its fixes (17 disagreements, above). The two named tests were written
  after `CoreSources` existed, against the API the move gives, so they first ran green; each is held by its rule-offs
  below (R1, R6, R7).

### (2) A value resting at its route's first node; `holder_of_the_order` once

- **The disagreement.** `resting_node` names the start of a part's first unsettled segment; the row
  `stuck-decide-held-at-is-a-held-node` asked for a segment ending at that node. A route handed on starts at an inner
  node with no withdrawal (`check_start`), so a stuck part of a linked order whose value rests at its route's first
  node, where no segment of the order's routes ends, was refused an honest permit at append. A route chosen otherwise
  starts at an outer node, where `resting_node` names nothing, so only a route handed on meets it.
- **The fix** (`2212ee40`). The row takes a segment starting or ending at `held_at`; its `at` field still holds it to
  the one node the value rests at. README row and CHANGELOG line in core-internal's unreleased 1.0.0; admitted case
  `--a-wallet-held-at-its-routes-first-node` (`world--sdh`'s history with `ord-1` handed on from `ord-0`, each part
  starting at an inner node, `p3` a single deposit from `ledger-start`; the hand-on by `start-wallet`).
- **`holder_of_the_order`, written once**: public in core-internal's `standing.rs`. The row, the narrowed party rule,
  policy's R-MOVER check (core-authz's `involves_holder` is gone) and core-orch's party port call it. Policy's copy
  required `Outcome::Observed`, the others didn't; the one definition keeps the guard, which changes nothing on a
  checked history, since `What::allows` holds every kind it reads (acceptance, input, arrival, holding withdrawal) to
  `observed` (R10 below: no test goes red, as expected of a second guard).
- **Test.** `value resting at the route's first node is held at it by the log` [`REQ-AUZ-4`] (core-authz
  `stuck/tests.rs`, through the admitting log): `resting_node` names `ledger-start`; the register's wallet there moves
  the hand-on, the entry records `held_at: ledger-start` and the log admits it; the wallet of another node is refused.
  Its first run failed on the test's own world (a chosen route must start with a withdrawal, a test artefact: the world
  is now a route handed on); the row's fix was already in, so its red is R9's.

### (3) A regulator's read of the trail, through Core

- **The entry point** (`29ccf3bc`). core-audit's `measures::hand_trail(policy, &TrailRequest {request, reader,
  permission, at}, history)` builds the read check of the trail, asks policy's read check through `read_trail`, and
  on a read policy allowed and the log shows recorded hands over `read_trail`'s trail: the entries and their receipts
  up to that record, each statement by commitment alone. Every other reader gets `Handed::Withheld`, whatever policy's
  reason, and whether the read was refused, not known, or unrecorded: no class, no receipt, nothing of the log. The
  assembly's `Core::read_trail(reader, permission)` calls it at Core's clock under a fresh request id; the transport in
  front of it is the deployment's (`F-030`).
- **Tests** (`assembly_policy.rs`). `regulator holding the read permission is handed the trail and the read is
  recorded` [`REQ-AUD-12`]: granted by the operator's login, the regulator is handed the log whole, ending in its read's
  record (permitted, its reader, permission and `Trail`), the read adding that one entry, and no `bytes` or `salt` in it.
  `caller without the read permission is handed nothing of the trail` [`REQ-AUD-12`, `REQ-AUZ-7`]: a regulator naming no
  permission or one not granted, another regulator naming the grant, the customer's login, a refusing deployment, and
  the regulator after revocation: each `Withheld`, each read recorded refused. Both passed at their first run, the entry
  point written with them; R11 is their red.
- `F-022`'s note on `REQ-AUD-12` now names the entry point.

### Defaults, reversible, each failing closed

1. An order's record is the handed-over filing's, else the log's `order_opened`, else its acceptance; its parties are
   its acceptance's first. Where they could differ, the 772 compared answers never did.
2. A reserve's declarations come only from the advertisement the log's key register accepted last for it, whose
   stated entry the log holds; none accepted, or one offering no reserve, declares nothing (the review of `8f7ddefd`).
3. A log, a store or the order register that can't be read now gives not known; the deployment's registers held
   mutably elsewhere give not known too.
4. `Holder`, `Provider` and `Consent` are the deployment's (its delegations, its providers' answers, a person's
   consents), answered nothing in the assembly as before.
5. The deployment's register of holding accounts goes on across a reopen; its other registers are still set again on
   each Core put together (`T-020-46`'s default).
6. Every reader not handed the trail is told `Withheld` alone, a not-known or unrecorded read as a refused one.
7. The differential lives in history (`1657cc7c`), not in the tree: it needed the old answers, which the move removes.

### Gaps, with owners

1. ~~The first-trust arm of `Declarations` checks no signature.~~ It failed open (the review of `8f7ddefd`); closed by
   reading the register's acceptance (below). A durable log record of an acceptance stays unbuilt: the register is
   rebuilt from the log's own records on every opening, so none is needed for v0.
2. A production assembly implements `DeploymentRegisters`, `LogEntries` and `KeptBytes`, forwards `FiledOrders` to
   `OpenedOrders`, and hands each waiting filing's rebuilt record over again on a reopen, as the scenario assembly does.
   Owner: `F-030`.
3. The trail is handed whole, no window or page; a regulator's transport, and naming the regulator, are the
   deployment's. Owners: `F-030`, `F-033`.

### Rule-off

Each rule switched off alone from `303517f2` by `scratchpad/t-020-55/ruleoff.py`, its tests run through the lock, the
file restored from its text and touched (with `contracts/src/lib.rs`).

| Rule switched off | Tests that went red |
|---|---|
| R1: a Core source (`Requester`) asked of the deployment | `the_deployment_is_asked_only_its_own_registers`, `an_unreadable_log_or_store_gives_not_known` (core-authz); `policys_log_backed_answers_are_cores_and_the_assembly_supplies_only_the_deployments_registers`, `authorisation_given_in_a_role_the_fixed_rule_refuses_is_refused_beside_the_engine`, `question_about_a_person_on_the_orders_shortlist_reaches_the_engine_and_its_log` (`assembly_policy`) |
| R2: an unreadable log or store answers nothing, not not-known | `an_unreadable_log_or_store_gives_not_known` |
| R3: an advertisement read only where verified (no first trust) | `a_filing_on_an_automatic_deposit_starts_under_the_real_policy` (`assembly_arrivals`), `policys_log_backed_answers_are_cores_and_the_assembly_supplies_only_the_deployments_registers` |
| R4: any advertisement read, trusted or not | none: no test holds an advertisement the log doesn't trust (gap 1) |
| R5: a filing's handed-over record not read | `an_unreadable_log_or_store_gives_not_known`; `a_filing_on_an_automatic_deposit_starts_under_the_real_policy`; `deposit_waits`' three (`a_deposit_with_no_firm_rate_waits_and_runs_on_its_customers_acceptance`, `a_waiting_deposit_is_accepted_in_a_later_session_after_core_reopens`, `a_waiting_deposit_past_its_lifetime_is_ended_and_nothing_moved`) |
| R6: the register of holding accounts not read (nothing for every node) | `holding_account_answer_reads_the_deployments_register_not_holdings_carried_across_a_reopen` and seven more of `assembly_real_wallet` (`stuck_order_of_the_assembled_core_is_handed_on_through_the_real_plain_wallet` among them) |
| R7: the register of holding accounts not kept across a reopen | `holding_account_answer_reads_the_deployments_register_not_holdings_carried_across_a_reopen`, `holding_account_whose_module_restarted_is_re_landed_from_the_log_before_a_release`, `holdings_and_accepted_routes_of_a_reopened_order_are_rebuilt_from_the_log` |
| R8: a waiting filing's record not handed to Core's sources on a reopen | `a_waiting_deposit_is_accepted_in_a_later_session_after_core_reopens` |
| R9: the row takes only a segment ending at `held_at` | `every_relation_of_the_citation_table_holds_and_breaks_as_its_cases_say` (contracts), `value_resting_at_the_routes_first_node_is_held_at_it_by_the_log` (core-authz) |
| R10: `holder_of_the_order` counts entries of any outcome | none: a second guard, every kind it reads is `observed` by `What::allows` |
| R11: a refused trail read handed something | `caller_without_the_read_permission_is_handed_nothing_of_the_trail` |

### Local checks

Through the lock, `contracts/src/lib.rs` touched first, at the notes' commit: `cargo fmt --all -- --check`; `cargo clippy
--workspace --all-targets -- -D warnings`; `cargo test` of `rutent-contracts` (225 pass), `rutent-core-authz` (160),
`rutent-core-audit` (146), `rutent-core-orch` (330) and `rutent-scenarios` (471, every binary, the policy-touching
ones among them: `assembly_policy`, `assembly_wiring`, `stuck`, `stuck_mover`, `assembly_real_wallet`, `reopened`,
`audit_scenarios`, `bulk_measures`); the doc check (`RUSTDOCFLAGS="-D warnings"`); `plan-check --strict` (0 findings)
and `tag-check --strict` (2384 tests, 0 findings). No provider's answer changed, so the role suite wasn't run apart.
The full suite runs in CI.

### T-020-55, the review of `8f7ddefd` (PR `#313`, comment 7173496820)

- **The blocker: `Declarations` failed open.** Every advertisement enters the log as a stated entry attributed
  unverified, and the log's key register (`core-audit/src/log/keys.rs`, `Register::join`) then accepts it or drops it.
  Core trusted any such entry whose claimed key its maker's grant named, skipping every rule the register applies
  (the signature, `validate_statement`'s maker = provider, deployment and version, newer than the last accepted, the
  skew, `validate_successor`), and took the latest in log order. The reviewer's probes: a forged copy of
  `sim-reserve-a`'s advertisement (garbage signature, declarations flipped, made in 2030) turned the operator's
  authorisation for the user from refused to permitted; `sim-reserve-b`'s advertisement naming `sim-reserve-a`
  flipped `sim-reserve-a`'s declarations. The 772-answer differential couldn't see it: every scenario's advertisements
  are honest, as R4's green said.
- **The fix, no contract change.** The register keeps each accepted advertisement's commitment
  (`Accepted::commitment`), and `AuditLog::accepted_advertisement(provider)` exposes the one it accepted last: that
  commitment and the reserve declaration it makes. core-authz's `AcceptedAdvertisements` (in place of `KeptBytes`)
  reads it, and `CoreSources` takes the declaration only where the log holds a stated advertisement under that
  commitment. None accepted, or one offering no reserve: missing, so the decision resting on it is refused; a log that
  can't be read: not known. No fallback to the grant-key reading, and no read of the evidence store. The assembly's
  `LogRead` answers both of Core's reads; `StoreRead` is gone, and `serde_json` is a dev-dependency of core-authz again.
- **Tests** (`assembly_policy.rs`, each written before the fix and seen red at `8f7ddefd`).
  `forged advertisement under a granted key changes no declaration` [`REQ-AUZ-9`, `REQ-CAT-6`] (red: `Yes, Yes` for
  `No, No`) and `another provider's advertisement naming the reserve changes no declaration` [`REQ-AUZ-9`,
  `REQ-CAT-6`] (the other reserve's signature copied, then signed anew validly with its own advertising key; red the
  same), each also holding the operator's authorisation for the user refused. `old advertisement replayed after a newer
  accepted one changes nothing` [`REQ-AUZ-9`, `REQ-CAT-6`]: the reserve advertises anew, validly, its declarations
  flipped, and Core reads them; its older advertisement replayed changes nothing. Red at `8f7ddefd` on its first
  assertion (the old reading didn't read the newer advertisement), not the replay's, which R13 and R14 hold.
  `Core::append_advertisement` is their hook: a stated advertisement appended as `Core::advertise` appends one, with
  no catalogue join.
- **The two sources no scenario asked, directly.** `segment order answer is the log's plan for every segment`
  [`REQ-AUZ-7`] and `statement answer is the log's maker for every statement` [`REQ-AUD-5`]: over a run order, every
  segment of every `route_chosen` is its order's and no other's, and every commitment its first stated entry's maker's
  party's and no other's; an unknown segment or commitment is nothing. Both passed at once (the code is `Records'`
  moved), as a check of the move rather than a rule's red.
- **Re-checked the same way.** `Kept` and `KeptAbout` ask only whether the log names a statement under a commitment
  about the order, which is what the store keeps; `Showing` requires a verified result; `Statement` gives the party the
  log attributed. None trusts a claim the log didn't make.
- **Rule-off** (`scratchpad/t-020-55/ruleoff2.py`, from `207f3a47`, each alone, restored and touched):

| Rule switched off | Tests that went red |
|---|---|
| R4: the register takes an advertisement with a bad signature or made past the skew (the probe's two defects) | `forged_advertisement_under_a_granted_key_changes_no_declaration`; core-audit's `a_dropped_or_crossed_key_never_signs_again`, `an_advertisement_orchestration_signed_names_no_key_of_the_provider` |
| R4, as `8f7ddefd` read it (any stated advertisement under a granted key) | the three advertisement tests, at `8f7ddefd` before the fix |
| R12: the register takes an advertisement another maker made (`validate_statement`, `validate_successor` off) | core-audit's four key-register tests; no scenario test, since the register's publisher check still refuses `sim-reserve-b`'s advertisement as `sim-reserve-a`'s |
| R13: the register takes an advertisement no newer than the one it accepted | `old_advertisement_replayed_after_a_newer_accepted_one_changes_nothing`; core-audit's three |
| R14: Core reads the register's first accepted advertisement, not its last | `old_advertisement_replayed_after_a_newer_accepted_one_changes_nothing` |
| R15: the accepted advertisement read though the log holds no stated entry under it | `an_unreadable_log_or_store_gives_not_known` (core-authz) |
| R16: a reserve with no accepted advertisement reads as declaring | `the_deployment_is_asked_only_its_own_registers` (core-authz, its assertion added after the first run of this rule-off, which turned nothing red) |
| RP (the reviewer's): a permitted trail read handed `Withheld` | `regulator_holding_the_read_permission_is_handed_the_trail_and_the_read_is_recorded` |
- **Local checks** (through the lock, `contracts/src/lib.rs` touched first): fmt; workspace clippy `-D warnings`;
  `rutent-core-authz` (160 pass) and `rutent-core-audit` (146); the policy scenario binaries (`assembly_policy`,
  `assembly_wiring`, `stuck`, `stuck_mover`, `assembly_real_wallet`, `reopened`, `audit_scenarios`, `bulk_measures`,
  `assembly_arrivals`, `deposit_waits`, `authz_scenarios`, `execution`, `deposit_destination`, `execution_routes`: 160
  pass); the doc check; `plan-check --strict` (0 findings), `tag-check --strict` (2389 tests, 0 findings); the
  contract guard against `origin/develop` (no violations). No contract or core-orch file changed in this round.
