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
- The guard follows references: a contract whose schemas name another's URNs needs a major bump when that one's
  major moves, since its shapes change through the reference though none of its paths do. (DEC-4, DEC-20, T-020-01)
- The guard also follows Rust uses: a contract whose module uses another's (`crate::<module>`, re-exports included)
  needs a major bump when that one's major moves, and a changelog heading that isn't a version is refused even when
  its contract is untouched (the review of `T-020-01`). (DEC-4, DEC-20, T-020-01)
- The guard expands a use's braces at every depth and reads `super::` from the file's own module, so the host's
  `use super::...` counts as a use of the state machine's module (the round-3 review's low 6). (DEC-4, DEC-20,
  T-020-01)
- core-internal's next round, folded into its unreleased 1.0.0 (`T-020-21`): a notice's summary, a public end's
  mismatch and its raise, the checkpoint and checkpoint-opening schemas, a decline of an outcome report on its own
  call, an arrival's earlier arrival, an operation that ended with no answer, the holding account's record's node
  and label, a late status asked as a decided action, a stuck part's evidence, a charge no formula covers, and rows
  for a decision's caller, a statement's role and an arrival answer's question; router-api's `Operation::stated_out`
  documents its `None`, folded into its unreleased 4.0.0. Each contract's changelog names the changes. (DEC-4,
  DEC-20, T-020-21)
- core-internal's `outcome-confirmation-cites-verified-evidence` accepts, for a hand-on, a statement about the linked
  order the decision named through the transaction of the effect's own withdrawal, and holds a report to the holding
  account's own maker, so the holding account's own answer can be the evidence of a hand-on's outcome; folded into the unreleased 1.0.0 (`T-020-26`). (DEC-4, DEC-20, T-020-26)
- core-internal's round after `T-020-20`, folded into its unreleased 1.0.0 (`T-020-30`): a stuck resolution's
  evidence entry required, a report pair's tuple terms, a measure record's role, a replay's list of receipts, a
  deliver's report on its provider's own word, and a verifier's grant of a class. Its changelog names each. No file
  shared by the contracts changed. (DEC-4, DEC-20, T-020-30)
- core-internal's round after `T-020-30`, folded into its unreleased 1.0.0 (`T-020-36`): a publisher's record of a
  node's segments verified about an order through its segments' transactions, the record naming its node and held to
  that node's publisher and to one per node, a late status naming its fresh transaction, and a route handed on started on the
  holding account the hand-on named, whoever runs its first move. Its changelog names each. No file shared by the contracts changed. (DEC-4, DEC-20, T-020-36)
- core-internal's round after `T-020-36`, folded into its unreleased 1.0.0 (`T-020-38`): each report of a pair names
  its own segment, the received one the route's next after the sent one, each by the provider the log moved its
  segment with. Its changelog names each. No file shared by the contracts changed. (DEC-4, DEC-20, T-020-38)
- The contract round for the owner's answers of 2026-11-18 (`T-020-39`): core-internal's roll back, a confirmer's
  evidence from a party to the order, an erasure overriding its citations, an acceptance given on a deposit and the
  rule on a statement received after its grants' revocation, folded into its unreleased 1.0.0; provider-role's
  deposit payouts, folded into its unreleased 3.1.0; router-api's `batched_payout`, folded into its unreleased 4.0.0.
  Each contract's changelog names the changes. No file shared by the contracts changed. (DEC-4, DEC-20, T-020-39)
- core-internal's quote awaiting its customer (`T-020-41`): `shown` gains an optional `awaiting` and a notice's summary may be of a waiting order (`NoticeOf::Waiting`), folded into its
  unreleased 1.0.0. Its changelog names the change. No file shared by the contracts changed. (DEC-4, DEC-20, T-020-41)
