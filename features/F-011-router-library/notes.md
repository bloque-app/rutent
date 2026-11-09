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
  operator's fee is taken from that, at the destination, in its unit, by its formula (at its maximum where it gives
  one, as route evaluation counts a fee). It is taken there, not from the origin's amount, because taking it first
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
