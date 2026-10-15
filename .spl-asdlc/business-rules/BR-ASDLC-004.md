# BR-ASDLC-004 — Release angle sub-items and the brief are created only once the candidate's gate is green, never at the release item's creation.

Status: Enforced

## Rule

The release item, the candidate's record and the embarked labels come at the first tick after the cut; the angle sub-items, the consolidation item and the brief of a candidate wait for that candidate's own gate run to conclude `success`, and the brief links that run and its evidence pack. A red, pending or unreadable gate, or a brief whose facts (evidence pack included) cannot be read, opens nothing of the round (a red gate opens its gate-fix ticket, BR-ASDLC-016); each later RC waits for its own gate.

## Evidence

An agent never starts on a candidate the deterministic layer refused, and never without the brief that links the gate's evidence (spec 05 §2–§3, REQ-REL-046/048).

## Test

`src/tick/release_tests.rs`: `a_red_gate_opens_the_release_item_but_no_angle_nor_brief`, `the_gate_turning_green_opens_the_round_once_with_the_evidence`, `a_later_candidate_with_a_red_gate_waits_again`, `an_unreadable_gate_writes_nothing_more_and_says_why`, `a_green_gate_whose_evidence_cannot_be_read_opens_nothing`.
