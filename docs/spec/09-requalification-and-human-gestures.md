# 09 — Human gestures

The method allows exactly these human gestures in a cycle. Each is a verb or a GitHub action, logged, never an edit of markers by hand.

| Gesture | How | Effect |
|---|---|---|
| **Open a cycle** | `release-cut` workflow dispatch (`open`), or `spl-asdlc release cut --open` which dispatches it | Release branch and first RC; the bridge creates the issue at the next tick |
| **Tag the next RC** | Same workflow, action `candidate`, or the bridge when the policy automates it | New RC on the release branch |
| **Requalify a finding** | `spl-asdlc release requalify <ticket> --severity <blocker|should-fix|nit> --reason "<text>"` | Changes the finding ticket's severity; adds or removes its `blocks` relation to the release issue; records who, when and why on the ticket and on the release issue; the GO is recomputed at the next tick |
| **Give the GO** (policy `human` only) | Approve on the release issue (a comment `/go` by the owner, or the tracker's approval field when it has one) | Added to the computed conditions; never replaces them |
| **Take an escalation** | The release issue is assigned to the owner with the hold label when the passes are exhausted | Requalify, allow one more pass (`max_rc_passes` raised for this cycle in the issue), or abandon |
| **Abandon a cycle** | `spl-asdlc release abandon --product <p>` | Keeps the tags, merges the release branch back into `main`, cancels the release issue, turns remaining blockers into tickets |
| **Close by waiver** | `spl-asdlc release close --product <p> --version <v>` | Closes a cycle whose component is deliberately not deployed; the waiver and its reason are logged on the release issue |
| **Validate a post-deploy box** | Check the box in the ticket's `## Post deploy` section, once verified in production — the agent of the ticket's validation sub-task, or the owner when the sub-task is reassigned to them | No verb: the bridge moves the ticket from Post Deploy to Done at the tick that finds every box of that section checked |
| **Resume a deployment** | The project's `release-publish` dispatch on the validated RC tag | Project-specific; the bridge only reads the resulting `deployed` labels |

Everything else is either the bridge's or an agent's. In particular nobody writes a status, moves a ticket to Done, or edits a report after it was merged: Done is written by the bridge, at closure or when every `## Post deploy` box of a ticket in Post Deploy is checked.
