# 07 — The bridge

One deterministic process, `spl-asdlc tick`, that turns GitHub facts into tracker state and tracker state into GitHub actions. It is the only writer of statuses. It contains no model call.

## Invariants

1. **It runs in the user's environment**, never in CI: a daemon or scheduled job (a tracker's own runner, a cron, a system timer), or by hand (`spl-asdlc tick` from a shell or a skill). The same binary and the same configuration serve every host.
2. **It polls GitHub.** Default every five minutes, configurable. Nothing needs to be exposed on the network; an optional relay can push webhook events into a queue the bridge pulls, which only shortens the latency.
3. **It writes under one identity**: a GitHub App installed on the repositories (the same App the CI workflows use), with a fallback to a personal token where an App is not allowed; the tracker identity is a service account of the tracker. The binary prints under which names it writes.
4. **Every write is idempotent.** The bridge keeps an action log (append-only, one line per decision: when, repository, object, action, inputs, outcome). A fact already acted upon is skipped; a fact whose action failed is retried at the next tick; a tracker read that fails writes nothing and retries.
5. **It fails closed.** An unreadable configuration, registry, ticket or PR link stops the decision that depends on it and says so in the log; it never guesses and never degrades to "no constraint".
6. **One merge per repository per tick**, so that the next tick re-reads a base that moved.
7. **It leaves no heartbeat in the data.** A tick that finds nothing to do writes nothing, anywhere. A tick that works leaves only its effects (transitions, comments, links, merges), signed by its identity: that is its report. It never rewrites an unchanged value. A failed tick (error, time budget exceeded) is reported once to its host (the host's job log, the action log), never as a fact on an item.

## What a tick does, in order, per configured repository

1. **Read**: open PRs (state, draft, base, head SHA, checks, reviews, threads, labels, linked ticket), release branches and RC tags with their annotations, gate runs per tag and the evidence artifact, release-please PRs, published releases, `deployed: production` labels, merged merge-backs; recent merged PRs for the sweep.
2. **Release coordination** (`05-release-cycle.md`): new branch + tag → release issue, candidate record, labels; RC tagged → candidate record, labels, the previous candidate's consolidation item Done; gate of that RC green → its angle sub-items, consolidation item blocked by each of them (BR-ASDLC-020), then its brief linking the gate's evidence (never before, BR-ASDLC-004); angle PR appeared → its sub-item In Progress; angle PR ready and green → its sub-item In Review; changes requested by a coordinator at its head → its sub-item In Progress; angle PR approved with a valid report → merge into the release branch, its sub-item Done, tickets from the report; GO reached → merge the release-please PR(s); release published → the consolidation items Done; release published and components deployed and merge-back merged → closure and release note; passes exhausted with blockers → escalation.
3. **PR review coordination** (`04-pr-review.md`): PR ready and green with a complete contract and a box ticked → In Review, angle sub-items, consolidation item blocked by each of them (BR-ASDLC-018); review submitted → sub-item done; coordinator approved with every angle sub-item of the round closed and checks green and threads resolved → merge, the PR's round closed, the ticket To Deploy, or where the state rules of `04-pr-review.md` §10 put it; changes requested → In Progress, unless another PR of the ticket is in review; an open PR on a ticket past the merge → the ticket back where §10 puts it.
4. **Sweep**: PRs merged outside the bridge in the last 24 hours get the same transitions, once; the round of every PR merged or closed without merge in that window is closed, and a ticket left In Review by a PR closed without merge goes where `04-pr-review.md` §10 puts it.
5. **Post-deploy validation** (`05-release-cycle.md` §11): every ticket labelled `post-deploy-validation/<product>` and still in Post Deploy whose `## Post deploy` boxes are all checked (the Done gate) → Done, and the label removed; one that still waits gets its validation sub-task if it has none (found by its marker among the ticket's children, never created twice); a resolved ticket loses the label. No verb.
6. **Write the action log**, then exit (or sleep until the next tick when run as a daemon).

## What it reads on GitHub (the facts)

| Fact | Where |
|---|---|
| PR readiness, checks, reviews, threads, labels, head SHA, base, up-to-date | Pull request API, checks API, review threads |
| Linked ticket | The tracker's attachment for the PR URL (tracker side), or the item URL in the PR body as a fallback declared by configuration |
| Release branch, RC tag, class | Refs and annotated tag message |
| Gate result and evidence pack | Workflow run on the tag's SHA; artifact `release-gate-evidence-<sha>` |
| Angle report | The file `.spl-asdlc/qa/<candidate>/<angle>.json` at the angle PR's head |
| Release-please PR and note | PR by branch prefix and label; the changelog section in its body |
| Published release, deployed components | Releases API; labels on the merged component release PRs |
| Merge-back | PR from `release/*` into `main`, merged |

## What it writes to the tracker (the `Tracker` trait)

Read an item and its description; create an item (with parent, labels, assignee hint); set a status by canonical name; add a label; comment; add a relation (`blocks`, `duplicate`); link an item to a PR URL and find an item by a PR URL; list the children of an item; find the release issue of a product and version; list items by label. Nothing else. Every text it writes is English.

## What it writes to GitHub

Merges (REST, pinned to a head SHA), branch deletions, labels, comments on PRs (one per cause, never repeated), the dispatch of the cut workflow when the policy tags the next RC automatically.

## Verbs

| Verb | Purpose |
|---|---|
| `tick` | One pass; `--dry-run` writes only the log of what it would do |
| `release cut --product <p> [--open]` | Dispatch the cut workflow (new cycle or next RC) |
| `release status --product <p>` | The GO decision and its inputs, as text and JSON |
| `release close --product <p> --version <v>` | Force the closure of a cycle the facts say is over (human waiver, logged) |
| `release requalify <ticket> --severity <s> --reason <r>` | Human requalification of a finding |
| `ticket check <item>` | Contract judgement |
| `config check` | Validate `spl-asdlc.toml`, merged with its instance overlay (spec 08) |
| `doctor` | Verify the setup of a repository (App, secrets, rulesets, tracker identity, config and its instance overlay) |
