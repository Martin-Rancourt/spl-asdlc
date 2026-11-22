# 2W8R — A verdict given in the tracker counts only when a person who owns the coordination or the release signs it, and the bridge commits it unchanged.

Status: Enforced

## Rule

Once every angle PR of a candidate is merged and its coordination item is open, the bridge asks on that item, once per candidate, for the verdict on every finding of the merged reports (a comment opening with `spl-asdlc:verdict-request candidate=<tag>`, schema `spl-asdlc/verdict-request/1`, `docs/reference/06-qa-report.md`). A verdict given back on the item (a comment opening with `spl-asdlc:verdict candidate=<tag>` and carrying a `spl-asdlc/qa-verdict/1` verdict in a fenced `json` block) counts only when the tracker says its author is a person and that person is the coordination item's assignee or the release's owner; the latest such comment is the one read. Any other author's verdict comment is ignored and told once, on the item, why. A valid one is committed unchanged by the bridge on `qa/<candidate>/consolidation`, from the release branch's tip, in a PR of its own to the release branch naming the coordination item; that PR is merged by the rules of any verdict PR (V7QD), the bridge counting as the coordinator only while the PR's file is the verdict signed. An invalid one is told once with its errors and commits nothing. A newer signed verdict before the merge replaces the file on the same branch; a merged verdict is never committed again. So the repository keeps every verdict, whoever gave it, and an agent still commits its own verdict in its own PR.

## Evidence

The owner's request of 2026-11-21, ASDLC-RS9P: a person who gives a release's GO should not have to write the JSON or open a PR; the verdict stays in the repository, signed in the tracker.

## Test

`tick::release_tests::tracker_verdict::the_findings_are_asked_on_the_coordination_item_once_every_angle_is_merged`, `tick::release_tests::tracker_verdict::a_verdict_the_owner_signs_in_the_tracker_is_committed_unchanged_in_a_pr_of_the_bridges`, `tick::release_tests::tracker_verdict::the_bridges_verdict_pr_is_merged_while_it_carries_the_signed_verdict`, `tick::release_tests::tracker_verdict::a_newer_verdict_replaces_the_file_and_a_merged_one_is_never_committed_again`, `tick::release_tests::tracker_verdict::a_verdict_from_anyone_else_or_invalid_is_told_once_and_commits_nothing`, `tick::tracker_verdict::tests::only_a_person_who_owns_the_coordination_or_the_release_gives_the_verdict`, `tick::tracker_verdict::tests::the_request_carries_every_finding_as_given_and_forges_no_marker`, `tracker::livela::tests::a_comment_is_a_persons_only_when_the_store_says_its_author_is_human`, `tracker::linear::tests::a_comment_is_a_persons_only_with_a_user_and_no_bot_actor` (unit test job).
