//! An in-memory GitHub for tests: facts are set by hand, writes are recorded
//! (and applied where a later read must see them), failures are injected.
//!
//! A read nobody scripted answers `Unreadable` ("not scripted"), never an
//! empty list: a test that forgot a fact fails loudly instead of passing on
//! "nothing there". Only lookups by name answer `NotFound` when unscripted:
//! a file, a release, a tag object (GitHub's 404 for "it does not exist").
//! Writes are recorded and applied where a later read must see them (a
//! merged PR moves to the merged list; labels and branches change).

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use super::GitHub;
use super::types::*;

#[derive(Default)]
pub struct FakeState {
    pub identity: Option<Identity>,
    pub default_branch: BTreeMap<RepoSlug, String>,
    pub branches: BTreeMap<RepoSlug, Vec<GitRef>>,
    pub tags: BTreeMap<RepoSlug, Vec<GitRef>>,
    pub tag_objects: HashMap<(RepoSlug, String), AnnotatedTag>,
    pub commits: HashMap<(RepoSlug, String), Read<Commit>>,
    pub compares: HashMap<(RepoSlug, String, String), Read<Comparison>>,
    /// `(repo, head)`: `head`'s whole history (`since_root`).
    pub histories: HashMap<(RepoSlug, String), Read<Comparison>>,
    /// `(repo, rev, path)`; a missing entry is `NotFound`.
    pub files: HashMap<(RepoSlug, String, String), Read<String>>,
    /// `(repo, rev, path)`: a directory listing scripted as such (a failed
    /// read); otherwise `dir` lists the `files` directly under `path`.
    pub dirs: HashMap<(RepoSlug, String, String), Read<Vec<String>>>,
    pub open_pulls: BTreeMap<RepoSlug, Read<Vec<PullRequest>>>,
    pub merged_pulls: BTreeMap<RepoSlug, Read<Vec<PullRequest>>>,
    /// Closed without merge; none when not scripted.
    pub closed_pulls: BTreeMap<RepoSlug, Read<Vec<PullRequest>>>,
    pub pull_files: HashMap<(RepoSlug, u64), Read<Vec<String>>>,
    pub commit_pulls: HashMap<(RepoSlug, String), Read<Vec<CommitPull>>>,
    pub required_checks: HashMap<(RepoSlug, String), Read<RequiredChecks>>,
    pub rulesets: BTreeMap<RepoSlug, Read<Vec<Ruleset>>>,
    pub comments: HashMap<(RepoSlug, u64), Read<Vec<Comment>>>,
    /// A missing entry is `NotFound` (the release does not exist).
    pub releases: HashMap<(RepoSlug, String), Read<Release>>,
    pub runs: HashMap<(RepoSlug, String), Read<Vec<WorkflowRun>>>,
    pub jobs: HashMap<(RepoSlug, u64), Read<Vec<Job>>>,
    pub artifacts: HashMap<(RepoSlug, u64), Read<Vec<Artifact>>>,
    /// `(repo, artifact id, path)`: a file inside an artifact's zip.
    pub artifact_files: HashMap<(RepoSlug, u64, String), Read<String>>,
    /// Every write, as one line (`merge acme#12 at <sha> squash`, …).
    pub writes: Vec<String>,
    /// Writes whose line starts with a key here fail with that error.
    pub failing_writes: Vec<(String, WriteError)>,
    next_id: u64,
}

#[derive(Default)]
pub struct FakeGitHub {
    pub state: RefCell<FakeState>,
}

fn not_scripted<T>(what: String) -> Read<T> {
    Err(ReadError::unreadable(what, "not scripted in the fake"))
}

impl FakeGitHub {
    pub fn new() -> FakeGitHub {
        FakeGitHub::default()
    }

    pub fn writes(&self) -> Vec<String> {
        self.state.borrow().writes.clone()
    }

