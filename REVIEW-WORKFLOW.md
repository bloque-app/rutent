# Review workflow

How a pull request is reviewed in rutent when an agent writes it, reviews it, or both. It is adapted from a review
workflow used in other repositories. The rules carry over because they describe how agents fail at
review, not how a language does. What is specific to rutent is marked **Here**.

[HOUSE-RULES.md](HOUSE-RULES.md) decides who may merge what. This file decides how a review runs.

## Branches

- Work is developed against **`develop`**. A milestone pull request (`m1`, `m2`, …) targets `develop`.
- **`main` is for releases.** A `develop` → `main` pull request is opened when a release is cut, and the owner merges it.
- **Here:** a merge into `develop` needs no human. A change touching a `DEC-20` path (HOUSE-RULES.md) waits for the
  owner's click on the release into `main`, where `main`'s code-owner rule enforces it.

## Roles

**Implementer**: writes the change and owns the pull request. A person or an agent.

**Reviewer**: reviews it, and owns the findings and being wrong about them. A person or an agent.

The two are never the same agent instance: an agent reviewing its own diff confirms its own reasoning.

A review may be posted from the same GitHub account as the pull request, as when the owner's agents both write and
review. The implementer answers it all the same. GitHub records such a review as a comment, not an approval, and
`develop`'s protection asks for no approval count for that reason.

**A review is scoped to a diff, not to a pull request number.** If the approach changes after a review, the review is
void and runs again.

## 1. Scope the review before running it

- Name the target explicitly: the pull request number, or its head commit. `develop...HEAD` is whatever happens to be
  checked out.
- Review in a dedicated worktree (`git worktree add --detach <dir> <sha>`), never in someone else's checkout.
- Confirm the file count matches what GitHub reports before reading a line.
- For a large diff, separate what moved from what changed.

## 2. Answer three questions before hunting defects

Answer them in order, in the review body, even when the answer is short.

1. **What changed, architecturally?** Not a file list: the design shift, the capability the system now has, and what
   the change defers or leaves half-done.
2. **What invariants do the tests assert, and what can they not assert?**
   - An absolute or a delta?
   - Can the test pass vacuously: an early return, a stub that satisfies the assertion by accident?
   - Does it pin the decision the code made, or the premise that decision rests on? A test that encodes the same
     belief as the code cannot fail when the belief is wrong. When the premise is a claim about a body of code, make the
     test read that code.
   - A seam between two tested halves is not tested.
3. **Is test coverage lost, or missing?** Loss is coverage that existed and no longer does: count it. A gap is what
   was never covered: rank it by consequence.

**Here:**

- Count the tests on the base and the head and state both numbers, for each crate the diff touches.
- Check the `// verifies:` comments: does the test verify what its ids say? The tag check proves an id exists, not that
  the test covers it.
- Say whether the change touches a `contracts/` schema, a fixture, or a contract's version. The contract guard flags
  it; the reviewer judges it.

## 3. Fan out by dimension, then verify centrally

- Split by subsystem, one helper per dimension, each given exact files and asked for `file:line` and a failure scenario.
- **Verify every finding against the source before relaying it.** A helper's finding is a hypothesis.
- Never truncate a search whose value is exhaustiveness. When the number is the finding, scan the structure, not the
  text, and publish the method beside the number. Two scans that disagree are data.
- Print the whole file when the claim is that something is absent from it.
- Mark each finding **CONFIRMED**, **REFUTED** or **UNVERIFIED**, and report the refuted list.

## 4. Separate the defect from its consequence

*This code is wrong* and *this is what follows* fail independently. Before assigning severity, ask what catches the
defect today: a guard, a fail-closed default, a check upstream. Then ask which way it fails, and what failing closed
costs.

## 5. State reachability apart from severity

"Breaks" and "breaks once someone enables the flag" are different facts. Both go in the finding. Check the flags and
the configuration, and say whether you used evidence or assumption.

## 6. Post findings to GitHub, coordinate in a channel

**GitHub is the record.** Inline comments for anything anchored to a changed line; the review body for the rest. The
body also says what was checked and found clean: a review listing only problems misleads.

**The channel is coordination**: an append-only file outside the repository, in the agents' project directory,
`pr<N>-review-chat.md`. Keep a status table at the top, one row per finding.

