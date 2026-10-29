# BR-ASDLC-003 — A ticket in Post Deploy is its own validation: no sub-task; an agent checks what it verifies in production and hands the same ticket to the owner for the rest.

Status: Enforced

## Rule

When the bridge moves a ticket into Post Deploy (at the closure, or at the merge of a ticket with nothing to deploy) it writes the status alone: it creates no validation sub-task, adds no label, and keeps the ticket's assignee. Post Deploy stays outside the in-progress group: an implementer is never served the ticket to implement it. An agent takes the ticket first and checks each `## Post deploy` box it verified itself in production, with its proof; when boxes remain that need a human, it assigns this same ticket to the owner (`owner.tracker_user`) with a comment naming them. The validation is an action, never a PM decision, so the ticket gets no escalation label for it. The ticket stays in Post Deploy until every box of that section is checked (BR-ASDLC-002).

## Evidence

The owner's decision of 2026-10-07: post-release verification needs someone to do it, and the validation states are never served to an implementer (`docs/reference/03-vocabulary.md`). The owner's decision of 2026-10-27 (ASDLC-3XWH): a second item was not wanted; the boxes of the implemented ticket are the validation.

## Test

`tick::release_tests::the_closure_moves_a_ticket_to_post_deploy_and_creates_no_validation_sub_task`, `tick::release_tests::a_partially_checked_post_deploy_ticket_stays_in_post_deploy`, `method::release::closure::tests::the_closure_comment_names_what_stays_open_and_why`, `method::vocabulary::tests::post_deploy_stays_outside_the_in_progress_group`
