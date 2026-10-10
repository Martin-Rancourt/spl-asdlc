# Requirements — the `Tracker` trait and its adapters

The tracker operations the bridge needs are the union of REQ-REL-098 (`release.md`) and what the merge pass reads (`merge.md`: REQ-MERGE-029, 065, 069, 075). The trait is `spl_asdlc::tracker::Tracker`; nothing else is asked of a tracker.

| Operation | Why | Contract |
|---|---|---|
| `identity` | Every write is attributed to the service identity; markers are trusted only in its comments | Unreadable is an error |
| `team_key`, `status_names` | Embarked tickets are the release team's; `doctor` checks the status table | A status the team lacks refuses the write that needs it |
| `user` | Assignee hints, the owner, the implementer | Not found and ambiguous are errors, never a guess |
| `get` | Ticket contract, state, labels, parent, archived flag | Not found vs unreadable kept apart |
| `relations` | `blocks` both ways (GO, cycle check), `duplicate` both ways (Done gate) | A cut page is flagged, and the Done gate fails closed on it |
| `comments` | Markers of once-only effects, merge records | The whole thread, oldest first, with `by_bridge` |
| `children`, `list_by_label` | Release members, sub-items, findings | A list the tracker cuts is unreadable |
| `items_linked_to_pr` | The PR ↔ ticket link (`docs/spec/01-ticket-contract.md` §7) | An answer that does not say it is whole is unreadable |
| `archives`, `archive` | At the closure, the cycle's resolved items leave the open count of a tracker that limits it (BR-ASDLC-019, spec 05 §11) | Linear archives (`issueArchive`, never `trash`; a lost answer is read back); Livela has no limit, `archives` is false and `archive` writes nothing. An item already archived records nothing; the caller never archives an open item |
| `ensure_label`, `create_item`, `update_description`, `add_label`, `remove_label`, `set_status` (with the assignee in the same write), `assign`, `comment`, `update_comment`, `relate`, `unrelate`, `link_pr` | The writes of the method | A write the tracker does not confirm is refused. A write of a value already there records nothing (no useless facts, `docs/livela-contract.md`). A creation whose answer is lost is looked up, never repeated blindly. |

Only Done and Canceled are resolved. In Review, To Deploy and Post Deploy are open: they still block their dependents, whatever category the tracker files them under (the `validation` group, `docs/livela-contract.md`).

## Conformance

`spl_asdlc::tracker::conformance::run` (or `run_with`, given a counter of the changes the tracker recorded) is the acceptance of an adapter, and of `docs/livela-contract.md`. Each write is read back. The suite:

- finds the service identity by name and by id; `get` of an unknown key is NotFound;
- creates a release item with two angle children;
- links a PR URL and finds the item back by it (and an unlinked URL finds nothing);
- moves a child through In Review → To Deploy → Post Deploy → Done;
- sets a status and an assignee in one call;
- lays a `blocks` relation and reads it from both ends and only in its direction; a relation to an unknown item is refused;
- comments twice as the service identity and reads the thread oldest first, editing a comment where the tracker can;
- describes, labels, unlabels, assigns and unassigns;
- repeats every write unchanged and checks that nothing is recorded or doubled;
- lists by label, removes the relation;
- cancels what it created, on failure too. The in-memory tracker passes it in the unit tests; `tests/live_linear_conformance.rs` runs it on a real Linear team (it writes: use a test team).