| Type | Meaning | The other side should |
|---|---|---|
| `READY_FOR_REVIEW` | new commits pushed | re-review, then `REVIEW_POSTED` |
| `REVIEW_POSTED` | findings are on the pull request | address them, then `ADDRESSED` |
| `ADDRESSED` | fixed, with commit hashes | verify, and update the status table |
| `DISPUTED` | the finding is wrong, or the fix is worse than the defect | argue it here, not on the pull request |
| `BLOCKED` | needs the owner's decision | say which decision, and why it cannot be defaulted |
| `FYI` | context | nothing |

**Here:** a `BLOCKED` decision goes on the decision form as an item, not only in the channel.

## 7. Dispute is a first-class outcome

If a finding is wrong, the implementer posts `DISPUTED` rather than implementing it. The reviewer is not assumed
correct. A dispute upheld is a good outcome and is recorded as one; a dispute left lingering is a finding nobody owns,
so close each one as upheld or withdrawn. Record a self-correction when it happens: it is the strongest evidence a round
produced.

## 8. Verify a fix; do not accept a commit message

- Re-check each claim against the source at the new head.
- **Did a test get flipped?** Read the old assertion and decide whether it pinned correct behaviour.
- **Are the additions pure?** `git show --stat`.
- **Run the mutation yourself.** Break the code a new test covers and watch it go red. A guard nobody has watched fail
  is not a guard.
- A test that discovers its subjects (walks a directory, scans the records) asserts it found some, so it cannot pass
  on nothing.
- After the merge lands, check again at `origin/develop`. A branch that once passed CI is not the branch anyone ships.

## 9. Gates before merge

- **Read the log, not the badge.** A filtered run can go green having run nothing.
- **Here:** the four required checks are `ci`, `records`, `contract-guard` and `dependency licence record`. Locally:

  ```
  cargo run -q -p rutent-records --bin tag-check
  cargo run -q -p rutent-records --bin plan-check -- --strict
  cargo run -q -p rutent-contract-guard -- origin/develop
  cargo run -q -p rutent-licence-check
  ```

  Each agent uses its own `CARGO_TARGET_DIR` (HOUSE-RULES.md).
- A skipped hook is disclosed and substantiated, never quiet.
- Say which local failures come from the environment, and let CI verify.

## 10. Merge

- A milestone pull request merges into `develop` with a merge commit, never a squash, and its branches are kept
  (HOUSE-RULES.md).
- **Here:** the orchestrating session merges into `develop` once the reviewer approves and every check passes. Because
  a review posted from the author's own account can't approve on GitHub, the reviewer approves in the review's text,
  and says nothing is left open.
- A release pull request lists the `DEC-20` paths it carries, for the owner's click.
- A `develop` → `main` pull request is always the owner's.

## Anti-patterns

- **Sharing one worktree between concurrent agents.** `git reset`, `git stash`, `git checkout --` and `git restore`
  destroy other agents' uncommitted work.
- **`git add -A`.** Stage the paths you changed, by name.
- **`gh pr edit --body "$(…)"`.** A failed substitution passes an empty body and wipes the description. Use
  `--body-file`.
- **Reading a document as authoritative without checking its age.** `git log -- <file>` settles it. A claim about how
  the repository works has a timestamp.
- **Treating a green test as proof it ran.**
- **Padding.** "No defects found" is a good answer.
- **Deleting a guard because it looks ad hoc.** Find out why it was added.

## Checklists

Reviewer:

- [ ] Target confirmed against GitHub's file count, in a dedicated worktree
- [ ] The three questions answered in the review body
- [ ] Every relayed finding verified against the source; refuted list included
- [ ] Defect separated from consequence; fail direction and existing guards checked
- [ ] Reachability stated with severity
- [ ] Findings on GitHub, inline where anchorable; clean areas recorded
- [ ] Channel open, status table current, `DISPUTED` invited explicitly
- [ ] Any counted finding comes from a structural scan, with its method published
- [ ] New guards mutation-tested by the reviewer
- [ ] Scanning tests assert they found something

Implementer:

- [ ] Disputed rather than implemented anything believed wrong
- [ ] No test weakened, deleted or skipped to make a fix pass
- [ ] Tests counted on base and head for every touched crate
- [ ] Said plainly what is unverified locally
- [ ] Any skipped hook disclosed and substantiated

Before merge:

- [ ] Every finding closed, or resolved as an upheld dispute
- [ ] Fixes re-verified at the merged head
- [ ] CI read by check name
- [ ] Base is `develop`, unless this is a release
- [ ] The reviewer has approved, in a review or a comment, with nothing left open
- [ ] A release into `main`: its `DEC-20` paths are listed, and the owner has clicked
