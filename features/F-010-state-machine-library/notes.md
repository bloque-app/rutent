# F-010 State machine library: notes

## T-010-02: the contract guard's three findings

- **Reading: "a contract's dependents".** The `M1` finding says contract dependents and the root `Cargo.lock` are not
  tied to bumps, and the task row says a change to them "that moves a contract's dependency" needs that contract's
  bump. Read as the dependency edges of the contracts crate: the guard follows `rutent-contracts` through the root
  `Cargo.lock` (its dependencies, theirs, and the dev-dependency) and fails when any reached package differs between
  the base and the head. A crate that depends on `contracts/` (`components/*`) is a consumer and does not trigger it.
  A change to `contracts/Cargo.toml` itself is a shared-file change, so the first rule covers it.
- **Reading: "that contract's bump".** All five contracts ship as one crate (`rutent-contracts`), and `CONTRACTS`
  does not say which contract uses which dependency, so a moved dependency counts as touching every contract: each
  needs a greater version and a new bullet naming a `DEC-*`. A contract new at the head still needs none.
- **Reading: the plan's decisions.** A decision is defined when `PLAN.md` has a table row starting with ``| `DEC-<n>` ``.
  The guard reads `PLAN.md` from the head working tree and exits 2 if it is missing. Every `DEC-<n>` a new bullet names
  must be defined, not just one. The shared changelog's new bullets are now held to the same rule as each contract's
  (the README already said "any changelog"; the code only checked contract changelogs).
- **Shared files.** A contract's version bump no longer covers a shared file. The old passing test for it is replaced
  by the required failing one.
- **DEC-20 paths touched.** `tools/contract-guard/` and `contracts/` (`README.md` states the rules; `CHANGELOG.md`
  holds the shared entry the new rule requires of this very change). No contract's files changed, so no bump.
- **Limit of "moved".** A move is any change to a reached package's identity: version, source, checksum and its
  dependency list. So another crate enabling a feature of a shared dependency (for example `preserve_order` on
  `serde_json`) changes that package's dependency list and forces every contract's bump and a `DEC-20` click for work
  that is not contract work. If it bites, narrow the identity to name, version and checksum.

## The VOS licence, adopted in part (owner, item `vos-licence-v0`, 2026-11-09)

The owner ruled a partial adoption: "There's nothing ruling that the entire repo must be on a single licence."
AGPL-3.0-or-later applies only to the package that links `vos-sdk`, the VOS program `T-010-05` builds, and to
binaries built from it. That package declares `license = "AGPL-3.0-or-later"`. `components/stm` and every other
package stay MIT and runtime-neutral behind the host contract, so the purpose-built store stays a drop-in fallback.
`licences/RECORD`'s `vos` line reads permitted under `DEC-26` and `vos-licence-v0`.
