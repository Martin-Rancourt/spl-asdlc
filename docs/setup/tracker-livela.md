# 3b. Tracker: Livela

## Starting a project: the guided setup in Livela

A project starts on SPL-ASDLC from Livela itself, not from this crate's command line. The person who owns the repository does it in Livela's interface, and every step either happens there or on the GitHub page that Livela links to. Nobody runs a `spl-asdlc` command, a `gh` command or a shell on the machine that hosts the runner.

1. **Create the project in Livela** (its prefix becomes the tickets' key), set its GitHub repository, and turn SPL-ASDLC on. The project page then shows the guided setup: each step with the state `doctor` gives it, and the gesture that completes it.
2. **The App**: install the SPL-ASDLC GitHub App on the repository, on its GitHub page (`github-app.md` for its permissions).
3. **The secrets**: set the App's id and private key as repository secrets, on the repository's settings page. `doctor` reports them « not checkable »: GitHub never shows a secret's value.
4. **The rulesets**: create them on the repository's settings page, as `repository.md` describes them, with the App as the only bypass.
5. **Connect GitHub in Livela** with the person's own identity, then open the **setup PR** from the project page. Livela opens it with that identity, because workflow files need « Workflows: write », which the App does not have. The PR holds what `spl_asdlc::setup::files` gives: `.spl-asdlc/config.toml`, the workflow callers pinned at the minor line the runner embeds (`@vX.Y`), the process page and `AGENTS.md`.
6. **Merge the setup PR** once its checks are green. At the next pass, `doctor` runs on the merged configuration. When every step is done, the project page says « doctor is green: the runner moves this project ».

From then on, the runner is the project's only bridge: a ticket moved to Ready, taken by an agent from « what's next », goes through its PR, its merge and a release with no other gesture than the method's own. Check `doctor` from a second surface (the web, the Mac or the iPhone app) with « Check again »: both read the same project.

The rest of this page describes what the runner does on its side, for whoever maintains it or writes another host.

- **Service participant**: an `agent` participant named `spl-asdlc` (`docs/reference/livela-contract.md`, point 4).
- **Statuses**: `in_review`, `to_deploy` and `post_deploy` are the `validation` group: open, still blocking, never served by "what's next" (contract, point 1). `set_status` accepts the English names, so the canonical names are the tracker's (the configuration maps no status). Triage is the one exception: Livela tickets have no Triage status (triage is an item type of its own), so the adapter writes the canonical Triage as Backlog. A non-blocking finding and a finding requalified as non-blocking land in Backlog, and the glue maps no status.
- **Refined, Ready, In Progress**: a refined ticket stays in Backlog, marked by a `refined` field Livela computes from the description (a goal, at least one acceptance criterion and a release note), which nobody writes; a person moves it to Ready when agents may take it. An implementing agent takes work from Ready only, and its verb ("implement the next ticket") moves the ticket it takes to In Progress.
- **Hosting**: the bridge is a `DaemonJob` of the Livela runner (contract, point 3), one job per project that runs the method.
  - Each job is named after its project, `spl-asdlc:<prefix>`: a failed pass is recorded under `job:spl-asdlc:<prefix>`, so a project's failure never hides another's.
  - The configuration is the repository's `.spl-asdlc/config.toml`, read at its default branch through the App at each pass (`spl_asdlc::job::pass_from_repository`), merged with the project's instance overlay, which the runner passes as a value from the project's settings. Nothing is read from the runner's disk: no checkout, no `LIVELA_SPL_ASDLC_CONFIG`.
  - `LIVELA_JOB_BUDGET_SECONDS` (60 by default) is the budget of each pass.
  - The App's key and the tracker identity come from the environment the runner starts with, or as values from its settings (`env`).
  - It writes directly in the local store, and the facts propagate through Livela's own sync.
  - A pass that changes nothing writes nothing; only a failed pass is recorded.
  - `livela-runner jobs` runs one pass by hand.
  - The runner depends on the `spl-asdlc` crate by version, from crates.io, pinned on a minor line (`~1.N`), and the project's workflow callers name the same line (`@v1.N`); `spl-asdlc` depends on nothing of Livela (contract, point 3; BR-ASDLC-010).
- **Doctor and gestures**: the runner calls them in process, with its adapter and the configuration read as above (`spl_asdlc::embed::Project`): `doctor` returns each check with its state, its explanation and the gesture that fixes it, and `embedded version` says whether the callers name the embedded minor line; `release status`, `cut`, `requalify`, `abandon` and `retract` return what they did or why they were refused, with the name of the person who acts, and write the action log as the CLI does. The `spl-asdlc` binary cannot open a Livela store.
- **Setup PR**: `spl_asdlc::setup::files` gives the files of a project's setup PR (the configuration's shape, the callers at the embedded minor line, the process page, `AGENTS.md`) and its branch, title and body; the runner opens it with the person's GitHub identity, since the workflow files need "Workflows: write" and the App keeps its narrow permissions.
- **Locks**: the bridge writes its transitions whoever holds the item's lock; a lock protects an agent's work on the content, not the status (contract, point 6; BR-ASDLC-011).
- **Links**: the bridge adds the PR URL as a web link on the item and finds items by that URL (contract, point 4).
- **Project**: `[tracker] project` names the Livela project, by its prefix (`TD`), its name or its id; the Livela host always has one.
  - The items the bridge creates land in it, sub-items included, and their short ids carry its prefix, which is the team key.
  - The bridge's searches see only its items: by tag, by PR URL, the children and the duplicates of an item. A key of another project's item is not found.
  - `doctor` reports the project found or unknown. While no project answers to the key, every read and write of the adapter fails, naming the project, before any write.

