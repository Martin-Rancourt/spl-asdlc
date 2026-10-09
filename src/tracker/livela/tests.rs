//! The Livela adapter on an in-memory store that keeps the contract's
//! rules: ticket statuses by canonical name, relations held by one side,
//! web links, participants, and a fact signed by the current author for
//! every write, as the core's log does. The glue for the real store is
//! verified against the Livela core (`docs/setup/tracker-livela.md`).

use super::*;
use std::collections::BTreeMap;

const STATUSES: [&str; 9] = [
    "Triage",
    "Backlog",
    "Ready",
    "In Progress",
    "In Review",
    "To Deploy",
    "Post Deploy",
    "Done",
    "Canceled",
];

struct MemStore {
    items: BTreeMap<String, LivelaItem>,
    participants: BTreeMap<String, String>,
    next: u64,
    author: String,
    /// `(author, write)`, as the core's fact log signs them.
    facts: Vec<(String, String)>,
}

impl Default for MemStore {
    fn default() -> Self {
        MemStore {
            items: BTreeMap::new(),
            participants: BTreeMap::new(),
            next: 0,
            author: "runner".into(),
            facts: Vec::new(),
        }
    }
}

impl MemStore {
    fn id(&mut self) -> String {
        self.next += 1;
        format!("{:08}", self.next)
    }

    fn fact(&mut self, what: String) {
        self.facts.push((self.author.clone(), what));
    }
}

