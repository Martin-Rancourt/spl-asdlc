# BR-ASDLC-006 — One PR per ticket by default.

Status: Proposed

## Rule

A ticket has as many commits as needed and one PR; a second PR needs a real reason, which its implementer records in the ticket's comments (`docs/reference/00-overview.md` invariant 2, `docs/reference/01-ticket-contract.md` §7). A PR is linked to exactly one ticket.

## Evidence

Owner's decision on several PRs per ticket (2026-10-07), reconciling spec 00 §2 and spec 01 §7.

## Test

Enforced by the agent protocols, not by code: the implementer records the reason (`docs/agents/implement.md` step 8); the reviewer reads it and, when everything should have been one PR, records why on the ticket and the coordinator requests changes (`docs/agents/review-pr.md` step 3). The bridge's side, a ticket with several PRs handled per PR, is tested by `tick::tests::while_another_pr_is_in_review_the_ticket_stays_in_review`.
