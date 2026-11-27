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
`DEC-20` paths. Nothing of ours depends on it: `T-024-25` adds the scenario assembly's dev-dependency.

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
  `reader_restarting`. Memory is bounded by `RLIMIT_AS` on Linux (`memory_bytes`).
- **(3) Signing outside the reader** (`src/keys.rs`). The worker hands the host the reader's answer body; the host
  holds it to the question (`worker::holds`: the request, the network, the question as asked, the configured version,
  `ReaderAnswer::check`, and for `network_rules` the configured identity and trust rule) and builds the envelope: Core's
  deployment and protocol version from the configuration, the reader as maker, its listed key. It resolves the private
  half by name at that moment (`StatementKeys`), refuses one whose public half isn't the listed key, signs the
  provider-role signing input, and drops it. An answer that doesn't hold is signed as unreadable, with why. The key is
  never in the reader's frames, arguments or environment.
- **(4) Egress, enforced by the host** (`src/egress.rs`, `src/jail.rs`). On Linux the reader starts under `unshare` in
  new user (root mapped), network, IPC, UTS, mount and PID namespaces with `--fork --kill-child`, as `CMP-PROV` starts a
  provider's process, and `--mount-proc` beside them; between fork and exec every descriptor above 2 is closed, core
  dumps are off, open files bounded, `RLIMIT_AS` set and `no_new_privs` on; the environment is cleared and the working
  directory is an empty one. Its one channel is its standard input and output (`src/wire.rs`: frames of JSON). A pool
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
  also runs on Linux in `ci.yml`'s workspace test step. Not run locally (macOS).
- `reader_that_crashes_or_hangs_is_unreadable_and_the_other_readers_answer_on` [`REQ-MDL-11`] (`tests/readers.rs`)
- `reader_never_holds_its_statement_key` [`REQ-MDL-6`] (`tests/keys.rs`, alone in its binary since it sets the
  environment the production key store reads)
- `evm_reader_with_fewer_than_two_agreeing_rpc_answers_is_unreadable` [`REQ-ORD-39`, `DEC-28`] (`tests/readers.rs`)
- `substrate_reader_checks_the_networks_genesis_before_answering` [`REQ-ORD-39`, `DEC-28`] (`tests/readers.rs`)
- `restarted_reader_host_answers_nothing_until_each_readers_checks_pass_again` [`NFR-6`] (`tests/readers.rs`)

Beside them: `core_holds_the_host_to_the_operators_ca_and_the_listed_name_and_takes_a_rotated_leaf` [`DEC-28`],
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
7. Bounds by default: 2 GiB of address space, 10 s per question, 60 s for the start-up checks, 16 questions waiting,
   1 s before a restart, 5 s per egress call, 1 MiB per frame. A configuration sets each per reader; zero is refused.
8. The host reads its chain and key at start: rotating the leaf is a restart, with no listing change.
9. The production key store is the host's environment (`EnvKeys`, `RUTENT_READER_KEY_<READER>_<NAME>`), which ECS
   fills from its secrets at start: a rotated key is used from the next start. The readers' environment is cleared.
10. The binary makes the host undumpable on Linux (`PR_SET_DUMPABLE` 0), so a process of its user can't read its
    memory or environment; and `--mount-proc` keeps the host's processes out of a reader's `/proc`.
11. On macOS, for development only, a reader runs under `sandbox-exec` with `(deny network*)`.
12. A light client's relay hands back what a connection gives within 2 s of a send, at most 64 KiB.

### Departures from the row, and gaps

1. **The jail.** The row says the reader runs in the jail `CMP-PROV` uses. That launcher (`sandbox::isolated`, with the
   in-process `confine`) is in `rutent_prov::module`, and the boundary admits the host only to `rutent_prov::egress`
   (`T-020-59`). The host composes the same `unshare` namespaces itself (`jail::UNSHARE`, with `--mount-proc`), and the
   pre-exec limits; `confine`'s in-process layers (the empty read-only root, dropped capabilities, Landlock, seccomp)
   are not applied, since a reader is the operator's own program, not the provider binary. Gap, for the lead and
   `T-031-02`/`T-031-03`: move the launcher where the boundary admits it, or have each real reader confine itself.
2. **The key resolver.** The row names `T-014-01`'s resolver. It is `rutent_prov::proxy::Resolver`, also outside the
   boundary. The host defines `StatementKeys` with the same contract (resolve by name when signing, keyed by reader, no
   copy kept), and ships `EnvKeys`. Gap, for the lead: an OpenBao/Vault binding for the host, either by a boundary
   change that admits the resolver or a re-export under `rutent_prov::egress` (a `DEC-20` edit to prov, not this row's).
3. **The egress layer's clock.** The lead asked for the host's clock to be handed to the egress layer through
   `with_clock`. `OperationClock` is in `rutent_prov::module::platform`, which the host can't name, so the resolver and
   the connector keep their default, `Monotonic`, which is the host's own monotonic clock. The names are confusing on
   prov's side: `Call::due` and `Dial::due` are documented as "when the operation the call serves must end, if it has a
   deadline", which for the reader host is its own give-up time (`egress_give_up_after_ms`), never Core's answer
   deadline. Prov's API is not renamed here.
4. **A light client's connections.** The egress layer has no session interface (a `Connector` is one request and one
   answer, and refuses `wss`), so the host opens them with the layer's resolver and dialer and its own address check,
   and records them in its own `ConnectionRecord`: the module-interface `Hop` has no scheme for a p2p transport. The
   relay is a send and what comes back within a wait, not a stream. Gap, for `T-031-02`: a streaming relay for
   `smoldot-light`'s transport.
5. **`wss` in an RPC pool.** The contract admits `wss` endpoints; the production connector refuses them, so such an
   endpoint never answers and counts against the quorum. Gap, for `T-031-03`.
6. **The metadata check.** The host holds a light client's reported genesis to its configuration; the metadata it
   relies on and answering only on finalised state are the reader's own to hold (its code is the operator's), as are
   an RPC reader's confirmations. The host can't check either.
7. **The jail test** runs only on Linux, in CI; it was not run locally.

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
