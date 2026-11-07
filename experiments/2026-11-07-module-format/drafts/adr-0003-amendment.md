---
Status: experimental
amends: adr/0003-module-format-sandbox.md
experiment: EXP-2
---

# Draft: amendment to ADR-0003, the admitted module format

A draft of `EXP-2` (Narthex `REQ-EXP-3`). It is not accepted and not part of ADR-0003 until promotion consolidates
it (Narthex `REQ-EXP-5`). It decides the format ADR-0003 left to the experiment, and keeps the process boundary
ADR-0003 made firm (`DEC-7`). `T-008-04` writes it from the [outcome](../outcome.md); until then every section is
empty.

## Context

To be written by `T-008-04`.

## Outcome of EXP-2

To be written by `T-008-04`: the format or formats admitted, citing the outcome page and the run ids behind each
number.

## Decision

To be written by `T-008-04`.

## Consequences

To be written by `T-008-04`, including what changes for `THR-85` and for `AC-20`.

## Points the amendment must reconcile

- ADR-0003 says PolkaVM is Apache-2.0. PolkaVM's manifest at `v0.38.0` says `MIT/Apache-2.0`, and its README offers
  either licence at the user's option. The amendment states the licence as the source states it.
- ADR-0003 says PVM has one documented guest language. At `v0.38.0` its tree also carries a C guest header, with no
  C guest program or test; the amendment states what the runs found (`G-22`).
