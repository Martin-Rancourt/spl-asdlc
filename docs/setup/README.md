# Setting up SPL-ASDLC for a repository

Four steps, in this order. `spl-asdlc doctor` verifies every point below and names what is missing; the procedure is written so that each sentence maps to one check.

1. `github-app.md` — create the GitHub App the CI and the bridge write with, and install it on the repository.
2. `repository.md` — rulesets, secrets, variables, the workflow callers, `spl-asdlc.toml`, the registry and the sensitive paths; a light project skips the release steps (below).
3. `tracker-linear.md` or `tracker-livela.md` — the service identity and the statuses.
4. `bridge.md` — where the bridge runs (a tracker's runner, a cron, a timer, a skill) and with which secrets.

## Start light, add layers

A project starts light and adds the method's layers as it grows. Each layer is a few keys in `spl-asdlc.toml` (the template shows them, commented, under "Layers") plus, for the release cycle, the setup steps of `repository.md` it needs. There is no mode to set: the configuration's keys are the choice.

| Level | Configuration | What the method does |
|---|---|---|
| **Light** (the template as written) | `angles = []` under `[review]`, no `[release]` section | Tickets with their contract, PRs, CI and the bridge, with an empty business-rule registry. No PR review: a PR merges on its CI, with no round and no approval (`docs/spec/04-pr-review.md` §11). No release cycle: a merged ticket is Done (`docs/spec/05-release-cycle.md`, "Without a release cycle"). |
| **Standard** | Light, plus one `[[review.angles]]` table with `review.coordinators`, and the `[release]` section with `angles = []` | Each PR gets a review round and merges on a coordinator's approval. Merged tickets wait To Deploy; a release is cut, gated and published, and its GO rests on the gate (no QA angle). |
| **Full** | Standard, plus `[[release.angles]]` with `release.coordinators`, and `review.sensitive_paths` | Each candidate is reviewed on its QA angles before the GO. A PR that touches a sensitive path (`.github/review-paths.json`) gets the sensitive angle. |

The layers are independent: a project may take them in another order, or stop at any of them. Adding one needs no migration: the bridge applies it from its next tick, tickets already Done stay Done, and release angles apply from the next candidate.
