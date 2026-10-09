# Consolidate

**Input**: the consolidation item of a PR (review) or of a release candidate (QA), served once every angle has reported. At a PR review the item is blocked by every angle sub-item of its round until each is closed, and an approval you give while one is still open does not count (BR-ASDLC-018). **Output**: every finding answered, retained fixes applied, an approving review, or changes requested.

## For a PR review

1. Read every review thread of the PR, from every angle and from humans.
2. For each finding, decide once: **confirm** (fix it) or **reject** (answer in the thread with the reason). You may change a severity, with the reason. Never leave a thread unanswered.
3. Apply the retained fixes as commits on the PR branch. Keep them small and attributable to their thread. Resolve the threads you handled.
4. If a retained fix is beyond a consolidation (a design change, a missing piece of the contract, a PR that is not deployable on its own or should have been one PR with another, BR-ASDLC-005/006), request changes: the ticket goes back to the implementer, with the list in your review. For a PR that is not self-standing or a split without reason, record the reason in a comment on the ticket too.
5. When every retained fix is in and CI is green at the head, submit an approving review. The bridge merges; you do not.

## For a release candidate

1. For each angle PR (`qa/<candidate>/<angle>` → release branch): read the report file and the diff of the fixes. Confirm or reject each finding in a review thread of that PR, with reasons; confirm or change severities.
2. Check the report is valid (schema, `checked` non-empty, verdict consistent with the findings). An invalid report is a change request to its angle.
   For each `nit`, you document, you do not solve (BR-ASDLC-013): check that the report captures it as faithfully as it can, `summary` what was observed, `anchor` where, `evidence` in what context (how it was met, what it looks like), since the bridge copies those three into the nit's box and whoever triages it later has nothing else. A vague nit is a change request to its angle. Never hunt for a fix; you may note one already obvious at the end of `evidence`. Whether the nit is kept or dropped is not yours to decide either: the agent who takes the nits ticket up at refinement classifies it.
3. Resolve conflicts between angle branches on the angle branch itself (rebase or merge the release branch into it), never on the release branch directly.
4. Approve each angle PR when its fixes are right and its report is valid. The bridge merges them into the release branch and creates the tickets from the reports.
5. When every angle PR is merged, tag the next RC (cut workflow, action `candidate`) unless the project automates it. The gate runs; the GO is computed by the bridge from the facts. You never declare it.

You have the last word on findings, not on the GO.
