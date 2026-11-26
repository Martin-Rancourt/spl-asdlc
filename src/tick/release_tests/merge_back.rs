//! A published release whose merge back into `main` is blocked (ASDLC-PQ5J):
//! the cause said on the release item, one fix ticket, its
//! `chore/merge-back-*` PR merged as a merge commit only when its branch
//! holds the release branch's tip, the replay dispatched once, and the
//! closure closing the fix ticket, also when a person resolved it by hand.

use super::*;

const MERGE_BACK_PR: u64 = 40;
const RESOLUTION_PR: u64 = 41;

/// The jobs of a publication run whose deployment went through and whose
/// merge back concluded red.
fn red_close_jobs() -> Vec<crate::github::Job> {
    [
        ("verify / verify", "success"),
        ("verify / merge back into main branch", "skipped"),
        ("verify / closing-guard", "success"),
        ("publish", "success"),
        ("close / verify", "skipped"),
        ("close / merge back into main branch", "failure"),
        ("close / closing-guard", "success"),
    ]
    .into_iter()
    .map(|(name, c)| crate::github::Job {
        name: name.into(),
        conclusion: Some(c.into()),
    })
    .collect()
}

/// The merge-back PR the publication opened, `BRANCH` into `main`.
fn merge_back_pr(merge_state: &str, check: &str) -> PullRequest {
    let mut pr = pull_request(MERGE_BACK_PR);
    pr.head_ref = BRANCH.into();
    pr.head_sha = "c1".into();
    pr.title = format!("chore(release): merge {BRANCH} back into main");
    pr.author = Some(Actor {
        login: bot("spl-asdlc"),
        is_bot: true,
    });
    pr.merge_state = merge_state.into();
    if merge_state == "DIRTY" {
        pr.mergeable = crate::github::Mergeable::Conflicting;
    }
    pr.checks = vec![CheckEntry {
        kind: CheckKind::Run,
        name: "required-check".into(),
        workflow: Some("checks".into()),
        status: Some("COMPLETED".into()),
        conclusion: Some(check.into()),
        started_at: None,
        details_url: None,
    }];
    pr
}

/// A `chore/merge-back-*` PR into `main`, green, at `head`.
fn resolution_pr(head: &str) -> PullRequest {
    let mut pr = green(pull_request(RESOLUTION_PR));
    pr.head_ref = "chore/merge-back-2026-10-05-15-00-utc".into();
    pr.head_sha = head.into();
    pr.title = "chore(release): merge back acme-v1.2.0 (ABC-9)".into();
    pr
}

impl World {
    /// rc.1 released on its GO, its publication deployed and its merge back
    /// red, the merge-back PR open as given, `main` without the release.
    fn blocked(pr: PullRequest) -> World {
        let mut w = World::without_angles();
        w.config.release.fix_assignee = Some("fixer-handle".into());
        w.tracker
            .state
            .borrow_mut()
            .users
            .push(crate::tracker::User {
                id: "u-fixer".into(),
                name: "fixer-handle".into(),
            });
        let r = w.tick();
        assert_eq!(w.go_writes(), go_lines("c1"), "{r:?}");
        let mut s = w.github.state.borrow_mut();
        let run = publication_run(50, "c1", Some("failure"));
        s.jobs.insert((repo(), run.id), Ok(red_close_jobs()));
        s.runs.insert((repo(), PUBLISH_WF.into()), Ok(vec![run]));
        s.compares.insert(
            (repo(), RELEASE_TAG.into(), "main".into()),
            cmp(CompareStatus::Behind, &[], &[]),
        );
        s.required_checks.insert(
            (repo(), "main".into()),
            Ok(RequiredChecks {
                contexts: ["required-check".to_string()].into(),
                strict: false,
            }),
        );
        s.open_pulls.insert(repo(), Ok(vec![pr]));
        drop(s);
        w
    }

    fn fix_tickets(&self) -> Vec<crate::tracker::Item> {
        let release = self.release_item();
        self.tracker
            .children(&release.key)
            .unwrap()
            .into_iter()
            .filter(|c| texts::merge_back_fix_tag(&c.description).is_some())
            .collect()
    }

