//! The release pass and the closure against the in-memory GitHub and
//! tracker (spec 05): the cycle opened at the first tick after the cut, its
//! QA round and brief only once the candidate's gate is green, the
//! angle PRs merged with their findings filed once, the GO and the merge of
//! the release PR, the closure and the release note.

use super::*;
use crate::config::Config;
use crate::github::fake::{FakeGitHub, pull_request};
use crate::github::{
    Actor, Artifact, CheckEntry, CheckKind, Commit, CommitPull, CommitSummary, CompareStatus,
    Comparison, GitRef, Identity, ObjectType, PrState, PullRequest, Release, RepoSlug,
    RequiredChecks, Review, Timestamp, WorkflowRun,
};
use crate::method::release::gate;
use crate::method::texts::{self, BridgeItem};
use crate::method::vocabulary::Status;

const POST_DEPLOY_LABEL: &str = "post-deploy-validation/acme";
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
    crate::config::spec_example::config()
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

fn evidence(name: &str) -> Artifact {
    Artifact {
        id: 7,
        name: name.into(),
        expired: false,
    }
}

/// The gate run `id` of `candidate` at `sha`, concluded as given.
fn gate_run(id: u64, candidate: &str, sha: &str, conclusion: Option<&str>) -> WorkflowRun {
    WorkflowRun {
        id,
        head_sha: sha.into(),
        head_branch: candidate.into(),
        event: "push".into(),
        status: if conclusion.is_some() {
            "completed".into()
        } else {
            "in_progress".into()
        },
        conclusion: conclusion.map(str::to_string),
        attempt: 1,
        url: format!("https://github.com/example-org/acme/actions/runs/{id}"),
        created_at: None,
        updated_at: None,
    }
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
                Ok(vec![gate_run(1, CAND, "c1", Some("success"))]),
            );
            s.artifacts.insert(
                (repo(), 1),
                Ok(vec![evidence(&gate::evidence_artifact("c1"))]),
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

const CAND2: &str = "acme-v1.2.0-rc.2";

impl World {
    /// The gate runs of the product, newest first.
    fn gate(&self, runs: Vec<WorkflowRun>) {
        self.github
            .state
            .borrow_mut()
            .runs
            .insert((repo(), "release-gate.yml".into()), Ok(runs));
    }

    /// The round of `candidate`: its angle sub-items and its consolidation item.
    fn round(&self, candidate: &str) -> Vec<crate::tracker::Item> {
        let release = self.release_item();
        self.tracker
            .children(&release.key)
            .unwrap()
            .into_iter()
            .filter(
                |c| match texts::bridge_item(&c.description, c.parent.as_deref()) {
                    Some(BridgeItem::Angle { scope, .. })
                    | Some(BridgeItem::Consolidation { scope }) => scope == candidate,
                    _ => false,
                },
            )
            .collect()
    }

    /// The briefs posted on the release item for `candidate`.
    fn briefs(&self, candidate: &str) -> Vec<String> {
        let release = self.release_item();
        self.tracker
            .comments(&release.key)
            .unwrap()
            .into_iter()
            .map(|c| c.body)
            .filter(|b| b.starts_with(&format!("### Release brief — `{candidate}`")))
            .collect()
    }

    /// The tracker writes of one more tick.
    fn writes_of_a_tick(&self) -> (Report, Vec<String>) {
        let before = self.tracker.writes().len();
        let r = self.tick();
        (r, self.tracker.writes()[before..].to_vec())
    }

    /// The fix of the first pass merged and `rc.2` tagged on it.
    fn cut_rc2(&self) {
        let mut s = self.github.state.borrow_mut();
        s.tags.insert(
            repo(),
            vec![tag("acme-v1.1.0", "r0"), tag(CAND, "c1"), tag(CAND2, "c2")],
        );
        s.branches.insert(repo(), vec![tag(BRANCH, "c2")]);
        s.commits.insert(
            (repo(), "c2".into()),
            Ok(Commit {
                sha: "c2".into(),
                message: "chore: candidate".into(),
                committer_date: Timestamp::parse("2026-10-05T11:50:00Z"),
                parents: vec![],
            }),
        );
        s.compares.insert(
            (repo(), "c2".into(), BRANCH.into()),
            cmp(CompareStatus::Identical, &[], &[]),
        );
        s.compares.insert(
            (repo(), "acme-v1.1.0".into(), "c2".into()),
            cmp(CompareStatus::Ahead, &["m1"], &["src/a.rs", "src/b.rs"]),
        );
        s.compares.insert(
            (repo(), CAND.into(), "c2".into()),
            cmp(CompareStatus::Ahead, &["f1"], &["src/b.rs"]),
        );
        s.artifacts.insert(
            (repo(), 2),
            Ok(vec![evidence(&gate::evidence_artifact("c2"))]),
        );
    }

    /// The runs of the gate's automatic-rerun workflow, newest first.
    fn reruns(&self, runs: Vec<WorkflowRun>) {
        self.github
            .state
            .borrow_mut()
            .runs
            .insert((repo(), "release-gate-auto-rerun.yml".into()), Ok(runs));
    }

    /// The `results/suites.json` of the evidence packs (artifact 7).
    fn suites(&self, json: &str) {
        self.github
            .state
            .borrow_mut()
            .artifact_files
            .insert((repo(), 7, gate::SUITES_FILE.into()), Ok(json.to_string()));
    }

    /// The gate-fix tickets under the release item.
    fn gate_fixes(&self) -> Vec<crate::tracker::Item> {
        let release = self.release_item();
        self.tracker
            .children(&release.key)
            .unwrap()
            .into_iter()
            .filter(|c| texts::gate_fix_candidate(&c.description).is_some())
            .collect()
    }

    /// A red first attempt of `candidate`'s gate whose rerun settled
    /// without a rerun, `failed` named by its evidence pack.
    fn red_settled(&self, id: u64, candidate: &str, sha: &str, failed: &str) {
        self.gate(vec![gate_run(id, candidate, sha, Some("failure"))]);
        self.reruns(vec![rerun_run(90 + id, sha, "completed")]);
        self.suites(&format!(r#"{{"failed":[{failed}],"suites":[]}}"#));
    }
}

/// A run of the automatic-rerun workflow, triggered by a gate run on `sha`.
fn rerun_run(id: u64, sha: &str, status: &str) -> WorkflowRun {
    WorkflowRun {
        event: "workflow_run".into(),
        status: status.into(),
        conclusion: (status == "completed").then(|| "success".to_string()),
        ..gate_run(id, CAND, sha, None)
    }
}

#[test]
fn a_red_gate_opens_the_release_item_but_no_angle_nor_brief() {
    let w = World::new();
    w.red_settled(1, CAND, "c1", r#""tests""#);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let item = w.release_item();
    assert!(item.description.contains(&format!("`{CAND}`")));
    assert!(
        w.tracker
            .comments(&item.key)
            .unwrap()
            .iter()
            .any(|c| c.body.starts_with(&format!("### Candidate `{CAND}`")))
    );
    assert!(w.tracker.item("ABC-5").labels.contains(&LABEL.to_string()));
    assert!(w.round(CAND).is_empty(), "no angle starts on a red gate");
    assert!(w.briefs(CAND).is_empty());
    assert_eq!(w.gate_fixes().len(), 1, "but the gate's own fix does");

    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert!(writes.is_empty(), "{writes:?}");
}

#[test]
fn the_gate_turning_green_opens_the_round_once_with_the_evidence() {
    let w = World::new();
    w.gate(vec![gate_run(1, CAND, "c1", None)]);
    w.tick();
    assert!(w.round(CAND).is_empty(), "a pending gate opens nothing");
    let (_, writes) = w.writes_of_a_tick();
    assert!(writes.is_empty(), "{writes:?}");

    w.gate(vec![gate_run(1, CAND, "c1", Some("success"))]);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let round = w.round(CAND);
    assert_eq!(round.len(), 3, "two angles and the consolidation item");
    assert!(round.iter().all(|i| i.state.status == Some(Status::Ready)));
    let briefs = w.briefs(CAND);
    assert_eq!(briefs.len(), 1);
    assert!(
        briefs[0].contains("- Gate run: https://github.com/example-org/acme/actions/runs/1\n"),
        "{}",
        briefs[0]
    );
    assert!(
        briefs[0].contains(
            "- Evidence pack: https://github.com/example-org/acme/actions/runs/1#artifacts\n"
        ),
        "{}",
        briefs[0]
    );

    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert!(writes.is_empty(), "{writes:?}");
}

#[test]
fn a_later_candidate_with_a_red_gate_waits_again() {
    let w = World::new();
    w.tick();
    assert_eq!(w.round(CAND).len(), 3);
    w.cut_rc2();
    w.gate(vec![
        gate_run(2, CAND2, "c2", Some("failure")),
        gate_run(1, CAND, "c1", Some("success")),
    ]);
    w.reruns(vec![rerun_run(92, "c2", "completed")]);
    w.suites(r#"{"failed":["tests"]}"#);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let item = w.release_item();
    assert!(item.description.contains(&format!("`{CAND2}`")));
    assert!(
        w.tracker
            .comments(&item.key)
            .unwrap()
            .iter()
            .any(|c| c.body.starts_with(&format!("### Candidate `{CAND2}`")))
    );
    assert!(w.round(CAND2).is_empty(), "rc.2 waits for its own gate");
    assert!(w.briefs(CAND2).is_empty());
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert!(writes.is_empty(), "{writes:?}");

    w.gate(vec![
        gate_run(2, CAND2, "c2", Some("success")),
        gate_run(1, CAND, "c1", Some("success")),
    ]);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.round(CAND2).len(), 3);
    let briefs = w.briefs(CAND2);
    assert_eq!(briefs.len(), 1);
    assert!(
        briefs[0].contains("actions/runs/2#artifacts"),
        "{}",
        briefs[0]
    );
}

// ---- BR-ASDLC-015: an item carries its protocol, frozen at creation ----

#[test]
fn a_release_rounds_items_carry_their_protocol_and_keep_it_while_open() {
    let w = World::new();
    w.tick();
    let round = w.round(CAND);
    assert_eq!(round.len(), 3);
    for i in &round {
        assert!(
            i.description.contains("\n## Protocol\n")
                && i.description.contains(&format!(
                    "Embedded by spl-asdlc {}",
                    texts::protocol::VERSION
                ))
                && i.description
                    .contains("\"schema\": \"spl-asdlc/qa-report/1\""),
            "{}",
            i.description
        );
    }
    // A later bridge with another protocol: the open items are not rewritten.
    let mut older = Vec::new();
    for i in &round {
        let marker = i.description.lines().next().unwrap();
        let text = format!(
            "{marker}\n\n## Goal\nAn older text.\n\n## Acceptance criteria\n- [ ] it\n\n## Business rules\nNo new rule.\n\n## Protocol\n\nAn older protocol.\n"
        );
        w.tracker.update_description(&i.key, &text).unwrap();
        older.push((i.key.clone(), text));
    }
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert!(writes.is_empty(), "{writes:?}");
    for (key, text) in &older {
        assert_eq!(&w.tracker.item(key).description, text);
    }
    // The next candidate's items carry the protocol of the bridge that
    // creates them.
    w.cut_rc2();
    w.gate(vec![
        gate_run(2, CAND2, "c2", Some("success")),
        gate_run(1, CAND, "c1", Some("success")),
    ]);
    assert!(w.tick().ok());
    let round2 = w.round(CAND2);
    assert_eq!(round2.len(), 3);
    assert!(round2.iter().all(|i| {
        i.description
            .contains("\n## Protocol\n\nEmbedded by spl-asdlc")
    }));
    for (key, text) in &older {
        assert_eq!(&w.tracker.item(key).description, text);
    }
}

#[test]
fn an_unreadable_gate_writes_nothing_more_and_says_why() {
    let w = World::new();
    w.github.state.borrow_mut().runs.insert(
        (repo(), "release-gate.yml".into()),
        Err(crate::github::ReadError::unreadable(
            "runs of release-gate.yml",
            "HTTP 502",
        )),
    );
    let r = w.tick();
    assert!(
        r.failures
            .iter()
            .any(|f| f.contains(&format!("the gate of {CAND}"))),
        "{r:?}"
    );
    assert!(w.round(CAND).is_empty());
    assert!(w.briefs(CAND).is_empty());
    let (r, writes) = w.writes_of_a_tick();
    assert!(!r.ok());
    assert!(writes.is_empty(), "{writes:?}");
}

#[test]
fn a_green_gate_whose_evidence_cannot_be_read_opens_nothing() {
    let w = World::new();
    w.github.state.borrow_mut().artifacts.insert(
        (repo(), 1),
        Err(crate::github::ReadError::unreadable(
            "artifacts of run 1",
            "HTTP 502",
        )),
    );
    let r = w.tick();
    assert!(
        r.failures.iter().any(|f| f.contains("the artifacts of")),
        "{r:?}"
    );
    assert!(
        w.round(CAND).is_empty(),
        "no agent starts without its brief"
    );
    assert!(w.briefs(CAND).is_empty());

    // A pack the run does not carry is no more a brief than an unread one.
    w.github
        .state
        .borrow_mut()
        .artifacts
        .insert((repo(), 1), Ok(vec![evidence("something-else")]));
    let (r, writes) = w.writes_of_a_tick();
    assert!(
        r.failures
            .iter()
            .any(|f| f.contains("carries no evidence pack `release-gate-evidence-c1`")),
        "{r:?}"
    );
    assert!(writes.is_empty(), "{writes:?}");

    w.github.state.borrow_mut().artifacts.insert(
        (repo(), 1),
        Ok(vec![evidence(&gate::evidence_artifact("c1"))]),
    );
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.round(CAND).len(), 3);
    assert_eq!(w.briefs(CAND).len(), 1);
}

#[test]
fn a_release_coordinator_app_approves_and_a_person_of_its_name_does_not() {
    // (configured, the review's author as read, merged)
    for (configured, author, merged) in [
        (bot("coord-app"), bot("Coord-App"), true),
        ("app/coord-app".to_string(), bot("coord-app"), true),
        (bot("coord-app"), "coord-app".to_string(), false),
        ("coord-app".to_string(), bot("coord-app"), false),
    ] {
        let mut w = World::new();
        w.config.release.as_mut().unwrap().coordinators = vec![configured.clone()];
        w.tick();
        let mut pr = w.angle_pr(30, "security", &report_json("security", ""));
        pr.reviews[0].author = Some(author.clone());
        w.open_on_branch(vec![pr.clone()]);
        w.tick();
        assert_eq!(
            w.github.writes().contains(&format!(
                "merge example-org/acme#30 at {} merge",
                pr.head_sha
            )),
            merged,
            "configured {configured}, reviewed by {author}: {:?}",
            w.github.writes()
        );
    }
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
    assert!(
        nits.description
            .contains("Finding security-3 — where: `src/a.rs:1` — context: e")
    );
    assert!(crate::method::contract::has_contract(&nits.description));
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

/// The last allowed candidate answered by every angle with a blocker left:
/// the tick that files the blocker escalates. The owner is known, the hold
/// label is not in the tracker yet.
fn escalating() -> World {
    let mut w = World::new();
    w.config.release.as_mut().unwrap().max_rc_passes = 1;
    w.tracker
        .state
        .borrow_mut()
        .users
        .push(crate::tracker::User {
            id: "u-owner".into(),
            name: "owner-handle".into(),
        });
    w.tick();
    let mut merged = Vec::new();
    for (n, angle, findings) in [
        (30, "security", finding("security-1", "blocker")),
        (31, "product", String::new()),
    ] {
        let mut pr = w.angle_pr(n, angle, &report_json(angle, &findings));
        pr.state = PrState::Merged;
        pr.merge_commit_sha = Some(format!("merge-of-{n}"));
        merged.push(pr);
    }
    w.github
        .state
        .borrow_mut()
        .merged_pulls
        .insert(repo(), Ok(merged));
    w
}

fn escalation_comments(w: &World) -> usize {
    let release = w.release_item();
    w.tracker
        .comments(&release.key)
        .unwrap()
        .iter()
        .filter(|c| {
            texts::has_marker(
                &c.body,
                &texts::marker("qa-escalation", &[("candidate", CAND)]),
            )
        })
        .count()
}

#[test]
fn an_escalation_creates_its_missing_label_before_adding_it() {
    let w = escalating();
    assert!(!w.tracker.label_exists("blocked-by-human").unwrap());
    w.tick();
    w.tick();
    let release = w.release_item();
    let writes = w.tracker.writes();
    let ensure = writes
        .iter()
        .position(|l| l == "ensure-label blocked-by-human")
        .unwrap_or_else(|| panic!("label created: {writes:?}"));
    let add = writes
        .iter()
        .position(|l| *l == format!("label {} +blocked-by-human", release.key))
        .unwrap_or_else(|| panic!("label added: {writes:?}"));
    assert!(ensure < add, "{writes:?}");
    assert!(release.labels.contains(&"blocked-by-human".to_string()));
    assert_eq!(
        release.assignee.map(|a| a.name),
        Some("owner-handle".to_string())
    );
    assert_eq!(escalation_comments(&w), 1);
    let before = w.tracker.writes().len();
    w.tick();
    assert_eq!(
        w.tracker.writes().len(),
        before,
        "escalated once: {:?}",
        &w.tracker.writes()[before..]
    );
}

#[test]
fn a_failed_label_write_still_explains_once_and_the_label_is_retried() {
    let w = escalating();
    w.tracker
        .fail_writes_starting_with("ensure-label blocked-by-human");
    w.tick();
    w.tick();
    let release = w.release_item();
    assert!(!release.labels.contains(&"blocked-by-human".to_string()));
    assert_eq!(
        release.assignee.map(|a| a.name),
        Some("owner-handle".to_string())
    );
    assert_eq!(escalation_comments(&w), 1, "{:?}", w.tracker.writes());
    assert_eq!(w.signal.sent.borrow().len(), 1);
    // The next ticks retry what is missing, the label, and only that.
    w.tick();
    assert_eq!(escalation_comments(&w), 1);
    w.tracker.state.borrow_mut().failing_writes.clear();
    let before = w.tracker.writes().len();
    w.tick();
    assert_eq!(
        w.tracker.writes()[before..].to_vec(),
        vec![
            "ensure-label blocked-by-human".to_string(),
            format!("label {} +blocked-by-human", release.key),
        ]
    );
    assert_eq!(escalation_comments(&w), 1);
    assert_eq!(w.signal.sent.borrow().len(), 1);
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
fn a_merged_pr_ships_while_its_ticket_is_still_in_progress() {
    // BR-ASDLC-005: every merged PR is deployable; its ticket, back In
    // Progress for the boxes a later PR delivers, is embarked and left alone.
    let w = World::new();
    w.tracker.seed(
        "ABC-5",
        "Shipped in two PRs",
        "## Goal\nShip it.\n\n## Acceptance criteria\n- [x] shipped\n- [ ] fast\n\n## Business rules\nNo new rule.\n\n## Release note\nYou can now ship.\n",
        Status::InProgress,
        &[],
    );
    w.tracker
        .link("ABC-5", "https://github.com/example-org/acme/pull/5");
    w.tick();
    assert!(
        w.tracker.item("ABC-5").labels.contains(&LABEL.to_string()),
        "embarked"
    );
    publish(&w);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        w.tracker.item("ABC-5").state.status,
        Some(Status::InProgress),
        "not judged: its later PR is on the way"
    );
    assert!(
        !w.tracker
            .writes()
            .iter()
            .any(|l| l.starts_with("comment ABC-5")),
        "{:?}",
        w.tracker.writes()
    );
    assert_eq!(
        w.tracker.item(&w.release_item().key).state.status,
        Some(Status::Done),
        "the cycle closes"
    );
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

/// The nits of one merged report, under `release.nits`.
fn nits_world(grouping: crate::config::NitsGrouping) -> World {
    let mut w = World::new();
    w.config.release.as_mut().unwrap().nits = grouping;
    w.tick();
    let findings = [finding("security-1", "nit"), finding("security-2", "nit")].join(",");
    let pr = w.angle_pr(30, "security", &report_json("security", &findings));
    w.open_on_branch(vec![pr]);
    w
}

#[test]
fn the_shared_nits_ticket_is_a_full_contract_that_asks_to_triage() {
    let w = nits_world(crate::config::NitsGrouping::Shared);
    w.tick();
    w.tick();
    let nits = w
        .tracker
        .list_by_label(LABEL)
        .unwrap()
        .into_iter()
        .find(|i| i.title == "Nits — acme v1.2.0")
        .expect("nits");
    let d = &nits.description;
    // What `ticket check` judges, and nothing exempts it by marker.
    assert!(
        crate::method::contract::missing_from_contract(d).is_empty(),
        "{d}"
    );
    assert_eq!(crate::method::business_rules::new_rule_statement(d), None);
    assert!(texts::bridge_item(d, None).is_none(), "{d}");
    assert!(d.contains("## Business rules\nNo new rule."), "{d}");
    assert!(
        d.contains("classify each box below as **kept** or **dropped**"),
        "{d}"
    );
    let boxes = crate::method::markdown::checkboxes_in_sections(
        d,
        crate::method::contract::is_acceptance_heading,
    );
    assert_eq!(boxes.len(), 2, "{d}");
    assert!(
        boxes[0]
            .line
            .contains(&format!("`{CAND}/security/security-1`"))
    );
    assert!(
        boxes[1]
            .line
            .contains(&format!("`{CAND}/security/security-2`"))
    );
    assert_eq!(nits.state.status, Some(Status::Triage));
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert!(writes.is_empty(), "{writes:?}");
}

#[test]
fn a_nits_ticket_of_the_older_format_is_left_alone_and_said() {
    let w = nits_world(crate::config::NitsGrouping::Shared);
    let older = "- [ ] src/b.rs:2: an older nit\n";
    w.tracker.seed(
        "ABC-77",
        "Nits — acme v1.2.0",
        older,
        Status::Triage,
        &[LABEL],
    );
    w.tick();
    let (r, writes) = w.writes_of_a_tick();
    assert!(writes.iter().all(|x| !x.contains("ABC-77")), "{writes:?}");
    assert_eq!(w.tracker.item("ABC-77").description, older);
    assert!(
        r.failures
            .iter()
            .any(|f| f.starts_with("ABC-77: the nits ticket carries no contract")),
        "{r:?}"
    );
}

#[test]
fn per_nit_grouping_files_one_contract_ticket_per_nit_in_triage_once() {
    let w = nits_world(crate::config::NitsGrouping::PerNit);
    w.tick();
    w.tick();
    let labelled = w.tracker.list_by_label(LABEL).unwrap();
    assert!(labelled.iter().all(|i| !i.title.starts_with("Nits")));
    let nits: Vec<_> = labelled
        .iter()
        .filter(|i| i.title.ends_with("(nit — security)"))
        .collect();
    assert_eq!(nits.len(), 2, "{labelled:?}");
    for n in &nits {
        assert!(
            crate::method::contract::has_contract(&n.description),
            "{}",
            n.description
        );
        assert!(n.description.contains("## Goal\nTriage the nit"));
        assert_eq!(n.state.status, Some(Status::Triage));
        assert!(!n.labels.contains(&"qa-finding".to_string()));
        assert!(crate::method::release::findings::parse_finding_marker(&n.description).is_some());
    }
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert!(writes.is_empty(), "{writes:?}");
}

#[test]
fn a_dry_run_writes_no_nit_in_either_grouping() {
    for grouping in [
        crate::config::NitsGrouping::Shared,
        crate::config::NitsGrouping::PerNit,
    ] {
        let w = nits_world(grouping);
        // The angle PR already merged: the dry tick reads its report.
        {
            let mut s = w.github.state.borrow_mut();
            let mut pr = match s.open_pulls.insert(repo(), Ok(Vec::new())) {
                Some(Ok(mut list)) => list.remove(0),
                _ => unreachable!(),
            };
            pr.state = PrState::Merged;
            pr.merge_commit_sha = Some("merge-of-30".into());
            s.merged_pulls.insert(repo(), Ok(vec![pr]));
        }
        let before = w.tracker.writes().len();
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
        let r = run(&t, &mut log);
        assert!(r.failures.is_empty(), "{grouping:?}: {r:?}");
        assert_eq!(w.tracker.writes().len(), before, "{grouping:?}");
        assert!(
            intents
                .lines()
                .iter()
                .any(|l| l.contains("Nits — acme") || l.contains("Finding security-1 (nit")),
            "{grouping:?}: {:?}",
            intents.lines()
        );
    }
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
    let r = run(&t, &mut log);
    assert!(w.tracker.writes().is_empty() && w.github.writes().is_empty());
    // The item it would create is read as empty, never as a failure.
    assert!(r.failures.is_empty(), "{r:?}");
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
    let text = crate::config::spec_example::repository()
        .replace(
            "# components = [\"app\", \"website\", \"admin\"]",
            "components = [\"app\", \"website\"]",
        )
        .replace(
            "# version_component = \"app\"",
            "version_component = \"app\"",
        )
        .replace("# gate_record = ", "gate_record = ");
    crate::config::spec_example::parse(&text)
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
    let text = crate::config::spec_example::repository()
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
    w.config = crate::config::spec_example::parse(&text);
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

#[test]
fn a_version_released_without_a_release_item_of_the_bridge_is_not_its_cycle() {
    // Released before the bridge took the product over: no item to open,
    // nothing to close, no note to send.
    let w = World::new();
    publish(&w);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert!(w.tracker.writes().is_empty(), "{:?}", w.tracker.writes());
    assert!(w.github.writes().is_empty(), "{:?}", w.github.writes());
}

// ---- post-deploy validation (BR-ASDLC-002, BR-ASDLC-003) ----

const PR5: &str = "https://github.com/example-org/acme/pull/5";
const POST: &str = "## Goal\nShip it.\n\n## Non-goals\n- More.\n\n## Acceptance criteria\n- [x] shipped\n\n## Post deploy\n- [ ] the dashboard shows it\n- [ ] the export opens\n\n## Business rules\nNo new rule.\n";

fn validation_tasks(w: &World, parent: &str) -> Vec<crate::tracker::Item> {
    let marker = texts::post_deploy_validation_marker(parent);
    w.tracker
        .children(parent)
        .unwrap()
        .into_iter()
        .filter(|c| texts::has_marker(&c.description, &marker))
        .collect()
}

/// A cycle closed over one shipped ticket whose only open boxes are its
/// `## Post deploy` ones.
fn closed_with_post_deploy() -> World {
    let w = World::new();
    w.tracker
        .seed("ABC-5", "Shipped", POST, Status::ToDeploy, &[]);
    w.tracker.link("ABC-5", PR5);
    w.tick();
    publish(&w);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    w
}

fn check_box(w: &World, key: &str, text: &str) {
    let d = w
        .tracker
        .item(key)
        .description
        .replace(&format!("- [ ] {text}"), &format!("- [x] {text}"));
    w.tracker.update_description(key, &d).unwrap();
}

fn labelled(w: &World, key: &str) -> bool {
    w.tracker
        .item(key)
        .labels
        .iter()
        .any(|l| l == POST_DEPLOY_LABEL)
}

#[test]
fn the_closure_moves_a_ticket_to_post_deploy_with_one_unassigned_validation_sub_task() {
    let w = closed_with_post_deploy();
    let ticket = w.tracker.item("ABC-5");
    assert_eq!(ticket.state.status, Some(Status::PostDeploy));
    assert!(labelled(&w, "ABC-5"), "the post-deploy pass finds it again");
    assert_eq!(ticket.assignee, None, "the ticket itself is never served");
    let tasks = validation_tasks(&w, "ABC-5");
    assert_eq!(tasks.len(), 1, "{:?}", w.tracker.writes());
    let task = &tasks[0];
    assert_eq!(task.state.status, Some(Status::Ready), "a followed status");
    assert_eq!(task.assignee, None);
    assert!(crate::method::contract::has_contract(&task.description));
    assert!(task.description.contains("the owner (`owner-handle`)"));
    let release = w.release_item();
    let closure = w
        .tracker
        .comments(&release.key)
        .unwrap()
        .into_iter()
        .find(|c| c.body.starts_with("Deployed."))
        .expect("the closure comment");
    assert!(closure.body.contains("Moved to `Post Deploy`: `ABC-5`"));
    assert!(!closure.body.contains("post-release-check"), "no verb");
    // Idempotent: the label, the status and the sub-task are there.
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
fn a_partially_checked_post_deploy_ticket_stays_in_post_deploy() {
    let w = closed_with_post_deploy();
    check_box(&w, "ABC-5", "the dashboard shows it");
    let before = w.tracker.writes().len();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        w.tracker.item("ABC-5").state.status,
        Some(Status::PostDeploy)
    );
    assert_eq!(
        w.tracker.writes().len(),
        before,
        "{:?}",
        &w.tracker.writes()[before..]
    );
}

#[test]
fn a_post_deploy_ticket_moves_to_done_at_the_tick_that_finds_every_box_checked() {
    let w = closed_with_post_deploy();
    check_box(&w, "ABC-5", "the dashboard shows it");
    check_box(&w, "ABC-5", "the export opens");
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.tracker.item("ABC-5").state.status, Some(Status::Done));
    assert!(!labelled(&w, "ABC-5"), "the index keeps only what waits");
    // The sub-task is the agent's or the owner's to close.
    assert_eq!(
        validation_tasks(&w, "ABC-5")[0].state.status,
        Some(Status::Ready)
    );
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
fn a_lost_creation_answer_is_found_and_the_sub_task_never_created_twice() {
    let w = World::new();
    w.tick();
    w.tracker.seed(
        "ABC-9",
        "Waiting",
        POST,
        Status::PostDeploy,
        &[POST_DEPLOY_LABEL],
    );
    // The creation went through and its answer was lost: the sub-task is
    // there, its marker visible as the tracker keeps it.
    let (title, body) = texts::post_deploy_validation_item(
        "ABC-9",
        "Waiting",
        "https://tracker.test/ABC-9",
        "owner-handle",
    );
    w.tracker.seed(
        "ABC-10",
        &title,
        &texts::visible_markers(&body),
        Status::Ready,
        &[],
    );
    w.tracker.state.borrow_mut().set_parent("ABC-10", "ABC-9");
    let before = w.tracker.writes().len();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        w.tracker.writes().len(),
        before,
        "{:?}",
        &w.tracker.writes()[before..]
    );
    assert_eq!(validation_tasks(&w, "ABC-9").len(), 1);
}

#[test]
fn a_refused_validation_sub_task_is_created_at_a_later_tick_once() {
    let w = World::new();
    w.tracker
        .seed("ABC-5", "Shipped", POST, Status::ToDeploy, &[]);
    w.tracker.link("ABC-5", PR5);
    w.tick();
    publish(&w);
    w.tracker
        .fail_writes_starting_with("create Post-deploy validation");
    let r = w.tick();
    assert!(!r.ok(), "the refusal is reported");
    assert_eq!(
        w.tracker.item("ABC-5").state.status,
        Some(Status::PostDeploy)
    );
    assert!(validation_tasks(&w, "ABC-5").is_empty());
    w.tracker.state.borrow_mut().failing_writes.clear();
    // The release item is closed by now: the post-deploy pass repairs it.
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(validation_tasks(&w, "ABC-5").len(), 1);
    let before = w.tracker.writes().len();
    w.tick();
    assert_eq!(w.tracker.writes().len(), before);
}

#[test]
fn an_unreadable_read_moves_nothing_and_is_reported() {
    let w = World::new();
    w.tick();
    let checked = POST.replace("- [ ]", "- [x]");
    w.tracker.seed(
        "ABC-9",
        "Waiting",
        &checked,
        Status::PostDeploy,
        &[POST_DEPLOY_LABEL],
    );
    w.tracker.make_unreadable("ABC-9");
    let before = w.tracker.writes().len();
    let r = w.tick();
    assert!(r.failures.iter().any(|f| f.starts_with("ABC-9")), "{r:?}");
    assert_eq!(
        w.tracker.item("ABC-9").state.status,
        Some(Status::PostDeploy)
    );
    // The list itself unreadable: said, nothing written.
    w.tracker.state.borrow_mut().unreadable.clear();
    w.tracker.make_unreadable(POST_DEPLOY_LABEL);
    let r = w.tick();
    assert!(
        r.failures.iter().any(|f| f.contains(POST_DEPLOY_LABEL)),
        "{r:?}"
    );
    assert_eq!(
        w.tracker.writes().len(),
        before,
        "{:?}",
        &w.tracker.writes()[before..]
    );
}

#[test]
fn a_ticket_resolved_by_hand_loses_the_index_label_once() {
    let w = World::new();
    w.tick();
    w.tracker.seed(
        "ABC-9",
        "Dropped",
        POST,
        Status::Canceled,
        &[POST_DEPLOY_LABEL],
    );
    let before = w.tracker.writes().len();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        w.tracker.writes()[before..],
        [format!("label ABC-9 -{POST_DEPLOY_LABEL}")]
    );
    assert_eq!(w.tracker.item("ABC-9").state.status, Some(Status::Canceled));
    let before = w.tracker.writes().len();
    w.tick();
    assert_eq!(w.tracker.writes().len(), before);
}

#[test]
fn a_dry_run_of_the_post_deploy_pass_writes_nothing() {
    let w = World::new();
    w.tick();
    let checked = POST.replace("- [ ]", "- [x]");
    w.tracker.seed(
        "ABC-9",
        "Waiting",
        &checked,
        Status::PostDeploy,
        &[POST_DEPLOY_LABEL],
    );
    let before = w.tracker.writes().len();
    let intents = crate::dry::Intents::default();
    let tr = crate::dry::DryTracker {
        inner: &w.tracker,
        intents: &intents,
    };
    let t = Tick {
        tracker: &tr,
        dry_run: true,
        ..w.tick_ctx()
    };
    let mut log = ActionLog::in_memory();
    let mut report = Report::default();
    post_deploy::pass(&t, &mut log, &mut report);
    assert!(report.ok(), "{report:?}");
    assert_eq!(w.tracker.writes().len(), before);
    assert_eq!(
        intents.lines(),
        vec![
            "tracker: ABC-9 → Done".to_string(),
            format!("tracker: label ABC-9 -{POST_DEPLOY_LABEL}"),
        ]
    );
}

#[test]
fn a_ticket_whose_latest_pr_shipped_later_is_left_to_that_release() {
    // ABC-5 is embarked through PR #5, but its latest merged PR (#99) came
    // after this release's range: the release that ships #99 judges it.
    let run = |latest: &str| {
        let w = World::new();
        w.tick();
        crate::tracker::Tracker::comment(
            &w.tracker,
            "ABC-5",
            &format!("Merged.\n\nmerged_pr: https://github.com/example-org/acme/pull/{latest}"),
        )
        .unwrap();
        publish(&w);
        let r = w.tick();
        assert!(r.ok(), "{r:?}");
        w.tracker.item("ABC-5").state.status
    };
    assert_eq!(run("99"), Some(Status::ToDeploy));
    assert_eq!(run("5"), Some(Status::Done));
}

#[test]
fn the_gate_record_the_bridge_lands_is_no_tip_gap() {
    // Once the record is on the release branch, the branch is ahead of the
    // candidate by that one file: the GO holds and the components merge.
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
    w.open_on_branch(vec![app, site.clone()]);
    w.tick();
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
    {
        let mut s = w.github.state.borrow_mut();
        s.files.insert(
            (repo(), BRANCH.into(), "docs/releases/v1.2.0.md".into()),
            Ok(record),
        );
        s.compares.insert(
            (repo(), "c1".into(), BRANCH.into()),
            cmp(CompareStatus::Ahead, &["r1"], &["docs/releases/v1.2.0.md"]),
        );
    }
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
            "merge example-org/acme#42 at {} squash",
            site.head_sha
        )],
        "{r:?}"
    );
}

#[test]
fn the_deployment_ledger_reads_the_component_prs_merged_into_the_release_branch() {
    // Release-please's component PRs target the release branch: a ledger
    // read off the default branch would find nothing and never close.
    let mut w = World::new();
    w.config = multi_config();
    w.tick();
    let item = w.release_item();
    publish(&w);
    let merged = |n: u64, component: &str, deployed: bool| {
        let mut pr = component_pr(&w, n, component);
        pr.base_ref = BRANCH.into();
        pr.state = crate::github::PrState::Merged;
        pr.merged_at = Timestamp::parse("2026-10-05T13:00:00Z");
        if deployed {
            pr.labels.push("deployed: production".into());
        }
        pr
    };
    let app = merged(41, "app", true);
    let site = merged(42, "website", false);
    w.github
        .state
        .borrow_mut()
        .merged_pulls
        .insert(repo(), Ok(vec![app.clone(), site]));
    let r = w.tick();
    assert!(
        !r.failures.iter().any(|f| f.contains("ledger")),
        "the ledger is read: {r:?}"
    );
    assert_ne!(
        w.tracker.item(&item.key).state.status,
        Some(Status::Done),
        "a component not deployed holds the closure"
    );
    // Both deployed: the closure proceeds.
    let site = merged(42, "website", true);
    w.github
        .state
        .borrow_mut()
        .merged_pulls
        .insert(repo(), Ok(vec![app, site]));
    let r = w.tick();
    assert!(!r.failures.iter().any(|f| f.contains("ledger")), "{r:?}");
}

// ---- a ticket the closure reopens is unassigned (BR-ASDLC-009) ----

/// ABC-5 shipped with `description`, assigned to whoever took it last.
fn assigned_ticket(w: &World, description: &str) {
    w.tracker
        .seed("ABC-5", "Shipped", description, Status::ToDeploy, &[]);
    w.tracker
        .link("ABC-5", "https://github.com/example-org/acme/pull/5");
    let last = crate::tracker::User {
        id: "u-last".into(),
        name: "last-taker".into(),
    };
    w.tracker.assign("ABC-5", Some(&last)).unwrap();
}

const INCOMPLETE: &str = "## Goal\nShip it.\n\n## Acceptance criteria\n- [x] shipped\n- [ ] fast\n\n## Business rules\nNo new rule.\n";

#[test]
fn the_closure_reopens_an_incomplete_ticket_unassigned_once() {
    let w = World::new();
    assigned_ticket(&w, INCOMPLETE);
    w.tick();
    assert!(w.tracker.item("ABC-5").assignee.is_some());
    publish(&w);
    let before = w.tracker.writes().len();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let ticket = w.tracker.item("ABC-5");
    assert_eq!(ticket.state.status, Some(Status::InProgress));
    assert_eq!(ticket.assignee, None, "nobody is named");
    assert!(
        w.tracker.writes()[before..].contains(&"status ABC-5 In Progress (unassigned)".to_string()),
        "state and assignee in one write: {:?}",
        w.tracker.writes()
    );
    let said = w.tracker.comments("ABC-5").unwrap();
    assert!(
        said.iter().any(|c| c
            .body
            .contains(crate::method::done_gate::RETURNED_UNASSIGNED)),
        "{said:?}"
    );
    let before = w.tracker.writes().len();
    w.tick();
    assert_eq!(w.tracker.writes().len(), before, "a replay writes nothing");
}

#[test]
fn the_closure_reopens_a_ticket_citing_an_unknown_rule_unassigned() {
    let w = World::new();
    assigned_ticket(
        &w,
        "## Goal\nShip it.\n\n## Acceptance criteria\n- [x] shipped\n\n## Business rules\nBR-ACME-001 applies.\n\n## Release note\nYou can now ship.\n",
    );
    w.github.state.borrow_mut().files.insert(
        (
            repo(),
            "acme-v1.2.0".into(),
            "docs/business-rules.md".into(),
        ),
        Ok("| ID | Rule |\n|---|---|\n| BR-ACME-002 | Another rule. |\n".into()),
    );
    w.tick();
    publish(&w);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let ticket = w.tracker.item("ABC-5");
    assert_eq!(ticket.state.status, Some(Status::InProgress));
    assert_eq!(ticket.assignee, None);
    let before = w.tracker.writes().len();
    w.tick();
    assert_eq!(w.tracker.writes().len(), before, "a replay writes nothing");
}

#[test]
fn a_dry_run_of_the_closure_prints_the_unassign() {
    let w = World::new();
    assigned_ticket(&w, INCOMPLETE);
    w.tick();
    publish(&w);
    let before = w.tracker.writes().len();
    let intents = crate::dry::Intents::default();
    let tr = crate::dry::DryTracker {
        inner: &w.tracker,
        intents: &intents,
    };
    let sig = crate::dry::DrySignal { intents: &intents };
    let t = Tick {
        tracker: &tr,
        signal: &sig,
        dry_run: true,
        ..w.tick_ctx()
    };
    let mut log = ActionLog::in_memory();
    run(&t, &mut log);
    assert_eq!(
        w.tracker.writes().len(),
        before,
        "the real tracker saw no write"
    );
    assert!(w.tracker.item("ABC-5").assignee.is_some());
    assert!(
        intents
            .lines()
            .contains(&"tracker: ABC-5 → In Progress, unassigned".to_string()),
        "{:?}",
        intents.lines()
    );
}

// ---- the gate-fix ticket (spec 05 §3, BR-ASDLC-016) ----

impl World {
    fn tick_at(&self, now: &str) -> Report {
        let t = Tick {
            now: Timestamp::parse(now).unwrap(),
            ..self.tick_ctx()
        };
        let mut log = ActionLog::in_memory();
        run(&t, &mut log)
    }
}

const RUN1: &str = "https://github.com/example-org/acme/actions/runs/1";

#[test]
fn a_red_gate_opens_one_urgent_gate_fix_ticket_once_its_rerun_settled() {
    let w = World::new();
    w.red_settled(1, CAND, "c1", r#""tests","lint""#);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let release = w.release_item();
    let fixes = w.gate_fixes();
    assert_eq!(fixes.len(), 1, "{:?}", w.tracker.writes());
    let fix = &fixes[0];
    assert_eq!(fix.title, format!("Make the gate green — {CAND}"));
    assert_eq!(fix.state.status, Some(Status::Ready));
    assert_eq!(fix.assignee, None, "taken by role, nobody named");
    assert_eq!(fix.parent.as_deref(), Some(release.key.as_str()));
    assert!(fix.labels.contains(&LABEL.to_string()));
    assert_eq!(
        w.tracker.priority(&fix.key),
        Some(crate::tracker::Priority::Urgent)
    );
    assert_eq!(
        crate::method::contract::missing_from_contract(&fix.description),
        Vec::<String>::new(),
        "ticket check accepts it"
    );
    assert!(
        fix.description.contains(&format!(
            "- [ ] Each failing suite passes on `{BRANCH}`: `tests`, `lint`."
        )),
        "{}",
        fix.description
    );
    assert!(fix.description.contains(&format!("- Gate run: {RUN1}")));
    assert!(fix.description.contains("No new rule."));
    assert!(fix.description.contains("- No new feature."));
    assert_eq!(
        texts::gate_fix_candidate(&fix.description).as_deref(),
        Some(CAND)
    );

    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert!(
        writes.is_empty(),
        "a second tick writes nothing: {writes:?}"
    );
}

#[test]
fn a_green_or_pending_gate_opens_no_gate_fix_ticket() {
    let w = World::new();
    w.gate(vec![gate_run(1, CAND, "c1", None)]);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert!(w.gate_fixes().is_empty(), "pending");
    let w = World::new();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert!(w.gate_fixes().is_empty(), "green");
}

#[test]
fn a_red_gate_whose_rerun_may_still_come_opens_nothing_until_it_settled() {
    let w = World::new();
    w.gate(vec![gate_run(1, CAND, "c1", Some("failure"))]);
    w.suites(r#"{"failed":["tests"]}"#);
    // The rerun workflow has not run for this gate run yet.
    w.reruns(Vec::new());
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert!(w.gate_fixes().is_empty(), "the rerun may still come");
    // It runs: the rerun it may make is not decided yet.
    w.reruns(vec![rerun_run(91, "c1", "in_progress")]);
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert!(writes.is_empty(), "{writes:?}");
    // It reran the gate: the second attempt is pending.
    w.reruns(vec![rerun_run(91, "c1", "completed")]);
    let mut second = gate_run(1, CAND, "c1", None);
    second.attempt = 2;
    w.gate(vec![second.clone()]);
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert!(writes.is_empty(), "{writes:?}");
    // The second attempt is red too: final, whatever the rerun workflow says.
    second.status = "completed".into();
    second.conclusion = Some("failure".into());
    w.gate(vec![second]);
    w.reruns(vec![
        rerun_run(93, "c1", "in_progress"),
        rerun_run(91, "c1", "completed"),
    ]);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.gate_fixes().len(), 1);
    let (_, writes) = w.writes_of_a_tick();
    assert!(writes.is_empty(), "{writes:?}");
}

/// A red first attempt of the gate of `CAND` that completed at 11:45.
fn red_completed_at_11_45() -> WorkflowRun {
    WorkflowRun {
        created_at: Timestamp::parse("2026-10-05T11:31:00Z"),
        updated_at: Timestamp::parse("2026-10-05T11:45:00Z"),
        ..gate_run(1, CAND, "c1", Some("failure"))
    }
}

impl World {
    fn writes_of_a_tick_at(&self, now: &str) -> (Report, Vec<String>) {
        let before = self.tracker.writes().len();
        let r = self.tick_at(now);
        (r, self.tracker.writes()[before..].to_vec())
    }
}

#[test]
fn a_red_gate_with_no_rerun_seen_waits_up_to_its_bound_then_opens_its_ticket() {
    let w = World::new();
    w.gate(vec![red_completed_at_11_45()]);
    w.suites(r#"{"failed":["tests"]}"#);
    w.reruns(Vec::new());
    // `gate_rerun_wait_minutes = 30`: until 12:15 the rerun may still come.
    let r = w.tick_at("2026-10-05T12:14:59Z");
    assert!(r.ok(), "{r:?}");
    assert!(
        w.gate_fixes().is_empty(),
        "no rerun visible before the bound"
    );
    // From the bound on, the ticket all the same: the rerun workflow may be
    // absent or broken, and the ticket says so.
    let r = w.tick_at("2026-10-05T12:15:00Z");
    assert!(r.ok(), "{r:?}");
    let fixes = w.gate_fixes();
    assert_eq!(fixes.len(), 1, "{:?}", w.tracker.writes());
    let d = &fixes[0].description;
    assert!(
        d.contains("- Automatic rerun: no run of the automatic rerun workflow showed up within 30 minutes of the completion of"),
        "{d}"
    );
    assert!(crate::method::contract::has_contract(d));
    assert_eq!(fixes[0].state.status, Some(Status::Ready));
    assert_eq!(
        w.tracker.priority(&fixes[0].key),
        Some(crate::tracker::Priority::Urgent)
    );
    let (r, writes) = w.writes_of_a_tick_at("2026-10-05T12:30:00Z");
    assert!(r.ok(), "{r:?}");
    assert!(
        writes.is_empty(),
        "a second tick writes nothing: {writes:?}"
    );
}

#[test]
fn a_rerun_still_running_past_the_bound_keeps_the_wait() {
    let w = World::new();
    w.gate(vec![red_completed_at_11_45()]);
    w.suites(r#"{"failed":["tests"]}"#);
    w.reruns(vec![rerun_run(91, "c1", "in_progress")]);
    let (r, writes) = w.writes_of_a_tick_at("2026-10-05T13:00:00Z");
    assert!(r.ok(), "{r:?}");
    assert!(
        w.gate_fixes().is_empty(),
        "the rerun it shows may still rerun the gate"
    );
    assert!(
        !writes
            .iter()
            .any(|l| l.starts_with("create Make the gate green")),
        "{writes:?}"
    );
    // It completed without rerunning: settled, and nothing says it was unseen.
    w.reruns(vec![rerun_run(91, "c1", "completed")]);
    let r = w.tick_at("2026-10-05T13:05:00Z");
    assert!(r.ok(), "{r:?}");
    let fixes = w.gate_fixes();
    assert_eq!(fixes.len(), 1);
    assert!(!fixes[0].description.contains("Automatic rerun:"));
}

#[test]
fn an_absent_rerun_workflow_is_said_and_the_bound_still_opens_the_ticket() {
    let w = World::new();
    w.gate(vec![red_completed_at_11_45()]);
    w.suites(r#"{"failed":["tests"]}"#);
    w.github.state.borrow_mut().runs.insert(
        (repo(), "release-gate-auto-rerun.yml".into()),
        Err(crate::github::ReadError::NotFound {
            what: "runs of release-gate-auto-rerun.yml".into(),
        }),
    );
    let r = w.tick_at("2026-10-05T12:00:00Z");
    assert!(
        r.failures.iter().any(|f| f.contains("not found")
            && f.contains("the gate-fix ticket opens 30 minutes after the gate run completed")),
        "{r:?}"
    );
    assert!(w.gate_fixes().is_empty());
    w.tick_at("2026-10-05T12:15:00Z");
    assert_eq!(w.gate_fixes().len(), 1, "never a silence");
}

#[test]
fn an_unreadable_rerun_workflow_opens_nothing_and_says_why() {
    let w = World::new();
    w.gate(vec![gate_run(1, CAND, "c1", Some("failure"))]);
    w.suites(r#"{"failed":["tests"]}"#);
    let r = w.tick();
    assert!(
        r.failures
            .iter()
            .any(|f| f.contains(&format!("the automatic rerun of the gate of {CAND}"))),
        "{r:?}"
    );
    assert!(w.gate_fixes().is_empty());
}

#[test]
fn a_red_gate_whose_evidence_cannot_be_read_still_opens_its_ticket() {
    let w = World::new();
    w.gate(vec![gate_run(1, CAND, "c1", Some("failure"))]);
    w.reruns(vec![rerun_run(91, "c1", "completed")]);
    w.github.state.borrow_mut().artifact_files.insert(
        (repo(), 7, gate::SUITES_FILE.into()),
        Err(crate::github::ReadError::unreadable(
            "results/suites.json in artifact 7",
            "HTTP 502",
        )),
    );
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let fixes = w.gate_fixes();
    assert_eq!(fixes.len(), 1, "never a silence");
    let d = &fixes[0].description;
    assert!(
        d.contains("unknown: the evidence pack could not be read"),
        "{d}"
    );
    assert!(d.contains("HTTP 502"), "{d}");
    assert!(
        d.contains(&format!(
            "- [ ] The suites that failed in the gate run {RUN1} (unknown here"
        )),
        "{d}"
    );
    assert!(crate::method::contract::has_contract(d));
    assert_eq!(
        w.tracker.priority(&fixes[0].key),
        Some(crate::tracker::Priority::Urgent)
    );

    // A run that carries no pack is no better read.
    let w = World::new();
    w.gate(vec![gate_run(1, CAND, "c1", Some("failure"))]);
    w.reruns(vec![rerun_run(91, "c1", "completed")]);
    w.github
        .state
        .borrow_mut()
        .artifacts
        .insert((repo(), 1), Ok(Vec::new()));
    w.tick();
    let fixes = w.gate_fixes();
    assert_eq!(fixes.len(), 1);
    assert!(
        fixes[0]
            .description
            .contains("the run carries no evidence pack `release-gate-evidence-c1`"),
        "{}",
        fixes[0].description
    );
}

#[test]
fn a_newer_green_candidate_closes_the_gate_fix_ticket_once() {
    let w = World::new();
    w.red_settled(1, CAND, "c1", r#""tests""#);
    w.tick();
    let fix = w.gate_fixes().remove(0);
    // The implementer took it, no PR of it merged yet: the green gate of
    // rc.2 (a rerun made it green, say) makes it moot.
    w.tracker
        .set_status(&fix.key, Status::InProgress, None)
        .unwrap();
    w.cut_rc2();
    w.gate(vec![
        gate_run(2, CAND2, "c2", Some("success")),
        gate_run(1, CAND, "c1", Some("failure")),
    ]);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.tracker.item(&fix.key).state.status, Some(Status::Done));
    assert_eq!(
        w.tracker.item(&fix.key).description,
        fix.description,
        "the bridge ticks no box of it"
    );
    let said: Vec<String> = w
        .tracker
        .comments(&fix.key)
        .unwrap()
        .into_iter()
        .map(|c| c.body)
        .filter(|b| b.contains(&format!("`{CAND2}` is green")))
        .collect();
    assert_eq!(said.len(), 1, "a comment names the green candidate");
    assert_eq!(w.round(CAND2).len(), 3, "and the round of rc.2 opens");

    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert!(writes.is_empty(), "{writes:?}");
}

#[test]
fn a_gate_fix_ticket_resolved_in_review_or_to_deploy_is_left_alone_on_a_green_gate() {
    for status in [
        Status::InReview,
        Status::ToDeploy,
        Status::PostDeploy,
        Status::Done,
        Status::Canceled,
    ] {
        let w = World::new();
        w.red_settled(1, CAND, "c1", r#""tests""#);
        w.tick();
        let fix = w.gate_fixes().remove(0);
        w.tracker.set_status(&fix.key, status, None).unwrap();
        w.cut_rc2();
        w.gate(vec![
            gate_run(2, CAND2, "c2", Some("success")),
            gate_run(1, CAND, "c1", Some("failure")),
        ]);
        let r = w.tick();
        assert!(r.ok(), "{r:?}");
        // Past the merge it follows the flow of any ticket: To Deploy at
        // its fix's merge, Done at the closure, never closed by the bridge.
        assert_eq!(w.tracker.item(&fix.key).state.status, Some(status));
        assert!(w.tracker.comments(&fix.key).unwrap().is_empty(), "{status}");
        assert_eq!(
            w.tracker.item(&fix.key).description,
            fix.description,
            "{status}: no box ticked by the bridge"
        );
    }
}

#[test]
fn a_still_red_next_gate_opens_a_new_gate_fix_ticket_for_its_candidate() {
    let w = World::new();
    w.red_settled(1, CAND, "c1", r#""tests""#);
    w.tick();
    let first = w.gate_fixes().remove(0);
    // Its fix merged: To Deploy, as any ticket.
    w.tracker
        .set_status(&first.key, Status::ToDeploy, None)
        .unwrap();
    // The gate of rc.2 is the verification, and it is still red.
    w.cut_rc2();
    w.gate(vec![
        gate_run(2, CAND2, "c2", Some("failure")),
        gate_run(1, CAND, "c1", Some("failure")),
    ]);
    w.reruns(vec![
        rerun_run(91, "c1", "completed"),
        rerun_run(92, "c2", "completed"),
    ]);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let fixes = w.gate_fixes();
    assert_eq!(fixes.len(), 2, "{:?}", w.tracker.writes());
    let second = fixes
        .iter()
        .find(|f| f.key != first.key)
        .expect("a new ticket");
    assert_eq!(
        texts::gate_fix_candidate(&second.description).as_deref(),
        Some(CAND2)
    );
    assert_eq!(second.state.status, Some(Status::Ready));
    assert_eq!(
        w.tracker.priority(&second.key),
        Some(crate::tracker::Priority::Urgent)
    );
    let old = w.tracker.item(&first.key);
    assert_eq!(old.state.status, Some(Status::ToDeploy), "left to its flow");
    assert_eq!(old.description, first.description);
    assert!(w.tracker.comments(&first.key).unwrap().is_empty());

    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert!(writes.is_empty(), "never opened twice: {writes:?}");
}

#[test]
fn a_red_gate_older_than_stagnation_hours_raises_the_stagnation_notice() {
    let w = World::new();
    w.red_settled(1, CAND, "c1", r#""tests""#);
    // Tagged at 11:30; `stagnation_hours = 2`.
    let r = w.tick_at("2026-10-05T14:00:00Z");
    assert!(r.ok(), "{r:?}");
    let sent = w.signal.sent.borrow().clone();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(
        sent[0].starts_with("[stagnation] Release stuck"),
        "{sent:?}"
    );
    assert!(sent[0].contains("Candidate gate red"), "{sent:?}");
    assert_eq!(w.gate_fixes().len(), 1, "and the fix is served");
    w.tick_at("2026-10-05T15:00:00Z");
    assert_eq!(w.signal.sent.borrow().len(), 1, "said once");
}

#[test]
fn a_dry_run_prints_the_urgent_gate_fix_ticket_it_would_open() {
    let w = World::new();
    w.red_settled(1, CAND, "c1", r#""tests""#);
    // The release item exists; its gate-fix ticket does not yet.
    w.tracker
        .fail_writes_starting_with("create Make the gate green");
    w.tick();
    assert!(w.gate_fixes().is_empty());
    let release = w.release_item();
    let before = w.tracker.writes().len();
    let intents = crate::dry::Intents::default();
    let tr = crate::dry::DryTracker {
        inner: &w.tracker,
        intents: &intents,
    };
    let sig = crate::dry::DrySignal { intents: &intents };
    let t = Tick {
        tracker: &tr,
        signal: &sig,
        dry_run: true,
        ..w.tick_ctx()
    };
    let mut log = ActionLog::in_memory();
    let r = run(&t, &mut log);
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.tracker.writes().len(), before, "the tracker saw no write");
    let want = format!(
        "tracker: create \"Make the gate green — {CAND}\" [Ready] priority urgent under {}",
        release.key
    );
    assert!(intents.lines().contains(&want), "{:?}", intents.lines());
}

#[test]
fn the_validation_sub_task_is_created_urgent_and_a_dry_run_says_so() {
    let w = closed_with_post_deploy();
    let tasks = validation_tasks(&w, "ABC-5");
    assert_eq!(tasks.len(), 1);
    assert_eq!(
        w.tracker.priority(&tasks[0].key),
        Some(crate::tracker::Priority::Urgent),
        "production is waiting on it"
    );
    let before = w.tracker.writes().len();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        w.tracker.writes().len(),
        before,
        "a second tick writes nothing"
    );

    let w = World::new();
    w.tick();
    w.tracker.seed(
        "ABC-9",
        "Waiting",
        POST,
        Status::PostDeploy,
        &[POST_DEPLOY_LABEL],
    );
    let intents = crate::dry::Intents::default();
    let tr = crate::dry::DryTracker {
        inner: &w.tracker,
        intents: &intents,
    };
    let t = Tick {
        tracker: &tr,
        dry_run: true,
        ..w.tick_ctx()
    };
    let mut log = ActionLog::in_memory();
    let mut report = Report::default();
    post_deploy::pass(&t, &mut log, &mut report);
    assert!(report.ok(), "{report:?}");
    assert_eq!(
        intents.lines(),
        vec![
            "tracker: create \"Post-deploy validation — ABC-9: Waiting\" [Ready] priority urgent under ABC-9"
                .to_string()
        ]
    );
}

// ---- archiving at the closure (BR-ASDLC-019) ----

/// A PR review round of `ticket` (an angle sub-item and its consolidation
/// item), seeded under it in `status`.
fn review_round(w: &World, ticket: &str, keys: [&str; 2], status: Status) {
    let (title, body) = texts::review_angle_item(ticket, PR5, "aaaaaaa1", "security", None, &[]);
    w.tracker.seed(keys[0], &title, &body, status, &[]);
    let scope = texts::round_scope(PR5, "aaaaaaa1");
    let (title, body) =
        texts::consolidation_item(&scope, ticket, texts::Consolidating::PullRequest);
    w.tracker.seed(keys[1], &title, &body, status, &[]);
    let mut s = w.tracker.state.borrow_mut();
    s.set_parent(keys[0], ticket);
    s.set_parent(keys[1], ticket);
}

fn nits_title(w: &World) -> String {
    crate::method::release::findings::nits_title(&w.config.product.name, "1.2.0")
}

fn consolidation_of_release(w: &World) -> crate::tracker::Item {
    let release = w.release_item();
    w.tracker
        .children(&release.key)
        .unwrap()
        .into_iter()
        .find(|c| {
            matches!(
                texts::bridge_item(&c.description, c.parent.as_deref()),
                Some(BridgeItem::Consolidation { .. })
            )
        })
        .expect("the consolidation item")
}

fn archives(w: &World) -> Vec<String> {
    w.tracker
        .writes()
        .into_iter()
        .filter(|l| l.starts_with("archive "))
        .collect()
}

/// A cycle opened over every kind of item the closure meets: the shipped
/// ticket `ABC-5` with a resolved review round (`ABC-21`, `ABC-22`); a
/// ticket that goes to Post Deploy (`ABC-31`); one sent back In Progress
/// (`ABC-32`) with a resolved round (`ABC-33`, `ABC-34`); a finding left in
/// Triage (`ABC-30`); a gate-fix child of the release item (`ABC-40`) in
/// `gate_fix`; the nits ticket (`ABC-41`) in `nits`.
fn every_kind(gate_fix: Status, nits: Status) -> World {
    let w = World::new();
    w.tick();
    let release = w.release_item();
    review_round(&w, "ABC-5", ["ABC-21", "ABC-22"], Status::Done);
    w.tracker.seed(
        "ABC-30",
        "Finding product-1",
        "A finding.",
        Status::Triage,
        &[LABEL],
    );
    w.tracker
        .seed("ABC-31", "Post deploy", POST, Status::ToDeploy, &[LABEL]);
    w.tracker.seed(
        "ABC-32",
        "Incomplete",
        INCOMPLETE,
        Status::ToDeploy,
        &[LABEL],
    );
    review_round(&w, "ABC-32", ["ABC-33", "ABC-34"], Status::Canceled);
    w.tracker
        .seed("ABC-40", "Fix the gate", "A gate fix.", gate_fix, &[]);
    w.tracker
        .state
        .borrow_mut()
        .set_parent("ABC-40", &release.key);
    w.tracker
        .seed("ABC-41", &nits_title(&w), "Nits.", nits, &[LABEL]);
    publish(&w);
    w
}

#[test]
fn the_closure_archives_what_the_cycle_resolved_once_and_never_an_open_item() {
    let w = every_kind(Status::Done, Status::Done);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let release = w.release_item();
    assert_eq!(release.state.status, Some(Status::Done));
    let consolidation = consolidation_of_release(&w).key;
    let security = w.sub_item("security").key;
    let product = w.sub_item("product").key;
    // The release item, its resolved children (angle sub-items,
    // consolidation item, gate fix), the nits ticket, the review rounds of
    // the embarked tickets, the ticket the closure moved to Done.
    let resolved = [
        release.key.as_str(),
        &security,
        &product,
        &consolidation,
        "ABC-40",
        "ABC-41",
        "ABC-21",
        "ABC-22",
        "ABC-33",
        "ABC-34",
        "ABC-5",
    ];
    for key in resolved {
        assert!(w.tracker.item(key).archived, "{key} is archived");
    }
    // Never an open item: Post Deploy and its validation sub-task, In
    // Progress, a finding in Triage.
    let task = validation_tasks(&w, "ABC-31");
    assert_eq!(task.len(), 1);
    for key in ["ABC-30", "ABC-31", "ABC-32", task[0].key.as_str()] {
        assert!(!w.tracker.item(key).archived, "{key} is open: kept");
    }
    let done = archives(&w);
    assert_eq!(done.len(), resolved.len(), "each once: {done:?}");
    assert_eq!(
        done.last(),
        Some(&format!("archive {}", release.key)),
        "the release item last"
    );
    // A second tick writes nothing.
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
fn an_open_nits_ticket_stays_and_an_open_child_keeps_the_release_item() {
    let w = every_kind(Status::Ready, Status::Triage);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let release = w.release_item();
    assert_eq!(release.state.status, Some(Status::Done), "the cycle closes");
    assert!(
        !w.tracker.item("ABC-41").archived,
        "an open nits ticket stays"
    );
    assert!(!w.tracker.item("ABC-40").archived, "an open child stays");
    assert!(
        !release.archived,
        "nor is the release item archived over an open child"
    );
    assert!(w.tracker.item("ABC-5").archived, "the rest is archived");
    assert!(w.sub_item("security").archived);
    let before = w.tracker.writes().len();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.tracker.writes().len(), before, "a replay writes nothing");
    // The child resolved, the next tick archives it and the release item.
    w.tracker.set_status("ABC-40", Status::Done, None).unwrap();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert!(w.tracker.item("ABC-40").archived);
    assert!(w.tracker.item(&release.key).archived);
    assert!(!w.tracker.item("ABC-41").archived, "still open, still kept");
}

#[test]
fn a_failed_archive_never_undoes_the_closure_and_is_retried() {
    let w = every_kind(Status::Done, Status::Done);
    w.tracker.fail_writes_starting_with("archive ABC-21");
    let r = w.tick();
    assert!(!r.ok(), "the failure is reported");
    assert!(
        r.failures.iter().any(|f| f.contains("archive")),
        "{:?}",
        r.failures
    );
    let release = w.release_item();
    assert_eq!(
        release.state.status,
        Some(Status::Done),
        "the closure stands"
    );
    assert!(!w.tracker.item("ABC-21").archived);
    assert!(!release.archived, "the release item waits for the retry");
    assert!(w.tracker.item("ABC-5").archived, "the others went through");
    assert_eq!(w.signal.sent.borrow().len(), 1, "the note was sent once");
    w.tracker.state.borrow_mut().failing_writes.clear();
    let before = w.tracker.writes().len();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        w.tracker.writes()[before..],
        [
            "archive ABC-21".to_string(),
            format!("archive {}", release.key)
        ],
        "only what was left"
    );
    let before = w.tracker.writes().len();
    w.tick();
    assert_eq!(w.tracker.writes().len(), before, "then nothing");
    assert_eq!(w.signal.sent.borrow().len(), 1, "never sent again");
}

#[test]
fn a_tracker_without_an_open_item_limit_archives_nothing() {
    let mut w = every_kind(Status::Done, Status::Done);
    w.tracker.archiving = false;
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        w.tracker.item(&w.release_item().key).state.status,
        Some(Status::Done)
    );
    assert!(archives(&w).is_empty(), "{:?}", w.tracker.writes());
    let before = w.tracker.writes().len();
    w.tick();
    assert_eq!(w.tracker.writes().len(), before, "a replay writes nothing");
}

#[test]
fn a_release_item_closed_by_hand_is_not_archived_from() {
    let w = World::new();
    w.tick();
    let release = w.release_item();
    w.tracker
        .set_status(&release.key, Status::Done, None)
        .unwrap();
    publish(&w);
    let before = w.tracker.writes().len();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        w.tracker.writes().len(),
        before,
        "no closure ran: nothing archived"
    );
}

#[test]
fn a_dry_run_of_the_closure_prints_the_archives() {
    let w = every_kind(Status::Done, Status::Done);
    let release = w.release_item();
    let before = w.tracker.writes().len();
    let intents = crate::dry::Intents::default();
    let tr = crate::dry::DryTracker {
        inner: &w.tracker,
        intents: &intents,
    };
    let sig = crate::dry::DrySignal { intents: &intents };
    let t = Tick {
        tracker: &tr,
        signal: &sig,
        dry_run: true,
        ..w.tick_ctx()
    };
    let mut log = ActionLog::in_memory();
    run(&t, &mut log);
    assert_eq!(
        w.tracker.writes().len(),
        before,
        "the real tracker saw no write"
    );
    let lines = intents.lines();
    for key in ["ABC-5", "ABC-21", "ABC-40", release.key.as_str()] {
        assert!(
            lines.contains(&format!("tracker: archive {key}")),
            "{key}: {lines:?}"
        );
    }
    for key in ["ABC-30", "ABC-31", "ABC-32"] {
        assert!(
            !lines.contains(&format!("tracker: archive {key}")),
            "{key}: {lines:?}"
        );
    }
}

// ---- spec 05 §3 and BR-ASDLC-020: the coordinator receives the
// consolidation item; the angle items in the normal flow, the consolidation
// as the overall review ----

impl World {
    /// The consolidation item of `candidate`.
    fn consolidation(&self, candidate: &str) -> crate::tracker::Item {
        self.round(candidate)
            .into_iter()
            .find(|i| {
                matches!(
                    texts::bridge_item(&i.description, i.parent.as_deref()),
                    Some(BridgeItem::Consolidation { .. })
                )
            })
            .expect("a consolidation item")
    }

    /// The angle sub-items of `candidate`, sorted.
    fn angle_keys(&self, candidate: &str) -> Vec<String> {
        let mut keys: Vec<String> = self
            .round(candidate)
            .into_iter()
            .filter(|i| {
                matches!(
                    texts::bridge_item(&i.description, i.parent.as_deref()),
                    Some(BridgeItem::Angle { .. })
                )
            })
            .map(|i| i.key)
            .collect();
        keys.sort();
        keys
    }

    /// What blocks `key`, sorted.
    fn blockers(&self, key: &str) -> Vec<String> {
        let mut b: Vec<String> = self
            .tracker
            .relations(key)
            .unwrap()
            .blocked_by
            .into_iter()
            .map(|r| r.other)
            .collect();
        b.sort();
        b
    }

    /// One more tick writes nothing, on either side.
    fn quiet(&self) {
        let github = self.github.writes().len();
        let (r, writes) = self.writes_of_a_tick();
        assert!(r.ok(), "{r:?}");
        assert!(writes.is_empty(), "{writes:?}");
        assert_eq!(
            self.github.writes().len(),
            github,
            "{:?}",
            self.github.writes()
        );
    }
}

fn coordinator(state: &str, head: &str) -> Review {
    Review {
        author: Some("coordinator-login".into()),
        body: "consolidated".into(),
        state: state.into(),
        commit_sha: Some(head.into()),
        submitted_at: None,
    }
}

#[test]
fn a_release_angle_item_follows_its_pr_to_review_back_to_work_and_done_at_the_merge() {
    let w = World::new();
    w.tick();
    let sub = w.sub_item("security").key;
    let mut pr = w.angle_pr(30, "security", &report_json("security", ""));
    pr.reviews.clear();

    // The PR appears as a draft: the angle's work started.
    pr.draft = true;
    w.open_on_branch(vec![pr.clone()]);
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(writes, vec![format!("status {sub} In Progress")]);
    w.quiet();

    // Ready, its required checks green: In Review, for the coordinator.
    pr.draft = false;
    w.open_on_branch(vec![pr.clone()]);
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(writes, vec![format!("status {sub} In Review")]);
    w.quiet();

    // The coordinator requests changes at the head: back to its angle.
    pr.reviews = vec![coordinator("CHANGES_REQUESTED", &pr.head_sha)];
    w.open_on_branch(vec![pr.clone()]);
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(writes, vec![format!("status {sub} In Progress")]);
    w.quiet();

    // The angle pushes its fix; the coordinator approves the new head: In
    // Review, merged, Done (never To Deploy).
    let path = format!(".spl-asdlc/qa/{CAND}/security.json");
    pr.head_sha = "fixedhead0000001".into();
    w.github.state.borrow_mut().files.insert(
        (repo(), pr.head_sha.clone(), path),
        Ok(report_json("security", "")),
    );
    pr.reviews.push(coordinator("APPROVED", &pr.head_sha));
    w.open_on_branch(vec![pr.clone()]);
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        writes,
        vec![
            format!("status {sub} In Review"),
            format!("status {sub} Done")
        ]
    );
    assert!(w.github.writes().contains(&format!(
        "merge example-org/acme#30 at {} merge",
        pr.head_sha
    )));
    assert_eq!(w.sub_item("security").state.status, Some(Status::Done));
    w.quiet();
}

#[test]
fn an_angle_pr_that_does_not_name_its_sub_item_drives_nothing_and_is_told_once_per_head() {
    let w = World::new();
    w.tick();
    let sub = w.sub_item("security").key;
    let mut pr = w.angle_pr(30, "security", &report_json("security", ""));
    pr.body = "Angle security".into();
    w.github
        .state
        .borrow_mut()
        .comments
        .insert((repo(), 30), Ok(Vec::new()));
    w.open_on_branch(vec![pr.clone()]);
    let github = w.github.writes().len();
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert!(writes.is_empty(), "no status: {writes:?}");
    let said: Vec<String> = w.github.writes()[github..].to_vec();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with("comment example-org/acme#30 "),
        "{said:?}"
    );
    assert!(said[0].contains(&format!("name `{sub}`")), "{said:?}");
    assert_eq!(w.sub_item("security").state.status, Some(Status::Ready));
    w.quiet();

    // A new head that still names nothing is told again, once.
    pr.head_sha = "otherhead0000001".into();
    w.open_on_branch(vec![pr]);
    let github = w.github.writes().len();
    w.tick();
    let said = w.github.writes()[github..].to_vec();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(!said[0].starts_with("merge"), "{said:?}");
    w.quiet();
}

#[test]
fn the_release_consolidation_item_is_blocked_by_every_angle_of_its_candidate_once() {
    let w = World::new();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let consolidation = w.consolidation(CAND);
    let angles = w.angle_keys(CAND);
    assert_eq!(angles.len(), 2);
    assert_eq!(w.blockers(&consolidation.key), angles);
    assert!(
        consolidation
            .description
            .contains("once every angle PR of the candidate is merged"),
        "the overall review: {}",
        consolidation.description
    );
    w.quiet();

    // A relation lost (or a round opened before them): repaired once.
    for rel in w.tracker.relations(&consolidation.key).unwrap().blocked_by {
        if rel.other == angles[0] {
            w.tracker.unrelate(&rel.id).unwrap();
        }
    }
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        writes,
        vec![format!("relate {} Blocks {}", angles[0], consolidation.key)]
    );
    assert_eq!(w.blockers(&consolidation.key), angles);
    w.quiet();
}

#[test]
fn a_release_relation_that_cannot_be_written_holds_the_brief_until_it_is_laid() {
    let w = World::new();
    w.tracker.fail_writes_starting_with("relate");
    let r = w.tick();
    assert!(!r.ok(), "the failure is reported");
    assert_eq!(w.round(CAND).len(), 3, "the items are created");
    let consolidation = w.consolidation(CAND);
    assert!(w.blockers(&consolidation.key).is_empty());
    assert!(
        w.briefs(CAND).is_empty(),
        "fail-closed: no brief while the consolidation is not blocked by its angles"
    );
    w.tracker.state.borrow_mut().failing_writes.clear();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.blockers(&consolidation.key), w.angle_keys(CAND));
    assert_eq!(
        w.briefs(CAND).len(),
        1,
        "the brief, once the relations are laid"
    );
    w.quiet();
    assert_eq!(w.briefs(CAND).len(), 1, "the brief, once");
}

#[test]
fn a_newer_candidate_tagged_closes_the_older_consolidation_once() {
    let w = World::new();
    w.tick();
    let older = w.consolidation(CAND).key;
    w.cut_rc2();
    // Tagged, its gate not green yet: the older overall review is over all
    // the same.
    w.gate(vec![
        gate_run(2, CAND2, "c2", None),
        gate_run(1, CAND, "c1", Some("success")),
    ]);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.tracker.item(&older).state.status, Some(Status::Done));
    for angle in w.angle_keys(CAND) {
        assert_eq!(
            w.tracker.item(&angle).state.status,
            Some(Status::Ready),
            "only the consolidation is ended by the newer candidate"
        );
    }
    w.quiet();

    // rc.2's own round, once its gate is green: its consolidation open,
    // blocked by its own angles.
    w.gate(vec![
        gate_run(2, CAND2, "c2", Some("success")),
        gate_run(1, CAND, "c1", Some("success")),
    ]);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let newer = w.consolidation(CAND2);
    assert_eq!(newer.state.status, Some(Status::Ready));
    assert_eq!(w.blockers(&newer.key), w.angle_keys(CAND2));
    w.quiet();
}

