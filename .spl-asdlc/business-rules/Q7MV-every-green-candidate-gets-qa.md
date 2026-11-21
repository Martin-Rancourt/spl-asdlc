# Q7MV — Every green release candidate gets its configured QA angles.

Status: Enforced

## Rule

Under the default `release.later_passes = "full"` policy, every release candidate whose gate is green opens every configured QA angle and its consolidation item, whatever its RC number. A red candidate opens no QA round and consumes no QA pass. `release.max_rc_passes` controls escalation only. Under `later_passes = "gate-and-coordinator"`, candidates after the first keep their lighter flow with no new angle.

## Evidence

The GO is computed from the latest green candidate's own QA rather than from how many tags preceded it (`docs/reference/05-release-cycle.md` §7–§10; `docs/reference/08-configuration.md`).

## Test

`tick::release_tests::a_full_later_candidate_gets_qa_after_earlier_red_candidates`, `tick::release_tests::a_green_candidate_recorded_without_angles_is_retried_after_upgrade`, `tick::release_tests::a_lighter_later_pass_counts_the_first_pass_and_is_a_go_on_a_green_gate`.
