# rutent contracts

The contracts every later module and runtime is written against (`PLAN.md` `DEC-3`, `DEC-4`, `DEC-5`). They
freeze at `C1` (`T-006-01`); from then on a change needs a version bump, a changelog entry naming a `DEC-*`, and the
owner's click (`DEC-20`).

## The five contracts

All five live in one crate, `rutent-contracts`. Each has a directory for its language-neutral artefacts and a Rust
module for its types. The host contract sits inside the state-machine package (`DEC-3`) but has its own version and
changelog.

| Contract | Artefacts | Rust module | Feature |
|---|---|---|---|
| `provider-role` | `contracts/provider-role/` | `contracts/src/provider_role/` | `F-002` |
| `module-interface` | `contracts/module-interface/` | `contracts/src/module_interface/` | `F-003` |
| `router-api` | `contracts/router-api/` | `contracts/src/router_api/` | `F-004` |
| `state-machine` | `contracts/state-machine/` | `contracts/src/state_machine/` | `F-005` |
| `host` | `contracts/state-machine/host/` | `contracts/src/state_machine/host/` | `F-005` |

`contracts/CONTRACTS` lists the same mapping in a form tools read. A path belongs to the contract whose artefact or
module directory is its longest matching prefix, so `contracts/state-machine/host/...` is the host contract's, not
the state machine's. The guard takes a path as touching every contract that the base or the head `CONTRACTS` assigns
it to, so a re-mapping cannot move a file out from under its contract's bump.

## Inside a contract's directory

```
contracts/<contract>/
  CHANGELOG.md                 the version and its history
  schemas/<shape>.schema.json  JSON Schema, draft 2020-12
  fixtures/valid/<case>.json   each must validate
  fixtures/invalid/<case>.json each must be rejected; the file name says why
```

- **Schemas** are the language-neutral artefact (`DEC-4`). Each has `"$schema":
  "https://json-schema.org/draft/2020-12/schema"` and an `$id` of the form `urn:rutent:<contract>:<shape>`. A schema
  that uses another contract's shape refers to it by that `$id`; it never copies it.
- **Fixtures** are plain JSON. Every invalid fixture breaks exactly one rule, named in its file name
  (`exchange-spanning-two-nodes.json`). A shape that travels as a stream has stream fixtures in JSON Lines, one
  value per line (`snapshot-stream-cut-short-before-its-end.jsonl`), checked entry by entry and as a whole.
- **Rust types** mirror the schemas with `serde`. For the router and state-machine APIs (library APIs, `DEC-4`)
  the Rust types are the API, and their records serialise to the schemas.
- **Tests** live in `contracts/tests/<contract>.rs`. They load the schemas and fixtures, assert that every valid
  fixture passes and every invalid one fails, and carry the plan's test names with a `// verifies:` comment
  (`DEC-21`). `jsonschema` is a dev-dependency only. A file directly under `contracts/tests/` whose stem is a
  contract's name, with `-` or `_` (`provider_role.rs`, `host.rs`), belongs to that contract; any other file there
  is shared.

## Versions and changelogs

The first `## <version>` heading of a contract's `CHANGELOG.md` is its current version. Each Rust module exposes
`pub const VERSION: &str` equal to it, and a test checks the two match.

```markdown
# provider-role changelog

## 0.2.0
- Operation and result schemas per role, with the statement envelope. (DEC-4, T-002-02)

## 0.1.0
- Advertisement and declaration schemas. (DEC-4, T-002-01)
```

- Before `C1`, versions are `0.x.y`, starting at `0.1.0`. `T-006-01` sets every contract to `1.0.0`.
- Every bullet names at least one `DEC-*`, and names the task that made it.
- A change to anything a contract owns needs a new top heading, greater than the base's, with at least one bullet.
  A contract that the base `CONTRACTS` lists but that owns no file at the base is new: its first version needs no
  bump, only a bullet naming a `DEC-*`. A contract that owns base files still needs a base changelog.
- Files under `contracts/` that belong to no contract (`Cargo.toml`, `src/lib.rs`, this README, `CONTRACTS`, and
  any file under `contracts/tests/` that names no contract) are shared. A change to them needs a bullet in
  `contracts/CHANGELOG.md` naming a `DEC-*`, or a version bump of some contract in the same change.

The contract guard (`T-001-05`) checks exactly these rules against a base ref passed as an argument (default
`origin/main`).

## Dependencies

`serde` and `serde_json` are dependencies, and `jsonschema` (default features off) is a dev-dependency, pinned here
once for every contract. Add nothing else without a reason in the feature notes. Every dependency needs a licence
decision in the record (`REQ-MOD-5`, `T-001-04`).
