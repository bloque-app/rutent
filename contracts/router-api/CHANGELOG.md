# router-api changelog

## 4.0.0
- A hold is bound to its decision record (the review of round 3, item 1). `HeldQuote::check_hold` compared only
  the candidate's id, and ids such as `c1` recur across records, so a hold of one record's `c1` passed for another's
  at the same reserve within its validity. It now takes the record's id and digest, as core-internal's
  `decision_record` entry names them (`check_hold(hold, record_id: &Id, record_digest: &module_interface::Digest)`),
  and refuses a hold whose `record` names another. Folded into this unreleased major. A provider-role fixture,
  `hold-of-another-records-candidate-of-the-same-id`, refused against the choice round's held quote; `hold-for-an-order`
  admitted. (DEC-4, DEC-20, REQ-QTE-15, THR-127, T-020-01)
- Stated terms and held quotes from signed statements (round 3 of the contract, item q, with provider-role 3.1.0's
  `terms` and `hold`). `StatedTerms::from_statement(statement, received_at)` builds a history entry from a
  provider's signed terms, taking only its tuple's provider's, with a slippage regime exactly where the tuple sets a
  rate, and states it at the later of `made_at` and Core's `received_at`; `StatedTerms::stated_at` is now that time,
  when the terms bind from, so the router's `binding`, which filters on it, no longer binds a segment on terms dated
  before it started but received after. `HeldQuote::check_hold(hold)` holds a reserve's signed hold to the chosen
  candidate, its maker to the reserve each part starts at, its validity to the earliest expiry of the candidate's
  figures, and the quote to no longer than the hold. Both new and public. Folded into this unreleased major; the
  `stated-terms` schema's `stated_at` says the same. Tests read provider-role's fixtures. (DEC-4, DEC-20, REQ-QTE-6,
  REQ-QTE-15, T-020-01, T-020-07)
- Breaking: an operation's `amount_out` is what arrives by it, and confirmation reads its tuple's terms (round 3 of
  the contract, item r). The confirmation rule required an operation's `amount_out` to equal its provider's answer
  exactly, while the floor, each part's chain (the next `amount_in` is the previous `amount_out`) and the
  `lowest_total_cost` objective read it as what arrives; under a tuple whose terms report `gross` the two differ by
  the charge, so no valid record could rank, or judge the floor, on what arrives. Confirmation now reads the tuple
  figure's `terms.reporting`: under `gross`, a same-asset operation (a withdrawal, a route or a deposit) states the
  answer's `amount_out` less its charge, the fee restated in its out unit (provider-role 3.1.0's
  `less_the_charge`); an exchange's output stays as its answer states it; under `net` nothing changes. A new public
  method, `Operation::stated_out(tuple, answer_out, answer_fee)`, which the router suite's
  `amounts_follow_their_figures` reads too. The router's search is unchanged here (`T-011-07`). Folded into this
  unreleased major. A valid fixture, `decision-record-choosing-a-gross-deposit-stated-as-what-arrives`, and an
  invalid one made from a patch, `decision-record-choosing-a-gross-deposit-stated-as-its-answer`. (DEC-4, DEC-5,
  DEC-20, REQ-CAT-11, REQ-QTE-11, REQ-ORD-27, T-020-01)
- A held quote never outlives what its quote rests on: `HeldQuote::check` refuses a `valid_until` later than the
  earliest expiry of the figures the chosen candidate cites, each at the snapshot's `taken_at` plus its validity
  less its age (`Figure::expires_at`, `DecisionRecord::earliest_expiry`, both new and public; a figure that states no
  validity caps nothing). Before it, a hold could run past the quotes it was priced on (round 3 of the contract,
  item v). Folded into this unreleased major. The valid fixture's hold now ends at its quotes' earliest expiry,
  15:01:55; a new invalid fixture, `held-quote-valid-until-past-its-quotes-figures`, a second later. (DEC-4, DEC-20,
  REQ-QTE-15, REQ-QTE-4, T-020-01)
- An operation's `fee` is provider-role's fee by reference, so with provider-role 3.1.0 it may carry the optional
  `billed_separately`, what the provider stated of billing it separately. The router never sets it. Folded into this
  unreleased major. (DEC-4, DEC-20, REQ-AUD-11, T-020-09)
- Judgements agree by tuple, subject and class: two of one subject for one tuple, one yes and one no, are refused
  only where they judge one class. A tuple whose provider's rules need two classes may hold a yes for one and a no
  for the other, so the no reaches the record and drops its candidate as `ineligible` (`REQ-AUD-4`, `REQ-CPL-6`).
  0.4.0's rule compared subjects alone, the orchestrator's reading, changed on the coordinator's word on
  `T-020-06`'s review. Folded into this unreleased major; a new valid fixture,
  `catalogue-snapshot-judgements-of-two-classes-disagreeing`. (DEC-4, DEC-20, T-020-06)
- A major of its own, not folded into 3.0.0: 3.0.0 was released into `develop` with `M2` (pull request #60, merged
  as `fb883e0`) while this change was on `M3`'s branch, so the contract guard reads 3.0.0 as released and the change
  below, new required fields and a new rule, needs a bump. Same content as before. (DEC-4, DEC-20, T-020-02)
