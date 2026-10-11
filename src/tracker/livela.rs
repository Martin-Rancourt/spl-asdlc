//! The Livela adapter: in process, on the Livela core's `Store`
//! (`docs/livela-contract.md`, `docs/setup/tracker-livela.md`).
//!
//! The adapter is written against [`LivelaStore`], the slice of the store
//! the contract promises. The Livela runner implements it for its `Store`
//! (a few lines of glue), so this crate needs no dependency on the Livela
//! core and the core needs none on this crate's internals.
//!
//! Mapping: an item's key is its short id; labels are tags; the bridge
//! writes as the `spl-asdlc` agent participant; a relation has no id of its
//! own, so its id is `<kind>:<item>:<target>`.
//!
//! No useless facts (contract, "No heartbeat"): every write first reads the
//! value it would change and writes nothing when the value is already there.
//! Every write is signed by the `spl-asdlc` participant: the adapter sets the
//! store's author for the write and restores the previous author after it,
//! so the runner's other writers keep their own signature.
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
    /// The `spl-asdlc` agent participant, created if absent.
    fn bridge_participant(&mut self) -> Result<String, String>;
    /// The `spl-asdlc` agent participant if it exists (reads create nothing).
    fn existing_bridge_participant(&self) -> Option<String>;
    /// Sign the next writes with this participant; returns the previous
    /// author, which the adapter restores after its write.
    fn swap_author(&mut self, participant: &str) -> String;
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
    /// Status labels of the `ticket` type, in order.
    fn ticket_statuses(&self) -> Vec<String>;
    fn find_by_web_link(&self, url: &str) -> Vec<String>;
    /// A new `ticket` in `status` (a label of [`Self::ticket_statuses`]);
    /// `priority` 1 urgent … 4 low. Returns its id.
    #[allow(clippy::too_many_arguments)]
    fn create_ticket(
        &mut self,
        title: &str,
        description: &str,
        tags: &[String],
        assignee: Option<&str>,
        parent: Option<&str>,
        priority: Option<u8>,
        status: &str,
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
}

/// The code of a short id (`TD-7K3F` → `7K3F`): the core resolves by the
/// code alone, the human prefix may change (Livela contract v4, decision 14).
fn code_of(key: &str) -> &str {
    key.rsplit_once('-').map(|(_, c)| c).unwrap_or(key)
}

/// The core's shortest code.
const MIN_CODE: usize = 4;

impl<S: LivelaStore> Livela<S> {
    pub fn new(store: S, statuses: StatusTable) -> Livela<S> {
        Livela {
            store: RefCell::new(store),
            statuses,
        }
    }

    pub fn into_store(self) -> S {
        self.store.into_inner()
    }

    /// A write signed by the `spl-asdlc` participant, the store's previous
    /// author restored after it whatever its outcome.
    fn as_bridge<T>(
        &self,
        what: impl Into<String>,
        f: impl FnOnce(&mut S, &str) -> Result<T, String>,
    ) -> TResult<T> {
        let what = what.into();
        let mut store = self.store.borrow_mut();
        let me = store
            .bridge_participant()
            .map_err(|e| TrackerError::refused("the spl-asdlc participant", e))?;
        let previous = store.swap_author(&me);
        let r = f(&mut store, &me);
        store.swap_author(&previous);
        r.map_err(|e| TrackerError::refused(what, e))
    }

    /// The internal id of a key, resolved by its code as the core does; a
    /// code shorter than the core's shortest is no key (it would match by
    /// prefix). A code several items share is unreadable, never a guess.
    fn id_of(&self, key: &str) -> TResult<String> {
        if code_of(key).chars().count() < MIN_CODE {
            return Err(TrackerError::NotFound {
                what: key.to_string(),
            });
        }
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
        self.raw_id(key, &id)
    }

    fn raw_id(&self, what: &str, id: &str) -> TResult<LivelaItem> {
        self.store
            .borrow()
            .item(id)
            .map_err(|e| TrackerError::unreadable(what, e))
    }

    fn short_of(&self, id: &str) -> TResult<String> {
        self.store
            .borrow()
            .item(id)
            .map(|i| i.short_id)
            .map_err(|e| TrackerError::unreadable(id, e))
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

    fn item_of(&self, i: &LivelaItem) -> TResult<Item> {
        let status = self.statuses.canonical(&i.status);
        Ok(Item {
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
            parent: i.parent.as_deref().map(|p| self.short_of(p)).transpose()?,
            archived: i.deleted,
        })
    }

    fn tracker_status(&self, status: Status) -> TResult<String> {
        let name = self.statuses.tracker_name(status);
        self.store
            .borrow()
            .ticket_statuses()
            .into_iter()
            .find(|s| s.eq_ignore_ascii_case(name))
            .ok_or_else(|| {
                TrackerError::refused(
                    format!("status {status}"),
                    format!("the ticket type has no status \"{name}\""),
                )
            })
    }
}

impl<S: LivelaStore> Tracker for Livela<S> {
    /// The participant is created on first need, once.
    fn identity(&self) -> TResult<User> {
        let existing = self.store.borrow().existing_bridge_participant();
        let id = match existing {
            Some(id) => id,
            None => self
                .store
                .borrow_mut()
                .bridge_participant()
                .map_err(|e| TrackerError::refused("the spl-asdlc participant", e))?,
        };
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
        self.item_of(&raw)
    }

