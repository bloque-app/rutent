# F-004 notes

Readings taken where `SPEC.md` or `PLAN.md` is silent or ambiguous, recorded per Narthex `REQ-SKL-24`.

## T-004-01

- **Purity and time.** `route` takes no `self` and reads only its three arguments (ADR-0006, `AC-2`). The only time
  it sees is the snapshot's `taken_at` and each figure's `age_seconds` and `valid_for_seconds` at it, so "past its
  stated validity" (`REQ-QTE-4`) is judged from the record alone. An advertised tuple or price states no validity
  and holds until replaced.
- **Age and origin.** Every figure carries `origin` (provider and provider version, which also gives `REQ-AUD-4`'s
  module versions) and `age_seconds`. A quote's age and validity must agree with its envelope's `made_at` and its
  `valid_until`, in whole seconds with fractions dropped. Rust checks this, so the two can't disagree.
- **The decision record.** It holds the request, the snapshot and the bounds verbatim, the router's version, every
  candidate with its drop reason, and the chosen candidate by id (`REQ-AUD-4`). That is everything `route` reads,
  so T-004-02 can call `route` again on the record's own inputs. The trait returns this one record, and
  `candidates()` and `chosen()` are views into it. That way the SAD's triple can't disagree with itself.
- **No route.** A record with no `chosen` means no route. Every candidate then has a drop reason. This matches the
  state machine's `no_route { decision_record }`.
- **Record id.** A record's id is its request's `request_id`, since a pure function can't mint one. `follows`
  names the previous request for the same order at a recalculation, which chains the records (SAD §3).
- **Envelope on records.** A decision record is the deployment's statement, `Statement<DecisionRecord>`. The router
  returns the unsigned record, and the deployment signs it when it records it. A pure function can neither sign
  nor read `made_at`.
- **Policy bounds.** They are not wrapped in an envelope. `PLAN.md` asks for the envelope on quotes and records only.
  They carry the deployment and protocol version every statement is judged against, so `void_statement` is
  recomputable from the record (ruled by the orchestrator, 2026-11-06, after review).
- **Split-shaped.** Every candidate is a split of one or more parts, and each part is an ordered list of typed
  operations (`REQ-ORD-37`, `REQ-QTE-16`). v0 sets `max_parts` to 1 (`DEC-8`). The parts' amounts in add up to the
  request's amount, and each part runs from its origin to its destination (`REQ-ORD-42`).
- **Typed operations (ruled by the orchestrator, 2026-11-06).** Routes are made of typed operations
  (`REQ-ORD-37`), so the router contract owns the shape: `urn:rutent:router-api:operation#/$defs/operation`, its
  kind at `#/$defs/kind` and in Rust `router_api::OperationKind`, and its two ends as provider-role nodes and
  denominations. It stands alone and is open for embedding; the state machine's segment plan is to `$ref` it
  next. The router's priced operation extends it and closes it with `unevaluatedProperties: false`. There is one
  vocabulary, so a fifth kind is one protocol change (`REQ-PRT-5`).
- **Operations cite figures.** An operation names the snapshot figures it was evaluated with. It does not embed its
  tuple, so each tuple appears once in the record. Amounts are evaluated at the actual amount, after the fees
  before them (`REQ-QTE-12`). Each operation takes in what the one before it delivered.
- **Direction (`REQ-QTE-1`, ADR-0006).** A quote states which amount it fixes, `exact_in` or `exact_out`, as
  provider-role's quotes do; the path direction is its `from` then `to`. The request's amount is what leaves the
  origin. Whether a customer may instead fix the amount received was
  raised to the owner, who ruled `spec-fixed-receive`: no fixed-receive in v0. The request keeps the amount sent
  and the optional floor.
- **Objectives (`REQ-QTE-11`, `AC-2`).** SPEC names only the default, lowest total cost, and `AC-2` needs at
  least two objectives. I added `fastest_settlement`, which rests on `REQ-QTE-3`'s declared settlement times. The
  full list is **raised to the owner**. The objective is required in the request: whoever builds the request
  writes the default, so the record always names the objective it was computed for.
  - The objective is a tagged `{kind}` object, `#[non_exhaustive]` in Rust, so more can be added (ruled by the
    orchestrator, 2026-11-06).
  - `fastest_settlement` over a split is the slowest part's time; a part's time is the sum of its operations'
    declared settlement seconds (ruled by the orchestrator, 2026-11-06).
  - Ties go to the lowest `candidate_id`, comparing bytes, so an independent recompute agrees (`THR-10`; ruled by
    the orchestrator, 2026-11-06).
