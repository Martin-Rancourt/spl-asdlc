//! The facts the bridge reads on GitHub, and the outcomes of its writes.
//!
//! These are plain data: judging them (green or not, approved or not, a
//! merge-back or not) is the `method` module's job.

use std::collections::BTreeSet;
use std::fmt;

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// `owner/name` of a repository.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RepoSlug {
    pub owner: String,
    pub name: String,
}

impl RepoSlug {
    pub fn parse(s: &str) -> Option<RepoSlug> {
        let (owner, name) = s.trim().split_once('/')?;
        let ok = |p: &str| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        };
        (ok(owner) && ok(name)).then(|| RepoSlug {
            owner: owner.to_string(),
            name: name.to_string(),
        })
    }
}

impl fmt::Display for RepoSlug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

/// An instant as GitHub reports it (RFC 3339).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(pub OffsetDateTime);

impl Timestamp {
    pub fn parse(s: &str) -> Option<Timestamp> {
        OffsetDateTime::parse(s, &Rfc3339).ok().map(Timestamp)
    }

    /// GitHub's "not started" placeholder (`0001-01-01T00:00:00Z`) reads as `None`.
    pub fn parse_started(s: &str) -> Option<Timestamp> {
        Timestamp::parse(s).filter(|t| t.0.year() > 1)
    }

    pub fn now() -> Timestamp {
        Timestamp(OffsetDateTime::now_utc())
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.format(&Rfc3339) {
            Ok(s) => f.write_str(&s),
            Err(_) => write!(f, "{:?}", self.0),
        }
    }
}

/// A read that did not return a trustworthy answer is never "nothing there".
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    /// GitHub answered 404: the object does not exist (or is invisible to
    /// this identity, which GitHub reports the same way).
    #[error("{what}: not found")]
    NotFound { what: String },
    /// Anything else: an HTTP failure, a malformed answer, a cut list.
    #[error("{what} could not be read: {detail}")]
    Unreadable {
        what: String,
        status: Option<u16>,
        detail: String,
    },
}

impl ReadError {
    pub fn unreadable(what: impl Into<String>, detail: impl Into<String>) -> ReadError {
        ReadError::Unreadable {
            what: what.into(),
            status: None,
            detail: detail.into(),
        }
    }

    pub fn is_not_found(&self) -> bool {
        matches!(self, ReadError::NotFound { .. })
    }
}

pub type Read<T> = Result<T, ReadError>;

/// A write GitHub did not perform, or whose outcome is unknown: with no
/// status (the transport failed after sending) or a 5xx, GitHub may have
/// performed it; the caller re-reads before concluding (REQ-MERGE-058).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{what} refused{}: {detail}", status.map(|s| format!(" (HTTP {s})")).unwrap_or_default())]
pub struct WriteError {
    pub what: String,
    pub status: Option<u16>,
    pub detail: String,
}

pub type Write<T> = Result<T, WriteError>;

/// Who the bridge writes as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    /// A GitHub App; writes appear as `<slug>[bot]`.
    App { slug: String, app_id: String },
    /// A personal token's user.
    User { login: String },
}

impl Identity {
    /// The login GitHub shows on what this identity writes.
    pub fn login(&self) -> String {
        match self {
            Identity::App { slug, .. } => format!("{slug}[bot]"),
            Identity::User { login } => login.clone(),
        }
    }
}

impl fmt::Display for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Identity::App { slug, app_id } => {
                write!(
                    f,
                    "GitHub App {slug} (id {app_id}), writing as {}",
                    self.login()
                )
            }
            Identity::User { login } => write!(f, "personal token of {login}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub login: String,
    pub is_bot: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectType {
    Commit,
    Tag,
    Other,
}

/// A ref and the object it points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRef {
    /// Short name: `release/2026-10-05-14-03-utc`, `acme-v1.2.3-rc.1`.
    pub name: String,
    pub sha: String,
    pub object_type: ObjectType,
}

/// An annotated tag object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnotatedTag {
    pub name: String,
    pub message: String,
    pub tagger_date: Option<Timestamp>,
    /// The object the tag points at (a commit for an RC tag).
    pub target_sha: String,
    pub target_type: ObjectType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub sha: String,
    pub message: String,
    pub committer_date: Option<Timestamp>,
    pub parents: Vec<String>,
}

