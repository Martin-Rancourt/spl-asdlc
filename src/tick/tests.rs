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
    let spec = include_str!("../../docs/spec/08-configuration.md");
    let start = spec.find("```toml\n").unwrap() + "```toml\n".len();
    let end = start + spec[start..].find("```").unwrap();
    Config::parse(&spec[start..end]).unwrap()
}

struct World {
    config: Config,
    github: FakeGitHub,
    tracker: FakeTracker,
    signal: Recorder,
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
        let mut log = ActionLog::in_memory();
        let report = run(&t, &mut log);
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

    // The same facts, a second tick: nothing written, nothing logged.
    let before = (w.tracker.writes().len(), w.github_writes().len());
    let (report, log) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(log.entries().is_empty(), "{:?}", log.entries());
    assert_eq!((w.tracker.writes().len(), w.github_writes().len()), before);
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
    for mut pr in prs {
        pr.reviews = vec![review("coordinator-login", "APPROVED", &pr.head_sha, "ok")];
        w.open(pr);
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
        &texts::merged_comment(&pr.url, "main", None, "To Deploy"),
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
    let mut pr = in_review(&w, 7);
    pr.reviews = vec![review("coordinator-login", "APPROVED", &pr.head_sha, "ok")];
    w.open(pr);
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
    for mut pr in prs {
        pr.reviews = vec![review("coordinator-login", "APPROVED", &pr.head_sha, "ok")];
        w.open(pr);
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
    pr.reviews = vec![review("coordinator-login", "APPROVED", &pr.head_sha, "ok")];
    w.open(pr);
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
