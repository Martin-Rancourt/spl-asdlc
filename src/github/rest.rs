//! The GitHub client: REST for refs, files, runs and writes; GraphQL for
//! pull requests (one query carries checks, reviews and threads).

use std::collections::BTreeSet;

use base64::Engine;
use serde_json::{Value, json};
use time::OffsetDateTime;

use super::GitHub;
use super::auth::{Credentials, TokenCache, app_jwt};
use super::http::{HttpRequest, HttpResponse, Transport, UreqTransport};
use super::types::*;

const API: &str = "https://api.github.com";
const API_VERSION: &str = "2022-11-28";

/// Open PRs beyond this many make the listing unreadable.
const MAX_OPEN_PULLS: usize = 1000;
/// GitHub lists at most 3000 files of a PR.
const MAX_PULL_FILES: usize = 3000;
/// GitHub's compare lists at most 300 files.
const MAX_COMPARE_FILES: usize = 300;
/// A paged list longer than this many pages is refused.
const MAX_PAGES: usize = 1000;
/// A file read out of an artifact larger than this is refused.
const MAX_ARTIFACT_FILE: u64 = 1024 * 1024;

pub struct RestGitHub {
    transport: Box<dyn Transport>,
    credentials: Credentials,
    base_url: String,
    tokens: TokenCache,
}

impl RestGitHub {
    pub fn new(credentials: Credentials) -> RestGitHub {
        RestGitHub::with_transport(credentials, Box::new(UreqTransport::default()), API)
    }

    pub fn with_transport(
        credentials: Credentials,
        transport: Box<dyn Transport>,
        base_url: &str,
    ) -> RestGitHub {
        RestGitHub {
            transport,
            credentials,
            base_url: base_url.trim_end_matches('/').to_string(),
            tokens: TokenCache::default(),
        }
    }

    // ---- plumbing ----

    fn jwt(&self) -> Result<String, String> {
        match &self.credentials {
            Credentials::App {
                app_id,
                private_key_pem,
            } => app_jwt(app_id, private_key_pem, OffsetDateTime::now_utc()),
            Credentials::Token(_) => Err("a personal token has no App JWT".into()),
        }
    }

    /// The bearer token for calls on `repo`.
    fn token(&self, repo: &RepoSlug) -> Result<String, String> {
        match &self.credentials {
            Credentials::Token(t) => Ok(t.clone()),
            Credentials::App { .. } => {
                let now = OffsetDateTime::now_utc();
                if let Some(t) = self.tokens.get(repo, now) {
                    return Ok(t);
                }
                let jwt = self.jwt()?;
                let installation = self
                    .raw(
                        "GET",
                        &format!("/repos/{repo}/installation"),
                        &jwt,
                        None,
                        None,
                    )
                    .map_err(|e| format!("the App's installation on {repo}: {e}"))?;
                let installation = expect_json(&installation, 200)
                    .map_err(|e| format!("the App is not installed on {repo} ({e})"))?;
                let id = installation["id"]
                    .as_u64()
                    .ok_or_else(|| format!("no installation id for {repo}"))?;
                let minted = self
                    .raw(
                        "POST",
                        &format!("/app/installations/{id}/access_tokens"),
                        &jwt,
                        Some(b"{}".to_vec()),
                        None,
                    )
                    .map_err(|e| format!("installation token for {repo}: {e}"))?;
                let minted = expect_json(&minted, 201)
                    .map_err(|e| format!("installation token for {repo} refused ({e})"))?;
                let token = minted["token"]
                    .as_str()
                    .ok_or_else(|| "installation token missing in GitHub's answer".to_string())?
                    .to_string();
                let expires = minted["expires_at"]
                    .as_str()
                    .and_then(Timestamp::parse)
                    .map(|t| t.0)
                    .unwrap_or(now + time::Duration::minutes(30));
                self.tokens.put(repo, token.clone(), expires);
                Ok(token)
            }
        }
    }

    fn raw(
        &self,
        method: &'static str,
        path: &str,
        bearer: &str,
        body: Option<Vec<u8>>,
        accept: Option<&str>,
    ) -> Result<HttpResponse, String> {
        let url = if path.starts_with("http") {
            // A `Link` URL: the token goes to the API host and nowhere else.
            if !path.starts_with(&format!("{}/", self.base_url)) {
                return Err(format!("refusing to send the token to {path}"));
            }
            path.to_string()
        } else {
            format!("{}{}", self.base_url, path)
        };
        let mut headers = vec![
            (
                "Accept".to_string(),
                accept.unwrap_or("application/vnd.github+json").to_string(),
            ),
            ("Authorization".to_string(), format!("Bearer {bearer}")),
            ("X-GitHub-Api-Version".to_string(), API_VERSION.to_string()),
            (
                "User-Agent".to_string(),
                concat!("spl-asdlc/", env!("CARGO_PKG_VERSION")).to_string(),
            ),
        ];
        if body.is_some() {
            headers.push(("Content-Type".to_string(), "application/json".to_string()));
        }
        self.transport.send(HttpRequest {
            method,
            url,
            headers,
            body,
        })
    }

    /// One call on a repository, authenticated, answered or refused.
    fn call(
        &self,
        repo: &RepoSlug,
        method: &'static str,
        path: &str,
        body: Option<Value>,
        accept: Option<&str>,
    ) -> Result<HttpResponse, (Option<u16>, String)> {
        let token = self.token(repo).map_err(|e| (None, e))?;
        let body = body.map(|b| b.to_string().into_bytes());
        let response = self
            .raw(method, path, &token, body.clone(), accept)
            .map_err(|e| (None, e))?;
        if response.status == 401 && matches!(self.credentials, Credentials::App { .. }) {
            // A revoked or re-scoped installation token: mint a new one, once.
            self.tokens.evict(repo);
            let token = self.token(repo).map_err(|e| (None, e))?;
            return self
                .raw(method, path, &token, body, accept)
                .map_err(|e| (None, e));
        }
        Ok(response)
    }

    /// GET a JSON value; 404 is `NotFound`, any other non-2xx `Unreadable`.
    fn get_json(&self, repo: &RepoSlug, what: &str, path: &str) -> Read<(Value, HttpResponse)> {
        let response = self
            .call(repo, "GET", path, None, None)
            .map_err(|(status, detail)| ReadError::Unreadable {
                what: what.to_string(),
                status,
                detail,
            })?;
        classify_read(what, response)
    }

