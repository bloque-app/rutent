# F-015 notes

Readings taken where `SPEC.md` or `PLAN.md` is silent or ambiguous, recorded per Narthex `REQ-SKL-24`.

## T-015-01

- The scale is not redefined. `EvidenceGrade`, `EvidencePiece` and `Evidence` (the per-provider declaration) already
  sit in the frozen provider-role contract; `components/evidence` imports them and adds the rules around them.
- "Claim above the declared grade" reads as a claim stronger than declared (a lower number): the declaration is what
  a floor can rely on, so a piece claiming more is refused for the whole outcome. A piece weaker than declared is not
  refused here; the outcome simply grades lower, and whether that clears the operator's floor is the router's call.
- "A statement alone is graded as a statement": a grade 1 or 2 claim counts only when a check backs it, a final
  matching public record (`verify_movement`) for 1, and that or a receipt from a different publisher for 2. An
  unbacked claim is graded 4, never as proof. Grades 3 and 4 need no check. Proof means grade 1 or 2. The outcome's
  grade is its best piece's.
- A receipt from the same publisher as the provider earns no check (`THR-124`): `receipt_check` returns `Unchecked`.
  It is downgraded to a statement rather than refusing the outcome, because the provider's own word still stands.
  Since the PR #60 review, `receipt_check` takes a `VerifiedReceipt`, not two publisher strings (see the review
  section below).
- The network interface carries finality and the reorg rule as declared data (`NetworkRules`), and the verdict on a
  movement as `Status` (unknown, pending, final, reorged out). The checker trusts the network to say when a movement
  is final; interpreting depth or checkpoints is each network's task in `F-031`. `T-015-02` builds the simulated
  network behind `PublicNetwork`.
- Movement-to-segment matching is by network, destination, asset and amount together, with the destination being the
  route's one destination. Amount alone never matches (`DEC-13`). Amounts are compared as the decimal strings the
  contracts use, in the segment's unit.
- One destination per route (`one_destination`): exactly one segment ends at an outer node, and it is the last, a
  deposit. Anything else is reported as a named violation, not panicked on and not guessed around.
- `proptest` is a dev-dependency (`DEC-18`); its lock additions have `licences/RECORD` lines, all permissive.

## T-015-02

- The simulated network is `SimulatedNetwork`, in its own module (`simnet`). It is a hand-driven chain: `record`
  puts a movement in the tip block, `advance` adds blocks, `checkpoint` covers every block to the tip, `reorg(depth)`
  undoes the newest `depth` blocks' movements. It imports nothing but the interface (`DEC-2`).
- Confirmations count the including block as the first, so `Confirmations(n)` is final when the tip is `n - 1`
  blocks past the including block. `Confirmations(0)` is read as `Confirmations(1)`. `Instant` is final on record.
  `Checkpoint` is final once a checkpoint taken at or after the including block's height exists.
- A reorg undoes a movement only when it is not final and the reorg rule allows it: `Never` protects every movement,
  `DepthBound(d)` protects one with more than `d` confirmations, `UntilFinal` protects only final ones. A final
  movement is never undone, whatever depth a reorg asks for. Reorgs do not lower the tip height.
- Recording a movement id the network already holds (not reorged out) is refused, as `T-015-01` reads a movement id
  as unreusable. A reorged-out id may be recorded again, in a new block, because a reorg puts the transaction back in
  the pool.
- A checkpoint covers the movements recorded before it, by order of recording, not by block: a movement recorded
  after a checkpoint, even into the same tip block, is not final until the next checkpoint. A reorged-out id may come
  back only with the movement it first named, since a transaction id fixes its content (`ContentChanged` otherwise).

## T-015-03

- The contracts define no record of a private node's inner segments, so `components/evidence` defines one
  (`replay::RecordedSegment`): asset and amount in, asset and amount out, and the fee in the asset that went in.
  Amounts are canonical decimal integer strings; anything else (`1e3`, a leading zero, a sign) is recorded as
  malformed rather than guessed at.
- Replay checks what can be checked without a rate: each segment takes in what the one before gave out, and where
  the asset stays the same, amount in equals amount out plus the fee. In every segment the fee may not exceed the
  amount in. Where the asset changes, the exchange rate itself stays unchecked: it is the node's own.
- A route across several private nodes is replayed node by node in route order; what one node hands on must be what
  the next records taking in. Only the last node's output is compared with the public movement out and the stated
  outcome. A fault inside a node does not stop the later comparisons: they run on what the record says,
  and every mismatch is recorded (`REQ-ORD-41`: record any mismatch). An empty record is a `NoSegments` mismatch from
  `replay_node`; `replay_route` still reports it as `Unreplayable`.
- The comparison with the public movement reuses `verify_movement` with the replayed value as the expectation, so
  the network alone decides finality (`T-015-01` reading). An unknown, pending or reorged-out exit is recorded as a
  `PublicExit` mismatch: there is no final public movement out to be consistent with.
- A public exit with no node recorded, or a node with no segments, is `Unreplayable`, neither a match nor a
  mismatch: `REQ-ORD-41` makes recording a SHOULD, so its absence is reported, not punished.
- A match is `Replay::Consistent` and produces no `Check`, so it cannot lift a grade; `proves_segments_happened`
  is always false (`THR-131`: a provider can invent segments to fit its exit). A route ending inside the node is
  `Replay::PublishersWord`, whatever was recorded.
- The module is `pub mod replay` and its items are not re-exported at the crate root, to keep `lib.rs` clear of
  `T-015-02`'s edits. One movement evidences one exit; reuse across routes is `distinct_movements`' job at the
  caller.

## PR #60 review (finding 2)

- Only two paths build a backing check, and a check is only as good as the network reader, the receipt verifier
  and the publisher identities its caller supplies. A network reader that calls every movement final, or a
  verifier that accepts every receipt, earns grade 1 or 2 through those paths. The checker owns the rules, not the
  trust anchors; the `Check` docs say so first.
- A receipt check rests on a verified receipt. `receipt_check` takes a `VerifiedReceipt`, whose fields are private:
  only `verify_receipt` builds one, over a `ReceiptVerifier` the caller passes. `verify_receipt` refuses a receipt
  that acknowledges no reference (empty or only whitespace), one the verifier doesn't find signed by its maker for
  that reference, and one whose maker has no publisher in the verifier's catalogue. The receipt keeps its maker.
- The publisher-difference rule (`THR-124`) stays in `receipt_check`, but both publishers are read earlier, in one
  call: `verify_receipt` takes the provider whose outcome the receipt is meant to back, and the verifier's
  `publishers(maker, provider)` answers for both from one catalogue snapshot. Two lookups could straddle a catalogue
  change and let `THR-124` pass by mistake. A maker or a provider the verifier doesn't know refuses the receipt
  (`UnknownMaker`, `UnknownProvider`). `receipt_check` then needs no verifier.
- A receipt check records the provider it was judged for, and `grade_outcome` takes the outcome's provider: a
  receipt check backs a piece only in that provider's outcome. A check judged for one provider can't be carried to
  another's.