impl LivelaStore for MemStore {
    fn bridge_participant(&mut self) -> Result<String, String> {
        if let Some(id) = self.existing_bridge_participant() {
            return Ok(id);
        }
        let id = format!("p-{}", self.id());
        self.participants.insert(id.clone(), PARTICIPANT.into());
        self.fact(format!("participant {id}"));
        Ok(id)
    }
    fn existing_bridge_participant(&self) -> Option<String> {
        self.participants
            .iter()
            .find(|(_, n)| *n == PARTICIPANT)
            .map(|(id, _)| id.clone())
    }
    fn swap_author(&mut self, participant: &str) -> String {
        std::mem::replace(&mut self.author, participant.to_string())
    }
    fn participants_named(&self, name: &str) -> Vec<(String, String)> {
        self.participants
            .iter()
            .filter(|(_, n)| *n == name)
            .map(|(i, n)| (i.clone(), n.clone()))
            .collect()
    }
    fn participant_name(&self, id: &str) -> Option<String> {
        self.participants.get(id).cloned()
    }
    fn resolve(&self, short_id: &str) -> Result<Option<String>, String> {
        Ok(self
            .items
            .values()
            .find(|i| code_of(&i.short_id).eq_ignore_ascii_case(code_of(short_id)))
            .map(|i| i.id.clone()))
    }
    fn item(&self, id: &str) -> Result<LivelaItem, String> {
        let mut item = self.items.get(id).cloned().ok_or("no such item")?;
        item.blocks = self
            .items
            .values()
            .filter(|o| o.blocked_by.contains(&id.to_string()))
            .map(|o| o.id.clone())
            .collect();
        Ok(item)
    }
    fn items(&self, tag: Option<&str>, parent: Option<&str>) -> Vec<LivelaItem> {
        self.items
            .values()
            .filter(|i| tag.is_none_or(|t| i.tags.iter().any(|x| x == t)))
            .filter(|i| parent.is_none_or(|p| i.parent.as_deref() == Some(p)))
            .cloned()
            .collect()
    }
    fn duplicates_of(&self, id: &str) -> Vec<String> {
        self.items
            .values()
            .filter(|i| i.duplicate_of.as_deref() == Some(id))
            .map(|i| i.id.clone())
            .collect()
    }
    fn prefix(&self) -> String {
        "TD".into()
    }
    fn ticket_statuses(&self) -> Vec<String> {
        STATUSES.iter().map(|s| s.to_string()).collect()
    }
    fn find_by_web_link(&self, url: &str) -> Vec<String> {
        self.items
            .values()
            .filter(|i| i.web_links.iter().any(|w| w == url))
            .map(|i| i.id.clone())
            .collect()
    }
    fn create_ticket(
        &mut self,
        title: &str,
        description: &str,
        tags: &[String],
        assignee: Option<&str>,
        parent: Option<&str>,
        _priority: Option<u8>,
        status: &str,
    ) -> Result<String, String> {
        if !STATUSES.contains(&status) {
            return Err(format!("ticket has no status {status}"));
        }
        let id = self.id();
        self.fact(format!("create {id}"));
        let item = LivelaItem {
            id: id.clone(),
            short_id: format!("TD-{}", &id[4..]),
            title: title.into(),
            description: description.into(),
            status: status.into(),
            tags: tags.to_vec(),
            assignee: assignee.map(str::to_string),
            parent: parent.map(str::to_string),
            ..LivelaItem::default()
        };
        self.items.insert(id.clone(), item);
        Ok(id)
    }
    fn set_status(&mut self, id: &str, status: &str) -> Result<(), String> {
        self.fact(format!("status {id} {status}"));
        let canon = STATUSES
            .iter()
            .find(|s| s.eq_ignore_ascii_case(status))
            .ok_or(format!("ticket has no status {status}"))?;
        self.items.get_mut(id).ok_or("no item")?.status = canon.to_string();
        Ok(())
    }
    fn set_description(&mut self, id: &str, text: &str) -> Result<(), String> {
        self.fact(format!("describe {id}"));
        self.items.get_mut(id).ok_or("no item")?.description = text.into();
        Ok(())
    }
    fn add_tag(&mut self, id: &str, tag: &str) -> Result<(), String> {
        self.fact(format!("tag {id} +{tag}"));
        let i = self.items.get_mut(id).ok_or("no item")?;
        if !i.tags.iter().any(|t| t == tag) {
            i.tags.push(tag.into());
        }
        Ok(())
    }
    fn remove_tag(&mut self, id: &str, tag: &str) -> Result<(), String> {
        self.fact(format!("tag {id} -{tag}"));
        self.items
            .get_mut(id)
            .ok_or("no item")?
            .tags
            .retain(|t| t != tag);
        Ok(())
    }
    fn assign(&mut self, id: &str, participant: Option<&str>) -> Result<(), String> {
        self.fact(format!("assign {id} {participant:?}"));
        self.items.get_mut(id).ok_or("no item")?.assignee = participant.map(str::to_string);
        Ok(())
    }
    fn comment(&mut self, id: &str, author: &str, text: &str) -> Result<String, String> {
        self.fact(format!("comment {id}"));
        let cid = format!("c-{}", self.id());
        self.items.get_mut(id).ok_or("no item")?.comments.push((
            cid.clone(),
            author.into(),
            text.into(),
        ));
        Ok(cid)
    }
    fn add_relation(&mut self, id: &str, kind: LivelaRelation, target: &str) -> Result<(), String> {
        if !self.items.contains_key(target) {
            return Err("no target".into());
        }
        self.fact(format!("relate {id} {kind:?} {target}"));
        let i = self.items.get_mut(id).ok_or("no item")?;
        match kind {
            LivelaRelation::BlockedBy => i.blocked_by.push(target.into()),
            LivelaRelation::RelatedTo => i.related_to.push(target.into()),
            LivelaRelation::DuplicateOf => i.duplicate_of = Some(target.into()),
        }
        Ok(())
    }
    fn remove_relation(
        &mut self,
        id: &str,
        kind: LivelaRelation,
        target: &str,
    ) -> Result<(), String> {
        self.fact(format!("unrelate {id} {kind:?} {target}"));
        let i = self.items.get_mut(id).ok_or("no item")?;
        match kind {
            LivelaRelation::BlockedBy => i.blocked_by.retain(|x| x != target),
            LivelaRelation::RelatedTo => i.related_to.retain(|x| x != target),
            LivelaRelation::DuplicateOf => i.duplicate_of = None,
        }
        Ok(())
    }
    fn add_web_link(&mut self, id: &str, url: &str, _title: &str) -> Result<(), String> {
        self.fact(format!("link {id} {url}"));
        self.items
            .get_mut(id)
            .ok_or("no item")?
            .web_links
            .push(url.into());
        Ok(())
    }
}

