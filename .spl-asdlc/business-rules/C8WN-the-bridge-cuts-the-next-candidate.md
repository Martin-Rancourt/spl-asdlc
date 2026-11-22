# C8WN — With `auto_candidate`, the bridge cuts the next candidate once per release branch head, when the branch moved past the latest candidate and every fix of the release is merged.

Status: Enforced

## Rule

With `release.auto_candidate = true`, at a tick that merged nothing and computed no GO, the bridge dispatches `release.cut_workflow` with `action=candidate` on the release branch when the branch moved past the latest candidate with a file that is not bookkeeping (`05-release-cycle.md` §8), no gate-fix ticket of the release and no blocking finding's fix ticket of the release is still before To Deploy, the release tag is not there yet, and the latest candidate is not the last allowed pass (`release.max_rc_passes`, where the escalation hands the cycle to the owner). It dispatches once per branch head: the dispatch first, then a comment on the release issue carrying the marker `spl-asdlc:auto-candidate branch=<branch> head=<sha>`; a head whose marker is there is never dispatched again, and a failed dispatch writes no marker and is retried at the next tick. A dry run prints the dispatch and writes nothing. Without `auto_candidate`, nothing is dispatched, and `release status` says the branch moved past the candidate and that a cut of the next candidate is due. The cut follows the facts, never a judgment: a coordinator's `hold` only files the fixes.

## Evidence

The owner's decision of 2026-11-21, ASDLC-WMD9: after a `hold` verdict, the bridge cuts the next candidate once the fixes are merged, so that a cycle never waits on a person between two candidates.

## Test

`tick::release_tests::auto_candidate::with_auto_candidate_the_bridge_cuts_the_next_candidate_once_per_branch_head`, `tick::release_tests::auto_candidate::bookkeeping_after_the_candidate_cuts_nothing`, `tick::release_tests::auto_candidate::no_cut_while_a_fix_ticket_of_the_release_is_before_to_deploy`, `tick::release_tests::auto_candidate::without_auto_candidate_nothing_is_dispatched_and_status_says_a_cut_is_due`, `tick::release_tests::auto_candidate::a_failed_dispatch_writes_no_marker_and_is_retried_at_the_next_tick`, `tick::release_tests::auto_candidate::no_cut_past_the_last_allowed_pass`, `tick::release_tests::auto_candidate::a_dry_run_prints_the_cut_it_would_dispatch` (unit test job).
