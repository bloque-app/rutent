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