fn new_item(title: &str, status: Status) -> NewItem {
    NewItem {
        title: title.into(),
        description: String::new(),
        labels: vec![],
        status,
        priority: None,
        assignee: None,
        parent: None,
    }
}

#[test]
fn the_livela_adapter_passes_the_conformance_suite_without_comment_edits() {
    let t = Livela::new(MemStore::default(), StatusTable::identity());
    let facts = || t.store.borrow().facts.len() as u64;
    let report = conformance::run_with(&t, &StatusTable::identity(), "1", Some(&facts)).unwrap();
    assert_eq!(
        report.not_offered,
        vec!["comment edit"],
        "the one missing point of the contract"
    );
    assert!(
        report
            .passed
            .contains(&"unchanged writes record nothing".to_string())
    );
}

#[test]
fn every_write_is_signed_by_the_bridge_and_the_runner_keeps_its_author() {
    let t = Livela::new(MemStore::default(), StatusTable::identity());
    let a = t.create_item(&new_item("a", Status::Ready)).unwrap();
    let b = t.create_item(&new_item("b", Status::Ready)).unwrap();
    t.set_status(&a.key, Status::InReview, None).unwrap();
    t.update_description(&a.key, "x").unwrap();
    t.add_label(&a.key, "l").unwrap();
    t.relate(&a.key, &b.key, RelationKind::Blocks).unwrap();
    t.link_pr(&a.key, "https://github.com/o/r/pull/1", "PR")
        .unwrap();
    t.comment(&a.key, "hi").unwrap();
    let me = t.identity().unwrap().id;
    let store = t.into_store();
    assert_eq!(store.author, "runner", "the previous author is restored");
    let unsigned: Vec<_> = store
        .facts
        .iter()
        .filter(|(by, what)| *by != me && !what.starts_with("participant"))
        .collect();
    assert!(unsigned.is_empty(), "{unsigned:?}");
}

#[test]
fn reads_and_unchanged_writes_leave_no_fact() {
    let t = Livela::new(MemStore::default(), StatusTable::identity());
    let a = t.create_item(&new_item("a", Status::ToDeploy)).unwrap();
    let before = t.store.borrow().facts.len();
    t.get(&a.key).unwrap();
    t.comments(&a.key).unwrap();
    t.list_by_label("x").unwrap();
    t.set_status(&a.key, Status::ToDeploy, None).unwrap();
    t.update_description(&a.key, "").unwrap();
    t.remove_label(&a.key, "absent").unwrap();
    t.assign(&a.key, None).unwrap();
    assert_eq!(t.store.borrow().facts.len(), before);
}

#[test]
fn a_status_write_can_unassign_and_a_replay_leaves_no_fact() {
    let t = Livela::new(MemStore::default(), StatusTable::identity());
    let me = t.identity().unwrap();
    let mut item = new_item("a", Status::ToDeploy);
    item.assignee = Some(me);
    let a = t.create_item(&item).unwrap();
    t.set_status(&a.key, Status::InProgress, Some(None))
        .unwrap();
    let back = t.get(&a.key).unwrap();
    assert_eq!(back.state.status, Some(Status::InProgress));
    assert_eq!(back.assignee, None);
    let before = t.store.borrow().facts.len();
    t.set_status(&a.key, Status::InProgress, Some(None))
        .unwrap();
    assert_eq!(t.store.borrow().facts.len(), before);
}

#[test]
fn a_creation_in_an_unknown_status_writes_nothing() {
    let mut map = std::collections::BTreeMap::new();
    map.insert("Ready".to_string(), "Prete".to_string());
    let t = Livela::new(MemStore::default(), StatusTable::from_config(&map));
    let err = t.create_item(&new_item("a", Status::Ready)).unwrap_err();
    assert!(matches!(err, TrackerError::Refused { .. }), "{err}");
    assert!(t.store.borrow().items.is_empty());
    assert!(t.store.borrow().facts.is_empty());
}

#[test]
fn the_validation_statuses_are_open_on_livela() {
    let t = Livela::new(MemStore::default(), StatusTable::identity());
    let a = t.create_item(&new_item("a", Status::PostDeploy)).unwrap();
    assert_eq!(a.state.status, Some(Status::PostDeploy));
    assert!(!a.state.is_resolved());
}

