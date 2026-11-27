# L7BG — A break-glass patch carries only the named commits, all merged, and leaves only on a person's go.

Status: Enforced

## Rule

When the bridge cannot ship its own fix, `spl-asdlc release patch --break-glass` prepares the patch in a checkout: a release branch from the tag in production, and on it only the commits the person named, cherry-picked with `-x` in the order the default branch took them, then the patch version, its notes and the project's tests. Each named commit must be on the remote's default branch and not already in production; any other commit refuses the whole patch before any branch is made. The command pushes nothing and creates no tag: it stops before the tag and prints the exact commands of the go, which a person runs.

## Evidence

ASDLC-KEUY: on 2026-11-26 the bridge (in the Livela runner) was down and its fix shipped by a break-glass done by hand in about forty minutes. Only the fix left, nothing unpublished from the default branch, and the tag waited for an explicit go from the owner. The command keeps both properties and does the rest.

## Test

`break_glass::tests::v0_28_1_replayed_a_patch_branch_ready_tests_green_and_no_tag`, `break_glass::tests::a_commit_not_merged_into_the_default_branch_is_refused_and_nothing_changes`, `break_glass::tests::a_commit_only_local_is_not_merged_and_is_refused`, `break_glass::tests::a_commit_already_in_production_is_refused` (unit test job).