- **Floor (`REQ-ORD-27`).** The floor is the least amount accepted at the destination, in its unit. It is optional
  and never `null`.
- **Request endpoints.** An outer origin or destination names only its network. The reserve is the first or last
  operation's provider, chosen by the router (`REQ-CAT-17`).
- **Quotes, dry runs and eligibility (`REQ-QTE-1`, `REQ-QTE-3`, `REQ-CAT-14`; ruled by the orchestrator,
  2026-11-06).** They are the providers' own statements, carried whole and unchanged so their signatures still
  verify, never re-expressed:
  - a fresh quote is the provider's signed `urn:rutent:provider-role:result` for a dry-run transfer or conversion,
    whose outcome is its `transfer_quote` or `conversion_quote`, for a ramp's or an exchange's tuple;
  - a dry run is its signed result for a dry-run withdrawal, deposit, transfer or conversion, accepted or refused,
    for its tuple's kind of operation;
  - eligibility is a verifier's signed `urn:rutent:provider-role:judgement`. Its maker is the figure's provider,
    the verifier, not the tuple's. Which class a tuple needs isn't stated anywhere yet, so the link from a
    judgement to its tuple is the figure's `tuple` alone.
  - The segment's ends come from the figure's tuple, since provider results name a transaction, not ends.
- **Prices and order books (`REQ-QTE-5`, `REQ-EXT-2`).** An advertised price is a fee formula: a flat part, a
  proportional `Ratio` and an optional maximum. Its rate is either fixed or given by levels, so the rate can depend
  on the amount, as an order book's depth does. Provider-role has a charged fee but no formula, so the formula
  is router-local.
- **Two rounds (`REQ-CAT-5`, `DEC-8`; ruled by the orchestrator, 2026-11-06).** `route` stays one pure
  function. Core calls it on the advertised snapshot (a shortlist round, which chooses nothing; its kept
  candidates are the shortlist), then on a refreshed snapshot that adds fresh quotes, dry-run answers and
  eligibility answers for the shortlisted providers only (a choice round, which names the shortlist round it
  refines). Each round has its own request id, so both records are kept (`REQ-AUD-4`). That the refreshed snapshot
  asks only shortlisted providers needs the shortlist record, so it is Core's to keep, not this contract's to check.
- **Module version (`REQ-AUD-4`; ruled by the orchestrator, 2026-11-06).** It is the admitted module's version,
  its admission artefact's (`F-003`). Each provider origin records the provider version and the module version.
- **Ceilings (`REQ-ORD-8`, `THR-136`; review fix, ruled by the orchestrator, 2026-11-06).**
  - The bounds hold ceilings per provider and per publisher, each in one asset.
  - The snapshot holds in-flight exposure from the operator's records, with an operator origin.
  - Each provider origin names its publisher.
  - A candidate's value at a provider is the sum of its operations' `amount_in` there across all parts, plus
    exposure; over a ceiling it is dropped as `over_ceiling`. Value across assets isn't converted, so a ceiling
    is per asset.
- **Validity rounding (`REQ-QTE-4`; review fix).** Ages round up and validities round down, so a figure whose age
  is at most its validity is never past its `valid_until`. It may drop a figure in its last second, a safe loss.
- **Rounding of fees and rates (`REQ-QTE-5`; review fix).** The fee is `flat + ceil(a × n / d)` since 2.0.0
  (`floor` before; see "Review round 1 of the M1 pull request" below), counted at `maximum` where one is given. The conversion rounds down once, after summing the levels. A net amount above the
  last level isn't served (`outside_band`).
- **Integers.** Every integer a signed statement carries is at most 2^53 - 1, following provider-role's signing
  rule (RFC 8785), so seconds are a `Seconds` newtype.
- **Drop reasons (`REQ-AUD-4`).**
  - stale figure (`REQ-QTE-4`);
  - void statement (`REQ-PRT-1`);
  - outside a band (`REQ-QTE-12`);
  - excluded by policy (`REQ-QTE-11`);
  - below the floor (`REQ-ORD-27`);
  - no positive outcome (`REQ-QTE-13`);
  - suspect rate cycle (ADR-0006);
  - outranked by another candidate;
  - a fresh quote voided by a condition in its `voided_by` (`quote_voided`), whose vocabulary is provider-role's;
  - a refused dry run, or an ineligible answer;
  - over a ceiling (`over_ceiling`).
