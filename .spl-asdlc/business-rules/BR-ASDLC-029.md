# BR-ASDLC-029 — The method's review angles are an optional library; the method checks are not.

Status: Enforced

## Rule

The review angles of a PR and the QA angles of a release are written once in the method (`docs/agents/angles/`, listed in `docs/agents/angles/README.md`) so that projects reuse them instead of rewriting them, and a project activates only the ones it names in its `.spl-asdlc/config.toml`: `angles = []` is no angle, and an angle nobody names never runs. The method checks are a different category: they run on every PR of every project whatever angles it activates, and no configuration removes them (BR-ASDLC-024). `docs/reference/08-configuration.md`, "Angles".

## Evidence

The owner's decision in the angle brainstorm of 2026-10-18 (SID-974), implemented by SID-1192.

## Test

`config::tests::an_explicit_empty_angle_list_is_no_pr_review_and_needs_no_coordinator`, `config::tests::a_release_with_no_qa_angle_is_written_explicitly_and_needs_no_coordinator`, `method::texts::protocol::tests::the_library_lists_every_angle_and_its_retired_table_reads_as_rows`, and the tests of BR-ASDLC-024
