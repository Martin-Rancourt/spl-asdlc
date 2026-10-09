//! The tick against the in-memory GitHub and tracker: the PR pass and the
//! sweep, end to end, and the promise that a second tick on unchanged facts
//! writes nothing, anywhere.

use super::*;
use crate::action_log::Outcome;
use crate::config::Config;
use crate::github::fake::{FakeGitHub, pull_request};
use crate::github::{
    Actor, CheckEntry, CheckKind, Identity, PrState, PullRequest, RequiredChecks, Review,
    ReviewThread,
};
use crate::method::texts::{self, BridgeItem};
use crate::method::vocabulary::Status;
use crate::notify::Recorder;
use crate::tracker::fake::FakeTracker;

const COMPLETE: &str = "## Goal\nIt works.\n\n## Acceptance criteria\n- [x] it works\n- [ ] it is fast\n\n## Business rules\nNo new rule.\n";

fn config() -> Config {
    crate::config::spec_example::config()
}

struct World {
    config: Config,
    github: FakeGitHub,
    tracker: FakeTracker,
    signal: Recorder,
    /// The action log the ticks of this world append to, as a host's.
    log_path: std::path::PathBuf,
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.log_path);
    }
}

/// A log file no other test writes.
fn log_file() -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("spl-asdlc-tick-{}-{n}.jsonl", std::process::id()))
}