    pub fn fail_writes_starting_with(&self, prefix: &str, status: Option<u16>, detail: &str) {
        self.state.borrow_mut().failing_writes.push((
            prefix.to_string(),
            WriteError {
                what: prefix.to_string(),
                status,
                detail: detail.to_string(),
            },
        ));
    }

    fn record(&self, line: String) -> Write<()> {
        let mut s = self.state.borrow_mut();
        if let Some((_, e)) = s.failing_writes.iter().find(|(p, _)| line.starts_with(p)) {
            let e = e.clone();
            s.writes.push(format!("REFUSED {line}"));
            return Err(e);
        }
        s.writes.push(line);
        Ok(())
    }

    fn with_open_pull(&self, repo: &RepoSlug, number: u64, f: impl FnOnce(&mut PullRequest)) {
        if let Some(Ok(list)) = self.state.borrow_mut().open_pulls.get_mut(repo) {
            if let Some(p) = list.iter_mut().find(|p| p.number == number) {
                f(p);
            }
        }
    }

    fn id(&self) -> u64 {
        let mut s = self.state.borrow_mut();
        s.next_id += 1;
        s.next_id
    }
}

impl GitHub for FakeGitHub {
    fn identity(&self) -> Read<Identity> {
        match &self.state.borrow().identity {
            Some(i) => Ok(i.clone()),
            None => not_scripted("identity".into()),
        }
    }

    fn default_branch(&self, repo: &RepoSlug) -> Read<String> {
        match self.state.borrow().default_branch.get(repo) {
            Some(b) => Ok(b.clone()),
            None => not_scripted(format!("default branch of {repo}")),
        }
    }

    fn branches(&self, repo: &RepoSlug, prefix: &str) -> Read<Vec<GitRef>> {
        match self.state.borrow().branches.get(repo) {
            Some(b) => Ok(b
                .iter()
                .filter(|r| r.name.starts_with(prefix))
                .cloned()
                .collect()),
            None => not_scripted(format!("branches of {repo}")),
        }
    }

    fn tags(&self, repo: &RepoSlug, prefix: &str) -> Read<Vec<GitRef>> {
        match self.state.borrow().tags.get(repo) {
            Some(t) => Ok(t
                .iter()
                .filter(|r| r.name.starts_with(prefix))
                .cloned()
                .collect()),
            None => not_scripted(format!("tags of {repo}")),
        }
    }

    fn tag_object(&self, repo: &RepoSlug, sha: &str) -> Read<AnnotatedTag> {
        match self
            .state
            .borrow()
            .tag_objects
            .get(&(repo.clone(), sha.to_string()))
        {
            Some(t) => Ok(t.clone()),
            None => Err(ReadError::NotFound {
                what: format!("tag object {sha}"),
            }),
        }
    }

    fn commit(&self, repo: &RepoSlug, rev: &str) -> Read<Commit> {
        match self
            .state
            .borrow()
            .commits
            .get(&(repo.clone(), rev.to_string()))
        {
            Some(c) => c.clone(),
            None => not_scripted(format!("commit {rev}")),
        }
    }

    fn compare(&self, repo: &RepoSlug, base: &str, head: &str) -> Read<Comparison> {
        match self
            .state
            .borrow()
            .compares
            .get(&(repo.clone(), base.to_string(), head.to_string()))
        {
            Some(c) => c.clone(),
            None => not_scripted(format!("{base}...{head}")),
        }
    }

    fn since_root(&self, repo: &RepoSlug, head: &str) -> Read<Comparison> {
        match self
            .state
            .borrow()
            .histories
            .get(&(repo.clone(), head.to_string()))
        {
            Some(c) => c.clone(),
            None => not_scripted(format!("the history of {head}")),
        }
    }

    fn file(&self, repo: &RepoSlug, path: &str, rev: &str) -> Read<String> {
        match self
            .state
            .borrow()
            .files
            .get(&(repo.clone(), rev.to_string(), path.to_string()))
        {
            Some(f) => f.clone(),
            None => Err(ReadError::NotFound {
                what: format!("{path} at {rev}"),
            }),
        }
    }