- The crate doesn't verify signatures itself. That's a reversible default, for two reasons. The crate depends on
  nothing of ours but the contracts. That is F-001's reading, stricter than `DEC-2`, which restricts only the
  three outer libraries; `CMP-EVIDENCE` isn't one of them (`ARCHITECTURE.md`), and the boundary check enforces
  the stricter reading. So the crate can't reuse the provider library's strict Ed25519 verifier, and a second copy
  of that verifier would be a second `DEC-20` signature-checking path. And while the provider-role envelope
  (`contracts/src/provider_role/envelope.rs`) defines the signed statement, a receipt's body has no shape in the
  contracts, so the crate would have to invent one. The verifier is the trust anchor, on the same footing as the
  network reader behind grade 1. Core implements it over the catalogue it accepted and the provider library's
  verifier.
- A check is bound to what it checked: a public record to its movement, a receipt to the reference it acknowledges
  (`Check::bound_to`) and to its maker (`Check::receipt_maker`). A check backs a piece only when that is the piece's
  `reference`, and a receipt only when its maker is the piece's `by`, the party whose evidence it is
  (`REQ-ORD-33`). Otherwise the piece is graded a statement, like any unbacked claim. The comparisons are exact,
  with no normalisation, so Core (`F-020`) must fill `reference` and `by` as the check names them. The rule is
  applied in grading, not in a constructor, because `Claim`'s fields are public.
- One receipt reference backing two pieces refuses the outcome (`ReceiptReused`), as one movement backing two does
  (`MovementReused`).
- A receipt backs grade 2 at most: a grade 1 claim on a receipt check is graded a statement, as before. A test and a
  property now pin it.
- The replay tests now pin four more rules, each with a fixture where only that rule can fire: an asset change
  between segments in one node, an asset change at the handoff between two nodes, a stated asset other than the
  replayed one, and the canonical spelling of amounts (`"0990"` and `"+5"`, which Rust's integer parse accepts).

### Hand-offs to `M3`

- Take a receipt as a provider-role `Statement`, so the crate reads `maker` from the signed bytes and computes
  `signing_input` itself. The injected trait then shrinks to a signature under a key, and the catalogue lookup.
  This needs a receipt body shape in the provider-role contract.

## T-015-04: the reader host

`DEC-28`, ADR-0007 as amended (the owner's finding 7 of 2026-11-25 and his form answers `reader-registration-path` and
`reader-answer-trust` of 2026-11-26). Written first as `T-013-12` (issue #324); built from `T-020-59`'s commit
(`m3` at `a45e302b`). A new crate, `rutent-reader-host`, in `components/reader-host/`, MIT as the workspace and `@core`
in `licences/RECORD`. It edits `components/reader-host/` (new), the root `Cargo.toml`'s members, `Cargo.lock` (the
member's own entry: every dependency it names was locked already, so no `licences/RECORD` package line is new, only its
`@core` and `@artifact` lines), `.github/CODEOWNERS`, `HOUSE-RULES.md` and `.github/workflows/jail-probes.yml`, all
`DEC-20` paths. Nothing of ours depends on it: `T-024-25` adds the scenario assembly's dev-dependency. Since the lead's
follow-up on #348 (and the row's part (7), plan `87aed6c0`), it also edits `tools/boundary-check/` and two new public
paths of `rutent-prov`, `src/confine.rs` and `src/keys.rs`, with their `pub mod` lines in `src/lib.rs` (`DEC-20` paths
too, added to `.github/CODEOWNERS` and `HOUSE-RULES.md`).

### Design

- **One process, several readers** (`Host`, `src/host.rs`). `Host::start(HostConfig, Services)` starts one worker
  thread per reader, each owning its reader's child process (`src/worker.rs`). `Host::answer(question bytes)` gives the
  reader's signed answer as the statement's bytes, or nothing where there is no reader to sign for. The binary
  (`src/main.rs`) reads its configuration (`HostConfig::read`, JSON), resolves keys from its environment, writes each
  call's record to standard error as a JSON line, and serves the link. `cargo doc` needs no step of its own: the
  workspace doc step covers the crate.
- **(1) mTLS** (`src/link.rs`). `rustls` with `ring` only: `cargo tree -p rutent-reader-host -e features -i aws-lc-rs`
  finds no package (`aws-lc-rs` is in no lock). The server's client verifier is the web PKI's over the operator's CA,
  the one anchor, wrapped so that Core's certificate must also be valid for the subject name configured for Core
  (`--core-name`, `rustls::client::verify_server_name` on the parsed leaf). A client certificate is mandatory. The host
  presents the chain and key it reads at start; Core pins the operator's CA and the SAN the listing names, never a leaf
  (`T-020-60`'s side; a test here shows a rotated leaf under the same CA and name taken by an unchanged client). No
  session resumption, no tickets, no early data. One frame per question (at most 64 KiB) and one per answer; at most 64
  connections, idle ones closed after 60 s.
- **(2) One process per reader.** A worker's phases: starting (the reader runs its checks; every question unreadable,
  `checks_not_passed`), ready, asking (one question with the reader, the rest waiting up to `pending`), down (crashed,
  hung, a limit hit, checks failed or timed out; every question unreadable; started again after `restart_after_ms`).
  A crash is the end of the reader's output; a hang is `hang_after_ms` with no answer; a limit hit is a frame over
  `max_frame` or one that doesn't read as the wire's. The host kills the process (and, on Linux, `unshare`'s
  `--kill-child` takes the reader with it), answers the question unreadable and every waiting one
  `reader_restarting`. Memory is bounded by the jail's address space (`PROVIDER_ADDRESS_SPACE`, 4 GiB, set by
  `confine`) and the operator's task; the first build's own `memory_bytes` went, since `confine` raises the limit to
  the provider's and a lower hard limit set before it would refuse the raise.
- **(3) Signing outside the reader** (`src/keys.rs`). The worker hands the host the reader's answer body; the host
  holds it to the question (`worker::holds`: the request, the network, the question as asked, the configured version,
  `ReaderAnswer::check`, and for `network_rules` the configured identity and trust rule) and builds the envelope: Core's
  deployment and protocol version from the configuration, the reader as maker, its listed key. It resolves the private
  half at that moment as the platform resolves a provider's (`keys::resolve_key` over `T-014-01`'s `Resolver`, through
  `rutent_prov::keys`): under the reader's name, by the listed public key's text, given up at `egress_give_up_after_ms`,
  and never the deployment's operator key. The binary builds the resolver as the platform builds a role's keys bucket:
  `kv_bucket` over `HttpsTransport::for_role` from a process role's settings (`--key-store`, OpenBao or Vault), with the
  token the deployment delivers in `RUTENT_READER_KEY_STORE_TOKEN`, which it takes out of its environment before any
  thread starts. It refuses a key whose public half isn't the listed key, signs the
  provider-role signing input, and drops it. An answer that doesn't hold is signed as unreadable, with why. The key is
  never in the reader's frames, arguments or environment.
