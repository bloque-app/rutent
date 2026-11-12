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
- **Bounded in time, from Core's clock** (review of `d2604d3`, S1). An open session ends at its request's `until`,
  which must fall after Core's now and no more than `LONGEST_SESSION_SECS`, 15 minutes, after it, whoever asks. The
  bound was first measured from the request's `at`, so a request stating a moment a year on opened a session ending
  then. The engine now reads its own clock (`session::Clock`, the host's `SystemClock` unless `Engine::set_clock`
  hands it another), never a time the request states, before the consultation, as it reads the register. The trait
  mirrors `T-020-05`'s `snapshot::Clock`, which core-authz can't import (`DEC-2`); the assembly can hand both the one
  clock. Default: the bound a constant; a deployment's own bound is its policy service's to add, which can refuse
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
  holds the acceptance and the effect; were it to say yes wrongly, the log still refuses the read as allowed
  (`customer-read-is-of-its-own-order`, `provider-read-is-of-its-own-segment`), and the engine records and answers
  it as refused (review of `d2604d3`, S2: it first answered `Err` and recorded nothing on that path).
- **Before the assembly answers the sources** (`T-024-02`; review of `d2604d3`, S3). Fail-closed only where an input
  is needed. Every session needs one (`Source::Account`), so none opens, and these reads need one, so each is
  refused: a customer's of an order or a segment, a provider's of its segment, any segment read by a permission
  (`SegmentOrder`), and any read of a kept statement without a permission (`Statement`). The reads that need no
  input are decided by the register and the deployment's policy alone, and the deployment's policy may allow them:
  a customer's or a provider's read of who the checker is, and a regulator's or a checker's read by a permission granted
  through policy of the trail, an order or a kept statement.
- **A segment is read as one of the order the read names** (review of `d2604d3`, B2). A segment read names its
  order and segment apart, and neither the customer's rule nor `covers` tied the two, so a customer naming its own
  order beside another order's segment was allowed, and a permission over one order covered another's segments. A
  segment read now asks the segment's record (`Source::SegmentOrder(segment)`, the input the named order) whether it
  belongs to that order, for every reader but a provider, whose record (`SegmentProvider`) already names both. The
  log's half, `customer-read-is-of-its-own-order` and `covers` checking the order alone, is a core-internal change,
  `F-020`'s, raised with the contract's author.