#[test]
fn the_release_published_closes_the_consolidation_before_the_closure() {
    let w = World::new();
    w.tick();
    let consolidation = w.consolidation(CAND).key;
    publish(&w);
    // The closure waits (the branch still holds the candidate): the
    // consolidation is Done all the same.
    w.github
        .state
        .borrow_mut()
        .branches
        .insert(repo(), vec![tag(BRANCH, "c1")]);
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(writes, vec![format!("status {consolidation} Done")]);
    assert_ne!(w.release_item().state.status, Some(Status::Done));
    w.quiet();
}

#[test]
fn a_dry_run_prints_the_angle_items_flow_and_the_relations() {
    let w = World::new();
    w.tick();
    let sub = w.sub_item("security").key;
    let consolidation = w.consolidation(CAND).key;
    for rel in w.tracker.relations(&consolidation).unwrap().blocked_by {
        w.tracker.unrelate(&rel.id).unwrap();
    }
    let mut pr = w.angle_pr(30, "security", &report_json("security", ""));
    pr.reviews.clear();
    pr.draft = true;
    w.open_on_branch(vec![pr]);
    let before = w.tracker.writes().len();
    let intents = crate::dry::Intents::default();
    let tr = crate::dry::DryTracker {
        inner: &w.tracker,
        intents: &intents,
    };
    let sig = crate::dry::DrySignal { intents: &intents };
    let t = Tick {
        tracker: &tr,
        signal: &sig,
        dry_run: true,
        ..w.tick_ctx()
    };
    let mut log = ActionLog::in_memory();
    run(&t, &mut log);
    assert_eq!(
        w.tracker.writes().len(),
        before,
        "nothing written but the relations removed above"
    );
    let lines = intents.lines();
    assert!(
        lines.contains(&format!("tracker: {sub} → In Progress")),
        "{lines:?}"
    );
    assert!(
        lines.contains(&format!("tracker: {sub} Blocks {consolidation}")),
        "{lines:?}"
    );
}