- **(4) Egress, enforced by the host** (`src/egress.rs`, `src/jail.rs`). The reader runs in the jail `CMP-PROV` runs a
  provider's process in, through `rutent_prov::confine`: the host starts it with `isolated` under `Limits::PROVIDER`
  (on Linux `unshare` in new user, network, IPC, UTS, mount and PID namespaces with `--fork --kill-child`; the
  environment cleared, every descriptor above 2 closed, core dumps off and open files bounded; on macOS the provider's
  deny-by-default `sandbox-exec` profile), and the reader calls `jail::confine_reader` (`confine(Role::Provider)`)
  before it reads anything: `no_new_privs`, the empty read-only root, every capability dropped, Landlock with no rule,
  the provider's `seccomp` allow-list and its address space. On Linux the host holds it to that before it takes the
  reader's start-up checks (`jail::check_confined`): it reads the reader's `/proc` status (the one child of the
  `unshare` it started) and refuses it as `reader_not_confined` unless `NoNewPrivs` is 1, `Seccomp` is 2 and no
  capability is held, the bounding set included. The working directory is an empty one. Its one channel is its standard input and output (`src/wire.rs`: frames of JSON). A pool
  request goes through `rutent_prov::egress::Egress::send` under a running declaration the host builds from the trust
  rule (one destination per endpoint, by its name in the rule, public addresses, one operation `read`), so the layer's
  own checks hold: declared destinations only, each redirect checked, the name resolved once and every address held to
  the public ranges, TLS verified and never downgraded, and its `OutboundCallRecord` (provider = reader, module version
  = reader's version, transaction = request id). A light client's connection is opened only for a declared protocol and
  port, under its cap, to a resolved address outside every special-purpose range, through the egress layer's
  `NameResolver` and `Dialer`; it is recorded as a `ConnectionRecord` with the same four facts. Calls made for the
  reader's start-up checks serve no request, and are recorded under `start-up`, never a drawn id.
- **(5) The trust rules.** `rpc_pool`: the host sends the reader's request to each endpoint it names (each host once)
  and hands back a body only where at least the quorum agree and no other body reaches it; otherwise `too_few_rpc_answers`
  or `rpc_answers_disagree`. During a question, any pool call not agreed, or any undeclared endpoint named, makes the
  host sign the answer as unreadable (`trust_rule_not_met`) whatever the reader says, unless it said unreadable itself.
  `light_client`: the reader is refusing until it reports `checked` with the identity it found and the host holds it
  to the configuration's; a reader that reports another network is down (`checks_failed`) and checked again at its next
  start. The same holds for an RPC reader, whose check is the chain id through the pool.
- **(6) Restarts.** A restarted host reads its configuration again and each reader starts in the starting phase: every
  question unreadable until that reader's checks pass.
- **Clocks.** The question's deadline is Core's: the host hands the question to the reader as Core sent it and reads
  the deadline against no clock of its own. Its own bounds are `ReaderLimits`' `hang_after_ms`, `start_within_ms`,
  `restart_after_ms` and `egress_give_up_after_ms`, on the operating system's monotonic clock; the last is what the
  host passes as `Call::due` to the egress layer. The host never draws a request id.
- **Stand-ins** (test code only): `examples/standin_reader.rs`, an RPC reader and a light-client reader, each with
  hostile acts for the tests (`ignore-quorum`, `undeclared`, `other-request`, `other-rules`, `skip-checks`, `crash`,
  `hang`, `inspect`, `probe-network=`). The tests find the built example beside their own executable, or at
  `RUTENT_STANDIN_READER`.

### Tests

The ten named tests, each with the row's tags:

- `reader_host_takes_a_connection_only_from_a_client_certificate_the_operators_ca_issued` [`DEC-28`] (`tests/link.rs`)
- `readers_call_to_an_endpoint_it_did_not_declare_is_refused_by_the_reader_host_and_recorded` [`REQ-MDL-7`,
  `REQ-MDL-8`, `THR-79`] (`tests/egress.rs`)
- `reader_redirected_to_an_undeclared_endpoint_is_refused_by_the_reader_host` [`REQ-MDL-7`] (`tests/egress.rs`)
- `light_client_reader_reaches_only_its_declared_protocols_and_ports_and_no_more_connections_than_its_cap`
  [`REQ-MDL-7`, `REQ-MDL-11`] (`tests/egress.rs`)
- `reader_process_has_no_network_route_but_the_reader_host` [`REQ-MDL-7`] (`tests/jail.rs`): Linux only, ignored off
  Linux with a body that panics if run by hand. It runs in `jail-probes.yml`'s job in a step of its own, after the
  stand-ins are built, and the step greps for the test's `ok` line, so a skip, an ignore or a filter fails the job. It
  also runs on Linux in `ci.yml`'s workspace test step. Not run locally (macOS). Beside the confined reader, which
  reaches nothing but the host, it starts one that skips `confine` and shows the host refuse it as
  `reader_not_confined`.
- `reader_that_crashes_or_hangs_is_unreadable_and_the_other_readers_answer_on` [`REQ-MDL-11`] (`tests/readers.rs`)
- `reader_never_holds_its_statement_key` [`REQ-MDL-6`] (`tests/keys.rs`, alone in its binary since it sets the
  environment the binary reads its store's token from): the key resolved per answer through `KvResolver` over a
  stand-in KV store on loopback, by the reader's name and the listed key's text, with the token, and neither key nor
  token in anything the reader saw
- `evm_reader_with_fewer_than_two_agreeing_rpc_answers_is_unreadable` [`REQ-ORD-39`, `DEC-28`] (`tests/readers.rs`)
- `substrate_reader_checks_the_networks_genesis_before_answering` [`REQ-ORD-39`, `DEC-28`] (`tests/readers.rs`)
- `restarted_reader_host_answers_nothing_until_each_readers_checks_pass_again` [`NFR-6`] (`tests/readers.rs`)

Beside them: `reader_host_reaches_the_providers_jail_and_key_resolver_through_their_admitted_paths_only` [`DEC-28`,
`DEC-2`, `REQ-MDL-6`, `REQ-MDL-7`] (`tools/boundary-check`), and
`core_holds_the_host_to_the_operators_ca_and_the_listed_name_and_takes_a_rotated_leaf` [`DEC-28`],
`host_signs_only_an_answer_that_names_the_question_it_was_asked` [`REQ-MDL-6`, `DEC-28`],
`reader_with_its_questions_bound_reached_answers_the_next_unreadable_at_once` [`REQ-MDL-11`], and unit tests of the
seed's one spelling, the listed key, the special-purpose ranges and the host's time. The issue listed nine tests; the
row names ten (the light client's cap), and the row is followed.

The code came first, so the rule-off runs below stand in for seeing each test red first.

### Defaults, reversible, each failing closed

1. Nothing to sign for (a question that doesn't read as one, a reader this host doesn't run, a key the store can't
   give or that isn't the listed one) is an empty frame on the link, never an unsigned answer; Core reads it as
   unreadable.
2. A question about another network than the reader's is answered unreadable (`question_about_another_network`).
3. A reader's answer that doesn't hold to the question is signed as unreadable with why, rather than dropped.
4. An endpoint that redirects away, resolves where it mustn't, fails TLS or doesn't answer counts as that endpoint not
   answering: the others may still reach the quorum (as a lying endpoint of three is outvoted). Only the reader naming
   an undeclared endpoint itself makes its answer unreadable whatever the pool agreed.
5. Agreement is equality of the parsed JSON bodies (of the text where a body isn't JSON). Two bodies each reaching the
   quorum is disagreement.
6. A reader whose checks find another network is down and checked again at each restart (`restart_after_ms`); it never
   answers until they match.
7. Bounds by default: 10 s per question, 60 s for the start-up checks, 16 questions waiting,
   1 s before a restart, 5 s per egress call, 1 MiB per frame. A configuration sets each per reader; zero is refused.
8. The host reads its chain and key at start: rotating the leaf is a restart, with no listing change.
9. The production key store is the platform's: a process role's vault bucket (`--key-store`), its token delivered in
   `RUTENT_READER_KEY_STORE_TOKEN` and taken out of the host's environment at start. A rotated key is used from the
   next answer. The readers' environment is cleared.
10. The binary makes the host undumpable on Linux (`PR_SET_DUMPABLE` 0), so a process of its user can't read its
    memory or environment.
11. A reader that isn't confined when it reports its checks is down (`reader_not_confined`) and started again after
    `restart_after_ms`; it never answers.
12. A light client's relay hands back what a connection gives within 2 s of a send, at most 64 KiB.

### Departures from the row, and gaps

The first build (#348's first commit) left three departures that the lead's follow-up closed in this row, since
pre-production gaps are fixed in the milestone that finds them: the jail was the namespaces alone, composed by the
host; the key store was a seam of the host's own (`EnvKeys`); and the egress layer's clock couldn't be handed over.
`tools/boundary-check` now admits `rutent-reader-host` to three paths of `rutent-prov` and nothing more
(`PROV_PATHS`): `egress`, `confine` (`PROV_CONFINE`: `isolated`, `confine`, `Limits`, `Role`, `PROVIDER_ADDRESS_SPACE`,
re-exported from `module::sandbox`) and `keys` (`PROV_KEYS`: `T-014-01`'s `Resolver` and `ResolveError`, `KvResolver`,
`KvConfig`, `kv_bucket`, `Bucket`, `RoleSettings`, `Server`, `StoreAddress`, `StoreScheme`, `Token`, `HttpsTransport`,
`TransportError`, `InvalidConfig`, `InvalidSettings`, re-exported from `proxy`). Each file holds that one re-export
and nothing else, which the check reads (`Why::WidensAnAdmittedPath`); a longer name, a group, a glob, or any other
path into `module` or `proxy` is still refused (`Why::ReachesBeyondItsPaths`, renamed from `ReachesBeyondEgress`).
`rutent-prov`'s `src/lib.rs` says it belongs to `T-012-01`; its two `pub mod` lines are this row's, at the lead's
request.

1. **The egress layer's clock.** The lead's first brief asked for the host's clock to be handed to the egress layer
   through `with_clock`; the follow-up kept `Monotonic`. `OperationClock` stays out of the admitted paths, so the
   resolver and the connector keep their default, the host's own monotonic clock. The names are confusing on prov's
   side: `Call::due` and `Dial::due` are documented as "when the operation the call serves must end, if it has a
   deadline", which for the reader host is its own give-up time (`egress_give_up_after_ms`), never Core's answer
   deadline. Prov's API is not renamed here.
2. **A light client's connections** (gap, owned by `T-031-02`). The egress layer has no session interface (a
   `Connector` is one request and one answer, and refuses `wss`), so the host opens them with the layer's resolver
   and dialer and its own address check, and records them in its own `ConnectionRecord`: the module-interface `Hop`
   has no scheme for a p2p transport. The relay is a send and what comes back within a wait, not a stream;
   `T-031-02` gives `smoldot-light`'s transport the streaming relay it needs.
3. **`wss` in an RPC pool** (gap, owned by `T-031-03`). The contract admits `wss` endpoints; the production
   connector refuses them, so such an endpoint never answers and never counts toward the quorum. `T-031-03` gives a
   pool's `wss` endpoints a session to answer over, or lists `https` endpoints only.
4. **The seccomp allow-list is a provider's.** A reader confined as `Role::Provider` makes only the calls a provider's
   process makes: no timed sleep (`nanosleep`), no thread (`RLIMIT_NPROC` 0), no socket. The stand-ins wait by
   yielding. A real reader that needs more is `T-031-02`'s and `T-031-03`'s to raise, as a role of its own in
   `module::sandbox` and `PROV_CONFINE`, never by leaving the jail.
5. **The metadata check.** The host holds a light client's reported genesis to its configuration; the metadata it
   relies on and answering only on finalised state are the reader's own to hold (its code is the operator's), as are
   an RPC reader's confirmations. The host can't check either.
6. **The jail test** runs only on Linux, in CI's `jail-probes` job (and `ci.yml`'s workspace test step); it was not
   run locally, and neither was the host's confinement check, which only Linux has.

### Rule-off

Each rule switched off alone in a copy of the source, the tests that hold it run, the source put back
(by a scratch script of the author's, not kept). Every one went red on an assertion; the jail's own rule is Linux's and was not run
locally.

| Rule switched off | Test | Red |
|---|---|---|
| Core's certificate must carry Core's configured name | `reader_host_takes_a_connection_only_from_a_client_certificate_the_operators_ca_issued` | the operator's CA's certificate for another name was answered |
| A client certificate is mandatory | the same | a connection with no certificate was answered |
| An answer resting on an unagreed pool call or an undeclared endpoint is unreadable | `readers_call_to_an_endpoint_it_did_not_declare_is_refused_by_the_reader_host_and_recorded`; `evm_reader_with_fewer_than_two_agreeing_rpc_answers_is_unreadable` | `left: None`, the straying and the careless readers' answers signed as answered |
| The quorum | `evm_reader_with_fewer_than_two_agreeing_rpc_answers_is_unreadable` | `left: None`, one answer of three taken |
| A light client's protocol and port | `light_client_reader_reaches_only_its_declared_protocols_and_ports_and_no_more_connections_than_its_cap` | the connection records differ: port 8080 and `wss:30333` opened |
| A light client's cap | the same | a third connection opened |
| A peer's address is a public one | the same | the loopback peer dialled |
| The checks' identity is the configuration's | `substrate_reader_checks_the_networks_genesis_before_answering` | the reader of another genesis became ready |
| A starting reader is asked nothing | `restarted_reader_host_answers_nothing_until_each_readers_checks_pass_again` | `left: Some("reader_crashed")`: the question reached the reader mid-checks |
| The answer names the request asked | `host_signs_only_an_answer_that_names_the_question_it_was_asked` | Core's check refused the answer signed for another request |
| The network's rules are the configuration's | the same | `left: None`, a stricter quorum signed as the reader's rules |
| The reader's environment is cleared | `reader_never_holds_its_statement_key` | the host's key variable reached the reader |
| A hung reader is stopped at its bound | `reader_that_crashes_or_hangs_is_unreadable_and_the_other_readers_answer_on` | never answered; killed at 240 s |
| The questions waiting are bounded | `reader_with_its_questions_bound_reached_answers_the_next_unreadable_at_once` | `left: Some("reader_restarting")`: the third question waited |
| Only the listed key signs | `keys::tests::answer_is_signed_only_with_the_readers_listed_key` | `left: Ok(..)`, another key signed |

After the follow-up, the same way:

| Rule switched off | Test | Red |
|---|---|---|
| The reader starts in prov's jail (`isolated`, not a plain command) | `reader_never_holds_its_statement_key` | the host's store token reached the reader's environment |
| Only the admitted prov paths pass | `reader_host_reaches_the_providers_jail_and_key_resolver_through_their_admitted_paths_only` | `left: []`, paths into `module` and `proxy` passed |
| An admitted path holds its one re-export | the same | `left: []`, a widened `keys.rs` passed |
| No more paths than `egress`, `confine` and `keys` (`module` added) | the same, and `reader_host_reaches_only_the_contracts_and_the_providers_egress_and_nothing_reaches_it` | `rutent_prov::module::..` passed |

The jail's in-process layers (empty root, capabilities, Landlock, seccomp) and the host's check of them exist only on
Linux, so they can't be switched off and run here. Each layer alone is `T-025-02`'s probes in the same job, and the
jail test's unconfined reader is the host's check seen red on its own.

### The review of #348 at `4c3448cf` (review 6088670252)

CHANGES, on one High and one Medium, with six Lows; each fixed in this row (pre-production).

- **H1. A reader that stops reading froze its worker.** Writes to a reader now go through a writer thread of its
  own over a bounded queue (`WRITE_FRAMES`, 32 frames, and `WRITE_BYTES`, 4 MiB); the worker only queues, and a
  reader whose frames wait past either bound has stopped reading: it is killed (`reader_stopped_reading`), its
  questions answered unreadable, and it is started again. The phase's bound (a question's `hang_after_ms`, the start-up
  checks' `start_within_ms`) is read on every turn of the worker, so a reader that keeps talking is stopped at its bound
  as a silent one is. Each egress call is given up at the phase's bound too (`Serving::until`), so one call can't
  carry a question past it. The frames from a reader's output to its worker are counted in and out (`Gauge`): at most
  `READ_FRAMES` (64) and four `max_frame`s' worth of bytes waiting, and a reader over either is stopped
  (`reader_hit_a_limit`). The first reason found by the output or the input thread wins and is read ahead of the
  frames still waiting; frames of a process already stopped are dropped with it. A reader may ask for at most
  `calls_per_question` (64) egress calls in one question, or in its start-up checks, which bounds the amplification of
  a flood. An asker waits for its reply at most `(hang_after_ms + egress_give_up_after_ms) × (pending + 1)`
  (`host::ask_bound`), so a link thread is held by one reader's share alone. Test:
  `reader_that_keeps_talking_or_stops_reading_is_stopped_at_its_bound_and_the_others_answer_on` [`REQ-MDL-11`] (a
  reader that asks the pool every 5 ms for a large answer and never reads, stopped as `reader_stopped_reading`, and
  one that asks without end, reading each reply, and never answers, stopped as `reader_hung`; each within 2.5 s of a
  1.5 s bound and started again, while a third answers in under a second), and
  `worker::tests::reader_whose_frames_wait_past_their_bound_is_stopped` [`REQ-MDL-11`].
- **M1. The admitted paths could be redirected.** Once the reader host is a member, `tools/boundary-check` now holds
  prov's crate root to declaring each admitted path plainly (`declared_plainly`: exactly one `pub mod confine;` and
  one `pub mod keys;`, no attribute before them, so no `#[path]` or `#[cfg_attr(.., path = ..)]`, and no inline body),
  requires `src/confine.rs` and `src/keys.rs` to exist and hold their one re-export (a missing file is a finding, no
  longer skipped), and refuses a `src/confine/` or `src/keys/` directory. `components/prov/src/lib.rs` and
  `components/prov/src/module/sandbox.rs` join `.github/CODEOWNERS` and `HOUSE-RULES.md`. The test of the admitted
  paths now covers the redirect on its own line, on the same line and through `cfg_attr`, an inline body, a missing
  declaration, a second declaration, a missing file and a module directory, each with the real file left as a decoy.
- **L1.** The host's check reads the status of every thread of the reader (`/proc/<pid>/task/*/status`), refuses a
  reader with a child process, and refuses one whose network namespace it can read as its own. It runs on the first
  frame of any kind but a log line, and nothing (no call, no check, no answer) is served before it passes. Prov's
  `seccomp` keeps no `TSYNC` flag: with RLIMIT_NPROC 0 and no `clone` in the allow-list, a reader makes no thread
  after `confine`, and one made before it is read and refused.
- **L2.** For an RPC pool, the host asks the pool for the chain id itself (`eth_chainId`, through the quorum) before
  it takes the reader's checks, and refuses a reader whose pool names another chain, whatever the reader reported.
  Test: `rpc_readers_chain_id_is_held_to_the_pools_own_answer_not_the_readers_report` [`REQ-ORD-39`, `DEC-28`]. A light
  client's genesis can't be checked so without speaking its peer protocol: gap, owned by `T-031-02`, whose reader
  pins its chain spec by digest.
- **L3.** An RPC pool's movement answer (`movement_status`, `movement_of`) is signed as answered only if at least one
  pool call agreed in that question; otherwise `trust_rule_not_met`. Case added to
  `evm_reader_with_fewer_than_two_agreeing_rpc_answers_is_unreadable` (a reader that answers without asking).
- **L4.** The link's handshake has one deadline for the whole of it (`HANDSHAKE`, 10 s), and at most
  `MAX_HANDSHAKING` (16) connections may be in their handshake at once, so peers that never authenticate can't hold
  the slots Core needs (`LinkLimits`, `Link::serve_with`). Test:
  `link_closes_a_handshake_past_its_deadline_and_keeps_room_for_core` [`DEC-28`].
- **L5.** Each reader starts with `oom_score_adj` 1000 (set between fork and exec, inherited), so the kernel kills a
  reader, which the host restarts, before the host. The task's memory, the readers' together, is the operator's to
  size: gap, owned by `F-030`.
- **L6.** The jail test's unconfined reader runs the same network probe with only the namespaces around it, before
  the host refuses it, and must be refused with `NetworkUnreachable`, never seccomp's `PermissionDenied`: the network
  namespace shown to hold on its own.

Rule-offs, each alone and reverted:

| Rule switched off | Test | Red |
|---|---|---|
| Writes go through the bounded writer (a blocking write in the worker instead) | `reader_that_keeps_talking_or_stops_reading_is_stopped_at_its_bound_and_the_others_answer_on` | never answered: the worker blocked on the flooder's full input; killed at 240 s |
| The frames waiting for the worker are bounded | `worker::tests::reader_whose_frames_wait_past_their_bound_is_stopped` | `left: 75`: every frame forwarded |
| The crate root declares an admitted path plainly | `reader_host_reaches_the_providers_jail_and_key_resolver_through_their_admitted_paths_only` | `left: []`, the `#[path]` redirect passed |
| A missing admitted file fails closed | the same | `left: []`, the missing `keys.rs` passed |
| No module directory beside an admitted file | the same | `left: []`, `src/confine/` passed |
| An RPC pool's movement answer rests on an agreed call | `evm_reader_with_fewer_than_two_agreeing_rpc_answers_is_unreadable` | `left: None`: the reader that asked nothing was signed as answered |

The phase's bound read on every turn, switched off alone, left that round's test green. Round 2 showed it is
load-bearing (a light client that keeps one send queued ahead of each reply always leaves its worker an event), and
the test now has that reader: see round 2 below.

### The review of #348 at `e0f07090` (review 6089810793)

CHANGES, on H2, M2 and M1′, with L7 and L8; each fixed in this row.

- **H2. CI's jail test was red.** In a network namespace with only a down `lo`, an IPv6 connect fails with
  `AddrNotAvailable` (no source address), not `NetworkUnreachable`. The unconfined reader's probe now passes on any
  no-route error (`NetworkUnreachable`, `AddrNotAvailable`, `HostUnreachable`) and still fails on seccomp's
  `PermissionDenied` or a connection. Linux clippy, fmt and the doc check ran on no earlier head (CI skipped them after
  the red test step), so the Linux-only code (`jail::linux`, `tests/jail.rs`) was read by eye for them; CI's next run is
  their first.
- **M2. The per-turn bound is load-bearing.** A light-client `Send` now counts against `calls_per_question`, and its
  wait for what comes back ends with the phase it serves (`Serving::until`). The named test
  `reader_that_keeps_talking_or_stops_reading_is_stopped_at_its_bound_and_the_others_answer_on` gains two readers, each
  with the call bound set out of the way: `stream`, a light client that keeps sixteen sends queued ahead of each reply
  to a peer that never answers (`wait`), and `pipeline`, an RPC reader that keeps two slow pool requests queued ahead
  of each reply. Each is stopped as `reader_hung` within 2.5 s of its 1.5 s bound, and the test goes red with the
  per-turn bound switched off (below).
- **M1′. The admitted paths pinned text, not what it resolves to.** The check now resolves each admitted path's
  re-exports by file layout and holds each name to its definition:
  - **(a)** no `#[path]` (or `cfg_attr` path, or `include!`) anywhere under prov's `src/`, and no `[lib]` table in its
    manifest, so module resolution follows file layout alone;
  - every module on the way (`module`, `module::sandbox`, `proxy`, `proxy::{https, kv, resolver, settings,
    transport}`) is declared plainly in its parent (`pub mod` or `pub(crate) mod`, no attribute, no body, once), its
    file exists (`<name>/mod.rs` for a parent with no `<name>.rs` beside it, `<name>.rs` for a leaf with no `<name>/`
    beside it);
  - **(b)** `keys.rs` now re-exports each item from the file that defines it (`crate::proxy::kv::KvResolver` and so
    on; the five proxy modules it names became `pub(crate) mod`), and each name `confine.rs` and `keys.rs` re-export
    must be defined in its source file's own items, once (`defined_in`: a `pub fn`, `struct`, `enum`, `union`,
    `const`, `static`, `trait` or `type`), with no top-level `pub use` naming it or globbing, and no `pub mod` or
    `pub macro` of that name beside it. The reviewer's alias (`pub use crate::proxy as PROVIDER_ADDRESS_SPACE;` beside
    the constant), a nested re-export, a glob, a second definition, a missing one, a module beside a function, a
    redirect in `module/mod.rs`, a `#[path]` elsewhere in the package, a directory beside a module's file and a `[lib]`
    table are each tested (`providers_admitted_paths_resolve_by_layout_to_the_items_their_files_define`);
  - **(c)** `components/prov/Cargo.toml` and `components/prov/src/module/mod.rs` join `.github/CODEOWNERS` and
    `HOUSE-RULES.md`; the files that define the key resolver are under `components/prov/src/proxy/`, already listed.
  - **The boundary check is a lint over source, not a proof.** It reads the text of the files it names and resolves
    paths by their layout; it doesn't run the compiler's name resolution, and macro-generated items, build scripts or
    a code path it doesn't model could still widen what the reader host reaches. The control behind it is code review:
    every file it reads for the admitted paths is a `DEC-20` path, which reaches `main` only with the owner's click
    after a separate security review.
- **L7.** The host's network-namespace check fails closed: a reader whose `/proc/<pid>/ns/net` (or the host's) can't
  be read is refused (`reader_namespace_unreadable`).