- **Policy bounds.** They hold the policy version, the hop limit, the most parts, and excluded providers and
  tuples. The operator's per-tuple slippage (`REQ-QTE-9`) binds a segment once it starts, not the route choice, so
  it is not in the bounds yet.
- **Reconciled with provider-role 0.3.0 (ruled by the orchestrator, 2026-11-06).** No placeholder is left. The
  envelope, fee, rate, timestamp, decimal, safe integer, protocol version and void conditions are provider-role's,
  by `$ref`, and its Rust types are used: `Statement<T>`, `Deployment`, `Fee`, `VoidedBy`, `SafeInteger`,
  `ResultBody`, `Judgement`, with `check_shape` and `check_context`. Where provider-role's Rust type is a plain
  `String` (fee amounts, times), router-api's `check` parses it with its own strict newtype. Rate, timestamp and
  protocol version in router-api's own fields stay router-api's strict newtypes over the provider-role schemas.
- **Void statements (`REQ-PRT-1`).** The chosen candidate may use no statement that provider-role's
  `check_context` finds void at the bounds' deployment and protocol version, and a `void_statement` reason must
  name one it does.

### After the second review (router-api 0.2.0)

- **Drops are supported (review fix).** A drop reason that names a figure names one its candidate cites, and the
  figure bears the drop out:
  - `stale_figure`: the figure is past its validity;
  - `void_statement`: it is void by `check_context`;
  - `quote_voided`: a fresh quote that lists the condition, past its validity for `validity_passes`, at another
    amount than a citing operation for `amount_changes`; the other conditions rest on facts outside the record,
    so only their being listed is checked;
  - `dry_run_refused`: a refused dry run;
  - `ineligible`: a judgement of `no`.
- **Confirmation in the choice round (ruled by the orchestrator, 2026-11-06).** The choice round may choose only a
  candidate whose every operation carries the providers' answers from after the shortlist.
  - I read "a fresh quote and dry run" as one accepted dry-run answer of the operation's provider for its tuple,
    at its amount, within its validity and not void.
  - For a route or exchange that is the fresh quote, since provider-role's `transfer_quote` and `conversion_quote`
    are themselves `dry_run_accepted` answers. For a withdrawal or deposit, which have no quote kind, it is the
    reserve's accepted dry run.
  - Any other candidate is dropped `unconfirmed {part, operation}`, naming an unconfirmed operation. The chosen
    candidate also uses no figure past its validity.
- **Operations cite their tuple (review fix).** Each operation cites a tuple figure, the first it cites, from its
  own provider, with its two ends in their units.
- **Ceilings match on units (review nit).** A ceiling, an exposure and an `over_ceiling` reason name the asset as a
  denomination, id and decimals, and match on both.

### After the third review (router-api 0.3.0)

- **Figures are bound to their operation's tuple (review fix).** Matching a figure by id alone let a refused dry
  run or a judgement of no sit on an operation it isn't about. Every operation now cites one tuple, and every
  price, fresh quote, dry run or judgement it cites names that tuple. So a drop's figure is cited by an operation
  whose tuple it names.
- **What a kept candidate may not cite (review fix).** A candidate kept (chosen, or on the shortlist) cites no
  refused dry run and no judgement of no, as well as no void or stale figure.
- **Confirmation is on terms (review fix).** A confirming answer equals its operation's amount in, fee and amount
  out, so an operation can't claim better terms than its provider confirmed and win the ranking on them.
- **Recomputed drops (review nit).**
  - `below_floor`, `no_positive_outcome`, `over_ceiling` (with the exposures), `excluded_by_policy` and
    `outside_band` are recomputed from the candidate's amounts, the request's floor and the bounds.
  - A kept candidate must pass each of those tests, and must keep within the part and hop limits.
  - A ceiling binds a candidate only where it puts value: an exposure already over a ceiling doesn't drop a
    candidate that puts nothing there.
- **Unchecked, and why.**
  - `outranked` is checked only for naming a candidate of the record, and `suspect_rate_cycle` is not checked at
    all. Both rest on the ranking, which `T-004-02` recomputes.
  - That a confirming answer was made after the shortlist round can't be checked: the choice record names the
    shortlist round by `refines` but doesn't contain it.
  - `quote_voided` is accepted on a listed condition alone for conditions other than `validity_passes` and
    `amount_changes`.

