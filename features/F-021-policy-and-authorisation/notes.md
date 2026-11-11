# F-021 notes

Readings taken where `SPEC.md` or `PLAN.md` is silent or ambiguous, recorded per Narthex `REQ-SKL-24`.

## T-021-01

- **What the engine is.** `Engine<L, P, S>` in `components/core-authz/src/engine/` holds the log, the fixed rules
  (`Fixed`), the deployment's policy service (`P: Policy`) and the sources of a decision's inputs (`S: Sources`:
  verifiers' judgements, providers' answers). Every policy call reaches one consultation, `consult`, through
  `Asked`, which names each call's request. `Decisions::decide` is built here; each other policy trait is its own
  row's.
- **How a decision is reached.** The fixed rules are asked first, then the deployment's service, each in three
  steps: what the call needs, each input asked of its source in that order, then the ruling over the inputs given.
  An unreachable service gives not known (`policy_unreachable`), an unreachable source not known
  (`verifier_unreachable`), and either answering without the input a refusal (`missing_input`). Within one policy
  the first that fails decides, and later inputs are not asked. A yes is an input the service may refuse on, never
  the decision (`REQ-CPL-3`).
- **A refusal beats not known.** Between the fixed rules and the deployment's service, among the fixed rules, and
  in a pair `(A, B)` of policies, a refusal or a missing input from either is the answer even where the other can't
  be reached, and the first refusal asked is the one given. The deployment's service isn't asked once a fixed rule
  refused.
- **The fixed rules are the engine's.** `Engine::new` puts `Fixed` in front of the deployment's policy, so no
  assembly can leave a rule out. `FIXED` in `engine/mod.rs` lists them in order; `T-021-01` adds none, since its
  rules are the consultation itself. Each later row (`T-021-02` to `T-021-07`) adds its own entry there, and the lead
  sequences those edits, as `PLAN.md` §4.4 has it for a type two rows share.
- **One permit site.** `consult` is the only place in Core that writes `Answer::Permit`, and the other permitting
  answers (`SessionOutcome::Opened`, `ReadOutcome::Allowed`, `ChargeOutcome::Within`, `PersonOutcome::Allowed`) may
  be written only in `engine/`. A later row that answers a permitting session, read, charge or question takes it
  from the engine's verdict.
- **Authentication says who.** The engine reads nothing of the caller to permit; it hands the caller to policy as it
  came (`REQ-AUZ-1`, `AC-25`).
- **The log answers first.** The decide entry is appended before the answer is given, so an entry the log refuses
  (a `post_route` whose decision record isn't in the log, say) comes back as `Err` and no answer is issued. `decide`
  permits `move_value` on the rulings alone; holding it to a registered authorisation that covers it is
  `T-021-04`'s fixed rule, and until then the log refuses the effect (`move-rests-on-its-authorisation`).
- **`AC-17` against `REQ-AUZ-5`.** `AC-17` says an unreachable verifier gives a refusal; `REQ-AUZ-5` and
  core-internal's README make it not known, never a refusal and never a permit. The engine answers not known, and
  nothing moves either way. Pending on the owner's form as `spec-ac17-not-known`, default "not known"; the test
  keeps its `AC-17` tag. Unless the owner amends `AC-17`, `T-024-07`'s scenario test "unreachable verifier gives a
  refusal and nothing moves" (`PLAN.md` §4.4) contradicts the engine: the engine gives not known, so that test, as
  named, expects a refusal it won't get.
- **An unreachable provider** gives `verifier_unreachable`: core-internal's `NotKnownCause` has no cause of its own
  for a provider, and a provider answering eligibility stands where a verifier would (`REQ-CPL-6`). A cause of its
  own is a core-internal bump, `F-020`'s.
- **A decision's id is its request's**, as `T-020-01`'s stubs had it.
- **Sources hand over what is there to be had**: a judgement its holder presented (ADR-0012), or a provider's answer
  to a question policy allowed. The engine asks no one about a person; that goes through `ask_about_person` first
  (`T-021-07`).
- **`Unbuilt<L>` is `Engine<L, Absent, Absent>`**, a type alias, so the other rows' stubs, on `Unbuilt<L>` in their
  own modules, build unchanged and still permit nothing. A row that builds its call implements it on
  `Engine<L, P, S>`, reusing `consult` and `answer`, deletes its stub, and moves its line of the table in
  `every_policy_call_reaches_the_one_consultation_or_is_marked_unbuilt` from unbuilt to built.
- **The scan is a tripwire.** `engine/entry_points.rs` reads Core's three crates' source with syn (a dev-dependency,
  already locked): one permit site, effects and the outer libraries' carrying-out methods only in core-orch and only
  where a permit or the effect is in hand, no rename, alias or bare import of a watched name, no file mounted with a
  `path` attribute or `include!`. Names are matched without types. `CARRY_OUT` is pinned against every `&mut self`
  method of prov's platform and catalogue: each is listed as carrying out a decided action or as carrying out none,
  with why. The guard is the log: `T-020-01`'s split of its append surface (c60eb61) lets a decision's entry reach it
  only through policy's handle, `PolicyLog`, which the assembly hands to the engine alone, and orchestration's
  `Log` takes no decision kind. What the scan can't see (a value read through serde, where an effect's receipt came
  from) is that handle's, `T-020-08`'s and `T-025-05`'s.
- **The engine appends only through policy's handle.** `Engine<L, P, S>` asks only `L: PolicyLog`, so it holds no
  general handle and can't write anything but a decision. `record` builds the entry, checks it, makes it a
  `DecisionEntry`, which refuses any kind but a decision's, and appends it with `append_decision`, in that order, then
  answers. The engine with no policy is still `Unbuilt::new(log)`, as the scenario assembly builds it around a
  `PolicyHandle`; the engine with a deployment's policy is `Engine::with_policy(log, policy, sources)`, which puts the
  fixed rules first.