// ---- spec 05 §3: the coordinator receives the consolidation item ----

fn release_coordinator() -> crate::tracker::User {
    crate::tracker::User {
        id: "u-coordinator".into(),
        name: "coordinator-handle".into(),
    }
}

#[test]
fn a_candidates_consolidation_item_goes_to_its_coordinator_once() {
    let w = World::new();
    assert_eq!(
        w.config
            .release
            .as_ref()
            .unwrap()
            .coordinator_assignee
            .as_deref(),
        Some("coordinator-handle")
    );
    w.tracker
        .state
        .borrow_mut()
        .users
        .push(release_coordinator());
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.consolidation(CAND).assignee, Some(release_coordinator()));
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert!(writes.is_empty(), "{writes:?}");
}

#[test]
fn without_a_release_coordinator_assignee_the_consolidation_item_is_unassigned() {
    let mut w = World::new();
    w.config.release.as_mut().unwrap().coordinator_assignee = None;
    w.tracker
        .state
        .borrow_mut()
        .users
        .push(release_coordinator());
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.consolidation(CAND).assignee, None);
}

#[test]
fn an_unknown_release_coordinator_leaves_the_item_unassigned_and_never_reassigns_it() {
    let w = World::new();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let item = w.consolidation(CAND);
    assert_eq!(item.assignee, None, "the tracker does not know the name");
    w.tracker
        .state
        .borrow_mut()
        .users
        .push(release_coordinator());
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert!(writes.is_empty(), "never reassigned: {writes:?}");
    assert_eq!(w.tracker.item(&item.key).assignee, None);
}

