# 03 — Vocabulary

Canonical names. A tracker adapter maps them to the tracker's own names; configuration may rename the display, never the meaning.

## Statuses of a work item (ticket, bug)

| Canonical | Meaning | Who writes it |
|---|---|---|
| **Triage** | New; nobody has judged it yet. | The author |
| **Backlog** | Judged worth keeping; no contract yet, or not scheduled. | The owner |
| **Ready** | Contract verified by refinement; may be taken. | Refinement (human or the bridge on `ticket check`) |
| **In Progress** | Taken by an implementer. | The bridge when a branch or PR appears; the runtime's claim |
| **In Review** | The PR is ready and CI is green; angle sub-items exist. | The bridge |
| **To Deploy** | Merged; waiting for the next release. A request for deployment, never a claim of completion. | The bridge, at merge |
| **Post Deploy** | Released; only `## Post deploy` boxes remain to be verified in production. | The bridge, at closure, which also creates the ticket's validation sub-task |
| **Done** | Released and every box ticked. | The bridge: at closure, or, for a ticket in Post Deploy, at the tick that finds every box of its `## Post deploy` section checked (no verb: checking the boxes is the gesture) |
| **Canceled** | Will not be done. | The owner |

In Review, To Deploy and Post Deploy are **validation** states: the item is still open and still blocks what depends on it, but no implementer is ever served it. Each of these states belongs to a process (PR review, release cycle, post-release verification) driven by the bridge and the coordinator. A tracker that has status groups puts them in a group of their own, outside what it serves to implementers. Post Deploy in particular stays outside the in-progress group: the post-release verification is served through the ticket's **validation sub-task**, never through the ticket itself.

## Items created by the bridge

| Item | Parent | Purpose |
|---|---|---|
| **Release issue** | none | One per cycle: title names the product and the version; carries the brief and the GO decision. |
| **Angle sub-item** | the release issue, or the PR's ticket | One per configured angle; served to a reviewer; its PR (release) or its findings (PR review) are its output. |
| **Consolidation item** | same parent | Served to the coordinator once every angle has reported. |
| **Validation sub-task** | a ticket the closure moved to Post Deploy | One per ticket, unassigned, in Ready, with a complete contract: an agent verifies the parent's `## Post deploy` boxes in production and checks the ones it can verify, then closes it; a check that needs a human, or is beyond the agent, is named in a comment and the sub-task is reassigned to the owner (`owner.tracker_user`). |
| **Finding ticket** | none, related to the release issue | A non-blocking finding to fix later; labelled `qa-finding`. |
| **Blocker** | none, `blocks` the release issue | A blocking finding that an angle could not fix in its branch. |

## Severities and verdicts

| Severity | Meaning |
|---|---|
| `blocker` | The release must not ship with it. Fixed in the angle branch, or becomes a blocker ticket. |
| `should-fix` | Real, not blocking; becomes a finding ticket. |
| `nit` | Cosmetic; listed in the report, one nits ticket per release. |

| Verdict | Meaning |
|---|---|
| `go` | Nothing blocking from this angle. |
| `go-with-note` | Nothing blocking; findings to carry as tickets. |
| `hold` | At least one blocker not fixed. |

## Labels

`release/<product>-v<version>` on every ticket embarked in a cycle; `qa-finding` on finding tickets; `blocked-by-human` on an item waiting for a human gesture; `post-deploy-validation` on a ticket in Post Deploy (the bridge's index, since a tracker cannot be listed by status: set by the closure, removed once the ticket is resolved).

## Other terms

- **Candidate**: a commit on the release branch, identified by its RC tag.
- **Embarked ticket**: a ticket linked to a PR that merged a commit of the candidate's range.
- **Range**: `<last published release>..<candidate>`.
- **Gate**: the deterministic CI of a candidate (suites, audits, evidence pack).
- **Evidence pack**: the artifact the gate publishes for the reviewers.
- **Brief**: what the bridge posts on the release issue: embarked tickets with their acceptance criteria, business rules touched, link to the evidence pack.
- **Angle branch**: `qa/<candidate>/<angle>`, cut from the release branch by a reviewer.
- **Angle report**: the mandatory file an angle commits in its branch (`06-qa-report.md`).
- **Record**: the concatenation of the angle reports of a cycle, on `main` after the merge-back.
