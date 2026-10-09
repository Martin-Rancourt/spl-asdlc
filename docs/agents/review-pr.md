# Review a PR from one angle

**Input**: an angle sub-item linked to a PR, the angle's notes, the ticket's contract. **Output**: a submitted review on the PR with one thread per finding.

1. Read the ticket's goal and acceptance criteria, then the diff. You review the diff, not the project: what this PR adds, changes or removes.
2. Apply your angle's notes (`angles/<angle>.md`, which your sub-item embeds after this protocol) and the project's angle notes (your sub-item links them when the project has some). Check the business rules the diff touches: a changed behaviour that a registry row describes must still satisfy the row and its test.
3. Check that the PR is deployable on its own (BR-ASDLC-005). A PR may deliver only part of its ticket: that is allowed, and its ticket stays open for the next PR. But once merged it may ship to production at any time, so it must stand by itself: nothing half-wired, nothing that only works once a later PR lands. When the ticket has several PRs, read the reason its implementer recorded in the ticket's comments (BR-ASDLC-006). A PR that is not self-standing, or a split with no real reason (everything should have been one PR), is a `blocker` finding: say why in the thread, and record the same reason in a comment on the ticket. The coordinator who confirms it requests changes, and the bridge sends the ticket back to In Progress.
4. For each finding, open one review thread on the line or file, first line `severity: blocker | should-fix | nit`, then what is wrong, why, and what would fix it. Evidence over opinion: a failing command, a counter-example, a rule id.
5. Do not fix anything on the branch: the coordinator does. Do not post findings in the tracker; the only exception is the reason of step 3, recorded on the ticket.
6. Submit the review, its body naming your sub-item's key: "request changes" when at least one `blocker`, "comment" otherwise. The bridge closes your sub-item when it finds a submitted review whose body names its key, and the rest of the PR's round when the PR is merged or closed.

Fast is part of the job: a PR review is minutes, not hours. If the diff is too large to review honestly, say so in one thread as a `should-fix` and review what you can.
