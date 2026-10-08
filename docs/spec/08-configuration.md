# 08 — Configuration

One file per repository, `spl-asdlc.toml` at the root, read by the bridge and validated by `spl-asdlc config check`. Everything the method leaves to the project is here; nothing here is read by CI workflows (they take explicit inputs). An invalid file stops the bridge for that repository; nothing falls back to a default shape.

## Example

A complete file. The example below is valid as written: the crate's test suite parses it.

```toml
schema = 1

[product]
name = "acme"                          # used in tags, labels and release issue titles
repository = "example-org/acme"        # <owner>/<repo> on GitHub

[owner]                                # the human of the method: escalations, human GO
tracker_user = "owner-handle"          # the owner as the tracker knows them
github_login = "owner-login"           # required when go_policy = "human"

[tracker]
kind = "linear"                        # "linear" | "livela"
team = "ABC"                           # team key or workspace id
service_identity = "spl-asdlc"         # the service account that writes
team_keys = ["ABC"]                    # identifier prefixes of the team's items
link_fallback = "none"                 # "none" | "body-url"

# Canonical status -> tracker status. Only the ones the tracker names differently.
[tracker.statuses]
"To Deploy" = "To deploy"
"Post Deploy" = "Post deploy"

[release]
candidate_tag = "acme-v{version}-rc.{rc}"
release_tag = "acme-v{version}"
branch_prefix = "release/"
max_rc_passes = 2
later_passes = "full"                  # "full" | "gate-and-coordinator"
go_policy = "computed"                 # "computed" | "human"
nits = "shared"                        # "shared" | "per-nit": one nits ticket per release, or one per nit
record = ".spl-asdlc/qa/{candidate}/"  # where angle reports live
release_pr_label = "autorelease: pending"
release_please_authors = ["app/release-app"]
bookkeeping_paths = [".release-please-manifest.json", "CHANGELOG.md"]
registry_path = "docs/business-rules.md"
business_rule_paths = ["docs/business-rules.md", "**/business-rules.md"]
stagnation_hours = 2
coordinators = ["coordinator-login"]
escalation_label = "blocked-by-human"
auto_candidate = false
cut_workflow = "release-cut.yml"
gate_workflow = "release-gate.yml"
release_please_workflow = "release-please.yml"
release_please_config = "release-please-config.json"
release_please_manifest = ".release-please-manifest.json"
# Multi-component products
# components = ["app", "website", "admin"]
# version_component = "app"
# gate_record = "docs/releases/v{version}.md"

[[release.angles]]
key = "security"
assignee = "reviewer-handle"           # participant hint, optional
notes = ".spl-asdlc/angles/security.md" # optional project notes

[[release.angles]]
key = "product"

[review]
merge_method = "squash"                # "squash" | "merge"
require_up_to_date = true
diff_size_extra_angle = 1500
diff_size_angle = "maintainability"
human_approval = "never"               # "never" | "sensitive-paths" | "always"
sensitive_paths = ".github/review-paths.json"  # globs that add the sensitive angle
sensitive_angle = "security"
coordinators = ["coordinator-login"]
human_reviewer = "owner-login"         # optional: asked to look at a green PR stuck on a human
ticketless_authors = ["dependabot"]    # Apps whose PRs need no ticket: the bridge opens one (spec 04 §9)

[[review.angles]]
key = "maintainability"

[[review.path_angles]]
paths = ["**/auth/**", "**/migrations/**"]
angle = "security"

[bridge]
poll_minutes = 5
github_identity = "app"                # "app" | "token"
sweep_hours = 24

[[notify]]
kind = "telegram"
target = "env:SPL_ASDLC_TELEGRAM_CHAT" # a secret reference, never a value
token = "env:SPL_ASDLC_TELEGRAM_TOKEN"
events = ["release-note", "escalation", "stagnation", "stuck-pr", "blind-pass"]
```

## Rules