// ---- BR-ASDLC-020: the coordinator is served the angle item In Review ----

fn angle_reviewer() -> crate::tracker::User {
    crate::tracker::User {
        id: "u-reviewer".into(),
        name: "reviewer-handle".into(),
    }
}

impl World {
    fn knows(&self, users: &[crate::tracker::User]) {
        self.tracker
            .state
            .borrow_mut()
            .users
            .extend(users.iter().cloned());
    }

    /// The security angle PR, ready and green, not reviewed yet.
    fn ready_angle_pr(&self) -> PullRequest {
        let mut pr = self.angle_pr(30, "security", &report_json("security", ""));
        pr.reviews.clear();
        self.open_on_branch(vec![pr.clone()]);
        pr
    }
}

#[test]
fn in_review_serves_the_coordinator_and_changes_requested_serve_the_angle_again() {
    let w = World::new();
    assert_eq!(
        w.config
            .release
            .as_ref()
            .unwrap()
            .angles
            .iter()
            .find(|a| a.key == "security")
            .and_then(|a| a.assignee.as_deref()),
        Some("reviewer-handle")
    );
    w.knows(&[release_coordinator(), angle_reviewer()]);
    w.tick();
    let sub = w.sub_item("security").key;
    assert_eq!(w.tracker.item(&sub).assignee, Some(angle_reviewer()));

    let mut pr = w.ready_angle_pr();
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        writes,
        vec![format!(
            "status {sub} In Review (assigned coordinator-handle)"
        )]
    );
    w.quiet();

    pr.reviews = vec![coordinator("CHANGES_REQUESTED", &pr.head_sha)];
    w.open_on_branch(vec![pr.clone()]);
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        writes,
        vec![format!(
            "status {sub} In Progress (assigned reviewer-handle)"
        )]
    );
    w.quiet();

    // Fixed and approved: In Review for the coordinator, then Done at the
    // merge, the assignee left as it is.
    pr.head_sha = "fixedhead0000001".into();
    w.github.state.borrow_mut().files.insert(
        (
            repo(),
            pr.head_sha.clone(),
            format!(".spl-asdlc/qa/{CAND}/security.json"),
        ),
        Ok(report_json("security", "")),
    );
    pr.reviews.push(coordinator("APPROVED", &pr.head_sha));
    w.open_on_branch(vec![pr]);
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        writes,
        vec![
            format!("status {sub} In Review (assigned coordinator-handle)"),
            format!("status {sub} Done"),
        ]
    );
    assert_eq!(w.tracker.item(&sub).assignee, Some(release_coordinator()));
    w.quiet();
}

