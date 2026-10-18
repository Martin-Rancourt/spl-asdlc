# BR-ASDLC-032 — The protocol and angle texts an item carries are those of the version of spl-asdlc the project is pinned on, the same as its workflows; texts that cannot be read there create no item, and the bridge never copies other texts instead.

Status: Enforced

## Rule

The pin is the one `<owner>/<repo>@<ref>` that every job calling a reusable workflow `asdlc-*.yml` of the method names, under `.github/workflows/` at the revision the work is on (a PR's base branch, a candidate's tag). The bridge reads the job's protocol and the angle's text in that repository at that ref, with the token it holds for the project, once per tick and only for an item to create, and copies them into the item, which keeps them while open; a release angle's report schema stays the bridge's own, the one it checks reports against. So one update of the callers' ref brings the new workflows and the new texts together, and the project sees and may refuse both. No caller, callers on two refs, or texts that cannot be read create no item: the tick reports it, says it once on the ticket or the release item, and tries again at every tick. `docs/reference/08-configuration.md`, "Angles"; `docs/setup/github-app.md`.

## Evidence

The owner's decision in the angle brainstorm of 2026-10-18 (SID-974), implemented by SID-1192.

## Test

`tick::tests::a_rounds_items_carry_the_texts_of_the_version_the_projects_callers_pin`, `tick::tests::two_projects_pinned_on_two_versions_each_get_their_own_texts_read_once_per_tick`, `tick::tests::texts_that_cannot_be_read_open_no_round_and_are_said_once_never_replaced`, `tick::tests::without_a_caller_of_the_method_no_version_is_told_and_no_round_opens`, `tick::release_tests::a_candidates_items_carry_the_texts_of_the_version_its_callers_pin_at_its_tag`, `tick::release_tests::a_candidates_texts_that_cannot_be_read_open_no_item_and_are_said_once_on_the_release_item`, `method::texts::protocol::tests::an_item_carries_the_texts_of_the_pinned_version_and_says_where_they_come_from`, `method::texts::protocol::tests::a_release_item_keeps_the_bridges_own_report_schema_whatever_the_pin`, `github::rest::tests::a_file_of_another_repository_is_read_with_the_projects_installation_token`, `doctor::tests::the_method_version_its_texts_and_each_angle_are_judged_against_the_pin`
