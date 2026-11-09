# F-016 notes

Readings taken where `SPEC.md` or `PLAN.md` is silent or ambiguous, recorded per Narthex `REQ-SKL-24`.

## T-016-01

- **Layout.** The interface lives in `components/prov/src/holding/mod.rs`. `lib.rs` already declared `holding`, so it
  is untouched. It imports nothing of ours (`DEC-2`): nodes, orders and assets are plain identifiers and amounts are
  whole units (`u128`) of the account's one asset. Binding the identifiers to the contract types is left to the
  wallet task (`T-016-02`), which meets the module interface.
- **What the interface is.** `HoldingAccount::reading` returns the balance as the account reports it and the value
  held for each order, in one value so both are of one moment. `reconcile` is the `REQ-AUD-15` check over a reading.
- **Two figures, not one.** `INV-1` and `REQ-AUD-15` only mean something when the balance and the per-order
  attribution can differ, so a reading carries both. `OrderLedger` is the reference account that keeps the
  attribution itself and reports its sum as the balance, which is the property `T-016-01` names. A real wallet
  reports its own balance, so a drift shows up as a difference.
- **Differences only detect.** `reconcile` takes a shared reference and returns `Short { by }` or `Over { by }`; it
  completes nothing and changes nothing (as `REQ-ORD-9`). Delivery to the operator is `CMP-CORE-AUDIT`'s (`F-022`).
- **Private nodes.** A reading says whose word its balance is (`BalanceSource::Module` or `Publisher(name)`), and a
  difference carries that label. Which publisher's interface backs a private node is `F-012`'s catalogue entry.
- **Edges.** A reading naming one order twice is refused. A sum that overflows `u128` is an error, never a match.
  Zero holds and releases are refused, and a release above what an order holds is refused with nothing changed.
  An order released in full leaves no entry.
- **Property tests without a property-test crate.** As in `F-012`: a seeded xorshift generator, 400 cases, a failure
  names its seed. The generator is copied into the test file so `holding/` stays clear of `tests/common`.
- **What each property covers.** The first INV-1 property is partly circular: `OrderLedger` computes its balance from
  its attribution, so it proves the ledger's bookkeeping (holds, releases, refusals) against an independent model,
  not the check. The second property and `account_whose_own_balance_drifts_from_its_attribution_is_flagged` cover
  the check itself: an account whose own balance differs from its attribution is flagged Short or Over with the right
  size, and `Matches` is drawn about a third of the time and asserted to be hit.
- **Zero entries and asset.** `reconcile` refuses a zero-amount entry, as `hold` and `release` do. It does not check
  `asset`: the reading carries one asset for the whole account, and `asset` is checked when `T-016-02` binds the
  contract types.
- **Test-first.** The tests and the implementation were written in one pass, so the red run was not observed
  separately; the tests were written from the interface's contract before the code was checked against them.
- **Review round.** The tests added in answer to review were written first. The zero-entry test was seen failing
  before the code changed. The drifting-account test and the `Matches` coverage passed at once, since they exercise
  behaviour already present; they were added to pin it, not to drive new code.
