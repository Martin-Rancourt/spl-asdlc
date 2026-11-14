# M6TQ — The contract check informs; it never gates alone.

Status: Enforced

## Rule

The check run `spl-asdlc / contract` the bridge publishes on a linked PR's head is not a required check of any ruleset, and the bridge's reading of a PR's checks leaves it out: a red contract check never makes a PR's CI red, and never holds a PR whose ticket's contract is complete. What holds a PR is the bridge's merge rule, which reads the ticket's contract itself (C9VD). A job of a workflow that happens to carry the same name is a check like any other. When GitHub refuses the check (403, the App without Checks: write), the bridge publishes none, keeps its hold comment, says so once in the tick's report and does not fail the tick.

## Evidence

The owner's decisions of 2026-11-02 (the check is for clarity on GitHub, the merge rule stays as it is) and 2026-11-13 (without the permission the bridge degrades quietly, a permanent safety net), ASDLC-57KZ.

## Test

`tick::tests::a_red_contract_check_never_holds_a_pr_alone`, `tick::tests::without_the_checks_permission_the_hold_comment_stays_and_the_pass_goes_on`, `method::checks::tests::the_bridges_contract_check_never_colours_the_checks` (unit test job).
