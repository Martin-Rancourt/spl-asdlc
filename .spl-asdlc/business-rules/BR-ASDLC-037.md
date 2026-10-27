# BR-ASDLC-037 — A ticket whose PR is opened always goes through In Review; with no review configured, one pass writes In Review, merges and writes To Deploy.

Status: Enforced

## Rule

After the implementation, a ticket whose PR is opened (not a draft) always goes to In Review, before To Deploy: its history always shows In Review. The review step is the one that reads the project's configuration and decides whether there is a review to run.

- No review configured (no angle applies): the bridge does both steps in the same pass. It writes In Review, sees there is no review to run, merges on green CI and writes To Deploy, without waiting for the next pass.
- A review configured: the review happens between passes, and the bridge waits for it with the ticket In Review, then merges at a later pass.

The bridge never merges a PR whose ticket is not In Review: when the In Review write fails, the merge waits for a pass that writes it. The spec and the process page are unchanged; this rule fixes the bridge to them.

## Evidence

The owner's decision of 2026-10-26 (voice session, ASDLC-FDHS), after a sandbox ticket with no review configured went In Progress → To Deploy in one write: its In Review write had not landed, and with no review round nothing wrote it again before the merge.

## Test

`tick::tests::without_a_review_one_pass_writes_in_review_merges_and_writes_to_deploy`, `tick::tests::a_failed_in_review_write_holds_the_merge`, `tick::tests::an_approved_green_pr_is_merged_and_its_ticket_goes_to_deploy`
