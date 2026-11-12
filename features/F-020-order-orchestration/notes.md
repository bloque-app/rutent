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
