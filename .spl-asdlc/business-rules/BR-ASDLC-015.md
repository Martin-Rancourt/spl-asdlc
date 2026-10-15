# BR-ASDLC-015 — An item the bridge serves to an agent carries its protocol: the agent never depends on reading the method's repository to know what to do.

Status: Enforced

## Rule

A PR-review angle sub-item embeds `docs/agents/review-pr.md` and the angle's file `docs/agents/angles/<angle>.md`; a release angle sub-item embeds `docs/agents/review-release.md`, the angle's file and the schema and rules of the angle report (`docs/spec/06-qa-report.md`); a consolidation item embeds `docs/agents/consolidate.md` (and, for a candidate, the same schema and rules). An angle the method has no file for (a project's own) gets the generic protocol and a line saying no method notes exist for it; the project's own notes (`notes` of the angle) are linked, never embedded. The protocol sits under a `## Protocol` section with its headings demoted, so the item stays one complete contract, and the item says what closes it in the bridge's terms. The texts are compiled into the binary: an item carries the protocol of the bridge version that created it and is frozen at creation, found again by a marker that holds no protocol text and never rewritten. Every item stays within a conservative 60 000 characters; a protocol that would outgrow it is trimmed in its file, never cut silently (`docs/spec/04-pr-review.md` §3, `docs/spec/05-release-cycle.md` §3, `docs/spec/07-the-bridge.md`).

## Evidence

The owner's decision of 2026-10-08: the PR-review item said "as `docs/agents/review-pr.md` describes it", a path of the method's repository absent from a project's own, and never named the angle's file; everything served to an agent carries a full contract (BR-ASDLC-012).

## Test

`method::texts::tests::a_pr_review_angle_sub_item_embeds_the_pr_review_protocol_and_its_angles_notes`, `method::texts::tests::a_release_angle_sub_item_embeds_the_release_protocol_its_angles_notes_and_the_report_schema`, `method::texts::tests::a_consolidation_item_embeds_the_consolidation_protocol_and_says_what_closes_it`, `method::texts::tests::a_project_only_angle_gets_the_generic_protocol_a_line_and_its_linked_notes`, `method::texts::tests::the_longest_item_stays_within_the_trackers_size`, `method::texts::tests::the_markers_carry_no_protocol_text`, `method::texts::protocol::tests::every_angle_file_of_the_method_is_compiled_in`, `method::texts::protocol::tests::a_protocol_carries_no_marker_and_no_contract_heading`, `tick::tests::a_rounds_items_carry_their_protocol_and_one_found_by_its_marker_is_never_rewritten`, `tick::tests::a_new_angle_sub_item_embeds_the_pr_review_protocol_and_links_the_projects_notes`, `tick::release_tests::a_release_rounds_items_carry_their_protocol_and_keep_it_while_open`
