# router-api changelog

## 0.5.0
- A snapshot holds one tuple figure per provider and ends, and at most one price per tuple, so a copy can't dodge
  the agreement rules or the levels; all judgements in one snapshot about one relying
  provider's tuples are about one subject (ADR-0012).
  (DEC-4, T-004-01)
- An operation cites each figure once; a fresh quote meeting a refused dry run for one tuple and amount has its
  own fixture. (DEC-4, T-004-01)

## 0.4.0
- Every operation cites one price, for its own tuple, and any dry run it cites is for its amount in; a dry-run
  figure records the amount it asked about. (DEC-4, T-004-01)
- A snapshot gives every provider one publisher, exposures included, and its answers agree: no accepted and
  refused dry runs for one tuple and amount, no yes and no judgements of one subject for one tuple. The
  every-drop-reason fixture no longer contradicts itself. Recomputed drops are documented as consistent with the
  stated amounts only; recomputing amounts from prices is deferred to `T-004-02`. (DEC-4, T-004-01)

## 0.3.0
- A figure is bound to its operation's tuple: every operation cites one tuple, and every price, fresh quote, dry
  run or judgement it cites names that tuple, so a drop's figure is cited where it applies. A confirming answer
  equals its operation's amount in, fee and amount out. (DEC-4, T-004-01)
- A kept candidate cites no refused dry run, no judgement of no, no void or stale figure; arrives at or above the
  floor and with something; passes no ceiling; uses nothing excluded; keeps within the part and hop limits and
  its tuples' bands. `below_floor`, `no_positive_outcome`, `over_ceiling`, `excluded_by_policy` and `outside_band`
  are recomputed; what stays unchecked is documented. (DEC-4, T-004-01)

## 0.2.0
- Drop reasons are supported by what they cite: a reason that names a figure names one its candidate cites, and
  the figure bears it out (stale, void, a quote that names the condition, a refused dry run, a judgement of no).
  (DEC-4, T-004-01)
- The choice round chooses only a candidate whose every operation is confirmed by an accepted dry-run answer of
  its provider for its tuple, at its amount, and drops any other as `unconfirmed`; every operation cites a tuple
  of its own provider with its two ends; ceilings and exposure match on asset id and decimals. (DEC-4, T-004-01)

## 0.1.0
- The router API: the request with objective and floor, the catalogue snapshot with each figure's age and
  origin, the quote as a provider's statement in the envelope, the policy bounds, candidates with drop reasons,
  the chosen split of N parts, each an ordered list of typed operations, and the decision record in the envelope,
  with the `route(request, snapshot, bounds)` trait, schemas and fixtures. (DEC-4, DEC-8, T-004-01)
- Tightened after review: ceilings per provider and per publisher in the policy bounds, in-flight exposure in the
  snapshot, a publisher and module version on each provider origin, and an `over_ceiling` reason, a split's parts
  counted together; the deployment and protocol version that void a statement are among the bounds; ages round
  up and validities down; every `check` recurses; exact rounding rules for fees, rates and levels.
  (DEC-4, DEC-8, T-004-01)
- Two rounds, a shortlist then a choice that refines it, with fresh-quote, dry-run and eligibility figures and
  their drop reasons; the objective is a tagged object, `fastest_settlement` the slowest part's time, and ties go
  to the lowest `candidate_id`; a quote's fee is checked as an operation's, `voided_by` has a vocabulary with a
  `quote_voided` reason; quote, envelope and slippage fields follow the provider-role names; integers stay
  within 2^53 - 1; each schema-rejected fixture names its exact errors. (DEC-4, DEC-8, T-004-01)
- Reconciled with provider-role 0.3.0: the envelope, fee, rate, timestamp, decimal, safe integer and void
  conditions are provider-role's, by `$ref` and in Rust; a fresh quote or dry run is the provider's signed result
  and eligibility a verifier's signed judgement, each carried whole; voidness is decided by `check_context`; the
  local placeholders and quote, dry-run and eligibility schemas are gone. The typed operation is this contract's,
  at `operation#/$defs/operation`, standing alone. (DEC-4, T-004-01)