- **L8.** At most `MAX_HANDSHAKING_PER_SOURCE` (4) of the 16 handshake slots may be held from one source address
  (`LinkLimits::handshaking_per_source`), so one peer that never authenticates can't starve Core of handshakes. Peers
  from many addresses still can: gap, owned by `F-030`, whose deployment keeps the link's port reachable from Core's
  task alone (a security group, or the task's network policy).

Rule-offs, each alone and reverted:

| Rule switched off | Test | Red |
|---|---|---|
| The phase's bound read on every turn | `reader_that_keeps_talking_or_stops_reading_is_stopped_at_its_bound_and_the_others_answer_on` | the `pipeline` reader (two slow pool requests queued ahead of each reply) was not stopped within 2.5 s: its worker always had a frame waiting |
| A re-exported name is defined in its source file | `providers_admitted_paths_resolve_by_layout_to_the_items_their_files_define` | the alias beside `PROVIDER_ADDRESS_SPACE` passed |
| No `#[path]` anywhere under prov's `src/` | the same | the redirect in `module/mod.rs` passed |
| No `[lib]` table in prov's manifest | the same | `[lib] path = "src/lib_wide.rs"` passed |
| No directory beside a module's file | the same | `src/proxy/kv/` passed |
| Handshakes per source address | `link_closes_a_handshake_past_its_deadline_and_keeps_room_for_core` | the third connection from one address was held to the handshake's deadline, not closed at once |

The `stream` reader the reviewer described (one send queued ahead of each reply) no longer holds its worker once a
send's wait ends with the phase (M2's fix): after the bound each send returns at once, the worker drains faster than
the reader refills, and the timeout path stops it too. So the `pipeline` reader, whose stand-in endpoints take 20 ms
whatever the deadline, is the case that shows the per-turn bound load-bearing; the `stream` reader stays in the test.

