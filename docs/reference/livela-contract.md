# Livela contract

What the Livela tracker offers for its `Tracker` adapter and for hosting the bridge. First written on 2026-10-05 against the Livela core and **implemented in Livela the same day**; the conformance suite `crates/livela-core/tests/spl_asdlc_contract.rs` in the Livela repository is green. The decisions below override the first version where they differ. Livela is spelled "LiveLà" wherever it is displayed. The core crate is `livela-core` and the runner is `livela-runner` (formerly `todo-core` and `todo-runner`).

## 1. Status groups: `validation` is the fourth group

Groups are `preparation`, `active`, `validation`, `closed`. Statuses of `ticket` and `bug` (same list), in order:

| Group | Statuses |
|---|---|
| `preparation` | `backlog` (no `triage` since 2026-10-08, below) |
| `active` | `ready`, `in_progress` |
| `validation` | `in_review`, `to_deploy`, `post_deploy` |
| `closed` | `done`, `cancel` |

An item in `validation` stays **open**: it still blocks its dependants (`blocked_by`) until it is really done. But "what's next" **never serves it**: each status of this group has its own process (PR review, release cycle, post-release verification), not an implementer. "What's next" serves the `active` group only.

Consequences for the bridge and the coordinator:
- They are the ones who move an item through `in_review → to_deploy → post_deploy → done`; no implementing agent will ever receive it.
- The canonical names "To Deploy" and "Post Deploy" map to `to_deploy` and `post_deploy`; `set_status` accepts the id or the English name.
- There is no `post_release` status.
- There is no `triage` status on a ticket since triage became an item type of its own (2026-10-08). The canonical Triage, which the bridge writes for a non-blocking finding and a finding requalified as non-blocking, is the ticket's `backlog` on Livela. The adapter names Triage among its statuses, writes it as Backlog and reads Backlog as Backlog. A ticket type that still has a `triage` status keeps it.
- Refined is not a status: a refined ticket stays in Backlog, and Livela marks it with a `refined` field computed from the description (a goal, at least one acceptance criterion and a release note). No agent writes it, and the bridge does not read it.
- Ready and In Progress are as the method says them (`docs/method.md`): a person moves a refined ticket to Ready when agents may take it, implementing agents take work from Ready only, and the agent's verb ("implement the next ticket") moves the ticket it takes to In Progress. The bridge moves neither.
- The method reads every tracker the same way: only Done and Canceled are resolved (`ItemState::is_resolved`). A tracker that files To Deploy as a completed state, as a Linear team may, changes nothing.

## 2. No heartbeat in the data

- A daemon job that passes and does nothing **writes nothing**.
- A pass that works leaves only its writes on the items (transitions, comments, links), signed by the participant `spl-asdlc`: that is its report, in each item's activity.
- Only a **failed** pass (error, or time budget exceeded) leaves an entry, in the activity of the entity `job:<name>` (readable with `list_activity("job:spl-asdlc:<prefix>")` for the bridge of one project). The runner writes it (`run_jobs`), not the bridge.
- Agent heartbeats belong to the agent runtime, not to Livela. In Livela an agent that starts a piece of work takes a lock on the item, which appears in the activity; its actions, an error, or nothing if the lock expires, follow.
- The bridge **never rewrites an unchanged value**: no useless fact at every pass.

**How the adapter holds this:**
- Before each write it reads the status, description, tags, assignee, relations or links it would change, and writes nothing when the value is already there. Reads create nothing.
- It signs each write as `spl-asdlc` and gives the store back its previous author (`LivelaStore::swap_author`), so the runner's other writers keep their own signature.
- The conformance suite checks this with a counter of the facts the journal receives.

## 3. Hosting: the bridge is a `DaemonJob`

The `spl-asdlc` repository is public and its crate is published on crates.io from the version tags `v<X.Y.Z>`, by the publication workflow from 1.4.1 (`.spl-asdlc/development.md`, "Releases"; 1.3.2 was published by hand, 1.3.3 and 1.4.0 are not on crates.io): the Livela runner depends on the crate by version, from crates.io. No separate binary, no private dependency. The job contract is in the Livela core (`livela_core`, no platform dependency):

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
- The job reads no configuration path: the bridge reads the repository's `.spl-asdlc/config.toml` at its default branch, and the runner passes the instance overlay as a value ("A host with several projects" below). `LIVELA_SPL_ASDLC_CONFIG` is no longer needed.
- Secrets are read by the job itself, never stored in the facts.
- `livela-runner jobs` runs one pass by hand.
- The job may call `store.set_author(id_spl_asdlc)`; the previous author is restored afterwards.

