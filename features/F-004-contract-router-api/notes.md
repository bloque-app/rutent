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
- **Rounding of fees and rates (`REQ-QTE-5`; review fix).** The fee is `flat + floor(a × n / d)`, counted at
  `maximum` where one is given. The conversion rounds down once, after summing the levels. A net amount above the
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

