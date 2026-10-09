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

    // The same facts, a second tick: nothing written, nothing logged.
    let before = (w.tracker.writes().len(), w.github_writes().len());
    let (report, log) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(log.entries().is_empty(), "{:?}", log.entries());
    assert_eq!((w.tracker.writes().len(), w.github_writes().len()), before);
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
    let mut pr = in_review(&w, 7);
    check_every_box(&w, "ABC-7");
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

fn approved(mut pr: PullRequest) -> PullRequest {
    pr.reviews = vec![review("coordinator-login", "APPROVED", &pr.head_sha, "ok")];
    pr
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

/// A second tick on unchanged facts writes nothing, anywhere.
fn quiet(w: &World) {
    let before = (w.tracker.writes().len(), w.github_writes().len());
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
    w.open(approved(pr.clone()));
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
    w.open(approved(pr.clone()));
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
    w.open(approved(pr.clone()));
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

    w.open(approved(pr.clone()));
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
    w.open(approved(a.clone()));
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
    w.open(approved(b.clone()));
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
    w.open(approved(a));
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
    w.open(approved(pr.clone()));
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
