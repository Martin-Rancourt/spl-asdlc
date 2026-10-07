//! The release pass and the closure against the in-memory GitHub and
//! tracker (spec 05): the cycle opened at the first tick after the cut, the
//! angle PRs merged with their findings filed once, the GO and the merge of
//! the release PR, the closure and the release note.

use super::*;
use crate::config::Config;
use crate::github::fake::{FakeGitHub, pull_request};
use crate::github::{
    Actor, CheckEntry, CheckKind, Commit, CommitPull, CommitSummary, CompareStatus, Comparison,
    GitRef, Identity, ObjectType, PrState, PullRequest, Release, RepoSlug, RequiredChecks, Review,
    Timestamp, WorkflowRun,
};
use crate::method::texts::{self, BridgeItem};
use crate::method::vocabulary::Status;
use crate::notify::Recorder;
use crate::tracker::Tracker;
use crate::tracker::fake::FakeTracker;

const BRANCH: &str = "release/2026-10-05-14-03-utc";
const CAND: &str = "acme-v1.2.0-rc.1";
const LABEL: &str = "release/acme-v1.2.0";

fn repo() -> RepoSlug {
    RepoSlug::parse("example-org/acme").unwrap()
}

fn config() -> Config {
    let spec = include_str!("../../docs/spec/08-configuration.md");
    let start = spec.find("```toml\n").unwrap() + "```toml\n".len();
    let end = start + spec[start..].find("```").unwrap();
    Config::parse(&spec[start..end]).unwrap()
}

fn tag(name: &str, sha: &str) -> GitRef {
    GitRef {
        name: name.into(),
        sha: sha.into(),
        object_type: ObjectType::Commit,
    }
}

fn cmp(status: CompareStatus, commits: &[&str], files: &[&str]) -> crate::github::Read<Comparison> {
    Ok(Comparison {
        status,
        ahead_by: commits.len() as u64,
        behind_by: 0,
        commits: commits
            .iter()
            .map(|c| CommitSummary {
                sha: c.to_string(),
                subject: format!("feat: {c}"),
            })
            .collect(),
        files: files.iter().map(|f| f.to_string()).collect(),
    })
}

fn green(mut pr: PullRequest) -> PullRequest {
    pr.checks = vec![CheckEntry {
        kind: CheckKind::Run,
        name: "required-check".into(),
        workflow: Some("checks".into()),
        status: Some("COMPLETED".into()),
        conclusion: Some("SUCCESS".into()),
        started_at: None,
        details_url: None,
    }];
    pr
}

fn report_json(angle: &str, findings: &str) -> String {
    let verdict = if findings.contains("blocker") {
        "hold"
    } else {
        "go"
    };
    format!(
        r#"{{"schema":"spl-asdlc/qa-report/1","candidate":"{CAND}","angle":"{angle}","reviewer":"reviewer-handle",
           "verdict":"{verdict}","checked":["the whole project"],"findings":[{findings}]}}"#
    )
}

fn finding(id: &str, severity: &str) -> String {
    format!(
        r#"{{"id":"{id}","severity":"{severity}","summary":"Finding {id}","anchor":"src/a.rs:1","evidence":"e","fixed_in_branch":false,"testable":true,"proposed_test":"test_{id}","ticket":null}}"#
    )
}

struct World {
    config: Config,
    github: FakeGitHub,
    tracker: FakeTracker,
    signal: Recorder,
}

