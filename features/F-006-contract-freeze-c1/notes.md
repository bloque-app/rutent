# F-006 notes

Readings taken where `SPEC.md` or `PLAN.md` is silent or ambiguous, recorded per Narthex `REQ-SKL-24`.

## T-006-01

- The five contracts are at 1.0.0: provider-role, module-interface, router-api, state-machine and host. Each
  changelog has a `## 1.0.0` heading with one bullet (`DEC-3`, `DEC-4`, `DEC-20`, `T-006-01`), each `pub const VERSION`
  says `"1.0.0"`, and `contracts/CHANGELOG.md` and `contracts/README.md` say that `C1` has happened: versions are
  now 1.x.y, a breaking change bumps the major, and every change waits for the owner's click (`DEC-20`).
- The guard takes a major bump as any greater version (`semver` tuple comparison), so 0.x.y to 1.0.0 passes with no
  special case. The plan names no feature directory; `F-006-contract-freeze-c1` follows the other features.
- Git tags are not created on `m1`. They are created at the merge commit on `main`, after the external review,
  because the M1 PR may change or be squash-merged, and a tag on a commit that stops existing names nothing.
- Changes the reviewer asks for on the M1 PR are post-C1 changes under `DEC-4` and `DEC-20`: each needs a version
  bump, a changelog entry naming a `DEC-*`, and the owner's click.

### Version-bearing fields, by class

Rewritten with a text substitution that matched key and old value, so no fixture was reformatted. No fixture holds a
signature or digest computed over its own bytes (the contracts crate has no crypto dependency), so a rewrite breaks
none.

| Field | Class | Done | Count |
|---|---|---|---|
| `protocol_version` | names the provider-role protocol | `0.3.0` to `1.0.0` | 3373 in fixtures (provider-role 163, module-interface 65, router-api 3145); 2 in `conformance/src/router/cases.rs` now read `provider_role::VERSION` |
| `interface` | names the module-interface version | `0.1.0` (91) and `0.2.0` (2) to `1.0.0` | 93, all in module-interface fixtures |
| `host_version` | names the host contract | `0.1.1` to `1.0.0` | 10 |
| `state_machine_version` | names the state machine | `0.4.0` to `1.0.0` | 10 |
| `provider_version`, `module_version`, `router_version`, `version` (advertisement, declaration, history), `declared_in_version`, `caller_version`, `policy_version`, `platform_version` | a release's own version, or not a contract version | unchanged | not counted |

No other field names a contract's version. A walk over every bare `version` key in the fixtures found each under
`declaration`, `statement` or `history`, never a contract's.

### Fixtures left on an old version

- `module-interface/fixtures/invalid/successive-releases-under-another-protocol-version.json`: its point is that the
  two releases name different protocol versions (`0.3.0` against `0.4.0`); `validate` compares the two with each
  other (`admission.rs`), not with `VERSION`. Left whole, including its two `interface` fields (`0.1.0`).
- `provider-role/fixtures/invalid/statement-envelope-protocol-version-not-major-minor-patch.json`: `v3` is malformed
  on purpose.
- `state-machine/host/fixtures/invalid/snapshot-entry-header-with-a-version-not-semver.json`: `state_machine_version`
  is `latest` on purpose; its well-formed `host_version` (`0.1.1`) is left with it, so the file stays as written.

### Rust

- `contracts/tests/provider-role.rs`: `check_context` is called with `provider_role::VERSION`; the refusal case uses
  `2.0.0` (any version other than the current one).
- `conformance/src/router/cases.rs`: both `protocol_version` values use `provider_role::VERSION`.
- Left alone, as pure format tests: `conformance/src/role/encoding.rs` (`is_protocol_version("0.3.0")`) and
  `contracts/src/router_api/atoms.rs` (`ProtocolVersion::new("0.1.0")`).
- The stores' format entries (`purpose_built_store.rs`, `reference_host.rs`), the host import's check and the test
  support all compare with `contract::VERSION` and `machine::VERSION`, so they accept 1.0.0 with no edit. Their tests
  refuse `9.9.9`, `99.0.0` and a format-2 entry, which stay different. The fixtures once named `0.4.0` while the
  state machine was at 0.4.1, so no fixture check ever compared them with `VERSION`.

### Derived versions

`other_protocol_versions("1.0.0")` in `conformance/src/role/suite.rs` gives a higher major `2.0.0`, a higher patch
`1.0.1`, and a lower `0.0.0` (the code takes the previous major when minor and patch are zero). `0.0.0` is a valid
protocol version in both layers (the schema pattern `^(0|[1-9][0-9]*)\.…` and `is_protocol_version`), so the lower
probe stays as it is and no change to 0.9.0 was needed. A new unit test pins the three values, checks each is valid
and differs from `1.0.0`; the role suite's probes, run against the stand-ins in the workspace tests, refuse each.

## After C1: the M1 security review's bumps

The first post-`C1` change, under `DEC-4` and `DEC-20`: provider-role 2.0.0, module-interface 2.0.0 and host 2.0.0
(`T-002-03`), router-api 1.0.1 (`T-004-02`). state-machine stays at 1.0.0: nothing it owns changed. 1.0.0 is not
amended; each contract has a new top heading.

### Version-bearing fields, rewritten again

The same text substitution, matching key and old value.

| Field | Done | Count |
|---|---|---|
| `protocol_version` | `1.0.0` to `2.0.0`, provider-role's new version | at the branch head, 182 in provider-role (163 at `m1`, the rest new fixtures), 81 in module-interface (65 at `m1`), and 3145 in router-api (as at `m1`), rewritten in router-api's own commit |
| `interface` | `1.0.0` to `2.0.0` | at the branch head, 104, all in module-interface fixtures (93 at `m1`, the rest new fixtures) |
| `host_version` | `1.0.0` to `2.0.0` | 10 |
| `state_machine_version` | unchanged at `1.0.0` | |

The fixtures left on an old version on purpose are unchanged: `successive-releases-under-another-protocol-version`
(`0.3.0` against `0.4.0`, `interface` `0.1.0`), `statement-envelope-protocol-version-not-major-minor-patch` (`v3`)
and `snapshot-entry-header-with-a-version-not-semver` (`latest`, with `host_version` `0.1.1`). No release version
(`provider_version`, `module_version`, `version`) was touched.

### Rust

- `contracts/tests/provider-role.rs`: the refusal case used `2.0.0` as "another version", which is now the current
  one; it uses `1.0.0`, the version before. Its other operator key was `"B".repeat(43)`, which isn't canonical and
  no longer builds a `PublicKey`; it is `"A".repeat(43)`.
- `conformance/src/role/suite.rs`: the pinned test now derives from `provider_role::VERSION`, asserts it is `2.0.0`,
  and pins what `other_protocol_versions("2.0.0")` gives when run: a higher major `3.0.0`, a higher patch `2.0.1`, and
  the previous major `1.0.0`. All three are valid and differ from `2.0.0`.
- No other Rust literal used `2.0.0` or `3.0.0` as "a version other than the current one".
