# F-020 notes

Readings taken where `SPEC.md` or `PLAN.md` is silent or ambiguous, recorded per Narthex `REQ-SKL-24`.

## T-020-02

- **One bump, folded into 3.0.0.** router-api 3.0.0 isn't on `develop` yet (`M2`'s pull request is open), so the
  change is a third bullet of 3.0.0's changelog entry, not 4.0.0. The contract guard against `origin/develop`
  reads 3.0.0 against 2.x, a major, and passes.
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

- **Core's clock.** `snapshot::Clock` is Core's clock; `SystemClock` reads the host's and rounds a part of a second
  up, so an expiry is never read late. `Snapshots::form` takes no time from its caller: it reads the clock once and
  uses that reading for the catalogue's snapshot, every statement's key check and `taken_at` (`F-012`'s notes).
  The clock lives in this module because this row carries the inherited item; other rows reach it as
  `snapshot::Clock`.
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
  still counts against its publisher. Its publisher is the catalogue's; a provider whose publisher the catalogue
  doesn't know fails the formation (`ExposureWithoutPublisher`) rather than drop its exposure. A stuck order's
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
  own maker, its bytes taken with `provider_role::member`. A nested envelope inside a list can't be reached by
  member names, so it refuses the whole statement. A maker the operator publishes, and `bind_operator_statement`
  (the recorded decision among the operator's own), go to `verify_operator_statement`, the deployment's
  `operator_key`. No figure kind the contract carries today nests a statement; the walk is there for the README's
  "nested ones included", and is tested through `bind_statement`.
- **What else keeps a statement out.** A quote's or dry run's maker that isn't its tuple's provider; a tuple
  not in the snapshot; a verifier whose grant isn't in force for judgements at Core's clock, or whose origin is
  unknown (its accepted advertisement and its release accepted last give it, since a verifier has no tuple figure);
  a statement dated after `taken_at` though within the clock skew, whose age the contract can't state; and a
  figure the contract or the bounds' check against the snapshot refuses. Each statement is added on trial, so one
  bad statement is refused alone rather than failing the formation.
- **Refusals recorded.** Core-internal's table gives this row "a statement refused before the record, a route
  refused → stated entry `unverified`; `order_refusal`". `record_refused` keeps the bytes in the evidence store and
  appends a stated entry, `signature: unverified`, naming the claimed maker; `record_route_refusal` appends an
  `order_refusal`. `form` itself appends nothing, so a statement stays out even if the append fails.
- **One adapter to the log.** Per the coordinator (the log's append surface is being split, `T-020-01b`), every
  append of this module goes through `snapshot/journal.rs`, and it writes no decision kind.
- **The route guard (`REQ-ORD-16`, `REQ-ORD-36`, `REQ-ORD-37`).** `check_route` reads a route as received JSON,
  one part, so a contract type's own check on the way in can't mask Core's rule. Each rule has one owning check:
  typed and ordered (a non-empty list of the four kinds with two ends each, each starting where the one before
  ended), an asset changed only at an exchange (a change of decimals counts), and a withdrawal from an outer node
  first and a deposit to one last; `check_typed_operation`'s node rules follow. `check_part` runs it on a part the
  router chose. The class is `malformed_request`, the only one of the seven that fits. A route handed on under
  `REQ-ORD-43`, which starts at a holding account, is `T-020-13`'s; this guard refuses it until then.
- **Tests, and the order.** The code came first, then the tests, so each key rule was switched off in turn and its
  tests seen red: the exposure figures (all three exposure tests), the clock read once per formation replaced by a
  fixed reading (the clock test), the key binding (both statement tests), the nested walk (both), the operator
  path (the operator test), parts summed per provider (the provider ceiling test), the asset rule, the withdrawal
  first, the deposit last, and the order (each its route test). A tenth test, beyond the row's list,
  `judgement_is_carried_only_while_its_verifiers_grant_is_in_force_at_cores_clock`, covers the eligibility path
  (the verifier's origin, its grant at Core's clock), and went red with the verifier's grant check switched off. Every snapshot a test forms is checked against
  router-api's `catalogue-snapshot` schema and round-trips. The ceiling tests run through `RouteFinder`.
- **Touches**: `Cargo.lock` and `components/core-orch/Cargo.toml` (this row's lines: `serde`, `serde_json`; dev
  `ed25519-dalek`, `jsonschema`, all in the lock already, so no new package and no `licences/RECORD` line).
  Signature checking is a `DEC-20` concern: this row's binding of statements to keys is in `T-025-05`'s scope.
