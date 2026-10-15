# BR-ASDLC-009 — A ticket the closure returns to In Progress is unassigned; the agent system handles reprise by role, with nobody named.

Status: Enforced

## Rule

Whether the closure sends it back for its Done gate or for a cited rule the registry lacks, the bridge clears the assignee in the same write as the state, and says so in its comment; it never keeps the ticket on its current assignee and never resolves someone to hand it to (no rework assignee, no author marker). Rework is distributed by being pickable. A replay finds the ticket In Progress and writes nothing (`docs/spec/05-release-cycle.md` §11, REQ-REL-089).

## Evidence

The owner's decision on `docs/open-points.md` #9 (2026-10-07).

## Test

`tick::release_tests::the_closure_reopens_an_incomplete_ticket_unassigned_once`, `tick::release_tests::the_closure_reopens_a_ticket_citing_an_unknown_rule_unassigned`, `tick::release_tests::a_dry_run_of_the_closure_prints_the_unassign`, `method::done_gate::tests::a_reopened_ticket_is_said_unassigned_with_nobody_named`, `tracker::linear::tests::a_status_write_can_unassign_in_the_same_write`, `tracker::livela::tests::a_status_write_can_unassign_and_a_replay_leaves_no_fact`