#[test]
fn sent_back_with_no_angle_assignee_the_item_is_unassigned() {
    let mut w = World::new();
    for a in &mut w.config.release.as_mut().unwrap().angles {
        a.assignee = None;
    }
    w.knows(&[release_coordinator(), angle_reviewer()]);
    w.tick();
    let sub = w.sub_item("security").key;
    let mut pr = w.ready_angle_pr();
    w.tick();
    assert_eq!(w.tracker.item(&sub).assignee, Some(release_coordinator()));
    pr.reviews = vec![coordinator("CHANGES_REQUESTED", &pr.head_sha)];
    w.open_on_branch(vec![pr]);
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        writes,
        vec![format!("status {sub} In Progress (unassigned)")],
        "taken by role"
    );
    w.quiet();
}

#[test]
fn an_unset_or_unknown_coordinator_leaves_the_angle_items_assignee() {
    for unset in [true, false] {
        let mut w = World::new();
        if unset {
            w.config.release.as_mut().unwrap().coordinator_assignee = None;
        }
        // Unknown: the tracker knows the angle's reviewer, not the coordinator.
        w.knows(&[angle_reviewer()]);
        w.tick();
        let sub = w.sub_item("security").key;
        w.ready_angle_pr();
        let (r, writes) = w.writes_of_a_tick();
        assert!(r.ok(), "{r:?}");
        assert_eq!(writes, vec![format!("status {sub} In Review")], "{unset}");
        assert_eq!(w.tracker.item(&sub).assignee, Some(angle_reviewer()));
        w.quiet();
    }
}

