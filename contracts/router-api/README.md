# router-api contract

The router API (`DEC-4`, `DEC-8`, ADR-0006, SAD §3): `route(request, catalogueSnapshot, policyBounds) →
{candidates, chosen, decisionRecord}`, a pure function. The Rust types in `contracts/src/router_api/` are the API,
and their records serialise to the schemas here. The version is the first heading of `CHANGELOG.md`.

| Shape | Schema `$id` | Rust |
|---|---|---|
| Shared atoms | `urn:rutent:router-api:common` | `Endpoint`, `Ratio`, `DeclaredCall`, `RouteCall`, `CallKind` |
| Request | `urn:rutent:router-api:request` | `Request` |
| Catalogue snapshot | `urn:rutent:router-api:catalogue-snapshot` | `CatalogueSnapshot` |
| Policy bounds | `urn:rutent:router-api:policy-bounds` | `PolicyBounds` |
| Typed operation: kind and ends | `urn:rutent:router-api:operation#/$defs/operation` (kind: `#/$defs/kind`) | `OperationKind` |
| Priced operation, which extends it | `urn:rutent:router-api:operation` | `Operation` |
| Candidate | `urn:rutent:router-api:candidate` | `Candidate` |
| Decision record, in the envelope | `urn:rutent:router-api:decision-record` | `RecordedDecision` |

## What it refers to, never copies

- From provider-role: identifiers, versions, amounts, safe integers, timestamps, decimals and protocol versions
  (`common`); inner and outer nodes, denominations and the reserve, exchange and ramp tuples (`advertisement`);
  the fee, the rate and the void conditions (`result`); the statement envelope (`statement-envelope`), whose
  `deployment` the policy bounds use by its `#/properties/deployment` pointer; and, carried whole and unchanged
  so their signatures still verify, a provider's signed result for a fresh quote (`transfer_quote` or
  `conversion_quote`) or a dry run (`result`), and a verifier's signed judgement for eligibility (`judgement`).
  In Rust these are provider-role's own types: `Statement<ResultBody>`, `Statement<Judgement>`, `Fee`,
  `VoidedBy`, `SafeInteger`, `Deployment`, with `check_shape` and `check_context`.

## The typed operation

This contract owns the typed operation of `REQ-ORD-37` (ruled by the orchestrator, 2026-11-06): its kind,
withdrawal, route, exchange or deposit and no other (`REQ-PRT-5`), and its two ends, each a provider-role node and
denomination. `urn:rutent:router-api:operation#/$defs/operation` is open, so that a schema embedding it, such as
the state machine's segment plan, adds its own fields and closes it with `unevaluatedProperties: false`; the
router's priced operation does just that.

## The calls a route makes

Since 3.0.0 (`T-011-06`, `REQ-MDL-21`, `THR-86`) each segment of a route names the cross-module calls it makes.
The platform lets a module call another provider's module only where its release declares the call and a segment of
its route names it (`judge_call`, `T-014-04`), so the route is what bounds a module to the providers it takes.

- **The source.** A `calls` figure in the snapshot states what a provider's admitted module declares, as its
  release's `outside.calls` lists them: each callee once, with the kinds that may be asked of it. It is the
  provider's, its origin's `module_version` naming the release, and it states no validity, as an advertisement
  doesn't. At most one per provider.
- **The kind** is the module interface's: a provider-role operation kind (`withdrawal`, `transfer`,
  `balance_verification`, …), as `validate_call_against` and the call record use it, by `$ref` to
  `urn:rutent:provider-role:operation#/$defs/operation/properties/kind`; in Rust, `CallKind`, the module
  interface's `OperationKind`. It is not the typed operation's kind, and it is never `holding_balance`, the
  operator's own question to a holding, which no module may call for.
- **The segment's calls.** An operation's `calls` lists `{callee, kind}` pairs, each once, none to its own provider,
  and is absent when it names none. Each is declared in the one calls figure the operation cites, of the release its
  tuple comes from (same provider and `module_version`), and its callee is the provider of another operation of the
  same part. An operation cites a calls figure exactly when it names a call.
- **What a router names.** The contract refuses any call outside that; the router suite checks a router names every
  call it allows, every kind declared for every callee on the part, on every candidate.

## Two rounds

Core calls `route` twice per routing (ruled by the orchestrator, 2026-11-06). The shortlist round runs over
advertised figures and chooses nothing; the candidates it keeps are the shortlist. The choice round runs over a
snapshot that adds fresh quotes, dry runs and eligibility judgements for the shortlisted providers only
(`REQ-CAT-5`), names the shortlist round it refines, and chooses at most one candidate. Both records are kept
(`REQ-AUD-4`). `refines` carries the shortlist round's request id and its snapshot's `taken_at`; in the choice
round an accepted answer made before that time is not usable, so it confirms nothing; a refusal or a judgement
binds whenever it was made, so no choice of `taken_at` hides bad news.