    /// GET every page of a list (follows `Link: rel="next"`); the list may
    /// sit under `key` of each page. More than `max` items is unreadable.
    fn get_pages(
        &self,
        repo: &RepoSlug,
        what: &str,
        path: &str,
        key: Option<&str>,
        max: usize,
    ) -> Read<Vec<Value>> {
        let mut out = Vec::new();
        let mut next = Some(path.to_string());
        let mut pages = 0;
        while let Some(p) = next.take() {
            pages += 1;
            if pages > MAX_PAGES {
                return Err(ReadError::unreadable(
                    what,
                    format!("more than {MAX_PAGES} pages: the list would be cut"),
                ));
            }
            let (value, response) = self.get_json(repo, what, &p)?;
            let items = match key {
                Some(k) => value.get(k).cloned().unwrap_or(Value::Null),
                None => value,
            };
            let Value::Array(items) = items else {
                return Err(ReadError::unreadable(what, "the answer is not a list"));
            };
            out.extend(items);
            if out.len() > max {
                return Err(ReadError::unreadable(
                    what,
                    format!("more than {max} entries: the list would be cut"),
                ));
            }
            next = response.header("link").and_then(next_link);
        }
        Ok(out)
    }

    fn graphql(&self, repo: &RepoSlug, what: &str, query: &str, variables: Value) -> Read<Value> {
        let response = self
            .call(
                repo,
                "POST",
                "/graphql",
                Some(json!({"query": query, "variables": variables})),
                None,
            )
            .map_err(|(status, detail)| ReadError::Unreadable {
                what: what.to_string(),
                status,
                detail,
            })?;
        let (value, _) = classify_read(what, response)?;
        if let Some(errors) = value.get("errors").and_then(Value::as_array) {
            if !errors.is_empty() {
                let not_found = errors
                    .iter()
                    .all(|e| e.get("type").and_then(Value::as_str) == Some("NOT_FOUND"));
                let detail = errors
                    .iter()
                    .filter_map(|e| e.get("message").and_then(Value::as_str))
                    .collect::<Vec<_>>()
                    .join("; ");
                return Err(if not_found {
                    ReadError::NotFound {
                        what: what.to_string(),
                    }
                } else {
                    ReadError::unreadable(what, clip(&detail))
                });
            }
        }
        value
            .get("data")
            .cloned()
            .filter(|d| !d.is_null())
            .ok_or_else(|| ReadError::unreadable(what, "GraphQL answered no data"))
    }

    fn write(
        &self,
        repo: &RepoSlug,
        what: &str,
        method: &'static str,
        path: &str,
        body: Option<Value>,
        expect: &[u16],
    ) -> Write<Value> {
        let response = self
            .call(repo, method, path, body, None)
            .map_err(|(status, detail)| WriteError {
                what: what.to_string(),
                status,
                detail,
            })?;
        if expect.contains(&response.status) {
            Ok(serde_json::from_slice(&response.body).unwrap_or(Value::Null))
        } else {
            Err(WriteError {
                what: what.to_string(),
                status: Some(response.status),
                detail: message_of(&response),
            })
        }
    }

    fn pull_query(states: &str, base: Option<&str>, first: u32) -> String {
        let base_arg = if base.is_some() {
            ", baseRefName: $base"
        } else {
            ""
        };
        let base_var = if base.is_some() {
            ", $base: String!"
        } else {
            ""
        };
        format!(
            "query($owner: String!, $name: String!, $cursor: String{base_var}) {{
  repository(owner: $owner, name: $name) {{
    pullRequests(states: [{states}], first: {first}, after: $cursor{base_arg}, orderBy: {{field: UPDATED_AT, direction: DESC}}) {{
      totalCount
      pageInfo {{ hasNextPage endCursor }}
      nodes {{ ...Pull }}
    }}
  }}
}}
{PULL_FRAGMENT}"
        )
    }

    /// Walk the pages of a PR connection, newest update first, handing each
    /// PR to `visit` until it answers `false`. Returns GitHub's total count.
    fn pull_pages(
        &self,
        repo: &RepoSlug,
        what: &str,
        states: &str,
        base: Option<&str>,
        first: u32,
        visit: &mut dyn FnMut(PullRequest) -> Result<bool, String>,
    ) -> Read<Option<u64>> {
        let query = Self::pull_query(states, base, first);
        let mut cursor = Value::Null;
        for _ in 0..MAX_PAGES {
            let mut vars = json!({"owner": repo.owner, "name": repo.name, "cursor": cursor});
            if let Some(b) = base {
                vars["base"] = json!(b);
            }
            let data = self.graphql(repo, what, &query, vars)?;
            let conn = &data["repository"]["pullRequests"];
            let (nodes, more) = connection(&data["repository"], "pullRequests")
                .map_err(|e| ReadError::unreadable(what, e))?;
            let total = conn["totalCount"].as_u64();
            for n in nodes {
                let pull = pull_from_gql(n).map_err(|e| ReadError::unreadable(what, e))?;
                if !visit(pull).map_err(|e| ReadError::unreadable(what, e))? {
                    return Ok(total);
                }
            }
            if !more {
                return Ok(total);
            }
            cursor = conn["pageInfo"]["endCursor"].clone();
            if !cursor.is_string() {
                return Err(ReadError::unreadable(what, "a next page without a cursor"));
            }
        }
        Err(ReadError::unreadable(
            what,
            format!("more than {MAX_PAGES} pages: the list would be cut"),
        ))
    }
}

const PULL_FRAGMENT: &str = "fragment Pull on PullRequest {
  number url title body state isDraft
  author { __typename login }
  baseRefName headRefName headRefOid isCrossRepository
  labels(first: 50) { pageInfo { hasNextPage } nodes { name } }
  mergeable mergeStateStatus additions deletions createdAt updatedAt mergedAt
  mergedBy { __typename login }
  mergeCommit { oid }
  reviews(first: 100) { pageInfo { hasNextPage } nodes { author { login } state body commit { oid } submittedAt } }
  reviewThreads(first: 100) { pageInfo { hasNextPage } nodes { isResolved isOutdated path comments(first: 1) { nodes { author { login } body } } } }
  commits(last: 1) { nodes { commit { oid statusCheckRollup { contexts(first: 100) {
    pageInfo { hasNextPage }
    nodes {
      __typename
      ... on CheckRun { name status conclusion startedAt detailsUrl checkSuite { workflowRun { workflow { name } } } }
      ... on StatusContext { context state targetUrl createdAt }
    }
  } } } } }
}";

fn clip(s: &str) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > 200 {
        s.chars().take(200).collect::<String>() + "…"
    } else {
        s
    }
}

