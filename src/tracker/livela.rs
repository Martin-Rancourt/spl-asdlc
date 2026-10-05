//! The Livela adapter: in process, on the Livela core's `Store`
//! (`docs/livela-contract.md`, `docs/setup/tracker-livela.md`).
//!
//! The adapter is written against [`LivelaStore`], the slice of the store
//! the contract promises. The Livela runner implements it for its `Store`
//! (a few lines of glue, step 4b), so this crate needs no dependency on the
//! Livela core and the core needs none on this crate's internals.
//!
//! Mapping: an item's key is its short id; labels are tags; the bridge
//! writes as the `spl-asdlc` agent participant; a relation has no id of its
//! own, so its id is `<kind>:<item>:<target>`.
//!
//! Missing from the contract (said, never guessed around): editing a
//! comment. `update_comment` is refused; the caller posts its fact anew.

use std::cell::RefCell;

use super::*;
use crate::method::vocabulary::StateKind;

/// The name of the bridge's agent participant (contract, item 4).
pub const PARTICIPANT: &str = "spl-asdlc";

/// An item as the Livela core describes it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LivelaItem {
    pub id: String,
    pub short_id: String,
    pub title: String,
    pub description: String,
    /// The status label (`To Deploy`).
    pub status: String,
    pub tags: Vec<String>,
    pub assignee: Option<String>,
    pub parent: Option<String>,
    pub blocked_by: Vec<String>,
    pub blocks: Vec<String>,
    pub related_to: Vec<String>,
    pub duplicate_of: Option<String>,
    /// `(comment id, author participant id, text)`, oldest first.
    pub comments: Vec<(String, String, String)>,
    pub web_links: Vec<String>,
    pub deleted: bool,
}

/// The relation kinds the Livela core stores (from the item that holds them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LivelaRelation {
    BlockedBy,
    RelatedTo,
    DuplicateOf,
}

/// The slice of the Livela `Store` the adapter uses: exactly the contract.
pub trait LivelaStore {
    /// The `spl-asdlc` agent participant, created if absent; every later
    /// write of the store is signed by it.
    fn bridge_participant(&mut self) -> Result<String, String>;
    /// Participants answering to a name: `(id, name)`.
    fn participants_named(&self, name: &str) -> Vec<(String, String)>;
    fn participant_name(&self, id: &str) -> Option<String>;
    /// The internal id of a short id; `Ok(None)` when no item has it, an
    /// error when several do.
    fn resolve(&self, short_id: &str) -> Result<Option<String>, String>;
    fn item(&self, id: &str) -> Result<LivelaItem, String>;
    /// Items with this tag, or children of this parent.
    fn items(&self, tag: Option<&str>, parent: Option<&str>) -> Vec<LivelaItem>;
    /// Items whose `duplicate_of` is this one.
    fn duplicates_of(&self, id: &str) -> Vec<String>;
    /// The human prefix of the short ids.
    fn prefix(&self) -> String;
    /// Status labels of the `ticket` type.
    fn ticket_statuses(&self) -> Vec<String>;
    fn find_by_web_link(&self, url: &str) -> Vec<String>;
    /// A new `ticket`; returns its id.
    fn create_ticket(
        &mut self,
        title: &str,
        description: &str,
        tags: &[String],
        assignee: Option<&str>,
        parent: Option<&str>,
        urgent: bool,
    ) -> Result<String, String>;
    /// Status by canonical name; an error when the type has no such status.
    fn set_status(&mut self, id: &str, status: &str) -> Result<(), String>;
    fn set_description(&mut self, id: &str, text: &str) -> Result<(), String>;
    fn add_tag(&mut self, id: &str, tag: &str) -> Result<(), String>;
    fn remove_tag(&mut self, id: &str, tag: &str) -> Result<(), String>;
    fn assign(&mut self, id: &str, participant: Option<&str>) -> Result<(), String>;
    fn comment(&mut self, id: &str, author: &str, text: &str) -> Result<String, String>;
    fn add_relation(&mut self, id: &str, kind: LivelaRelation, target: &str) -> Result<(), String>;
    fn remove_relation(
        &mut self,
        id: &str,
        kind: LivelaRelation,
        target: &str,
    ) -> Result<(), String>;
    fn add_web_link(&mut self, id: &str, url: &str, title: &str) -> Result<(), String>;
}