## Computing, exactly

So that anyone can recompute a fee and a choice (`REQ-QTE-5`, `THR-10`):

- **Fee.** On an amount `a`, `flat + ceil(a × numerator / denominator)`; where a `maximum` is given, route
  evaluation counts the fee at the maximum. The fee rounds up and the output down, so a route is never evaluated
  as better than its providers may make it, and a charge rounded either way is within its formula.
- **Conversion.** The net `n = a − fee` fills the levels in order; the output is
  `floor(Σ take_i × rate_i × 10^to.decimals / 10^from.decimals)`, rounded once. A fixed rate is one unbounded
  level; a net amount above the last level isn't served (`outside_band`); with no rate,
  `floor(n × 10^destination.decimals / 10^origin.decimals)`.
- **Validity.** Ages round up and validities round down, so a figure whose age is at most its validity is never
  past its `valid_until`.
- **Confirmation.** The choice round chooses only a candidate whose every operation cites an accepted dry-run
  answer of its provider for its tuple, at its amount, within its validity and not void: the fresh quote for a
  route or exchange, the reserve's accepted dry run for a withdrawal or deposit. Any other is `unconfirmed`.
- **Citations.** Every operation cites every usable statement (within its validity, not void) about its tuple
  that bears on it: each fresh quote and dry run for its amount in, and each eligibility judgement. So no
  confirming answer, refusal or judgement of no can be left out. A fresh quote it cites is for its amount in,
  except the quote a `quote_voided` reason for `amount_changes` names; and usable accepted answers for one tuple
  and amount agree on their fee and amount out. A `stale_figure`, `void_statement` or `quote_voided` reason that
  names a statement holds only where no operation citing it also cites a usable accepted answer for its tuple at
  its amount in, so a bad statement added beside a good answer can't drop a route.
- **Ceilings.** A ceiling matches on provider or publisher and on asset id and decimals. A candidate's value at a
  provider, in a ceiling's asset, is the sum over its operations there, across all parts, of each one's
  `amount_in` where its from asset is the ceiling's, or else its `amount_out` where its to asset is, so value an
  exchange produces counts too; plus the snapshot's exposure there. At a publisher, the same over its providers.
- **Outcome.** A candidate arrives with something only if every part does: `no_positive_outcome` holds when any
  one part arrives with nothing (`REQ-QTE-13`).
- **Objective.** `lowest_total_cost` wants the largest sum of the parts' last `amount_out`; `fastest_settlement`
  the smallest slowest-part time, a part's time being the sum of its operations' declared settlement seconds.
  Ties go to the lowest `candidate_id`, comparing bytes.
- **Void statements.** A statement whose envelope names another deployment or protocol version than the bounds'
  is void (`void_statement`), as provider-role's `check_context` decides; the chosen candidate uses none.

## Recomputing from the record

The decision record holds the request, the snapshot and the bounds verbatim, and the router's version. Every time
the router reads is in it: the snapshot's `taken_at`, and each figure's age and validity in whole seconds. So
`route(&record.request, &record.snapshot, &record.bounds)` gives the record again, with no provider call (`AC-2`,
`REQ-AUD-4`). The router returns the record; the deployment wraps it in its envelope and signs it when it records
it, since a pure function can neither sign nor read a clock.

## Rules and where they are enforced

Deserialising an `Operation`, `Part`, `Candidate`, `PolicyBounds`, `CatalogueSnapshot` or `DecisionRecord` runs
its `check`, so Rust accepts no invalid fixture. Every `check` recurses through what it holds. A value built field
by field is checked only when `check` is called: `route`'s inputs must pass their checks, and a `Router` returns
only records that pass theirs, so a stored record recomputes.

