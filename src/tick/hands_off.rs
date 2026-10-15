//! The hands-off label (`docs/spec/07-the-bridge.md`, "Hands off"): a
//! tracker item or a GitHub PR that carries `bridge.hands_off_label` was
//! taken over by a person, and the tick writes nothing to it.
//!
//! [`HandsOff`] wraps the tick's tracker and GitHub, like the dry-run
//! wrappers (`crate::dry`): every read passes through and is remembered
//! (whether each item and PR read carries the label, which item a comment or
//! a relation belongs to), and every write that would touch an object under
//! the label is refused before it reaches the backend. It is the one guard:
//! no pass checks the label before a write, [`Tick::act`](super::Tick::act)
//! turns a refusal into a "hands-off" line of the report, never a failure.
//!
//! What a tracker write touches: the item it names; for a new item, its
//! parent; for a relation, both ends. A tracker write on an item this tick
//! has not read yet reads it first (a read that fails refuses the write, fail
//! closed). A GitHub write touches the PR or issue it names; the tick writes
//! to a PR only after reading it, so a number never read this tick (an issue)
//! is not checked.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::github::{
    self, GitHub, MergeMethod, PullRequest, Read, RepoSlug, Timestamp, Write, WriteError,
};
use crate::method::vocabulary::Status;
use crate::tracker::{
    Comment, Item, NewItem, RelationKind, Relations, TResult, Tracker, TrackerError, User,
};

/// Whether `labels` include the hands-off label (labels compare without
/// regard to case, as the trackers and GitHub show them).
pub fn carries(labels: &[String], label: &str) -> bool {
    labels.iter().any(|l| l.eq_ignore_ascii_case(label))
}

/// The report's name of a PR.
pub fn pr_object(number: u64) -> String {
    format!("pr#{number}")
}

#[derive(Default)]
struct Seen {
    /// Item key (and id) → carries the label, as last read this tick.
    items: HashMap<String, bool>,
    /// PR number → carries the label, as last read this tick.
    prs: HashMap<u64, bool>,
    /// Tracker comment id → the key of its item.
    item_comments: HashMap<String, String>,
    /// Tracker relation id → the keys of its two ends.
    relations: HashMap<String, Vec<String>>,
    /// GitHub comment id → the PR or issue it is on.
    pr_comments: HashMap<u64, u64>,
    /// Objects a write was refused on, not yet taken by the tick.
    refused: Vec<String>,
}

/// The guard of one tick.
pub struct HandsOff<'a> {
    label: String,
    tracker: &'a dyn Tracker,
    github: &'a dyn GitHub,
    seen: RefCell<Seen>,
}

impl<'a> HandsOff<'a> {
    pub fn new(label: &str, tracker: &'a dyn Tracker, github: &'a dyn GitHub) -> HandsOff<'a> {
        HandsOff {
            label: label.to_string(),
            tracker,
            github,
            seen: RefCell::new(Seen::default()),
        }
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    /// The objects (`ABC-12`, `pr#7`) a write was refused on since the last
    /// call.
    pub fn take_refused(&self) -> Vec<String> {
        std::mem::take(&mut self.seen.borrow_mut().refused)
    }

    fn see_item(&self, item: &Item) {
        let held = carries(&item.labels, &self.label);
        let mut seen = self.seen.borrow_mut();
        seen.items.insert(item.key.clone(), held);
        if !item.id.is_empty() {
            seen.items.insert(item.id.clone(), held);
        }
    }

    fn see_items(&self, items: &TResult<Vec<Item>>) {
        if let Ok(items) = items {
            items.iter().for_each(|i| self.see_item(i));
        }
    }

    fn see_pr(&self, pr: &PullRequest) {
        self.seen
            .borrow_mut()
            .prs
            .insert(pr.number, carries(&pr.labels, &self.label));
    }

    fn see_prs(&self, prs: &Read<Vec<PullRequest>>) {
        if let Ok(prs) = prs {
            prs.iter().for_each(|p| self.see_pr(p));
        }
    }

    fn refuse(&self, object: String) -> String {
        let detail = format!("carries the label {}: hands off", self.label);
        self.seen.borrow_mut().refused.push(object);
        detail
    }

