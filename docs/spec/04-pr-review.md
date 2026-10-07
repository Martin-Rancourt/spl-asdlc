# 04 — PR review

Fast, on the diff, with several angles and one coordinator. The volume is tens of PRs a day: nothing here may drown the PR or the tracker.

## Invariants

1. **CI first.** A PR is reviewed only when it is ready (not draft), its required checks are green, and its branch is up to date with its base when the base requires it. The content of the CI is the project's (build, tests, lint, project guards); the method only requires a single aggregated required check whose failure or skip blocks.
2. **The ticket's contract gates the review.** The linked ticket must have a complete contract and at least one box ticked (`01-ticket-contract.md`). Otherwise the PR waits and its author is told once.
3. **The bridge opens the review.** When 1 and 2 hold, the bridge sets the ticket **In Review** and creates one **angle sub-item** per configured angle plus one **consolidation item**, all linked to the PR. Which angles apply is configuration: a base set per project, plus angles added by the paths the PR touches (a sensitive path adds the security angle; a UI path adds the design angle).
4. **Findings live in the PR.** Each reviewer reads the diff from its angle, with the ticket's contract and the business rules touched as context, and posts its findings as review comments on the PR (one thread per finding, severity in the first line). A human who comments on the PR is one more angle. The tracker never receives a copy; the sub-item is closed by the bridge when the reviewer's review is submitted.
5. **The coordinator fixes and approves.** Once every angle has submitted, the consolidation item is served. The coordinator reads every thread, rejects false alarms with a reason in the thread, applies the retained fixes as commits on the PR branch, resolves the threads it has handled, and submits an approving review. If the change needed is beyond a fix, it requests changes and the ticket goes back to the implementer (In Progress).
6. **CI again, then the bridge merges.** The merge requires: coordinator approval at the head SHA, required checks green at that SHA, no unresolved thread, branch up to date if required, one merge per repository per tick. The merge is a squash by default (configurable), pinned to the head SHA; the branch is deleted; the ticket goes to **To Deploy**.
7. **The merge is idempotent and observable.** The bridge records every merge decision in its action log; a PR merged outside the bridge is swept and given the same transitions, once.
8. **Exclusions.** PRs of the release machinery (release-please PRs, release records, merge-backs) are never reviewed by angles and never merged by this path; `05-release-cycle.md` governs them.
9. **PRs of Apps that need no ticket.** A PR is linked to exactly one ticket; the one exception is a PR authored by an App the project lists (`review.ticketless_authors`, for example Dependabot). When such a PR is linked to no ticket, the bridge opens one: labelled `ticketless-pr`, with a complete contract whose one box the PR meets, linked to the PR. From the next tick the PR takes the path of every PR (review round, merge, To Deploy, release, closure). Only an App qualifies, never a human whose login matches. Any other PR without a ticket waits, and its author is told once.

## What the reviewer receives

- The diff, the PR description, the linked ticket's goal and acceptance criteria.
- The business rules touched by the diff (rows of the registry whose test files or cited paths intersect the diff) and the registry id format.
- Its angle protocol (`docs/agents/review-pr.md`) and the project's angle notes if any.
- The severity scale and the verdict vocabulary (`03-vocabulary.md`).

## What the coordinator receives

Everything the reviewers received, plus every review thread, the CI status, and the consolidation protocol (`docs/agents/consolidate.md`): every finding is confirmed or rejected with a reason, exactly once; a rejected finding is answered in its thread; a confirmed one is fixed on the branch or sent back to the implementer.

## Configuration (per project)

- Base angles and path-triggered angles, each with an optional assignee hint.
- Merge method (`squash` | `merge`), whether the branch must be up to date, the diff size above which an extra angle is added.
- Whether a human must approve in addition to the coordinator (policy `human-approval: always | sensitive-paths | never`).
- The Apps whose PRs need no ticket (§9).
