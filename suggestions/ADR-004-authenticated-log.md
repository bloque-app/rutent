# ADR-004: Authenticated log, checkpoints and receipts

Non-binding. See `README.md`. R123 later made checkpoints public: see `ADR-014`.

## Context

History can't be rewritten without a checker outside the operator seeing it (`REQ-AUD-9`, `INV-6`). Each party can keep evidence of its own part (`REQ-AUD-5`). rutent also runs with no outside platform (`REQ-OPS-4`). R71 makes the checker whoever the operator appoints, such as an auditor. One is required, and named to customers and providers (R92).

## Options

**Merkle log, signed checkpoints sent to the checker.** Short proofs of presence and of append-only growth. Needs someone to really check.

**Hash chain with checkpoints.** Cheap to write. Proving one entry can mean a long trace.

**Outside anchor: a public chain, or independent witnesses.** Answers split views. It pulls against independent operation: writes or checkpoints gain an outside dependency (`REQ-OPS-4`), and public commitments raise a question nobody has ruled on.

**Database ledger feature.** Binds evidence to one vendor's product, and one such product has ended support.

**VOS's own log.** R62 names this as a reason for VOS. At the commit read it is compacted and holds no inputs, and production builds carry no anchoring client (ADR-001).

## What we'd try first, and why

A Merkle log apart from the execution store, in a documented format. The operator signs checkpoints on a schedule and delivers them to its appointed checker, who keeps them and tests consistency. Leave room for witnesses, without needing them.

Give each party a signed receipt with an inclusion proof for what it stated and was told, in the shape of RFC 9943, a young standard. Keep each provider's signed statement as received (`REQ-AUD-2`). Receipts held by a counterparty are the only thing here that can show an omission.

## Consequences

Adds a second write path, and record and effect can't drift apart (`INV-7`). An unexamined log can be tampered with unseen. With one checker, split views remain unless parties compare receipts.

This doesn't show what was never recorded (`THR-35`), whether an entry is true, or anything after full compromise (`THR-34`). It doesn't help with an absent or friendly checker (`THR-100`). Entries hold references only (`REQ-AUD-6`). Retention, erasure and whose clock defines "when" are unruled.

## Requirements served

`REQ-AUD-2`, `REQ-AUD-3`, `REQ-AUD-5`, `REQ-AUD-6`, `REQ-AUD-9`, `REQ-AUD-10`, `INV-6`.

## Sources

- `research/2026-11-02-durable-state.md`: RFC 9943 status and section 9.2 checked. Other tamper-evidence claims are marked for rechecking, so unchecked.
- `research/2026-11-02-vos.md`: compaction checked.
- R62, R71.
