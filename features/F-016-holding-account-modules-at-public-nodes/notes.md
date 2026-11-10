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

## T-016-02

- **Layout.** The wallet's guest is `components/prov/guests/plain-wallet/` (the `F-016` goal line's path, which
  `T-012-01`'s and `T-013-01`'s notes left to this task): a workspace of its own with its own `Cargo.lock`, one crate
  with a `sandboxed` and an `outside` feature, as `module-guests/rust/` is. It depends on `wit-bindgen` (the pin
  `module-guests` uses), `serde_json` and `rutent-contracts`, and nothing else; its lock was seeded from
  `module-guests/Cargo.lock` and resolved offline, so every package has a `licences/RECORD` line at its locked
  version. `tools/licence-check` walks `components/` for locks, so it reads the new one. `build.sh` builds the two
  parts and wraps them with `module-guests`' tool (`RUTENT_GUEST_TOOL` names a built one), so no `wit-component` is
  added. The host side is `src/holding/wallet.rs`; `catalogue/` and `module/` are untouched.
- **What the wallet speaks.** The plain wallet is a reserve-category module over the provider-role contract's own
  types (`ResultBody`, serialised by the guest, so the platform's output check and `result_matches` judge it like any
  module's). Reading: a `deposit` places value resting at the node (`origin`) in the account, and a `withdrawal`
  takes value out to the node (`destination`); the other side is the account's own outer node, named by its
  provider. The order is the operation's `order` input (`ORDER_INPUT`), since no operation names an order. A
  `balance_verification`'s `account` is an order: it answers whether that order holds the stated amount. A dry run,
  a zero or malformed amount, a missing order, another node or asset than the first operation fixed, and a release
  above what the order holds are refused as `final`, and change nothing. Its declaration declares no outside
  operation, destination or secret.
- **Where the balance comes from (superseded below).** The first version of this task read the balance as the sum of
  what the host had recorded the wallet confirmed; see "Review round" for why that was wrong and what replaced it.
- **A holding account does not advertise.** The provider-role README says so (`REQ-CAT-23`), yet admission judges a
  release against the advertisement the catalogue accepted for the releasing provider (`T-013-04`), and finds none
  (`Unverified::NoAdvertisement`) for a provider with no advertisement. Reading: the wallet's provider is admitted
  as any provider is, with an accepted advertisement and a grant, and in the tests that is `Signed::new()`'s
  reserve and ramp providers. A holding account that admits with no advertisement needs the contract and
  `T-013-04`'s judge to say so; raised here, not done.
- **Registration means something.** `Catalogue::register_public_holding` takes any string, so `Holdings::register`
  refuses unless the platform runs the module (`active_version`: a release waiting for its fresh acceptance, or one
  never admitted, does not run) and the node is on the operator's public list (the catalogue's `NodeError`). It
  also refuses a node that has an account and a wallet that is already another node's. The check that the module
  *is* a wallet is the operator's choice of provider; the platform does not show a running declaration.
- **Asset and node checks.** `HoldingError` gained `BadAmount`, `WrongAsset` and `WrongNode` (additive), and
  `OrderLedger` gained `node()` and `asset()`. `WalletHolding` checks the node, the asset and the amount (digits only,
  no sign, no leading zero, within `u128`) before the wallet is asked anything, on a copy of the ledger that
  replaces the account's only when the wallet's result confirms the same amount in and out. The wallet checks node
  and asset again, so a host that did not would be refused by it.
- **`INV-5`.** `Holdings::land` is where value rests between segments: in a registered node's account, or refused
  (`NoHoldingAccount`); there is no other place in the type for it. rutent's own position is what the tests
  compute as landed less released less what rests in the accounts, and the property keeps it at zero after every step.
- **Admission refusals.** `Platform::admit` still answers an assertion that does not verify with
  `Admitted::Unverified` and a release while another waits with `VersionAlreadyAdmitted`; the contract's
  `AssertionUnverified` and `AdmissionPending` (`T-010-09`) are not yet mapped by `module/`, which this task does
  not touch. The tests assert what the platform gives today.