### After the fourth review (router-api 0.4.0)

- **Scope (ruled by the orchestrator, 2026-11-06).** `T-004-01` checks a decision record's structure and
  cross-references. Recomputing amounts from prices, and the ranking, belong to `T-004-02`, whose acceptance is
  "choice is recomputed from its record with no provider call".
- **What the recomputed drops check.** `below_floor`, `no_positive_outcome`, `over_ceiling`, `excluded_by_policy`
  and `outside_band`, and the matching tests on a kept candidate, use each operation's stated fee and
  `amount_out`. They show the record's stated amounts consistent with its drops, not with the prices it cites.
- **One price per operation (review fix).** Every operation cites exactly one price, for its own tuple, so a
  price's levels can't be dodged by leaving it out.
- **One publisher per provider (review fix).** Across a snapshot, exposures included, a provider has one
  publisher, so an exposure filed under another publisher can't dodge a publisher ceiling.
- **Answers agree (ruled by the orchestrator, 2026-11-06).** Within one round's snapshot, dry-run answers for
  one tuple and amount don't disagree (one accepted, one refused), and judgements of one subject for one tuple
  don't disagree. A snapshot that holds disagreeing answers is refused.
  - A refusal doesn't restate its amount, so a dry-run figure now records `amount_in`, the amount it asked
    about. An accepted answer must state the same. Any dry run an operation cites must be for its amount in,
    or the agreement rule could be dodged by citing a refusal at another amount.
  - Fresh quotes count as accepted answers, since provider-role's quotes are `dry_run_accepted` answers.
  - The judgement rule keys on subject and tuple, as ruled, not on class: which classes a tuple needs isn't
    stated anywhere yet.
  - The every-drop-reason fixture is fixed. c4's refused dry run is for 40,000,000, in a split of two parts
    (the bounds now allow two), while chosen c1 withdraws 100,000,000. c5's judgement of no is for another
    tuple, `f-ex-a`, than the yes that chosen c1 cites on `f-ex-b`.

### After the fifth review (router-api 0.5.0)

- **One tuple per provider and ends (review fix).** A snapshot refuses two tuple figures with one provider and
  the same two ends (the `TupleKey` key). Otherwise a copy could carry wider bands, or a refused dry run beside an
  accepted one on the original.
- **One price per tuple (review fix).** A snapshot holds at most one price per tuple figure, so a second, looser
  price can't dodge the levels.
- **One subject per relying provider (ruled by the orchestrator, 2026-11-06, refining the earlier ruling, then
  corrected after the delta review).** Subject references are per relying provider (ADR-0012; the judgement's
  `subject` is "the one given to this relying provider"), so a snapshot can't share one subject across providers.
  - The rule: within one snapshot, all judgements about one relying provider's tuples are about one subject. A
    snapshot holding two subjects for one relying provider is refused.
  - The relying provider is the provider of the tuple a judgement names, not the verifier that made it. Grouping
    on the verifier, as first written, let a second verifier bring another subject to the same tuple, and refused
    one verifier judging two relying providers under their own references.
  - The request carries no subject: linking a provider's reference to the customer is Core's (M3).
  - In the fixtures, exchange-a's tuples are judged under `subj-77` and exchange-b's under `subj-42`. A valid
    fixture has one verifier judging both under their own references, and an invalid one has a second verifier
    bringing `subj-99` to exchange-a's tuple.
- **Fresh quotes in dry-run agreement.** Now tested: a fresh quote and a refused dry run for one tuple and amount
  are refused together.
- **Distinct figures (review nit).** An operation cites each figure once; a repeat is a "duplicate figure".

## Deferred to T-004-02

- An operation's fee and `amount_out`, recomputed from the price or quote it cites.
- The ranking under the objective, and so `outranked`.
- `suspect_rate_cycle`.
- A positive outcome per part. It only matters with splitting, which v0 leaves out (ADR-0006).
- Ceilings counting value produced in the output asset. They now count only `amount_in` in a ceiling's asset.
- Shortlist-round snapshots carrying fresh quotes. A shortlist round runs over advertised figures, but nothing
  here refuses a statement in its snapshot.


## T-004-02

The router suite is `rutent-conformance`'s `router` (`conformance/src/router/`), with its reference double and its
own tests under `conformance/tests/`. Readings taken where SPEC, PLAN or the contract leave a choice:

