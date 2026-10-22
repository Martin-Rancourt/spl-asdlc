# Livela contract

What the Livela tracker offers for its `Tracker` adapter and for hosting the bridge. First written on 2026-10-05 against the Livela core and **implemented in Livela the same day**; the conformance suite `crates/livela-core/tests/spl_asdlc_contract.rs` in the Livela repository is green. The decisions below override the first version where they differ. Livela is spelled "LiveLà" wherever it is displayed. The core crate is `livela-core` and the runner is `livela-runner` (formerly `todo-core` and `todo-runner`).

## 1. Status groups: `validation` is the fourth group

Groups are `preparation`, `active`, `validation`, `closed`. Statuses of `ticket` and `bug` (same list), in order:

| Group | Statuses |
|---|---|
| `preparation` | `triage`, `backlog` |
| `active` | `ready`, `in_progress` |
| `validation` | `in_review`, `to_deploy`, `post_deploy` |
| `closed` | `done`, `cancel` |

An item in `validation` stays **open**: it still blocks its dependants (`blocked_by`) until it is really done. But "what's next" **never serves it**: each status of this group has its own process (PR review, release cycle, post-release verification), not an implementer. "What's next" serves the `active` group only.

Consequences for the bridge and the coordinator:
- They are the ones who move an item through `in_review → to_deploy → post_deploy → done`; no implementing agent will ever receive it.
- The canonical names "To Deploy" and "Post Deploy" map to `to_deploy` and `post_deploy`; `set_status` accepts the id or the English name.
- There is no `post_release` status.
- Refined is not a status: a refined ticket stays in Backlog, and Livela marks it with a `refined` field computed from the description (a goal, at least one acceptance criterion and a release note). No agent writes it, and the bridge does not read it.
- The method reads every tracker the same way: only Done and Canceled are resolved (`ItemState::is_resolved`). A tracker that files To Deploy as a completed state, as a Linear team may, changes nothing.

## 2. No heartbeat in the data

- A daemon job that passes and does nothing **writes nothing**.
- A pass that works leaves only its writes on the items (transitions, comments, links), signed by the participant `spl-asdlc`: that is its report, in each item's activity.
- Only a **failed** pass (error, or time budget exceeded) leaves an entry, in the activity of the entity `job:<name>` (readable with `list_activity("job:spl-asdlc")`). The runner writes it (`run_jobs`), not the bridge.
- Agent heartbeats belong to the agent runtime, not to Livela. In Livela an agent that starts a piece of work takes a lock on the item, which appears in the activity; its actions, an error, or nothing if the lock expires, follow.
- The bridge **never rewrites an unchanged value**: no useless fact at every pass.

**How the adapter holds this:**
- Before each write it reads the status, description, tags, assignee, relations or links it would change, and writes nothing when the value is already there. Reads create nothing.
- It signs each write as `spl-asdlc` and gives the store back its previous author (`LivelaStore::swap_author`), so the runner's other writers keep their own signature.
- The conformance suite checks this with a counter of the facts the journal receives.

## 3. Hosting: the bridge is a `DaemonJob`

The `spl-asdlc` repository is public and its crate is published on crates.io from the version tags `v<X.Y.Z>` (from 1.3.2): the Livela runner depends on the crate by version, from crates.io. No separate binary, no private dependency. The job contract is in the Livela core (`livela_core`, no platform dependency):

```rust
pub trait DaemonJob {
    fn name(&self) -> &str;                       // the failure entity is `job:<name>`
    fn run(&mut self, store: &mut Store, ctx: &JobContext) -> JobOutcome;
}
pub struct JobContext { pub budget: Duration, pub config: Option<PathBuf> }
pub struct JobOutcome { pub ok: bool, pub message: String }  // JobOutcome::ok(..) / ::failed(..)
```

How the daemon runs the job:
- `livela-runner daemon` passes every N seconds (300 by default) and calls each job after the code tasks.
- The budget is `LIVELA_JOB_BUDGET_SECONDS` (60 s by default); exceeding it is a failure.
- `LIVELA_SPL_ASDLC_CONFIG` holds the path of the configuration, `.spl-asdlc/config.toml` in the repository's checkout; the bridge merges the instance overlay beside it, `config.instance.toml`, when there is one (spec 08).
- Secrets are read by the job itself, never stored in the facts.
- `livela-runner jobs` runs one pass by hand.
- The job may call `store.set_author(id_spl_asdlc)`; the previous author is restored afterwards.

**Decided by the owner (2026-10-07): the dependency arrow goes from Livela to ASDLC** (BR-ASDLC-010).
- `spl-asdlc` does not depend on `livela-core`. It names no private account, puts nothing of Livela in its `Cargo.lock`, and carries no second copy of the `DaemonJob` trait.
- The runner depends on the public `spl-asdlc` crate by version, from crates.io, and writes the wrapper on its side. `BridgeJob: livela_core::DaemonJob` lives in the runner's code, next to its other jobs, registered in one line in `registered()` of `crates/livela-runner/src/jobs.rs`. So does the store glue (`LivelaStore` for its `Store`), and later `GithubSyncJob` (point 5).
- What `spl-asdlc` offers the wrapper: the pass (`spl_asdlc::job::pass`), the adapter (`spl_asdlc::tracker::livela::Livela`) and the slice of the store the adapter needs (`LivelaStore`). `docs/setup/tracker-livela.md` says what the wrapper does, so that another runtime can write its own.
- Why not a dependency on the core: the Livela repository is private and the crate's license is `UNLICENSED`; a Git dependency would name an account in `Cargo.lock` (BR-ASDLC-001); and it would give the runner two copies of the crate, so that `BridgeJob` would implement a trait the runner does not know.

