# rutent

A funds and assets router. rutent moves value between any networks and assets its catalogue of providers can reach, never holds customer value, and keeps an authenticated log of who did what, when, and with what outcome.

rutent is a Bloque open-source project, succeeding one of Bloque's existing services.

## Status

Requirements only. `SPEC.md` v0.4.1 was accepted by the project's owner on 2026-11-05, which ends the specification step. Nothing is built yet; design comes next, in the open.

## Read in this order

1. [`OVERVIEW.md`](OVERVIEW.md): rutent in a page.
2. [`SPEC.md`](SPEC.md): the requirements, invariants, acceptance criteria and threat model. It decides.
3. [`INTEGRATORS.md`](INTEGRATORS.md): what a product built on rutent does around an order.
4. [`PRODUCT.md`](PRODUCT.md): every requirement's source, in the owner's own words.

Then, for how it could be built:

- [`suggestions/`](suggestions/): non-binding design records. They propose; `SPEC.md` decides.
- [`research/`](research/): the reports behind the suggestions. Each says which of its claims were checked.
- [`profile.oss-ledger.md`](profile.oss-ledger.md): the sector profile the specification was checked against.

## Conventions

- MUST, MUST NOT, SHOULD, SHOULD NOT and MAY carry their RFC 2119 meaning, and appear only in numbered tables.
- Identifiers are stable: `REQ-*` requirements, `INV-*` invariants, `AC-*` acceptance criteria, `THR-*` threats, `PS-*` the owner's statements.
- `R` numbers (R1 to R143) are the owner's rulings. The rulings themselves, the decision form and the working notes are kept in the project's private design repository, so a few citations name files that aren't here (`notes/…`, `decisions/…`).

## Licence

MIT. See [`LICENSE`](LICENSE).
