# Consolidate

**Input**: the consolidation item of a PR (review) or of a release candidate (QA), served once every angle has reported; at a release candidate, also each angle PR as it comes. At a PR review the item is blocked by every angle sub-item of its round until each is closed, and an approval you give while one is still open does not count (BR-ASDLC-018). At a release candidate the item is blocked by every angle sub-item of the candidate, and an angle sub-item closes when its angle PR is merged, so the item is served once every angle PR is merged (BR-ASDLC-020). **Output**: at a PR review, every finding answered, retained fixes applied, an approving review, or changes requested; at a release candidate, each angle PR approved or sent back, then the overall review and the next candidate.

## For a PR review

1. Read every review thread of the PR, from every angle and from humans.
2. For each finding, decide once: **confirm** (fix it) or **reject** (answer in the thread with the reason). You may change a severity, with the reason. Never leave a thread unanswered.
3. Apply the retained fixes as commits on the PR branch. Keep them small and attributable to their thread. Resolve the threads you handled.
4. If a retained fix is beyond a consolidation (a design change, a missing piece of the contract, a PR that is not deployable on its own or should have been one PR with another, BR-ASDLC-005/006), request changes: the ticket goes back to the implementer, with the list in your review. For a PR that is not self-standing or a split without reason, record the reason in a comment on the ticket too.
5. When every retained fix is in and CI is green at the head, submit an approving review. The bridge merges; you do not.

## For a release candidate

A candidate asks two things of you, at two moments: you review each angle PR as it comes, as its reviewer (steps 1 to 4), and, once every angle PR is merged, you do the overall review its consolidation item serves (steps 5 to 8).

### Each angle PR, as its reviewer

The angle's own sub-item serves you this review: when its angle PR is ready (not draft) and its required checks are green, the bridge moves the sub-item to **In Review** and assigns it to you (`release.coordinator_assignee`), in the same write. Review the PR then; the sub-item stays the angle's work item, so leave its status to the bridge.

1. For each angle PR (`qa/<candidate>/<angle>` → release branch): read the report file and the diff of the fixes. Confirm or reject each finding in a review thread of that PR, with reasons; confirm or change severities.
2. Check the report is valid (the schema and rules of `docs/spec/06-qa-report.md`, which the angle's sub-item embeds, as your consolidation item does after this protocol: `checked` non-empty, verdict consistent with the findings). An invalid report is a change request to its angle.
   For each `nit`, you document, you do not solve (BR-ASDLC-013): check that the report captures it as faithfully as it can, `summary` what was observed, `anchor` where, `evidence` in what context (how it was met, what it looks like), since the bridge copies those three into the nit's box and whoever triages it later has nothing else. A vague nit is a change request to its angle. Never hunt for a fix; you may note one already obvious at the end of `evidence`. Whether the nit is kept or dropped is not yours to decide either: the agent who takes the nits ticket up at refinement classifies it.
3. Resolve conflicts between angle branches on the angle branch itself (rebase or merge the release branch into it), never on the release branch directly.
4. Approve the angle PR at its head when its fixes are right and its report is valid; otherwise request changes at its head, and its sub-item goes back **In Progress**, assigned to its angle's configured assignee (or to nobody, taken by role). The bridge merges an approved angle PR into the release branch, moves its sub-item to **Done** and creates the tickets from the report. Do not wait for the other angles: each PR is reviewed and merged on its own.

### The overall review, once every angle PR is merged

The candidate's consolidation item is served once every angle PR of the candidate is merged (BR-ASDLC-020).

5. Read every merged report of the candidate together, not one by one.
6. Confirm or reject each non-blocking finding (`should-fix`, `nit`) in the light of all the reports. The bridge has already filed their tickets from the merged reports, unchanged, rejected findings included (the report carries no mark of a rejection, `docs/spec/06-qa-report.md`): say your verdict in a comment on each finding's ticket, with the reason and the thread for every rejection, including those you rejected in an angle PR's thread at step 1; the ticket of a rejected finding is then canceled with that reason (by the owner), and a rejected `nit` of the shared nits ticket is dropped at its triage. A `nit` you still document, you do not solve (step 2).
7. Check that nothing falls between the angles: a concern no angle owned, or two angles that each assumed the other covered it. Say what you checked, and what you found, in a comment on the release item.
8. Tag the next RC (cut workflow, action `candidate`, dispatched on the release branch, never on `main`, once release-please has run on its tip) unless the project automates it. The gate runs; the GO is computed by the bridge from the facts. You never declare it. Your consolidation item goes **Done** when the newer candidate is tagged, or when the release is published.

You have the last word on findings, not on the GO.
