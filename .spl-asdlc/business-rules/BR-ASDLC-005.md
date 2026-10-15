# BR-ASDLC-005 — Every PR is independently deployable.

Status: Proposed

## Rule

A merged PR can ship to production at any time, even when it does not close its ticket: the release embarks its ticket whatever the ticket's state (but Canceled), and the closure judges only a ticket in To Deploy or Post Deploy, leaving one still In Progress or In Review to its next PR (`docs/spec/04-pr-review.md` §10, `docs/spec/05-release-cycle.md` §2, §11).

## Evidence

Owner's decision on several PRs per ticket (2026-10-07): a PR may not close its ticket, so each one must stand by itself.

## Test

Enforced by review, not by code: the angle reviewer checks it (`docs/agents/review-pr.md` step 3) and the coordinator requests changes on a PR that is not self-standing (`docs/agents/consolidate.md` step 4). The bridge's side, shipping a merged PR whatever its ticket's state, is tested by `tick::release_tests::a_merged_pr_ships_while_its_ticket_is_still_in_progress`.
