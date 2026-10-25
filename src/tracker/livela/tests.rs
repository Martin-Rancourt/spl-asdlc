//! The Livela adapter on an in-memory store that keeps the contract's
//! rules: projects whose prefix their items' short ids carry, ticket statuses
//! by canonical name, relations held by one side, web links, participants,
//! and a fact signed by the current author for every write, as the core's
//! log does. The glue for the real store is verified against the Livela core
//! (`docs/setup/tracker-livela.md`).

use super::*;
use std::collections::BTreeMap;
use std::rc::Rc;

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
    /// `TD` and `QA`, two projects of one store.
    projects: Vec<LivelaProject>,
    items: BTreeMap<String, LivelaItem>,
    participants: BTreeMap<String, String>,
    next: u64,
    author: String,
    /// `(author, write)`, as the core's fact log signs them.
    facts: Vec<(String, String)>,
    /// The priority each ticket was created with.
    priorities: BTreeMap<String, Option<u8>>,
    /// A status the ticket type lacks, for the refusal tests.
    lacks: Option<&'static str>,
}

impl Default for MemStore {
    fn default() -> Self {
        let project = |prefix: &str, name: &str| LivelaProject {
            id: format!("project-{}", prefix.to_lowercase()),
            prefix: prefix.into(),
            name: name.into(),
        };
        MemStore {
            projects: vec![project("TD", "Delivery"), project("QA", "Quality")],
            items: BTreeMap::new(),
            participants: BTreeMap::new(),
            next: 0,
            author: "runner".into(),
            facts: Vec::new(),
            priorities: BTreeMap::new(),
            lacks: None,
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
    fn project(&self, reference: &str) -> Result<Option<LivelaProject>, String> {
        let found: Vec<&LivelaProject> = self
            .projects
            .iter()
            .filter(|p| {
                p.id == reference || p.prefix.eq_ignore_ascii_case(reference) || p.name == reference
            })
            .collect();
        match found.as_slice() {
            [] => Ok(None),
            [p] => Ok(Some((*p).clone())),
            _ => Err(format!("{} projects answer to {reference}", found.len())),
        }
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
    fn items(&self, project: &str, tag: Option<&str>, parent: Option<&str>) -> Vec<LivelaItem> {
        self.items
            .values()
            .filter(|i| i.projects.iter().any(|p| p == project))
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
    fn ticket_statuses(&self) -> Vec<String> {
        STATUSES
            .iter()
            .filter(|s| Some(**s) != self.lacks)
            .map(|s| s.to_string())
            .collect()
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
        project: &str,
        title: &str,
        description: &str,
        tags: &[String],
        assignee: Option<&str>,
        parent: Option<&str>,
        priority: Option<u8>,
        status: &str,
    ) -> Result<String, String> {
        if !STATUSES.contains(&status) {
            return Err(format!("ticket has no status {status}"));
        }
        let prefix = self
            .projects
            .iter()
            .find(|p| p.id == project)
            .map(|p| p.prefix.clone())
            .ok_or(format!("no project {project}"))?;
        let id = self.id();
        self.fact(format!("create {id}"));
        self.priorities.insert(id.clone(), priority);
        let item = LivelaItem {
            id: id.clone(),
            short_id: format!("{prefix}-{}", &id[4..]),
            title: title.into(),
            description: description.into(),
            status: status.into(),
            tags: tags.to_vec(),
            assignee: assignee.map(str::to_string),
            parent: parent.map(str::to_string),
            projects: vec![project.into()],
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
    fn set_pr_status(&mut self, id: &str, url: &str, code: &str) -> Result<(), String> {
        self.fact(format!("pr status {id} {url} {code}"));
        let item = self.items.get_mut(id).ok_or("no item")?;
        item.pr_statuses.retain(|(u, _)| u != url);
        item.pr_statuses.push((url.into(), code.into()));
        Ok(())
    }
}

/// One store shared by the adapters of its two projects.
type Shared = Rc<RefCell<MemStore>>;

impl LivelaStore for Shared {
    fn bridge_participant(&mut self) -> Result<String, String> {
        self.borrow_mut().bridge_participant()
    }
    fn existing_bridge_participant(&self) -> Option<String> {
        self.borrow().existing_bridge_participant()
    }
    fn swap_author(&mut self, participant: &str) -> String {
        self.borrow_mut().swap_author(participant)
    }
    fn participants_named(&self, name: &str) -> Vec<(String, String)> {
        self.borrow().participants_named(name)
    }
    fn participant_name(&self, id: &str) -> Option<String> {
        self.borrow().participant_name(id)
    }
    fn project(&self, reference: &str) -> Result<Option<LivelaProject>, String> {
        self.borrow().project(reference)
    }
    fn resolve(&self, short_id: &str) -> Result<Option<String>, String> {
        self.borrow().resolve(short_id)
    }
    fn item(&self, id: &str) -> Result<LivelaItem, String> {
        self.borrow().item(id)
    }
    fn items(&self, project: &str, tag: Option<&str>, parent: Option<&str>) -> Vec<LivelaItem> {
        self.borrow().items(project, tag, parent)
    }
    fn duplicates_of(&self, id: &str) -> Vec<String> {
        self.borrow().duplicates_of(id)
    }
    fn ticket_statuses(&self) -> Vec<String> {
        self.borrow().ticket_statuses()
    }
    fn find_by_web_link(&self, url: &str) -> Vec<String> {
        self.borrow().find_by_web_link(url)
    }
    fn create_ticket(
        &mut self,
        project: &str,
        title: &str,
        description: &str,
        tags: &[String],
        assignee: Option<&str>,
        parent: Option<&str>,
        priority: Option<u8>,
        status: &str,
    ) -> Result<String, String> {
        self.borrow_mut().create_ticket(
            project,
            title,
            description,
            tags,
            assignee,
            parent,
            priority,
            status,
        )
    }
    fn set_status(&mut self, id: &str, status: &str) -> Result<(), String> {
        LivelaStore::set_status(&mut *self.borrow_mut(), id, status)
    }
    fn set_description(&mut self, id: &str, text: &str) -> Result<(), String> {
        self.borrow_mut().set_description(id, text)
    }
    fn add_tag(&mut self, id: &str, tag: &str) -> Result<(), String> {
        self.borrow_mut().add_tag(id, tag)
    }
    fn remove_tag(&mut self, id: &str, tag: &str) -> Result<(), String> {
        self.borrow_mut().remove_tag(id, tag)
    }
    fn assign(&mut self, id: &str, participant: Option<&str>) -> Result<(), String> {
        LivelaStore::assign(&mut *self.borrow_mut(), id, participant)
    }
    fn comment(&mut self, id: &str, author: &str, text: &str) -> Result<String, String> {
        LivelaStore::comment(&mut *self.borrow_mut(), id, author, text)
    }
    fn add_relation(&mut self, id: &str, kind: LivelaRelation, target: &str) -> Result<(), String> {
        self.borrow_mut().add_relation(id, kind, target)
    }
    fn remove_relation(
        &mut self,
        id: &str,
        kind: LivelaRelation,
        target: &str,
    ) -> Result<(), String> {
        self.borrow_mut().remove_relation(id, kind, target)
    }
    fn add_web_link(&mut self, id: &str, url: &str, title: &str) -> Result<(), String> {
        self.borrow_mut().add_web_link(id, url, title)
    }
    fn set_pr_status(&mut self, id: &str, url: &str, code: &str) -> Result<(), String> {
        self.borrow_mut().set_pr_status(id, url, code)
    }
}

/// The adapter of the `TD` project on its own store.
fn td(store: MemStore) -> Livela<MemStore> {
    Livela::new(store, "TD", StatusTable::identity())
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
fn the_livela_adapter_passes_the_conformance_suite_on_two_projects_of_one_store() {
    let shared: Shared = Rc::new(RefCell::new(MemStore::default()));
    let facts = || shared.borrow().facts.len() as u64;
    // The same run id in both: the same label and PR URL, so that the second
    // run's counts (three items by label, one by the PR URL) prove it sees
    // none of the first run's items.
    for prefix in ["TD", "QA"] {
        let t = Livela::new(shared.clone(), prefix, StatusTable::identity());
        let report =
            conformance::run_with(&t, &StatusTable::identity(), "1", Some(&facts)).unwrap();
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
        assert!(
            report.created.iter().all(|k| k.starts_with(prefix)),
            "{prefix}: {:?}",
            report.created
        );
    }
}

#[test]
fn neither_project_sees_the_others_items() {
    let shared: Shared = Rc::new(RefCell::new(MemStore::default()));
    let td = Livela::new(shared.clone(), "TD", StatusTable::identity());
    let qa = Livela::new(shared.clone(), "QA", StatusTable::identity());
    let labelled = |title: &str| NewItem {
        labels: vec!["l".into()],
        ..new_item(title, Status::Ready)
    };
    let a = td.create_item(&labelled("a")).unwrap();
    let child = td
        .create_item(&NewItem {
            parent: Some(a.key.clone()),
            ..labelled("child")
        })
        .unwrap();
    let b = qa.create_item(&labelled("b")).unwrap();
    assert!(a.key.starts_with("TD-") && child.key.starts_with("TD-"));
    assert!(b.key.starts_with("QA-"));
    let url = "https://github.com/o/r/pull/1";
    td.link_pr(&a.key, url, "PR").unwrap();
    qa.link_pr(&b.key, url, "PR").unwrap();

    let keys = |items: Vec<Item>| items.into_iter().map(|i| i.key).collect::<Vec<_>>();
    assert_eq!(
        keys(td.list_by_label("l").unwrap()),
        [a.key.clone(), child.key.clone()]
    );
    assert_eq!(keys(qa.list_by_label("l").unwrap()), vec![b.key.clone()]);
    assert_eq!(
        qa.find_labelled_containing("l", "").unwrap().len(),
        1,
        "the default search goes through list_by_label"
    );
    assert_eq!(td.items_linked_to_pr(url).unwrap(), vec![a.key.clone()]);
    assert_eq!(qa.items_linked_to_pr(url).unwrap(), vec![b.key.clone()]);
    assert!(qa.get(&a.key).unwrap_err().is_not_found());
    assert!(td.get(&b.key).unwrap_err().is_not_found());
    assert!(qa.children(&a.key).unwrap_err().is_not_found());
    assert_eq!(keys(td.children(&a.key).unwrap()), vec![child.key.clone()]);

    // A duplicate laid in the store across projects is not seen from the
    // other one.
    let (a_id, b_id) = (a.id.clone(), b.id.clone());
    shared
        .borrow_mut()
        .add_relation(&b_id, LivelaRelation::DuplicateOf, &a_id)
        .unwrap();
    assert!(td.relations(&a.key).unwrap().duplicates.is_empty());

    // No write reaches the other project's items, and none is recorded.
    let before = shared.borrow().facts.len();
    assert!(
        qa.relate(&b.key, &a.key, RelationKind::Blocks)
            .unwrap_err()
            .is_not_found()
    );
    assert!(qa.comment(&a.key, "x").unwrap_err().is_not_found());
    assert!(
        qa.set_status(&a.key, Status::Done, None)
            .unwrap_err()
            .is_not_found()
    );
    assert!(
        qa.create_item(&NewItem {
            parent: Some(a.key.clone()),
            ..new_item("orphan", Status::Ready)
        })
        .unwrap_err()
        .is_not_found()
    );
    let relation = format!("blocked_by:{a_id}:{b_id}");
    shared
        .borrow_mut()
        .add_relation(&a_id, LivelaRelation::BlockedBy, &b_id)
        .unwrap();
    let before_unrelate = shared.borrow().facts.len();
    assert!(qa.unrelate(&relation).unwrap_err().is_not_found());
    assert_eq!(shared.borrow().facts.len(), before_unrelate);
    assert_eq!(before + 1, before_unrelate, "only the store's own relation");
}

#[test]
fn the_team_key_is_the_projects_prefix_and_its_items_land_in_it() {
    let t = Livela::new(MemStore::default(), "Quality", StatusTable::identity());
    assert_eq!(t.team_key().unwrap(), "QA");
    let a = t.create_item(&new_item("a", Status::Ready)).unwrap();
    let child = t
        .create_item(&NewItem {
            parent: Some(a.key.clone()),
            ..new_item("child", Status::Ready)
        })
        .unwrap();
    let store = t.into_store();
    for id in [&a.id, &child.id] {
        assert_eq!(store.items[id].projects, ["project-qa"]);
    }
}

#[test]
fn a_tracker_project_is_found_by_its_prefix_name_or_id() {
    let t = td(MemStore::default());
    for reference in ["TD", "td", "Delivery", "project-td"] {
        assert_eq!(
            t.project(reference).unwrap().as_deref(),
            Some("Delivery"),
            "{reference}"
        );
    }
    assert!(t.project("ZZ").unwrap_err().is_not_found());
}

#[test]
fn a_project_several_answer_to_is_unreadable() {
    let mut store = MemStore::default();
    store.projects.push(LivelaProject {
        id: "project-x".into(),
        prefix: "XX".into(),
        name: "TD".into(),
    });
    let t = td(store);
    assert!(matches!(
        t.project("TD").unwrap_err(),
        TrackerError::Unreadable { .. }
    ));
    assert!(matches!(
        t.team_key().unwrap_err(),
        TrackerError::Unreadable { .. }
    ));
}

#[test]
fn an_adapter_of_an_unknown_project_is_built_and_writes_nothing() {
    let t = Livela::new(MemStore::default(), "ZZ", StatusTable::identity());
    assert!(t.project("ZZ").unwrap_err().is_not_found());
    let errs = [
        t.identity().map(|_| ()),
        t.team_key().map(|_| ()),
        t.status_names().map(|_| ()),
        t.get("TD-0001").map(|_| ()),
        t.list_by_label("l").map(|_| ()),
        t.items_linked_to_pr("https://github.com/o/r/pull/1")
            .map(|_| ()),
        t.create_item(&new_item("a", Status::Ready)).map(|_| ()),
    ];
    for err in errs {
        let err = err.unwrap_err();
        assert!(matches!(err, TrackerError::Unreadable { .. }), "{err}");
        assert!(err.to_string().contains("Livela project ZZ"), "{err}");
    }
    let store = t.into_store();
    assert!(store.items.is_empty());
    assert!(store.participants.is_empty());
    assert!(store.facts.is_empty());
}

#[test]
fn every_write_is_signed_by_the_bridge_and_the_runner_keeps_its_author() {
    let t = td(MemStore::default());
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
fn an_urgent_item_is_created_at_the_stores_priority_one() {
    let t = td(MemStore::default());
    let urgent = NewItem {
        priority: Some(Priority::Urgent),
        ..new_item("a", Status::Ready)
    };
    let a = t.create_item(&urgent).unwrap();
    let b = t.create_item(&new_item("b", Status::Ready)).unwrap();
    let store = t.into_store();
    assert_eq!(store.priorities[&a.id], Some(1));
    assert_eq!(store.priorities[&b.id], None, "the store's default");
}

#[test]
fn reads_and_unchanged_writes_leave_no_fact() {
    let t = td(MemStore::default());
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
    let t = td(MemStore::default());
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
    let store = MemStore {
        lacks: Some("Ready"),
        ..MemStore::default()
    };
    let t = td(store);
    let err = t.create_item(&new_item("a", Status::Ready)).unwrap_err();
    assert!(matches!(err, TrackerError::Refused { .. }), "{err}");
    assert!(t.store.borrow().items.is_empty());
    assert!(t.store.borrow().facts.is_empty());
}

#[test]
fn the_validation_statuses_are_open_on_livela() {
    let t = td(MemStore::default());
    let a = t.create_item(&new_item("a", Status::PostDeploy)).unwrap();
    assert_eq!(a.state.status, Some(Status::PostDeploy));
    assert!(!a.state.is_resolved());
}

#[test]
fn a_post_deploy_validation_is_created_in_post_deploy_assigned_with_no_label() {
    let t = td(MemStore::default());
    let owner = t.identity().unwrap();
    let item = NewItem {
        title: "Post-deploy validation".into(),
        description: "d".into(),
        labels: vec![],
        status: Status::PostDeploy,
        priority: Some(Priority::Urgent),
        assignee: Some(owner.clone()),
        parent: None,
    };
    let created = t.create_item(&item).unwrap();
    assert_eq!(created.state.status, Some(Status::PostDeploy));
    assert_eq!(created.assignee, Some(owner));
    assert!(created.labels.is_empty(), "{:?}", created.labels);
}

#[test]
fn a_key_resolves_by_its_code_and_never_by_a_shorter_one() {
    let t = td(MemStore::default());
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
    let t = td(MemStore::default());
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
    let t = td(MemStore::default());
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
    let t = td(MemStore::default());
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
    let t = td(MemStore::default());
    assert!(t.get("TD-NOPE").unwrap_err().is_not_found());
    assert!(matches!(
        t.update_comment("c-1", "x").unwrap_err(),
        TrackerError::Refused { .. }
    ));
}

#[test]
fn livela_has_no_open_item_limit_and_archives_nothing() {
    let t = td(MemStore::default());
    let a = t.create_item(&new_item("a", Status::Done)).unwrap();
    let before = t.store.borrow().facts.len();
    assert!(!t.archives(), "the bridge asks Livela for no archive");
    t.archive(&a.key).unwrap();
    assert_eq!(t.store.borrow().facts.len(), before, "no fact recorded");
    assert!(!t.get(&a.key).unwrap().archived);
}

/// A host calls `doctor` with its Livela adapter: the project is reported
/// found, or unknown and invalid with its fix.
#[test]
fn doctor_runs_on_the_livela_adapter_and_names_its_project() {
    use crate::doctor::{self, Verdict};
    use crate::github::RepoSlug;
    use crate::github::fake::FakeGitHub;
    let mut config = crate::config::spec_example::config();
    config.tracker.kind = crate::config::TrackerKind::Livela;
    let github = FakeGitHub::new();
    github.state.borrow_mut().default_branch.insert(
        RepoSlug::parse(&config.product.repository).unwrap(),
        "main".into(),
    );
    let project = |config: &crate::config::Config, reference: &str| {
        let t = Livela::new(MemStore::default(), reference, StatusTable::identity());
        doctor::run_embedded(config, None, &github, &t, "a host", Some("1.4.0"))
            .into_iter()
            .find(|c| c.name == "tracker.project")
            .expect("the project's check")
    };
    config.tracker.project = Some("TD".into());
    let found = project(&config, "TD");
    assert_eq!(found.verdict, Verdict::Ok, "{found}");
    config.tracker.project = Some("NOPE".into());
    let unknown = project(&config, "NOPE");
    assert_eq!(unknown.verdict, Verdict::Invalid, "{unknown}");
    let fix = unknown.fix.expect("its fix");
    assert_eq!(fix.page, "docs/setup/tracker-livela.md");
}

/// A host's gesture with its Livela adapter: the same function the CLI
/// calls.
#[test]
fn a_gesture_runs_on_the_livela_adapter() {
    use crate::github::fake::FakeGitHub;
    use crate::github::{Identity, RepoSlug};
    let mut config = crate::config::spec_example::config();
    config.tracker.kind = crate::config::TrackerKind::Livela;
    let github = FakeGitHub::new();
    let repo = RepoSlug::parse(&config.product.repository).unwrap();
    {
        let mut s = github.state.borrow_mut();
        s.identity = Some(Identity::App {
            slug: "spl-asdlc".into(),
            app_id: "1".into(),
        });
        s.default_branch.insert(repo.clone(), "main".into());
        s.branches.insert(repo.clone(), Vec::new());
        s.tags.insert(repo, Vec::new());
    }
    let t = td(MemStore::default());
    let p = crate::embed::Project::new(&config, &github, &t, None, &|_| None).unwrap();
    let out = p.cut(true, "a person");
    assert!(out.done, "{out:?}");
    let status = p.release_status().unwrap_err();
    assert!(status.contains("no release candidate"), "{status}");
}

/// The GitHub sync on the Livela adapter: a PR that names its item by its
/// address is linked, its status recorded, each write signed by the bridge,
/// and a second pass records nothing.
#[test]
fn the_github_sync_links_and_records_on_livela_signed_by_the_bridge() {
    use crate::github::fake::{FakeGitHub, pull_request};
    use crate::github::{Identity, RepoSlug};
    let mut config = crate::config::spec_example::config();
    config.tracker.kind = crate::config::TrackerKind::Livela;
    config.tracker.team_keys.clear();
    let t = td(MemStore::default());
    let a = t.create_item(&new_item("a", Status::InProgress)).unwrap();
    let b = t.create_item(&new_item("b", Status::InProgress)).unwrap();
    let github = FakeGitHub::new();
    let repo = RepoSlug::parse(&config.product.repository).unwrap();
    let by_address = crate::github::PullRequest {
        body: format!("livela://item/{}", a.key),
        updated_at: crate::github::Timestamp::parse("2026-10-25T10:00:00Z"),
        ..pull_request(1)
    };
    let by_title = crate::github::PullRequest {
        title: format!("feat: x ({})", b.key),
        draft: true,
        updated_at: crate::github::Timestamp::parse("2026-10-25T10:01:00Z"),
        ..pull_request(2)
    };
    {
        let mut s = github.state.borrow_mut();
        s.identity = Some(Identity::App {
            slug: "spl-asdlc".into(),
            app_id: "1".into(),
        });
        s.open_pulls
            .insert(repo.clone(), Ok(vec![by_address.clone(), by_title.clone()]));
        s.updated_pulls
            .insert(repo.clone(), Ok(vec![by_address.clone(), by_title.clone()]));
        s.merged_pulls.insert(repo.clone(), Ok(Vec::new()));
        s.closed_pulls.insert(repo, Ok(Vec::new()));
    }
    let dir = std::env::temp_dir().join(format!("spl-asdlc-livela-sync-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let host = crate::job::Host {
        config_path: std::path::Path::new(crate::config::DEFAULT_PATH),
        tracker: &t,
        budget: None,
        log_path: Some(dir.join("acme.jsonl")),
        env: &|_| None,
    };
    let before = t.store.borrow().facts.len();
    let out = crate::sync::pass_with(&host, &config, &github);
    assert!(out.ok, "{out:?}");
    assert_eq!(
        t.pr_status(&a.key, &by_address.url).unwrap(),
        Some(PrStatus::Open(Checks::None))
    );
    assert_eq!(
        t.pr_status(&b.key, &by_title.url).unwrap(),
        Some(PrStatus::Draft)
    );
    let after = t.store.borrow().facts.len();
    assert_eq!(after - before, 4, "two links, two statuses");
    let again = crate::sync::pass_with(&host, &config, &github);
    assert!(again.ok, "{again:?}");
    assert_eq!(t.store.borrow().facts.len(), after, "no heartbeat");
    let me = t.identity().unwrap().id;
    let store = t.into_store();
    assert_eq!(store.author, "runner", "the previous author is restored");
    assert!(
        store.facts[before..].iter().all(|(by, _)| *by == me),
        "{:?}",
        &store.facts[before..]
    );
}

/// Livela tickets have no Triage since triage became an item type of its
/// own: the canonical Triage is their Backlog, on two projects of one store.
#[test]
fn a_ticket_type_without_triage_takes_it_as_backlog_and_passes_the_suite() {
    let shared: Shared = Rc::new(RefCell::new(MemStore {
        lacks: Some("Triage"),
        ..MemStore::default()
    }));
    let facts = || shared.borrow().facts.len() as u64;
    for prefix in ["TD", "QA"] {
        let t = Livela::new(shared.clone(), prefix, StatusTable::identity());
        assert!(t.status_names().unwrap().contains(&"Triage".to_string()));
        conformance::run_with(&t, &StatusTable::identity(), "1", Some(&facts)).unwrap();
    }
    let t = Livela::new(shared.clone(), "TD", StatusTable::identity());
    // A non-blocking finding is created in Triage, a requalified one moved
    // there: both land in Backlog, and read as Backlog.
    let finding = t.create_item(&new_item("finding", Status::Triage)).unwrap();
    assert_eq!(finding.state.status, Some(Status::Backlog));
    let fix = t.create_item(&new_item("fix", Status::Ready)).unwrap();
    t.set_status(&fix.key, Status::Triage, Some(None)).unwrap();
    assert_eq!(t.get(&fix.key).unwrap().state.status, Some(Status::Backlog));
    let before = shared.borrow().facts.len();
    t.set_status(&fix.key, Status::Triage, Some(None)).unwrap();
    assert_eq!(shared.borrow().facts.len(), before, "unchanged: no fact");
}

/// A ticket type that still has a Triage status keeps it.
#[test]
fn a_ticket_type_with_triage_keeps_it() {
    let t = td(MemStore::default());
    let a = t.create_item(&new_item("a", Status::Triage)).unwrap();
    assert_eq!(a.state.status, Some(Status::Triage));
    let names = t.status_names().unwrap();
    assert_eq!(names.iter().filter(|n| *n == "Triage").count(), 1);
}