    fn relations(&self, key: &str) -> TResult<Relations> {
        let raw = self.raw(key)?;
        let rel = |kind: &str, holder: &str, target: &str, other: &str| -> TResult<Relation> {
            Ok(Relation {
                id: format!("{kind}:{holder}:{target}"),
                other: self.short_of(other)?,
            })
        };
        let mut out = Relations::default();
        for b in &raw.blocked_by {
            out.blocked_by.push(rel("blocked_by", &raw.id, b, b)?);
        }
        for b in &raw.blocks {
            // Held by the blocked item as its `blocked_by`.
            out.blocks.push(rel("blocked_by", b, &raw.id, b)?);
        }
        for r in &raw.related_to {
            out.related.push(rel("related_to", &raw.id, r, r)?);
        }
        if let Some(d) = &raw.duplicate_of {
            out.duplicates.push(rel("duplicate_of", &raw.id, d, d)?);
        }
        let dups = self.store.borrow().duplicates_of(&raw.id);
        for d in dups {
            out.duplicates.push(rel("duplicate_of", &d, &raw.id, &d)?);
        }
        Ok(out)
    }

    fn comments(&self, key: &str) -> TResult<Vec<Comment>> {
        let me = self.store.borrow().existing_bridge_participant();
        let raw = self.raw(key)?;
        Ok(raw
            .comments
            .iter()
            .map(|(id, author, text)| Comment {
                id: id.clone(),
                author: Some(self.user_of(author)),
                by_bridge: me.as_deref() == Some(author.as_str()),
                body: text.clone(),
            })
            .collect())
    }

    fn children(&self, key: &str) -> TResult<Vec<Item>> {
        let id = self.id_of(key)?;
        let items = self.store.borrow().items(None, Some(&id));
        items.iter().map(|i| self.item_of(i)).collect()
    }

    fn list_by_label(&self, label: &str) -> TResult<Vec<Item>> {
        let items = self.store.borrow().items(Some(label), None);
        items.iter().map(|i| self.item_of(i)).collect()
    }

    fn items_linked_to_pr(&self, pr_url: &str) -> TResult<Vec<String>> {
        let ids = self.store.borrow().find_by_web_link(pr_url);
        ids.iter().map(|id| self.short_of(id)).collect()
    }

    fn label_exists(&self, _name: &str) -> TResult<bool> {
        // Tags exist by being used: any can be added.
        Ok(true)
    }

    fn ensure_label(&self, _name: &str) -> TResult<()> {
        // Tags exist by being used.
        Ok(())
    }

    fn create_item(&self, new: &NewItem) -> TResult<Item> {
        // Every check before the first write: a refused creation leaves nothing.
        let status = self.tracker_status(new.status)?;
        let parent = match &new.parent {
            Some(p) => Some(self.id_of(p)?),
            None => None,
        };
        let id = self.as_bridge(format!("creation of \"{}\"", new.title), |s, _| {
            s.create_ticket(
                &new.title,
                &new.description,
                &new.labels,
                new.assignee.as_ref().map(|u| u.id.as_str()),
                parent.as_deref(),
                new.priority.map(Priority::level),
                &status,
            )
        })?;
        let raw = self.raw_id(&new.title, &id)?;
        self.item_of(&raw)
    }

    fn update_description(&self, key: &str, description: &str) -> TResult<()> {
        let raw = self.raw(key)?;
        if raw.description == description {
            return Ok(());
        }
        self.as_bridge(format!("description of {key}"), |s, _| {
            s.set_description(&raw.id, description)
        })
    }

    fn add_label(&self, key: &str, label: &str) -> TResult<()> {
        let raw = self.raw(key)?;
        if raw.tags.iter().any(|t| t.eq_ignore_ascii_case(label)) {
            return Ok(());
        }
        self.as_bridge(format!("tag {label} on {key}"), |s, _| {
            s.add_tag(&raw.id, label)
        })
    }

    fn remove_label(&self, key: &str, label: &str) -> TResult<()> {
        let raw = self.raw(key)?;
        if !raw.tags.iter().any(|t| t.eq_ignore_ascii_case(label)) {
            return Ok(());
        }
        self.as_bridge(format!("tag {label} off {key}"), |s, _| {
            s.remove_tag(&raw.id, label)
        })
    }