#[test]
fn a_dry_run_prints_the_coordinator_served_in_review() {
    let w = World::new();
    w.knows(&[release_coordinator()]);
    w.tick();
    let sub = w.sub_item("security").key;
    w.ready_angle_pr();
    let before = w.tracker.writes().len();
    let intents = crate::dry::Intents::default();
    let tr = crate::dry::DryTracker {
        inner: &w.tracker,
        intents: &intents,
    };
    let sig = crate::dry::DrySignal { intents: &intents };
    let t = Tick {
        tracker: &tr,
        signal: &sig,
        dry_run: true,
        ..w.tick_ctx()
    };
    let mut log = ActionLog::in_memory();
    run(&t, &mut log);
    assert_eq!(w.tracker.writes().len(), before, "nothing written");
    let lines = intents.lines();
    assert!(
        lines.contains(&format!(
            "tracker: {sub} → In Review, assigned to coordinator-handle"
        )),
        "{lines:?}"
    );
}

// ---- without a release cycle (spec 05, "Without a release cycle") ----

#[test]
fn without_a_release_section_the_tick_skips_the_release_and_closure_passes() {
    let mut w = World::new();
    w.config.release = None;
    // The cut and a green gate: a cycle would open at the first tick.
    for _ in 0..2 {
        let r = w.tick();
        assert!(r.ok(), "{r:?}");
    }
    assert!(w.tracker.writes().is_empty(), "{:?}", w.tracker.writes());
    assert!(w.github.writes().is_empty(), "{:?}", w.github.writes());
    assert!(
        w.tracker.list_by_label(LABEL).unwrap().is_empty(),
        "no release item"
    );

    // A cycle opened while the section was there, then published and
    // merged back: the closure would judge ABC-5 at the next tick.
    w.config.release = config().release;
    assert!(w.tick().ok());
    w.release_item();
    publish(&w);
    w.config.release = None;
    let before = (w.tracker.writes().len(), w.github.writes().len());
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        (w.tracker.writes().len(), w.github.writes().len()),
        before,
        "{:?}",
        w.tracker.writes()
    );
    assert_eq!(
        w.tracker.item("ABC-5").state.status,
        Some(Status::ToDeploy),
        "no closure judges it"
    );
    assert!(w.signal.sent.borrow().is_empty(), "no release note");
}