- Breaking: the inputs Core keeps beside a decision record, so it reaches them through the contract (`DEC-2`,
  `REQ-MOD-3`) and the router's quote rules take them from here. `PolicyBounds`
  gains two required fields, so a record names the bounds it ran under (`REQ-AUD-4`): `operator_fee`, the
  operator's own fee formula (`REQ-QTE-2`), taken at the destination in the request's `to` unit from what the
  providers deliver, capped by its maximum and never raised to it, its proportion at most one; and
  `slippage_limits`, the slippage the operator sets per tuple (`REQ-QTE-9`), keyed by provider and ends, at most
  one per tuple. A new check, `PolicyBounds::check_against`, which `DecisionRecord::check` runs: every exchange
  tuple of the snapshot that settles within three days (`THREE_DAYS_SECONDS`, 259,200, inclusive) by its price or a
  fresh quote has a limit. The floor is judged after the operator's fee (`REQ-ORD-27`): `below_floor` now holds
  where what is left after the fee is below the floor, and a kept candidate must leave at least the floor. The
  contract checks both exactly, with no division (`ceil(a × n / d) ≤ k` exactly when `a × n ≤ k × d`), by a
  `no_std` product of amounts and `Amount::checked_sub`. Ranking stays on the providers' amounts before the fee, and a decision record says so
  in a new required field, `ranking`, whose one value is `before_operator_fee` (`REQ-QTE-11`). Three new shapes:
  `stated-terms` (`StatedTerms`, `REQ-QTE-6`, naming its tuple so a history is keyed), `settlement-times` (`SettlementTimes`, `REQ-QTE-3`) and `held-quote`
  (`HeldQuote`, `REQ-QTE-15`). New required fields and a new rule are a major under `DEC-4`. Every policy-bounds and
  decision-record fixture gains the fields; six valid fixtures and seventeen invalid ones are new, five of them made from patches.
  (DEC-2, DEC-4, DEC-20, T-020-02)

## 3.0.0
- Breaking: three drop reasons for the candidates 2.x could not state, so a decision record names every path the
  search reaches that ends anywhere, and why it was dropped (`REQ-AUD-4`; ruled by the owner, 2026-11-09, item
  `aud4-unstateable-candidates`). `no_price {part, operation}`: a path through a tuple the snapshot holds no price
  for. `fee_above_amount {part, operation}`: an operation whose fee, by its price's formula and counted at its
  maximum where one is given, is above the amount that reaches it. `nothing_delivered {part, operation}`: an
  operation short of the destination that delivers nothing, its fee taking all that enters or its output rounding
  to 0. Each is shaped as `beyond_levels`: the part is listed up to and including the named operation, its last,
  which arrives with 0 and needn't reach the destination. A `no_price` operation states a fee of 0 and cites its
  tuple and no price, and the snapshot holds no price for that tuple; every other operation still cites one. A
  `fee_above_amount` operation states its amount in as its fee and cites no usable accepted answer at its amount
  in; that the formula's fee is above its amount in is the router suite's to recompute. A `nothing_delivered`
  operation doesn't end at the destination, where such a part is `no_positive_outcome`, and its terms are
  recomputed as any operation's. Before, a router left such a path out of the record, which then couldn't say why
  a tuple wasn't used. A new Rust variant breaks an exhaustive match and the schema's `reason` enum grows, so this
  is a major. `DropReason` is `#[non_exhaustive]` from here, as `Objective` is: that spares Rust callers, whose
  matches need a wildcard arm, but the contract is language-neutral, and a reader on an older schema rejects a
  reason it doesn't know, so a new wire value is still a major under `DEC-4`. Two valid fixtures carry the
  three reasons; eleven invalid ones break their rules. (DEC-4, DEC-8, DEC-20, T-011-05)
- Breaking, folded into the unreleased 3.0.0: each segment of a route names the cross-module calls it makes, so the
  platform's call rule (`REQ-MDL-21`, `THR-86`) takes the calls a route names from the router's decision, not from a
  list named by hand. A new figure kind, `calls`: what a provider's admitted module declares, as its release's
  `outside.calls` lists them (`{callee, kinds}`, each callee once), from the provider, its `module_version` naming the
  release, with no validity, at most one per provider. A new optional field on the priced operation, `calls`: the
  `{callee, kind}` pairs the segment names, each once, never empty when present, none to its own provider. The kind
  is the module interface's, a provider-role operation kind, by `$ref` to provider-role's operation kind, and in Rust
  `CallKind`, the module interface's `OperationKind`, but never `holding_balance`, the operator's own question to a
  holding, which module-interface already refuses in a declared call: the schema's `call_kind` excludes it, and
  the Rust checks refuse it in a calls figure and in an operation's calls. The record's check: an operation cites one calls figure exactly
  when it names a call, its own module release's; each call is declared there and is to the provider of another
  operation of its part. That a router names every such call is the router suite's. A new figure kind and a new
  wire field are a major under `DEC-4` whatever Rust's `#[non_exhaustive]` spares: a reader on 2.x's schema rejects
  both. Two new valid fixtures, a calls figure in the one of every kind of figure, and twenty invalid ones; the new decision-record fixture is `rutent-router`'s own
  record over its inputs, as its tests show. (DEC-4, DEC-8, DEC-20, T-011-06)

## 2.1.0
- The Rust code behind this contract (`contracts/src/router_api`) is `no_std` with `alloc`:
  it uses `core` and `alloc` paths, and the crate's
  default `std` feature turns on `std` only for its dependencies and tests. Minor, not major: no schema, Rust type or
  behaviour changes, and every consumer that builds with default features builds unchanged. Fixtures keep naming
  their current version. (DEC-4, DEC-20, DEC-26, T-010-01)

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
