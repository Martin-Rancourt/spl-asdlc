# BR-ASDLC-022 — A release QA finding is blocking or not: a blocking one becomes an urgent fix ticket assigned to the release fixer and blocks the GO until fixed in a later candidate; a non-blocking one becomes a ticket logged for later.

Status: Enforced

## Rule

An angle reviews the candidate from its angle, classifies each finding as blocking or non-blocking and documents it (what was observed, where, in what context), and fixes nothing in its branch: its angle PR carries its mandatory report only (`.spl-asdlc/qa/<candidate>/<angle>.json` by default, `item` its sub-item's key). From a merged report, the bridge files each blocking finding as an urgent fix ticket assigned to `release.fix_assignee`, the one key the gate-fix tickets also use (BR-ASDLC-016; unassigned, taken by role, when the key is unset or the tracker does not know it), which blocks the release issue: while it is open there is no GO, and its fix merged to the release branch makes the next candidate. Each non-blocking finding becomes an ordinary ticket, logged for later. The coordinator confirms or rejects each finding and may change whether it is blocking, with a reason. At a PR review nothing changes: the coordinator applies the fixes it retains at the PR.

## Evidence

The owner's decision on QA findings (2026-10-14), replacing the three-severity model of the release QA (`blocker`, `should-fix`, `nit`), the fixes made in angle branches, the shared nits ticket and `release.nits`.

## Test

`method::release::report::tests::a_finding_is_blocking_or_not_and_the_verdict_follows`, `method::release::report::tests::a_severity_or_a_fix_in_the_branch_is_refused_in_an_open_angle_pr`, `method::release::report::tests::a_merged_report_of_the_older_schema_is_still_read`, `tick::release_tests::an_approved_angle_pr_with_a_valid_report_is_merged_and_its_findings_filed_once`, `tick::release_tests::a_merged_report_of_the_older_schema_is_still_filed_blocker_as_blocking`, `tick::release_tests::an_angle_pr_that_carries_more_than_its_report_is_not_merged_and_told_once_per_head`, `tick::release_tests::a_blocking_finding_requalified_as_non_blocking_stops_blocking_and_back`, `tick::release_tests::with_no_qa_angle_a_blocker_still_holds_the_release`, `method::release::findings::tests::requalification_plans_follow_whether_the_finding_blocks`, `cli::tests::the_requalify_verb_says_whether_the_finding_blocks`, `config::tests::the_nits_grouping_is_gone_and_refused_as_unknown`
