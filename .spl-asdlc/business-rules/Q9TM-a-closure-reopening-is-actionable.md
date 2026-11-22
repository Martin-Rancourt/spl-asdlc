# Q9TM — A closure message that reopens a ticket is complete and explicit enough for an agent to act on it with no other instruction.

Status: Enforced

## Rule

When the release closure sends a ticket back to In Progress for its open boxes, its comment lists every unchecked box in full and verbatim, never cut to a count: under the ticket itself first, then under each duplicate merged into it, named by its id and title, a box of a `## Post deploy` section said so. When some of those boxes come from duplicates, it says in plain words how to reconcile them: check on the duplicate each box the ticket's work already proves, with a comment there citing the evidence (a comment, a PR, a test); hand each box that needs a gesture in production to a human through a decision request; do the work of any other. It says what happens next: once only the ticket's own `## Post deploy` boxes are left, the ticket goes to Post Deploy as usual, and to Done once every box is checked. It ends saying the ticket is unassigned (BR-ASDLC-009).

## Evidence

The owner's decision of 2026-11-18, ASDLC-32ZC: the closure reopened a ticket with five box lines and "+21 more", most of them inherited from merged duplicates, and the agent that took it went round in circles about twenty times without reconciling them.

## Test

`method::done_gate::tests::a_reopen_lists_every_open_box_in_full_grouped_by_origin`, `method::done_gate::tests::a_reopen_skips_an_origin_with_nothing_open_and_names_a_post_deploy_box`, `method::done_gate::tests::a_reopen_with_duplicate_boxes_says_how_to_reconcile_them_and_what_comes_next`, `method::done_gate::tests::a_reopen_with_only_its_own_boxes_has_nothing_to_reconcile`, `method::done_gate::tests::a_reopen_for_an_unreadable_or_absent_contract_lists_no_boxes`, `tick::release_tests::the_closure_reopens_a_ticket_for_its_duplicates_boxes_listed_by_origin`
