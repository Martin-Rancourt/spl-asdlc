# Y9KR — The business-rule registry is checked in CI: a PR on the errors it brings, a release candidate on every error.

Status: Enforced

## Rule

`spl-asdlc doctor` checks the installation only, never the registry. The registry's errors (a rule file whose format is wrong, a test an `Enforced` rule cites that is not in the repository) are judged by `spl-asdlc rules check`, read through `git` at a commit. The PR guards (`asdlc-pr-checks.yml`, `asdlc-pr-guards.yml`) run it at the head against the base and fail only on the errors the PR brings: an error already on the base never blocks a PR. The release gate (`asdlc-release-gate.yml`) runs it at the candidate as its always-declared suite `business_rules`, and any error, however old, blocks the release; the suite never reads as green when it did not run.

## Evidence

Owner's decision of 2026-11-27 (ASDLC-YX9K): two projects' doctors were red because of their registries, while the PRs that broke them had merged. The PR that breaks a rule is refused, and its author fixes it on the spot.

## Test

`method::business_rules::check::tests::a_change_is_judged_only_on_the_errors_it_brings`, `rules_check::tests::the_registry_is_read_from_each_commit_never_from_the_working_tree`, `doctor::tests::each_point_of_the_procedure_is_ok_missing_or_invalid` (unit test job); `tests/workflows/pr_guards.rs`: `a_pr_fails_only_on_the_registry_errors_it_brings`; `tests/workflows/release_gate.rs`: `the_candidate_answers_for_every_error_of_the_business_rule_registry` (workflow test job).
