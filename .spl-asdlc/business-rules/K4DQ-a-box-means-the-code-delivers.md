# K4DQ — A checked box means the PR's code delivers the criterion; what can only be observed in production is a Post deploy box.

Status: Enforced

## Rule

A ticket's box is checked when the code of its PR delivers the criterion, not when the criterion is in effect: no box outside `## Post deploy` waits for this PR, another PR or a release to be merged or deployed. The review verifies each checked box against the diff. A criterion that can only be observed in production is written under `## Post deploy`; the ready gate does not require those boxes, and they stay open until verified in production (`docs/agents/process.md`, "Refine a ticket" step 4 and "Implement a ticket" step 4; `docs/agents/review-pr.md`, step 3; `docs/reference/01-ticket-contract.md`, 4 and 5).

## Evidence

The owner's decision of 2026-11-12 (ASDLC-YKDU): on ASDLC-VKWN the bridge sent a PR back to draft because two boxes were left open by an implementer who thought them true only after the merges of that PR and of another repository's PR.

## Test

`tick::tests::ready_counts_unfinished_linked_children_but_exempts_post_deploy_and_fences`, `method::contract::tests::the_boxes_before_deploy_set_the_post_deploy_section_apart`
