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
  keeps its `AC-17` tag.
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
  with why. The guard is the log: `T-020-01`'s follow-up splits its append surface so that only policy's handle takes
  a decision's entry, and the engine will append through it once that lands. What the scan can't see (a value read
  through serde, where an effect's receipt came from) is that handle's, `T-020-08`'s and `T-025-05`'s.
