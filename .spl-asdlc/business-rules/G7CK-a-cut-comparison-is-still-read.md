# G7CK — A comparison GitHub cuts at 300 files is still read: its commits are complete, and its file list is marked incomplete.

Status: Enforced

## Rule

GitHub's compare lists at most 300 files. A comparison whose file list holds 300 files or more is read like any other: its commits are read whole (every page, or unreadable as before), and its file list is the one GitHub gave, marked cut (`Comparison::files_cut`). It is never refused for its file count, so a reader that needs only the commits (the embarked tickets, the closure) works as usual. A text that counts a cut list says « 300 file(s) or more: GitHub cut the list at 300 », never a count it is not. The history from the repository's root (a first release) reads the whole tree or refuses it, as before, and is never cut.

## Evidence

ASDLC-P3U7: a candidate whose range since the last release had exactly 300 files stalled its release, every pass failing on its embarked tickets and its brief. ASDLC-X7AX had fixed the same cap for the hold check only (`compare_status`).

## Test

`github::rest::tests::a_comparison_reads_every_commit_and_marks_a_cut_file_list`, `tick::release_tests::cut_file_list::the_fake_github_cuts_a_comparison_at_300_files_and_keeps_its_commits`, `tick::release_tests::cut_file_list::a_candidate_whose_range_github_cuts_at_300_files_is_labelled_briefed_sensible_with_every_angle`, `method::texts::tests::a_file_list_github_cut_is_said_cut_and_makes_the_class_sensible` (unit test job).
