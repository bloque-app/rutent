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

## T-002-03

- **Where it lives.** The role suite is `conformance/src/role/`, in `rutent-conformance`, which depends only on
  `rutent-contracts`. The stand-ins are public in `role::stand_in`, so other suites and Core tests can use them
  (`REQ-MOD-4`).
- **The interface.** `RoleProvider` takes the contract's typed operation statements and returns its typed result
  statements, as the brief asks. It has four methods:
  - `advertisement()` gives the provider's statement.
  - `handle(...)` answers one operation.
  - `state()` gives an opaque fingerprint for the no-effect, repeat and undo checks.
  - `status(tx)` is an optional status query (`REQ-CAT-16`).
- **The harness is a separate trait** (ruled after the second review). `RoleHarness: RoleProvider` holds the test
  hooks `force_outcome_unknown()`, `force_failure()` and `credit_reversal(...)`, so a reserve's production
  adapter, which implements `RoleProvider` alone, can't fabricate the reserve's word (ADR-0008). `WithoutHooks`
  runs a provider with no hooks, and what they provoke is then uncovered.
- **The suite trusts `state()`.** "A dry run changes nothing" (`REQ-CAT-14`), "a repeat changes nothing"
  (`REQ-CAT-10`) and "a reversal returns the state" (`REQ-CAT-15`) are all checked by comparing fingerprints. A
  real provider in `M2` must give an honest one. The fingerprint is the value the provider has moved on and holds
  on its books: a success changes it, and a refusal, a dry run or a failure doesn't. A failed segment's value,
  held until its reversal, isn't part of it. The reserve stand-in's balances move with its withdrawals and
  deposits.
- **Some answers need a test hook.** A provider can't be made to report outcome unknown or a failure from
  outside, nor to receive a reversed credit. Without the hook, outcome unknown, a failure and its reversal, or a
  confirmed reversal stay uncovered, and the provider doesn't pass.
- **What "passes" means** (closed in review). A report passes with no findings, nothing uncovered and nothing
  deferred. Until `M2` every run defers `SignatureVerifies`, so the stand-ins pass but for that check
  (`passed_but_for_deferred`), and pass whole with a verifier that concludes. The suite derives the answers
  required from the advertisement, and reports any it didn't see as `Uncovered`:
  - per value-moving tuple: a dry run accepted, a succeeded execution (so every quote is compared with one), a
    refusal outside its terms, and a refusal for missing inputs where it declares any;
  - per exchange or ramp tuple, also: a failure, and its true reversal;
  - per reserve asset: a balance answer, and a reversal confirmed and one not confirmed;
  - per verifier class: a judgement, and a refusal of an unadvertised class;
  - for the provider: outcome unknown where it moves value, a resolving status query where it declares one, and
    a refusal outside its role.

  A provider that refuses everything breaks no rule but fails on coverage alone, with no hand-written list
  (`stand_in_that_refuses_everything_fails_on_coverage_alone`).
- **Where the suite's checks come from:**
  - Everything is derived from the advertisement: its tuples, its bands' edges, a real operation above them, a
    tuple with an unadvertised node or asset, missing inputs, the operations outside its role, balance checks
    and confirmations for a reserve, and judgements and an unadvertised class for a verifier.
  - An amount outside the bands, or a tuple the provider doesn't advertise, must be refused (`REQ-CAT-13`,
    `REQ-CAT-14`). So must any operation outside the role (`REQ-CAT-9`), a reversal included for a reserve or
    verifier, and a reversal confirmation for anyone but a reserve.
  - The evidence on a success, failure or reversal must reach the grade the provider declares, which is its
    weakest (`REQ-ORD-39`).
  - A nested judgement or confirmation goes through the same envelope, maker and key checks as its result.
