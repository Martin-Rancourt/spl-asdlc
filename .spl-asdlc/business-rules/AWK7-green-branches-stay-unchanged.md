# AWK7 — A green PR merges without a reflexive base refresh.

Status: Enforced

## Rule

Agents and the bridge never update a green PR merely because its base moved. Currency is opt-in for a project or a GitHub strict ruleset.

## Evidence

Owner’s decisions of 2026-10-27, ASDLC-NN5K (parallel PRs, phase 1).

## Test

`tick::tests::a_green_behind_pr_without_currency_requirement_merges_without_refresh` (unit test job).
