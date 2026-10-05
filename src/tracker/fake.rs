//! An in-memory tracker for tests: items, relations, comments and links held
//! in memory, every write recorded, failures injected by write prefix. Like
//! the real adapters, a write of an unchanged value records nothing.

use std::cell::RefCell;
use std::collections::BTreeMap;

use super::*;
use crate::method::vocabulary::StateKind;

#[derive(Debug, Clone)]
struct Stored {
    item: Item,
    comments: Vec<Comment>,
    links: Vec<String>,
}

#[derive(Default)]
pub struct FakeState {
    items: BTreeMap<String, Stored>,
    relations: Vec<(String, String, String, RelationKind)>,
    labels: Vec<String>,
    pub users: Vec<User>,
    pub writes: Vec<String>,
    pub failing_writes: Vec<String>,
    pub unreadable: Vec<String>,
    next: u64,
}

pub struct FakeTracker {
    pub team: String,
    pub bridge: User,
    pub statuses: StatusTable,
    pub state: RefCell<FakeState>,
}

impl Default for FakeTracker {
    fn default() -> Self {
        FakeTracker::new("ABC")
    }
}

impl FakeTracker {
    pub fn new(team: &str) -> FakeTracker {
        FakeTracker {
            team: team.to_string(),
            bridge: User {
                id: "u-bridge".into(),
                name: "spl-asdlc".into(),
            },
            statuses: StatusTable::identity(),
            state: RefCell::new(FakeState::default()),
        }
    }

    /// Put an item in the tracker as if a person had created it.
    pub fn seed(&self, key: &str, title: &str, description: &str, status: Status, labels: &[&str]) {
        let item = Item {
            id: format!("id-{key}"),
            key: key.to_string(),
            title: title.to_string(),
            description: description.to_string(),
            url: format!("https://tracker.test/{key}"),
            state: ItemState::of(status),
            labels: labels.iter().map(|s| s.to_string()).collect(),
            assignee: None,
            parent: None,
            archived: false,
        };
        self.state.borrow_mut().items.insert(
            key.to_string(),
            Stored {
                item,
                comments: Vec::new(),
                links: Vec::new(),
            },
        );
    }

    pub fn item(&self, key: &str) -> Item {
        self.state.borrow().items[key].item.clone()
    }

    pub fn writes(&self) -> Vec<String> {
        self.state.borrow().writes.clone()
    }

    pub fn fail_writes_starting_with(&self, prefix: &str) {
        self.state
            .borrow_mut()
            .failing_writes
            .push(prefix.to_string());
    }

    /// Reads of this key answer `Unreadable`.
    pub fn make_unreadable(&self, key: &str) {
        self.state.borrow_mut().unreadable.push(key.to_string());
    }

    /// A comment from someone other than the bridge.
    pub fn human_comment(&self, key: &str, author: &str, body: &str) {
        let mut s = self.state.borrow_mut();
        s.next += 1;
        let id = format!("c{}", s.next);
        if let Some(st) = s.items.get_mut(key) {
            st.comments.push(Comment {
                id,
                author: Some(User {
                    id: format!("u-{author}"),
                    name: author.to_string(),
                }),
                by_bridge: false,
                body: body.to_string(),
            });
        }
    }

    pub fn link(&self, key: &str, url: &str) {
        if let Some(st) = self.state.borrow_mut().items.get_mut(key) {
            st.links.push(url.to_string());
        }
    }

    fn record(&self, line: String) -> TResult<()> {
        let mut s = self.state.borrow_mut();
        if s.failing_writes
            .iter()
            .any(|p| line.starts_with(p.as_str()))
        {
            s.writes.push(format!("REFUSED {line}"));
            return Err(TrackerError::refused(line, "injected failure"));
        }
        s.writes.push(line);
        Ok(())
    }

    fn read(&self, key: &str) -> TResult<Stored> {
        let s = self.state.borrow();
        if s.unreadable.iter().any(|k| k == key) {
            return Err(TrackerError::unreadable(key, "injected outage"));
        }
        s.items
            .get(key)
            .cloned()
            .ok_or_else(|| TrackerError::NotFound {
                what: key.to_string(),
            })
    }

    fn with<T>(&self, key: &str, f: impl FnOnce(&mut Stored) -> T) -> TResult<T> {
        let mut s = self.state.borrow_mut();
        let st = s.items.get_mut(key).ok_or_else(|| TrackerError::NotFound {
            what: key.to_string(),
        })?;
        Ok(f(st))
    }
}

