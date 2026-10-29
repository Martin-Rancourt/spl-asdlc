# 03 — Vocabulary

Canonical names. A tracker names its statuses exactly so; the configuration maps none (`08-configuration.md`). An adapter whose tracker keys statuses by its own ids maps them in its code.

## Statuses of a work item (ticket, bug)

| Canonical | Meaning | Who writes it |
|---|---|---|
| **Triage** | New; nobody has judged it yet. On Livela, whose tickets have no Triage (triage is an item type of its own there), a ticket the bridge writes in Triage lands in Backlog and reads as Backlog (`livela-contract.md` §1). | The author |
| **Backlog** | Judged worth keeping; refined or not. Refinement leaves a refined ticket in Backlog and marks it refined: a label in Linear, a `refined` field Livela computes from the description (a goal, at least one acceptance criterion and a release note), which nobody writes in Livela (`docs/agents/process.md`, "Refine a ticket"). The bridge neither writes nor reads that marker. | The owner; refinement marks it refined |
| **Ready** | Refined, and released to agents: implementing agents take work from Ready only. | A person; `ticket check` only judges and writes nothing |
| **In Progress** | Taken: work in progress, its PR not opened yet or in draft. | Whoever starts the ticket, agent or person (with Livela, the agent's verb that serves it), never the bridge as a backup: a branch or a draft PR on a ticket still Ready writes nothing, and a PR opened puts it In Review; the bridge when a coordinator requests changes on its PR at the head, or when a PR resolves and a box remains open with no PR in review (`04-pr-review.md` §10); for a release angle sub-item, when its angle PR is opened or a coordinator requests changes on it at its head; a draft PR is out of the flow and moves nothing (`05-release-cycle.md` §4) |
| **In Review** | A PR of the ticket is opened, not a draft, whatever its CI says: CI runs first, and the review round (angle sub-items and consolidation item), when the plan has angles, opens once CI is green (`04-pr-review.md` §1, §3). A draft PR is out of the flow: the bridge ignores it and writes nothing for it, so the ticket stays where it is, In Review included when the PR is put in draft during its round. A release angle sub-item whose angle PR is opened (not a draft) with its required checks green (`05-release-cycle.md` §4). | The bridge, at the tick that finds the PR opened (from Backlog, Ready or In Progress, with no In Progress on the way); after a request for changes, when the round of the new head opens |
| **To Deploy** | Merged, every box ticked (the `## Post deploy` ones apart), no PR open; waiting for the next release. A request for deployment, never a claim of completion. Never written for a ticket with nothing to deploy (the `tracker.no_deploy_label`). | The bridge, at merge |
| **Post Deploy** | Released (with nothing to deploy: merged); only `## Post deploy` boxes remain to be verified in production. | The bridge, at closure, or at the merge that delivered the ticket; the ticket itself is then validated in production (BR-ASDLC-003) |
| **Done** | Released and every box ticked; for a ticket with nothing to deploy (the `tracker.no_deploy_label`), merged and every box ticked (merging is delivering, `docs/method.md`, "Merge"). For an item the bridge created: its work is over (below). | The bridge: at closure, at merge for a ticket with nothing to deploy, or, for a ticket in Post Deploy, at the tick that finds every box of its `## Post deploy` section checked (no verb: checking the boxes is the gesture); a release angle sub-item when the bridge merges its angle PR, or finds it merged by a person (never To Deploy: it is method work, the release issue tracks the delivery); a release consolidation item when a newer candidate is tagged or the release is published |
| **Canceled** | Will not be done. | The owner |

In Review, To Deploy and Post Deploy are **validation** states: the item is still open and still blocks what depends on it, but no implementer is ever served it. Each of these states belongs to a process (PR review, release cycle, post-release verification) driven by the bridge and the coordinator. A release angle sub-item In Review is the one exception to whom it goes: the bridge assigns it to the coordinator, as the reviewer of its angle PR, never to an implementer (`05-release-cycle.md` §4). A tracker that has status groups puts them in a group of their own, outside what it serves to implementers. Post Deploy in particular stays outside the in-progress group: the ticket itself is the post-release verification: an agent takes it first and hands it to the owner for what needs a human (BR-ASDLC-003), but it is never served to an implementer.

## Items created by the bridge

| Item | Parent | Purpose |
|---|---|---|
| **Release issue** | none | One per cycle: title names the product and the version; carries the brief and the GO decision; assigned to the owner (`owner.tracker_user`), unassigned when the tracker does not know that person; **In Progress** while its cycle runs, from its creation to its closure, which moves it to Done (one found in Ready or Backlog is moved In Progress at the next tick). |
| **Angle sub-item** | the release issue, or the PR's ticket | One per configured angle; served to a reviewer; its PR (release) or its findings (PR review) are its output. A PR review's sub-items belong to their PR's round: they close when that PR resolves, whatever becomes of the ticket. A release angle sub-item follows the normal flow of a work item, In Progress, In Review (assigned to the coordinator, who is served its PR's review), back In Progress on changes requested (assigned to its angle's assignee, or to nobody), and is Done when the bridge merges its angle PR (BR-ASDLC-020). |
| **Consolidation item** | same parent | Served to the coordinator once every angle has reported: it is blocked by every angle sub-item of its round until each is resolved. At a PR review (BR-ASDLC-018) it closes with its round. At a release candidate (BR-ASDLC-020) it is the **overall review**, served once every angle PR of the candidate is merged (the coordinator reviewed each angle PR as it came); it is Done when a newer candidate is tagged or the release is published, and the closure closes it otherwise. |
| **Gate-fix ticket** | the release issue | One per candidate whose gate concluded red once its automatic rerun settled, or once `gate_rerun_wait_minutes` passed with no rerun seen (`05-release-cycle.md` §3, BR-ASDLC-016): assigned to `release.fix_assignee` (unassigned when unset), in Ready, **urgent**, labelled with the release label, with a complete contract (the gate green on the release branch; the failing suites from the evidence pack, or "unknown", and the gate run; its one criterion, the failing suites passing on the release branch). An implementer takes it by role, fixes on the release branch and ticks that criterion as any; the ticket then goes To Deploy at its fix's merge and Done at the closure. The next candidate's gate is the verification: still red, it gets a gate-fix ticket of its own; green, the bridge closes Done a gate-fix ticket that never reached a merge (still Triage, Backlog, Ready or In Progress) and leaves any other alone. A ticket like any other, not a bridge item: its PR links to it. |
| **Validation sub-task** | none any more: earlier versions created one under a ticket moved to Post Deploy | The ticket is its own validation (BR-ASDLC-003). An open sub-task an earlier version left (marker `spl-asdlc:post-deploy-validation parent=<key>`) is canceled once by the bridge, with a comment that sends its reader to the ticket (BR-ASDLC-017). |
| **Finding ticket** | none, related to the release issue | A non-blocking finding, logged for later: in Triage, labelled `qa-finding`, normal priority, unassigned (BR-ASDLC-022). |
| **Blocking finding's fix ticket** | none, `blocks` the release issue | A blocking finding: in Ready, **urgent**, assigned to `release.fix_assignee` (unassigned when unset), carrying the blocker marker; no GO while it is open (BR-ASDLC-022). |

## Findings and verdicts

A finding of a release QA is **blocking** (the release must not ship with it) or **non-blocking** (real, to fix later); the angle documents it and fixes nothing (`06-qa-report.md`). At a PR review a finding carries a severity in its thread, and the coordinator fixes at the PR the ones it retains (`04-pr-review.md`); an angle's file grades its findings on that scale, a `blocker` reading as blocking at a release, a `should-fix` or a `nit` as non-blocking.

| Severity (PR review) | Meaning |
|---|---|
| `blocker` | The PR must not merge with it: changes are requested. |
| `should-fix` | Real, not blocking. |
| `nit` | Cosmetic. |

| Verdict (release angle report) | Meaning |
|---|---|
| `go` | No blocking finding from this angle. |
| `hold` | At least one blocking finding. |

## Labels

`release/<product>-v<version>` on every ticket embarked in a cycle, and on the items the bridge creates for it; the no-deploy label (`tracker.no_deploy_label`, default `no-deploy`) on a ticket with nothing to deploy, which a person or refinement adds and the bridge only reads: Done at its merge, never embarked by a release (`04-pr-review.md` §10); `qa-finding` on finding tickets; the escalation label (`release.escalation_label`, default `blocked-by-human`) on an item waiting for a human gesture; the hold label (`release.hold_label`, default `hold`), which a person puts on a release issue to hold its GO and the bridge only reads; `ticketless-pr` on the ticket the bridge opens for an App's PR that has none (`04-pr-review.md` §9); `post-deploy-validation/<product>` on a ticket in Post Deploy (the bridge's index, since a tracker cannot be listed by status: set by the closure, removed once the ticket is resolved).

## Other terms

- **Candidate**: a commit on the release branch, identified by its RC tag.
- **Embarked ticket**: a ticket linked to a PR that merged a commit of the candidate's range, but a ticket with nothing to deploy (the no-deploy label).
- **Range**: `<last published release>..<candidate>`; for a product's first release, with no published release below the candidate, the candidate's whole history, from the repository's root (`05-release-cycle.md`, "The range and the first release").
- **Gate**: the deterministic CI of a candidate (suites, audits, evidence pack).
- **Evidence pack**: the artifact the gate publishes for the reviewers.
- **Brief**: what the bridge posts on the release issue: embarked tickets with their acceptance criteria, business rules touched, link to the evidence pack.
- **Angle branch**: `qa/<candidate>/<angle>`, cut from the release branch by a reviewer.
- **Angle report**: the mandatory file an angle commits in its branch (`06-qa-report.md`).
- **Record**: the concatenation of the angle reports of a cycle, on `main` after the merge-back.
