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

## T-001-03

Readings taken where `PLAN.md` is silent, recorded per Narthex `REQ-SKL-24`.

- Both checks live in one std-only crate, `tools/records` (`rutent-records`), with two binaries, `tag-check` and `plan-check`, over a shared reading of which identifiers the documents define. Two crates would have needed a third for that shared code or a copy of it. Each binary takes `--strict` (exit 1 on a finding; the default is report mode, exit 0) and `--root DIR` (default the current directory). The crate adds no dependency.
- Defined identifiers: the first cell of a table row when it is exactly one identifier (`SPEC.md`, `PLAN.md`, `PRODUCT.md`); backticked identifiers in a heading (this gives `EXP-1`, `EXP-2`, `C1` and `M1` to `M5`, which no table row defines); `ADR-NNNN` from each ADR's title line. Patterns (`DEC-*`) and placeholders (`EXP-n`) are not identifiers.
- `PRODUCT.md` is read too, although the task names only `SPEC.md`, the ADRs and `PLAN.md`: `T-007-03` serves `PS-66`, which only `PRODUCT.md` defines (`PLAN.md` §1 says `PS-*` are `SPEC.md`'s and `PRODUCT.md`'s). Without it the real plan fails its own check on a correct citation.
- Withdrawn identifiers come from `SPEC.md` prose, never from table rows (a threat row may talk about a yes being withdrawn): "Number N in this family is withdrawn" under a `` ### `PREFIX-*` `` heading withdraws `PREFIX-N` (today `REQ-AUZ-10`); a prose line "`ID` is withdrawn" does too. A withdrawn id is not a defined one, and the tag check reports it as withdrawn, not unknown.
- Narthex's own identifiers (`Narthex `REQ-CHN-12``) are skipped in a task's Serves cell, since no document in this repository defines them. An unbackticked `ADR-0002` in a Serves cell counts (`T-007-04`).
- A test is a line whose first token is `#[test]`. Its tag is the `// verifies:` comment directly above it, skipping `#[...]` attribute lines and `///` doc comment lines in between; a tag may continue on `//` lines beneath it. A blank line between the comment and the test breaks the link. Ids are separated by commas or spaces. A word that is not identifier-shaped is reported as unknown. Other test attributes (`#[tokio::test]`) are not read; the workspace has none.
- The tag check walks every `.rs` file under the root, skipping `target` and hidden directories.
- The plan check reads every table whose header starts `Task` and has `Depends on` and `Serves`; the feature tables give each feature its milestone (the Milestone column in §4.2, else the single `M<n>` in the section heading, which is how `F-001` to `F-008` get `M1`). A task's feature comes from its identifier (`T-005-02` is `F-005`). A task with no known milestone, a dependency that is not a task, and a task missing from or twice in the waves are findings.
- "Waves of §5" is read as: each task is in exactly one wave and in a later wave than every task it waits on. The check does not require the smallest possible wave number, and it does not read the Mermaid diagram.
- "Serves at least one defined identifier" is read as: at least one non-Narthex id that is defined, and no id that is unknown or withdrawn (a typo in Serves is reported even when another id is fine).
- `HOUSE-RULES.md` records the owner's ruling `construction-autonomy` (2026-11-06) and says it overrides the per-task pull requests of `DEC-20` and the two-helper cap of `DEC-22` until `PLAN.md` is amended. It also says no agent merges a pull request, which is this task's reading of "the orchestrating session integrates each task"; the merge method of the milestone pull request is left to the owner. The `DEC-20` paths are listed as directories where the plan names a component (`components/core-authz/`, `components/core-audit/`); the credential proxy, resolvers and admission code have no directory yet, so they are listed by name.
- The CI workflow `.github/workflows/records.yml` runs `cargo run --locked`, so it needs `Cargo.lock` committed; it uses only the runner's own Rust toolchain and `actions/checkout`, pinned to a full commit SHA (`v7.0.1`).
- Finding about the real `PLAN.md`: none. With `PRODUCT.md` read, the plan check passes strictly (0 findings), and the tag check finds no untagged test or bad id in the workspace.
- The three fixture tests that pass before the checks exist (a consistent plan, tags with attributes between, the real plan) pass vacuously against the stub; the eight negative tests were seen failing first because the stub reported nothing.
## T-001-04

Readings taken where `PLAN.md` is silent, recorded per Narthex `REQ-SKL-24`.