impl World {
    fn new() -> World {
        let github = FakeGitHub::new();
        {
            let mut s = github.state.borrow_mut();
            s.identity = Some(Identity::App {
                slug: "spl-asdlc".into(),
                app_id: "1".into(),
            });
            s.open_pulls.insert(repo(), Ok(Vec::new()));
            s.tags.insert(repo(), Vec::new());
            s.branches.insert(repo(), Vec::new());
            s.default_branch.insert(repo(), "main".into());
            s.merged_pulls.insert(repo(), Ok(Vec::new()));
            s.required_checks.insert(
                (repo(), "main".into()),
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
        World {
            config: config(),
            github,
            tracker: FakeTracker::new("ABC"),
            signal: Recorder::default(),
            log_path: log_file(),
        }
    }

    fn tick(&self, dry_run: bool) -> (Report, ActionLog) {
        let t = Tick {
            config: &self.config,
            repo: repo(),
            github: &self.github,
            tracker: &self.tracker,
            signal: &self.signal,
            github_login: bot("spl-asdlc"),
            now: crate::github::Timestamp::parse("2026-10-05T12:00:00Z").unwrap(),
            deadline: None,
            dry_run,
        };
        // A dry run keeps its log in memory, as the command line does.
        let mut log = if dry_run {
            ActionLog::in_memory()
        } else {
            ActionLog::at(self.log_path.clone())
        };
        let report = run(&t, &mut log);
        log.flush().unwrap();
        (report, log)
    }

    fn open(&self, pr: PullRequest) {
        let mut s = self.github.state.borrow_mut();
        s.pull_files
            .insert((repo(), pr.number), Ok(vec!["src/lib.rs".into()]));
        s.comments
            .entry((repo(), pr.number))
            .or_insert_with(|| Ok(Vec::new()));
        if let Some(Ok(list)) = s.open_pulls.get_mut(&repo()) {
            list.retain(|p| p.number != pr.number);
            list.push(pr);
        }
    }

    fn ticket(&self, key: &str, status: Status, pr: &PullRequest) {
        self.tracker.seed(key, "A ticket", COMPLETE, status, &[]);
        self.tracker.link(key, &pr.url);
    }

    fn github_writes(&self) -> Vec<String> {
        self.github.writes()
    }
}

fn repo() -> crate::github::RepoSlug {
    crate::github::RepoSlug::parse("example-org/acme").unwrap()
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

fn review(author: &str, state: &str, sha: &str, body: &str) -> Review {
    Review {
        author: Some(author.into()),
        body: body.into(),
        state: state.into(),
        commit_sha: Some(sha.into()),
        submitted_at: None,
    }
}

fn children_of(w: &World, key: &str) -> Vec<crate::tracker::Item> {
    crate::tracker::Tracker::children(&w.tracker, key).unwrap()
}

/// The implementer ticks the last box: the ticket's contract is complete.
fn check_every_box(w: &World, key: &str) {
    let all = COMPLETE.replace("- [ ] it is fast", "- [x] it is fast");
    crate::tracker::Tracker::update_description(&w.tracker, key, &all).unwrap();
}

// ---- spec 04 §3: entering In Review ----

#[test]
fn a_green_pr_with_a_complete_contract_opens_a_review_round_then_nothing_more() {
    let w = World::new();
    let pr = green(pull_request(7));
    w.ticket("ABC-1", Status::InProgress, &pr);
    w.open(pr.clone());
    let (report, log) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert_eq!(w.tracker.item("ABC-1").state.status, Some(Status::InReview));
    let children = children_of(&w, "ABC-1");
    let kinds: Vec<BridgeItem> = children
        .iter()
        .filter_map(|c| texts::bridge_item(&c.description, c.parent.as_deref()))
        .collect();
    let scope = texts::round_scope(&pr.url, &pr.head_sha);
    assert!(kinds.contains(&BridgeItem::Angle {
        scope: scope.clone(),
        angle: "maintainability".into()
    }));
    assert!(kinds.contains(&BridgeItem::Consolidation { scope }));
    let writes = w.tracker.writes();
    assert_eq!(
        writes.last().unwrap(),
        "status ABC-1 In Review",
        "In Review last: {writes:?}"
    );
    assert!(log.entries().iter().all(|e| e.outcome == Outcome::Done));

    // The same facts, a second tick: nothing written; the log notes once
    // why the PR waits.
    quiet(&w);
}

// ---- BR-ASDLC-015: an item carries its protocol, frozen at creation ----

#[test]
fn a_rounds_items_carry_their_protocol_and_one_found_by_its_marker_is_never_rewritten() {
    let w = World::new();
    let pr = green(pull_request(7));
    w.ticket("ABC-1", Status::InProgress, &pr);
    w.open(pr.clone());
    // An angle sub-item of this round, created by an older bridge (its
    // creation answer lost), with another protocol.
    let scope = texts::round_scope(&pr.url, &pr.head_sha);
    let m = texts::marker("angle", &[("scope", &scope), ("angle", "maintainability")]);
    let older = format!(
        "{m}\n\n## Goal\nAn older text.\n\n## Acceptance criteria\n- [ ] it\n\n## Business rules\nNo new rule.\n\n## Protocol\n\nAn older protocol.\n"
    );
    w.tracker.seed(
        "ABC-900",
        "Maintainability review",
        &older,
        Status::Ready,
        &[],
    );
    w.tracker.state.borrow_mut().set_parent("ABC-900", "ABC-1");
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert_eq!(w.tracker.item("ABC-900").description, older);
    assert!(
        !w.tracker.writes().iter().any(|x| x.starts_with("describe")),
        "{:?}",
        w.tracker.writes()
    );
    let children = children_of(&w, "ABC-1");
    let angles: Vec<_> = children
        .iter()
        .filter(|c| {
            matches!(
                texts::bridge_item(&c.description, c.parent.as_deref()),
                Some(BridgeItem::Angle { .. })
            )
        })
        .collect();
    assert_eq!(angles.len(), 1, "found by its marker, not created again");
    // The item the tick created carries the protocol of this bridge.
    let consolidation = children
        .iter()
        .find(|c| {
            matches!(
                texts::bridge_item(&c.description, c.parent.as_deref()),
                Some(BridgeItem::Consolidation { .. })
            )
        })
        .unwrap();
    assert!(
        consolidation.description.contains(&format!(
            "Embedded by spl-asdlc {}",
            texts::protocol::VERSION
        )) && consolidation.description.contains("### Consolidate"),
        "{}",
        consolidation.description
    );
    quiet(&w);
}

#[test]
fn a_new_angle_sub_item_embeds_the_pr_review_protocol_and_links_the_projects_notes() {
    let mut w = World::new();
    w.config.review.angles[0].notes = Some(".spl-asdlc/angles/maintainability.md".into());
    let pr = green(pull_request(7));
    w.ticket("ABC-1", Status::InProgress, &pr);
    w.open(pr.clone());
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    let angle = children_of(&w, "ABC-1")
        .into_iter()
        .find(|c| {
            matches!(
                texts::bridge_item(&c.description, c.parent.as_deref()),
                Some(BridgeItem::Angle { .. })
            )
        })
        .unwrap();
    let d = &angle.description;
    assert!(d.contains("### Review a PR from one angle"), "{d}");
    assert!(d.contains("### Angle: maintainability"), "{d}");
    assert!(
        d.contains(
            "- Project notes: `.spl-asdlc/angles/maintainability.md`, in the project's repository"
        ),
        "{d}"
    );
    assert!(crate::method::contract::missing_from_contract(d).is_empty());
}

#[test]
fn an_unreadable_sensitive_list_is_said_once_and_holds_the_merge() {
    let w = World::new();
    w.github.state.borrow_mut().files.clear();
    let mut pr = in_review(&w, 7);
    pr.reviews = vec![review("coordinator-login", "APPROVED", &pr.head_sha, "ok")];
    w.open(pr);
    w.tick(false);
    w.tick(false);
    assert!(!w.github_writes().iter().any(|l| l.starts_with("merge")));
    let notices = w.signal.sent.borrow().clone();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].starts_with("[stuck-pr]") && notices[0].contains("configuration invalid"));
}

#[test]
fn a_ready_ticket_goes_in_progress_when_its_pr_appears() {
    let w = World::new();
    let pr = pull_request(7); // no checks yet
    w.ticket("ABC-1", Status::Ready, &pr);
    w.open(pr);
    w.tick(false);
    assert_eq!(
        w.tracker.item("ABC-1").state.status,
        Some(Status::InProgress)
    );
}

#[test]
fn an_incomplete_contract_is_said_once_on_the_pr() {
    let w = World::new();
    let pr = green(pull_request(7));
    w.tracker
        .seed("ABC-1", "A ticket", "## Goal\nx\n", Status::InProgress, &[]);
    w.tracker.link("ABC-1", &pr.url);
    w.open(pr);
    w.tick(false);
    w.tick(false);
    let comments: Vec<String> = w
        .github_writes()
        .into_iter()
        .filter(|l| l.starts_with("comment"))
        .collect();
    assert_eq!(comments.len(), 1, "{comments:?}");
    assert!(comments[0].contains("The contract of ABC-1 is incomplete"));
    assert_eq!(
        w.tracker.item("ABC-1").state.status,
        Some(Status::InProgress)
    );
}

#[test]
fn a_pr_without_a_ticket_is_told_once_and_left_alone() {
    let w = World::new();
    w.open(green(pull_request(7)));
    w.tick(false);
    w.tick(false);
    let comments: Vec<String> = w
        .github_writes()
        .into_iter()
        .filter(|l| l.starts_with("comment"))
        .collect();
    assert_eq!(comments.len(), 1);
    assert!(w.tracker.writes().is_empty());
}

#[test]
fn a_behind_branch_is_brought_up_to_date_before_the_round() {
    let w = World::new();
    let mut pr = green(pull_request(7));
    pr.merge_state = "BEHIND".into();
    w.ticket("ABC-1", Status::InProgress, &pr);
    w.open(pr);
    w.tick(false);
    assert_eq!(
        w.github_writes(),
        vec!["update-branch example-org/acme#7 at head-7"]
    );
    assert_eq!(
        w.tracker.item("ABC-1").state.status,
        Some(Status::InProgress)
    );
}

// ---- spec 04 §4-6: reviews, approval, merge ----

fn in_review(w: &World, number: u64) -> PullRequest {
    let pr = green(pull_request(number));
    w.ticket(&format!("ABC-{number}"), Status::InProgress, &pr);
    w.open(pr.clone());
    w.tick(false);
    assert_eq!(
        w.tracker.item(&format!("ABC-{number}")).state.status,
        Some(Status::InReview)
    );
    pr
}

#[test]
fn an_approved_green_pr_is_merged_and_its_ticket_goes_to_deploy() {
    let w = World::new();
    let mut pr = in_review(&w, 7);
    check_every_box(&w, "ABC-7");
    let angle = children_of(&w, "ABC-7")
        .into_iter()
        .find(|c| c.title.starts_with("Maintainability"))
        .unwrap();
    pr.reviews = vec![
        review(
            "angle-agent",
            "COMMENTED",
            &pr.head_sha,
            &format!("Reviewed for {}.", angle.key),
        ),
        review("coordinator-login", "APPROVED", &pr.head_sha, "LGTM"),
    ];
    pr.threads = vec![ReviewThread {
        resolved: true,
        outdated: false,
        path: None,
        first_author: None,
        first_body: String::new(),
    }];
    w.open(pr);
    let (report, log) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(log.entries().iter().all(|e| e.outcome == Outcome::Done));
    assert_eq!(w.tracker.item(&angle.key).state.status, Some(Status::Done));
    assert!(
        w.github_writes()
            .contains(&"merge example-org/acme#7 at head-7 squash".to_string())
    );
    assert!(
        w.github_writes()
            .contains(&"delete-branch example-org/acme feat/change-7".to_string())
    );
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::ToDeploy));
    let comments = crate::tracker::Tracker::comments(&w.tracker, "ABC-7").unwrap();
    assert!(comments.iter().any(|c| {
        c.body
            .contains("merged_pr: https://github.com/example-org/acme/pull/7")
    }));
}