- **Where the double lives.** The boundary check allows `components/router` to hold it, but there the suite's own
  tests couldn't use it, since the conformance crate may depend on the contracts alone. So it is test-only, in
  `conformance/tests/router_double/`, beside the breakers, as the host suite's double is. That also leaves
  `CMP-ROUTER`'s design to M2.
- **Dependencies.** `num-bigint` 0.4.8, `serde` and `serde_json` are added to the conformance crate. All three
  were already in `Cargo.lock` and `licences/RECORD`, permitted under `licence-permissive-standing`, so the lock
  gains no package and the record no line. Amounts are up to 78 digits and their products are longer, so the
  arithmetic is arbitrary-precision whole numbers. Nothing is floating point.
- **Which figure sets an operation's amounts.** An operation that cites usable accepted answers for its amount
  in (a fresh quote, or a reserve's accepted dry run, within its validity and not void) states each one's fee and
  `amount_out`. Any other operation states what its price gives on its amount in. This follows the contract's
  confirmation rule, which already holds the chosen candidate to its answers' terms. Answers at another amount,
  past their validity or void set nothing.
- **No output above the last level.** The contract says a net above a price's last level "isn't served: the
  price gives no output". I read that as zero. An operation after such a one can't be stated, since what enters
  an operation is positive, so a router can list such a candidate only when the unserved operation is its last.
  An operation whose fee is above what enters it can't be stated at all. The double lists neither, and a
  candidate with an unserved operation mid-route can't be recorded by any router (listed as unchecked below).
- **Settlement time.** For `fastest_settlement`, an operation's seconds come from the usable fresh quote it cites
  for its amount in, or else its price. A quote at another amount never counts: since 0.6.0 the contract lets an
  operation cite one only where its candidate is dropped `quote_voided` for `amount_changes` naming it, so such a
  candidate is never ranked. Usable quotes for one amount agree on their terms but may not on their time; where
  several are cited, the slowest counts.
- **`suspect_rate_cycle`, defined.** A part follows a suspect rate cycle when it comes back to a node and asset
  it held before with strictly more than it held there: free profit, possibly stale or false, never followed
  (ADR-0006). The two holdings may be counted in different units of the asset (an exchange from USDC in 6
  decimals to COP and back to USDC in 18), so they are compared in whole numbers over a common unit: `a` in
  `da` decimals against `b` in `db` is `a × 10^db` against `b × 10^da`. No kept or outranked candidate may follow
  one, and the reason may name only a candidate that does.
- **Who is eligible, and the ranking.** The pool is the kept candidates and the outranked ones.
  - An outranked candidate must be one the contract would accept in the choice's place. The suite checks that by
    running `DecisionRecord::check` on the record with that candidate chosen (or kept, in a shortlist round) and
    the chosen one outranked by it. So the contract's rules for a kept candidate apply to it unchanged.
  - Every kept candidate ranks ahead of every outranked one, and an outranked candidate's `by` is in the pool
    and ranks ahead of it. In the choice round that makes the chosen candidate the first of the pool, and a choice
    round with any candidate in the pool must choose.
  - In the shortlist round, the router may cut the shortlist, since the bounds give no size, but only from the
    bottom: what it keeps must rank ahead of what it outranks. The double keeps every eligible candidate.
  - Every other drop reason is the contract's to recompute, and it does; the suite accepts any reason that holds
    and asks no order among them. The double's order (in its module header) is its own.
- **Contract change, router-api 0.6.0.** These rules moved into the contract. For the first two, the suite would
  have refused records that the contract gave a router no valid way to write; the citation rules close a hole the
  review of the first draft found, where the suite trusted each operation's `figures` against the snapshot:
  - A positive outcome is per part. Before, `no_positive_outcome` was refused unless the whole candidate arrived
    with nothing, so a split with one empty part had no reason that fit. Now a kept candidate arrives with
    something in every part, and the reason holds when any one part arrives with nothing.
  - Ceilings count what an operation produces. Before, an `over_ceiling` reason for a ceiling in an exchange's
    output asset was refused, since only `amount_in` counted. Now an operation counts its `amount_in` where its
    from asset is the ceiling's, or else its `amount_out` where its to asset is. Each operation counts once per
    ceiling, so a route keeping its asset isn't counted twice.
  - An operation cites every usable statement about its tuple that bears on it: each fresh quote and dry run
    for its amount in, and each eligibility judgement (ruled by the orchestrator after review, 2026-11-07).
    Otherwise a router could drop the best route as `unconfirmed` by leaving its quote out and choose a worse
    one, or choose a route by leaving out a refusal or a judgement of no. Statements past their validity or void
    needn't be cited: they confirm nothing and refuse nothing.
  - A fresh quote an operation cites is for its amount in, as a dry run already had to be, so a quote at another
    amount can't lend a route a slower settlement time and reverse the AC-2 choice. The one exception is the
    quote a `quote_voided` reason for `amount_changes` names, which is that very mismatch; its candidate is never
    ranked.
  - Usable accepted answers for one tuple and amount agree on their fee and amount out. Without this, an
    operation made to cite them all couldn't state terms that follow every one, and the router would have no
    valid record. It extends the earlier ruling that answers for one tuple and amount don't disagree.
  - A `stale_figure`, `void_statement` or `quote_voided` reason holds only where no operation citing the named
    statement also cites a usable accepted answer for its tuple at its amount in (the delta review's fix, ruled
    by the orchestrator, 2026-11-07). The citation rule stops a good figure being left out; this stops a bad one
    being added in, as a stale quote beside a confirmed one, or a quote at another amount through the
    `amount_changes` exception, to drop a confirmed route and choose a worse one. No valid record is lost: a
    router can always leave a statement past its validity, void or at another amount out. The rule binds only a
    reason that names a statement, since an operation can't leave out its tuple or its price.
  - New fixtures: a valid split dropped for one empty part, an invalid shortlist keeping one (a shortlist
    round, so that it breaks only this rule and not confirmation), a valid record whose ceiling on exchange-a's
    USDC output drops the best candidate and moves the choice to the next, and an invalid one choosing it; for
    the citation rules, the best route dropped as unconfirmed with its quote left out, a choice leaving a
    judgement of no uncited, a quote at another amount on an operation it doesn't drop, and two usable answers
    disagreeing; for the drop rule, the review's two scenarios (`c1` removed from the one-part split, `c2` chosen,
    and `c3` on `c1`'s path dropped by a stale quote, or by a quote at another amount, beside its usable
    answers), and a `quote_voided` candidate citing a second quote at another amount that its reason doesn't name.
  - Existing fixtures, changed to the citation rule. Every operation now cites every usable statement about its
    tuple at its amount (a script added them). Then:
    - The fixtures' shared snapshot held a judgement of no for `f-ex-a`, which every route through exchange-a
      had left uncited, the chosen ones included. It is removed from every fixture where no candidate used it,
      and kept where one does (the every-drop-reason record and those made from it).
    - In `split-of-one-part`, `c2` cites its answers and is confirmed, so it is `outranked` by `c1` again, as
      T-004-01 had it; the first draft of this task had re-marked it `unconfirmed` for want of the citations.
    - In `split-of-three-parts`, `single` cited its answers would be confirmed and better than the chosen
      split. Its deposit's confirming answer is removed from that snapshot, so it is dropped `unconfirmed` at
      operation 2 and the split stays the choice.
    - In the every-drop-reason record, `c7` was the chosen route with its answers left out. It is now that
      route split 70,000,000 and 30,000,000, priced, unconfirmed at the 70,000,000 withdrawal, which no answer
      covers.
    - The routes dropped by a bad statement beside a usable answer (`c3` of the one-part split and of the
      ceiling-on-output record, `c3` and `c6` of the every-drop-reason record, and the invalid fixtures that copy
      them unchanged) now take the same route split 70,000,000 and 30,000,000. At 69,930,000 COP exchange-a has
      no usable answer, so the bad statement is the only one: a stale quote `f-ex-a-quote-old-70m` and a void
      one `f-ex-a-quote-elsewhere-70m` are added at that amount, and `quote_voided` names `f-ex-a-quote-50m` as
      before. Those records allow two parts.
    - `split-with-a-part-arriving-with-nothing` is built from `split-of-three-parts`, so it lacks
      `f-confirm-dep-in-249250500` too, and its `single` is dropped `unconfirmed` at operation 2 likewise.
    - `choosing-an-unconfirmed-candidate` loses its withdrawal's answer from its snapshot, so the choice is
      unconfirmed for real; `quote-voided-by-a-condition-it-does-not-name` names the quote at its operation's
      amount.
    - Every valid decision record among the fixtures passes the suite (a test checks this).
  - The shortlist rule (no statements in a shortlist snapshot) stays in the suite. A router can always meet it,
    so it needs no contract change.
