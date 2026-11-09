# F-012 notes

Readings taken where `SPEC.md` or `PLAN.md` is silent or ambiguous, recorded per Narthex `REQ-SKL-24`.

## T-012-01

- **Layout.** `lib.rs` declares `catalogue`, `module`, `proxy`, `egress` and `holding`. The last four are one-line
  stubs (`mod.rs` with a doc comment) that `F-013`, `F-014` and `F-016` fill. `F-016` also owns the wallet guest
  under `components/prov/guests/`, which is not created here.
- **No clock, no provider names.** The catalogue is a plain value. Every time is a caller-passed `Instant` (whole
  seconds since the epoch), and nothing in it names a provider (`REQ-MOD-4`).
  The caller (the router or Core) must pass its own clock as `at`: a stale `at` would honour an expired grant.
  `T-012-02`'s skew check on an advertisement's `made_at` uses that same `at`.
- **What "scoped to categories and tuples" means.** A tuple is identified by `TupleKey`: category, origin,
  destination and asset (an exchange adds its `to` asset). A grant must name at least one tuple for every
  value-moving category; a verifier has no tuples (`DEC-16`), so a category alone covers it.
- **Several grants for one provider.** A tuple is usable when any grant of the provider in force covers it. When
  none is in force, the refusal reports the standing of the most recently started grant. Revocation wins over
  expiry. Grants are never removed, so re-entry adds a grant and the earlier ones stay visible (`REQ-CAT-2`).
- **Entry needs a grant.** `Catalogue::join` refuses an advertisement from a provider with no grant in force. How
  the grant's keys are first trusted, and signature checks, are `T-012-02`.
- **Operator bounds.** Per provider and tuple: `Restricted`, or a `Max` segment amount in the tuple's input unit.
  `REQ-CAT-1` gives no further bound kinds; a ceiling on value in flight is `REQ-ORD-8` (`F-020`).
- **Reversal capacity (`DEC-11`, ADR-0010).** Each grant carries a recorded check: bond (asset, amount, third-party
  holder), ceiling, who checked and when. A grant can't be built without one. The ceiling can't exceed the bond. When
  the bond is re-checked the ceiling follows it down, never up; a bond in another asset drops the ceiling to zero
  since the two can't be compared. ADR-0010 notes that a bond checks nothing when operator and provider are one
  party; DEC-11 lets that stand while every provider is the operator's own, so the check doesn't refuse it.
- **Owed reversals (`REQ-CAT-7`).** The catalogue keeps a list of reversals owed. Silence, lost standing, grant
  expiry and revocation never discharge one; only a confirmation does (ADR-0008). Who feeds it, and how orders
  create the entries, is `F-020`'s.
- **Declarations (`REQ-CAT-6`).** Recorded per provider, attributable to the maker. An unchanged declaration is
  recorded once, even across versions. A changed one in a new version is a new record and users must be shown it
  again. A changed one inside an already-recorded version is refused. Shown-to-user is tracked per user and
  declaration; `accept` needs the current one shown.
- **Publisher (`REQ-CAT-20`).** A provider and each private node it declares are recorded with the advertisement's
  publisher. One publisher may hold several providers and nodes. A provider or node later naming another publisher is
  refused, and a refused join leaves no trace.
- **Property tests without a property-test crate.** None is adopted yet in the workspace, so the four property
  tests use a seeded xorshift generator and 400 cases each; a failure names its seed. Adopting `proptest` later
  would only need a `licences/RECORD` line.
- **Test-first.** The test files were written before the catalogue's code and failed to build against the missing
  API; the code was then written to the tests.

## T-012-03

- **What the snapshot holds.** `Catalogue::snapshot(id, at)` gives the router-api `CatalogueSnapshot` plus a list of
  every advertised tuple that stayed out, with its reason. It holds the tuple figures (reserve, exchange, ramp) with
  their terms: bands, increment, units, gross or net, required inputs. A price, quote, dry run, eligibility or
  exposure figure comes from another source (`F-011`, `F-015`, `F-020`), not from an advertisement, so none is made
  here. The router's snapshot has no place for evidence availability or safe retry, so they stay readable through
  `Catalogue::declared_by(provider)`.