    fn dir(&self, repo: &RepoSlug, path: &str, rev: &str) -> Read<Vec<String>> {
        let s = self.state.borrow();
        if let Some(d) = s
            .dirs
            .get(&(repo.clone(), rev.to_string(), path.to_string()))
        {
            return d.clone();
        }
        let prefix = format!("{}/", path.trim_end_matches('/'));
        let mut names: Vec<String> = s
            .files
            .iter()
            .filter(|((r, at, _), text)| r == repo && at == rev && text.is_ok())
            .filter_map(|((_, _, p), _)| p.strip_prefix(&prefix))
            .filter(|name| !name.contains('/'))
            .map(str::to_string)
            .collect();
        if names.is_empty() {
            return Err(ReadError::NotFound {
                what: format!("{path} at {rev}"),
            });
        }
        names.sort();
        Ok(names)
    }

    fn open_pulls(&self, repo: &RepoSlug, base: Option<&str>) -> Read<Vec<PullRequest>> {
        match self.state.borrow().open_pulls.get(repo) {
            Some(Ok(p)) => Ok(p
                .iter()
                .filter(|p| base.is_none_or(|b| p.base_ref == b))
                .cloned()
                .collect()),
            Some(Err(e)) => Err(e.clone()),
            None => not_scripted(format!("open PRs of {repo}")),
        }
    }

    fn pull(&self, repo: &RepoSlug, number: u64) -> Read<PullRequest> {
        let s = self.state.borrow();
        let lists = [s.open_pulls.get(repo), s.merged_pulls.get(repo)];
        if lists.iter().all(Option::is_none) {
            return not_scripted(format!("PR #{number}"));
        }
        for list in lists.into_iter().flatten() {
            match list {
                Ok(list) => {
                    if let Some(p) = list.iter().find(|p| p.number == number) {
                        return Ok(p.clone());
                    }
                }
                Err(e) => return Err(e.clone()),
            }
        }
        Err(ReadError::NotFound {
            what: format!("PR #{number}"),
        })
    }

    fn merged_pulls(
        &self,
        repo: &RepoSlug,
        base: Option<&str>,
        since: Option<Timestamp>,
        limit: usize,
    ) -> Read<Vec<PullRequest>> {
        match self.state.borrow().merged_pulls.get(repo) {
            Some(Ok(p)) => {
                let list: Vec<PullRequest> = p
                    .iter()
                    .filter(|p| base.is_none_or(|b| p.base_ref == b))
                    .filter(|p| match (since, p.merged_at) {
                        (Some(s), Some(m)) => m >= s,
                        (Some(_), None) => false,
                        (None, _) => true,
                    })
                    .cloned()
                    .collect();
                if list.len() > limit {
                    return Err(ReadError::unreadable(
                        format!("merged PRs of {repo}"),
                        format!("more than {limit} merged PRs"),
                    ));
                }
                Ok(list)
            }
            Some(Err(e)) => Err(e.clone()),
            None => not_scripted(format!("merged PRs of {repo}")),
        }
    }

    fn closed_pulls(
        &self,
        repo: &RepoSlug,
        since: Timestamp,
        limit: usize,
    ) -> Read<Vec<PullRequest>> {
        match self.state.borrow().closed_pulls.get(repo) {
            Some(Ok(p)) => {
                let list: Vec<PullRequest> = p
                    .iter()
                    .filter(|p| p.updated_at.is_none_or(|u| u >= since))
                    .cloned()
                    .collect();
                if list.len() > limit {
                    return Err(ReadError::unreadable(
                        format!("closed PRs of {repo}"),
                        format!("more than {limit} closed PRs"),
                    ));
                }
                Ok(list)
            }
            Some(Err(e)) => Err(e.clone()),
            None => Ok(Vec::new()),
        }
    }

    fn pull_files(&self, repo: &RepoSlug, number: u64) -> Read<Vec<String>> {
        match self.state.borrow().pull_files.get(&(repo.clone(), number)) {
            Some(f) => f.clone(),
            None => not_scripted(format!("files of PR #{number}")),
        }
    }