- **Unchecked, still.** That the record lists every candidate the catalogue allows: the suite recomputes the
  choice among the candidates listed, and the cases' expected routes catch a missing one on the suite's own inputs
  only. The `quote_voided` conditions that rest on facts outside the record are as T-004-01 left them. (The
  unserved mid-route candidate and the shortlist timing, listed here at 0.6.0, are settled by the pre-freeze
  audit below.)

## The orchestrator's pre-freeze audit (router-api 0.7.0, state-machine 0.4.1)

Before the contract freeze (`T-006-01`), the orchestrator audited the open items of `T-004-02` and ruled
(2026-11-07) that three need a contract change now, since after 1.0.0 each would cost a `DEC-*` entry and the
owner's click. They are done as a follow-up to `T-004-02`.

- **One check for the bare typed operation.** state-machine 0.4.0's `SegmentPlan::check` repeated the router's
  unpriced rules, and one had already been found missing from that copy. router-api now exposes
  `check_typed_operation(kind, from, to)` (the ends' provider-role atoms, outer and inner ends, the asset kept,
  two nodes or one) and `check_typed_chain(ends)` (each starts where the one before ended). `Operation::check`
  and `Part::check` call them; the state machine calls them in place of its copy, keeping its own wording for a
  broken chain and its own rule that a route ends with a deposit. state-machine goes to 0.4.1; its fixtures and
  tests keep their results. The router's operations now also check their ends' atoms, as the schemas do.
