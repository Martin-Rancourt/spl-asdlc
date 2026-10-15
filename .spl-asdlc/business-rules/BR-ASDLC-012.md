# BR-ASDLC-012 — Everything served to an agent carries a full contract; every ticket generated from a release QA finding carries one, rather than any runtime exempting it by marker.

Status: Enforced

## Rule

A finding's ticket, blocking or not (BR-ASDLC-022), is written with a goal, non-goals, the finding under `## Acceptance criteria`, saying what was observed, where and in what context, and `No new rule.` under `## Business rules`. There is no shared nits ticket. A shared nits ticket an older bridge opened is never written into.

## Evidence

The owner's decision of 2026-10-07: the bridge wrote the shared nits ticket as bare checkbox lines, which a contract guard at pickup refused (`docs/open-points.md` #14, spec 05 §6). The owner's decision on QA findings (2026-10-14, BR-ASDLC-022): one ticket per finding, no shared nits ticket, no `release.nits`.

## Test

`method::release::findings::tests::a_blocking_finding_ticket_is_a_contract_to_fix_on_the_release_branch`, `method::release::findings::tests::a_non_blocking_finding_ticket_is_a_contract_logged_for_later`, `method::release::findings::tests::report_text_cannot_forge_a_box_or_a_marker_in_the_ticket`, `tick::release_tests::an_approved_angle_pr_with_a_valid_report_is_merged_and_its_findings_filed_once`, `config::tests::the_nits_grouping_is_gone_and_refused_as_unknown`
