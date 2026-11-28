# FS78 — Every ticket the bridge creates is refined as it is created: no person completes its contract.

Status: Enforced

## Rule

An item the bridge creates as a ticket carries a complete contract from its creation: a `## Goal`, `## Acceptance criteria` with at least one box, `## Business rules` (« No new rule. » when it brings none), and, unless a tracker exempts it by its identity marker, a `## Release note` with an `EN:` and an `FR:` line. The writer of a release's unified note and the fixer of a blocked merge back write their change and tick their boxes; nobody completes their ticket by hand. A unified note ticket whose PR is merged (To Deploy or Post Deploy) no longer holds the GO, and the closure closes it with the release issue's other children.

## Evidence

Owner's decision of 2026-11-27 (ASDLC-DJCU): the unified note ticket of a release was created without its business rules and release note, so the tracker counted it as unrefined, its PR went back to draft, and once merged it held the GO it was waiting on. The owner completed and closed it by hand on three releases.

## Test

`tick::release_note::tests::each_note_task_is_refined_as_it_is_created`, `method::texts::tests::the_merge_back_fix_ticket_is_refined_as_it_is_created`, `tick::tests::a_note_pr_whose_box_is_ticked_merges_and_is_never_returned_to_draft`, `tick::release_tests::a_note_ticket_merged_to_deploy_frees_the_go_and_the_closure_closes_it` (unit test job).
