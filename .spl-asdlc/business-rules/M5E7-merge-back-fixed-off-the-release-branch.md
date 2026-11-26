# M5E7 — A blocked merge back is fixed on a branch cut from the release branch, with `main` merged into it, never by bringing `main` into `release/**`.

Status: Enforced

## Rule

When the publication's merge back of a release into the default branch is blocked (a conflict, a required check red, the App refused by the ruleset), the fix lives on a branch `chore/merge-back-<stamp>` cut from the release branch's tip, into which the default branch is merged and on which the fix is committed. The fix ticket the bridge opens carries this recipe and forbids bringing the default branch into `release/**`. The bridge never merges a `chore/merge-back-*` PR whose branch does not contain the release branch's tip, and says so once per head on the PR.

## Evidence

ASDLC-PQ5J: a project's merge back (2026-11-23) was red on `required-check`, because the default branch had moved after the cut. A release branch that receives the default branch carries code no candidate covered.

## Test

`tick::release_tests::merge_back::a_dirty_merge_back_is_said_once_and_gets_one_fix_ticket_across_passes`, `tick::release_tests::merge_back::a_resolution_pr_without_the_release_tip_is_refused_and_said_once_per_head` (unit test job).
