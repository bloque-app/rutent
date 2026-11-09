# F-011 notes

Readings taken where `SPEC.md` or `PLAN.md` is silent or ambiguous, recorded per Narthex `REQ-SKL-24`.

## T-011-01

- **Scope.** The row's Done-when is "the router suite of `T-004-02` and these tests pass on the library". The
  suite's cells choose a route in the choice round, follow the objective, break ties by `candidate_id`, drop
  unconfirmed, refused, ineligible, over-ceiling, below-floor and rate-cycle candidates, and record `beyond_levels`.
  So this task builds the whole route pipeline the suite runs, not only the search: the shortlist mechanics, the
  ranking and the record are in it. `T-011-02`, `T-011-03` and `T-011-04` add their own named tests on this code, and
  the rules the suite doesn't reach: `quote_voided` and the quote rules of `T-011-03`, and the documentation pass of
  `T-011-04`.
- **The suite runs as a dev-dependency.** `components/router` depends on `rutent-contracts` and `num-bigint`. Its
  tests also use `rutent-conformance`, the one dev-dependency of ours the boundary check allows (`DEC-2`), and
  `serde` and `serde_json`. All were already in `Cargo.lock` at the versions recorded in `licences/RECORD`, so no
  line was added.
- **Exact arithmetic.** Amounts are up to 78 digits, so the router computes in `num_bigint::BigUint`, following
  `contracts/router-api/README.md` ("Computing, exactly"). The suite's arithmetic is its own, in
  `conformance/src/router/price.rs`, so its `amounts-follow-their-figures` check isn't the router agreeing with
  itself.
- **Bands after fees, in the segment's own unit (`REQ-QTE-12`).** Each operation is evaluated on what the one
  before delivered: after that one's fee and its conversion between units. Its band is checked on that amount, in
  its tuple's input unit, and its net, after its own fee, against its price's last level. The test puts a change
  from 6 to 18 decimals between the two segments. A router that checked the later band on the request's amount, or
  in the origin's unit, would keep a candidate the test drops.
- **"Never used" (`REQ-QTE-4`).** A figure past its validity never sets an operation's terms. A stale fresh quote
  or dry run binds nothing, so its operation is priced from its advertised price and dropped as `stale_figure`. An
  advertised price that states a validity and is past it drops every candidate through its tuple. Such a candidate
  is still listed with its reason, as `REQ-AUD-4` wants every candidate considered. The contract's citation rule is
  followed: a stale or void statement is cited only where no usable answer covers the operation.
- **The route's price (`REQ-QTE-8`), a gap.** `SPEC.md` says a route's quoted price is "the sum of its segments'
  advertised prices". Segments charge in different assets and units, so a sum across them has no meaning without
  rates. I read it as composition. Each segment's fee is its own price evaluated on what reaches it, and the route
  adds nothing of its own. In one asset and one unit, that makes the amount in less the amount out exactly the sum
  of the segments' fees. The named test and a property test check that case. The operator's fee shown apart
  (`REQ-QTE-2`) is `T-011-03`'s.
- **Any origin to any destination (`REQ-QTE-14`).** Every advertised tuple with its price is an edge: a reserve
  between an outer and an inner node, a ramp between inner nodes, an exchange between assets at one node. Nothing
  requires local money or an exchange. The named test is `AC-26`'s shape: an on-chain origin's reserve, two ramps,
  and a reserve at the destination. Every endpoint the tuples reach within the hop limit gets a route, and no other
  endpoint does. A property test checks the same over generated networks.
- **Order books stay possible (`REQ-EXT-2`), reviewed.** Nothing in the search assumes a fixed rate. An exchange's
  price may be `Pricing::Levels`, an order book's depth, and it is filled level by level at the actual net amount
  and rounded once. A net above the last level is served nothing: mid-route, the path is recorded up to that
  operation and dropped as `beyond_levels`; at the destination, it is dropped as `outside_band`. An order book
  resolved by the exchange provider reaches the router as levels in its price, or as its fresh quote at the actual
  amount in the choice round, so neither the router nor the catalogue rules it out.
- **One tuple once per path.** A path takes each tuple at most once, so the search ends within the hop limit. A
  path may come back to an endpoint it has passed. Where it comes back with more than it held, it is dropped as a
  suspect rate cycle (ADR-0006).
- **What the search can't state.** A tuple with no price is no segment, since an operation must cite one. An
  operation whose fee would be above what enters it can't be stated (the contract's `Operation::check`), so the
  path ends there unlisted. The contract has no drop reason for either case.