- The record is `licences/RECORD`, whitespace-separated: `name version licence... decision reference`. The licence is the middle field and may hold spaces; the reference is one token with no spaces (`DEC-6,ADR-0004`). A line with fewer than five fields, or an unknown decision, is an error, not a pass.
- A `not-adopted` version marks an entry for a dependency not yet in `Cargo.lock`. The check never fails on it, and `not-adopted` lines are not matched against the lockfile. VOS (`AGPL-3.0-or-later`) and Vault (`BUSL-1.1`) carry it.
- VOS carries `DEC-6,ADR-0004`. Vault carries `DEC-9,ADR-0004`: `PLAN.md` ties Vault to `DEC-9`, and the coordinator corrected the brief's `DEC-6` for it.
- Three declared licences used `/` (`Apache-2.0/MIT`, `MIT/Apache-2.0`), which is not SPDX. They are recorded as `Apache-2.0 OR MIT` and `MIT OR Apache-2.0`. Other expressions are copied from `cargo metadata` as declared; `r-efi` declares `LGPL-2.1-or-later` as one `OR` alternative and the record keeps it.
- The reference `licence-permissive-standing` appears in no tracked plan document; it is used as the brief directs for every locked package.
- The check does not flag a record line whose version Cargo.lock no longer holds. The brief names two failure modes only.
- A locked package is external unless it has no `source` and its name is a workspace member. Members come from the root manifest's `members` array; each member's `[package]` name and version are read from its manifest, and an inherited version (`version.workspace = true`) is resolved from the root's `[workspace.package]`. A locked package matches a member only on both name and version, so `rutent-contracts 0.2.0` against a member at 0.1.0 is external. The array is read in plain form only; a glob, a commented member line or any other syntax is an error, so no member is silently missed and no external package is silently skipped.
- A second record line for the same name and version is an error, not a first-match lookup. A permitted line placed above a pending one would otherwise pass.
- The workspace member `rutent-licence-check` adds one source-less entry to `Cargo.lock`. Workspace members are not recorded.
- CI pins `actions/checkout` to the SHA of its latest release tag, `v7.0.1`, resolved with `gh api`.
## T-001-05

Readings taken where `PLAN.md` is silent, recorded per Narthex `REQ-SKL-24`.

- The guard is the `tools/contract-guard` crate (`rutent-contract-guard`), with no dependencies. Its core is a pure function over the changed paths, the `CONTRACTS` text and a base and head lookup; `main` reads git and runs it. Tests use in-memory fixtures and create no commits.
- A path belongs to the contract whose artefact or module directory is its longest matching prefix, matched on whole path segments. `contracts/state-machine/host/...` and `contracts/src/state_machine/host/...` belong to `host`.
- A contract's version is the first `## ` heading of its changelog, and it must be plain `MAJOR.MINOR.PATCH`. A changed contract needs a greater head version than the base's and a top section with at least one bullet. A changelog missing at the base means the contract is new, and then only the bullet rule applies.
- A bullet is a `- ` or `* ` line with its continuation lines. A bullet is new when its text is absent from the base file. Every new bullet in any changelog must contain `DEC-<digits>`. The guard checks the form only; it does not confirm that the decision exists in `PLAN.md`.
- A change to a shared file under `contracts/` needs a new `DEC-*` bullet in `contracts/CHANGELOG.md`, or a version bump of some contract in the same change. A new contract does not count as a bump.
- Changed paths come from `git diff -z --name-only --no-renames <merge-base> HEAD`, where the merge base is `git merge-base <base> HEAD`. Names are NUL-separated, so git's quoting of non-ASCII names cannot hide a path. Base text is read with `git show <merge-base>:<path>`, so the diff and the base agree.
- The base is `origin/<base_ref>` for a pull request, `github.event.before` for a push, and `origin/main` for a new branch. A missing base ref is a usage error, not a pass.
- A file directly under `contracts/tests/` whose stem is a contract name, with `-` or `_`, belongs to that contract (`provider_role.rs`, `host.rs`). Any other file there stays shared. This settles the point an earlier reading left open.
- A path touches every contract that either the base or the head `CONTRACTS` assigns it to; the base file is read with `git show`. A contract absent from the base `CONTRACTS` is new. A contract that the base lists but whose base changelog is missing is an error, not a new contract.
- A contract listed in the base `CONTRACTS` with no file at the base is new, and its first version needs no bump. `CONTRACTS` lists all five contracts from the start, so without this rule every first version failed the guard. Base files are listed with `git ls-tree -r -z`. A contract that owns base files but has no base changelog is still an error.
- A new top heading alone is not a bump: its section must carry a bullet the base's top section lacks.
- The guard runs on a push against `github.event.before`, and on a new branch (all-zero `before`) against `origin/main`. The value reaches the shell through `env:` only.
- `parse_contracts` rejects two contracts whose artefact or module directories are equal. Nesting stays legal, because owners are found by longest prefix.
- A bullet names a decision only when `DEC-` is standalone (no letter, digit or underscore before it), digits follow, and the digits end the token. So `(see DEC-4x)` and `_DEC-4` name none.
