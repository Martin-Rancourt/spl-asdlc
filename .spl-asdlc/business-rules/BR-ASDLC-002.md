# BR-ASDLC-002 — A ticket leaves Post Deploy for Done at the bridge's tick, once every box of its `## Post deploy` section is checked; there is no verb.

Status: Enforced

## Rule

Checking the boxes, once verified in production, is the whole gesture. At each tick the bridge reads every item in Post Deploy and moves to Done each ticket whose Done gate is ok (its boxes and its duplicates'), whatever its labels: a ticket that came from another tracker without the `post-deploy-validation/<product>` label of earlier versions closes like any other, and a ticket that still carries that label loses it as it goes Done. A ticket with one box still open stays in Post Deploy and the bridge writes nothing for it; a read that fails moves nothing and is reported.

## Evidence

The owner's decision of 2026-10-07: the closure comment named a `release post-release-check` verb the bridge does not have, so nothing ever moved a ticket out of Post Deploy (`.spl-asdlc/open-points.md` #1). The owner's decision of 2026-10-27 (ASDLC-3XWH): the validation happens on the ticket itself, and the tickets migrated without the label waited in Post Deploy with every box checked.

## Test

`tick::release_tests::a_post_deploy_ticket_moves_to_done_at_the_tick_that_finds_every_box_checked`, `tick::release_tests::a_post_deploy_ticket_moves_to_done_with_or_without_the_label`, `tick::release_tests::a_partially_checked_post_deploy_ticket_stays_in_post_deploy`, `tick::release_tests::an_unreadable_read_moves_nothing_and_is_reported`, `tick::release_tests::a_dry_run_of_the_post_deploy_pass_writes_nothing`, `method::release::closure::tests::the_closure_comment_names_what_stays_open_and_why`
