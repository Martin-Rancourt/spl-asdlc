//! The tracker behind an interface (`docs/spec/00-overview.md` invariant 8,
//! `07-the-bridge.md`, REQ-REL-098): read an item and its contract, set a
//! status by canonical name, comment, label, relate, link a PR, find by PR
//! URL, create the bridge's items. Nothing else. Every text is English.
//!
//! Reads distinguish "not found" from "could not read"; writes say whether
//! the tracker refused. The method speaks canonical statuses; the adapter
//! maps them through the project's status table.

pub mod conformance;
pub mod fake;
pub mod linear;
pub mod livela;

use std::fmt;

use crate::method::vocabulary::{ItemState, Status};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TrackerError {
    #[error("{what}: not found")]
    NotFound { what: String },
    #[error("{what} could not be read: {detail}")]
    Unreadable { what: String, detail: String },
    #[error("{what} refused: {detail}")]
    Refused { what: String, detail: String },
}

impl TrackerError {
    pub fn unreadable(what: impl Into<String>, detail: impl Into<String>) -> TrackerError {
        TrackerError::Unreadable {
            what: what.into(),
            detail: detail.into(),
        }
    }

    pub fn refused(what: impl Into<String>, detail: impl Into<String>) -> TrackerError {
        TrackerError::Refused {
            what: what.into(),
            detail: detail.into(),
        }
    }

    pub fn is_not_found(&self) -> bool {
        matches!(self, TrackerError::NotFound { .. })
    }
}

pub type TResult<T> = Result<T, TrackerError>;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct User {
    pub id: String,
    pub name: String,
}

impl fmt::Display for User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

/// An item as the method reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// The backend's internal id.
    pub id: String,
    /// The identifier people use (`ABC-12`).
    pub key: String,
    pub title: String,
    pub description: String,
    pub url: String,
    pub state: ItemState,
    pub labels: Vec<String>,
    pub assignee: Option<User>,
    /// Identifier of the parent item.
    pub parent: Option<String>,
    pub archived: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationKind {
    /// `a blocks b`: `b` waits on `a`.
    Blocks,
    Related,
    Duplicate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relation {
    pub id: String,
    pub other: String,
}

/// An item's relations, both directions, and whether a page was cut.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Relations {
    /// Items this one blocks.
    pub blocks: Vec<Relation>,
    /// Items that block this one.
    pub blocked_by: Vec<Relation>,
    pub related: Vec<Relation>,
    /// Items linked by `duplicate`, in either direction.
    pub duplicates: Vec<Relation>,
    /// A relation list was cut by pagination: the answer is partial.
    pub cut: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub id: String,
    pub author: Option<User>,
    /// Written by the bridge's own tracker identity: the only comments whose
    /// markers the bridge trusts.
    pub by_bridge: bool,
    pub body: String,
}

/// What the bridge creates: a release item, an angle sub-item, a
/// consolidation item, a finding ticket, the nits ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewItem {
    pub title: String,
    pub description: String,
    pub labels: Vec<String>,
    pub status: Status,
    /// 1 urgent … 4 low; `None` for none.
    pub priority: Option<u8>,
    pub assignee: Option<User>,
    /// Identifier of the parent.
    pub parent: Option<String>,
}

/// The tracker operations the bridge needs.
pub trait Tracker {
    /// The service identity the bridge writes as.
    fn identity(&self) -> TResult<User>;
    /// The team's identifier prefix (`ABC`).
    fn team_key(&self) -> TResult<String>;
    /// Tracker status names the team has, to check the status table.
    fn status_names(&self) -> TResult<Vec<String>>;
    /// Resolve a person by id, name or display name (assignee hints).
    fn user(&self, reference: &str) -> TResult<User>;

    fn get(&self, key: &str) -> TResult<Item>;
    fn relations(&self, key: &str) -> TResult<Relations>;
    /// The whole thread, oldest first.
    fn comments(&self, key: &str) -> TResult<Vec<Comment>>;
    fn children(&self, key: &str) -> TResult<Vec<Item>>;
    /// Every item carrying a label (a list the tracker cuts is unreadable).
    fn list_by_label(&self, label: &str) -> TResult<Vec<Item>>;
    /// Items whose link (attachment, web link) is this PR URL.
    fn items_linked_to_pr(&self, pr_url: &str) -> TResult<Vec<String>>;

