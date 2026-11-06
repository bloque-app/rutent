# provider-role contract

What a provider advertises and declares to join the catalogue (`F-002`, `T-002-01`). Format-neutral and
language-neutral (`DEC-4`): nothing here assumes a module format. Operations and results per role come in
`T-002-02`.

## Schemas

| `$id` | File | Holds |
|---|---|---|
| `urn:rutent:provider-role:advertisement` | `schemas/advertisement.schema.json` | One provider's advertisement: identifier, publisher, version, offers by category, declarations, private nodes |
| `urn:rutent:provider-role:declarations` | `schemas/declarations.schema.json` | What a provider declares: tracking, and for value-moving providers evidence grade, availability and safe retry |
| `urn:rutent:provider-role:reserve-declaration` | `schemas/reserve-declaration.schema.json` | A reserve's declaration at publishing (`REQ-CAT-6`) |
| `urn:rutent:provider-role:common` | `schemas/common.schema.json` | Shared atoms: identifier, version, amount |

The schemas refer to each other by `$id`. A validator registers all four and needs nothing from the network.
Every fixture is an advertisement, so the other three schemas are exercised through it.

## Shape in brief

- `offers` is keyed by category, `reserve`, `exchange`, `ramp` and `verifier`, with at least one (`REQ-CAT-8`).
- Each role has its own tuple shape (`REQ-CAT-13`):
  - **exchange:** `{node, from, to, terms}`, one inner node by construction. `from` and `to` each name an asset
    and its `decimals`.
  - **ramp:** `{origin, destination, asset, decimals, terms}`, between inner nodes, keeping one asset by
    construction.
  - **reserve:** `{origin, destination, asset, decimals, terms}`. One end is an outer node `{network}` and the
    other an inner node. Outer to inner is a withdrawal; inner to outer is a deposit (`REQ-CAT-17`).
  - **verifier:** `{classes}`, with no tuples (`DEC-16`).
- In ramp and reserve tuples, `decimals` is `{origin, destination}`, the unit at each end. One asset can be counted
  differently on two networks, such as USDC with 6 decimals on one chain and 18 on another (`REQ-CAT-11`).
- The advertisement implies what it would otherwise repeat:
  - An outer node belongs to the advertising reserve, so it names only its network.
  - `private_nodes` lists unique identifiers, and their publisher is the advertisement's `publisher`
    (`REQ-CAT-20`, `REQ-CAT-22`).
- An inner node is an identifier, from the operator's public list or a declared private node.
- `terms` holds:
  - inclusive amount `bands`, as whole numbers of the origin (or `from`) unit;
  - an optional fill `increment`;
  - `reporting`, `gross` or `net`;
  - `required_inputs`, which may be empty but is never left out (`REQ-CAT-11`, `REQ-CAT-12`).
- `declarations` holds:
  - `tracking`, at least one of `identifier` and `status_query`, for every provider (`REQ-CAT-16`);
  - `evidence`, with a `grade` from 1 to 4 on ADR-0007's scale (the grade the provider gives on every outcome,
    so its weakest) and an `availability` in seconds or `"unbounded"` (`REQ-CAT-10`, `REQ-ORD-39`);
  - `safe_retry` (`REQ-CAT-10`).

  `evidence` and `safe_retry` are required once the advertisement has a reserve, exchange or ramp offer. A
  verifier-only provider may leave them out (`DEC-16`).
- A holding account doesn't advertise through this contract (`REQ-CAT-23`, `REQ-ORD-16`). So no reserve tuple runs
  between two inner nodes.
- No shape rules out an exchange resolving orders on a book (`REQ-EXT-2`). An exchange tuple doesn't say how its
  price is made.

## Which rule lives where

A valid advertisement passes the schemas, deserialises into `rutent_contracts::provider_role::Advertisement`, and
gets `Ok` from `provider_role::validate`. Two kinds of rule stay in Rust, because JSON Schema can't express them:
comparing two fields, and comparing a key across list entries.

