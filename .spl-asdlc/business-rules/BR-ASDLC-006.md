# BR-ASDLC-006 — One ticket ships in one PR.

Status: Enforced

## Rule

A ticket ships in one PR, with as many commits as needed. A technical split uses sibling tickets linked through `blocked by`, each with its own PR. Every ticket box outside `## Post deploy` must be checked before ready; an unfinished linked task child counts as unchecked. A PR is linked to exactly one ticket.

## Evidence

Owner's decision of 2026-10-29: all ticket work is declared complete at ready, with feedback before review, and technical splits use sibling tickets.

## Test

`tick::tests::ready_requires_every_box_and_retries_on_the_same_head`, `tick::tests::ready_counts_unfinished_linked_children_but_exempts_post_deploy_and_fences`, `tick::tests::unchecked_ready_contract_returns_unstarted_tickets_and_preserves_feedback_on_failure`. The one-PR rule is also required by `docs/agents/process.md` and `docs/agents/review-pr.md`.
