# ADR-012: Staged rollout and independent rebuild

Non-binding. See `README.md`.

## Context

An upgrade can be staged, observed and rolled back, and a failed one leaves the previous version in force (`REQ-MDL-4`). What ran traces to the admitted binary and the platform version that made its machine code, and can be re-derived (`REQ-MDL-2`). A release that widens capabilities takes fresh acceptance (`REQ-MDL-3`). R27 lets acceptance be automated at first, so a signed malicious release is the case in mind (`THR-4`).

## Options

**Staged rollout with an observation period.** Limits a signed bad release, and gives monitors time. Slows urgent fixes.

**Activate at once, with fast rollback.** Simple. Every order sees the bad release first.

**Independent re-derivation.** A second machine or party rebuilds machine code from the admitted binary and compares. Catches a compromised build step (`THR-18`).

**Rebuild from reviewed source.** Would catch more, including a publisher's poisoned build. Hard across languages, and publishers may not share source. Goes beyond `REQ-MDL-2`.

**Catalogue metadata with TUF.** Addresses rollback and freeze (`THR-17`). Adds roles, and says nothing of honesty.

## What we'd try first, and why

Send a small share of new segments to the new version for a stated period, comparing the `REQ-AUD-7` measures with the old one, then widen. Don't mark the old version stale until staging ends: a stale mark moves in-flight segments to the newest version (`REQ-MDL-19`), which skips staging (`THR-16`). That permission is the operator's (`REQ-MDL-5`).

Keep every admitted binary and compiler version. Re-derive on separate hardware after each compiler fix, and on a schedule.

## Consequences

Costs per-provider traffic splitting, and low-volume providers may see too little to judge. An emergency path is needed and is itself a target, so give it its own recorded permission (`REQ-AUZ-6`).

Re-derivation detects only, and only once someone compares. Run by the operator, it is weaker than an outside party, and an outside party pulls against `REQ-OPS-4` if made mandatory.

This doesn't catch an attack inside granted capabilities, or one that stays quiet through the period. A trusted publisher turning malicious stays accepted (`THR-87`, R27).

## Requirements served

`REQ-MDL-2`, `REQ-MDL-3`, `REQ-MDL-4`, `REQ-MDL-5`, `REQ-OPS-3`, `THR-16`, `THR-17`, `THR-18`.

## Sources

- `research/2026-11-02-provider-modules.md`: cosign's verifier bypass checked. Supply-chain cases and candidates are report's tags, some leads only.
- `research/2026-11-02-vos.md`: VOS lacks staging and rollback at the commit read, report's tag.
- R27, R31.
