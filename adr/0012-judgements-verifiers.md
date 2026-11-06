---
status: Accepted (Pablo, 2026-11-06, decision-form item `judgements-verifier-model`)
decision_item: judgements-verifier-model
supersedes: adopts suggestions/ADR-008-judgements.md
---

# ADR-0012: Judgements and verifier integration

## Context

`REQ-CPL-1/4/10` require a judgement to state maker, subject reference, class, time and validity; to be
non-forgeable and non-repudiable; and to let each provider know a person under a *different* subject reference,
so only the operator can join them across providers. `suggestions/ADR-008` proposes a short-lived signed
judgement per class, carried by the holder's key, with **one subject reference per provider** (never one global
reference) specifically so the operator can't join a person's activity across providers and nobody else can
catch one person fronting across providers.

Two access gaps affect this ADR directly, found by `research/2026-11-02-compliance-proofs.md` and
`research/2026-11-03-verifier-privacy.md` and not resolved by this session: the current FATF Recommendations
(English) were never read — every FATF claim in both reports rests on a 2018/2019 Spanish edition or 2013 OSCE
slide deck, because the FATF site returned a bot challenge on every attempt; and Mexico's "facilitation" reading
(whether rutent's "instructs only" operator posture survives under Mexican law) rests on one law firm's secondary
account of the DOF text, not the text itself. Per this project's own sourcing rule ("legal claims need counsel,
never inference"), neither gap is resolved here. Recording them: this ADR is adopted provisionally, flagged as a
named risk in `ARCHITECTURE.md` §6, rather than blocked on primary-source access this session cannot get past.
Whether the `reg-fintech` profile escalation (`R8`) is ever triggered is a STEP-1 profile matter this ADR doesn't
reopen.

## Options considered

1. **Per-check verifier call**, always fresh, no stored judgement. Lets the verifier see and potentially join
   every check a person makes across providers — the exact correlation risk `REQ-CPL-10` is meant to prevent.
2. **Unlinkable proofs / BBS / zero-knowledge credentials.** Immature; no regulated production use found by
   either report for KYC/eligibility specifically.
3. **Public-chain attestation.** Publishes the wallet-to-identity link on-chain, with no status/revocation server
   — worse privacy than the option chosen, not better.
4. **A short-lived signed judgement, per class, carried by the holder's key, with one subject reference per
   provider.** Plain signed token first; selective disclosure (SD-JWT, RFC 9901) added only if judgements grow
   more fields. No status callback (a callback would itself tell the issuer who's checking) — expiry substitutes
   for revocation. Where nobody holds a key, falls back to provider-vouching or a per-check call, which
   reintroduces option 1's risk in that fallback case only.

## Decision

Option 4, allocated to `CMP-CORE-AUTHZ` (judgement as a policy input, `REQ-CPL-3`) with the verifier role contract
itself living in `CMP-PROV`. RFC 9901 §10.1's own text (read at source, confirmed by this session's earlier
research fork) states plainly that issuer-to-verifier unlinkability cannot be achieved against a colluding or
coerced verifier for SD-JWT-style credentials — so the per-provider subject-reference design is this ADR's actual
privacy mechanism, not the SD-JWT format itself, which is deferred until judgements grow enough fields to need it.

## Consequences

- A yes outlives the underlying facts until expiry — `REQ-CPL-1`'s validity window is the only check against a
  stale judgement; there is no revocation mechanism by design.
- Whether a bare judgement of this shape satisfies any *specific* provider's own legal compliance duties is
  explicitly deferred — an engineer's reading, not legal advice, per `suggestions/ADR-008`'s own caveat.
- The Mexico-facilitation and FATF-primary-source gaps above are **not resolved by adopting this ADR** — they are
  named risks carried into `ARCHITECTURE.md` §6, to be closed by counsel independently of whether this ADR's
  technical design is accepted.
- `REQ-CPL-12` (loss from a false judgement falls on the relying provider, not the operator or customer) is
  unaffected by this ADR's mechanism — it's a liability allocation this design doesn't change, only records.
