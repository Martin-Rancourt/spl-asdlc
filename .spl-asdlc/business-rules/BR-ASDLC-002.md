# BR-ASDLC-002 — A ticket leaves Post Deploy for Done at the bridge's tick, once every box of its `## Post deploy` section is checked; there is no verb.

Status: Enforced

## Rule

Checking the boxes, once verified in production, is the whole gesture. At each tick the bridge reads the tickets labelled `post-deploy-validation/<product>` that are still in Post Deploy and moves to Done each one whose Done gate is ok (its boxes and its duplicates'), then removes the label; a ticket with one box still open stays in Post Deploy, and the bridge writes nothing but a missing validation sub-task; a read that fails moves nothing and is reported.

## Evidence

The owner's decision of 2026-10-07: the closure comment named a `release post-release-check` verb the bridge does not have, so nothing ever moved a ticket out of Post Deploy (`.spl-asdlc/open-points.md` #1).

## Test

`tick::release_tests::a_post_deploy_ticket_moves_to_done_at_the_tick_that_finds_every_box_checked`, `tick::release_tests::a_partially_checked_post_deploy_ticket_stays_in_post_deploy`, `tick::release_tests::an_unreadable_read_moves_nothing_and_is_reported`, `tick::release_tests::a_ticket_resolved_by_hand_loses_the_index_label_once`, `method::release::closure::tests::the_closure_comment_names_what_stays_open_and_why`
