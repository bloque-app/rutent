---
Status: experimental
amends: adr/0002-runtime-host-seam.md
experiment: EXP-1
decision_item: exp1-adopt-runtime (pending)
---

# Draft: amendment to ADR-0002, the runtime that implements the host contract

A draft of `EXP-1` (Narthex `REQ-EXP-3`). It is not accepted and not part of ADR-0002 until the owner answers item
`exp1-adopt-runtime` and promotion consolidates it (Narthex `REQ-EXP-5`). It amends ADR-0002 and does not reopen
`runtime-host-seam` (`DEC-6`). It is written for the item's recommended option. [If the owner chooses
VOS](#if-the-owner-chooses-vos) says what changes.

## Context

ADR-0002 made `CMP-STM` host-agnostic and left the runtime to a milestone-1 experiment: VOS (`PS-66`), or a
purpose-built store as the ruled fallback. `EXP-1` scored both behind the host contract frozen at `C1` (host
contract 1.0.0, state-machine contract 1.0.0). It used the host-contract suite (`T-005-03`), the gap list of
`research/2026-11-02-vos.md`, and the licence rows of `DEC-6` ([scoring sheet](../scoring.md)). The scored runs:

- `EXP-1-pb-20261107-1`, `-2` and `-3`: the purpose-built store, `rutent_stm::purpose_built_store`, store format 1;
- `EXP-1-vos-20261107-1`: VOS at `vos-core` `1aadf44fa2cb1eaedc015ee82343ac118cc89f1e`, through `T-007-03`'s
  adapter.

## Outcome of EXP-1

From the [outcome page](../outcome.md):

- **VOS: drop for v0.**
- **The purpose-built store: adapt.**

The numbers, each with its run:

- **Gate.** Both pass all 17 host-contract rows (`EXP-1-pb-20261107-1`, `EXP-1-vos-20261107-1`).
- **Shared gaps.** Of the 29 gap rows scored for both, outside the module rows, the store fails 17. VOS fails 21,
  and passes 6 more only through the adapter (`EXP-1-pb-20261107-1`, `-2`, `-3`; `EXP-1-vos-20261107-1`). Every
  row the store fails, VOS fails too.
- **VOS-only fails.** VOS fails 9 rows the store doesn't (`EXP-1-vos-20261107-1`). Four of them bear on a
  production v0:
  - `G-37`: two pinned nightly toolchains;
  - `G-39`: a restored backup counts as a compromised node;
  - `G-48`: forks under a personal account;
  - `G-49`: no CI.
- **Module rows.** The 16 module-runtime rows are not counted. The store is a host, not a module runtime; those
  rows are `EXP-2`'s (ADR-0003).
- **Licence rows.**
  - The store brings no new package (`L-04`, `EXP-1-pb-20261107-1`).
  - VOS's licence decision is pending (`L-01`).
  - 25 VOS crates have no line (`L-02`): 11 AGPL-3.0-or-later and 14 Apache-2.0.
  - 221 of the adapter's 240 other packages have no line either (`L-03`, `EXP-1-vos-20261107-1`).

## Decision

The purpose-built store implements the host contract for v0. `M2` builds `CMP-STM`'s library on it (`F-010`).

- **VOS is not adopted** at `1aadf44`. The adapter and its notes stay as `EXP-1`'s record, and its code does not
  merge.
- **The host contract stays runtime-neutral.** A later experiment may score a tagged VOS release, or another
  runtime, against it, as its own `EXP-n`, without reopening `runtime-host-seam`.
- **The reasons.** Every gap the store leaves open, VOS leaves open too, and VOS has nine more. The store's gaps
  close in rutent's own MIT code. VOS's shared gaps would close upstream, in a one-maintainer project with no tagged
  release (`G-47`, `G-46`). The store needs no licence ruling, and VOS needs one plus 246 more licence lines.
- **VOS's role.** The state machine ran in the adapter, not in VOS, so VOS was tried as a replicated store, not as
  the whole runtime `PS-66` names.

**Which wording stands on the licence.** ADR-0002 says VOS's licence needs counsel before adoption. The owner's
later ruling `counsel-licences` and `DEC-6` replace that step: a dependency's licence is the owner's decision,
recorded case by case in `licences/RECORD` (`REQ-MOD-5`). `DEC-6`'s wording stands, and ADR-0002's counsel sentences
read as superseded by it. This records which ruling applies. It draws no legal conclusion about any licence.

## Consequences

- **The flag.** The store leaves its experimental flag: `F-010` makes `purpose_built_store` the library's store,
  in place of the `purpose-built-store` feature that is off by default. It brings no new package, so
  `licences/RECORD` needs no new line and `E4`'s licence clause holds.
- **Before v0 reaches production,** `F-010` closes these, which both candidates fail:
  - `G-54`: a damaged, acknowledged last frame is cut without a refusal;
  - `G-33`: the store holds every record in memory, with no stated bound;
  - `G-40` and `G-51`: the store format has no upgrade path;
  - power loss, never reached by a test, on the deployment platform's sync.
- **Owned elsewhere.** The other gaps go to their features, as the outcome's
  [follow-up rows](../outcome.md#follow-up-rows) list them:
  - the audit trail to `F-022`, apart from the execution store (ADR-0005);
  - metrics and load figures to `F-033`;
  - `G-26` to `F-025`;
  - the release to `F-034`.
- **The `vos` line.** It stays `not-adopted` and `pending` in `licences/RECORD`. The licence check never fails on
  such a line, and no VOS code is in rutent's `Cargo.lock`.
- **The module side.** `EXP-2` (ADR-0003) is unaffected. The module-runtime rows VOS failed are reported to it,
  not decided here.

## If the owner chooses VOS

Then the Decision is reversed, with these changes:

- the owner's licence decision on VOS goes into `licences/RECORD` first, with 246 more lines for the 25 VOS crates
  and the 221 other packages (`DEC-6`, `REQ-MOD-5`). Permissive ones are permitted by the standing ruling
  `licence-permissive-standing`; copyleft ones go to the owner one by one;
- the adapter merges behind its flag only after that (`T-007-03`);
- the nine VOS-only fails, and the shared gaps above, become `F-010`'s work against an upstream rutent doesn't
  control;
- the six `pass (adapter)` rows stay closed only while `T-007-03`'s adapter code, the journal actor and the host
  adapter, is kept.
