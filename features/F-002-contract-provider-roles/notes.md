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

## T-002-02

- **One envelope for every party statement.**
  - `REQ-PRT-1` names deployment and protocol version. The envelope also carries `shape`, the `$id` of the
    statement's schema, so signed bytes can't be presented as another kind of statement.
  - It carries `maker` and `made_at`, since a statement stays attributed to its maker (`REQ-AUD-2`) and a
    judgement states its time made (`REQ-CPL-1`).
  - It carries `signature`, since a quote can't be denied (`REQ-QTE-1`) and a restatement must be detectable
    (`REQ-AUD-2`). The bytes it covers and the scheme were first left open, since neither SPEC nor the plan
    settles them. They are now ruled (see "Review rulings" below).
- **Shape is checked in Rust too.** Deserialising a `Statement<T>` doesn't check `shape`.
  `Statement::check_shape` does, for the types with a fixed shape. A reader calls it before trusting a
  statement, as the schema's `const` does.
- **Operations are statements too.** §3 lists the operator as a party, and `REQ-PRT-1` makes any party's
  statement void elsewhere. So an operation is the operator's statement, in the same envelope, and an operation
  replayed at another deployment is void there.
- **Kinds.** `REQ-CAT-9` names withdrawals, deposits and balance verifications for reserves, conversions for
  exchanges, transfers for ramps, and judgements for verifiers. These six are the operation kinds. `REQ-ORD-37`'s
  "route" operation is a ramp's transfer.
- **Dry run.** A dry run (`REQ-CAT-14`) is a flag on the value-moving operations.
  - It is answered by `dry_run_accepted`, or by a refusal for a segment the provider can't serve. Its class is
    usually `final`, and `REQ-CAT-18`'s other two classes apply as well.
  - Whether a result answers a dry run follows from the transaction it matches (`REQ-ORD-1`), so results carry no
    dry-run flag of their own.
  - A balance verification is never a dry run.
- **The detailed quote.**
  - `REQ-CAT-9`'s "detailed quotes with rates and fees" is the conversion's `dry_run_accepted`: amounts in and
    out, the rate, the fee and a validity. A ramp "exposes fees" in its own dry-run answer, without a rate.
  - `REQ-QTE-1`'s full quote, which also has direction, what voids it, slippage regime and settlement time
    (`REQ-QTE-3`), belongs to the router contract (`T-004-01`). It isn't added here.
  - `fee`, `rate` and `outcome_evidence` are `$defs` it can refer to.
- **Amounts in results.** A settled movement reports `amount_in` in the origin unit and `amount_out` in the
  destination unit, gross or net as the tuple declares (`REQ-CAT-11`), so the next party's report can be compared
  (`REQ-ORD-9`). The rate is whole to-assets per whole from-asset, before the fee, as a decimal string.
- **Outcome unknown.** It is a status of its own that carries nothing. Succeeded and failed each require evidence,
  graded on ADR-0007's scale. That is `REQ-ORD-1`'s "left only on stated evidence": a later result with evidence
  resolves an unknown one.
- **Refusal classes.**
  - `REQ-CAT-18`'s "lifts at a known time, waits on the person, or is final" are three classes:
    `lifts_at_known_time` (which states `lifts_at`), `waits_on_person` and `final`.
  - A refusal answers any kind of operation. A reserve's refusal of a balance check made outside a session
    (`REQ-CAT-3`) is one.
- **Balance verification.**
  - The operation names the session of the holder or their delegate (`REQ-CAT-3`), an account, an asset and the
    stated amount.
  - The answer is `sufficient`, true or false, and nothing else (`INV-9`).
  - A reserve that can't know yet waits (`REQ-CAT-3`), so there is no "unknown" answer.
- **Judgement, kept minimal (`DEC-16`).** Shaped per ADR-0012 and `REQ-CPL-1`, and the rest waits for `F-053`.
  - The request carries only a subject reference, one per relying provider, and a class. The verifier sees a
    person and a class only.
  - The judgement is a signed statement for one class: maker and time made from the envelope, plus subject
    reference, class, answer and validity.
  - The answer is `yes` or `no`. "Not known" and "not asked" are rutent's own states (`REQ-AUZ-5`, `REQ-CPL-1`),
    not a verifier's answer.
  - Holder keys, presentation and selective disclosure are not shaped here.
- **Fixture names.** Following the state-machine contract, a fixture's name starts with its shape. The 0.2.0
  advertisement fixtures were renamed with an `advertisement-` prefix.

### T-002-02 review rulings (ruled by the orchestrator, 2026-11-06)

- **Signing rule.**
  - The signed bytes are the RFC 8785 (JCS) canonical JSON of the whole envelope with only `signature.value`
    removed, so `scheme`, `key`, `shape`, `deployment` and `protocol_version` are all signed.
  - `scheme` is the one value `ed25519` (RFC 8032), verified strictly. `value` is base64url without padding.
  - Every integer that can be signed is at most 2^53 − 1, or is carried as a string.
  - Signature verification itself is `M2` and `M3` work. The contract adds `check_context` beside `check_shape`.
