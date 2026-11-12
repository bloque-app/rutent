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
- **One yes for a split order, a contract gap.** `Offer::Offered` names a yes for each part, and core-internal's
  acceptance names one, `reserve_yes`. The acceptance cites the first part's; each part withdraws at the origin
  reserve, and the rounds offer no route without each yes, but the log ties only the first to the acceptance. A gap
  for a new `F-020` row with a core-internal bump: `reserve_yes` as a list, one per part.
- **The session comes with the request.** The user's session is opened through policy (`Sessions::open_session`)
  by whoever takes the user's login before it reaches intake, and handed to the catalogue by the assembly, as
  `F-021`'s notes have the register's hand-off. Intake doesn't open it; the log holds the acceptance to a permitted
  `open_session` of the requester's that hasn't ended.
- **The destination** (`REQ-ORD-24`). No provider-role operation resolves a destination or names its owner, so a port,
  `intake::Destinations`, answers it: an owner, not yet, or no. The assembly backs it from the destination reserve's
  register; Core records each answer as a `destination_check`. It is resolved when the order opens, and that owner is
  kept; at acceptance it is resolved again. Not yet waits (`destination_not_yet_resolved`, accept again later; once it
  resolves, to the owner first resolved, it is accepted), no ends the order (`destination_refused`), and another owner
  makes the order wait for the customer (`destination_check` `owner_changed`, no refusal, since none of the seven
  classes is a wait for the customer): the requester's next acceptance names that owner and goes through policy.
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
- **Should-fix 6, a split order's yes.** A `// T-020-01e:` note where the acceptance takes the first part's yes;
  each part's is cited once core-internal's acceptance names them.
- **Nit.** The carry-on path doesn't resolve the destination again: policy permitted on the resolution made then.
- **Rule-off, each red:** a carry-on by any actor of the party; a stated party trusted, without `ActsFor` (on the
  carry-on and on the confirmation); an owner change released without naming the owner; a refusal of the caller
  ending the order; a retry rebuilding its entries; a refused append waited on; any session taken; one register for
  every party; told to anyone stating the party; and `open` appending before it keeps the order. The earlier
  rule-offs were run again on the new code, each red.

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