- **Age and origin.** A figure's age counts from when the catalogue joined the advertisement (`join`'s `at`), and it
  states no validity, since an advertisement holds until replaced. `T-012-02` has the statement's `made_at` and
  should count from that instead. The module version in the origin is recorded by `set_module_version` (the
  admission artefact's version, `F-013`); a provider with none recorded has its tuples left out.
- **Caller's clock.** `at` is the caller's, as in `T-012-01`; the snapshot's `taken_at` is that instant.
- **Evidence floor, fail closed.** `REQ-ORD-39` says the operator MUST set a floor per tuple. A tuple with none is
  left out, and so is one whose provider declares no evidence. The floor is the weakest grade allowed, so a provider
  declaring a larger number than the floor is out. The floor is per tuple, not per provider and tuple.
- **Nodes (`REQ-CAT-22`).** The operator's list is public nodes only; the private ones are those providers declare
  (already kept per publisher). A node can't be on both: adding a declared-private node to the public list, or
  joining an advertisement that declares a public node private, is refused. Every inner node on a tuple must be on
  one list. An outer node is named by its network, and its reserve is the advertising provider, so the figure's
  origin carries the reserve.
- **Holding accounts (`REQ-CAT-23`).** "Every node where value rests" is every inner node of a tuple; outer nodes
  have none. A public node's account is registered by the operator, a private node's by its recorded publisher; a
  tuple with an inner node lacking one is left out. The reference is opaque to the catalogue.
- **Tuple shape (`REQ-CAT-13`, `REQ-CAT-21`, `AC-30`).** `check_tuple` runs the router-api's typed-operation rules, the
  same the state machine uses. A grant naming a malformed tuple is refused (`GrantError::MalformedTuple`), and the
  snapshot checks again. An advertisement can't write an exchange spanning two nodes, so the refusal is tested on
  tuple keys and grants.
- **Check on the way out.** Every snapshot passes `CatalogueSnapshot::check()`, and the tests also validate it against
  the router-api schemas with `jsonschema`, already in the lock as `contracts/`'s dev-dependency (a dev-dependency of
  `components/prov` too; no new package, no new `licences/RECORD` line).
- **Test-first.** The two test files were written against the API before the code and the code written to them.
  Property tests use the same seeded generator as `T-012-01`.

## T-012-04

- **No allowance (`DEC-15`).** ADR-0013's bucket, higher rate and refunds are not built. The bound is a plain count:
  at most `max_counted` counted checks per account inside a window of `window_secs`, set by the operator. With no
  bound set, no check is sent (fail closed): `REQ-CAT-3` says the bound MUST exist. Any amount may be asked
  ("thresholds are free").
- **What counts as a check (`REQ-CAT-4`).** A balance verification, and a withdrawal dry run (its schema requires a
  session for that reason). A real withdrawal is the order, and other dry runs (deposit, transfer, conversion) take
  no session and reveal nothing about the holder's balance, so they are not checks. A check counts when it is sent,
  so a reserve's refusal of it still counts. A check refused by the catalogue (no session, over the bound) is logged
  and not counted.
- **Session register.** Core issues sessions; the catalogue is told `open_session(id, account, expires_at)` and only
  holds which account a session speaks for. A withdrawal dry run names a session but no account, so its account is
  the session's. A check with an unknown or expired session, or none, is refused; a session of another account is
  refused apart. A check refused for want of a session can only be logged against an account when the operation
  names one (a verification does; a dry-run withdrawal with no session does not).
- **Own order.** The caller states the holder's own order (`own_order`). Such a check is logged, not counted. The
  catalogue can't tell a genuine order from a probe passing as one; that is `F-020`'s to guard.
- **What the asker is told (`INV-9`).** `Verdict` is yes, no, a refusal class without a time, or not known. A
  verification's refusal keeps its two classes (the contract gives it no `lifts_at`). A dry run's acceptance is a
  yes whatever its figures. Its refusal with a lift time collapses to not known, since the time would say more than
  yes or no. An answer for another transaction or kind is not known and leaves the check pending, so silence still
  applies.
- **Silent reserve (`REQ-CAT-19`, `NFR-8`).** The operator sets `set_answer_within(seconds)`; with none set, no
  check is sent. A check due at `sent + seconds` is marked at that moment, by `sweep_silent(at)` or by a late answer,
  once, with an `Alert`. The mark runs from the due moment, not from when the sweep ran. The reserve's reserve
  tuples are then `Unusable::TemporarilyUnavailable`, so the snapshot leaves them out. Other categories are not
  affected. A reserve marked silent can still be asked, since that is how it comes back. An answer in time lifts the
  mark; a late answer is delivered but doesn't.
- **True undo (`REQ-CAT-15`).** The advertisement carries no reversibility field, and the catalogue's only
  reversal data is each grant's ceiling (`DEC-11`). So `admit_segment` declines an exchange or ramp segment with no
  grant in force having a non-zero ceiling, or an amount over the ceiling when the tuple's asset is the bond's. A
  ceiling is in the bond's asset, so an amount in another asset can't be compared and passes that check. A reserve
  is never declined (`ASM-4`). `judge_reversal` counts only a succeeded result with the same node, asset and amount
  as the reversal asked; anything else stays owed.
