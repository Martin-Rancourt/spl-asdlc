# BR-ASDLC-012 — Everything served to an agent carries a full contract; every ticket generated from a release QA finding carries one, rather than any runtime exempting it by marker.

Status: Enforced

## Rule

A finding's ticket, blocking or not (BR-ASDLC-022), is written with a goal, non-goals, the finding under `## Acceptance criteria`, saying what was observed, where and in what context, and `No new rule.` under `## Business rules`. There is no shared nits ticket. A shared nits ticket of an older cycle that carries no contract is left alone and said, never written into.

## Evidence

The owner's decision of 2026-10-07: the bridge wrote the shared nits ticket as bare checkbox lines, which a contract guard at pickup refused (`docs/open-points.md` #14, spec 05 §6). The owner's decision on QA findings (2026-10-14, BR-ASDLC-022): one ticket per finding, no shared nits ticket, no `release.nits`.

## Test

`method::release::findings::tests::the_shared_nits_ticket_is_a_contract_whose_goal_is_to_triage`, `method::release::findings::tests::a_nit_ticket_of_its_own_is_a_contract_keyed_like_a_finding`, `method::release::findings::tests::a_nit_goes_under_the_acceptance_criteria_once_and_the_ticket_stays_a_contract`, `method::release::findings::tests::a_nits_ticket_without_a_contract_is_left_alone`, `tick::release_tests::the_shared_nits_ticket_is_a_full_contract_that_asks_to_triage`, `tick::release_tests::per_nit_grouping_files_one_contract_ticket_per_nit_in_triage_once`, `tick::release_tests::a_nits_ticket_of_the_older_format_is_left_alone_and_said`, `config::tests::the_nits_grouping_is_shared_or_per_nit_and_nothing_else` (to be updated: owner's decision on QA findings, pending the code change)
