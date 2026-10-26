# BR-ASDLC-024 — The project owns its CI content; the method checks always run on every PR and cannot be turned off.

Status: Enforced

## Rule

Every PR of every project runs the method checks: the title check and the guards, in one job of `asdlc-pr-checks.yml` (or, for a project that has not moved to it, `asdlc-pr-title.yml` and `asdlc-pr-guards.yml`), and both count: the branch rulesets require `required-check` and the title check, and `required-check` refuses a PR whose needs do not include the guards or whose `may-skip` lists them. No input of these workflows turns them off. Everything else a project's CI runs is the project's own: its tests, detectors, guard commands and gate suites, added by its own callers. SPL-ASDLC chooses none of them, imposes no default suite, and offers no switch, flag or on/off mechanism for them; a project that wants one keeps it in its own workflows. `spl-asdlc doctor` reports a project whose workflows do not run every method check, or whose rulesets do not require both.

## Evidence

The owner's decision of 2026-10-18 (SID-869), after a project turned its heavy PR checks off with a switch of its own: that choice is the project's, and the method must neither offer such a switch nor let one reach its own checks.

## Test

`tests/workflows/lints.rs`: `no_input_turns_the_method_checks_off`, `the_title_check_has_no_if_anywhere`; `doctor::tests::a_project_that_does_not_call_every_method_check_is_reported`, `doctor::tests::the_one_workflow_of_every_method_check_is_enough`, `doctor::tests::a_branch_ruleset_that_does_not_require_the_title_check_is_missing`; `tests/workflows/required_check.rs`: `the_methods_guards_are_a_need_and_never_skippable`
