# AUW5 — Merge every green PR before handling failures.

Status: Enforced

## Rule

On each pass, attempt all already-green PRs before refreshing a failing PR or returning a conflict. Age orders the PRs within each group; there is no one-merge-per-pass limit, except that a project requiring up-to-date branches (`review.require_up_to_date`) merges one PR per base and pass, the next one being behind once the first lands.

## Evidence

Owner’s decisions of 2026-10-27, ASDLC-NN5K (parallel PRs, phase 1).

## Test

`tick::tests::all_green_prs_merge_before_an_older_red_pr_is_refreshed`, `tick::tests::every_green_pr_is_merged_in_one_tick`, `tick::tests::a_project_requiring_up_to_date_branches_merges_one_pr_per_base_and_tick` (unit test job).
