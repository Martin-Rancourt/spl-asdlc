# Consolidate

**Input**: the consolidation item of a PR (review) or of a release candidate (QA), served once every angle has reported. **Output**: every finding answered, retained fixes applied, an approving review, or changes requested.

## For a PR review

1. Read every review thread of the PR, from every angle and from humans.
2. For each finding, decide once: **confirm** (fix it) or **reject** (answer in the thread with the reason). You may change a severity, with the reason. Never leave a thread unanswered.
3. Apply the retained fixes as commits on the PR branch. Keep them small and attributable to their thread. Resolve the threads you handled.
4. If a retained fix is beyond a consolidation (a design change, a missing piece of the contract), request changes: the ticket goes back to the implementer, with the list in your review.
5. When every retained fix is in and CI is green at the head, submit an approving review. The bridge merges; you do not.

## For a release candidate

1. For each angle PR (`qa/<candidate>/<angle>` → release branch): read the report file and the diff of the fixes. Confirm or reject each finding in a review thread of that PR, with reasons; confirm or change severities.
2. Check the report is valid (schema, `checked` non-empty, verdict consistent with the findings). An invalid report is a change request to its angle.
3. Resolve conflicts between angle branches on the angle branch itself (rebase or merge the release branch into it), never on the release branch directly.
4. Approve each angle PR when its fixes are right and its report is valid. The bridge merges them into the release branch and creates the tickets from the reports.
5. When every angle PR is merged, tag the next RC (cut workflow, action `candidate`) unless the project automates it. The gate runs; the GO is computed by the bridge from the facts. You never declare it.

You have the last word on findings, not on the GO.
