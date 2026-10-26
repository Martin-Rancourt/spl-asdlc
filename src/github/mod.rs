//! GitHub: the facts the bridge reads and the few writes it makes
//! (`docs/reference/07-the-bridge.md`, REQ-REL-099).
//!
//! Every read answers with a value, [`ReadError::NotFound`] (GitHub said 404)
//! or [`ReadError::Unreadable`] (anything else, including a list GitHub cut):
//! "I could not read" is never "there is nothing". The classification is by
//! HTTP status, never by the wording of a message. The bridge talks to the
//! API only; it never needs a clone.

pub mod auth;
pub mod fake;
pub use crate::http;
mod rest;
pub mod types;
pub(crate) mod zip;

pub use rest::RestGitHub;
pub use types::*;

/// What the bridge asks of GitHub. [`RestGitHub`] is the real client;
/// [`fake::FakeGitHub`] serves the tests.
pub trait GitHub {
    // ---- identity and repository ----

    /// Under which name the bridge writes.
    fn identity(&self) -> Read<Identity>;
    fn default_branch(&self, repo: &RepoSlug) -> Read<String>;
    /// The repository variable `name` (GitHub Actions variables);
    /// `NotFound` when the repository has none of that name.
    fn variable(&self, repo: &RepoSlug, name: &str) -> Read<Variable>;
    /// The hourly quotas of the identity that reads `repo` (an App's are
    /// shared by every repository of its installation). Reading them costs
    /// none of them.
    fn quota(&self, repo: &RepoSlug) -> Read<Vec<Quota>>;

    // ---- refs and history ----

    /// Branches whose name starts with `prefix` (e.g. `release/`).
    fn branches(&self, repo: &RepoSlug, prefix: &str) -> Read<Vec<GitRef>>;
    /// Tags whose name starts with `prefix`.
    fn tags(&self, repo: &RepoSlug, prefix: &str) -> Read<Vec<GitRef>>;
    /// The annotated tag object `sha` (a [`GitRef`] of type [`ObjectType::Tag`]).
    fn tag_object(&self, repo: &RepoSlug, sha: &str) -> Read<AnnotatedTag>;
    /// The commit a revision (sha, branch or tag) resolves to.
    fn commit(&self, repo: &RepoSlug, rev: &str) -> Read<Commit>;
    /// `base...head`, refused when GitHub cannot return it whole.
    fn compare(&self, repo: &RepoSlug, base: &str, head: &str) -> Read<Comparison>;
    /// `head`'s whole history, as a comparison from the repository's root:
    /// every commit `head` reaches and every file at `head` (no base, so no
    /// `base_committed_at`). The range of a first release, which no earlier
    /// release bounds; refused when GitHub cannot return it whole.
    fn since_root(&self, repo: &RepoSlug, head: &str) -> Read<Comparison>;
    /// The text of `path` at `rev`; `NotFound` when GitHub says 404 (the
    /// caller decides whether the revision itself exists).
    fn file(&self, repo: &RepoSlug, path: &str, rev: &str) -> Read<String>;
    /// The text of `path` at `rev` in another repository, `source`, read with
    /// the token the bridge holds for `repo` (with an App, its installation on
    /// `repo`): the method's texts a project is pinned on
    /// (`crate::method::pin`). `NotFound` when GitHub says 404, which it also
    /// says of a private `source` that token cannot see.
    fn file_from(&self, repo: &RepoSlug, source: &RepoSlug, path: &str, rev: &str) -> Read<String>;
    /// The names of the files directly in the directory `path` at `rev`
    /// (no sub-directory), sorted; `NotFound` when GitHub says 404 (the
    /// caller decides whether the revision itself exists).
    fn dir(&self, repo: &RepoSlug, path: &str, rev: &str) -> Read<Vec<String>>;

    // ---- pull requests ----

    /// Every open PR, optionally on one base; a list GitHub cut is unreadable.
    fn open_pulls(&self, repo: &RepoSlug, base: Option<&str>) -> Read<Vec<PullRequest>>;
    fn pull(&self, repo: &RepoSlug, number: u64) -> Read<PullRequest>;
    /// Up to `limit` PRs updated at or after `since`, oldest update first.
    /// `handled_at_since` disambiguates PRs sharing the checkpoint's second. The real
    /// client lists PRs (`/pulls`, never `/issues`: the App has no Issues
    /// permission) in pages of 100 and hydrates the rows kept in one GraphQL
    /// query, never with one request per PR.
    fn updated_pulls(
        &self,
        repo: &RepoSlug,
        since: Option<Timestamp>,
        handled_at_since: &[u64],
        limit: usize,
    ) -> Read<Vec<PullRequest>>;
    /// PRs merged since `since`, optionally on one base, newest first; more
    /// than `limit` of them is unreadable (a partial list is not the list).
    fn merged_pulls(
        &self,
        repo: &RepoSlug,
        base: Option<&str>,
        since: Option<Timestamp>,
        limit: usize,
    ) -> Read<Vec<PullRequest>>;
    /// PRs closed without being merged and updated since `since`, newest
    /// first; more than `limit` of them is unreadable.
    fn closed_pulls(
        &self,
        repo: &RepoSlug,
        since: Timestamp,
        limit: usize,
    ) -> Read<Vec<PullRequest>>;
    /// Paths a PR changes; a listing GitHub caps is unreadable.
    fn pull_files(&self, repo: &RepoSlug, number: u64) -> Read<Vec<String>>;
    /// PRs GitHub associates with a commit.
    fn commit_pulls(&self, repo: &RepoSlug, sha: &str) -> Read<Vec<CommitPull>>;
    /// Required status checks of a branch: classic protection and rulesets.
    fn required_checks(&self, repo: &RepoSlug, branch: &str) -> Read<RequiredChecks>;
    /// Every ruleset that applies to the repository, whatever its target or
    /// enforcement, each read by id (conditions and rules).
    fn rulesets(&self, repo: &RepoSlug) -> Read<Vec<Ruleset>>;
    /// Comments on a PR or an issue, oldest first.
    fn issue_comments(&self, repo: &RepoSlug, number: u64) -> Read<Vec<Comment>>;