    /// `Err` when the item carries the label (or cannot be read to say).
    fn item(&self, key: &str) -> TResult<()> {
        let known = self.seen.borrow().items.get(key).copied();
        let held = match known {
            Some(held) => held,
            None => match self.tracker.get(key) {
                Ok(item) => {
                    self.see_item(&item);
                    carries(&item.labels, &self.label)
                }
                // Nothing to protect: the write itself says what it finds.
                Err(e) if e.is_not_found() => false,
                Err(e) => return Err(e),
            },
        };
        if held {
            let detail = self.refuse(key.to_string());
            return Err(TrackerError::refused(key, detail));
        }
        Ok(())
    }

    /// `Err` when the PR was read this tick carrying the label.
    fn pr(&self, number: u64) -> Write<()> {
        let held = self.seen.borrow().prs.get(&number).copied() == Some(true);
        if held {
            let detail = self.refuse(pr_object(number));
            return Err(WriteError {
                what: format!("#{number}"),
                status: None,
                detail,
            });
        }
        Ok(())
    }
}

impl Tracker for HandsOff<'_> {
    fn identity(&self) -> TResult<User> {
        self.tracker.identity()
    }
    fn team_key(&self) -> TResult<String> {
        self.tracker.team_key()
    }
    fn status_names(&self) -> TResult<Vec<String>> {
        self.tracker.status_names()
    }
    fn user(&self, reference: &str) -> TResult<User> {
        self.tracker.user(reference)
    }
    fn get(&self, key: &str) -> TResult<Item> {
        let item = self.tracker.get(key);
        if let Ok(i) = &item {
            self.see_item(i);
        }
        item
    }
    fn relations(&self, key: &str) -> TResult<Relations> {
        let relations = self.tracker.relations(key);
        if let Ok(r) = &relations {
            let mut seen = self.seen.borrow_mut();
            for arc in r
                .blocks
                .iter()
                .chain(&r.blocked_by)
                .chain(&r.related)
                .chain(&r.duplicates)
            {
                seen.relations
                    .insert(arc.id.clone(), vec![key.to_string(), arc.other.clone()]);
            }
        }
        relations
    }
    fn comments(&self, key: &str) -> TResult<Vec<Comment>> {
        let comments = self.tracker.comments(key);
        if let Ok(list) = &comments {
            let mut seen = self.seen.borrow_mut();
            for c in list {
                seen.item_comments.insert(c.id.clone(), key.to_string());
            }
        }
        comments
    }
    fn children(&self, key: &str) -> TResult<Vec<Item>> {
        let items = self.tracker.children(key);
        self.see_items(&items);
        items
    }
    fn list_by_label(&self, label: &str) -> TResult<Vec<Item>> {
        let items = self.tracker.list_by_label(label);
        self.see_items(&items);
        items
    }
    fn find_labelled_containing(&self, label: &str, needle: &str) -> TResult<Vec<Item>> {
        let items = self.tracker.find_labelled_containing(label, needle);
        self.see_items(&items);
        items
    }
    fn items_linked_to_pr(&self, pr_url: &str) -> TResult<Vec<String>> {
        self.tracker.items_linked_to_pr(pr_url)
    }
    fn label_exists(&self, name: &str) -> TResult<bool> {
        self.tracker.label_exists(name)
    }
    fn ensure_label(&self, name: &str) -> TResult<()> {
        self.tracker.ensure_label(name)
    }
    fn create_item(&self, item: &NewItem) -> TResult<Item> {
        if let Some(parent) = &item.parent {
            self.item(parent)?;
        }
        let created = self.tracker.create_item(item)?;
        self.see_item(&created);
        Ok(created)
    }
    fn update_description(&self, key: &str, description: &str) -> TResult<()> {
        self.item(key)?;
        self.tracker.update_description(key, description)
    }
    fn add_label(&self, key: &str, label: &str) -> TResult<()> {
        self.item(key)?;
        self.tracker.add_label(key, label)
    }
    fn remove_label(&self, key: &str, label: &str) -> TResult<()> {
        self.item(key)?;
        self.tracker.remove_label(key, label)
    }
    fn set_status(
        &self,
        key: &str,
        status: Status,
        assignee: Option<Option<&User>>,
    ) -> TResult<()> {
        self.item(key)?;
        self.tracker.set_status(key, status, assignee)
    }
    fn assign(&self, key: &str, user: Option<&User>) -> TResult<()> {
        self.item(key)?;
        self.tracker.assign(key, user)
    }
    fn comment(&self, key: &str, body: &str) -> TResult<Comment> {
        self.item(key)?;
        self.tracker.comment(key, body)
    }
    fn can_edit_comments(&self) -> bool {
        self.tracker.can_edit_comments()
    }
    fn update_comment(&self, comment_id: &str, body: &str) -> TResult<()> {
        let key = self.seen.borrow().item_comments.get(comment_id).cloned();
        if let Some(key) = key {
            self.item(&key)?;
        }
        self.tracker.update_comment(comment_id, body)
    }
    fn relate(&self, a: &str, b: &str, kind: RelationKind) -> TResult<()> {
        self.item(a)?;
        self.item(b)?;
        self.tracker.relate(a, b, kind)
    }
    fn unrelate(&self, relation_id: &str) -> TResult<()> {
        let ends = self.seen.borrow().relations.get(relation_id).cloned();
        for key in ends.unwrap_or_default() {
            self.item(&key)?;
        }
        self.tracker.unrelate(relation_id)
    }
    fn link_pr(&self, key: &str, pr_url: &str, title: &str) -> TResult<()> {
        self.item(key)?;
        self.tracker.link_pr(key, pr_url, title)
    }
    fn archives(&self) -> bool {
        self.tracker.archives()
    }
    fn archive(&self, key: &str) -> TResult<()> {
        self.item(key)?;
        self.tracker.archive(key)
    }
    fn keeps_html_comments(&self) -> bool {
        self.tracker.keeps_html_comments()
    }
    fn keys_in_text(&self, text: &str) -> Vec<String> {
        self.tracker.keys_in_text(text)
    }
}

