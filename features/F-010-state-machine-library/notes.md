# F-010 State machine library: notes

## T-010-02: the contract guard's three findings

- **Reading: "a contract's dependents".** The `M1` finding says contract dependents and the root `Cargo.lock` are not
  tied to bumps, and the task row says a change to them "that moves a contract's dependency" needs that contract's
  bump. Read as the dependency edges of the contracts crate: the guard follows `rutent-contracts` through the root
  `Cargo.lock` (its dependencies, theirs, and the dev-dependency) and fails when any reached package differs between
  the base and the head. A crate that depends on `contracts/` (`components/*`) is a consumer and does not trigger it.
  A change to `contracts/Cargo.toml` itself is a shared-file change, so the first rule covers it.
- **Reading: "that contract's bump".** All five contracts ship as one crate (`rutent-contracts`), and `CONTRACTS`
  does not say which contract uses which dependency, so a moved dependency counts as touching every contract: each
  needs a greater version and a new bullet naming a `DEC-*`. A contract new at the head still needs none.
- **Reading: the plan's decisions.** A decision is defined when `PLAN.md` has a table row starting with ``| `DEC-<n>` ``.
  The guard reads `PLAN.md` from the head working tree and exits 2 if it is missing. Every `DEC-<n>` a new bullet names
  must be defined, not just one. The shared changelog's new bullets are now held to the same rule as each contract's
  (the README already said "any changelog"; the code only checked contract changelogs).
- **Shared files.** A contract's version bump no longer covers a shared file. The old passing test for it is replaced
  by the required failing one.
- **DEC-20 paths touched.** `tools/contract-guard/` and `contracts/` (`README.md` states the rules; `CHANGELOG.md`
  holds the shared entry the new rule requires of this very change). No contract's files changed, so no bump.
- **Limit of "moved".** A move is any change to a reached package's identity: version, source, checksum and its
  dependency list. So another crate enabling a feature of a shared dependency (for example `preserve_order` on
  `serde_json`) changes that package's dependency list and forces every contract's bump and a `DEC-20` click for work
  that is not contract work. If it bites, narrow the identity to name, version and checksum.

## T-010-03: the contract changes `M1` routed to `M2`

- **One bump each.** Read as one version per change, not one per contract: provider-role 2.3.0 (the encoder) and
  2.4.0 (bytes as received for the verifier); module-interface 2.2.0 (`validate_call_against`), 2.3.0 (refusal
  reasons) and 3.0.0 (`delivered` only when answered), each its own `##` section, minors first so one major covers
  the breaking one. The guard reads only the top section, so one commit carries all five. Router-api,
  state-machine and host are unchanged: no file of theirs moved and no dependency of the crate moved.
- **Where the encoder lives.** In provider-role (`provider_role::canonical`), beside the signing rule, so the host
  contract's `exactly-once` wording ("the same canonical JSON under RFC 8785") is now true without a host bump; the
  host's README still names RFC 8785 and not the module. The VOS program and the native client still write
  `serde_json`'s form for their own dedup; moving them onto `to_canonical` is `T-010-05`'s, with the program.
- **Its own parser.** `serde_json::Value` keeps the last of two equal member names, and `serde_json`'s default float
  parsing is not always correctly rounded without `float_roundtrip`. Turning on a feature of `serde_json` from
  `contracts/` would move the root lock and force every contract's bump (T-010-02's note above), so the encoder
  parses I-JSON itself and reads numbers with `core`'s correctly rounded parser. No dependency added.
- **Numbers.** `core`'s `{:e}` gives the shortest digits that read back but does not break an exact tie to even, as
  ECMA-262's Note 2 asks (RFC 8785 Appendix B's `1424953923781206.25`, which `core` writes as `...206.3`). The
  encoder checks the exact expansion for a tie and takes the even neighbour where it reads back.
- **`to_canonical` and non-finite floats.** It goes through `serde_json::to_vec`, which writes NaN and infinities as
  `null`. No contract type holds a float, so this is documented on the function rather than guarded.
- **Refusal reasons.** `AdmissionRefusal` and `PlatformRefusal` were the module-boundary suite's, not the
  contract's. The plan calls them module-interface's, so they move into `contracts/src/module_interface/refusal.rs`
  and the suite re-exports them under the same paths. Added: `DisallowedImport(PartName)`,
  `VersionAlreadyAdmitted`, `LimitReached`, the last named after the outbound call's existing `limit_reached`.
  `UnreadableBinary` and `Unavailable` stay, for what they say. Rust-only types, no schema: nothing records them yet
  (`T-013-*` may). The reference double refuses a re-admitted version, and, given `with_step_limit`, a module past
  it; the suite has no probe for either, which stays `F-024`'s with the suite's next version.
- **`undeclared_kind`.** Accepted only where the callee and the forwarded kind are both declared; an
  `undeclared_callee` refusal of a declared callee stays untrue, since the record's callee is the one judged. The
  record the wasm binding's `tests/calls.rs` writes for that case meets this rule as written; the binding was not
  rebuilt here.
- **`unanswered`.** A new outcome, not a refusal: the platform passed the call on, so the callee may have acted.
  `validate_call_against` treats it as passed on (`UndeclaredCallDelivered`, name kept, docs widened). The reference
  double still records `delivered`, since its callee is a counter that always "answers"; a binding-side test of a
  callee that never answers is `T-013-02`'s. Major, since a 2.x `delivered` for an unanswered call is untrue now and
  the Rust enum gained a variant. Fixtures keep naming interface 2.0.1, as `T-010-01` left them; nothing compares it.
- **Bytes as received.** `RoleProvider` now returns `Received<T>`, the JSON a provider sent and the statement read
  from it, so the suite never re-serialises. `SignatureVerifier::verify` takes `received`; a nested judgement or
  confirmation is handed over as its own text inside the result's (`provider_role::member`), so a verifier can
  canonicalise it as received. The suite also flags `EnvelopeWellFormed` when a statement's text has no signing
  input. The stand-ins implement a private typed trait and send through `Received::sent`.
- **Not run here.** The `EXP-2` bindings and the VOS adapter depend on `conformance/` and `contracts/` by path,
  outside the workspace, and were not rebuilt: the bindings implement `ModuleBinding`, whose refusal types keep their
  paths but gain variants, and implement no `RoleProvider`. Their workflows run only on their own branches.
- **DEC-20 path touched:** `contracts/`.
