# 58JN — The owner's answer « No new rule. » to a contract decision request declares that the ticket has no new rule.

Status: Enforced

## Rule

When the bridge's contract decision request is answered « No new rule. » (the case and a final period ignored, « None » accepted) while the ticket's `## Business rules` section is missing or empty, the bridge writes `## Business rules` with `No new rule.` into the description, and nothing else; the PR goes on at the same tick when nothing else is missing. Any other answer is left to people. The bridge never writes a goal or acceptance criteria.

## Evidence

Owner's decision of 2026-11-13, ASDLC-G8M2.

## Test

`tick::tests::the_answer_no_new_rule_writes_the_declaration_and_the_pr_goes_on_at_the_same_tick`, `tick::tests::another_answer_writes_nothing_and_says_once_that_the_pr_waits`, `tick::tests::a_missing_goal_is_only_asked_for`, `method::contract::tests::no_new_rule_is_the_answer_case_and_final_period_aside` (unit test job).
