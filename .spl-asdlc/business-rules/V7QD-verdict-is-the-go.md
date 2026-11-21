# V7QD — A candidate with QA angles reaches the GO only with its coordinator's merged verdict `go`; every finding is judged once in it, and only the accepted ones become tickets.

Status: Enforced

## Rule

Once every angle report of a candidate is merged, its coordinator commits a verdict (`release.record` rendered for the candidate, with the reserved key `consolidation`; schema `spl-asdlc/qa-verdict/1`, `docs/reference/06-qa-report.md`) from the branch `qa/<candidate>/consolidation`: each finding of every merged report judged exactly once, accepted (blocking or not) or rejected with its reason, what was checked between the angles, and `go` or `hold`; a `go` carries no accepted blocking finding. The bridge merges the verdict PR when it names the coordination item, carries a valid verdict and nothing else, every angle PR of the candidate is merged, its author is a release coordinator or one approved it at its head, its required checks are green and it is up to date; what else keeps it from the merge is said once per head, on the PR. From the merged verdict the bridge files the accepted findings only (BR-ASDLC-022), each once. The GO of a candidate with QA angles needs its merged verdict `go`: a missing verdict or a `hold` is a reason of the HOLD, of `release status` and of the stagnation notice. A candidate with no QA angle, or a later pass under the lighter policy, needs none. The verdict names no person and no agent (BR-ASDLC-001): its commit and its `item` say who. No release angle may take the key `consolidation`.

## Evidence

The owner's decision of 2026-11-21, ASDLC-8SPA: the coordinator gives the release's GO in a committed verdict, which keeps every judgment, a rejection always with its reason, for a later analysis of the reviews' quality.

## Test

`method::release::verdict::tests::a_complete_verdict_is_valid_and_files_only_the_accepted_findings`, `method::release::verdict::tests::a_rejection_needs_its_reason`, `method::release::verdict::tests::every_finding_is_judged_once_and_none_is_unknown`, `method::release::verdict::tests::a_go_carries_no_accepted_blocking_finding`, `method::release::go::tests::a_candidate_with_qa_angles_needs_its_verdict_go`, `tick::release_tests::the_merged_verdict_files_its_accepted_findings_once_and_closes_the_coordination`, `tick::release_tests::the_verdict_pr_is_merged_once_every_angle_is_and_only_when_valid`, `tick::release_tests::a_release_coordinator_app_approves_a_verdict_and_a_person_of_its_name_does_not`, `tick::release_tests::an_angle_pr_with_a_valid_report_is_merged_and_files_nothing_on_its_own`, `config::tests::the_verdict_key_is_no_release_angle`
