# router-api changelog

## 2.0.0
- Breaking, after review round 1 of the M1 pull request: a fee formula's proportion rounds up, so the fee is
  `flat + ceil(a × numerator / denominator)`, and the output still rounds down. Before, it rounded down, which ranked
  a route up to one unit per operation better than its providers may make it, and put a provider rounding up over
  its own formula. A router computing 1.x's fee fails the router suite wherever the proportion doesn't divide
  evenly. `split-2-thin`, in `decision-record-split-with-a-part-arriving-with-nothing` and in the invalid shortlist
  that keeps it, is re-solved so its thin part still arrives with exactly 0; the router suite's double rounds up,
  and a flaw rounding down is refused. (REQ-QTE-5, DEC-4, DEC-5, DEC-20, T-004-02)
- The invalid catalogue-snapshot and decision-record fixtures are made: each is a valid fixture of its shape with an
  RFC 6902 patch in `fixtures/patches/`, and a test refuses any that isn't, or has no patch. Every fixture's
  content is unchanged. (DEC-4, DEC-20, T-004-02)

## 1.0.2
- No change to this contract's shapes or types: its fixtures name provider-role 2.0.1 as their `protocol_version`,
  after provider-role's fix to its newer-than check. (DEC-4, DEC-20, T-002-03)

## 1.0.1
- No change to this contract's shapes or types, after the M1 security review: the README's "Unchecked by either"
  and the Rust docs state, as the orchestrator ruled, that nothing in a decision record ties a statement's
  `signature.key` to its maker, since the snapshot carries no keys, and that Core ties each provider statement,
  nested ones included, to a `statements` key of its provider's advertisement current when it was made, and the
  operator's to `operator_key`, before a record is formed. The
  envelope it embeds is provider-role 2.0.0's, by `$ref` and in Rust, so its keys are `PublicKey`s compared by their
  bytes; the fixtures name protocol version 2.0.0. (DEC-4, DEC-20, T-004-02)

## 1.0.0
- Frozen at C1: from here a change needs a version bump, a changelog entry naming a DEC-*, and the owner's click. (DEC-3, DEC-4, DEC-20, T-006-01)

## 0.7.0
- `check_typed_operation` and `check_typed_chain` check a bare typed operation and a chain of them, with no
  prices; `Operation::check` and `Part::check` call them, and so does the state machine's segment plan, so the
  rules are stated once. An operation's ends now have their provider-role atoms checked in Rust too.
  (DEC-4, T-004-02)
- A drop reason `beyond_levels {part, operation}`: the part is listed up to and including an operation whose net
  is above its price's last level, which arrives with 0, and needn't reach the destination; the checker
  verifies the net is beyond the levels. (DEC-4, T-004-02)
- A choice round's `refines` is an object, the shortlist round's request id and its snapshot's `taken_at`, at or
  before the choice round's own snapshot. A confirming answer is made at or after that stated time, and an
  accepted answer made before it is not usable in the choice round; a refusal or a judgement binds whenever it
  was made. That the stated time is the shortlist's real one is Core's, which keeps the shortlist record.
  (DEC-4, T-004-02)

## 0.6.0
- A candidate's outcome is judged per part: a kept candidate arrives with something in every part, and
  `no_positive_outcome` holds when any one part arrives with nothing, so a split with one empty part has a reason
  that fits it. (DEC-4, T-004-02)
- A ceiling counts the value an operation produces as well as what it takes in: an operation puts its
  `amount_in` in its from asset and, where its to asset is another, its `amount_out` in its to asset, each
  operation once per ceiling. (DEC-4, T-004-02)
- What this contract leaves unchecked, the amounts recomputed from prices and the ranking, is checked by the
  router suite (`rutent-conformance`'s `router`, `DEC-5`). The split fixtures drop their unconfirmed alternative
  as `unconfirmed`, not `outranked`, so every valid decision record also passes the suite; new fixtures cover a
  split with one empty part and a ceiling passed by what an exchange produces. (DEC-4, DEC-5, T-004-02)
- Every operation cites every usable fresh quote and dry run about its tuple for its amount in, and every usable
  judgement about its tuple, so a confirming answer, a refusal or a judgement of no can't be left out; a fresh
  quote it cites is for its amount in, unless a `quote_voided` reason for `amount_changes` names it; usable
  accepted answers for one tuple and amount agree on their terms. The fixtures cite accordingly: the shared
  judgement of no for `f-ex-a` is kept only where a candidate uses it, and `c2` of the one-part split is outranked
  again. (DEC-4, T-004-02)
- A `stale_figure`, `void_statement` or `quote_voided` reason that names a statement holds only where no
  operation citing it also cites a usable accepted answer for its tuple at its amount in, so an extra bad
  statement can't drop a confirmed route. The fixtures that dropped a route that way take it split 70,000,000
  and 30,000,000, where the bad statement is the only answer. (DEC-4, T-004-02)

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
