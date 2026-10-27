//! The release pass and the closure against the in-memory GitHub and
//! tracker (spec 05): the cycle opened at the first tick after the cut, its
//! QA round and brief only once the candidate's gate is green, the
//! angle PRs merged with their findings filed once, the GO and the release
//! tagged and published, the closure and the release note.

use super::*;
use crate::config::Config;
use crate::github::fake::{FakeGitHub, METHOD_AT as AT, METHOD_REF, pull_request};
use crate::github::{
    Actor, Artifact, CheckEntry, CheckKind, Commit, CommitPull, CommitSummary, CompareStatus,
    Comparison, GitRef, Identity, ObjectType, PrState, PullRequest, Release, RepoSlug,
    RequiredChecks, Review, Timestamp, WorkflowRun,
};
use crate::method::release::gate;
use crate::method::texts::{self, BridgeItem, protocol};
use crate::method::vocabulary::Status;

const POST_DEPLOY_LABEL: &str = "post-deploy-validation/acme";
use crate::notify::Recorder;
use crate::tracker::Priority;
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
                committed_at: None,
            })
            .collect(),
        files: files.iter().map(|f| f.to_string()).collect(),
        base_committed_at: None,
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
    let verdict = if findings.contains(r#""blocking":true"#) {
        "hold"
    } else {
        "go"
    };
    format!(
        r#"{{"schema":"spl-asdlc/qa-report/2","candidate":"{CAND}","angle":"{angle}","item":"ABC-123",
           "verdict":"{verdict}","checked":["the whole project"],"findings":[{findings}]}}"#
    )
}

fn finding(id: &str, blocking: bool) -> String {
    format!(
        r#"{{"id":"{id}","blocking":{blocking},"summary":"Finding {id}","anchor":"src/a.rs:1","evidence":"e","testable":true,"proposed_test":"test_{id}"}}"#
    )
}

/// A merged report of the older schema, whose findings had a severity.
fn legacy_report_json(angle: &str, findings: &[(&str, &str)]) -> String {
    let findings: Vec<String> = findings
        .iter()
        .map(|(id, severity)| {
            format!(
                r#"{{"id":"{id}","severity":"{severity}","summary":"Finding {id}","anchor":"src/a.rs:1","evidence":"e","fixed_in_branch":false,"testable":false,"proposed_test":null,"ticket":null}}"#
            )
        })
        .collect();
    format!(
        r#"{{"schema":"spl-asdlc/qa-report/1","candidate":"{CAND}","angle":"{angle}","item":"ABC-123",
           "verdict":"go-with-note","checked":["the whole project"],"findings":[{}]}}"#,
        findings.join(",")
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
        github.pin_method(&repo(), METHOD_REF);
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
                    title: "feat: five".into(),
                    merge_commit_sha: Some("m1".into()),
                    base_ref: "main".into(),
                    head_ref: "feat/five".into(),
                    ..CommitPull::default()
                }]),
            );
            s.runs.insert(
                (repo(), "release-gate.yml".into()),
                Ok(vec![gate_run(1, CAND, "c1", Some("success"))]),
            );
            // No publication has run yet.
            s.runs
                .insert((repo(), "release-publish.yml".into()), Ok(Vec::new()));
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
            for sha in ["c1", "c2"] {
                s.files
                    .insert((repo(), sha.into(), "CHANGELOG.md".into()), Ok(changelog()));
            }
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
            method_texts: Default::default(),
            low_quota: false,
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

    /// The changes of an angle PR at `head` since it left the release
    /// branch: its report and nothing else.
    fn report_only(&self, head: &str, angle: &str) {
        let path = format!(".spl-asdlc/qa/{CAND}/{angle}.json");
        self.github.state.borrow_mut().compares.insert(
            (repo(), BRANCH.into(), head.into()),
            cmp(CompareStatus::Ahead, &["q1"], &[&path]),
        );
    }

    fn angle_pr(&self, number: u64, angle: &str, report: &str) -> PullRequest {
        let sub = self.sub_item(angle);
        let mut pr = green(pull_request(number));
        pr.base_ref = BRANCH.into();
        pr.head_ref = format!("qa/{CAND}/{angle}");
        pr.body = format!("Angle {angle}, sub-item {}", sub.key);
        pr.reviews = vec![Review {
            id: None,
            author: Some("coordinator-login".into()),
            body: "ok".into(),
            state: "APPROVED".into(),
            commit_sha: Some(pr.head_sha.clone()),
            submitted_at: None,
        }];
        let path = format!(".spl-asdlc/qa/{CAND}/{angle}.json");
        self.report_only(&pr.head_sha, angle);
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

impl World {
    /// The tracker knows the project's owner (`[owner] tracker_user`).
    fn owner_known(&self) {
        self.tracker
            .state
            .borrow_mut()
            .users
            .push(crate::tracker::User {
                id: "u-owner".into(),
                name: "owner-handle".into(),
            });
    }
}

#[test]
fn the_release_item_is_assigned_to_the_owner_when_the_tracker_knows_them() {
    let w = World::new();
    w.owner_known();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        w.release_item().assignee.map(|u| u.name),
        Some("owner-handle".to_string())
    );
}

#[test]
fn the_release_item_is_unassigned_when_the_tracker_does_not_know_the_owner() {
    let w = World::new();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.release_item().assignee, None);
}

#[test]
fn a_release_item_that_exists_unassigned_is_not_reassigned() {
    let w = World::new();
    w.tick();
    assert_eq!(w.release_item().assignee, None);
    // The owner becomes known in the middle of the cycle.
    w.owner_known();
    let before = w.tracker.writes().len();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.release_item().assignee, None);
    let writes = &w.tracker.writes()[before..];
    assert!(writes.is_empty(), "{writes:?}");
}

