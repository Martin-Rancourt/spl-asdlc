# X4PV — A file list GitHub cut counts as touching every reviewed path: never fewer reviews because GitHub stopped listing.

Status: Enforced

## Rule

A file list GitHub cut at 300 (G7CK) counts as touching every path a reader looks for. The candidate whose range, or whose diff since the previous candidate, is cut is given the class `sensible` in its brief and its candidate comment, whatever its tag says, and the brief says the list was cut by GitHub at 300 and that its paths and business rules touched are the first 300 only. Every configured angle is offered on it as on any candidate under the release's `later_passes` policy; a cut list never removes one. A release branch that moved past its candidate by a cut list is a tip gap, and the next candidate is due (C8WN), even when every listed file is bookkeeping.

## Evidence

ASDLC-P3U7: the security angle of a candidate whose range had 300 files was never offered, so its release could not reach its GO. A cut list hides paths: reading it as complete could skip a review.

## Test

`method::release::gate::tests::a_cut_file_list_makes_the_candidate_sensible`, `method::texts::tests::a_file_list_github_cut_is_said_cut_and_makes_the_class_sensible`, `tick::release_tests::cut_file_list::a_candidate_whose_range_github_cuts_at_300_files_is_labelled_briefed_sensible_with_every_angle`, `tick::release_tests::cut_file_list::a_later_candidate_whose_diff_since_the_previous_one_is_cut_says_so_and_is_sensible`, `tick::release_tests::cut_file_list::a_range_github_lists_whole_keeps_its_tags_class`, `tick::release_tests::cut_file_list::a_release_branch_that_moved_past_its_candidate_by_a_cut_list_is_a_tip_gap`, `tick::release_tests::auto_candidate::a_cut_file_list_after_the_candidate_is_never_bookkeeping_only` (unit test job).