impl Commit {
    pub fn subject(&self) -> &str {
        self.message.lines().next().unwrap_or("")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitSummary {
    pub sha: String,
    pub subject: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareStatus {
    /// `head` contains `base` and more.
    Ahead,
    /// `base` contains `head` and more.
    Behind,
    Identical,
    Diverged,
}

/// `base...head` as GitHub's compare answers it, complete or refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comparison {
    pub status: CompareStatus,
    pub ahead_by: u64,
    pub behind_by: u64,
    /// Commits of `base..head`, oldest first.
    pub commits: Vec<CommitSummary>,
    /// Files changed between the merge base and `head`.
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrState {
    Open,
    Closed,
    Merged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mergeable {
    Mergeable,
    Conflicting,
    Unknown,
}

/// One entry of a commit's check rollup: a check run or a commit status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckEntry {
    pub kind: CheckKind,
    /// Check-run name or status context.
    pub name: String,
    /// Workflow of a check run, when it belongs to one.
    pub workflow: Option<String>,
    /// Check-run status (`QUEUED`, `IN_PROGRESS`, `COMPLETED`, …).
    pub status: Option<String>,
    /// Check-run conclusion, or commit-status state.
    pub conclusion: Option<String>,
    pub started_at: Option<Timestamp>,
    pub details_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckKind {
    Run,
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    pub author: Option<String>,
    /// The review's own text (an angle names its sub-item there).
    pub body: String,
    /// `APPROVED`, `CHANGES_REQUESTED`, `COMMENTED`, `DISMISSED`, `PENDING`.
    pub state: String,
    pub commit_sha: Option<String>,
    pub submitted_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewThread {
    pub resolved: bool,
    pub outdated: bool,
    pub path: Option<String>,
    pub first_author: Option<String>,
    pub first_body: String,
}

/// A pull request with everything a tick judges it on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequest {
    pub number: u64,
    pub url: String,
    pub title: String,
    pub body: String,
    pub state: PrState,
    pub draft: bool,
    pub author: Option<Actor>,
    pub base_ref: String,
    /// Empty when GitHub did not say (treated as a merge-back by the method).
    pub head_ref: String,
    /// The head branch lives in another repository (a fork). Unknown reads
    /// as true: the bridge never deletes a branch it cannot place.
    pub cross_repository: bool,
    pub head_sha: String,
    pub labels: Vec<String>,
    pub mergeable: Mergeable,
    /// `mergeStateStatus`: `CLEAN`, `BEHIND`, `DIRTY`, `BLOCKED`, `UNSTABLE`, …
    pub merge_state: String,
    pub additions: u64,
    pub deletions: u64,
    pub created_at: Option<Timestamp>,
    pub updated_at: Option<Timestamp>,
    pub merged_at: Option<Timestamp>,
    pub merged_by: Option<Actor>,
    pub merge_commit_sha: Option<String>,
    /// Check rollup of the head commit.
    pub checks: Vec<CheckEntry>,
    pub reviews: Vec<Review>,
    pub threads: Vec<ReviewThread>,
    /// Nested lists GitHub cut (checks, reviews, threads, labels); a judgement
    /// that depends on one of them must read it as unreadable.
    pub truncated: Vec<String>,
}

/// A PR GitHub associates with a commit (`commits/{sha}/pulls`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitPull {
    pub number: u64,
    pub url: String,
    pub merged_at: Option<Timestamp>,
    pub merge_commit_sha: Option<String>,
    pub base_ref: String,
    pub head_ref: String,
}

/// Required status checks of a branch, from classic protection and rulesets.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RequiredChecks {
    pub contexts: BTreeSet<String>,
    /// "Require branches to be up to date before merging".
    pub strict: bool,
}

/// A ruleset that applies to a repository (its own or an organisation's),
/// as GitHub returns it by id. Its bypass list is not read: an App's token
/// sees it empty without admin rights.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ruleset {
    pub id: u64,
    pub name: String,
    /// `branch`, `tag` or `push`.
    pub target: String,
    /// `active`, `evaluate` or `disabled`; only `active` enforces anything.
    pub enforcement: String,
    /// `conditions.ref_name.include`: fnmatch patterns on full refs
    /// (`refs/heads/…`, `refs/tags/…`), `~DEFAULT_BRANCH` or `~ALL`.
    pub include: Vec<String>,
    /// `conditions.ref_name.exclude`, the same syntax.
    pub exclude: Vec<String>,
    /// The type of each rule (`deletion`, `update`, `required_status_checks`, …).
    pub rules: BTreeSet<String>,
    /// The contexts its `required_status_checks` rules require.
    pub required_checks: BTreeSet<String>,
}

impl Ruleset {
    pub fn is_active(&self) -> bool {
        self.enforcement == "active"
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    pub url: String,
    pub body: String,
    pub draft: bool,
    pub prerelease: bool,
    pub published_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRun {
    pub id: u64,
    pub head_sha: String,
    pub head_branch: String,
    pub event: String,
    pub status: String,
    /// `None` while the run has not concluded.
    pub conclusion: Option<String>,
    pub url: String,
    pub created_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub name: String,
    pub conclusion: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub id: u64,
    pub name: String,
    pub expired: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub id: u64,
    pub author: Option<Actor>,
    pub body: String,
    pub created_at: Option<Timestamp>,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMethod {
    Squash,
    Merge,
}

impl MergeMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            MergeMethod::Squash => "squash",
            MergeMethod::Merge => "merge",
        }
    }
}