**Verify**: the runner's `doctor` for the project (`Project::doctor`) reports `tracker: ok`, `tracker.project: ok` and `embedded version: ok`.

## What the runner writes on its side

The operator copies nothing: the glue and the daemon job live in the runner's code, in the Livela repository, next to its other jobs, and depend on `spl-asdlc` by version (contract, point 3). This crate depends on no `livela-core`, so it could not hold them. The listings below are illustrative sketches of what each consumer writes on its side, kept so that another runtime can write its own; the runner's code is the reference.

### The store glue

The adapter (`spl_asdlc::tracker::livela::Livela`) is written against `LivelaStore`, the slice of the core's `Store` the contract promises. The runner implements it for its `Store`.

The adapter is built for one project (`[tracker] project`). For it, the slice has:

- `project(reference)`: the project a prefix, name or id names (`LivelaProject { id, prefix, name }`), `None` when none does, an error when several do;
- `items(project, tag, parent)`: the items of that project only;
- `create_ticket(project, ..)`: the new ticket lands in that project;
- `LivelaItem::projects`: the projects an item belongs to, which the adapter checks on every item it reads, so that a key, a PR link or a duplicate of another project is never its own.

The store-wide `prefix()` is gone: the team key is the project's prefix.

For the GitHub sync ("The GitHub sync job" below), the slice has:

- `LivelaItem::pr_statuses`: the status recorded for each PR the item links, `(url, code)`, the code of `PrStatus::code` (`draft`, `open:passing`, `merged`, `released:<tag>`, …);
- `set_pr_status(id, url, code)`: record it, on the web link of that PR (the core field SID-1253 adds; the glue writes it as one fact).

This glue was run against `livela-core` on 2026-10-05 and passed the conformance suite (`spl_asdlc::tracker::conformance::run_with`, 60 steps), with a journal that counts facts:

- every step passed except the comment edit, which the core does not offer;
- writing unchanged values recorded no fact;
- every fact on the items was signed by `spl-asdlc`;
- the runner's own author was restored after each write.

The project parts (`project`, the `project` of `items` and `create_ticket`, `projects` in `convert`) came after that run and are a sketch, not yet run against the core. In this crate, the suite runs on two projects of one in-memory store, and neither sees the other's items.