- **Unused terms.** A tuple's fill `increment`, its `reporting` and its `required_inputs` don't enter the search
  yet. No requirement in this row asks for them.
- **Candidate names.** Candidates are named `c001`, `c002`, … in order of the tuple ids they take. Ids are padded to
  one width, so byte order is path order, and the names don't depend on the order in which the snapshot lists its
  advertised figures. A property test reverses the snapshot and checks the candidates are unchanged. Within one
  operation, cited statements keep the snapshot's order.
- **Property tests (`DEC-18`) without a new dependency.** The properties are plain `#[test]` functions over
  networks drawn from a fixed-seed splitmix64 generator. A failure names the seed that reproduces it. `proptest`
  would bring several new crates into `Cargo.lock`, each needing a `licences/RECORD` line.

## Review of T-011-01 (fresh reviewer, 2026-11-09)

- **The search's cost.** The exhaustive search is bounded only by `max_hops` (a `NonZeroU8`, so up to 255). It is
  exponential in a dense catalogue, and each found path clones its steps. ADR-0006 lets policy set the hop limit, so
  this stands for now; a node budget is a candidate for a later task if catalogues grow.
- **Candidates the contract can't state.** Unpriced tuples and operations whose fee exceeds what enters them never
  appear as candidates, because the router API has no drop reason for them. `REQ-AUD-4` ("every candidate
  considered") then holds only for operations the contract can state. Open for `T-011-04` (the decision record) to
  settle, either by a drop reason under a `DEC-4` bump or by recording this reading as the contract's.

## T-011-03

- **Where the rules live.** The quote rules are pure functions in `components/router/src/quote.rs`, public as
  `rutent_router::quote`. None reads a clock, a provider or anything but its arguments; a time enters as a
  `Timestamp` the caller passes. `route` itself is unchanged: the router API's record has no place for an
  operator's fee, a firmness, a commitment or a hold, so these are checks Core runs on a record, not drops.
- **Inputs the contract doesn't carry, an item to raise.** The router API holds no operator's fee formula
  (`REQ-QTE-2`), no per-tuple slippage the operator sets (`REQ-QTE-9`), no history of a provider's stated terms
  with their times (`REQ-QTE-6`), no actual settlement times (`REQ-QTE-3`) and no held quote (`REQ-QTE-15`). The
  module defines each as its own argument type (`FeeFormula` for the operator, `SlippageLimit`, `StatedTerms`,
  `Timestamp` pairs, `HeldQuote`) rather than changing `contracts/`, a `DEC-20` path needing a `DEC-4` bump. Whether
  `PolicyBounds` should carry the operator's fee and the slippage limits, so a decision record names them
  (`REQ-AUD-4`), is open. So is how Core reaches these types: `DEC-2` has Core reach the libraries only through
  their public contracts, and these types are the router crate's, not `contracts/`'s.
- **A quote's fields (`REQ-QTE-1`).** The contract's types already require every element, so `quote_fields`
  refuses what the types can't: an amount in that isn't a positive whole number, an amount out or fee that isn't a
  whole number, a fee with no asset, a `valid_until` that isn't a timestamp or a figure with no validity, an empty
  `voided_by`, a conversion's rate that isn't a decimal, a figure from no provider, and a statement made by another
  party than the figure's provider. "A forged one is detected" needs the signature verified against the provider's
  keys, which the snapshot doesn't carry: that stays Core's precondition (router-api README, "Unchecked by
  either"), and no signature crate was added.
- **The price shown (`REQ-QTE-2`), read as composition.** Per the owner's ruling `spec-route-price-composition`,
  the providers' terms compose: what they deliver is the last operation's `amount_out`, summed over parts. The
  operator's fee is taken from that, at the destination, in its unit, by its formula: `flat + ceil(a × n / d)`,
  capped by its maximum where it gives one and never raised to it. (Route evaluation counts a provider's fee at its
  maximum, since the charge may come to it; a fee the operator shows is its own, known before execution, so the
  maximum only caps it. Changed after review.) It is taken there, not from the origin's amount, because taking it first
  would change every segment's amount and so void each quote asked at that amount (`amount_changes`). The only
  difference between the providers' terms and what is shown is then the operator's fee; `check_shown` refuses a
  shown amount with any other difference.
- **Quote and price (`REQ-QTE-7`).** `quoted_price` labels what it shows a quote. `executed_price` labels it the
  price, and only once every operation has an executed result; until then it is refused and what is shown stays
  the quote. The executed amounts are taken as given, from each provider's result.
- **Three calendar days (`REQ-QTE-9`, `REQ-QTE-10`).** A provider declares settlement in seconds from segment
  start, and the start's date isn't known when the route is quoted, so three calendar days are read as
  `3 × 86 400 = 259 200` seconds, inclusive: 259 200 is within three days, 259 201 is over. A segment's declared
  time is the slowest usable fresh quote it cites, or else its price, as `fastest_settlement` counts it
  (router-api `Objective`); a dry run's time isn't counted, as the contract counts only fresh quotes.
- **An unknown settlement time fails closed.** An operation that cites neither a usable fresh quote nor a price,
  which no record the contract accepts holds, has its time counted as `u64::MAX`, past any declared one (at most
  2^53 − 1). `firmness` takes it as over three days, so the route has no firm rate; `commitments` takes it as
  within them, so the segment is bound, never freed (after review).
- **Slippage within three days (`REQ-QTE-9`).** Within three days, an exchange's rate may move against the
  customer by at most the tighter of its quote's own regime (firm is 0, bounded is its bound) and the operator's
  limit for the tuple. A floating regime, or no quote, leaves the operator's limit. Within three days, an exchange
  on a tuple with no operator limit is named (`NoSlippageLimit`), not passed. A reserve or ramp sets no rate, so
  only its fees are committed. A move in the customer's favour is always within. Rates compare exactly, by
  cross-multiplication.
- **No firm rate (`REQ-QTE-10`, `AC-13`).** A route with any segment over three days has no firm rate.
  `firmness` names each such segment by part, operation and tuple, and `may_start` refuses it until the customer
  has accepted. Recording the acceptance is Core's (`REQ-ORD-10`).
- **Binding from segment start (`REQ-QTE-6`).** The terms bound are the last stated at or before the segment
  started, ties going to the one listed last; before the start, everything is a quote. Core keeps the history of a
  tuple's stated terms with their times; a snapshot's price figure states only its age at `taken_at`.
- **Held quotes (`REQ-QTE-15`).** A hold is a choice round's record that chose a candidate, with a validity at or
  after the record's snapshot. It is usable up to and at its validity, refused after it, and always labelled a
  quote. It binds no provider, whose terms bind only from segment start, so a hold that lapses or whose quotes
  expire has no effect beyond itself. Using a hold doesn't re-check its quotes' own validity; Core routes again
  when it starts the route.
- **Formula against charge (`REQ-QTE-5`, `AC-14`).** `formula_fee` computes `flat + ceil(a × n / d)`, its maximum
  aside, so anyone with the inputs can compute the fee before use. `check_charge` flags a charge above the maximum
  where one is given, the charge being known only after execution, and else above the formula's fee. A charge
  below either is within. The search's own fee (`price::fee`) is unchanged: it counts the maximum, which is right
  for evaluating a route but not for checking a charge.
- **`quote_voided` is not here.** The `T-011-01` notes left `quote_voided` open beside these rules. It is a drop
  reason in `route`, so it belongs with the search and the shortlist, which `T-011-02` is changing now. This task
  doesn't touch it.
- **Property tests (`DEC-18`), in `tests/quote.rs`, with no new dependency.** They use the same fixed-seed
  splitmix64 as `T-011-01`. They check that a charge at its bound is never flagged and one unit more always is;
  that a route is firm exactly when no segment is over three days, naming each that is; that no terms stated after
  a segment starts ever bind it; that what is shown differs from the providers' terms by the operator's fee alone;
  that a hold is refused exactly past its validity; and that a rate at its committed slippage is within while one
  just past it is not.

## T-011-02

- **What was already there.** `T-011-01` built the choice round's drops: a refused dry run (`dry_run_refused`), a
  path arriving with nothing (`no_positive_outcome`) and, in a choice round, an operation no answer confirmed
  (`unconfirmed`). This task adds the questions between the two rounds, in the module `shortlist`, and its named
  tests on the existing drops.
- **Asked only of the shortlist (`REQ-CAT-5`), and pure.** The router calls no provider. `shortlist::asks` reads a
  shortlist round's record alone and gives, by provider, one question for each operation of a kept candidate: the
  fresh quote for a route or exchange, the reserve's dry run for a withdrawal or deposit, at the amount that reaches
  it, each asked once. A provider on dropped candidates only, or on none, is asked nothing. Core puts the questions
  and records the answers in the choice round's snapshot; `Asks::admits` says which figures may enter it, so an
  answer nobody asked for stays out. The choice round is then the same pure function of its inputs.
- **Only what the step needs.** A question names its tuple, the operation's kind and ends, and the amount entering
  it. It names no candidate, no request and no other provider's tuple.
- **A rival seen through amounts, a gap (`REQ-CAT-5`).** That holds for one question, not for the set a provider
  receives. Questions are deduplicated on tuple and amount, so a later tuple reached through two kept earlier paths
  gets two questions at two amounts: with reserves R1 or R2 before a ramp B, B is asked at both, learns that an
  alternative before it was considered, and reads the difference of their fees from the gap. Asking only the best
  path's amount would leave the alternatives unconfirmed and lose the fallback `AC-3` needs, so I left it. Delivering
  the questions so they can't be linked is Core's (`F-020`), or this is accepted residual risk; raised as an item for
  the owner.
- **Eligibility, a gap.** No advertised tuple in the snapshot names the class or the verifier its provider's rules
  need. So a question names only the tuple eligibility is checked for (`REQ-CPL-6`); which judgement to seek, of
  which verifier, for which party, is the catalogue's grants (`F-012`), and Core's to put. `Asks::admits` lets in a
  judgement for an asked tuple whatever its verifier.
- **A refused dry run (`AC-3`, `REQ-CAT-14`).** Every kept candidate's providers are asked together, so another
  provider serving the same ends already has its answer when one refuses, and the choice round uses it with no
  second round. The test makes the refusing reserve the cheaper one, so its drop is what lets the other be chosen.
- **No positive arrival (`REQ-QTE-13`).** A shortlist round that keeps nothing is refused before anyone is asked:
  `asks` returns `Refusal::NoRoute`, and the choice round over the same figures chooses nothing. Where the main path
  arrives with nothing and an alternative with something, only the alternative's provider is asked, and it is
  chosen. `NoRoute` also covers a shortlist emptied for other reasons, such as the bounds, since none of them leaves
  a path to offer.
- **Asks are at the shortlist's amounts, a reading.** A question asks at the amount the shortlist round evaluated
  from advertised prices. Where an earlier operation's fresh quote then differs from its price, a later operation's
  amount shifts, the answer it got no longer covers it, and it is `unconfirmed`. Asking again at the new amount is
  Core's (`F-020`): the provider is still on the shortlist. This task doesn't add it.
- **Seeing the tests fail.** The questions' tests failed first to compile, with no `shortlist` module. The
  refusal and no-positive-arrival tests pass on `T-011-01`'s code, so I disabled the two drops in `judge.rs` for one
  run and saw them fail: the refusal test on its drop reason, the no-positive-arrival test on the contract refusing
  the record. Then I restored `judge.rs` unchanged.
- **Property tests (`DEC-18`).** Over the generated networks, the asked providers are exactly those of kept
  operations, each asked only about its own tuples, and the questions don't change when the snapshot is reversed.
  In a choice round built from the questions' answers, half of them refused at random, no refused option is chosen,
  the choice always arrives with something, a shortlisted route whose every answer accepted always leaves a choice,
  and an answer from a provider off the shortlist is never admitted. The choice-round property runs without
  exchanges, since its answers carry no rate or slippage; its conversion branch is unused there. These are appended
  to `tests/properties.rs` to reuse its generator; `tests/common/` is left as it was.
- **Seeing the properties fail.** Each was run once against a mutated `asks`, then `shortlist.rs` was restored. Asking
  every candidate's operations, not only the kept ones', trips the first property (seed 9). Asking only each kept
  candidate's first operation trips the second (seed 61: a confirmed shortlisted route is not chosen).

## Review of T-011-02 (fresh reviewer, 2026-11-09)

- **One amount per tuple, the default applied for item `cat5-single-amount-asks`.** The reviewer confirmed the gap
  above, so it is closed on the safe default, which the owner can overrule on the form. Each provider is asked
  about each of its tuples once, at one amount. Where kept paths reach a tuple at different amounts, the question
  is at the amount on the kept candidate ranked first: by the request's objective, as the choice round ranks, then
  by `candidate_id`. For fastest settlement, the shortlist round holds no fresh quote, so each operation's seconds
  are its price's. The other paths' operations at that tuple stay `unconfirmed` in the choice round until Core asks
  again at their amounts (`F-020`). This trades away the fallback through those paths in one choice round. `AC-3`'s
  fallback to another provider of the same tuple still holds, since that provider is asked about its own tuple.
  Keeping questions unlinkable across calls, with no shared session, transaction or identity between them, is
  `F-020`'s. The test `provider_reached_at_two_amounts_is_asked_at_one_only` failed first, with two questions to
  the ramp. The first property now checks one question per kept tuple. Against the old per-amount keying it fails
  at seed 60.
- **A refused fresh quote, read differently from the review.** The review asked to admit a refused `FreshQuote` and
  read it as a refusal. The contract refuses such a figure in any snapshot (`CatalogueSnapshot::check`: "a fresh
  quote is a provider's dry-run quote", accepted only). So a ramp or exchange refuses as a `DryRun` figure holding
  its refused transfer or conversion, and `judge.rs` already drops on that as `dry_run_refused`. `Asks::admits`
  keeps a refused `FreshQuote` out and admits the refused `DryRun` for a fresh-quote question. `judge.rs` is
  unchanged. The test `ramp_that_refuses_its_quote_is_dropped_as_refused_not_unconfirmed` shows all three: the
  admission, the contract's refusal of the other form, and the drop. It passed on first run, since it pins down
  existing behaviour.
- **The form asked.** `Asks::admits` now also matches the question's `wants`. A fresh-quote question admits the
  accepted quote or a refused dry run. A dry-run question admits a dry run, accepted or refused. The test
  `answer_of_another_kind_than_asked_is_not_admitted` failed first, admitting a reserve's answer dressed as a fresh
  quote.
- **`Question`'s doc** now says that a question names no rival per question, and that the set a provider receives
  is kept clean by the one-amount rule.

## T-011-04

- **What was already there.** `T-011-01` built the ranking by objective, the record and the one part per
  candidate, since the router suite needed them. This task adds the row's named tests and properties on that code,
  documents the objective and the record in `lib.rs`, and settles the three items the earlier notes left open. No
  file under `src/` changes but `lib.rs`'s docs, and `VERSION` stays, since no record changes.
- **The default objective (`REQ-QTE-11`).** The request always names its objective, and the contract has whoever
  builds it write `lowest_total_cost` when the customer chose none (`Objective`'s doc). So the router has no
  default of its own to apply. `Objective` is `#[non_exhaustive]`, and an objective the contract adds later is
  ranked as lowest total cost until the router learns it, which keeps the default the fallback. Giving `Objective`
  a `Default` would be a `contracts/` change, and nothing here needs it.
- **Changed objective (`AC-2`).** The test is `AC-2`'s shape: a withdrawal, two exchanges at one rate, one cheaper
  and one faster, and a ramp. Its choice round answers every kept operation at every amount a kept candidate takes
  it at, so one snapshot serves both objectives and no provider is asked again between them. That is more than
  Core asks under the one-amount rule (`T-011-02`'s review); the router doesn't care which answers were asked for.
  Lowest total cost chooses the cheaper exchange, fastest settlement the faster one, and each drops the other as
  outranked by its choice.
- **Recomputing with no provider call (`AC-2`, `REQ-AUD-4`).** No provider call is structural: `route` takes the
  request, the snapshot and the bounds and nothing else, and the record holds all three verbatim. The test keeps
  only the record's bytes, reads them back, and has the router suite's `recompute` (its own arithmetic, `DEC-5`)
  find the record's choice under both objectives; every candidate but the chosen one names its drop, and routing
  the record's own inputs gives its bytes again.
- **One part (`REQ-QTE-16`, `DEC-8`).** The test allows three parts where two cheap ramps each serve only 600 of
  the 1,000 asked, so a splitting router would send 500 through each. Every candidate in both rounds is one part,
  the cheap ramps are dropped as `outside_band`, and the dear ramp that serves it all is chosen.
- **Seeing the tests fail.** All four named tests passed on first run, since they pin down existing behaviour. Each
  was run against a mutated router, then the source restored unchanged: ranking ignoring the objective failed the
  objective and recompute tests (the suite's ranking check); a candidate split in two when `max_parts` is above 1
  failed the one-part test (the contract refuses the record); and an operation stated with its fee capped at what
  enters it failed the unstatable-tuples test.
- **Property tests (`DEC-18`), appended to `tests/properties.rs`.** Over generated networks, exchanges included: the
  choice round over every kept operation answered always chooses, and the suite's `recompute`, from the record's
  bytes alone, finds the same choice (the existing property ran shortlist rounds and checked only `passes`); every
  candidate is one part with `max_parts` drawn from 1 to 8; changing the objective changes no candidate, part or
  drop but `outranked` and the choice; and a tuple whose price is removed changes no candidate beyond the record's
  snapshot, as if the tuple were absent. Each failed under a mutation, then restored: ranking ignoring the objective
  (seed 117), the split in two, the band checked before validity under one objective only (seed 9), and an unpriced
  tuple borrowing another's price.

### The open items

- **Paths the contract can't state, the reading recorded as the contract's (`REQ-AUD-4`).** A candidate is a path
  whose every operation the contract can state. A tuple with no price is no operation (`Operation` cites one
  price), nor is one whose fee would be above what enters it (`Operation::check`), so neither appears as a
  candidate, and "every candidate considered" means every path of statable operations. The choice still
  recomputes from the record alone: both stay in the record's snapshot verbatim, and the rule that drops them reads
  nothing else. This matches the contract README, whose "Unchecked by either" already says the record isn't
  checked to list every candidate. A test and a property pin it down.
- **Why not a `DEC-4` bump now.** A drop reason for them is a new `DropReason` variant, which breaks an exhaustive
  match, so it is a major under the precedent of module-interface 3.0.0: router-api 3.0.0. `PLAN.md` has `F-011`
  build `route` "as router-api 2.0.0 defines it", which 2.1.0 still meets and 3.0.0 wouldn't, and this task can't
  edit `PLAN.md`. M3's Core contract task may reopen router-api anyway, for the inputs below, so one major there can
  carry both. The contract also only adds amounts, so it couldn't check an over-fee claim itself: the router suite
  would.
- **Item to raise, for M3's contract task.** A drop reason for paths that can't be stated, shaped as
  `beyond_levels` (0.7.0): the part listed up to and including the operation that can't be stated, which arrives
  with 0, the part needn't reach the destination, and the reason names `{part, operation}`. For an unpriced tuple,
  that operation cites its tuple and no price, with a fee of 0, and the contract checks the snapshot holds no price
  for it. For a fee above the amount, the fee is stated as the amount in, and the suite checks the formula's fee on
  it is above it. The suite's `amounts-follow-their-figures` check skips the named operation.
- **`quote_voided`, still unowned.** This row doesn't serve `REQ-QTE-1`, and neither it nor the suite needs the
  reason: the suite checks the route chosen, not the drop reasons, and an operation whose answer was asked at
  another amount is truthfully `unconfirmed`. The likely owner is `F-020`, which asks again at the shifted amount
  (`T-011-02`'s notes). The contract already allows it: in a choice round, an unconfirmed operation for which a
  usable fresh quote of its tuple at another amount lists `amount_changes` may cite that quote and be dropped as
  `quote_voided` for `amount_changes` instead. For `validity_passes` the router keeps `stale_figure`, which is as
  true and needs no condition listed.
- **The inputs the contract doesn't carry, open for M3's Core contract task.** The decision record needs none of
  the operator's fee formula (`REQ-QTE-2`), the slippage limits (`REQ-QTE-9`), the history of stated terms
  (`REQ-QTE-6`), the actual settlement times (`REQ-QTE-3`) or the held quote (`REQ-QTE-15`) to recompute the
  choice: `Objective` ranks on the providers' last `amount_out` and declared seconds, before the operator's fee.
  So `PolicyBounds` is unchanged. One consequence to note: the operator's fee is taken the same way from every
  candidate, at the destination, and where its proportion is at most one, what the customer receives after it
  never falls as what arrives grows. Ranking after it could then differ from ranking before it only where it turns
  a difference into a tie, which goes to the lowest `candidate_id`. All
  five, and whether the ranking should be net of the operator's fee, stay open for that task, together with how
  Core reaches the `quote` module's types under `DEC-2`.

## REQ-AUD-4 partly met until router-api 3.0.0 (review of T-011-04, 2026-11-09)

A path through an unpriced tuple, or through an operation whose fee exceeds what enters it, is a candidate in
`REQ-AUD-4`'s sense ("every candidate and why each was dropped"), but the router leaves it out with no drop reason,
because router-api 2.0.0's `DropReason` has no variant for it and isn't `#[non_exhaustive]`, so adding one is a major
bump. `REQ-AUD-4` is therefore partly met. The orchestrator's default, reversible and on the decision form
(item `aud4-unstateable-candidates`): the drop reasons come with M3's Core contract task as router-api 3.0.0, under
`DEC-4` and the owner's click at the release into `main`.
