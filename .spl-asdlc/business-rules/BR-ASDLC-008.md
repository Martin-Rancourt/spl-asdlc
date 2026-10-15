# BR-ASDLC-008 — The state of a ticket follows its PRs, by two rules in order.

Status: Enforced

## Rule

Whenever a PR of a ticket resolves, or a ticket past the merge has an open PR: (1) the review rule has priority, another PR of the ticket in review keeps it In Review, a PR sent back by the coordinator puts it In Progress; (2) the completeness rule, with no PR in review a ticket whose boxes (the `## Post deploy` section apart) are not all ticked goes back In Progress; then an open PR on a complete ticket keeps it In Review (REQ-MERGE-028); only a ticket with every box ticked, nothing open and a PR merged goes To Deploy. In a project without a release cycle (no `[release]` section) merging is delivering: that ticket is judged at once by the Done gate, Done, or Post Deploy with its validation sub-task while only its own `## Post deploy` boxes are open (`docs/spec/05-release-cycle.md`, "Without a release cycle"). When an open PR's ticket could not be read, nothing is written (`docs/spec/04-pr-review.md` §10).

## Evidence

Owner's decision on several PRs per ticket (2026-10-07).

## Test

`method::ticket_state::tests::*`, `method::contract::tests::the_boxes_before_deploy_set_the_post_deploy_section_apart`, `tick::tests::a_merge_that_leaves_a_box_open_sends_the_ticket_back_to_work`, `tick::tests::a_merge_with_every_box_checked_goes_to_deploy_once`, `tick::tests::a_second_pr_on_a_ticket_past_the_merge_brings_it_back_to_review`, `tick::tests::changes_requested_on_one_pr_wait_while_another_is_in_review`, `tick::tests::an_unreadable_ticket_of_another_pr_holds_the_state_after_a_merge`, `tick::tests::without_a_release_cycle_a_merge_with_every_box_checked_is_done_once`, `tick::tests::without_a_release_cycle_post_deploy_boxes_wait_in_post_deploy_then_done`, `tick::tests::without_a_release_cycle_a_pr_merged_outside_the_bridge_is_swept_to_done_once`, `tick::tests::adding_a_release_section_later_switches_to_the_full_flow_and_leaves_done_tickets_done`
