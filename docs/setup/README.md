# Setting up SPL-ASDLC for a repository

Four steps, in this order. `spl-asdlc doctor` verifies every point below and names what is missing; the procedure is written so that each sentence maps to one check.

1. `github-app.md` — create the GitHub App the CI and the bridge write with, and install it on the repository.
2. `repository.md` — rulesets, secrets, variables, the workflow callers, `spl-asdlc.toml`, the registry and the sensitive paths.
3. `tracker-linear.md` or `tracker-livela.md` — the service identity and the statuses.
4. `bridge.md` — where the bridge runs (a tracker's runner, a cron, a timer, a skill) and with which secrets.
