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
  breaking one rule, and 191 cases of the log's admission of an entry, from one coherent log, against a citation table
  of 47 relations derived from SPEC.md, each with the ids that demand it. An acceptance given cites its origin reserve's
  yes and the user's session; a shortlist's decision record names its shortlisted providers. (DEC-2, DEC-4, DEC-20,
  T-020-01)
- Folded into 1.0.0 before its release: the log takes entries through two handles. `Log::append` takes a
  `GeneralEntry`, which holds no decision kind, and is orchestration's and the checks'; `PolicyLog::append_decision`
  takes a `DecisionEntry`, which holds only decision kinds, and is policy's alone. Neither type can be built or read
  holding the other's kinds, and `admit_entry` takes the path an entry came by and refuses a decision kind on the
  general one, so no part but the policy engine can write the decision an effect is admitted on. Twenty-two of the
  admission cases name the path. (DEC-2, DEC-4, DEC-20, REQ-AUZ-1, INV-7, T-020-01)
- Folded into 1.0.0 before its release: promoting a staged release is a catalogue change of its own, `promote`,
  about one module release, so it names the version it puts in force. Before it, no decided action carried the
  platform's promotion, so policy could not decide who may promote, a permission `REQ-AUZ-6`'s list of separate
  permissions takes in (`REQ-MDL-4`, ADR-0014). One valid fixture added. (DEC-4, DEC-20, REQ-AUZ-6, T-021-03)
