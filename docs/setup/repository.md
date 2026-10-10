# 2. The repository

**Rulesets** (Settings → Rules → Rulesets):
- `main`: require a pull request, require the status check `required-check`, require branches to be up to date, block force pushes; bypass list: the App only.
- `release/**`: same, bypass list: the App only.
- Tags `*-rc.*` and release tags: block updates and deletions (an RC tag is immutable).

`spl-asdlc doctor` reports one line per ruleset above. `ruleset <default branch>` and `ruleset release/**` (`[release] branch_prefix` followed by `**`) are `ok` when an active branch ruleset that covers the branch requires the status check `required-check`; the release branches are judged on sample names of the configured shape, so no release branch needs to exist. `ruleset tags` is `ok` when active tag rulesets restrict updates and deletions on every tag the configuration creates. It proves coverage of the configured release and candidate tag templates, rather than accepting a few matching sample refs: use `~ALL`, the template's wildcard form (for example `refs/tags/acme-v*`), `refs/tags/*-rc.*` for candidates, or a literal prefix followed by `*`; an exclusion or an ambiguous pattern is reported `missing` conservatively. A ruleset whose enforcement is `evaluate` or `disabled` counts for nothing and makes its line `invalid`. Patterns are read as GitHub reads them: full refs (`refs/heads/…`, `refs/tags/…`), `~DEFAULT_BRANCH`, `~ALL`, the exclusions winning. The doctor does not judge the other rules (pull request, up to date, force pushes) nor the bypass list, which an App's token reads empty: check those in the settings.

**Secrets**: `SPL_ASDLC_APP_ID`, `SPL_ASDLC_APP_PRIVATE_KEY` (the App), plus whatever the project's publication and deployment steps need. Never a tracker key: the CI does not write to the tracker.

**Variables**: `SPL_ASDLC_RUNNER_READY` (`true` to run the gate on the project's self-hosted runner, otherwise the hosted runner is used for what fits there).

**Workflow callers**: one short workflow per reusable workflow, under `.github/workflows/`, calling `<owner>/spl-asdlc/.github/workflows/asdlc-<unit>.yml@v1` with the project's inputs (test command, suites, runner labels, publication and deployment steps). The callers own the triggers, the concurrency groups and the GitHub environment; the reusable workflows own nothing else. Examples, to copy and edit (replace `<owner>`, the `acme` tag patterns, the App's slug and the `make` commands):

| Example | Calls | Trigger |
|---|---|---|
| `docs/setup/examples/pr.yml` | `asdlc-pr-guards.yml`, the project's tests, the `required-check` action | `pull_request` on `main` and `release/**` |
| `docs/setup/examples/pr-title.yml` | `asdlc-pr-title.yml` | `pull_request`, `edited` included; its own workflow so that a title edit never cancels the tests |
| `docs/setup/examples/release-cut.yml` | `asdlc-release-cut.yml` | `workflow_dispatch` (`action`, `class`, the reasons, `dry_run`), dispatched by the bridge |
| `docs/setup/examples/release-gate.yml` | `asdlc-release-gate.yml`, after the project's build | `push` of an RC tag |
| `docs/setup/examples/release-gate-auto-rerun.yml` | `asdlc-release-gate-auto-rerun.yml` | `workflow_run` of `release-gate` |
| `docs/setup/examples/release-publish.yml` | `asdlc-release-publish.yml` three times (`verify`, `record-deployed`, `close`), the project's publication between | `release: published`; `workflow_dispatch` to resume or replay |
| `docs/setup/examples/release-please.yml` | `googleapis/release-please-action`, as the App | `push` to `release/**`; `workflow_dispatch` for the bridge's redispatch, refused on any other ref |

The file names `release-cut.yml`, `release-gate.yml`, `release-gate-auto-rerun.yml` and `release-please.yml` are the defaults of `[release] cut_workflow`, `gate_workflow`, `gate_rerun_workflow` and `release_please_workflow`: the bridge dispatches and reads them by name. A third-party action is pinned by full commit SHA with its version in a comment, as in this repository's own workflows. The tests under `tests/workflows/examples.rs` check every example against the workflow it calls: inputs and secrets declared, required ones given, permissions granted.

