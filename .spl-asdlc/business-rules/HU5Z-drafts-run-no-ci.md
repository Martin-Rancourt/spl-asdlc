# HU5Z — Ongoing work runs no CI.

Status: Enforced

## Rule

Implementers work on a branch without a PR or in a draft PR. Every CI job of the PR workflow is gated off for a draft, its required check included, and a draft never passes; readiness starts CI. The agents' instructions (`docs/agents/process.md`, step 3) require it.

## Evidence

Owner’s decisions of 2026-10-27, ASDLC-NN5K (parallel PRs, phase 1).

## Test

`workflows::examples::the_pr_workflow_is_unfiltered_and_aggregates_every_job`, `workflows::own::the_pr_workflow_is_unfiltered_runs_on_prs_only_and_aggregates_every_job`, `workflows::required_check::a_draft_pr_never_passes_even_with_every_job_green` (workflow tests). A project's own PR workflow is its to gate the same way (`docs/setup/repository.md`).