    fn set_status(
        &self,
        key: &str,
        status: Status,
        assignee: Option<Option<&User>>,
    ) -> TResult<()> {
        let raw = self.raw(key)?;
        let name = self.tracker_status(status)?;
        let move_it = !raw.status.eq_ignore_ascii_case(&name);
        let assign = assignee
            .map(|a| a.map(|u| u.id.as_str()))
            .filter(|a| *a != raw.assignee.as_deref());
        if !move_it && assign.is_none() {
            return Ok(());
        }
        self.as_bridge(format!("status {status} of {key}"), |s, _| {
            if move_it {
                s.set_status(&raw.id, &name)?;
            }
            if let Some(a) = assign {
                s.assign(&raw.id, a)?;
            }
            Ok(())
        })
    }

    fn assign(&self, key: &str, user: Option<&User>) -> TResult<()> {
        let raw = self.raw(key)?;
        let to = user.map(|u| u.id.as_str());
        if raw.assignee.as_deref() == to {
            return Ok(());
        }
        self.as_bridge(format!("assignee of {key}"), |s, _| s.assign(&raw.id, to))
    }

    fn comment(&self, key: &str, body: &str) -> TResult<Comment> {
        let id = self.id_of(key)?;
        let (cid, me) = self.as_bridge(format!("comment on {key}"), |s, me| {
            s.comment(&id, me, body).map(|c| (c, me.to_string()))
        })?;
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
        let (ra, rb) = (self.raw(a)?, self.raw(b)?);
        let (holder, rel, target) = match kind {
            // `a blocks b` is held by `b`: b is blocked by a.
            RelationKind::Blocks => {
                if rb.blocked_by.contains(&ra.id) {
                    return Ok(());
                }
                (&rb.id, LivelaRelation::BlockedBy, &ra.id)
            }
            RelationKind::Related => {
                if ra.related_to.contains(&rb.id) || rb.related_to.contains(&ra.id) {
                    return Ok(());
                }
                (&ra.id, LivelaRelation::RelatedTo, &rb.id)
            }
            RelationKind::Duplicate => match &ra.duplicate_of {
                Some(d) if *d == rb.id => return Ok(()),
                Some(d) => {
                    return Err(TrackerError::refused(
                        format!("relation {a} Duplicate {b}"),
                        format!("{a} is already a duplicate of {}", self.short_of(d)?),
                    ));
                }
                None => (&ra.id, LivelaRelation::DuplicateOf, &rb.id),
            },
        };
        self.as_bridge(format!("relation {a} {kind:?} {b}"), |s, _| {
            s.add_relation(holder, rel, target)
        })
    }

    fn unrelate(&self, relation_id: &str) -> TResult<()> {
        let mut parts = relation_id.splitn(3, ':');
        let (Some(kind), Some(holder), Some(target)) = (parts.next(), parts.next(), parts.next())
        else {
            return Err(TrackerError::refused(
                "unrelate",
                format!("\"{relation_id}\" is not a relation id"),
            ));
        };
        let not_found = || TrackerError::NotFound {
            what: format!("relation {relation_id}"),
        };
        let item = self.store.borrow().item(holder).map_err(|_| not_found())?;
        let (kind, present) = match kind {
            "blocked_by" => (
                LivelaRelation::BlockedBy,
                item.blocked_by.iter().any(|x| x == target),
            ),
            "related_to" => (
                LivelaRelation::RelatedTo,
                item.related_to.iter().any(|x| x == target),
            ),
            "duplicate_of" => (
                LivelaRelation::DuplicateOf,
                item.duplicate_of.as_deref() == Some(target),
            ),
            other => {
                return Err(TrackerError::refused(
                    "unrelate",
                    format!("unknown relation kind {other}"),
                ));
            }
        };
        if !present {
            return Err(not_found());
        }
        self.as_bridge(format!("removal of {relation_id}"), |s, _| {
            s.remove_relation(holder, kind, target)
        })
    }

    // Livela counts no open items against a limit: nothing to archive
    // (`docs/livela-contract.md` §7).
    fn archives(&self) -> bool {
        false
    }

    fn archive(&self, _key: &str) -> TResult<()> {
        Ok(())
    }

    fn keeps_html_comments(&self) -> bool {
        // The description and comments are stored as written.
        true
    }

    fn keys_in_text(&self, text: &str) -> Vec<String> {
        let mut keys: Vec<String> = text
            .match_indices("livela://item/")
            .map(|(i, m)| {
                text[i + m.len()..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                    .collect::<String>()
            })
            .filter(|k| !k.is_empty())
            .collect();
        keys.sort();
        keys.dedup();
        keys
    }

    fn link_pr(&self, key: &str, pr_url: &str, title: &str) -> TResult<()> {
        let id = self.id_of(key)?;
        if self.store.borrow().find_by_web_link(pr_url).contains(&id) {
            return Ok(());
        }
        self.as_bridge(format!("link of {pr_url} on {key}"), |s, _| {
            s.add_web_link(&id, pr_url, title)
        })
    }
}

#[cfg(test)]
mod tests;
