# spl-asdlc — notes for the security angle

These notes add spl-asdlc's own surfaces to the method's angle (`docs/agents/angles/security.md`); they never replace it (BR-ASDLC-031). spl-asdlc has no user, no screen and no data store of its own: what it holds is other repositories' write access, and what it ships is code that other repositories run.

## Surfaces

- **The bridge's credentials** (identity and privilege): the GitHub App's key and the installation tokens it mints (`src/github/auth.rs`), and the tracker's key (`src/tracker/`). Look for a token that outlives its use, a token sent to a host other than the API it was minted for, and a secret value that reaches a log, an error, a comment or the action log (`src/action_log.rs`).
- **What the bridge reads from GitHub and the tracker** (input and cost): PR bodies, titles, labels, ticket descriptions and comments are written by anyone who can open a PR or edit a ticket. Look for a field that changes what the bridge merges, tags or publishes, and for a response or an artifact with no size limit (`src/http.rs`, `src/github/zip.rs`).
- **The HTTP transport** (ingress and response): redirects stay explicit and checked by the caller; a redirect followed by the transport itself was a blocker once.
- **The reusable workflows and the action** (delivery chain): `.github/workflows/asdlc-*.yml` and `actions/` run in every repository on the method, with that repository's tokens. Look for a `permissions` wider than the job needs, a secret passed to a job or step that does not use it, an action not pinned to a full commit, a shell step that interpolates `${{ }}` from a PR or an event, and a `workflow_run` or `pull_request_target` trigger that runs untrusted code with secrets.
- **This repository's release chain** (delivery chain): the moving minor tag `v<X.Y>` every caller follows, the release binaries (`binaries.yml`) and the crate (`release-publish.yml`, environment `crates-io`). A release a person or the App did not publish, or a tag moved backwards, reaches every caller at once.
- **Rule zero** (data): no instance data in the tree (BR-ASDLC-001); `scripts/check-no-instance-names.sh` is the check, and a candidate that adds an identifier form it misses is a finding.

## Checks

- `cargo test --locked` passes on the candidate, workflow tests included (`tests/workflows/`).
- `scripts/check-no-instance-names.sh` passes on the candidate.