impl Tracker for FakeTracker {
    fn identity(&self) -> TResult<User> {
        Ok(self.bridge.clone())
    }

    fn team_key(&self) -> TResult<String> {
        Ok(self.team.clone())
    }

    fn status_names(&self) -> TResult<Vec<String>> {
        Ok(Status::ALL
            .iter()
            .map(|s| self.statuses.tracker_name(*s).to_string())
            .collect())
    }

    fn user(&self, reference: &str) -> TResult<User> {
        if reference == self.bridge.name || reference == self.bridge.id {
            return Ok(self.bridge.clone());
        }
        self.state
            .borrow()
            .users
            .iter()
            .find(|u| u.id == reference || u.name.eq_ignore_ascii_case(reference))
            .cloned()
            .ok_or_else(|| TrackerError::NotFound {
                what: format!("user {reference}"),
            })
    }

    fn get(&self, key: &str) -> TResult<Item> {
        self.read(key).map(|s| s.item)
    }

    fn relations(&self, key: &str) -> TResult<Relations> {
        self.read(key)?;
        let s = self.state.borrow();
        let mut out = Relations::default();
        for (id, a, b, kind) in &s.relations {
            let rel = |other: &str| Relation {
                id: id.clone(),
                other: other.to_string(),
            };
            match kind {
                RelationKind::Blocks if a == key => out.blocks.push(rel(b)),
                RelationKind::Blocks if b == key => out.blocked_by.push(rel(a)),
                RelationKind::Related if a == key => out.related.push(rel(b)),
                RelationKind::Related if b == key => out.related.push(rel(a)),
                RelationKind::Duplicate if a == key => out.duplicates.push(rel(b)),
                RelationKind::Duplicate if b == key => out.duplicates.push(rel(a)),
                _ => {}
            }
        }
        Ok(out)
    }

    fn comments(&self, key: &str) -> TResult<Vec<Comment>> {
        self.read(key).map(|s| s.comments)
    }

    fn children(&self, key: &str) -> TResult<Vec<Item>> {
        self.read(key)?;
        Ok(self
            .state
            .borrow()
            .items
            .values()
            .filter(|s| s.item.parent.as_deref() == Some(key))
            .map(|s| s.item.clone())
            .collect())
    }

    fn list_by_label(&self, label: &str) -> TResult<Vec<Item>> {
        Ok(self
            .state
            .borrow()
            .items
            .values()
            .filter(|s| s.item.labels.iter().any(|l| l == label))
            .map(|s| s.item.clone())
            .collect())
    }

    fn items_linked_to_pr(&self, pr_url: &str) -> TResult<Vec<String>> {
        Ok(self
            .state
            .borrow()
            .items
            .values()
            .filter(|s| s.links.iter().any(|l| l == pr_url))
            .map(|s| s.item.key.clone())
            .collect())
    }

    fn ensure_label(&self, name: &str) -> TResult<()> {
        if self.state.borrow().labels.iter().any(|l| l == name) {
            return Ok(());
        }
        self.record(format!("ensure-label {name}"))?;
        self.state.borrow_mut().labels.push(name.to_string());
        Ok(())
    }

    fn create_item(&self, new: &NewItem) -> TResult<Item> {
        self.record(format!("create {} [{}]", new.title, new.status))?;
        let key = {
            let mut s = self.state.borrow_mut();
            s.next += 1;
            format!("{}-{}", self.team, 1000 + s.next)
        };
        self.seed(&key, &new.title, &new.description, new.status, &[]);
        self.with(&key, |st| {
            st.item.labels = new.labels.clone();
            st.item.assignee = new.assignee.clone();
            st.item.parent = new.parent.clone();
        })?;
        Ok(self.item(&key))
    }

    fn update_description(&self, key: &str, description: &str) -> TResult<()> {
        if self.read(key)?.item.description == description {
            return Ok(());
        }
        self.record(format!("describe {key}"))?;
        self.with(key, |st| st.item.description = description.to_string())
    }

    fn add_label(&self, key: &str, label: &str) -> TResult<()> {
        if self.read(key)?.item.labels.iter().any(|l| l == label) {
            return Ok(());
        }
        self.record(format!("label {key} +{label}"))?;
        self.with(key, |st| {
            if !st.item.labels.iter().any(|l| l == label) {
                st.item.labels.push(label.to_string());
            }
        })
    }