#[test]
fn an_approval_on_an_older_head_or_an_open_thread_does_not_merge() {
    let w = World::new();
    let mut pr = in_review(&w, 7);
    pr.reviews = vec![review("coordinator-login", "APPROVED", "older", "ok")];
    w.open(pr.clone());
    w.tick(false);
    pr.reviews = vec![review("coordinator-login", "APPROVED", &pr.head_sha, "ok")];
    pr.threads = vec![ReviewThread {
        resolved: false,
        outdated: false,
        path: None,
        first_author: None,
        first_body: String::new(),
    }];
    w.open(pr);
    w.tick(false);
    assert!(!w.github_writes().iter().any(|l| l.starts_with("merge")));
}

#[test]
fn changes_requested_send_the_ticket_back_without_a_loop() {
    let w = World::new();
    let mut pr = in_review(&w, 7);
    pr.reviews = vec![review(
        "coordinator-login",
        "CHANGES_REQUESTED",
        &pr.head_sha,
        "redo",
    )];
    w.open(pr.clone());
    w.tick(false);
    assert_eq!(
        w.tracker.item("ABC-7").state.status,
        Some(Status::InProgress)
    );
    w.tick(false);
    assert_eq!(
        w.tracker.item("ABC-7").state.status,
        Some(Status::InProgress),
        "not back in review while the request stands on this head"
    );
    // The implementer pushes: a new head opens a new round.
    pr.head_sha = "head-7b".into();
    w.open(pr);
    w.tick(false);
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::InReview));
}

#[test]
fn one_merge_per_repository_per_tick() {
    let w = World::new();
    let mut prs = Vec::new();
    for n in [7, 8] {
        prs.push(in_review(&w, n));
    }
    for pr in prs {
        w.open(approved(&w, pr));
    }
    w.tick(false);
    let merges: Vec<String> = w
        .github_writes()
        .into_iter()
        .filter(|l| l.starts_with("merge"))
        .collect();
    assert_eq!(merges, vec!["merge example-org/acme#7 at head-7 squash"]);
    w.tick(false);
    assert!(
        w.github_writes()
            .contains(&"merge example-org/acme#8 at head-8 squash".to_string())
    );
}

// ---- spec 07: fail closed, dry run, sweep ----

#[test]
fn an_unreadable_pr_list_writes_nothing_and_says_so() {
    let w = World::new();
    w.github.state.borrow_mut().open_pulls.insert(
        repo(),
        Err(crate::github::ReadError::unreadable("open PRs", "HTTP 502")),
    );
    let (report, log) = w.tick(false);
    assert!(!report.ok());
    assert!(log.entries().is_empty());
    assert!(w.github_writes().is_empty() && w.tracker.writes().is_empty());
}

#[test]
fn a_dry_run_reads_everything_and_writes_nothing() {
    let w = World::new();
    let pr = green(pull_request(7));
    w.ticket("ABC-1", Status::InProgress, &pr);
    w.open(pr);
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
        now: crate::github::Timestamp::parse("2026-10-05T12:00:00Z").unwrap(),
        deadline: None,
        dry_run: true,
    };
    let mut log = ActionLog::in_memory();
    run(&t, &mut log);
    assert!(w.tracker.writes().is_empty() && w.github_writes().is_empty());
    assert!(!log.entries().is_empty());
    assert!(log.entries().iter().all(|e| e.outcome == Outcome::WouldDo));
    assert!(
        intents
            .lines()
            .contains(&"tracker: ABC-1 → In Review".to_string())
    );
}

#[test]
fn a_pr_merged_outside_the_bridge_is_swept_once() {
    let w = World::new();
    let mut pr = pull_request(9);
    pr.state = PrState::Merged;
    pr.merged_at = crate::github::Timestamp::parse("2026-10-05T10:00:00Z");
    pr.merged_by = Some(Actor {
        login: "someone-login".into(),
        is_bot: false,
    });
    w.ticket("ABC-9", Status::InReview, &pr);
    check_every_box(&w, "ABC-9");
    w.github
        .state
        .borrow_mut()
        .merged_pulls
        .insert(repo(), Ok(vec![pr]));
    w.tick(false);
    assert_eq!(w.tracker.item("ABC-9").state.status, Some(Status::ToDeploy));
    let after = w.tracker.writes().len();
    w.tick(false);
    assert_eq!(w.tracker.writes().len(), after, "once");
}

#[test]
fn a_ticket_the_closure_sent_back_is_not_swept_forward_again() {
    let w = World::new();
    let mut pr = pull_request(9);
    pr.state = PrState::Merged;
    pr.merged_at = crate::github::Timestamp::parse("2026-10-05T10:00:00Z");
    w.ticket("ABC-9", Status::InProgress, &pr);
    crate::tracker::Tracker::comment(
        &w.tracker,
        "ABC-9",
        &texts::merged_comment(&pr.url, "main", None, "To Deploy", "the closure"),
    )
    .unwrap();
    w.github
        .state
        .borrow_mut()
        .merged_pulls
        .insert(repo(), Ok(vec![pr]));
    let before = w.tracker.writes().len();
    w.tick(false);
    assert_eq!(w.tracker.writes().len(), before);
}

/// The login GitHub shows for an App.
fn bot(slug: &str) -> String {
    format!("{slug}[bot]")
}

// ---- regressions from the lot 6-7 review ----

#[test]
fn a_status_write_that_fails_after_the_merge_is_retried_by_the_sweep() {
    let w = World::new();
    let pr = in_review(&w, 7);
    check_every_box(&w, "ABC-7");
    w.open(approved(&w, pr));
    w.tracker
        .fail_writes_starting_with("status ABC-7 To Deploy");
    let (report, _) = w.tick(false);
    assert!(!report.ok());
    assert!(w.github_writes().iter().any(|l| l.starts_with("merge")));
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::InReview));
    w.tracker.state.borrow_mut().failing_writes.clear();
    w.tick(false);
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::ToDeploy));
}

#[test]
fn a_refused_merge_does_not_starve_the_next_candidate() {
    let w = World::new();
    let mut prs = Vec::new();
    for n in [7, 8] {
        prs.push(in_review(&w, n));
    }
    for pr in prs {
        w.open(approved(&w, pr));
    }
    w.github
        .fail_writes_starting_with("merge example-org/acme#7", Some(409), "ruleset");
    w.tick(false);
    assert!(
        w.github_writes()
            .contains(&"merge example-org/acme#8 at head-8 squash".to_string())
    );
}

