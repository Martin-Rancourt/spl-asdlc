# H4NC — A PR held for an incomplete ticket contract asks its owner a decision on the ticket.

Status: Enforced

## Rule

When the bridge holds a PR because its ticket's contract is incomplete, it raises one decision request on the ticket, on a tracker that has them: once per PR and per set of missing elements, naming the PR, each missing element and how to answer. It withdraws its open request once the contract is complete, or once the missing elements change (a new request then asks for the new set). A tracker without decision requests is told in a comment on the ticket, as before.

## Evidence

Owner's decision of 2026-11-13, ASDLC-G8M2: a green PR stayed held for a day for a missing `## Business rules` section, said only in comments nobody was waiting for.

## Test

`tick::tests::a_held_contract_raises_one_decision_request_naming_the_pr_and_the_gap`, `tick::tests::the_request_is_withdrawn_once_the_contract_is_complete_by_another_way`, `tick::tests::a_tracker_without_decisions_keeps_the_comment`, `tracker::conformance::tests::a_tracker_with_decisions_raises_reads_and_withdraws_one_in_the_suite` (unit test job).
