//! `tick --dry-run`: the real reads, no write. The wrappers pass every read
//! to the real client and answer every write as if it had succeeded,
//! recording what would have been written; the tick then logs each action as
//! [`Outcome::WouldDo`](crate::action_log::Outcome::WouldDo).

use std::cell::RefCell;

use crate::github::{self, GitHub, Read, Release, RepoSlug, Timestamp, Write};
use crate::method::vocabulary::{ItemState, Status};
use crate::notify::Signal;
use crate::tracker::{Comment, Item, NewItem, RelationKind, Relations, TResult, Tracker, User};

/// The writes a dry run would have made, in order.
#[derive(Debug, Default)]
pub struct Intents(RefCell<Vec<String>>);

impl Intents {
    pub fn push(&self, line: String) {
        self.0.borrow_mut().push(line);
    }

    pub fn lines(&self) -> Vec<String> {
        self.0.borrow().clone()
    }
}

pub struct DryGitHub<'a> {
    pub inner: &'a dyn GitHub,
    pub intents: &'a Intents,
}

impl GitHub for DryGitHub<'_> {
    fn identity(&self) -> Read<github::Identity> {
        self.inner.identity()
    }
    fn default_branch(&self, repo: &RepoSlug) -> Read<String> {
        self.inner.default_branch(repo)
    }
    fn variable(&self, repo: &RepoSlug, name: &str) -> Read<github::Variable> {
        self.inner.variable(repo, name)
    }
    fn branches(&self, repo: &RepoSlug, prefix: &str) -> Read<Vec<github::GitRef>> {
        self.inner.branches(repo, prefix)
    }
    fn tags(&self, repo: &RepoSlug, prefix: &str) -> Read<Vec<github::GitRef>> {
        self.inner.tags(repo, prefix)
    }
    fn tag_object(&self, repo: &RepoSlug, sha: &str) -> Read<github::AnnotatedTag> {
        self.inner.tag_object(repo, sha)
    }
    fn commit(&self, repo: &RepoSlug, rev: &str) -> Read<github::Commit> {
        self.inner.commit(repo, rev)
    }
    fn compare(&self, repo: &RepoSlug, base: &str, head: &str) -> Read<github::Comparison> {
        self.inner.compare(repo, base, head)
    }
    fn since_root(&self, repo: &RepoSlug, head: &str) -> Read<github::Comparison> {
        self.inner.since_root(repo, head)
    }
    fn file(&self, repo: &RepoSlug, path: &str, rev: &str) -> Read<String> {
        self.inner.file(repo, path, rev)
    }
    fn file_from(&self, repo: &RepoSlug, source: &RepoSlug, path: &str, rev: &str) -> Read<String> {
        self.inner.file_from(repo, source, path, rev)
    }
    fn dir(&self, repo: &RepoSlug, path: &str, rev: &str) -> Read<Vec<String>> {
        self.inner.dir(repo, path, rev)
    }
    fn open_pulls(&self, repo: &RepoSlug, base: Option<&str>) -> Read<Vec<github::PullRequest>> {
        self.inner.open_pulls(repo, base)
    }
    fn pull(&self, repo: &RepoSlug, number: u64) -> Read<github::PullRequest> {
        self.inner.pull(repo, number)
    }

    fn updated_pulls(
        &self,
        repo: &RepoSlug,
        since: Option<github::Timestamp>,
        handled_at_since: &[u64],
        limit: usize,
    ) -> Read<Vec<github::PullRequest>> {
        self.inner
            .updated_pulls(repo, since, handled_at_since, limit)
    }
    fn merged_pulls(
        &self,
        repo: &RepoSlug,
        base: Option<&str>,
        since: Option<Timestamp>,
        limit: usize,
    ) -> Read<Vec<github::PullRequest>> {
        self.inner.merged_pulls(repo, base, since, limit)
    }
    fn closed_pulls(
        &self,
        repo: &RepoSlug,
        since: Timestamp,
        limit: usize,
    ) -> Read<Vec<github::PullRequest>> {
        self.inner.closed_pulls(repo, since, limit)
    }
    fn pull_files(&self, repo: &RepoSlug, number: u64) -> Read<Vec<String>> {
        self.inner.pull_files(repo, number)
    }
    fn commit_pulls(&self, repo: &RepoSlug, sha: &str) -> Read<Vec<github::CommitPull>> {
        self.inner.commit_pulls(repo, sha)
    }
    fn required_checks(&self, repo: &RepoSlug, branch: &str) -> Read<github::RequiredChecks> {
        self.inner.required_checks(repo, branch)
    }
    fn rulesets(&self, repo: &RepoSlug) -> Read<Vec<github::Ruleset>> {
        self.inner.rulesets(repo)
    }
    fn issue_comments(&self, repo: &RepoSlug, number: u64) -> Read<Vec<github::Comment>> {
        self.inner.issue_comments(repo, number)
    }
    fn release(&self, repo: &RepoSlug, tag: &str) -> Read<github::Release> {
        self.inner.release(repo, tag)
    }
    fn workflow_runs(
        &self,
        repo: &RepoSlug,
        workflow: &str,
        branch: Option<&str>,
        head_sha: Option<&str>,
        event: Option<&str>,
    ) -> Read<Vec<github::WorkflowRun>> {
        self.inner
            .workflow_runs(repo, workflow, branch, head_sha, event)
    }
    fn run_jobs(&self, repo: &RepoSlug, run_id: u64) -> Read<Vec<github::Job>> {
        self.inner.run_jobs(repo, run_id)
    }
    fn run_artifacts(&self, repo: &RepoSlug, run_id: u64) -> Read<Vec<github::Artifact>> {
        self.inner.run_artifacts(repo, run_id)
    }
    fn artifact_file(&self, repo: &RepoSlug, artifact_id: u64, path: &str) -> Read<String> {
        self.inner.artifact_file(repo, artifact_id, path)
    }

    fn merge_pull(
        &self,
        repo: &RepoSlug,
        number: u64,
        head_sha: &str,
        method: github::MergeMethod,
    ) -> Write<String> {
        self.intents.push(format!(
            "github {repo}: merge #{number} at {head_sha} ({})",
            method.as_str()
        ));
        Ok(format!("dry-run-merge-of-{number}"))
    }
    fn update_branch(&self, repo: &RepoSlug, number: u64, head_sha: &str) -> Write<()> {
        self.intents.push(format!(
            "github {repo}: update branch of #{number} at {head_sha}"
        ));
        Ok(())
    }
    fn delete_branch(&self, repo: &RepoSlug, branch: &str) -> Write<()> {
        self.intents
            .push(format!("github {repo}: delete branch {branch}"));
        Ok(())
    }
    fn create_tag(&self, repo: &RepoSlug, tag: &str, sha: &str, _message: &str) -> Write<()> {
        self.intents
            .push(format!("github {repo}: create tag {tag} at {sha}"));
        Ok(())
    }
    fn create_release(&self, repo: &RepoSlug, tag: &str, name: &str, body: &str) -> Write<Release> {
        self.intents
            .push(format!("github {repo}: publish release {tag} ({name})"));
        Ok(Release {
            id: 0,
            tag: tag.to_string(),
            url: format!("(dry run) release {tag}"),
            body: body.to_string(),
            draft: false,
            prerelease: false,
            published_at: None,
            author: None,
        })
    }
    fn delete_release(&self, repo: &RepoSlug, release_id: u64) -> Write<()> {
        self.intents.push(format!(
            "github {repo}: delete release {release_id} (would retract)"
        ));
        Ok(())
    }
    fn delete_tag(&self, repo: &RepoSlug, tag: &str) -> Write<()> {
        self.intents
            .push(format!("github {repo}: delete tag {tag} (would retract)"));
        Ok(())
    }
    fn create_branch(&self, repo: &RepoSlug, branch: &str, sha: &str) -> Write<()> {
        self.intents
            .push(format!("github {repo}: create branch {branch} at {sha}"));
        Ok(())
    }
    fn add_labels(&self, repo: &RepoSlug, number: u64, labels: &[String]) -> Write<()> {
        self.intents.push(format!(
            "github {repo}: label #{number} +{}",
            labels.join(" +")
        ));
        Ok(())
    }
    fn remove_label(&self, repo: &RepoSlug, number: u64, label: &str) -> Write<()> {
        self.intents
            .push(format!("github {repo}: label #{number} -{label}"));
        Ok(())
    }
    fn comment(&self, repo: &RepoSlug, number: u64, body: &str) -> Write<github::Comment> {
        self.intents.push(format!(
            "github {repo}: comment on #{number}: {}",
            first_line(body)
        ));
        Ok(github::Comment {
            id: 0,
            author: None,
            body: body.to_string(),
            created_at: None,
            url: String::new(),
        })
    }
    fn update_comment(&self, repo: &RepoSlug, comment_id: u64, body: &str) -> Write<()> {
        self.intents.push(format!(
            "github {repo}: edit comment {comment_id}: {}",
            first_line(body)
        ));
        Ok(())
    }
    fn put_file(
        &self,
        repo: &RepoSlug,
        branch: &str,
        path: &str,
        _content: &str,
        message: &str,
    ) -> Write<String> {
        self.intents.push(format!(
            "github {repo}: write {path} on {branch} ({message})"
        ));
        Ok("dry-run-commit".to_string())
    }
    fn create_pull(
        &self,
        repo: &RepoSlug,
        head: &str,
        base: &str,
        title: &str,
        _body: &str,
    ) -> Write<u64> {
        self.intents
            .push(format!("github {repo}: open PR {head} → {base}: {title}"));
        Ok(0)
    }
    fn dispatch_workflow(
        &self,
        repo: &RepoSlug,
        workflow: &str,
        git_ref: &str,
        inputs: &[(String, String)],
    ) -> Write<()> {
        let inputs: Vec<String> = inputs.iter().map(|(k, v)| format!("{k}={v}")).collect();
        self.intents.push(format!(
            "github {repo}: dispatch {workflow} on {git_ref} {}",
            inputs.join(" ")
        ));
        Ok(())
    }
    fn request_reviewers(&self, repo: &RepoSlug, number: u64, logins: &[String]) -> Write<()> {
        self.intents.push(format!(
            "github {repo}: request review of #{number} from {}",
            logins.join(", ")
        ));
        Ok(())
    }
    fn dismiss_review(
        &self,
        repo: &RepoSlug,
        number: u64,
        review_id: u64,
        message: &str,
    ) -> Write<()> {
        self.intents.push(format!(
            "github {repo}: dismiss review {review_id} on #{number}: {message}"
        ));
        Ok(())
    }
}

