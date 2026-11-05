# ADR-008: Judgements and verifiers

Non-binding. See `README.md`.

## Context

A question to a verifier carries a person and a class only (`REQ-CPL-4`, R33). A user or agent may present, and providers may vouch (`REQ-CPL-7`, R34). Only shortlisted providers are asked (`REQ-CPL-6`, R35). A judgement is usable only for its person and class, within its validity, by someone with authority (`REQ-CPL-1`). Each provider knows a person under a different reference, and only the operator joins them (R107).

## Options

**Verifier called per check.** Always fresh. Sees every check, and joins them.

**Time-limited judgement carried by the person.** Verifier is contacted at issuance only. Stale until it expires, and needs a holder with a key.

**Selective disclosure (SD-JWT).** RFC 9901. With a person and a class there is little to hide. Presentations link unless issued per use. Against a colluding checker, issuer unlinkability "cannot be achieved" in that scheme.

**Unlinkable proofs (BBS, zero-knowledge).** Claim to remove linking. Drafts or young libraries, and no regulated production use was found, in limited searching.

**Public-chain attestation.** No status server. Publishes the link between wallet and verified person.

## What we'd try first, and why

A short-lived signed judgement per class, carried by the person or agent, bound to the holder's key, and presented with audience and deployment named (`REQ-PRT-1`). A plain signed token first, adding selective disclosure only if judgements grow more fields. No status callback, since a callback tells the maker who checked; expiry stands in for revocation. Where nobody holds a key, a provider vouches or a per-check call remains.

Issue one reference per provider.

## Consequences

A yes outlives the facts until it expires (`THR-93`, accepted under R64). No source states the right lifetime. Issuance adds load, and shows the verifier a cadence.

Per-provider references stop the operator joining a person's activity, and also stop anyone catching one person fronting (`THR-48`, `THR-92`). Their issuer can still reverse them.

This doesn't catch a false yes (`THR-91`) or remove what a verifier learns by being asked (`THR-42`). Whether a bare judgement meets any provider's own duties is an engineer's reading, not legal advice, and stays deferred (R79, `THR-95`). Nothing here states law.

## Requirements served

`REQ-CPL-1`, `REQ-CPL-4`, `REQ-CPL-6`, `REQ-CPL-7`, `REQ-CPL-10`, `REQ-PRT-1`.

## Sources

- `research/2026-11-03-verifier-privacy.md`: RFC 9901 section 10.1 and OpenID Connect section 8 checked. Absence findings are limited searches; FATF reading rests on a 2013 deck.
- `research/2026-11-02-compliance-proofs.md`: format trade-offs are inference.
- R33, R34, R35, R64.