**Decided by the owner (2026-10-07): the dependency arrow goes from Livela to ASDLC** (BR-ASDLC-010).
- `spl-asdlc` does not depend on `livela-core`. It names no private account, puts nothing of Livela in its `Cargo.lock`, and carries no second copy of the `DaemonJob` trait.
- The runner depends on the public `spl-asdlc` crate by version, from crates.io, and writes the wrapper on its side. `BridgeJob: livela_core::DaemonJob` lives in the runner's code, next to its other jobs, registered in one line in `registered()` of `crates/livela-runner/src/jobs.rs`. So does the store glue (`LivelaStore` for its `Store`), and `GithubSyncJob` ("GitHub → item sync" below).
- What `spl-asdlc` offers the wrapper: the pass (`spl_asdlc::job::pass_from_repository`, and `spl_asdlc::job::pass` for a configuration on disk), the adapter (`spl_asdlc::tracker::livela::Livela`), the slice of the store the adapter needs (`LivelaStore`), `doctor` and the gestures (`spl_asdlc::embed::Project`), and the files of a project's setup PR (`spl_asdlc::setup::files`). `docs/setup/tracker-livela.md` says what the wrapper does, so that another runtime can write its own.
- Why not a dependency on the core: the Livela repository is private and the crate's license is `UNLICENSED`; a Git dependency would name an account in `Cargo.lock` (BR-ASDLC-001); and it would give the runner two copies of the crate, so that `BridgeJob` would implement a trait the runner does not know.

### A host with several projects

A Livela workspace holds several projects; each that runs the method has its own repository, its own configuration and its own bridge.
- **One bridge job per project**, named `spl-asdlc:<prefix>` after the project's prefix, so that a failed pass is recorded per project, under `job:spl-asdlc:<prefix>`. Each job builds the adapter for its project (`Livela::new(store, prefix, ..)`, point 4), so its items land in that project and its searches see only them.
- **The configuration is read from the repository**: `.spl-asdlc/config.toml` at the default branch, through the App (`spl_asdlc::embed::read_config`), at each pass. A file that is missing, unreadable or invalid fails the pass, which writes nothing.
- **The identities are values**: the runner keeps each project's instance overlay (spec 08, "The instance overlay") in the project's settings and passes its text. The merge follows the rules of the files on disk: a key set in both is an error naming both, an instance key in the repository's file is refused. Nothing is read from the runner's disk.
- **`doctor` is callable by the runner** with its adapter (`spl_asdlc::embed::Project::doctor`): each check carries its state, its explanation and the gesture that fixes it, serialisable, and `embedded version` says whether the project's workflow callers name the minor line the runner embeds (`v1.N` for a crate pinned `~1.N`).
- **The gestures are functions** (`Project::release_status`, `cut`, `requalify`, `abandon`, `retract`): the CLI calls the same ones.

## 4. The core API the adapter uses (delivered)

- `Store::ensure_participant(ParticipantType::Agent, "spl-asdlc")`: creates the participant once.
- `Store::create_item_with(ItemType::Ticket, BaseFields { title, parent, tags, priority, .. })`: the release item and its angle children, the gate-fix tickets. The store has a priority field, so the bridge's canonical priority is set at creation: the adapter passes its level, 1 urgent … 4 low (`create_ticket`'s `priority`), and the wrapper maps it to the store's `Priority` (`docs/setup/tracker-livela.md`). A gate-fix ticket (BR-ASDLC-016) is created at 1, urgent; an item created without one keeps the store's default.
- `Store::add_web_link(id, url, title)` then `Store::find_by_web_link(url)` → ids, oldest first. A trailing slash and the case of the scheme and host are ignored.
- `Store::set_status(id, "to_deploy")` or `set_status(id, "To Deploy")`: canonical name (id or English name). An `InvalidStatus` error (code `invalid_status`) when the type has no such status.
- `Filter { tag: Some(..) }` and `Filter { parent: Some(..) }` with `list_items_filtered`: by tag, and the children of an item. With neither (`items(project, None, None)`), every item of the project: the post-deploy pass keeps those in Post Deploy (`05-release-cycle.md` §11).
- One project (`[tracker] project`). The adapter is built for one item of type `project`, named by its `prefix`, its title or its id (`LivelaStore::project`); ambiguous is an error, never a guess.
  - It creates every item in that project (`BaseFields { projects }`), a sub-item too, and lists with `Filter { project: Some(..) }`.
  - It keeps only that project's items from the store-wide searches (`find_by_web_link`, the duplicates), and a key whose item is in another project is not found.
  - The team key is the project's `prefix`, which its items' short ids carry.