pub struct Livela<S: LivelaStore> {
    store: RefCell<S>,
    statuses: StatusTable,
    bridge: RefCell<Option<String>>,
}

impl<S: LivelaStore> Livela<S> {
    pub fn new(store: S, statuses: StatusTable) -> Livela<S> {
        Livela {
            store: RefCell::new(store),
            statuses,
            bridge: RefCell::new(None),
        }
    }

    pub fn into_store(self) -> S {
        self.store.into_inner()
    }

    fn me(&self) -> TResult<String> {
        if let Some(id) = self.bridge.borrow().clone() {
            return Ok(id);
        }
        let id = self
            .store
            .borrow_mut()
            .bridge_participant()
            .map_err(|e| TrackerError::refused("the spl-asdlc participant", e))?;
        *self.bridge.borrow_mut() = Some(id.clone());
        Ok(id)
    }

    fn id_of(&self, key: &str) -> TResult<String> {
        match self.store.borrow().resolve(key) {
            Ok(Some(id)) => Ok(id),
            Ok(None) => Err(TrackerError::NotFound {
                what: key.to_string(),
            }),
            Err(e) => Err(TrackerError::unreadable(key, e)),
        }
    }

    fn raw(&self, key: &str) -> TResult<LivelaItem> {
        let id = self.id_of(key)?;
        self.store
            .borrow()
            .item(&id)
            .map_err(|e| TrackerError::unreadable(key, e))
    }

    fn short_of(&self, id: &str) -> String {
        self.store
            .borrow()
            .item(id)
            .map(|i| i.short_id)
            .unwrap_or_else(|_| id.to_string())
    }

    fn user_of(&self, participant: &str) -> User {
        User {
            id: participant.to_string(),
            name: self
                .store
                .borrow()
                .participant_name(participant)
                .unwrap_or_else(|| participant.to_string()),
        }
    }

    fn item_of(&self, i: &LivelaItem) -> Item {
        let status = self.statuses.canonical(&i.status);
        Item {
            id: i.id.clone(),
            key: i.short_id.clone(),
            title: i.title.clone(),
            description: i.description.clone(),
            url: format!("livela://item/{}", i.short_id),
            state: ItemState {
                name: i.status.clone(),
                status,
                kind: status.map(StateKind::of).unwrap_or(StateKind::Unknown),
            },
            labels: i.tags.clone(),
            assignee: i.assignee.as_deref().map(|a| self.user_of(a)),
            parent: i.parent.as_deref().map(|p| self.short_of(p)),
            archived: i.deleted,
        }
    }

    fn refused(what: impl Into<String>) -> impl FnOnce(String) -> TrackerError {
        let what = what.into();
        move |e| TrackerError::refused(what, e)
    }
}

impl<S: LivelaStore> Tracker for Livela<S> {
    fn identity(&self) -> TResult<User> {
        let id = self.me()?;
        Ok(self.user_of(&id))
    }

    fn team_key(&self) -> TResult<String> {
        Ok(self.store.borrow().prefix())
    }

    fn status_names(&self) -> TResult<Vec<String>> {
        Ok(self.store.borrow().ticket_statuses())
    }

    fn user(&self, reference: &str) -> TResult<User> {
        if let Some(name) = self.store.borrow().participant_name(reference) {
            return Ok(User {
                id: reference.to_string(),
                name,
            });
        }
        let found = self.store.borrow().participants_named(reference);
        match found.as_slice() {
            [(id, name)] => Ok(User {
                id: id.clone(),
                name: name.clone(),
            }),
            [] => Err(TrackerError::NotFound {
                what: format!("participant {reference}"),
            }),
            _ => Err(TrackerError::unreadable(
                format!("participant {reference}"),
                "several participants answer to this name",
            )),
        }
    }

