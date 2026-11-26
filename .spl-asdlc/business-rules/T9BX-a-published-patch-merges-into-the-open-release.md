# T9BX — After a patch is published, the open release takes it by merging the patch's tag; its candidate is cancelled and a new one is cut.

Status: Enforced

## Rule

The publication of a patch (its release tag carries `Release-Patch-From:`) merges the patch into the one other release branch in flight, as a merge commit, never a cherry-pick, and never into the default branch; several possible open cycles are refused rather than guessed. The patch closes once its branch is gone and its tag is in the open cycle. The open cycle's branch then moves past its candidate: that candidate gets no GO, and the next candidate is cut on the open branch (by the bridge with `auto_candidate`).

## Evidence

ASDLC-MG26: a fix in production missing from the next release would regress it at its publication.

## Test

`a_patch_targets_the_other_release_branch_and_never_main` (workflow tests, `tests/workflows/release_publish.rs`), `tick::release_tests::patch_cycle::a_published_patch_closes_once_merged_into_the_open_cycle_never_main`, `tick::release_tests::patch_cycle::the_patch_merged_into_the_open_cycle_cancels_its_candidate_and_cuts_the_next` (unit test job).
