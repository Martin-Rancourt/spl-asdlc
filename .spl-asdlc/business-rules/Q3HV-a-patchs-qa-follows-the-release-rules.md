# Q3HV — A patch's pre-release QA follows the same rules as any release; no special case.

Status: Enforced

## Rule

A patch cycle gets its own release item and its own QA round, opened by the same release pass as any cycle: each angle is offered only on its changed paths, measured over the diff since the release in production, and the GO is computed by the same rules. The open cycle beside it is untouched until the patch is published.

## Evidence

ASDLC-MG26: a fix urgent enough for a patch is the change most likely to skip QA; the method keeps one rule for every release.

## Test

`tick::release_tests::patch_cycle::a_patch_cut_beside_an_open_cycle_gets_its_own_release_item_and_round`, `a_patch_is_prepared_classed_since_production_and_pushed_with_its_first_candidate` (workflow tests, `tests/workflows/release_cut.rs`).