    fn commit_pulls(&self, repo: &RepoSlug, sha: &str) -> Read<Vec<CommitPull>> {
        match self
            .state
            .borrow()
            .commit_pulls
            .get(&(repo.clone(), sha.to_string()))
        {
            Some(p) => p.clone(),
            None => not_scripted(format!("PRs of commit {sha}")),
        }
    }

    fn required_checks(&self, repo: &RepoSlug, branch: &str) -> Read<RequiredChecks> {
        match self
            .state
            .borrow()
            .required_checks
            .get(&(repo.clone(), branch.to_string()))
        {
            Some(r) => r.clone(),
            None => not_scripted(format!("required checks of {branch}")),
        }
    }

    fn rulesets(&self, repo: &RepoSlug) -> Read<Vec<Ruleset>> {
        match self.state.borrow().rulesets.get(repo) {
            Some(r) => r.clone(),
            None => not_scripted(format!("rulesets of {repo}")),
        }
    }

    fn issue_comments(&self, repo: &RepoSlug, number: u64) -> Read<Vec<Comment>> {
        match self.state.borrow().comments.get(&(repo.clone(), number)) {
            Some(c) => c.clone(),
            None => not_scripted(format!("comments of #{number}")),
        }
    }

    fn release(&self, repo: &RepoSlug, tag: &str) -> Read<Release> {
        match self
            .state
            .borrow()
            .releases
            .get(&(repo.clone(), tag.to_string()))
        {
            Some(r) => r.clone(),
            None => Err(ReadError::NotFound {
                what: format!("release {tag}"),
            }),
        }
    }

    fn workflow_runs(
        &self,
        repo: &RepoSlug,
        workflow: &str,
        branch: Option<&str>,
        head_sha: Option<&str>,
        event: Option<&str>,
    ) -> Read<Vec<WorkflowRun>> {
        match self
            .state
            .borrow()
            .runs
            .get(&(repo.clone(), workflow.to_string()))
        {
            Some(Ok(runs)) => Ok(runs
                .iter()
                .filter(|r| branch.is_none_or(|b| r.head_branch == b))
                .filter(|r| head_sha.is_none_or(|s| r.head_sha == s))
                .filter(|r| event.is_none_or(|e| r.event == e))
                .cloned()
                .collect()),
            Some(Err(e)) => Err(e.clone()),
            None => not_scripted(format!("runs of {workflow}")),
        }
    }

    fn run_jobs(&self, repo: &RepoSlug, run_id: u64) -> Read<Vec<Job>> {
        match self.state.borrow().jobs.get(&(repo.clone(), run_id)) {
            Some(j) => j.clone(),
            None => not_scripted(format!("jobs of run {run_id}")),
        }
    }

    fn run_artifacts(&self, repo: &RepoSlug, run_id: u64) -> Read<Vec<Artifact>> {
        match self.state.borrow().artifacts.get(&(repo.clone(), run_id)) {
            Some(a) => a.clone(),
            None => not_scripted(format!("artifacts of run {run_id}")),
        }
    }

    fn artifact_file(&self, repo: &RepoSlug, artifact_id: u64, path: &str) -> Read<String> {
        match self
            .state
            .borrow()
            .artifact_files
            .get(&(repo.clone(), artifact_id, path.to_string()))
        {
            Some(f) => f.clone(),
            None => not_scripted(format!("{path} in artifact {artifact_id}")),
        }
    }

