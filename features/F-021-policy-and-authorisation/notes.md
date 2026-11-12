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

### T-021-04, per part (finding 2 of the review of `d3de45a`)

Branch `f-021/t-021-04b-per-part`, from `m3` at `8cac73f`, with core-internal's `Authorisation.parts`
(`41257db`) in it.

- **The register is keyed by part.** An authorisation speaks for the parts it names, or every part where it names
  none; the order as a whole, which the order's start reads, only one naming no part speaks for. Each part's terms in force fold the authorisations that speak for it, by the same rules as before: the
  first sets them, the same terms join, the customer's other terms replace, and anyone else's other terms set nothing
  (the race rule, kept). A replacement naming `p1` replaces nothing for `p2`, which goes on under `auth-17`.
- **A move is held to its own part.** The move doesn't name its part, so the engine reads it, on the same read, from
  the plans the log holds: `Recorded::part_of`, one definition, `funds::part_in`, core-internal's own reading
  (`part_of_segment`, private in `relations.rs`): a `route_chosen` part listing the segment, or the part a
  `route_recalculated` or `destination_changed` gave it to, the first plan listing it winning, as the contract's
  `find_map` does. A part that can't be read is `missing_input`. A segment no plan lists is in no part and refused
  `outside_authorisation`, as the log admits no such move (`move-segment-is-in-its-orders-plan`): first it was held to
  the order as a whole, which let `auth-17`, replaced for `p1`, move such a segment (approval of `8556f36`, low 1).
  Every
  read implements it: the core-authz test logs, the segments kit's `PolicyPath`, and the scenarios' `PolicyHandle`
  through `Entries`. That the part is the plan's is the assembly's word, as the rest of the read is; the trait's docs
  say so.
- **An authorisation is held to every part it speaks for.** One naming no part is held to the order as a whole and
  to every part an authorisation of the order names. A new id is a new authorisation where it names other terms than
  those in force for any of those, and only the customer gives one. An id given again is decided again only while it
  is in force for every one of them: the order's `auth-17`, replaced for `p1`, still speaks for `p2`, but given
  again it is refused, since permitted again it would set `p1`'s old destination anew.
- **The terms are the log's.** `same_terms` now compares the origin reserve too, as core-internal's `terms` does
  (destination, to asset, floor, origin reserve): an authorisation from another origin reserve is a new one.
- **Where the register is stricter than the log** (a reading, unchanged): the log's `in_force` takes a part's
  latest customer authorisation and admits a move under any authorisation of the same terms; the engine also holds a
  move to an id in force, so an id replaced covers nothing more even once its terms are given again under another.
- **The assembly's start of an order** reads the acceptance's authorisation for the order as a whole
  (`in_force_as(order, None, ..)`): an order starts before any part has an authorisation of its own.