/// A host's daemon runs the bridge: nothing panics on a configuration
/// without `[release]`, even a step the pass never reaches without it.
#[test]
fn without_a_release_section_the_release_pass_and_its_readers_return_cleanly() {
    let mut w = World::new();
    w.config.release = None;
    let t = w.tick_ctx();
    let mut log = ActionLog::in_memory();
    let mut report = Report::default();
    assert!(!release::pass(&t, &mut log, &mut report), "no merge");
    assert!(report.ok(), "nothing to report: {report:?}");
    assert!(log.entries().is_empty(), "{:?}", log.entries());
    assert_eq!(release::section(&t).unwrap_err(), release::NO_RELEASE_CYCLE);
    let candidate = release::Candidate {
        tag: CAND.into(),
        version: crate::method::release::shape::Version::parse("1.2.0").unwrap(),
        rc: 1,
        sha: "c1".into(),
        class: "minor".into(),
        tagged_at: None,
    };
    assert!(release::offered(&t, &candidate).is_empty());
    assert!(w.github.writes().is_empty());
    assert!(w.tracker.writes().is_empty());
}

#[test]
fn without_a_release_section_the_release_verbs_say_there_is_no_cycle() {
    let mut w = World::new();
    w.config.release = None;
    let t = w.tick_ctx();
    let mut log = ActionLog::in_memory();
    for refused in [
        crate::verbs::cut(&t, &mut log, true).unwrap_err(),
        crate::verbs::status(&t).err().unwrap(),
        crate::verbs::close(&t, &mut log, "1.2.0", "a reason", "someone").unwrap_err(),
        crate::verbs::abandon(&t, &mut log, "someone").unwrap_err(),
        crate::verbs::requalify(
            &t,
            &mut log,
            "ABC-5",
            crate::method::vocabulary::Severity::Nit,
            "a reason",
            "someone",
        )
        .unwrap_err(),
    ] {
        assert_eq!(
            refused,
            "acme has no release cycle: its configuration has no [release] section"
        );
    }
    assert!(w.github.writes().is_empty());
    assert!(w.tracker.writes().is_empty());
}

