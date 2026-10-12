# Manual gestures

The bridge automates every step of the flow, and is never the only actor able to perform one (BR-ASDLC-021). This page gives, for each step, the exact command or GitHub click to do it by hand, and what the bridge does at its next tick. The normative list of gestures, and what a step done by hand implies, is spec 09's audit (`docs/spec/09-requalification-and-human-gestures.md`, "Every step can be done by a human in GitHub"); this page is its runbook and adds nothing to it.

Conventions:

- `OWNER/REPO` is the repository; every `gh` command takes `--repo OWNER/REPO`, or runs from a checkout of it.
- `<release branch>` is the cycle's release branch, `release/<stamp>-utc`; `<rc tag>` and `<release tag>` follow the patterns of `[release]` in `spl-asdlc.toml`.
- The workflow file names are the defaults of `[release]` (`cut_workflow = "release-cut.yml"`, `gate_workflow = "release-gate.yml"`) and of the example callers (`docs/setup/examples/`): use the project's own names when they differ.
- The bridge's verbs (`spl-asdlc release ...`, `spl-asdlc tick`) run where the bridge runs (`bridge.md`), with its configuration, its instance overlay and its credentials.
- Who may merge or push where is the rulesets' decision (`repository.md`). A gesture the ruleset refuses is not worked around: the ruleset is changed, or the step is left to the bridge.

Before a gesture, see what the bridge sees: `spl-asdlc tick --dry-run` prints what the next tick would do, each held PR on a line `waiting: pr#<n> <reason>`, and `spl-asdlc release status` prints the GO decision and its inputs (exit 1 when there is no GO).

## Merge a PR

When the bridge holds a delivery PR (the `waiting:` line says why) and a person decides to merge it anyway:

```sh
gh pr view <n> --repo OWNER/REPO --json headRefOid --jq .headRefOid
gh pr merge <n> --repo OWNER/REPO --squash --delete-branch --match-head-commit <head sha>
```

Or in GitHub: the PR, **Squash and merge**. Use the merge method `[review]` configures (squash by default). An angle PR into the release branch is merged as a merge commit (`--merge`), as the bridge merges it.

**The bridge, next tick**: the sweep sees the PR merged and gives the ticket the same transition the bridge's own merge would have (`docs/spec/04-pr-review.md` §7, §10), once, with a comment that names who merged; the PR's review round closes. The ticket goes To Deploy when every box is ticked and no other PR of it is open (Done, or Post Deploy, without a release cycle). For an angle PR, the release pass reads it from the PRs merged into the release branch: its sub-item Done, the findings of its report filed, once each.

## Open a release cycle

```sh
gh workflow run release-cut.yml --repo OWNER/REPO --ref main -f action=open
```

Or, on the bridge's host, `spl-asdlc release cut --open`, which dispatches the same workflow. In GitHub: **Actions**, the cut workflow, **Run workflow** on `main`, action `open`. Add `-f dry_run=true` to print the plan and push nothing.

