---
status: Accepted (Pablo, 2026-11-06, decision-form item `evidence-grading-checker`); amended by the owner's finding 7 on the M3 demo (Pablo, in chat, 2026-11-25): each public network's reader is an operator's module the platform runs
decision_item: evidence-grading-checker; amendment from the owner's finding in chat (2026-11-25), no form item
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

## Amendment: where each public network's reader runs (2026-11-25)

The owner reviewed `M3`'s demo on 2026-11-25 and called this a design flaw to fix before `M3` goes out (finding 7 of
the demo's notes, given in chat). Each public network's reader runs in Core's process today. It should run as a host
extension: a module the platform admits and runs, outside Core. One point of the design stands: a public network is
one the operator checks itself, so its reader is the operator's own.

### Context

- **The code.** Core reads public records through `core-orch::closing::PublicRecords`, a synchronous trait with three
  calls: `rules`, `status` and `movement`. The assembly passes it in as `&dyn PublicRecords`. Its only implementation
  is the scenario harness's `Readings`, over copies of the simulated networks. The harness refreshes those copies
  after each operation it carries out (`conformance/scenarios/src/assembly/grading.rs`). A real reader written to
  this trait would be linked into Core's process. A hostile RPC answer, or a fault in the reader, would then reach
  Core itself.
- **The documents.** `ARCHITECTURE.md` §2 keeps `CMP-EVIDENCE` apart from every provider's module and allows it as a
  standalone checking service. This ADR and `DEC-13` give each network's finality, reorg rule and matching to its own
  task in `F-031`. None of them says how a reader is packaged, admitted, versioned or isolated, or how it reaches its
  RPC nodes.
- **The precedent.** A public node's holding account is already an operator's module outside the core
  (`REQ-CAT-23`). It is admitted on the operator's own signed release, with no provider advertisement (`T-016-04`),
  and registered by the operator for its node. The platform asks it the operator's own questions, such as
  `holding_balance`, through the module runtime. A reader fits the same shape.

### Options considered

1. **Keep the reader in Core's process, behind `PublicRecords`.** Rejected by the owner. A reader's faults and its
   RPC answers would share Core's process, its RPC destinations would be reached with no declared allow-list, and no
   record would name the code that read a movement.
2. **A standalone checking service outside the platform** (the reading `ARCHITECTURE.md` §2 allows). Rejected for
   v0. It would need its own admission, signing, egress control, sandbox and versioning. The platform already gives
   every module all five (`REQ-MDL-1`, `REQ-MDL-7`, `REQ-MDL-11`, `REQ-MDL-2`), so a second system would duplicate
   them and could drift from them.
3. **An operator's module on the platform, like the holding wallet.** Chosen. Core keeps its seam, and the platform
   gives the reader everything the module boundary gives a provider.

### Decision

Each public network's reader is an operator's module that the platform admits and runs. `CMP-EVIDENCE`'s checks
(`grade_outcome`, `public_record_check`, `verify_movement`, the replay) stay where they are. Only the reading of a
public network moves out of Core.

- **Packaging and admission.** A reader is a module the operator publishes, of the kind `T-016-04` made for holding
  accounts: admitted through the platform's admission (`CMP-PROV`, `F-013`) on the operator's own signed release
  assertion, with integrity and signature checked and acceptance recorded (`REQ-MDL-1`), and with no provider
  advertisement behind it. The operator registers an admitted reader for one network on its public list
  (`REQ-CAT-22`), the network the reader's own answer names. There is one reader per network, and a reader serves
  one network. A provider's module is never a reader, and a reader is never a provider's module.
- **Its questions.** A reader answers three operator's questions, carried by the provider role contract in the same
  way as `holding_balance` and `status_query`: `network_rules` (the network it reads, its finality definition and
  its reorg rule), `movement_status` (a movement by its id: unknown, pending, final or reorged out, with its
  destination, asset and amount) and `movement_of` (the movement the network ties to a segment's transaction, or
  none). Each is read-only and moves nothing. A module may not call another with them, since each is the operator's
  own question. A reader's declaration handles these three questions and nothing else, so a reader moves no value
  (`REQ-MDL-8`, `REQ-MDL-9`). The shapes fold into provider-role 4.0.0 and module-interface 5.0.0, both unreleased
  (`DEC-4`).
- **Egress.** The reader's outside part declares its RPC nodes as its destinations. The platform's egress layer
  refuses every other destination, checking redirects and altered resolution (`REQ-MDL-7`, `THR-79`). It records
  every outbound call with destination, version and transaction (`REQ-MDL-8`). The platform enforces the allow-list;
  the module's own code is not trusted to keep to it. An RPC key, where a node needs one, is a secret behind the
  credential proxy (`REQ-MDL-6`).