- **A read is refused unless a rule lets its reader make it** (review of `d2604d3`, B3). The read rule listed the
  customer, the provider, the regulator and the checker and let every other role through, so the operator, a
  verifier and Core's own parts read the trail, any order and any segment, and any role read any kept statement.
  The rule now refuses by default. A customer and a provider read who the deployment's checker is, the `checker`
  resource (`REQ-AUD-10`: named to customers and providers), and no other resource without a permission: any name
  was let through at first, a made-up `operator_keys` included, and left to the deployment's policy. Any party
  reads, without a permission, a kept statement the evidence store's record says is its own
  (`Source::Statement(commitment)`, the input the reader's party: its maker, or the party it was told to about its
  own part of an order, `REQ-AUD-5`), and a checker reads one by a permission over it. Every other read rests on a
  permission. A permission is granted only to a regulator or a checker (core-internal's `PermissionChange`), and held
  to the reader's party, so the operator reads the trail only where its party holds such a grant. Whether the
  operator may read the trail with no permission is on the owner's form, default no.
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

## T-021-03

- **What it builds.** One entry of `FIXED`, its two functions in `components/core-authz/src/roles/`, over
  `decide` (built by `T-021-01`) and `authorise` (`T-021-04`'s). No call of its own: `T-021-03` owns no trait method
  of core-internal, so no row of the built and unbuilt table in `every_policy_call_reaches_the_one_consultation_or_is_marked_unbuilt`
  moves. The engine edits are the shared type's, made in this row's turn: two `Source` variants appended
  (`Permissions`, `Declarations`), the `FIXED` entry after `T-021-02`'s, `promote` moved in the scan from
  `CARRIES_OUT_NOTHING` to `CARRY_OUT`, and `T-021-01`'s tests given the inputs the new rule asks for (below).
- **Only the operator posts a route** (`REQ-AUZ-2`). A `post_route` asked in any other role is refused before the
  deployment's policy or any source is asked. A suggestion has no call of its own: it reaches a route only as the
  operator's post naming its suggester (`suggested_by`), which the decide entry holds whole beside the operator as
  `who`. Whether the operator names a suggestion it posts is orchestration's to keep; the engine can't see a
  suggestion that never reached it (`THR-90`).
- **Each permission is held actor by actor.** The operator's permissions (`roles::Permission`: posting a route and
  each `CatalogueChange`, ten with promotion) are asked of the deployment's register (`Source::Permissions { party,
  permission }`, the input the actor, `acts`), one input per decision, for a caller in the operator's role only.
  Keyed by party, every login of the operator would hold every permission, which is the insider of `THR-88` and no
  separation at all. A caller in any other role is refused a catalogue change outright, the publisher's included
  (`REQ-MDL-5`). A register that says no refuses, one that says nothing gives a missing input, one out of reach not
  known. Default: posting a route needs the register's yes too, since the role is only what the caller's request
  says; reversible by dropping that one input.
- **The role is the request's, and never enough alone.** The engine, as `T-021-01` and `T-021-02` read it, takes the
  caller's role as the assembly hands it from external authentication. Wherever this rule lets a role through, a
  record binds the actor to it: the register's yes, by `acts`, for an operator's permission or Core's actor, and the
  order's record for its requester; a caller stating a role without one is refused (review of `f7df0c2`, S4: a
  provider stating the operator's role against a register listing only the operator's logins). A source that
  checked a party's roles in general would be the deployment's to add (`F-030`).
- **Acting for a user: no role stands on the request's word** (`REQ-AUZ-9`; review of `f7df0c2`, B1, S1, S2). The
  rule first let a stated role decide alone: the operator's login stating Core's role moved value with no declaration,
  stating a customer's gave an authorisation, and a customer stating the operator's role moved value. Each actor is
  now held to a record:
  - a customer, or its agent, moves value on and authorises only an order its party requested
    (`Source::Requester(order)`, the input the party). `T-021-02`'s `OrderParties` answers requester, beneficiary or
    payer, which would let a beneficiary authorise, so this is a source of its own;
  - Core moves value only through an actor the register lists as carrying the authorisation onward
    (`Permission::CarryOnward`, by `acts`);
  - the operator acts only through a login the register lists for it (`Permission::ActForUser`, by `acts`), at the
    order's own origin reserve (`Source::OrderOrigin(order)`, the input the reserve the request names), and only where
    that reserve declared it can't require the user's own withdrawal (`cannot_require_user_withdrawal`). It files an
    authorisation also where the reserve declared it starts routes on automatic deposits
    (`starts_on_automatic_deposit`, `REQ-ORD-13`, `AC-11`), the two declarations kept apart. Any listed login may act
    so at a reserve that declared it: `ActForUser` is one permission, not one per reserve (N2);
  - the origin reserve itself authorises its order's withdrawal, with no user present, where its party is the order's
    origin and it declared it starts routes on automatic deposits (`REQ-ORD-13`). It moves no value of its own asking.

  No other role acts for the user. A reserve answers both declarations, as `REQ-CAT-6` has it make both at publishing;
  never declared is a missing input, declared the other way a no, and either refuses. The decide entry's `who` is who
  acted; an authorisation's giver is `given_by`, recorded by `T-021-04`'s call.
- **No party stands on the request's word either** (review of `07c4be9`, S5). External authentication establishes
  only `acts`, and the rules read the stated `party`: `cust-9` stating `cust-1` moved value on and authorised
  `cust-1`'s order, and `evil-pay` stating `us-bank` authorised `us-bank`'s automatic deposit. One fixed rule, first in
  `FIXED`, now asks the deployment's register of actors (`Source::ActsFor { acts, party }`, answered by `acts`)
  whether each actor a request names acts for the party it states: the caller, the giver, the decider, the mover,
  the requester, the reader, the asker, a permission's granter and grantee, and a route's suggester. A no refuses as
  `no_accountable_party`, nothing a missing input, on every call. It is fixed once at the engine, so `T-021-02`'s
  session and read rules, which read the stated party, now read a bound one too; `session/` is unchanged but for its
  tests' source, which answers that word. It serves `REQ-AUZ-8`, every agent acting for an identified accountable
  party, and `T-021-05` builds its limits on it rather than adding its own.
- **What the assembly answers.** Before `T-024-02` wires the sources, every move and every authorisation needs an
  input, so none is permitted: fail-closed, and every policy call now needs the register of actors too. The register
  lists who each actor acts for, Core's actors and the operator's logins; the order's
  record, the log's acceptance or Core's intake, its requester and origin reserve; the catalogue, each reserve's two
  declarations, a no for one a reserve that published didn't make (N3). Who is listed, and each change to it, is the
  deployment's to record (`F-030`, N4). An authorisation asked before its order's record exists gets a missing input
  and is refused.
- **A permit to promote doesn't pin the version** (review of `f7df0c2`, S3). The permit names the version decided on,
  but the platform's `promote` puts whatever is staged in force. core-internal's doc says so, and `T-020-08`'s row in
  `PLAN.md` now has Core compare `lifecycle().staged(provider)` with the permit's version before it calls `promote`,
  refusing on a mismatch, with its own test.
- **The rule on an authorisation waits for `T-021-04`.** Its stub answers not known without consulting, so the rule
  holds from when that row builds `authorise` through `consult`. The test drives the rule through `consult` directly
  until then.
- **Promotion** (the review of PR #60, hand-off from `retrospectives/M2.md`). The platform's `promote` put a staged
  release in force for any caller, and no decided action carried it. `T-021-03` adds `promote` to core-internal's
  `CatalogueChange`, about one release so it names the staged version, folded into 1.0.0 (core-internal isn't on
  `develop` yet), with one valid fixture; `PLAN.md` §4.4 gives `contracts/` to `T-020-01` and `T-020-02` alone in
  `M3`, and this edit is the one exception, made because the brief gave this row promotion and no action existed for
  it. The alternative was a new `F-020` row. The row's Work and Tests in `PLAN.md` name it now. The scan lists the
  platform's `promote` as carrying out `change_catalogue: promote`, so core-orch reaches it only holding its effect.
- **The lifecycle hand-off is closed in the scan, not in `platform.rs`.** `accept`, `withdraw`, `revoke`,
  `declare_exploited`, `mark_stale` and now `promote` are each reached only by core-orch holding the effect of a
  permitted decision for that change. The platform still takes any `by`: `T-020-08` must pass it the permitting
  decision's caller, so the platform's records name the same actor as the log's.
- **Left as they are.** `roll_back` ends a staging and keeps the version in force (ADR-0014); no row names it a
  permission, so it stays in `CARRIES_OUT_NOTHING`, an item for the owner's form beside promotion. `admit` starts
  only a release that needs no fresh acceptance (`REQ-MDL-3`'s automated acceptance); a widened one waits for
  `accept`, which is decided. Its reason in the scan said "before acceptance", which was inaccurate, and now says so.
  Whether an automated acceptance should itself be a decision is the same item.
- **`T-021-01`'s tests, given their inputs.** `every_decided_action` lists `promote` (13 actions); the request of a
  post or a catalogue change is the operator's login, and the sources say yes to every input the fixed rules name,
  so the deployment's ruling is what is decided. `authenticated_caller_is_still_decided_by_policy` now expects the
  deployment's policy to see each caller the fixed rules let through, as it came, and the rest refused by the fixed
  rule, and its known operator holds the permission the rule asks for, so the refusal of the other login stays the
  deployment's. Since the review of `f7df0c2`, Core's move needs the register to list its actor: the engine tests' sources
  answer that one word for `core-orch` themselves, outside the inputs they count, and the unbuilt engine's move is
  not known for that unreachable input (`verifier_unreachable`) rather than for its absent policy. Since the review
  of `07c4be9`, the sources of `T-021-01`'s and `T-021-02`'s tests also answer the register's yes, for every
  actor they name, outside what they count, and the unbuilt engine's grant is not known for that input.

### T-021-02, the operator's read of an order's state and age

- **What it builds.** `REQ-ORD-5` makes an order's state and age visible to its customer and the operator, and
  `T-021-02`'s refuse-by-default had left the operator only reads by a permission, which core-internal grants only to
  a regulator or a checker. core-internal 1.0.0 now has a read of an order's state and age alone
  (`Reads::OrderState`, `T-020-01`, folded in at `dfe8a45`). The read rule lets the operator make it without a
  permission, through a login the deployment's register of permissions lists for it; every other operator read
  still needs a permission. A customer's read of it asks the order's record, as a read of the order does, and a
  provider never reads it.
- **Through the roles register, keyed by `acts`.** The login is held to a new permission of `T-021-03`'s register,
  `Permission::ReadOrderState`, asked by the actor's `acts` within its party (`Source::Permissions`), as every
  operator power is: posting a route, each catalogue change, acting for a user. Reading an order's state isn't
  acting for a user, so it doesn't ride on `ActForUser`. The variant is `T-021-03`'s type, added in this row's turn;
  the rule that asks it is in `session/`. `T-021-03`'s rule that every actor acts for the party it states still runs
  first.
- **T-020-08's read asks the state.** `Segments::status` asked a full read of the order (`Reads::Order`), which the
  operator can't make without a permission; it now asks `Reads::OrderState`, the read `REQ-ORD-5` names, for the
  customer and the operator alike. Its scenario test,
  `orders_state_and_age_are_readable_by_its_customer_and_the_operator`, takes back the operator's read, which waited
  on this row, and the kit's register lists the operator's login for it.

## T-021-04

- **What it builds.** `Authorisations::authorise` on `Engine<L, P, S>` in `components/core-authz/src/funds/`,
  decided through `consult` and recorded through `PolicyLog` before it answers. The unbuilt stub is gone; `authorise`
  moved from unbuilt to built in `every_policy_call_reaches_the_one_consultation_or_is_marked_unbuilt`. The engine
  edits are the shared type's, made in this row's turn: one field (`authorised`), and `decide` holding a move's
  verdict to the register. No `FIXED` entry and no `Source` variant: the rules read what the log holds as permitted,
  which a fixed rule, a plain function of the request, can't.
- **The register is the log's** (review of `d3de45a`, finding 1; hardened on its re-review, finding 1). It was first
  kept in memory, so a fresh engine, after a restart or beside another over the same log, held nothing and failed
  open: the operator's own destination was permitted, and a replaced authorisation given again. The register now
  holds nothing of its own. Before every `authorise` and every move the engine reads every authorisation the log holds
  as permitted, in log order, and decides on that alone; its own permit reaches the register only through the log.
  The read is on policy's handle itself, `L: PolicyLog + funds::Recorded`, one wire and not two: a separate setter
  (`set_record`, now gone) let the assembly hand a snapshot, a filtered read or another log's, which failed open the
  same way. "Permitted" is defined once, `funds::permitted_in`, core-internal's own reading of a permitted entry (its
  private `permitted` in `relations.rs`). The engine refuses `missing_input` a read that fails, one that doesn't
  extend the read before it, and one that lacks the engine's own last permitted append, keeping the register it read
  last. What it can't detect is the trait's doc: a first read, or one frozen after the engine's last append, that is
  of another log or misses another engine's appends, passes both checks, and that the read is the very log the handle
  appends to, whole and current, is the assembly's word. The coordinator's default, on the owner's form: the register
  rebuilt from the log's permitted `authorise` entries, and an engine that can't read refusing. This row reads it
  before every call rather than once when the engine opens, since one rebuild leaves two engines over one log apart.
  This row's own defaults, reversible and not on the form: the read is a core-authz trait over the log's own
  `entries()` (core-audit's `AuditLog` has it, so nothing was added there), not a read on core-internal's
  `PolicyLog`, which is the contract author's to add if wanted; and the whole log is read each time, O(n) in its
  entries, with no index kept across reads, since a v0 log is small.
- **The failure code** (re-review, finding 4). An unreadable log, or one whose read the engine can't trust, is
  `REQ-AUZ-5`'s unreachable case: an input out of reach. The engine answers `missing_input`, a refusal, as the default;
  `not_known` with `verifier_unreachable` would read closer to `REQ-AUZ-5`. Both fail closed: nothing moves on either.
- **A move acts only inside the authorisation in force.** A `move_value` is permitted only where the authorisation
  it names is in force for its order and its route ends inside it: the destination (reserve, node and account), the
  to asset, and at least the floor. Anything else is refused `outside_authorisation`. These are the four fields the
  log's `move-rests-on-its-authorisation` holds an effect to; the register adds what the log can't see, that an
  authorisation replaced covers nothing more. The move's `origin_reserve` isn't held here: `REQ-AUZ-3` names four
  things, and the operator's origin is `T-021-03`'s rule (`Source::OrderOrigin`).
- **Held after `consult`, not before** (unlike `T-021-02`'s grant rule). A refusal of the fixed rules or of the
  deployment's policy stands, so a mover acting for no accountable party is refused as such (`REQ-AUZ-8`, held
  before any other rule); otherwise the register's refusal is the answer, since a refusal beats not known. The
  register only refuses; `funds::held` never writes a permit, so the scan's permit sites are unchanged.
- **A new authorisation.** An order's terms in force are set by its first permitted authorisation. After it, one with
  the same order, destination, to asset and floor joins them, and the moves under either stay inside. One naming
  anything else, the floor included, replaces them, and the ids under them, only where the customer gave it (role
  `customer`, its person or its agent, whose party `T-021-03`'s rule holds to the order's requester): the customer's
  own choice after a failed final deposit is such a new authorisation (`REQ-ORD-43`, `INV-10`). The operator acting
  for the user and the origin reserve authorise the order at its origin where the roles let them, and change nothing
  of it after: refused `not_permitted`, never `outside_authorisation`, which intake reads as ending the order. Their
  authorisation only ever joins the terms in force, even where engines racing over one log left one of other terms
  there (re-review, finding 3): two engines both read `auth-17` in force, one permits the customer's `auth-18`, the
  other the operator's restatement `auth-70` of `auth-17`, appended after it; `auth-70` sets nothing and is in force
  under no terms, so the old destination isn't back by the operator's hand. Terms given again after others replaced
  them are in force under their new id alone. An authorisation's id names that authorisation only: the same again,
  while in force, is decided again; anything else under the id, or the id once replaced, is refused, so the log never
  holds as permitted one the register holds as replaced. The contract author is asked for the matching log row.
- **Defaults taken** (reversible):
  - a new authorisation with the same terms (a restatement, as intake's retry would bring) is permitted to whoever
    the roles let authorise, and keeps the earlier id in force beside it;
  - a new authorisation changing only the floor is the customer's too, with no failed deposit needed, since
    `REQ-ORD-27` has the order wait for the customer past its floor and `INV-10` restricts only the destination and
    the to asset;
  - an authorisation replaced covers no move asked after it, the in-flight ones under it included. That holds only
    because the state machine changes a destination solely from `waiting_for_customer`, where nothing of the part is
    in flight: a move permitted before the replacement still holds its permit, and its effect can still land under
    the old authorisation, which the log admits (`move-rests-on-its-authorisation` cites the authorisation the move
    named, not the one in force);
  - a customer's agent gives a new authorisation as its person does. Until `T-021-05` builds an agent's scope, an
    agent of the customer can change the destination with no check of the scope its party set: a dependency on
    `T-021-05`.
- **Gap: policy doesn't see the failed final deposit.** The rule holds who gives a new destination, not when. It is
  safe only while orchestration calls `authorise` with new terms only for an order in `waiting_for_customer` after a
  failed final deposit, a condition `T-020-13` owns and must hold. Downstream, the state machine takes
  `destination_changed` only from `waiting_for_customer`, the log admits it only on its acceptance
  (`changed-destination-rests-on-its-acceptance`), a move's segment must be in a plan the log holds
  (`move-segment-is-in-its-orders-plan`), and a recalculation keeps the part's destination
  (`recalculation_keeps_destination`). Holding it in policy too would take a `Source` for the order's state, which
  every assembly's sources would have to answer: `T-020-13` builds that path, and `T-024-02` would answer the source.
  An item for the owner, default no.
- **Gap: two engines in separate processes.** Reading the log before each decision closes a restart and two engines
  in one process, and an operator's authorisation raced past a customer's never sets the terms. What still races
  between the read and the append is a move: one engine permits a move under `auth-17` on its read, the other has just
  appended the customer's `auth-18`, and the move's permit stands, its effect admitted by the log, which holds it to
  the authorisation the move named. Closing that takes the log holding the rule as an admission row of its own,
  `F-022`'s or the contract author's. It doesn't block while Core is one process.
- **Pending: a split order's other part** (review of `d3de45a`, finding 2). When one part's final deposit fails and the
  customer's new authorisation replaces the order's only one, a part still heading to the first destination is
  refused under both. The coordinator's default: an authorisation names the parts it covers, and a replacement
  replaces the earlier one only for those parts. It waits on a `part` field in core-internal 1.0.0's authorisation,
  the contract author's; `customers_new_destination_after_a_failed_deposit_is_authorised_anew` is marked as the
  single-part case until then.
- **The switch in the assembly (`T-024-02`).** `T-024-02` merged into m3 first, and this row carried the switch when
  it merged m3 (`970828a`). `PolicySide::authorise`, which answered beside the engine and wrote on policy's handle
  outside `consult`, where the permit scan doesn't look, now forwards to the engine's `authorise`. `AuditLog`, and so
  the assembly's `PolicyHandle<CoreLog>`, reads its entries for the engine through `Entries`, owned, the log's borrow
  dropped before the engine appends. The assembly's `Records` already answered what `authorise` asks, so no source
  changed. Its start of an order looked up the acceptance's authorisation as the first permitted `authorise` of that
  id and order (T-024-02's L2 fix); it agreed with the register for a first authorisation, but not once one was
  replaced. It now reads through the register (`Authorised::of`, `in_force_as`, the view the engine holds a move to),
  so an order starts only under an authorisation in force. A Core opened again over a `durable::Disk` rebuilds its
  register from the log, which closes the register's part of the restart hand-off to `F-030`. Tests in the assembly:
  `authorisation given again with other terms is refused by the engine`, which the stand-in would have permitted, and
  `order starts under no authorisation the customer replaced`. Rule-offs, `cargo test -p rutent-scenarios --lib
  assembly::tests`: the stand-in put back turned the first red, the first-permitted lookup put back the second.
- **The segments kit, and the scenarios' handle.** `conformance/scenarios/tests/segments_support/` seeded the order's
  authorisation as a permitted `authorise` entry straight into the log, so every move of `T-020-08`'s and
  `T-020-10`'s scenario tests was refused. The kit now gives the customer's authorisation through the engine
  (`Policy` implements `Authorisations`), its policy handle (`PolicyPath`) reads its `TestLog`, its `Register` answers
  `Requester` for the customer, and the seeding functions take the kit's `Policy`. The entry it leaves is the
  engine's: by the customer, its decision id the authorisation's. `conformance/scenarios/src/lib.rs`'s `PolicyHandle`
  reads its log through a trait of the assembly's, `Entries`, which `log-handles.rs`'s recording log and core-audit's
  `Unbuilt` (no read: refused) implement.
- **Other rows' tests, given the authorisation.** `T-021-01`'s and `T-021-03`'s tests decide the fixtures' move
  (`auth-17` for `ord-17`); their logs read as permitted that authorisation, held before the test began, and then what
  they took, so what they decide is still their own rules. The unbuilt engine's move and authorisation are not known
  again, as before this row (`verifier_unreachable`).
- **`roles/`'s doc** said the rule on an authorisation held "from when `T-021-04` builds `authorise`"; it now says it
  holds on every `authorise`.
- **Rule-offs.** Each rule switched off once, the file restored and touched, `cargo test -p rutent-core-authz --lib`.
  Tests written for rules already there (findings 3 and 4 of the first review, the failed read) passed when written
  and went red under their mutation; the tests of the re-review's findings 1 and 3 went red before the fix.

  | Rule switched off | Tests that went red |
  |---|---|
  | destination held | the property; `route outside ...`; `customer's new destination ...` |
  | to asset held | the property; `route outside the authorisation needs a new one` |
  | floor held | the property; `route outside the authorisation needs a new one` |
  | order held | the property; `route outside the authorisation needs a new one` |
  | new terms reset the ids in force (an authorisation replaced covers nothing; M2) | the property; `route outside ...`; `earlier terms given again ...`; `register is rebuilt ...`; `replaced authorisation given again ...` |
  | an authorisation never permitted covers nothing | the property and six named tests |
  | new terms only from the customer | the property; `customer's new destination ...`; `new floor ...`; `register is rebuilt ...`; `two engines ...` |
  | an id names one authorisation | the property; `route outside ...`; `register is rebuilt ...`; `replaced authorisation given again ...`; `operator's restatement raced ...` |
  | the fixed rules' refusal stands (register after `consult`) | `route outside the authorisation needs a new one` |
  | the register's refusal beats not known | `route outside the authorisation needs a new one` |
  | `decide` holds a move to the register | the property and ten named tests |
  | `authorise` decided through `consult` | `route outside ...`; the built and unbuilt table; the entry-point scan; the unbuilt engine's test |
  | the register read before an authorisation | the property and seven named tests |
  | the register read before a move | the property; nine of this row's; seven of `T-021-01`'s; four of `T-021-03`'s |
  | only a `permitted` entry is read as permitted | the property; four named tests |
  | the floor is one of the terms | `new floor from anyone but the customer is refused`; `route outside ...` |
  | a failed read gives an empty register | `register that cannot be read refuses every authorisation and move` |
  | a failed read keeps the last register | `register that cannot be read refuses every authorisation and move` |
  | a read extends the one before it | `truncated read of the log is refused`; `reordered read of the log is refused` |
  | M7: the extension check weakened to a read no shorter | `reordered read of the log is refused` |
  | a read holds the engine's own last permit | `snapshot read missing the engine's own permit is refused`; `read over another log is refused` |
  | another's new terms never set them (the race) | `operator's restatement raced past a new destination is not in force` |
  | M6: that rule narrowed to the operator, the origin reserve let set terms | `operator's restatement raced past a new destination is not in force` |
  | a restatement joins the ids in force (M3) | the property; `restatement keeps the earlier id in force beside it` |
  | a same-id retry held to being in force (M5) | the property; `register is rebuilt ...`; `replaced authorisation given again ...`; `operator's restatement raced ...` |