## T-015-05: what the check of T-015-04 left

The milestone reviewer's check of `T-015-04` at `ce68f7a2` (review channel, 2026-11-27): one Low-Medium and eight
Lows, each handled here. `DEC-20` paths edited: `components/reader-host/`, `rutent-prov`'s `keys` path
(`components/prov/src/keys.rs`, and the facade it re-exports, `components/prov/src/proxy/statement_keys.rs`, with its
declaration in `components/prov/src/proxy/mod.rs`), `tools/boundary-check/` and `.github/workflows/jail-probes.yml`.
`rutent-prov`'s `confine` path is not edited: the host's bound is set in its own pre-exec step, beside the launcher's.

### The Low-Medium: the host bounds a reader's memory

- **Set by the host, before the reader runs.** `jail::command` adds a pre-exec step after `isolated`'s own (where
  `oom_score_adj` was already raised): `setrlimit(RLIMIT_AS)`, soft and hard, at `PROVIDER_ADDRESS_SPACE` (4 GiB),
  Linux only. It is inherited through `unshare`'s fork and the reader's `exec`, so a reader that skips its own
  `confine` is bounded all the same. It can't raise it: raising a hard limit takes `CAP_SYS_RESOURCE` in the initial
  user namespace, and the reader is root only in its own. Its own `confine` sets the same bound again, which a
  process may always do (an equal hard limit is no raise). The bound is set in reader-host's pre-exec step, not in
  prov's `restrict`, so the provider's and the compiler's launch is unchanged and `rutent_prov::confine` needs no new
  item: a reversible choice; moving it into `Limits` is a `module::sandbox` change for a later row if wanted.