#[test]
fn a_fork_branch_is_never_deleted_in_the_base_repository() {
    let w = World::new();
    let mut pr = in_review(&w, 7);
    pr.cross_repository = true;
    w.open(approved(&w, pr));
    w.tick(false);
    assert!(w.github_writes().iter().any(|l| l.starts_with("merge")));
    assert!(
        !w.github_writes()
            .iter()
            .any(|l| l.starts_with("delete-branch"))
    );
}

#[test]
fn a_marker_in_someone_elses_comment_is_not_the_bridges() {
    let w = World::new();
    let pr = green(pull_request(7));
    w.open(pr.clone());
    let forged = format!(
        "nothing to see\n\n{}",
        texts::marker("no-ticket", &[("pr", &pr.url)])
    );
    w.github.state.borrow_mut().comments.insert(
        (repo(), 7),
        Ok(vec![crate::github::Comment {
            id: 1,
            author: Some(Actor {
                login: "someone-login".into(),
                is_bot: false,
            }),
            body: forged,
            created_at: None,
            url: String::new(),
        }]),
    );
    w.tick(false);
    assert_eq!(
        w.github_writes()
            .iter()
            .filter(|l| l.starts_with("comment"))
            .count(),
        1,
        "the bridge still says it once"
    );
}

// ---- spec 04 §9: PRs of Apps that need no ticket ----

fn app_pr(number: u64, slug: &str) -> PullRequest {
    let mut pr = green(pull_request(number));
    pr.title = "build(deps): bump sharp from 0.35.2 to 0.35.4".into();
    pr.author = Some(Actor {
        login: bot(slug),
        is_bot: true,
    });
    pr
}

fn pr_ticket(w: &World, pr: &PullRequest) -> Option<crate::tracker::Item> {
    let keys = crate::tracker::Tracker::items_linked_to_pr(&w.tracker, &pr.url).unwrap();
    keys.first().map(|k| w.tracker.item(k))
}

#[test]
fn a_pr_of_a_ticketless_app_gets_a_ticket_and_then_the_path_of_every_pr() {
    let mut w = World::new();
    w.config.review.ticketless_authors = vec!["dependabot".into()];
    let pr = app_pr(7, "dependabot");
    w.open(pr.clone());
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    let ticket = pr_ticket(&w, &pr).expect("the bridge opened and linked a ticket");
    assert_eq!(ticket.state.status, Some(Status::Ready));
    assert!(ticket.labels.contains(&"ticketless-pr".to_string()));
    assert!(ticket.title.contains("PR #7"), "{}", ticket.title);
    assert!(
        crate::method::contract::missing_from_contract(&ticket.description).is_empty()
            && !crate::method::contract::declares_nothing(&ticket.description),
        "its contract is complete: {}",
        ticket.description
    );
    assert!(
        !w.github_writes().iter().any(|l| l.starts_with("comment")),
        "no 'no ticket' comment: {:?}",
        w.github_writes()
    );

    // The next tick takes it like any ticketed PR: the review round opens.
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert_eq!(
        w.tracker.item(&ticket.key).state.status,
        Some(Status::InReview)
    );
    assert!(!children_of(&w, &ticket.key).is_empty());
    let opened = w
        .tracker
        .writes()
        .iter()
        .filter(|l| l.starts_with("create PR #7"))
        .count();
    assert_eq!(opened, 1, "{:?}", w.tracker.writes());
}

#[test]
fn a_ticket_opened_before_whose_link_was_lost_is_linked_again_not_opened_twice() {
    let mut w = World::new();
    w.config.review.ticketless_authors = vec!["dependabot".into()];
    let pr = app_pr(7, "dependabot");
    let (title, description) = texts::pr_ticket(7, &pr.title, &bot("dependabot"), &pr.url);
    w.tracker.seed(
        "ABC-9",
        &title,
        &description,
        Status::Ready,
        &["ticketless-pr"],
    );
    w.open(pr.clone());
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert_eq!(pr_ticket(&w, &pr).map(|i| i.key), Some("ABC-9".to_string()));
    assert!(
        !w.tracker.writes().iter().any(|l| l.starts_with("create")),
        "{:?}",
        w.tracker.writes()
    );
}

#[test]
fn only_the_listed_apps_skip_the_ticket_and_never_a_human_with_their_login() {
    let mut w = World::new();
    w.config.review.ticketless_authors = vec!["dependabot".into()];
    w.open(app_pr(7, "renovate"));
    let mut human = app_pr(8, "dependabot");
    human.author = Some(Actor {
        login: "dependabot".into(),
        is_bot: false,
    });
    w.open(human);
    w.tick(false);
    assert!(w.tracker.writes().is_empty(), "{:?}", w.tracker.writes());
    let comments = w
        .github_writes()
        .into_iter()
        .filter(|l| l.starts_with("comment"))
        .count();
    assert_eq!(comments, 2, "both are told they need a ticket");
}

#[test]
fn a_ticketless_pr_closed_without_merge_cancels_its_ticket_and_sub_items() {
    let mut w = World::new();
    w.config.review.ticketless_authors = vec!["dependabot".into()];
    let pr = app_pr(7, "dependabot");
    w.open(pr.clone());
    w.tick(false);
    w.tick(false);
    let ticket = pr_ticket(&w, &pr).unwrap();
    assert_eq!(ticket.state.status, Some(Status::InReview));
    // Dependabot supersedes it: closed, not merged.
    {
        let mut s = w.github.state.borrow_mut();
        if let Some(Ok(list)) = s.open_pulls.get_mut(&repo()) {
            list.clear();
        }
        let mut closed = pr.clone();
        closed.state = PrState::Closed;
        s.closed_pulls.insert(repo(), Ok(vec![closed]));
    }
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert_eq!(
        w.tracker.item(&ticket.key).state.status,
        Some(Status::Canceled)
    );
    for child in children_of(&w, &ticket.key) {
        assert_eq!(child.state.status, Some(Status::Canceled), "{}", child.key);
    }
    // Reopened: it stays on its canceled ticket until a person reopens it;
    // the bridge neither revives it nor opens a second one.
    {
        let mut s = w.github.state.borrow_mut();
        s.closed_pulls.insert(repo(), Ok(Vec::new()));
    }
    w.open(pr.clone());
    let before = w.tracker.writes().len();
    w.tick(false);
    assert_eq!(
        w.tracker.writes().len(),
        before,
        "{:?}",
        &w.tracker.writes()[before..]
    );
}