- **The unserved mid-route candidate (`REQ-AUD-4`, ADR-0006's exhaustive search).** Ruled: a drop reason
  `beyond_levels {part, operation}`. A candidate dropped with it lists that part's operations only up to and
  including the unserved one, whose `amount_out` is 0; that part alone needn't reach the destination. The checker
  verifies the operation is its part's last, arrives with 0, and that its net (amount in less fee) is above its
  cited price's last level. The suite recomputes it as any operation: the fee from the price and, above the last
  level, nothing out. The double now records such a route instead of dropping it from the search. Fixtures: a
  valid shortlist with a route through a shallow exchange, and invalid ones naming an operation before the last,
  stating something out, and naming an operation within its price's levels.
- **Shortlist timing.** Ruled: a choice round's `refines` carries the shortlist round's `taken_at` with its request
  id, and a confirming answer must be made at or after it. A chosen candidate confirmed only by earlier answers
  is refused with its own message. Every choice fixture's `refines` now carries the shortlist fixture's
  `taken_at`, 14:59:50; an invalid fixture moves it past the one-part split's answers.
  - My first draft made every statement made before `taken_at` unusable. Review found that let whoever sets
    `taken_at` hide a refusal or a judgement of no, made before it or between it and the answers. Ruled by the
    orchestrator, 2026-11-07: the cutoff applies to accepted answers only. A refusal or a judgement binds while
    within its validity and not void, whenever it was made. Two invalid fixtures leave such a no uncited.
  - `refines.taken_at` is at or before the choice round's own snapshot (a fixture breaks it). Beyond that it is
    checked against its stated value only: backdating can't be seen from the choice record, and Core ties
    `refines` to the shortlist record it keeps (`REQ-AUD-4`). The README lists this under "Unchecked by
    either".
  - A `beyond_levels` reason's two conditions on the named operation, being its part's last and arriving with
    0, are one rule with one message, so the fixture naming an operation before the last breaks only that.
- **Documented limits, no change.** That the record lists every candidate, and the `quote_voided` conditions that
  rest on facts outside the record, stay as the README states them.

## The M1 security review (router-api 1.0.1, `T-004-02`)

- **M3: decision records can't show who signed.** A record carries each statement whole and checks its `maker`
  against its figure's provider, but the snapshot carries no keys, so `signature.key` is tied to nothing. The
  orchestrator ruled: state the precondition, add no keys. Core ties each provider statement to its provider's
  advertised keys (provider-role 2.0.0: a key listed for `statements`), and the operator's to `operator_key`,
  before a record is formed. This matches the earlier ruling that Core ties `refines` to the shortlist record it
  keeps. It is in the README's "Unchecked by either" and in the Rust docs (`mod.rs`, `check_envelope`,
  `RecordedDecision`).
