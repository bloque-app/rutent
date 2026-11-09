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
