---
status: Accepted (Pablo, 2026-11-06, decision-form item `runtime-host-seam`)
decision_item: runtime-host-seam
supersedes: adopts and reframes suggestions/ADR-001-durable-runtime.md
---

# ADR-0002: Runtime / host seam for `CMP-STM`

## Context

`suggestions/ADR-001` proposed writing the state machine's transition rules as a host-independent library first,
then running a time-boxed spike of VOS (`virto-network/vos-core`) — the runtime Pablo's own brief named as his
preference (`PS-66`) — behind a thin wrapper, fault-injected against a gap list, falling back to a purpose-built
core if the gaps stayed open.

`research/2026-11-02-vos.md` (re-verified at a pinned commit by this session's own spot-check) found real,
present-tense gaps if VOS were adopted as-is: no publisher-signature check, a compacted log that keeps only a
tail of entries and drops inputs (`PS-21`'s who/what/when/outcome needs more than VOS's log gives today), no
second sandbox layer, no containment fuzzing, a Rust-only guest SDK, and — load-bearing for `REQ-MOD-5` — the
`vos-core`/`vos-sdk` workspace is AGPL-3.0-or-later, with no linking exception found in the report. Compatibility
with rutent's MIT licensing intent needs counsel before adoption either way, independent of whether VOS otherwise
performs.

The real design question is not *when* to run a spike, but *whether `CMP-STM`'s contract itself needs to assume a
specific runtime to be designed at all.* It doesn't: `REQ-STM-1/3` ("one current state... changes only by
recorded input... replay-deterministic") and `REQ-OPS-3` ("replaceable execution environment without losing
in-flight orders") are both expressible against an abstract durable-store/host contract, with no runtime named.

## Options considered

1. **Commit to VOS now**, writing `CMP-STM` directly against its APIs. Fastest if VOS works out; commits
   `ARCHITECTURE.md`'s deployment view to an unaudited, near-single-author, AGPL-licensed dependency before
   counsel has weighed in and before its sandbox/log gaps are closed — a real risk to reverse later if it doesn't.
2. **Write `CMP-STM` host-agnostic now**, against a small contract (durable transition storage; recorded-input
   delivery, exactly-once to the state machine; environment-swap without losing in-flight orders), and treat
   *which* runtime implements that contract — VOS, or a purpose-built store — as a milestone-1 experiment
   (`REQ-CHN-12`'s "first milestone" framing, Narthex's `experiments/` mechanism) rather than a STEP-2 commitment.
3. **Build a purpose-built runtime now**, skipping VOS entirely. Discards real, already-gathered evidence (VOS's
   gaps are specific and fixable in principle) before anyone has tried to close them; premature given `SPEC.md`
   itself left this choice open.

## Decision

Option 2, Pablo's ruling, 2026-11-06 (`runtime-host-seam`, "Host-agnostic now, experiment later").

`CMP-STM` is specified and built against a host-agnostic contract. The module sandbox gets the same treatment
(ADR-0003): process-per-provider is firm now, Wasm-vs-PVM is the experiment. Either runtime choice still needs
the durable-store/host contract this ADR defines, so `REQ-OPS-3`'s environment-swap requirement is satisfied by
the contract's existence, independent of which runtime eventually implements it.

VOS remains the leading experiment candidate given `PS-66`, but its AGPL-3.0 licensing and sandbox/log gaps must
be resolved — by counsel on the licence, by further evaluation on the gaps — before the experiment's outcome can
become an adoption decision, not before the experiment can run.

## Consequences

- `CMP-STM`'s public contract (frozen at milestone 1 per `REQ-CHN-12`) is runtime-agnostic from day one; no later
  runtime swap requires rewriting the state machine's own logic, only reimplementing the host contract's
  adapter.
- The VOS AGPL-3.0 question is explicitly *not* resolved by this ADR — it is deferred to counsel, tracked
  separately, and gates adoption, not evaluation.
- A milestone-1 experiment (VOS vs. a purpose-built store, fault-injected against the gap list `research/2026-11-02-vos.md`
  names) produces its own one-page outcome (adopt/adapt/drop, per Narthex's experiment convention) and amends
  this ADR; it does not reopen `runtime-host-seam` itself.