- **The tests.** `tests/holding_wallet.rs` signs the wallet's release for real, as `tests/module_admission_signed.rs`
  does (`tests/holding_support/` builds the wallet like `built_standins` builds the stand-ins: into the build
  directory, again when the guest's, the WIT's or a contract's source is newer; `RUTENT_PLAIN_WALLET` names a
  prebuilt directory). The `INV-5` property runs 400 seeded cases as one history over two wallet nodes, one public
  node with no account and one unlisted node, with one admission for the whole run; the refusal and landing branches
  are asserted to be drawn.
- **CI.** The `ci` job's guest steps named `module-guests/` only, so the wallet's guest had none; it now has a format
  step and a clippy step for each part for `wasm32-unknown-unknown`.
- **`DEC-20`.** Module admission is exercised, not edited: no `DEC-20` path is touched.
- **The wallet's record lives in its instance.** A provider-process restart (`Platform::restart`) empties the wallet's
  record while the host's `WalletHolding` keeps its own. Durable wallet state is outside this task; `T-016-03` or a
  later task decides whether the wallet reads its record from the provider's state.
- **The property asks the wallet.** After each step at a registered node, the `INV-5` property asks that node's wallet
  a `balance_verification` for the order it touched and asserts that it covers the model's amount and not one unit
  more, so the property is about the wallet's word and not only the host's mirror of it.