## 4. The core API the adapter uses (delivered)

- `Store::ensure_participant(ParticipantType::Agent, "spl-asdlc")`: creates the participant once.
- `Store::create_item_with(ItemType::Ticket, BaseFields { title, parent, tags, priority, .. })`: the release item and its angle children, the gate-fix tickets and the validation sub-tasks. The store has a priority field, so the bridge's canonical priority is set at creation: the adapter passes its level, 1 urgent … 4 low (`create_ticket`'s `priority`), and the wrapper maps it to the store's `Priority` (`docs/setup/tracker-livela.md`). A gate-fix ticket (BR-ASDLC-016) and a validation sub-task (BR-ASDLC-017) are created at 1, urgent; an item created without one keeps the store's default.
- `Store::add_web_link(id, url, title)` then `Store::find_by_web_link(url)` → ids, oldest first. A trailing slash and the case of the scheme and host are ignored.
- `Store::set_status(id, "to_deploy")` or `set_status(id, "To Deploy")`: canonical name (id or English name). An `InvalidStatus` error (code `invalid_status`) when the type has no such status.
- `Filter { tag: Some(..) }` and `Filter { parent: Some(..) }` with `list_items_filtered`: by tag, and the children of an item.
- `Store::add_relation(b, RelationKind::BlockedBy, a)` → `a.relations.blocks` contains b.
- `Store::comment(id, author, text)`; `Store::list_activity(id)`: every fact carries its author.
- `Store::set_author` and `Store::current_author`.
- `Store::find_by_short_id`. It resolves by the code alone, because the human prefix may change (contract v4, decision 14). The code also matches by prefix, so the adapter refuses a code shorter than four characters.
  - A code lengthens when another item comes to share it, so a key the bridge noted earlier can come to match two items.
  - That is unreadable, and the bridge waits rather than guessing.
- MCP (for agents, not for the adapter): 21 tools, among them `set_status`, `find_by_web_link`, `search`, `name_session`, `whats_next`, `release`.

## 5. Still to come

- **Editing a comment.** The bridge would keep one status row per PR on a ticket and rewrite it in place (REQ-MERGE-069), so that informational facts never pile up as comments. The core has no comment edit. Until it does:
  - the adapter declares it (`can_edit_comments` is false);
  - the bridge writes no rewritten row on Livela;
  - informational facts stay in its action log, and messages that ask someone to act are comments, as everywhere.
- **GitHub → item sync** (PR status and review comments mirrored on the item). This is a Livela feature built on the `github` module of this crate: a second `DaemonJob`, `GithubSyncJob`. The method does not require it.

## 6. Locks and agent sessions (for the coordinator)

- A lock is a fresh token per lock, discreetly visible in the item's activity (a 4-character short code) with the session's name. It lasts 30 minutes for an agent and until the end of the day for a human.
- An MCP session is named after its client at connection, or with the `name_session` tool. A session releases its own locks; another client may release one with the token.
- Locks are advisory: the core's writes do not check them.
- **Decided by the owner (2026-10-07): the bridge writes its transitions whoever holds the lock** (BR-ASDLC-011).
  - It is the only automated writer of state transitions; a person may still move an item in the tracker (spec 09).
  - A lock protects an agent's work on the item's **content**, never its **status**.
  - The bridge neither waits for a lock to be released nor notifies its holder of a transition; the adapter does not read locks.

## 7. No open-item limit: nothing is archived

- At the closure of a cycle the bridge archives what the cycle resolved, on a tracker that counts its open items against a limit (BR-ASDLC-019, spec 05 §11).
- Livela has no such limit. The adapter says so: `archives` is false, and `archive` is a no-op that writes nothing.
- The bridge therefore asks Livela for no archive, and a closed cycle leaves no fact on Livela beyond its transitions and comments.
- The core needs no archive API for the method. An item Livela reports deleted reads as archived, and the bridge never writes to it.

## Conformance

The adapter's suite (`spl_asdlc::tracker::conformance::run_with`) runs the following steps:

- create a release item with two angle children;
- link a PR URL and find the item back by that URL, and only that item;
- move a child through `in_review → to_deploy → post_deploy → done`;
- set a status and an assignee in one write;
- lay a `blocks` relation and read it from both ends, only in its direction;
- comment twice as `spl-asdlc` and read the thread oldest first;
- describe, label, unlabel, assign and unassign, each read back;
- repeat every write unchanged and check that no fact is recorded and nothing is doubled;
- list by tag;
- cancel what it created, also when a step fails.

The glue passes it on `livela-core`. Every write is attributed to the `spl-asdlc` participant and visible in the activity log, and the store's previous author is restored after it.

Livela's own replay on its core (`spl_asdlc_contract.rs`) also checks that an item in `validation` still blocks its dependants and is not served by "what's next".
