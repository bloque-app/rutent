# F-020 notes

Readings taken where `SPEC.md` or `PLAN.md` is silent or ambiguous, recorded per Narthex `REQ-SKL-24`.

## T-020-02

- **One bump: 4.0.0.** This row first folded its change into router-api 3.0.0, unreleased then, as a third bullet
  of its changelog entry. `M2` was then merged into `develop` (pull request #60, `fb883e0`), so the contract guard
  read 3.0.0 as released and refused the change at 3.0.0 ("version not bumped"). The row's Work names that case:
  the new required `PolicyBounds` fields make router-api 4.0.0. The bullet moved, unchanged, into a 4.0.0 entry
  that says why, and the guard against `origin/develop` passes at 4.0.0.
- **What `PolicyBounds` gained.** `operator_fee`, a `FeeFormula`, and `slippage_limits`, both required. The fee is
  taken at the destination in the request's `to` unit from what the providers deliver, capped by its maximum and
  never raised to it, as `quote::quoted_price` already took it. Its proportion is at most one (`PolicyBounds::check`):
  a fee taking more than what arrives in proportion can't be shown, and the ranking reading below needs it.
- **Slippage limits keyed by tuple, not figure id.** `quote::SlippageLimit` named a tuple figure by id, which is
  local to one snapshot; the shortlist and choice rounds read different ones. A limit now names its tuple by
  provider and ends, as `excluded_tuples` does, and `commitments` matches it on the operation's provider and ends.
- **"Policy bounds without a slippage limit … are invalid" needs the snapshot.** Whether a tuple settles within
  three days is known only from its price or quotes, so the rule is `PolicyBounds::check_against(snapshot)`, and
  `DecisionRecord::check` runs it; the `Router` trait's precondition names it. It holds for exchange tuples only,
  since reserves and ramps set no rate (`commitments` treats them so already). A tuple counts as within three days
  if its price or any accepted fresh quote about it declares at most 259,200 seconds, so the rule fails closed.
  `THREE_DAYS_SECONDS` moved into router-api; `quote` re-exports it.
- **The floor after the fee, and what the contract checks.** The floor is "the least the customer accepts at `to`",
  so the router now drops a candidate as `below_floor` when what is left after the operator's fee is below it
  (the review of `M2`'s pull request, the example 99,000 / 99,500 / 1,000). The contract checks it exactly, both
  ways (after review: a first pass left it to the suite and relaxed the contract, which accepted a false
  `below_floor` even at a zero fee). No division is needed: `ceil(a × n / d) ≤ k` exactly when `a × n ≤ k × d`, and
  the cap is a second branch, `maximum ≤ a − floor`; a `no_std` long multiplication of amounts and
  `Amount::checked_sub` do it. The double judges the floor after the fee, and a flaw `FLOOR_BEFORE_THE_FEE` is now
  refused by the contract itself on a new case. `judge.rs`, `quote::shown` and the public
  `quote::received_after_operator_fee` share one function, so the floor, the price shown and Core's checks at
  each settled segment (`AC-42`) can't disagree.
- **"The record says its ranking is before the operator's fee."** A wire field on the record, `ranking`, with one
  value, `before_operator_fee`, rather than prose alone, so ranking net of the fee would be a new value a reader
  sees. On the record, not the bounds: it says what the router did, not what the operator chose.
- **Item for the form, `m3-rank-before-fee`.** `REQ-QTE-2` has the operator's formula follow `REQ-QTE-5`,
  which lets it depend on the tuple. With a formula per tuple, ranking before the fee may not give `REQ-QTE-11`'s
  lowest total cost. The bounds hold one formula, so the case can't arise yet; allowing it is a major. The property
  `one_operator_formula_at_the_destination_ranks_candidates_the_same_before_and_after_its_fee` pins the reading,
  and fails at seed 9 when one candidate's fee is computed by another formula.
- **Types moved, checks stayed.** `StatedTerms`, `SettlementTimes` (the declared and actual pair) and `HeldQuote`
  are router-api's, with schemas and fixtures. `HeldQuote::hold`, `redeem` and `label` became the free functions
  `quote::hold`, `quote::redeem` and `quote::held_label`, since the router can't add methods to a contract type.
  `quote::measure` reads a `SettlementTimes`; `SegmentTime::measure` builds one and calls it. `StatedTerms` names
  its tuple (after review), since nothing else would key a tuple's history. `hold` runs `HeldQuote::check`, the
  record's own included, and `redeem` refuses a hold whose record chose nothing as `NothingChosen`.
- **The quote functions keep their explicit arguments.** `quoted_price` and `commitments` still take the formula and
  the limits as arguments, so their tests can vary them; Core passes the record's own
  `bounds.operator_fee` and `bounds.slippage_limits`, which the docs say.
- **`rutent_router::VERSION` is `rutent-router-0.4.0`**: the same inputs may now drop as below the floor a candidate
  0.3.0 kept. The fixture the router's own record must equal names it.
- **Tests' bounds.** Every existing bounds gets a zero operator's fee, so no expected route moved. Tests and cases
  with exchanges get a limit of 50 basis points for every exchange (`common::limited`, `cases::limits_for`), drawn
  after the other draws so the property networks are unchanged.
- **Seen failing first.** `floor_is_judged_on_what_the_customer_receives_after_the_operators_fee` failed against
  the router judging the floor before the fee (kept, not `below_floor`), then passed with `judge.rs` restored. The
  ranking property failed under the mutation above. The contract's four "… is invalid" tests were written with the types
  they test, so they weren't seen failing first; each rejects its fixture with the named rule. After review, the contract's floor rules
  were each switched off once: judging the floor before the fee let the kept-below fixture through (three tests
  failed), and dropping the `below_floor` rule let all three false-`below_floor` fixtures through (three failed).
- **A limit is 1 to 10,000 basis points**, as provider-role's bounded slippage is; a limit of 0 isn't allowed, and
  an exchange held to no move states a firm rate in its quote.
- **Touches `DEC-20` paths**: `contracts/`. Also edits the router's `quote` and `judge` modules and `lib.rs`, and the
  router suite in `conformance/src/router/`, as the row's Work and `PLAN.md` §4.4 allow.
