# BR-ASDLC-017 — A validation sub-task an earlier version created is canceled once, with a comment that sends its reader to the ticket.

Status: Enforced

## Rule

An open item in Post Deploy that carries the marker `spl-asdlc:post-deploy-validation parent=<key>`, hidden or visible and whatever key it names (one of a tracker the items came from included), is a validation sub-task of an earlier version (BR-ASDLC-003 creates none any more). The bridge comments on it once, saying that the validation now happens on its parent ticket and naming that ticket, then moves it to Canceled. The comment carries the marker `spl-asdlc:post-deploy-validation-retired`, so a cancel that failed is retried at a later tick without a second comment; once canceled the sub-task is out of Post Deploy and the next passes write nothing more about it. A sub-task already resolved is never written to. Its parent ticket is not touched: it waits on its own boxes.

## Evidence

The owner's decision of 2026-10-27 (ASDLC-3XWH): the bridge never closed the validation sub-tasks it created, so they had to be closed by hand after their parents went Done, and those created under an earlier tracker's key were duplicated.

## Test

`tick::release_tests::an_open_validation_sub_task_is_canceled_once_with_a_comment_whatever_key_it_names`, `tick::release_tests::a_refused_cancel_is_retried_at_a_later_tick_without_a_second_comment`, `tick::release_tests::a_closed_validation_sub_task_is_left_alone`, `tick::release_tests::a_dry_run_of_the_post_deploy_pass_writes_nothing`, `method::texts::tests::a_validation_sub_task_is_recognised_by_its_marker_whatever_key_it_names`