pub struct DryTracker<'a> {
    pub inner: &'a dyn Tracker,
    pub intents: &'a Intents,
}

/// The key of an item a dry run would create.
const NEW_KEY: &str = "(new: ";

fn would_create(key: &str) -> bool {
    key.starts_with(NEW_KEY)
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

impl Tracker for DryTracker<'_> {
    fn identity(&self) -> TResult<User> {
        self.inner.identity()
    }
    fn team_key(&self) -> TResult<String> {
        self.inner.team_key()
    }
    fn status_names(&self) -> TResult<Vec<String>> {
        self.inner.status_names()
    }
    fn user(&self, reference: &str) -> TResult<User> {
        self.inner.user(reference)
    }
    fn project(&self, reference: &str) -> TResult<Option<String>> {
        self.inner.project(reference)
    }
    fn get(&self, key: &str) -> TResult<Item> {
        self.inner.get(key)
    }
    // An item this dry run would have created exists nowhere: it has no
    // relation, comment or child yet, as it would have none once created.
    fn relations(&self, key: &str) -> TResult<Relations> {
        if would_create(key) {
            return Ok(Relations::default());
        }
        self.inner.relations(key)
    }
    fn comments(&self, key: &str) -> TResult<Vec<Comment>> {
        if would_create(key) {
            return Ok(Vec::new());
        }
        self.inner.comments(key)
    }
    fn children(&self, key: &str) -> TResult<Vec<Item>> {
        if would_create(key) {
            return Ok(Vec::new());
        }
        self.inner.children(key)
    }
    fn list_by_label(&self, label: &str) -> TResult<Vec<Item>> {
        self.inner.list_by_label(label)
    }
    fn find_labelled_containing(&self, label: &str, needle: &str) -> TResult<Vec<Item>> {
        self.inner.find_labelled_containing(label, needle)
    }
    fn items_linked_to_pr(&self, pr_url: &str) -> TResult<Vec<String>> {
        self.inner.items_linked_to_pr(pr_url)
    }
    fn label_exists(&self, name: &str) -> TResult<bool> {
        self.inner.label_exists(name)
    }
    fn ensure_label(&self, name: &str) -> TResult<()> {
        // A label some item carries exists: the real write would be a no-op.
        if !self.inner.list_by_label(name)?.is_empty() {
            return Ok(());
        }
        self.intents.push(format!("tracker: ensure label {name}"));
        Ok(())
    }
    fn create_item(&self, item: &NewItem) -> TResult<Item> {
        self.intents.push(format!(
            "tracker: create \"{}\" [{}]{}{}{}",
            item.title,
            item.status,
            item.priority
                .map(|p| format!(" priority {p}"))
                .unwrap_or_default(),
            item.parent
                .as_ref()
                .map(|p| format!(" under {p}"))
                .unwrap_or_default(),
            item.assignee
                .as_ref()
                .map(|u| format!(", assigned to {}", u.name))
                .unwrap_or_default()
        ));
        Ok(Item {
            id: String::new(),
            key: format!("{NEW_KEY}{})", item.title),
            title: item.title.clone(),
            description: item.description.clone(),
            url: String::new(),
            state: ItemState::of(item.status),
            labels: item.labels.clone(),
            assignee: item.assignee.clone(),
            parent: item.parent.clone(),
            archived: false,
        })
    }
    fn update_description(&self, key: &str, _description: &str) -> TResult<()> {
        self.intents
            .push(format!("tracker: rewrite the description of {key}"));
        Ok(())
    }
    fn add_label(&self, key: &str, label: &str) -> TResult<()> {
        self.intents.push(format!("tracker: label {key} +{label}"));
        Ok(())
    }
    fn remove_label(&self, key: &str, label: &str) -> TResult<()> {
        self.intents.push(format!("tracker: label {key} -{label}"));
        Ok(())
    }
    fn set_status(
        &self,
        key: &str,
        status: Status,
        assignee: Option<Option<&User>>,
    ) -> TResult<()> {
        let who = match assignee {
            None => String::new(),
            Some(None) => ", unassigned".to_string(),
            Some(Some(u)) => format!(", assigned to {}", u.name),
        };
        self.intents.push(format!("tracker: {key} → {status}{who}"));
        Ok(())
    }
    fn assign(&self, key: &str, user: Option<&User>) -> TResult<()> {
        self.intents.push(format!(
            "tracker: assign {key} to {}",
            user.map(|u| u.name.as_str()).unwrap_or("nobody")
        ));
        Ok(())
    }
    fn comment(&self, key: &str, body: &str) -> TResult<Comment> {
        self.intents
            .push(format!("tracker: comment on {key}: {}", first_line(body)));
        Ok(Comment {
            id: String::new(),
            author: None,
            by_bridge: true,
            body: body.to_string(),
        })
    }
    fn can_edit_comments(&self) -> bool {
        self.inner.can_edit_comments()
    }
    fn update_comment(&self, comment_id: &str, body: &str) -> TResult<()> {
        self.intents.push(format!(
            "tracker: edit comment {comment_id}: {}",
            first_line(body)
        ));
        Ok(())
    }
    fn relate(&self, a: &str, b: &str, kind: RelationKind) -> TResult<()> {
        self.intents.push(format!("tracker: {a} {kind:?} {b}"));
        Ok(())
    }
    fn unrelate(&self, relation_id: &str) -> TResult<()> {
        self.intents
            .push(format!("tracker: remove relation {relation_id}"));
        Ok(())
    }
    fn link_pr(&self, key: &str, pr_url: &str, _title: &str) -> TResult<()> {
        self.intents
            .push(format!("tracker: link {key} to {pr_url}"));
        Ok(())
    }
    fn archives(&self) -> bool {
        self.inner.archives()
    }
    fn archive(&self, key: &str) -> TResult<()> {
        self.intents.push(format!("tracker: archive {key}"));
        Ok(())
    }
    fn keeps_html_comments(&self) -> bool {
        self.inner.keeps_html_comments()
    }
    fn keys_in_text(&self, text: &str) -> Vec<String> {
        self.inner.keys_in_text(text)
    }
}

