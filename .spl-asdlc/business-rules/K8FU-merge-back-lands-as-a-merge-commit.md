# K8FU — A merge back PR lands only as a merge commit: the release tag becomes an ancestor of `main`.

Status: Enforced

## Rule

The bridge merges a `chore/merge-back-*` PR with the merge method `merge`, pinned to the head it read, never `squash`, whatever the project's merge method. Once that merge has made the release tag an ancestor of the default branch, the bridge dispatches the publication on the release branch with `replay: true`, once per release. The replay deletes the branch and publishes nothing, and the closure goes on.

## Evidence

ASDLC-PQ5J. A squash would leave the release tag outside the default branch's history, and the release would never close.

## Test

`tick::release_tests::merge_back::the_resolution_pr_is_merged_as_a_merge_commit_then_replayed_once_and_the_closure_closes_the_fix` (unit test job).
