# Open method points

Points the specification leaves open, raised to the owner. Until each is decided, the bridge does the fail-closed thing given here: it waits, and it invents nothing. When a point is settled, the specification says so and its entry here is marked decided.

| # | Point | Until decided |
|---|---|---|
| 1 | Who moves a ticket from Post Deploy to Done, and when. Spec 03 and 09 give Done only to the closure; the closure comment names a `release post-release-check` verb that spec 07 does not have. | Nothing moves Post Deploy. |
| 2 | ~~Release angle sub-items and the brief: at the creation of the release item (spec 05 §2), or once the gate of the candidate is green (spec 05 §3, REQ-REL-046/048).~~ **Decided by the owner**: once the candidate's gate is green, never at the release item's creation; spec 05 §2–§3 say so, BR-ASDLC-004 enforces it. | Decided: no longer waits. |
| 3 | The human GO: the owner's `/go` on the release item is a tracker comment, but spec 08 names `owner.github_login`. | Under `go_policy = "human"` the GO is never given. |
| 4 | Whose approval counts as the human one under `review.human_approval`. | A PR that needs it is not merged; `review.human_reviewer` is asked once. |
| 5 | Several PRs per ticket (spec 00 §2 against spec 01 §7); the end of a review round's sub-items at merge or when a PR is closed unmerged; REQ-MERGE-028 (a ticket past the merge with an open PR). | A PR claimed by several tickets waits; sub-items are left as they are; no REQ-MERGE-028 repair. |
| 6 | Writing a finding's ticket id back into the merged report (spec 06 §4 against spec 09). | No write-back: the finding marker on the ticket carries the mapping. |
| 7 | Whether `ticket check` moves a complete Triage or Backlog ticket to Ready (spec 03). | `ticket check` writes nothing. |
| 8 | The format of "one more pass" (`max_rc_passes` raised for a cycle, spec 09). | Only the configured `max_rc_passes` counts. |
| 9 | The implementer a reopened ticket goes back to at closure (REQ-REL-089). | Its current assignee; unassigned is said in the comment. |
| 10 | A rewritten tracking row on trackers that can edit comments (REQ-MERGE-069). | No row on any tracker: informational facts stay in the action log. |
| 11 | Where the public crate gets `livela-core` from, so that `BridgeJob` implements the runner's trait (`docs/livela-contract.md` §3). | The wrapper lives with the glue in `docs/setup/tracker-livela.md`. |
| 12 | Whether the bridge may write a transition on an item an agent holds locked (Livela locks are advisory). | It writes: the bridge is the only writer of transitions. |