- **Test-first.** The tests were written first. The first run failed on the unresolved `holding::wallet`, since
  neither the module nor the guest existed, and the wallet and host were then written to meet them. The six tests
  passed at once on the first complete run; the direct-withdrawal assertions of the refusal test were added after
  and pass at once, since they pin behaviour already present (the wallet's own refusal of a release above what an
  order holds, which the host's pre-check otherwise hides).
- **A finding of the stronger property, fixed.** The first run of the wallet-asking property failed at its third
  case: a wallet that had taken nothing yet answered a `balance_verification` of zero as not covered, because no
  asset was fixed. It now covers a stated nothing and nothing more.

## T-016-02, review round

- **The blocker: the balance was the host's own sum.** `WalletHolding::reading` built both the balance and the
  per-order split from the host's copy and labelled it `BalanceSource::Module`, so `reconcile` could never return
  `Short`, and a wallet that lost its record (a restarted provider process) still looked whole (`INV-1`,
  `REQ-AUD-15`). Fixed at the source: no provider-role shape carried a balance, so one is added now (pre-production,
  so the gap is fixed in this milestone). `holding_balance` is the operator's question to a holding account's
  module, naming the provider, the node and the asset, and its result is the balance in whole units, zero included,
  or a refusal. It is provider-role **3.0.0** (a new operation kind and result kind on the wire: a major under
  `DEC-4`) and module-interface **4.0.0** (`OperationKind::HoldingBalance`, a new variant on a closed enum, folded into the
  unreleased 4.0.0 of `m2`: `develop` holds 2.0.1; the declaration schema takes its kinds from provider-role's, so it needed no edit). Each has valid and invalid
  fixtures, a changelog entry, Rust tests in `contracts/tests/`, and the role suite's version pin and `Kind` moved;
  the stand-ins refuse the new kind outside their role. `contracts/CHANGELOG.md` records both.
- **A reading is two halves from two places.** `WalletHolding::read` asks the wallet for its balance and takes the
  attribution (the value held for each order) from the host's record of what the wallet confirmed. The
  `HoldingAccount` impl for `WalletHolding` is gone: its `reading(&self)` had no way to ask the wallet. The
  trait stays for `OrderLedger`, the reference account. `Holdings::readings` now takes the platform and the
  operator and returns one result per account, in node order, so an account whose wallet could not be read shows its
  failure and is never dropped from the audit's view. `Holdings::attributions` is the host's own record, for tests.
- **Registration checks the role.** `Holdings::register` also refuses (`NotReserve`) a provider whose accepted
  advertisement offers no reserve. That is the declared category the platform lets us check without touching
  `module/`; it does not see the running declaration's handled kinds, which a later task can pin. Full admission of
  an operator-published holding account, with no provider advertisement, is the new row `T-016-04`. The tests now
  register reserve providers only (`add_reserve_provider`).
- **The wallet fixes its node and asset only on success.** A refused first operation (a release of what nothing
  holds, in another asset at another node) used to fix the account to that node and asset, locking the wallet. It
  now fixes them when a movement goes through.
- **Tests added** (`tests/holding_wallet.rs`, and the contract tests): the restart test (`reconcile` reports `Short
  { by: 150 }` for the orders the host thinks held), the unreadable account shown with its failure, a ramp not
  registered, `NodeTaken`, `WalletTaken`, `BlankAsset` and a private node refused, and the refused first operation.
  The INV-5 property now reads each account's balance from its wallet and reconciles it after every step. Written
  first, they failed to compile against the old `Holdings` (no `read`, `readings` without a platform) and the old
  `RegisterError`; the guest's bind and holding-balance behaviour was exercised through the rebuilt wallet.
- **`DEC-20`.** `contracts/` (provider-role 3.0.0, module-interface 4.0.0) and `.github/` (a new CI guest step, a
  `CODEOWNERS` path) are `DEC-20` paths this round touches; the click comes with the release into `main`.
- **`holding_balance` is not a cross-module call.** A module could declare a call of that kind and a route could name
  it, and `judge_call` let it through, so a module could read an account's total, which for a node holding one order
  is that order's balance. `judge_call` now refuses the kind as `undeclared_kind` however it is declared or named, so
  only the operator's audit reads it through the host. The change is in `proxy/calls.rs` (a barred kind and a filter
  on the route's naming); the property's kinds no longer include it, and a test covers a declared call, a named
  call, both, and an ordinary call as the control. The contract's declaration check refuses it too:
  `validate_declaration` returns `DeclaredCallOfHoldingBalance` for a `calls` list that names it, folded into the
  unreleased module-interface 4.0.0 with an invalid fixture and a changelog line.

## T-016-03

- **What a handed-on start is.** `Holdings::start_handed_on(platform, operator, route, order, amount)` takes a route
  whose first segment starts at an inner node, withdraws the order's `amount` from that node's holding account
  through the wallet (the same `withdrawal` operation `release` sends), and returns a `HandedOnStart`: the starting
  reserve (the account's provider), the node, the route's first segment, and a `WithdrawalRecord`. That record is the
  proof of `REQ-ORD-40`, and it settles the first segment. The code is `holding/wallet/handed_on.rs`.
- **Reading: the proof is the wallet's own evidence piece.** The record is accepted only when the wallet's succeeded
  result carries a piece whose `by` is the wallet's provider and whose reference is that wallet's own withdrawal
  reference. A success without one (`NoRecord`) leaves the account as it was. The plain wallet states its piece at
  grade 3 (`SignedByProvider`); `REQ-ORD-40` makes the holding account's record the proof at a public node whatever
  that grade, so the grade is not what makes it proof.
- **Reading: the evidence checker is untouched.** `grade_outcome` grades a segment outcome from a provider the route
  runs through (`REQ-ORD-39`); a handed-on route's withdrawal is not one, because the holding account is the host's
  own registered module and the withdrawal is the host's own operation. So no interface was added to
  `components/evidence`, and `DEC-2` holds (prov imports only the contracts). A private node's version, where the
  record is its publisher's word and labelled so, is not built here: private nodes have no holding module.
- **Refusals.** A route that is empty or starts at an outer node is `NotHandedOn`; one that starts at an inner node
  with no registered account is `ValueNotAtStart(node)` (the state machine's reason for a route that does not start
  where value sits); a withdrawal above what the order holds, of another asset, of zero, or one the wallet does not
  confirm is `Wallet(..)`. In every refusal nothing moved: a failed withdrawal starts no route.
- **What the host does not check.** That the node is where the first order's value sits is a rule across two orders
  (`contracts/state-machine/README.md`); the host checks the order's attribution at the account, which is the
  narrower and sufficient thing for the withdrawal.
- **Refactor.** `WalletHolding::movement` now returns the would-be ledger and the wallet's settlement and no longer
  replaces the ledger itself; `land` and `release` do, so their behaviour is unchanged.
- **Tests** (`tests/holding_handed_on.rs`), written first (the first run failed to compile on the missing
  `start_handed_on`): the proof test, the refusal test, and an `INV-5` property over 400 seeded steps of landings and
  handed-on starts at one node, which after each step reads the balance from the wallet and checks that what landed,
  less what was handed on, less what rests in the account is zero.
- **`DEC-20`.** No `DEC-20` path is touched.

## T-016-03, review round

- **The record is for this transaction.** `movement` now also returns the exact transaction id it sent, and
  `proof_of_withdrawal(provider, transaction, evidence)` accepts only the wallet's own piece (`by` is the provider)
  whose reference is exactly `{provider}-withdrawal-{transaction}`. A stale reference, another order's or a longer
  one sharing the prefix is `NoRecord`.
- **`NoRecord` is tested through that function.** A lying wallet would need its own Wasm guest, so the test
  calls the pure check: no piece, another party's piece, a stale reference and a prefix-sharing one are each
  refused. The function reads and changes nothing, and `withdraw_with_record` replaces the ledger only after it
  returns, so a refusal leaves the attribution and balance as they were by construction; the refusal test of the
  routes asserts that for the other refusals.
- **A wrong asset is `ValueNotAtStart`.** The contract's reason covers the asset too, so a route whose first
  segment is in an asset the account does not hold is `ValueNotAtStart(node)`, before the wallet is asked anything.

## T-016-04

- **What "operator-published" is.** The operator records a module's name in the catalogue
  (`Catalogue::publish_operator_module`), and from then on the catalogue names the operator as its publisher
  (`PublisherIndex::publisher` gives `Publisher::Operator`). The record is kept apart from the publishers that
  advertisements name, not as a reserved publisher string, so no advertisement can claim it. The two exclude each
  other: an advertisement naming a module the operator publishes is refused (`PublisherError::ProviderIsTheOperators`),
  and the operator can't publish under a provider a publisher stands behind (`ProviderOfAPublisher`). So neither
  can release the other's module (`REQ-MDL-1`). A repeat changes nothing. `publisher_of_provider` still gives only an
  advertisement's publisher; the snapshot and `providers_of` never see the operator's modules.
- **Which rules hold is the catalogue's record, never the assertion's.** `Catalogue`'s judge
  (`module/admission.rs`) takes a module the operator publishes only on the operator's own release, and every other
  module only on its provider's. The operator's release has three checks. `Catalogue::verify_operator_statement`
  (`catalogue/operator.rs`) checks the context and the clock skew, that the signing key is the `operator_key` of the
  deployment the catalogue serves, and that the Ed25519 signature verifies strictly. Then the contract's
  `validate_operator_assertion` runs against that same deployment. Last, a check before compiling that the declared
  statement key signs no advertisement anywhere (`Catalogue::is_advertising_key`); recording the key after compiling
  would catch the same thing. A provider's module signed with the operator key still fails the provider's rules
  (`NoAdvertisementThen` or `KeyNotInForce`). A module the operator publishes, signed with any other key, is
  `NotSignedByOperator`.
- **The operator key comes from the deployment settings.** It is the catalogue's context
  (`Catalogue::set_context`), the deployment the operator configures, and is never taken from the assertion or from
  an advertisement. The platform's `read` checks the assertion against its own envelope too, so the two settings
  must agree. This is the same arrangement `T-014-02` made for the proxy's operator key.
- **The contract.** No provider-role shape was needed. Module-interface 4.0.0 (unreleased, `m2` only) gains
  `validate_operator_assertion(artefact, deployment)` and the `RuleViolation` variant
  `OperatorAssertionNotSignedWithOperatorKey`. It also gains an `operator-artefact` fixture bundle: the deployment
  (schema `statement-envelope#/properties/deployment`) and the artefact, with one valid fixture and three invalid
  ones. The README's rule table and the changelog say so.
- **Reading: the maker of an operator release.** The deployment names the operator's key but not its party, so
  neither the contract nor the judge checks `maker`; the key is what binds the release to the operator, as
  `check_signed_by_operator` binds an operation. The tests use `rutent-operator`.
- **Reading: the statement key of an operator module.** The operator chooses it, its own release vouches for it,
  and it must not be the operator key or an advertising key. No advertisement lists it, so the credential proxy
  (`proxy/shared.rs`), which resolves a statement key only when an advertisement lists it, would not resolve one
  for an operator module. The plain wallet declares no secrets and its results are placeholders today, so nothing
  asks. `proxy/` is `T-014-05`'s and is untouched; resolving an operator module's key is left to a later row.
- **Registration.** `Holdings::register` now refuses a module the catalogue doesn't record as the operator's
  (`RegisterError::NotOperatorPublished`). That replaces `T-016-02`'s interim `NotReserve` check, which looked
  for a reserve in the provider's accepted advertisement and is now gone.
- **The catalogue holds the rule too.** `Catalogue::register_public_holding` was public and took any reference,
  so a caller with the catalogue could still make a provider's module a node's account without `Holdings`. It now
  refuses a reference the catalogue doesn't record as the operator's (`NodeError::NotOperatorModule`), checked after
  the node checks. `Holdings::register` keeps its own check as the earlier error. The catalogue tests that register
  an account publish it first. `public_nodes_holding_account_is_only_a_module_the_operator_publishes`
  (`tests/catalogue_snapshot.rs`) covers a provider's module and an unpublished one. It was seen failing (a
  provider's module accepted) before the check was added.
- **Tests.** `tests/holding_operator.rs` has three tests:
  - `operator_published_holding_module_is_admitted_on_the_operators_own_release`: the publisher record. It checks
    refusals before compiling: a provider's or a statements key, an operator key declared as the statement key, and
    an altered assertion. Then it admits, accepts (judged again), runs, registers and lands value.
  - `operator_and_providers_cannot_release_each_others_modules`: a provider's module signed with the operator key,
    the operator publishing under a provider's name, and a provider advertising under the operator's module.
  - `provider_module_cannot_be_registered_as_a_holding_account`: a reserve's and a ramp's own wallets, running, are
    refused.

  The contract has `operator_release_is_signed_with_the_operator_key_of_the_platforms_deployment`. Every release is
  signed for real. The tests' deployment `operator_key` is now the public half of a test signing key
  (`tests/common::operator_key`), where it used to be a literal no one held the private half of. The whole `prov`
  suite passed after that one change.
- **No test registers a provider's module.** `holding_support` publishes each wallet as the operator's
  (`plain-wallet-polygon`, `plain-wallet-ethereum`) and signs its release with the operator key, and
  `add_reserve_provider` is gone. `holding_wallet.rs` and `holding_handed_on.rs` register only those wallets.
  `provider_that_is_not_a_reserve_is_not_registered_as_a_holding_account` is removed: its ramp case is now part of
  the provider-module test.
- **Test-first.** The new tests first failed to compile against the missing API. Next came the publisher record
  and the error variants, with the judge's operator path and the registration check still missing. On that run the
  first test failed on `NoAdvertisementThen` where `NotSignedByOperator` was expected, and the third failed on a
  reserve's module being registered. The second passed at once: the publisher record it checks was already in
  place. The contract test failed with the rule disabled (an operator assertion from another deployment was
  accepted), and passed with it restored.
- **`DEC-20`.** This task touches four `DEC-20` paths:
  - module admission and signature checking: `module/admission.rs` (the judge's operator path);
  - the catalogue's verifier, `catalogue/operator.rs` (`verify_operator_statement`), with `is_advertising_key` and
    the `NotSignedByOperator` reason in `catalogue/advertisements.rs`;
  - the publisher record admission relies on, `catalogue/publisher.rs`, and the holding-account gate on it,
    `catalogue/mod.rs` and `catalogue/nodes.rs`;
  - `contracts/` (module-interface 4.0.0).

  `module/platform.rs` is untouched.