    fn get(&self, key: &str) -> TResult<Item> {
        let raw = self.raw(key)?;
        Ok(self.item_of(&raw))
    }

    fn relations(&self, key: &str) -> TResult<Relations> {
        let raw = self.raw(key)?;
        let rel = |kind: &str, holder: &str, target: &str, other: &str| Relation {
            id: format!("{kind}:{holder}:{target}"),
            other: self.short_of(other),
        };
        let mut out = Relations::default();
        for b in &raw.blocked_by {
            out.blocked_by.push(rel("blocked_by", &raw.id, b, b));
        }
        for b in &raw.blocks {
            // Held by the blocked item as its `blocked_by`.
            out.blocks.push(rel("blocked_by", b, &raw.id, b));
        }
        for r in &raw.related_to {
            out.related.push(rel("related_to", &raw.id, r, r));
        }
        if let Some(d) = &raw.duplicate_of {
            out.duplicates.push(rel("duplicate_of", &raw.id, d, d));
        }
        for d in self.store.borrow().duplicates_of(&raw.id) {
            out.duplicates.push(rel("duplicate_of", &d, &raw.id, &d));
        }
        Ok(out)
    }

    fn comments(&self, key: &str) -> TResult<Vec<Comment>> {
        let me = self.me()?;
        let raw = self.raw(key)?;
        Ok(raw
            .comments
            .iter()
            .map(|(id, author, text)| Comment {
                id: id.clone(),
                author: Some(self.user_of(author)),
                by_bridge: *author == me,
                body: text.clone(),
            })
            .collect())
    }

    fn children(&self, key: &str) -> TResult<Vec<Item>> {
        let id = self.id_of(key)?;
        let items = self.store.borrow().items(None, Some(&id));
        Ok(items.iter().map(|i| self.item_of(i)).collect())
    }

    fn list_by_label(&self, label: &str) -> TResult<Vec<Item>> {
        let items = self.store.borrow().items(Some(label), None);
        Ok(items.iter().map(|i| self.item_of(i)).collect())
    }

    fn items_linked_to_pr(&self, pr_url: &str) -> TResult<Vec<String>> {
        let ids = self.store.borrow().find_by_web_link(pr_url);
        Ok(ids.iter().map(|id| self.short_of(id)).collect())
    }

    fn ensure_label(&self, _name: &str) -> TResult<()> {
        // Tags exist by being used.
        Ok(())
    }

    fn create_item(&self, new: &NewItem) -> TResult<Item> {
        self.me()?;
        let parent = match &new.parent {
            Some(p) => Some(self.id_of(p)?),
            None => None,
        };
        let id = self
            .store
            .borrow_mut()
            .create_ticket(
                &new.title,
                &new.description,
                &new.labels,
                new.assignee.as_ref().map(|u| u.id.as_str()),
                parent.as_deref(),
                new.priority == Some(1),
            )
            .map_err(Self::refused(format!("creation of \"{}\"", new.title)))?;
        self.store
            .borrow_mut()
            .set_status(&id, self.statuses.tracker_name(new.status))
            .map_err(Self::refused(format!("status of \"{}\"", new.title)))?;
        let raw = self
            .store
            .borrow()
            .item(&id)
            .map_err(|e| TrackerError::unreadable(&new.title, e))?;
        Ok(self.item_of(&raw))
    }

    fn update_description(&self, key: &str, description: &str) -> TResult<()> {
        self.me()?;
        let id = self.id_of(key)?;
        self.store
            .borrow_mut()
            .set_description(&id, description)
            .map_err(Self::refused(format!("description of {key}")))
    }

    fn add_label(&self, key: &str, label: &str) -> TResult<()> {
        self.me()?;
        let id = self.id_of(key)?;
        self.store
            .borrow_mut()
            .add_tag(&id, label)
            .map_err(Self::refused(format!("tag {label} on {key}")))
    }

