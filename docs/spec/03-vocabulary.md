# 03 — Vocabulary

Canonical names. A tracker names its statuses exactly so; the configuration maps none (`08-configuration.md`). An adapter whose tracker keys statuses by its own ids maps them in its code.

## Statuses of a work item (ticket, bug)

| Canonical | Meaning | Who writes it |
|---|---|---|
| **Triage** | New; nobody has judged it yet. | The author |
| **Backlog** | Judged worth keeping; no contract yet, or not scheduled. | The owner |
| **Ready** | Contract verified by refinement; may be taken. | Refinement (human or the bridge on `ticket check`) |
| **In Progress** | Taken by an implementer. | The bridge when a branch or PR appears, or when a PR resolves and a box remains open with no PR in review (`04-pr-review.md` §10); the runtime's claim |
| **In Review** | A PR of the ticket is ready and CI is green; its angle sub-items exist. Also a ticket every box of which is ticked while a PR of it is open and not reviewed yet (`04-pr-review.md` §10). | The bridge |
| **To Deploy** | Merged, every box ticked (the `## Post deploy` ones apart), no PR open; waiting for the next release. A request for deployment, never a claim of completion. Never written in a project without a release cycle (`05-release-cycle.md`, "Without a release cycle"). | The bridge, at merge |
| **Post Deploy** | Released (without a release cycle: merged); only `## Post deploy` boxes remain to be verified in production. | The bridge, at closure, or at merge in a project without a release cycle; either also creates the ticket's validation sub-task |
| **Done** | Released and every box ticked; in a project without a release cycle, merged and every box ticked (merging is delivering). | The bridge: at closure, at merge in a project without a release cycle, or, for a ticket in Post Deploy, at the tick that finds every box of its `## Post deploy` section checked (no verb: checking the boxes is the gesture) |
| **Canceled** | Will not be done. | The owner |

In Review, To Deploy and Post Deploy are **validation** states: the item is still open and still blocks what depends on it, but no implementer is ever served it. Each of these states belongs to a process (PR review, release cycle, post-release verification) driven by the bridge and the coordinator. A tracker that has status groups puts them in a group of their own, outside what it serves to implementers. Post Deploy in particular stays outside the in-progress group: the post-release verification is served through the ticket's **validation sub-task**, never through the ticket itself.

## Items created by the bridge

| Item | Parent | Purpose |
|---|---|---|
| **Release issue** | none | One per cycle: title names the product and the version; carries the brief and the GO decision. |
| **Angle sub-item** | the release issue, or the PR's ticket | One per configured angle; served to a reviewer; its PR (release) or its findings (PR review) are its output. A PR review's sub-items belong to their PR's round: they close when that PR resolves, whatever becomes of the ticket. |
| **Consolidation item** | same parent | Served to the coordinator once every angle has reported; at a PR review it is blocked by every angle sub-item of its round until each is resolved (BR-ASDLC-018); closes with its round. |
| **Gate-fix ticket** | the release issue | One per candidate whose gate concluded red once its automatic rerun settled, or once `gate_rerun_wait_minutes` passed with no rerun seen (`05-release-cycle.md` §3, BR-ASDLC-016): unassigned, in Ready, **urgent**, labelled with the release label, with a complete contract (the gate green on the release branch; the failing suites from the evidence pack, or "unknown", and the gate run; its one criterion, the failing suites passing on the release branch). An implementer takes it by role, fixes on the release branch and ticks that criterion as any; the ticket then goes To Deploy at its fix's merge and Done at the closure. The next candidate's gate is the verification: still red, it gets a gate-fix ticket of its own; green, the bridge closes Done a gate-fix ticket that never reached a merge (still Triage, Backlog, Ready or In Progress) and leaves any other alone. A ticket like any other, not a bridge item: its PR links to it. |
| **Validation sub-task** | a ticket the closure (or, without a release cycle, the merge) moved to Post Deploy | One per ticket, unassigned, in Ready, **urgent** (production is waiting on it, BR-ASDLC-017), with a complete contract: an agent verifies the parent's `## Post deploy` boxes in production and checks the ones it can verify, then closes it; a check that needs a human, or is beyond the agent, is named in a comment and the sub-task is reassigned to the owner (`owner.tracker_user`). |
| **Finding ticket** | none, related to the release issue | A non-blocking finding to fix later; labelled `qa-finding`. |
| **Blocker** | none, `blocks` the release issue | A blocking finding that an angle could not fix in its branch. |

## Severities and verdicts

| Severity | Meaning |
|---|---|
| `blocker` | The release must not ship with it. Fixed in the angle branch, or becomes a blocker ticket. |
| `should-fix` | Real, not blocking; becomes a finding ticket. |
| `nit` | Cosmetic; listed in the report, documented rather than solved; a box of the release's nits ticket, or a ticket of its own (`release.nits`), to triage: kept or dropped. |

| Verdict | Meaning |
|---|---|
| `go` | Nothing blocking from this angle. |
| `go-with-note` | Nothing blocking; findings to carry as tickets. |
| `hold` | At least one blocker not fixed. |

## Labels

`release/<product>-v<version>` on every ticket embarked in a cycle, and on the items the bridge creates for it; `qa-finding` on finding tickets; `blocked-by-human` on an item waiting for a human gesture; `post-deploy-validation/<product>` on a ticket in Post Deploy (the bridge's index, since a tracker cannot be listed by status: set by the closure, removed once the ticket is resolved).

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