- **Dry runs (`REQ-CAT-14`).** `assess_dry_run(&self, ..)` takes `&self`: it changes nothing. A refusal or a failed
  or unknown answer removes the option (`removes_option`); a result that states a settlement for a dry run is an
  error, as is any operation that isn't a dry run.
- **Refusal classes (`REQ-CAT-18`).** `RefusalClass` is lifts at a known time (with the time as stated), waits on the
  person, or final. The provider applies the limit by amount; the catalogue only reads the class.
- **Where value is after a result (`INV-2`).** `segment_effect` gives `NothingTaken` for a failed, refused or
  dry-run result: nothing leaves the origin, so the value stays in full whatever the direction. This sidesteps
  `exact_out`, where the operation's amount is not an origin amount. A succeeded result gives `Taken` with its
  `amount_in`; unknown gives `Unresolved`, never "at the origin".
- **Test-first.** Both test files were written against this API before any code and failed to build; the code was
  then written to them. Property tests use the seeded generator of `T-012-01`.

### T-012-04, review answers

- **Withdrawal dry run is a check.** `assess_dry_run` refuses a withdrawal dry run (`DryRunError::RevealsSufficiency`):
  it goes through `begin_check` and `answer_check`, so it has a session, a bound and a log, and its refusal's lift
  time never leaves. The other dry runs (deposit, transfer, conversion) keep `assess_dry_run`.
- **Silence needs no sweep.** A reserve with a check unanswered past `sent + answer time` is unusable and ineligible
  from that moment; `sweep_silent` only raises the alert and records the mark.
- **Lifting the mark.** An in-time answer lifts it only when no other check of that reserve is unanswered past its
  time. A check never answered therefore keeps the reserve out until it is answered, late or not.
- **Dry run answered as done.** `segment_effect` refuses a dry-run operation whose result states a settlement
  (`EffectError::NotADryRunAnswer`).
- **`own_order`** stays as it is: the guard is `F-020`'s.

## T-012-02

- **Crate.** `ed25519-dalek` 2.2 with default features off (pure Rust, `no_std`-capable, BSD-3-Clause), over
  `curve25519-dalek` 4.1. It has strict verification (`verify_strict`: a small-order `R` refused, `R` compared as
  the bytes sent) and is the widely reviewed one. The lock gains about 25 packages, all permissive (MIT, Apache-2.0,
  BSD-3-Clause); only `zeroize 1.9.1` was missing from `licences/RECORD` (`1.9.0` was there), and it is
  now recorded. No locked package changed version. The tests also use it to sign, as a dev-dependency with its
  default features.
- **What the verifier refuses, each with its own `SignatureError`.** `catalogue/signature.rs`, in two layers:
  `verify_ed25519` over raw bytes (`F-013` reuses it for release assertions) and `verify_envelope` over a statement
  as received (the bytes signed are `signing_input(received)`). `ed25519-dalek` 2.2 decompresses a `y` at or above
  2^255 - 19 without a word, so the canonical check is its own: `y < p`, then decompression, then `is_weak()`
  (small order), then `S < L` (the malleable twin), then `verify_strict`. A signature's text goes through
  `decode_signature`, which takes 86 characters of unpadded base64url whose last one is in `AQgw`; a key's text goes
  through the contract's `PublicKey`. Test vectors: RFC 8032 section 7.1, tests 1 to 3 (verified, and re-signed
  from the secret key), plus `p` to `p + 18` with both sign bits, six of the eight small-order points, and `S + L`.
- **Admission uses it: a `DEC-20` path.** `Catalogue::join` and `Catalogue::verify_statement` are what module
  admission (`T-013-04`) calls for signature checking.