#[test]
fn two_open_tickets_for_one_ticketless_pr_open_none_and_say_so() {
    let mut w = World::new();
    w.config.review.ticketless_authors = vec!["dependabot".into()];
    let pr = app_pr(7, "dependabot");
    let (title, description) = texts::pr_ticket(7, &pr.title, &bot("dependabot"), &pr.url);
    for key in ["ABC-8", "ABC-9"] {
        w.tracker
            .seed(key, &title, &description, Status::Ready, &["ticketless-pr"]);
    }
    w.open(pr.clone());
    let (report, _) = w.tick(false);
    assert!(
        report.failures.iter().any(|f| f.contains("ABC-8, ABC-9")),
        "{report:?}"
    );
    assert!(w.tracker.writes().is_empty(), "{:?}", w.tracker.writes());
}

// ---- spec 04 §10: a ticket and its PRs (BR-ASDLC-007, BR-ASDLC-008) ----

/// Every angle of the PR's round has submitted its review, then the
/// coordinator approves at the head (BR-ASDLC-018).
fn approved(w: &World, mut pr: PullRequest) -> PullRequest {
    pr.reviews = angle_reviews(w, &pr);
    pr.reviews
        .push(review("coordinator-login", "APPROVED", &pr.head_sha, "ok"));
    pr
}

/// A submitted review naming each angle sub-item linked to the PR.
fn angle_reviews(w: &World, pr: &PullRequest) -> Vec<Review> {
    crate::tracker::Tracker::items_linked_to_pr(&w.tracker, &pr.url)
        .unwrap()
        .into_iter()
        .filter(|k| {
            let item = w.tracker.item(k);
            matches!(
                texts::bridge_item(&item.description, item.parent.as_deref()),
                Some(BridgeItem::Angle { .. })
            )
        })
        .map(|k| {
            review(
                "angle-agent",
                "COMMENTED",
                &pr.head_sha,
                &format!("Reviewed for {k}."),
            )
        })
        .collect()
}

/// The items of the review rounds of a PR, on its ticket.
fn round(w: &World, ticket: &str, pr: &PullRequest) -> Vec<crate::tracker::Item> {
    let items: Vec<_> = children_of(w, ticket)
        .into_iter()
        .filter(|c| merge::round_of(c, &pr.url))
        .collect();
    assert!(!items.is_empty(), "{} has a round on {ticket}", pr.url);
    items
}

fn all_in(items: &[crate::tracker::Item], status: Status) -> bool {
    items.iter().all(|i| i.state.status == Some(status))
}

/// A second tick on unchanged facts writes nothing, anywhere, but the
/// action log's note of why a PR waits when that is new; a third, nothing
/// at all.
fn quiet(w: &World) {
    let before = (w.tracker.writes().len(), w.github_writes().len());
    let (report, log) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(
        log.entries().iter().all(|e| e.outcome == Outcome::Waiting),
        "{:?}",
        log.entries()
    );
    let (report, log) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(log.entries().is_empty(), "{:?}", log.entries());
    assert_eq!(
        (w.tracker.writes().len(), w.github_writes().len()),
        before,
        "{:?}",
        &w.tracker.writes()[before.0..]
    );
}

fn merge_comment(w: &World, ticket: &str, pr: &PullRequest) -> String {
    crate::tracker::Tracker::comments(&w.tracker, ticket)
        .unwrap()
        .into_iter()
        .find(|c| c.body.contains(&format!("merged_pr: {}", pr.url)))
        .expect("the merge is recorded")
        .body
}

fn close_unmerged(w: &World, pr: &PullRequest) {
    let mut s = w.github.state.borrow_mut();
    if let Some(Ok(list)) = s.open_pulls.get_mut(&repo()) {
        list.retain(|p| p.number != pr.number);
    }
    let mut closed = pr.clone();
    closed.state = PrState::Closed;
    s.closed_pulls.insert(repo(), Ok(vec![closed]));
}

#[test]
fn a_merge_that_leaves_a_box_open_sends_the_ticket_back_to_work() {
    let w = World::new();
    let pr = in_review(&w, 7);
    w.open(approved(&w, pr.clone()));
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(w.github_writes().iter().any(|l| l.starts_with("merge")));
    assert_eq!(
        w.tracker.item("ABC-7").state.status,
        Some(Status::InProgress),
        "the completeness rule: a box is open"
    );
    let comment = merge_comment(&w, "ABC-7", &pr);
    assert!(comment.contains("state: In Progress"), "{comment}");
    assert!(
        comment.contains("1 box(es) of its contract remain open"),
        "{comment}"
    );
    quiet(&w);
}

#[test]
fn a_merge_with_every_box_checked_goes_to_deploy_once() {
    let w = World::new();
    let pr = in_review(&w, 7);
    check_every_box(&w, "ABC-7");
    w.open(approved(&w, pr.clone()));
    w.tick(false);
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::ToDeploy));
    assert!(merge_comment(&w, "ABC-7", &pr).contains("state: To Deploy"));
    quiet(&w);
}

#[test]
fn a_rounds_items_close_when_its_pr_merges_while_the_ticket_stays_open() {
    let w = World::new();
    let pr = in_review(&w, 7);
    assert!(all_in(&round(&w, "ABC-7", &pr), Status::Ready));
    w.open(approved(&w, pr.clone()));
    w.tick(false);
    assert_eq!(
        w.tracker.item("ABC-7").state.status,
        Some(Status::InProgress),
        "the ticket stays open for a later PR"
    );
    assert!(
        all_in(&round(&w, "ABC-7", &pr), Status::Done),
        "the angle sub-items and the consolidation item close with their PR"
    );
    quiet(&w);
}

#[test]
fn a_rounds_items_are_canceled_when_its_pr_is_closed_unmerged_while_the_ticket_stays_open() {
    let w = World::new();
    let pr = in_review(&w, 7);
    close_unmerged(&w, &pr);
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(all_in(&round(&w, "ABC-7", &pr), Status::Canceled));
    assert_eq!(
        w.tracker.item("ABC-7").state.status,
        Some(Status::InProgress),
        "no PR in review and a box open: back to work, not canceled"
    );
    let said = crate::tracker::Tracker::comments(&w.tracker, "ABC-7")
        .unwrap()
        .iter()
        .filter(|c| c.body.contains("was closed without merge"))
        .count();
    assert_eq!(said, 1);
    quiet(&w);
}

#[test]
fn a_second_pr_on_a_ticket_past_the_merge_brings_it_back_to_review() {
    let w = World::new();
    let all = COMPLETE.replace("- [ ] it is fast", "- [x] it is fast");
    w.tracker
        .seed("ABC-7", "A ticket", &all, Status::ToDeploy, &[]);
    let pr = green(pull_request(8));
    w.tracker.link("ABC-7", &pr.url);
    w.open(pr.clone());

    // REQ-MERGE-028 under the state rules: back In Review, the PR not
    // operated on this tick.
    w.tick(false);
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::InReview));
    assert!(children_of(&w, "ABC-7").is_empty());

    // The review rule: its round opens and its cycle runs in full.
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(all_in(&round(&w, "ABC-7", &pr), Status::Ready));
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::InReview));
    quiet(&w);

    w.open(approved(&w, pr.clone()));
    w.tick(false);
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::ToDeploy));
    quiet(&w);
}

