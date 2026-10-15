# Setting up SPL-ASDLC for a repository

Four steps, in this order. `spl-asdlc doctor` verifies every point below and names what is missing; the procedure is written so that each sentence maps to one check.

1. `github-app.md` — create the GitHub App the CI and the bridge write with, and install it on the repository.
2. `repository.md` — rulesets, secrets, variables, the workflow callers, and the `.spl-asdlc/` folder: `config.toml`, `business-rules/` and `qa/`.
3. `tracker-linear.md` or `tracker-livela.md` — the service identity and the statuses.
4. `bridge.md` — where the bridge runs (a tracker's runner, a timer at the interval the host chooses, a skill) and with which secrets.

Once it runs, any step of the flow can be taken over by hand: `manual-gestures.md` gives each step's command or click, what the bridge does at its next tick, and the commands an AI session needs permission for to take them on the owner's go.

## Start light, add layers

A project starts light and adds the method's optional layers as it grows: PR review, and QA angles in the release cycle. The release cycle itself is not a layer: every project has one. Each layer is a few keys in `.spl-asdlc/config.toml`. There is no mode to set: the configuration's keys are the choice.

| Level | Configuration | What the method does |
|---|---|---|
| **Light** (the template, its `[release]` section uncommented) | `angles = []` under `[review]`, and the `[release]` section with `angles = []` | Tickets with their contract, PRs, CI and the bridge, with no business rule yet. No PR review: a PR merges on its CI, with no round and no approval (`docs/spec/04-pr-review.md` §11). Merged tickets wait To Deploy; a release is cut, its CI runs, and its GO rests on that CI (no QA angle). |
| **Standard** | Light, plus `[[review.angles]]` tables with `review.coordinators` | Each PR gets a review round on the angles that apply to it and merges on a coordinator's approval. An angle runs on every PR, or only on PRs that touch its paths (`[[review.path_angles]]`, `docs/spec/08-configuration.md`). |
| **Full** | Standard, plus `[[release.angles]]` with `release.coordinators` | Each candidate is reviewed on its QA angles before the GO. |

At any level, `release.fix_assignee` names the tracker participant who fixes release problems: the bridge assigns them the urgent fix ticket of a red release CI and, with QA angles, that of each blocking finding. Left unset, those tickets are unassigned, taken by role.

The layers are independent: a project may take them in another order, or stop at any of them. Adding one needs no migration: the bridge applies it from its next tick, tickets already Done stay Done, and release angles apply from the next candidate.