1. Secrets are never in the file. A channel's `target` and `token` are references `env:<VARIABLE>` (`VARIABLE`: capital letters, digits and `_`, not starting with a digit) resolved from the environment of the host that runs the bridge; a literal value there is an error. Identities (`service_identity`, logins, `github_identity`) are names, never credentials: the credentials themselves come from the host (`docs/setup/bridge.md`).
2. Tag patterns must contain `{version}` exactly once and `{rc}` exactly once for the candidate; the release tag contains `{version}` exactly once and no `{rc}`; `record` contains `{candidate}` exactly once and `gate_record` `{version}` exactly once; no other placeholder is accepted. A multi-component product must name its components, its version component and its gate record; a version component without components is an error.
3. Angle keys name branches (`qa/<candidate>/<angle>`) and files (`<angle>.json`): lower-case letters, digits and hyphens, not starting with a hyphen, unique per list. Product and component names follow the same shape (they appear in tags and labels). The protocol files are looked up by key under `docs/agents/angles/` of this product, then under the project's notes.
4. `go_policy = "human"` adds the owner's approval on the release issue to the computed conditions; it never replaces them.
5. The sensitive-path file follows the format of `.github/review-paths.json`: a list of globs and a list of documented exceptions; it is read at the PR's base ref, so a PR cannot weaken the list that judges it. When `sensitive_paths` is declared, the file must exist and be valid at the base: absent or unreadable, the PR waits, one human notice says "configuration invalid for this repository", and `doctor` reports it — never "no sensitive path". Only when the key is not declared at all are there no sensitive paths.
6. Unknown keys are errors (a misspelt key is never a silently ignored setting). `config check` lists every problem of a well-formed file; a file that is not valid TOML, carries an unknown key or a value of the wrong type (a string where a number is expected) is reported by its first such error, with its line.
7. Every default below is what the bridge uses when the key is absent; a present key with an invalid value is an error, never replaced by its default.
8. A list never names the same entry twice (paths, logins, components, events): a repeated entry is an error, not a harmless duplicate, because it usually hides a typo next to it.

## Keys

"Required" keys have no default: their absence makes the file invalid. "Fail-closed" says what the bridge does when the value is wrong or what it names cannot be read at run time.

### Top level and `[product]`

| Key | Default | Meaning | Fail-closed |
|---|---|---|---|
| `schema` | required | Version of this format; `1`. | Another version: file invalid. |
| `product.name` | required | Product name in tags, labels (`release/<product>-v<version>`) and release issue titles; lower-case letters, digits and hyphens. | Invalid: file invalid. |
| `product.repository` | required | `<owner>/<repo>` on GitHub. | Unreadable through the bridge's identity: the repository is skipped for the tick, `doctor` reports it. |

### `[owner]`

| Key | Default | Meaning | Fail-closed |
|---|---|---|---|
| `owner.tracker_user` | required | The owner as the tracker knows them; escalations are assigned to them (`05-release-cycle.md` §10), and a validation sub-task names them as whom to reassign a check an agent cannot verify (§11). | Unknown to the tracker: the escalation is reported as failed and retried; `doctor` reports it. |
| `owner.github_login` | none; required when `go_policy = "human"` | The login whose `/go` on the release issue counts as the human GO. | Absent under a human policy: file invalid. |

### `[tracker]`

