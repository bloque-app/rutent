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
