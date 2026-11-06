---
status: Accepted (Pablo, 2026-11-06, decision-form item `module-sandbox-boundary`)
decision_item: module-sandbox-boundary
supersedes: adopts suggestions/ADR-002-module-format-sandbox.md
---

# ADR-0003: Module format and sandbox boundary

## Context

`REQ-MDL-15/16` require the platform to accept at least one common, verifiable, portable binary format from any
compiling language, with the platform producing the machine code, and require the outside-world-facing part of a
module to be split from its sandboxed part. `suggestions/ADR-002` proposed treating the *process boundary* and
the *binary format* as separate choices, and trialling both Wasm and PVM, two languages each, scored against
`AC-20` (multi-language sandboxing).

`research/2026-11-02-provider-modules.md` found Wasmtime published 62 advisories in 2026 against 4 in 2025 (5
Critical, 2 needing no non-default setting) — independently re-confirmed by this session's earlier research fork
— but the same report is explicit that raw advisory counts across differently-scrutinized projects aren't a
safety ranking. PVM (`polkavm`, Apache-2.0) has only one guest language documented today, which on its own fails
`AC-20`'s two-language bar until a second is built or found. No report found a measurement of sandbox overhead
under a provider-call-dominated (network-wait) workload for either.

Per ADR-0002's ruling, this ADR does not need to pick a format now — only the process boundary, which is firm
regardless of format.

## Options considered

1. **Sandbox-only boundary (Wasm/PVM in-process), no process separation.** Rejected: a compiler or runtime bug
   that escapes the sandbox escapes everything in that process, including every other provider's module.
2. **One OS process per provider, sandboxed within it**, with the admitting compiler itself isolated (it parses
   hostile binary input from an untrusted publisher). Defense in depth: a sandbox escape is contained to one
   provider's process, not the whole platform.
3. **microVM/gVisor per provider.** Strongest isolation; highest operational cost (a full VM or gVisor sandbox
   per provider, for potentially many low-volume providers) with no report finding this cost justified at
   rutent's expected provider count.
4. **V8 isolates.** JS/Wasm only; even V8's own maintainers describe isolates as needing an outer sandbox, so this
   doesn't reduce what option 2 already requires.

## Decision

Option 2: one OS process per provider, regardless of eventual binary format, with the admitting compiler run in
its own isolation. The binary format itself (Wasm, PVM, or both) is a milestone-1 experiment per ADR-0002's
ruling — one stand-in module per provider category, two languages, scored against `AC-20` — not a STEP-2
commitment. A second format is only adopted if it earns its own upkeep cost, not by default.

## Consequences

- `ARCHITECTURE.md` §5's deployment view states the process boundary as firm; no later format experiment can
  remove it without a new ADR.
- `THR-85` (unknown sandbox escape) is **not newly mitigated** by this ADR alone — it narrows the blast radius of
  an escape (one provider's process, not the platform) but doesn't close the escape class itself. This is the
  residual risk `ARCHITECTURE.md` §6 states explicitly, pending the format experiment's outcome.
- `REQ-MDL-13` (one provider's module change doesn't reload the service) follows directly from the process
  boundary: restarting one provider's process is independent of every other.
- If PVM is later chosen, its one-guest-language limitation must be resolved (a second language built, or Wasm
  kept as the second format) before `AC-20` can be claimed satisfied.