```rust
use livela_core::{
    BaseFields, Filter, ItemType, ParticipantType, Priority, RelationKind, ShortIdLookup, Sort, Store,
};
use spl_asdlc::tracker::livela::{LivelaItem, LivelaProject, LivelaRelation, LivelaStore, PARTICIPANT};

/// The runner's store, as the `spl-asdlc` adapter sees it.
pub struct Glue<'a>(pub &'a mut Store);

fn rel(k: LivelaRelation) -> RelationKind {
    match k {
        LivelaRelation::BlockedBy => RelationKind::BlockedBy,
        LivelaRelation::RelatedTo => RelationKind::RelatedTo,
        LivelaRelation::DuplicateOf => RelationKind::DuplicateOf,
    }
}

fn convert(i: livela_core::Item) -> LivelaItem {
    let status = i.item_type.status(&i.status).map(|st| st.label.to_string()).unwrap_or(i.status.clone());
    LivelaItem {
        id: i.id,
        short_id: i.short_id,
        title: i.title,
        description: i.description,
        status,
        tags: i.tags,
        assignee: i.assignee,
        parent: i.relations.parent,
        blocked_by: i.relations.blocked_by,
        blocks: i.relations.blocks,
        related_to: i.relations.related_to,
        duplicate_of: i.relations.duplicate_of,
        comments: i.comments.into_iter().map(|c| (c.id, c.author, c.text)).collect(),
        pr_statuses: i.web_links.iter().filter_map(|w| Some((w.url.clone(), w.pr_status.clone()?))).collect(),
        web_links: i.web_links.into_iter().map(|w| w.url).collect(),
        deleted: i.deleted,
        projects: i.projects,
    }
}

fn all(s: &Store, f: Filter) -> Vec<LivelaItem> {
    s.list_items_filtered(&Filter { include_unavailable: true, ..f }, Sort::default())
        .into_iter()
        .map(convert)
        .collect()
}

fn priority(p: Option<u8>) -> Option<Priority> {
    match p? {
        1 => Some(Priority::Urgent),
        2 => Some(Priority::High),
        3 => Some(Priority::Medium),
        _ => Some(Priority::Low),
    }
}

impl LivelaStore for Glue<'_> {
    fn bridge_participant(&mut self) -> Result<String, String> {
        self.0.ensure_participant(ParticipantType::Agent, PARTICIPANT).map_err(|e| e.to_string())
    }
    fn existing_bridge_participant(&self) -> Option<String> {
        let mut ids: Vec<String> = self
            .0
            .list_participants()
            .into_iter()
            .filter(|p| p.name == PARTICIPANT && p.participant_type == ParticipantType::Agent)
            .map(|p| p.id)
            .collect();
        ids.sort();
        ids.into_iter().next()
    }
    fn swap_author(&mut self, participant: &str) -> String {
        let previous = self.0.current_author();
        self.0.set_author(participant);
        previous
    }
    fn participants_named(&self, name: &str) -> Vec<(String, String)> {
        self.0.list_participants().into_iter().filter(|p| p.name == name).map(|p| (p.id, p.name)).collect()
    }
    fn participant_name(&self, id: &str) -> Option<String> {
        self.0.get_participant(id).ok().map(|p| p.name)
    }
    fn project(&self, reference: &str) -> Result<Option<LivelaProject>, String> {
        // The items of type `project`, by their prefix, their title or their id.
        let projects = Filter { item_type: Some(ItemType::Project), ..Filter::default() };
        let mut found: Vec<LivelaProject> = self
            .0
            .list_items_filtered(&projects, Sort::default())
            .into_iter()
            .filter(|p| !p.deleted)
            .map(|p| LivelaProject { prefix: p.prefix.unwrap_or_default(), id: p.id, name: p.title })
            .filter(|p| p.id == reference || p.name == reference || p.prefix.eq_ignore_ascii_case(reference))
            .collect();
        match found.len() {
            0 | 1 => Ok(found.pop()),
            n => Err(format!("{n} projects answer to {reference}")),
        }
    }
    fn resolve(&self, short_id: &str) -> Result<Option<String>, String> {
        match self.0.find_by_short_id(short_id) {
            ShortIdLookup::Found { id } => Ok(Some(id)),
            ShortIdLookup::NotFound => Ok(None),
            ShortIdLookup::Ambiguous { ids } => Err(format!("{} items share {short_id}", ids.len())),
        }
    }
    fn item(&self, id: &str) -> Result<LivelaItem, String> {
        self.0.get_item(id).map(convert).map_err(|e| e.to_string())
    }
    fn items(&self, project: &str, tag: Option<&str>, parent: Option<&str>) -> Vec<LivelaItem> {
        all(
            &self.0,
            Filter {
                project: Some(project.to_string()),
                tag: tag.map(str::to_string),
                parent: parent.map(str::to_string),
                ..Filter::default()
            },
        )
    }
    fn duplicates_of(&self, id: &str) -> Vec<String> {
        all(&self.0, Filter::default()).into_iter().filter(|i| i.duplicate_of.as_deref() == Some(id)).map(|i| i.id).collect()
    }
    fn ticket_statuses(&self) -> Vec<String> {
        ItemType::Ticket.statuses().iter().map(|s| s.label.to_string()).collect()
    }
    fn find_by_web_link(&self, url: &str) -> Vec<String> {
        self.0.find_by_web_link(url)
    }
    fn create_ticket(
        &mut self,
        project: &str,
        title: &str,
        description: &str,
        tags: &[String],
        assignee: Option<&str>,
        parent: Option<&str>,
        priority_: Option<u8>,
        status: &str,
    ) -> Result<String, String> {
        let id = self
            .0
            .create_item_with(
                ItemType::Ticket,
                BaseFields {
                    title: title.into(),
                    description: Some(description.into()).filter(|d: &String| !d.is_empty()),
                    tags: tags.to_vec(),
                    assignee: assignee.map(str::to_string),
                    parent: parent.map(str::to_string),
                    projects: vec![project.to_string()],
                    priority: priority(priority_),
                    ..BaseFields::default()
                },
            )
            .map_err(|e| e.to_string())?;
        // The ticket starts in the type's initial status; one more fact only when it differs.
        if !ItemType::Ticket.initial_status().label.eq_ignore_ascii_case(status) {
            self.0.set_status(&id, status).map_err(|e| e.to_string())?;
        }
        Ok(id)
    }
    fn set_status(&mut self, id: &str, status: &str) -> Result<(), String> {
        self.0.set_status(id, status).map_err(|e| e.to_string())
    }
    fn set_description(&mut self, id: &str, text: &str) -> Result<(), String> {
        self.0.set_field(id, "description", serde_json::json!(text)).map_err(|e| e.to_string())
    }
    fn add_tag(&mut self, id: &str, tag: &str) -> Result<(), String> {
        self.0.add_tag(id, tag).map_err(|e| e.to_string())
    }
    fn remove_tag(&mut self, id: &str, tag: &str) -> Result<(), String> {
        self.0.remove_tag(id, tag).map_err(|e| e.to_string())
    }
    fn assign(&mut self, id: &str, participant: Option<&str>) -> Result<(), String> {
        self.0.assign(id, participant).map_err(|e| e.to_string())
    }
    fn comment(&mut self, id: &str, author: &str, text: &str) -> Result<String, String> {
        self.0.comment(id, author, text).map_err(|e| e.to_string())
    }
    fn add_relation(&mut self, id: &str, kind: LivelaRelation, target: &str) -> Result<(), String> {
        self.0.add_relation(id, rel(kind), target).map_err(|e| e.to_string())
    }
    fn remove_relation(&mut self, id: &str, kind: LivelaRelation, target: &str) -> Result<(), String> {
        self.0.remove_relation(id, rel(kind), target).map_err(|e| e.to_string())
    }
    fn add_web_link(&mut self, id: &str, url: &str, title: &str) -> Result<(), String> {
        self.0.add_web_link(id, url, Some(title)).map(|_| ()).map_err(|e| e.to_string())
    }
    // The PR status field of a web link is the core's (SID-1253): a sketch.
    fn set_pr_status(&mut self, id: &str, url: &str, code: &str) -> Result<(), String> {
        self.0.set_web_link_pr_status(id, url, code).map_err(|e| e.to_string())
    }
}
```

