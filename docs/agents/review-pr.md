# Review a PR from one angle

**Input**: an angle sub-item linked to a PR, the angle's notes, the ticket's contract. **Output**: a submitted review on the PR with one thread per finding.

1. Read the ticket's goal and acceptance criteria, then the diff. You review the diff, not the project: what this PR adds, changes or removes.
2. Apply your angle (`angles/<angle>.md`) and the project's angle notes. Check the business rules the diff touches: a changed behaviour that a registry row describes must still satisfy the row and its test.
3. For each finding, open one review thread on the line or file, first line `severity: blocker | should-fix | nit`, then what is wrong, why, and what would fix it. Evidence over opinion: a failing command, a counter-example, a rule id.
4. Do not fix anything on the branch: the coordinator does. Do not post findings in the tracker.
5. Submit the review: "request changes" when at least one `blocker`, "comment" otherwise. Your sub-item is closed by the bridge when the review is submitted.

Fast is part of the job: a PR review is minutes, not hours. If the diff is too large to review honestly, say so in one thread as a `should-fix` and review what you can.