    fn blocked_comments(&self) -> Vec<String> {
        let release = self.release_item();
        self.tracker
            .comments(&release.key)
            .unwrap()
            .into_iter()
            .map(|c| c.body)
            .filter(|b| b.contains("spl-asdlc:merge-back-blocked"))
            .collect()
    }

    fn open_resolution(&self, pr: PullRequest, holds_tip: CompareStatus) {
        let mut s = self.github.state.borrow_mut();
        s.compares.insert(
            (repo(), "c1".into(), pr.head_sha.clone()),
            cmp(holds_tip, &[], &[]),
        );
        s.comments.insert((repo(), pr.number), Ok(Vec::new()));
        if let Some(Ok(open)) = s.open_pulls.get_mut(&repo()) {
            open.push(pr);
        }
    }

    /// The release tag is in `main` now; the release branch still there
    /// unless `deleted`.
    fn merged_back(&self, deleted: bool) {
        let mut s = self.github.state.borrow_mut();
        s.compares.insert(
            (repo(), RELEASE_TAG.into(), "main".into()),
            cmp(CompareStatus::Ahead, &[], &[]),
        );
        s.open_pulls.insert(repo(), Ok(Vec::new()));
        if deleted {
            s.branches.insert(repo(), Vec::new());
        }
    }

    fn writes_starting(&self, prefix: &str) -> Vec<String> {
        self.github
            .writes()
            .into_iter()
            .filter(|l| l.starts_with(prefix))
            .collect()
    }
}

#[test]
fn a_dirty_merge_back_is_said_once_and_gets_one_fix_ticket_across_passes() {
    let w = World::blocked(merge_back_pr("DIRTY", "SUCCESS"));
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let said = w.blocked_comments();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].contains("real conflict") && said[0].contains("pull/40"),
        "{}",
        said[0]
    );
    let fixes = w.fix_tickets();
    assert_eq!(fixes.len(), 1);
    let fix = &fixes[0];
    assert_eq!(
        fix.title,
        format!("Make the merge back green — {RELEASE_TAG}")
    );
    assert_eq!(fix.state.status, Some(Status::Ready));
    assert_eq!(
        fix.assignee.as_ref().map(|u| u.name.as_str()),
        Some("fixer-handle")
    );
    assert_eq!(fix.parent.as_deref(), Some(w.release_item().key.as_str()));
    for needle in [
        "chore/merge-back-<stamp>",
        &format!("cut from the tip of `{BRANCH}`"),
        "`main` is merged into it",
        "Its PR into `main`",
        "naming this ticket",
        "Never bring `main` into `release/**`",
    ] {
        assert!(
            fix.description.contains(needle),
            "{needle}: {}",
            fix.description
        );
    }
    assert!(
        crate::method::contract::Verdict::of(Some(&fix.description))
            == crate::method::contract::Verdict::Complete,
        "a complete contract"
    );
    // A later pass on the same block writes nothing more.
    let (gh, tr) = (w.github.writes().len(), w.tracker.writes().len());
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.fix_tickets().len(), 1);
    assert_eq!(
        (w.github.writes().len(), w.tracker.writes().len()),
        (gh, tr),
        "{:?}",
        &w.tracker.writes()[tr..]
    );
}

#[test]
fn a_red_required_check_and_an_app_refusal_are_each_read_as_the_cause() {
    let w = World::blocked(merge_back_pr("BLOCKED", "FAILURE"));
    w.tick();
    let said = w.blocked_comments();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].contains("`required-check`") && said[0].contains("red"),
        "{}",
        said[0]
    );
    assert_eq!(w.fix_tickets().len(), 1);

    let w = World::blocked(merge_back_pr("BLOCKED", "SUCCESS"));
    w.tick();
    let said = w.blocked_comments();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].contains("refused the App's merge"), "{}", said[0]);
    assert_eq!(w.fix_tickets().len(), 1);
}

