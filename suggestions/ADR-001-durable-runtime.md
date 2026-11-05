# ADR-001: Durable execution and runtime

Non-binding. See `README.md`.

## Context

Orders wait on asynchronous providers (`REQ-STM-5`). States change only on recorded inputs (`REQ-STM-3`). Replay must reach the same states (`NFR-4`, R110). The state machine stays a usable library (`REQ-MOD-1`). The owner prefers VOS as the whole runtime, "as vos-core is heading", except the module side that reaches the outer world (R22). R38 makes that a suggestion, not a decision.

## Options

**VOS, whole runtime (owner's preference).** Hot install, metered guests, replicated log. Gaps at commit `1aadf44`; upstream gives steps, not dates:

- Extension host and external signals to workflows not built (`REQ-MDL-16`).
- No publisher signature check (`REQ-MDL-1`).
- Log is compacted, holds results not inputs, and keeps no client signature or operator key (`REQ-AUD-1`, `REQ-AUD-9`).
- Rust-only SDK (`REQ-MDL-15`).
- No staging, rollback or uninstall (`REQ-MDL-4`).
- Operators read all state, unencrypted (`REQ-MDL-6`).
- Other store layouts are refused; a restored backup counts as a compromised key (`NFR-6`).
- Nearly one author, no releases, no external audit yet. Own scale run missed target on one host (`NFR-1`).
- Host and SDK are AGPL-3.0-or-later, no linking exception found (`REQ-MOD-5`).

**VOS's Apache-2.0 PVM crates alone.** Brings no state, log or API.

**Own transition core, pluggable durability.** Rebuilds the part where Jepsen found Conductor's serious bugs.

**Server engine (Temporal, Conductor).** Neither is a library (`REQ-MOD-1`).

**Embedded durable library (DBOS-style).** Tied to Postgres, and one language per application.

## What we'd try first, and why

Write the transition rules as a plain library with no runtime types. That keeps `REQ-MOD-1` whatever host follows. Then run a time-boxed spike under VOS behind a thin wrapper, injecting faults at each boundary, scored against the gap list. If the gaps stay open, fall back to the own core.

## Consequences

Costs a wrapper and a possibly discarded spike. Adopting VOS also needs a separate audit store, and ties modules to its formats. No engine checked settles an unknown provider outcome (`REQ-ORD-1`) or commits across providers (`INV-3`).

What AGPL means for an MIT platform, for SDK-built modules, and for reusing components is a legal question. The report didn't settle it. `REQ-MOD-5` wants a recorded decision, with counsel.

## Requirements served

`REQ-STM-3`, `REQ-MOD-1`, `REQ-MOD-5`, `NFR-6`.

## Sources

- `research/2026-11-02-vos.md`: licences, compaction, scale line checked; the rest report's tags. Nothing was run.
- `research/2026-11-02-durable-state.md`: Jepsen checked; engine licences and scale figures unchecked.
- R22, R29, R38.