- **Readings taken for the review's gaps:**
  - **Refusal classes.** `REQ-CAT-18` names the classes but not which applies when. A refusal of what the
    provider never serves (outside its bands, tuples or role, or a reversal of a segment it never served) must
    be `final`. A refusal for missing inputs must be `waits_on_person`, since the order waits for the customer
    (`REQ-CAT-12`). A balance check or withdrawal dry run outside the holder's session may be refused with
    either class a balance refusal allows.
  - **Status queries.** One that still answers outcome unknown doesn't resolve it, and is flagged.
  - **Safe retry.** Where `safe_retry` is true, the suite repeats an executed transaction once. The repeat must
    change nothing and be answered the same. Where it is false or absent, the suite never repeats one
    (`REQ-CAT-10`: "a retry MUST NOT be sent where the provider doesn't declare it safe").
  - **The provider honours its own quote's slippage** (the name ruled after the second review). The execution
    may not charge a higher fee than quoted, in another asset, or give less out, or at a lower rate, than quoted.
    A firm quote, and a ramp's or reserve's answer, which set no rate, allow nothing less. A bounded quote allows
    its own `max_bps`. A floating quote bounds the amount and rate not at all. Giving more than quoted isn't
    flagged. A quote must fix the direction asked. The operator's per-tuple slippage bound (`REQ-QTE-9`) is the
    router's to apply, not this suite's.
  - **A failure moves nothing on.** A failure that changes `state()` is flagged, like a success that doesn't.
  - **A success changes something.** A success that leaves `state()` unchanged is flagged as not an execution.
  - **Reversals** (revised after the second review). Every exchange and ramp tuple counts as reversible, as
    `REQ-CAT-15` implies: one that can't truly undo a segment must decline the segment, so the advertisement
    needs no reversal field. SPEC reverses failed segments (`AC-4`, `INV-3`, and the contract's own
    `operation-reversal-of-a-failed-transfer` fixture), so the suite drives a segment to fail through
    `force_failure` and asks for its reversal, in full, at its origin, in the asset it left. A success must
    leave the state as it was before the segment. A final refusal there is flagged, since the provider should
    have declined the segment. A non-final refusal or outcome unknown leaves the reversal owed (`REQ-CAT-7`) and
    uncovered. The suite also asks to reverse the settled segment. SPEC owes nothing there, so any refusal is
    allowed, but a success must still be a true undo. The exchange stand-in refuses it until a known time, which
    exercises `lifts_at_known_time` as a valid refusal (`REQ-CAT-18`).
  - **Confirmations** (revised after the second review). The suite asks about a credit that never arrived. Then
    it credits one through `credit_reversal` and asks about that credit, the same credit for one unit more (a
    short return), a fresh segment with the same asset and amount, and the credited segment in another asset.
    Only the exact credit may be confirmed ("only that specific credit's arrival", ADR-0008), and its statement
    must name the segment, asset and amount asked. No question may change state.
  - **Operations from elsewhere** (second, third and fourth reviews). Each foreign probe is sent several ways,
    since a provider can check one field of the envelope and not another:
    - under this deployment's name with another operator key, signed with that key;
    - under another deployment's name, with the operator's key;
    - naming another operator key in the deployment, signed with the operator's key;
    - under a higher major, a higher patch and a lower protocol version, since `check_context` asks for an
      exact match;
    - signed with a key other than the operator's.

    The probes cover:
    - a dry run and a real execution per tuple;
    - a replay of the executed transaction itself, so a provider that answers repeats from its records before
      checking the envelope is caught (`REQ-PRT-1`'s "replayed at another deployment");
    - the reversal of the settled segment and of a second failed one (left unreversed);
    - a reserve's in-session balance check and its confirmation of the credited segment;
    - a verifier's judgement question for each class it advertises. Where an honest answer would be yes, a provider that
    serves the foreign question would really give one (`INV-9`, ADR-0008). Anything but a final refusal is
    flagged under `RefusesForeignOperations` alone, whatever class a non-final refusal states, so one breaker
    trips one check. The stand-ins check `check_context` and `check_signed_by_operator` before anything else.
  - **The second failure is checked like the first** (fourth review). It counts as its own subject's
    `Failure`, and a hook that says yes without failing is flagged under `ExecutionReported`.
  - **A status query's success is held to the quote** (third review). The status probe receives the tuple's
    quote, and a success counts as the tuple's execution only once it has been compared with one.
  - **Certification goes through `WithoutHooks`** (third review). The report records whether a hook took
    effect (`hooks_used`), so a hooked run can't be passed off as the adapter's.
  - **A certification is two reports** (ruled by the orchestrator, for `F-031` to apply in `M4`). Both are on
    the same adapter build:
    1. a `WithoutHooks` run with `hooks_used` false, in which every check that needs no hook passes and only
       hook-only answers are uncovered;
    2. a hooked run against the provider's test mode or sandbox, where it has one, passing everything.

    The suite names the first case `Report::passed_without_hooks()`, so it is machine-checkable. It also
    requires nothing deferred, as `passed()` does. The hook-only answers are those
    `Exercised::needs_a_hook` lists: outcome unknown and its status query, a failure and its reversal, and a
    confirmed reversed credit. `certification_takes_a_run_without_hooks_and_a_hooked_one` tests both reports for
    each stand-in, and shows a hooked run, a deferred check, a silent provider and a broken rule each failing
    the first.
  - **What a true undo rests on.** The suite judges a reversal from what the provider reports, its node, asset,
    amount and evidence, and from its `state` fingerprint. It has no independent view of the origin node, and
    Core completes a reversal only on the origin reserve's confirmation (`REQ-ORD-3`).
  - **The other asset** a confirmation is asked about is another asset the reserve advertises, where it has
    one, and an unadvertised asset otherwise.
  - **The suite's protocol version.** `SuiteContext` defaults to the contract's `VERSION`, `0.4.0`. The
    contract's fixtures keep `0.3.0`, since their tests check each fixture against its own context and none
    compares it with `VERSION`. `T-006-01` moves both to `1.0.0`.
  - **The ramp's internal steps** are a log apart from its state, since a reversal returns the value, not its
    history.
- **INV-9 through typed results.** A balance answer carrying anything but yes or no can't even be built through
  the contract's Rust types. That case is covered on the wire by `T-002-02`'s schema and Rust tests. Here the suite
  checks that a balance check is answered or refused, only in the holder's session, and changes nothing.
- **A breaker per check.** Each of the 21 checks has at least one stand-in, or for `SignatureVerifies` a
  rejecting verifier, that is flagged by that check alone (`stand_in_that_breaks_its_role_is_flagged`).
- **Signatures are deferred.** The suite checks every envelope's shape, context and canonical encodings, and hands
  each signature to a `SignatureVerifier`. `DeferredVerifier` verifies nothing and the report counts the deferred
  ones. Verification lands in `M2`, and will need the statement's bytes as received, so the hook's signature will
  change then.
- **Internal steps.** The ramp with internal steps converts to internal units and back while serving a transfer.
  The suite sees only the segment and passes it, and the test checks the steps happened (`REQ-CAT-21`, `AC-30`).
- **Placeholder keys and signatures.** Stand-ins and the suite use well-formed, canonical placeholders. They are
  not real keys and sign nothing.

### The reversal operation (0.4.0)

Added in T-002-03 after review; T-002-02's operation list lacked it. `REQ-CAT-15`, `REQ-ORD-3` and ADR-0008 depend
on it.

- **Who reverses.** An exchange or a ramp, by a `reversal` operation. A reserve doesn't reverse: `REQ-CAT-15`
  expects it to be infallible, and a failed final deposit follows `REQ-ORD-43`.
- **What a reversal names.** Its own transaction, the transaction it `reverses`, and the segment's origin
  `node`, `asset` and full `amount`. That is `REQ-ORD-3`'s "back at the segment's origin node, in full and in the
  asset it left". It is never a dry run.
- **No `failed`.** "Only a true undo counts as a reversal" (`REQ-CAT-15`), so the outcome is `succeeded`,
  `unknown` or `refused`. A refusal leaves the reversal owed (`REQ-CAT-7`), and its class says when it may lift.
- **The provider's success is only its word.** `REQ-ORD-3` completes a reversal only on another party's
  statement. So a `succeeded` reversal carries evidence, and Core completes it on the origin reserve's
  confirmation (ADR-0008).
- **The confirmation.** A `reversal_confirmation` asks the reserve at the origin node whether one credit
  arrived. It names the segment's transaction, asset and amount, and no session or account, so it can't serve as
  a balance check (`REQ-CAT-4`). The answer is:
  - `confirmed`, carrying the reserve's own enveloped `urn:rutent:provider-role:reversal-confirmation`
    statement, with the same three fields and nothing else;
  - `not_confirmed`, carrying nothing; a short return is not a confirmation;
  - `refused`.

  Like a nested judgement, a nested confirmation must share its result's context and maker.
- **Its grade isn't in the contract.** ADR-0008 records the confirmation as "the reserve's word", graded on
  ADR-0007's scale by where the reserve operates. Grading is the evidence checker's (`F-015`), so the statement
  carries no grade.
- **One reserve per node.** ADR-0008 leaves value returning to a different reserve at the same node open. The
  confirmation is asked of the one reserve at the origin node.
- **No `null` (router review).** An optional field is present with a value or absent. `holder_key`, `session`,
  `increment`, `evidence`, `safe_retry` and the offers all refuse `null` in Rust as in the schemas, since the
  router contract embeds these types and promises never to write `null`.