impl World {
    /// A cut just happened: the release branch and the first candidate,
    /// one shipped ticket in the range, the gate green.
    fn new() -> World {
        let github = FakeGitHub::new();
        {
            let mut s = github.state.borrow_mut();
            s.identity = Some(Identity::App {
                slug: "spl-asdlc".into(),
                app_id: "1".into(),
            });
            s.default_branch.insert(repo(), "main".into());
            s.tags
                .insert(repo(), vec![tag("acme-v1.1.0", "r0"), tag(CAND, "c1")]);
            s.branches.insert(repo(), vec![tag(BRANCH, "c1")]);
            s.commits.insert(
                (repo(), "c1".into()),
                Ok(Commit {
                    sha: "c1".into(),
                    message: "chore: candidate".into(),
                    committer_date: Timestamp::parse("2026-10-05T11:30:00Z"),
                    parents: vec![],
                }),
            );
            s.compares.insert(
                (repo(), "c1".into(), BRANCH.into()),
                cmp(CompareStatus::Identical, &[], &[]),
            );
            s.compares.insert(
                (repo(), "acme-v1.1.0".into(), "c1".into()),
                cmp(CompareStatus::Ahead, &["m1"], &["src/a.rs"]),
            );
            s.commit_pulls.insert(
                (repo(), "m1".into()),
                Ok(vec![CommitPull {
                    number: 5,
                    url: "https://github.com/example-org/acme/pull/5".into(),
                    merged_at: None,
                    merge_commit_sha: Some("m1".into()),
                    base_ref: "main".into(),
                    head_ref: "feat/five".into(),
                }]),
            );
            s.runs.insert(
                (repo(), "release-gate.yml".into()),
                Ok(vec![WorkflowRun {
                    id: 1,
                    head_sha: "c1".into(),
                    head_branch: CAND.into(),
                    event: "push".into(),
                    status: "completed".into(),
                    conclusion: Some("success".into()),
                    url: "https://github.com/example-org/acme/actions/runs/1".into(),
                    created_at: None,
                }]),
            );
            s.open_pulls.insert(repo(), Ok(Vec::new()));
            s.merged_pulls.insert(repo(), Ok(Vec::new()));
            s.required_checks.insert(
                (repo(), BRANCH.into()),
                Ok(RequiredChecks {
                    contexts: ["required-check".to_string()].into(),
                    strict: false,
                }),
            );
            s.files.insert(
                (repo(), "main".into(), ".github/review-paths.json".into()),
                Ok(r#"{"paths": ["**/auth/**"]}"#.into()),
            );
        }
        let tracker = FakeTracker::new("ABC");
        tracker.seed(
            "ABC-5",
            "Shipped",
            "## Goal\nShip it.\n\n## Acceptance criteria\n- [x] shipped\n\n## Business rules\nNo new rule.\n\n## Release note\nYou can now ship.\n",
            Status::ToDeploy,
            &[],
        );
        tracker.link("ABC-5", "https://github.com/example-org/acme/pull/5");
        World {
            config: config(),
            github,
            tracker,
            signal: Recorder::default(),
        }
    }

    fn tick(&self) -> Report {
        let t = Tick {
            config: &self.config,
            repo: repo(),
            github: &self.github,
            tracker: &self.tracker,
            signal: &self.signal,
            github_login: bot("spl-asdlc"),
            now: Timestamp::parse("2026-10-05T12:00:00Z").unwrap(),
            deadline: None,
            dry_run: false,
        };
        let mut log = ActionLog::in_memory();
        run(&t, &mut log)
    }

    fn release_item(&self) -> crate::tracker::Item {
        self.tracker
            .list_by_label(LABEL)
            .unwrap()
            .into_iter()
            .find(|i| {
                matches!(
                    texts::bridge_item(&i.description, None),
                    Some(BridgeItem::Release { .. })
                )
            })
            .expect("a release item")
    }

    fn sub_item(&self, angle: &str) -> crate::tracker::Item {
        let release = self.release_item();
        self.tracker
            .children(&release.key)
            .unwrap()
            .into_iter()
            .find(|c| {
                texts::bridge_item(&c.description, c.parent.as_deref())
                    == Some(BridgeItem::Angle {
                        scope: CAND.into(),
                        angle: angle.into(),
                    })
            })
            .expect("a sub-item")
    }

    fn open_on_branch(&self, prs: Vec<PullRequest>) {
        self.github
            .state
            .borrow_mut()
            .open_pulls
            .insert(repo(), Ok(prs));
    }

    fn angle_pr(&self, number: u64, angle: &str, report: &str) -> PullRequest {
        let sub = self.sub_item(angle);
        let mut pr = green(pull_request(number));
        pr.base_ref = BRANCH.into();
        pr.head_ref = format!("qa/{CAND}/{angle}");
        pr.body = format!("Angle {angle}, sub-item {}", sub.key);
        pr.reviews = vec![Review {
            author: Some("coordinator-login".into()),
            body: "ok".into(),
            state: "APPROVED".into(),
            commit_sha: Some(pr.head_sha.clone()),
            submitted_at: None,
        }];
        let path = format!(".spl-asdlc/qa/{CAND}/{angle}.json");
        let mut s = self.github.state.borrow_mut();
        s.files.insert(
            (repo(), pr.head_sha.clone(), path.clone()),
            Ok(report.to_string()),
        );
        s.files.insert(
            (repo(), format!("merge-of-{number}"), path),
            Ok(report.to_string()),
        );
        pr
    }
}

#[test]
fn the_first_tick_after_the_cut_opens_the_cycle_and_the_second_writes_nothing() {
    let w = World::new();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let item = w.release_item();
    assert_eq!(item.title, "Release acme v1.2.0");
    assert!(
        item.description.contains(&format!("`{CAND}`")),
        "candidate line"
    );
    for angle in ["security", "product"] {
        assert_eq!(w.sub_item(angle).state.status, Some(Status::Ready));
    }
    let comments = w.tracker.comments(&item.key).unwrap();
    assert!(
        comments
            .iter()
            .any(|c| c.body.starts_with(&format!("### Candidate `{CAND}`")))
    );
    assert!(comments.iter().any(|c| c.body.contains("Release brief")));
    assert!(
        w.tracker.item("ABC-5").labels.contains(&LABEL.to_string()),
        "embarked"
    );
    assert!(w.github.writes().is_empty(), "nothing to merge yet");

    let before = w.tracker.writes().len();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        w.tracker.writes().len(),
        before,
        "{:?}",
        &w.tracker.writes()[before..]
    );
}

