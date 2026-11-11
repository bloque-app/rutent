# House rules

These are rutent's house rules (`PLAN.md` `DEC-25`). They take Narthex's defaults and add what rutent needs: the
paths of `DEC-20` that wait for the owner's click, the tag syntax for Rust tests, and the owner's rulings of
2026-11-06 on how construction is integrated and of 2026-11-08 on the branch model. Every brief to an agent carries
this file in full.

`PLAN.md` 0.2.0 carries both owner's rulings below
([2026-11-06](#the-owners-ruling-of-2026-11-06-construction-autonomy),
[2026-11-08](#the-owners-ruling-of-2026-11-08-branch-model)) in `DEC-20`, `DEC-22` and `DEC-25`. Where the two rulings
differ, the later one wins. Everywhere else `SPEC.md` and `PLAN.md` decide, and this file never edits them.

## What this file changes from Narthex's defaults

| Narthex default | Here |
|---|---|
| One pull request per task | One pull request per milestone, for an external reviewer (the ruling below) |
| An agent merges its own pull request when its task says so | The orchestrating session integrates each task into the milestone branch and pushes it. A milestone pull request targets `develop` and goes through the [review workflow](REVIEW-WORKFLOW.md); `main` takes only releases, merged by the owner |
| At most two helpers at once (`PLAN.md` `DEC-22`) | No cap on parallel agents (the ruling below) |
| A generic tag syntax for tests | Rust tests use a `// verifies: ID, ID` comment on the line directly above the test (attributes may sit between) |
| No path always waits for the lead | The `DEC-20` paths always wait for the owner's click, given on the release into `main` |

Everything else below is Narthex's default, carried unchanged.

## The owner's ruling of 2026-11-06 (`construction-autonomy`)

The owner gave this ruling in chat on 2026-11-06 and it is recorded on the decision form as `construction-autonomy`:

- Construction produces **one pull request per milestone** (`m1`, `m2`, …), opened for an external reviewer.
- The **orchestrating session integrates** each task into the milestone branch and **pushes** it. Task agents commit
  locally and do not push.
- There is **no cap on parallel agents**.

This replaced the per-task pull requests of `PLAN.md` `DEC-20` and the two-helper cap of `DEC-22`; `PLAN.md` 0.2.0
states it. The branch and commit naming below still holds for each task; only the pull request moves up
to the milestone. The `DEC-20` paths still wait for the owner's click, after a separate security review of those paths.
This ruling put the click on the milestone pull request; `branch-model` later moved it to the release into `main`.

## The owner's ruling of 2026-11-08 (`branch-model`)

The owner gave this ruling in chat on 2026-11-08 and it is recorded on the decision form as `branch-model`. It
replaces the "no agent merges" part of the ruling above:

- Work is developed against **`develop`**. Each milestone pull request targets `develop`, not `main`. `main` stays the
  repository's default branch (form item `default-branch`, 2026-11-09), so open every pull request with its base set:
  `gh pr create --base develop`.
- Review follows [REVIEW-WORKFLOW.md](REVIEW-WORKFLOW.md). An agent may answer a review even when the review was
  posted from the same GitHub account as the pull request.
- Once the reviewer approves and every check passes, the orchestrating session merges the milestone pull request into
  `develop`. **A merge into `develop` needs no human**, whatever paths it touches (the owner, in chat, later on
  2026-11-08).
- **`main` is for releases.** A `develop` → `main` pull request is merged by the owner, never by an agent. The owner's
  `DEC-20` click is given there.

## Identity and signing

- Commit with plain `git commit`, using the identity and signing key the repository or global git config already has.
- Never pass `-c user.name=...`, `-c user.email=...` or `--author=...`, and never set `GIT_AUTHOR_*` or `GIT_COMMITTER_*`.
- Never pass `--no-gpg-sign`, `-c commit.gpgsign=false` or `-c tag.gpgsign=false`, and never change `gpg.*` or `user.signingkey`.
- Never take an identity from session context and put it in a commit.
- If signing fails, stop and report. Never fall back to an unsigned commit.
- This repository does not use DCO sign-off.

## Commits, branches and pull requests

- One task, one commit. The message is `T-NNN-NN: what it does`, ending with the `Co-Authored-By` trailer the brief names.
- A milestone branch (`m1`, `m2`, …) is cut from `develop`. A task's branch is `f-NNN/t-NNN-NN-slug`, cut from the
  milestone branch. The pull request is the milestone's, against `develop`: its
  title starts with the milestone (`M1: ...`) and its body lists the tasks it holds, the identifiers they serve and
  their tests.
- Do not push, open pull requests or issues, or comment on a repository unless the brief says so. A task brief does not.
- Never edit `SPEC.md` or `PLAN.md` unless the task says so. On a gap in either, raise an item and record your reading
  in the feature notes; do not ask in chat.
- No claude.ai links, home-directory paths or private names in tracked files.

## Paths that wait for the owner's click (`DEC-20`)

A change to any of these reaches `main` only with the owner's click on the release pull request, after a separate
security review. It may land on `develop` before that, once the reviewer approves (the `branch-model` ruling):

- `contracts/`, including each version bump;
- the credential proxy and its resolvers (`components/prov/src/proxy/`), and the egress layer and its name
  resolvers, which carry the credentials outward (`components/prov/src/egress/`);
- module admission and signature checking: the assertion's reading and judging
  (`components/prov/src/module/admission.rs`, `components/prov/src/catalogue/signature.rs`); the platform, which
  admits and accepts releases, decides which need a fresh acceptance, checks the statement key and signs or refuses
  each answer (`components/prov/src/module/platform.rs`); the admitting compiler
  (`components/prov/src/module/engine.rs`, `components/prov/src/module/bin/compiler.rs`); and `EXP-2`'s bindings,
  which hold earlier ones (`experiments/2026-11-07-module-format/wasm-binding/` and `pvm-binding/`);
- `CMP-CORE-AUTHZ`'s policy engine (`components/core-authz/`);
- the log's and the checkpoints' signing (`components/core-audit/`);
- the repository's own gates: `.github/`, `licences/`, `tools/licence-check/` and `tools/contract-guard/`.

`.github/CODEOWNERS` holds each of these paths for the owner, and `main`'s protection requires code-owner review.
Its `enforce_admins` is off, so the owner's own merge of the release is the click. A path added to this list is
added there in the same change.

A task whose diff touches one of these says so in its commit body.

## Worktrees and build directories

- One agent per feature, in its own git worktree and its own build directory (`CARGO_TARGET_DIR`).
- Never run verification twice in one worktree, and never share a build directory between two agents.
- Never use `git stash`: the stash stack is shared across worktrees.

## No durable work in /tmp

- Briefs, clones, scripts and anything else that must outlive the session live in the repository or a project
  directory, never in `/tmp` or another directory the operating system cleans.
- Throwaway files a test creates and deletes in the same run are fine; use the platform temp API for those.

## Merge rule

- The orchestrating session integrates tasks into the milestone branch. A task is integrated only when its checks pass
  and a fresh reviewer agent (not the author) finds nothing.
- A milestone pull request targets `develop`. Once the reviewer approves under [REVIEW-WORKFLOW.md](REVIEW-WORKFLOW.md)
  and every check passes, the orchestrating session merges it, without waiting for anyone, with a merge commit, never a squash. The milestone
  branch and its task branches are kept, because run notes cite their commits.
- The `DEC-20` paths wait for the owner's click on the release into `main`, where `main`'s code-owner rule enforces
  it. A release pull request lists every `DEC-20` path it carries since the last release.
- `main` takes only releases: a `develop` → `main` pull request, merged by the owner. `main`'s protection requires
  code-owner review; `develop`'s requires a pull request and the four checks (`ci`, `records`, `contract-guard`,
  `dependency licence record`), with no approval count, since a review posted from the author's own account cannot
  approve.
- A default or an agent's own reading never authorises an outward action.

## Test naming and tags

- Test names are plain sentences that say what happens. No ids in the name. In Rust that is a snake_case sentence.
- Each test names the ids it verifies in a comment directly above it: `// verifies: REQ-MOD-1, DEC-21`. Attributes
  such as `#[should_panic]` may sit between the comment and the test.
- Write the test first, see it fail for the right reason, then make it pass.
- The ids must be ones `SPEC.md`, the ADRs, `PLAN.md` or `PRODUCT.md` define, and never a withdrawn one such as
  `REQ-AUZ-10`.
- The tag check enforces this: `cargo run -p rutent-records --bin tag-check` (add `--strict` to fail on a finding).
  CI runs it in report mode for now.
- The plan check keeps `PLAN.md` honest: `cargo run -p rutent-records --bin plan-check -- --strict` checks that every
  task serves a defined identifier, that there is no dependency cycle, that no task waits on a later milestone, and
  that each task's wave is later than its dependencies'. CI runs it strictly.

## Waiting on CI

- Never end a turn to wait on CI, and never poll it from the agent. Wait with one blocking command, for example
  `gh run watch <run-id> --exit-status`.

## Token budget

- Filter before reading: `tail -30`, `grep`, `head`, `jq` or a line range. Never put a raw CI log or a large file in context.
- One job per helper, short. Every spawn names its model. Sonnet by default for bounded coding, reviews, docs and fixes;
  Opus for design and code whose design is still open; Haiku for mechanical, rule-driven work.

## Checklists

- A checklist that ends a milestone or a task has every box ticked before the work counts as done.
