//! The Linear adapter (GraphQL, `docs/setup/tracker-linear.md`).
//!
//! The service identity writes with its API key. Linear answers GraphQL
//! errors with HTTP 200: an answer whose errors all say the entity was not
//! found is `NotFound`; any other error is `Unreadable` (a read) or
//! `Refused` (a write). A list Linear would cut is unreadable.

use std::cell::RefCell;
use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::*;
use crate::http::{HttpRequest, Transport, UreqTransport};
use crate::method::vocabulary::StateKind;

pub const ENDPOINT: &str = "https://api.linear.app/graphql";
/// The variable the bridge reads its tracker key from.
pub const ENV_TOKEN: &str = "SPL_ASDLC_TRACKER_TOKEN";
/// A label list or a relation list longer than this is refused.
const MAX_LIST: usize = 1000;

const ISSUE_FIELDS: &str = "id identifier title description url archivedAt
  state { name type }
  labels(first: 100) { nodes { name } pageInfo { hasNextPage } }
  assignee { id name displayName }
  parent { identifier }";
/// The fields of an item in a list. Linear scores a query by the product of
/// the page sizes and refuses one over its complexity budget, so a list of
/// 50 asks for at most 20 labels per item; an item with more is unreadable
/// (fail closed), never cut.
const LIST_ISSUE_FIELDS: &str = "id identifier title description url archivedAt
  state { name type }
  labels(first: 20) { nodes { name } pageInfo { hasNextPage } }
  assignee { id name displayName }
  parent { identifier }";

pub struct Linear {
    transport: Box<dyn Transport>,
    endpoint: String,
    token: String,
    team_key: String,
    statuses: StatusTable,
    cache: RefCell<Cache>,
}

#[derive(Default)]
struct Cache {
    viewer: Option<User>,
    team: Option<(String, BTreeMap<String, String>)>,
    labels: BTreeMap<String, String>,
}

fn kind_of(t: &str) -> StateKind {
    match t {
        "triage" => StateKind::Triage,
        "backlog" => StateKind::Backlog,
        "unstarted" => StateKind::Unstarted,
        "started" => StateKind::Started,
        "completed" => StateKind::Completed,
        "canceled" => StateKind::Canceled,
        _ => StateKind::Unknown,
    }
}