fn message_of(response: &HttpResponse) -> String {
    let parsed: Option<Value> = serde_json::from_slice(&response.body).ok();
    let message = parsed
        .as_ref()
        .and_then(|v| v.get("message"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| String::from_utf8_lossy(&response.body).to_string());
    clip(&format!("HTTP {}: {message}", response.status))
}

fn classify_read(what: &str, response: HttpResponse) -> Read<(Value, HttpResponse)> {
    match response.status {
        200..=299 => {
            let value = serde_json::from_slice(&response.body)
                .map_err(|e| ReadError::unreadable(what, format!("the answer is not JSON: {e}")))?;
            Ok((value, response))
        }
        404 => Err(ReadError::NotFound {
            what: what.to_string(),
        }),
        status => Err(ReadError::Unreadable {
            what: what.to_string(),
            status: Some(status),
            detail: message_of(&response),
        }),
    }
}

fn expect_json(response: &HttpResponse, status: u16) -> Result<Value, String> {
    if response.status != status {
        return Err(message_of(response));
    }
    serde_json::from_slice(&response.body).map_err(|e| format!("not JSON: {e}"))
}

/// The `rel="next"` URL of a `Link` header.
fn next_link(link: &str) -> Option<String> {
    link.split(',').find_map(|part| {
        let (url, rel) = part.split_once(';')?;
        rel.contains("rel=\"next\"").then(|| {
            url.trim()
                .trim_start_matches('<')
                .trim_end_matches('>')
                .to_string()
        })
    })
}

/// Percent-encode one path segment (or a path when `keep_slash`).
pub(crate) fn encode(s: &str, keep_slash: bool) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        let c = b as char;
        if c.is_ascii_alphanumeric()
            || matches!(c, '-' | '_' | '.' | '~')
            || (keep_slash && c == '/')
        {
            out.push(c);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn str_of(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn opt_str(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn time_of(v: &Value, key: &str) -> Option<Timestamp> {
    v.get(key)
        .and_then(Value::as_str)
        .and_then(Timestamp::parse)
}

fn object_type(s: &str) -> ObjectType {
    match s {
        "commit" => ObjectType::Commit,
        "tag" => ObjectType::Tag,
        _ => ObjectType::Other,
    }
}

/// GraphQL names an App by its slug; REST and the UI by `<slug>[bot]`. The
/// bridge compares logins in the REST spelling.
fn actor_gql(v: &Value) -> Option<Actor> {
    let login = v.get("login")?.as_str()?.to_string();
    let is_bot = v.get("__typename").and_then(Value::as_str) == Some("Bot");
    let login = if is_bot && !login.ends_with("[bot]") {
        format!("{login}[bot]")
    } else {
        login
    };
    Some(Actor { login, is_bot })
}

fn actor_rest(v: &Value) -> Option<Actor> {
    let login = v.get("login")?.as_str()?.to_string();
    let is_bot = v.get("type").and_then(Value::as_str) == Some("Bot");
    Some(Actor { login, is_bot })
}

/// The nodes of a GraphQL connection and whether GitHub cut it. A
/// connection without `nodes` or without a definite `hasNextPage` is
/// malformed, never empty.
fn connection<'a>(v: &'a Value, field: &str) -> Result<(&'a Vec<Value>, bool), String> {
    let conn = &v[field];
    let nodes = conn["nodes"]
        .as_array()
        .ok_or_else(|| format!("`{field}` carries no node list"))?;
    let more = conn["pageInfo"]["hasNextPage"]
        .as_bool()
        .ok_or_else(|| format!("`{field}` does not say whether it was cut"))?;
    Ok((nodes, more))
}

fn required_u64(v: &Value, key: &str, number: u64) -> Result<u64, String> {
    v[key]
        .as_u64()
        .ok_or_else(|| format!("PR #{number} carries no `{key}`"))
}

/// A PR node of the GraphQL query.
fn pull_from_gql(v: &Value) -> Result<PullRequest, String> {
    let number = v["number"].as_u64().ok_or("a PR without a number")?;
    let mut truncated = Vec::new();
    let (label_nodes, cut) = connection(v, "labels")?;
    if cut {
        truncated.push("labels".to_string());
    }
    let labels = label_nodes.iter().map(|l| str_of(l, "name")).collect();
    let (review_nodes, cut) = connection(v, "reviews")?;
    if cut {
        truncated.push("reviews".to_string());
    }
    let reviews = review_nodes
        .iter()
        .map(|r| Review {
            author: r["author"]["login"].as_str().map(str::to_string),
            body: str_of(r, "body"),
            state: str_of(r, "state"),
            commit_sha: r["commit"]["oid"].as_str().map(str::to_string),
            submitted_at: time_of(r, "submittedAt"),
        })
        .collect();
    let (thread_nodes, cut) = connection(v, "reviewThreads")?;
    if cut {
        truncated.push("reviewThreads".to_string());
    }
    let threads = thread_nodes
        .iter()
        .map(|t| {
            let first = &t["comments"]["nodes"][0];
            ReviewThread {
                resolved: t["isResolved"].as_bool().unwrap_or(false),
                outdated: t["isOutdated"].as_bool().unwrap_or(false),
                path: opt_str(t, "path"),
                first_author: first["author"]["login"].as_str().map(str::to_string),
                first_body: str_of(first, "body"),
            }
        })
        .collect();
    let head_sha = opt_str(v, "headRefOid").unwrap_or_default();
    let last_commit = &v["commits"]["nodes"][0]["commit"];
    let rollup = &last_commit["statusCheckRollup"];
    let mut checks = Vec::new();
    if last_commit["oid"].as_str() != Some(head_sha.as_str()) {
        // The rollup belongs to another commit than the head (a push GitHub
        // has not caught up with): it says nothing about the head.
        truncated.push("checks".to_string());
    } else if !rollup.is_null() {
        let (nodes, cut) = connection(rollup, "contexts")?;
        if cut {
            truncated.push("checks".to_string());
        }
        for c in nodes {
            checks.push(if c["__typename"].as_str() == Some("StatusContext") {
                CheckEntry {
                    kind: CheckKind::Status,
                    name: str_of(c, "context"),
                    workflow: None,
                    status: None,
                    conclusion: opt_str(c, "state"),
                    started_at: time_of(c, "createdAt"),
                    details_url: opt_str(c, "targetUrl"),
                }
            } else {
                CheckEntry {
                    kind: CheckKind::Run,
                    name: str_of(c, "name"),
                    workflow: c["checkSuite"]["workflowRun"]["workflow"]["name"]
                        .as_str()
                        .map(str::to_string),
                    status: opt_str(c, "status"),
                    conclusion: opt_str(c, "conclusion"),
                    started_at: c["startedAt"].as_str().and_then(Timestamp::parse_started),
                    details_url: opt_str(c, "detailsUrl"),
                }
            });
        }
    }
    let state = match v["state"].as_str() {
        Some("OPEN") => PrState::Open,
        Some("MERGED") => PrState::Merged,
        Some("CLOSED") => PrState::Closed,
        other => return Err(format!("PR #{number} has an unknown state {other:?}")),
    };
    let merged_at = time_of(v, "mergedAt");
    if state == PrState::Merged && merged_at.is_none() {
        return Err(format!("PR #{number} is merged but carries no merge time"));
    }
    let mergeable = match v["mergeable"].as_str() {
        Some("MERGEABLE") => Mergeable::Mergeable,
        Some("CONFLICTING") => Mergeable::Conflicting,
        _ => Mergeable::Unknown,
    };
    Ok(PullRequest {
        number,
        url: str_of(v, "url"),
        title: str_of(v, "title"),
        body: str_of(v, "body"),
        state,
        draft: v["isDraft"].as_bool().unwrap_or(false),
        author: actor_gql(&v["author"]),
        base_ref: str_of(v, "baseRefName"),
        head_ref: str_of(v, "headRefName"),
        cross_repository: v["isCrossRepository"].as_bool().unwrap_or(true),
        head_sha,
        labels,
        mergeable,
        merge_state: str_of(v, "mergeStateStatus"),
        additions: required_u64(v, "additions", number)?,
        deletions: required_u64(v, "deletions", number)?,
        created_at: time_of(v, "createdAt"),
        updated_at: time_of(v, "updatedAt"),
        merged_at,
        merged_by: actor_gql(&v["mergedBy"]),
        merge_commit_sha: v["mergeCommit"]["oid"].as_str().map(str::to_string),
        checks,
        reviews,
        threads,
        truncated,
    })
}

/// Every element mapped, or the read refused at the first malformed one.
fn all<T>(what: &str, items: &[Value], f: impl Fn(&Value) -> Option<T>) -> Read<Vec<T>> {
    items
        .iter()
        .map(|v| f(v).ok_or_else(|| ReadError::unreadable(what, "a malformed entry in the list")))
        .collect()
}

/// A ruleset's own page; `None` when a field the judgement needs is absent
/// or of the wrong type (a rule without a type, a pattern that is no text).
fn ruleset_of(v: &Value) -> Option<Ruleset> {
    let patterns = |key: &str| -> Option<Vec<String>> {
        v.get("conditions")?
            .get("ref_name")?
            .get(key)?
            .as_array()?
            .iter()
            .map(|p| p.as_str().map(str::to_string))
            .collect()
    };
    let mut rules = BTreeSet::new();
    let mut required_checks = BTreeSet::new();
    let listed = v.get("rules")?.as_array()?;
    for rule in listed {
        let kind = rule["type"].as_str()?;
        if kind == "required_status_checks" {
            for c in rule
                .get("parameters")?
                .get("required_status_checks")?
                .as_array()?
            {
                required_checks.insert(c["context"].as_str()?.to_string());
            }
        }
        rules.insert(kind.to_string());
    }
    Some(Ruleset {
        id: v["id"].as_u64()?,
        name: v["name"].as_str()?.to_string(),
        target: v["target"].as_str()?.to_string(),
        enforcement: v["enforcement"].as_str()?.to_string(),
        include: patterns("include")?,
        exclude: patterns("exclude")?,
        rules,
        required_checks,
    })
}

fn matching_refs(repo: &RepoSlug, kind: &str, prefix: &str) -> String {
    if prefix.is_empty() {
        format!("/repos/{repo}/git/matching-refs/{kind}?per_page=100")
    } else {
        format!(
            "/repos/{repo}/git/matching-refs/{kind}/{}?per_page=100",
            encode(prefix, true)
        )
    }
}

fn git_ref(v: &Value, prefix: &str) -> Option<GitRef> {
    let full = v.get("ref")?.as_str()?;
    Some(GitRef {
        name: full.strip_prefix(prefix)?.to_string(),
        sha: v["object"]["sha"].as_str()?.to_string(),
        object_type: object_type(v["object"]["type"].as_str().unwrap_or("")),
    })
}

impl GitHub for RestGitHub {
    fn identity(&self) -> Read<Identity> {
        match &self.credentials {
            Credentials::App { app_id, .. } => {
                let jwt = self
                    .jwt()
                    .map_err(|e| ReadError::unreadable("the GitHub App", e))?;
                let response = self
                    .raw("GET", "/app", &jwt, None, None)
                    .map_err(|e| ReadError::unreadable("the GitHub App", e))?;
                let (v, _) = classify_read("the GitHub App", response)?;
                Ok(Identity::App {
                    slug: str_of(&v, "slug"),
                    app_id: v["id"]
                        .as_u64()
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| app_id.clone()),
                })
            }
            Credentials::Token(token) => {
                let response = self
                    .raw("GET", "/user", token, None, None)
                    .map_err(|e| ReadError::unreadable("the token's user", e))?;
                let (v, _) = classify_read("the token's user", response)?;
                Ok(Identity::User {
                    login: str_of(&v, "login"),
                })
            }
        }
    }

    fn default_branch(&self, repo: &RepoSlug) -> Read<String> {
        let what = format!("repository {repo}");
        let (v, _) = self.get_json(repo, &what, &format!("/repos/{repo}"))?;
        opt_str(&v, "default_branch")
            .ok_or_else(|| ReadError::unreadable(what, "no default branch in the answer"))
    }

    fn branches(&self, repo: &RepoSlug, prefix: &str) -> Read<Vec<GitRef>> {
        let what = format!("branches {prefix}* of {repo}");
        let items = self.get_pages(
            repo,
            &what,
            &matching_refs(repo, "heads", prefix),
            None,
            10_000,
        )?;
        all(&what, &items, |v| git_ref(v, "refs/heads/"))
    }

    fn tags(&self, repo: &RepoSlug, prefix: &str) -> Read<Vec<GitRef>> {
        let what = format!("tags {prefix}* of {repo}");
        let items = self.get_pages(
            repo,
            &what,
            &matching_refs(repo, "tags", prefix),
            None,
            100_000,
        )?;
        all(&what, &items, |v| git_ref(v, "refs/tags/"))
    }

    fn tag_object(&self, repo: &RepoSlug, sha: &str) -> Read<AnnotatedTag> {
        let what = format!("tag object {sha} of {repo}");
        let (v, _) = self.get_json(
            repo,
            &what,
            &format!("/repos/{repo}/git/tags/{}", encode(sha, false)),
        )?;
        Ok(AnnotatedTag {
            name: str_of(&v, "tag"),
            message: str_of(&v, "message"),
            tagger_date: time_of(&v["tagger"], "date"),
            target_sha: v["object"]["sha"]
                .as_str()
                .ok_or_else(|| ReadError::unreadable(&what, "no target object"))?
                .to_string(),
            target_type: object_type(v["object"]["type"].as_str().unwrap_or("")),
        })
    }

    fn commit(&self, repo: &RepoSlug, rev: &str) -> Read<Commit> {
        let what = format!("commit {rev} of {repo}");
        let (v, _) = self.get_json(
            repo,
            &what,
            &format!("/repos/{repo}/commits/{}", encode(rev, true)),
        )?;
        let sha = opt_str(&v, "sha").ok_or_else(|| ReadError::unreadable(&what, "no sha"))?;
        Ok(Commit {
            sha,
            message: str_of(&v["commit"], "message"),
            committer_date: time_of(&v["commit"]["committer"], "date"),
            parents: v["parents"]
                .as_array()
                .ok_or_else(|| ReadError::unreadable(&what, "no parent list in the answer"))?
                .iter()
                .map(|p| str_of(p, "sha"))
                .collect(),
        })
    }

    fn compare(&self, repo: &RepoSlug, base: &str, head: &str) -> Read<Comparison> {
        let what = format!("{base}...{head} in {repo}");
        let path = format!(
            "/repos/{repo}/compare/{}...{}?per_page=100",
            encode(base, true),
            encode(head, true)
        );
        let (first, _) = self.get_json(repo, &what, &path)?;
        let status = match first["status"].as_str() {
            Some("ahead") => CompareStatus::Ahead,
            Some("behind") => CompareStatus::Behind,
            Some("identical") => CompareStatus::Identical,
            Some("diverged") => CompareStatus::Diverged,
            other => {
                return Err(ReadError::unreadable(
                    &what,
                    format!("unknown comparison status {other:?}"),
                ));
            }
        };
        let total = first["total_commits"]
            .as_u64()
            .ok_or_else(|| ReadError::unreadable(&what, "no commit count in the answer"))?;
        let summary = |c: &Value| CommitSummary {
            sha: str_of(c, "sha"),
            subject: str_of(&c["commit"], "message")
                .lines()
                .next()
                .unwrap_or("")
                .to_string(),
        };
        let mut commits: Vec<CommitSummary> = first["commits"]
            .as_array()
            .map(|a| a.iter().map(summary).collect())
            .unwrap_or_default();
        let mut page = 2;
        while (commits.len() as u64) < total {
            let (v, _) = self.get_json(repo, &what, &format!("{path}&page={page}"))?;
            let more: Vec<CommitSummary> = v["commits"]
                .as_array()
                .map(|a| a.iter().map(summary).collect())
                .unwrap_or_default();
            if more.is_empty() {
                return Err(ReadError::unreadable(
                    &what,
                    format!("GitHub listed {} of {total} commits", commits.len()),
                ));
            }
            commits.extend(more);
            page += 1;
        }
        let files: Vec<String> = first["files"]
            .as_array()
            .ok_or_else(|| ReadError::unreadable(&what, "no file list in the answer"))?
            .iter()
            .map(|f| str_of(f, "filename"))
            .collect();
        if files.len() >= MAX_COMPARE_FILES {
            return Err(ReadError::unreadable(
                &what,
                format!(
                    "{} files or more: GitHub cuts the list at {MAX_COMPARE_FILES}",
                    files.len()
                ),
            ));
        }
        Ok(Comparison {
            status,
            ahead_by: first["ahead_by"].as_u64().unwrap_or(0),
            behind_by: first["behind_by"].as_u64().unwrap_or(0),
            commits,
            files,
        })
    }

    fn file(&self, repo: &RepoSlug, path: &str, rev: &str) -> Read<String> {
        let what = format!("{path} at {rev} in {repo}");
        let response = self
            .call(
                repo,
                "GET",
                &format!(
                    "/repos/{repo}/contents/{}?ref={}",
                    encode(path, true),
                    encode(rev, false)
                ),
                None,
                Some("application/vnd.github.raw+json"),
            )
            .map_err(|(status, detail)| ReadError::Unreadable {
                what: what.clone(),
                status,
                detail,
            })?;
        match response.status {
            200 => String::from_utf8(response.body)
                .map_err(|_| ReadError::unreadable(&what, "the file is not UTF-8 text")),
            404 => Err(ReadError::NotFound { what }),
            status => Err(ReadError::Unreadable {
                what,
                status: Some(status),
                detail: message_of(&response),
            }),
        }
    }

    fn open_pulls(&self, repo: &RepoSlug, base: Option<&str>) -> Read<Vec<PullRequest>> {
        let what = format!("open PRs of {repo}");
        let mut out: Vec<PullRequest> = Vec::new();
        let total = self.pull_pages(repo, &what, "OPEN", base, 50, &mut |pull| {
            if !out.iter().any(|p| p.number == pull.number) {
                out.push(pull);
            }
            if out.len() > MAX_OPEN_PULLS {
                return Err(format!("more than {MAX_OPEN_PULLS} open PRs"));
            }
            Ok(true)
        })?;
        // Pages are offsets of a moving list: a PR updated between two pages
        // shifts the others, and one can be skipped. The count says so.
        if total != Some(out.len() as u64) {
            return Err(ReadError::unreadable(
                &what,
                format!(
                    "the list moved while it was read ({} read, GitHub counts {})",
                    out.len(),
                    total
                        .map(|t| t.to_string())
                        .unwrap_or_else(|| "nothing".into())
                ),
            ));
        }
        Ok(out)
    }

    fn pull(&self, repo: &RepoSlug, number: u64) -> Read<PullRequest> {
        let what = format!("PR #{number} of {repo}");
        let query = format!(
            "query($owner: String!, $name: String!, $number: Int!) {{
  repository(owner: $owner, name: $name) {{ pullRequest(number: $number) {{ ...Pull }} }}
}}
{PULL_FRAGMENT}"
        );
        let data = self.graphql(
            repo,
            &what,
            &query,
            json!({"owner": repo.owner, "name": repo.name, "number": number}),
        )?;
        let node = &data["repository"]["pullRequest"];
        if node.is_null() {
            return Err(ReadError::NotFound { what });
        }
        pull_from_gql(node).map_err(|e| ReadError::unreadable(what, e))
    }

    fn merged_pulls(
        &self,
        repo: &RepoSlug,
        base: Option<&str>,
        since: Option<Timestamp>,
        limit: usize,
    ) -> Read<Vec<PullRequest>> {
        let what = format!(
            "PRs merged{} of {repo}",
            base.map(|b| format!(" into {b}")).unwrap_or_default()
        );
        let mut out: Vec<PullRequest> = Vec::new();
        self.pull_pages(repo, &what, "MERGED", base, 50, &mut |pull| {
            // Ordered by update time: once a PR was last updated before the
            // window, every later one was too (a merge updates a PR).
            if let (Some(since), Some(updated)) = (since, pull.updated_at) {
                if updated < since {
                    return Ok(false);
                }
            }
            let in_window = match (since, pull.merged_at) {
                (Some(since), Some(merged)) => merged >= since,
                _ => true,
            };
            if in_window && !out.iter().any(|p| p.number == pull.number) {
                out.push(pull);
            }
            if out.len() > limit {
                return Err(format!(
                    "more than {limit} merged PRs: the list would be cut"
                ));
            }
            Ok(true)
        })?;
        Ok(out)
    }

    fn closed_pulls(
        &self,
        repo: &RepoSlug,
        since: Timestamp,
        limit: usize,
    ) -> Read<Vec<PullRequest>> {
        let what = format!("PRs closed without merge of {repo}");
        let mut out: Vec<PullRequest> = Vec::new();
        // Small pages: the full fragment on 50 closed PRs takes GitHub close
        // to its timeout (a 502), and the window usually ends on the first page.
        self.pull_pages(repo, &what, "CLOSED", None, 10, &mut |pull| {
            // Newest update first: past the window, every later one is too.
            if pull.updated_at.is_some_and(|u| u < since) {
                return Ok(false);
            }
            if !out.iter().any(|p| p.number == pull.number) {
                out.push(pull);
            }
            if out.len() > limit {
                return Err(format!(
                    "more than {limit} closed PRs: the list would be cut"
                ));
            }
            Ok(true)
        })?;
        Ok(out)
    }

    fn pull_files(&self, repo: &RepoSlug, number: u64) -> Read<Vec<String>> {
        let what = format!("files of PR #{number} of {repo}");
        let items = self.get_pages(
            repo,
            &what,
            &format!("/repos/{repo}/pulls/{number}/files?per_page=100"),
            None,
            MAX_PULL_FILES - 1,
        )?;
        // A rename names both paths: moving a file out of a sensitive
        // directory touches that directory.
        let mut out = Vec::new();
        for f in &items {
            let name = opt_str(f, "filename")
                .ok_or_else(|| ReadError::unreadable(&what, "a file entry without a name"))?;
            if let Some(previous) = opt_str(f, "previous_filename") {
                out.push(previous);
            }
            out.push(name);
        }
        Ok(out)
    }

    fn commit_pulls(&self, repo: &RepoSlug, sha: &str) -> Read<Vec<CommitPull>> {
        let what = format!("PRs of commit {sha} in {repo}");
        let items = self.get_pages(
            repo,
            &what,
            &format!(
                "/repos/{repo}/commits/{}/pulls?per_page=100",
                encode(sha, false)
            ),
            None,
            1000,
        )?;
        all(&what, &items, |p| {
            Some(CommitPull {
                number: p["number"].as_u64()?,
                url: str_of(p, "html_url"),
                merged_at: time_of(p, "merged_at"),
                merge_commit_sha: opt_str(p, "merge_commit_sha"),
                base_ref: str_of(&p["base"], "ref"),
                head_ref: str_of(&p["head"], "ref"),
            })
        })
    }

    fn required_checks(&self, repo: &RepoSlug, branch: &str) -> Read<RequiredChecks> {
        let what = format!("required checks of {branch} in {repo}");
        let mut out = RequiredChecks::default();
        // Classic protection: 404 is an answer ("no protection rule").
        match self.get_json(
            repo,
            &what,
            &format!(
                "/repos/{repo}/branches/{}/protection/required_status_checks",
                encode(branch, false)
            ),
        ) {
            Ok((v, _)) => {
                out.strict |= v["strict"].as_bool().unwrap_or(false);
                for c in v["contexts"].as_array().into_iter().flatten() {
                    if let Some(c) = c.as_str() {
                        out.contexts.insert(c.to_string());
                    }
                }
                for c in v["checks"].as_array().into_iter().flatten() {
                    if let Some(c) = c["context"].as_str() {
                        out.contexts.insert(c.to_string());
                    }
                }
            }
            Err(ReadError::NotFound { .. }) => {}
            Err(e) => return Err(e),
        }
        // Rulesets: GitHub resolves which active rules apply to the branch.
        let rules = self.get_pages(
            repo,
            &what,
            &format!(
                "/repos/{repo}/rules/branches/{}?per_page=100",
                encode(branch, false)
            ),
            None,
            10_000,
        );
        let rules = match rules {
            Ok(r) => r,
            Err(ReadError::NotFound { .. }) => {
                return Err(ReadError::unreadable(
                    &what,
                    "the rules of the branch: not found",
                ));
            }
            Err(e) => return Err(e),
        };
        for rule in rules {
            if rule["type"].as_str() != Some("required_status_checks") {
                continue;
            }
            let params = &rule["parameters"];
            out.strict |= params["strict_required_status_checks_policy"]
                .as_bool()
                .unwrap_or(false);
            for c in params["required_status_checks"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if let Some(c) = c["context"].as_str() {
                    out.contexts.insert(c.to_string());
                }
            }
        }
        Ok(out)
    }

    fn rulesets(&self, repo: &RepoSlug) -> Read<Vec<Ruleset>> {
        let what = format!("rulesets of {repo}");
        // The list names each ruleset; only its own page carries the
        // conditions and the rules. A 404 is no answer about rulesets.
        let not_found = |e: ReadError, of: &str| match e {
            ReadError::NotFound { .. } => ReadError::unreadable(&what, format!("{of}: not found")),
            e => e,
        };
        let listed = self
            .get_pages(
                repo,
                &what,
                &format!("/repos/{repo}/rulesets?includes_parents=true&per_page=100"),
                None,
                1000,
            )
            .map_err(|e| not_found(e, "the list"))?;
        let mut out = Vec::new();
        for entry in &listed {
            let id = entry["id"]
                .as_u64()
                .ok_or_else(|| ReadError::unreadable(&what, "a ruleset without an id"))?;
            let (v, _) = self
                .get_json(
                    repo,
                    &what,
                    &format!("/repos/{repo}/rulesets/{id}?includes_parents=true"),
                )
                .map_err(|e| not_found(e, &format!("ruleset {id}")))?;
            out.push(ruleset_of(&v).ok_or_else(|| {
                ReadError::unreadable(&what, format!("ruleset {id}: a malformed answer"))
            })?);
        }
        Ok(out)
    }

    fn issue_comments(&self, repo: &RepoSlug, number: u64) -> Read<Vec<Comment>> {
        let what = format!("comments of #{number} of {repo}");
        let items = self.get_pages(
            repo,
            &what,
            &format!("/repos/{repo}/issues/{number}/comments?per_page=100"),
            None,
            10_000,
        )?;
        all(&what, &items, |c| {
            Some(Comment {
                id: c["id"].as_u64()?,
                author: actor_rest(&c["user"]),
                body: str_of(c, "body"),
                created_at: time_of(c, "created_at"),
                url: str_of(c, "html_url"),
            })
        })
    }

    fn release(&self, repo: &RepoSlug, tag: &str) -> Read<Release> {
        let what = format!("release {tag} of {repo}");
        let (v, _) = self.get_json(
            repo,
            &what,
            &format!("/repos/{repo}/releases/tags/{}", encode(tag, false)),
        )?;
        Ok(Release {
            tag: str_of(&v, "tag_name"),
            url: str_of(&v, "html_url"),
            body: str_of(&v, "body"),
            draft: v["draft"].as_bool().unwrap_or(false),
            prerelease: v["prerelease"].as_bool().unwrap_or(false),
            published_at: time_of(&v, "published_at"),
        })
    }

    fn workflow_runs(
        &self,
        repo: &RepoSlug,
        workflow: &str,
        branch: Option<&str>,
        head_sha: Option<&str>,
        event: Option<&str>,
    ) -> Read<Vec<WorkflowRun>> {
        let what = format!("runs of {workflow} in {repo}");
        let mut path = format!(
            "/repos/{repo}/actions/workflows/{}/runs?per_page=100",
            encode(workflow, false)
        );
        for (k, v) in [("branch", branch), ("head_sha", head_sha), ("event", event)] {
            if let Some(v) = v {
                path.push_str(&format!("&{k}={}", encode(v, false)));
            }
        }
        let (v, _) = self.get_json(repo, &what, &path)?;
        let runs = v["workflow_runs"]
            .as_array()
            .ok_or_else(|| ReadError::unreadable(&what, "no run list in the answer"))?;
        all(&what, runs, |r| {
            Some(WorkflowRun {
                id: r["id"].as_u64()?,
                head_sha: str_of(r, "head_sha"),
                head_branch: str_of(r, "head_branch"),
                event: str_of(r, "event"),
                status: str_of(r, "status"),
                conclusion: opt_str(r, "conclusion"),
                // Absent reads as the first attempt: a rerun is then
                // still awaited, never presumed spent.
                attempt: r["run_attempt"].as_u64().unwrap_or(1),
                url: str_of(r, "html_url"),
                created_at: time_of(r, "created_at"),
                updated_at: time_of(r, "updated_at"),
            })
        })
    }

    fn run_jobs(&self, repo: &RepoSlug, run_id: u64) -> Read<Vec<Job>> {
        let what = format!("jobs of run {run_id} of {repo}");
        let items = self.get_pages(
            repo,
            &what,
            &format!("/repos/{repo}/actions/runs/{run_id}/jobs?per_page=100"),
            Some("jobs"),
            10_000,
        )?;
        Ok(items
            .iter()
            .map(|j| Job {
                name: str_of(j, "name"),
                conclusion: opt_str(j, "conclusion"),
            })
            .collect())
    }

    fn run_artifacts(&self, repo: &RepoSlug, run_id: u64) -> Read<Vec<Artifact>> {
        let what = format!("artifacts of run {run_id} of {repo}");
        let items = self.get_pages(
            repo,
            &what,
            &format!("/repos/{repo}/actions/runs/{run_id}/artifacts?per_page=100"),
            Some("artifacts"),
            10_000,
        )?;
        all(&what, &items, |a| {
            Some(Artifact {
                id: a["id"].as_u64()?,
                name: str_of(a, "name"),
                expired: a["expired"].as_bool().unwrap_or(false),
            })
        })
    }

    fn artifact_file(&self, repo: &RepoSlug, artifact_id: u64, path: &str) -> Read<String> {
        let what = format!("{path} in artifact {artifact_id} of {repo}");
        let unreadable = |status: Option<u16>, detail: String| ReadError::Unreadable {
            what: what.clone(),
            status,
            detail,
        };
        let mut response = self
            .call(
                repo,
                "GET",
                &format!("/repos/{repo}/actions/artifacts/{artifact_id}/zip"),
                None,
                None,
            )
            .map_err(|(status, detail)| unreadable(status, detail))?;
        // GitHub answers with a short-lived link to the zip: followed
        // without the token, which goes to the API host and nowhere else.
        if matches!(response.status, 301 | 302 | 303 | 307 | 308) {
            let location = response
                .header("location")
                .ok_or_else(|| {
                    unreadable(Some(response.status), "a redirect with no location".into())
                })?
                .to_string();
            response = self
                .transport
                .send(HttpRequest {
                    method: "GET",
                    url: location,
                    headers: vec![(
                        "User-Agent".to_string(),
                        concat!("spl-asdlc/", env!("CARGO_PKG_VERSION")).to_string(),
                    )],
                    body: None,
                })
                .map_err(|e| unreadable(None, e))?;
        }
        match response.status {
            200 => {}
            // The artifact itself is gone (deleted, or expired and purged).
            404 | 410 => {
                return Err(unreadable(
                    Some(response.status),
                    "the artifact could not be downloaded".into(),
                ));
            }
            status => return Err(unreadable(Some(status), message_of(&response))),
        }
        match super::zip::extract(&response.body, path, MAX_ARTIFACT_FILE) {
            Ok(bytes) => String::from_utf8(bytes)
                .map_err(|_| ReadError::unreadable(&what, "the file is not UTF-8 text")),
            Err(super::zip::ZipError::Absent) => Err(ReadError::NotFound { what }),
            Err(super::zip::ZipError::Unreadable(e)) => Err(ReadError::unreadable(&what, e)),
        }
    }

    fn merge_pull(
        &self,
        repo: &RepoSlug,
        number: u64,
        head_sha: &str,
        method: MergeMethod,
    ) -> Write<String> {
        let v = self.write(
            repo,
            &format!("merge of PR #{number}"),
            "PUT",
            &format!("/repos/{repo}/pulls/{number}/merge"),
            Some(json!({"sha": head_sha, "merge_method": method.as_str()})),
            &[200],
        )?;
        match (v["merged"].as_bool(), opt_str(&v, "sha")) {
            (Some(true), Some(sha)) => Ok(sha),
            _ => Err(WriteError {
                what: format!("merge of PR #{number}"),
                status: Some(200),
                detail: "GitHub did not confirm the merge".into(),
            }),
        }
    }

    fn update_branch(&self, repo: &RepoSlug, number: u64, head_sha: &str) -> Write<()> {
        self.write(
            repo,
            &format!("update of the branch of PR #{number}"),
            "PUT",
            &format!("/repos/{repo}/pulls/{number}/update-branch"),
            Some(json!({"expected_head_sha": head_sha})),
            &[202],
        )
        .map(|_| ())
    }

    fn delete_branch(&self, repo: &RepoSlug, branch: &str) -> Write<()> {
        self.write(
            repo,
            &format!("deletion of branch {branch}"),
            "DELETE",
            &format!("/repos/{repo}/git/refs/heads/{}", encode(branch, true)),
            None,
            &[204],
        )
        .map(|_| ())
    }

    fn create_branch(&self, repo: &RepoSlug, branch: &str, sha: &str) -> Write<()> {
        self.write(
            repo,
            &format!("creation of branch {branch}"),
            "POST",
            &format!("/repos/{repo}/git/refs"),
            Some(json!({"ref": format!("refs/heads/{branch}"), "sha": sha})),
            &[201],
        )
        .map(|_| ())
    }

    fn add_labels(&self, repo: &RepoSlug, number: u64, labels: &[String]) -> Write<()> {
        self.write(
            repo,
            &format!("labels on #{number}"),
            "POST",
            &format!("/repos/{repo}/issues/{number}/labels"),
            Some(json!({"labels": labels})),
            &[200],
        )
        .map(|_| ())
    }

    fn remove_label(&self, repo: &RepoSlug, number: u64, label: &str) -> Write<()> {
        self.write(
            repo,
            &format!("removal of label {label} from #{number}"),
            "DELETE",
            &format!(
                "/repos/{repo}/issues/{number}/labels/{}",
                encode(label, false)
            ),
            None,
            &[200],
        )
        .map(|_| ())
    }

    fn comment(&self, repo: &RepoSlug, number: u64, body: &str) -> Write<Comment> {
        let v = self.write(
            repo,
            &format!("comment on #{number}"),
            "POST",
            &format!("/repos/{repo}/issues/{number}/comments"),
            Some(json!({"body": body})),
            &[201],
        )?;
        let id = v["id"].as_u64().ok_or_else(|| WriteError {
            what: format!("comment on #{number}"),
            status: Some(201),
            detail: "GitHub answered no comment id".into(),
        })?;
        Ok(Comment {
            id,
            author: actor_rest(&v["user"]),
            body: str_of(&v, "body"),
            created_at: time_of(&v, "created_at"),
            url: str_of(&v, "html_url"),
        })
    }

    fn update_comment(&self, repo: &RepoSlug, comment_id: u64, body: &str) -> Write<()> {
        self.write(
            repo,
            &format!("update of comment {comment_id}"),
            "PATCH",
            &format!("/repos/{repo}/issues/comments/{comment_id}"),
            Some(json!({"body": body})),
            &[200],
        )
        .map(|_| ())
    }

    fn put_file(
        &self,
        repo: &RepoSlug,
        branch: &str,
        path: &str,
        content: &str,
        message: &str,
    ) -> Write<String> {
        let what = format!("{path} on {branch}");
        // Replacing a file needs the blob it replaces.
        let existing = match self.call(
            repo,
            "GET",
            &format!(
                "/repos/{repo}/contents/{}?ref={}",
                encode(path, true),
                encode(branch, false)
            ),
            None,
            None,
        ) {
            Ok(r) if r.status == 200 => serde_json::from_slice::<Value>(&r.body)
                .ok()
                .and_then(|v| opt_str(&v, "sha")),
            Ok(r) if r.status == 404 => None,
            Ok(r) => {
                return Err(WriteError {
                    what,
                    status: Some(r.status),
                    detail: message_of(&r),
                });
            }
            Err((status, detail)) => {
                return Err(WriteError {
                    what,
                    status,
                    detail,
                });
            }
        };
        let mut body = json!({
            "message": message,
            "content": base64::engine::general_purpose::STANDARD.encode(content),
            "branch": branch,
        });
        if let Some(sha) = existing {
            body["sha"] = json!(sha);
        }
        let v = self.write(
            repo,
            &what,
            "PUT",
            &format!("/repos/{repo}/contents/{}", encode(path, true)),
            Some(body),
            &[200, 201],
        )?;
        Ok(str_of(&v["commit"], "sha"))
    }

    fn create_pull(
        &self,
        repo: &RepoSlug,
        head: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> Write<u64> {
        let v = self.write(
            repo,
            &format!("PR {head} -> {base}"),
            "POST",
            &format!("/repos/{repo}/pulls"),
            Some(json!({"head": head, "base": base, "title": title, "body": body})),
            &[201],
        )?;
        v["number"].as_u64().ok_or_else(|| WriteError {
            what: format!("PR {head} -> {base}"),
            status: Some(201),
            detail: "GitHub answered no PR number".into(),
        })
    }

    fn dispatch_workflow(
        &self,
        repo: &RepoSlug,
        workflow: &str,
        git_ref: &str,
        inputs: &[(String, String)],
    ) -> Write<()> {
        let inputs: serde_json::Map<String, Value> =
            inputs.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
        self.write(
            repo,
            &format!("dispatch of {workflow} on {git_ref}"),
            "POST",
            &format!(
                "/repos/{repo}/actions/workflows/{}/dispatches",
                encode(workflow, false)
            ),
            Some(json!({"ref": git_ref, "inputs": inputs})),
            &[204],
        )
        .map(|_| ())
    }

    fn request_reviewers(&self, repo: &RepoSlug, number: u64, logins: &[String]) -> Write<()> {
        self.write(
            repo,
            &format!("review request on PR #{number}"),
            "POST",
            &format!("/repos/{repo}/pulls/{number}/requested_reviewers"),
            Some(json!({"reviewers": logins})),
            &[201],
        )
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests;