- **`Stuck::follow` (core-orch, `T-020-11`'s; not edited here).** It already takes the in-force authorisation per
  part, by the log's own rule (the first covering the part, then each later one its customer gave), so it agrees
  with the register on each part's terms: items (1), (2) and (4) of `T-020-11`'s list are met. Item (3) isn't: the
  ends first followed for a part stand, and following again with others is refused for the whole order
  (`found.iter().any(..)` returns `NotItsAuthorisation`), so once `p1`'s new destination is in force, following the
  order again refuses `p2` too, and `p1`'s new ends are never taken. The change, for `T-020-11`'s owner: record per
  part whether its authorisation in force is a later one its customer gave (`replaced`, set where `current` was
  already some when a customer's covering one is taken); refuse only a part whose held ends differ and which wasn't
  so replaced; and where it was, replace its held ends (`insert`, not `or_insert`). One more difference, for the same
  owner: `follow` names the customer by the acceptance's requester, the engine by the giver's role (the roles rule
  holds a customer's party to the requester); and `origin_account` isn't one of the terms, so two authorisations of
  the same terms with different origin accounts are one for the engine and the log, and two ends for `follow`.
- **Tests, each red before the change:** `split order's other part moves on under its authorisation after one part's
  new destination` (the reviewer's sequence, a restart included), `move on another part under a part's replacement
  is refused`, and the property test's sequences, now over two parts, for the whole order or one part, with moves of
  either part's segments and of one no plan lists. Written for rules the change brought and seen red under their
  rule-off: `segment's part is read from the plans the log holds`, `authorisation from another origin reserve is a new
  one`, `authorisation replaced for one part given again is refused`, `move whose part cannot be read is refused`.
- **Rule-offs**, each red:

  | Rule switched off | Tests that went red |
  |---|---|
  | a part named is the part spoken for | the property; the split order's test; `move on another part ...` |
  | the order as a whole only by one naming no part | the property; `move on another part ...` |
  | a move held to its own part | the property; the split order's test |
  | one naming no part held to every part named | the property |
  | a segment whose part can't be read refuses | `move whose part cannot be read is refused` |
  | the origin reserve is a term | `authorisation from another origin reserve is a new one` |
  | `part_in` reads a `route_chosen` | `segment's part is read from the plans the log holds` |
  | `part_in` reads a `destination_changed` | `segment's part is read from the plans the log holds` |
  | `part_in` of the order alone | `segment's part is read from the plans the log holds` |
  | an id given again held to being in force for every part | `authorisation replaced for one part given again is refused`; the property |
  | the order held (rechecked on the new path) | the property; `route outside the authorisation needs a new one` |
  | a segment no plan lists is refused (low 1) | `move of a segment no plan lists is refused`; the property |
  | the first plan listing a segment wins (low 2) | `segment two plans list is of the part the first places it in` |

- **The lows of the approval of `8556f36`.** The two tests above are new: the reviewer's sequence (`auth-17` for the
  whole order, `auth-18` naming `p1`, a move of `s-unplanned` under either) went red before the fix; the first-plan
  test passed when written and went red with `part_in` reading the last plan. `T-021-01`'s, `T-021-03`'s and
  `T-021-05`'s test logs now read a plan placing the fixtures' move's segment, `seg-2`, in a part
  (`funds::plan_of_the_fixtures_move`, test-only), and the funds tests' log places it in `p0`; the property's model
  refuses a move of a segment no plan lists. Rechecked on the new path, each red: the order as a whole only by one
  naming no part, a move held to its own part, one naming no part held to every part named.

## T-021-05

- **What it builds.** `Charges::charge` on `Engine<L, P, S>` in `components/core-authz/src/parties/`, decided through
  `consult`, held to the party's bound and appended through `PolicyLog` before it answers; and one fixed rule, the
  second in `FIXED`, right after `T-021-03`'s `ActsFor` rule, its two functions in `parties/`. The unbuilt stub is
  gone, and `charge` moved from unbuilt to built in
  `every_policy_call_reaches_the_one_consultation_or_is_marked_unbuilt`. The engine edits are the shared type's,
  made in this row's turn: one `Source` variant appended (`AgentScope`), the `FIXED` entry, one field (`charged`) in
  the two constructors, and `charge_outcome`, the one function that writes `ChargeOutcome::Within`, from the verdict,
  which the scan now pins as a fourth permit site. The engine's imports are unchanged, to keep the merge with
  `T-021-04` small.
- **An agent without an accountable party** (`REQ-AUZ-8`). `T-021-03`'s rule already refuses an actor the register
  of actors lists for another party (`no_accountable_party`) and one it says nothing of (a missing input). This row
  adds what the register can't hold: an agent (`kind: agent`) that states itself its own party (`party == acts`)
  answers to no one, and is refused `no_accountable_party` on every call, with no input asked, whatever the
  register would say. Default: a person or a component may be its own party (a person customer is), an agent may
  not.
- **An agent acts within the scope its party set** (`REQ-AUZ-8`). Each call an agent makes is a `parties::Scope`:
  one per policy call, one per decided action (a catalogue change by its kind), a charge by its kind of work. The
  rule asks the party's register of its agents (`Source::AgentScope { party, scope }`, the input the agent's `acts`,
  as `Permissions` answers by the actor): a no refuses `not_permitted`, nothing gives a missing input, out of reach
  not known. A person and a component are asked no scope. Who a party's agents are and what each may do is the
  deployment's register to keep and record (`F-030`). Default: the scope is a set of calls, not of amounts or
  destinations; a finer scope is the deployment's policy service's to add, which can only refuse more.
- **`authorise` is in the scope, with no hook** (`T-021-04`'s dependency). `T-021-04`'s `authorise` goes through
  `consult(Asked::Authorise)`, so this fixed rule holds an agent's authorisation, the customer's new destination
  after a failed final deposit included, to `Scope::Authorise` as soon as both rows are in. Until `T-021-04` merges,
  the test drives the rule through `consult` directly, as `T-021-03`'s did. Gap: a party can't let its agent
  authorise an order at its origin but not give it new terms after, since telling the two apart needs
  `T-021-04`'s register of authorisations in force, which runs after the consultation, where no source is asked.
  If the owner wants that, the hook is `funds::Authorised::refuses`, where `anew` is computed: a new authorisation by
  an agent would also need the party's word on a `Scope` of its own, which means asking a source there. Not built;
  owner: `T-021-04`'s author at the merge, default no (review of `b63af67`).
- **Limits bind at the party** (`REQ-AUZ-8`, `REQ-OPS-1`, `AC-18`). Two limits, each counted per party, never per
  actor: work (`Limit::Work`: `request` and `fan_out` together, so the requests an order fans out to providers count
  against its caller's own) and questions about people (`Limit::People`: `person_question`). A charge that fits is
  counted only after its entry is in the log; one over the bound counts nothing and its work is not done. A charge's
  id names one charge: the same charge again answers within and counts once, anything else under a counted id is
  refused, as `T-021-02`'s session ids are. An id is its party's own: the counter keys a charge by its party and its
  id, so two parties' charges under one id are two (review of `b63af67`). Default: the ids counted are kept for the
  window now only, so the same charge given again in a later window counts again there. The party is the charge's
  own (`Charge::check` holds it to its caller's), bound to the actor by `T-021-03`'s rule.
- **The window is Core's clock's.** A fixed window, `WINDOW_SECS` (one hour), aligned to the epoch, read from the
  engine's clock (`session::Clock`, the one sessions are bounded by), never from a charge's `at`: a charge stating a
  later moment still counts in the window now (`T-021-02`'s review, S1). Defaults, reversible: one hour; 10 000
  pieces of work and 1 000 questions about people per party per window (`Uniform`), the same for every party, until
  the assembly hands the deployment's own (`Engine::set_bounds`, a `parties::Bounds` answering per party and
  limit). The window only moves forward: Core's clock moving back keeps counting in the latest window (review of
  `b63af67`). A fixed window lets a party spend its bound at the end of one window and again at the start of the
  next, twice the bound in a moment; accepted, and a sliding window is the deployment's to ask for.
- **The counter is the log's** (review of `b63af67`, finding 6; built at the merge of `T-021-04`, `85386b8`). Held in
  memory alone, the counter failed open after a restart. Before every charge the engine now reads the log on its
  policy handle, through `T-021-04`'s `funds::Recorded`, which gains `charged()` beside `permitted()`
  (`parties::charged_in` over the log's entries: each verified `charge` entry whose outcome is `permitted`), and
  rebuilds the counter from what it read (`Charged::rebuild`): the latest window's charges, each once under its
  party's id, what it held before dropped. A log that can't be read refuses the charge (`missing_input`, so over a
  bound of 0). The window is keyed by Core's clock with no contract change: the engine records each charge's entry
  at the moment Core's clock counted it (`Charged::counted_at`, the latest window's start where the clock stood
  back), and the charge's own `at` stays inside the entry's `what`. An entry's `when` is Core's clock by contract
  (`TimeSource::CoreClock`), and nothing in core-internal ties a charge entry's time to its request's. Default,
  reversible: so the time field the coordinator asked the contract author for isn't needed for this; if it lands,
  `charged_in` reads it instead of `when`. The whole log is read each time, O(n) in its entries, as `T-021-04`'s
  register is. Gap: two engines in separate processes race between the read and the append, as `T-021-04`'s note
  says of its own; F-022's or the contract author's to close with an admission row.
- **A refused charge is over a bound of 0** (contract gap, not made). core-internal's `ChargeOutcome` has only
  `within` and `over`, so a charge policy refuses or can't decide is answered `over { bound: 0 }`, its entry
  `refused { class: over_bound }`, and the refusal's own class (`no_accountable_party`, `not_permitted`,
  `missing_input`) or not known is lost from the record. The stub and the assembly's stand-in answered the same.
  The change, for core-internal's single writer: add `refused { class }` and `not_known { cause }` to
  `ChargeOutcome` (Rust `types.rs`, and `charge-answer.schema.json`'s `outcome`), mapped by `ChargeOutcome::outcome`
  to `refused { class }` and `not_known { cause }`, as `PersonOutcome` already is, with a valid fixture of each and
  the README's "How each answer is recorded" row updated. Then `charge_outcome` gives the verdict's own answer.
- **Person questions are charged as such** (review of `b63af67`, finding 7). `core-orch`'s `rounds::Run::charge`
  charged a round's providers' questions and its person questions together as one `fan_out`, so nothing reached the
  party's limit on asking about people. It now charges the person questions as `person_question` first, then the
  providers' as `fan_out`, each before any is asked, and over either bound none is sent (`NotOffered::OverBound`).
  Where the first fits and the second doesn't, the first stays counted though nothing was asked: accepted, as a
  charge that fits is work committed. The edit is to `T-020-06`'s module, merged, at the coordinator's instruction.
- **For `T-021-06` and `T-021-07`.** `parties::accountable(&Actor)` and `parties::same_accountable_actor(a, b)` are
  the one definition of an actor's accountable party and of two actors being one there: two agents of one party, its
  person and its agent, or one login stating two parties, since a login may act for more than one (review of
  `b63af67`, finding 2: `op-login-2` deciding as `operator` and moving as `operator-desk-b`). Two actors are distinct
  only where both their parties and their logins differ. `T-021-06` compares the decider and the mover with it. Each reads a party the
  consultation bound, so only of a call the fixed rules let through. `T-021-07` bounds questions about people at the
  asker's party through `Charges::charge` with `Work::PersonQuestion` (`Limit::People`), or reuses
  `Charged::standing` and `Charged::take` inside `ask_about_person`; `REQ-CPL-5`'s bound per order is its own.
- **Other rows' tests.** Every agent the fixtures name now needs its party's scope: the sources of `T-021-01`'s
  (`Given`), `T-021-02`'s (`Says`) and `T-021-03`'s (`Register`) tests answer `AgentScope` yes outside what they count,
  as they answer `ActsFor`. `provider_posting_a_route_is_refused` named an agent `bridge-pay` acting for
  `bridge-pay`; it now names `bridge-pay-agent`, since an agent is never its own party.
- **Hand-offs at merge** (both carried out at the merge of `T-021-04`, `85386b8`, below).
  - `T-024-02`: its `ActsFor` arm must bind the kind (`Source::ActsFor { acts, party, kind }`, yes only for the kind
    the register lists for the login). `Records::supply` matches every `Source` with no catch-all, so it needs an
    `AgentScope` arm too (the
    deployment's register of agents' scopes; until there is one, yes for the agents the scenarios name, or `Missing`,
    which refuses every agent's call). `PolicySide::charge` becomes `self.engine.borrow_mut().charge(c)`, and
    `CHARGE_BOUND` goes; the assembly may hand the engine its bounds (`set_bounds`).
  - `T-021-04`: its `authorised` field and constructor lines sit beside `charged`, and both rows rewrite the unbuilt
    list in `every_policy_call_reaches_the_one_consultation_or_is_marked_unbuilt`: after both it is
    `["T-021-06", "T-021-07"]`. Its tests' sources (`Sayings`) say yes to every input, `AgentScope` included, so they
    need nothing.
- **Rule-offs.** Each rule switched off once, the file restored and touched, `cargo test -p rutent-core-authz --lib`.
  Switching the whole row off first (the fixed rule passing everything and every charge the stub's `over`) turned
  all six of this row's charge and scope tests red, and the built and unbuilt table.

  | Rule switched off | Tests that went red |
  |---|---|
  | an agent that is its own party refused | `agent without an accountable party is refused`; `scope rule takes an input it was not given as missing` |
  | a scope asked of an agent | `agent outside its party's scope is refused` |
  | a scope's no refuses | `agent outside its party's scope is refused`; `scope rule takes an input ...` |
  | a scope that says nothing is missing (the rule's arm) | `scope rule takes an input it was not given as missing`, which calls the rule directly: through the engine no test reaches the arm, since `verdict` refuses a missing input before any rule rules, so the arm is a second guard |
  | a scope asked only of an agent | this row's scope, accountable-party and thousand-agent tests; 23 of `T-021-02`'s and `T-021-03`'s; the built and unbuilt table |
  | each call its own scope | `agent outside its party's scope is refused` |
  | the rule in `FIXED` | `agent without an accountable party ...`; `agent outside its party's scope ...` |
  | a charge decided through `consult` | `agent without an accountable party ...`; `agent outside ...`; the unbuilt engine's test; the built and unbuilt table; the entry-point scan |
  | a charge policy doesn't permit is over 0 | `agent without an accountable party ...`; `agent outside ...`; `bound is held over core clock's window ...`; the unbuilt engine's test; the table; the scan |
  | counted by the party, not the actor | `thousand agents under one party hit one limit`; `requests fanned out ...`; `bound is held ...` |
  | a fan-out counts | `requests fanned out to providers count against the caller's party` |
  | a fan-out on the work bound | `requests fanned out to providers count against the caller's party` |
  | questions about people on a bound of their own | `thousand agents ...`; `requests fanned out ...` |
  | a charge over its bound counts nothing | `agent without ...`; `thousand agents ...`; `requests fanned out ...`; `bound is held ...` |
  | the bound held | `thousand agents ...`; `requests fanned out ...`; `bound is held ...` |
  | the same charge counts once | `bound is held over core clock's window and a charge counts once` |
  | another charge under a counted id refused | `bound is held over core clock's window and a charge counts once` |
  | the window from Core's clock, not the charge's `at` | `bound is held over core clock's window and a charge counts once` |
  | the window starts again | `bound is held over core clock's window and a charge counts once` |
  | counted only after the log takes it | `charge the log does not take counts nothing` |
  | `same_party` by the party, not the actor (now `same_accountable_actor`) | `actors are compared at their accountable party` |

### T-021-05, the review of `b63af67`

- **The kind of actor is bound by the register** (finding 1, critical). Nothing held `Actor.kind`: `ActsFor` checked
  the login and the party, so `agent-7`, scoped to `charge: request`, was refused `move_value` as an agent and
  permitted it stating `kind: person`, and the rule that an agent is never its own party fell the same way.
  `Source::ActsFor` now carries `kind`, and the register answers by the login, the party and the kind together, so a
  login stating a kind the register doesn't list for it acts for no one (`no_accountable_party`). The edit is to
  `T-021-03`'s rule in `roles/`, merged, kept to the one field; every test source that answers `ActsFor` was updated
  (`roles/tests.rs` and `T-021-01`'s and `T-021-02`'s answer it whatever the kind, since no test of theirs turns on
  it; `parties/tests.rs` and the segments kit bind it). Intake's own `Actors` register (`T-020-03`) still answers by
  login and party: it decides only whether intake serves the caller, and every action it serves is decided by
  policy, which now binds the kind.
- **One login is one actor** (finding 2, high). `same_party` is now `same_accountable_actor`, true where the parties
  or the logins are the same.
- **The window moves forward only** (finding 3), and **a charge's id is its party's** (the nit).
- **A refused or not-known charge is logged as `over_bound`** (finding 4, high). Waits on core-internal's
  `ChargeOutcome` gaining `refused { class }` and `not_known { cause }`, which the contract author is adding;
  `contracts/` is untouched here. Once it is in `m3`, `charge_outcome` gives the verdict's own answer and core-orch's
  `rounds::Run::charge` maps the two new outcomes (each, like `over`, sends nothing).
- **Each call's scope is pinned** (finding 5) by `each_scope_permits_its_own_call_alone`: for each scope, an agent
  scoped to it alone is permitted its call and refused every other, the table naming each call's scope by hand.
- **The counter is the log's** (finding 6): above, built at the merge of `T-021-04`.
- **Person questions are charged against the people limit** (finding 7): above.
- **A route's suggester asks no scope** (the nit): it doesn't act in the call; the operator who posts does, and the
  suggester is still held to `ActsFor`, kind included.
- **Rule-offs of the fixes.** Each switched off once, the file restored and touched; core-authz's lib tests, or
  core-orch's `rounds` for its edit. The new tests went red before the fixes (`each_scope_permits_its_own_call_alone`
  passed, pinning a mapping that was already right; it goes red under each mapping mutation below).

  | Rule switched off | Tests that went red |
  |---|---|
  | the register told the stated kind (a fixed kind sent) | `agent stating another kind acts for no one`; `agent without an accountable party ...`; `agent outside its party's scope ...`; `thousand agents ...` |
  | one login is one actor | `actors are compared at their accountable party` |
  | the window moves forward only | `clock moving back keeps counting in the latest window` |
  | a charge counted back is counted at the latest window's start | `clock moving back keeps counting in the latest window` |
  | `decide_stuck` its own scope (made `move_stuck`'s) | `each scope permits its own call alone` |
  | `check_read` its own scope (made `move_value`'s) | `each scope permits its own call alone` |
  | `resolve_stuck` a stuck move (made `move_value`'s) | `each scope permits its own call alone` |
  | a catalogue change scoped by its kind | `each scope permits its own call alone` |
  | a charge's id its party's | `charge ids are each party's own`; `counter is rebuilt from the charges counted` |
  | rebuild: only the latest window's charges | `counter is rebuilt from the charges counted` |
  | rebuild: the latest window, a later charge's included | `counter is rebuilt from the charges counted` |
  | rebuild: what the counter held dropped | `counter is rebuilt from the charges counted` |
  | person questions charged as `person_question` | `person questions are charged against the caller's people limit` |
- **The first table, again.** Every rule-off of `T-021-05`'s first table was run again on the fixed code: each still
  turns red, now often under more tests than listed there (the review's new tests among them).

### T-021-05, merging `T-021-04` (`85386b8`)

- **The engine's lines.** `authorised` and `charged` sit side by side in the struct and both constructors; the
  unbuilt list in `every_policy_call_reaches_the_one_consultation_or_is_marked_unbuilt` is `["T-021-06", "T-021-07"]`.
- **`Recorded` reads charges too.** Every implementer answers `charged()`: the test logs of `T-021-01`, `T-021-03`
  and `T-021-04` by `charged_in` over what they hold (the log that takes nothing fails its read), the assembly's
  `PolicyHandle` and the segments kit's `PolicyPath` over their logs' entries. `Charges` on the engine needs
  `L: PolicyLog + Recorded`, as `decide` and `authorise` do.
- **The agent scope covers `authorise` for real.** `agent_outside_its_partys_scope_is_refused` now drives the
  engine's `authorise`: an agent its party didn't scope for it is refused and recorded so, another agent of the party
  scoped for it authorises. `funds/tests.rs`'s one `ActsFor` need names the kind (`component`).
- **The assembly** (`conformance/scenarios/src/assembly/policy.rs`, `T-024-02`'s). Its register of actors keeps each
  login's kind (`Registers::list`), and its sources answer `ActsFor` by login, party and kind (`acts_as`; intake's
  `Actors` still asks `acts_for`, login and party). It keeps each agent's scope (`Registers::scope`, `in_scope`): a
  party that set none for an agent gives a missing input; no scenario names an agent yet.
  `PolicySide::charge` is the engine's, and `CHARGE_BOUND` is gone. `policys_sources_answer_from_the_records`
  checks the kind and the scope.
- **Rule-offs of the merge work** (core-authz's lib tests):

  | Rule switched off | Tests that went red |
  |---|---|
  | the counter rebuilt from the log before a charge | `bound is held ...`; `clock moving back ...`; `counter is the log's across a restart` |
  | a log that can't be read refuses | `counter is the log's across a restart` |
  | a charge's entry at Core's clock, not its own `at` | `clock moving back ...`; `bound is held ...` |
  | only a permitted charge entry counted | `bound is held ...`; `clock moving back ...`; `agent without ...`; `counter is the log's ...`; `requests fanned out ...`; `thousand agents ...` |
  | the agent scope on the engine's `authorise` (the rule out of `FIXED`) | `agent outside its party's scope is refused`; `agent stating another kind ...`; `agent without ...` |

### T-021-05, the re-review

- **A stale read starts no bound again** (item 1). The read of charges is held to the same two checks as `T-021-04`'s
  (`funds::Authorised::read`): it must extend the read the counter was last rebuilt from, and hold the engine's own
  last charge counted within its bound, as its entry holds it (`Charged::stands`). Otherwise the counter is an input
  the decision doesn't have, `missing_input`, over a bound of 0, and the counter stays as it was read last. As with
  `T-021-04`'s, a first read of another log, or one frozen after the engine's last charge, passes: that the read is of
  the very log the handle appends to is the assembly's word. Besides, no rebuild counts a window below the engine's own
  charges in it (`Charged::counted`, `own`), a floor behind the two checks.
- **A jump of the clock freezes no window** (item 2; revised in round 3, below). A charge counted more than one
  window ahead of Core's now doesn't choose the window, and is set apart: `Charged::ahead()` lists them after each
  read. The entry stays in the log, so a fresh engine over it is not frozen either. One window ahead still chooses the
  window, so a clock stepping back less than a window keeps the latest window.
- **A time no entry can hold is refused** (item 3). Where Core's clock gives a moment RFC 3339 can't hold, the charge is
  refused as `Err(AppendFailure::Refused)`, nothing recorded and nothing counted, never recorded at the time the charge
  states.
- **Intake binds the kind; reads of its status are a hand-off** (item 4). `intake::Actors::acts_for` takes the stated
  kind, and the assembly answers it from the register policy reads (`Registers::acts_as`), so an agent's login stating
  itself a person, or a person's stating itself an agent, is told nothing, quotes nothing and opens nothing. Routing
  `told` and `kept_for` through `check_read`, so an agent outside its party's `check_read` scope reads none of its
  party's orders, is not contained: intake answers an order's status from `open`, `quote` and `accept` before the log
  holds the order's acceptance, and a customer's read of an order asks the log's record of its parties
  (`Source::OrderParties`), so through `check_read` every customer would be refused its own new order. It needs either
  a read of an order's status that policy decides on intake's own record, or intake answering status only once the
  acceptance is in the log. Owner: `T-020-03`'s, raised with the coordinator. Until then, an agent its party didn't
  scope for `check_read` learns its party's orders' status through intake, though it can't move, authorise or charge
  outside its scope, each of which policy decides.
- **The cost of reading the whole log** (the nit). Every charge reads every permitted `charge` entry and rebuilds the
  counter, and the read check compares it with the last read: O(n) in the log's charge entries per charge, so O(n^2)
  over a run (`thousand_agents_under_one_party_hit_one_limit`, two thousand charges, runs in about three seconds in
  debug). Accepted for v0, as `T-021-04`'s read is; an index kept by the log, or a read of the entries after a given
  receipt, is `F-022`'s or `F-030`'s to add.
- **Rule-offs** (core-authz's lib tests, core-orch's `intake` for its edit):

  | Rule switched off | Tests that went red |
  |---|---|
  | a read extends the one before | `stale read of the log starts no bound again` (the truncated read) |
  | a read holds the engine's own last charge | `stale read of the log starts no bound again` (the empty read) |
  | no window counted below the engine's own | `rebuild counts no window below the engine's own` |
  | the engine's own of a window gone left out | `rebuild counts no window ...`; `bound is held ...`; `clock jumping far ahead freezes no window` |
  | a charge far ahead neither chooses the window nor counts | `clock jumping far ahead freezes no window` |
  | a charge far ahead set apart for the operator | `clock jumping far ahead freezes no window` |
  | a time no entry holds is refused | `charge at a time no entry can hold is refused` |
  | intake's `told` holds the kind | `actor stating another kind is not the requester` |
  | intake's `open` holds the kind | `actor stating another kind is not the requester` |
  | intake's own order's record (`speaks_for`) holds the kind | `actor stating another kind is not the requester` |

### T-021-05, round 3 (approved, before the merge)

- **A step back of more than a window gives no fresh bound** (low 1, the reviewer's fix). A charge ahead of Core's now
  by more than a window still never chooses the window, but it now counts in the window chosen. A clock stepping back
  more than a window (1 h 1 s after the bound was spent: the next charge is over), or two engines over one log whose
  clocks are further apart than a window, give the party no second bound; a jump forward and back takes off the bound
  only what was charged during the glitch, in every window until Core's clock reaches it, which fails closed. The 2030
  jump still recovers: the window follows Core's now, now's charges leave it at the next window, the glitch's stay.
  Tests: `clock_stepping_back_more_than_a_window_gives_no_fresh_bound`,
  `engines_with_skewed_clocks_hold_a_party_to_one_bound`, and `clock_jumping_far_ahead_freezes_no_window` as revised.
- **Who surfaces `ahead()`** (low 2). Nothing calls `Charged::ahead()` yet. Owner: `T-023-01`, the operator's notices.
  It must surface to the operator, after a charge whose read set any apart, each charge counted ahead of Core's now
  by more than a window: its party, its request id, its kind of work and count, and the moment it was counted at
  beside Core's now, so the operator sees a clock that jumped and what it cost the party. The assembly reaches it as
  `engine.charged().ahead()`.
- **The intake hand-off is tracked** (item 3): recorded in `F-020`'s notes under `T-020-03`, for its owner, due before
  `M4`, with the reviewer's severity: low, the agent's own party only, read only.
- **Rule-offs** (core-authz's lib tests):

  | Rule switched off | Tests that went red |
  |---|---|
  | a charge far ahead still counts in the window | `clock stepping back more than a window ...`; `engines with skewed clocks ...`; `clock jumping far ahead ...` |
  | a charge far ahead chooses no window | `clock jumping far ahead freezes no window` |

### T-021-05, at integration (contract head `acde1cf`)

- **Finding 4 is closed.** core-internal's `ChargeOutcome` now has `refused { class }` and `not_known { cause }`.
  `charge_outcome` answers a charge policy refuses with its own class and one it can't decide as not known, so the
  log holds the real answer, never `over_bound` for a bound not passed. core-orch's `rounds::Run::charge` says it
  too: `NotOffered::ChargeRefused { class }` and `NotOffered::ChargeNotKnown { cause }`, nothing sent on either.
  Intake and arrivals map them to `RefusedByPolicy` and `NoRouteRightNow` (two arms each, added in rounds' commit
  under the coordinator's one-off exception to file ownership).
- **`counted_at`.** A charge counted within its bound says when Core's clock counted it in the contract's own field,
  `What::Charge::counted_at`, present exactly where the entry is `permitted`; the entry's time is the charge's own
  again, as every call's is. `charged_in` rebuilds by `counted_at`. This replaces the earlier default of recording the
  counting moment as the entry's `when`.
- **The two new decision kinds.** `report_outcome` and `confirm_outcome` are policy calls of the engine (`Asked`
  variants, each a `Scope` of its own, held to `ActsFor` by its reporter or confirmer) and rows of the unbuilt table,
  `T-021-06`'s to build. `StuckOrders` is implemented on every engine, not only `Engine<L, Absent, Absent>`, so the
  assembly's `PolicySide` forwards all four of its calls to the engine, each answered not known until then.
  core-audit's exhaustive kind list names `report_outcome`.
- **What the log refused, classified.** One history in this row's tests: `report_outcome` and `confirm_outcome` of
  the contract's fixture requests through the assembly, refused as "an outcome's report is of a move of this log whose
  outcome is unknown" and its confirmation's twin. Unrealistic test history: the accepted order has no move whose
  outcome is unknown, and the rows hold rightly. The test now asserts the refusal leaves nothing recorded; driving a
  real unknown outcome is `T-021-06`'s and the stuck scenarios'.
- **Rule-offs** (each switched off once, the file restored and touched):

  | Rule switched off | Tests that went red |
  |---|---|
  | a refused charge answers refused for its class | five of `parties`' tests, `agent without ...` and `stale read ...` among them |
  | a charge not known answers not known | `the unbuilt engine permits nothing ...`; `agent outside its party's scope ...` |
  | the rebuild reads `counted_at` | `a counted charge says when Core counted it`, and eleven others of the counter's |
  | a charge's entry at its own time | `a counted charge says when Core counted it` |
  | rounds: a refused charge says refused | `a charge policy refuses or cannot decide sends nothing and says why` |
  | rounds: a charge not known says not known | `a charge policy refuses or cannot decide sends nothing and says why` |
  | the assembly's charge is the engine's | all five of `assembly_policy` |
  | the assembly's outcome report is the engine's | `a move's outcome reported or confirmed reaches the engine and its log` |

## T-021-06

- **What it builds.** `StuckOrders` on `Engine<L, P, S>` in `components/core-authz/src/stuck/`, for
  `L: PolicyLog + Receipted`, `P: Policy`, `S: Sources`: `decide_stuck`, `move_stuck`, `report_outcome` and
  `confirm_outcome`, each decided through `consult` and recorded through `PolicyLog` before it answers. All four left
  the unbuilt table of `every_policy_call_reaches_the_one_consultation_or_is_marked_unbuilt`, which is now
  `["T-021-07"]`; each is driven there on the contract's own coherent world (`fixtures/admission/world--sd`, `--sm`,
  `--report_es`, `--settled_es`), through the guarded log, consulted once and ruled once. Two layers: a fixed rule, the fifth in `FIXED`, over what the request alone says; and rules read from the log on
  policy's handle, held to the consultation's verdict after it, as `T-021-04`'s register is, since a move names the
  decision it carries out and not its decider, and a confirmation names its report and its move by receipt.
- **Reading of "the ownership row".** Taken as the rows that name `core-authz` `stuck` (`T-021-06`) as their
  implementer or owner: core-internal's call table (`decide_stuck`, `move_stuck`), the `StuckOrders` trait as a whole
  (`report_outcome`, `confirm_outcome` too), and the README's hand-off rows naming `T-021-06` (the evidence relied on,
  the decide step's transaction, the outcome reported and confirmed, the `ActsFor` chains), with `DEC-14`'s "separate
  decide and move permissions". Every permission they name is built: deciding, moving and confirming, one each.
  The transaction row needs nothing of policy's beyond recording the decide request whole: the log holds the
  transaction fresh (`stuck-resolution-transaction-is-its-own`) and the effect under it. A hand-on
  (`resolution: hand_on`) and a delivery are decided and moved on the same two permissions. Reversible.
- **One accountable actor: "reaches", never "meets"** (the coordinator's ruling on the review of `fa19450`,
  reversible, on the owner's form). Two actors count as one where they share a login, share a party, or either's
  `ActsFor` chain reaches the other's party (`Source::Reaches { from, to }`). Two whose chains only meet at a party
  both reach, two desks of the operator, are distinct: ADR-0011's separate staff within one party. One rule
  (`Engine::one_actor`) holds the decider against the mover and the reporter against the confirmer. The same reading
  holds an actor to a party (`Engine::of_party`): it is the party's where its login or party is that party, or its
  chain reaches it. That is how the order's customer, the provider holding the funds and the deployment's operator
  are each told. It replaces `fa19450`'s `Source::CommonParty`, which refused chains that met, and its default 7,
  which compared decide and move by `same_accountable_actor` alone.
- **The fixed rule** (`stuck::needs`, `stuck::rule`), asked after every earlier row's, so each actor is bound to its
  party and kind (`T-021-03`) and an agent to its scope (`T-021-05`) first:
  - deciding needs `Permission::DecideStuck`, moving `Permission::MoveStuck`, confirming
    `Permission::ConfirmOutcome`, each asked of the register by login (`Source::Permissions`), so one alone allows
    only its own step (`REQ-AUZ-4`, `DEC-14`, the `REQ-AUZ-6` pattern);
  - deciding and moving are the operator's (`REQ-ORD-28`: the operator resolves and bears a stuck order). A decider
    or a mover must state the operator's role, or it is refused `not_permitted` before an input is asked, and must be
    the deployment operator's by the register, in its party or reaching it (`Source::OfOperator`), since a role is
    only the request's word. Default, reversible: a confirmer may act in any role the register gives the permission
    to, since the README's silent `move_value` has Core report and "one person" confirm, and a checker might;
    reporting needs no permission, since only the move's own mover reports, which the log holds
    (`outcome-report-rests-on-its-move`), so `report_outcome` has no rule of this row's beyond the fixed rules every
    call is held to;
  - the provider holding the funds never decides (`THR-70`): a decider that is the decision's `resolution.by` by its
    party or login is refused outright, and one whose chain reaches it is refused (`Source::Reaches` must say no);
  - neither the decider nor the confirmer acts for a party of the order, its requester, beneficiary or payer
    (`Source::OrderParties` must say no). Asked by party, not by role: `ActsFor` binds a login's party and kind but
    not its role, so a customer's login stating `operator` is still the customer. Default: "the order's customer" is
    any of its three parties. Their chains are held below;
  - each evidence commitment the decision names is one the store keeps, about the order or about no order in
    particular (`Source::Kept`), and at least one is about the order itself (`Source::KeptAbout`), or the decision is
    refused `missing_input`: a digest the store keeps nothing under names no evidence, and evidence about no order
    alone names none of this order's (`REQ-AUZ-4`, `F-020`'s hand-off "the evidence digests checked against the
    store"; the second half is the coordinator's ruling, replacing default 6). A decision naming none at all still
    fails its contract check and is refused unrecorded, as the callee rule says.
- **Rules read from the log** (`stuck::Receipted`, a trait of this module, on policy's handle: every entry with its
  receipt, in log order; steps are found by receipt equality, as the contract's rows find them):
  - neither the decider nor the confirmer reaches any party the log's acceptance of the order names, refused
    `not_permitted` (`THR-70`); an order the log has no acceptance of refuses `missing_input`;
  - a move carries out a decision the log holds as permitted, answering the request it names, with the decision id
    it names, of its own order, or it is refused `not_permitted`; every such decision must pass;
  - its mover and that decider are one actor, refused `same_actor` (`REQ-AUZ-4`, `REQ-AUZ-8`: two agents of one party,
    its person and its agent, one login under two parties, a desk's bot whose party acts for the desk);
  - a confirmation cites a report the log holds as permitted, of its own move (effect), order and outcome, or it is
    refused `not_permitted`; its confirmer and the reporter are one actor, refused `same_actor` (core-internal's
    README hand-off: the log reads no register, so it can't see along the chains);
  - neither the mover nor the confirmer is the desk of the provider holding the funds (`THR-70`): the resolution's
    `by` for a `resolve_stuck`, the move's `provider` for a segment's `move_value`. An actor is that desk where its
    party or its login is the provider, or its own chain reaches it, refused `not_permitted`. Chains that meet the
    holder's only above it don't count: with `operator-desk-b -> operator` and `bank-co -> operator`, `op-login-5`
    moves;
  - a log that can't be read refuses `missing_input`; a register of chains out of reach leaves the answer not known
    (`verifier_unreachable`, as an unreachable source gives in `consult`); a register that says nothing refuses
    `missing_input`. These rules run only where the consultation didn't refuse, and never permit
    (`stuck::held`): a refusal stands, a refusal of theirs beats not known, and their not known replaces a permit.
- **What "reaches" does to the contract's world.** The world pairs a decider and a confirmer in `operator` with a
  mover and reporter on `operator-desk-b`. Where the register lists `operator-desk-b` as acting for `operator`, the
  mover's chain reaches the decider's party, so the two are one actor. This row's tests put the decider and the
  confirmer on `operator-desk-a`, a sibling desk, and the engine's table answers the world's pair with a register
  that links neither. The world's fixtures are the contract's and are left as they are.
- **Stale reads.** Every rule here looks a step up and refuses where it isn't found, so a stale or truncated read
  fails closed. `T-021-04`'s two read checks (the read extends the last; it holds the engine's own last append) are
  not applied: nothing here counts or replaces, so a read missing entries can only refuse. What a read of another log
  could do, name another decider or reporter, is the assembly's word that the read is of the very log the handle
  appends to, as `T-021-04`'s and `T-021-05`'s notes say. Reversible.
- **Refusal classes.** `same_actor` (core-internal's own class for `REQ-AUZ-4`) for one actor at both steps or at
  both words; `not_permitted` for a role, a permission, a customer, the holder's desk, or no step behind the call;
  `missing_input` for evidence the store doesn't keep and a log that can't be read. No contract change was needed.
- **Edits outside this row's module**, each the smallest that lets the rules be asked, as `T-021-05`'s were:
  - `engine/` (`T-021-01`'s): four `Source` variants appended (`Kept`, `KeptAbout`, `Reaches`, `OfOperator`; the
    review of `fa19450` replaced `CommonParty`); the fifth `FIXED` entry;
    `Engine::not_known` removed, since no stub uses it now, and the `Unbuilt` doc says a built call answers on it as
    it rules; in its tests, `Receipted` on `Taking`, `Guarded`, `Seen` and `PolicysHandleOnly` (`Taking` and
    `PolicysHandleOnly` giving each entry a receipt of its own, `nth_receipt`), the four calls built in the table with
    the world's histories (`world_before`, `say`), and the unbuilt engine's test expecting a report not known (an
    actor's register out of reach) and a decision, a move and a confirmation refused (an order with no acceptance,
    nothing permitted behind them; a refusal beats not known).
  - `roles/` (`T-021-03`'s): three `Permission` variants appended (`DecideStuck`, `MoveStuck`, `ConfirmOutcome`).
  - `parties/tests.rs` (`T-021-05`'s): `Calls::by` takes the stuck steps in the operator's role, and its register
    answers `OrderParties` yes only for the read fixture's order, so no caller is a party of the stuck fixtures', and
    `Reaches` no, so each caller is its own party's alone.
  - `conformance/scenarios/` (`F-024`'s): `Entries::receipted` (provided, `None` by default, the audit log's
    entries as it holds them), `Receipted` on `PolicyHandle` and on the kit's `PolicyPath` (`receipt_at`), the
    assembly's `Records` answering `Kept` and `KeptAbout` from the log's stated entries, `Reaches` from its register
    (`Registers::chain`, `Registers::reaches`) and `OfOperator` as reaching the kit's operator party, two stale doc
    lines in `assembly/policy.rs`, and `policys_sources_answer_from_the_records` checking each.
- **Hand-offs naming `T-021-06`, checked off.**
  - `F-020`'s "Gap for `T-021-06`: policy's stuck rules are unbuilt" (`T-020-11`): built. Its questions answered: a
    customer never decides; the desk of the provider holding the funds never moves; neither confirms; the evidence
    digests are checked against the store (`Source::Kept`).
  - `F-020`'s `T-020-11` review answers, finding 5 (the roles a decider and a mover may hold) and round 4's S1 (the
    confirmer's roles): built as above.
  - This file's `T-021-05` hand-off (`same_accountable_actor` for the decider and the mover): used, and for the
    reporter and the confirmer too.
  - This file's `T-021-05` integration note (`report_outcome`, `confirm_outcome`, `T-021-06`'s to build; a real unknown
    outcome to drive): built; a real unknown outcome, the world's silent `resolve_stuck` and its silent withdrawal,
    is driven through the engine in this module's tests. Through the assembly it is `T-024-06`'s scenario.
  - core-internal's README: the call table's two rows, the hand-off rows on the evidence, the transaction, the
    outcome reported and confirmed, and the `ActsFor` chains (and the CHANGELOG's two entries naming `T-021-06`):
    built. No contract change.
  - `PLAN.md`'s `T-024-06` depends on this row: its `AC-6` scenario (`failed segment with no route left is resolved
    under policy by two actors`) can now run on the engine. Owner: `T-024-06`. No test yet drives the assembly's
    `PolicySide` through a permitted decide, move and confirm; for one to pass, the scenario must list each login
    (`Registers::list`) and give it its permission (`Registers::permit` with `DecideStuck`, `MoveStuck` or
    `ConfirmOutcome`); name decision evidence whose commitment the log holds as a stated entry about the order or
    about no order, at least one about the order itself (the kit's `"ab"`×32 digest is refused `missing_input`); use a mover whose party, login and chain
    don't reach the resolution's `by`; a decider and a mover the register gives the operator (`Registers::list` along a
    chain to the kit's `operator` party); a decider and a confirmer that are none of the order's parties and whose
    chains reach none; and a confirmer that neither reaches its reporter's party nor is reached by it. For a
    `resolve_stuck`, sibling desks under `operator` (`desk-a -> operator`, `desk-b -> operator`) satisfy every one of
    these: one decides and confirms, the other moves and reports; a person listed in `operator` itself does not pair
    with a desk under it. For a segment's silent `move_value` (`AC-6`'s other path), whose reporter is Core, they
    do too since round 2 of the review: Core acts for a party of its own (`kit::CORE_PARTY`, `rutent`), so a desk
    under `operator` neither reaches it nor meets it below the operator. Two desks under one of the operator's
    divisions, not directly under it, meet there and are one actor (round 2).
- **Hand-offs left, each to its owner** (doc lines this row made stale, in files it doesn't own):
  - `T-020-11`'s `components/core-orch/src/stuck/mod.rs`: the module's **Gaps** paragraph and the doc comments of
    `known_outcome` and `confirm_outcome` still say policy's stuck rules are unbuilt and the roles `T-021-06`'s to rule.
  - `F-024`'s `conformance/scenarios/tests/segments_support/resolving.rs` (`staff`, `PermitsEveryStep`),
    `tests/assembly_policy.rs` (`a_moves_outcome_reported_or_confirmed_reaches_the_engine_and_its_log`) and
    `tests/stuck.rs` (`outcome_report_under_policys_unbuilt_rules_settles_nothing`, whose name and doc read
    "unbuilt"): the engine there is `Unbuilt`, the one with no policy service and no sources, and still answers not
    known, so each test holds; only their words are stale.
- **Gaps, each with its owner.**
  - The deployment's registers of permissions, of chains and of what each login acts for are the deployment's to keep
    and record (`F-030`). The assembly's register lists one party per login, so its chains are a login's party and
    that party's own; a login listed for several parties is `F-030`'s.
  - The evidence store's word (`Source::Kept`): the assembly answers it from the log's stated entries, which name each
    kept statement by its commitment; production answers it from the store itself (`KeptStatements`, `T-022-03`),
    wired by `F-030`. Only that the store keeps the evidence, about the order, is checked; whether it bears the
    decision out is the decider's judgement, recorded with the decision, and the reviewer's to read.
  - Two engines over one log in separate processes race between the read and the append, as `T-021-04`'s and
    `T-021-05`'s reads do; `F-022`'s or the contract author's to close with an admission row. Here the log's own rows hold
    part of it after: `outcome-confirmation-rests-on-its-report` compares the reporter's and the confirmer's party and
    login, but `stuck-move-carries-out-its-decide-step` compares the decider's and the mover's party only, so a race
    could put one login under two parties at both steps through the log (corrected on the review of `fa19450`).
    Contract item, queued by the coordinator with the contract author: add a `login` field to
    `stuck-move-carries-out-its-decide-step`, refusing a stuck move whose mover's `acts` is its decide step's
    decider's.
  - The read is O(n) in the log's entries per move or confirmation, as `T-021-04`'s is; an index is `F-022`'s or
    `F-030`'s.
- **Rule-offs of `fa19450`.** Each rule switched off once by a scratch script, the file restored and touched; core-authz's lib
  tests, or `rutent-scenarios`' `assembly_wiring` for the assembly's. Every test of this module was written first and
  seen red against the stub (all nine failed). Four rule-offs first turned nothing red: the holder's desk by party or
  login and the reporter and confirmer as one actor, each masked by the chain rule behind it, and only a permitted
  report and a report of the confirmation's own move, which no case drove; a case was added for each (a register
  whose chains are all apart, a refused report, a report cited for another move), and each then went red.

  | Rule switched off | Tests that went red |
  |---|---|
  | decider in the operator's role | `deciding moving and confirming are separate permissions` |
  | mover in the operator's role | `deciding moving and confirming are separate permissions` |
  | deciding its own permission (asks the move's) | `every policy call reaches the one consultation or is marked unbuilt`; `deciding moving and confirming are separate permissions` |
  | moving its own permission (asks the decide's) | `deciding moving and confirming are separate permissions`; `every policy call reaches the one consultation or is marked unbuilt` |
  | confirming its own permission (asks the move's) | `deciding moving and confirming are separate permissions`; `every policy call reaches the one consultation or is marked unbuilt` |
  | the decider no party of the order | `neither the customer nor the holders desk resolves its order` |
  | the confirmer no party of the order | `neither the customer nor the holders desk resolves its order` |
  | each evidence one the store keeps | `resolution naming no evidence is refused`; `step with nothing permitted behind it is refused` |
  | the rule in FIXED | `deciding moving and confirming are separate permissions`; `resolution naming no evidence is refused`; `step with nothing permitted behind it is refused`; `neither the customer nor the holders desk resolves its order` |
  | a move carries out a decision the log holds | `the unbuilt engine permits nothing on any call and records first`; `step with nothing permitted behind it is refused` |
  | only a permitted decision | `step with nothing permitted behind it is refused` |
  | a decision of the move's own order | `step with nothing permitted behind it is refused` |
  | decider and mover not one accountable actor | `one actor attempting both decide and move is refused`; `two agents of one party count as one actor` |
  | the holder's desk by party or login | `neither the customer nor the holders desk resolves its order` |
  | the holder's desk by chain | `neither the customer nor the holders desk resolves its order` |
  | the mover not the holder's desk | `neither the customer nor the holders desk resolves its order` |
  | only a permitted report | `step with nothing permitted behind it is refused` |
  | a report of the confirmation's own move | `step with nothing permitted behind it is refused` |
  | a report of the same outcome | `step with nothing permitted behind it is refused` |
  | reporter and confirmer not one accountable actor | `two agents of one party count as one actor` |
  | reporter's and confirmer's chains apart | `confirmer whose chain meets the reporters is refused` |
  | the confirmer not the holder's desk | `neither the customer nor the holders desk resolves its order` |
  | a move_value's holder its provider | `neither the customer nor the holders desk resolves its order` |
  | a resolve_stuck's holder its resolution's | `neither the customer nor the holders desk resolves its order`; `every policy call reaches the one consultation or is marked unbuilt` |
  | an unread log refuses the move | `unread log or unreached register moves nothing` |
  | an unread log refuses the confirmation | `unread log or unreached register moves nothing` |
  | a register of chains out of reach is not known | `unread log or unreached register moves nothing` |
  | a read rule's refusal held to the verdict | `the unbuilt engine permits nothing on any call and records first`; `neither the customer nor the holders desk resolves its order`; `step with nothing permitted behind it is refused`; `one actor attempting both decide and move is refused`; `two agents of one party count as one actor`; `unread log or unreached register moves nothing`; `confirmer whose chain meets the reporters is refused` |
  | a read rule's not known held to a permit | `unread log or unreached register moves nothing` |
  | the assembly: chains answered from the register | `policys sources answer from the records` |
  | the assembly: a chain followed past its first link | `policys sources answer from the records` |
  | the assembly: kept evidence about its order | `policys sources answer from the records` |
  | the assembly: unkept evidence says no | `policys sources answer from the records` |

### T-021-06, the review of `fa19450`

The reviewer confirmed each security finding with a probe; findings 1 and 2 blocked. The coordinator ruled on each
(reversible defaults, on the owner's form), and m3 at `066b2a7` (`T-021-04b`) was merged in the same round, cleanly.
The rules and the tests above describe the code after this round.

| # | Finding | Answer | Test |
|---|---|---|---|
| 1 | High: `alice` (`operator-desk-a`) decides and `alice-bot` (`desk-a-bots -> operator-desk-a`) moves | One accountable actor is "reaches, never meets": one login, one party, or either's chain reaching the other's party (`Source::Reaches`, replacing `CommonParty`), for the decider against the mover | `one actor attempting both decide and move is refused` (the bot, and a person of `operator`, which the decider's chain reaches) |
| 2 | Medium: `cust-1-ops -> cust-1` decides | A decider or confirmer whose chain reaches any party the log's acceptance of the order names is refused; an order with no acceptance is a missing input | `neither the customer nor the holders desk resolves its order`; `unread log or unreached register moves nothing` |
| 3 | Medium: `bank-co-desk` states the operator's role | A decider or mover must also be the operator's by the register (`Source::OfOperator`); the holder never decides, by its party or login, or by its chain reaching `resolution.by` | `deciding moving and confirming are separate permissions` (`acme-staff`); `neither the customer ...` |
| 4 | Medium: the holder check refused chains that met | Refused only where the actor's own chain reaches the holder: with `operator-desk-b -> operator` and `bank-co -> operator`, `op-login-5` moves | `neither the customer ...` |
| 5 | Reporter against confirmer | The same rule; siblings under `operator` confirm. The test is renamed `confirmer_whose_chain_reaches_the_reporters_is_refused` | `confirmer whose chain reaches the reporters is refused` |
| 6 | Low: the race note said the log's stuck-move row holds the login | Corrected above; a `login` field on `stuck-move-carries-out-its-decide-step` is a contract item, queued by the coordinator | — |
| 7 | Low: the `T-024-06` hand-off | The chain conditions added, and that sibling desks under `operator` satisfy them | — |
| 8 | Default 6, evidence | At least one evidence item about the order itself (`Source::KeptAbout`) | `resolution naming no evidence is refused` |
| 9 | Nits | `Taking` and `PolicysHandleOnly` give each entry its own receipt (`nth_receipt`); cases for a move naming another request than its decision answered and for a confirmation of another order than its report's | `step with nothing permitted behind it is refused` |

- **Tests first, this round.** The `CommonParty` source was replaced, so the old rules and the new tests share no
  API to run red against; each new rule's test was instead seen red by switching that rule off (below), every one of
  them. Five rule-offs first turned nothing red, or weren't applied: the holder deciding by party or login, masked
  by its chain; the customer by the order's record, directly, masked by the log's acceptance read after; and the
  assembly's two evidence rules, whose text formatting moved. Cases were added (a register whose chains reach nothing;
  the fixed rule asked alone through `consult`; a statement about no order) and the scripts' strings fixed, and each
  then went red.
- **Rule-offs of this round** (core-authz's lib tests, or `rutent-scenarios`' `assembly_wiring` for the assembly's;
  each switched off once, the file restored and touched):

  | Rule switched off | Tests that went red |
  |---|---|
  | decider in the operator's role | `deciding moving and confirming are separate permissions` |
  | the holder never decides, by party or login | `neither the customer nor the holders desk resolves its order` |
  | the holder never decides, by chain | `neither the customer nor the holders desk resolves its order` |
  | the decider the operator's by the register | `deciding moving and confirming are separate permissions` |
  | mover in the operator's role | `deciding moving and confirming are separate permissions` |
  | the mover the operator's by the register | `deciding moving and confirming are separate permissions` |
  | deciding its own permission (asks the move's) | `every policy call reaches the one consultation or is marked unbuilt`; `deciding moving and confirming are separate permissions` |
  | moving its own permission (asks the decide's) | `deciding moving and confirming are separate permissions`; `every policy call reaches the one consultation or is marked unbuilt` |
  | confirming its own permission (asks the move's) | `deciding moving and confirming are separate permissions`; `every policy call reaches the one consultation or is marked unbuilt` |
  | the decider no party of the order, directly | `neither the customer nor the holders desk resolves its order` |
  | the confirmer no party of the order, directly | `neither the customer nor the holders desk resolves its order` |
  | each evidence one the store keeps | `resolution naming no evidence is refused` |
  | one evidence about the order itself | `resolution naming no evidence is refused` |
  | the rule in FIXED | `deciding moving and confirming are separate permissions`; `neither the customer nor the holders desk resolves its order`; `step with nothing permitted behind it is refused`; `resolution naming no evidence is refused` |
  | the decider reaches no party of the order | `the unbuilt engine permits nothing on any call and records first`; `neither the customer nor the holders desk resolves its order`; `unread log or unreached register moves nothing` |
  | an order the log has no acceptance of is missing | `the unbuilt engine permits nothing on any call and records first`; `unread log or unreached register moves nothing` |
  | an unread log refuses the decision | `unread log or unreached register moves nothing` |
  | a move carries out a decision the log holds | `the unbuilt engine permits nothing on any call and records first`; `step with nothing permitted behind it is refused` |
  | only a permitted decision | `step with nothing permitted behind it is refused` |
  | a decision answering the move's request | `step with nothing permitted behind it is refused` |
  | a decision of the move's own order | `step with nothing permitted behind it is refused` |
  | one actor: one login or one party | `two agents of one party count as one actor` |
  | one actor: the first's chain reaches the second's party | `one actor attempting both decide and move is refused`; `confirmer whose chain reaches the reporters is refused` |
  | one actor: the second's chain reaches the first's party | `one actor attempting both decide and move is refused`; `confirmer whose chain reaches the reporters is refused` |
  | decider and mover not one actor | `one actor attempting both decide and move is refused`; `two agents of one party count as one actor` |
  | an actor is a party's by its party or login | `neither the customer nor the holders desk resolves its order` |
  | an actor is a party's by its chain | `neither the customer nor the holders desk resolves its order` |
  | the mover not the holder's desk | `neither the customer nor the holders desk resolves its order` |
  | only a permitted report | `step with nothing permitted behind it is refused` |
  | a report of the confirmation's own move | `step with nothing permitted behind it is refused` |
  | a report of the confirmation's own order | `step with nothing permitted behind it is refused` |
  | a report of the same outcome | `step with nothing permitted behind it is refused` |
  | reporter and confirmer not one actor | `confirmer whose chain reaches the reporters is refused`; `two agents of one party count as one actor` |
  | the confirmer reaches no party of the order | `neither the customer nor the holders desk resolves its order` |
  | the confirmer not the holder's desk | `neither the customer nor the holders desk resolves its order` |
  | a move_value's holder its provider | `neither the customer nor the holders desk resolves its order` |
  | a resolve_stuck's holder its resolution's | `neither the customer nor the holders desk resolves its order`; `every policy call reaches the one consultation or is marked unbuilt` |
  | an unread log refuses the move | `unread log or unreached register moves nothing` |
  | an unread log refuses the confirmation | `unread log or unreached register moves nothing` |
  | a register of chains out of reach is not known | `unread log or unreached register moves nothing` |
  | a read rule's refusal held to the verdict | `the unbuilt engine permits nothing on any call and records first`; `confirmer whose chain reaches the reporters is refused`; `neither the customer nor the holders desk resolves its order`; `one actor attempting both decide and move is refused`; `step with nothing permitted behind it is refused`; `two agents of one party count as one actor`; `unread log or unreached register moves nothing` |
  | a read rule's not known held to a permit | `unread log or unreached register moves nothing` |
  | the assembly: reaches answered from the register | `policys sources answer from the records` |
  | the assembly: a chain followed past its first link | `policys sources answer from the records` |
  | the assembly: the operator's along its chain | `policys sources answer from the records` |
  | the assembly: kept evidence about its order | `policys sources answer from the records` |
  | the assembly: evidence about the order itself | `policys sources answer from the records` |
  | the assembly: unkept evidence says no | `policys sources answer from the records` |

### T-021-06, round 2 of the review

All four round-1 probes were refused; the coordinator refined the ruling, and these are its points as built
(reversible defaults, on the owner's form). The tests were written first this round and seen red against the round-1
rules: the three probes' tests failed (`one actor attempting both ...`, `neither the customer ...`,
`confirmer whose chain reaches ...`) and the assembly's source test (Core's party), before any rule changed.

| # | Point | Answer | Test |
|---|---|---|---|
| 1 | High: chains meeting below the operator were two actors (`alice1` on `desk-a1` and `alice2` on `desk-a2`, both under `alice-person` under `operator`; `alice-d` and `alice-m` in two bot parties under `operator-desk-a`) | One actor also where the two chains meet at any party below the deployment operator's own party (`Source::MeetsBelowOperator`), for decider against mover and reporter against confirmer. Only desks directly under the operator, meeting nowhere below it, are two | `one actor attempting both decide and move is refused`; `confirmer whose chain reaches the reporters is refused` |
| 2 | Medium: the customer and holder checks looked one way | A decider or confirmer is refused where its chain reaches a party of the order or where that party's chain reaches its party (the customer's holding company); a confirmer the same against the holder (`bank-group` over `bank-co`); the mover's holder check stays one way (`REQ-CAT-20`) | `neither the customer nor the holders desk resolves its order` |
| 3 | Core gets a party of its own | `kit::CORE_PARTY` (`rutent`) for `kit::core()`, and for Core's actors in core-orch's tests (`closing.rs`, `segments.rs`, `snapshot_support`, `intake_support`'s policy actor), so the decide caller and the reporter carry one party; the assembly's register lets nothing but Core's own components act for Core's party or a party reaching it; Core passes no `OfOperator`, so it never decides or moves | `core never decides or moves`; `policys sources answer from the records` |

- **Reading of "below the operator".** Each chain is cut where it reaches the operator's party, which is not compared,
  nor anything above it, so a party above the operator (its holding company) doesn't make every pair of its staff one
  actor. Default, reversible.
- **The holder never decides, both ways** (default, not ruled). The decider is also refused where the holder's chain
  reaches its party, as the confirmer is, since only the mover was ruled one way (`REQ-CAT-20`). A fixed rule
  (`Source::Reaches { from: by, to: decider's party }` must say no).
- **Only the parties a chain names count.** Either way, an actor is a party's only along the chains: a desk hanging
  under one of that party's parents, its sibling, is not the party's (the meeting rule is for two actors, not for an
  actor and the customer or the holder).
- **The contract's fixtures.** No test of this row relies on the world's actor shape: each puts the decider and the
  confirmer on `operator-desk-a` and the world's mover and reporter on `operator-desk-b`, siblings directly under the
  operator. The world's own pair, a decider in `operator` and a mover on `operator-desk-b`, is one actor under the
  register these tests give it; `contracts/` is not this row's to edit, so the fixtures stay as they are and the
  engine's table answers the world's pair on a register that links neither.
- **Hand-offs to `F-030`** (the production registers):
  - the register of actors and parties must be complete: `Reaches` and `MeetsBelowOperator` answer no for a link the
    register doesn't hold, so a missing link lets two actors pass as two;
  - the chain `MeetsBelowOperator` compares is defined as the parties reachable from the login without passing through
    the operator's party (round 3). A register that links a party above the operator by another path, around it,
    makes that party part of the chains below, so every two staff reaching it meet there and are refused as one actor:
    an over-refusal, failing closed, and the known result of such a register, not a bug;
  - a provider's desk listed under the operator rests on a `REQ-CAT-20` record that the operator and that provider
    are one legal party;
  - Core's own party, apart from the operator's, with nothing but Core's components acting for it or reaching it, is a
    deployment requirement;
  - the assembly's register maps each login and party to one parent (`Registers::actors`), a known limit: a party
    acting for two parents is the production register's.

- **Rule-offs of round 2** (core-authz's lib tests, or `rutent-scenarios`' `assembly_wiring` for the assembly's and
  the kit's; each switched off once, the file restored and touched). The round-1 table's rules were not run again: this
  round only added cases to their tests. The arm of `MeetsBelowOperator` out of reach is a second guard, as
  `Reaches`' is, and no test reaches it on its own, since a confirmation asks `Reaches` first.

  | Rule switched off | Tests that went red |
  |---|---|
  | one actor: chains meeting below the operator | `one actor attempting both decide and move is refused`; `confirmer whose chain reaches the reporters is refused` |
  | chains meeting below the operator refuse | `one actor attempting both decide and move is refused`; `confirmer whose chain reaches the reporters is refused` |
  | the holder never decides, its chain reaching the decider's party | `neither the customer nor the holders desk resolves its order` |
  | a party of the order reaching the decider's or confirmer's party | `neither the customer nor the holders desk resolves its order` |
  | the holder reaching the confirmer's party | `neither the customer nor the holders desk resolves its order` |
  | either way: the party's chain reaching the actor's party | `neither the customer nor the holders desk resolves its order` |
  | the decider the operator's by the register (Core) | `deciding moving and confirming are separate permissions`; `core never decides or moves` |
  | the mover the operator's by the register (Core) | `deciding moving and confirming are separate permissions`; `core never decides or moves` |
  | the assembly: chains meeting below the operator | `policys sources answer from the records` |
  | the assembly: a chain cut at the operator's party | `policys sources answer from the records` |
  | the assembly: none but Core's components under Core's party | `policys sources answer from the records` |
  | the assembly: Core's own components act for it | `assembled order closes on the public record and the reserves statement`; `policy the log core and the world read one clock`; `policys sources answer from the records`; `silent provider records its deposit failed and moves nothing for it`; `rounds transactions are kept across routings and no segment runs under one` |
  | the kit: Core's own party | `policy the log core and the world read one clock`; `policys sources answer from the records`; `assembled order closes on the public record and the reserves statement`; `rounds transactions are kept across routings and no segment runs under one`; `a tuples id is the digest of its canonical json`; `evidence store is bound to cores own log`; `reserve a settlement account is at is not one of the orders parties`; `staging is the one the scenario sets`; `silent provider records its deposit failed and moves nothing for it`; `catalogues grant is the one the log records` |

### T-021-06, round 3 of the review (approved)

Approved, with one medium finding fixed before the merge, and two lows in these notes.

| # | Finding | Answer | Test |
|---|---|---|---|
| 1 | Medium: the customer's or the holder's sister company decides or confirms (`cust-1 -> cust-holdco` and `cust-sister -> cust-holdco`; `bank-co` and `bank-sister` under `bank-group`) | A decider or confirmer is also refused where its chain and the order party's, or the holder's, meet below the operator (`MeetsBelowOperator`, now over logins or parties): in `of_party_either_way`, for the customer and the confirmer's holder, and in the fixed rule, for the decider's holder. The mover's holder check stays one way: the holder's sister moves | `neither the customer nor the holders desk resolves its order` (all three probes, and the sister's move) |
| 2 | Low: the `F-030` hand-off on chains | Sharpened above: the parties reachable without passing through the operator's party; the bypass over-refusal recorded as the known result of such a register | — |
| 3 | Low: the contract world's `world--sd` and `world--sm` put the decider in `operator` and the mover on `operator-desk-b`, a pair refused under a realistic register | Contract item, for the contract author through the coordinator: put the world's decider on a desk, a sibling of `operator-desk-b` directly under `operator` | — |

- **Test first.** The sister probes were written first and seen red against round 2's rules (the customer's sister's
  decision was permitted), then the rule built.
- **Rule-offs of round 3** (core-authz's lib tests; each switched off once, the file restored and touched):

  | Rule switched off | Tests that went red |
  |---|---|
  | a decider or confirmer meeting the customer or the holder below the operator | `neither the customer nor the holders desk resolves its order` |
  | a decider meeting the holder below the operator (the fixed rule) | `neither the customer nor the holders desk resolves its order` |

