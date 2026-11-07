# contracts crate changelog

Changes to files under `contracts/` that belong to no single contract. Each contract keeps its own changelog in its
directory (see `README.md`).

- Contract layout, versioning and changelog conventions; `serde`, `serde_json` and `jsonschema` pinned for every
  contract. (DEC-3, DEC-4, DEC-5)
- Contract test files: `contracts/tests/<contract>.rs` belongs to that contract, so the guard counts it with the
  contract. (DEC-4, T-001-05)
- Guard bypasses closed: quoted and non-ASCII paths, `CONTRACTS` re-mapping from the base as well as the head,
  decision lookalikes such as `NODEC-4`, and heading-only bumps. (DEC-4, T-001-05)
- `src/lib.rs` exposes the router-api module. (DEC-4, T-004-01)
- A contract listed in the base `CONTRACTS` with no files at the base is new, so its first version needs no bump;
  one that owns base files still needs a base changelog. (DEC-4, T-001-05)
- Stream fixtures in JSON Lines (`.jsonl`) for shapes that travel as a stream. (DEC-4, T-005-02)
- `lib.rs` exports the `module_interface` module, for the module-interface contract. (DEC-4, T-003-01)
- `C1`: all five contracts are at 1.0.0, so versions are now 1.x.y, a breaking change bumps the major, and every change
  waits for the owner's click. Fixtures name 1.0.0 where they name a contract's own version. (DEC-4, DEC-20, T-006-01)