Wire it as `Livela::new(Glue(&mut store), project, StatusTable::identity())`, `project` being the configuration's `[tracker] project`. The project is resolved on first need, so the adapter is built even when no project answers yet.

The adapter reads before every write and skips a write that changes nothing. It signs each write as `spl-asdlc` through `swap_author` and gives the store back its previous author.

### The daemon job

The runner's `BridgeJob` (`docs/reference/livela-contract.md` §3) is a wrapper of about thirty lines, one instance per project that runs the method, registered with the runner's jobs in `crates/livela-runner/src/jobs.rs`. What it does:

- its name is `spl-asdlc:<prefix>`, so a failed pass is recorded under `job:spl-asdlc:<prefix>`;
- it holds what the project's settings give: the repository, the project's prefix and the instance overlay's text;
- it builds the App's client and the adapter on the runner's store for that project;
- it runs one pass (`spl_asdlc::job::pass_from_repository`) with 80% of the job's budget, and turns the pass's outcome into the runner's `JobOutcome`; a configuration that cannot be read at the default branch fails the pass, which then writes nothing;
- the action log and its lock go under the runner's state directory, one file per project.

A sketch:

```rust
use std::path::PathBuf;

use livela_core::{DaemonJob, JobContext, JobOutcome, Store};
use spl_asdlc::config::Source;
use spl_asdlc::github::RepoSlug;
use spl_asdlc::job::{RepositoryHost, pass_from_repository};
use spl_asdlc::tracker::StatusTable;
use spl_asdlc::tracker::livela::Livela;

/// The bridge of one project, as a job of the runner's daemon (`docs/reference/livela-contract.md` §3).
pub struct BridgeJob {
    pub name: String,           // `spl-asdlc:<prefix>`
    pub repository: RepoSlug,   // the project's repository
    pub prefix: String,         // the project's prefix: `[tracker] project`
    pub instance: String,       // the instance overlay, from the project's settings
    pub log: PathBuf,           // under the runner's state directory
}

impl DaemonJob for BridgeJob {
    fn name(&self) -> &str {
        &self.name
    }

    fn run(&mut self, store: &mut Store, ctx: &JobContext) -> JobOutcome {
        let env = |k: &str| std::env::var(k).ok();
        let github = match spl_asdlc::host::app_github(&env) {
            Ok(g) => g,
            Err(e) => return JobOutcome::failed(e),
        };
        let tracker = Livela::new(Glue(store), &self.prefix, StatusTable::identity());
        let out = pass_from_repository(
            &RepositoryHost {
                repository: self.repository.clone(),
                instance: Some(Source { label: format!("{} settings", self.name), text: self.instance.clone() }),
                tracker: &tracker,
                // Stop taking decisions with a margin: the runner fails a pass over its budget.
                budget: Some(ctx.budget.mul_f32(0.8)),
                log_path: Some(self.log.clone()),
                env: &env,
            },
            &github,
        );
        if out.ok { JobOutcome::ok(out.message) } else { JobOutcome::failed(out.message) }
    }
}
```

