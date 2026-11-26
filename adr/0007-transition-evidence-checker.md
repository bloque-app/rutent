---
status: Accepted (Pablo, 2026-11-06, decision-form item `evidence-grading-checker`); amended by the owner's finding 7 on the M3 demo (Pablo, in chat, 2026-11-25) and his form answers of 2026-11-26 (items `reader-registration-path` and `reader-answer-trust`): each public network's reader is a reader service, a separate process of the operator's own, listed by a decided catalogue change and reached over mTLS
decision_item: evidence-grading-checker, reader-registration-path, reader-answer-trust
supersedes: adopts suggestions/ADR-013-transition-evidence.md; introduces `CMP-EVIDENCE`
---

# ADR-0007: Transition evidence grading and the evidence checker

## Context

`REQ-ORD-39` requires every segment's outcome to carry the best evidence its provider can give, graded, with the
operator setting a per-tuple evidence floor. `SPEC.md` requires the grading scheme to exist but leaves its shape
open. `suggestions/ADR-013` — explicitly flagged by its own source confidence as **inference, not sourced**, no
report having studied evidence grading directly — proposes a four-grade scale and a read-only checker apart from
any provider's module, since a module validating itself "marks its own work."

## Options considered

1. **One evidentiary rule for everything**, requiring third-party proof universally. Rejected: excludes private
   nodes entirely, where no third party exists to ask.
2. **Evidence only at route endpoints.** Cheap, but a false mid-route success would move value onward
   undetected — fails the intent behind `REQ-ORD-39` requiring *every* segment to carry graded evidence.
