---
status: Accepted (Pablo, 2026-11-06, decision-form items `credential-proxy-model` and `credential-proxy-scope`)
decision_item: credential-proxy-scope (follows credential-proxy-model)
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

**Option 1 now, option 2 before any outside provider joins.** Pablo first picked a shared proxy over this ADR's
original recommendation (`credential-proxy-model`), then, asked about the risk that adds, ruled
(`credential-proxy-scope`): a shared proxy while every provider on the catalogue is the operator's own, and one
credential-proxy process per provider before any provider outside that roster is admitted.

Why this holds against `REQ-MDL-9`: that requirement bounds a compromised *module*. With one OS process per
provider (ADR-0003, accepted), the shared proxy answers each lookup only for the calling process's own provider,
so a compromised module still can't read another provider's secrets. What the shared proxy adds is a
concentration threat: one exploit of the proxy itself, reachable from any provider's outside part, reaches every
provider's secrets (`THR-79`'s "without a bound, at every provider" case). That risk is accepted by the reserved
item `credential-proxy-scope`, which closes it under `REQ-CHN-7`, for as long as the roster is the operator's own.

Pablo's mitigation, in his words: "separating access to different vaults (internal stuff vault buckets, external
integrations' vault buckets) depending on the process role at the deployment level." This ADR records it as
stated; its shape is a deployment matter (`ARCHITECTURE.md` §5), not designed further here.

The switch to per-provider proxies is a precondition of admitting the first provider Bloque doesn't operate. Pablo
puts every provider under Bloque "for now" and outside ones at "V1 and further", and v0 is a first build ahead of
SPEC's v1 (`v0-before-v1`). So the shared proxy is v0's, and per-provider proxies are due before v1, or before
the first outside provider if one is admitted sooner.

Either way, the resolver interface is the same: Vault and OpenBao both implemented
against the narrow resolve-by-name contract, so the interface is proven non-vendor-shaped against two real,
API-compatible but differently-licensed implementations — and an operator who cannot accept Vault's BSL terms for
their own deployment has a fully open drop-in alternative already in hand, not a hypothetical "second resolver."

Per `REQ-MOD-5`, Vault's BSL-1.1 and OpenBao's MPL-2.0 are recorded here as facts about each dependency, not a
legal reading of either; using Vault as an external, operator-run service rutent's proxy talks to (never
redistributing Vault itself) is this session's own non-legal reading of why the BSL's field-of-use restriction
likely doesn't bind rutent's own licensing — stated as a reading, not advice, and worth a quick confirmation from
counsel alongside the other licensing questions already deferred (ADR-0002).

## Consequences

- `THR-79` is narrowed by the process boundary and caller-scoped lookups, but not by construction while the proxy
  is shared: a compromise of the proxy itself reaches every provider's secrets. Accepted by `credential-proxy-scope`
  while the roster is the operator's own; per-provider proxies remove it.
- `THR-80` (credential over-scope) is unchanged by this choice; `OBL-1` still bounds it.
- Rotating a provider's credential needs no new module release (`REQ-MDL-6`'s "rotation needs no new release"
  satisfied directly by the resolve-by-name-at-call-time interface).
- Misuse at an allowed destination, or secret data leaving in a request to one, remains out of scope for this
  ADR, as `suggestions/ADR-003` already stated.
- `CMP-PROV`'s data model (`ARCHITECTURE.md` §4) keys every credential by provider even while the proxy is shared,
  so splitting into per-provider proxies later moves configuration, not data.
