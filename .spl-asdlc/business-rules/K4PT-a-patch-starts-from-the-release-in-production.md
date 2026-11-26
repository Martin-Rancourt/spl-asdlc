# K4PT — A patch starts from the tag of the release in production, never from `main`.

Status: Enforced

## Rule

The cut workflow's `patch` action cuts the patch's release branch from the release tag of X.Y.(Z−1), the release in production, and the `release patch` gesture computes the patch version from that release (production + 0.0.1). A patch is refused when that release tag is missing or when its version already has a cycle. The default branch is never the patch's base; a cycle already in flight does not block it.

## Evidence

ASDLC-MG26 (LVL-ZTSN, 24 nov. 2026): a fix had to reach production while another cycle was open, which took a tag by hand and a ruleset bypass.

## Test

`a_patch_starts_from_the_tag_in_production_beside_a_cycle_in_flight` (workflow tests, `tests/workflows/release_cut.rs`), `tick::release_tests::release_patch_dispatches_the_cut_from_the_release_in_production`, `tick::release_tests::release_patch_needs_a_release_in_production_and_a_free_version` (unit test job).
