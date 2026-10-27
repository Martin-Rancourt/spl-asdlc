# Setting up SPL-ASDLC for a repository

Four steps, in this order. `spl-asdlc doctor` verifies every point below and names what is missing; the procedure is written so that each sentence maps to one check.

1. `github-app.md` — create the GitHub App the CI and the bridge write with, and install it on the repository.
2. `repository.md` — rulesets, secrets, variables, the workflow callers, and the `.spl-asdlc/` folder: `config.toml`, `business-rules/`, `qa/` and the agents' process page `process.md`, with the project's agent file `AGENTS.md` beside it (the PR guards check the page on every PR, `doctor` does not).
3. `tracker-linear.md` or `tracker-livela.md` — the service identity and the statuses.
4. `bridge.md` — where the bridge runs (a tracker's runner, a timer at the interval the host chooses, a skill) and with which secrets.

A project on Livela starts from Livela's guided setup, not from this list: see « Starting a project: the guided setup in Livela » in `tracker-livela.md`. A host application that embeds the crate (the Livela runner, `tracker-livela.md`) takes steps 2 and 4 itself: `spl_asdlc::setup::files` gives the files of the repository's setup PR (step 2's configuration, workflow callers pinned at the embedded minor line `@vX.Y`, process page and `AGENTS.md`), which the host opens with the person's GitHub identity; the App, its secrets and the rulesets stay the person's steps, and the host's `doctor` checks each one.

Once it runs, any step of the flow can be taken over by hand: `manual-gestures.md` gives each step's command or click, what the bridge does at its next tick, and the commands an AI session needs permission for to take them on the owner's go.

## Optional steps

The method has one procedure (`docs/agents/process.md`, BR-ASDLC-027). A project's configuration only says which of its optional steps are present: PR review, and QA angles in the release cycle. A step that is absent is skipped; no configuration changes how a present step works. The release cycle itself is not optional: every project has one. Each optional step is a few keys in `.spl-asdlc/config.toml`. There is no mode or flow to choose: the configuration's keys are the choice.

| Step | Configuration | What the method does |
|---|---|---|
| The core (the template as written) | `angles = []` under `[review]`, and the `[release]` section with `angles = []` | Tickets with their contract, PRs, CI and the bridge. No PR review: a PR merges on its CI, with no round and no approval (`docs/reference/04-pr-review.md` §11). Merged tickets wait To Deploy; a release is cut, its CI runs, and its GO rests on that CI (no QA angle). |
| PR review | `[[review.angles]]` tables with `review.coordinators` | Each PR gets a review round on the angles that apply to it and merges on a coordinator's approval. An angle runs on every PR, or only on PRs that touch its paths (its `paths`, `docs/reference/08-configuration.md`). |
| Release QA | `[[release.angles]]` tables with `release.coordinators` | Each candidate is reviewed on its QA angles before the GO. |

In every configuration, `release.fix_assignee` names the tracker participant who fixes release problems: the bridge assigns them the urgent fix ticket of a red release CI and, with QA angles, that of each blocking finding. Left unset, those tickets are unassigned, taken by role.

The optional steps are independent: a project may add them in either order, or neither. Adding one needs no migration: the bridge applies it from its next tick, tickets already Done stay Done, and release angles apply from the next candidate.