The single-project job of 1.3, which read `LIVELA_SPL_ASDLC_CONFIG` with `spl_asdlc::job::pass`, was checked against `livela-core` with `run_jobs`:

- Without GitHub credentials, the pass fails and says why (`SPL_ASDLC_APP_ID is not set`).
- It writes no item.
- The runner records the failure, and only it, under the job's entity.

A pass that works leaves only its writes on the items; a pass with nothing to do leaves nothing. The tick stops taking decisions at 80% of `LIVELA_JOB_BUDGET_SECONDS`, because the runner fails a pass that overruns its budget.

The runner takes the `spl-asdlc` crate by version, from crates.io; the arrow never points the other way (BR-ASDLC-010).

Every pass, the bridge's and the sync's, reads the App's quota first and ends its message with what is left (`GitHub quota: core <remaining>/<limit> until <hh:mm>Z, graphql …`), ok or failed. Every project of the runner shares the App's one installation, so one quota. Low on it, the sync waits and the bridge skips its sweep; almost out, the bridge reads nothing until the reset. Both answer ok and say so in the message (`07-the-bridge.md`, "What a tick does"). A runner that writes only failed passes to its journal shows the line of a pass that works only in its job record: to follow the quota, it writes the line of every pass.

### The GitHub sync job

A second `DaemonJob` per project, beside the bridge's, also in the runner's code (BR-ASDLC-010): `GithubSyncJob`, named `spl-asdlc-sync:<prefix>`, so that its failed passes are recorded under `job:spl-asdlc-sync:<prefix>`. The method does not require it: a project runs SPL-ASDLC without it. It is the bridge's job with another pass:

- it takes the same inputs: the repository, the project's adapter, the instance overlay as a value, the App's client, 80% of the budget, and a `log_path` under the runner's state directory;
- it runs `spl_asdlc::sync::pass_from_repository`, whose memory (the last pass, the links it laid, the PRs it follows) is the file `<log stem>.sync.json` beside that path, with its own lock.

What a pass writes, through the adapter only, each write signed by `spl-asdlc` with the store's author restored:

- the link of a PR that names a ticket of the project, by Livela's rules (`pr_names_item`, SID-1219): the ticket's id in the branch or the title, or in the body on purpose only (a line `Refs`, `Closes`, `Fixes`, `Resolves`, or `livela://item/<id>`); a draft is linked like any PR; a link already there is not written again, and a link a person removed is never laid again;
- the status of each PR linked to an open ticket (`set_pr_status`): draft, open with its checks (passing, failing, pending), merged, released in `<tag>`, or closed; a status already recorded is never written again, so a pass with nothing new writes nothing.

What it never writes: a status, an assignee, a label, a description, a comment, anything on GitHub. Those stay the bridge's, which moves the tickets from the facts as before.

What it reads: PRs updated after its durable checkpoint, oldest first, at most 50 per pass (the first pass starts at the beginning of the repository history), and PRs linked to tickets still open, by their number, at most 100 per pass in turn from where the last pass stopped; the release tags, to tell a released PR (the oldest release holding its merge commit, found in a few compares). It lists them from the PR list (`/pulls`, newest update first down to the checkpoint), never the issue list: the App has no Issues permission (`github-app.md`), then reads the PRs themselves 25 to a GraphQL request, with only what the sync needs (no labels, reviews or threads): a larger request runs into GitHub's timeout. No search API, no person's token.

The checkpoint is the list's update time of the last PR handled, written after each one: a pass stopped by its time budget or by a GitHub error keeps what it did, and the next pass resumes after it. A pass the budget stopped after some work is a step of a catch-up and reports OK; one that could do nothing within it fails. A PR whose link or status fails no longer holds the checkpoint back: it is read again with the followed PRs until it goes through. The pass's failures are on its one line, after its counts.

