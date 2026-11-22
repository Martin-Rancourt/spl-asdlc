# W4KD — Merging duplicates into a ticket keeps their boxes; the closure counts them until they are checked.

Status: Enforced

## Rule

A ticket's contract is every box of its description and of every ticket reached through its `duplicate` relations (`docs/reference/01-ticket-contract.md` §3). A merge into a ticket never drops, copies or exempts a duplicate's boxes, whatever the duplicate's status: the Done gate counts each one until it is checked on the duplicate itself. At the release closure, a ticket whose only open boxes are its own `## Post deploy` boxes goes to Post Deploy; a duplicate's open box, a `## Post deploy` one included, sends it back to In Progress, since it cannot be checked through the ticket (`docs/reference/05-release-cycle.md` §11).

## Evidence

The owner's decision of 2026-11-18, ASDLC-32ZC: the owner keeps the simple model "merge duplicates, the system handles both", and the closure reopened a ticket for 23 boxes inherited from three merged duplicates.

## Test

`method::done_gate::tests::the_duplicates_boxes_hold_the_ticket_until_checked_on_the_duplicate`, `method::done_gate::tests::reopens_a_ticket_whose_post_release_box_belongs_to_its_duplicate`, `method::done_gate::tests::evaluate_done_gate_a_duplicates_state_never_exempts_its_criteria`, `tick::release_tests::the_closure_reopens_a_ticket_for_its_duplicates_boxes_listed_by_origin`