/// Two PRs of one ticket, both in review.
fn two_in_review(w: &World) -> (PullRequest, PullRequest) {
    let a = green(pull_request(7));
    let b = green(pull_request(8));
    w.ticket("ABC-7", Status::InProgress, &a);
    w.tracker.link("ABC-7", &b.url);
    w.open(a.clone());
    w.open(b.clone());
    w.tick(false);
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::InReview));
    assert!(all_in(&round(w, "ABC-7", &a), Status::Ready));
    assert!(all_in(&round(w, "ABC-7", &b), Status::Ready));
    quiet(w);
    (a, b)
}

#[test]
fn while_another_pr_is_in_review_the_ticket_stays_in_review() {
    let w = World::new();
    let (a, b) = two_in_review(&w);
    w.open(approved(&w, a.clone()));
    w.tick(false);
    assert!(
        w.github_writes()
            .contains(&"merge example-org/acme#7 at head-7 squash".to_string())
    );
    assert_eq!(
        w.tracker.item("ABC-7").state.status,
        Some(Status::InReview),
        "the review rule comes before the completeness rule"
    );
    assert!(merge_comment(&w, "ABC-7", &a).contains("state: In Review"));
    assert!(all_in(&round(&w, "ABC-7", &a), Status::Done));
    assert!(
        all_in(&round(&w, "ABC-7", &b), Status::Ready),
        "the other PR's round is its own"
    );
    quiet(&w);

    // The last PR merges: the completeness rule decides.
    w.open(approved(&w, b.clone()));
    w.tick(false);
    assert_eq!(
        w.tracker.item("ABC-7").state.status,
        Some(Status::InProgress)
    );
    assert!(all_in(&round(&w, "ABC-7", &b), Status::Done));
    quiet(&w);
}

#[test]
fn changes_requested_on_one_pr_wait_while_another_is_in_review() {
    let w = World::new();
    let (a, mut b) = two_in_review(&w);
    b.reviews = vec![review(
        "coordinator-login",
        "CHANGES_REQUESTED",
        &b.head_sha,
        "redo",
    )];
    w.open(b.clone());
    w.tick(false);
    assert_eq!(
        w.tracker.item("ABC-7").state.status,
        Some(Status::InReview),
        "PR #7 is still in review"
    );
    quiet(&w);
    // PR #7 merges: PR #8, sent back, is its implementer's.
    w.open(approved(&w, a));
    w.tick(false);
    assert_eq!(
        w.tracker.item("ABC-7").state.status,
        Some(Status::InProgress)
    );
    assert!(
        all_in(&round(&w, "ABC-7", &b), Status::Ready),
        "PR #8 is still open: its round stays"
    );
    quiet(&w);
}

#[test]
fn a_pr_closed_unmerged_leaves_the_ticket_in_review_while_another_is_reviewed() {
    let w = World::new();
    let (a, b) = two_in_review(&w);
    close_unmerged(&w, &b);
    w.tick(false);
    assert!(all_in(&round(&w, "ABC-7", &b), Status::Canceled));
    assert!(all_in(&round(&w, "ABC-7", &a), Status::Ready));
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::InReview));
    quiet(&w);
}

#[test]
fn a_pr_whose_round_is_done_but_not_merged_keeps_the_ticket_in_review() {
    // PR #7 finished its round and waits for its merge (an approval, the
    // one-merge slot); PR #8 is closed unmerged. The ticket stays In Review:
    // the pass and the state rules agree, and no tick flips it back and forth.
    let w = World::new();
    let (a, b) = two_in_review(&w);
    for item in round(&w, "ABC-7", &a) {
        crate::tracker::Tracker::set_status(&w.tracker, &item.key, Status::Done, None).unwrap();
    }
    close_unmerged(&w, &b);
    w.tick(false);
    assert!(all_in(&round(&w, "ABC-7", &b), Status::Canceled));
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::InReview));
    quiet(&w);
    quiet(&w);
}

#[test]
fn an_unreadable_ticket_of_another_pr_holds_the_state_after_a_merge() {
    let w = World::new();
    let pr = in_review(&w, 7);
    let other = green(pull_request(9));
    w.tracker
        .seed("ABC-9", "Another", COMPLETE, Status::InProgress, &[]);
    w.tracker.link("ABC-9", &other.url);
    w.open(other);
    w.tracker.make_unreadable("ABC-9");
    w.open(approved(&w, pr.clone()));
    let (report, _) = w.tick(false);
    assert!(
        w.github_writes()
            .iter()
            .any(|l| l.starts_with("merge example-org/acme#7"))
    );
    assert!(!report.ok());
    assert_eq!(
        w.tracker.item("ABC-7").state.status,
        Some(Status::InReview),
        "PR #9 may be this ticket's: the state waits (fail closed)"
    );
    // Readable again: the sweep settles it.
    w.tracker.state.borrow_mut().unreadable.clear();
    w.tick(false);
    assert_eq!(
        w.tracker.item("ABC-7").state.status,
        Some(Status::InProgress)
    );
}

// ---- SID-1008: an App's login, never a person's; why a PR waits ----
//
// Authors are as the bridge reads them: an App in the REST spelling
// (`bot()`), a person bare.

/// Whether the PR merges on one approval by `author` with `configured` as
/// the only coordinator; the report when it does not.
fn merges_on(configured: &str, author: &str) -> Result<(), Report> {
    let mut w = World::new();
    w.config.review.coordinators = vec![configured.to_string()];
    let mut pr = in_review(&w, 7);
    pr.reviews = vec![review(author, "APPROVED", &pr.head_sha, "ok")];
    w.open(pr);
    let (report, _) = w.tick(false);
    if w.github_writes()
        .contains(&"merge example-org/acme#7 at head-7 squash".to_string())
    {
        Ok(())
    } else {
        Err(report)
    }
}

#[test]
fn a_coordinator_apps_approval_merges_and_a_person_of_its_name_does_not() {
    for configured in [
        bot("coord-app"),
        bot("COORD-APP"),
        "app/Coord-App".to_string(),
    ] {
        assert_eq!(
            merges_on(&configured, &bot("coord-app")),
            Ok(()),
            "{configured}"
        );
        let held = merges_on(&configured, "coord-app").expect_err("a person is not the App");
        assert_eq!(
            held.waiting_lines(),
            vec!["waiting: pr#7 no coordinator approval at head-7 (approvals from: coord-app)"]
        );
    }
    assert_eq!(merges_on("coord", "Coord"), Ok(()), "a person, case folded");
    assert!(
        merges_on("coord-app", &bot("coord-app")).is_err(),
        "a bare login is a person, never the App"
    );
}