#[test]
fn an_approved_angle_pr_with_a_valid_report_is_merged_and_its_findings_filed_once() {
    let w = World::new();
    w.tick();
    let findings = [
        finding("security-1", "blocker"),
        finding("security-2", "should-fix"),
        finding("security-3", "nit"),
    ]
    .join(",");
    let pr = w.angle_pr(30, "security", &report_json("security", &findings));
    w.open_on_branch(vec![pr.clone()]);
    w.tick();
    assert!(w.github.writes().contains(&format!(
        "merge example-org/acme#30 at {} merge",
        pr.head_sha
    )));
    assert_eq!(w.sub_item("security").state.status, Some(Status::Done));
    // The fake moved the PR to the merged list; the next tick files the findings.
    w.tick();
    let release = w.release_item();
    let labelled = w.tracker.list_by_label(LABEL).unwrap();
    let blocker = labelled
        .iter()
        .find(|i| i.title.starts_with("Finding security-1"))
        .expect("blocker");
    assert!(
        w.tracker
            .relations(&release.key)
            .unwrap()
            .blocked_by
            .iter()
            .any(|b| b.other == blocker.key)
    );
    assert!(
        labelled
            .iter()
            .any(|i| i.title.starts_with("Finding security-2")
                && i.labels.contains(&"qa-finding".to_string()))
    );
    let nits = labelled
        .iter()
        .find(|i| i.title == "Nits — acme v1.2.0")
        .expect("nits");
    assert!(nits.description.contains("src/a.rs:1: Finding security-3"));
    let before = w.tracker.writes().len();
    w.tick();
    assert_eq!(
        w.tracker.writes().len(),
        before,
        "filed once: {:?}",
        &w.tracker.writes()[before..]
    );
}

