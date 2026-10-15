# 08 — Configuration

One file per repository, `spl-asdlc.toml` at the root, read by the bridge and validated by `spl-asdlc config check`. Everything the method leaves to the project is here; nothing here is read by CI workflows (they take explicit inputs) but by the cut's `spl-asdlc release prepare`, which reads the release's shape from the checked-out file (the product's name, the release tag pattern, `release.version_files`, `release.changelog_sections`), never an identity nor an instance overlay: it accepts a file whose instance keys an overlay carries, and refuses an unknown key as always. The identities of the installation (who and where) may instead come from an instance overlay kept on the host that runs the bridge, outside the repository, so that the repository's file stays neutral (see "The instance overlay"). An invalid file, or an invalid merge of the two, stops the bridge for that repository; nothing falls back to a default shape.

## Example

The repository's file, then its instance overlay. Both are valid as written: the crate's test suite parses them and merges them. The example leaves out some optional keys; the tables under "Keys" list every key with its default. A project that keeps everything in one file writes the overlay's keys in its `spl-asdlc.toml`, each in its table, and an angle's `assignee` in that angle's entry.

```toml
schema = 1

[product]
name = "acme"                          # used in tags, labels and release issue titles

[tracker]
kind = "linear"                        # "linear" | "livela"
link_fallback = "none"                 # "none" | "body-url"
no_deploy_label = "no-deploy"          # a ticket with nothing to deploy: Done at its merge

[release]
candidate_tag = "acme-v{version}-rc.{rc}"
release_tag = "acme-v{version}"
branch_prefix = "release/"
max_rc_passes = 2
later_passes = "full"                  # "full" | "gate-and-coordinator"
go_policy = "computed"                 # "computed" | "human"
record = ".spl-asdlc/qa/{candidate}/"  # where angle reports live
bookkeeping_paths = []                  # files allowed to change after the candidate
registry_path = "docs/business-rules.md"
business_rule_paths = ["docs/business-rules.md", "**/business-rules.md"]
stagnation_hours = 2
escalation_label = "blocked-by-human"
auto_candidate = false
cut_workflow = "release-cut.yml"
gate_workflow = "release-gate.yml"
gate_rerun_workflow = "release-gate-auto-rerun.yml"
gate_rerun_wait_minutes = 30
# changelog_sections: the defaults, one section per Conventional Commit type

[[release.version_files]]               # Cargo.toml and Cargo.lock need no entry
path = "web/package.json"
pattern = '"version": "{version}"'

[[release.version_files]]
path = "docs/version.txt"
pattern = "acme {version}"

[[release.angles]]
key = "security"
notes = ".spl-asdlc/angles/security.md" # optional project notes; the assignee is in the overlay

[[release.angles]]
key = "product"

[review]
merge_method = "squash"                # "squash" | "merge"
require_up_to_date = true
diff_size_extra_angle = 1500            # off (0) by default; above 0 it names the angle it adds
diff_size_angle = "maintainability"
human_approval = "never"               # "never" | "sensitive-paths" | "always"
sensitive_paths = ".github/review-paths.json"  # globs that add the sensitive angle
sensitive_angle = "security"
ticketless_authors = ["dependabot"]    # Apps whose PRs need no ticket: the bridge opens one (spec 04 §9)
# No PR review instead (spec 04 §11): `angles = []` here, no path angle, no
# sensitive_paths, no size threshold; review.coordinators is then optional.

[[review.angles]]                      # base angles of every PR review
key = "maintainability"
notes = ".spl-asdlc/angles/maintainability.md" # optional project notes, linked from the angle's sub-items

[[review.path_angles]]
paths = ["**/auth/**", "**/migrations/**"]
angle = "security"

[bridge]
poll_minutes = 5
github_identity = "app"                # "app" | "token"
sweep_hours = 24
# hands_off_label = "spl-asdlc:hands-off"  # the default; the label by which a person takes an object over
```

The instance overlay, `spl-asdlc.instance.toml` on the host:

```toml
schema = 1

[product]
repository = "example-org/acme"        # <owner>/<repo> on GitHub

[owner]                                # the human of the method: escalations, human GO
tracker_user = "owner-handle"          # the owner as the tracker knows them
github_login = "owner-login"           # required when go_policy = "human"

[tracker]
team = "ABC"                           # team key or workspace id
service_identity = "spl-asdlc"         # the service account that writes
team_keys = ["ABC"]                    # identifier prefixes of the team's items

[release]
coordinators = ["coordinator-login"]
coordinator_assignee = "coordinator-handle" # optional: receives each candidate's consolidation item, and each angle item In Review
fix_assignee = "fixer-handle"          # optional: receives the urgent fix tickets of a red gate and of a blocking finding

[[release.angles]]
key = "security"                       # an angle the repository's file declares
assignee = "reviewer-handle"           # participant hint, optional

[review]
coordinators = ["coordinator-login"]
coordinator_assignee = "coordinator-handle" # optional: receives each PR round's consolidation item
human_reviewer = "owner-login"         # optional: asked to look at a green PR stuck on a human

[[notify]]
kind = "telegram"
target = "env:SPL_ASDLC_TELEGRAM_CHAT" # a secret reference, never a value
token = "env:SPL_ASDLC_TELEGRAM_TOKEN"
events = ["release-note", "escalation", "stagnation", "stuck-pr", "blind-pass"]
```

## Rules

1. Secrets are never in the file. A channel's `target` and `token` are references `env:<VARIABLE>` (`VARIABLE`: capital letters, digits and `_`, not starting with a digit) resolved from the environment of the host that runs the bridge; a literal value there is an error. Identities (`service_identity`, logins, `github_identity`) are names, never credentials: the credentials themselves come from the host (`docs/setup/bridge.md`).
2. Tag patterns must contain `{version}` exactly once and `{rc}` exactly once for the candidate; the release tag contains `{version}` exactly once and no `{rc}`; `record` contains `{candidate}` exactly once and a version file's `pattern` `{version}` exactly once; no other placeholder is accepted. A multi-component product (a non-empty `release.components`, a `release.version_component` or a `release.gate_record`) is refused: the release cycle prepares one version per product (`05-release-cycle.md` §1, §9), and a product whose components ship their own versions is not supported yet.
3. Angle keys name branches (`qa/<candidate>/<angle>`) and files (`<angle>.json`): lower-case letters, digits and hyphens, not starting with a hyphen, unique per list. Product names follow the same shape (they appear in tags and labels). The bridge embeds in each angle sub-item the method's file of that key under `docs/agents/angles/` of this product, compiled into its binary, or a line saying the method has no notes for an angle of the project's own; the project's notes (`notes`) are linked from the item, never embedded (BR-ASDLC-015).
4. `go_policy = "human"` adds the owner's approval on the release issue to the computed conditions; it never replaces them. That approval is **not implemented**: the bridge does not read it (`docs/open-points.md` #3), so under `human` it never reaches a GO, and a person tags and publishes the release instead (`09-requalification-and-human-gestures.md`).
5. The sensitive-path file follows the format of `.github/review-paths.json`: a list of globs and a list of documented exceptions; it is read at the PR's base ref, so a PR cannot weaken the list that judges it. When `sensitive_paths` is declared, the file must exist and be valid at the base: absent or unreadable, the PR waits, one human notice says "configuration invalid for this repository", and `doctor` reports it — never "no sensitive path". Only when the key is not declared at all are there no sensitive paths.
6. Unknown keys are errors (a misspelt key is never a silently ignored setting). `config check` lists every problem of a well-formed file; a file that is not valid TOML, carries an unknown key or a value of the wrong type (a string where a number is expected) is reported by its first such error, with its line.
7. Every default below is what the bridge uses when the key is absent; a present key with an invalid value is an error, never replaced by its default.
8. A list never names the same entry twice (paths, logins, version files, CHANGELOG types, events): a repeated entry is an error, not a harmless duplicate, because it usually hides a typo next to it.
9. Write a GitHub App as `<slug>[bot]` or `app/<slug>` (the one alias, read as `<slug>[bot]`); a bare login is a person (or a user account), never an App. The bridge reads every review and PR author in the REST spelling: GitHub's GraphQL API names an App by its bare slug, and the bridge normalises it to `<slug>[bot]` when it reads it. It then compares a configured login with an author exactly, ignoring case only. In `release.coordinators` and `review.coordinators`, `<slug>[bot]` matches only the App, and a person who registered the login `<slug>` never approves in its name; a bare `<slug>` matches only the account `<slug>`, never the App (fail closed). `review.ticketless_authors` names only Apps (a human author never qualifies), so there a bare `<slug>` means the App too.
10. No PR review is an explicit choice (`04-pr-review.md` §11): `angles = []` under `[review]` means no base angle, while a file that writes neither `angles = []` nor a `[[review.angles]]` table is invalid, never read as "no angle". A PR can get an angle when the project has a base angle, a `[[review.path_angles]]` entry, a declared `sensitive_paths` or a `diff_size_extra_angle` above 0 (the size trigger is off by default); `review.coordinators` is required whenever one can, and optional when none can (the project then has no PR review: a PR merges on its CI and the other merge conditions, with no round and no approval). `human_approval` other than `never` is accepted with no possible angle: the human's approval is then the only one required.

## The instance overlay

The repository's file carries the method's shape (tags, angles, review rules, workflows); a repository that must carry no instance data (a public one, or one that serves several installations) leaves the identities of its installation to an overlay file on the host that runs the bridge. The overlay is optional: a private project may keep everything in `spl-asdlc.toml`.

**Instance keys**, the only keys an overlay may carry:

| Key | Why it is instance data |
|---|---|
| `schema` | The overlay's format; `1`, as the repository's file. Required in the overlay. |
| `product.repository` | Where the product lives. |
| `owner.tracker_user`, `owner.github_login` | Who the owner is. |
| `tracker.team`, `tracker.service_identity`, `tracker.team_keys` | Which team, under which account. |
| `release.coordinators` | Logins. |
| `review.coordinators`, `review.human_reviewer` | Logins. |
| `release.coordinator_assignee`, `release.fix_assignee`, `review.coordinator_assignee` | Participants. |
| the `assignee` of a `[[release.angles]]` or `[[review.angles]]` entry | A participant. Given as an entry with `key` and `assignee` only; `key` must name an angle the repository's file declares, never declare one. |
| `[[notify]]` | Destinations (as references, rule 1). The channels are one key: all in one file. |

Every other key is the method's shape and stays in the repository's file, where it is reviewed: `tracker.kind`, `review.ticketless_authors` and `bridge.github_identity` included.

**Merge.** The bridge reads the repository's file ∪ the overlay, fail-closed:
1. Each file is first read alone: not valid TOML, an unknown key, a value of the wrong type is an error with its line (rule 6). In the overlay, a key that is not an instance key is an error ("not an instance key"), with its line.
2. A key set in both files is an error, never an override, whatever the two values are: a silent override would let a file outside review hide a reviewed setting (BR-ASDLC-014). So is an angle's `assignee` set in both, and `[[notify]]` tables in both.
3. Required keys and every rule above are checked on the merged result; each error names the file its value comes from.
4. The overlay never creates the `[release]` section: a release key in the overlay (a login, a participant, an angle's `assignee`) while the repository's file has no `[release]` section is an error, since the project chose no release cycle and the key would complete nothing.

**Where it is found.** Every command that takes `--file` takes `--instance <path>`. Without `--instance`, the overlay is the sibling of the `--file` when there is one, and there is none otherwise: `spl-asdlc.instance.toml` beside a file named `spl-asdlc.toml`, `<stem>.instance.toml` beside any other (`acme.toml` → `acme.instance.toml`), so that two configurations in one directory never share an overlay. A named `--instance` that does not exist is an error; nothing at the sibling's path only means no overlay, while something there that is not a readable file (a directory, a dangling link) is an error. `tick`, which takes several `--file`, accepts `--instance` only with a single `--file` and refuses it otherwise: each configuration's overlay is then its sibling. A host that hands the bridge a configuration path (the Livela runner) gets the sibling the same way. `doctor` reports `instance overlay` (the path read, or `none`) and judges the repository's file at the default branch merged with that overlay. The overlay is never committed: this repository's `.gitignore` lists `spl-asdlc.instance.toml`, and a project that keeps an overlay beside a checkout does the same.

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
| `owner.github_login` | none; required when `go_policy = "human"` | The login whose `/go` on the release issue is meant to count as the human GO; not read yet (rule 4): a person tags and publishes the release instead (spec 09). | Absent under a human policy: file invalid. |

### `[tracker]`

| Key | Default | Meaning | Fail-closed |
|---|---|---|---|
| `tracker.kind` | required | `linear` or `livela`. | Other value: file invalid. |
| `tracker.team` | required | Team key (Linear) or workspace id (Livela) where release items and finding tickets are created. | Unknown team: no release item is created, the candidate waits ("no team"). |
| `tracker.service_identity` | required | Name of the service account the bridge writes as; printed by the binary. | A token for another identity: `doctor` reports the mismatch. |
| `tracker.team_keys` | `[]` (ask the tracker) | Identifier prefixes (`ABC`; capital letters and digits, starting with a letter) of the team's items; only those count as embarked tickets of a release. | Empty and the tracker cannot say: the embarked list is unreadable, the brief is not posted. |
| `tracker.link_fallback` | `"none"` | How a PR names its ticket when the tracker holds no link for the PR: `none` (only the tracker's own link), `body-url` (the tracker item URL in the PR body). Titles, branch names and bare identifiers are never read. | No link: the PR is not reviewed nor merged; a green PR raises one human notice. |
| `tracker.no_deploy_label` | `"no-deploy"` | The tracker label of a ticket with nothing to deploy (documentation, say). Its merge delivers it: the ticket the state rules would put To Deploy goes Done instead (Post Deploy with its validation sub-task while its own `## Post deploy` boxes wait, In Progress while a box of a duplicate is open), and no release embarks it (`04-pr-review.md` §10). A person or refinement puts it on the ticket; the bridge only reads it and never creates it. `doctor` says whether the tracker has it. | Absent from a ticket: To Deploy at the merge, as any. Absent from the tracker: nothing changes until a ticket carries it. |

The configuration maps no status: the tracker names each canonical status (`03-vocabulary.md`) exactly so, and the bridge reads and writes them by that name. A `[tracker.statuses]` table is an unknown key (rule 6). A canonical status the team lacks: the write fails and is retried; `doctor` names it. An adapter whose tracker keys statuses by its own ids maps them in its code.

### `[release]`

The section is optional, and so is the release cycle it configures (`05-release-cycle.md`, "Without a release cycle"): without it, none of the keys below is read or required, `release.coordinators` and `[[release.angles]]` included, and a merged ticket whose every box is ticked is Done. Two defaults still serve the PR review: its business-rule guard reads the registry at `docs/business-rules.md` (`release.registry_path`), and the PR pass leaves alone a PR whose head branch starts with `release/` (`release.branch_prefix`), so that a merge back is never squashed. Present, the section is the release cycle and every row below applies; adding it later switches to the full cycle at the next tick, with no migration.

| Key | Default | Meaning | Fail-closed |
|---|---|---|---|
| `release.candidate_tag` | `<product>-v{version}-rc.{rc}` | RC tag pattern. | Placeholders wrong: file invalid. |
| `release.release_tag` | `<product>-v{version}` | Final tag pattern. | Placeholders wrong: file invalid. |
| `release.branch_prefix` | `release/` | Prefix of release branches; the rest is `<YYYY-MM-DD-HH-MM>-utc`. | Not a path ending in `/`: file invalid. |
| `release.max_rc_passes` | `2` | RC passes with QA before escalation to the owner. | Not a positive integer: file invalid. |
| `release.later_passes` | `"full"` | Whether passes after the first run every angle (`full`) or only the gate and the coordinator. | Other value: file invalid. |
| `release.go_policy` | `"computed"` | `human` adds the owner's approval to the computed GO; that approval is not read yet, so under `human` the bridge never reaches a GO (rule 4). | Other value: file invalid. |
| `release.record` | `.spl-asdlc/qa/{candidate}/` | Directory of the angle reports (`06-qa-report.md`). | Without `{candidate}`: file invalid. |
| `release.bookkeeping_paths` | `[]` | Files allowed to differ between the latest RC and the release branch tip (a file the project's own tooling writes on the branch after the candidate), as exact paths. The reports of the latest RC's own angles (`record` rendered for that RC) are always allowed and need no entry (`05-release-cycle.md` §8). The release itself is tagged on the RC's commit, so none of these files reaches it. | Empty: the RC must equal the tip, but for those; anything else is a tip gap and the GO is false. |
| `release.registry_path` | `docs/business-rules.md` | The business-rule registry (`02-business-rules.md`). | Unreadable at the judged ref: the guard fails closed. |
| `release.business_rule_paths` | `["docs/business-rules.md", "**/business-rules.md"]` | Paths whose change the brief singles out as "business rules touched". | — |
| `release.stagnation_hours` | `2` | A GO false for longer than this is reported once. | Not a positive integer: file invalid. |
| `release.coordinators` | required with `[release]` while there is a release angle; optional with `angles = []` | GitHub logins whose approval at the head SHA lets the bridge merge an angle PR; a coordinator App is written `<slug>[bot]` or `app/<slug>`, a bare login being a person (rule 9). | No approval by one of them: the angle PR is not merged. Absent or empty while a release angle is configured: file invalid. |
| `release.coordinator_assignee` | none | The tracker participant (an agent the tracker serves, like an angle's `assignee`) who receives the consolidation item of each candidate, assigned when the bridge creates it (`05-release-cycle.md` §3), and each angle sub-item when it goes In Review, in the same write, to review its angle PR (§4, BR-ASDLC-020); an existing consolidation item is never reassigned. | Absent, or unknown to the tracker: the consolidation item is created unassigned, and an angle sub-item goes In Review with its assignee left as it is; `doctor` reports an unknown participant. |
| `release.fix_assignee` | none | The tracker participant who fixes release problems (an agent the tracker serves, like an angle's `assignee`): the gate-fix ticket of a red gate (`05-release-cycle.md` §3, BR-ASDLC-016) and the urgent fix ticket of a blocking QA finding (§5, BR-ASDLC-022) are created assigned to it, as is a finding requalified as blocking (`09-requalification-and-human-gestures.md`). | Absent, or unknown to the tracker: those tickets are created unassigned, taken by role; `doctor` reports an unknown participant. |
| `release.escalation_label` | `blocked-by-human` | Label added to the release issue when it waits for the owner; the bridge creates it at the first escalation when the tracker lacks it. | — (`doctor` says whether it exists yet) |
| `release.auto_candidate` | `false` | When true the bridge dispatches the next RC itself once a pass is merged; otherwise the coordinator does. | — |
| `release.cut_workflow` | `release-cut.yml` | Workflow the bridge dispatches for `release cut`. | Not a file name: file invalid; absent on GitHub: the dispatch fails and says so. |
| `release.gate_workflow` | `release-gate.yml` | Workflow whose run on the RC tag is the gate. | No run on the tag: the gate is not green. |
| `release.gate_rerun_workflow` | `release-gate-auto-rerun.yml` | Workflow whose `workflow_run` of a red gate reruns it once (a runner abort, known flakes); the bridge opens a red gate's gate-fix ticket once this run of the gate's first attempt completed, or once `gate_rerun_wait_minutes` passed with no such run (`05-release-cycle.md` §3). | Not a file name: file invalid; its runs unreadable: no gate-fix ticket yet, said at each tick; absent on GitHub: said at each tick, and read as no run, so the bounded wait opens the ticket; never run for the gate run: the bounded wait opens the ticket. |
| `release.gate_rerun_wait_minutes` | `30` | How long after a red gate run completed the bridge waits for a run of `gate_rerun_workflow` to show up; past it with none visible, it opens the gate-fix ticket anyway, saying no rerun was seen (the rerun workflow may be absent or broken). A rerun run that showed up and is still running keeps the wait (`05-release-cycle.md` §3). | Not a positive integer: file invalid. |
| `[[release.version_files]]` | none | The files besides Rust's own that `release prepare` writes the version into, each a `path` inside the repository and a `pattern` holding `{version}` once (literal text around it, such as `"version": "{version}"`); every match of the pattern, its `{version}` any semantic version, gets the new one. `Cargo.toml` (`[package]` or `[workspace.package]`) and `Cargo.lock` (the local packages at the old version) need no entry: `release prepare` writes them natively, with no network. | A pattern without `{version}` once, or a path outside the repository: file invalid. A file absent, or one nothing matches, at the cut: `release prepare` refuses and the cut creates nothing; `doctor` reports a missing file. |
| `[[release.changelog_sections]]` | one section per type `asdlc-pr-title.yml` allows by default: `feat` Features, `fix` Bug Fixes, `perf` Performance, `revert` Reverts, `docs` Documentation, `style` Style, `chore` Chores, `refactor` Refactoring, `test` Tests, `build` Build, `ci` Continuous Integration | The CHANGELOG sections of the release notes, in order: a Conventional Commit `type` (lower-case letters) and its `section` title. A type no section lists is left out of the notes and bumps nothing; a breaking change is always listed, first, under `⚠ BREAKING CHANGES`. Keep it equal to the types the PR title check allows (REQ-CI-016). | A type twice, not lower-case, or `[]`: file invalid. |
| `[[release.angles]]` | required with `[release]`: at least one entry, or `angles = []` written under `[release]` | `key` (required), `title` (default `<Key> review`), `assignee` (participant hint: the angle item is created assigned to it, and is assigned to it again when a coordinator sends its angle PR back, `05-release-cycle.md` §4), `notes` (path). `angles = []` is a release with no QA angle: the bridge opens no angle sub-item and no consolidation item, the brief says no QA angle is configured, and the GO rests on the gate and the conditions that involve no angle (`05-release-cycle.md` §3, §8). Angles configured later apply from the next candidate: a candidate recorded with none keeps none. | No angle written at all: file invalid — the file forgot them, it never means none. An `assignee` unknown to the tracker: the angle item is created unassigned, and unassigned when sent back; `doctor` reports it. |

`release.release_pr_label` and the `release.release_please_*` keys are unknown keys, refused (rule 6).

### `[review]`

| Key | Default | Meaning | Fail-closed |
|---|---|---|---|
| `review.merge_method` | `"squash"` | Merge method of ordinary PRs. | Other value: file invalid. |
| `review.require_up_to_date` | `true` | Bring a behind PR up to date with its base before merging. | — |
| `review.diff_size_extra_angle` | `0` (no size trigger) | Changed lines (additions + deletions) above which `diff_size_angle` is added; `0` adds no angle by size. A project that wants the size trigger sets both this threshold and `diff_size_angle`. | Negative: file invalid. |
| `review.diff_size_angle` | none; required while the threshold is above 0 | The angle the size trigger adds. | Missing with a threshold: file invalid. |
| `review.human_approval` | `"never"` | Whether a human must approve besides the coordinator (alone, for a PR with no angle): `never`, `sensitive-paths`, `always`. | `sensitive-paths` without `sensitive_paths`: file invalid. |
| `review.sensitive_paths` | not declared | Path of the sensitive-path list read at the PR's base (rule 5). | Declared and absent or unreadable: the PR waits, one human notice. |
| `review.sensitive_angle` | `security` | The angle a sensitive path adds. | — |
| `review.coordinators` | required while a PR can get an angle (rule 10); otherwise none | GitHub logins whose approval at the head SHA lets the bridge merge a PR that has a review round; a coordinator App is written `<slug>[bot]` or `app/<slug>`, a bare login being a person (rule 9); a PR with no angle needs none. A later `COMMENTED` review never cancels an approval; a standing `CHANGES_REQUESTED` from any configured login blocks. | No approval at the head SHA: a PR with a round is not merged, and the tick's report says so with the approvals it did find (spec 07). Absent or empty while a PR can get an angle: file invalid. |
| `review.coordinator_assignee` | none | The tracker participant (an agent the tracker serves, like an angle's `assignee`) who receives the consolidation item of each PR round, assigned when the bridge creates it (`04-pr-review.md` §3); an existing consolidation item is never reassigned. | Absent, or unknown to the tracker: the item is created unassigned; `doctor` reports an unknown participant. |
| `review.ticketless_authors` | `[]` | Apps whose PRs need no ticket (`<slug>`, `<slug>[bot]` or `app/<slug>`, rule 9): for such a PR linked to no ticket, the bridge opens one labelled `ticketless-pr` with a complete contract, and the PR takes the usual path (spec 04 §9). | A human account whose login matches never qualifies; any other PR without a ticket waits. |
| `review.human_reviewer` | none | GitHub login asked to look at a green PR stuck on something only a human can lift. | Absent: the notice says that a reviewer must be assigned by hand. |
| `[[review.angles]]` | required: one table per base angle, or `angles = []` under `[review]` for none | Base angles of every PR review: `key` (required), `assignee` (participant hint), `notes` (path, linked from the angle's sub-items). `angles = []` is no base angle: with no other angle possible, no PR review (rule 10, `04-pr-review.md` §11). | Neither written: file invalid — no PR review is said, never assumed. An `assignee` unknown to the tracker: the angle item is created unassigned; `doctor` reports it. |
| `[[review.path_angles]]` | none | `paths` (globs, `*` crosses `/`) and the `angle` they add. | — |

### `[bridge]`

| Key | Default | Meaning | Fail-closed |
|---|---|---|---|
| `bridge.poll_minutes` | `5` | Cadence of the tick when run as a daemon. | Not a positive integer: file invalid. |
| `bridge.github_identity` | `"app"` | Write as the GitHub App, or as a personal token where an App is not allowed. | Credentials missing on the host: the bridge refuses to start and names them. |
| `bridge.sweep_hours` | `24` | Window of the sweep of PRs merged outside the bridge. | Not a positive integer: file invalid. |
| `bridge.hands_off_label` | `spl-asdlc:hands-off` | The label by which a person takes a tracker item, or a GitHub PR (the GitHub label of the same name), over by hand: the tick writes nothing to an object that carries it (`07-the-bridge.md`, "Hands off"; `09-requalification-and-human-gestures.md`). It sits under `[bridge]` because it bounds what the bridge itself writes, on the tracker and on GitHub alike, with or without a review or a release cycle. The bridge never creates it: a person adds it where they need it. | Empty: file invalid. Absent from the tracker or from GitHub: nothing is under it; `doctor` says whether the tracker has it, never as a missing piece. |

### `[[notify]]`

| Key | Default | Meaning | Fail-closed |
|---|---|---|---|
| `notify.kind` | required | The channel; this build ships `telegram`. | A channel the build does not ship: file invalid. |
| `notify.target` | required | Reference to the destination (chat id, address, webhook). | Not a reference: file invalid; unset on the host: the notice is not sent, logged as an error, retried. |
| `notify.token` | required for `telegram` | Reference to the channel's credential. | As `target`. |
| `notify.events` | required | Any of `release-note`, `escalation`, `stagnation`, `stuck-pr`, `blind-pass`. An event no channel takes has nothing to send: its comment is the notice, never a failure (spec 07, "What it sends"). | Unknown event: file invalid. |