    fn ensure_label(&self, name: &str) -> TResult<()>;
    fn create_item(&self, item: &NewItem) -> TResult<Item>;
    fn update_description(&self, key: &str, description: &str) -> TResult<()>;
    fn add_label(&self, key: &str, label: &str) -> TResult<()>;
    fn remove_label(&self, key: &str, label: &str) -> TResult<()>;
    /// Set a status by canonical name; `assignee` `Some` changes the
    /// assignee in the same write (`Some(None)` unassigns).
    fn set_status(&self, key: &str, status: Status, assignee: Option<Option<&User>>)
    -> TResult<()>;
    fn assign(&self, key: &str, user: Option<&User>) -> TResult<()>;
    fn comment(&self, key: &str, body: &str) -> TResult<Comment>;
    /// Whether the tracker can edit a comment in place. Without it, the
    /// bridge keeps no rewritten status row (merge Q22): informational facts
    /// stay in its action log, messages that ask someone to act are comments.
    fn can_edit_comments(&self) -> bool {
        true
    }
    fn update_comment(&self, comment_id: &str, body: &str) -> TResult<()>;
    /// `relate(a, b, Blocks)`: `a` blocks `b`. The caller checks for loops.
    fn relate(&self, a: &str, b: &str, kind: RelationKind) -> TResult<()>;
    fn unrelate(&self, relation_id: &str) -> TResult<()>;
    fn link_pr(&self, key: &str, pr_url: &str, title: &str) -> TResult<()>;

    /// Whether HTML comments survive a write: when they do not, the bridge
    /// writes its markers visible (as inline code).
    fn keeps_html_comments(&self) -> bool {
        false
    }

    /// Item identifiers named by tracker URLs in a text (the configured
    /// `link_fallback = "body-url"`); never bare identifiers.
    fn keys_in_text(&self, _text: &str) -> Vec<String> {
        Vec::new()
    }
}

/// The status table of a project: canonical name -> tracker name, and back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusTable {
    pairs: Vec<(Status, String)>,
}

impl StatusTable {
    pub fn from_config(statuses: &std::collections::BTreeMap<String, String>) -> StatusTable {
        StatusTable {
            pairs: Status::ALL
                .iter()
                .map(|s| {
                    (
                        *s,
                        statuses
                            .get(s.name())
                            .cloned()
                            .unwrap_or_else(|| s.name().to_string()),
                    )
                })
                .collect(),
        }
    }

    pub fn identity() -> StatusTable {
        StatusTable::from_config(&Default::default())
    }

    pub fn tracker_name(&self, status: Status) -> &str {
        self.pairs
            .iter()
            .find(|(s, _)| *s == status)
            .map(|(_, n)| n.as_str())
            .unwrap_or_else(|| status.name())
    }

    pub fn canonical(&self, tracker_name: &str) -> Option<Status> {
        self.pairs
            .iter()
            .find(|(_, n)| n.eq_ignore_ascii_case(tracker_name))
            .map(|(s, _)| *s)
    }

    /// Canonical statuses whose tracker name the team lacks.
    pub fn missing(&self, names: &[String]) -> Vec<Status> {
        self.pairs
            .iter()
            .filter(|(_, n)| !names.iter().any(|x| x.eq_ignore_ascii_case(n)))
            .map(|(s, _)| *s)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_table_maps_both_ways() {
        let mut map = std::collections::BTreeMap::new();
        map.insert("To Deploy".to_string(), "To deploy".to_string());
        let t = StatusTable::from_config(&map);
        assert_eq!(t.tracker_name(Status::ToDeploy), "To deploy");
        assert_eq!(t.tracker_name(Status::Done), "Done");
        assert_eq!(t.canonical("to deploy"), Some(Status::ToDeploy));
        assert_eq!(t.canonical("Shipped"), None);
        let missing = t.missing(&["Done".into(), "To deploy".into()]);
        assert!(missing.contains(&Status::Triage));
        assert!(!missing.contains(&Status::ToDeploy));
    }
}