#[test]
fn a_tick_against_the_candidates_log_as_linear_stores_it_writes_nothing() {
    let w = World::new();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let key = w.release_item().key;
    // Linear's normalised form: `*` bullets, blank lines after the headings
    // and around the items, trailing spaces.
    w.tracker.restyle(&key, |d| {
        d.replace("\n- `", "\n\n* `")
            .replace("first candidate", "first candidate  ")
            .replace("## Goal\n", "## Goal\n\n")
    });
    let normalised = w.tracker.item(&key).description;
    assert!(
        normalised.contains(&format!("* `{CAND}` —")),
        "{normalised}"
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
    assert_eq!(w.tracker.item(&key).description, normalised);
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
        // The items of a candidate's milestone count as the release's own.
        let mut children = Vec::new();
        for ch in self.tracker.children(&release.key).unwrap() {
            if texts::milestone_scope(&ch.description).is_some() {
                children.extend(self.tracker.children(&ch.key).unwrap());
            } else {
                children.push(ch);
            }
        }
        children
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
        self.reruns(vec![rerun_run(90 + id, "completed")]);
        self.suites(&format!(r#"{{"failed":[{failed}],"suites":[]}}"#));
    }
}

/// The default branch's head commit, which GitHub gives every run of a
/// `workflow_run` workflow whatever the commit of the run that triggered it.
const DEFAULT_HEAD: &str = "main-head";

/// A run of the automatic-rerun workflow, triggered by a gate run: as
/// GitHub lists it, on the default branch's head, not the candidate's.
fn rerun_run(id: u64, status: &str) -> WorkflowRun {
    WorkflowRun {
        event: "workflow_run".into(),
        status: status.into(),
        conclusion: (status == "completed").then(|| "success".to_string()),
        ..gate_run(id, "main", DEFAULT_HEAD, None)
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
    w.reruns(vec![rerun_run(92, "completed")]);
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

// ---- BR-ASDLC-032: the texts of the version the candidate pins ----

#[test]
fn a_candidates_items_carry_the_texts_of_the_version_its_callers_pin_at_its_tag() {
    use crate::method::texts::protocol::{Job, angle_path};
    let w = World::new();
    let method = RepoSlug::parse(crate::github::fake::METHOD_REPO).unwrap();
    {
        let mut s = w.github.state.borrow_mut();
        // The candidate's own callers pin v1.3.0; elsewhere the project is
        // on the fixtures' v1.
        s.files.insert(
            (repo(), CAND.into(), ".github/workflows/pr-title.yml".into()),
            Ok("jobs:\n  title:\n    uses: example-org/spl-asdlc/.github/workflows/asdlc-pr-title.yml@v1.3.0\n".into()),
        );
        let mut put = |path: String, text: &str| {
            s.files
                .insert((method.clone(), "v1.3.0".into(), path), Ok(text.into()));
        };
        put(
            Job::ReleaseAngle.path().into(),
            "# Review a release candidate from one angle\n\nThe pinned release protocol.",
        );
        put(
            Job::ReleaseConsolidation.path().into(),
            "# Consolidate\n\nThe pinned consolidation protocol.",
        );
        put(
            angle_path("security"),
            "# Angle: security\n\nThe pinned security text.",
        );
    }
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let round = w.round(CAND);
    assert_eq!(round.len(), 3);
    for i in &round {
        let d = &i.description;
        assert!(
            d.contains("The method's texts at `example-org/spl-asdlc@v1.3.0`"),
            "{d}"
        );
        assert!(
            d.contains("\"schema\": \"spl-asdlc/qa-report/2\""),
            "the bridge's own schema: {d}"
        );
    }
    let security = round
        .iter()
        .find(|i| i.title.starts_with("Security"))
        .unwrap();
    assert!(
        security
            .description
            .contains("The pinned release protocol.")
    );
    assert!(security.description.contains("The pinned security text."));
    // `product` has no text at v1.3.0: a line says so, never this build's.
    let product = round
        .iter()
        .find(|i| i.title.starts_with("Product"))
        .unwrap();
    assert!(
        product.description.contains(
            "The method has no notes for the `product` angle at `example-org/spl-asdlc@v1.3.0`"
        ),
        "{}",
        product.description
    );
}

#[test]
fn a_candidates_texts_that_cannot_be_read_open_no_item_and_are_said_once_on_the_release_item() {
    let w = World::new();
    w.github.state.borrow_mut().files.insert(
        (repo(), CAND.into(), ".github/workflows/pr-title.yml".into()),
        Ok("jobs:\n  title:\n    uses: example-org/spl-asdlc/.github/workflows/asdlc-pr-title.yml@v9.9.9\n".into()),
    );
    for _ in 0..2 {
        let r = w.tick();
        assert!(!r.ok(), "{r:?}");
        let said = r.failures.join("; ");
        assert!(
            said.contains(&format!("the method's texts for the QA items of {CAND}"))
                && said.contains("is not at example-org/spl-asdlc@v9.9.9"),
            "{said}"
        );
    }
    assert!(w.round(CAND).is_empty(), "no item, so no fallback");
    let release = w.release_item();
    let notices: Vec<_> = w
        .tracker
        .comments(&release.key)
        .unwrap()
        .into_iter()
        .filter(|c| c.body.contains("spl-asdlc:method-texts"))
        .collect();
    assert_eq!(notices.len(), 1, "{notices:?}");
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
                    "The method's texts at `{AT}`, the version of spl-asdlc this project's workflows are pinned on, copied by the bridge (spl-asdlc {})",
                    texts::protocol::VERSION
                ))
                && i.description
                    .contains("\"schema\": \"spl-asdlc/qa-report/2\""),
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
            .contains(&format!("\n## Protocol\n\nThe method's texts at `{AT}`"))
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
        w.config.release.coordinators = vec![configured.clone()];
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
fn an_angle_pr_dismisses_an_apps_superseded_request_for_changes_and_a_person_holds_it() {
    // (author of the earlier request for changes, merged)
    for (author, merged) in [(bot("angle-app"), true), ("passer-by".to_string(), false)] {
        let w = World::new();
        w.tick();
        let mut pr = w.angle_pr(30, "security", &report_json("security", ""));
        pr.reviews.insert(
            0,
            Review {
                id: Some(51),
                author: Some(author.clone()),
                body: "redo".into(),
                state: "CHANGES_REQUESTED".into(),
                commit_sha: Some("older".into()),
                submitted_at: None,
            },
        );
        w.open_on_branch(vec![pr.clone()]);
        let report = w.tick();
        let writes = w.github.writes();
        let merge = format!("merge example-org/acme#30 at {} merge", pr.head_sha);
        let dismissal = writes
            .iter()
            .position(|l| l.starts_with("dismiss-review example-org/acme#30 51: "));
        assert_eq!(writes.contains(&merge), merged, "{author}: {writes:?}");
        assert_eq!(dismissal.is_some(), merged, "{author}: {writes:?}");
        if merged {
            let at = writes.iter().position(|l| *l == merge).unwrap();
            assert!(dismissal.unwrap() < at, "{writes:?}");
            assert!(writes[dismissal.unwrap()].contains(&format!(
                "coordinator-login approved this PR at its head `{}`",
                pr.head_sha
            )));
        } else {
            assert!(
                report.waiting_lines().contains(
                    &"waiting: pr#30 changes requested by passer-by (a person; the bridge never dismisses it)"
                        .to_string()
                ),
                "{:?}",
                report.waiting_lines()
            );
            assert_eq!(w.sub_item("security").state.status, Some(Status::InReview));
        }
    }
}

#[test]
fn an_approved_angle_pr_with_a_valid_report_is_merged_and_its_findings_filed_once() {
    let mut w = World::new();
    w.config.release.fix_assignee = Some("fixer-handle".into());
    w.tracker
        .state
        .borrow_mut()
        .users
        .push(crate::tracker::User {
            id: "u-fixer".into(),
            name: "fixer-handle".into(),
        });
    w.tick();
    let findings = [
        finding("security-1", true),
        finding("security-2", false),
        finding("security-3", false),
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
    // The blocking finding: an urgent fix ticket for the release fixer,
    // which blocks the release issue.
    let blocking = labelled
        .iter()
        .find(|i| i.title.starts_with("Finding security-1"))
        .expect("the blocking finding's ticket");
    assert_eq!(blocking.title, "Finding security-1 (blocking — security)");
    assert_eq!(w.tracker.priority(&blocking.key), Some(Priority::Urgent));
    assert_eq!(
        blocking.assignee.as_ref().map(|u| u.name.as_str()),
        Some("fixer-handle")
    );
    assert_eq!(blocking.state.status, Some(Status::Ready));
    assert!(!blocking.labels.contains(&"qa-finding".to_string()));
    assert!(crate::method::contract::missing_from_contract(&blocking.description).is_empty());
    assert!(
        w.tracker
            .relations(&release.key)
            .unwrap()
            .blocked_by
            .iter()
            .any(|b| b.other == blocking.key)
    );
    // Each non-blocking finding: a ticket of its own, logged for later.
    for id in ["security-2", "security-3"] {
        let logged = labelled
            .iter()
            .find(|i| i.title.starts_with(&format!("Finding {id}")))
            .expect("a non-blocking finding's ticket");
        assert_eq!(
            logged.title,
            format!("Finding {id} (non-blocking — security)")
        );
        assert!(logged.labels.contains(&"qa-finding".to_string()));
        assert_eq!(w.tracker.priority(&logged.key), Some(Priority::Normal));
        assert_eq!(logged.assignee, None);
        assert_eq!(logged.state.status, Some(Status::Triage));
        assert!(crate::method::contract::missing_from_contract(&logged.description).is_empty());
        assert!(
            w.tracker
                .relations(&logged.key)
                .unwrap()
                .related
                .iter()
                .any(|r| r.other == release.key)
        );
    }
    assert!(
        labelled.iter().all(|i| !i.title.starts_with("Nits")),
        "no nits ticket"
    );
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

/// The section `release prepare` wrote for 1.2.0 when the cut opened the
/// cycle: the notes of the release.
const NOTES: &str = "## [1.2.0](https://github.com/example-org/acme/compare/acme-v1.1.0...acme-v1.2.0) (2026-10-05)\n\n\n### Features\n\n* m1 ([#5](https://github.com/example-org/acme/issues/5)) ([m1](https://github.com/example-org/acme/commit/m1))";

/// The CHANGELOG the cut committed on the release branch before tagging
/// its first candidate.
fn changelog() -> String {
    format!("# Changelog\n\n{NOTES}\n\n## [1.1.0](x) (2026-09-01)\n\n\n### Bug Fixes\n\n* older\n")
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

/// What the GO writes on GitHub for a candidate at `sha`: the release tag
/// on its commit, then the release with the prepared notes.
fn go_lines(sha: &str) -> Vec<String> {
    vec![
        format!("create-tag example-org/acme acme-v1.2.0 at {sha}"),
        "create-release example-org/acme acme-v1.2.0 acme 1.2.0".to_string(),
    ]
}

impl World {
    /// The release tag and release the bridge wrote on GitHub.
    fn go_writes(&self) -> Vec<String> {
        self.github
            .writes()
            .into_iter()
            .filter(|l| l.starts_with("create-tag") || l.starts_with("create-release"))
            .collect()
    }

    fn go_comments(&self) -> usize {
        self.tracker
            .writes()
            .iter()
            .filter(|l| l.contains("GO computed"))
            .count()
    }
}

#[test]
fn the_go_tags_the_candidate_and_publishes_its_prepared_notes_and_nothing_else() {
    let w = World::new();
    w.tick();
    all_angles_merged(&w);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.go_writes(), go_lines("c1"), "{r:?}");
    assert!(
        !w.github.writes().iter().any(|l| l.starts_with("merge")),
        "no release PR: nothing merged on the GO"
    );
    let release = w
        .github
        .release(&repo(), "acme-v1.2.0")
        .expect("the release is published");
    assert_eq!(
        release.body, NOTES,
        "the notes the cut prepared, as they are"
    );
    assert_eq!(w.go_comments(), 1);
    // The release published: the next tick is the closure's, and tags
    // nothing again.
    let before = w.go_writes();
    w.tick();
    assert_eq!(w.go_writes(), before);
    assert_eq!(w.go_comments(), 1);
}

/// Spec 09 (BR-ASDLC-021): the release tag a person pushed on the
/// candidate is taken as done; the bridge publishes only the release.
#[test]
fn a_release_tag_made_by_hand_on_the_candidate_is_taken_as_done() {
    let w = World::new();
    w.tick();
    all_angles_merged(&w);
    w.github
        .state
        .borrow_mut()
        .tags
        .get_mut(&repo())
        .unwrap()
        .push(tag("acme-v1.2.0", "c1"));
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        w.go_writes(),
        vec!["create-release example-org/acme acme-v1.2.0 acme 1.2.0".to_string()],
        "{r:?}"
    );
}

#[test]
fn a_release_tag_on_another_commit_stops_the_go_and_says_why() {
    let w = World::new();
    w.tick();
    all_angles_merged(&w);
    w.github
        .state
        .borrow_mut()
        .tags
        .get_mut(&repo())
        .unwrap()
        .push(tag("acme-v1.2.0", "elsewhere"));
    let r = w.tick();
    assert!(w.go_writes().is_empty(), "{:?}", w.github.writes());
    assert!(
        r.failures
            .iter()
            .any(|f| f.contains("acme-v1.2.0 exists on") && f.contains("not on the candidate")),
        "{r:?}"
    );
}

#[test]
fn a_candidate_without_its_prepared_notes_is_not_released() {
    let w = World::new();
    w.tick();
    all_angles_merged(&w);
    w.github.state.borrow_mut().files.insert(
        (repo(), "c1".into(), "CHANGELOG.md".into()),
        Ok("# Changelog\n\n## [1.1.0](x) (2026-09-01)\n".into()),
    );
    let r = w.tick();
    assert!(w.go_writes().is_empty(), "{:?}", w.github.writes());
    assert!(
        r.failures
            .iter()
            .any(|f| f.contains("has no section for 1.2.0")),
        "{r:?}"
    );
}

#[test]
fn a_release_refused_after_its_tag_is_retried_without_a_second_tag() {
    let w = World::new();
    w.tick();
    all_angles_merged(&w);
    w.github
        .fail_writes_starting_with("create-release", Some(500), "boom");
    let r = w.tick();
    assert!(!r.ok());
    assert_eq!(w.go_comments(), 0);
    w.github.state.borrow_mut().failing_writes.clear();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        w.go_writes(),
        vec![
            "create-tag example-org/acme acme-v1.2.0 at c1".to_string(),
            "create-release example-org/acme acme-v1.2.0 acme 1.2.0".to_string(),
        ],
        "one tag, and the release once accepted"
    );
    assert_eq!(w.go_comments(), 1);
}

#[test]
fn the_brief_lists_no_commit_as_outside_the_notes() {
    // The notes follow the graph: a commit merged into the default branch
    // while the previous cycle was open is in this range and in its notes.
    let w = World::new();
    w.tick();
    let briefs = w.briefs(CAND);
    assert_eq!(briefs.len(), 1);
    assert!(
        !briefs[0].contains("Not in the release note"),
        "{}",
        briefs[0]
    );
}

/// The product's first release: no published release below the candidate,
/// so its range is the whole history, read from the repository's root, and
/// no tag was set by hand. `extra` commits follow PR #5's feature.
fn first_release(extra: &[(&str, &str)]) -> World {
    let w = World::new();
    {
        let mut s = w.github.state.borrow_mut();
        s.tags.insert(repo(), vec![tag(CAND, "c1")]);
        s.compares
            .remove(&(repo(), "acme-v1.1.0".into(), "c1".into()));
        let mut commits = vec![("r0", "initial"), ("m1", "feat: m1")];
        commits.extend_from_slice(extra);
        for (sha, _) in &commits {
            if *sha != "m1" {
                s.commit_pulls
                    .insert((repo(), sha.to_string()), Ok(Vec::new()));
            }
        }
        s.histories.insert(
            (repo(), "c1".into()),
            Ok(Comparison {
                status: CompareStatus::Ahead,
                ahead_by: commits.len() as u64,
                behind_by: 0,
                commits: commits
                    .iter()
                    .map(|(sha, subject)| CommitSummary {
                        sha: sha.to_string(),
                        subject: subject.to_string(),
                        committed_at: Timestamp::parse("2026-10-05T10:00:00Z"),
                    })
                    .collect(),
                files: vec!["README.md".into(), "src/a.rs".into()],
                base_committed_at: None,
            }),
        );
    }
    w
}

#[test]
fn a_first_release_is_measured_from_the_repository_root_with_no_tag_set_by_hand() {
    let w = first_release(&[("f3", "fix: the very first fix (#3)")]);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    // The whole history is the range: PR #5's ticket is embarked.
    assert!(
        w.tracker.item("ABC-5").labels.contains(&LABEL.to_string()),
        "embarked"
    );
    let briefs = w.briefs(CAND);
    assert_eq!(briefs.len(), 1);
    let brief = &briefs[0];
    assert!(
        brief.contains("- Diff since the repository's root: first release, no earlier release to measure from — 2 file(s)\n"),
        "{brief}"
    );
    assert!(brief.contains("**ABC-5**"), "{brief}");
    // The GO tags the candidate and publishes the notes the cut prepared.
    all_angles_merged(&w);
    let r = w.tick();
    assert_eq!(w.go_writes(), go_lines("c1"), "{r:?}");
}

/// Spec 09 (BR-ASDLC-021): a person tags the candidate and publishes the
/// release in GitHub instead of the bridge. The bridge picks up from that
/// state: it tags nothing and says no GO of its own, closes the cycle once
/// the release is merged back, records who published it, and writes nothing
/// more at the next tick.
#[test]
fn a_release_published_by_a_human_is_picked_up_without_a_rewrite_and_recorded_at_closure() {
    let w = World::new();
    w.tick();
    let release = w.release_item();
    all_angles_merged(&w);
    publish(&w);
    w.github
        .state
        .borrow_mut()
        .releases
        .get_mut(&(repo(), "acme-v1.2.0".into()))
        .unwrap()
        .as_mut()
        .unwrap()
        .author = Some(Actor {
        login: "owner-login".into(),
        is_bot: false,
    });
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert!(w.go_writes().is_empty(), "{:?}", w.github.writes());
    assert_eq!(w.go_comments(), 0);
    assert_eq!(
        w.tracker.item(&release.key).state.status,
        Some(Status::Done)
    );
    let closure = w
        .tracker
        .comments(&release.key)
        .unwrap()
        .into_iter()
        .find(|c| c.body.contains("spl-asdlc:closure"))
        .expect("the closure comment");
    assert!(
        closure
            .body
            .contains("was published by `owner-login`, a human, in GitHub"),
        "{}",
        closure.body
    );
    let before = (w.tracker.writes().len(), w.github.writes().len());
    w.tick();
    assert_eq!(
        (w.tracker.writes().len(), w.github.writes().len()),
        before,
        "nothing written twice"
    );
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
                id: 1,
                tag: "acme-v1.2.0".into(),
                url: "https://github.com/example-org/acme/releases/tag/acme-v1.2.0".into(),
                body: "### Features\n\n* ship".into(),
                draft: false,
                prerelease: false,
                published_at: None,
                author: None,
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
                id: 1,
                tag: "acme-v1.2.0".into(),
                url: String::new(),
                body: String::new(),
                draft: false,
                prerelease: false,
                published_at: None,
                author: None,
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
            method_texts: Default::default(),
            low_quota: false,
        }
    }
}

#[test]
fn release_cut_dispatches_the_next_candidate_or_refuses_a_second_cycle() {
    let w = World::new();
    let t = w.tick_ctx();
    let mut log = ActionLog::in_memory();
    let line = crate::verbs::cut(&t, &mut log, false, "me").unwrap();
    assert!(line.contains("action candidate"), "{line}");
    assert!(
        w.github
            .writes()
            .iter()
            .any(|l| l.contains("release-cut.yml") && l.contains(BRANCH))
    );
    let refused = crate::verbs::cut(&t, &mut log, true, "me").unwrap_err();
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
fn a_blocking_finding_requalified_as_non_blocking_stops_blocking_and_back() {
    let mut w = World::new();
    w.config.release.fix_assignee = Some("fixer-handle".into());
    w.tracker
        .state
        .borrow_mut()
        .users
        .push(crate::tracker::User {
            id: "u-fixer".into(),
            name: "fixer-handle".into(),
        });
    w.tick();
    let pr = w.angle_pr(
        30,
        "security",
        &report_json("security", &finding("security-1", true)),
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
        crate::verbs::requalify(&t, &mut log, &blocker, false, " ", "me").is_err(),
        "a reason is required"
    );
    assert_eq!(
        crate::verbs::requalify(&t, &mut log, &blocker, true, "Still a defect.", "me").unwrap(),
        format!("{blocker} is already blocking: nothing to do")
    );
    crate::verbs::requalify(
        &t,
        &mut log,
        &blocker,
        false,
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
    assert_eq!(item.assignee, None, "logged for later, unassigned");
    assert!(
        w.tracker
            .relations(&blocker)
            .unwrap()
            .related
            .iter()
            .any(|r| r.other == release.key)
    );
    assert!(w.tracker.comments(&release.key).unwrap().iter().any(|c| {
        c.body
            .starts_with("Requalified by me: `blocking` → `non-blocking`.")
    }));
    // And back: blocking again, served to the release fixer.
    crate::verbs::requalify(&t, &mut log, &blocker, true, "Reachable after all.", "me").unwrap();
    let item = w.tracker.item(&blocker);
    assert_eq!(
        crate::method::release::findings::parse_blocker_marker(&item.description).as_deref(),
        Some(release.key.as_str())
    );
    assert!(!item.labels.contains(&"qa-finding".to_string()));
    assert_eq!(item.state.status, Some(Status::Ready));
    assert_eq!(
        item.assignee.as_ref().map(|u| u.name.as_str()),
        Some("fixer-handle")
    );
    assert!(
        w.tracker
            .relations(&release.key)
            .unwrap()
            .blocked_by
            .iter()
            .any(|b| b.other == blocker)
    );
}

/// The last allowed candidate answered by every angle with a blocker left:
/// the tick that files the blocker escalates. The owner is known, the hold
/// label is not in the tracker yet.
fn escalating() -> World {
    let mut w = World::new();
    w.config.release.max_rc_passes = 1;
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
        (30, "security", finding("security-1", true)),
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
            id: 1,
            tag: "acme-v1.2.0".into(),
            url: String::new(),
            body: String::new(),
            draft: false,
            prerelease: false,
            published_at: None,
            author: None,
        }),
    );
    s.branches.insert(repo(), Vec::new());
    s.compares.insert(
        (repo(), "acme-v1.2.0".into(), "main".into()),
        cmp(CompareStatus::Ahead, &[], &[]),
    );
}