#[test]
fn a_report_that_does_not_validate_holds_the_angle_and_is_said_once() {
    let w = World::new();
    w.tick();
    let pr = w.angle_pr(30, "security", "{not json");
    w.open_on_branch(vec![pr]);
    w.tick();
    w.tick();
    assert!(!w.github.writes().iter().any(|l| l.starts_with("merge")));
    let sub = w.sub_item("security");
    let said: Vec<_> = w
        .tracker
        .comments(&sub.key)
        .unwrap()
        .into_iter()
        .filter(|c| c.body.contains("has no valid report"))
        .collect();
    assert_eq!(said.len(), 1);
}

fn release_pr(w: &World, staged: &str) -> PullRequest {
    let mut pr = pull_request(40);
    pr.base_ref = BRANCH.into();
    pr.head_ref = "release-please--branches--release".into();
    pr.title = format!("chore(release): release acme {staged}");
    pr.body = format!("<summary>acme: {staged}</summary>");
    pr.labels = vec!["autorelease: pending".into()];
    pr.author = Some(Actor {
        login: bot("release-app"),
        is_bot: true,
    });
    let changelog = format!("## [{staged}](x) (2026-10-05)\n\n### Features\n\n* m1 ([m1](x))\n");
    w.github.state.borrow_mut().files.insert(
        (repo(), pr.head_sha.clone(), "CHANGELOG.md".into()),
        Ok(changelog),
    );
    pr
}

fn all_angles_merged(w: &World) {
    let mut merged = Vec::new();
    for (n, angle) in [(30, "security"), (31, "product")] {
        let mut pr = w.angle_pr(n, angle, &report_json(angle, ""));
        pr.state = PrState::Merged;
        pr.merge_commit_sha = Some(format!("merge-of-{n}"));
        merged.push(pr);
    }
    w.github
        .state
        .borrow_mut()
        .merged_pulls
        .insert(repo(), Ok(merged));
}

#[test]
fn the_go_computed_merges_the_release_pr_and_nothing_else() {
    let w = World::new();
    w.tick();
    all_angles_merged(&w);
    let rp = release_pr(&w, "1.2.0");
    w.open_on_branch(vec![rp.clone()]);
    let r = w.tick();
    let merges: Vec<String> = w
        .github
        .writes()
        .into_iter()
        .filter(|l| l.starts_with("merge"))
        .collect();
    assert_eq!(
        merges,
        vec![format!(
            "merge example-org/acme#40 at {} squash",
            rp.head_sha
        )],
        "{r:?}"
    );
}

#[test]
fn a_release_pr_staging_another_version_is_no_go() {
    let w = World::new();
    w.tick();
    all_angles_merged(&w);
    w.open_on_branch(vec![release_pr(&w, "1.3.0")]);
    w.tick();
    assert!(!w.github.writes().iter().any(|l| l.starts_with("merge")));
}

#[test]
fn the_closure_judges_the_shipped_tickets_sends_the_note_and_closes_the_item() {
    let w = World::new();
    w.tick();
    let release = w.release_item();
    {
        let mut s = w.github.state.borrow_mut();
        s.releases.insert(
            (repo(), "acme-v1.2.0".into()),
            Ok(Release {
                tag: "acme-v1.2.0".into(),
                url: "https://github.com/example-org/acme/releases/tag/acme-v1.2.0".into(),
                body: "### Features\n\n* ship".into(),
                draft: false,
                prerelease: false,
                published_at: None,
            }),
        );
        s.branches.insert(repo(), Vec::new());
        s.compares.insert(
            (repo(), "acme-v1.2.0".into(), "main".into()),
            cmp(CompareStatus::Ahead, &[], &[]),
        );
    }
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.tracker.item("ABC-5").state.status, Some(Status::Done));
    for angle in ["security", "product"] {
        assert_eq!(
            w.sub_item(angle).state.status,
            Some(Status::Done),
            "children close"
        );
    }
    let sent = w.signal.sent.borrow().clone();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].starts_with("[release-note] acme 1.2.0 is published."));
    assert!(sent[0].contains("You can now ship."));
    assert_eq!(
        w.tracker.item(&release.key).state.status,
        Some(Status::Done)
    );
    let before = w.tracker.writes().len();
    w.tick();
    assert_eq!(w.tracker.writes().len(), before);
    assert_eq!(w.signal.sent.borrow().len(), 1, "the note is sent once");
}