#[test]
fn a_merge_back_still_pending_or_a_close_still_running_opens_nothing() {
    let w = World::blocked(merge_back_pr("BLOCKED", "IN_PROGRESS"));
    w.tick();
    assert!(w.fix_tickets().is_empty());
    assert!(w.blocked_comments().is_empty());

    let w = World::blocked(merge_back_pr("DIRTY", "SUCCESS"));
    {
        let mut s = w.github.state.borrow_mut();
        let mut run = publication_run(50, "c1", None);
        run.status = "in_progress".into();
        s.runs.insert((repo(), PUBLISH_WF.into()), Ok(vec![run]));
    }
    w.tick();
    assert!(w.fix_tickets().is_empty(), "the close still polls its PR");
}

#[test]
fn the_resolution_pr_is_merged_as_a_merge_commit_then_replayed_once_and_the_closure_closes_the_fix()
{
    let w = World::blocked(merge_back_pr("DIRTY", "SUCCESS"));
    w.tick();
    assert_eq!(w.fix_tickets().len(), 1);
    w.open_resolution(resolution_pr("h41"), CompareStatus::Ahead);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        w.writes_starting("merge "),
        vec![format!(
            "merge example-org/acme#{RESOLUTION_PR} at h41 merge"
        )],
        "a merge commit, never a squash"
    );
    // Merged: the tag is in main, the branch still there; the replay once.
    w.merged_back(false);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let replay = format!("dispatch example-org/acme {PUBLISH_WF} on {BRANCH} replay=true");
    assert_eq!(w.writes_starting("dispatch "), vec![replay.clone()]);
    w.tick();
    assert_eq!(
        w.writes_starting("dispatch "),
        vec![replay],
        "dispatched once"
    );
    // The replay deleted the branch: the closure closes the release.
    w.merged_back(true);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let fix = &w.fix_tickets()[0];
    assert_eq!(fix.state.status, Some(Status::Done));
    assert_eq!(w.tracker.item("ABC-5").state.status, Some(Status::Done));
    assert_eq!(
        w.tracker.item(&w.release_item().key).state.status,
        Some(Status::Done)
    );
}

#[test]
fn a_resolution_pr_without_the_release_tip_is_refused_and_said_once_per_head() {
    let w = World::blocked(merge_back_pr("DIRTY", "SUCCESS"));
    w.tick();
    w.open_resolution(resolution_pr("h42"), CompareStatus::Diverged);
    w.tick();
    w.tick();
    assert!(w.writes_starting("merge ").is_empty(), "never merged");
    let refusals: Vec<String> = w
        .writes_starting("comment ")
        .into_iter()
        .filter(|l| l.contains(&format!("#{RESOLUTION_PR}")))
        .collect();
    assert_eq!(refusals.len(), 1, "{refusals:?}");
}

#[test]
fn a_red_resolution_pr_is_not_merged() {
    let w = World::blocked(merge_back_pr("DIRTY", "SUCCESS"));
    w.tick();
    let mut pr = resolution_pr("h43");
    pr.checks[0].conclusion = Some("FAILURE".into());
    w.open_resolution(pr, CompareStatus::Ahead);
    w.tick();
    assert!(w.writes_starting("merge ").is_empty());
}

#[test]
fn a_merge_back_resolved_by_hand_closes_the_release_and_its_fix_ticket() {
    let w = World::blocked(merge_back_pr("DIRTY", "SUCCESS"));
    w.tick();
    assert_eq!(w.fix_tickets().len(), 1);
    // A person merged the resolution and replayed: the branch is gone.
    w.merged_back(true);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert!(w.writes_starting("merge ").is_empty());
    assert!(w.writes_starting("dispatch ").is_empty());
    let fix = &w.fix_tickets()[0];
    assert_eq!(fix.state.status, Some(Status::Done));
    let comments = w.tracker.comments(&fix.key).unwrap();
    assert!(
        comments
            .iter()
            .any(|c| c.body.contains("spl-asdlc:merge-back-fix-closed")),
        "{comments:?}"
    );
    assert_eq!(
        w.tracker.item(&w.release_item().key).state.status,
        Some(Status::Done)
    );
}