- **Isolation.** The reader runs in its own jailed process, like any module (ADR-0003 as amended), and is never
  linked into Core. Fuel, memory, a deadline and output limits apply (`REQ-MDL-11`), and its output is held to its
  question's schema (`REQ-MDL-12`). A reader that faults, loops, overruns a limit, answers past its deadline or
  answers in the wrong shape yields **unreadable**. Unreadable never crashes Core, and it never becomes a
  confirmation. An answer that names another network, movement or transaction than the one asked is unreadable too.
  Adding, upgrading or removing a reader reloads nothing else (`REQ-MDL-13`).
- **Versioning.** The platform hands back each answer with the provider identifier and the version in force of the
  reader that gave it. Each `evidence` entry that rests on a public record names the reader and its version beside
  the network and the movement, as a decision record names module versions (`REQ-AUD-4`), so what read a movement is
  traceable to an admitted binary (`REQ-MDL-2`). `core-internal`'s public movement gains the reader, folded into its
  unreleased 1.0.0. A reader's new release is staged, promoted and rolled back as any module's is (`REQ-MDL-4`,
  ADR-0014). A segment moves only to an accepted version (`REQ-MDL-5`).
- **Calling convention.** `PublicRecords` stays Core's seam, in `core-orch::closing` (`REQ-MOD-3`). Core implements it
  with an adapter that asks each network's registered reader through the platform's call path: the platform's
  synchronous handling of an operator's question, under a transaction of Core's own, as `status_query` is asked.
  The calls stay synchronous. The platform bounds each one, so a call returns an answer or unreadable within its
  deadline.
  - **The read-then-refresh pattern changes.** Today the harness copies each network and refreshes the copy after
    every operation. Under the adapter, Core holds no copy of any network. A **pass** is one grading of a part's
    results, or one round of Core's schedule. Within a pass the adapter asks each question once and keeps the answer,
    so one grading sees one answer for each movement. It drops the answers when the pass ends, and the next pass
    asks again. What was read stays on record through the `evidence` entries, which `Closing::recorded` rebuilds
    after a restart. No reading is kept beside the log.
  - **Unreadable has a value of its own in the seam.** `PublicRecords`' calls answer either what the reader said or
    `Unreadable`. Closing treats `Unreadable` as "this segment doesn't settle yet". It records no grade for it, does
    not grade it down to the provider's word, and does not count it as a movement not found. The next pass asks again.
    Reading the network again is the only way to settle the segment. If the reader stays unreadable, the stuck watch
    times the segment, as it times any segment whose outcome is not final (`INV-3`, `REQ-ORD-29`). At routing, a final
    deposit to a network whose reader is unreadable is not routed to, as for a network Core reads no record of
    (`REQ-ORD-33`).
- **Simulation.** The simulated networks' readers become modules too: a reader guest per simulated network, admitted
  and registered as a real reader would be, reaching the simulated network's RPC endpoint through egress. The
  scenarios then exercise the real path: admission, egress, the sandbox and the adapter. The harness's in-process
  `PublicRecords` leaves Core's path. Its planted faults, such as a network that ties a deposit by destination and
  amount, become behaviours of the simulated reader.
- **What stays.** The reader is the operator's own, never a provider's or a third party's. A public record is grade 1
  because anyone can check it on the network: the `evidence` entry names the network, the movement and the reader
  that read it, so another party can run its own reader against the same movement. `CMP-EVIDENCE` stays operator-run
  (`ARCHITECTURE.md` §5, `REQ-OPS-4`). Each network's finality definition, reorg rule and matching are still settled
  per network when the network joins the catalogue (`F-031`). From now on they are settled in that network's reader
  module.

### Consequences

- **`M3`'s rows** (`PLAN.md`, `DEC-28`, wave 44 and after):
  - the contract round for the reader's questions (`T-020-59`);
  - the platform's admission, registration, egress and running of a reader (`T-013-12`);
  - Core's adapter, with the reader named in each `evidence` entry (`T-020-60`);
  - the simulated readers as modules, with the harness's reader removed from Core's path (`T-024-25`).
  `T-024-10` and `T-025-05` wait on all four.
- **`F-031`'s per-network tasks** each ship a reader module, admitted on this path, and no longer change Core.
- **A residual, stated.** A reader is the operator's code, so a reader that lies about a movement is the operator's
  fault, as the holding wallet's balance is the operator's module's word (`REQ-AUD-15`). The platform bounds what a
  reader can do: it moves no value, reaches only its declared RPC nodes and can't crash Core. It can't make the
  reader truthful. A hostile RPC node is the reader's to guard against, for example by asking more than one node.
  That is part of each network's own task (`F-031`), not of this ADR.
- **The cost.** Each read of a public record crosses a process boundary, and each pass asks each question once. No
  target is set before `M4`. `F-033` measures it with the rest of the platform.