impl GitHub for HandsOff<'_> {
    fn identity(&self) -> Read<github::Identity> {
        self.github.identity()
    }
    fn default_branch(&self, repo: &RepoSlug) -> Read<String> {
        self.github.default_branch(repo)
    }
    fn branches(&self, repo: &RepoSlug, prefix: &str) -> Read<Vec<github::GitRef>> {
        self.github.branches(repo, prefix)
    }
    fn tags(&self, repo: &RepoSlug, prefix: &str) -> Read<Vec<github::GitRef>> {
        self.github.tags(repo, prefix)
    }
    fn tag_object(&self, repo: &RepoSlug, sha: &str) -> Read<github::AnnotatedTag> {
        self.github.tag_object(repo, sha)
    }
    fn commit(&self, repo: &RepoSlug, rev: &str) -> Read<github::Commit> {
        self.github.commit(repo, rev)
    }
    fn compare(&self, repo: &RepoSlug, base: &str, head: &str) -> Read<github::Comparison> {
        self.github.compare(repo, base, head)
    }
    fn since_root(&self, repo: &RepoSlug, head: &str) -> Read<github::Comparison> {
        self.github.since_root(repo, head)
    }
    fn file(&self, repo: &RepoSlug, path: &str, rev: &str) -> Read<String> {
        self.github.file(repo, path, rev)
    }
    fn dir(&self, repo: &RepoSlug, path: &str, rev: &str) -> Read<Vec<String>> {
        self.github.dir(repo, path, rev)
    }
    fn open_pulls(&self, repo: &RepoSlug, base: Option<&str>) -> Read<Vec<PullRequest>> {
        let prs = self.github.open_pulls(repo, base);
        self.see_prs(&prs);
        prs
    }
    fn pull(&self, repo: &RepoSlug, number: u64) -> Read<PullRequest> {
        let pr = self.github.pull(repo, number);
        if let Ok(p) = &pr {
            self.see_pr(p);
        }
        pr
    }
    fn merged_pulls(
        &self,
        repo: &RepoSlug,
        base: Option<&str>,
        since: Option<Timestamp>,
        limit: usize,
    ) -> Read<Vec<PullRequest>> {
        let prs = self.github.merged_pulls(repo, base, since, limit);
        self.see_prs(&prs);
        prs
    }
    fn closed_pulls(
        &self,
        repo: &RepoSlug,
        since: Timestamp,
        limit: usize,
    ) -> Read<Vec<PullRequest>> {
        let prs = self.github.closed_pulls(repo, since, limit);
        self.see_prs(&prs);
        prs
    }
    fn pull_files(&self, repo: &RepoSlug, number: u64) -> Read<Vec<String>> {
        self.github.pull_files(repo, number)
    }
    fn commit_pulls(&self, repo: &RepoSlug, sha: &str) -> Read<Vec<github::CommitPull>> {
        self.github.commit_pulls(repo, sha)
    }
    fn required_checks(&self, repo: &RepoSlug, branch: &str) -> Read<github::RequiredChecks> {
        self.github.required_checks(repo, branch)
    }
    fn rulesets(&self, repo: &RepoSlug) -> Read<Vec<github::Ruleset>> {
        self.github.rulesets(repo)
    }
    fn issue_comments(&self, repo: &RepoSlug, number: u64) -> Read<Vec<github::Comment>> {
        let comments = self.github.issue_comments(repo, number);
        if let Ok(list) = &comments {
            let mut seen = self.seen.borrow_mut();
            for c in list {
                seen.pr_comments.insert(c.id, number);
            }
        }
        comments
    }
    fn release(&self, repo: &RepoSlug, tag: &str) -> Read<github::Release> {
        self.github.release(repo, tag)
    }
    fn workflow_runs(
        &self,
        repo: &RepoSlug,
        workflow: &str,
        branch: Option<&str>,
        head_sha: Option<&str>,
        event: Option<&str>,
    ) -> Read<Vec<github::WorkflowRun>> {
        self.github
            .workflow_runs(repo, workflow, branch, head_sha, event)
    }
    fn run_jobs(&self, repo: &RepoSlug, run_id: u64) -> Read<Vec<github::Job>> {
        self.github.run_jobs(repo, run_id)
    }
    fn run_artifacts(&self, repo: &RepoSlug, run_id: u64) -> Read<Vec<github::Artifact>> {
        self.github.run_artifacts(repo, run_id)
    }
    fn artifact_file(&self, repo: &RepoSlug, artifact_id: u64, path: &str) -> Read<String> {
        self.github.artifact_file(repo, artifact_id, path)
    }

    fn merge_pull(
        &self,
        repo: &RepoSlug,
        number: u64,
        head_sha: &str,
        method: MergeMethod,
    ) -> Write<String> {
        self.pr(number)?;
        self.github.merge_pull(repo, number, head_sha, method)
    }
    fn update_branch(&self, repo: &RepoSlug, number: u64, head_sha: &str) -> Write<()> {
        self.pr(number)?;
        self.github.update_branch(repo, number, head_sha)
    }
    fn delete_branch(&self, repo: &RepoSlug, branch: &str) -> Write<()> {
        self.github.delete_branch(repo, branch)
    }
    fn create_branch(&self, repo: &RepoSlug, branch: &str, sha: &str) -> Write<()> {
        self.github.create_branch(repo, branch, sha)
    }
    fn create_tag(&self, repo: &RepoSlug, tag: &str, sha: &str, message: &str) -> Write<()> {
        self.github.create_tag(repo, tag, sha, message)
    }
    fn create_release(
        &self,
        repo: &RepoSlug,
        tag: &str,
        name: &str,
        body: &str,
    ) -> Write<github::Release> {
        self.github.create_release(repo, tag, name, body)
    }
    fn add_labels(&self, repo: &RepoSlug, number: u64, labels: &[String]) -> Write<()> {
        self.pr(number)?;
        self.github.add_labels(repo, number, labels)
    }
    fn remove_label(&self, repo: &RepoSlug, number: u64, label: &str) -> Write<()> {
        self.pr(number)?;
        self.github.remove_label(repo, number, label)
    }
    fn comment(&self, repo: &RepoSlug, number: u64, body: &str) -> Write<github::Comment> {
        self.pr(number)?;
        let comment = self.github.comment(repo, number, body)?;
        self.seen
            .borrow_mut()
            .pr_comments
            .insert(comment.id, number);
        Ok(comment)
    }
    fn update_comment(&self, repo: &RepoSlug, comment_id: u64, body: &str) -> Write<()> {
        let number = self.seen.borrow().pr_comments.get(&comment_id).copied();
        if let Some(number) = number {
            self.pr(number)?;
        }
        self.github.update_comment(repo, comment_id, body)
    }
    fn put_file(
        &self,
        repo: &RepoSlug,
        branch: &str,
        path: &str,
        content: &str,
        message: &str,
    ) -> Write<String> {
        self.github.put_file(repo, branch, path, content, message)
    }
    fn create_pull(
        &self,
        repo: &RepoSlug,
        head: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> Write<u64> {
        self.github.create_pull(repo, head, base, title, body)
    }
    fn dispatch_workflow(
        &self,
        repo: &RepoSlug,
        workflow: &str,
        git_ref: &str,
        inputs: &[(String, String)],
    ) -> Write<()> {
        self.github
            .dispatch_workflow(repo, workflow, git_ref, inputs)
    }
    fn request_reviewers(&self, repo: &RepoSlug, number: u64, logins: &[String]) -> Write<()> {
        self.pr(number)?;
        self.github.request_reviewers(repo, number, logins)
    }
    fn dismiss_review(
        &self,
        repo: &RepoSlug,
        number: u64,
        review_id: u64,
        message: &str,
    ) -> Write<()> {
        self.pr(number)?;
        self.github.dismiss_review(repo, number, review_id, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::fake::{FakeGitHub, pull_request};

    const LABEL: &str = "spl-asdlc:hands-off";

    #[test]
    fn a_write_to_an_item_under_the_label_is_refused_before_the_tracker() {
        let t = crate::tracker::fake::FakeTracker::default();
        let g = FakeGitHub::new();
        t.seed(
            "ABC-1",
            "t",
            "d",
            Status::InReview,
            &["Spl-Asdlc:Hands-Off"],
        );
        t.seed("ABC-2", "t", "d", Status::InReview, &[]);
        let guard = HandsOff::new(LABEL, &t, &g);
        // Never read this tick: the guard reads it first.
        assert!(guard.set_status("ABC-1", Status::ToDeploy, None).is_err());
        assert!(Tracker::comment(&guard, "ABC-1", "x").is_err());
        assert!(
            guard
                .relate("ABC-2", "ABC-1", RelationKind::Blocks)
                .is_err()
        );
        let child = NewItem {
            title: "child".into(),
            description: String::new(),
            labels: Vec::new(),
            status: Status::Backlog,
            priority: None,
            assignee: None,
            parent: Some("ABC-1".into()),
        };
        assert!(guard.create_item(&child).is_err());
        assert_eq!(guard.take_refused(), vec!["ABC-1"; 4]);
        assert!(guard.take_refused().is_empty(), "taken once");
        assert!(
            t.writes().is_empty(),
            "the tracker saw no write: {:?}",
            t.writes()
        );
        // Another item is written as usual.
        guard.set_status("ABC-2", Status::ToDeploy, None).unwrap();
        assert_eq!(t.writes().len(), 1);
    }

    #[test]
    fn a_write_to_a_pr_read_under_the_label_is_refused_before_github() {
        let t = crate::tracker::fake::FakeTracker::default();
        let g = FakeGitHub::new();
        let repo = RepoSlug::parse("example-org/acme").unwrap();
        let mut held = pull_request(7);
        held.labels = vec![LABEL.to_string()];
        g.state
            .borrow_mut()
            .open_pulls
            .insert(repo.clone(), Ok(vec![held, pull_request(8)]));
        let guard = HandsOff::new(LABEL, &t, &g);
        guard.open_pulls(&repo, None).unwrap();
        assert!(
            guard
                .merge_pull(&repo, 7, "h", MergeMethod::Squash)
                .is_err()
        );
        assert!(GitHub::comment(&guard, &repo, 7, "x").is_err());
        assert!(guard.dismiss_review(&repo, 7, 1, "x").is_err());
        assert!(guard.request_reviewers(&repo, 7, &["a".into()]).is_err());
        assert_eq!(guard.take_refused(), vec!["pr#7"; 4]);
        assert!(g.writes().is_empty(), "{:?}", g.writes());
        guard
            .merge_pull(&repo, 8, "h", MergeMethod::Squash)
            .unwrap();
        assert_eq!(g.writes().len(), 1);
    }
}