    // ---- releases and workflow runs ----

    fn release(&self, repo: &RepoSlug, tag: &str) -> Read<Release>;
    /// The newest runs (at most 100) of a workflow file, filtered as given,
    /// newest first. It is the head of the history, not the history: callers
    /// take the newest matching run and never read absence into it.
    fn workflow_runs(
        &self,
        repo: &RepoSlug,
        workflow: &str,
        branch: Option<&str>,
        head_sha: Option<&str>,
        event: Option<&str>,
    ) -> Read<Vec<WorkflowRun>>;
    /// The jobs of a run's latest attempt, as GitHub names them (a job of a
    /// called workflow is `<caller job> / <called job>`).
    fn run_jobs(&self, repo: &RepoSlug, run_id: u64) -> Read<Vec<Job>>;
    fn run_artifacts(&self, repo: &RepoSlug, run_id: u64) -> Read<Vec<Artifact>>;
    /// The text of `path` inside the artifact `artifact_id`, read from its
    /// zip: `NotFound` when the zip holds no such file; a zip that cannot be
    /// downloaded or read is unreadable.
    fn artifact_file(&self, repo: &RepoSlug, artifact_id: u64, path: &str) -> Read<String>;

    // ---- writes ----

    /// Merge pinned to `head_sha`: a head that moved is refused by GitHub.
    /// Returns the merge commit.
    fn merge_pull(
        &self,
        repo: &RepoSlug,
        number: u64,
        head_sha: &str,
        method: MergeMethod,
    ) -> Write<String>;
    /// Merge the base into the head (REST `update-branch`), pinned to the head.
    fn update_branch(&self, repo: &RepoSlug, number: u64, head_sha: &str) -> Write<()>;
    fn delete_branch(&self, repo: &RepoSlug, branch: &str) -> Write<()>;
    fn create_branch(&self, repo: &RepoSlug, branch: &str, sha: &str) -> Write<()>;
    /// Create the annotated tag `tag` on the commit `sha`, under the
    /// bridge's identity: the tag object, then its ref. GitHub refuses a tag
    /// that exists.
    fn create_tag(&self, repo: &RepoSlug, tag: &str, sha: &str, message: &str) -> Write<()>;
    /// Publish the GitHub release of an existing tag, with its notes.
    fn create_release(&self, repo: &RepoSlug, tag: &str, name: &str, body: &str) -> Write<Release>;
    /// Delete the GitHub release `release_id` (the retraction of a release
    /// whose publication deployed nothing); its tag is left as it is.
    fn delete_release(&self, repo: &RepoSlug, release_id: u64) -> Write<()>;
    /// Delete the tag ref `refs/tags/<tag>`, as the bridge's App, which the
    /// tag rulesets let bypass them.
    fn delete_tag(&self, repo: &RepoSlug, tag: &str) -> Write<()>;
    fn add_labels(&self, repo: &RepoSlug, number: u64, labels: &[String]) -> Write<()>;
    /// Removing a label the PR does not carry is a success.
    fn remove_label(&self, repo: &RepoSlug, number: u64, label: &str) -> Write<()>;
    fn comment(&self, repo: &RepoSlug, number: u64, body: &str) -> Write<Comment>;
    fn update_comment(&self, repo: &RepoSlug, comment_id: u64, body: &str) -> Write<()>;
    /// Create or replace `path` on `branch` (contents API); returns the commit.
    fn put_file(
        &self,
        repo: &RepoSlug,
        branch: &str,
        path: &str,
        content: &str,
        message: &str,
    ) -> Write<String>;
    /// Open a PR; returns its number.
    fn create_pull(
        &self,
        repo: &RepoSlug,
        head: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> Write<u64>;
    fn dispatch_workflow(
        &self,
        repo: &RepoSlug,
        workflow: &str,
        git_ref: &str,
        inputs: &[(String, String)],
    ) -> Write<()>;
    fn request_reviewers(&self, repo: &RepoSlug, number: u64, logins: &[String]) -> Write<()>;
    /// Dismiss a review of a PR (REST `dismissals`), with the message GitHub
    /// shows in its place.
    fn dismiss_review(
        &self,
        repo: &RepoSlug,
        number: u64,
        review_id: u64,
        message: &str,
    ) -> Write<()>;
}
