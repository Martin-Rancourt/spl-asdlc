# PMF7 — A human review is a review angle assigned to a person; there is no human-approval setting.

Status: Enforced

## Rule

A person reviews a PR from an angle whose `assignee` is that person, asked by its paths or by its ticket label; the angle's sub-item is done when it is resolved in the tracker (Done or Canceled) or when a submitted review names its key. `review.human_approval` and `review.human_reviewer` are retired: `human_approval = "never"` and `human_reviewer` are accepted, read nowhere and reported by `doctor`; any other value of `human_approval` makes the configuration invalid, so that a project that asked for a human approval is never read as asking none.

## Evidence

Owner's decisions of 2026-10-29, ASDLC-DZ96: the all-or-nothing human approval held every PR forever (open point #4, now decided).

## Test

`config::tests::the_retired_human_approval_keys_are_accepted_harmless_and_refused_otherwise`, `doctor::tests::a_retired_human_approval_key_is_reported_with_the_angle_that_replaces_it` (unit test job).
