# 3b. Tracker: Livela

- **Service participant**: an `agent` participant named `spl-asdlc` (`docs/reference/livela-contract.md`, point 4).
- **Statuses**: `in_review`, `to_deploy` and `post_deploy` are the `validation` group: open, still blocking, never served by "what's next" (contract, point 1). `set_status` accepts the English names, so the canonical names are the tracker's (the configuration maps no status).
- **Refined, Ready, In Progress**: a refined ticket stays in Backlog, in the dedicated refined state; a person moves it to Ready when agents may take it. An implementing agent takes work from Ready only, and its verb ("implement the next ticket") moves the ticket it takes to In Progress.
- **Hosting**: the bridge is a `DaemonJob` of the Livela runner (contract, point 3).
  - The daemon calls it every pass with `LIVELA_SPL_ASDLC_CONFIG` (the path of `.spl-asdlc/config.toml`; its instance overlay is the `config.instance.toml` beside it, when there is one) and `LIVELA_JOB_BUDGET_SECONDS` (60 by default).
  - The App's key and the tracker identity come from the environment the runner starts with.
  - It writes directly in the local store, and the facts propagate through Livela's own sync.
  - A pass that changes nothing writes nothing; only a failed pass is recorded, under `job:spl-asdlc`.
  - `livela-runner jobs` runs one pass by hand.
  - The runner depends on the `spl-asdlc` crate by version, from crates.io; `spl-asdlc` depends on nothing of Livela (contract, point 3; BR-ASDLC-010).
- **Locks**: the bridge writes its transitions whoever holds the item's lock; a lock protects an agent's work on the content, not the status (contract, point 6; BR-ASDLC-011).
- **Links**: the bridge adds the PR URL as a web link on the item and finds items by that URL (contract, point 4).
- **Project**: `[tracker] project` is accepted and ignored for now: the bridge creates its items in the workspace, and `doctor` reports the key as ignored.

**Verify**: `spl-asdlc doctor` run from the runner's environment reports `tracker: ok` and `bridge-host: livela-runner`.

## What the runner writes on its side

The operator copies nothing: the glue and the daemon job live in the runner's code, in the Livela repository, next to its other jobs, and depend on `spl-asdlc` by version (contract, point 3). This crate depends on no `livela-core`, so it could not hold them. The listings below are illustrative sketches of what each consumer writes on its side, kept so that another runtime can write its own; the runner's code is the reference.

### The store glue

The adapter (`spl_asdlc::tracker::livela::Livela`) is written against `LivelaStore`, the slice of the core's `Store` the contract promises. The runner implements it for its `Store`.

This glue was run against `livela-core` on 2026-10-05 and passed the conformance suite (`spl_asdlc::tracker::conformance::run_with`, 60 steps), with a journal that counts facts:

- every step passed except the comment edit, which the core does not offer;
- writing unchanged values recorded no fact;
- every fact on the items was signed by `spl-asdlc`;
- the runner's own author was restored after each write.

```rust
use livela_core::{
    BaseFields, Filter, ItemType, ParticipantType, Priority, RelationKind, ShortIdLookup, Sort, Store,
};
use spl_asdlc::tracker::livela::{LivelaItem, LivelaRelation, LivelaStore, PARTICIPANT};

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
        web_links: i.web_links.into_iter().map(|w| w.url).collect(),
        deleted: i.deleted,
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
    fn items(&self, tag: Option<&str>, parent: Option<&str>) -> Vec<LivelaItem> {
        all(&self.0, Filter { tag: tag.map(str::to_string), parent: parent.map(str::to_string), ..Filter::default() })
    }
    fn duplicates_of(&self, id: &str) -> Vec<String> {
        all(&self.0, Filter::default()).into_iter().filter(|i| i.duplicate_of.as_deref() == Some(id)).map(|i| i.id).collect()
    }
    fn prefix(&self) -> String {
        livela_core::short_id::DEFAULT_PREFIX.to_string()
    }
    fn ticket_statuses(&self) -> Vec<String> {
        ItemType::Ticket.statuses().iter().map(|s| s.label.to_string()).collect()
    }
    fn find_by_web_link(&self, url: &str) -> Vec<String> {
        self.0.find_by_web_link(url)
    }
    fn create_ticket(
        &mut self,
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
}
```

Wire it as `Livela::new(Glue(&mut store), StatusTable::identity())`.

The adapter reads before every write and skips a write that changes nothing. It signs each write as `spl-asdlc` through `swap_author` and gives the store back its previous author.

### The daemon job

The runner's `BridgeJob` (`docs/reference/livela-contract.md` §3) is a wrapper of about twenty lines, registered in one line in `registered()` of `crates/livela-runner/src/jobs.rs`. What it does:

- its name is `spl-asdlc`, so a failed pass is recorded under `job:spl-asdlc`;
- it fails, saying why, when `LIVELA_SPL_ASDLC_CONFIG` is not set or the configuration does not load;
- it builds the adapter on the runner's store, with the status table of the configuration;
- it runs one pass (`spl_asdlc::job::pass`) with 80% of the job's budget, and turns the pass's outcome into the runner's `JobOutcome`.

A sketch:

```rust
use std::path::Path;

use livela_core::{DaemonJob, JobContext, JobOutcome, Store};
use spl_asdlc::tracker::StatusTable;
use spl_asdlc::tracker::livela::Livela;

/// The bridge as a job of the runner's daemon (`docs/reference/livela-contract.md` §3).
pub struct BridgeJob;

impl DaemonJob for BridgeJob {
    fn name(&self) -> &str {
        "spl-asdlc"
    }

    fn run(&mut self, store: &mut Store, ctx: &JobContext) -> JobOutcome {
        let Some(path) = ctx.config.as_deref() else {
            return JobOutcome::failed("LIVELA_SPL_ASDLC_CONFIG is not set");
        };
        let tracker = Livela::new(Glue(store), StatusTable::identity());
        let out = spl_asdlc::job::pass(&spl_asdlc::job::Host {
            config_path: Path::new(path),
            tracker: &tracker,
            // Stop taking decisions with a margin: the runner fails a pass over its budget.
            budget: Some(ctx.budget.mul_f32(0.8)),
            log_path: None,
            env: &|k| std::env::var(k).ok(),
        });
        if out.ok { JobOutcome::ok(out.message) } else { JobOutcome::failed(out.message) }
    }
}
```

Checked against `livela-core` with `run_jobs`:

- Without GitHub credentials, the pass fails and says why (`SPL_ASDLC_APP_ID is not set`).
- It writes no item.
- The runner records the failure, and only it, under `job:spl-asdlc`.

A pass that works leaves only its writes on the items; a pass with nothing to do leaves nothing. The tick stops taking decisions at 80% of `LIVELA_JOB_BUDGET_SECONDS`, because the runner fails a pass that overruns its budget.

The runner takes the `spl-asdlc` crate by version, from crates.io; the arrow never points the other way (BR-ASDLC-010).
