# 3P4R — The bridge merges a `chore/merge-back-*` PR only when its branch contains the release branch's tip and its checks are green.

Status: Enforced

## Rule

The bridge merges a `chore/merge-back-*` PR into the default branch only when all of these hold:
- the merge back of a published release is blocked and its fix ticket exists;
- the PR is ready, from this repository, and based on the default branch;
- its branch contains the release branch's tip;
- its check rollup is green with the default branch's required checks, the title check included.

A conflicting or red PR waits. A merge back that a person resolved by hand is taken as done: the closure closes the fix ticket.

## Evidence

ASDLC-PQ5J. Before this rule the bridge never merged a `chore/merge-back-*` PR, and the owner merged such a fix by hand.

## Test

`tick::release_tests::merge_back::a_red_resolution_pr_is_not_merged`, `tick::release_tests::merge_back::a_resolution_pr_without_the_release_tip_is_refused_and_said_once_per_head`, `tick::release_tests::merge_back::a_merge_back_resolved_by_hand_closes_the_release_and_its_fix_ticket`, `tick::merge_back::tests::only_a_ready_chore_merge_back_pr_of_this_repository_into_main_resolves` (unit test job).
