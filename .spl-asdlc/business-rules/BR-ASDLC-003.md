# BR-ASDLC-003 — Post Deploy stays outside the in-progress group; its validation sub-task is an operator action.

Status: Enforced

## Rule

The ticket in Post Deploy is never served to an implementer. When the bridge moves a ticket into Post Deploy it creates, once, a validation sub-task under it: assigned to the operator (`owner.tracker_user`) in Post Deploy, urgent (BR-ASDLC-017), with a complete contract and the hidden marker `spl-asdlc:post-deploy-validation parent=<id>`, by which it is found again among the ticket's children (a lost answer is found, never created twice; a refused creation is retried at a later tick). The operator verifies the parent's `## Post deploy` boxes in production, checks them and closes the sub-task. The sub-task is an action, never a PM decision, so it has no escalation label.

## Evidence

The owner's decision of 2026-10-07: post-release verification needs someone to do it, and the validation states are never served (`docs/reference/03-vocabulary.md`).

## Test

`tick::release_tests::the_closure_moves_a_ticket_to_post_deploy_with_one_unassigned_validation_sub_task`, `tick::release_tests::a_lost_creation_answer_is_found_and_the_sub_task_never_created_twice`, `tick::release_tests::a_refused_validation_sub_task_is_created_at_a_later_tick_once`, `method::texts::tests::the_post_deploy_validation_sub_task_is_a_contract_marked_as_the_bridges`, `method::vocabulary::tests::post_deploy_stays_outside_the_in_progress_group`
