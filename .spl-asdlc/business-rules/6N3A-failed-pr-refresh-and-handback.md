# 6N3A — A failed PR is refreshed once against a moved base, then handed back as draft.

Status: Enforced

## Rule

Red CI refreshes only when the current base is missing. A still-red head returns to its implementer with failed-check links. Conflicts return with exact files and instructions to merge the base while resolving. The same head/base request is not repeated; automatic refreshes are capped at three per PR. A failed read/write does not hide an unexplained PR.

## Evidence

Owner’s decisions of 2026-10-27, ASDLC-NN5K (parallel PRs, phase 1).

## Test

`tick::tests::red_ci_with_current_base_returns_to_draft_with_diagnostics_once`, `tick::tests::a_conflicting_pr_returns_exact_paths_to_its_implementer_before_drafting`, `tick::tests::a_refreshed_failed_pr_merges_when_its_new_head_turns_green` (unit test job).