| Rule | Schema | Rust |
|---|---|---|
| Identifiers, versions, amounts, decimals and timestamps keep their provider-role patterns | `pattern` | `Id`, `Version`, `Amount`, `PositiveAmount`, `Decimal`, `Timestamp` |
| A timestamp is UTC, seconds `00` to `59`, on a real date | `pattern` | `Timestamp` (also refuses `02-30`) |
| An optional field, when present, holds a value, never `null` | `type` | `deserialize_with = "some"` |
| No unknown fields, on any object or kind | `additionalProperties` | `deny_unknown_fields` |
| A list required non-empty or distinct is so | `minItems`, `uniqueItems` | `NonEmpty`, `Distinct` |
| Every figure has an age and an origin (`REQ-QTE-4`) | `required` | field type |
| A quote has a validity, its envelope, its direction, what voids it and its settlement time (`REQ-QTE-1`, `REQ-QTE-3`) | `required`, `if`/`then` | field type, `CatalogueSnapshot::check` |
| A quote's rate and slippage regime come together | `dependentRequired` | `CatalogueSnapshot::check` |
| A quote's envelope names the quote shape; a record's, the record shape | `const` | `check_shape`, `CatalogueSnapshot::check` |
| An operation is typed, and only with the four kinds (`REQ-ORD-37`, `REQ-PRT-5`) | `required`, `enum` | `OperationKind` |
| A withdrawal goes outer to inner, a deposit inner to outer, a route and an exchange at inner nodes | `if`/`then` | `Operation::check` |
| A withdrawal, route or deposit keeps its asset; a route's nodes differ; an exchange stays at one node | can't be stated | `Operation::check` |
| An operation's fee is in its from asset and at most its amount in | can't be stated | `Operation::check` |
| Each operation of a part starts where the one before ended, with what it delivered | can't be stated | `Part::check` |
| A drop reason's indices name an operation; no candidate is outranked by itself | can't be stated | `Candidate::check` |
| A `no_price`, `fee_above_amount` or `nothing_delivered` reason names its part's last operation, which arrives with 0, stating a fee of 0 (`no_price`) or its amount in (`fee_above_amount`); that part alone needn't reach the destination, and a `nothing_delivered` operation doesn't end there | can't be stated | `Candidate::check`, `DecisionRecord::check` |
| A `no_price` operation cites its tuple and no price, and the snapshot holds no price for that tuple; a `fee_above_amount` operation cites no usable accepted answer at its amount in | can't be stated | `DecisionRecord::check` |
| Figure ids are unique; a price or quote names a tuple figure from the same provider, and states a rate exactly when the tuple is an exchange's; levels increase | can't be stated | `CatalogueSnapshot::check` |
| Candidate ids are unique; only the chosen candidate lacks a drop reason; every part runs from the request's origin to its destination; parts add up to the request's amount; every figure and candidate named is in the record | can't be stated | `DecisionRecord::check` |
| Every figure's origin names provider, publisher, provider version and module version, or the operator's source; exposure alone comes from the operator | `required`, `if`/`then` | `Origin`, `CatalogueSnapshot::check` |
| One ceiling per provider or publisher and asset; one exposure per provider and asset | can't be stated | `PolicyBounds::check`, `CatalogueSnapshot::check` |
| A statement's envelope names its shape and its maker is its figure's provider | `const` | `check_shape`, `CatalogueSnapshot::check` |
| A fresh quote is a transfer quote for a ramp's tuple or a conversion quote for an exchange's; a dry run answers for its tuple's kind of operation | `enum`, `const` on kind and status | `CatalogueSnapshot::check` |
| A fresh quote's fee is in its tuple's input asset and at most its amount in; it names each void condition once | `uniqueItems` for the last | `CatalogueSnapshot::check` |
| Every operation cites one tuple, of its own provider with its two ends, and every other figure it cites (price, fresh quote, dry run, judgement) names that tuple | can't be stated | `DecisionRecord::check` |
| A kept candidate (chosen, or on the shortlist) cites no void statement (by `check_context`), no figure past its validity, no refused dry run and no judgement of no | can't be stated | `DecisionRecord::check` |
| A kept candidate arrives at or above the floor and with something in every part, passes no ceiling (counting what its operations produce as well as what they take in), uses no excluded provider or tuple, keeps within the part and hop limits, and has every operation within its tuple's bands and its price's levels | can't be stated | `DecisionRecord::check` |
| The choice round chooses only a candidate whose every operation is confirmed: an accepted dry-run answer of its provider for its tuple with its amount in, fee and amount out; an `unconfirmed` reason names an unconfirmed operation | can't be stated | `DecisionRecord::check` |
| A drop reason that names a figure names one its candidate cites, and the figure supports the drop: stale past its validity, void by `check_context`, a refused dry run, a judgement of no; `quote_voided` names a quote that lists the condition, past its validity for `validity_passes` and at another amount for `amount_changes`, and is accepted on the listed condition alone for the others, which rest on facts outside the record | can't be stated | `DecisionRecord::check` |
| `below_floor`, `no_positive_outcome` (any one part arriving with nothing), `over_ceiling` (with the exposures), `excluded_by_policy` and `outside_band` hold of the candidate they drop, and fail of a kept one. These recompute from the operations' stated amounts and fees, so they show the record's stated amounts consistent with its drops, not with the prices it cites | can't be stated | `DecisionRecord::check` |
| Every operation but the one a `no_price` reason names cites one price, for its own tuple, and any dry run it cites is for its amount in | can't be stated | `DecisionRecord::check` |
| Every operation cites every usable fresh quote and dry run about its tuple for its amount in, and every usable judgement about its tuple; a fresh quote it cites is for its amount in, unless a `quote_voided` reason for `amount_changes` names it | can't be stated | `DecisionRecord::check` |
| Usable accepted answers (fresh quotes included) for one tuple and amount agree on their fee and amount out | can't be stated | `DecisionRecord::check` |
| A `stale_figure`, `void_statement` or `quote_voided` reason that names a statement holds only where no operation citing it also cites a usable accepted answer for its tuple at its amount in | can't be stated | `DecisionRecord::check` |
| Every provider has one publisher across the snapshot, exposures included | can't be stated | `CatalogueSnapshot::check` |
| One tuple figure per provider and ends, and at most one price per tuple figure, so a copy can't dodge the agreement rules or the levels | can't be stated | `CatalogueSnapshot::check` |
| All judgements in one snapshot about one relying provider's tuples are about one subject: the subject reference is the relying provider's (ADR-0012), and the relying provider is the provider of the tuple a judgement names, not the verifier | can't be stated | `CatalogueSnapshot::check` |
| An operation cites each figure once | `uniqueItems` | `Operation::check` ("duplicate figure") |
| An operation names each call once and none to its own provider; a present `calls` is never empty; a call's kind is a provider-role operation kind, never `holding_balance` (3.0.0) | `uniqueItems`, `minItems`, `enum` and `not` for the first, the third and the fourth | `Operation::check`, `non_empty_list`, `CallKind` |
| A calls figure is a provider's, states no validity, lists each callee once and each kind once per callee, declares no call for `holding_balance`, and names no call to its own provider; a snapshot holds at most one per provider (3.0.0) | `const`, `not`, `uniqueItems` and `not` for the first, the second and the kinds | `CatalogueSnapshot::check` |
| An operation cites one calls figure exactly when it names a call: its own module release's, by provider and `module_version`; each call it names is declared there, and is to the provider of another operation of its part (3.0.0, `REQ-MDL-21`) | can't be stated | `DecisionRecord::check` |
| A bare typed operation's rules (ends' atoms, outer and inner ends, asset kept, two nodes or one) and a chain's continuity, checked once for the router's operations and the state machine's segment plans | can't be stated | `check_typed_operation`, `check_typed_chain` |
| A `beyond_levels` reason names its part's last operation, which arrives with 0 and whose net is above its price's last level; that part alone needn't reach the destination | can't be stated | `Candidate::check`, `DecisionRecord::check` |
| A choice round's `refines` carries the shortlist round's `taken_at`, at or before its own snapshot's; a confirming answer is made at or after it, and an accepted answer made before it isn't usable, while a refusal or a judgement binds whenever made | `required` | `Refines`, `DecisionRecord::check` |
| Within one snapshot, dry-run answers (fresh quotes included) for one tuple and amount agree, and so do judgements of one subject for one tuple; an accepted dry run states the amount its figure records | can't be stated | `CatalogueSnapshot::check` |
| A statement figure's age is its `made_at` to `taken_at`, rounded up; its validity, to `valid_until`, rounded down, and absent for a refused dry run | can't be stated | `CatalogueSnapshot::check` |
| A shortlist round chooses nothing; a choice round names the round it refines | `not`, `required` | `Round`, `DecisionRecord::check` |
| Integers are plain JSON integers, such as `2`, not `2.0`, and at most 2^53 - 1 | `maximum`; JSON Schema's `integer` accepts `2.0` | `SafeInteger`, `u32`, `NonZeroU8` |

