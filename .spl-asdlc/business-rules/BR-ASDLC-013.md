# BR-ASDLC-013 — The consolidation agent documents problems, it does not solve them (it may note an already-obvious solution, never hunt for one).

Status: Proposed

## Rule

For each finding of a release QA report, blocking or not, it makes sure the report captures, as faithfully as it can, what was observed, where and in what context, since the finding's ticket carries those words alone. The rule covers the release QA, where no angle fixes anything and every finding becomes a ticket (BR-ASDLC-022); at a PR review the coordinator still applies the fixes it retains, nits included (`docs/agents/consolidate.md`, PR review, step 3), so a PR's nit never becomes a ticket.

## Evidence

The owner's decision of 2026-10-07 on the nits work (`docs/open-points.md` #14), and its scope confirmed by the owner the same day: fixed at the PR, accumulated at the QA. Extended to every release finding by the owner's decision on QA findings (2026-10-14, BR-ASDLC-022).

## Test

Enforced by the agent protocols, not by code: `docs/agents/consolidate.md` (release candidate, step 2) and `docs/agents/review-release.md` step 3; spec 06 rule 5 says what each field of a nit carries (to be aligned with BR-ASDLC-022).