#[test]
fn only_the_coordinator_apps_own_request_for_changes_sends_the_ticket_back() {
    for (author, sent_back) in [(bot("Coord-App"), true), ("coord-app".to_string(), false)] {
        let mut w = World::new();
        w.config.review.coordinators = vec![bot("coord-app")];
        let mut pr = in_review(&w, 7);
        pr.reviews = vec![review(&author, "CHANGES_REQUESTED", &pr.head_sha, "redo")];
        w.open(pr);
        w.tick(false);
        let (report, _) = w.tick(false);
        if sent_back {
            assert_eq!(
                w.tracker.item("ABC-7").state.status,
                Some(Status::InProgress),
                "{author}"
            );
            assert_eq!(
                report.waiting_lines(),
                vec![format!(
                    "waiting: pr#7 {author} requested changes at head-7: the implementer's turn"
                )]
            );
        } else {
            assert_eq!(
                w.tracker.item("ABC-7").state.status,
                Some(Status::InReview),
                "a person of the App's name sends nothing back"
            );
            assert_eq!(
                report.waiting_lines(),
                vec!["waiting: pr#7 no coordinator approval at head-7 (no approval at this head)"]
            );
        }
    }
}

#[test]
fn a_ticketless_app_is_recognised_however_the_configuration_spells_it() {
    for configured in [
        "dependabot".to_string(),
        bot("Dependabot"),
        "app/dependabot".to_string(),
    ] {
        let mut w = World::new();
        w.config.review.ticketless_authors = vec![configured.clone()];
        let pr = app_pr(7, "dependabot");
        w.open(pr.clone());
        w.tick(false);
        assert!(pr_ticket(&w, &pr).is_some(), "configured {configured}");
    }
}

#[test]
fn a_pr_without_its_coordinators_approval_says_why_at_every_tick_and_logs_it_once() {
    let w = World::new();
    let mut pr = in_review(&w, 7);
    pr.reviews = vec![
        review("dev-agent", "APPROVED", &pr.head_sha, "ok"),
        review(&bot("qa-app"), "APPROVED", &pr.head_sha, "ok"),
    ];
    w.open(pr.clone());
    let writes = (w.tracker.writes().len(), w.github_writes().len());
    let line = format!(
        "waiting: pr#7 no coordinator approval at head-7 (approvals from: dev-agent, {})",
        bot("qa-app")
    );
    let (report, log) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert_eq!(report.waiting_lines(), vec![line.clone()]);
    let notes: Vec<_> = log.entries().iter().collect();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert_eq!(notes[0].outcome, Outcome::Waiting);
    assert_eq!(notes[0].object, "pr#7");
    assert_eq!(notes[0].inputs["head"], "head-7");
    assert!(crate::action_log::describe(notes[0]).ends_with(&format!(
        "pr#7: waiting — {}",
        line.trim_start_matches("waiting: pr#7 ")
    )));

    // The same reason at the same head: said again, logged no more.
    let (report, log) = w.tick(false);
    assert_eq!(report.waiting_lines(), vec![line]);
    assert!(log.entries().is_empty(), "{:?}", log.entries());

    // Another reason: logged again.
    pr.reviews = vec![review("coordinator-login", "APPROVED", "older", "ok")];
    w.open(pr.clone());
    let (report, log) = w.tick(false);
    assert_eq!(
        report.waiting_lines(),
        vec![
            "waiting: pr#7 no coordinator approval at head-7: coordinator-login approved older, \
             an older head (no approval at this head)"
                .to_string()
        ]
    );
    assert_eq!(log.entries().len(), 1);
    assert_eq!(
        (w.tracker.writes().len(), w.github_writes().len()),
        writes,
        "waiting writes nothing to GitHub or the tracker"
    );

    // A dry run says it too, and keeps nothing.
    let (report, log) = w.tick(true);
    assert_eq!(report.waiting.len(), 1);
    assert!(log.entries().is_empty(), "{:?}", log.entries());
}

#[test]
fn a_held_pr_says_what_holds_it() {
    let w = World::new();
    let mut draft = green(pull_request(7));
    draft.draft = true;
    w.ticket("ABC-7", Status::InProgress, &draft);
    w.open(draft);
    let mut red = green(pull_request(8));
    red.checks[0].conclusion = Some("FAILURE".into());
    w.ticket("ABC-8", Status::InProgress, &red);
    w.open(red);
    let silent = green(pull_request(9));
    w.ticket("ABC-9", Status::Backlog, &silent);
    w.open(silent);
    let (report, _) = w.tick(false);
    assert_eq!(
        report.waiting_lines(),
        vec![
            "waiting: pr#7 it is a draft",
            "waiting: pr#8 checks are red at head-8 (`required-check`)",
            "waiting: pr#9 its ticket ABC-9 is Backlog, not In Progress or In Review",
        ]
    );
}

#[test]
fn a_ready_pr_left_for_the_next_tick_says_so() {
    let w = World::new();
    let prs: Vec<PullRequest> = [7, 8].into_iter().map(|n| in_review(&w, n)).collect();
    for pr in prs {
        w.open(approved(pr));
    }
    let (report, _) = w.tick(false);
    assert_eq!(
        report.waiting_lines(),
        vec![
            "waiting: pr#8 ready, but a PR was merged this tick: one merge per repository per tick"
        ]
    );
}

// ---- BR-ASDLC-018: the consolidation starts only after its angles ----

/// The round's consolidation item and its angle sub-items, by key.
fn round_keys(w: &World, ticket: &str, pr: &PullRequest) -> (String, Vec<String>) {
    let mut consolidation = None;
    let mut angles = Vec::new();
    for c in round(w, ticket, pr) {
        match texts::bridge_item(&c.description, c.parent.as_deref()) {
            Some(BridgeItem::Consolidation { .. }) => consolidation = Some(c.key),
            Some(BridgeItem::Angle { .. }) => angles.push(c.key),
            _ => {}
        }
    }
    assert!(!angles.is_empty());
    (consolidation.expect("a consolidation item"), angles)
}

fn blockers(w: &World, key: &str) -> Vec<String> {
    let mut b: Vec<String> = crate::tracker::Tracker::relations(&w.tracker, key)
        .unwrap()
        .blocked_by
        .into_iter()
        .map(|r| r.other)
        .collect();
    b.sort();
    b
}