#[test]
fn a_release_note_no_channel_takes_is_done_once_and_never_retried() {
    let w = World {
        signal: Recorder {
            no_channel: true,
            ..Recorder::default()
        },
        ..World::new()
    };
    w.tick();
    let release = w.release_item();
    publish(&w);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert!(w.signal.sent.borrow().is_empty());
    let said = |w: &World| {
        w.tracker
            .comments(&release.key)
            .unwrap()
            .iter()
            .filter(|c| c.body.contains("has nothing to send"))
            .count()
    };
    assert_eq!(said(&w), 1);
    assert_eq!(
        w.tracker.item(&release.key).state.status,
        Some(Status::Done),
        "the cycle closes"
    );
    let before = (w.tracker.writes().len(), w.github.writes().len());
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        (w.tracker.writes().len(), w.github.writes().len()),
        before,
        "{:?}",
        &w.tracker.writes()[before.0..]
    );
    assert_eq!(said(&w), 1);
}

#[test]
fn a_failing_channel_is_retried_and_the_note_restyled_is_not_rewritten() {
    let w = World::new();
    w.tick();
    let release = w.release_item();
    publish(&w);
    let failing = World {
        signal: Recorder {
            fail: true,
            ..Recorder::default()
        },
        ..w
    };
    let describe = format!("describe {}", release.key);
    let r = failing.tick();
    assert!(
        r.failures.iter().any(|f| f.contains("notify release-note")),
        "{r:?}"
    );
    assert!(failing.tracker.writes().contains(&describe));
    // Linear's normalised form of the note: `*` bullets, blank lines.
    failing
        .tracker
        .restyle(&release.key, |d| d.replace("\n- ", "\n\n* "));
    let restyled = failing.tracker.item(&release.key).description;
    assert!(restyled.contains("\n* **ABC-5**"), "{restyled}");
    let before = failing.tracker.writes().len();
    let r = failing.tick();
    assert!(
        r.failures.iter().any(|f| f.contains("notify release-note")),
        "retried: {r:?}"
    );
    assert!(
        !failing.tracker.writes()[before..].contains(&describe),
        "{:?}",
        &failing.tracker.writes()[before..]
    );
    assert_eq!(failing.tracker.item(&release.key).description, restyled);
    assert_ne!(
        failing.tracker.item(&release.key).state.status,
        Some(Status::Done)
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
fn a_ticket_with_nothing_to_deploy_is_never_embarked() {
    // Its merge delivered it (Done): no release label, no judgement; the
    // closure of the release whose range holds its merge archives it
    // (BR-ASDLC-019), once, and the note announces it.
    let w = World::new();
    w.tracker.seed(
        "ABC-5",
        "Documentation",
        "## Goal\nDocument it.\n\n## Acceptance criteria\n- [x] documented\n\n## Business rules\nNo new rule.\n",
        Status::Done,
        &["no-deploy"],
    );
    w.tracker
        .link("ABC-5", "https://github.com/example-org/acme/pull/5");
    w.tick();
    publish(&w);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let item = w.tracker.item("ABC-5");
    assert!(!item.labels.contains(&LABEL.to_string()), "not embarked");
    assert_eq!(item.state.status, Some(Status::Done));
    let touching: Vec<String> = w
        .tracker
        .writes()
        .into_iter()
        .filter(|l| l.ends_with("ABC-5") || l.contains(" ABC-5 "))
        .collect();
    assert_eq!(touching, vec!["archive ABC-5"], "{:?}", w.tracker.writes());
    assert!(item.archived);
    let release = w.release_item();
    assert!(
        release
            .description
            .contains("- **ABC-5** — Documentation: Document it. (#5)"),
        "{}",
        release.description
    );
    assert!(release.archived, "the release item last");
    let before = w.tracker.writes().len();
    w.tick();
    assert_eq!(w.tracker.writes().len(), before, "archived once");
}

#[test]
fn a_ticket_with_nothing_to_deploy_whose_latest_pr_is_after_the_range_is_not_archived() {
    let w = World::new();
    w.tracker.seed(
        "ABC-5",
        "Documentation",
        "## Goal\nDocument it.\n\n## Acceptance criteria\n- [x] documented\n\n## Business rules\nNo new rule.\n",
        Status::Done,
        &["no-deploy"],
    );
    w.tracker
        .link("ABC-5", "https://github.com/example-org/acme/pull/5");
    w.tracker
        .comment(
            "ABC-5",
            "merged\n\nmerged_by: bridge\nmerged_pr: https://github.com/example-org/acme/pull/9\n",
        )
        .unwrap();
    w.tick();
    publish(&w);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert!(
        !w.tracker.item("ABC-5").archived,
        "its later release archives it"
    );
    assert_eq!(
        w.tracker.item(&w.release_item().key).state.status,
        Some(Status::Done)
    );
}

#[test]
fn a_blocker_canceled_as_a_false_alarm_is_not_filed_again() {
    let w = World::new();
    w.tick();
    let pr = w.angle_pr(
        30,
        "security",
        &report_json("security", &finding("security-1", true)),
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
fn a_merged_report_of_the_older_schema_is_still_filed_blocker_as_blocking() {
    let w = World::new();
    w.tick();
    let old = legacy_report_json(
        "security",
        &[("security-1", "blocker"), ("security-2", "nit")],
    );
    let mut pr = w.angle_pr(30, "security", &old);
    pr.state = PrState::Merged;
    pr.merge_commit_sha = Some("merge-of-30".into());
    w.github
        .state
        .borrow_mut()
        .merged_pulls
        .insert(repo(), Ok(vec![pr]));
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let release = w.release_item();
    let labelled = w.tracker.list_by_label(LABEL).unwrap();
    let blocking = labelled
        .iter()
        .find(|i| i.title == "Finding security-1 (blocking — security)")
        .expect("a blocker reads as blocking");
    assert_eq!(w.tracker.priority(&blocking.key), Some(Priority::Urgent));
    assert!(
        w.tracker
            .relations(&release.key)
            .unwrap()
            .blocked_by
            .iter()
            .any(|b| b.other == blocking.key)
    );
    let logged = labelled
        .iter()
        .find(|i| i.title == "Finding security-2 (non-blocking — security)")
        .expect("a nit reads as non-blocking");
    assert!(logged.labels.contains(&"qa-finding".to_string()));
    assert!(labelled.iter().all(|i| !i.title.starts_with("Nits")));
}

#[test]
fn an_angle_pr_that_carries_more_than_its_report_is_not_merged_and_told_once_per_head() {
    let w = World::new();
    w.tick();
    let pr = w.angle_pr(30, "security", &report_json("security", ""));
    let path = format!(".spl-asdlc/qa/{CAND}/security.json");
    {
        let mut s = w.github.state.borrow_mut();
        s.compares.insert(
            (repo(), BRANCH.into(), pr.head_sha.clone()),
            cmp(CompareStatus::Ahead, &["q1", "q2"], &[&path, "src/a.rs"]),
        );
        s.comments.insert((repo(), 30), Ok(Vec::new()));
    }
    w.open_on_branch(vec![pr.clone()]);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    w.tick();
    assert!(
        !w.github.writes().iter().any(|l| l.starts_with("merge")),
        "{:?}",
        w.github.writes()
    );
    let said: Vec<_> = w
        .github
        .writes()
        .into_iter()
        .filter(|l| {
            l.starts_with("comment example-org/acme#30 ") && l.contains("besides its report")
        })
        .collect();
    assert_eq!(said.len(), 1, "told once: {said:?}");
    assert!(said[0].contains("`src/a.rs`"), "{}", said[0]);
    assert_ne!(w.sub_item("security").state.status, Some(Status::Done));
}

#[test]
fn a_dry_run_of_the_release_pass_writes_nothing() {
    let w = World::new();
    w.owner_known();
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
        method_texts: Default::default(),
        low_quota: false,
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
            .any(|l| l.contains("create \"Release acme v1.2.0\"")
                && l.ends_with(", assigned to owner-handle")),
        "{:?}",
        intents.lines()
    );
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
    w.owner_known();
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
fn the_closure_moves_a_ticket_to_post_deploy_with_one_operator_validation_sub_task() {
    let w = closed_with_post_deploy();
    let ticket = w.tracker.item("ABC-5");
    assert_eq!(ticket.state.status, Some(Status::PostDeploy));
    assert!(labelled(&w, "ABC-5"), "the post-deploy pass finds it again");
    assert_eq!(ticket.assignee, None, "the ticket itself is never served");
    let tasks = validation_tasks(&w, "ABC-5");
    assert_eq!(tasks.len(), 1, "{:?}", w.tracker.writes());
    let task = &tasks[0];
    assert_eq!(task.state.status, Some(Status::PostDeploy));
    assert_eq!(
        task.assignee.as_ref().map(|u| u.name.as_str()),
        Some("owner-handle")
    );
    assert!(
        task.labels.is_empty(),
        "a validation is an action, not a PM decision: {:?}",
        task.labels
    );
    assert!(crate::method::contract::has_contract(&task.description));
    assert!(
        task.description
            .contains("operator responsible for this validation (`owner-handle`)")
    );
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
    // The operator's validation task remains available for them to close.
    assert_eq!(
        validation_tasks(&w, "ABC-5")[0].state.status,
        Some(Status::PostDeploy)
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
fn an_open_legacy_validation_is_moved_to_post_deploy_and_assigned_to_the_operator_once() {
    let w = World::new();
    w.owner_known();
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
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let task = w.tracker.item("ABC-10");
    assert_eq!(task.state.status, Some(Status::PostDeploy));
    assert_eq!(
        task.assignee.as_ref().map(|u| u.name.as_str()),
        Some("owner-handle")
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
    assert_eq!(validation_tasks(&w, "ABC-9").len(), 1);
}

/// A validation sub-task already closed is never reopened by the migration.
#[test]
fn a_closed_legacy_validation_is_left_alone() {
    let w = World::new();
    w.owner_known();
    w.tick();
    w.tracker.seed(
        "ABC-9",
        "Waiting",
        POST,
        Status::PostDeploy,
        &[POST_DEPLOY_LABEL],
    );
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
        Status::Done,
        &[],
    );
    w.tracker.state.borrow_mut().set_parent("ABC-10", "ABC-9");
    let before = w.tracker.writes().len();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let task = w.tracker.item("ABC-10");
    assert_eq!(task.state.status, Some(Status::Done));
    assert_eq!(task.assignee, None);
    assert!(
        w.tracker.writes()[before..]
            .iter()
            .all(|l| !l.contains("ABC-10")),
        "{:?}",
        &w.tracker.writes()[before..]
    );
    assert_eq!(validation_tasks(&w, "ABC-9").len(), 1);
}

#[test]
fn a_refused_validation_sub_task_is_created_at_a_later_tick_once() {
    let w = World::new();
    w.owner_known();
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
        method_texts: Default::default(),
        low_quota: false,
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
    rule_at_the_tag(&w, "BR-ACME-002");
    w.tick();
    publish(&w);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let ticket = w.tracker.item("ABC-5");
    assert_eq!(ticket.state.status, Some(Status::InProgress));
    assert_eq!(ticket.assignee, None);
    let said = w.tracker.comments("ABC-5").unwrap();
    assert!(
        said.iter()
            .any(|c| c.body.contains("`BR-ACME-001`")
                && c.body.contains("`.spl-asdlc/business-rules/`")),
        "{said:?}"
    );
    let before = w.tracker.writes().len();
    w.tick();
    assert_eq!(w.tracker.writes().len(), before, "a replay writes nothing");
}

/// The rule file `id` in the registry's directory at the release tag.
fn rule_at_the_tag(w: &World, id: &str) {
    w.github.state.borrow_mut().files.insert(
        (
            repo(),
            "acme-v1.2.0".into(),
            format!(".spl-asdlc/business-rules/{id}.md"),
        ),
        Ok(format!("# {id} — A rule.\n")),
    );
}

#[test]
fn the_closure_finds_a_cited_rule_by_its_file_at_the_release_tag() {
    let w = World::new();
    assigned_ticket(
        &w,
        "## Goal\nShip it.\n\n## Acceptance criteria\n- [x] shipped\n\n## Business rules\nBR-ACME-001 applies.\n\n## Release note\nYou can now ship.\n",
    );
    rule_at_the_tag(&w, "BR-ACME-001");
    w.tick();
    publish(&w);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.tracker.item("ABC-5").state.status, Some(Status::Done));
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
        method_texts: Default::default(),
        low_quota: false,
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
            "- [ ] The cause of each failing suite is fixed in a PR whose base is `{BRANCH}`: `tests`, `lint`."
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
fn a_gate_fix_ticket_is_assigned_to_the_release_fixer_when_one_is_known() {
    let mut w = World::new();
    w.config.release.fix_assignee = Some("fixer-handle".into());
    w.tracker
        .state
        .borrow_mut()
        .users
        .push(crate::tracker::User {
            id: "u-fixer".into(),
            name: "fixer-handle".into(),
        });
    w.red_settled(1, CAND, "c1", r#""tests""#);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let fixes = w.gate_fixes();
    assert_eq!(fixes.len(), 1);
    assert_eq!(
        fixes[0].assignee.as_ref().map(|u| u.name.as_str()),
        Some("fixer-handle")
    );
    // A fixer the tracker does not know: unassigned, taken by role.
    let mut w = World::new();
    w.config.release.fix_assignee = Some("no-such-fixer".into());
    w.red_settled(1, CAND, "c1", r#""tests""#);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.gate_fixes()[0].assignee, None);
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
    w.reruns(vec![rerun_run(91, "in_progress")]);
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert!(writes.is_empty(), "{writes:?}");
    // It reran the gate: the second attempt is pending.
    w.reruns(vec![rerun_run(91, "completed")]);
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
        rerun_run(93, "in_progress"),
        rerun_run(91, "completed"),
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
    w.reruns(vec![rerun_run(91, "in_progress")]);
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
    w.reruns(vec![rerun_run(91, "completed")]);
    let r = w.tick_at("2026-10-05T13:05:00Z");
    assert!(r.ok(), "{r:?}");
    let fixes = w.gate_fixes();
    assert_eq!(fixes.len(), 1);
    assert!(!fixes[0].description.contains("Automatic rerun:"));
}

/// A run of the automatic-rerun workflow created `at`.
fn rerun_run_at(id: u64, status: &str, at: &str) -> WorkflowRun {
    WorkflowRun {
        created_at: Timestamp::parse(at),
        updated_at: Timestamp::parse(at),
        ..rerun_run(id, status)
    }
}

#[test]
fn a_rerun_decided_on_the_default_branch_head_opens_the_ticket_at_the_next_tick() {
    let w = World::new();
    // The gate run completed red at 11:45 on the candidate's commit `c1`.
    w.gate(vec![red_completed_at_11_45()]);
    w.suites(r#"{"failed":["tests"]}"#);
    // Only a rerun run created before it completed: another gate run's.
    w.reruns(vec![rerun_run_at(80, "completed", "2026-10-05T11:20:00Z")]);
    let (r, writes) = w.writes_of_a_tick_at("2026-10-05T11:47:00Z");
    assert!(r.ok(), "{r:?}");
    assert!(w.gate_fixes().is_empty(), "{writes:?}");
    // Its own, on the default branch's head, is still deciding: it waits.
    w.reruns(vec![
        rerun_run_at(91, "in_progress", "2026-10-05T11:45:04Z"),
        rerun_run_at(80, "completed", "2026-10-05T11:20:00Z"),
    ]);
    let r = w.tick_at("2026-10-05T11:48:00Z");
    assert!(r.ok(), "{r:?}");
    assert!(w.gate_fixes().is_empty(), "the rerun is not decided yet");
    // It decided no rerun: the ticket at the next tick, long before the
    // 30-minute bound, and nothing says no rerun was seen.
    w.reruns(vec![
        rerun_run_at(91, "completed", "2026-10-05T11:45:04Z"),
        rerun_run_at(80, "completed", "2026-10-05T11:20:00Z"),
    ]);
    let r = w.tick_at("2026-10-05T11:49:00Z");
    assert!(r.ok(), "{r:?}");
    let fixes = w.gate_fixes();
    assert_eq!(fixes.len(), 1, "{:?}", w.tracker.writes());
    assert!(!fixes[0].description.contains("Automatic rerun:"));
    assert_ne!(DEFAULT_HEAD, "c1");
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
    w.reruns(vec![rerun_run(91, "completed")]);
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
            "- [ ] The cause of the suites that failed in the gate run {RUN1} (unknown here"
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
    w.reruns(vec![rerun_run(91, "completed")]);
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
        assert_eq!(
            w.tracker.comments(&fix.key).unwrap().len(),
            1,
            "{status}: its identity comment only"
        );
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
    w.reruns(vec![rerun_run(91, "completed"), rerun_run(92, "completed")]);
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
    assert_eq!(
        w.tracker.comments(&first.key).unwrap().len(),
        1,
        "its identity comment only"
    );

    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert!(writes.is_empty(), "never opened twice: {writes:?}");
}

/// The description an agent leaves when it ticks every box by rewriting it
/// without its first line, the marker (seen on 2026-10-18).
fn ticked_without_marker(description: &str) -> String {
    description
        .lines()
        .skip(1)
        .collect::<Vec<_>>()
        .join("\n")
        .replace("- [ ]", "- [x]")
}

#[test]
fn a_gate_fix_ticket_whose_description_lost_its_marker_is_found_by_its_comment_and_never_opened_twice()
 {
    let w = World::new();
    w.red_settled(1, CAND, "c1", r#""tests""#);
    w.tick();
    let fix = w.gate_fixes().remove(0);
    let marker = texts::gate_fix_marker(CAND);
    // Its identity comment, posted with it, where agents do not edit.
    let comments = w.tracker.comments(&fix.key).unwrap();
    assert_eq!(comments.len(), 1, "{comments:?}");
    assert!(comments[0].by_bridge && texts::has_marker(&comments[0].body, &marker));

    let ticked = ticked_without_marker(&fix.description);
    assert!(!texts::has_marker(&ticked, &marker));
    w.tracker.restyle(&fix.key, |_| ticked.clone());
    assert!(w.gate_fixes().is_empty(), "invisible by its description");

    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        writes,
        vec![
            format!("describe {}", fix.key),
            format!(
                "comment {} {}",
                fix.key,
                texts::marker_restored_comment().lines().next().unwrap()
            ),
        ],
        "found by its comment, its marker written back, nothing created"
    );
    let restored = w.tracker.item(&fix.key);
    assert!(texts::has_marker(&restored.description, &marker));
    assert!(
        restored.description.contains("- [x]"),
        "the agent's tick kept"
    );
    assert_eq!(w.gate_fixes().len(), 1);

    // Written back once: the next tick finds it by its description.
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert!(writes.is_empty(), "{writes:?}");
}

#[test]
fn several_gate_fix_tickets_found_by_their_comments_open_none_and_are_named() {
    let w = World::new();
    w.red_settled(1, CAND, "c1", r#""tests""#);
    w.tick();
    let fix = w.gate_fixes().remove(0);
    let release = w.release_item();
    let marker = texts::gate_fix_marker(CAND);
    w.tracker
        .seed("ABC-77", &fix.title, &fix.description, Status::Ready, &[]);
    w.tracker
        .state
        .borrow_mut()
        .set_parent("ABC-77", &release.key);
    w.tracker
        .comment("ABC-77", &texts::identity_comment(&marker))
        .unwrap();
    for key in [fix.key.as_str(), "ABC-77"] {
        w.tracker.restyle(key, ticked_without_marker);
    }

    let (r, writes) = w.writes_of_a_tick();
    assert!(
        !writes.iter().any(|w| w.starts_with("create ")),
        "{writes:?}"
    );
    assert!(
        r.failures
            .iter()
            .any(|f| f.contains("several gate-fix tickets")
                && f.contains(&fix.key)
                && f.contains("ABC-77")),
        "{r:?}"
    );
    // And so at every tick while both are there.
    let (r, writes) = w.writes_of_a_tick();
    assert!(
        !writes.iter().any(|w| w.starts_with("create ")),
        "{writes:?}"
    );
    assert!(
        r.failures
            .iter()
            .any(|f| f.contains("several gate-fix tickets")),
        "{r:?}"
    );
}

#[test]
fn a_gate_fix_ticket_whose_comments_cannot_be_read_opens_none() {
    let w = World::new();
    w.red_settled(1, CAND, "c1", r#""tests""#);
    w.tick();
    let fix = w.gate_fixes().remove(0);
    w.tracker.restyle(&fix.key, ticked_without_marker);
    w.tracker.make_unreadable(&fix.key);
    let (r, writes) = w.writes_of_a_tick();
    assert!(
        !writes.iter().any(|w| w.starts_with("create ")),
        "fail-closed: {writes:?}"
    );
    assert!(!r.ok(), "{r:?}");
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
        method_texts: Default::default(),
        low_quota: false,
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
    w.owner_known();
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
        method_texts: Default::default(),
        low_quota: false,
        ..w.tick_ctx()
    };
    let mut log = ActionLog::in_memory();
    let mut report = Report::default();
    post_deploy::pass(&t, &mut log, &mut report);
    assert!(report.ok(), "{report:?}");
    assert_eq!(
        intents.lines(),
        vec![
            "tracker: create \"Post-deploy validation — ABC-9: Waiting\" [Post Deploy] priority urgent under ABC-9, assigned to owner-handle"
                .to_string(),
            format!(
                "tracker: comment on (new: Post-deploy validation — ABC-9: Waiting): {}",
                texts::identity_comment("").lines().next().unwrap()
            ),
        ]
    );
}

// ---- archiving at the closure (BR-ASDLC-019) ----

/// A PR review round of `ticket` (an angle sub-item and its consolidation
/// item), seeded under it in `status`.
fn review_round(w: &World, ticket: &str, keys: [&str; 2], status: Status) {
    let (title, body) = texts::review_angle_item(
        ticket,
        PR5,
        "aaaaaaa1",
        "security",
        None,
        &[],
        &protocol::Texts::of_this_build(AT, protocol::Job::PrAngle, Some("security")),
    );
    w.tracker.seed(keys[0], &title, &body, status, &[]);
    let scope = texts::round_scope(PR5, "aaaaaaa1");
    let (title, body) = texts::consolidation_item(
        &scope,
        ticket,
        texts::Consolidating::PullRequest,
        &protocol::Texts::of_this_build(AT, texts::Consolidating::PullRequest.job(), None),
    );
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
    w.owner_known();
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
        method_texts: Default::default(),
        low_quota: false,
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

// ---- the bridge's own sub-items are not embarked tickets (spec 05 §11) ----

/// The review round of `ABC-5` on PR 5 (`ABC-21` the angle item, `ABC-22`
/// the consolidation item, whose marker an agent dropped), linked to the PR
/// beside its ticket, in `status`, with the release label an older bridge
/// gave them when `labelled`.
fn linked_round(w: &World, status: Status, labelled: bool) {
    review_round(w, "ABC-5", ["ABC-21", "ABC-22"], status);
    w.tracker.restyle("ABC-22", |d| {
        d.lines().skip(1).collect::<Vec<_>>().join("\n")
    });
    assert_eq!(
        texts::bridge_item(&w.tracker.item("ABC-22").description, Some("ABC-5")),
        None
    );
    for key in ["ABC-21", "ABC-22"] {
        w.tracker.link(key, PR5);
        if labelled {
            w.tracker.add_label(key, LABEL).unwrap();
        }
    }
}

#[test]
fn the_review_round_items_of_an_embarked_pr_are_neither_labelled_nor_briefed() {
    let w = World::new();
    linked_round(&w, Status::ToDeploy, false);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert!(w.tracker.item("ABC-5").labels.contains(&LABEL.to_string()));
    for key in ["ABC-21", "ABC-22"] {
        assert!(
            w.tracker.item(key).labels.is_empty(),
            "{key} is the bridge's, not an embarked ticket"
        );
    }
    let brief = &w.briefs(CAND)[0];
    assert!(brief.contains("ABC-5"), "{brief}");
    assert!(
        !brief.contains("ABC-21") && !brief.contains("ABC-22"),
        "{brief}"
    );
}

#[test]
fn the_closure_judges_the_ticket_and_never_reopens_its_labelled_round_items() {
    let w = World::new();
    // Linear moved the items linked to the merged PR to To Deploy; their
    // boxes are never ticked, angle items close by review.
    linked_round(&w, Status::ToDeploy, true);
    w.tick();
    publish(&w);
    let before = w.tracker.writes().len();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        w.tracker.item("ABC-5").state.status,
        Some(Status::Done),
        "the ticket is judged"
    );
    let writes = &w.tracker.writes()[before..];
    for key in ["ABC-21", "ABC-22"] {
        assert_eq!(
            w.tracker.item(key).state.status,
            Some(Status::ToDeploy),
            "{key} is not reopened"
        );
        assert!(
            !writes
                .iter()
                .any(|l| l.contains(&format!(" {key} ")) || l.ends_with(&format!(" {key}"))),
            "nothing written on {key}: {writes:?}"
        );
    }
    let release = w.release_item();
    let closure = w
        .tracker
        .comments(&release.key)
        .unwrap()
        .into_iter()
        .find(|c| c.body.contains("spl-asdlc:closure"))
        .expect("the closure comment");
    assert!(
        !closure.body.contains("ABC-21") && !closure.body.contains("ABC-22"),
        "{}",
        closure.body
    );
}

#[test]
fn labelled_round_items_resolved_are_archived_once_with_the_cycle() {
    let w = World::new();
    linked_round(&w, Status::Done, true);
    w.tick();
    publish(&w);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.tracker.item("ABC-5").state.status, Some(Status::Done));
    for key in ["ABC-21", "ABC-22", "ABC-5"] {
        assert!(w.tracker.item(key).archived, "{key} is archived");
    }
    let done = archives(&w);
    for key in ["ABC-21", "ABC-22"] {
        assert_eq!(
            done.iter()
                .filter(|l| **l == format!("archive {key}"))
                .count(),
            1,
            "{key} once: {done:?}"
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
        id: None,
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

    // The PR appears as a draft: out of the flow, nothing written.
    pr.draft = true;
    w.open_on_branch(vec![pr.clone()]);
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert!(writes.is_empty(), "{writes:?}");
    assert_eq!(w.sub_item("security").state.status, Some(Status::Ready));

    // Marked ready, its checks not reported yet: the angle's work started.
    pr.draft = false;
    let checks = std::mem::take(&mut pr.checks);
    w.open_on_branch(vec![pr.clone()]);
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(writes, vec![format!("status {sub} In Progress")]);
    w.quiet();

    // Its required checks green: In Review, for the coordinator.
    pr.checks = checks;
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
    w.report_only(&pr.head_sha, "security");
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

/// An agent edited the markers away: the items are found by their exact
/// titles among the release item's children and never created twice
/// (REQ-REL-048).
#[test]
fn a_round_item_whose_marker_was_mangled_is_found_by_its_title_and_not_recreated() {
    let w = World::new();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let round = w.round(CAND);
    assert_eq!(round.len(), 3, "two angles and the consolidation item");
    for item in &round {
        let mangled = item.description.replace("spl-asdlc:", "spl-asdlc :");
        w.tracker.update_description(&item.key, &mangled).unwrap();
    }
    assert!(w.round(CAND).is_empty(), "no marker is readable any more");
    let release = w.release_item().key;
    let children = w.tracker.children(&release).unwrap().len();
    let (_, writes) = w.writes_of_a_tick();
    assert!(
        !writes.iter().any(|l| l.starts_with("create")),
        "{writes:?}"
    );
    assert_eq!(w.tracker.children(&release).unwrap().len(), children);
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
fn a_newer_candidate_tagged_cancels_every_open_qa_item_of_the_older_once() {
    let w = World::new();
    w.tick();
    let older = w.consolidation(CAND).key;
    let angles = w.angle_keys(CAND);
    // One angle finished before the newer candidate: it stays Done.
    w.tracker
        .set_status(&angles[0], Status::Done, None)
        .unwrap();
    w.cut_rc2();
    // Tagged, its gate not green yet: it redoes every review all the same
    // (BR-ASDLC-038), so the older candidate's open QA items are canceled.
    w.gate(vec![
        gate_run(2, CAND2, "c2", None),
        gate_run(1, CAND, "c1", Some("success")),
    ]);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.tracker.item(&older).state.status, Some(Status::Canceled));
    assert_eq!(w.tracker.item(&angles[0]).state.status, Some(Status::Done));
    for angle in &angles[1..] {
        assert_eq!(w.tracker.item(angle).state.status, Some(Status::Canceled));
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

/// The milestones under the release item: `(title, its children's keys
/// sorted)`.
fn milestones(w: &World) -> Vec<(String, Vec<String>)> {
    let release = w.release_item();
    w.tracker
        .children(&release.key)
        .unwrap()
        .into_iter()
        .filter(|ch| texts::milestone_scope(&ch.description).is_some())
        .map(|m| {
            let mut keys: Vec<String> = w
                .tracker
                .children(&m.key)
                .unwrap()
                .into_iter()
                .map(|i| i.key)
                .collect();
            keys.sort();
            (m.title, keys)
        })
        .collect()
}

#[test]
fn on_a_tracker_with_milestones_each_candidate_gets_its_own_qa_milestone() {
    let mut w = World::new();
    w.tracker.milestones = true;
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    // One milestone named after the candidate, holding one item per angle
    // and the overall review; nothing of the round directly under the
    // release item.
    let mut first = w.angle_keys(CAND);
    let consolidation = w.consolidation(CAND);
    assert_eq!(w.blockers(&consolidation.key), first);
    assert_eq!(first.len(), config().release.angles.len());
    first.push(consolidation.key);
    first.sort();
    assert_eq!(milestones(&w), vec![(CAND.to_string(), first.clone())]);
    assert_eq!(w.briefs(CAND).len(), 1);
    w.quiet();

    // A newer candidate redoes every review in its own milestone; the older
    // one's open items are canceled.
    w.cut_rc2();
    w.gate(vec![
        gate_run(2, CAND2, "c2", Some("success")),
        gate_run(1, CAND, "c1", Some("success")),
    ]);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let ms = milestones(&w);
    assert_eq!(ms.len(), 2, "{ms:?}");
    assert_eq!(ms[0], (CAND.to_string(), first.clone()));
    assert_eq!(ms[1].0, CAND2);
    assert_eq!(ms[1].1.len(), first.len());
    for key in &first {
        assert_eq!(w.tracker.item(key).state.status, Some(Status::Canceled));
    }
    let newer = w.consolidation(CAND2);
    assert_eq!(newer.state.status, Some(Status::Ready));
    assert_eq!(w.blockers(&newer.key), w.angle_keys(CAND2));
    w.quiet();
}

#[test]
fn a_milestone_that_cannot_be_created_creates_no_qa_item_and_is_retried() {
    let mut w = World::new();
    w.tracker.milestones = true;
    w.tracker.fail_writes_starting_with("create milestone");
    let r = w.tick();
    assert!(!r.ok(), "{r:?}");
    assert!(w.round(CAND).is_empty());
    assert!(w.briefs(CAND).is_empty());
    w.tracker.state.borrow_mut().failing_writes.clear();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(milestones(&w).len(), 1);
    assert_eq!(w.briefs(CAND).len(), 1);
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
    // Ready, its checks not reported yet: its sub-item would start.
    pr.checks.clear();
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
        method_texts: Default::default(),
        low_quota: false,
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
        w.config.release.coordinator_assignee.as_deref(),
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
    w.config.release.coordinator_assignee = None;
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
    w.report_only(&pr.head_sha, "security");
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
    for a in &mut w.config.release.angles {
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
            w.config.release.coordinator_assignee = None;
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
        method_texts: Default::default(),
        low_quota: false,
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

// ---- SID-1016: a release with no QA angle; its GO is the gate ----

impl World {
    /// The cut of `World::new`, for a project with `release.angles = []`
    /// and no release coordinator.
    fn without_angles() -> World {
        let mut w = World::new();
        w.config.release.angles.clear();
        w.config.release.coordinators.clear();
        w
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
fn with_no_qa_angle_a_green_gate_is_a_go_and_the_release_is_published() {
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
    let (_, d) = crate::verbs::status(&w.tick_ctx()).unwrap();
    assert!(d.go, "{:?}", d.reasons);
    let r = w.tick();
    assert_eq!(w.go_writes(), go_lines("c1"), "{r:?}");
}

#[test]
fn with_no_qa_angle_a_red_gate_is_no_go() {
    let w = World::without_angles();
    w.red_settled(1, CAND, "c1", r#""tests""#);
    w.tick();
    let r = w.tick();
    assert!(w.go_writes().is_empty(), "{r:?}");
    let (_, d) = crate::verbs::status(&w.tick_ctx()).unwrap();
    assert!(!d.go);
    assert_eq!(d.reasons, vec!["the CI gate is not green"]);
    assert!(w.briefs(CAND).is_empty(), "no brief on a red gate");
    assert_eq!(w.gate_fixes().len(), 1, "the gate's own fix still opens");
}

#[test]
fn with_no_qa_angle_a_blocker_still_holds_the_release() {
    let w = World::without_angles();
    // The gate still running: no GO at the tick that opens the cycle.
    w.gate(vec![gate_run(1, CAND, "c1", None)]);
    w.tick();
    let release = w.release_item();
    w.tracker
        .seed("ABC-9", "Blocker", "## Goal\nFix.\n", Status::Ready, &[]);
    w.tracker
        .relate("ABC-9", &release.key, crate::tracker::RelationKind::Blocks)
        .unwrap();
    w.gate(vec![gate_run(1, CAND, "c1", Some("success"))]);
    w.tick();
    assert!(w.go_writes().is_empty());
    let (_, d) = crate::verbs::status(&w.tick_ctx()).unwrap();
    assert_eq!(d.reasons, vec!["open blocking issues: ABC-9"]);
}

#[test]
fn angles_configured_later_apply_from_the_next_candidate() {
    // Under the human policy the GO waits for the owner: rc.1, whose gate
    // is green and which owes no angle, stays in flight.
    let human = |mut c: Config| {
        c.release.go_policy = crate::config::GoPolicy::Human;
        c
    };
    let mut w = World::without_angles();
    w.config = human(w.config.clone());
    w.tick();
    assert!(w.round(CAND).is_empty());
    // The project configures its angles while rc.1 is in flight.
    w.config = human(config());
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

// ---- the candidate's own reports after it, and the lighter later pass ----

/// The example with `[release]` edited as `from` → `to`.
fn release_config(from: &str, to: &str) -> Config {
    let text = crate::config::spec_example::repository().replace(from, to);
    crate::config::spec_example::parse(&text)
}

impl World {
    /// The release branch moved past the candidate at `sha` by `files`.
    fn moved_past(&self, sha: &str, files: &[&str]) {
        self.github.state.borrow_mut().compares.insert(
            (repo(), sha.into(), BRANCH.into()),
            cmp(CompareStatus::Ahead, &["r1"], files),
        );
    }

    /// The `release status` decision's tip gaps.
    fn tip_gaps(&self) -> Vec<String> {
        crate::verbs::status(&self.tick_ctx()).unwrap().1.tip_gaps
    }
}

#[test]
fn the_last_passs_own_report_after_its_candidate_is_a_go() {
    // rc.1 is the last allowed pass: its angle reports land on the release
    // branch after it, and no candidate follows to carry them.
    let mut w = World::new();
    w.config = release_config("max_rc_passes = 2", "max_rc_passes = 1");
    w.tick();
    all_angles_merged(&w);
    w.moved_past(
        "c1",
        &[
            &format!(".spl-asdlc/qa/{CAND}/security.json"),
            &format!(".spl-asdlc/qa/{CAND}/product.json"),
        ],
    );
    assert!(w.tip_gaps().is_empty());
    let r = w.tick();
    assert_eq!(w.go_writes(), go_lines("c1"), "{r:?}");
}

#[test]
fn a_change_beside_the_candidates_report_is_still_a_tip_gap() {
    let mut w = World::new();
    w.config = release_config("max_rc_passes = 2", "max_rc_passes = 1");
    w.tick();
    all_angles_merged(&w);
    w.moved_past(
        "c1",
        &[&format!(".spl-asdlc/qa/{CAND}/security.json"), "src/a.rs"],
    );
    assert_eq!(
        w.tip_gaps(),
        vec![format!(
            "{BRANCH} moved past {CAND} (1 file(s)): tag the next candidate"
        )]
    );
    let r = w.tick();
    assert!(w.go_writes().is_empty(), "{r:?}");
}

#[test]
fn an_earlier_candidates_report_after_the_latest_is_a_tip_gap() {
    let w = World::new();
    w.tick();
    w.cut_rc2();
    w.gate(vec![
        gate_run(2, CAND2, "c2", Some("success")),
        gate_run(1, CAND, "c1", Some("success")),
    ]);
    w.tick();
    w.moved_past("c2", &[&format!(".spl-asdlc/qa/{CAND}/security.json")]);
    assert_eq!(
        w.tip_gaps(),
        vec![format!(
            "{BRANCH} moved past {CAND2} (1 file(s)): tag the next candidate"
        )]
    );
    w.moved_past("c2", &[&format!(".spl-asdlc/qa/{CAND2}/security.json")]);
    assert!(w.tip_gaps().is_empty());
}

#[test]
fn a_lighter_later_pass_counts_the_first_pass_and_is_a_go_on_a_green_gate() {
    let mut w = World::new();
    w.config = release_config(
        "later_passes = \"full\"",
        "later_passes = \"gate-and-coordinator\"",
    );
    w.tick();
    all_angles_merged(&w);
    w.cut_rc2();
    w.gate(vec![
        gate_run(2, CAND2, "c2", Some("success")),
        gate_run(1, CAND, "c1", Some("success")),
    ]);
    let (_, d) = crate::verbs::status(&w.tick_ctx()).unwrap();
    assert!(d.go, "{:?}", d.reasons);
    let r = w.tick();
    assert!(
        w.round(CAND2).is_empty(),
        "the lighter pass offers no angle"
    );
    assert_eq!(w.go_writes(), go_lines("c2"), "{r:?}");
}

#[test]
fn a_lighter_later_pass_waits_for_an_angle_the_first_pass_did_not_merge() {
    let mut w = World::new();
    w.config = release_config(
        "later_passes = \"full\"",
        "later_passes = \"gate-and-coordinator\"",
    );
    w.tick();
    let mut security = w.angle_pr(30, "security", &report_json("security", ""));
    security.state = PrState::Merged;
    security.merge_commit_sha = Some("merge-of-30".into());
    w.github
        .state
        .borrow_mut()
        .merged_pulls
        .insert(repo(), Ok(vec![security]));
    w.cut_rc2();
    w.gate(vec![
        gate_run(2, CAND2, "c2", Some("success")),
        gate_run(1, CAND, "c1", Some("success")),
    ]);
    let (_, d) = crate::verbs::status(&w.tick_ctx()).unwrap();
    assert_eq!(d.reasons, vec!["angles missing: product"]);
    let r = w.tick();
    assert!(w.go_writes().is_empty(), "{r:?}");
}

// ---- the sandbox run of 2026-10-15 ----

#[test]
fn release_status_after_the_cycle_closed_says_no_cycle_is_in_flight() {
    let w = World::new();
    w.tick();
    assert!(matches!(
        crate::verbs::release_status(&w.tick_ctx()).unwrap(),
        crate::verbs::CycleStatus::InFlight(..)
    ));
    publish(&w);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let release = w.tracker.item(&w.release_item().key);
    assert_eq!(release.state.status, Some(Status::Done), "the cycle closed");
    match crate::verbs::release_status(&w.tick_ctx()).unwrap() {
        crate::verbs::CycleStatus::Over { release_tag, item } => {
            assert_eq!(release_tag, "acme-v1.2.0");
            assert_eq!(item, release.key);
        }
        crate::verbs::CycleStatus::InFlight(..) => panic!("the cycle is over"),
    }
}

/// PRs of the range besides the ticket's: one with no ticket and its own
/// `## Release note`, one with no ticket and none, and the release's own
/// bookkeeping (a merge back, an angle report PR, a `chore(release)` PR).
fn more_prs_in_the_range(w: &World) {
    let mut s = w.github.state.borrow_mut();
    s.compares.insert(
        (repo(), "acme-v1.1.0".into(), "c1".into()),
        cmp(
            CompareStatus::Ahead,
            &["m1", "m6", "m7", "m8", "m9", "m10"],
            &["src/a.rs"],
        ),
    );
    for (n, head, title, body) in [
        (
            6,
            "fix/six",
            "fix: six",
            "## Summary\nNot this.\n\n## Release note\nSix is fixed.\n",
        ),
        (7, "docs/seven", "docs: seven", "## Summary\nA doc.\n"),
        (8, "release/2026-09-01-10-00-utc", "chore: merge back", ""),
        (9, "qa/acme-v1.1.0-rc.1/security", "qa: security report", ""),
        (10, "chore/prepare", "chore(release): 1.2.0", ""),
    ] {
        s.commit_pulls.insert(
            (repo(), format!("m{n}")),
            Ok(vec![CommitPull {
                number: n,
                url: format!("https://github.com/example-org/acme/pull/{n}"),
                title: title.into(),
                body: body.into(),
                merge_commit_sha: Some(format!("m{n}")),
                base_ref: "main".into(),
                head_ref: head.into(),
                ..CommitPull::default()
            }]),
        );
    }
}

#[test]
fn the_note_and_the_brief_announce_every_pr_of_the_range_but_the_bookkeeping() {
    let w = World::new();
    more_prs_in_the_range(&w);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let release = w.release_item();
    let brief = w
        .tracker
        .comments(&release.key)
        .unwrap()
        .into_iter()
        .find(|c| c.body.contains("### Release brief"))
        .expect("the brief")
        .body;
    assert!(brief.contains("#### No linked ticket"), "{brief}");
    assert!(brief.contains("- **PR #6** — fix: six\n"), "{brief}");
    assert!(brief.contains("- **PR #7** — docs: seven\n"), "{brief}");
    let d = &release.description;
    assert!(
        d.contains("- **ABC-5** — Shipped: You can now ship. (#5)"),
        "{d}"
    );
    assert!(d.contains("### No linked ticket"), "{d}");
    assert!(d.contains("- **PR #6** — fix: six: Six is fixed."), "{d}");
    assert!(d.contains("- **PR #7** — docs: seven\n"), "{d}");
    for (text, n) in [
        (&brief, 8),
        (&brief, 9),
        (&brief, 10),
        (d, 8),
        (d, 9),
        (d, 10),
    ] {
        assert!(
            !text.contains(&format!("PR #{n}")),
            "bookkeeping #{n}: {text}"
        );
    }
    // The note sent at the closure announces them too.
    publish(&w);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let sent = w.signal.sent.borrow().join("\n");
    assert!(
        sent.contains("- **PR #6** — fix: six: Six is fixed."),
        "{sent}"
    );
    assert!(sent.contains("- **PR #7** — docs: seven"), "{sent}");
}

/// The release item's description with its note taken out again, the
/// fresh item's placeholder in its place.
fn placeholder_back(w: &World, key: &str) {
    use crate::method::release::note::{END, PLACEHOLDER, START};
    // The markers as this tracker stores them (made visible when it drops
    // HTML comments).
    let (start, end) = if w.tracker.keeps_html_comments() {
        (START.to_string(), END.to_string())
    } else {
        (texts::visible_markers(START), texts::visible_markers(END))
    };
    w.tracker.restyle(key, |d| {
        let (s, e) = (d.find(&start).unwrap(), d.find(&end).unwrap() + end.len());
        format!("{}_{PLACEHOLDER}_{}", &d[..s], &d[e..])
    });
}

#[test]
fn the_release_item_carries_its_note_before_the_go_publishes_the_release() {
    use crate::method::release::note::PLACEHOLDER;
    // The brief writes it: a GO a person makes in GitHub finds it there.
    let w = World::new();
    w.tick();
    let release = w.release_item();
    assert!(
        release
            .description
            .contains("- **ABC-5** — Shipped: You can now ship. (#5)"),
        "{}",
        release.description
    );
    assert!(!release.description.contains(PLACEHOLDER));
    // Taken out again, the GO writes it back before it tags and publishes.
    placeholder_back(&w, &release.key);
    all_angles_merged(&w);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.go_writes(), go_lines("c1"), "{r:?}");
    let d = w.tracker.item(&release.key).description;
    assert!(!d.contains(PLACEHOLDER), "{d}");
    assert!(
        d.contains("- **ABC-5** — Shipped: You can now ship. (#5)"),
        "{d}"
    );
    assert!(
        d.contains(&format!("`{CAND}`")),
        "the candidate line stays: {d}"
    );
}

#[test]
fn a_release_note_that_cannot_be_written_holds_the_go() {
    let w = World::new();
    w.tick();
    let release = w.release_item();
    placeholder_back(&w, &release.key);
    w.tracker
        .fail_writes_starting_with(&format!("describe {}", release.key));
    all_angles_merged(&w);
    let r = w.tick();
    assert!(
        r.failures
            .iter()
            .any(|f| f.contains("describe release note")),
        "{r:?}"
    );
    assert!(
        w.go_writes().is_empty(),
        "never published with the placeholder"
    );
}

#[test]
fn a_go_tick_and_a_closure_that_find_the_same_note_write_nothing() {
    use crate::method::release::note::END;
    let w = World::new();
    w.tick();
    let release = w.release_item();
    let describe = format!("describe {}", release.key);
    assert!(
        w.tracker.writes().contains(&describe),
        "the brief writes it"
    );
    // Linear's normalised form: `*` bullets with its own spacing, blank
    // lines, and the end marker indented as an older issue has it.
    let end = if w.tracker.keeps_html_comments() {
        END.to_string()
    } else {
        texts::visible_markers(END)
    };
    w.tracker.restyle(&release.key, |d| {
        d.replace("\n- **", "\n\n*   **")
            .replace(&format!("\n\n{end}"), &format!("\n  {end}"))
    });
    let restyled = w.tracker.item(&release.key).description;
    assert!(restyled.contains(&format!("\n  {end}")), "{restyled}");
    assert!(restyled.contains("*   **ABC-5**"), "{restyled}");
    let note_writes = |w: &World, from: usize| -> Vec<String> {
        w.tracker.writes()[from..]
            .iter()
            .filter(|l| **l == describe)
            .cloned()
            .collect()
    };
    all_angles_merged(&w);
    let before = w.tracker.writes().len();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.go_writes(), go_lines("c1"), "{r:?}");
    assert!(note_writes(&w, before).is_empty(), "the GO tick");
    publish(&w);
    let before = w.tracker.writes().len();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        w.tracker.item(&release.key).state.status,
        Some(Status::Done),
        "the cycle closes"
    );
    assert!(note_writes(&w, before).is_empty(), "the closure");
    assert_eq!(w.tracker.item(&release.key).description, restyled);
}

#[test]
fn the_release_item_is_in_progress_from_its_creation_to_its_closure() {
    let w = World::new();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let release = w.release_item();
    assert_eq!(release.state.status, Some(Status::InProgress));
    assert!(
        w.tracker
            .writes()
            .iter()
            .any(|l| l.starts_with("create Release acme v1.2.0 [In Progress]")),
        "{:?}",
        w.tracker.writes()
    );
    publish(&w);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        w.tracker.item(&release.key).state.status,
        Some(Status::Done)
    );
}

#[test]
fn a_release_item_found_ready_or_in_backlog_is_moved_in_progress_once() {
    for from in [Status::Ready, Status::Backlog] {
        let w = World::new();
        w.tick();
        let release = w.release_item();
        // As an earlier bridge created it, or a person put it back.
        w.tracker.set_status(&release.key, from, None).unwrap();
        let before = w.tracker.writes().len();
        let r = w.tick();
        assert!(r.ok(), "{r:?}");
        let writes = w.tracker.writes()[before..].to_vec();
        assert_eq!(writes.len(), 1, "{from:?}: {writes:?}");
        assert!(writes[0].contains(&release.key), "{writes:?}");
        let moved = w.tracker.item(&release.key);
        assert_eq!(moved.state.status, Some(Status::InProgress), "{from:?}");
        assert_eq!(moved.assignee, release.assignee, "its assignee kept");
        let before = w.tracker.writes().len();
        let r = w.tick();
        assert!(r.ok(), "{r:?}");
        assert_eq!(
            w.tracker.writes().len(),
            before,
            "once: {:?}",
            &w.tracker.writes()[before..]
        );
    }
}

// ---- SID-1178: a release whose publication failed before deploying is retracted ----

const RELEASE_TAG: &str = "acme-v1.2.0";
const PUBLISH_WF: &str = "release-publish.yml";

/// The run of the publication workflow on the release `RELEASE_TAG` at `sha`.
fn publication_run(id: u64, sha: &str, conclusion: Option<&str>) -> WorkflowRun {
    WorkflowRun {
        event: "release".into(),
        head_branch: RELEASE_TAG.into(),
        created_at: Timestamp::parse("2026-10-05T12:01:00Z"),
        ..gate_run(id, RELEASE_TAG, sha, conclusion)
    }
}

/// The jobs of a publication run, as GitHub names them: the two calls of
/// `asdlc-release-publish.yml` around the project's `publish` job.
fn publication_jobs(verify: &str, publish: &str) -> Vec<crate::github::Job> {
    let close = if publish == "success" {
        "success"
    } else {
        "skipped"
    };
    [
        ("verify / verify", verify),
        ("verify / close", "skipped"),
        ("verify / closing-guard", "success"),
        ("publish", publish),
        ("close / verify", "skipped"),
        ("close / close", close),
        ("close / closing-guard", "failure"),
    ]
    .into_iter()
    .map(|(name, c)| crate::github::Job {
        name: name.into(),
        conclusion: Some(c.into()),
    })
    .collect()
}

impl World {
    /// rc.1 released on its GO (no QA angle), then its publication run as
    /// given, with its jobs.
    fn published_then(
        run: WorkflowRun,
        jobs: crate::github::Read<Vec<crate::github::Job>>,
    ) -> World {
        let w = World::without_angles();
        let r = w.tick();
        assert_eq!(w.go_writes(), go_lines("c1"), "{r:?}");
        let mut s = w.github.state.borrow_mut();
        s.jobs.insert((repo(), run.id), jobs);
        s.runs.insert((repo(), PUBLISH_WF.into()), Ok(vec![run]));
        drop(s);
        w
    }

    fn retraction_comments(&self) -> Vec<String> {
        let release = self.release_item();
        self.tracker
            .comments(&release.key)
            .unwrap()
            .into_iter()
            .map(|c| c.body)
            .filter(|b| b.contains("spl-asdlc:retracted"))
            .collect()
    }

    fn retract_writes(&self) -> Vec<String> {
        self.github
            .writes()
            .into_iter()
            .filter(|l| l.starts_with("delete-release") || l.starts_with("delete-tag"))
            .collect()
    }

    fn release_tag_exists(&self) -> bool {
        self.github.state.borrow().tags[&repo()]
            .iter()
            .any(|t| t.name == RELEASE_TAG)
    }

    /// rc.2 cut on the fix, its gate green.
    fn rc2_green(&self) {
        self.cut_rc2();
        self.gate(vec![
            gate_run(2, CAND2, "c2", Some("success")),
            gate_run(1, CAND, "c1", Some("success")),
        ]);
    }
}

#[test]
fn a_release_whose_verification_refused_is_retracted_once_and_awaits_its_go() {
    let w = World::published_then(
        publication_run(50, "c1", Some("failure")),
        Ok(publication_jobs("failure", "skipped")),
    );
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let writes = w.retract_writes();
    assert_eq!(writes.len(), 2, "{writes:?}");
    assert!(writes[0].starts_with("delete-release example-org/acme "));
    assert_eq!(
        writes[1],
        format!("delete-tag example-org/acme {RELEASE_TAG}")
    );
    assert!(w.github.release(&repo(), RELEASE_TAG).is_err(), "deleted");
    assert!(!w.release_tag_exists(), "the tag is deleted");
    let said = w.retraction_comments();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].contains("actions/runs/50")
            && said[0].contains("`verify / verify` concluded `failure`")
            && said[0].contains(&format!("the next candidate takes `{RELEASE_TAG}`")),
        "{}",
        said[0]
    );
    // Idempotent: nothing left to retract, and no second GO on rc.1.
    let (gh, tr) = (w.github.writes().len(), w.tracker.writes().len());
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        (w.github.writes().len(), w.tracker.writes().len()),
        (gh, tr),
        "{:?}",
        &w.tracker.writes()[tr..]
    );
    let (_, d) = crate::verbs::status(&w.tick_ctx()).unwrap();
    assert!(!d.go);
    assert_eq!(d.retracted.as_deref(), Some(RELEASE_TAG));
    assert!(
        d.reasons.iter().any(|r| r.contains("was retracted")),
        "{:?}",
        d.reasons
    );
}

#[test]
fn after_a_retraction_the_next_green_candidate_gets_the_go_on_the_same_version() {
    let w = World::published_then(
        publication_run(50, "c1", Some("failure")),
        Ok(publication_jobs("failure", "skipped")),
    );
    w.tick();
    assert_eq!(w.retract_writes().len(), 2);
    w.rc2_green();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let go = w.go_writes();
    assert_eq!(go[2..], go_lines("c2"), "{go:?}");
    let release = w.github.release(&repo(), RELEASE_TAG).unwrap();
    assert_eq!(release.body, NOTES);
    assert_eq!(w.go_comments(), 2);
}

#[test]
fn a_tag_left_by_a_failed_retraction_is_deleted_at_the_next_candidates_go() {
    let w = World::published_then(
        publication_run(50, "c1", Some("failure")),
        Ok(publication_jobs("failure", "skipped")),
    );
    w.github
        .fail_writes_starting_with("delete-tag", Some(422), "refused");
    let r = w.tick();
    assert!(!r.ok(), "the failed deletion is said");
    assert!(w.github.release(&repo(), RELEASE_TAG).is_err());
    assert!(w.release_tag_exists());
    w.github.state.borrow_mut().failing_writes.clear();
    // rc.1 gets no second GO on its leftover tag.
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.go_writes(), go_lines("c1"));
    let leftover = w.github.state.borrow().tags[&repo()]
        .iter()
        .find(|t| t.name == RELEASE_TAG)
        .cloned()
        .unwrap();
    w.rc2_green();
    w.github
        .state
        .borrow_mut()
        .tags
        .get_mut(&repo())
        .unwrap()
        .push(leftover);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let writes = w.github.writes();
    let tail = &writes[writes.len() - 3..];
    assert_eq!(
        tail[0],
        format!("delete-tag example-org/acme {RELEASE_TAG}")
    );
    assert_eq!(tail[1..], go_lines("c2"));
    assert_eq!(w.retraction_comments().len(), 1, "said once");
}

#[test]
fn a_release_whose_publication_deployed_runs_or_was_taken_over_is_never_retracted() {
    let dispatched = WorkflowRun {
        event: "workflow_dispatch".into(),
        head_branch: BRANCH.into(),
        created_at: Timestamp::parse("2026-10-05T12:30:00Z"),
        ..gate_run(51, "", "c1", Some("success"))
    };
    let cases: Vec<(&str, Vec<WorkflowRun>, Vec<crate::github::Job>)> = vec![
        (
            "delivered",
            vec![publication_run(50, "c1", Some("success"))],
            publication_jobs("success", "success"),
        ),
        (
            "the deploy failed",
            vec![publication_run(50, "c1", Some("failure"))],
            publication_jobs("success", "failure"),
        ),
        (
            "still running",
            vec![publication_run(50, "c1", None)],
            publication_jobs("failure", "skipped"),
        ),
        (
            "a resume dispatched",
            vec![dispatched, publication_run(50, "c1", Some("failure"))],
            publication_jobs("failure", "skipped"),
        ),
        (
            "another release's run",
            vec![publication_run(50, "c0", Some("failure"))],
            publication_jobs("failure", "skipped"),
        ),
    ];
    for (case, runs, jobs) in cases {
        let w = World::published_then(publication_run(50, "c1", None), Ok(jobs));
        w.github
            .state
            .borrow_mut()
            .runs
            .insert((repo(), PUBLISH_WF.into()), Ok(runs));
        let r = w.tick();
        assert!(r.ok(), "{case}: {r:?}");
        assert!(w.retract_writes().is_empty(), "{case}");
        assert!(w.retraction_comments().is_empty(), "{case}");
        assert!(w.github.release(&repo(), RELEASE_TAG).is_ok(), "{case}");
        let refused = crate::verbs::retract(
            &w.tick_ctx(),
            &mut ActionLog::in_memory(),
            RELEASE_TAG,
            "a person",
        );
        assert!(refused.is_err(), "{case}: {refused:?}");
        assert!(w.retract_writes().is_empty(), "{case}");
    }
}

#[test]
fn jobs_that_cannot_be_read_retract_nothing_and_are_said() {
    let w = World::published_then(
        publication_run(50, "c1", Some("failure")),
        Err(crate::github::ReadError::unreadable(
            "jobs of run 50",
            "boom",
        )),
    );
    let r = w.tick();
    assert!(
        r.failures
            .iter()
            .any(|f| f.contains("the publication run of acme-v1.2.0")),
        "{r:?}"
    );
    assert!(w.retract_writes().is_empty());
    assert!(w.retraction_comments().is_empty());
    let refused = crate::verbs::retract(
        &w.tick_ctx(),
        &mut ActionLog::in_memory(),
        RELEASE_TAG,
        "a person",
    )
    .unwrap_err();
    assert!(refused.contains("boom"), "{refused}");
    assert!(w.retract_writes().is_empty());
}

#[test]
fn release_retract_retracts_by_hand_once_and_refuses_what_is_not_a_release() {
    let w = World::published_then(
        publication_run(50, "c1", Some("failure")),
        Ok(publication_jobs("cancelled", "skipped")),
    );
    let t = w.tick_ctx();
    let mut log = ActionLog::in_memory();
    let refused = crate::verbs::retract(&t, &mut log, CAND, "a person").unwrap_err();
    assert!(refused.contains("is not a release tag"), "{refused}");
    let line = crate::verbs::retract(&t, &mut log, RELEASE_TAG, "a person").unwrap();
    assert!(line.contains("retracted"), "{line}");
    assert_eq!(w.retract_writes().len(), 2);
    let said = w.retraction_comments();
    assert_eq!(said.len(), 1);
    assert!(
        said[0].contains("a person, with `spl-asdlc release retract`, retracts it"),
        "{}",
        said[0]
    );
    let again = crate::verbs::retract(&t, &mut log, RELEASE_TAG, "a person").unwrap();
    assert!(again.contains("nothing to retract"), "{again}");
    assert_eq!(w.retract_writes().len(), 2);
    // The tick finds nothing left to retract either.
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.retract_writes().len(), 2);
    assert_eq!(w.go_writes(), go_lines("c1"), "no second GO on rc.1");
}

#[test]
fn a_dry_run_says_it_would_retract_and_writes_nothing() {
    let w = World::published_then(
        publication_run(50, "c1", Some("failure")),
        Ok(publication_jobs("failure", "skipped")),
    );
    let (gh, tr) = (w.github.writes().len(), w.tracker.writes().len());
    let intents = crate::dry::Intents::default();
    let github = crate::dry::DryGitHub {
        inner: &w.github,
        intents: &intents,
    };
    let tracker = crate::dry::DryTracker {
        inner: &w.tracker,
        intents: &intents,
    };
    let signal = crate::dry::DrySignal { intents: &intents };
    let t = Tick {
        github: &github,
        tracker: &tracker,
        signal: &signal,
        dry_run: true,
        method_texts: Default::default(),
        low_quota: false,
        ..w.tick_ctx()
    };
    let r = run(&t, &mut ActionLog::in_memory());
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        (w.github.writes().len(), w.tracker.writes().len()),
        (gh, tr)
    );
    let lines = intents.lines();
    assert!(
        lines
            .iter()
            .any(|l| l.contains("delete release") && l.ends_with("(would retract)")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.ends_with(&format!("delete tag {RELEASE_TAG} (would retract)"))),
        "{lines:?}"
    );
}

// ---- the pause switch and the hold label (a person's gestures) ----

impl World {
    /// A person sets the repository variable `SPL_ASDLC_PAUSED` in GitHub.
    fn pause(&self, value: &str, at: &str) {
        self.github.state.borrow_mut().variables.insert(
            (repo(), "SPL_ASDLC_PAUSED".into()),
            Ok(crate::github::Variable {
                name: "SPL_ASDLC_PAUSED".into(),
                value: value.into(),
                updated_at: Some(at.into()),
            }),
        );
    }

    fn pause_notices(&self) -> usize {
        let item = self.release_item();
        self.tracker
            .comments(&item.key)
            .unwrap()
            .iter()
            .filter(|c| c.body.contains("The bridge is paused"))
            .count()
    }
}

#[test]
fn a_paused_tick_writes_nothing_and_an_unpaused_one_acts_again() {
    let w = World::new();
    w.pause("true", "2026-10-05T11:45:00Z");
    // No cycle in flight yet: nothing at all is written.
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert!(r.paused);
    assert_eq!(r.summary(), "paused by SPL_ASDLC_PAUSED: 0 action(s)");
    assert!(w.github.writes().is_empty(), "{:?}", w.github.writes());
    assert!(w.tracker.writes().is_empty(), "{:?}", w.tracker.writes());

    // Unset: the bridge acts again and opens the cycle.
    w.github.state.borrow_mut().variables.clear();
    let r = w.tick();
    assert!(r.ok() && !r.paused, "{r:?}");
    assert!(r.actions > 0);

    // Paused with a cycle in flight and its GO due: the release item is
    // told once, nothing else is written, the release is not tagged.
    all_angles_merged(&w);
    w.pause("true", "2026-10-05T11:50:00Z");
    let (github_before, tracker_before) = (w.github.writes(), w.tracker.writes().len());
    let r = w.tick();
    assert!(r.ok() && r.paused, "{r:?}");
    assert_eq!(r.actions, 1, "{r:?}");
    assert_eq!(w.pause_notices(), 1);
    assert_eq!(w.tracker.writes().len(), tracker_before + 1);
    assert_eq!(w.github.writes(), github_before, "nothing on GitHub");
    let r = w.tick();
    assert_eq!(r.actions, 0, "{r:?}");
    assert_eq!(w.pause_notices(), 1, "said once per setting");

    // `false` is no pause: the GO is computed, the release tagged.
    w.pause("false", "2026-10-05T11:55:00Z");
    let r = w.tick();
    assert!(r.ok() && !r.paused, "{r:?}");
    assert_eq!(w.go_writes(), go_lines("c1"), "{r:?}");
}

#[test]
fn a_pause_switch_that_cannot_be_read_pauses_nothing_and_is_said() {
    let w = World::new();
    w.github.state.borrow_mut().variables.insert(
        (repo(), "SPL_ASDLC_PAUSED".into()),
        Err(crate::github::ReadError::Unreadable {
            what: "variable SPL_ASDLC_PAUSED of example-org/acme".into(),
            status: Some(403),
            detail: "HTTP 403: Resource not accessible by integration".into(),
        }),
    );
    let r = w.tick();
    assert!(r.ok() && !r.paused, "{r:?}");
    assert!(r.actions > 0, "the bridge acts as before: {r:?}");
    assert!(
        r.summary()
            .contains("could not be read, so the bridge acts as not paused"),
        "{}",
        r.summary()
    );
}

#[test]
fn a_held_go_tags_nothing_until_the_label_goes_and_release_status_says_so() {
    let w = World::new();
    w.tick();
    let item = w.release_item();
    // A person holds the GO with the hold label on the release item.
    w.tracker.add_label(&item.key, "hold").unwrap();
    // The angle PR is still merged under the hold.
    let pr = w.angle_pr(30, "security", &report_json("security", ""));
    w.open_on_branch(vec![pr.clone()]);
    w.tick();
    assert!(
        w.github.writes().contains(&format!(
            "merge example-org/acme#30 at {} merge",
            pr.head_sha
        )),
        "{:?}",
        w.github.writes()
    );
    all_angles_merged(&w);
    w.open_on_branch(Vec::new());
    for _ in 0..2 {
        let r = w.tick();
        assert!(r.ok(), "{r:?}");
        assert!(w.go_writes().is_empty(), "held: {:?}", w.go_writes());
    }
    let (_, d) = crate::verbs::status(&w.tick_ctx()).unwrap();
    assert!(!d.go);
    assert_eq!(d.held.as_deref(), Some("hold"));
    assert_eq!(
        d.reasons,
        vec![format!(
            "the GO is held by the label `hold` on {}: removing it lets the GO be computed again",
            item.key
        )]
    );
    // The label removed: the GO is computed again.
    w.tracker.remove_label(&item.key, "hold").unwrap();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(w.go_writes(), go_lines("c1"), "{r:?}");
}

// ---- a published and closed release item is left alone ----

/// The release item as Linear stores it: `*` bullets spaced by blank lines.
fn linear_restyle(w: &World, key: &str) {
    w.tracker.restyle(key, |d| d.replace("\n- ", "\n\n* "));
}

#[test]
fn several_ticks_after_the_closure_write_nothing_to_the_release_item() {
    let w = World::new();
    w.tick();
    let release = w.release_item();
    publish(&w);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(
        w.tracker.item(&release.key).state.status,
        Some(Status::Done)
    );
    linear_restyle(&w, &release.key);
    let stored = w.tracker.item(&release.key).description;
    let before = (w.tracker.writes().len(), w.github.writes().len());
    for _ in 0..5 {
        let r = w.tick();
        assert!(r.ok(), "{r:?}");
    }
    assert_eq!(
        (w.tracker.writes().len(), w.github.writes().len()),
        before,
        "{:?}",
        &w.tracker.writes()[before.0..]
    );
    let item = w.tracker.item(&release.key);
    assert_eq!(item.state.status, Some(Status::Done));
    assert_eq!(item.description, stored);
    assert_eq!(w.signal.sent.borrow().len(), 1, "the note is sent once");
}

/// How many lines of the `## Candidates` section name `candidate`.
fn candidate_lines(description: &str, candidate: &str) -> usize {
    description
        .split("## Candidates")
        .nth(1)
        .unwrap_or_default()
        .split("\n## ")
        .next()
        .unwrap_or_default()
        .lines()
        .filter(|l| l.contains(&format!("`{candidate}` —")))
        .count()
}

#[test]
fn repeated_ticks_over_two_candidates_list_each_candidate_once() {
    let w = World::new();
    w.tick();
    let key = w.release_item().key;
    for _ in 0..3 {
        linear_restyle(&w, &key);
        let r = w.tick();
        assert!(r.ok(), "{r:?}");
    }
    w.cut_rc2();
    for _ in 0..3 {
        let r = w.tick();
        assert!(r.ok(), "{r:?}");
        linear_restyle(&w, &key);
    }
    let d = w.tracker.item(&key).description;
    assert_eq!(candidate_lines(&d, CAND), 1, "{d}");
    assert_eq!(candidate_lines(&d, CAND2), 1, "{d}");
}

#[test]
fn the_note_names_a_readable_ticket_with_no_release_note_section_by_its_goal() {
    let w = World::new();
    w.tracker.seed(
        "ABC-5",
        "Shipped",
        "## Goal\nShip it.\n\n## Acceptance criteria\n- [x] shipped\n\n## Business rules\nNo new rule.\n",
        Status::ToDeploy,
        &[],
    );
    w.tracker.link("ABC-5", PR5);
    // The PR's review round, labelled with the release by an older bridge.
    linked_round(&w, Status::Done, true);
    w.tick();
    let release = w.release_item();
    publish(&w);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let d = w.tracker.item(&release.key).description;
    assert!(d.contains("- **ABC-5** — Shipped: Ship it. (#5)"), "{d}");
    assert!(!d.contains("ticket unreadable"), "{d}");
    assert!(!d.contains("ABC-21") && !d.contains("ABC-22"), "{d}");
    let sent = w.signal.sent.borrow().join("\n");
    assert!(!sent.contains("ticket unreadable"), "{sent}");
}

const NEXT: &str = "acme-v1.3.0-rc.1";
const NEXT_BRANCH: &str = "release/2026-10-06-09-00-utc";
const NEXT_LABEL: &str = "release/acme-v1.3.0";

impl World {
    /// The next cycle, cut from `main` once `acme-v1.2.0` was merged back:
    /// its branch, which holds the older candidate too, and its first
    /// candidate.
    fn cut_next_cycle(&self) {
        let mut s = self.github.state.borrow_mut();
        s.tags.insert(
            repo(),
            vec![
                tag("acme-v1.1.0", "r0"),
                tag(CAND, "c1"),
                tag("acme-v1.2.0", "c1"),
                tag(NEXT, "d1"),
            ],
        );
        s.branches.insert(repo(), vec![tag(NEXT_BRANCH, "d1")]);
        s.commits.insert(
            (repo(), "d1".into()),
            Ok(Commit {
                sha: "d1".into(),
                message: "chore: candidate".into(),
                committer_date: Timestamp::parse("2026-10-06T09:30:00Z"),
                parents: vec![],
            }),
        );
        for (base, status) in [
            ("c1", CompareStatus::Ahead),
            ("d1", CompareStatus::Identical),
        ] {
            s.compares.insert(
                (repo(), base.into(), NEXT_BRANCH.into()),
                cmp(status, &[], &[]),
            );
        }
        s.compares.insert(
            (repo(), "acme-v1.2.0".into(), "d1".into()),
            cmp(CompareStatus::Ahead, &[], &["src/c.rs"]),
        );
    }
}

#[test]
fn a_closure_the_next_cycle_overtook_is_finished_and_then_left_alone() {
    let w = World::new();
    w.tick();
    let release = w.release_item();
    publish(&w);
    // The note's channel fails: the closure is left open.
    let failing = World {
        signal: Recorder {
            fail: true,
            ..Recorder::default()
        },
        ..w
    };
    failing.tick();
    assert_ne!(
        failing.tracker.item(&release.key).state.status,
        Some(Status::Done)
    );
    // The next cycle is cut before the channel is repaired.
    failing.cut_next_cycle();
    let w = World {
        signal: Recorder::default(),
        ..failing
    };
    let r = w.tick();
    assert!(
        !r.failures.iter().any(|f| f.contains(&release.key)),
        "{r:?}"
    );
    assert_eq!(
        w.tracker.item(&release.key).state.status,
        Some(Status::Done),
        "{r:?}"
    );
    let sent = w.signal.sent.borrow().clone();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].starts_with("[release-note] acme 1.2.0 is published."));
    assert!(
        !w.tracker.list_by_label(NEXT_LABEL).unwrap().is_empty(),
        "the next cycle runs beside it"
    );
    // Later ticks write nothing more to the closed release item.
    let before = w.tracker.writes().len();
    for _ in 0..3 {
        w.tick();
    }
    let touched: Vec<String> = w.tracker.writes()[before..]
        .iter()
        .filter(|l| l.contains(&release.key))
        .cloned()
        .collect();
    assert!(touched.is_empty(), "{touched:?}");
    assert_eq!(w.signal.sent.borrow().len(), 1);
}