#[test]
fn a_key_resolves_by_its_code_and_never_by_a_shorter_one() {
    let t = Livela::new(MemStore::default(), StatusTable::identity());
    let a = t.create_item(&new_item("a", Status::Ready)).unwrap();
    assert!(t.get(&a.key).is_ok());
    let code = a.key.split_once('-').unwrap().1;
    assert!(
        t.get(&format!("XY-{code}")).is_ok(),
        "the human prefix may change"
    );
    assert!(
        t.get(&format!("TD-{}", &code[..3]))
            .unwrap_err()
            .is_not_found(),
        "too short"
    );
}

#[test]
fn a_second_duplicate_target_is_refused_not_overwritten() {
    let t = Livela::new(MemStore::default(), StatusTable::identity());
    let a = t.create_item(&new_item("a", Status::Triage)).unwrap();
    let b = t.create_item(&new_item("b", Status::Triage)).unwrap();
    let c = t.create_item(&new_item("c", Status::Triage)).unwrap();
    t.relate(&a.key, &b.key, RelationKind::Duplicate).unwrap();
    let err = t
        .relate(&a.key, &c.key, RelationKind::Duplicate)
        .unwrap_err();
    assert!(matches!(err, TrackerError::Refused { .. }));
    assert_eq!(t.relations(&a.key).unwrap().duplicates[0].other, b.key);
}

#[test]
fn a_blocks_relation_reads_from_both_ends_and_unrelates_by_its_id() {
    let t = Livela::new(MemStore::default(), StatusTable::identity());
    let mk = |title: &str| {
        t.create_item(&NewItem {
            title: title.into(),
            description: String::new(),
            labels: vec![],
            status: Status::Triage,
            priority: None,
            assignee: None,
            parent: None,
        })
        .unwrap()
    };
    let (release, blocker) = (mk("release"), mk("blocker"));
    t.relate(&blocker.key, &release.key, RelationKind::Blocks)
        .unwrap();
    let r = t.relations(&release.key).unwrap();
    assert_eq!(r.blocked_by[0].other, blocker.key);
    let b = t.relations(&blocker.key).unwrap();
    assert_eq!(b.blocks[0].other, release.key);
    assert_eq!(
        b.blocks[0].id, r.blocked_by[0].id,
        "one relation, one id from either end"
    );
    t.unrelate(&r.blocked_by[0].id).unwrap();
    assert!(t.relations(&release.key).unwrap().blocked_by.is_empty());
}

#[test]
fn duplicates_are_read_in_both_directions() {
    let t = Livela::new(MemStore::default(), StatusTable::identity());
    let mk = |title: &str| {
        t.create_item(&NewItem {
            title: title.into(),
            description: String::new(),
            labels: vec![],
            status: Status::Triage,
            priority: None,
            assignee: None,
            parent: None,
        })
        .unwrap()
    };
    let (a, b) = (mk("a"), mk("b"));
    t.relate(&b.key, &a.key, RelationKind::Duplicate).unwrap();
    assert_eq!(t.relations(&a.key).unwrap().duplicates[0].other, b.key);
    assert_eq!(t.relations(&b.key).unwrap().duplicates[0].other, a.key);
}

#[test]
fn an_unknown_short_id_is_not_found_and_a_comment_edit_is_refused() {
    let t = Livela::new(MemStore::default(), StatusTable::identity());
    assert!(t.get("TD-NOPE").unwrap_err().is_not_found());
    assert!(matches!(
        t.update_comment("c-1", "x").unwrap_err(),
        TrackerError::Refused { .. }
    ));
}

#[test]
fn livela_has_no_open_item_limit_and_archives_nothing() {
    let t = Livela::new(MemStore::default(), StatusTable::identity());
    let a = t.create_item(&new_item("a", Status::Done)).unwrap();
    let before = t.store.borrow().facts.len();
    assert!(!t.archives(), "the bridge asks Livela for no archive");
    t.archive(&a.key).unwrap();
    assert_eq!(t.store.borrow().facts.len(), before, "no fact recorded");
    assert!(!t.get(&a.key).unwrap().archived);
}