3. **A four-grade scale** — (1) checkable by anyone (a public chain record), (2) signed by another party (the
   next provider's own receipt, or the rail itself), (3) signed by the provider with something external to
   compare later, (4) the provider's bare statement — each provider declaring which grade it can give, with route
   choice able to prefer stronger-evidence segments. Grade 2's cheapest form is the next provider's own receipt,
   since it already must confirm arrival; valid only when that next provider is a genuinely different publisher
   (fails when one publisher runs both sides of a hop, `THR-124`).
4. **Validation by the provider itself.** Rejected: a module checking its own work adds no new assurance and
   would require a protocol change for every new validating-provider role.

For public-network validation specifically: model each segment's claim as a role-shaped tuple of provable inputs
(ramp = transfer-in + transfer-out; exchange = asset-A-out + asset-B-in; reserve = withdrawal or deposit), checked
by a read-only checker run apart from any provider's module. That checker needs three network-specific things
settled per network: a finality definition, reorg handling, and how a movement ties back to a specific segment —
the hard part, since transfers often carry no order reference. One-destination-per-route is this ADR's answer;
matching by amount alone is called a guess (flagged `F-23` in the source draft) and rejected.

## Decision

The four-grade scale, declared per provider. `CMP-EVIDENCE` (named in `ARCHITECTURE.md` §2) is the read-only,
per-public-node checker that validates grade-1/2 claims against public records, run apart from any provider's
module. It is the same component ADR-0005 uses for private-node replay — one checker, not two, so a provider's
own module is never the thing marking its segment's work.

## Consequences

- Grade 2 (next-provider's receipt) fails by design when one publisher runs both sides of a hop (`THR-124`,
  unchanged, accepted) — route choice that prefers grade 2 must know the publisher behind each hop, not just the
  provider identifier, to avoid false reliance.
- Private nodes rest on grade 3/4 by necessity; a low evidence floor there is an explicit, recorded operator
  choice (`REQ-ORD-39`'s per-tuple floor), not a gap this ADR hides.
- `CMP-EVIDENCE` needs, per public network it checks, a finality definition and reorg-handling rule settled before
  it can run — this is construction-time work (`STEP-3`), not resolved by this ADR, and should be tracked per
  network as it's added to the catalogue.
- One-destination-per-route is now a load-bearing assumption for movement-to-segment matching; a future route
  shape that violates it (if ever proposed) would need this ADR revisited, not just `CMP-ROUTER` changed.
- This whole ADR rests on inference, not a sourced precedent (`suggestions/ADR-013`'s own confidence label) — it
  should be the first place to revisit if real operational evidence later contradicts the grading scheme's
  assumptions.

## Amendment: where each public network's reader runs (2026-11-25, revised 2026-11-26)

The owner reviewed `M3`'s demo on 2026-11-25 and called this a design flaw to fix before `M3` goes out (finding 7 of
the demo's notes, given in chat). Each public network's reader runs in Core's process today. It should run as a host
extension, outside Core. One point of the design stands: a public network is one the operator checks itself, so its
reader is the operator's own.

This amendment was first written on 2026-11-25 with the reader as a Wasm module in the platform's sandbox (commit
`08a9db85`). No row built on it. It is rewritten here on the owner's form answers of 2026-11-26
(`reader-registration-path`, `reader-answer-trust`) and on what VOS offers today, below.

### Context

- **The code.** Core reads public records through `core-orch::closing::PublicRecords`, a synchronous trait with three
  calls: `rules`, `status` and `movement`. The assembly passes it in as `&dyn PublicRecords`. Its only implementation
  is the scenario harness's `Readings`, over copies of the simulated networks. The harness refreshes those copies
  after each operation it carries out (`conformance/scenarios/src/assembly/grading.rs`). A real reader written to
  this trait would be linked into Core's process. A hostile RPC answer, or a fault in the reader, would then reach
  Core itself.
- **The documents.** `ARCHITECTURE.md` §2 keeps `CMP-EVIDENCE` apart from every provider's module and allows it as a
  standalone checking service. This ADR and `DEC-13` give each network's finality, reorg rule and matching to its own
  task in `F-031`. None of them said how a reader is packaged, listed, versioned or isolated, or how it reaches its
  RPC nodes.
- **What VOS offers** (read from source by the Aledgent session on 2026-11-26). The pinned `vos-core` commit
  (`5c78975`, `DEC-26`) and upstream's head (`7296d867`) have no extension host: `core-model.md` says the extension
  host is not designed, and only the host-call range `0x480..=0x4ff` is reserved for it. The older Codeberg VOS line
  has extensions, but they are `cdylib`s loaded into the daemon's own process, they link `vos` (so AGPL-3.0), and only
  durable `Local` roots reach them, never a Raft or CRDT space. `CMP-STM`'s three-replica space is Raft. So a reader
  can't be a VOS host extension today.
- **The owner's answers.**
  - `reader-registration-path`: a hybrid. Readers are kept apart from the core instance and may live as tasks in one
    ECS process, so they reach the core host over a network link, with mTLS. Once a reader is available, the operator
    lists its public network through the catalogue, by a change policy decides, naming how to find the reader (a host
    setting). If the reader exists and is reachable, the network can be public. Otherwise the network fails to start
    and can't be listed.
  - `reader-answer-trust`: it depends on the network. An EVM network's reader needs at least 2 RPCs out of a preset
    pool. A Substrate network's reader, such as Kreivo's, uses a light client, which already reaches many endpoints.
- **The precedent.** Bloque's `aledgent-settle` (`services/settle` in `bloque-app/aledgent`, Apache-2.0 OR MIT) is a
  separate service that reads a Substrate chain. It embeds the `smoldot-light` 2.1 light client, from the
  `bloque-app/smoldot` fork at `67ad6c37`. On start it checks the genesis hash, the metadata's transaction surface,
  the pallet, the registered keys and the asset decimals, and on any failure it declares itself refusing. It reads
  through a cursor gated on finality, and one instance is active per key set, fenced by a lease. settle reaches the
  core only through ingress; ingress's link to the core (F-038) runs over mTLS under the operator's CA, and a sequencer
  checks ingress's signature on every input and fences stale epochs.

### Options considered

1. **Keep the reader in Core's process, behind `PublicRecords`.** Rejected by the owner (finding 7).
2. **A VOS host extension.** Not available: the pinned line has no extension host, and the older line's extensions
   run inside the daemon, link AGPL code, and are reachable only from `Local` roots, not from `CMP-STM`'s Raft space.
3. **An operator's Wasm module in the platform's sandbox, like the holding wallet** (this amendment's first text).
   Replaced. A Substrate reader needs a light client, which keeps peer connections and state for as long as it runs,
   and the platform runs modules per operation. The owner's answer puts readers outside the core instance, as
   separate tasks reached over the network.
4. **A reader service.** Chosen: a separate process of the operator's own, reached over mTLS, listed through the
   catalogue by a decided change, and checked by Core before its network starts.

### Decision

Each public network's reader is a **reader service**. `CMP-EVIDENCE`'s checks (`grade_outcome`,
`public_record_check`, `verify_movement`, the replay) stay where they are. Only the reading of a public network moves
out of Core. A reader reports what it observed, as signed answers. It never decides an outcome: Core and the state
machine decide.

- **The process.** A reader service is a separate process, MIT-licensed as the rest of this repository, and the
  operator's own. A reader host runs it (`CMP-EVIDENCE`'s standalone part, `components/reader-host/`): one process,
  deployable as one ECS task, that runs several readers. Each reader is its own child process, so a reader's fault,
  crash or hang stops that reader alone, and the host restarts it. A reader is never linked into Core, and it is
  never a provider's code or a third party's.
- **The link.** Core reaches each reader host over mTLS under the operator's CA. Core's client certificate and the
  host's server certificate are both the operator's. Each answer is a statement in the contracts' envelope, made by
  the reader and signed with the reader's own key. The reader host signs it, so the reader's child process never holds
  the key. Core checks the host's certificate against the operator's CA and the host's subject name in the listing,
  and the answer's signature against the reader's statement key in the listing. Transport identity alone is not
  authorisation, so both are checked. The listing never pins the host's leaf certificate, so rotating it under the
  same CA and name needs no catalogue change. Core also checks that the answer names the question and the request
  Core sent, and that it arrived by its deadline on Core's own clock, before it reads anything in it. An answer that
  fails any check is refused, and Core reads it as unreadable.
- **Why no sequencer.** Aledgent's link puts a sequencer in front of its core to fence stale epochs. A reader's link
  needs none, on three conditions, each held by Core:
  - **(a) Only Core asks.** A reader's answer is never fetched inside `CMP-STM`'s replicas. It enters the state
    machine only through what Core records: the `evidence` entry, with `read_by`.
  - **(b) The request id is random.** Core draws each request id at random, never from a counter, so a host can't
    compute an answer before the question is asked.
  - **(c) Core's clock decides the deadline.** An answer's deadline is checked against Core's clock, never the
    host's.
- **Listing.** A public network is listed by a catalogue change that policy decides (`REQ-AUZ-1`), held to the
  operator. The change names:
  - the network, and its identity: a genesis hash, or a chain id;
  - the reader's endpoint: the host setting that says where its reader host is, and the reader's name on it;
  - the reader's identity: the operator's CA, the host's subject name (its SAN) and the reader's statement key, never
    the host's leaf certificate;
  - the reader's version;
  - the network's trust rule.
  The listing is appended before it applies, with its result after it, and Core rebuilds it from the log on reopening.
- **The start-up check.** When Core opens or reopens, and whenever a listing is decided, Core reaches each listed
  reader and asks it `network_rules`. It checks the answer against the listing: the network's identity, its finality
  definition and reorg rule, the reader's version and its trust rule. A network whose reader is unreachable, or fails
  the check, does not start. It is not on the public list (`REQ-CAT-22`), nothing is routed to it, and its segments
  read as unreadable. The operator is told why. Core's other networks run on. The check runs again at each reopen.
- **Its questions.** The reader answers three questions, each read-only: `network_rules` (the network, its identity,
  its finality definition and reorg rule, the reader's version and its trust rule), `movement_status` (a movement by
  its id: unknown, pending, final or reorged out, with its destination, asset and amount) and `movement_of` (the
  movement the network ties to a segment's transaction, or none). Their shapes are a contract of their own, `reader`,
  versioned under `DEC-4` from 1.0.0. The reader service is outside Core, and outer code never names `core-internal`.
  The listing's change is `core-internal`'s, folded into its unreleased 1.0.0.
- **Trust rule, per network.** The reader applies its network's rule before it answers. An answer it can't stand
  behind is unreadable, never a guess.
  - **Substrate:** an embedded light client, with `aledgent-settle` as a design reference. Its trust is anchored on a
    pinned genesis and a checkpoint (a `lightSyncState`) in a chain spec pinned by SHA-256. It checks the genesis hash
    and the metadata it relies on before it answers anything, and it answers only on state its relay chain has
    finalised. A lying peer can stall it, but can't forge finalised state.
  - **EVM:** at least two agreeing answers from a preset pool of RPC endpoints. The pool is named in the listing. Two
    answers that disagree, or fewer than two answers, give unreadable.
- **Egress, outside the reader.** A reader's child process has no network route of its own. Its one channel is to the
  reader host. Every connection or request it makes goes through the host's egress, which reaches only the
  destinations the reader declares. For an RPC pool these are the pool's endpoints, by name. A light client's peers
  are discovered, so its rule is per protocol and port, not per address: the network's p2p ports and `wss`, with a cap
  on its connections. The host refuses every other destination, checking redirects and altered resolution
  as `CMP-PROV`'s egress layer does, and records each call with destination, reader, version and request
  (`REQ-MDL-7`, `REQ-MDL-8` applied to readers). A reader's own code is not trusted to keep to its list. An RPC key,
  where a node needs one, is resolved by name by the host and never reaches the reader's process (`REQ-MDL-6` applied
  to readers).
- **Unreadable.** Anything that keeps Core from a good answer is unreadable: a reader host unreachable, a refused
  certificate or signature, an answer to another question or request, an answer past its deadline by Core's clock, a
  malformed answer, a trust rule not
  met, or a network that failed its start-up check. Unreadable never crashes Core, and it never becomes a
  confirmation.
- **Versioning.** Each answer names the reader and its version, and Core holds that version to the listing's. Each
  `evidence` entry that rests on a public record names the reader and its version (`read_by`) beside the network and
  the movement, as a decision record names module versions (`REQ-AUD-4`). So what read a movement is on record. A new
  reader version is listed by a new decided change, and passes the start-up check before it answers.
- **Calling convention.** `PublicRecords` stays Core's seam, in `core-orch::closing` (`REQ-MOD-3`). Core implements it
  with an adapter over its mTLS client. The calls stay synchronous: each is bounded by a deadline, and returns either
  an answer or unreadable within it.
  - **The read-then-refresh pattern changes.** Today the harness copies each network and refreshes the copy after
    every operation. Under the adapter, Core holds no copy of any network. A **pass** is one grading of a part's
    results, or one round of Core's schedule. Within a pass the adapter asks each question once and keeps the answer,
    so one grading sees one answer for each movement. It drops the answers when the pass ends, and the next pass asks
    again. What was read stays on record through the `evidence` entries, which `Closing::recorded` rebuilds after a
    restart. No reading is kept beside the log.
  - **Unreadable has a value of its own in the seam.** `PublicRecords`' calls answer either what the reader said or
    `Unreadable`. Closing treats `Unreadable` as "this segment doesn't settle yet". It records no grade for it, does
    not grade it down to the provider's word, and does not count it as a movement not found. The next pass asks again.
    If the reader stays unreadable, the stuck watch times the segment, as it times any segment whose outcome is not
    final (`INV-3`, `REQ-ORD-29`). At routing, a final deposit to a network whose reader is unreadable is not routed
    to, as for a network Core reads no record of (`REQ-ORD-33`).
- **Simulation.** The simulated networks' readers become reader services too. They run in the test harness, behind
  the same protocol, and TLS inside one test process is enough. Each is listed by a decided change, checked at
  start-up and read through the adapter. A simulated Substrate-like network and a simulated EVM-like network with an
  RPC pool exercise both trust rules. The harness's in-process `PublicRecords` leaves Core's path. Its planted faults,
  such as a network that ties a deposit by destination and amount, become behaviours of the simulated readers.
- **What stays.** The reader is the operator's own, never a provider's or a third party's. A public record is grade 1
  because anyone can check it on the network: the `evidence` entry names the network, the movement and the reader
  that read it, so another party can run its own reader against the same movement. `CMP-EVIDENCE` stays operator-run
  (`ARCHITECTURE.md` §5, `REQ-OPS-4`). A public network's reader is not a provider, so its code may be published in
  this repository. Each network's finality definition, reorg rule and matching are still settled per network when the
  network joins the catalogue (`F-031`). From now on they are settled in that network's reader.

### Consequences

- **`M3`'s rows** (`PLAN.md`, `DEC-28`, waves 44 to 46):
  - the `reader` contract, the listing's catalogue change and policy's permission for it (`T-020-59`);
  - the reader host: its mTLS server, the readers' child processes, its egress, the EVM pool rule and the start-up
    check (`T-015-04`, written first as `T-013-12`);
  - the simulated readers as reader services (`T-024-25`);
  - Core's adapter, the start-up check, the network that doesn't start, and the reader named in each `evidence` entry
    (`T-020-60`).
  `T-024-10` and `T-025-05` wait on all four.
- **`M4`'s real readers** are `F-031`'s per-network tasks. Kreivo's reader follows `aledgent-settle`'s light client as
  a design reference only, in a crate of its own outside the root lock, since `smoldot-light` is copyleft (`T-031-02`;
  the owner's item `smoldot-licence`). TLS for the reader host and Core's client is `rustls` on `ring`, already
  permitted. The EVM reader's two-of-a-pool rule has no precedent and is new work (`T-031-03`).
- **Revisit** if upstream VOS designs an extension host that a Raft space can reach. This ADR would then weigh a
  reader as one.
- **Residuals, stated.**
  - A reader is the operator's code, so a reader that lies is the operator's fault, as the holding wallet's balance is
    the operator's module's word (`REQ-AUD-15`). The trust rules bound one lying RPC node, not a lying reader.
  - The reader host's signature proves that this reader host relayed an answer, not that this reader's code produced
    it. A compromised host can sign for every reader it runs. This is accepted because the host is the operator's
    own, as the reader is.
  - A light client's peers are discovered, not listed one by one, so its egress is allowed by protocol and port, with
    a cap on connections: a wider rule than an RPC pool's names. A lying peer can stall the client but can't forge
    finalised state. `F-031`'s Kreivo task (`T-031-02`) sets the rule and the client's JSON-RPC limits.
- **The cost.** Each read of a public record crosses the network to the reader host, and each pass asks each question
  once. No target is set before `M4`. `F-033` measures it with the rest of the platform.