**The workflows' tag**: callers name a moving major tag, `v1`, never a branch. The owner of `spl-asdlc` sets `v1` once the first real caller is green on the commit it names, and moves it forward (`git tag -f v1 <commit>`, then `git push -f origin v1`, by the owner) only for compatible changes: a fix, a new input with a default, a new output. A breaking change gets the next major tag at the commit that introduces it, `v2`, and `v1` stays where it was, so existing callers keep working until they move: an input removed, renamed or made required, a new required secret, a default that changes the behaviour, a called job renamed (check names are an interface, REQ-CI-001), or a called job asking for a permission its callers do not grant. The change's PR says which tag it needs, and its release note says what a caller changes to move. A caller that wants an immutable reference pins the full commit SHA instead of `v1`. `v<N>` is not a crate version: the crate is tagged `v<X.Y.Z>`, and only those tags build binaries; the tag ruleset must leave `v<N>` movable by the owner.

The release-please files are this repository's own (`release-please-config.json`, `.release-please-manifest.json`) as an example. When the component stays out of the tag (`include-component-in-tag: false`), release-please leaves it out of its PR title too; release-cut finds the staged version after the component's name, so set `pull-request-title-pattern` to name it, as this repository does.

**Release close**: the publication workflow merges the release branch back into `main` as a merge commit, through the App, which must stay on the `main` bypass list (the merge still waits for `required-check`). When the merge back is DIRTY (a real conflict), resolve it off the release branch, never by bringing `main` into `release/**`: cut `chore/merge-back-<stamp>` from the release branch, merge `main` into that branch and resolve there, open its PR into `main` and merge it as a merge commit, then dispatch the publication workflow on the release branch with `replay: true`, which deletes the branch and publishes nothing. Any other block (`required-check` not green, the App refused by the ruleset) is fixed where it is, then replayed the same way.

**Files** (templates in `docs/setup/templates/`, each valid as written and checked by `tests/setup_files.rs`):
- `spl-asdlc.toml` at the root (`docs/spec/08-configuration.md`); `spl-asdlc config check` must pass. Template: `templates/spl-asdlc.toml`; replace every placeholder, the check cannot tell a placeholder from a real name. A repository that carries no instance data (a public one) commits a neutral file, the method's shape, and leaves the instance keys (the repository, the owner, the team, the logins, the angles' assignees, the channels) to the instance overlay on the bridge's host (`bridge.md`); `config check` then passes with the overlay beside the file or named by `--instance`. A key in both files is an error, never an override (BR-ASDLC-014).
- `.gitignore` lists `spl-asdlc.instance.toml`, so that an overlay kept beside a checkout is never committed.
- `docs/business-rules.md` with the registry format (`docs/spec/02-business-rules.md`), even if empty of rows. Template: `templates/business-rules.md`.
- `.github/review-paths.json`: the sensitive paths and their documented exceptions; the file lists itself. Template: `templates/review-paths.json`.
- `release-please-config.json` and its manifest (release-please computes versions and changelogs; the method reads them). Its `changelog-sections` are the types `asdlc-pr-title.yml` allows (REQ-CI-016).
- Optional: `.spl-asdlc/angles/<angle>.md` notes, `.spl-asdlc/qa/` is written by angle branches.

The ticket skeleton, `templates/ticket.md`, is the contract of `docs/spec/01-ticket-contract.md` for the tracker's ticket template. `spl-asdlc ticket check --description <file>` judges a description before it is served. It reads headings and boxes, not prose, so the unedited skeleton passes it; its Business rules placeholder, though, is read as a stated rule, so a ticket that keeps it is held until the registry has the rule or the section says `None.`.

**Verify**: `spl-asdlc doctor --file <spl-asdlc.toml>` reports each item above as `ok`, `missing` or `invalid`.
