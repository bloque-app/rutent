# F-002 notes

These are the readings taken where `SPEC.md` is silent or ambiguous, recorded per Narthex `REQ-SKL-24`.

## T-002-01

- **Tuple shapes.**
  - `REQ-CAT-13` gives every tuple "as origin, destination and asset pair". Each role writes that information in
    its own shape.
  - An exchange's origin and destination "MUST be one node", so the node is written once (`node`).
  - A ramp "keeps the same asset", so its asset is written once (`asset`).
  - The two structural rules therefore sit in the schema. Only "a ramp's nodes differ" is a Rust rule.
- **Inner and outer nodes.**
  - Following `REQ-CAT-17` and the glossary, an inner node is referred to by identifier.
  - An outer node is "named for its network and its reserve". In a reserve tuple the reserve is the advertising
    provider, so the object writes only `{network}` and the reserve is implied (0.2.0, after review). Its `network`
    is a plain identifier and isn't tied to the public node list.
  - Only reserve tuples touch an outer node. Exchanges and ramps run within and between inner nodes ("exchanges run
    inside one network", `REQ-CAT-21`).
- **A reserve's outer node is its own.** `REQ-CAT-17` names the outer node for "its reserve", so the reserve in a
  reserve tuple's outer node is the advertising provider. Since 0.2.0 that holds by construction.
- **Reserve direction.** It comes from which end is the outer node: outer to inner is a withdrawal, inner to outer a
  deposit (glossary, "Outer node, inner node"). There is no separate direction field that could disagree.
- **Reserve assets.** "A reserve states which assets it accepts for deposits or redeems" (`REQ-CAT-13`) is read as
  one asset per reserve tuple, the same at both ends, though its unit may differ (see "Units").
- **An exchange may convert an asset to itself.** `REQ-CAT-13` asks only that it state a from asset and a to
  asset, and `AC-29` includes "one conversion that changes no value". The contract doesn't require that the two
  differ.
- **Units.**
  - `REQ-CAT-11`'s "units" is read as the scale the provider counts each asset in, `decimals`: one whole asset is
    `10^decimals` units.
  - Amounts are whole numbers of that unit, written as decimal strings.
  - Bands are inclusive and are in the input unit: a ramp's or reserve's origin, an exchange's `from`. "Any fill
    increments" makes `increment` optional.
  - **Units at each end.** `REQ-CAT-11` asks a provider to declare "its units" per tuple, against `THR-107`'s "one
    value in different units". One asset can be counted differently on two networks, such as USDC with 6 decimals
    on one and 18 on another. A ramp or reserve keeps its asset but may change its unit, so it declares
    `decimals: {origin, destination}`, both always given. An exchange already gives `from` and `to` their own
    `decimals`. The review cited `REQ-CAT-12` for this point; the requirement that names units is `REQ-CAT-11`.
- **Required inputs.**
  - `REQ-CAT-12`'s "inputs a segment needs" is declared per tuple, because a segment runs through one tuple.
  - The list is always present: empty means none, so a provider can't leave the declaration out.
  - Inputs are named by dotted lower-case paths, such as `beneficiary.bank_account`. Their vocabulary is settled
    with the operations in `T-002-02`.
- **Evidence and retry are per provider.**
  - The evidence grade and its availability are per provider (`REQ-CAT-10`, and `DEC-13`'s "declared per
    provider").
  - The declared grade is the one the provider gives on every outcome, so its weakest. `REQ-ORD-39` says a
    provider below the operator's per-tuple floor "MUST NOT be routed through", so the floor needs a grade that
    holds for each segment the provider serves. A "best it can give" reading would let a provider whose strongest
    tuple passes the floor be routed on a weaker one. (0.1.0 used "best"; corrected in 0.2.0, after review.)
  - There is no per-tuple override. `DEC-13` fixes the declaration per provider, and a provider whose evidence
    differs by tuple, such as public against private nodes, declares its weakest or is advertised as two
    providers. If routing later needs per-tuple credit, adding an optional per-tuple grade is a contract change
    that `DEC-13` would have to allow first.
  - Availability is a number of seconds or `"unbounded"`, such as a public chain record.
  - Safe retry is one boolean per provider: `REQ-CAT-10`'s "whether a repeat is safe".
- **"An identifier or a status query".**
  - `REQ-CAT-16` is read as either of two things: each transaction carries an identifier (glossary, "Transaction",
    "uniquely identified"), or the provider answers a status query.
  - It is declared as `tracking`, with at least one entry.
  - "With no exceptions" makes it apply to verifiers too.
- **The reserve declaration.**
  - `REQ-CAT-6`'s declaration sits in the reserve offer. Without it, the offer is invalid.
  - "Attributable to its maker" is an explicit `maker`. "A later version that changes one MUST be a new declaration"
    is carried by `declared_in_version`, the provider version where the declaration was made, which may be earlier
    than the advertisement's `version`.
  - Recording it once and showing it before acceptance are catalogue behaviour (`F-012`), not shape.
- **Verifiers needn't declare evidence or safe retry.**
  - `REQ-CAT-10`'s evidence is evidence "of an outcome", and `REQ-ORD-39` grades "every segment's outcome". A
    verifier "moves no value" (glossary), advertises no tuple (`REQ-CAT-13`), and so serves no segment.
  - What a judgement's evidence and repeat behaviour would be is for ADR-0012 and `F-053` (`DEC-16`).
  - So `evidence` and `safe_retry` are required once an advertisement has a reserve, exchange or ramp offer, and
    are optional otherwise. Optional rather than forbidden leaves `F-053` room without changing the contract,
    which `DEC-16` wants ("v1 changes no contract"). The verifier role stays in the contract.
- **A holding account doesn't advertise through this contract.**
  - `REQ-CAT-23` has the operator register a public node's holding account "as a module outside the core", and a
    private node's publisher "give the operator a way to interact with" it. Neither is a provider advertising
    tuples under the operator's grant (`REQ-CAT-2`).
  - It stands as a reserve only as the origin of a route handed on (`REQ-ORD-16`, `REQ-ORD-43`, `INV-5`): "the one
    reserve at an inner node" in `REQ-CAT-17`. Even then it counts as the origin reserve, not as a routed tuple.
  - So reserve tuples keep connecting an outer and an inner node, and inner-to-inner reserve tuples stay invalid.
    The holding account's own shape, and how a handed-on route names it, belong to the catalogue (`F-012`) and the
    module interface (`F-003`).
- **No tuple twice.** `REQ-CAT-11` declares bands and the rest per tuple, which presumes one set of terms per
  tuple. An offer that lists one tuple twice is invalid, even with identical terms. The key is origin,
  destination and asset for ramps and reserves, and node, from asset and to asset for exchanges.
- **One offer per category.**
  - `offers` is keyed by category, so a provider has at most one offer per category, with any number of tuples in
    it.
  - `REQ-CAT-8`'s "a category attaches to what is advertised" holds because each tuple sits under its category.
- **Private nodes.**
  - `REQ-CAT-22` says "a provider MUST declare and publish its private nodes". A private node an advertisement
    declares is therefore published by that advertisement's publisher.
  - Since 0.2.0, `private_nodes` lists unique identifiers. The publisher behind each node is the advertisement's
    `publisher`, which the record shows (`REQ-CAT-20`).
  - 0.1.0's node schema is removed. The operator's public list, and the check that every node in a tuple is on one
    list or the other, belong to the catalogue (`F-012`), which defines that list's shape.
- **Verifier classes are identifiers.** Their vocabulary, like judgements, waits for `F-053` (`DEC-16`).