    fn merge_pull(
        &self,
        repo: &RepoSlug,
        number: u64,
        head_sha: &str,
        method: MergeMethod,
    ) -> Write<String> {
        self.record(format!(
            "merge {repo}#{number} at {head_sha} {}",
            method.as_str()
        ))?;
        let merge_sha = format!("merge-of-{number}");
        let mut s = self.state.borrow_mut();
        let moved = match s.open_pulls.get_mut(repo) {
            Some(Ok(list)) => list
                .iter()
                .position(|p| p.number == number)
                .map(|i| list.remove(i)),
            _ => None,
        };
        if let Some(mut pull) = moved {
            pull.state = PrState::Merged;
            pull.merged_at = Some(Timestamp::now());
            pull.merge_commit_sha = Some(merge_sha.clone());
            pull.merged_by = s.identity.as_ref().map(|i| Actor {
                login: i.login(),
                is_bot: matches!(i, Identity::App { .. }),
            });
            if let Ok(list) = s
                .merged_pulls
                .entry(repo.clone())
                .or_insert_with(|| Ok(Vec::new()))
            {
                list.insert(0, pull);
            }
        }
        Ok(merge_sha)
    }

    fn update_branch(&self, repo: &RepoSlug, number: u64, head_sha: &str) -> Write<()> {
        self.record(format!("update-branch {repo}#{number} at {head_sha}"))
    }

    fn delete_branch(&self, repo: &RepoSlug, branch: &str) -> Write<()> {
        self.record(format!("delete-branch {repo} {branch}"))?;
        if let Some(list) = self.state.borrow_mut().branches.get_mut(repo) {
            list.retain(|b| b.name != branch);
        }
        Ok(())
    }

    fn create_tag(&self, repo: &RepoSlug, tag: &str, sha: &str, message: &str) -> Write<()> {
        self.record(format!("create-tag {repo} {tag} at {sha}"))?;
        let object = format!("tag-object-{tag}");
        let mut s = self.state.borrow_mut();
        s.tag_objects.insert(
            (repo.clone(), object.clone()),
            AnnotatedTag {
                name: tag.to_string(),
                message: message.to_string(),
                tagger_date: None,
                target_sha: sha.to_string(),
                target_type: ObjectType::Commit,
            },
        );
        s.tags.entry(repo.clone()).or_default().push(GitRef {
            name: tag.to_string(),
            sha: object,
            object_type: ObjectType::Tag,
        });
        Ok(())
    }

    fn create_release(&self, repo: &RepoSlug, tag: &str, name: &str, body: &str) -> Write<Release> {
        self.record(format!("create-release {repo} {tag} {name}"))?;
        let identity = self.state.borrow().identity.clone();
        let release = Release {
            tag: tag.to_string(),
            url: format!("https://github.com/{repo}/releases/tag/{tag}"),
            body: body.to_string(),
            draft: false,
            prerelease: false,
            published_at: None,
            author: identity.map(|i| Actor {
                login: i.login(),
                is_bot: true,
            }),
        };
        self.state
            .borrow_mut()
            .releases
            .insert((repo.clone(), tag.to_string()), Ok(release.clone()));
        Ok(release)
    }

    fn create_branch(&self, repo: &RepoSlug, branch: &str, sha: &str) -> Write<()> {
        self.record(format!("create-branch {repo} {branch} at {sha}"))?;
        self.state
            .borrow_mut()
            .branches
            .entry(repo.clone())
            .or_default()
            .push(GitRef {
                name: branch.to_string(),
                sha: sha.to_string(),
                object_type: ObjectType::Commit,
            });
        Ok(())
    }

    fn add_labels(&self, repo: &RepoSlug, number: u64, labels: &[String]) -> Write<()> {
        self.record(format!("add-labels {repo}#{number} {}", labels.join(", ")))?;
        self.with_open_pull(repo, number, |p| {
            for l in labels {
                if !p.labels.contains(l) {
                    p.labels.push(l.clone());
                }
            }
        });
        Ok(())
    }

    fn remove_label(&self, repo: &RepoSlug, number: u64, label: &str) -> Write<()> {
        self.record(format!("remove-label {repo}#{number} {label}"))?;
        self.with_open_pull(repo, number, |p| p.labels.retain(|l| l != label));
        Ok(())
    }

