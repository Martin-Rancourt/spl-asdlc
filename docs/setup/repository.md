# 2. The repository

**Rulesets** (Settings → Rules → Rulesets):
- `main`: require a pull request, require the status check `required-check`, require branches to be up to date, block force pushes; bypass list: the App only.
- `release/**`: same, bypass list: the App only.
- Tags `*-rc.*` and release tags: block updates and deletions (an RC tag is immutable).

**Secrets**: `SPL_ASDLC_APP_ID`, `SPL_ASDLC_APP_PRIVATE_KEY` (the App), plus whatever the project's publication and deployment steps need. Never a tracker key: the CI does not write to the tracker.

**Variables**: `SPL_ASDLC_RUNNER_READY` (`true` to run the gate on the project's self-hosted runner, otherwise the hosted runner is used for what fits there).

**Workflow callers**: one short workflow per reusable workflow, under `.github/workflows/`, calling `spl-asdlc-ci` at a pinned tag with the project's inputs (test command, suites, runner labels, publication and deployment steps). Examples are in `docs/setup/examples/`.

**Release close**: the publication workflow merges the release branch back into `main` as a merge commit, through the App, which must stay on the `main` bypass list (the merge still waits for `required-check`). When the merge back is DIRTY (a real conflict), resolve it off the release branch, never by bringing `main` into `release/**`: cut `chore/merge-back-<stamp>` from the release branch, merge `main` into that branch and resolve there, open its PR into `main` and merge it as a merge commit, then dispatch the publication workflow on the release branch with `replay: true`, which deletes the branch and publishes nothing. Any other block (`required-check` not green, the App refused by the ruleset) is fixed where it is, then replayed the same way.

**Files**:
- `spl-asdlc.toml` at the root (`docs/spec/08-configuration.md`); `spl-asdlc config check` must pass.
- `docs/business-rules.md` with the registry format (`docs/spec/02-business-rules.md`), even if empty of rows.
- `.github/review-paths.json`: the sensitive paths and their documented exceptions; the file lists itself.
- `release-please-config.json` and its manifest (release-please computes versions and changelogs; the method reads them).
- Optional: `.spl-asdlc/angles/<angle>.md` notes, `.spl-asdlc/qa/` is written by angle branches.

**Verify**: `spl-asdlc doctor --file <spl-asdlc.toml>` reports each item above as `ok`, `missing` or `invalid`.
