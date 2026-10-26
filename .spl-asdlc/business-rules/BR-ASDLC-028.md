# BR-ASDLC-028 — The delivered process page is never edited in the project: the project only adds its own instructions around it, the page wins on conflict, and a mandatory method check refuses any PR that changes it.

Status: Enforced

## Rule

`.spl-asdlc/process.md` stays exactly as the method delivered it. The project's own instructions (how to build and test, the folders that need care) go in its agent file, `AGENTS.md` (template: `docs/setup/templates/AGENTS.md`), which points to the page and says that the page wins when they contradict it. On every PR but the release chain's own heads, the guards (`asdlc-pr-checks.yml`, or `asdlc-pr-guards.yml`) compare the page at the PR's head with the page of the version it runs at, by git blob id, and fails when the page is missing or differs: a PR that edits it by hand is refused, and the error says how to refresh it from the ref the project's callers name. No input turns the check off; `process-page` only says where the copy lives.

## Evidence

The owner's decision of 2026-10-18 (SID-1193): changing the process is done in the method only, and a mandatory method check guards the page.

## Test

`workflows::pr_guards::the_pinned_process_page_passes_and_a_missing_or_edited_one_is_refused`, `workflows::pr_guards::the_process_check_runs_on_every_pr_but_the_skipped_heads`, `workflows::pr_guards::the_guards_carry_the_blob_id_of_this_versions_process_page`, `the_agent_file_template_points_to_the_process_page_and_lets_it_win` (`tests/setup_files.rs`)