- **`join` takes the received bytes.** `Catalogue::join(&[u8], at)` replaces `join(&Advertisement, at)`, so no
  advertisement is admitted without its signature checked. `JoinRefusal` gained the signature, key and clock
  variants. Existing tests join through `tests/common`'s helper, which signs the fixtures' advertisements anew with
  deterministic test keys (the fixtures' own signatures are placeholders).
- **Which deployment.** The catalogue is told once (`set_context(deployment, protocol_version)`); with none set no
  advertisement is joined. `check_shape` and `check_context` run on every advertisement (`REQ-PRT-1`, `THR-8`).
  A catalogue that guessed its deployment from the first advertisement would let the first caller choose it.
- **First trust (`REQ-CAT-2`).** The operator names the keys it trusts for a provider on a grant:
  `trust_advertising_key(grant_id, key)`. The first advertisement must be signed with a key a grant in force names,
  and every advertising key it lists must be named too (a listed key vouches for nothing, and a key it lists but
  the operator never named is refused). A key that is a statements key somewhere can't be trusted.
- **Later advertisements.** They pass `validate_successor` against the last accepted, then the signature verifies
  with the key that one lists for advertising. A provider that lost every advertising key is let back in by a grant
  made after its last accepted advertisement and signed with a key that grant names (the README's "fresh operator
  grant"), under the same rules otherwise.
- **No replay.** Strictly newer than the last accepted by `made_at`, to the nanosecond; checked by
  `validate_successor` on the successor path and again by the catalogue on both, so the grant path has it too.
- **The clock (`REQ-CAT-2`).** `made_at` more than the skew ahead of the caller's `at` is refused; a fraction of a
  second counts as the next second, so the check fails closed. `SPEC.md` gives no skew, so it is the operator's
  setting (`set_clock_skew`) with a default of 300 seconds. `verify_statement` applies the same bound with the same
  `at`.
- **Key history.** The catalogue keeps every accepted advertisement per provider, oldest first
  (`advertisement_history`); a refused one leaves nothing in it. A statements key an accepted advertisement dropped
  is never listed again by that provider, under either purpose. A key's purpose is held across providers in both
  directions, and a key the operator trusts for advertising counts as an advertising key from then on (so no one can
  list it for statements first). Another deployment's advertisements are refused (`WrongContext`), so its keys reach
  the catalogue only through `record_statement_key`, which the test for "in any deployment" uses: reading the README's "a key used for `statements` anywhere never accepted for
  `advertising` anywhere" one step further, an advertisement may not list for `statements` a key that is an advertising
  key of some provider either, or one provider could lock another out of its own key. The release side is
  `record_statement_key`, for `F-013` to call with each admitted release's `statement_key`.
- **Which key verifies a statement.** `verify_statement(received, at)` first requires the statement's deployment
  and protocol version to be the catalogue's (`THR-8`; keys may be shared across deployments). Then
  `signature.key` must be listed by the advertisement the catalogue holds, the last accepted, and by the one in
  force at the statement's `made_at` (the latest made at or before it), for advertising (an advertisement or a
  release assertion) or for statements (anything else). The first check means a leaked retired key can't sign a
  backdated release assertion (`THR-4`); the second, that a key can't sign for a time before it was listed. The
  limit that remains: the key in force today can sign a statement with any past date after its own listing.
- **`T-012-03`'s placeholder.** A figure's age now counts from the advertisement's `made_at` (whole seconds), not from
  when it was joined. A `made_at` ahead of `at` but within the skew gives age 0.
- **Role suite.** The suite's stand-ins sign with placeholders and conformance has no curve code, so
  `tests/role_suite.rs` runs each stand-in behind a wrapper that gives every statement a real signature (the seed is
  the placeholder key's bytes; a statement nested in a result is signed first). The catalogue, holding the stand-in's
  advertisement under a grant, is the suite's `SignatureVerifier`. All four stand-ins pass with no signature deferred;
  a stand-in run with its placeholders is flagged under `SignatureVerifies`. `rutent-conformance` is a dev-dependency
  of `components/prov`, as the boundary check allows (`DEC-2`).
- **Not done here.** Releases' assertion keys against the accepted advertisement (`validate_assertion_key`) and
  the admission record are `T-013-04`. The catalogue keeps no key history across restarts; persistence is outside
  `F-012`.
- **Test-first, as it happened.** The verifier's and the history's code was written before the test files in this
  task, against the README's rules, which breaks the brief. The tests were then written from the rules, and
  each was shown able to fail by disabling its rule in turn (canonical `y`, small order, `S < L`, skew, dropped
  key, purpose across providers, key at the statement's time, replay) and seeing its tests go red; the code was
  then restored. Property tests use the seeded generator of `T-012-01` (400 cases each); none needed a property-test crate.

### T-012-02, review answers

- **`verify_statement` checks the context** (blocker): a statement of another deployment or protocol version is
  `WrongContext`; a catalogue told no deployment verifies nothing.
- **Backdating**: checked against the last accepted advertisement as well as the one in force then (see "Which key
  verifies a statement" above, which replaces the earlier stated limit).
- **"In any deployment"**: tested as above, through `record_statement_key` and a refused advertisement of another
  deployment; the catalogue still serves one deployment.
- **Trusted keys** count as advertising keys at `trust_advertising_key`.

## For T-013-04: nested statements (re-review of T-012-02, 2026-11-09)

`verify_statement` checks the deployment and protocol version of the outer envelope only. A statement nested inside
another (for example a release assertion inside a result) still verifies on its own key, so a caller that reads a
nested statement must run `check_context` on it too. Admission (`T-013-04`) does this for every nested statement it
accepts.
