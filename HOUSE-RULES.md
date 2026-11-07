# House rules

These are rutent's house rules (`PLAN.md` `DEC-25`). They take Narthex's defaults and add what rutent needs: the
paths of `DEC-20` that wait for the owner's click, the tag syntax for Rust tests, and the owner's ruling of
2026-11-06 on how construction is integrated. Every brief to an agent carries this file in full.

Where this file and `PLAN.md` disagree, this file's [owner's ruling](#the-owners-ruling-of-2026-11-06-construction-autonomy)
wins until `PLAN.md` is amended; everywhere else `SPEC.md` and `PLAN.md` decide, and this file never edits them.

## What this file changes from Narthex's defaults

| Narthex default | Here |
|---|---|
| One pull request per task | One pull request per milestone, for an external reviewer (the ruling below) |
| An agent merges its own pull request when its task says so | No agent merges a pull request. The orchestrating session integrates each task into the milestone branch and pushes it |
| At most two helpers at once (`PLAN.md` `DEC-22`) | No cap on parallel agents (the ruling below) |
| A generic tag syntax for tests | Rust tests use a `// verifies: ID, ID` comment on the line directly above the test (attributes may sit between) |
| No path always waits for the lead | The `DEC-20` paths always wait for the owner's click |

Everything else below is Narthex's default, carried unchanged.

## The owner's ruling of 2026-11-06 (`construction-autonomy`)

The owner gave this ruling in chat on 2026-11-06 and it is recorded on the decision form as `construction-autonomy`:

- Construction produces **one pull request per milestone** (`m1`, `m2`, …), opened for an external reviewer.
- The **orchestrating session integrates** each task into the milestone branch and **pushes** it. Task agents commit
  locally and do not push.
- There is **no cap on parallel agents**.

This overrides the per-task pull requests of `PLAN.md` `DEC-20` and the two-helper cap of `DEC-22`, pending an
amendment to `PLAN.md`. The branch and commit naming below still holds for each task; only the pull request moves up
to the milestone. The `DEC-20` paths still wait for the owner's click, which now happens on the milestone pull request,
after a separate security review of those paths.

## Identity and signing

- Commit with plain `git commit`, using the identity and signing key the repository or global git config already has.
- Never pass `-c user.name=...`, `-c user.email=...` or `--author=...`, and never set `GIT_AUTHOR_*` or `GIT_COMMITTER_*`.
- Never pass `--no-gpg-sign`, `-c commit.gpgsign=false` or `-c tag.gpgsign=false`, and never change `gpg.*` or `user.signingkey`.
- Never take an identity from session context and put it in a commit.
- If signing fails, stop and report. Never fall back to an unsigned commit.
- This repository does not use DCO sign-off.

## Commits, branches and pull requests

- One task, one commit. The message is `T-NNN-NN: what it does`, ending with the `Co-Authored-By` trailer the brief names.
- A task's branch is `f-NNN/t-NNN-NN-slug`, cut from the milestone branch. The pull request is the milestone's: its
  title starts with the milestone (`M1: ...`) and its body lists the tasks it holds, the identifiers they serve and
  their tests.
- Do not push, open pull requests or issues, or comment on a repository unless the brief says so. A task brief does not.
- Never edit `SPEC.md` or `PLAN.md` unless the task says so. On a gap in either, raise an item and record your reading
  in the feature notes; do not ask in chat.
- No claude.ai links, home-directory paths or private names in tracked files.

## Paths that wait for the owner's click (`DEC-20`)

A change to any of these is never merged on an agent's own judgement. It waits for the owner's click on the pull
request, after a separate security review:

- `contracts/`, including each version bump;
- the credential proxy and its resolvers;
- module admission and signature checking;
- `CMP-CORE-AUTHZ`'s policy engine (`components/core-authz/`);
- the log's and the checkpoints' signing (`components/core-audit/`).

A task whose diff touches one of these says so in its commit body.

## Worktrees and build directories

- One agent per feature, in its own git worktree and its own build directory.
- Never run verification twice in one worktree, and never share a build directory between two agents.
- Never use `git stash`: the stash stack is shared across worktrees.

## No durable work in /tmp

- Briefs, clones, scripts and anything else that must outlive the session live in the repository or a project
  directory, never in `/tmp` or another directory the operating system cleans.
- Throwaway files a test creates and deletes in the same run are fine; use the platform temp API for those.

## Merge rule

- Under the owner's ruling, no agent merges a pull request: the orchestrating session integrates tasks into the
  milestone branch, and the milestone pull request is merged as the owner decides, after the external review.
- A task is integrated only when its checks pass and a fresh reviewer agent (not the author) finds nothing.
- The `DEC-20` paths wait for the owner's click, as above.
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
