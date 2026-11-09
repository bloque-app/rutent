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
