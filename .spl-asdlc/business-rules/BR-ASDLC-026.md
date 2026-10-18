# BR-ASDLC-026 — The process page reaches a project only through an ASDLC update, pinned on the same version as its workflows; no agent runtime fetches or injects process rules of its own.

Status: Enforced

## Rule

The process page, `docs/agents/process.md`, is copied into the project's repository at `.spl-asdlc/process.md` when the project installs the method and each time it moves its workflow callers to another version, from that version (`docs/setup/repository.md`, "The process page"). The agent reads the local file in its clone; nothing fetches the page at run time. The reusable `asdlc-pr-guards.yml` of each version carries the git blob id of that version's page, so the page a project must carry is always the one of the version its callers pin: when a caller on `v1` follows the tag to a version whose page changed, the project's PRs are refused until one refreshes the page. No agent runtime injects process rules of its own; a runtime only points the agent to the project's agent file.

## Evidence

The owner's decision of 2026-10-18 (SID-1193): the difference must be visible at the update, in the same update as the workflows, and the agent reads a file of its clone.

## Test

`workflows::pr_guards::the_guards_carry_the_blob_id_of_this_versions_process_page`, `workflows::pr_guards::the_pinned_process_page_passes_and_a_missing_or_edited_one_is_refused`, `the_agent_file_template_points_to_the_process_page_and_lets_it_win` (`tests/setup_files.rs`)