#[test]
fn a_failed_note_send_keeps_the_release_open_and_is_retried() {
    let w = World::new();
    w.tick();
    let release = w.release_item();
    {
        let mut s = w.github.state.borrow_mut();
        s.releases.insert(
            (repo(), "acme-v1.2.0".into()),
            Ok(Release {
                tag: "acme-v1.2.0".into(),
                url: String::new(),
                body: String::new(),
                draft: false,
                prerelease: false,
                published_at: None,
            }),
        );
        s.branches.insert(repo(), Vec::new());
        s.compares.insert(
            (repo(), "acme-v1.2.0".into(), "main".into()),
            cmp(CompareStatus::Ahead, &[], &[]),
        );
    }
    let failing = World {
        signal: Recorder {
            fail: true,
            ..Recorder::default()
        },
        ..w
    };
    let r = failing.tick();
    assert!(!r.ok());
    assert_ne!(
        failing.tracker.item(&release.key).state.status,
        Some(Status::Done)
    );
}

// ---- the verbs (spec 09) ----

impl World {
    fn tick_ctx(&self) -> Tick<'_> {
        Tick {
            config: &self.config,
            repo: repo(),
            github: &self.github,
            tracker: &self.tracker,
            signal: &self.signal,
            github_login: bot("spl-asdlc"),
            now: Timestamp::parse("2026-10-05T12:00:00Z").unwrap(),
            deadline: None,
            dry_run: false,
        }
    }
}

#[test]
fn release_cut_dispatches_the_next_candidate_or_refuses_a_second_cycle() {
    let w = World::new();
    let t = w.tick_ctx();
    let mut log = ActionLog::in_memory();
    let line = crate::verbs::cut(&t, &mut log, false).unwrap();
    assert!(line.contains("action candidate"), "{line}");
    assert!(
        w.github
            .writes()
            .iter()
            .any(|l| l.contains("release-cut.yml") && l.contains(BRANCH))
    );
    let refused = crate::verbs::cut(&t, &mut log, true).unwrap_err();
    assert!(refused.contains("a cycle is in flight"), "{refused}");
}

#[test]
fn release_status_says_why_there_is_no_go() {
    let w = World::new();
    w.tick();
    let (cycle, d) = crate::verbs::status(&w.tick_ctx()).unwrap();
    assert_eq!(cycle.candidate.tag, CAND);
    assert!(!d.go);
    assert!(!d.reasons.is_empty());
    assert!(w.github.writes().is_empty(), "read-only");
}

#[test]
fn a_blocker_requalified_as_should_fix_stops_blocking() {
    let w = World::new();
    w.tick();
    let pr = w.angle_pr(
        30,
        "security",
        &report_json("security", &finding("security-1", "blocker")),
    );
    w.open_on_branch(vec![pr]);
    w.tick();
    w.tick();
    let release = w.release_item();
    let blocker = w.tracker.relations(&release.key).unwrap().blocked_by[0]
        .other
        .clone();
    let t = w.tick_ctx();
    let mut log = ActionLog::in_memory();
    assert!(
        crate::verbs::requalify(
            &t,
            &mut log,
            &blocker,
            crate::method::vocabulary::Severity::ShouldFix,
            " ",
            "me"
        )
        .is_err(),
        "a reason is required"
    );
    crate::verbs::requalify(
        &t,
        &mut log,
        &blocker,
        crate::method::vocabulary::Severity::ShouldFix,
        "Not reachable in production.",
        "me",
    )
    .unwrap();
    assert!(
        w.tracker
            .relations(&release.key)
            .unwrap()
            .blocked_by
            .is_empty()
    );
    let item = w.tracker.item(&blocker);
    assert!(crate::method::release::findings::parse_blocker_marker(&item.description).is_none());
    assert!(item.labels.contains(&"qa-finding".to_string()));
    assert_eq!(item.state.status, Some(Status::Triage));
    assert!(
        w.tracker
            .comments(&release.key)
            .unwrap()
            .iter()
            .any(|c| c.body.starts_with("Requalified by me"))
    );
}

