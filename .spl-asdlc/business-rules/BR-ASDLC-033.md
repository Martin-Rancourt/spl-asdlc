# BR-ASDLC-033 — The project owns when an angle runs and who does it; the method's angle carries only its content.

Status: Enforced

## Rule

Which PRs an angle reviews (its `paths`, the size trigger `review.diff_size_extra_angle` and `review.diff_size_angle`), which candidates get the release angles, and who is served each angle (its `assignee`) are keys of the project's configuration. The method's angle file says what to look for, how to grade and what a finding shows, and nothing about when it runs. `docs/reference/08-configuration.md`, "Angles".

## Evidence

The owner's decision in the angle brainstorm of 2026-10-18 (SID-974), implemented by SID-1192.

## Test

`config::tests::a_review_angle_carries_its_paths_or_runs_on_every_pr`, `config::tests::a_size_trigger_is_off_by_default_and_names_the_angle_it_adds`, `method::review::tests::an_angle_on_paths_comes_only_with_a_pr_that_touches_one`, `method::texts::protocol::tests::every_angle_says_its_parts_and_a_workflow_angle_says_where_its_results_are`
