---
status: Proposed
decision_item: credential-proxy-model (pending)
supersedes: adopts suggestions/ADR-003-outside-part.md
---

# ADR-0004: Outside part — credential proxy and destination resolution

## Context

`REQ-MDL-6/7/8/9` require: the sandboxed part of a module never sees secret plaintext; the outside part reaches
only declared destinations; every outbound call is recorded against an identified transaction; a compromised
module's reach is bounded and stated. `suggestions/README.md` flags as explicitly open whether a *shared*
credential proxy (one process holding every provider's secrets) could ever satisfy `REQ-MDL-6/9` — and
`suggestions/ADR-003` answers by never building that case: one small credential-proxy process **per provider**,
admitted alongside that provider, so there is nothing shared to concentrate.

`suggestions/ADR-003` named Vault as the resolver and proposed building a second resolver "so the interface isn't
vendor-shaped." Checking this directly (this session, 2026-11-06): HashiCorp Vault moved from MPL-2.0 to the
Business Source License 1.1 in August 2023 and has stayed there; BSL is not OSI-approved open source and
restricts offering the software as a competing product (though it does not restrict *using* a Vault instance an
operator runs themselves as an external secret store rutent's proxy talks to — the restriction binds whoever
redistributes Vault itself, not whoever integrates with one). **OpenBao**, the Linux Foundation's community fork
of Vault created in response to that relicensing, is MPL-2.0 and API-compatible with Vault's KV interface — this
is a ready-made, genuinely open second resolver, not a hypothetical one.

## Options considered

1. **Shared credential proxy** (the open question `suggestions/README.md` names). Rejected again here: it
   concentrates every provider's secrets in one process, directly against `REQ-MDL-9`'s bounded-reach
   requirement.
2. **One small credential-proxy process per provider**, admitted together with that provider; a narrow interface
   (resolve-by-name at call time, no release needed for credential rotation); destination allow-lists deny-by-
   default from the module's own declaration, checked by name and connected address, re-checked on every
   redirect.
3. **Outside part unseals and calls directly**, no proxy. Rejected: plaintext secrets then spread across every
   provider's own outside-part code rather than staying in one audited narrow interface.
4. **Host-side signing service** that signs on the module's behalf without handing over the secret. Rejected:
   limits nothing about *what* gets signed, only *who* holds the key — doesn't address `REQ-MDL-7`'s
   destination-scoping requirement.

## Decision

**Still Proposed, not ruled.** Pablo's first answer on the decision form picked option 1 (shared proxy) over
this ADR's recommendation. Per `REQ-CHN-7`, a threat a design introduces must be mitigated by a requirement or
accepted by a reserved item — a shared proxy's residual risk (one process reachable from any provider's outside
part, holding every provider's secrets) isn't yet named as an accepted risk anywhere, so this ADR stays open
pending a sharper follow-up item (`credential-proxy-model-scope`) that states that risk plainly and asks again,
rather than treating the first answer as final. This text otherwise still describes the recommended design,
option 2, below.

One credential-proxy process per provider. Resolver interface: Vault and OpenBao both implemented
against the narrow resolve-by-name contract, so the interface is proven non-vendor-shaped against two real,
API-compatible but differently-licensed implementations — and an operator who cannot accept Vault's BSL terms for
their own deployment has a fully open drop-in alternative already in hand, not a hypothetical "second resolver."

Per `REQ-MOD-5`, Vault's BSL-1.1 and OpenBao's MPL-2.0 are recorded here as facts about each dependency, not a
legal reading of either; using Vault as an external, operator-run service rutent's proxy talks to (never
redistributing Vault itself) is this session's own non-legal reading of why the BSL's field-of-use restriction
likely doesn't bind rutent's own licensing — stated as a reading, not advice, and worth a quick confirmation from
counsel alongside the other licensing questions already deferred (ADR-0002).

## Consequences

- `THR-79`/`THR-80` (module network egress, credential over-scope) are narrowed by construction: there is no
  shared process whose compromise reaches every provider's secrets.
- Rotating a provider's credential needs no new module release (`REQ-MDL-6`'s "rotation needs no new release"
  satisfied directly by the resolve-by-name-at-call-time interface).
- Misuse at an allowed destination, or secret data leaving in a request to one, remains out of scope for this
  ADR, as `suggestions/ADR-003` already stated.
- `CMP-PROV`'s data model (`ARCHITECTURE.md` §4) carries one credential-proxy declaration per provider, not one
  shared configuration.
