# Livela contract

What the Livela tracker offers for its `Tracker` adapter and for hosting the bridge. First written on 2026-10-05 against the Livela core; **implemented in Livela the same day** (conformance suite `crates/todo-core/tests/spl_asdlc_contract.rs` in the Livela repository, green). The decisions below override the first version where they differ. Livela is spelled "LiveLà" wherever it is displayed; the core crate is still `todo-core`.

## 1. Status groups: `validation` is the fourth group

Groups are `preparation`, `active`, `validation`, `closed`. Statuses of `ticket` and `bug` (same list), in order:

| Group | Statuses |
|---|---|
| `preparation` | `triage`, `backlog` |
| `active` | `ready`, `in_progress` |
| `validation` | `in_review`, `to_deploy`, `post_deploy` |
| `closed` | `done`, `cancel` |

An item in `validation` stays **open**: it still blocks its dependants (`blocked_by`) until it is really done. But "what's next" **never serves it**: each status of this group has its own process (PR review, release cycle, post-release verification), not an implementer. "What's next" serves the `active` group only.

Consequence for the bridge and the coordinator: they are the ones who move an item through `in_review → to_deploy → post_deploy → done`; no implementing agent will ever receive it. The canonical names "To Deploy" and "Post Deploy" map to `to_deploy` and `post_deploy`; `set_status` accepts the id or the English name. There is no `post_release` status.

## 2. No heartbeat in the data

- A daemon job that passes and does nothing **writes nothing**.
- A pass that works leaves only its writes on the items (transitions, comments, links), signed by the participant `spl-asdlc`: that is its report, in each item's activity.
- Only a **failed** pass (error, or time budget exceeded) leaves an entry, in the activity of the entity `job:<name>` (readable with `list_activity("job:spl-asdlc")`).
- Agent heartbeats belong to the agent runtime, not to Livela. In Livela an agent that starts a piece of work takes a lock on the item, which appears in the activity; its actions, an error, or nothing if the lock expires, follow.
- The bridge **never rewrites an unchanged value**: no useless fact at every pass.

## 3. Hosting: the bridge is a `DaemonJob`

The `spl-asdlc` crate becomes public as soon as it is fit to share; the Livela runner then depends on it through Git or crates.io. No separate binary, no private dependency. The job contract is in the Livela core (`todo_core`, no platform dependency):

```rust
pub trait DaemonJob {
    fn name(&self) -> &str;                       // the failure entity is `job:<name>`
    fn run(&mut self, store: &mut Store, ctx: &JobContext) -> JobOutcome;
}
pub struct JobContext { pub budget: Duration, pub config: Option<PathBuf> }
pub struct JobOutcome { pub ok: bool, pub message: String }  // JobOutcome::ok(..) / ::failed(..)
```

The daemon (`todo-runner daemon`, every N seconds, 300 by default) calls each job after the code tasks, with the budget `LIVELA_JOB_BUDGET_SECONDS` (60 s by default; exceeding it is a failure) and the path of `spl-asdlc.toml` in `LIVELA_SPL_ASDLC_CONFIG`. Secrets are read by the job itself, never stored in the facts. `todo-runner jobs` runs one pass by hand. The job may call `store.set_author(id_spl_asdlc)`; the previous author is restored afterwards.

On the `spl-asdlc` side: implement `BridgeJob: todo_core::DaemonJob` (and later `GithubSyncJob`, point 5), depending on `todo-core`. Livela registers it in one line in `registered()` of `crates/todo-runner/src/jobs.rs` once the crate is public.

## 4. The core API the adapter uses (delivered)

- `Store::ensure_participant(ParticipantType::Agent, "spl-asdlc")`: creates the participant once.
- `Store::create_item_with(ItemType::Ticket, BaseFields { title, parent, tags, .. })`: the release item and its angle children.
- `Store::add_web_link(id, url, title)` then `Store::find_by_web_link(url)` → ids, oldest first; trailing slash and the case of scheme and host are ignored.
- `Store::set_status(id, "to_deploy")` or `set_status(id, "To Deploy")`: canonical name (id or English name); error `InvalidStatus` (code `invalid_status`) when the type has no such status.
- `Filter { tag: Some(..) }` and `Filter { parent: Some(..) }` with `list_items_filtered`: by tag, and the children of an item.
- `Store::add_relation(b, RelationKind::BlockedBy, a)` → `a.relations.blocks` contains b.
- `Store::comment(id, author, text)`; `Store::list_activity(id)`: every fact carries its author.
- MCP (for agents, not for the adapter): 21 tools, among them `set_status`, `find_by_web_link`, `search`, `name_session`, `whats_next`, `release`.

## 5. Still to come, not required by the method

GitHub → item sync (PR status and review comments mirrored on the item) as a Livela feature built on the `github` module of this crate: a second `DaemonJob`, `GithubSyncJob`.

## 6. Locks and agent sessions (for the coordinator)

A lock is a fresh token per lock, discreetly visible in the item's activity (a 4-character short code) with the session's name; 30 minutes for an agent, end of day for a human. An MCP session is named after its client at connection, or with the `name_session` tool. A session releases its own locks; another client may release with the token.

## Conformance

The adapter's conformance suite creates a release item with two angle children, links a PR URL, finds the item back by that URL, moves a child through `in_review → to_deploy → done`, verifies that an item in `validation` still blocks its dependants and is not served by "what's next", adds a `blocks` relation and reads it back, comments as `spl-asdlc`, lists by tag, verifies that an unchanged value is not rewritten, and that every write is attributed to the `spl-asdlc` participant and visible in the activity log. It is the same suite Livela runs on its core.