**What follows**: the cut prepares the release on the new branch (`spl-asdlc release prepare`: the version from the commits since the last release, the version files, the CHANGELOG section that is the release notes, all in the run's log), commits `chore(release): v<X.Y.Z>` and pushes the branch with its first candidate; the tag push starts the gate. A range with nothing to release refuses the run and creates nothing. **The bridge, next tick**: it creates the release issue, the candidate record and the labels.

To see the version and the notes before opening, in a checkout of `main`: `spl-asdlc release prepare --repository OWNER/REPO --dry-run --notes notes.md` (or the dispatch with `-f dry_run=true`). To force a version, prepare by hand on a release branch instead of the dispatch: `spl-asdlc release prepare --repository OWNER/REPO --version <X.Y.Z>`, commit `chore(release): v<X.Y.Z>`, then cut the candidate below on that branch.

## Cut a candidate

```sh
gh workflow run release-cut.yml --repo OWNER/REPO --ref <release branch> -f action=candidate
```

Always on the release branch, never `--ref main`. Or `spl-asdlc release cut` on the bridge's host. Never push an RC tag by hand: a hand-pushed tag skips the cut's checks (one cycle at a time, the release class in the annotation).

**What follows**: the tag push starts the gate. **The bridge, next tick**: the candidate record, its labels, the previous candidate's consolidation item Done; once the gate is green, the angle sub-items and the brief.

## Rerun the gate

The gate auto-rerun caller (`release-gate-auto-rerun.yml`) reruns a red gate run once by itself, when its suites never reported or every failed suite is an unexpired known flake. Any other rerun is by hand, on the gate's run for the RC tag:

```sh
gh run list --repo OWNER/REPO --workflow release-gate.yml --branch <rc tag> --limit 1
gh run rerun <run id> --repo OWNER/REPO --failed
```

Or in GitHub: the run, **Re-run jobs**, **Re-run failed jobs**. Never dispatch the gate workflow to rerun it: a dispatch is for diagnostics and certifies nothing.

**The bridge, next tick**: it reads the run on the tag. Green: the angle sub-items, the brief, and the gate-fix tickets the green run made moot closed. Red once the rerun settled: one urgent gate-fix ticket under the release issue, if there is none yet.

## Tag and publish the release by hand

A person the tag rulesets let tag may give the GO instead of the bridge, and takes it on themselves: the computed conditions (angles merged, no blocker, the candidate the branch's tip) are the bridge's, and only the publication's own checks still hold (the candidate's gate green, the release tag on the candidate's commit, or on a commit that adds nothing but its own QA reports). To keep the bridge from tagging first, add the hands-off label to the release issue.

```sh
git fetch origin --tags
git tag -a <release tag> <latest candidate tag>^{commit} -m "Release <release tag>"
git push origin <release tag>
git show <latest candidate tag>:CHANGELOG.md | awk '/^## \[?<X.Y.Z>[] ]/ {on=1; print; next} /^## / {on=0} on' > notes.md
gh release create <release tag> --repo OWNER/REPO --title "<product> <X.Y.Z>" --notes-file notes.md
```

The notes are the version's section of the CHANGELOG the cut committed: copy it whole, so that the release note of the closure finds each PR's line.

**What follows**: the publication runs on the published release; it accepts a release published by the App or by a human account, refuses any other bot, and says which (`go-published-by-kind`, its summary). **The bridge, next tick**: a tag a person pushed on the candidate is taken as done, the bridge then only publishes the release; once the release is published it tags and publishes nothing more and posts no GO of its own; it closes the cycle from the published release, and its closure comment records who published it.

## Publish by hand when the publication's build failed

When the build jobs of the publication run produced their artifacts and only the attach to the release failed. A transient failure needs no gesture: `gh run rerun <run id> --failed` reruns the failed jobs and the stages after them, the merge back included. A build job that failed left no artifact: rerun it first.

1. Download the run's build artifacts; each lands in a folder of its own, gathered into one:

   ```sh
   gh run download <run id> --repo OWNER/REPO --dir artifacts
   mkdir dist && find artifacts -type f -exec mv {} dist/ \;
   ```

2. Write the checksums, from inside `dist`:

   ```sh
   cd dist && sha256sum * > SHA256SUMS      # macOS: shasum -a 256 * > SHA256SUMS
   ```

3. Attach the files to the release:

   ```sh
   gh release upload <release tag> * --repo OWNER/REPO --clobber
   ```

4. Merge the release branch back, publishing nothing:

   ```sh
   gh workflow run release-publish.yml --repo OWNER/REPO --ref <release branch> -f replay=true
   ```

**The bridge, next tick**: the release is published; once the replay merged the branch back, the closure runs (see Close).

## Merge back by hand

The close stage opens a merge-back PR from the release branch into the default branch. A person may merge it instead, as a merge commit only, never a squash or a rebase: the release tag's commit must become an ancestor of the default branch.

```sh
gh pr list --repo OWNER/REPO --base main --head <release branch>
gh pr merge <n> --repo OWNER/REPO --merge
```

Or in GitHub: the PR, **Create a merge commit**. Then let the close stage delete the branch, or dispatch a replay (`-f replay=true`, above), which deletes it and publishes nothing. A real conflict is resolved off the release branch, never by bringing `main` into it: `repository.md`, "Release close".

**What follows**: the close stage takes a PR merged while it waits, a branch already held by the default branch, and a branch already merged and deleted as done, and stays green. **The bridge, next tick**: see Close.

## Close

The closure is the bridge's, not a GitHub gesture: it writes the tracker. A person brings about the facts it waits for:

- **the release published**;
- **the merge back merged**: no release branch holds the candidate, and the default branch holds the release tag.

**The bridge, next tick** after both: the closure (the embarked tickets judged, the release note, the release issue closed), then the archiving of what the cycle resolved. A release issue closed by hand in the tracker skips the judging and the archiving.

## Move a ticket by hand, and hold it

A status is the bridge's write (BR-ASDLC-011), derived from GitHub facts at each tick. The usual gesture is the fact in GitHub (open, ready or merge the PR, check a box), and the bridge moves the ticket at its next tick.

To move a ticket the bridge would move otherwise, hold it first: add the hands-off label (`spl-asdlc:hands-off` by default) to the tracker item, and to its PR when the PR must not be touched either (`gh pr edit <n> --repo OWNER/REPO --add-label spl-asdlc:hands-off`), then move the item in the tracker. **The bridge, next tick**: it leaves a labelled item or PR alone (no status, no assignment, no comment, no merge) and says so once in the tick's report. Removing the label hands it back: the next tick derives its status from the facts again.

## Dismiss a review

The bridge dismisses an App's request for changes that the coordinator's approval supersedes; it never dismisses a person's, which holds the merge (`waiting: pr#<n> changes requested by <login> (a person; the bridge never dismisses it)`). Its author may review again, or a person with the right dismisses it:

```sh
gh api repos/OWNER/REPO/pulls/<n>/reviews --jq '.[] | select(.state == "CHANGES_REQUESTED") | [.id, .user.login] | @tsv'
gh api --method PUT repos/OWNER/REPO/pulls/<n>/reviews/<review id>/dismissals -f message="<why>" -f event=DISMISS
```

Or in GitHub: the review, its menu, **Dismiss review**, with a message.

**The bridge, next tick**: the request no longer stands; the PR merges if every other condition holds, or the `waiting:` line names the next reason.

## Abandon a cycle

On the bridge's host:

```sh
spl-asdlc release abandon --file <spl-asdlc.toml> --product <p>
```

It keeps the tags, merges the release branch back into `main`, cancels the release issue and turns the remaining blockers into tickets. A new cycle starts with Open a release cycle.

## Requalify a finding

On the bridge's host:

```sh
spl-asdlc release requalify <ticket> --severity <blocker|should-fix|nit> --reason "<text>"
```

It changes the finding ticket's severity, adds or removes its `blocks` relation to the release issue, and records who, when and why on the ticket and on the release issue. **The bridge, next tick**: the GO is recomputed.

## Commands a session needs permission for

An AI session (Claude Code, for instance) can take these gestures on the owner's go. The owner allows the command prefixes below, in the permission rules of the session's settings (`permissions.allow` to run without a prompt, `permissions.ask` to confirm each run):

| Rule | Gesture |
|---|---|
| `Bash(gh pr merge *)` | Merge a PR, the merge-back PR |
| `Bash(gh pr edit *)` | Add or remove a label (the hands-off label) |
| `Bash(git push origin <release tag>)`, `Bash(gh release create *)` | Tag and publish the release by hand |
| `Bash(gh workflow run *)` | Open a cycle, cut a candidate, replay the publication |
| `Bash(gh run rerun *)` | Rerun the gate, rerun a failed publication job |
| `Bash(gh run download *)` | Download a run's build artifacts |
| `Bash(gh release upload *)` | Attach files to a release |
| `Bash(gh api --method PUT repos/OWNER/REPO/pulls/*/reviews/*/dismissals *)` | Dismiss a review |
| `Bash(ssh <bridge host> spl-asdlc release status*)`, `Bash(ssh <bridge host> spl-asdlc tick --dry-run*)` | Read the GO and what the next tick would do |
| `Bash(ssh <bridge host> spl-asdlc release *)` | Cut, requalify, abandon |

The reads (`gh pr list`, `gh pr view`, `gh run list`, `gh run view`, `gh api` without `--method`) change nothing. Every rule of the table but the two dry reads is an **outward action**: it merges, labels, dispatches, publishes or writes the tracker, on the shared repository, and most cannot be undone (a merge, an uploaded asset that replaced another, an abandoned cycle). A rule only lets the session run the command; each gesture still waits for the owner's go.
