# contracts crate changelog

Changes to files under `contracts/` that belong to no single contract. Each contract keeps its own changelog in its
directory (see `README.md`).

- Contract layout, versioning and changelog conventions; `serde`, `serde_json` and `jsonschema` pinned for every
  contract. (DEC-3, DEC-4, DEC-5)
- Contract test files: `contracts/tests/<contract>.rs` belongs to that contract, so the guard counts it with the
  contract. (DEC-4, T-001-05)
- Guard bypasses closed: quoted and non-ASCII paths, `CONTRACTS` re-mapping from the base as well as the head,
  decision lookalikes such as `NODEC-4`, and heading-only bumps. (DEC-4, T-001-05)