#[test]
fn abandoning_merges_the_branch_back_and_cancels_the_release() {
    let w = World::new();
    w.tick();
    {
        let mut s = w.github.state.borrow_mut();
        s.compares.insert(
            (repo(), "main".into(), BRANCH.into()),
            cmp(CompareStatus::Ahead, &["fix"], &["src/a.rs"]),
        );
        s.commits.insert(
            (repo(), BRANCH.into()),
            Ok(Commit {
                sha: "tip".into(),
                message: "fix".into(),
                committer_date: None,
                parents: vec![],
            }),
        );
    }
    let release = w.release_item();
    let t = w.tick_ctx();
    let mut log = ActionLog::in_memory();
    crate::verbs::abandon(&t, &mut log, "me").unwrap();
    let writes = w.github.writes();
    assert!(
        writes
            .iter()
            .any(|l| l.starts_with(&format!("create-pull example-org/acme {BRANCH} -> main"))),
        "{writes:?}"
    );
    assert!(
        writes.iter().any(|l| l.ends_with("at tip merge")),
        "{writes:?}"
    );
    assert_eq!(
        w.tracker.item(&release.key).state.status,
        Some(Status::Canceled)
    );
}

/// The login GitHub shows for an App.
fn bot(slug: &str) -> String {
    format!("{slug}[bot]")
}

// ---- regressions from the lot 6-7 review ----

fn publish(w: &World) {
    let mut s = w.github.state.borrow_mut();
    s.releases.insert(
        (repo(), "acme-v1.2.0".into()),
        Ok(Release {
            tag: "acme-v1.2.0".into(),
            url: String::new(),
            body: String::new(),
            draft: false,
            prerelease: false,
            published_at: None,
        }),
    );
    s.branches.insert(repo(), Vec::new());
    s.compares.insert(
        (repo(), "acme-v1.2.0".into(), "main".into()),
        cmp(CompareStatus::Ahead, &[], &[]),
    );
}

#[test]
fn a_reopened_ticket_is_said_once_while_the_closure_retries() {
    let w = World::new();
    w.tracker.seed(
        "ABC-5",
        "Shipped",
        "## Goal\nShip it.\n\n## Acceptance criteria\n- [x] shipped\n- [ ] fast\n\n## Business rules\nNo new rule.\n",
        Status::ToDeploy,
        &[],
    );
    w.tracker
        .link("ABC-5", "https://github.com/example-org/acme/pull/5");
    w.tick();
    publish(&w);
    let failing = World {
        signal: Recorder {
            fail: true,
            ..Recorder::default()
        },
        ..w
    };
    for _ in 0..3 {
        failing.tick();
    }
    assert_eq!(
        failing.tracker.item("ABC-5").state.status,
        Some(Status::InProgress)
    );
    let reopened = failing
        .tracker
        .comments("ABC-5")
        .unwrap()
        .iter()
        .filter(|c| c.by_bridge)
        .count();
    assert_eq!(reopened, 1);
}

#[test]
fn a_blocker_canceled_as_a_false_alarm_is_not_filed_again() {
    let w = World::new();
    w.tick();
    let pr = w.angle_pr(
        30,
        "security",
        &report_json("security", &finding("security-1", "blocker")),
    );
    w.open_on_branch(vec![pr]);
    w.tick();
    w.tick();
    let blockers = |w: &World| {
        w.tracker
            .list_by_label(LABEL)
            .unwrap()
            .into_iter()
            .filter(|i| i.title.starts_with("Finding security-1"))
            .collect::<Vec<_>>()
    };
    let first = blockers(&w);
    assert_eq!(first.len(), 1);
    w.tracker
        .set_status(&first[0].key, Status::Canceled, None)
        .unwrap();
    w.tick();
    assert_eq!(blockers(&w).len(), 1, "not re-filed");
}

