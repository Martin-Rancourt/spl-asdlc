# 08 — Configuration

One file per repository, `spl-asdlc.toml` at the root, read by the bridge and validated by `spl-asdlc config check`. Everything the method leaves to the project is here; nothing here is read by CI workflows (they take explicit inputs). An invalid file stops the bridge for that repository; nothing falls back to a default shape.

```toml
schema = 1

[product]
name = "<product>"            # used in tags, labels and release issue titles
repository = "<owner>/<repo>"

[tracker]
kind = "linear" | "livela"
team = "<team key or workspace id>"
service_identity = "<name of the service account that writes>"
# Canonical status -> tracker status. Only the ones the tracker names differently.
[tracker.statuses]
"In Review" = "In Review"
"To Deploy" = "To Deploy"
"Post Deploy" = "Post Deploy"

[release]
candidate_tag = "<product>-v{version}-rc.{rc}"
release_tag = "<product>-v{version}"
branch_prefix = "release/"
max_rc_passes = 2
later_passes = "full" | "gate-and-coordinator"
go_policy = "computed" | "human"
record = ".spl-asdlc/qa/{candidate}/"          # where angle reports live
# Multi-component products
# components = ["app", "website", "admin"]
# version_component = "app"
# release_pr_label = "autorelease: pending"

[[release.angles]]
key = "security"
assignee = "<participant hint, optional>"
notes = ".spl-asdlc/angles/security.md"       # optional project notes

[[release.angles]]
key = "product"

[review]
merge_method = "squash" | "merge"
require_up_to_date = true
diff_size_extra_angle = 400
human_approval = "never" | "sensitive-paths" | "always"
sensitive_paths = ".github/review-paths.json"  # globs that add the security angle

[[review.angles]]
key = "maintainability"

[[review.path_angles]]
paths = ["**/auth/**", "**/migrations/**"]
angle = "security"

[bridge]
poll_minutes = 5
github_identity = "app" | "token"
sweep_hours = 24

[[notify]]
kind = "email" | "telegram" | "google-chat"
target = "<address, chat id or webhook name — a secret reference, never a value>"
events = ["release-note", "escalation"]
```

## Rules

1. Secrets are never in the file: `target` and identities are references resolved from the environment or the host's secret store.
2. Tag patterns must contain `{version}` exactly once and `{rc}` exactly once for the candidate; a multi-component product must name its components and its version component.
3. Angle keys are free text; the protocol files are looked up by key under `docs/agents/angles/` of this product, then under the project's notes.
4. `go_policy = "human"` adds the owner's approval on the release issue to the computed conditions; it never replaces them.
5. The sensitive-path file follows the format of `.github/review-paths.json`: a list of globs and a list of documented exceptions; it is read at the PR's base ref, so a PR cannot weaken the list that judges it.
