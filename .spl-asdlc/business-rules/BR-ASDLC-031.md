# BR-ASDLC-031 — A project adds to a method angle, never replaces it: its notes complement the method's text, and a different angle is the project's own, under another key.

Status: Enforced

## Rule

An item of a method angle embeds the method's text of the angle and links the project's notes of it (`notes`, by convention `.spl-asdlc/angles/<key>.md`): the agent reads both, and nothing lets a project's file stand in for the method's text. A project that wants a different point of view declares an angle under a key the method does not have; its items carry the job's protocol and a line saying the method has no text for it. `docs/reference/08-configuration.md`, "Angles".

## Evidence

The owner's decision in the angle brainstorm of 2026-10-18 (SID-974), implemented by SID-1192.

## Test

`tick::tests::a_new_angle_sub_item_embeds_the_pr_review_protocol_and_links_the_projects_notes`, `method::texts::tests::a_project_only_angle_gets_the_generic_protocol_a_line_and_its_linked_notes`, `method::texts::protocol::tests::a_project_angle_gets_the_generic_protocol_and_says_the_method_has_no_notes`