- `Store::add_relation(b, RelationKind::BlockedBy, a)` → `a.relations.blocks` contains b.
- `Store::comment(id, author, text)`; `Store::list_activity(id)`: every fact carries its author.
- `Store::set_author` and `Store::current_author`.
- `Store::find_by_short_id`. It resolves by the code alone, because the human prefix may change (contract v4, decision 14). The code also matches by prefix, so the adapter refuses a code shorter than four characters.
  - A code lengthens when another item comes to share it, so a key the bridge noted earlier can come to match two items.
  - That is unreadable, and the bridge waits rather than guessing.
- MCP (for agents, not for the adapter): 21 tools, among them `set_status`, `find_by_web_link`, `search`, `name_session`, `whats_next`, `release`.

## 5. Still to come

- **A milestone under a release item, and the reviews of a PR** (BR-ASDLC-038, LVL-TCAY). The core already accepts any item as a milestone's parent (`Store::create_item_with(ItemType::Milestone, BaseFields { parent, .. })`); the store's slice has default methods that say what is not provided yet, so a runner that does not implement them builds unchanged and keeps today's layout:
  - `LivelaStore::creates_milestones` (default `false`) and `LivelaStore::create_milestone(project, title, description, parent)`: a `milestone` in the project, under the release item, its description as given (it carries the bridge's marker `spl-asdlc:milestone scope=<tag>`), signed by the current author. The bridge never sets its status, and reads it back with `item`, `items(project, None, Some(parent))` and its children like any item; its status label (`Open`, `Delivered`) is none of the method's. While `creates_milestones` is `false`, a candidate's QA items stay directly under the release item.
  - `LivelaStore::holds_pr_reviews` (default `false`), `LivelaStore::pr_reviews(id, url)` and `LivelaStore::set_pr_reviews(id, url, code)`: the reviews of the PR an item's web link names, as `PrReviews::code` writes them (`<login>:<verdict>` pairs separated by a space, a verdict being `approved`, `changes_requested`, `commented` or `dismissed`; empty for none), kept beside the PR's status and shown read-only. The sync writes a value only when it differs from the one recorded.

- **Editing a comment.** The bridge would keep one status row per PR on a ticket and rewrite it in place (REQ-MERGE-069), so that informational facts never pile up as comments. The core has no comment edit. Until it does:
  - the adapter declares it (`can_edit_comments` is false);
  - the bridge writes no rewritten row on Livela;
  - informational facts stay in its action log, and messages that ask someone to act are comments, as everywhere.

## GitHub → item sync

A ticket shows where each of its PRs stands, kept current by the runner even when no surface is open and no device is connected to GitHub. The pass is in this crate, `spl_asdlc::sync::pass_from_repository`, so that it serves any tracker; the runner hosts it as a second `DaemonJob` per project, `GithubSyncJob`, in its own code (BR-ASDLC-010; `docs/setup/tracker-livela.md`, "The GitHub sync job"). The method does not require it.
- **What it writes**, through the `Tracker` trait only: the link of a PR that names a ticket of the project (SID-1219's rules: the id in the branch or the title, or on purpose in the body; a link a person removed is never laid again), and the status of each PR linked to an open ticket (`Tracker::record_pr_status`: draft, open with its checks, merged, released in `<tag>`, closed). On Livela the status goes through `LivelaStore::set_pr_status`, on the web link of the PR; on Linear it is the subtitle of the PR's attachment. On a tracker that keeps them, it also mirrors each PR's GitHub reviews on the open tickets linked to it (`Tracker::record_pr_reviews`: each reviewer's latest verdict; a review list GitHub cut is never mirrored), read-only: GitHub is the source (BR-ASDLC-038); on Livela through `LivelaStore::set_pr_reviews`, point 5.
- **What it never writes**: a status, an assignee, a label, a description, a comment, a review comment; nothing on GitHub.
- **No heartbeat**: a value already recorded is never written again, and each write is signed by `spl-asdlc`, the store's author restored (point 2). The conformance suite checks it with its fact counter.
- **What it reads**: through the App's installation token, the PRs updated after its checkpoint (a bounded number per pass, oldest first) and the PRs linked to tickets still open, in small batches; no person's token and no search API.
- Each surface still asks GitHub when an item is opened (SID-1219, SID-1218): the recorded status is what it shows first.

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