/// Notifications a dry run would send.
pub struct DrySignal<'a> {
    pub intents: &'a Intents,
}

impl Signal for DrySignal<'_> {
    fn signal(&self, event: &str, text: &str) -> Result<Vec<String>, String> {
        self.intents
            .push(format!("notify {event}: {}", first_line(text)));
        Ok(vec!["dry-run".to_string()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::fake::FakeGitHub;
    use crate::tracker::fake::FakeTracker;

    #[test]
    fn a_dry_run_reads_for_real_and_writes_nothing() {
        let gh = FakeGitHub::default();
        let t = FakeTracker::default();
        t.seed("ABC-1", "t", "d", Status::InReview, &[]);
        let intents = Intents::default();
        let dry = DryTracker {
            inner: &t,
            intents: &intents,
        };
        assert_eq!(dry.get("ABC-1").unwrap().key, "ABC-1");
        dry.set_status("ABC-1", Status::ToDeploy, None).unwrap();
        dry.comment("ABC-1", "Merged.\nmore").unwrap();
        dry.archive("ABC-1").unwrap();
        assert!(t.writes().is_empty(), "the real tracker saw no write");
        assert_eq!(t.item("ABC-1").state.status, Some(Status::InReview));
        assert!(!t.item("ABC-1").archived);
        assert_eq!(dry.archives(), t.archives(), "the real capability");
        let repo = RepoSlug::parse("acme/widget").unwrap();
        let dgh = DryGitHub {
            inner: &gh,
            intents: &intents,
        };
        dgh.merge_pull(&repo, 7, "abc", github::MergeMethod::Squash)
            .unwrap();
        assert_eq!(
            intents.lines(),
            vec![
                "tracker: ABC-1 → To Deploy",
                "tracker: comment on ABC-1: Merged.",
                "tracker: archive ABC-1",
                "github acme/widget: merge #7 at abc (squash)",
            ]
        );
    }

    #[test]
    fn a_dry_run_shows_only_what_a_real_run_would_write() {
        let t = FakeTracker::default();
        t.seed(
            "ABC-1",
            "t",
            "d",
            Status::InReview,
            &["release/acme-v1.2.0"],
        );
        let intents = Intents::default();
        let dry = DryTracker {
            inner: &t,
            intents: &intents,
        };
        // A label an item carries exists: ensuring it writes nothing.
        dry.ensure_label("release/acme-v1.2.0").unwrap();
        dry.ensure_label("release/acme-v1.3.0").unwrap();
        // An item it would create is read as new: no comment, child or relation.
        let new = dry
            .create_item(&NewItem {
                title: "Release acme v1.3.0".into(),
                description: String::new(),
                labels: Vec::new(),
                status: Status::Ready,
                priority: None,
                assignee: None,
                parent: None,
            })
            .unwrap();
        assert!(dry.comments(&new.key).unwrap().is_empty());
        assert!(dry.children(&new.key).unwrap().is_empty());
        assert_eq!(dry.relations(&new.key).unwrap(), Relations::default());
        assert_eq!(
            intents.lines(),
            vec![
                "tracker: ensure label release/acme-v1.3.0",
                "tracker: create \"Release acme v1.3.0\" [Ready]",
            ]
        );
    }
}
