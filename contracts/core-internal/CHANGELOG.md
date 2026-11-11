# core-internal changelog

## 1.0.0
- The interfaces between `CMP-CORE-ORCH`, `CMP-CORE-AUTHZ` and `CMP-CORE-AUDIT`, settled as a sixth contract before the
  three parts are built side by side (`PLAN.md` §3, §4.4). From orchestration to policy: one decision call for every
  action that moves value, posts a route, changes the catalogue or moves a stuck order's value, answering permit, refuse
  with its class or not known, with a permit carrying its decision entry's receipt; the authorisation naming one order,
  its origin reserve, its destination, its to asset and any floor, which every value-moving effect stays inside; a stuck
  order's decide and move steps as two calls (ADR-0011), resolving it by delivering its value or handing it on to a
  linked order, the decider's party never the mover's; sessions; a charge of work to the caller's accountable party,
  answered within or over its bound; a question about a person, with its asker, the person's reference, its authority
  (with the shortlisted providers on a shortlisted route) and the providers already asked about its order, answered
  allowed, refused or past the bound; a read check of an order, a segment with its provider, the whole trail, a named
  resource or a kept statement, recorded either way, and read permissions granted and revoked through policy. From
  orchestration to the log's checks: a segment's pair of reports, a holding account's reading and the bulk measures'
  records, each answered with a receipt and nothing else. From orchestration and policy to the log: an append answering
  with a receipt before the effect it records, admitting an effect only on an unused permit for a permitting entry of
  the log that decided the same action, a stuck order's value only as its decide step's resolution names it, the
  provider that moves it included, a result only of an effect of the log and only once, and every other entry only on
  the earlier entries it cites, since a permit can be built anywhere, and each action on one path only (a stuck order's
  move never a decision request, an operator command one of a closed list that needs no decision, and the operator's; an
  arrival filed anew a refiling, by the operator or its reserve); an evidence store keeping each statement's bytes apart
  from the log and handing them, by permission, to a party or a checker with the steps to check them; two shapes of
  entry, stated and verified, each with who, what, when, the source of when and the outcome; a stated entry attributed
  only where its signature verified, holding the key it was signed with and a salted commitment to its bytes, which a
  call keeps in the evidence store apart from the log, so no statement's content can be confirmed from the log; a
  verified entry for every record Core's parts write, the state machine's recorded inputs and orchestration's own
  records included (the order's parties, a settlement-account beneficiary among them), each record a state-machine input
  cites joined to it by the input's id; the platform's events, the proxy's uses and each segment record handed over with
  their place in their drain. From the checks to the operator: a raise carrying a difference and nothing that can move
  value, answered only by whether it was delivered (ADR-0009). Every shape's check recurses, so a value built in memory
  is held to the rules a value read from the wire is. Twenty-six schemas, 105 valid fixtures and 105 invalid ones, each
  breaking one rule, and 219 cases of the log's admission of an entry, from one coherent log, against a citation table
  of 49 relations derived from SPEC.md, each with the ids that demand it. An acceptance given cites its origin reserve's
  yes and the user's session; a shortlist's decision record names its shortlisted providers. (DEC-2, DEC-4, DEC-20,
  T-020-01)
- Folded into 1.0.0 before its release: the log takes entries through two handles. `Log::append` takes a
  `GeneralEntry`, which holds no decision kind, and is orchestration's and the checks'; `PolicyLog::append_decision`
  takes a `DecisionEntry`, which holds only decision kinds, and is policy's alone. Neither type can be built or read
  holding the other's kinds, and `admit_entry` takes the path an entry came by and refuses a decision kind on the
  general one, so no part but the policy engine can write the decision an effect is admitted on. Twenty-two of the
  admission cases name the path. (DEC-2, DEC-4, DEC-20, REQ-AUZ-1, INV-7, T-020-01)
- Folded into 1.0.0 before its release: the log, not the writer, attributes a stated entry. A `GeneralEntry`
  refuses a stated entry attributed `verified`, so a statement reaches the general handle naming only its claimed
  maker and its bytes' commitment. `attribute` records it verified only where the bytes the evidence store keeps
  under that commitment are an envelope of this deployment, by that maker, of the entry's shape, time, key and
  transaction, whose signature verifies under a key that signs the maker's statements at its `made_at`, and
  otherwise unverified; it refuses an entry whose bytes aren't kept. The log holds three new traits for it:
  `KeptStatements`, the evidence store's read by commitment; `StatementKeys`, the deployment and the makers' keys
  from the catalogue, never from orchestration; and `Signatures`, the signature check. Twenty attribution cases,
  under `fixtures/attribution/`. (DEC-4, DEC-20, REQ-AUD-2, REQ-PRT-1, THR-3, T-020-01, T-022-01)
- Folded into 1.0.0 before its release: a reserve's yes is bound to its order through its signed transaction. A
  statement about an order or a segment is verified only where its `about` names the transaction its body names;
  `acceptance-rests-on-the-reserves-yes` requires that transaction; and a new none-other row over stated entries,
  `transaction-belongs-to-one-order` in `STATED_RELATIONS`, refuses a later entry naming a transaction under
  another order than the first. `Relation`, `Cites` and `Answers` take the citing entry's kind as a parameter,
  verified by default. (DEC-4, DEC-20, REQ-ORD-32, REQ-ORD-38, T-020-01)
- Folded into 1.0.0 before its release: a statement is cited only for what it is about, and a yes follows its
  question. Core appends each question it asks, an `operation` signed with the operator's key and about the order
  and transaction it asks under, before its answer; `answer-follows-its-question`, a second row of
  `STATED_RELATIONS`, admits a provider's result naming a transaction only after that question and about its
  order, so a genuine yes can't be bound to another order first. Every row that cites a statement (the sent and
  received reports, an arrival's report, graded evidence, what came after an order ended, a reliance's judgement)
  holds the order, segment and transaction its `about` names to the citing entry's, a transaction by the order it
  belongs to; a statement about nothing stays citable on its body. A judgement, which carries no transaction, is
  appended about no order and bound by its reliance. 206 admission cases and 19 attribution cases.
  (DEC-4, DEC-20, REQ-ORD-32, REQ-ORD-38, REQ-ORD-39, REQ-CPL-12, REQ-AUD-2, T-020-01)
- Folded into 1.0.0 before its release: a statement naming a transaction is bound through it. `attribute` verifies
  a statement whose body names a transaction only where its `about` names the same one, so no result is verified
  about nothing and none escapes `answer-follows-its-question` or the `about` checks; a judgement, which names no
  transaction, stays about nothing. `transaction-belongs-to-one-order` fixes the segment the first entry names as
  well as the order, and a cited statement's transaction must belong to the citing entry's segment where it names
  one. `answer-follows-its-question` compares the question's verified party with the deployment's operator party,
  which `admit_entry` now takes as a fourth argument and `StatementKeys::operator` answers; the stated rows read a
  `StatedCiting`. A customer's segment read names a segment its order's plan lists, and only a permitted
  revocation must revoke a grant, so a refused revocation of a permission never granted is recorded.
  (DEC-4, DEC-20, REQ-ORD-32, REQ-ORD-38, REQ-AUD-2, REQ-AUZ-7, T-020-01)
- Folded into 1.0.0 before its release: graded evidence claiming a signature rests on verified statements. Every
  statement a grade-2 or grade-3 evidence entry cites must be one the log verified; a grade-1 public record rests on
  the network and a grade-4 bare statement claims no signature, so neither is held to it. 219 admission cases and 20
  attribution cases. (DEC-4, DEC-20, REQ-ORD-39, REQ-AUD-2, T-020-01)