/// A random UUID (v4) for the ids Linear lets the client choose: a write
/// that times out can then be looked up instead of being repeated.
fn new_uuid() -> String {
    use std::hash::{BuildHasher, Hasher};
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let mut bytes = [0u8; 16];
    for (i, chunk) in bytes.chunks_mut(8).enumerate() {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(nanos);
        h.write_usize(i);
        chunk.copy_from_slice(&h.finish().to_be_bytes());
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

fn looks_like_uuid(r: &str) -> bool {
    r.len() == 36
        && r.chars().enumerate().all(|(i, c)| {
            matches!(i, 8 | 13 | 18 | 23) == (c == '-') && (c == '-' || c.is_ascii_hexdigit())
        })
}

fn s(v: &Value, k: &str) -> String {
    v.get(k)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn user_of(v: &Value) -> Option<User> {
    let id = v.get("id")?.as_str()?.to_string();
    let name = v
        .get("displayName")
        .and_then(Value::as_str)
        .filter(|n| !n.is_empty())
        .or_else(|| v.get("name").and_then(Value::as_str))
        .unwrap_or_default()
        .to_string();
    Some(User { id, name })
}

impl Linear {
    pub fn new(token: &str, team_key: &str, statuses: StatusTable) -> Linear {
        Linear::with_transport(
            token,
            team_key,
            statuses,
            Box::new(UreqTransport::default()),
            ENDPOINT,
        )
    }

    pub fn with_transport(
        token: &str,
        team_key: &str,
        statuses: StatusTable,
        transport: Box<dyn Transport>,
        endpoint: &str,
    ) -> Linear {
        Linear {
            transport,
            endpoint: endpoint.to_string(),
            token: token.to_string(),
            team_key: team_key.to_string(),
            statuses,
            cache: RefCell::new(Cache::default()),
        }
    }

    /// One GraphQL call; `Err((not_found, detail))`.
    fn call(&self, query: &str, variables: Value) -> Result<Value, (bool, String)> {
        let body = json!({"query": query, "variables": variables})
            .to_string()
            .into_bytes();
        let response = self
            .transport
            .send(HttpRequest {
                method: "POST",
                url: self.endpoint.clone(),
                headers: vec![
                    ("Authorization".into(), self.token.clone()),
                    ("Content-Type".into(), "application/json".into()),
                    (
                        "User-Agent".into(),
                        concat!("spl-asdlc/", env!("CARGO_PKG_VERSION")).into(),
                    ),
                ],
                body: Some(body),
            })
            .map_err(|e| (false, e))?;
        let value: Value = serde_json::from_slice(&response.body).map_err(|_| {
            (
                false,
                format!("HTTP {}: the answer is not JSON", response.status),
            )
        })?;
        if let Some(errors) = value
            .get("errors")
            .and_then(Value::as_array)
            .filter(|e| !e.is_empty())
        {
            let messages: Vec<String> = errors.iter().map(|e| s(e, "message")).collect();
            let not_found = messages.iter().all(|m| m.starts_with("Entity not found"));
            return Err((not_found, messages.join("; ")));
        }
        if !(200..300).contains(&response.status) {
            return Err((false, format!("HTTP {}", response.status)));
        }
        value
            .get("data")
            .cloned()
            .filter(|d| !d.is_null())
            .ok_or((false, "no data in the answer".to_string()))
    }

    fn read(&self, what: &str, query: &str, variables: Value) -> TResult<Value> {
        self.call(query, variables).map_err(|(nf, d)| {
            if nf {
                TrackerError::NotFound {
                    what: what.to_string(),
                }
            } else {
                TrackerError::unreadable(what, d)
            }
        })
    }

    fn write(&self, what: &str, query: &str, variables: Value, field: &str) -> TResult<Value> {
        let data = self.call(query, variables).map_err(|(nf, d)| {
            if nf {
                TrackerError::NotFound {
                    what: what.to_string(),
                }
            } else {
                TrackerError::refused(what, d)
            }
        })?;
        let result = data.get(field).cloned().unwrap_or(Value::Null);
        if result.get("success").and_then(Value::as_bool) != Some(true) {
            return Err(TrackerError::refused(
                what,
                "Linear did not confirm the write",
            ));
        }
        Ok(result)
    }

    fn team(&self) -> TResult<(String, BTreeMap<String, String>)> {
        if let Some(t) = &self.cache.borrow().team {
            return Ok(t.clone());
        }
        let what = format!("team {}", self.team_key);
        let data = self.read(
            &what,
            "query($key: String!) { teams(filter: {key: {eq: $key}}) { nodes { id key states(first: 100) { nodes { id name type } } } } }",
            json!({"key": self.team_key}),
        )?;
        let team = data["teams"]["nodes"]
            .as_array()
            .and_then(|n| n.first())
            .ok_or_else(|| TrackerError::NotFound { what: what.clone() })?;
        let id = s(team, "id");
        let states = team["states"]["nodes"]
            .as_array()
            .ok_or_else(|| TrackerError::unreadable(&what, "no state list"))?
            .iter()
            .map(|st| (s(st, "name"), s(st, "id")))
            .collect::<BTreeMap<_, _>>();
        let t = (id, states);
        self.cache.borrow_mut().team = Some(t.clone());
        Ok(t)
    }

    fn state_id(&self, status: Status) -> TResult<String> {
        let (_, states) = self.team()?;
        let name = self.statuses.tracker_name(status);
        states
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, id)| id.clone())
            .ok_or_else(|| {
                TrackerError::refused(
                    format!("status {status}"),
                    format!("team {} has no state named \"{name}\"", self.team_key),
                )
            })
    }

    fn item_of(&self, v: &Value) -> TResult<Item> {
        let key = s(v, "identifier");
        if v["labels"]["pageInfo"]["hasNextPage"].as_bool() != Some(false) {
            return Err(TrackerError::unreadable(
                &key,
                "the label list is cut or missing",
            ));
        }
        let state_name = s(&v["state"], "name");
        Ok(Item {
            id: s(v, "id"),
            key,
            title: s(v, "title"),
            description: s(v, "description"),
            url: s(v, "url"),
            state: ItemState {
                status: self.statuses.canonical(&state_name),
                kind: kind_of(&s(&v["state"], "type")),
                name: state_name,
            },
            labels: v["labels"]["nodes"]
                .as_array()
                .map(|a| a.iter().map(|l| s(l, "name")).collect())
                .unwrap_or_default(),
            assignee: user_of(&v["assignee"]),
            parent: v["parent"]["identifier"].as_str().map(str::to_string),
            archived: !v["archivedAt"].is_null(),
        })
    }

    /// All pages of an issue connection, refused beyond `MAX_LIST`.
    fn issues(
        &self,
        what: &str,
        query: &str,
        mut vars: Value,
        path: &[&str],
    ) -> TResult<Vec<Item>> {
        let mut out = Vec::new();
        let mut cursor = Value::Null;
        loop {
            vars["after"] = cursor.clone();
            let data = self.read(what, query, vars.clone())?;
            let mut conn = &data;
            for p in path {
                conn = &conn[*p];
            }
            let nodes = conn["nodes"]
                .as_array()
                .ok_or_else(|| TrackerError::unreadable(what, "no node list"))?;
            for n in nodes {
                out.push(self.item_of(n)?);
            }
            if out.len() > MAX_LIST {
                return Err(TrackerError::unreadable(
                    what,
                    format!("more than {MAX_LIST} items"),
                ));
            }
            match conn["pageInfo"]["hasNextPage"].as_bool() {
                Some(false) => return Ok(out),
                Some(true) => cursor = conn["pageInfo"]["endCursor"].clone(),
                None => {
                    return Err(TrackerError::unreadable(
                        what,
                        "the list does not say whether it was cut",
                    ));
                }
            }
        }
    }

    /// The team's label of this name, or the workspace's; Linear compares
    /// label names without case, so does the lookup.
    fn label_id(&self, name: &str) -> TResult<Option<String>> {
        let cache_key = name.to_lowercase();
        if let Some(id) = self.cache.borrow().labels.get(&cache_key) {
            return Ok(Some(id.clone()));
        }
        let (team_id, _) = self.team()?;
        let what = format!("label {name}");
        let data = self.read(
            &what,
            "query($name: String!, $team: ID!) { issueLabels(filter: {name: {eqIgnoreCase: $name}, or: [{team: {id: {eq: $team}}}, {team: {null: true}}]}, first: 50) { nodes { id name team { id } } pageInfo { hasNextPage } } }",
            json!({"name": name, "team": team_id}),
        )?;
        let conn = &data["issueLabels"];
        if conn["pageInfo"]["hasNextPage"].as_bool() != Some(false) {
            return Err(TrackerError::unreadable(
                &what,
                "the label list is cut or missing",
            ));
        }
        let id = conn["nodes"].as_array().and_then(|nodes| {
            nodes
                .iter()
                .find(|l| l["team"]["id"].as_str() == Some(team_id.as_str()))
                .or_else(|| nodes.iter().find(|l| l["team"].is_null()))
                .map(|l| s(l, "id"))
        });
        if let Some(id) = &id {
            self.cache.borrow_mut().labels.insert(cache_key, id.clone());
        }
        Ok(id)
    }

    fn issue_id(&self, key: &str) -> TResult<String> {
        Ok(self.get(key)?.id)
    }
}

impl Tracker for Linear {
    fn identity(&self) -> TResult<User> {
        if let Some(u) = &self.cache.borrow().viewer {
            return Ok(u.clone());
        }
        let data = self.read(
            "the tracker identity",
            "query { viewer { id name displayName } }",
            json!({}),
        )?;
        let u = user_of(&data["viewer"])
            .ok_or_else(|| TrackerError::unreadable("the tracker identity", "no viewer"))?;
        self.cache.borrow_mut().viewer = Some(u.clone());
        Ok(u)
    }

    fn team_key(&self) -> TResult<String> {
        self.team()?;
        Ok(self.team_key.clone())
    }

    fn status_names(&self) -> TResult<Vec<String>> {
        Ok(self.team()?.1.into_keys().collect())
    }

    fn user(&self, reference: &str) -> TResult<User> {
        if looks_like_uuid(reference) {
            let data = self.read(
                &format!("user {reference}"),
                "query($id: String!) { user(id: $id) { id name displayName } }",
                json!({"id": reference}),
            )?;
            return user_of(&data["user"]).ok_or_else(|| TrackerError::NotFound {
                what: format!("user {reference}"),
            });
        }
        let data = self.read(
            &format!("user {reference}"),
            "query($r: String!) { users(filter: {or: [{name: {eqIgnoreCase: $r}}, {displayName: {eqIgnoreCase: $r}}, {email: {eq: $r}}]}, first: 2) { nodes { id name displayName } } }",
            json!({"r": reference}),
        )?;
        let nodes = data["users"]["nodes"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        match nodes.len() {
            1 => user_of(&nodes[0])
                .ok_or_else(|| TrackerError::unreadable(reference, "a user without an id")),
            0 => Err(TrackerError::NotFound {
                what: format!("user {reference}"),
            }),
            _ => Err(TrackerError::unreadable(
                format!("user {reference}"),
                "several users answer to this name",
            )),
        }
    }

    fn get(&self, key: &str) -> TResult<Item> {
        let data = self.read(
            key,
            &format!("query($id: String!) {{ issue(id: $id) {{ {ISSUE_FIELDS} }} }}"),
            json!({"id": key}),
        )?;
        if data["issue"].is_null() {
            return Err(TrackerError::NotFound {
                what: key.to_string(),
            });
        }
        self.item_of(&data["issue"])
    }

    fn relations(&self, key: &str) -> TResult<Relations> {
        let data = self.read(
            &format!("relations of {key}"),
            "query($id: String!) { issue(id: $id) {
               relations(first: 100) { nodes { id type relatedIssue { identifier } } pageInfo { hasNextPage } }
               inverseRelations(first: 100) { nodes { id type issue { identifier } } pageInfo { hasNextPage } } } }",
            json!({"id": key}),
        )?;
        let issue = &data["issue"];
        if issue.is_null() {
            return Err(TrackerError::NotFound {
                what: key.to_string(),
            });
        }
        let mut out = Relations::default();
        for (field, other_field, outward) in [
            ("relations", "relatedIssue", true),
            ("inverseRelations", "issue", false),
        ] {
            let conn = &issue[field];
            match conn["pageInfo"]["hasNextPage"].as_bool() {
                Some(false) => {}
                Some(true) => out.cut = true,
                None => {
                    return Err(TrackerError::unreadable(
                        key,
                        format!("`{field}` does not say whether it was cut"),
                    ));
                }
            }
            for r in conn["nodes"].as_array().into_iter().flatten() {
                let rel = Relation {
                    id: s(r, "id"),
                    other: s(&r[other_field], "identifier"),
                };
                if rel.id.is_empty() || rel.other.is_empty() {
                    return Err(TrackerError::unreadable(
                        key,
                        format!("a relation in `{field}` names no item"),
                    ));
                }
                match (s(r, "type").as_str(), outward) {
                    ("blocks", true) => out.blocks.push(rel),
                    ("blocks", false) => out.blocked_by.push(rel),
                    ("duplicate", _) => out.duplicates.push(rel),
                    ("related" | "similar", _) => out.related.push(rel),
                    _ => {}
                }
            }
        }
        Ok(out)
    }

    fn comments(&self, key: &str) -> TResult<Vec<Comment>> {
        let me = self.identity()?;
        let what = format!("comments of {key}");
        let mut out: Vec<(String, Comment)> = Vec::new();
        let mut cursor = Value::Null;
        loop {
            let data = self.read(
                &what,
                "query($id: String!, $after: String) { issue(id: $id) { comments(first: 100, after: $after) {
                   nodes { id body createdAt user { id name displayName } } pageInfo { hasNextPage endCursor } } } }",
                json!({"id": key, "after": cursor}),
            )?;
            let conn = &data["issue"]["comments"];
            for c in conn["nodes"]
                .as_array()
                .ok_or_else(|| TrackerError::unreadable(&what, "no comment list"))?
            {
                let author = user_of(&c["user"]);
                let at = s(c, "createdAt");
                if at.is_empty() {
                    return Err(TrackerError::unreadable(&what, "a comment without a date"));
                }
                out.push((
                    at,
                    Comment {
                        id: s(c, "id"),
                        by_bridge: author.as_ref().is_some_and(|a| a.id == me.id),
                        author,
                        body: s(c, "body"),
                    },
                ));
            }
            if out.len() > 10 * MAX_LIST {
                return Err(TrackerError::unreadable(&what, "too many comments"));
            }
            match conn["pageInfo"]["hasNextPage"].as_bool() {
                Some(false) => break,
                Some(true) => cursor = conn["pageInfo"]["endCursor"].clone(),
                None => {
                    return Err(TrackerError::unreadable(
                        &what,
                        "the thread does not say whether it was cut",
                    ));
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out.into_iter().map(|(_, c)| c).collect())
    }

    fn children(&self, key: &str) -> TResult<Vec<Item>> {
        self.issues(
            &format!("children of {key}"),
            &format!(
                "query($id: String!, $after: String) {{ issue(id: $id) {{ children(first: 50, after: $after, includeArchived: true) {{ nodes {{ {LIST_ISSUE_FIELDS} }} pageInfo {{ hasNextPage endCursor }} }} }} }}"
            ),
            json!({"id": key}),
            &["issue", "children"],
        )
    }

    fn list_by_label(&self, label: &str) -> TResult<Vec<Item>> {
        let (team_id, _) = self.team()?;
        self.issues(
            &format!("items labelled {label}"),
            &format!(
                "query($label: String!, $team: ID!, $after: String) {{ issues(filter: {{labels: {{name: {{eqIgnoreCase: $label}}}}, team: {{id: {{eq: $team}}}}}}, first: 50, after: $after, includeArchived: true) {{ nodes {{ {LIST_ISSUE_FIELDS} }} pageInfo {{ hasNextPage endCursor }} }} }}"
            ),
            json!({"label": label, "team": team_id}),
            &["issues"],
        )
    }

    fn find_labelled_containing(&self, label: &str, needle: &str) -> TResult<Vec<Item>> {
        let (team_id, _) = self.team()?;
        self.issues(
            &format!("items labelled {label} containing {needle}"),
            &format!(
                "query($label: String!, $team: ID!, $needle: String!, $after: String) {{ issues(filter: {{labels: {{name: {{eqIgnoreCase: $label}}}}, team: {{id: {{eq: $team}}}}, description: {{contains: $needle}}}}, first: 50, after: $after, includeArchived: true) {{ nodes {{ {LIST_ISSUE_FIELDS} }} pageInfo {{ hasNextPage endCursor }} }} }}"
            ),
            json!({"label": label, "team": team_id, "needle": needle}),
            &["issues"],
        )
    }

    fn items_linked_to_pr(&self, pr_url: &str) -> TResult<Vec<String>> {
        let data = self.read(
            &format!("items linked to {pr_url}"),
            "query($url: String!) { attachmentsForURL(url: $url) { nodes { issue { identifier } } pageInfo { hasNextPage } } }",
            json!({"url": pr_url}),
        )?;
        let conn = &data["attachmentsForURL"];
        if conn["pageInfo"]["hasNextPage"].as_bool() != Some(false) {
            return Err(TrackerError::unreadable(
                pr_url,
                "the attachment list is cut or missing",
            ));
        }
        let mut keys: Vec<String> = conn["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|a| s(&a["issue"], "identifier"))
            .filter(|k| !k.is_empty())
            .collect();
        keys.sort();
        keys.dedup();
        Ok(keys)
    }

    fn ensure_label(&self, name: &str) -> TResult<()> {
        if self.label_id(name)?.is_some() {
            return Ok(());
        }
        let (team_id, _) = self.team()?;
        let r = self.write(
            &format!("label {name}"),
            "mutation($name: String!, $team: String!) { issueLabelCreate(input: {name: $name, teamId: $team}) { success issueLabel { id } } }",
            json!({"name": name, "team": team_id}),
            "issueLabelCreate",
        );
        match r {
            Ok(r) => {
                self.cache
                    .borrow_mut()
                    .labels
                    .insert(name.to_lowercase(), s(&r["issueLabel"], "id"));
                Ok(())
            }
            // Created meanwhile by another writer: the label is there.
            Err(e) => match self.label_id(name) {
                Ok(Some(_)) => Ok(()),
                _ => Err(e),
            },
        }
    }

    fn create_item(&self, item: &NewItem) -> TResult<Item> {
        let (team_id, _) = self.team()?;
        let state = self.state_id(item.status)?;
        let mut label_ids = Vec::new();
        for l in &item.labels {
            self.ensure_label(l)?;
            label_ids.push(self.label_id(l)?.ok_or_else(|| {
                TrackerError::refused(format!("label {l}"), "created but not found")
            })?);
        }
        let uuid = new_uuid();
        let mut input = json!({
            "id": uuid, "teamId": team_id, "title": item.title, "description": item.description,
            "labelIds": label_ids, "stateId": state,
        });
        if let Some(p) = item.priority {
            input["priority"] = json!(p);
        }
        if let Some(a) = &item.assignee {
            input["assigneeId"] = json!(a.id);
        }
        if let Some(parent) = &item.parent {
            input["parentId"] = json!(self.issue_id(parent)?);
        }
        let r = self.write(
            &format!("creation of \"{}\"", item.title),
            &format!("mutation($input: IssueCreateInput!) {{ issueCreate(input: $input) {{ success issue {{ {ISSUE_FIELDS} }} }} }}"),
            json!({"input": input}),
            "issueCreate",
        );
        match r {
            Ok(r) => self.item_of(&r["issue"]),
            // The answer was lost, not necessarily the write: the id we
            // chose says whether the item exists.
            Err(e @ TrackerError::Refused { .. }) => self.get(&uuid).map_err(|_| e),
            Err(e) => Err(e),
        }
    }

    fn update_description(&self, key: &str, description: &str) -> TResult<()> {
        self.write(
            &format!("description of {key}"),
            "mutation($id: String!, $d: String!) { issueUpdate(id: $id, input: {description: $d}) { success } }",
            json!({"id": key, "d": description}),
            "issueUpdate",
        )
        .map(|_| ())
    }

    fn add_label(&self, key: &str, label: &str) -> TResult<()> {
        let id = self.label_id(label)?.ok_or_else(|| {
            TrackerError::refused(
                format!("label {label} on {key}"),
                "the team has no such label",
            )
        })?;
        self.write(
            &format!("label {label} on {key}"),
            "mutation($id: String!, $l: String!) { issueAddLabel(id: $id, labelId: $l) { success } }",
            json!({"id": key, "l": id}),
            "issueAddLabel",
        )
        .map(|_| ())
    }

    fn remove_label(&self, key: &str, label: &str) -> TResult<()> {
        let Some(id) = self.label_id(label)? else {
            return Ok(());
        };
        self.write(
            &format!("removal of {label} from {key}"),
            "mutation($id: String!, $l: String!) { issueRemoveLabel(id: $id, labelId: $l) { success } }",
            json!({"id": key, "l": id}),
            "issueRemoveLabel",
        )
        .map(|_| ())
    }

    fn set_status(
        &self,
        key: &str,
        status: Status,
        assignee: Option<Option<&User>>,
    ) -> TResult<()> {
        let mut input = json!({"stateId": self.state_id(status)?});
        if let Some(a) = assignee {
            input["assigneeId"] = a.map(|u| json!(u.id)).unwrap_or(Value::Null);
        }
        self.write(
            &format!("status {status} of {key}"),
            "mutation($id: String!, $input: IssueUpdateInput!) { issueUpdate(id: $id, input: $input) { success } }",
            json!({"id": key, "input": input}),
            "issueUpdate",
        )
        .map(|_| ())
    }

    fn assign(&self, key: &str, user: Option<&User>) -> TResult<()> {
        self.write(
            &format!("assignee of {key}"),
            "mutation($id: String!, $input: IssueUpdateInput!) { issueUpdate(id: $id, input: $input) { success } }",
            json!({"id": key, "input": {"assigneeId": user.map(|u| json!(u.id)).unwrap_or(Value::Null)}}),
            "issueUpdate",
        )
        .map(|_| ())
    }

    fn comment(&self, key: &str, body: &str) -> TResult<Comment> {
        let me = self.identity()?;
        let id = self.issue_id(key)?;
        let uuid = new_uuid();
        let r = self.write(
            &format!("comment on {key}"),
            "mutation($id: String!, $b: String!, $c: String!) { commentCreate(input: {id: $c, issueId: $id, body: $b}) { success comment { id body user { id name displayName } } } }",
            json!({"id": id, "b": body, "c": uuid}),
            "commentCreate",
        );
        let comment = match r {
            Ok(r) => r["comment"].clone(),
            // The answer was lost, not necessarily the comment.
            Err(e @ TrackerError::Refused { .. }) => {
                let found = self.read(
                    &format!("comment {uuid}"),
                    "query($id: String!) { comment(id: $id) { id body user { id name displayName } } }",
                    json!({"id": uuid}),
                );
                match found {
                    Ok(d) if !d["comment"].is_null() => d["comment"].clone(),
                    _ => return Err(e),
                }
            }
            Err(e) => return Err(e),
        };
        let r = json!({ "comment": comment });
        let author = user_of(&r["comment"]["user"]);
        Ok(Comment {
            id: s(&r["comment"], "id"),
            by_bridge: author.as_ref().is_some_and(|a| a.id == me.id),
            author,
            body: s(&r["comment"], "body"),
        })
    }

    fn update_comment(&self, comment_id: &str, body: &str) -> TResult<()> {
        self.write(
            &format!("comment {comment_id}"),
            "mutation($id: String!, $b: String!) { commentUpdate(id: $id, input: {body: $b}) { success } }",
            json!({"id": comment_id, "b": body}),
            "commentUpdate",
        )
        .map(|_| ())
    }

    fn relate(&self, a: &str, b: &str, kind: RelationKind) -> TResult<()> {
        let kind = match kind {
            RelationKind::Blocks => "blocks",
            RelationKind::Related => "related",
            RelationKind::Duplicate => "duplicate",
        };
        let (ida, idb) = (self.issue_id(a)?, self.issue_id(b)?);
        let existing = self.relations(a)?;
        let list = match kind {
            "blocks" => &existing.blocks,
            "related" => &existing.related,
            _ => &existing.duplicates,
        };
        if list.iter().any(|x| x.other.eq_ignore_ascii_case(b)) {
            return Ok(());
        }
        if existing.cut {
            return Err(TrackerError::refused(
                format!("relation {a} {kind} {b}"),
                "the relation list is cut: cannot tell whether it exists",
            ));
        }
        self.write(
            &format!("relation {a} {kind} {b}"),
            "mutation($a: String!, $b: String!, $t: IssueRelationType!) { issueRelationCreate(input: {issueId: $a, relatedIssueId: $b, type: $t}) { success } }",
            json!({"a": ida, "b": idb, "t": kind}),
            "issueRelationCreate",
        )
        .map(|_| ())
    }

    fn unrelate(&self, relation_id: &str) -> TResult<()> {
        if relation_id.is_empty() {
            return Err(TrackerError::refused("unrelate", "no relation id"));
        }
        self.write(
            &format!("removal of relation {relation_id}"),
            "mutation($id: String!) { issueRelationDelete(id: $id) { success } }",
            json!({"id": relation_id}),
            "issueRelationDelete",
        )
        .map(|_| ())
    }

    fn archives(&self) -> bool {
        true
    }

    fn archive(&self, key: &str) -> TResult<()> {
        // `issueArchive` without `trash`: the issue leaves the open count
        // and stays searchable and restorable; it is never deleted.
        let id = self.issue_id(key)?;
        let r = self.write(
            &format!("archive of {key}"),
            "mutation($id: String!) { issueArchive(id: $id) { success } }",
            json!({"id": id}),
            "issueArchive",
        );
        match r {
            Ok(_) => Ok(()),
            // Archived meanwhile, or the answer was lost: the issue says.
            Err(e @ TrackerError::Refused { .. }) => match self.get(key) {
                Ok(i) if i.archived => Ok(()),
                _ => Err(e),
            },
            Err(e) => Err(e),
        }
    }

    fn keys_in_text(&self, text: &str) -> Vec<String> {
        static URL: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            regex::Regex::new(r"https://linear\.app/[A-Za-z0-9_-]+/issue/([A-Z][A-Z0-9]*-[0-9]+)")
                .unwrap()
        });
        let mut keys: Vec<String> = URL.captures_iter(text).map(|c| c[1].to_string()).collect();
        keys.sort();
        keys.dedup();
        keys
    }

    fn link_pr(&self, key: &str, pr_url: &str, title: &str) -> TResult<()> {
        // Linear refuses a second attachment of the same URL on an item: a
        // link already there is a no-op, and a lost answer is found here.
        if self
            .items_linked_to_pr(pr_url)?
            .iter()
            .any(|k| k.eq_ignore_ascii_case(key))
        {
            return Ok(());
        }
        let id = self.issue_id(key)?;
        self.write(
            &format!("link of {pr_url} on {key}"),
            "mutation($id: String!, $url: String!, $t: String) { attachmentLinkURL(issueId: $id, url: $url, title: $t) { success } }",
            json!({"id": id, "url": pr_url, "t": title}),
            "attachmentLinkURL",
        )
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests;