Rules this contract doesn't check. It checks a decision record's structure and cross-references (ruled by the
orchestrator, 2026-11-06). The router suite, `rutent-conformance`'s `router` (`T-004-02`, `DEC-5`), recomputes
the rest from the record alone; see `conformance/README.md`:

- That operation amounts match their figures: here each operation's fee and `amount_out` are taken as stated. The
  suite recomputes them from every usable accepted answer the operation cites for its amount in, or else from its price.
  For the operation a `fee_above_amount` reason names, it checks that the price's fee on the amount in is above
  it; the operation a `no_price` reason names has no price to recompute from.
- That the objective is served: here `outranked` is checked only for naming a candidate of the record, and
  `suspect_rate_cycle` not at all. The suite recomputes the ranking, checks that each outranked candidate could
  have been the choice, and that `suspect_rate_cycle` names a candidate that comes back somewhere with more.
- That a shortlist round's snapshot holds advertised figures only. The suite checks it.
- That a segment names every call its provider declares to another provider of its part (3.0.0): here only that it
  names none outside them. The suite recomputes the calls from the calls figure.

Unchecked by either:

- That the conditions of `quote_voided` other than `validity_passes` and `amount_changes` hold: they rest on facts
  outside the record.
- That the record lists every candidate: a router that leaves out a better route isn't caught by its record. Since
  3.0.0 the contract can state every path a search reaches that ends anywhere: those through a tuple with no
  price, an operation whose fee is above what reaches it, or one that delivers nothing short of the destination
  included, so a router has no reason to leave one out; the router suite's cases name the ones a router must list
  on its own inputs.
