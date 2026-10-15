# BR-ASDLC-013 — The consolidation agent documents problems, it does not solve them (it may note an already-obvious solution, never hunt for one).

Status: Enforced

## Rule

For each finding of a release QA report, blocking or not, it makes sure the report captures, as faithfully as it can, what was observed, where and in what context, since the finding's ticket carries those words alone. The rule covers the release QA, where no angle fixes anything and every finding becomes a ticket (BR-ASDLC-022); at a PR review the coordinator still applies the fixes it retains, nits included (`docs/agents/consolidate.md`, PR review, step 3), so a PR's nit never becomes a ticket.

## Evidence

The owner's decision of 2026-10-07 on the nits work (`docs/open-points.md` #14), and its scope confirmed by the owner the same day: fixed at the PR, accumulated at the QA. Extended to every release finding by the owner's decision on QA findings (2026-10-14, BR-ASDLC-022).

## Test

The agent protocols: `docs/agents/consolidate.md` (release candidate, step 2) and `docs/agents/review-release.md` step 3; spec 06 rule 5 says what each field of a finding carries. The code requires the context of every finding and copies what was observed, where and in what context into its ticket: `method::release::report::tests::every_finding_says_what_was_observed_where_and_in_what_context`, `method::release::findings::tests::a_blocking_finding_ticket_is_a_contract_to_fix_on_the_release_branch`, `method::release::findings::tests::a_non_blocking_finding_ticket_is_a_contract_logged_for_later`.
