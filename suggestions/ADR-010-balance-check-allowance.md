# ADR-010: Balance-check allowance

Non-binding. See `README.md`.

## Context

A balance check answers yes or no. Other callers' checks share one bound per account, the holder's own order doesn't count, and thresholds are free (`REQ-CAT-3`, `INV-9`, R20). Anything revealing sufficiency counts as a check (`REQ-CAT-4`). Since R96, checks happen only inside a user's or delegate's session, so most of this record no longer applies. It is kept for a later open protocol.

## Options

**Flat allowance.** Every outside check costs the same. Simple. A patient asker recovers a balance at the same cost as an honest one.

**Allowance that charges unmatched checks more.** "Charging" means a larger debit against the allowance, not money. Unmatched means no order at that amount from that asker. One lead for this shape: a participant's developer documents on a real rail's key directory, where failed lookups cost extra past a threshold. Search excerpt only.

**Check only for a real order at its amount.** Closes free thresholds. Runs against R20.

**Fixed coarse thresholds.** Bounds information per check. Also against R20.

## What we'd try first, and why

One bucket per account, kept by the platform rather than the module. Debit the higher rate at check time, and refund the difference when a matching order follows within a stated window. Dry-runs and refusals that reveal sufficiency debit the same way. Limits also bind at the accountable party (`REQ-AUZ-8`). This keeps R20 as ruled and makes probing slower than honest use.

## Consequences

The rates and window need tuning, and no report has data for them. Refunds need a link from check to order, and the log of thresholds is itself sensitive.

An asker can open and drop orders to earn refunds, so those count as work too (`REQ-OPS-1`, `THR-115`). One asker can use up an account's shared bound and block other callers; the holder's own orders stay exempt (`THR-54`).

This slows inference and doesn't stop it (`THR-38`, accepted under R20). The report's figure of about 40 answers is inference. Timing leaks remain (`THR-41`).

## Requirements served

`REQ-CAT-3`, `REQ-CAT-4`, `REQ-AUZ-8`, `INV-9`, `THR-39`, `THR-40`.

## Sources

- `research/2026-11-02-compliance-proofs.md`: Pix rule articles are report's source tag; the token bucket is a lead. Balance recovery on Lightning is measured by its authors on testnet, unchecked. Not legal advice.
- `notes/2026-11-03-threat-model-draft.md`, "For suggestions/".
- R20.
