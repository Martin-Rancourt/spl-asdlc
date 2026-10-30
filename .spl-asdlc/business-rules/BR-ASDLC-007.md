# BR-ASDLC-007 — A review round is scoped to its PR.

Status: Enforced

## Rule

Its angle sub-items and its consolidation item close when that PR resolves, Done at a merge and Canceled at a close without merge, whatever becomes of the ticket, which stays open for a later PR; a ticket In Review for another of its PRs gives a new PR its own round (`docs/reference/04-pr-review.md` §3, §10).

## Evidence

Owner's decision on several PRs per ticket (2026-10-07): a round left open after its PR resolved would be served to a reviewer with nothing to review.

## Test

`tick::tests::a_rounds_items_close_when_its_pr_merges_to_deploy`, `tick::tests::a_rounds_items_are_canceled_when_its_pr_is_closed_unmerged_while_the_ticket_stays_open`, `tick::tests::while_another_pr_is_in_review_the_ticket_stays_in_review`, `tick::tests::a_pr_closed_unmerged_leaves_the_ticket_in_review_while_another_is_reviewed`