#[test]
fn the_consolidation_item_is_blocked_by_every_angle_of_its_round_once() {
    let w = World::new();
    let pr = in_review(&w, 7);
    let (consolidation, mut angles) = round_keys(&w, "ABC-7", &pr);
    angles.sort();
    assert_eq!(blockers(&w, &consolidation), angles);
    let relations: Vec<String> = w
        .tracker
        .writes()
        .into_iter()
        .filter(|l| l.starts_with("relate"))
        .collect();
    assert_eq!(relations.len(), angles.len(), "{relations:?}");
    let writes = w.tracker.writes();
    let last_relate = writes
        .iter()
        .rposition(|l| l.starts_with("relate"))
        .unwrap();
    let round_comment = writes
        .iter()
        .position(|l| l.starts_with("comment ABC-7"))
        .unwrap();
    assert!(
        last_relate < round_comment,
        "the relations before the round comment: {writes:?}"
    );
    quiet(&w);
}

#[test]
fn a_round_missing_its_relations_gets_them_at_the_next_tick() {
    let w = World::new();
    let pr = in_review(&w, 7);
    let (consolidation, mut angles) = round_keys(&w, "ABC-7", &pr);
    angles.sort();
    // A round opened before the relations existed, or one lost by hand.
    for r in crate::tracker::Tracker::relations(&w.tracker, &consolidation)
        .unwrap()
        .blocked_by
    {
        crate::tracker::Tracker::unrelate(&w.tracker, &r.id).unwrap();
    }
    assert!(blockers(&w, &consolidation).is_empty());
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert_eq!(blockers(&w, &consolidation), angles);
    quiet(&w);
}

#[test]
fn a_relation_that_cannot_be_written_holds_the_round_and_is_retried() {
    let w = World::new();
    let pr = green(pull_request(7));
    w.ticket("ABC-7", Status::InProgress, &pr);
    w.open(pr.clone());
    w.tracker.fail_writes_starting_with("relate");
    let (report, _) = w.tick(false);
    assert!(!report.ok(), "the failure is reported");
    assert_eq!(
        w.tracker.item("ABC-7").state.status,
        Some(Status::InProgress),
        "the round does not open without its relations"
    );
    w.tracker.state.borrow_mut().failing_writes.clear();
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    let (consolidation, mut angles) = round_keys(&w, "ABC-7", &pr);
    angles.sort();
    assert_eq!(blockers(&w, &consolidation), angles);
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::InReview));
    quiet(&w);
}

#[test]
fn an_approval_while_an_angle_is_open_does_not_merge_and_says_why_once() {
    let w = World::new();
    let pr = in_review(&w, 7);
    check_every_box(&w, "ABC-7");
    let (_, angles) = round_keys(&w, "ABC-7", &pr);
    // The coordinator approved before any angle reviewer submitted.
    let mut early = pr.clone();
    early.reviews = vec![review("coordinator-login", "APPROVED", &pr.head_sha, "ok")];
    w.open(early);
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    let writes = w.github_writes();
    assert!(!writes.iter().any(|l| l.starts_with("merge")), "{writes:?}");
    let said: Vec<&String> = writes
        .iter()
        .filter(|l| l.contains("Waiting for the angle reviews"))
        .collect();
    assert_eq!(said.len(), 1, "{writes:?}");
    assert!(angles.iter().all(|a| said[0].contains(a.as_str())));
    quiet(&w);

    // The angles report: the same approval now counts.
    w.open(approved(&w, pr));
    w.tick(false);
    assert!(w.github_writes().iter().any(|l| l.starts_with("merge")));
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::ToDeploy));
}

#[test]
fn a_dry_run_says_the_relations_it_would_write() {
    let w = World::new();
    let pr = green(pull_request(7));
    w.ticket("ABC-1", Status::InProgress, &pr);
    w.open(pr);
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
        now: crate::github::Timestamp::parse("2026-10-05T12:00:00Z").unwrap(),
        deadline: None,
        dry_run: true,
    };
    let mut log = ActionLog::in_memory();
    run(&t, &mut log);
    assert!(w.tracker.writes().is_empty());
    let lines = intents.lines();
    let relations: Vec<&String> = lines
        .iter()
        .filter(|l| l.contains(" Blocks ") && l.contains("Consolidation"))
        .collect();
    assert!(!relations.is_empty(), "{lines:?}");
}

// ---- spec 04 §3: the coordinator receives the consolidation item ----

fn coordinator() -> crate::tracker::User {
    crate::tracker::User {
        id: "u-coordinator".into(),
        name: "coordinator-handle".into(),
    }
}

fn consolidation(w: &World, ticket: &str, pr: &PullRequest) -> crate::tracker::Item {
    let (key, _) = round_keys(w, ticket, pr);
    w.tracker.item(&key)
}

#[test]
fn a_pr_rounds_consolidation_item_goes_to_its_coordinator_once() {
    let w = World::new();
    assert_eq!(
        w.config.review.coordinator_assignee.as_deref(),
        Some("coordinator-handle")
    );
    w.tracker.state.borrow_mut().users.push(coordinator());
    let pr = in_review(&w, 7);
    assert_eq!(
        consolidation(&w, "ABC-7", &pr).assignee,
        Some(coordinator())
    );
    let (_, angles) = round_keys(&w, "ABC-7", &pr);
    for a in angles {
        assert_eq!(w.tracker.item(&a).assignee, None, "an angle has its own");
    }
    quiet(&w);
}

#[test]
fn without_a_coordinator_assignee_the_consolidation_item_is_unassigned() {
    let mut w = World::new();
    w.config.review.coordinator_assignee = None;
    w.tracker.state.borrow_mut().users.push(coordinator());
    let pr = in_review(&w, 7);
    assert_eq!(consolidation(&w, "ABC-7", &pr).assignee, None);
    quiet(&w);
}

#[test]
fn an_unknown_coordinator_leaves_the_item_unassigned_and_it_is_never_reassigned() {
    let w = World::new();
    let pr = in_review(&w, 7);
    let item = consolidation(&w, "ABC-7", &pr);
    assert_eq!(item.assignee, None, "the tracker does not know the name");
    quiet(&w);
    // The participant now exists: the existing item stays as it is.
    w.tracker.state.borrow_mut().users.push(coordinator());
    quiet(&w);
    assert_eq!(w.tracker.item(&item.key).assignee, None);
    // Someone takes it: the bridge leaves it with them.
    let taker = crate::tracker::User {
        id: "u-taker".into(),
        name: "taker-handle".into(),
    };
    crate::tracker::Tracker::assign(&w.tracker, &item.key, Some(&taker)).unwrap();
    quiet(&w);
    assert_eq!(w.tracker.item(&item.key).assignee, Some(taker));
}
