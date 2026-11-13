# provider-role changelog

## 3.1.0
- Docs (`T-020-18`'s review, item 4): two arrival notices under different transactions are two signed claims, each
  attributable to its reserve, which neither this contract nor the log can tell from two real credits; a reserve
  stating one credit twice under two references bears the loss (`arrival.rs`, the README's "Arrivals"). No shape or
  rule changed. (DEC-4, DEC-20, REQ-AUD-2, REQ-ORD-23, T-020-18)
- An arrival question names its asset with its unit, and an answer is held to both (`T-020-18`'s review, item 3).
  `check_answer` compared a notice's asset by its id alone, so a notice of the asset counted in another unit, whose
  amount means another quantity, answered the question. `ArrivalQuestion.asset` is now a `Denomination` (the schema's
  `asset` the advertisement's `denomination`), and `check_answer` compares the whole denomination (`InAnotherAsset`).
  One invalid fixture, `answered-arrival-question-in-another-unit`; the question fixtures name `COP` at 2 decimals.
  (DEC-4, DEC-20, REQ-ORD-1, REQ-ORD-23, REQ-CAT-11, T-020-18)
- A record of inner segments is made by a provider stating for its publisher (`T-020-18`'s review, item 2). A
  publisher holds no key, so `check_inner_segments` requiring its maker to be the publisher refused every record a
  real publisher could sign. `check_inner_segments(record, advertisement) -> Result<(), Vec<InnerSegmentsViolation
  {NotItsMakersAdvertisement, OfAnotherPublisher, NodeNotDeclared}>>` now takes the maker's advertisement, the one
  the catalogue accepted last for it: the advertisement is the maker's own, its `publisher` is the record's, and it
  declares the record's `node` among its `private_nodes`. `OfAnotherMaker` is gone. The label stays
  `publishers_word`. The record fixtures are made by `acme-internal-rail` for `acme` at `acme-ledger`; a bundle,
  `published-inner-segments-` (an advertisement and a record), with one valid fixture and three invalid, replaces
  `inner-segments-made-by-another-party`. (DEC-4, DEC-20, REQ-ORD-41, REQ-ORD-3, REQ-CAT-20, REQ-CAT-22, T-020-18)
- A publisher's record of the segments inside its private node is a signed statement of its own (`F-015`'s and
  `T-020-09`'s gap: `CMP-EVIDENCE`'s replay read a record no contract shaped, which the assembly's `Books` filled by
  hand). One schema, `inner-segments` (`urn:rutent:provider-role:inner-segments`, `INNER_SEGMENTS_SHAPE`,
  `InnerSegments {node, publisher, standing: Standing {PublishersWord}, segments: Vec<InnerSegment {transaction,
  asset_in, amount_in, asset_out, amount_out, fee}>}`), made by a provider stating for its
  publisher, held to its advertisement by `check_inner_segments` (the bullet above). `standing` is a label that travels with the record, always
  `publishers_word`: inside a private node there is no other witness, so a replay shows consistency with the public
  exit and never that the segments happened (`REQ-ORD-3`, `AC-35`, `THR-131`). Each segment names the transaction it
  ran under, as `Books::recorded` asks by node and transactions; the record names none of its own, so its stated
  entry is about nothing. The contract refuses no figure: a record whose segments don't add up or don't chain is
  still the publisher's record, and the replay records the mismatch (`REQ-ORD-41`). Minor, folded into 3.1.0 before
  its release: a shape and a check added, nothing changed. Two valid fixtures, twelve invalid. (DEC-4, DEC-20,
  REQ-ORD-41, REQ-ORD-3, T-020-18)
- The operator's question to a reserve of what arrived for an order, and the reserve's answer, are signed statements
  of their own (`T-020-04`'s gap). Core bound an arrival to its order on a balance verification, which says only that
  funds suffice. Two schemas:
  - `arrival-question` (`urn:rutent:provider-role:arrival-question`, `ARRIVAL_QUESTION_SHAPE`, `ArrivalQuestion
    {transaction, provider, session, account, node: OuterNode, asset: Denomination}`), signed with the deployment's operator key
    (`Statement<ArrivalQuestion>::check_signed_by_operator`, the operation's `NotSignedByOperator`). It names its own
    transaction, fresh for the order it is asked for, and no order, as no operation does; and the holder's session,
    since what arrived at an account tells whether its funds suffice for that much (`REQ-CAT-3`, `REQ-CAT-4`).
  - `arrival-answer` (`urn:rutent:provider-role:arrival-answer`, `ARRIVAL_ANSWER_SHAPE`, `ArrivalAnswer {transaction,
    outcome: ArrivalOutcome {Arrived(ArrivedCredits {notices: Vec<Statement<ArrivalNotice>>}), NothingArrived,
    Refused(BalanceRefused)}}`), made by the reserve asked. It names the question's transaction, so its stated entry
    is about it (`REQ-ORD-1`), and carries the reserve's notice of each credit whole: the same notice it sends of its
    own accord, under the question's transaction. Its notices are checked as nested statements (`check_shape`,
    `check_context`: `NestedMaker`), and a refusal never says when it lifts (`INV-9`). A reader keeps each notice
    it carries as its own stated entry, about its credit's transaction, which an arrival cites: a deposit is keyed
    on a notice, never on an answer (core-internal's `arrival-names-its-notice`, `T-020-18`'s review, item 1).
  - `check_answer(question, answer) -> Result<(), Vec<AnswerMismatch {AnswersAnotherQuestion, OfAnotherReserve,
    AtAnotherNode {notice}, InAnotherAsset {notice}, ToAnotherAccount {notice}}>>` holds the answer to the question.

  Shapes of their own, not an operation kind and a result kind, so `Operation` and `ResultBody` keep their variants:
  a kind would be a major (3.0.0's ruling), carrying module-interface, released at 4.0.0, to a major with it. Minor,
  folded into 3.1.0 before its release: two shapes and two checks added, nothing changed. Six valid fixtures, seventeen
  invalid (`answered-arrival-question-` is a bundle of a question and its answer). (DEC-4, DEC-20, REQ-ORD-1,
  REQ-ORD-23, REQ-CAT-3, REQ-CAT-4, INV-9, T-020-18)
- A reserve's notice of value arriving is a signed statement of its own (`T-020-04`'s gap). Before it, provider-role
  had no statement a reserve makes of value arriving, so Core took the outer node, asset, amount, account and holder
  from the assembly's port and kept the reserve's notice as bytes it read only for the transaction its body names.
  One schema, `arrival-notice` (`urn:rutent:provider-role:arrival-notice`, `ARRIVAL_NOTICE_SHAPE`, `ArrivalNotice
  {transaction, node: OuterNode, asset: Denomination, amount, account, holder}`), made by the reserve. Its
  `transaction` is the one the reserve gives the credit, its per-credit reference, the same in every notice of the
  credit: the deposit's signed transaction core-internal's log already keys on (`arrival-names-its-notice`,
  `one-order-per-deposit`), so the notice's stated entry is about it. `check_one_credit(earlier, later) ->
  Result<Credits {One, Two}, CreditRestated {OtherFigures}>` knows two notices of one credit, signed at different
  times, as one: the same maker, the same transaction, the same figures (`ArrivalNotice::same_figures`); another
  maker or transaction is another credit, and the same transaction with other figures is refused. Minor, folded into
  3.1.0 before its release: a shape and a check added, nothing changed. Three valid fixtures, eleven invalid
  (`arrival-notices-` is a bundle of two notices). (DEC-4, DEC-20, REQ-ORD-14, REQ-ORD-23, REQ-AUD-2, T-020-18)
- A hold names its own `transaction`, its id (the review of round 3, item 2), required in the schema and `Hold`, so
  Core appends its stated entry about it and the log keys the hold on it: a hold kept again under a new commitment is
  the same hold. One invalid fixture, `hold-without-its-transaction`. (DEC-4, DEC-20, REQ-QTE-15, T-020-01)
- A valid fixture, `hold-of-another-records-candidate-of-the-same-id`: a hold of `c1` of record `dr-0099`, which
  router-api's `HeldQuote::check_hold` refuses for the choice round's `c1` of `dr-0004` (the review of round 3,
  item 1). No shape or rule changed here. (DEC-4, DEC-20, REQ-QTE-15, T-020-01)
- A provider's terms and a reserve's hold are signed statements of their own (round 3 of the contract, item q).
  Core kept a provider's fee terms and a held quote only in memory, with nothing signed behind them. Two schemas:
  `terms` (`urn:rutent:provider-role:terms`, `TupleTerms {tuple: TermsTuple {provider, from, to: TupleEnd {node,
  asset}}, fee: FeeFormula {flat, proportional: Proportion {numerator, denominator}, maximum}, slippage}`), made by
  the tuple's provider, its slippage regime present exactly where the tuple sets a rate (`check_terms`,
  `TermsViolation`); and `hold` (`urn:rutent:provider-role:hold`, `Hold {record: HeldRecord {id, digest:
  RecordDigest}, candidate, for: HeldFor, valid_until}`), made by the reserve that starts each part, redeemed only
  for its order or an order accepted in its session (`check_redemption`, `RedemptionRefused`). Shapes
  `TERMS_SHAPE` and `HOLD_SHAPE`. Terms bind from the later of `made_at` and Core's `received_at`. The fee formula
  and the tuple's ends restate router-api's in this contract's own atoms, since this contract refers to no other;
  router-api's `fee_formula` and `endpoint` keep their own definitions. Minor, folded into 3.1.0 before its release:
  two shapes added, nothing changed. Four valid fixtures, seven invalid. (DEC-4, DEC-20, REQ-QTE-5, REQ-QTE-6,
  REQ-QTE-15, T-020-01, T-020-07)
- "Less the charge" has a stated unit (round 3 of the contract, item s). Under a tuple that reports `gross`, a
  segment that keeps its asset delivers its `amount_out` less its charge; `amount_out` is in the out unit and the fee
  in the origin unit, which differ where a reserve's two ends count in different decimals, so Core's reading
  (`segments::judge::handed_on`) subtracted unlike units. The charge subtracted is the fee restated in the
  operation's out asset's unit: multiplied out exactly where it is finer, divided and rounded up where it is
  coarser, so what arrives is never overstated. Two functions, `charge_in_out_unit(fee, origin_decimals,
  out_decimals)` and `less_the_charge(amount_out, fee, origin_decimals, out_decimals)`, which router-api's
  confirmation rule reads, with unit tests across 2, 6 and equal decimals. The fee's own unit, the origin unit, and
  `check_amounts` are unchanged. Minor, folded into 3.1.0 before its release: two functions added. (DEC-4, DEC-20,
  REQ-CAT-11, INV-1, T-020-01)
- A failed result may show whose fault the failure was, the customer's or the operator's: an optional `fault` on
  the `failed` outcome (`REQ-ORD-11`). Until the party it names accepts it, the provider carries the reversal.
  Before it, no shape carried it, so `T-020-12`'s Core read a stopgap prefix (`fault:`) in an evidence piece's
  reference. Minor, folded into 3.1.0 before its release: an optional field, absent on the wire where not shown. A
  valid and an invalid fixture. (DEC-4, DEC-20, REQ-ORD-11, T-020-01, T-020-12)
- A fee may say whether its provider bills it separately, outside the amounts it reports: an optional
  `billed_separately` on the result's `fee`. Absent means the provider doesn't say. Before it, no shape carried it, so
  a fee record (`REQ-AUD-11`) had to state what no provider said. Minor: an optional field, absent on the wire where
  not given, so a 3.0.0 statement reads the same. A valid and an invalid fixture. The protocol version moves with
  it, and `check_context` matches it exactly, so a statement made under 3.0.0 is refused under 3.1.0: a provider
  re-signs its statements, its advertisement among them, under 3.1.0. (DEC-4, DEC-20, REQ-AUD-11, T-020-09)

## 3.0.0
- Breaking: a holding account's balance gets its own operation and result, `holding_balance`, so `CMP-CORE-AUDIT`
  reads the balance from the account's module and not from the host's own record of what it sent it. Before, no
  shape carried a balance (a `balance_verification` answers yes or no for a stated amount, `INV-9`), so the plain
  wallet's host side could report only the sum it had itself recorded, and a wallet that lost its record (a
  restarted provider process) still looked whole. The operation names the provider, the node and the asset, and no
  session: it is the operator's own question and changes nothing. The result is the balance in whole units of the
  asset, zero included, or a refusal with its class. `Operation` and `ResultBody` gain a variant and the schemas
  gain the kind, so a reader on 2.x rejects it on the wire and an exhaustive Rust match breaks: a major (`DEC-4`).
  Valid and invalid fixtures, and Rust tests. Fixtures keep naming protocol 2.0.1. (DEC-4, DEC-20, T-016-02)

## 2.4.0
- A verifier gets the statement's bytes as received: `signing_input(received)` gives the bytes a maker signs under
  the signing rule, the RFC 8785 canonical form of the statement's text with only the top-level `signature.value`
  removed, refusing text that isn't an envelope (`NotAnEnvelope`) or isn't I-JSON. `member(json, path)` gives a
  nested statement's own text exactly as it sits in the outer one, such as a judgement in a verifier's result.
  The role suite's `SignatureVerifier` is handed those bytes (`T-010-03`; F-002's notes: the verifier canonicalises
  the JSON as received, never a re-serialisation). Minor: two functions added, no schema, type or rule changes.
  (DEC-4, DEC-20, T-010-03)

## 2.3.0
- One canonical JSON encoder, `provider_role::canonical`, written to RFC 8785, the scheme the signing rule and the
  host contract's `exactly-once` name: `canonicalize(json)` and `to_canonical(value)`, which refuses a value holding
  NaN or an infinity (`NonFiniteNumber`) before `serde_json` could write it as `null`. Members sort by their names'
  UTF-16 code units; numbers are written as the doubles they denote, as ECMAScript writes them, ties to even; strings
  escape only `"`, `\` and the C0 controls. Its own parser refuses duplicate member names, lone surrogates,
  noncharacters (RFC 7493 §2.1), numbers beyond a double and nesting past 128, where `serde_json::Value` would keep
  the last of two members silently. It passes RFC 8785's §3.2.2, §3.2.3 and Appendix B vectors, the NaN and Infinity
  rows included, and expands a number's exact value only where an even neighbour of its shortest digits reads back, so
  a long array of short numbers costs no expansion. Before, both rules named RFC 8785 and no code in `contracts/`
  wrote it; the VOS program's dedup and the native client wrote `serde_json`'s own form (`EXP-1` outcome, finding 7).
  Minor: added, no schema, type or rule changes, and the host contract's wording now has the encoder it names. (DEC-4,
  DEC-20, DEC-26, T-010-03)

## 2.2.0
- The Rust code behind this contract (`contracts/src/provider_role`) is `no_std` with `alloc`:
  it uses `core` and `alloc` paths, and the crate's
  default `std` feature turns on `std` only for its dependencies and tests. Minor, not major: no schema, Rust type or
  behaviour changes, and every consumer that builds with default features builds unchanged. Fixtures keep naming
  their current version. (DEC-4, DEC-20, DEC-26, T-010-01)

## 2.1.0
- After review round 1 on the M1 pull request (findings 3 and 6): `check_amounts(operation, result, terms)`
  publishes the rules that tie a result's amounts to the operation it answers, and returns the segment's
  `Remainder`. A result's `amount_in` is what left the origin, the charge included, whether the tuple reports gross
  or net. Its fee is in the asset the segment takes in (a conversion's `from`), in the origin unit, and at most
  `amount_in`, as the router counts it. For `exact_in`, and always for withdrawals and deposits, `amount_in` is at
  most the stated amount; the difference is the remainder at the origin node, below the tuple's fill increment, or
  zero where it declares none (`INV-1`, `REQ-CAT-11`, `REQ-QTE-5`, `THR-108`). Dry-run answers are held to the same
  rules. `answered-operation-` bundle fixtures; the role suite applies the check. Minor, not major: no schema or
  Rust type changes, every valid fixture stays valid, and the fee rule asks of a result what the router already
  asks of every quote and operation. Fixtures keep naming 2.0.1. (DEC-4, DEC-20, T-002-03)

## 2.0.1
- The newer-than check for `made_at` now fails closed: a time that isn't in the schema's form (UTC `Z`, at most 9
  fraction digits, so no `+05:00` offset) is never newer, instead of being compared as text. The README adds that
  the catalogue refuses an advertisement made more than a bounded skew ahead of its own clock. Every fixture's
  `protocol_version` names 2.0.1. (DEC-4, DEC-20, T-002-03)

## 2.0.0
- Breaking, after the M1 security review: every advertised key states its `purpose`, required, `advertising` or
  `statements`. An advertisement is signed with an `advertising` key it lists (`SignedWithStatementsKey`), and a
  key is listed once by its bytes, whatever the purposes (`KeyListedTwice`). `validate_successor(previous, next)`
  checks a later advertisement against the accepted one: same provider and deployment, made after it
  (`SuccessorNotNewer`, so a replayed older advertisement can't undo a key's removal), signed with a key the
  accepted one lists for `advertising`, scheme and bytes, and promoting none of its `statements` keys. So a module's outside part,
  which resolves its statement key, can no longer sign an advertisement that adds a key and then sign its
  successor release with it, nor rewrite the offers. The README states what the catalogue (`F-012`) must hold:
  the first advertisement's keys by the operator's grant, no replay of an older advertisement or a dropped
  statements key, and a key's purpose across the whole history, every deployment and every provider; and that the
  credential proxy (`F-014`) resolves only a release's declared statements key.
  (DEC-4, DEC-20, T-002-03)
- Breaking: every key in the Rust envelope and types is a `PublicKey`: `Deployment.operator_key`,
  `Signature.key`, `SigningKey.public_key` and a judgement's `holder_key`. It is built only from the canonical
  encoding and compares by its decoded bytes, so an alias with nonzero unused bits can't pass a "differs from"
  check; the docs state that the verifier (`M2`) must refuse non-canonical base64 and non-canonical points.
  Fixtures name protocol version 2.0.0. (DEC-4, DEC-20, T-002-03)

## 1.0.0
- Frozen at C1: from here a change needs a version bump, a changelog entry naming a DEC-*, and the owner's click. (DEC-3, DEC-4, DEC-20, T-006-01)

## 0.4.0
- The reversal operation, added in T-002-03 after review, since T-002-02's operation list lacked it: an
  exchange's or ramp's `reversal` of a segment, answered by a true undo with evidence, outcome unknown, or a
  refusal that leaves it owed, never a failure (REQ-CAT-15, REQ-ORD-3); and the reserve's
  `reversal_confirmation`, answered by its own signed statement (`urn:rutent:provider-role:reversal-confirmation`)
  that one credit arrived, naming only the segment's transaction, amount and asset, or by `not_confirmed`.
  A nested confirmation shares its result's context and maker. (DEC-4, DEC-14, ADR-0008, T-002-03)
- No optional field accepts `null`: a judgement's `holder_key`, a withdrawal's `session`, a tuple's `increment`,
  the `evidence` and `safe_retry` declarations and each offer are either present with a value or absent, in Rust
  as in the schemas. (DEC-4, T-002-03)

## 0.3.0
- Operation and result schemas per role, with the statement envelope every party statement travels in:
  withdrawal, deposit, transfer, conversion with a detailed quote, balance verification and judgement; dry runs;
  outcome unknown as its own result; refusal classes. Fixtures are now named by shape. (DEC-4, DEC-16, T-002-02)
- Tightened after review: the signing rule (RFC 8785 canonical JSON, Ed25519, integers at most 2^53 - 1); a
  deployment bound to its operator's key; the advertisement enveloped, with the provider's signing keys; a
  standalone enveloped judgement with an optional holder key, and a judgement question without a transaction;
  evidence as a list naming whose each piece is; the signed quote's direction, slippage regime, what voids it and
  settlement time; a session on withdrawal dry runs; no lifting time on balance refusals. (DEC-4, T-002-02)
- Makers bound to keys: an advertisement is made by its provider and signed with a key it publishes; an
  operation is signed with its deployment's operator key; a nested judgement shares its result's context and
  maker. Keys and signatures have one canonical encoding. A reserve's dry-run answer states what voids it and
  its settlement time. The signing rule says the top-level signature.value, the JSON as received, and no
  duplicate member names. (DEC-4, T-002-02)

## 0.2.0
- Tightened after review: a tuple listed twice is invalid; the evidence grade is the provider's weakest; private
  nodes are unique identifiers and an outer node names only its network, both implied by the advertisement; ramp
  and reserve tuples declare their unit at each end; a verifier-only provider needn't declare evidence or safe
  retry; the node schema is removed. (DEC-4, DEC-13, DEC-16, T-002-01)
- Integers must be plain JSON integers, which the Rust types enforce and the README records; a reserve tuple
  listed twice gets its own invalid fixture. (DEC-4, T-002-01)

## 0.1.0
- Advertisement and declaration schemas: the four categories, tuples per role, private nodes and their
  publisher, per-tuple terms, evidence, safe retry, tracking, required inputs and a reserve's declaration.
  (DEC-4, DEC-13, DEC-16, T-002-01)
