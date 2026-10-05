//! The Livela adapter on an in-memory store that keeps the contract's
//! rules: ticket statuses by canonical name, relations held by one side,
//! web links, participants. The glue for the real store is verified against
//! the Livela core when the runner links it (step 4b).

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

#[derive(Default)]
struct MemStore {
    items: BTreeMap<String, LivelaItem>,
    participants: BTreeMap<String, String>,
    next: u64,
}

impl MemStore {
    fn id(&mut self) -> String {
        self.next += 1;
        format!("{:08}", self.next)
    }
}

impl LivelaStore for MemStore {
    fn bridge_participant(&mut self) -> Result<String, String> {
        if let Some((id, _)) = self.participants.iter().find(|(_, n)| *n == PARTICIPANT) {
            return Ok(id.clone());
        }
        let id = format!("p-{}", self.id());
        self.participants.insert(id.clone(), PARTICIPANT.into());
        Ok(id)
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
            .find(|i| i.short_id == short_id)
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
        _urgent: bool,
    ) -> Result<String, String> {
        let id = self.id();
        let item = LivelaItem {
            id: id.clone(),
            short_id: format!("TD-{}", &id[4..]),
            title: title.into(),
            description: description.into(),
            status: "Triage".into(),
            tags: tags.to_vec(),
            assignee: assignee.map(str::to_string),
            parent: parent.map(str::to_string),
            ..LivelaItem::default()
        };
        self.items.insert(id.clone(), item);
        Ok(id)
    }
    fn set_status(&mut self, id: &str, status: &str) -> Result<(), String> {
        let canon = STATUSES
            .iter()
            .find(|s| s.eq_ignore_ascii_case(status))
            .ok_or(format!("ticket has no status {status}"))?;
        self.items.get_mut(id).ok_or("no item")?.status = canon.to_string();
        Ok(())
    }
    fn set_description(&mut self, id: &str, text: &str) -> Result<(), String> {
        self.items.get_mut(id).ok_or("no item")?.description = text.into();
        Ok(())
    }
    fn add_tag(&mut self, id: &str, tag: &str) -> Result<(), String> {
        let i = self.items.get_mut(id).ok_or("no item")?;
        if !i.tags.iter().any(|t| t == tag) {
            i.tags.push(tag.into());
        }
        Ok(())
    }
    fn remove_tag(&mut self, id: &str, tag: &str) -> Result<(), String> {
        self.items
            .get_mut(id)
            .ok_or("no item")?
            .tags
            .retain(|t| t != tag);
        Ok(())
    }
    fn assign(&mut self, id: &str, participant: Option<&str>) -> Result<(), String> {
        self.items.get_mut(id).ok_or("no item")?.assignee = participant.map(str::to_string);
        Ok(())
    }
    fn comment(&mut self, id: &str, author: &str, text: &str) -> Result<String, String> {
        let cid = format!("c-{}", self.id());
        self.items.get_mut(id).ok_or("no item")?.comments.push((
            cid.clone(),
            author.into(),
            text.into(),
        ));
        Ok(cid)
    }
    fn add_relation(&mut self, id: &str, kind: LivelaRelation, target: &str) -> Result<(), String> {
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
        let i = self.items.get_mut(id).ok_or("no item")?;
        match kind {
            LivelaRelation::BlockedBy => i.blocked_by.retain(|x| x != target),
            LivelaRelation::RelatedTo => i.related_to.retain(|x| x != target),
            LivelaRelation::DuplicateOf => i.duplicate_of = None,
        }
        Ok(())
    }
    fn add_web_link(&mut self, id: &str, url: &str, _title: &str) -> Result<(), String> {
        self.items
            .get_mut(id)
            .ok_or("no item")?
            .web_links
            .push(url.into());
        Ok(())
    }
}

#[test]
fn the_livela_adapter_passes_the_conformance_suite_without_comment_edits() {
    let t = Livela::new(MemStore::default(), StatusTable::identity());
    let report = conformance::run(&t, &StatusTable::identity(), "1").unwrap();
    assert_eq!(
        report.not_offered,
        vec!["comment edit"],
        "the one missing point of the contract"
    );
}

#[test]
fn every_write_is_signed_by_the_spl_asdlc_participant() {
    let t = Livela::new(MemStore::default(), StatusTable::identity());
    let item = t
        .create_item(&NewItem {
            title: "Release acme v1.2.3".into(),
            description: "d".into(),
            labels: vec!["release/acme-v1.2.3".into()],
            status: Status::Ready,
            priority: Some(1),
            assignee: None,
            parent: None,
        })
        .unwrap();
    assert_eq!(item.state.status, Some(Status::Ready));
    let c = t.comment(&item.key, "hello").unwrap();
    assert!(c.by_bridge);
    assert_eq!(t.identity().unwrap().name, PARTICIPANT);
    assert_eq!(
        t.comments(&item.key).unwrap()[0]
            .author
            .as_ref()
            .unwrap()
            .name,
        PARTICIPANT
    );
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