| Rule | Where | Invalid fixture |
|---|---|---|
| An exchange spans one inner node | schema (`exchange_tuple` has one `node`) | `exchange-spanning-two-nodes`, `exchange-outside-an-inner-node` |
| A ramp keeps its asset | schema (`ramp_tuple` has one `asset`) | `ramp-changing-its-asset` |
| A ramp runs between inner nodes | schema | `ramp-to-an-outer-node` |
| A ramp's two nodes differ | Rust, `RampWithinOneNode` | `ramp-within-one-node` |
| A reserve connects an outer and an inner node | schema (`oneOf`) | `reserve-between-two-inner-nodes`, `reserve-between-two-outer-nodes` |
| An outer node names only its network | schema | `outer-node-naming-its-reserve` |
| A reserve declares both answers, its maker and its version | schema | `reserve-without-declaration`, `reserve-declaration-without-maker`, `reserve-declaration-without-automatic-deposit-answer` |
| A verifier advertises classes, never tuples | schema (`additionalProperties: false`) | `verifier-advertising-a-tuple`, `verifier-without-classes` |
| At least one offer, only the four categories | schema | `advertisement-without-offers`, `offer-under-an-unknown-category` |
| An offer has at least one tuple | schema | `offer-without-tuples` |
| An offer lists each tuple once: ramp and reserve keyed on origin, destination and asset; exchange on node, from and to | Rust, `TupleDeclaredTwice` | `ramp-tuple-declared-twice`, `reserve-tuple-declared-twice`, `exchange-tuple-declared-twice` |
| Each tuple has bands, gross or net, required inputs, and units at each end | schema | `tuple-without-amount-bands`, `tuple-without-gross-or-net`, `tuple-without-required-inputs-declared`, `tuple-without-units-at-its-destination`, `exchange-asset-without-units` |
| Amounts are whole numbers; an increment is above zero | schema | `band-amount-with-a-fraction`, `zero-fill-increment` |
| A band's minimum is at or below its maximum | Rust, `BandMinAboveMax` | `band-with-min-above-max` |
| An identifier or a status query, verifiers included | schema (`minItems: 1`) | `provider-without-identifier-or-status-query`, `verifier-without-identifier-or-status-query` |
| A value-moving provider declares evidence and safe retry | schema (`if`/`then`) | `value-moving-provider-without-evidence-declaration`, `provider-without-safe-retry-declaration` |
| The evidence grade is 1 to 4, its availability is declared, and its seconds fit in 64 bits | schema | `evidence-grade-out-of-range`, `evidence-without-availability`, `evidence-availability-beyond-sixty-four-bits` |
| The provider names its publisher | schema | `provider-without-publisher` |
| A private node is declared once | schema (`uniqueItems`) | `private-node-declared-twice` |
| Integers (`grade`, `seconds`, `decimals`) are written as plain JSON integers, such as `2`, not `2.0` or `1e3` | Rust, when deserialising `Advertisement` | none: JSON Schema's `integer` accepts whole-valued numbers such as `2.0`, and `multipleOf: 1` does too, so only a pattern on the text could reject them, and a schema sees numbers, not text |

Some rules this contract doesn't check, because they need the catalogue (`F-012`):

- that every inner node in a tuple is on the operator's public list or is a declared private node (`REQ-CAT-22`);
- that a declaration is recorded once and shown before acceptance (`REQ-CAT-6`);
- the operator's evidence floor per tuple (`REQ-ORD-39`).

## Tests

`contracts/tests/provider-role.rs` does three things:

- It checks that every valid fixture passes both layers and round-trips through the Rust types unchanged.
- It checks that every invalid fixture is rejected by the layer the table above names. A schema rejection must give
  exactly the expected set of (location, keyword) errors, and a Rust rejection must give exactly the named
  violation.
- It checks that `VERSION` matches this changelog.