- **Why a patch.** The README's rule makes a fix a patch, and documentation is a fix: no schema, type or check of
  this contract changed, and every record valid at 1.0.0 is valid at 1.0.1 under the same provider-role version.
  The envelope it embeds is provider-role's, so `Signature.key` and `operator_key` are now `PublicKey`s, but that
  is provider-role 2.0.0's change, bumped and recorded there; the host 0.1.1 bump set the precedent of a patch for
  a contract whose fixtures follow another's.
- **Fixtures.** Every `protocol_version` names provider-role's `2.0.0` (3145 fields), the same anchored
  substitution as at `C1`. No router-api check compares a fixture's protocol version with a `VERSION`: each fixture
  is checked in its own deployment and protocol version, so its tests passed before the rewrite and after.
- **After the delta review, still 1.0.1:** the precondition names which advertisement applies, the provider's
  accepted one current at the statement's `made_at`, not one accepted later, and covers nested statements.

## Review round 1 of the M1 pull request (router-api 2.0.0, `T-004-02`)

- **The fee rounds up (finding 1, `REQ-QTE-5`).** The fee is `flat + ceil(a × n / d)`, and the conversion still
  rounds down once. Rounding the fee down was the unsafe direction, against this contract's own rule for time (ages
  up, validities down): the shortlist ranking read a route up to one unit per operation better than its providers
  may make it, and a provider rounding up was one unit over its advertised formula on any charge that doesn't
  divide evenly. The choice round wasn't affected, since it chooses only on the providers' own confirmed terms.
  - My reading of `REQ-QTE-5`'s "check a charged fee against it": the formula is now the most a charge may come
    to, so the check is charge ≤ formula (or ≤ `maximum` where one is given, as `AC-14` flags), and it holds
    whichever way a provider rounds. Which check flags what belongs to the feature that checks charges; this
    contract only fixes the formula's value.
  - A declared rounding per formula, the review's other option, was not taken. A rounding-up formula is an upper
    bound for a provider rounding either way, so the field would add a choice and no safety.
  - Why 2.0.0: no schema shape, Rust type or `check` changed, but the formula's value did, and the router suite
    (`DEC-5`) enforces it. A router correct at 1.0.2 fails the suite wherever the proportion doesn't divide
    evenly, so by the README's semver rule this is breaking.
  - Fixtures: only `split-2-thin`, in `decision-record-split-with-a-part-arriving-with-nothing`, has a fee that
    doesn't divide evenly on an operation priced from its price. Under rounding up its thin part would have
    stopped at an exchange giving 999,997 USDC units, below the deposit's 1 USDC flat fee, so it couldn't be
    stated at all. The split is re-solved: 401,204 COP units withdrawn (fee 402), 400,802 into exchange-a (fee
    802), exactly 1,000,000 out, and 0 after the deposit; the other part takes 99,598,796. The invalid shortlist
    that keeps `split-2-thin` carries the same amounts, so it still breaks only its one rule.
  - The double rounds up, and a new flaw, `FEE_ROUNDS_DOWN`, is refused by `amounts-follow-their-figures` alone on
    the shortlist case, which has a fee that doesn't divide evenly. The old unit test's 1/3 example now expects
    1,000,004, not 1,000,003: a deliberate flip, since the value it pinned is the one the finding shows unsafe.
- **Made fixtures (finding 11).** The 127 invalid catalogue-snapshot and decision-record fixtures are each a valid
  fixture of their shape with an RFC 6902 patch, in `fixtures/patches/`: 4,006 lines of patches beside about
  237,000 of fixtures. `contracts/tests/router-api.rs` applies each patch and compares the result with the
  committed fixture as JSON values, and refuses an invalid fixture of those shapes with no patch. It runs with the
  contract's other tests, so CI needs no new step.
  - Values, not bytes: `serde_json` here has no `preserve_order`, so a remade file comes out with its keys sorted.
    Turning `preserve_order` on would unify across the workspace and could change provider-role's canonical
    form, so it is left off. The committed fixtures keep their formatting; a fixture remade with
    `RUTENT_WRITE_FIXTURES=1` is written sorted, and only where it differs.
  - Each patch's base is the valid fixture of its shape nearest to it, found when the patches were first drawn
    up from the committed files; from now on the patch is the source.
  - The small hand-written families (requests, policy bounds, operations, candidates) aren't made: each is a few
    lines, and a patch would be no shorter.