// ---- SID-1016: a release with no QA angle; its GO is the gate ----

impl World {
    /// The cut of `World::new`, for a project with `release.angles = []`
    /// and no release coordinator.
    fn without_angles() -> World {
        let mut w = World::new();
        w.config.release.as_mut().unwrap().angles.clear();
        w.config.release.as_mut().unwrap().coordinators.clear();
        w
    }

    /// The merges the bridge made on GitHub.
    fn merges(&self) -> Vec<String> {
        self.github
            .writes()
            .into_iter()
            .filter(|l| l.starts_with("merge"))
            .collect()
    }
}

#[test]
fn a_release_with_no_qa_angle_opens_no_round_and_posts_the_brief_once() {
    let w = World::without_angles();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let item = w.release_item();
    assert!(
        item.description.contains("No QA angle is configured"),
        "{}",
        item.description
    );
    let comments = w.tracker.comments(&item.key).unwrap();
    let candidate = comments
        .iter()
        .find(|c| c.body.starts_with(&format!("### Candidate `{CAND}`")))
        .expect("the candidate is recorded");
    assert!(texts::has_marker(
        &candidate.body,
        &texts::no_qa_angle_marker(CAND)
    ));
    assert!(
        w.tracker.item("ABC-5").labels.contains(&LABEL.to_string()),
        "embarked"
    );
    assert!(
        w.round(CAND).is_empty(),
        "no angle sub-item, no consolidation item"
    );
    let briefs = w.briefs(CAND);
    assert_eq!(briefs.len(), 1);
    assert!(
        briefs[0].contains("No QA angle is configured"),
        "{}",
        briefs[0]
    );
    assert!(
        briefs[0].contains("actions/runs/1#artifacts"),
        "{}",
        briefs[0]
    );

    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert!(writes.is_empty(), "{writes:?}");
}

#[test]
fn with_no_qa_angle_a_green_gate_is_a_go_and_the_release_pr_is_merged() {
    let w = World::without_angles();
    w.tick();
    let (_, d) = crate::verbs::status(&w.tick_ctx()).unwrap();
    assert!(
        d.missing_angles.is_empty() && d.open_angles.is_empty(),
        "{d:?}"
    );
    assert!(
        !d.reasons
            .iter()
            .any(|r| r.contains("no angle sub-item and no verdict")),
        "{:?}",
        d.reasons
    );
    let rp = release_pr(&w, "1.2.0");
    w.open_on_branch(vec![rp.clone()]);
    let (_, d) = crate::verbs::status(&w.tick_ctx()).unwrap();
    assert!(d.go, "{:?}", d.reasons);
    let r = w.tick();
    assert_eq!(
        w.merges(),
        vec![format!(
            "merge example-org/acme#40 at {} squash",
            rp.head_sha
        )],
        "{r:?}"
    );
}

#[test]
fn with_no_qa_angle_a_red_gate_is_no_go() {
    let w = World::without_angles();
    w.red_settled(1, CAND, "c1", r#""tests""#);
    w.tick();
    w.open_on_branch(vec![release_pr(&w, "1.2.0")]);
    let r = w.tick();
    assert!(w.merges().is_empty(), "{r:?}");
    let (_, d) = crate::verbs::status(&w.tick_ctx()).unwrap();
    assert!(!d.go);
    assert_eq!(d.reasons, vec!["the CI gate is not green"]);
    assert!(w.briefs(CAND).is_empty(), "no brief on a red gate");
    assert_eq!(w.gate_fixes().len(), 1, "the gate's own fix still opens");
}

#[test]
fn with_no_qa_angle_a_blocker_still_holds_the_release() {
    let w = World::without_angles();
    w.tick();
    let release = w.release_item();
    w.tracker
        .seed("ABC-9", "Blocker", "## Goal\nFix.\n", Status::Ready, &[]);
    w.tracker
        .relate("ABC-9", &release.key, crate::tracker::RelationKind::Blocks)
        .unwrap();
    w.open_on_branch(vec![release_pr(&w, "1.2.0")]);
    w.tick();
    assert!(w.merges().is_empty());
    let (_, d) = crate::verbs::status(&w.tick_ctx()).unwrap();
    assert_eq!(d.reasons, vec!["open blocking issues: ABC-9"]);
}

#[test]
fn angles_configured_later_apply_from_the_next_candidate() {
    let mut w = World::without_angles();
    w.tick();
    assert!(w.round(CAND).is_empty());
    // The project configures its angles while rc.1 is in flight.
    w.config = config();
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert!(writes.is_empty(), "rc.1 keeps no angle: {writes:?}");
    assert!(w.round(CAND).is_empty());
    let (_, d) = crate::verbs::status(&w.tick_ctx()).unwrap();
    assert!(
        d.missing_angles.is_empty(),
        "rc.1 owes no angle: {:?}",
        d.reasons
    );

    w.cut_rc2();
    w.gate(vec![
        gate_run(2, CAND2, "c2", Some("success")),
        gate_run(1, CAND, "c1", Some("success")),
    ]);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        w.round(CAND2).len(),
        3,
        "two angles and the consolidation item"
    );
    let briefs = w.briefs(CAND2);
    assert_eq!(briefs.len(), 1);
    assert!(!briefs[0].contains("No QA angle"), "{}", briefs[0]);
}

#[test]
fn with_no_qa_angle_the_closure_works_as_today() {
    let w = World::without_angles();
    w.tick();
    let release = w.release_item();
    publish(&w);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.tracker.item("ABC-5").state.status, Some(Status::Done));
    assert_eq!(
        w.tracker.item(&release.key).state.status,
        Some(Status::Done)
    );
    assert_eq!(w.signal.sent.borrow().len(), 1, "the note is sent");
}
