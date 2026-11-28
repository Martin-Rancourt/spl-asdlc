# 3C74 — A patch carries nothing but PRs merged into `main` and the App's own bookkeeping commits of `main`.

Status: Enforced

## Rule

Beside the PRs of `pulls` (R7WM), a patch carries only the commits its caller names in the cut workflow's `carry` input, and only on the `patch` action: each must be on the default branch, not in production, not a merge, and authored and committed by the bridge's App (`<slug>[bot]`, its `users.noreply.github.com` address with or without its user id). They are cherry-picked with `-x` after the PRs, in the order the default branch took them, before `release prepare`; any other commit, or a conflict, refuses the whole patch and nothing is pushed. The patch's tag records them on `Release-Patch-Carry:`. No PR merged such a commit, so its copy embarks no ticket. An empty `carry` changes nothing.

## Evidence

ASDLC-7SWR (from LVL-ZTSN): a project whose release notes live in their own file writes it on `main` in one App commit before the cut; a patch, cut from the tag in production, never got that commit, and its publication refused the tag without the notes file.

## Test

`preflight_accepts_a_patch_that_names_its_version_and_its_prs`, `a_patch_carries_the_apps_commit_after_its_prs`, `an_empty_carry_carries_nothing`, `a_patch_refuses_a_commit_not_by_the_app`, `a_patch_refuses_a_commit_not_on_the_default_branch`, `a_carried_commit_that_does_not_apply_refuses_the_patch`, `the_patch_tag_records_its_carried_commits` (workflow tests, `tests/workflows/release_cut.rs`), `tick::release_tests::patch_cycle::a_carried_commit_embarks_no_ticket` (unit test job).