## T-021-02

- **What it builds.** `Sessions` and `Reads` on `Engine<L, P, S>` in `components/core-authz/src/session/`, each
  through `consult` and appended through `PolicyLog` before it answers. One entry joins `FIXED`; its two functions
  live in `session/`. The engine edits are the shared type's, made in this row's turn: three `Source` variants, the
  `FIXED` entry, the engine's `register` field, the two functions that take an open session and an allowed read from
  the verdict, the four rows of the table moved to built, two lines of the unbuilt test, and the scan's pin of
  permit sites.
- **A delegate is another party the holder names on that account.** core-internal's fixture
  `session-request-of-a-delegate` has the delegate as a party other than the holder. No contract call registers a
  delegation, so the fixed rule asks the holder, as an input (`Source::Holder { account, holder }`, the input the
  requester's party), and only a yes lets it through. A holder that says nothing gives a refusal, one out of reach not
  known (`verifier_unreachable`, the only cause core-internal has for an input's source). The requester's role isn't
  held to anything: the party is what holds the account.
- **The named holder must hold the account** (review of `d2604d3`, B1). The request names its holder, so the holder
  is held to the account's record (`Source::Account(account)`, the input the named holder), asked of every session,
  the holder's own included. Without it a party opened a session on another's account by naming itself, or an
  accomplice who vouched for it, the holder. The record is the reserve's, or the deployment's register of accounts,
  which the assembly answers.
- **Bounded in time.** An open session ends at its request's `until`, and the fixed rule refuses one asked to last
  longer than `LONGEST_SESSION_SECS`, 15 minutes, whoever asks. Default: a constant of the fixed rule, since the rules
  are plain functions of the request; a deployment's own bound is its policy service's to add, which can refuse
  shorter sessions but never open a longer one. A session's id is its request's, as a decision's is, so a retry opens
  the same session and the log takes one entry. An id already issued opens no other session, for another account,
  holder or end: the catalogue holds one session per id, so a second would take the first one's place and speak for
  an account its holder never opened it on. The rule reads the register, so it runs before the consultation, as the
  permissions' does. The register is in memory, so an id issued before a restart is the deployment's to keep
  (`F-030`).
- **The register the catalogue reads.** The engine keeps each session it opened, the account it speaks for and its
  end in its `Register`; `Register::open_in` hands them to the catalogue's `open_session`, its end taken down to the
  whole second, so the catalogue never holds a session past its end. Core's three crates import none of one another,
  so the assembly (`rutent-scenarios`, and `F-030`'s in production) calls it; that hand-off is `T-024-02`'s to wire.
  The register is in memory: what survives a restart is the deployment's (`F-030`).
- **Reads are held to what the log admits.** The fixed rule refuses on its face a regulator's or a checker's read
  naming no permission, a provider's of an order or the trail, or of a segment naming another provider, and a
  customer's of the trail. A customer's read of an order or its segment asks the order's record whether the reader
  is its requester, beneficiary or payer (`Source::OrderParties`), and a provider's of its segment asks the segment's
  record whether value moved through it (`Source::SegmentProvider`). The assembly answers both from the log, which
  holds the acceptance and the effect; were it to say yes wrongly, the log still refuses the read
  (`customer-read-is-of-its-own-order`, `provider-read-is-of-its-own-segment`), and nothing is answered.
- **A segment is read as one of the order the read names** (review of `d2604d3`, B2). A segment read names its
  order and segment apart, and neither the customer's rule nor `covers` tied the two, so a customer naming its own
  order beside another order's segment was allowed, and a permission over one order covered another's segments. A
  segment read now asks the segment's record (`Source::SegmentOrder(segment)`, the input the named order) whether it
  belongs to that order, for every reader but a provider, whose record (`SegmentProvider`) already names both. The
  log's half, `customer-read-is-of-its-own-order` and `covers` checking the order alone, is a core-internal change,
  `F-020`'s, raised with the contract's author.
- **A read naming a permission is held to the register.** The permission was granted, through `grant_permission`,
  to the reader's party over a scope that covers the read, and isn't revoked: the log's rows
  `permitted-read-rests-on-its-grant` and `permitted-read-is-not-revoked`, held by policy first. That rule reads what
  the engine itself decided, which a fixed rule, a plain function of the request, can't, so it runs before the
  consultation, beside the fixed rules: its refusal is the answer, and the deployment's policy isn't asked. A
  revocation is held to it the same way (`revocation-revokes-a-grant`). Grants are mirrored as the log takes them: a
  permission id granted twice holds both grants, and one revoked stays revoked.
- **A read is allowed or refused.** core-internal's `ReadOutcome` has no not known, so a read policy can't decide is
  refused, `not_permitted`. A cause of its own is a core-internal bump, `F-020`'s.
- **Two more permit sites, both taken from the verdict.** `SessionOutcome::Opened` and `ReadOutcome::Allowed` are
  written in `engine/mod.rs`, in `session_outcome` and `read_outcome`, each only on `Answer::Permit`, and the scan
  pins the three sites by name. A later row that answers a permitted charge or question adds its own the same way.
- **A refused revocation can't be recorded** (an item for `F-020`). `revocation-revokes-a-grant` cites every
  `revoke_permission`, whatever its outcome, so the log takes no entry of a revocation of a permission never granted,
  refused or not, and the call answers `Err`, nothing recorded. The read rows cite only permitted reads. Reading:
  the row should cite only a permitted revocation, which is a core-internal change, so it is `F-020`'s; until then
  the engine refuses it and the log keeps it out.