#[test]
fn two_nits_of_one_report_share_one_nits_ticket() {
    let w = World::new();
    w.tick();
    let findings = [finding("security-1", "nit"), finding("security-2", "nit")].join(",");
    let pr = w.angle_pr(30, "security", &report_json("security", &findings));
    w.open_on_branch(vec![pr]);
    w.tick();
    w.tick();
    let nits: Vec<_> = w
        .tracker
        .list_by_label(LABEL)
        .unwrap()
        .into_iter()
        .filter(|i| i.title.starts_with("Nits"))
        .collect();
    assert_eq!(nits.len(), 1);
    assert!(
        nits[0].description.contains("Finding security-1")
            && nits[0].description.contains("Finding security-2")
    );
}

#[test]
fn a_dry_run_of_the_release_pass_writes_nothing() {
    let w = World::new();
    let intents = crate::dry::Intents::default();
    let gh = crate::dry::DryGitHub {
        inner: &w.github,
        intents: &intents,
    };
    let tr = crate::dry::DryTracker {
        inner: &w.tracker,
        intents: &intents,
    };
    let sig = crate::dry::DrySignal { intents: &intents };
    let t = Tick {
        config: &w.config,
        repo: repo(),
        github: &gh,
        tracker: &tr,
        signal: &sig,
        github_login: bot("spl-asdlc"),
        now: Timestamp::parse("2026-10-05T12:00:00Z").unwrap(),
        deadline: None,
        dry_run: true,
    };
    let mut log = ActionLog::in_memory();
    run(&t, &mut log);
    assert!(w.tracker.writes().is_empty() && w.github.writes().is_empty());
    assert!(
        intents
            .lines()
            .iter()
            .any(|l| l.contains("create \"Release acme v1.2.0\"")),
        "{:?}",
        intents.lines()
    );
}

// ---- multi-component GO (spec 05 §9, REQ-REL-077/078) ----

fn multi_config() -> Config {
    let spec = include_str!("../../docs/spec/08-configuration.md");
    let start = spec.find("```toml\n").unwrap() + "```toml\n".len();
    let end = start + spec[start..].find("```").unwrap();
    let text = spec[start..end]
        .replace(
            "# components = [\"app\", \"website\", \"admin\"]",
            "components = [\"app\", \"website\"]",
        )
        .replace(
            "# version_component = \"app\"",
            "version_component = \"app\"",
        )
        .replace("# gate_record = ", "gate_record = ");
    Config::parse(&text).unwrap()
}