    fn comment(&self, repo: &RepoSlug, number: u64, body: &str) -> Write<Comment> {
        self.record(format!("comment {repo}#{number} {body}"))?;
        let id = self.id();
        let identity = self.state.borrow().identity.clone();
        let comment = Comment {
            id,
            author: identity.map(|i| Actor {
                login: i.login(),
                is_bot: matches!(i, Identity::App { .. }),
            }),
            body: body.to_string(),
            created_at: None,
            url: format!("https://github.com/{repo}/issues/{number}#issuecomment-{id}"),
        };
        let mut s = self.state.borrow_mut();
        if let Ok(list) = s
            .comments
            .entry((repo.clone(), number))
            .or_insert_with(|| Ok(Vec::new()))
        {
            list.push(comment.clone());
        }
        Ok(comment)
    }

    fn update_comment(&self, repo: &RepoSlug, comment_id: u64, body: &str) -> Write<()> {
        self.record(format!("update-comment {repo} {comment_id} {body}"))?;
        let mut s = self.state.borrow_mut();
        for list in s.comments.values_mut().flatten() {
            if let Some(c) = list.iter_mut().find(|c| c.id == comment_id) {
                c.body = body.to_string();
            }
        }
        Ok(())
    }

    fn put_file(
        &self,
        repo: &RepoSlug,
        branch: &str,
        path: &str,
        content: &str,
        message: &str,
    ) -> Write<String> {
        self.record(format!("put-file {repo} {branch} {path} ({message})"))?;
        self.state.borrow_mut().files.insert(
            (repo.clone(), branch.to_string(), path.to_string()),
            Ok(content.to_string()),
        );
        Ok(format!("commit-{}", self.id()))
    }

    fn create_pull(
        &self,
        repo: &RepoSlug,
        head: &str,
        base: &str,
        title: &str,
        _body: &str,
    ) -> Write<u64> {
        self.record(format!("create-pull {repo} {head} -> {base} {title}"))?;
        Ok(1000 + self.id())
    }

    fn dispatch_workflow(
        &self,
        repo: &RepoSlug,
        workflow: &str,
        git_ref: &str,
        inputs: &[(String, String)],
    ) -> Write<()> {
        let inputs = inputs
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(" ");
        self.record(format!("dispatch {repo} {workflow} on {git_ref} {inputs}"))
    }

    fn request_reviewers(&self, repo: &RepoSlug, number: u64, logins: &[String]) -> Write<()> {
        self.record(format!(
            "request-reviewers {repo}#{number} {}",
            logins.join(", ")
        ))
    }

    fn dismiss_review(
        &self,
        repo: &RepoSlug,
        number: u64,
        review_id: u64,
        message: &str,
    ) -> Write<()> {
        self.record(format!(
            "dismiss-review {repo}#{number} {review_id}: {message}"
        ))?;
        self.with_open_pull(repo, number, |p| {
            for r in p.reviews.iter_mut().filter(|r| r.id == Some(review_id)) {
                r.state = "DISMISSED".into();
            }
        });
        Ok(())
    }
}

/// A plain open PR for tests: green-less (no checks), no review, on `main`.
pub fn pull_request(number: u64) -> PullRequest {
    PullRequest {
        number,
        url: format!("https://github.com/example-org/acme/pull/{number}"),
        title: format!("feat: change {number}"),
        body: String::new(),
        state: PrState::Open,
        draft: false,
        author: Some(Actor {
            login: "author-login".into(),
            is_bot: false,
        }),
        base_ref: "main".into(),
        head_ref: format!("feat/change-{number}"),
        cross_repository: false,
        head_sha: format!("head-{number}"),
        labels: Vec::new(),
        mergeable: Mergeable::Mergeable,
        merge_state: "CLEAN".into(),
        additions: 10,
        deletions: 2,
        created_at: None,
        updated_at: None,
        merged_at: None,
        merged_by: None,
        merge_commit_sha: None,
        checks: Vec::new(),
        reviews: Vec::new(),
        threads: Vec::new(),
        truncated: Vec::new(),
    }
}
