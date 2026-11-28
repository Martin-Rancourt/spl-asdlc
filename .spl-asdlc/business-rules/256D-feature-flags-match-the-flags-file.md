# 256D — A repository's feature flags match its `feature-flags.toml`.

Status: Enforced

## Rule

When a repository has `feature-flags.toml` at its root, the PR guards fail if a flag read in the code is not in its `active` list, or if a flag of `active` is no longer named in any tracked file outside the flags file and its `exclude` paths. A malformed flags file fails them too. With no file, nothing runs. The check never reads a flag service. Format and checks: `docs/reference/11-feature-flags.md`.

## Evidence

ASDLC-S9JV, out of the owner's decision on the reuse of the feature-flags check (option A: a step of the PR guards, on when the file exists).

## Test

`without_a_flags_file_nothing_is_checked`, `an_active_flag_read_and_named_passes`, `a_flag_read_but_not_active_fails_the_pr`, `a_usage_pattern_finds_the_repositorys_own_reads`, `an_active_flag_no_file_names_any_more_fails_the_pr`, `only_tracked_files_outside_the_excluded_paths_count`, `a_malformed_flags_file_fails_the_pr`, `a_runner_without_python_3_11_fails_a_repository_with_flags`, `the_flags_step_runs_on_every_pr_but_the_skipped_heads` (workflow tests, `tests/workflows/pr_guards.rs`).