- **Makers' keys** are published in the catalogue. The enveloped advertisement carries the provider's public
  signing keys in `keys`, and the operator's grant (`REQ-CAT-2`) makes them authoritative.
- **Deployment identifiers** are globally unique and bound to the operator. `deployment` is `{name,
  operator_key}` (`THR-8`).
- **Party statements.** The advertisement and the reserve declaration are party statements (`REQ-CAT-6`,
  `REQ-PRT-1`). The advertisement is enveloped with a `shape` `const`. The declaration's `maker` must equal the
  envelope's `maker`, which Rust enforces (`ReserveDeclarationOfAnotherMaker`).
- **A reserve's signed statement for its own book** (`REQ-ORD-33`) is the result envelope. Evidence is a non-empty
  list of `{grade, reference, by}`, so the public record can sit beside it (ADR-0007).
- **The detailed quote.** The provider signs `REQ-QTE-1`'s direction, slippage regime and what voids the quote,
  and `REQ-QTE-3`'s settlement time, in the conversion and transfer quotes. The router contract `$ref`s them.
- **Withdrawal dry runs** reveal sufficiency (`REQ-CAT-4`), so they carry a session reference (`REQ-CAT-3`, R96),
  required on a dry run.
- **Balance refusals** carry no `lifts_at`, which would reveal timing (`INV-9`). Their classes are
  `waits_on_person` and `final`.
- **Judgement** (`DEC-16`).
  - The question carries no `transaction` (`REQ-CPL-4`).
  - `judged` carries a standalone enveloped judgement, `urn:rutent:provider-role:judgement`, with an optional
    `holder_key`. v1 can then hand it to its holder (ADR-0012) with no contract change.
  - `REQ-CPL-1`'s maker and time made are the envelope's. Subject reference, class and validity are fields.
- **`protocol_version`** has its own definition, `major.minor.patch`, apart from provider versions.

### Second review rulings (ruled by the orchestrator, 2026-11-06)

- **Advertisement maker and key binding.** An advertisement whose envelope `maker` isn't its `provider`, or whose
  `signature.key` isn't in its `keys`, is rejected (`AdvertisementOfAnotherMaker`, `SignedWithUnpublishedKey`).
- **Nested judgement.** `check_context` recurses into a judgement nested in a result, and its `maker` must equal
  the result's.
- **The reserve dry-run answer** states `voided_by` and `settlement_seconds`, since `REQ-QTE-1` and `REQ-QTE-3`
  apply to every segment.
- **Canonical base64url.** A 32-byte key's last character is one of `AEIMQUYcgkosw048`, and a 64-byte signature's
  one of `AQgw`, so each value has exactly one encoding.
- **Signing rule wording.**
  - The signed bytes leave out the top-level `signature.value` only.
  - The verifier canonicalises the JSON as received, never a re-serialisation of parsed types.
  - Duplicate member names are rejected.
- **An operation's signing key** is its deployment's `operator_key`, since operations are the operator's
  statements (`check_signed_by_operator`).

### Readings taken while applying the rulings

- **The advertisement's maker is the provider itself.** The advertisement carries "the provider's public
  signing keys", so the provider signs it with one of them. Its reserve declaration's `maker` is therefore the
  provider. The publisher stays named in the advertisement (`REQ-CAT-20`).
- **The judgement result names no transaction,** like the question (`REQ-CPL-4`). A judgement is matched by
  subject reference and class.
- **Slippage only where a rate is set.** `REQ-QTE-1` asks for a slippage regime "where its category sets a
  rate". So an exchange's quote states `slippage` and a ramp's quote has none. The regimes are:
  - `firm`;
  - `bounded`, with `max_bps`;
  - `floating`, for a segment past three days with no firm rate (`REQ-QTE-9`, `REQ-QTE-10`).
- **Direction on the operation too.** A quote's `direction` (`exact_in` or `exact_out`) only means something if the
  operation states which amount it fixes. So conversion and transfer operations carry `direction`, and their
  `amount` is in the unit it names. Withdrawals and deposits state the origin amount, as before.
- **What voids a quote** is a non-empty list drawn from:
  - `validity_passes`;
  - `amount_changes`;
  - `inputs_change`;
  - `rate_moves_beyond_slippage`;
  - `provider_stops_advertising`.

  SPEC names the element but not its vocabulary.
- **The withdrawal session is optional off a dry run,** since a reserve may start a route on an automatic
  deposit with no user present (`REQ-ORD-13`).
- **Rules across statements are Core's.** A dry-run answer to a real operation, an execution answer to a dry
  run, and a result whose maker isn't the provider asked all fail `REQ-ORD-1`'s "attributable to that segment's
  provider and matches that transaction". `CMP-CORE-ORCH` holds them when it matches a result to its operation.
