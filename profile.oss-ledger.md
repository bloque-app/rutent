---
name: oss-ledger sector profile (project-local stand-in)
status: provisional. Narthex lists oss-ledger as a candidate with no data file yet (OQ-4, REQ-DAT-18)
source: Narthex SPEC v0.3.0 §7.1 (candidate profiles), §6.3, §6.4, §6.6
date: 2026-11-02
---

# `oss-ledger`, as `rutent` uses it

Narthex defines this profile in one informative row: "`product`, plus conservation and atomicity invariants
with property or simulation tests; project guarantees split from operator obligations; an ISO 20022 mapping
where relevant". `product` adds "RFC 2119 prose; acceptance criteria as scenarios; linked OpenAPI; scope as a
pitch". The table below fills in the `REQ-DAT-18` elements for `STEP-1` only. It is not a Narthex artefact.

| `REQ-DAT-18` element | For `rutent` | Source |
|---|---|---|
| Required sections | Purpose and scope; glossary; parties; requirements; invariants; acceptance criteria; NFRs; threat model; project guarantees vs operator obligations; assumptions; deferred scope; open questions | §4.1, §7.1 |
| Identifier families | `PS-`, `REQ-`, `INV-`, `AC-`, `NFR-`, `THR-`, `OQ-`, `DEF-`, `ASM-`, plus `GUA-` (project guarantee) and `OBL-` (operator obligation), which this profile adds under `REQ-DAT-9` | §6.3 |
| Mandatory attributes | `REQ-`: RFC 2119 statement, source link, verification method. `INV-`: statement, at least one check. `AC-`: scenario, the requirement it accepts. `NFR-`: measure and bound. `THR-`: category, mitigation or accepting decision. `OQ-`: its decision item | §6.3 |
| Notation checks | RFC 2119 keywords only in numbered items and tables marked normative | §0 |
| Trace rules | Each requirement `derives-from` a `PS-`, another requirement, a threat or a profile obligation | `REQ-CHN-6`, §6.4 |
| Normative artefact | `SPEC.md` | `REQ-DAT-1` |
| Exports | None yet | |
| Gates and evidence | The `STEP-1` gate: every §4.3 element present or an item; every requirement traced; every threat mitigated or accepted; the lead accepts on the form | §4.1 |
| Standards cited | STRIDE (`REQ-CHN-7`, no edition); RFC 2119; ISO 20022 where relevant, edition to pin when one is cited (`REQ-KNB-6`) | |

## Obligations this profile adds

These bind `SPEC.md` directly and trace as "profile obligation":

- `oss-ledger-1`: conservation and atomicity invariants, each with a property or simulation test as its check.
- `oss-ledger-2`: project guarantees split from operator obligations.
- `oss-ledger-3`: an ISO 20022 mapping where relevant. Not in v1: deferred (R126, `DEF-5`).

## Not covered

`STEP-2` to `STEP-5` gates and evidence, because this project stops at `STEP-1`. The neighbouring `reg-fintech`
candidate ("`oss-ledger`, plus a PCI DSS and SOC 2 evidence map; a third-party register; validation independence
for models") is not applied. The owner's ruling (R8): add it "if regulation demands".
