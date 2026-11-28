# R7WM — A patch carries only PRs already merged into `main`, through their normal review.

Status: Enforced

## Rule

A patch names its PRs by number. Each must be merged, into the default branch, by a merge commit the default branch holds, and not already in production; any other PR refuses the whole patch and nothing is pushed. The chosen merge commits are cherry-picked with `-x` in the order the default branch took them; a conflict refuses the patch. Each cherry-pick embarks the ticket of the PR it copies, read from its `(cherry picked from commit …)` line. Beside them, a patch carries only the App's own commits named in `carry` (3C74).

## Evidence

ASDLC-MG26: a fix shipped by a patch has already gone through its PR review and its merge into `main`; a patch never carries code no review saw.

## Test

`tick::release_tests::release_patch_refuses_a_pr_not_merged_into_main_or_already_in_production` (unit test job), `a_patch_cherry_picks_its_merged_prs_in_the_order_main_took_them`, `a_patch_refuses_a_pr_it_cannot_carry_and_pushes_nothing` (workflow tests, `tests/workflows/release_cut.rs`).
