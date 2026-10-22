# BR-ASDLC-017 — A post-deploy validation is an urgent operator action, not a PM decision.

Status: Enforced

## Rule

The validation sub-task the bridge creates under a ticket in Post Deploy (BR-ASDLC-003) is created in Post Deploy, assigned to the operator, with the canonical priority urgent, which each tracker adapter maps to its own scale (Linear and Livela: 1). It has no escalation label: a validation is an action, not a PM decision. A replay that finds it creates nothing (`docs/reference/05-release-cycle.md` §11, `docs/reference/03-vocabulary.md`).

## Evidence

The owner's decision of 2026-10-08: a shipped ticket in Post Deploy waits on its verification in production, so that verification comes first among an agent's work.

## Test

`tick::release_tests::the_validation_sub_task_is_created_urgent_and_a_dry_run_says_so`, `tracker::livela::tests::an_urgent_item_is_created_at_the_stores_priority_one`, `tracker::linear::tests::an_item_is_created_with_its_state_labels_parent_and_priority`
