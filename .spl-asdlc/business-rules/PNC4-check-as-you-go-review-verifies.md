# PNC4 — An implementer checks a ticket's boxes as the work is done; the review verifies every checked box and unchecks and sends back any box not really done.

Status: Enforced

## Rule

The implementer checks each box of a ticket, acceptance criteria and business rules included, as soon as it is done, never at the merge. A box that becomes moot is checked too, with a comment on the ticket saying why. Every box checked is the signal that the PR is ready for review. The review (each angle reviewer, and the coordinator) verifies every checked box: a box checked but not really done is unchecked, with a comment on the ticket naming the box and what is missing, and the ticket goes back to its implementer. The boxes under `## Post deploy` stay open until they are verified in production.

The bridge carries the send-back: its ready gate reads the ticket on every pass over a ready PR, during the review too, so a box unchecked by the review returns the PR to draft and the ticket to In Progress at the next tick (`docs/reference/04-pr-review.md`, "Ready contract gate"). Checking, verifying and unchecking are the agents' gestures (`docs/agents/process.md`, step 4; `docs/agents/review-pr.md`, step 3; `docs/agents/consolidate.md`, step 1).

## Evidence

The owner's decision of 2026-11-12 (ASDLC-HKWG): tickets left with every box open until the merge showed no progress, and a finished task never checked was served again for nothing.

## Test

`tick::tests::a_ready_pr_with_a_box_reopened_during_review_returns_to_work`, `tick::tests::ready_requires_every_box_and_retries_on_the_same_head`
