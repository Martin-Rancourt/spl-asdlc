# 3b. Tracker: Livela

- **Service participant**: an `agent` participant named `spl-asdlc` (the contract, `docs/livela-contract.md`, point 4).
- **Statuses**: `to_deploy` and `post_deploy` exist for `ticket` and `bug` in the `validation` group (contract, point 1); `set_status` accepts the English names, so the mapping in `spl-asdlc.toml` is the identity.
- **Hosting**: the bridge is a `DaemonJob` of the Livela runner (contract, point 3): the runner depends on the public `spl-asdlc` crate and registers `BridgeJob`; the daemon calls it every pass with `LIVELA_SPL_ASDLC_CONFIG` (path of `spl-asdlc.toml`) and `LIVELA_JOB_BUDGET_SECONDS` (60 by default); the App's key and the tracker identity come from the environment the runner starts with. It writes directly in the local store, and the facts propagate through Livela's own sync. `todo-runner jobs` runs one pass by hand.
- **Links**: the bridge adds the PR URL as a web link on the item and finds items by that URL (contract, point 3).

**Verify**: `spl-asdlc doctor` run from the runner's environment reports `tracker: ok` and `bridge-host: livela-runner`.

## The glue in the runner

The adapter (`spl_asdlc::tracker::livela::Livela`) is written against `LivelaStore`, the slice of the core's `Store` the contract promises; the runner implements it for its `Store`. This glue passed the conformance suite (`spl_asdlc::tracker::conformance::run`) against the core on 2026-10-05, every step but the comment edit, which the core does not offer (`docs/livela-contract.md`):

```rust
use spl_asdlc::tracker::livela::{Livela, LivelaItem, LivelaRelation, LivelaStore, PARTICIPANT};
use spl_asdlc::tracker::{StatusTable, conformance};
use todo_core::journal::MemoryJournal;
use todo_core::model::{ItemType, ParticipantType, Priority, RelationKind, ShortIdLookup};
use todo_core::store::{BaseFields, Filter, Sort, Store, SystemClock};

struct Glue(Store);

fn rel(k: LivelaRelation) -> RelationKind {
    match k {
        LivelaRelation::BlockedBy => RelationKind::BlockedBy,
        LivelaRelation::RelatedTo => RelationKind::RelatedTo,
        LivelaRelation::DuplicateOf => RelationKind::DuplicateOf,
    }
}

fn convert(_s: &Store, i: todo_core::model::Item) -> LivelaItem {
    let status = i.item_type.status(&i.status).map(|st| st.label.to_string()).unwrap_or(i.status.clone());
    LivelaItem {
        id: i.id.clone(),
        short_id: i.short_id.clone(),
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
        .map(|i| convert(s, i))
        .collect()
}

impl LivelaStore for Glue {
    fn bridge_participant(&mut self) -> Result<String, String> {
        let id = self.0.ensure_participant(ParticipantType::Agent, PARTICIPANT).map_err(|e| e.to_string())?;
        self.0.set_author(&id);
        Ok(id)
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
        self.0.get_item(id).map(|i| convert(&self.0, i)).map_err(|e| e.to_string())
    }
    fn items(&self, tag: Option<&str>, parent: Option<&str>) -> Vec<LivelaItem> {
        all(&self.0, Filter { tag: tag.map(str::to_string), parent: parent.map(str::to_string), ..Filter::default() })
    }
    fn duplicates_of(&self, id: &str) -> Vec<String> {
        all(&self.0, Filter::default()).into_iter().filter(|i| i.duplicate_of.as_deref() == Some(id)).map(|i| i.id).collect()
    }
    fn prefix(&self) -> String {
        todo_core::short_id::DEFAULT_PREFIX.to_string()
    }
    fn ticket_statuses(&self) -> Vec<String> {
        ItemType::Ticket.statuses().iter().map(|s| s.label.to_string()).collect()
    }
    fn find_by_web_link(&self, url: &str) -> Vec<String> {
        self.0.find_by_web_link(url)
    }
    fn create_ticket(&mut self, title: &str, description: &str, tags: &[String], assignee: Option<&str>, parent: Option<&str>, urgent: bool) -> Result<String, String> {
        self.0
            .create_item_with(ItemType::Ticket, BaseFields {
                title: title.into(),
                description: Some(description.into()),
                tags: tags.to_vec(),
                assignee: assignee.map(str::to_string),
                parent: parent.map(str::to_string),
                priority: urgent.then_some(Priority::Urgent),
                ..BaseFields::default()
            })
            .map_err(|e| e.to_string())
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

Wire it as `Livela::new(Glue(store), StatusTable::from_config(&config.tracker.statuses))`; the identity mapping is right when the core's ticket statuses carry the canonical names.
