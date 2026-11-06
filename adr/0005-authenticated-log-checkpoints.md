---
status: Accepted (Pablo, 2026-11-06, decision-form item `log-checkpoints-mechanism`)
decision_item: log-checkpoints-mechanism
supersedes: merges suggestions/ADR-004-authenticated-log.md and suggestions/ADR-014-public-checkpoints-and-replay.md
---

# ADR-0005: Authenticated log and public checkpoints

## Context

`suggestions/ADR-004` proposed a Merkle log kept apart from the execution store, with the operator signing
scheduled checkpoints to the appointed checker, and RFC 9943-style signed receipts per party. Its own header
already notes it is partially superseded: ruling `R123`, made after ADR-004 was drafted, requires checkpoints to
be **public**, "just succinct proofs," explicitly not meant to be "a real indicator of activity, especially if
issued regularly" (`REQ-AUD-14`, `AC-37`). `suggestions/ADR-014` answers that later requirement, and because its
checkpoint mechanism builds directly on ADR-004's log (the checkpoint commits to the log's root), this ADR merges
both drafts into one lineage rather than keeping them as two ADRs that would otherwise need to reference each
other for every claim.

`research/2026-11-02-durable-state.md` independently confirms Jepsen's findings against Conductor (abandoned
workflows, vanished records, workflows marked complete that never ran) as a caution against leaning on a
workflow engine's own built-in log; `research/2026-11-02-vos.md` separately found VOS's own log compacted (keeps
a tail of 256 entries by default) and missing inputs — both reports converge on *not* trusting a runtime's
built-in log for `PS-21`'s who/what/when/outcome requirement, independent of ADR-0002's separate runtime
question.

## Options considered

**For the log:**
1. **A hash chain.** Cheap, but proving a single entry requires a long trace back through the whole chain.
2. **A Merkle log, separate from the execution store.** Short inclusion proofs; the operator signs scheduled
   commitments to the root. Receipts held by a counterparty are the only thing that can later show an omission.
3. **An outside anchor** (a public chain, or witnesses). Pulls against `REQ-OPS-4`'s independent-operation
   requirement by adding an outside dependency; raises an unruled public-commitment question.
4. **A database's built-in ledger feature, or VOS's own log.** Vendor lock-in (one such product has already
   reached end-of-support); VOS's log is compacted and drops inputs, as above.

**For checkpoints (`R123`):**
1. **Periodic Merkle root publication at raw size.** Rejected by `R123` itself and by `AC-37`: raw tree size is
   "the giveaway in most root formats" — it would leak activity level, including whether an idle period occurred.
2. **Fixed-tick, fixed-size, salted commitment** — a signed commitment to the log's root, bound to the tick plus a
   fresh random value each time, with no raw size ever published, so consecutive checkpoints never look alike and
   an unchanged root can't signal idleness.
3. **A ZK proof over the same root.** Stronger (and more costly) than option 2; floated as an addable-later
   enhancement, not a replacement — adding it later changes nothing readers already check.
4. **A real transparency log** (append-only, third-party-run). Better than self-publication, but requires an
   outside dependency, conflicting with `REQ-OPS-4` the same way option 3 for the log did.

## Decision

A Merkle log, separate from the execution store (`CMP-CORE-AUDIT`'s own store, not `CMP-STM`'s or `CMP-PROV`'s),
with RFC 9943-style signed receipts issued per party on request. Checkpoints: fixed-tick, fixed-size, salted
commitments to that log's root, published wherever the operator can publish (no outside transparency-log
dependency required, though one would be a strict improvement if the operator chooses to add it later without
changing what readers check). Replay of a private node's recorded inner segments against its public exit
(`REQ-ORD-41`) runs inside `CMP-EVIDENCE` (ADR-0007) — the same component, so a provider's module is never
checking itself.

## Consequences

- `AC-37`'s indistinguishability bar (even a genuinely idle period must not look different from a busy one) is
  met by construction: salt plus the absence of any published raw size, not by an operator policy that could be
  forgotten.
- `THR-131` (a checkpoint only shows internal consistency, never that recorded segments actually happened) is
  **unchanged and accepted** — no mechanism in this ADR closes it; `ARCHITECTURE.md` §6 states this as residual.
- Retention, erasure, and whose clock defines "when" are left unruled by this ADR, consistent with
  `suggestions/ADR-004`'s own stated gap; `OBL-9` (the operator sets retention/erasure for its jurisdiction)
  already covers this at the operator-obligation level, so it is not re-opened here as a rutent design question.
- Every party's receipt (option 2 above) is the only artifact in this design that can later demonstrate an
  omission from the log — this is worth stating plainly to the appointed checker when `OBL-4` names one.