fn component_pr(w: &World, number: u64, component: &str) -> PullRequest {
    let mut pr = green(pull_request(number));
    pr.base_ref = BRANCH.into();
    pr.head_ref = format!("release-please--branches--{BRANCH}--components--{component}");
    pr.title = format!("chore(release): release {component} 1.2.0");
    pr.labels = vec!["autorelease: pending".into()];
    pr.author = Some(Actor {
        login: bot("release-app"),
        is_bot: true,
    });
    let changelog = "## [1.2.0](x) (2026-10-05)\n\n### Features\n\n* m1 ([m1](x))\n".to_string();
    let mut s = w.github.state.borrow_mut();
    s.files.insert(
        (repo(), pr.head_sha.clone(), "release-please-config.json".into()),
        Ok(r#"{"packages": {"apps/app": {"component": "app"}, "apps/website": {"component": "website"}}}"#.into()),
    );
    s.files.insert(
        (
            repo(),
            pr.head_sha.clone(),
            format!("apps/{component}/CHANGELOG.md"),
        ),
        Ok(changelog),
    );
    pr
}

#[test]
fn a_multi_component_go_lands_the_record_then_merges_the_version_component_last() {
    let mut w = World::new();
    w.config = multi_config();
    w.tick();
    all_angles_merged(&w);
    w.github.state.borrow_mut().commits.insert(
        (repo(), BRANCH.into()),
        Ok(Commit {
            sha: "c1".into(),
            message: String::new(),
            committer_date: None,
            parents: vec![],
        }),
    );
    let app = component_pr(&w, 41, "app");
    let site = component_pr(&w, 42, "website");
    w.open_on_branch(vec![app.clone(), site.clone()]);
    let r = w.tick();
    let writes = w.github.writes();
    assert!(
        writes
            .iter()
            .any(|l| l.starts_with("put-file") && l.contains("docs/releases/v1.2.0.md")),
        "{writes:?} {r:?}"
    );
    assert!(
        writes.iter().any(|l| l.starts_with("create-pull")),
        "{writes:?}"
    );
    assert!(
        !writes.iter().any(|l| l.starts_with("merge")),
        "no release PR before the record"
    );

    // The record lands on the release branch: the components follow, one per tick.
    let record = w
        .github
        .state
        .borrow()
        .files
        .get(&(
            repo(),
            format!("release-record/{CAND}"),
            "docs/releases/v1.2.0.md".into(),
        ))
        .cloned()
        .unwrap()
        .unwrap();
    w.github.state.borrow_mut().files.insert(
        (repo(), BRANCH.into(), "docs/releases/v1.2.0.md".into()),
        Ok(record),
    );
    w.tick();
    let merges: Vec<String> = w
        .github
        .writes()
        .into_iter()
        .filter(|l| l.starts_with("merge"))
        .collect();
    assert_eq!(
        merges,
        vec![format!(
            "merge example-org/acme#42 at {} squash",
            site.head_sha
        )],
        "website first, app last"
    );
}

#[test]
fn the_release_please_config_is_read_where_the_product_says_and_never_guessed() {
    let mut w = World::new();
    let spec = include_str!("../../docs/spec/08-configuration.md");
    let start = spec.find("```toml\n").unwrap() + "```toml\n".len();
    let end = start + spec[start..].find("```").unwrap();
    let text = spec[start..end]
        .replace(
            "# components = [\"app\", \"website\", \"admin\"]",
            "components = [\"app\", \"website\"]",
        )
        .replace(
            "# version_component = \"app\"",
            "version_component = \"app\"",
        )
        .replace("# gate_record = ", "gate_record = ")
        .replace(
            "release_please_config = \"release-please-config.json\"",
            "release_please_config = \".release-please-config.json\"",
        );
    w.config = Config::parse(&text).unwrap();
    w.tick();
    all_angles_merged(&w);
    w.github.state.borrow_mut().commits.insert(
        (repo(), BRANCH.into()),
        Ok(Commit {
            sha: "c1".into(),
            message: String::new(),
            committer_date: None,
            parents: vec![],
        }),
    );
    // The configuration only at the default path: the product's own is
    // missing, so each component's note cannot be placed and the GO is false.
    let app = component_pr(&w, 41, "app");
    let site = component_pr(&w, 42, "website");
    w.open_on_branch(vec![app.clone(), site.clone()]);
    w.tick();
    let record = |w: &World| {
        w.github
            .writes()
            .iter()
            .any(|l| l.starts_with("put-file") && l.contains("docs/releases/v1.2.0.md"))
    };
    assert!(!record(&w), "{:?}", w.github.writes());

    // At the path the product names: the GO holds and the record is written.
    {
        let mut s = w.github.state.borrow_mut();
        for pr in [&app, &site] {
            let c = s
                .files
                .get(&(
                    repo(),
                    pr.head_sha.clone(),
                    "release-please-config.json".into(),
                ))
                .cloned()
                .unwrap();
            s.files.insert(
                (
                    repo(),
                    pr.head_sha.clone(),
                    ".release-please-config.json".into(),
                ),
                c,
            );
        }
    }
    w.tick();
    assert!(record(&w), "{:?}", w.github.writes());
}
