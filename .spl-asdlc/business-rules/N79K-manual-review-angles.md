# N79K — A PR's review angles are those its paths bring plus those its ticket's labels ask for.

Status: Enforced

## Rule

A PR's active review angles are the angles of `[[review.angles]]` whose `paths` its changed files touch, plus the angles whose label `angle/<key>` its ticket carries, even when no changed file matches them (manual adds, never subtracts). An angle with no `paths` applies only by its label. The plan is recomputed at every tick: removing the label cancels an angle only the label asked for, and never an angle the PR's paths bring. A ticket label `angle/<key>` naming no configured angle is reported once on the ticket and holds nothing. Each configured angle has its label in the tracker, created by the bridge when the tracker lacks it.

## Evidence

Owner's decisions of 2026-10-29, ASDLC-DZ96: a human decides at refinement, ticket by ticket, that a PR needs a given review (a design pass above all), through a label the tracker lets them pick.

## Test

`method::review::tests::a_ticket_label_adds_its_angle_even_when_no_file_matches`, `method::review::tests::an_angle_without_paths_never_applies_on_its_own`, `method::review::tests::a_label_removed_leaves_the_angles_the_paths_bring`, `method::review::tests::only_an_angle_label_naming_no_configured_angle_is_unknown`, `tick::tests::a_ticket_label_adds_its_angle_to_the_round_though_no_file_matches_it`, `tick::tests::removing_a_label_cancels_the_manual_angle_and_never_a_path_triggered_one`, `tick::tests::an_unknown_angle_label_is_told_once_on_the_ticket_and_holds_nothing`, `tick::tests::the_bridge_creates_the_label_of_each_angle_the_tracker_lacks_once` (unit test job).