- That `refines.taken_at` is the shortlist round's real time. The contract checks answers against the stated
  value, and that it is at or before the choice round's own snapshot; a backdated or postdated value can't be
  seen from the choice record. Core ties `refines` to the shortlist record it keeps (`REQ-AUD-4`).
- Who signed each statement (the M1 security review's M3, ruled by the orchestrator: state the precondition, add no
  keys). A record carries each fresh quote, dry run and eligibility judgement whole, and the contract checks that
  its `maker` is its figure's provider, but the snapshot carries no keys, so nothing in a record ties a statement's
  `signature.key` to its maker. **Precondition:** before a record is formed, Core ties each provider statement it
  carries to a key listed for `statements` in its provider's accepted advertisement that was current when the
  statement was made (its `made_at`), not one accepted later (provider-role's "Key purposes"), and the operator's own
  statements, the recorded decision among them, to the deployment's `operator_key`. Nested statements are covered
  too: a judgement inside a dry run or an eligibility figure, and any statement carried inside another, is tied the
  same way to its own maker. Keys
  compare by their decoded bytes (`PublicKey`). As with `refines`, this is Core's: the record can't show it, and a
  reader of a record alone relies on it.

Recorded since 0.7.0 (the orchestrator's pre-freeze audit, 2026-11-07): a candidate with an operation whose net is
above its price's last levels mid-route is listed up to that operation, which arrives with 0, and dropped as
`beyond_levels`; the contract checks the net is beyond the levels, and the suite recomputes the operation. A
confirming answer is checked to be made at or after the `taken_at` that `refines` states, against that stated
value only.

Recorded since 3.0.0 (ruled by the owner, 2026-11-09, item `aud4-unstateable-candidates`): a path through a tuple
with no price is listed up to that operation and dropped as `no_price`, and one through an operation whose fee is
above what reaches it, as `fee_above_amount`, and one through an operation that delivers nothing short of the
destination, as `nothing_delivered`, each shaped as `beyond_levels`, so a decision record names every path the
search reaches that ends anywhere (`REQ-AUD-4`). `DropReason` is `#[non_exhaustive]` from 3.0.0, which spares Rust
callers a broken match; the contract is language-neutral, though, and a reader on an older schema rejects a reason it
doesn't know, so a new wire value is still a major under `DEC-4`.

## Tests

`contracts/tests/router-api.rs` checks that every valid fixture passes the schemas and round-trips through Rust
unchanged; that every invalid fixture its `SCHEMA_RULES` table lists is rejected by the schemas with exactly the
(location, keyword) errors named there, and every one its `RUST_RULES` table lists is accepted by the schemas and
refused by Rust with the named rule; that a record built in Rust is checked whole; that Rust refuses every invalid fixture, with an
empty allow-list; that every patched invalid fixture is its base with its patch, as below; and that `VERSION` matches
the changelog.

## Made fixtures

The invalid catalogue snapshots and decision records are made, not copied. Each has a source,
`fixtures/patches/<name>.json`, holding `base`, the name of a valid fixture of the same shape, and `patch`, an RFC 6902
patch of `add`, `remove` and `replace` operations; `fixtures/invalid/<name>.json` is that base with that patch. The patch
shows what sets the fixture apart from a valid record, the rule it breaks and any setup that rule needs, so a reviewer
reads it rather than the whole record.

- The test refuses an invalid fixture of those shapes with no patch, a patch with no fixture or a missing base, a patch
  that changes nothing, and a committed fixture that isn't its base with its patch, compared as JSON values.
- To change one, edit its patch, or its base, and run the router-api tests with `RUTENT_WRITE_FIXTURES=1`. That rewrites
  each made fixture that differs, with its keys sorted, and leaves the others byte for byte.
- The other invalid fixtures (requests, policy bounds, operations, candidates) are a few lines each and hand-written.
