# F-001 notes

Readings taken where `PLAN.md` is silent (T-001-01), recorded per Narthex `REQ-SKL-24`.

- The seven component crates live under `components/` (`router`, `stm`, `prov`, `core-orch`, `core-authz`, `core-audit`, `evidence`); `contracts/` and `conformance/` sit at the repository root, as `DEC-20` names `contracts/`.
- Package names are `rutent-contracts`, `rutent-conformance`, `rutent-router`, `rutent-stm`, `rutent-prov`, `rutent-core-orch`, `rutent-core-authz`, `rutent-core-audit` and `rutent-evidence`.
- `CMP-EVIDENCE` is read stricter than the plan: `rutent-evidence` depends on nothing of ours but `rutent-contracts`.
- `rutent-conformance` depends on nothing of ours but `rutent-contracts`.
- Each of the three Core crates may depend on `rutent-contracts` and on the four libraries (router, stm, prov, evidence), and not on each other; their internal interfaces are settled later by their own contract task (`PLAN.md` §3 `M3`).
- At crate level, "only through their public contracts" means a Core crate may depend on a library crate; Rust visibility already keeps it off the library's internals, so the check is about the crate graph.
- Nothing depends on `rutent-conformance` except as a dev-dependency, and any crate may take it that way.
- The boundary check counts every dependency kind (normal, dev, build) as an edge; it reads only dependencies named `rutent-*` and ignores external ones, which T-001-04 governs.
- The boundary check lives in the `tools/boundary-check` crate (`rutent-boundary-check`), so `cargo test --workspace` runs it. It reads the workspace members' manifests with `std` alone, no external dependency. It is a tool, not a component, and may depend on nothing of ours.
- The manifest reader fails closed: it reads the plain `[dependencies]`, `[dev-dependencies]` and `[build-dependencies]` tables with bare keys, and reports as an error, never skips, any other form that could name a dependency: `target.` tables, `[dependencies.name]` sub-tables, quoted keys, `package =` renames (inline or dotted) and values that continue past their line. The `members` lookup reads the `members` key inside `[workspace]` only. It reads the manifests with `std` rather than `cargo metadata`, which would need a JSON parser.
- Every crate sets `publish = false` (inherited from `[workspace.package]`), so nothing reaches crates.io by accident.
- Test function names are snake_case forms of the four plan sentences, and each test also runs a fixture graph that breaks its rule, to show the check fails for the right reason.

## T-001-02

Readings taken where `PLAN.md` is silent.

- Documentation check: `missing_docs` at deny level for every crate, through `[workspace.lints.rust]` in the root manifest and `[lints] workspace = true` in each member. A public element without a doc comment, or a library or integration-test crate without a `//!` crate doc, then fails any compile of that crate. CI also runs it as its own step, `cargo doc --workspace --no-deps` with `RUSTDOCFLAGS=-D warnings`, which additionally fails on broken intra-doc links. Public means what rustc's `missing_docs` means: reachable from outside the crate.
- The test fixture is `tools/boundary-check/tests/fixtures/undocumented`, a crate with one undocumented `pub fn`, excluded from the workspace (`exclude` in the root manifest, and its own `[workspace]` root carrying the same lint table). The test `undocumented_public_element_fails_the_check` in `tools/boundary-check/tests/docs.rs` runs the same `cargo doc` command as CI on it, asserts failure and the message "missing documentation for a function", then runs it on a copy with the function documented and asserts success, so the failure is the missing doc and nothing else. A first run, before the fixture had its lints, failed on `workspace.lints was not defined`, and the message assertion caught it as the wrong reason. A second test, `every_workspace_crate_inherits_the_missing_docs_lint`, checks the root denies the lint and every member inherits it, so the fixture's result speaks for every crate.
- The fixture's lint table is a copy of the root's, not the root's own: a fixture outside the workspace cannot inherit from it. The second test pins the root's side.
- Boundary-check tool: no change to its code. Its manifest reader already accepts `[lints]`, `workspace = true` and `[workspace.lints.rust]`; a test (`a_manifest_with_lints_tables_still_parses`) now pins that. The integration test file `boundary.rs` gained a `//!` crate doc, which the lint requires.
- The nested `cargo doc` in the test uses the `CARGO` the outer cargo set and a target directory under the platform temp directory, so it neither locks nor pollutes the workspace target. The fixture's generated `Cargo.lock` is ignored by git.
- Workflow: triggers `pull_request` and `push` to `main` and `m*`; one job on `ubuntu-latest`, `permissions: contents: read`, stable toolchain with `clippy` and `rustfmt`. Pinned actions: `actions/checkout` (v4) and `dtolnay/rust-toolchain` (stable), each by full commit SHA with the tag in a comment; no cache action, to keep the third-party surface at two. Checkout sets `persist-credentials: false`.
- No new dependencies.