    fn remove_label(&self, key: &str, label: &str) -> TResult<()> {
        self.me()?;
        let id = self.id_of(key)?;
        self.store
            .borrow_mut()
            .remove_tag(&id, label)
            .map_err(Self::refused(format!("tag {label} off {key}")))
    }

    fn set_status(
        &self,
        key: &str,
        status: Status,
        assignee: Option<Option<&User>>,
    ) -> TResult<()> {
        self.me()?;
        let id = self.id_of(key)?;
        self.store
            .borrow_mut()
            .set_status(&id, self.statuses.tracker_name(status))
            .map_err(Self::refused(format!("status {status} of {key}")))?;
        if let Some(a) = assignee {
            self.store
                .borrow_mut()
                .assign(&id, a.map(|u| u.id.as_str()))
                .map_err(Self::refused(format!("assignee of {key}")))?;
        }
        Ok(())
    }

    fn assign(&self, key: &str, user: Option<&User>) -> TResult<()> {
        self.me()?;
        let id = self.id_of(key)?;
        self.store
            .borrow_mut()
            .assign(&id, user.map(|u| u.id.as_str()))
            .map_err(Self::refused(format!("assignee of {key}")))
    }

    fn comment(&self, key: &str, body: &str) -> TResult<Comment> {
        let me = self.me()?;
        let id = self.id_of(key)?;
        let cid = self
            .store
            .borrow_mut()
            .comment(&id, &me, body)
            .map_err(Self::refused(format!("comment on {key}")))?;
        Ok(Comment {
            id: cid,
            author: Some(self.user_of(&me)),
            by_bridge: true,
            body: body.to_string(),
        })
    }

    fn can_edit_comments(&self) -> bool {
        false
    }

    fn update_comment(&self, comment_id: &str, _body: &str) -> TResult<()> {
        Err(TrackerError::refused(
            format!("edit of comment {comment_id}"),
            "the Livela core offers no comment edit (docs/livela-contract.md, missing point)",
        ))
    }

    fn relate(&self, a: &str, b: &str, kind: RelationKind) -> TResult<()> {
        self.me()?;
        let (ia, ib) = (self.id_of(a)?, self.id_of(b)?);
        let what = format!("relation {a} {kind:?} {b}");
        let mut store = self.store.borrow_mut();
        match kind {
            // `a blocks b` is held by `b`: b is blocked by a.
            RelationKind::Blocks => store.add_relation(&ib, LivelaRelation::BlockedBy, &ia),
            RelationKind::Related => store.add_relation(&ia, LivelaRelation::RelatedTo, &ib),
            RelationKind::Duplicate => store.add_relation(&ia, LivelaRelation::DuplicateOf, &ib),
        }
        .map_err(|e| TrackerError::refused(what, e))
    }

    fn unrelate(&self, relation_id: &str) -> TResult<()> {
        self.me()?;
        let mut parts = relation_id.splitn(3, ':');
        let (Some(kind), Some(holder), Some(target)) = (parts.next(), parts.next(), parts.next())
        else {
            return Err(TrackerError::refused(
                "unrelate",
                format!("\"{relation_id}\" is not a relation id"),
            ));
        };
        let kind = match kind {
            "blocked_by" => LivelaRelation::BlockedBy,
            "related_to" => LivelaRelation::RelatedTo,
            "duplicate_of" => LivelaRelation::DuplicateOf,
            other => {
                return Err(TrackerError::refused(
                    "unrelate",
                    format!("unknown relation kind {other}"),
                ));
            }
        };
        self.store
            .borrow_mut()
            .remove_relation(holder, kind, target)
            .map_err(Self::refused(format!("removal of {relation_id}")))
    }

    fn link_pr(&self, key: &str, pr_url: &str, title: &str) -> TResult<()> {
        self.me()?;
        let id = self.id_of(key)?;
        if self.store.borrow().find_by_web_link(pr_url).contains(&id) {
            return Ok(());
        }
        self.store
            .borrow_mut()
            .add_web_link(&id, pr_url, title)
            .map_err(Self::refused(format!("link of {pr_url} on {key}")))
    }
}

#[cfg(test)]
mod tests;
