# ADR-003: The outside part: credentials, resolvers, destinations

Non-binding. See `README.md`.

## Context

The sandboxed part has no outside reach and sees no plaintext (`REQ-MDL-16`, `REQ-MDL-6`, R37). The outside part is the provider's code, under admission rules (`REQ-MDL-1`, R65). The platform offers common secret storage and unsealing (`REQ-MDL-17`). The owner's preferences: the host applies credentials; a common trait with Vault as one resolver (R28); a sidecar through a host extension, relaying its outcome to the extension inbox (R22). That extension host isn't built in VOS at the commit read (ADR-001).

## Options

**Proxy applies the credential to a call the module describes.** Plaintext has one small home, and each call is easy to record (`REQ-MDL-8`). Fits non-HTTP protocols, signed payloads and stateful sessions poorly.

**Outside part unseals and calls directly.** Any protocol. Plaintext sits across all of that code (`THR-83`).

**Host-side signing service.** The key can't leave. It doesn't limit what gets signed.

**Secret in the sandboxed part, with an allow-list.** Ruled out by R37, listed for comparison.

## What we'd try first, and why

One small proxy per provider, counted as that provider's outside part and admitted with it. A single shared proxy would concentrate every provider's secrets, and it reads badly against `REQ-MDL-6` ("unsealable only by its own outside part") and `REQ-MDL-9`.

Keep the resolver interface narrow: resolve by name at call time, so rotation takes no release. Build Vault as the owner proposes, plus one other resolver, so the interface isn't shaped around one product (`REQ-OPS-4`).

Allow-lists deny by default and come from the module's declaration. Check the name and the address actually connected to, again on each redirect (`REQ-MDL-7`). Bypasses reported in Spin, Wasmtime and Deno sit in that gap.

## Consequences

Costs per-protocol support and more processes. Vault itself is in no report, so its licence and fit are unexamined (`REQ-MOD-5`).

This doesn't stop misuse at an allowed host, or data leaving in a request to one (`THR-79`). Only the provider's own scoping limits that (`OBL-1`, `THR-80`), and the report found no survey of who offers it. A credential stolen and used elsewhere stays out of reach (`THR-81`). Module-to-module calls happen only where the route declares them (`REQ-MDL-21`).

## Requirements served

`REQ-MDL-6`, `REQ-MDL-7`, `REQ-MDL-8`, `REQ-MDL-9`, `REQ-MDL-16`, `REQ-MDL-17`, `OBL-1`.

## Sources

- `research/2026-11-02-provider-modules.md`: patterns and bypasses are report's tags, unchecked, some by title only.
- `notes/2026-11-03-threat-model-draft.md`, "For suggestions/".
- R22, R28, R37, R65.