    fn remove_label(&self, key: &str, label: &str) -> TResult<()> {
        if !self.read(key)?.item.labels.iter().any(|l| l == label) {
            return Ok(());
        }
        self.record(format!("label {key} -{label}"))?;
        self.with(key, |st| st.item.labels.retain(|l| l != label))
    }

    fn set_status(
        &self,
        key: &str,
        status: Status,
        assignee: Option<Option<&User>>,
    ) -> TResult<()> {
        let current = self.read(key)?.item;
        let assignee =
            assignee.filter(|a| a.map(|u| &u.id) != current.assignee.as_ref().map(|u| &u.id));
        if current.state.status == Some(status) && assignee.is_none() {
            return Ok(());
        }
        let who = match assignee {
            None => String::new(),
            Some(None) => " (unassigned)".into(),
            Some(Some(u)) => format!(" (assigned {})", u.name),
        };
        self.record(format!("status {key} {status}{who}"))?;
        self.with(key, |st| {
            st.item.state = ItemState {
                name: self.statuses.tracker_name(status).to_string(),
                status: Some(status),
                kind: StateKind::of(status),
            };
            if let Some(a) = assignee {
                st.item.assignee = a.cloned();
            }
        })
    }

    fn assign(&self, key: &str, user: Option<&User>) -> TResult<()> {
        if self.read(key)?.item.assignee.as_ref().map(|u| &u.id) == user.map(|u| &u.id) {
            return Ok(());
        }
        self.record(format!(
            "assign {key} {}",
            user.map(|u| u.name.as_str()).unwrap_or("(nobody)")
        ))?;
        self.with(key, |st| st.item.assignee = user.cloned())
    }

    fn comment(&self, key: &str, body: &str) -> TResult<Comment> {
        self.read(key)?;
        self.record(format!(
            "comment {key} {}",
            body.lines().next().unwrap_or("")
        ))?;
        let id = {
            let mut s = self.state.borrow_mut();
            s.next += 1;
            format!("c{}", s.next)
        };
        let c = Comment {
            id,
            author: Some(self.bridge.clone()),
            by_bridge: true,
            body: body.to_string(),
        };
        self.with(key, |st| st.comments.push(c.clone()))?;
        Ok(c)
    }

    fn update_comment(&self, comment_id: &str, body: &str) -> TResult<()> {
        self.record(format!("edit-comment {comment_id}"))?;
        let mut s = self.state.borrow_mut();
        for st in s.items.values_mut() {
            if let Some(c) = st.comments.iter_mut().find(|c| c.id == comment_id) {
                c.body = body.to_string();
                return Ok(());
            }
        }
        Err(TrackerError::NotFound {
            what: format!("comment {comment_id}"),
        })
    }

    fn relate(&self, a: &str, b: &str, kind: RelationKind) -> TResult<()> {
        self.read(a)?;
        self.read(b)?;
        let exists = self.state.borrow().relations.iter().any(|(_, x, y, k)| {
            *k == kind && ((x == a && y == b) || (kind != RelationKind::Blocks && x == b && y == a))
        });
        if exists {
            return Ok(());
        }
        self.record(format!("relate {a} {kind:?} {b}"))?;
        let mut s = self.state.borrow_mut();
        s.next += 1;
        let id = format!("r{}", s.next);
        s.relations.push((id, a.to_string(), b.to_string(), kind));
        Ok(())
    }

    fn unrelate(&self, relation_id: &str) -> TResult<()> {
        if relation_id.is_empty() {
            return Err(TrackerError::refused("unrelate", "no relation id"));
        }
        if !self
            .state
            .borrow()
            .relations
            .iter()
            .any(|(id, ..)| id == relation_id)
        {
            return Err(TrackerError::NotFound {
                what: format!("relation {relation_id}"),
            });
        }
        self.record(format!("unrelate {relation_id}"))?;
        self.state
            .borrow_mut()
            .relations
            .retain(|(id, ..)| id != relation_id);
        Ok(())
    }

    fn link_pr(&self, key: &str, pr_url: &str, _title: &str) -> TResult<()> {
        if self.read(key)?.links.iter().any(|l| l == pr_url) {
            return Ok(());
        }
        self.record(format!("link {key} {pr_url}"))?;
        self.with(key, |st| {
            if !st.links.iter().any(|l| l == pr_url) {
                st.links.push(pr_url.to_string());
            }
        })
    }
}
