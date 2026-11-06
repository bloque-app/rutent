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