- **Read by the host's check.** `check_confined` now reads, in order: `/proc/<pid>/limits` (the address space, soft
  and hard, at the bound or under it: else `reader_memory_unbounded`, or `reader_limits_unreadable`); each thread's
  status as before (`reader_not_confined`); no child; the network namespace as before; and `/proc/<pid>/mountinfo`
  (one mount, the root of its file system at `/`, read-only, a `tmpfs`: else `reader_root_not_empty`, or
  `reader_root_unreadable`). The limit is read first, so a reader without it is refused for it whatever else it
  skipped. Every read that fails is a refusal: fail closed, as the namespace check already was.
- **Landlock is not read.** The kernel gives no interface to another process's Landlock domain (`/proc` reports
  none), so the host can't check it. Gap, recorded: the host holds it only through the seccomp filter it does read,
  which prov's `confine` installs after Landlock; a hostile reader could install a filter of its own and skip
  Landlock, which the empty root and the namespaces still bound. No owner: there is nothing on the kernel's side to
  read.
- **macOS** sets no address-space bound (macOS doesn't enforce `RLIMIT_AS`), as before: it is development only, and
  `sandbox-exec`'s profile is its jail.

### The Lows

1. **A reader's content is trusted** (`DEC-28`). For `rpc_pool`, the host hands the reader only what at least the
   quorum of the pool's endpoints agreed on, and signs an answer resting on agreed calls; the quorum binds which
   endpoints agreed on what the host fetched, not what the reader then says they said. A hostile reader can make one
   harmless agreed call and answer a fabricated movement: that is within `DEC-28`'s trust in the operator's own
   reader, whose program is the operator's (part 5 of `lib.rs` says so).
2. **`rutent_prov::keys` is a facade** over a key store's statement keys. It re-exports `Resolver`, `ResolveError`,
   `StatementKeys`, its settings `KeyStoreSettings`, and what they are made of (`StoreAddress`, `StoreScheme`,
   `Server`, `InvalidSettings`). It does not re-export `RoleSettings` (since #354's review, L1, below), `Bucket`,
   `kv_bucket`, `KvResolver`, `KvConfig`, `InvalidConfig`, `Token`, `HttpsTransport` or `TransportError`.
   `StatementKeys::new(settings, token)` takes the token's text and builds one resolver, for the settings' keys path,
   over HTTPS (or plain HTTP to loopback). It holds the resolver behind a private boxed `Resolver`, so nothing gives
   back the token or the transport. `KeyStoreSettings` names the store, the mount and the keys' path, and no `secrets`
   path: a settings file that names one is refused (`deny_unknown_fields`). `tools/boundary-check` holds `keys` to
   this: `PROV_KEYS` names those items only, and no public item, field, trait impl, associated type or re-export of
   the facade's own file (`PROV_KEYS_FACADE`) may name what it keeps inside (`PROV_KEYS_WITHHELD`, read by
   `public_items_naming_any`, the conformance rule's reader generalised to a list of names). Which paths the token
   itself may read is the store's policy, not the facade's: a deployment gives the reader host a token whose policy
   reaches the readers' keys alone (gap for `F-030`'s deployment, beside L8 above).
3. **A handshake slot kept for Core.** `LinkLimits::reserved_for_core` (default `RESERVED_FOR_CORE`, 1): past the
   overall and per-source bounds, a connection from a source Core has authenticated from (the latest `CORE_SOURCES`,
   8, remembered once a handshake holds Core's certificate) takes a kept slot no other source can take. Before a
   handshake ends, the source address is the only signal, so **behind a proxy or load balancer** that hides the
   peer's address, every peer shares the proxy's and so Core's kept slot, and the bounds don't separate them. A
   deployment behind one keeps the peer's own address to the host (a TCP pass-through that preserves it; TLS is never
   terminated in front of the host, since Core's client certificate must reach it), or keeps the host's port
   reachable by Core's network alone (L8's gap, `F-030`). The edit to `link.rs` is the slot alone: a `CoreSlots`
   struct, its fallback in the accept loop, its release in `Shaking`, and its `authenticated` call after a handshake.
4. **A question past its deadline is dropped.** `Host::answer` reads the question's deadline against the host's wall
   clock and answers one already past it with nothing (an empty frame, which Core reads as unreadable), on arrival;
   the worker drops one whose deadline passed while it waited (`question_past_deadline`, answered with nothing too)
   and takes the next. Neither reaches the reader. This is the one use of the host's clock against the deadline:
   whether an answer is in time is still Core's, on Core's clock. A host clock ahead of Core's drops questions Core
   would have taken; the operator keeps the host's clock synchronised.
5. **The host's own server certificate as a client** is refused, and a test shows it, with a server certificate under
   Core's own name beside it, refused for its purpose alone (`rustls`'s web PKI client verifier holds the client
   authentication purpose). No rule was added: the behaviour was already there, so the test passed when written.