| Key | Default | Meaning | Fail-closed |
|---|---|---|---|
| `tracker.kind` | required | `linear` or `livela`. | Other value: file invalid. |
| `tracker.team` | required | Team key (Linear) or workspace id (Livela) where release items and finding tickets are created. | Unknown team: no release item is created, the candidate waits ("no team"). |
| `tracker.service_identity` | required | Name of the service account the bridge writes as; printed by the binary. | A token for another identity: `doctor` reports the mismatch. |
| `tracker.team_keys` | `[]` (ask the tracker) | Identifier prefixes (`ABC`; capital letters and digits, starting with a letter) of the team's items; only those count as embarked tickets of a release. | Empty and the tracker cannot say: the embarked list is unreadable, the brief is not posted. |
| `tracker.link_fallback` | `"none"` | How a PR names its ticket when the tracker holds no link for the PR: `none` (only the tracker's own link), `body-url` (the tracker item URL in the PR body). Titles, branch names and bare identifiers are never read. | No link: the PR is not reviewed nor merged; a green PR raises one human notice. |
| `tracker.statuses` | identity for every canonical status | Canonical status → tracker status, for the ones named differently. | A non-canonical key, or two canonical statuses mapped to one tracker status: file invalid. A mapped status the team lacks: the write fails and is retried; `doctor` reports it. |

### `[release]`

| Key | Default | Meaning | Fail-closed |
|---|---|---|---|
| `release.candidate_tag` | `<product>-v{version}-rc.{rc}` | RC tag pattern. | Placeholders wrong: file invalid. |
| `release.release_tag` | `<product>-v{version}` | Final tag pattern. | Placeholders wrong: file invalid. |
| `release.branch_prefix` | `release/` | Prefix of release branches; the rest is `<YYYY-MM-DD-HH-MM>-utc`. | Not a path ending in `/`: file invalid. |
| `release.max_rc_passes` | `2` | RC passes with QA before escalation to the owner. | Not a positive integer: file invalid. |
| `release.later_passes` | `"full"` | Whether passes after the first run every angle (`full`) or only the gate and the coordinator. | Other value: file invalid. |
| `release.go_policy` | `"computed"` | `human` adds the owner's approval to the computed GO. | Other value: file invalid. |
| `release.nits` | `"shared"` | How the nits of a release become tickets (`05-release-cycle.md` §6), chosen for the tracker: `shared`, one nits ticket per release whose boxes are the nits, for a tracker that limits its ticket count (Linear's free plan); `per-nit`, one ticket per nit in `Triage`, keyed by candidate, angle and finding id, for a tracker that does not (Livela). Either way each ticket carries a full contract whose goal is to triage. | Other value: file invalid. A shared nits ticket that carries no contract: left alone, said at each tick. |
| `release.record` | `.spl-asdlc/qa/{candidate}/` | Directory of the angle reports (`06-qa-report.md`). | Without `{candidate}`: file invalid. |
| `release.components` | `[]` | Components of a multi-component product, each with its own release PR. | Invalid names: file invalid. |
| `release.version_component` | none; required with components | The component whose release ends the cycle; merged last. | Missing, or not a component: file invalid. |
| `release.gate_record` | none; required with components | Path of the gate record (`{version}` once) the deployment reads. | Missing with components: file invalid. |
| `release.release_pr_label` | `autorelease: pending` | Label that identifies release-please PRs. | — |
| `release.release_please_authors` | required | GitHub logins allowed to author release-please PRs (`<slug>[bot]` and `app/<slug>` are both accepted at run time). | A release-shaped PR by anyone else is ignored and logged, never merged. |
| `release.bookkeeping_paths` | `[]` | Files allowed to differ between the latest RC and the release branch tip (release-please manifest, changelog, version files). | Empty: the RC must equal the tip exactly; anything else is a tip gap and the GO is false. |
| `release.registry_path` | `docs/business-rules.md` | The business-rule registry (`02-business-rules.md`). | Unreadable at the judged ref: the guard fails closed. |
| `release.business_rule_paths` | `["docs/business-rules.md", "**/business-rules.md"]` | Paths whose change the brief singles out as "business rules touched". | — |
| `release.stagnation_hours` | `2` | A GO false for longer than this is reported once. | Not a positive integer: file invalid. |
| `release.coordinators` | required | GitHub logins whose approval at the head SHA lets the bridge merge an angle PR. | No approval by one of them: the angle PR is not merged. |
| `release.escalation_label` | `blocked-by-human` | Label added to the release issue when it waits for the owner. | — |
| `release.auto_candidate` | `false` | When true the bridge dispatches the next RC itself once a pass is merged; otherwise the coordinator does. | — |
| `release.cut_workflow` | `release-cut.yml` | Workflow the bridge dispatches for `release cut`. | Not a file name: file invalid; absent on GitHub: the dispatch fails and says so. |
| `release.gate_workflow` | `release-gate.yml` | Workflow whose run on the RC tag is the gate. | No run on the tag: the gate is not green. |
| `release.release_please_workflow` | `release-please.yml` | Workflow redispatched once when release-please's PR is stale. | — |
| `release.release_please_config` | `release-please-config.json` | release-please's configuration (its `config-file`), read at the release PR's head for each component's changelog path. | Unreadable for a multi-component product: a note gap, the GO is false. |
| `release.release_please_manifest` | `.release-please-manifest.json` | release-please's manifest (its `manifest-file`). | — |
| `[[release.angles]]` | at least one required | `key` (required), `title` (default `<Key> review`), `assignee` (participant hint), `notes` (path). | No angle: file invalid — a release with no angle would compute its GO from nobody. |

### `[review]`

| Key | Default | Meaning | Fail-closed |
|---|---|---|---|
| `review.merge_method` | `"squash"` | Merge method of ordinary PRs. | Other value: file invalid. |
| `review.require_up_to_date` | `true` | Bring a behind PR up to date with its base before merging. | — |
| `review.diff_size_extra_angle` | `1500` | Changed lines (additions + deletions) above which `diff_size_angle` is added; `0` disables it. | Negative: file invalid. |
| `review.diff_size_angle` | none; required while the threshold is above 0 | The angle the size trigger adds. | Missing with a threshold: file invalid. |
| `review.human_approval` | `"never"` | Whether a human must approve besides the coordinator: `never`, `sensitive-paths`, `always`. | `sensitive-paths` without `sensitive_paths`: file invalid. |
| `review.sensitive_paths` | not declared | Path of the sensitive-path list read at the PR's base (rule 5). | Declared and absent or unreadable: the PR waits, one human notice. |
| `review.sensitive_angle` | `security` | The angle a sensitive path adds. | — |
| `review.coordinators` | required | GitHub logins whose approval at the head SHA lets the bridge merge a PR. A later `COMMENTED` review never cancels an approval; a standing `CHANGES_REQUESTED` from any configured login blocks. | No approval at the head SHA: not merged. |
| `review.ticketless_authors` | `[]` | Apps whose PRs need no ticket (`<slug>`, `<slug>[bot]` or `app/<slug>`): for such a PR linked to no ticket, the bridge opens one labelled `ticketless-pr` with a complete contract, and the PR takes the usual path (spec 04 §9). | A human account whose login matches never qualifies; any other PR without a ticket waits. |
| `review.human_reviewer` | none | GitHub login asked to look at a green PR stuck on something only a human can lift. | Absent: the notice says that a reviewer must be assigned by hand. |
| `[[review.angles]]` | at least one required | Base angles of every PR review: `key` (required), `assignee`. | No angle: file invalid. |
| `[[review.path_angles]]` | none | `paths` (globs, `*` crosses `/`) and the `angle` they add. | — |

### `[bridge]`

| Key | Default | Meaning | Fail-closed |
|---|---|---|---|
| `bridge.poll_minutes` | `5` | Cadence of the tick when run as a daemon. | Not a positive integer: file invalid. |
| `bridge.github_identity` | `"app"` | Write as the GitHub App, or as a personal token where an App is not allowed. | Credentials missing on the host: the bridge refuses to start and names them. |
| `bridge.sweep_hours` | `24` | Window of the sweep of PRs merged outside the bridge. | Not a positive integer: file invalid. |

### `[[notify]]`

| Key | Default | Meaning | Fail-closed |
|---|---|---|---|
| `notify.kind` | required | The channel; this build ships `telegram`. | A channel the build does not ship: file invalid. |
| `notify.target` | required | Reference to the destination (chat id, address, webhook). | Not a reference: file invalid; unset on the host: the notice is not sent, logged as an error, retried. |
| `notify.token` | required for `telegram` | Reference to the channel's credential. | As `target`. |
| `notify.events` | required | Any of `release-note`, `escalation`, `stagnation`, `stuck-pr`, `blind-pass`. | Unknown event: file invalid. |
