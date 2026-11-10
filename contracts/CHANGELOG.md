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
- After the M1 security review: versions follow semver from 1.0.0, not "1.x.y", since provider-role,
  module-interface and host are now at 2.0.0; a breaking change bumps the major, and every change waits for the
  owner's click. (DEC-4, DEC-20, T-002-03)
- The crate is `no_std` with `alloc`, behind a default-on `std` feature; `serde` and `serde_json` build without their
  default features, with `alloc`. Every contract's source changed, so each takes a minor bump: provider-role 2.2.0,
  module-interface 2.1.0, router-api 2.1.0, state-machine 2.1.0 and host 2.1.0. (DEC-4, DEC-20, DEC-26, T-010-01)
- Guard rules from the `M1` security review: a shared file needs this changelog's entry, not a contract's bump; a root
  `Cargo.lock` change that moves a package the crate reaches needs every contract's bump; a changelog entry naming a
  `DEC-*` that `PLAN.md` does not define fails. (DEC-4, DEC-20, T-010-02)
- The contract changes `M1` routed to `M2`, one bump each: provider-role 2.3.0 (one RFC 8785 canonical JSON
  encoder, `provider_role::canonical`) and 2.4.0 (a signed statement's bytes as received, for the verifier);
  module-interface 2.2.0 (`validate_call_against` and `undeclared_kind`), 2.3.0 (closer refusal reasons) and 3.0.0
  (`delivered` only when the callee answered). No file shared by the contracts changed. (DEC-4, DEC-20, T-010-03)
- A holding account's balance gets its own operation: provider-role 3.0.0 (`holding_balance`) and module-interface 4.0.0
  (`OperationKind::HoldingBalance`, folded into the unreleased major). No file shared by the contracts changed. (DEC-4, DEC-20, T-016-02)
- Guard: a `Cargo.lock` change that only adds or drops dependency edges of a package the crate reaches needs this
  changelog's entry, not every contract's bump; a change to a package's name, version, source or checksum still needs
  every bump. (DEC-4, DEC-20, T-010-02)
- Wasmtime 48.0.8 in `rutent-prov` turns on the `serde` and `allocator-api2` features of crates the contracts reach, which adds dependency edges to `bumpalo`, `hashbrown`, `referencing` and `smallvec` in the root `Cargo.lock`. No contract changed. (DEC-27, T-013-01)
- A sixth contract, `core-internal`, the interfaces between Core's three parts: a line in `CONTRACTS`, `pub mod
  core_internal` in `src/lib.rs`, and the sixth row of this README's table. Its own changelog opens at 1.0.0, and it
  refers to the provider-role, module-interface and state-machine shapes it carries; no other contract changed.
  (DEC-2, DEC-4, DEC-20, T-020-01)