6. **A zero `restart_after_ms` is refused** by `HostConfig::check`, as every other bound of zero is.
7. **The reader's working directory** is made by `jail::workdir`: a new directory, mode 0700, named with the reader,
   the host's process id, the generation and a random part (std's `RandomState`), so another user can't guess it; one
   already there (a symbolic link included) is refused, and left as it is, and the reader is down until its next
   start (`reader_did_not_start`).
8. **The program path** is the operator's configuration, as the rest of a reader's is: the host canonicalises it and
   runs it, and checks neither owner nor mode (`jail.rs`'s module documentation says so).
9. **The manifest's comment** (`components/reader-host/Cargo.toml`) names the three admitted paths, not the egress
   layer alone.

### Tests

- `reader_that_skips_its_own_memory_limit_is_refused_by_the_hosts_check` [`REQ-MDL-11`] (`tests/jail.rs`, Linux,
  run in `jail-probes.yml`'s reader-host step, whose `grep -qx` of its `... ok` line fails the job if it is skipped):
  the `unconfined` stand-in launched by prov's launcher alone has no bound and is refused for it
  (`reader_memory_unbounded`); launched by the host's, `/proc/<pid>/limits` shows the bound, soft and hard, and the
  check refuses it only for the rest of its jail (`reader_not_confined`). Every expectation is read before any fails.
- `host's own server certificate presented as a client is refused` [`DEC-28`] (`tests/link.rs`).
- `question past its deadline is dropped by the reader host` [`DEC-28`] (`tests/readers.rs`): dead on arrival, and
  dead by the time its turn comes behind a slow question, each answered with nothing and never in the `inspect`
  reader's log of the frames it read; the question ahead is answered, and so is the next.
- Beside them, each for a rule of this row: `host_reads_a_readers_address_space_limit_and_refuses_one_unbounded`
  [`REQ-MDL-11`] and `host_reads_a_readers_root_as_the_jails_empty_one_only` [`REQ-MDL-7`] (the check's readers of
  `/proc`, pure, so they run on every platform);
  `readers_working_directory_is_its_own_with_mode_0700_and_never_one_that_already_exists` [`REQ-MDL-7`];
  `reader_host_refuses_a_restart_delay_of_zero` [`REQ-MDL-11`];
  `slot_kept_for_core_is_taken_only_from_a_source_core_authenticated_from` and
  `link_keeps_a_handshake_slot_for_core_that_peers_from_other_sources_cant_fill` [`DEC-28`];
  `providers_keys_path_is_a_facade_over_statement_keys_that_gives_back_nothing_it_keeps_inside` [`DEC-28`, `DEC-2`,
  `REQ-MDL-6`]. `reader_never_holds_its_statement_key` now builds its keys through the facade.

### Rule-off

Each rule switched off alone in the source by a scratch script of the author's (not kept), the test that holds it run
with the stand-in readers built, the file put back. Every one went red on an assertion.

| Rule switched off | Test | Red |
|---|---|---|
| The address space read is at the bound, soft and hard | `host_reads_a_readers_address_space_limit_and_refuses_one_unbounded` | `unlimited unlimited` passed as bounded |
| The root is one mount alone | `host_reads_a_readers_root_as_the_jails_empty_one_only` | the empty root with `/proc` mounted beside it passed |
| The root is read-only | the same | a writable `tmpfs` root passed |
| The root is a `tmpfs` | the same | the machine's read-only `ext4` passed |
| The working directory's mode is 0700 | `readers_working_directory_is_its_own_with_mode_0700_and_never_one_that_already_exists` | `left: 755` |
| One already there is refused | the same | `unwrap_err()` on `Ok`: an existing directory taken |
| A zero restart delay is refused | `reader_host_refuses_a_restart_delay_of_zero` | `unwrap_err()` on `Ok`: the configuration passed |
| A question dead on arrival is dropped there | `question_past_its_deadline_is_dropped_by_the_reader_host` | the question dead on arrival while the reader was busy waited 1.05 s behind the first |
| A question that waited past its deadline is dropped at its turn | the same | `left: Some(..)`: the late question was handed to the reader and answered |
| The slot kept for Core | `link_keeps_a_handshake_slot_for_core_that_peers_from_other_sources_cant_fill` | Core was closed at once while a peer held the one slot |
| The kept slot only for a source Core authenticated from | the same, and `slot_kept_for_core_is_taken_only_from_a_source_core_authenticated_from` | Core's connection took the kept slot before Core had ever authenticated; a source not yet Core's took it |
| The facade's public items name nothing it keeps inside | `providers_keys_path_is_a_facade_over_statement_keys_that_gives_back_nothing_it_keeps_inside` | `left: []`: a public `KvResolver` field passed |
| `keys` narrowed (`KvResolver` admitted again) | the same | the real `keys.rs` no longer held `PROV_KEYS`'s re-exports |

The first build of the deadline test showed the drop on arrival not load-bearing: a dead question that arrived while
the reader was free reached the worker's drop at its turn, and was dropped there. The test now also sends one dead on
arrival while the reader is busy, which must come back at once rather than wait in the queue, and that is the case the
rule-off turned red. The host's launcher bound and its check's read of `/proc/<pid>/limits` exist only on Linux, so
their rule-offs ran on the draft PR's `jail-probes` job (below). The wiring of the root check on Linux has no test of
its own: no stand-in can pass the status check (seccomp, capabilities) without `confine`, which empties the root
first. It is held by the pure reader's rule-offs above and by `T-015-04`'s confined reader passing it in the same job.

On Linux, on the draft PR (#354): `bf1731f8`'s `jail-probes` run 46059050785 passed both reader-host probes. The
unbounded reader was refused with `reader_memory_unbounded` (`unlimited unlimited`). The hosted one was bounded at
`4294967296 4294967296` and refused only with `reader_not_confined`. `T-015-04`'s confined reader passed the new
limits and root checks. Then the two Linux rules were switched off together in `2746c4c7` and reverted in the next
commit. The test reads every expectation before it fails, so run 46060498666 shows each red on its own:

| Rule switched off | Test | Red |
|---|---|---|
| The host's launcher sets `RLIMIT_AS` | `reader_that_skips_its_own_memory_limit_is_refused_by_the_hosts_check` | the hosted reader's limit was `unlimited unlimited`, not the bound |
| The check reads `/proc/<pid>/limits` | the same | the unbounded reader was refused only as `reader_not_confined`, never for its address space |

### The review of #354 at `6e33c11b`

Approved, with no High or Medium findings. The reviewer found two Lows, both fixed in this row:

- **L1: the keys facade could be bypassed.** `RoleSettings` came through `keys` with its public `secrets` and `keys`
  paths, and `StatementKeys::for_role` built its resolver from `role.keys`. Setting `role.keys = role.secrets.clone()`
  read a declared secret, and the boundary check stayed green. Now:
  - The facade is built from `KeyStoreSettings`, defined in the facade's own file (`proxy/statement_keys.rs`). It
    names one path, `keys`, and no `secrets` path; JSON that names one is refused. `--key-store` reads that shape
    (default, reversible: a process role's settings file is refused rather than read with its `secrets` path
    ignored).
  - `RoleSettings` and `ProxySettings` leave `PROV_KEYS` and join `PROV_KEYS_WITHHELD`. The boundary check refuses
    them re-exported through `keys`, and refuses a public item of the facade naming them (a `for_role(&RoleSettings)`
    again), or naming `secrets` (a settings field for that path).
  - The claims are made true or softened. The facade's documentation and `main.rs` now say the settings name no path
    but the keys', and that which paths the token may read is the store's policy, the deployment's.
  - Tests: `key_store_settings_that_name_a_secrets_path_are_refused` [`REQ-MDL-6`] (prov's unit test), and the
    bypass's cases in `providers_keys_path_is_a_facade_over_statement_keys_that_gives_back_nothing_it_keeps_inside`.
    With `RoleSettings` out of `keys`, the reviewer's bypass no longer compiles in the reader host.
- **L2: the worker could overflow its stack.** The worker dropped each dead question by calling `next()`, which
  called `ask()` again, so a queue of expired questions recursed once per question, bounded only by an uncapped
  `pending`. A stack overflow aborts the whole host. Now:
  - `next()` is a loop: `ask()` says whether it handed the question over, and `next()` takes the next until one is
    handed over or none waits.
  - `HostConfig::check` refuses `pending` above `MAX_PENDING` (4096).
  - Tests: `thousands_of_expired_questions_behind_a_slow_one_are_dropped_without_growing_the_workers_stack`
    [`REQ-MDL-11`] (worker's unit test: 4095 dead questions behind a slow one whose answer arrives, on a thread with a
    256 KiB stack) and `reader_host_refuses_more_questions_waiting_than_its_cap` [`REQ-MDL-11`].
- `link.rs` is unchanged by these fixes. #353 (`T-024-25`) also edits `link.rs`, these notes and boundary-check's
  tests, and merges after this row.

Rule-offs for the two fixes, each alone and put back:

| Rule switched off | Test | Red |
|---|---|---|
| `RoleSettings` not admitted through `keys` (added to `PROV_KEYS`) | `providers_keys_path_is_a_facade_over_statement_keys_that_gives_back_nothing_it_keeps_inside` | `left == right` failed: the admitted list no longer matched the facade-only `keys.rs` |
| No public item of the facade names `RoleSettings` (taken out of `PROV_KEYS_WITHHELD`) | the same | `left: []`: `for_role(&RoleSettings)` on the facade passed |
| Key store settings refuse an unknown field | `key_store_settings_that_name_a_secrets_path_are_refused` | `unwrap_err()` on `Ok`: settings with a `secrets` path were read |
| `next()` is a loop (recursion per dead question put back) | `thousands_of_expired_questions_behind_a_slow_one_are_dropped_without_growing_the_workers_stack` | `has overflowed its stack`, the test process aborted (`SIGABRT`): what aborted the host |
| `pending` capped at `MAX_PENDING` | `reader_host_refuses_more_questions_waiting_than_its_cap` | `unwrap_err()` on `Ok`: 4097 questions waiting passed |
