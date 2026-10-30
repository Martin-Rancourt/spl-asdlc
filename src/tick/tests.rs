//! The tick against the in-memory GitHub and tracker: the PR pass and the
//! sweep, end to end, and the promise that a second tick on unchanged facts
//! writes nothing, anywhere.

use super::*;
use crate::action_log::Outcome;
use crate::config::Config;
use crate::github::fake::{FakeGitHub, METHOD_REF, METHOD_REPO, pull_request};
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
        github.pin_method(&repo(), METHOD_REF);
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
        }
        let tracker = FakeTracker::new("ABC");
        tracker.seed_angle_labels(&config());
        World {
            config: config(),
            github,
            tracker,
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
            method_texts: Default::default(),
            low_quota: false,
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
        id: None,
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
        writes.first().unwrap(),
        "status ABC-1 In Review",
        "In Review when the PR is opened, before its round: {writes:?}"
    );
    assert_eq!(
        writes.iter().filter(|x| x.contains("In Review")).count(),
        1,
        "{writes:?}"
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
            "The method's texts at `{}`, the version of spl-asdlc this project's workflows are pinned on, copied by the bridge (spl-asdlc {})",
            crate::github::fake::METHOD_AT,
            texts::protocol::VERSION
        )) && consolidation.description.contains("### Consolidate"),
        "{}",
        consolidation.description
    );
    quiet(&w);
}

#[test]
fn a_new_angle_sub_item_embeds_the_pr_review_protocol_and_links_the_projects_notes() {
    // The notes come from the example of spec 08, parsed: a review angle's
    // `notes` is read from the file and linked from its sub-item.
    let w = World::new();
    assert_eq!(
        w.config.review.angles[0].notes.as_deref(),
        Some(".spl-asdlc/angles/maintainability.md")
    );
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

// ---- BR-ASDLC-032: the texts of the version the project is pinned on ----

/// The method's protocols and angle texts at `METHOD_REPO@rev`, each
/// carrying `word`.
fn method_at(w: &World, rev: &str, word: &str) {
    use crate::method::texts::protocol::{Job, angle_path};
    let method = RepoSlug::parse(METHOD_REPO).unwrap();
    let mut s = w.github.state.borrow_mut();
    let mut put = |path: String, text: String| {
        s.files
            .insert((method.clone(), rev.to_string(), path), Ok(text));
    };
    put(
        Job::PrAngle.path().into(),
        format!("# Review a PR from one angle\n\nThe {word} PR-review protocol."),
    );
    put(
        Job::PrConsolidation.path().into(),
        format!("# Consolidate\n\nThe {word} consolidation protocol."),
    );
    for key in ["maintainability", "security"] {
        put(
            angle_path(key),
            format!("# Angle: {key}\n\nThe {word} {key} text."),
        );
    }
}

fn round_items(w: &World, key: &str) -> Vec<crate::tracker::Item> {
    children_of(w, key)
        .into_iter()
        .filter(|c| {
            matches!(
                texts::bridge_item(&c.description, c.parent.as_deref()),
                Some(BridgeItem::Angle { .. } | BridgeItem::Consolidation { .. })
            )
        })
        .collect()
}

#[test]
fn a_rounds_items_carry_the_texts_of_the_version_the_projects_callers_pin() {
    let w = World::new();
    w.github.pin_caller(&repo(), METHOD_REPO, "v1.3.0");
    method_at(&w, "v1.3.0", "pinned");
    let pr = green(pull_request(7));
    w.ticket("ABC-1", Status::InProgress, &pr);
    w.open(pr.clone());
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    let items = round_items(&w, "ABC-1");
    assert_eq!(items.len(), 2, "{items:?}");
    for i in &items {
        let d = &i.description;
        assert!(
            d.contains("The method's texts at `example-org/spl-asdlc@v1.3.0`"),
            "{d}"
        );
        // Never this build's own copy.
        assert!(
            !d.contains("### Review a PR from one angle\n\n**Input**"),
            "{d}"
        );
        assert!(!d.contains("### Consolidate\n\n**Input**"), "{d}");
    }
    let angle = items
        .iter()
        .find(|i| i.title.starts_with("Maintainability review"))
        .unwrap();
    assert!(angle.description.contains("The pinned PR-review protocol."));
    assert!(
        angle
            .description
            .contains("The pinned maintainability text.")
    );
    let consolidation = items
        .iter()
        .find(|i| i.title.starts_with("Consolidation"))
        .unwrap();
    assert!(
        consolidation
            .description
            .contains("The pinned consolidation protocol.")
    );
    // Read with the project's token, from the method's repository the
    // callers name, at their ref.
    let reads = w.github.state.borrow().reads_from.clone();
    assert!(
        reads
            .iter()
            .all(|r| r.starts_with("example-org/spl-asdlc@v1.3.0 ")
                && r.ends_with(" via example-org/acme")),
        "{reads:?}"
    );
}

#[test]
fn two_projects_pinned_on_two_versions_each_get_their_own_texts_read_once_per_tick() {
    use crate::method::texts::protocol::Job;
    let w = World::new();
    let beta = RepoSlug::parse("example-org/beta").unwrap();
    w.github.pin_caller(&repo(), METHOD_REPO, "v1.2.0");
    w.github.pin_caller(&beta, METHOD_REPO, "v1.3.0");
    method_at(&w, "v1.2.0", "older");
    method_at(&w, "v1.3.0", "newer");
    // One bridge, one GitHub client: a tick per repository.
    let tick_of = |r: &RepoSlug| Tick {
        config: &w.config,
        repo: r.clone(),
        github: &w.github,
        tracker: &w.tracker,
        signal: &w.signal,
        github_login: bot("spl-asdlc"),
        now: crate::github::Timestamp::parse("2026-10-05T12:00:00Z").unwrap(),
        deadline: None,
        dry_run: false,
        method_texts: Default::default(),
        low_quota: false,
    };
    let (acme, b) = (tick_of(&repo()), tick_of(&beta));
    let older = method_texts::texts(&acme, "main", Job::PrAngle, Some("security")).unwrap();
    let newer = method_texts::texts(&b, "main", Job::PrAngle, Some("security")).unwrap();
    assert_eq!(older.at, "example-org/spl-asdlc@v1.2.0");
    assert!(older.protocol.contains("The older PR-review protocol."));
    assert_eq!(
        older.angle.as_deref(),
        Some("# Angle: security\n\nThe older security text.")
    );
    assert_eq!(newer.at, "example-org/spl-asdlc@v1.3.0");
    assert!(newer.protocol.contains("The newer PR-review protocol."));
    assert!(newer.angle.unwrap().contains("The newer security text."));
    // A second item of the same tick reads nothing again; the protocol
    // already read is shared with another angle's.
    assert_eq!(
        method_texts::texts(&acme, "main", Job::PrAngle, Some("security")).unwrap(),
        older
    );
    method_texts::texts(&acme, "main", Job::PrAngle, Some("maintainability")).unwrap();
    let reads = w.github.state.borrow().reads_from.clone();
    let of_acme: Vec<_> = reads
        .iter()
        .filter(|r| r.ends_with("via example-org/acme"))
        .collect();
    assert_eq!(of_acme.len(), 3, "{reads:?}");
    assert!(of_acme.iter().all(|r| r.contains("@v1.2.0 ")), "{reads:?}");
    // An angle the pinned version has no text for: a project's own.
    let own = method_texts::texts(&acme, "main", Job::PrAngle, Some("accessibility")).unwrap();
    assert_eq!((own.angle, own.retired), (None, None));
    // The next tick reads again.
    let again = tick_of(&repo());
    method_texts::texts(&again, "main", Job::PrAngle, Some("security")).unwrap();
    assert_eq!(
        w.github
            .state
            .borrow()
            .reads_from
            .iter()
            .filter(|r| r.ends_with("via example-org/acme"))
            .count(),
        6
    );
}

#[test]
fn texts_that_cannot_be_read_open_no_round_and_are_said_once_never_replaced() {
    let w = World::new();
    // Pinned on a ref the method's repository does not hold (or a private
    // repository the token cannot see: GitHub says 404 for both).
    w.github.pin_caller(&repo(), METHOD_REPO, "v9.9.9");
    let pr = green(pull_request(7));
    w.ticket("ABC-1", Status::InProgress, &pr);
    w.open(pr.clone());
    for _ in 0..2 {
        let (report, _) = w.tick(false);
        assert!(!report.ok(), "{report:?}");
        let said = report.failures.join("; ");
        assert!(
            said.contains("the method's texts for the review round of")
                && said.contains("docs/agents/review-pr.md is not at example-org/spl-asdlc@v9.9.9")
                && said.contains("docs/setup/github-app.md"),
            "{said}"
        );
    }
    assert!(
        round_items(&w, "ABC-1").is_empty(),
        "no item, so no fallback"
    );
    let comments = w.tracker.comments("ABC-1").unwrap();
    let notices: Vec<_> = comments
        .iter()
        .filter(|c| c.body.contains("spl-asdlc:method-texts"))
        .collect();
    assert_eq!(notices.len(), 1, "said once: {comments:?}");
    assert!(
        notices[0]
            .body
            .contains("It never copies other texts instead"),
        "{}",
        notices[0].body
    );
    // Once the texts can be read, the round opens with them.
    method_at(&w, "v9.9.9", "late");
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    let items = round_items(&w, "ABC-1");
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|i| i.description.contains("@v9.9.9`")));
}

#[test]
fn without_a_caller_of_the_method_no_version_is_told_and_no_round_opens() {
    let w = World::new();
    w.github.state.borrow_mut().everywhere.clear();
    let pr = green(pull_request(7));
    w.ticket("ABC-1", Status::InProgress, &pr);
    w.open(pr.clone());
    let (report, _) = w.tick(false);
    let said = report.failures.join("; ");
    assert!(
        said.contains("calls a reusable workflow of the method"),
        "{said}"
    );
    assert!(round_items(&w, "ABC-1").is_empty());
}

#[test]
fn a_draft_pr_on_a_ticket_nobody_marked_started_writes_nothing() {
    // In Progress is the implementer's: the bridge is no backup for it.
    let w = World::new();
    let mut pr = green(pull_request(7));
    pr.draft = true;
    w.ticket("ABC-1", Status::Ready, &pr);
    w.open(pr);
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert_eq!(w.tracker.item("ABC-1").state.status, Some(Status::Ready));
    assert!(w.tracker.writes().is_empty(), "{:?}", w.tracker.writes());
    assert!(children_of(&w, "ABC-1").is_empty(), "no round for a draft");
    quiet(&w);
}

#[test]
fn a_draft_pr_keeps_its_ticket_in_progress() {
    let w = World::new();
    let mut pr = green(pull_request(7));
    pr.draft = true;
    w.ticket("ABC-1", Status::InProgress, &pr);
    w.open(pr);
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert_eq!(
        w.tracker.item("ABC-1").state.status,
        Some(Status::InProgress)
    );
    assert!(children_of(&w, "ABC-1").is_empty(), "no round for a draft");
    assert!(w.tracker.writes().is_empty(), "{:?}", w.tracker.writes());
    quiet(&w);
}

#[test]
fn an_opened_pr_moves_its_ticket_to_in_review_before_its_ci() {
    let w = World::new();
    let pr = pull_request(7); // no checks yet
    w.ticket("ABC-1", Status::InProgress, &pr);
    w.open(pr.clone());
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert_eq!(w.tracker.item("ABC-1").state.status, Some(Status::InReview));
    assert!(
        children_of(&w, "ABC-1").is_empty(),
        "the round waits for green CI"
    );
    quiet(&w);
    // CI turns green: the round opens, the ticket is not written In Review again.
    w.open(green(pr));
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(!children_of(&w, "ABC-1").is_empty());
    assert_eq!(
        w.tracker
            .writes()
            .iter()
            .filter(|x| x.contains("In Review"))
            .count(),
        1,
        "{:?}",
        w.tracker.writes()
    );
}

#[test]
fn a_ready_or_backlog_ticket_whose_pr_is_opened_goes_straight_to_in_review() {
    for from in [Status::Ready, Status::Backlog] {
        let w = World::new();
        let pr = pull_request(7);
        w.ticket("ABC-1", from, &pr);
        w.open(pr);
        w.tick(false);
        assert_eq!(
            w.tracker.item("ABC-1").state.status,
            Some(Status::InReview),
            "{from:?}"
        );
        assert_eq!(
            w.tracker.writes(),
            vec!["status ABC-1 In Review"],
            "{from:?}: no In Progress on the way"
        );
        quiet(&w);
    }
}

#[test]
fn a_pr_put_back_in_draft_is_out_of_the_flow_and_writes_nothing() {
    let w = World::new();
    let mut pr = pull_request(7);
    w.ticket("ABC-1", Status::InProgress, &pr);
    w.open(pr.clone());
    w.tick(false);
    assert_eq!(w.tracker.item("ABC-1").state.status, Some(Status::InReview));
    let before = (w.tracker.writes().len(), w.github_writes().len());
    pr.draft = true;
    w.open(pr.clone());
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert_eq!(
        report.waiting_lines(),
        vec!["waiting: pr#7 it is a draft, out of the flow until it is marked ready"]
    );
    assert_eq!(
        w.tracker.item("ABC-1").state.status,
        Some(Status::InReview),
        "nothing written from a draft"
    );
    assert_eq!(
        (w.tracker.writes().len(), w.github_writes().len()),
        before,
        "{:?}",
        &w.tracker.writes()[before.0..]
    );
    quiet(&w);
    // Ready again: back in the flow, where it left it.
    pr.draft = false;
    w.open(green(pr.clone()));
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(!round(&w, "ABC-1", &pr).is_empty(), "its round opens");
    assert_eq!(w.tracker.item("ABC-1").state.status, Some(Status::InReview));
}

#[test]
fn a_pr_put_in_draft_during_its_round_writes_nothing_and_its_round_goes_on_once_ready() {
    let w = World::new();
    let mut pr = in_review(&w, 7);
    check_every_box(&w, "ABC-7");
    let items = round(&w, "ABC-7", &pr);
    let before = (w.tracker.writes().len(), w.github_writes().len());
    pr.draft = true;
    // A draft's checks are not read: red, conflicting and approved, it is
    // neither told of its conflict nor merged.
    let mut draft = approved(&w, pr.clone());
    draft.checks.clear();
    draft.mergeable = crate::github::Mergeable::Conflicting;
    w.open(draft);
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert_eq!(
        (w.tracker.writes().len(), w.github_writes().len()),
        before,
        "{:?} {:?}",
        &w.tracker.writes()[before.0..],
        &w.github_writes()[before.1..]
    );
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::InReview));
    assert_eq!(round(&w, "ABC-7", &pr), items, "its round stays as it is");
    // Marked ready: the round goes on and the PR merges.
    pr.draft = false;
    w.open(approved(&w, pr.clone()));
    w.tick(false);
    assert!(
        w.github_writes()
            .contains(&"merge example-org/acme#7 at head-7 squash".to_string())
    );
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::ToDeploy));
}

#[test]
fn a_draft_pr_on_a_ticket_past_the_merge_leaves_it_where_it_is() {
    for status in [Status::ToDeploy, Status::Done] {
        let w = World::new();
        let all = COMPLETE.replace("- [ ] it is fast", "- [x] it is fast");
        w.tracker.seed("ABC-7", "A ticket", &all, status, &[]);
        let mut pr = green(pull_request(8));
        pr.draft = true;
        w.tracker.link("ABC-7", &pr.url);
        w.open(pr);
        let (report, _) = w.tick(false);
        assert!(report.ok(), "{report:?}");
        assert_eq!(w.tracker.item("ABC-7").state.status, Some(status));
        assert!(w.tracker.writes().is_empty(), "{:?}", w.tracker.writes());
        assert!(w.github_writes().is_empty(), "{:?}", w.github_writes());
    }
}

#[test]
fn a_draft_pr_counts_for_nothing_among_its_tickets_prs() {
    // BR-ASDLC-008: the merge of the ticket's other PR, every box ticked,
    // puts it To Deploy; a draft PR of the ticket does not hold it.
    let w = World::new();
    let pr = in_review(&w, 7);
    check_every_box(&w, "ABC-7");
    let mut draft = green(pull_request(8));
    draft.draft = true;
    w.tracker.link("ABC-7", &draft.url);
    w.open(draft);
    w.open(approved(&w, pr.clone()));
    w.tick(false);
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::ToDeploy));
    assert!(merge_comment(&w, "ABC-7", &pr).contains("state: To Deploy"));
}

/// One run of `name` in the `pr` workflow, completed with `conclusion`.
fn pr_run(name: &str, conclusion: &str, started: &str) -> CheckEntry {
    CheckEntry {
        kind: CheckKind::Run,
        name: name.into(),
        workflow: Some("pr".into()),
        status: Some("COMPLETED".into()),
        conclusion: Some(conclusion.into()),
        started_at: crate::github::Timestamp::parse(started),
        details_url: None,
    }
}

#[test]
fn a_ready_pr_whose_required_check_is_the_draft_runs_skipped_one_waits_then_merges() {
    // On a draft no job runs: GitHub reports every job skipped, and a
    // skipped job passes a required check. Marked ready, the PR waits for
    // the ready run's own success on the same head.
    let mut w = World::new();
    no_review(&mut w);
    let mut pr = ready_pr(&w);
    pr.checks = vec![
        pr_run("guards", "SKIPPED", "2026-10-15T10:00:00Z"),
        pr_run("required-check", "SKIPPED", "2026-10-15T10:00:00Z"),
    ];
    w.open(pr.clone());
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(!merged(&w), "{:?}", w.github_writes());
    assert_eq!(
        report.waiting_lines(),
        vec![
            "waiting: pr#7 the required check `required-check` was skipped at head-7: only a success counts"
        ]
    );
    // The ready run's first job succeeded; its required-check has not.
    pr.checks
        .push(pr_run("guards", "SUCCESS", "2026-10-15T10:05:00Z"));
    w.open(pr.clone());
    w.tick(false);
    assert!(!merged(&w), "{:?}", w.github_writes());
    // The ready run's required-check succeeded: the PR merges.
    pr.checks
        .push(pr_run("required-check", "SUCCESS", "2026-10-15T10:06:00Z"));
    w.open(pr);
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(merged(&w), "{:?}", w.github_writes());
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::ToDeploy));
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
        Some(Status::InReview),
        "opened: In Review, the round waiting for the contract"
    );
}

/// SID-1222: the hold is said on the ticket too, once per set of missing
/// parts, and nothing more once the contract is complete.
#[test]
fn an_incomplete_contract_is_said_once_on_the_ticket_too() {
    let w = World::new();
    let pr = green(pull_request(7));
    w.tracker
        .seed("ABC-1", "A ticket", "## Goal\nx\n", Status::InProgress, &[]);
    w.tracker.link("ABC-1", &pr.url);
    w.open(pr);
    let held = |w: &World| -> Vec<String> {
        w.tracker
            .comments("ABC-1")
            .unwrap()
            .into_iter()
            .filter(|c| c.body.contains("PR #7 is held"))
            .map(|c| c.body)
            .collect()
    };
    w.tick(false);
    w.tick(false);
    let said = held(&w);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].contains("the contract of this ticket is incomplete"),
        "{}",
        said[0]
    );
    assert!(
        said[0].contains("spl-asdlc ticket check ABC-1"),
        "{}",
        said[0]
    );
    // What is missing changes: said again, once.
    w.tracker
        .update_description(
            "ABC-1",
            "## Goal\nx\n\n## Acceptance criteria\n- [x] done\n",
        )
        .unwrap();
    w.tick(false);
    w.tick(false);
    assert_eq!(held(&w).len(), 2, "{:?}", held(&w));
    // Complete: nothing more is said on the ticket.
    w.tracker
        .update_description(
            "ABC-1",
            "## Goal\nx\n\n## Acceptance criteria\n- [x] done\n\n## Business rules\nNone.\n",
        )
        .unwrap();
    w.tick(false);
    w.tick(false);
    assert_eq!(held(&w).len(), 2, "{:?}", held(&w));
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

/// A PR whose branch names its ticket (`<type>/<ticket-id>-<description>`,
/// any case) is linked to it with no tracker link: the ticket goes In Review
/// and the bridge lays the tracker's link once (ASDLC-VW9U).
#[test]
fn a_pr_named_by_its_branch_is_linked_and_its_ticket_goes_in_review() {
    let w = World::new();
    w.tracker
        .seed("ABC-12", "A ticket", COMPLETE, Status::InProgress, &[]);
    let mut pr = pull_request(7);
    pr.head_ref = "fix/abc-12-mcp-settings".into();
    pr.title = "fix(mcp): settings (ABC-12)".into();
    w.open(pr.clone());
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert_eq!(
        w.tracker.item("ABC-12").state.status,
        Some(Status::InReview)
    );
    assert_eq!(
        w.tracker.items_linked_to_pr(&pr.url).unwrap(),
        vec!["ABC-12"]
    );
    assert!(
        !w.github_writes().iter().any(|l| l.starts_with("comment")),
        "no « no ticket » comment: {:?}",
        w.github_writes()
    );
    let links = |w: &World| {
        w.tracker
            .writes()
            .iter()
            .filter(|x| x.contains("link"))
            .count()
    };
    let laid = links(&w);
    w.tick(false);
    assert_eq!(links(&w), laid, "the link is laid once");
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
    assert_eq!(w.tracker.item("ABC-1").state.status, Some(Status::InReview));
    assert!(children_of(&w, "ABC-1").is_empty(), "no round yet");
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

/// A ticket whose PR is opened always goes through In Review (BR-ASDLC-037):
/// with no review configured, one pass writes In Review, merges and writes
/// To Deploy.
#[test]
fn without_a_review_one_pass_writes_in_review_merges_and_writes_to_deploy() {
    let mut w = World::new();
    w.config.review.angles = Vec::new();
    let pr = green(pull_request(7));
    w.ticket("ABC-7", Status::InProgress, &pr);
    check_every_box(&w, "ABC-7");
    w.open(pr);
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(
        w.github_writes()
            .contains(&"merge example-org/acme#7 at head-7 squash".to_string()),
        "{:?}",
        w.github_writes()
    );
    let statuses: Vec<String> = w
        .tracker
        .writes()
        .into_iter()
        .filter(|l| l.starts_with("status ABC-7 "))
        .collect();
    assert_eq!(
        statuses,
        vec!["status ABC-7 In Review", "status ABC-7 To Deploy"],
        "{:?}",
        w.tracker.writes()
    );
}

/// The In Review write failed: the PR is not merged past it, so the
/// ticket never skips In Review (BR-ASDLC-037).
#[test]
fn a_failed_in_review_write_holds_the_merge() {
    let mut w = World::new();
    w.config.review.angles = Vec::new();
    let pr = green(pull_request(7));
    w.ticket("ABC-7", Status::InProgress, &pr);
    check_every_box(&w, "ABC-7");
    w.open(pr);
    w.tracker
        .fail_writes_starting_with("status ABC-7 In Review");
    w.tick(false);
    assert!(
        !w.github_writes().iter().any(|l| l.starts_with("merge ")),
        "{:?}",
        w.github_writes()
    );
    assert_eq!(
        w.tracker.item("ABC-7").state.status,
        Some(Status::InProgress)
    );
}

/// GitHub reports `SUCCESS` next to a status still `IN_PROGRESS` (hung
/// post-job steps): the conclusion wins and the PR is merged (ASDLC-CPSK).
#[test]
fn a_required_check_that_succeeded_while_still_in_progress_merges() {
    let w = World::new();
    let mut pr = in_review(&w, 7);
    check_every_box(&w, "ABC-7");
    let angle = children_of(&w, "ABC-7")
        .into_iter()
        .find(|c| c.title.starts_with("Maintainability"))
        .unwrap();
    pr.checks[0].status = Some("IN_PROGRESS".into());
    pr.reviews = vec![
        review(
            "angle-agent",
            "COMMENTED",
            &pr.head_sha,
            &format!("Reviewed for {}.", angle.key),
        ),
        review("coordinator-login", "APPROVED", &pr.head_sha, "LGTM"),
    ];
    w.open(pr);
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(
        w.github_writes()
            .contains(&"merge example-org/acme#7 at head-7 squash".to_string()),
        "{:?}",
        w.github_writes()
    );
}

/// A wait on a running check names it, so a stuck PR is diagnosed without
/// GitHub's API.
#[test]
fn a_wait_on_a_running_check_names_it() {
    let w = World::new();
    let mut pr = in_review(&w, 7);
    check_every_box(&w, "ABC-7");
    pr.checks[0].status = Some("IN_PROGRESS".into());
    pr.checks[0].conclusion = None;
    w.open(pr);
    let (report, _) = w.tick(false);
    assert!(
        format!("{report:?}").contains("checks are still running at head-7: `required-check`"),
        "{report:?}"
    );
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
fn every_green_pr_is_merged_in_one_tick() {
    let mut w = World::new();
    w.config.review.require_up_to_date = false;
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
    assert_eq!(
        merges,
        vec![
            "merge example-org/acme#7 at head-7 squash",
            "merge example-org/acme#8 at head-8 squash"
        ]
    );
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
        method_texts: Default::default(),
        low_quota: false,
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
fn a_merge_is_swept_even_when_an_agent_mangled_the_consolidation_items_marker() {
    let w = World::new();
    let pr = in_review(&w, 9);
    check_every_box(&w, "ABC-9");
    let (consolidation, _) = round_keys(&w, "ABC-9", &pr);
    // An agent ticking a box rewrote the marker the tracker shows as
    // visible text: only its place under the ticket, linked to the same PR,
    // says what the item is.
    let mangled = w
        .tracker
        .item(&consolidation)
        .description
        .replace("spl-asdlc:", "spl-asdlc\\:");
    crate::tracker::Tracker::update_description(&w.tracker, &consolidation, &mangled).unwrap();
    let mut merged = pr.clone();
    merged.state = PrState::Merged;
    merged.merged_at = crate::github::Timestamp::parse("2026-10-05T10:00:00Z");
    {
        let mut s = w.github.state.borrow_mut();
        if let Some(Ok(list)) = s.open_pulls.get_mut(&repo()) {
            list.retain(|p| p.number != pr.number);
        }
        s.merged_pulls.insert(repo(), Ok(vec![merged]));
    }
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert_eq!(w.tracker.item("ABC-9").state.status, Some(Status::ToDeploy));
    assert_eq!(
        w.tracker.item(&consolidation).state.status,
        Some(Status::Ready),
        "never promoted as if it were the ticket"
    );
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
        &texts::merged_comment(
            &pr.url,
            "main",
            &texts::MergedBy::Bridge {
                approved_by: None,
                up_to_date: false,
            },
            "To Deploy",
            "the closure",
        ),
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
fn a_ticket_whose_link_and_marker_were_lost_is_found_by_its_comment_not_opened_twice() {
    let mut w = World::new();
    w.config.review.ticketless_authors = vec!["dependabot".into()];
    let pr = app_pr(7, "dependabot");
    let marker = texts::pr_ticket_marker(&pr.url);
    let (title, description) = texts::pr_ticket(7, &pr.title, &bot("dependabot"), &pr.url);
    // An agent rewrote its description without the marker line; the bridge's
    // identity comment stays.
    let edited: String = description.lines().skip(1).collect::<Vec<_>>().join("\n");
    assert!(!texts::has_marker(&edited, &marker));
    w.tracker
        .seed("ABC-9", &title, &edited, Status::Ready, &["ticketless-pr"]);
    w.tracker
        .comment("ABC-9", &texts::identity_comment(&marker))
        .unwrap();
    w.open(pr.clone());
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert_eq!(pr_ticket(&w, &pr).map(|i| i.key), Some("ABC-9".to_string()));
    assert!(
        !w.tracker.writes().iter().any(|l| l.starts_with("create")),
        "{:?}",
        w.tracker.writes()
    );
    assert!(texts::has_marker(
        &w.tracker.item("ABC-9").description,
        &marker
    ));
    let restored = w
        .tracker
        .writes()
        .iter()
        .filter(|l| l.as_str() == "describe ABC-9")
        .count();
    assert_eq!(restored, 1, "written back once: {:?}", w.tracker.writes());
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
    let comment = merge_comment(&w, "ABC-7", &pr);
    assert!(comment.contains("state: To Deploy"));
    assert!(
        comment.starts_with(&format!(
            "{} was green, approved by `coordinator-login` and up to date with `main`, so the bridge merged it.",
            pr.url
        )),
        "{comment}"
    );
    quiet(&w);
}

#[test]
fn the_merge_comment_says_up_to_date_only_when_the_project_requires_it() {
    let mut w = World::new();
    w.config.review.require_up_to_date = false;
    let pr = in_review(&w, 7);
    check_every_box(&w, "ABC-7");
    w.open(approved(&w, pr.clone()));
    w.tick(false);
    let comment = merge_comment(&w, "ABC-7", &pr);
    assert!(
        comment.starts_with(&format!(
            "{} was green and approved by `coordinator-login`, so the bridge merged it.",
            pr.url
        )),
        "{comment}"
    );
    assert!(!comment.contains("up to date"), "{comment}");
}

// ---- nothing to deploy (`tracker.no_deploy_label`) ----

#[test]
fn a_merge_of_a_ticket_with_nothing_to_deploy_is_done_once() {
    let w = World::new();
    let pr = in_review(&w, 7);
    check_every_box(&w, "ABC-7");
    crate::tracker::Tracker::add_label(&w.tracker, "ABC-7", "no-deploy").unwrap();
    w.open(approved(&w, pr.clone()));
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(w.github_writes().iter().any(|l| l.starts_with("merge")));
    assert_eq!(
        w.tracker.item("ABC-7").state.status,
        Some(Status::Done),
        "nothing to deploy: the merge delivered it"
    );
    let comment = merge_comment(&w, "ABC-7", &pr);
    assert!(comment.contains("state: Done"), "{comment}");
    quiet(&w);
}

#[test]
fn a_ticket_with_nothing_to_deploy_and_a_box_open_goes_back_to_work() {
    let w = World::new();
    let pr = in_review(&w, 7);
    crate::tracker::Tracker::add_label(&w.tracker, "ABC-7", "no-deploy").unwrap();
    w.open(approved(&w, pr));
    w.tick(false);
    assert_eq!(
        w.tracker.item("ABC-7").state.status,
        Some(Status::InProgress),
        "every box ticked first"
    );
}

#[test]
fn a_ticket_with_nothing_to_deploy_merged_outside_the_bridge_is_swept_to_done_once() {
    let mut w = World::new();
    w.config.tracker.no_deploy_label = "docs-only".into();
    let mut pr = pull_request(9);
    pr.state = PrState::Merged;
    pr.merged_at = crate::github::Timestamp::parse("2026-10-05T10:00:00Z");
    w.ticket("ABC-9", Status::InReview, &pr);
    check_every_box(&w, "ABC-9");
    crate::tracker::Tracker::add_label(&w.tracker, "ABC-9", "docs-only").unwrap();
    w.github
        .state
        .borrow_mut()
        .merged_pulls
        .insert(repo(), Ok(vec![pr]));
    w.tick(false);
    assert_eq!(w.tracker.item("ABC-9").state.status, Some(Status::Done));
    let after = w.tracker.writes().len();
    w.tick(false);
    assert_eq!(w.tracker.writes().len(), after, "once");
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
    // Every angle reported (BR-ASDLC-018): only the approval is in question.
    pr.reviews = angle_reviews(&w, &pr);
    pr.reviews
        .push(review(author, "APPROVED", &pr.head_sha, "ok"));
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
    w.ticket("ABC-9", Status::Triage, &silent);
    w.open(silent);
    let (report, _) = w.tick(false);
    assert_eq!(
        report.waiting_lines(),
        vec![
            "waiting: pr#7 it is a draft, out of the flow until it is marked ready",
            "waiting: pr#9 its ticket ABC-9 is Triage, not Backlog, Ready, In Progress or In Review",
        ]
    );
}

#[test]
fn no_green_pr_waits_for_an_artificial_merge_limit() {
    let mut w = World::new();
    w.config.review.require_up_to_date = false;
    let prs: Vec<PullRequest> = [7, 8].into_iter().map(|n| in_review(&w, n)).collect();
    for pr in prs {
        w.open(approved(&w, pr));
    }
    let (report, _) = w.tick(false);
    assert_eq!(report.waiting_lines(), Vec::<String>::new());
}

#[test]
fn a_project_requiring_up_to_date_branches_merges_one_pr_per_base_and_tick() {
    let w = World::new();
    assert!(
        w.config.review.require_up_to_date,
        "the spec's example opts in"
    );
    let prs: Vec<PullRequest> = [7, 8].into_iter().map(|n| in_review(&w, n)).collect();
    for pr in prs {
        w.open(approved(&w, pr));
    }
    let (report, _) = w.tick(false);
    let merges: Vec<String> = w
        .github_writes()
        .into_iter()
        .filter(|l| l.starts_with("merge"))
        .collect();
    assert_eq!(merges, vec!["merge example-org/acme#7 at head-7 squash"]);
    assert_eq!(
        report.waiting_lines(),
        vec![
            "waiting: pr#8 ready, but main moved this tick and the project requires an up-to-date branch"
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
    assert!(
        !w.tracker
            .writes()
            .iter()
            .any(|x| x.starts_with("comment ABC-7")),
        "the round does not open without its relations: {:?}",
        w.tracker.writes()
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

/// A round cut short, then an agent edited the markers of its items away:
/// they are found by their exact titles among the ticket's children and
/// never created twice (REQ-REL-048).
#[test]
fn a_round_item_whose_marker_was_mangled_is_found_by_its_title_and_not_recreated() {
    let w = World::new();
    let pr = green(pull_request(7));
    w.ticket("ABC-7", Status::InProgress, &pr);
    w.open(pr.clone());
    // Neither linked to the PR nor related: the next tick opens the round
    // again.
    w.tracker.fail_writes_starting_with("link");
    w.tracker.fail_writes_starting_with("relate");
    let (report, _) = w.tick(false);
    assert!(!report.ok(), "the round is cut short");
    let created = children_of(&w, "ABC-7");
    assert!(created.len() > 1, "the angles and the consolidation item");
    for item in &created {
        let mangled = item.description.replace("spl-asdlc:", "spl-asdlc :");
        crate::tracker::Tracker::update_description(&w.tracker, &item.key, &mangled).unwrap();
    }
    w.tracker.state.borrow_mut().failing_writes.clear();
    let before = w.tracker.writes().len();
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    let writes = w.tracker.writes()[before..].to_vec();
    assert!(
        !writes.iter().any(|l| l.starts_with("create")),
        "{writes:?}"
    );
    assert_eq!(children_of(&w, "ABC-7").len(), created.len());
    assert_eq!(
        w.tracker.item("ABC-7").state.status,
        Some(Status::InReview),
        "the round opens with the items it found: {writes:?}"
    );
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
    assert_eq!(
        report.waiting_lines(),
        vec![format!(
            "waiting: pr#7 its angle reviews are still open ({}): the approval at head-7 does not count yet",
            angles.join(", ")
        )],
        "the tick's report says why it waits (SID-1008)"
    );
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
        method_texts: Default::default(),
        low_quota: false,
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

// ---- spec 04 §11: a project with no PR review ----

/// The project configures no PR review: no angle, no size threshold, no
/// coordinator.
fn no_review(w: &mut World) {
    let r = &mut w.config.review;
    r.angles.clear();
    r.diff_size_extra_angle = 0;
    r.coordinators.clear();
    assert!(!r.angle_possible());
}

/// A green PR on a ticket whose every box is ticked, In Progress.
fn ready_pr(w: &World) -> PullRequest {
    let pr = green(pull_request(7));
    w.ticket("ABC-7", Status::InProgress, &pr);
    check_every_box(w, "ABC-7");
    w.open(pr.clone());
    pr
}

fn merged(w: &World) -> bool {
    w.github_writes()
        .contains(&"merge example-org/acme#7 at head-7 squash".to_string())
}

#[test]
fn a_pr_with_no_angle_merges_on_green_ci_with_no_round_nor_approval() {
    let mut w = World::new();
    no_review(&mut w);
    let pr = ready_pr(&w);
    let (report, log) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(merged(&w), "{:?}", w.github_writes());
    assert!(children_of(&w, "ABC-7").is_empty(), "no round, no sub-item");
    let writes = w.tracker.writes();
    assert_eq!(
        writes
            .iter()
            .filter(|x| x.starts_with("status"))
            .collect::<Vec<_>>(),
        ["status ABC-7 In Review", "status ABC-7 To Deploy"],
        "In Review when opened, as any PR: {writes:?}"
    );
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::ToDeploy));
    let comment = merge_comment(&w, "ABC-7", &pr);
    assert!(comment.contains("state: To Deploy"));
    assert!(
        comment.contains("was green and up to date with `main`; no review angle applies to it"),
        "{comment}"
    );
    assert!(
        !comment.contains("approved"),
        "no review, no approval: {comment}"
    );
    // The tick reports a merge with no review.
    let merge = log
        .entries()
        .iter()
        .find(|e| e.action.starts_with("merge"))
        .expect("the merge is logged");
    assert_eq!(merge.action, "merge (squash, no PR review)");
    assert_eq!(merge.inputs["approved_by"], serde_json::Value::Null);
    assert_eq!(merge.outcome, Outcome::Done);
    quiet(&w);
}

#[test]
fn a_pr_with_no_angle_and_a_red_ci_is_not_merged() {
    let mut w = World::new();
    no_review(&mut w);
    let mut pr = ready_pr(&w);
    pr.checks[0].conclusion = Some("FAILURE".into());
    w.open(pr);
    base_for_red(&w, "head-7", crate::github::CompareStatus::Ahead);
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(!merged(&w));
    assert!(children_of(&w, "ABC-7").is_empty());
    assert_eq!(
        w.tracker.item("ABC-7").state.status,
        Some(Status::InProgress)
    );
    quiet(&w);
}

#[test]
fn a_pr_with_no_angle_waits_for_its_branch_its_threads_and_a_request_for_changes() {
    let mut w = World::new();
    no_review(&mut w);
    // A behind branch is brought up to date first.
    let mut pr = ready_pr(&w);
    pr.merge_state = "BEHIND".into();
    w.open(pr.clone());
    w.tick(false);
    assert_eq!(
        w.github_writes(),
        vec!["update-branch example-org/acme#7 at head-7"]
    );
    // An open thread holds it.
    pr.merge_state = "CLEAN".into();
    pr.threads = vec![ReviewThread {
        resolved: false,
        outdated: false,
        path: None,
    }];
    w.open(pr.clone());
    w.tick(false);
    assert!(!merged(&w));
    // A named coordinator's standing request for changes holds it too.
    pr.threads.clear();
    w.config.review.coordinators = vec!["coordinator-login".into()];
    pr.reviews = vec![review(
        "coordinator-login",
        "CHANGES_REQUESTED",
        "older",
        "redo",
    )];
    w.open(pr.clone());
    w.tick(false);
    assert!(!merged(&w));
    pr.reviews.clear();
    w.open(pr);
    w.tick(false);
    assert!(merged(&w));
    assert!(children_of(&w, "ABC-7").is_empty());
}

#[test]
fn a_dry_run_shows_a_merge_with_no_pr_review() {
    let mut w = World::new();
    no_review(&mut w);
    ready_pr(&w);
    let before = (w.tracker.writes().len(), w.github_writes().len());
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
        method_texts: Default::default(),
        low_quota: false,
    };
    let mut log = ActionLog::in_memory();
    run(&t, &mut log);
    assert_eq!(
        (w.tracker.writes().len(), w.github_writes().len()),
        before,
        "a dry run writes nothing"
    );
    let lines: Vec<String> = log
        .entries()
        .iter()
        .map(crate::action_log::describe)
        .collect();
    assert!(
        lines
            .iter()
            .any(|l| l.contains("pr#7 ABC-7: merge (squash, no PR review) — would do")),
        "{lines:#?}"
    );
    assert!(
        intents
            .lines()
            .contains(&"github example-org/acme: merge #7 at head-7 (squash)".to_string()),
        "{:?}",
        intents.lines()
    );
}

/// The bridge's comments on an item saying neither the configuration nor
/// the ticket's labels ask for it any more.
fn not_asked(w: &World, key: &str) -> usize {
    crate::tracker::Tracker::comments(&w.tracker, key)
        .unwrap()
        .iter()
        .filter(|c| {
            c.by_bridge
                && c.body
                    .contains("Neither the configuration nor the ticket's `angle/` labels ask for")
        })
        .count()
}

#[test]
fn a_round_opened_before_the_angles_were_removed_is_canceled_and_the_pr_merges_on_its_ci() {
    let mut w = World::new();
    let pr = in_review(&w, 7);
    check_every_box(&w, "ABC-7");
    let items = round(&w, "ABC-7", &pr);
    assert!(all_in(&items, Status::Ready), "{items:?}");
    // The project drops its PR review; the PR has no approval at all.
    no_review(&mut w);
    let (report, log) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(merged(&w), "{:?}", w.github_writes());
    let after = round(&w, "ABC-7", &pr);
    assert!(all_in(&after, Status::Canceled), "{after:?}");
    for item in &items {
        assert_eq!(not_asked(&w, &item.key), 1, "{}", item.key);
    }
    let merge = log
        .entries()
        .iter()
        .find(|e| e.action.starts_with("merge"))
        .expect("the merge is logged");
    assert_eq!(merge.action, "merge (squash, no PR review)");
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::ToDeploy));
    quiet(&w);
}

#[test]
fn a_round_item_that_cannot_be_canceled_holds_the_merge_and_is_told_once() {
    let mut w = World::new();
    let pr = in_review(&w, 7);
    check_every_box(&w, "ABC-7");
    let (consolidation, angles) = round_keys(&w, "ABC-7", &pr);
    no_review(&mut w);
    w.tracker
        .fail_writes_starting_with(&format!("status {consolidation} Canceled"));
    let (report, _) = w.tick(false);
    assert!(!report.ok(), "the refused write is a failure");
    assert!(
        !merged(&w),
        "an item left open would be closed Done by the merge"
    );
    for a in &angles {
        assert_eq!(w.tracker.item(a).state.status, Some(Status::Canceled));
    }
    assert_eq!(not_asked(&w, &consolidation), 1);
    // The next tick cancels it, without saying it again, and merges.
    w.tracker.state.borrow_mut().failing_writes.clear();
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(merged(&w));
    assert_eq!(
        w.tracker.item(&consolidation).state.status,
        Some(Status::Canceled)
    );
    assert_eq!(not_asked(&w, &consolidation), 1);
    for a in &angles {
        assert_eq!(not_asked(&w, a), 1);
    }
    quiet(&w);
}

#[test]
fn a_round_keeps_the_angles_still_asked_for_and_cancels_the_others() {
    let mut w = World::new();
    let mut product = w.config.review.angles[0].clone();
    product.key = "product".into();
    product.title = "Product review".into();
    w.config.review.angles.push(product);
    let pr = in_review(&w, 7);
    check_every_box(&w, "ABC-7");
    let (consolidation, angles) = round_keys(&w, "ABC-7", &pr);
    let angle_of = |key: &str| {
        let item = w.tracker.item(key);
        match texts::bridge_item(&item.description, item.parent.as_deref()) {
            Some(BridgeItem::Angle { angle, .. }) => angle,
            other => panic!("{key} is not an angle sub-item: {other:?}"),
        }
    };
    let (dropped, kept): (Vec<String>, Vec<String>) =
        angles.into_iter().partition(|k| angle_of(k) == "product");
    assert_eq!((dropped.len(), kept.len()), (1, 1));
    // The project no longer asks for the product angle; the coordinator
    // approved before the remaining angle reviewed.
    w.config.review.angles.retain(|a| a.key != "product");
    let mut early = pr.clone();
    early.reviews = vec![review("coordinator-login", "APPROVED", &pr.head_sha, "ok")];
    w.open(early);
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(!merged(&w));
    assert_eq!(
        w.tracker.item(&dropped[0]).state.status,
        Some(Status::Canceled)
    );
    assert_eq!(not_asked(&w, &dropped[0]), 1);
    assert_eq!(w.tracker.item(&kept[0]).state.status, Some(Status::Ready));
    assert_eq!(
        w.tracker.item(&consolidation).state.status,
        Some(Status::Ready)
    );
    assert_eq!(
        report.waiting_lines(),
        vec![format!(
            "waiting: pr#7 its angle reviews are still open ({}): the approval at head-7 does not count yet",
            kept[0]
        )]
    );
    quiet(&w);

    // The remaining angle reviews: the approval counts and the PR merges.
    w.open(approved(&w, pr));
    w.tick(false);
    assert!(merged(&w));
    assert_eq!(w.tracker.item(&kept[0]).state.status, Some(Status::Done));
    assert_eq!(
        w.tracker.item(&consolidation).state.status,
        Some(Status::Done)
    );
    assert_eq!(
        w.tracker.item(&dropped[0]).state.status,
        Some(Status::Canceled)
    );
}

// ---- spec 04 §3, §11: manual angles, asked by the ticket's labels (ASDLC-DZ96) ----

/// The angle of each angle sub-item of the PR's round, by item key.
fn round_angles(w: &World, ticket: &str, pr: &PullRequest) -> Vec<(String, String)> {
    let mut angles: Vec<(String, String)> = round(w, ticket, pr)
        .into_iter()
        .filter_map(
            |c| match texts::bridge_item(&c.description, c.parent.as_deref()) {
                Some(BridgeItem::Angle { angle, .. }) => Some((angle, c.key)),
                _ => None,
            },
        )
        .collect();
    angles.sort();
    angles
}

/// A green PR whose ticket carries `labels`, In Progress.
fn labelled_pr(w: &World, labels: &[&str]) -> PullRequest {
    let pr = green(pull_request(7));
    w.tracker
        .seed("ABC-7", "A ticket", COMPLETE, Status::InProgress, labels);
    w.tracker.link("ABC-7", &pr.url);
    w.open(pr.clone());
    pr
}

#[test]
fn a_ticket_label_adds_its_angle_to_the_round_though_no_file_matches_it() {
    let w = World::new();
    // `src/lib.rs` touches only the `*` of maintainability; design has no
    // paths at all.
    let pr = labelled_pr(&w, &["angle/design"]);
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    let angles: Vec<String> = round_angles(&w, "ABC-7", &pr)
        .into_iter()
        .map(|(a, _)| a)
        .collect();
    assert_eq!(angles, vec!["design", "maintainability"]);
    quiet(&w);
}

#[test]
fn an_angle_without_paths_opens_no_round_item_without_its_label() {
    let w = World::new();
    let pr = labelled_pr(&w, &[]);
    w.tick(false);
    let angles: Vec<String> = round_angles(&w, "ABC-7", &pr)
        .into_iter()
        .map(|(a, _)| a)
        .collect();
    assert_eq!(angles, vec!["maintainability"], "design only by hand");
}

#[test]
fn removing_a_label_cancels_the_manual_angle_and_never_a_path_triggered_one() {
    let w = World::new();
    let pr = labelled_pr(&w, &["angle/design", "angle/maintainability"]);
    w.tick(false);
    let angles = round_angles(&w, "ABC-7", &pr);
    let key = |angle: &str| {
        angles
            .iter()
            .find(|(a, _)| a == angle)
            .map(|(_, k)| k.clone())
            .unwrap()
    };
    let (design, maintainability) = (key("design"), key("maintainability"));
    for label in ["angle/design", "angle/maintainability"] {
        crate::tracker::Tracker::remove_label(&w.tracker, "ABC-7", label).unwrap();
    }
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert_eq!(w.tracker.item(&design).state.status, Some(Status::Canceled));
    assert_eq!(not_asked(&w, &design), 1);
    // Its paths still bring maintainability: the label never removes it.
    assert_eq!(
        w.tracker.item(&maintainability).state.status,
        Some(Status::Ready)
    );
    assert_eq!(not_asked(&w, &maintainability), 0);
    quiet(&w);
}

/// The bridge's comments on the ticket about an unknown angle label.
fn unknown_label_comments(w: &World, key: &str) -> Vec<String> {
    crate::tracker::Tracker::comments(&w.tracker, key)
        .unwrap()
        .into_iter()
        .filter(|c| c.by_bridge && c.body.contains("spl-asdlc:unknown-angle-label"))
        .map(|c| c.body)
        .collect()
}

#[test]
fn an_unknown_angle_label_is_told_once_on_the_ticket_and_holds_nothing() {
    let w = World::new();
    let pr = labelled_pr(&w, &["angle/visual-qa", "bug"]);
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    let said = unknown_label_comments(&w, "ABC-7");
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].contains(
            "The label `angle/visual-qa` asks for a PR review angle this project does not configure"
        ),
        "{}",
        said[0]
    );
    // The PR goes on as if the label were not there.
    let angles: Vec<String> = round_angles(&w, "ABC-7", &pr)
        .into_iter()
        .map(|(a, _)| a)
        .collect();
    assert_eq!(angles, vec!["maintainability"]);
    quiet(&w);
    assert_eq!(unknown_label_comments(&w, "ABC-7").len(), 1);
}

#[test]
fn the_bridge_creates_the_label_of_each_angle_the_tracker_lacks_once() {
    let mut w = World::new();
    w.tracker = FakeTracker::new("ABC");
    w.tracker.seed_label("angle/security");
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    let created: Vec<String> = w
        .tracker
        .writes()
        .into_iter()
        .filter(|l| l.starts_with("ensure-label "))
        .collect();
    assert_eq!(
        created,
        vec![
            "ensure-label angle/maintainability",
            "ensure-label angle/design"
        ]
    );
    quiet(&w);
}

/// No angle on every PR, but one on its paths: a coordinator approves.
fn paths_only(w: &mut World) {
    let security = w
        .config
        .review
        .angles
        .iter()
        .find(|a| a.key == "security")
        .cloned()
        .expect("the example's angle on paths");
    no_review(w);
    w.config.review.angles = vec![security];
    w.config.review.coordinators = vec!["coordinator-login".into()];
    assert!(w.config.review.angle_possible());
}

#[test]
fn an_angles_path_still_opens_a_round_that_needs_the_coordinator() {
    let mut w = World::new();
    paths_only(&mut w);
    let pr = ready_pr(&w);
    w.github
        .state
        .borrow_mut()
        .pull_files
        .insert((repo(), 7), Ok(vec!["src/auth/login.rs".into()]));
    w.tick(false);
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::InReview));
    let scope = texts::round_scope(&pr.url, &pr.head_sha);
    let kinds: Vec<BridgeItem> = round(&w, "ABC-7", &pr)
        .iter()
        .filter_map(|c| texts::bridge_item(&c.description, c.parent.as_deref()))
        .collect();
    assert!(kinds.contains(&BridgeItem::Angle {
        scope: scope.clone(),
        angle: "security".into()
    }));
    assert!(kinds.contains(&BridgeItem::Consolidation { scope }));
    // Without the coordinator's approval, no merge.
    w.tick(false);
    assert!(!merged(&w));
    // Approved after its angle: merged as a reviewed PR. It still touches
    // the angle's path (opening it again reset its files).
    w.open(approved(&w, pr));
    w.github
        .state
        .borrow_mut()
        .pull_files
        .insert((repo(), 7), Ok(vec!["src/auth/login.rs".into()]));
    let (report, log) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(merged(&w));
    assert!(log.entries().iter().any(|e| e.action == "merge (squash)"
        && e.inputs["approved_by"] == "coordinator-login"));
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::ToDeploy));
}

#[test]
fn a_pr_that_gets_no_angle_merges_with_no_round_where_others_may_get_one() {
    let mut w = World::new();
    paths_only(&mut w);
    ready_pr(&w);
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(children_of(&w, "ABC-7").is_empty());
    assert!(merged(&w));
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::ToDeploy));
}

#[test]
fn changes_requested_on_a_pr_with_no_angle_send_the_ticket_back_until_a_new_push() {
    let mut w = World::new();
    paths_only(&mut w);
    let mut pr = ready_pr(&w);
    // An open thread holds it In Review.
    pr.threads = vec![ReviewThread {
        resolved: false,
        outdated: false,
        path: None,
    }];
    w.open(pr.clone());
    w.tick(false);
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::InReview));
    pr.reviews = vec![review(
        "coordinator-login",
        "CHANGES_REQUESTED",
        &pr.head_sha,
        "redo",
    )];
    w.open(pr.clone());
    w.tick(false);
    assert!(!merged(&w));
    assert_eq!(
        w.tracker.item("ABC-7").state.status,
        Some(Status::InProgress),
        "the implementer's turn"
    );
    quiet(&w);
    // The implementer pushes: opened again, In Review again.
    pr.head_sha = "head-7b".into();
    w.open(pr);
    w.tick(false);
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::InReview));
}

#[test]
fn an_unread_diff_holds_a_pr_with_no_angle_and_opens_no_round() {
    let mut w = World::new();
    no_review(&mut w);
    ready_pr(&w);
    w.github.state.borrow_mut().pull_files.insert(
        (repo(), 7),
        Err(crate::github::ReadError::unreadable("files", "HTTP 502")),
    );
    let (report, _) = w.tick(false);
    assert!(!report.ok(), "the unread diff is reported");
    assert!(!merged(&w));
    assert!(children_of(&w, "ABC-7").is_empty());
}

// ---- spec 04 §6: a request for changes the coordinator's approval supersedes ----

/// A standing request for changes, with the id GitHub gave it.
fn changes(author: &str, id: u64, sha: &str) -> Review {
    Review {
        id: Some(id),
        ..review(author, "CHANGES_REQUESTED", sha, "redo")
    }
}

/// A PR In Review whose every angle reported and whose coordinator
/// approved at the head, after `earlier` asked for changes on an older head.
fn approved_after(w: &World, earlier: Review) -> PullRequest {
    let pr = in_review(w, 7);
    let mut pr = approved(w, pr);
    pr.reviews.insert(0, earlier);
    w.open(pr.clone());
    pr
}

#[test]
fn an_apps_request_for_changes_superseded_by_the_coordinators_approval_is_dismissed_then_merged() {
    let w = World::new();
    approved_after(&w, changes(&bot("angle-app"), 41, "older"));
    let (report, log) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    let writes = w.github_writes();
    let dismissed = writes
        .iter()
        .position(|l| l.starts_with("dismiss-review example-org/acme#7 41: "))
        .expect("the App's review is dismissed");
    let merged = writes
        .iter()
        .position(|l| l == "merge example-org/acme#7 at head-7 squash")
        .expect("then the PR is merged");
    assert!(dismissed < merged, "{writes:?}");
    assert!(
        writes[dismissed].contains("coordinator-login approved this PR at its head `head-7`"),
        "the message names the approval and the head: {}",
        writes[dismissed]
    );
    let entry = log
        .entries()
        .iter()
        .find(|e| e.action.starts_with("dismiss"))
        .expect("the action log records the dismissal");
    assert_eq!(
        entry.action,
        format!("dismiss review of {}", bot("angle-app"))
    );
    assert_eq!(entry.inputs["review"], 41);
    assert_eq!(entry.inputs["approved_by"], "coordinator-login");
    assert_eq!(entry.outcome, Outcome::Done);
}

#[test]
fn a_dismissal_is_not_repeated_when_the_merge_is_refused() {
    let w = World::new();
    approved_after(&w, changes(&bot("angle-app"), 41, "older"));
    w.github
        .fail_writes_starting_with("merge example-org/acme#7", Some(405), "not now");
    let (report, _) = w.tick(false);
    assert!(!report.ok());
    w.github.state.borrow_mut().failing_writes.clear();
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    let writes = w.github_writes();
    assert_eq!(
        writes
            .iter()
            .filter(|l| l.starts_with("dismiss-review"))
            .count(),
        1,
        "{writes:?}"
    );
    assert!(writes.contains(&"merge example-org/acme#7 at head-7 squash".to_string()));
}

#[test]
fn a_persons_request_for_changes_holds_the_merge_and_is_never_dismissed() {
    let w = World::new();
    let mut pr = approved_after(&w, changes("passer-by", 42, "older"));
    // An App's request beside it is not dismissed either: nothing merges.
    pr.reviews
        .insert(0, changes(&bot("angle-app"), 41, "older"));
    w.open(pr);
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert_eq!(
        report.waiting_lines(),
        vec![
            "waiting: pr#7 changes requested by passer-by (a person; the bridge never dismisses it)"
        ]
    );
    let writes = w.github_writes();
    assert!(
        !writes
            .iter()
            .any(|l| l.starts_with("dismiss-review") || l.starts_with("merge")),
        "{writes:?}"
    );
    quiet(&w);
}

#[test]
fn without_the_coordinators_approval_at_the_head_nothing_is_dismissed() {
    let w = World::new();
    let mut pr = in_review(&w, 7);
    pr.reviews = angle_reviews(&w, &pr);
    pr.reviews
        .insert(0, changes(&bot("angle-app"), 41, "older"));
    pr.reviews
        .push(review("coordinator-login", "APPROVED", "older", "ok"));
    w.open(pr);
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    let writes = w.github_writes();
    assert!(
        !writes
            .iter()
            .any(|l| l.starts_with("dismiss-review") || l.starts_with("merge")),
        "{writes:?}"
    );
    assert!(
        report.waiting_lines()[0].starts_with("waiting: pr#7 no coordinator approval at head-7"),
        "{:?}",
        report.waiting_lines()
    );
}

#[test]
fn a_dry_run_prints_the_dismissals_before_the_merge() {
    let w = World::new();
    approved_after(&w, changes(&bot("angle-app"), 41, "older"));
    let before = (w.tracker.writes().len(), w.github_writes().len());
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
        method_texts: Default::default(),
        low_quota: false,
    };
    let mut log = ActionLog::in_memory();
    run(&t, &mut log);
    assert_eq!(
        (w.tracker.writes().len(), w.github_writes().len()),
        before,
        "a dry run writes nothing"
    );
    let lines: Vec<String> = log
        .entries()
        .iter()
        .map(crate::action_log::describe)
        .collect();
    let want = format!(
        "pr#7 ABC-7: dismiss review of {} — would do",
        bot("angle-app")
    );
    assert!(lines.iter().any(|l| l.contains(&want)), "{lines:#?}");
    let intents = intents.lines();
    let dismissal = intents
        .iter()
        .position(|l| l.starts_with("github example-org/acme: dismiss review 41 on #7: "))
        .expect("the dry run prints the dismissal");
    let merge = intents
        .iter()
        .position(|l| l == "github example-org/acme: merge #7 at head-7 (squash)")
        .expect("then the merge");
    assert!(dismissal < merge, "{intents:?}");
}

// ---- the business-rule guard reads `.spl-asdlc/business-rules/` (spec 02) ----

/// A ready PR whose ticket cites `id` in its Business rules, every box
/// ticked, with the registry's directory at its head holding `rules`.
fn citing(w: &mut World, id: &str, rules: &[&str]) {
    no_review(w);
    ready_pr(w);
    let description = COMPLETE
        .replace("- [ ] it is fast", "- [x] it is fast")
        .replace("No new rule.", &format!("- {id} applies."));
    crate::tracker::Tracker::update_description(&w.tracker, "ABC-7", &description).unwrap();
    let mut s = w.github.state.borrow_mut();
    for rule in rules {
        s.files.insert(
            (
                repo(),
                "head-7".into(),
                format!(".spl-asdlc/business-rules/{rule}.md"),
            ),
            Ok(format!("# {rule} — A rule.\n")),
        );
    }
}

#[test]
fn a_pr_citing_a_rule_whose_file_is_in_the_registry_at_its_head_merges() {
    let mut w = World::new();
    citing(&mut w, "BR-ACME-001", &["BR-ACME-001"]);
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(merged(&w), "{:?}", w.github_writes());
}

#[test]
fn a_pr_citing_a_rule_with_no_file_in_the_registry_is_held_and_told_so() {
    let mut w = World::new();
    citing(&mut w, "BR-ACME-002", &["BR-ACME-001"]);
    w.tick(false);
    assert!(!merged(&w));
    let said = w.github.state.borrow().comments[&(repo(), 7)]
        .as_ref()
        .unwrap()
        .iter()
        .map(|c| c.body.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(said.contains("`BR-ACME-002`"), "{said}");
    assert!(said.contains("`.spl-asdlc/business-rules/`"), "{said}");
}

#[test]
fn a_rule_stated_by_the_ticket_merges_once_its_pr_adds_the_rule_file() {
    let mut w = World::new();
    no_review(&mut w);
    ready_pr(&w);
    let description = COMPLETE
        .replace("- [ ] it is fast", "- [x] it is fast")
        .replace("No new rule.", "- Every export is signed.");
    crate::tracker::Tracker::update_description(&w.tracker, "ABC-7", &description).unwrap();
    w.github.state.borrow_mut().commits.insert(
        (repo(), "head-7".into()),
        Ok(crate::github::Commit {
            sha: "head-7".into(),
            message: "feat: sign exports".into(),
            committer_date: None,
            parents: Vec::new(),
        }),
    );
    w.tick(false);
    assert!(!merged(&w), "the PR adds no rule file");
    w.github.state.borrow_mut().pull_files.insert(
        (repo(), 7),
        Ok(vec![
            "src/lib.rs".into(),
            ".spl-asdlc/business-rules/BR-ACME-001.md".into(),
        ]),
    );
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(merged(&w), "{:?}", w.github_writes());
}

fn base_for_red(w: &World, head: &str, status: crate::github::CompareStatus) {
    use crate::github::{Commit, Comparison};
    let mut s = w.github.state.borrow_mut();
    s.commits.insert(
        (repo(), "main".into()),
        Ok(Commit {
            sha: "base-tip".into(),
            message: "base".into(),
            committer_date: None,
            parents: vec![],
        }),
    );
    s.compares.insert(
        (repo(), "base-tip".into(), head.into()),
        Ok(Comparison {
            status,
            ahead_by: 1,
            behind_by: 1,
            commits: vec![],
            files: vec![],
            base_committed_at: None,
        }),
    );
}

#[test]
fn a_green_behind_pr_without_currency_requirement_merges_without_refresh() {
    let mut w = World::new();
    no_review(&mut w);
    w.config.review.require_up_to_date = false;
    let mut pr = ready_pr(&w);
    pr.merge_state = "BEHIND".into();
    w.open(pr);
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(merged(&w));
    assert!(
        !w.github_writes()
            .iter()
            .any(|s| s.starts_with("update-branch"))
    );
}

#[test]
fn all_green_prs_merge_before_an_older_red_pr_is_refreshed() {
    let mut w = World::new();
    no_review(&mut w);
    w.config.review.require_up_to_date = false;
    for n in [3, 8, 9] {
        let mut pr = green(pull_request(n));
        if n == 3 {
            pr.checks[0].conclusion = Some("FAILURE".into());
        }
        w.ticket(&format!("ABC-{n}"), Status::InProgress, &pr);
        check_every_box(&w, &format!("ABC-{n}"));
        w.open(pr);
    }
    base_for_red(&w, "head-3", crate::github::CompareStatus::Diverged);
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    let writes = w.github_writes();
    let refresh = writes
        .iter()
        .position(|s| s.starts_with("update-branch"))
        .unwrap();
    let merges: Vec<_> = writes
        .iter()
        .enumerate()
        .filter(|(_, s)| s.starts_with("merge "))
        .collect();
    assert_eq!(merges.len(), 2);
    assert!(merges.iter().all(|(i, _)| *i < refresh));
    // GitHub has not yet published its async update: the accepted head/base
    // marker prevents a second refresh on unchanged facts.
    w.tick(false);
    assert_eq!(
        w.github_writes()
            .iter()
            .filter(|s| s.starts_with("update-branch"))
            .count(),
        1
    );
}

#[test]
fn red_ci_with_current_base_returns_to_draft_with_diagnostics_once() {
    let mut w = World::new();
    no_review(&mut w);
    let mut pr = ready_pr(&w);
    pr.checks[0].conclusion = Some("FAILURE".into());
    pr.checks[0].details_url = Some("https://example.test/failed-check".into());
    w.open(pr);
    base_for_red(&w, "head-7", crate::github::CompareStatus::Ahead);
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(!merged(&w));
    assert_eq!(
        w.tracker.item("ABC-7").state.status,
        Some(Status::InProgress)
    );
    assert!(
        w.github_writes()
            .iter()
            .any(|s| s == "draft-pull example-org/acme#7")
    );
    let s = w.github.state.borrow();
    let comments = s.comments.get(&(repo(), 7)).unwrap().as_ref().unwrap();
    assert!(
        comments
            .iter()
            .any(|c| c.body.contains("https://example.test/failed-check"))
    );
    drop(s);
    quiet(&w);
}

#[test]
fn a_conflicting_pr_returns_exact_paths_to_its_implementer_before_drafting() {
    let mut w = World::new();
    no_review(&mut w);
    let mut pr = ready_pr(&w);
    pr.mergeable = crate::github::Mergeable::Conflicting;
    w.open(pr);
    w.github.state.borrow_mut().conflict_files.insert(
        (repo(), 7),
        Ok(vec!["src/a file.rs".into(), "src/b.rs".into()]),
    );
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert_eq!(
        w.tracker.item("ABC-7").state.status,
        Some(Status::InProgress)
    );
    assert!(
        w.github_writes()
            .iter()
            .any(|s| s == "draft-pull example-org/acme#7")
    );
    assert!(
        w.tracker
            .comments("ABC-7")
            .unwrap()
            .iter()
            .any(|c| c.body.contains("src/a file.rs") && c.body.contains("src/b.rs"))
    );
    quiet(&w);
}

#[test]
fn an_unreadable_conflict_preview_keeps_the_pr_visible_and_does_not_guess_files() {
    let mut w = World::new();
    no_review(&mut w);
    let mut pr = ready_pr(&w);
    pr.mergeable = crate::github::Mergeable::Conflicting;
    w.open(pr);
    let (report, _) = w.tick(false);
    assert!(!report.ok());
    assert!(
        !w.github_writes()
            .iter()
            .any(|s| s.starts_with("draft-pull"))
    );
    assert!(!merged(&w));
}

#[test]
fn a_refreshed_failed_pr_merges_when_its_new_head_turns_green() {
    let mut w = World::new();
    no_review(&mut w);
    w.config.review.require_up_to_date = false;
    let mut pr = ready_pr(&w);
    pr.checks[0].conclusion = Some("FAILURE".into());
    w.open(pr.clone());
    base_for_red(&w, "head-7", crate::github::CompareStatus::Diverged);
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(!merged(&w));
    pr.head_sha = "refreshed-head".into();
    pr.checks[0].conclusion = Some("SUCCESS".into());
    w.open(pr);
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(
        w.github_writes()
            .contains(&"merge example-org/acme#7 at refreshed-head squash".into())
    );
    assert_eq!(
        w.github_writes()
            .iter()
            .filter(|s| s.starts_with("update-branch"))
            .count(),
        1
    );
}

#[test]
fn no_branch_refresh_is_attempted_when_the_base_cannot_be_read() {
    let mut w = World::new();
    no_review(&mut w);
    let mut pr = ready_pr(&w);
    pr.checks[0].conclusion = Some("FAILURE".into());
    w.open(pr);
    let (report, _) = w.tick(false);
    assert!(!report.ok());
    assert!(
        !w.github_writes()
            .iter()
            .any(|s| s.starts_with("update-branch") || s.starts_with("draft-pull"))
    );
}

#[test]
fn automatic_refreshes_stop_after_three_trusted_attempts_even_if_main_moves() {
    let mut w = World::new();
    no_review(&mut w);
    let mut pr = ready_pr(&w);
    pr.checks[0].conclusion = Some("FAILURE".into());
    w.open(pr);
    base_for_red(&w, "head-7", crate::github::CompareStatus::Diverged);
    let comments = (0..3)
        .map(|n| crate::github::Comment {
            id: n,
            author: Some(Actor {
                login: bot("spl-asdlc"),
                is_bot: true,
            }),
            body: texts::marker(
                "ci-refresh",
                &[("head", &format!("old-head-{n}")), ("base", "old-base")],
            ),
            created_at: None,
            url: String::new(),
        })
        .collect();
    w.github
        .state
        .borrow_mut()
        .comments
        .insert((repo(), 7), Ok(comments));
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert!(
        !w.github_writes()
            .iter()
            .any(|s| s.starts_with("update-branch"))
    );
    assert!(
        w.github_writes()
            .contains(&"draft-pull example-org/acme#7".into())
    );
    assert!(
        w.tracker
            .comments("ABC-7")
            .unwrap()
            .iter()
            .any(|c| c.body.contains("limit (3)"))
    );
}

#[test]
fn a_failed_handback_comment_never_hides_the_pr() {
    let mut w = World::new();
    no_review(&mut w);
    let mut pr = ready_pr(&w);
    pr.checks[0].conclusion = Some("FAILURE".into());
    w.open(pr);
    base_for_red(&w, "head-7", crate::github::CompareStatus::Ahead);
    w.tracker.fail_writes_starting_with("comment ABC-7");
    let (report, _) = w.tick(false);
    assert!(!report.ok());
    assert!(
        !w.github_writes()
            .iter()
            .any(|s| s.starts_with("draft-pull"))
    );
}

// ---- ASDLC-SX97: a box checked after the merge moves the ticket on ----

/// The release tag `acme-v1.0.0` published, holding `merge_sha`.
fn shipped(w: &World, merge_sha: &str) {
    use crate::github::{CompareStatus, Comparison, GitRef, ObjectType};
    let mut s = w.github.state.borrow_mut();
    s.tags.insert(
        repo(),
        vec![GitRef {
            name: "acme-v1.0.0".into(),
            sha: "release-1".into(),
            object_type: ObjectType::Commit,
        }],
    );
    s.compares.insert(
        (repo(), merge_sha.into(), "acme-v1.0.0".into()),
        Ok(Comparison {
            status: CompareStatus::Ahead,
            ahead_by: 1,
            behind_by: 0,
            commits: vec![],
            files: vec![],
            base_committed_at: None,
        }),
    );
}

/// The bridge's comments on a ticket that carry `marker`.
fn said(w: &World, key: &str, marker: &str) -> Vec<String> {
    crate::tracker::Tracker::comments(&w.tracker, key)
        .unwrap()
        .into_iter()
        .filter(|c| c.by_bridge && texts::has_marker(&c.body, marker))
        .map(|c| c.body)
        .collect()
}

/// PR #7 merged by the bridge while a box of ABC-7 is open: In Progress.
fn merged_with_a_box_open(w: &World) -> PullRequest {
    let pr = in_review(w, 7);
    w.open(approved(w, pr.clone()));
    w.tick(false);
    assert_eq!(
        w.tracker.item("ABC-7").state.status,
        Some(Status::InProgress)
    );
    pr
}

#[test]
fn a_box_checked_after_the_merge_moves_the_ticket_to_deploy_at_the_next_tick() {
    // AGFG-2B5W: merged with a box open, the box checked later.
    let w = World::new();
    let pr = merged_with_a_box_open(&w);
    // The box still open: it stays, and nothing is said.
    quiet(&w);
    check_every_box(&w, "ABC-7");
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::ToDeploy));
    let comments = said(&w, "ABC-7", &texts::settled_marker(&pr.url, "To Deploy"));
    assert_eq!(comments.len(), 1, "{comments:?}");
    assert!(
        comments[0].contains(&format!("after its PR {} was merged", pr.url)),
        "{}",
        comments[0]
    );
    assert!(comments[0].contains("state: To Deploy"), "{}", comments[0]);
    assert!(
        comments[0].contains("next: the release closure moves it to `Done`"),
        "{}",
        comments[0]
    );
    quiet(&w);
}

#[test]
fn a_ticket_whose_merge_already_shipped_goes_to_done_not_to_deploy() {
    // A release went out while the ticket waited In Progress.
    let w = World::new();
    let pr = merged_with_a_box_open(&w);
    shipped(&w, "merge-of-7");
    quiet(&w);
    check_every_box(&w, "ABC-7");
    w.tick(false);
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::Done));
    let comments = said(&w, "ABC-7", &texts::settled_marker(&pr.url, "Done"));
    assert_eq!(comments.len(), 1, "{comments:?}");
    assert!(
        comments[0].contains("That PR already shipped in `acme-v1.0.0`."),
        "{}",
        comments[0]
    );
    quiet(&w);
}

/// ABC-9 as the closure leaves it: shipped in `acme-v1.0.0`, sent back In
/// Progress with `description`'s box open.
fn sent_back_by_the_closure(w: &World, description: &str) -> PullRequest {
    let mut pr = pull_request(9);
    pr.state = PrState::Merged;
    pr.merged_at = crate::github::Timestamp::parse("2026-10-01T10:00:00Z");
    pr.merge_commit_sha = Some("merge-of-9".into());
    w.tracker
        .seed("ABC-9", "A ticket", description, Status::InProgress, &[]);
    w.tracker.link("ABC-9", &pr.url);
    for body in [
        texts::merged_comment(
            &pr.url,
            "main",
            &texts::MergedBy::Bridge {
                approved_by: None,
                up_to_date: false,
            },
            "To Deploy",
            "the closure",
        ),
        crate::method::done_gate::RETURNED_UNASSIGNED.to_string(),
    ] {
        crate::tracker::Tracker::comment(&w.tracker, "ABC-9", &body).unwrap();
    }
    w.github
        .state
        .borrow_mut()
        .merged_pulls
        .insert(repo(), Ok(vec![pr.clone()]));
    shipped(w, "merge-of-9");
    quiet(w);
    pr
}

#[test]
fn a_ticket_the_closure_sent_back_goes_to_done_once_its_box_is_checked() {
    let w = World::new();
    let pr = sent_back_by_the_closure(&w, COMPLETE);
    check_every_box(&w, "ABC-9");
    w.tick(false);
    assert_eq!(w.tracker.item("ABC-9").state.status, Some(Status::Done));
    let comments = said(&w, "ABC-9", &texts::settled_marker(&pr.url, "Done"));
    assert_eq!(comments.len(), 1, "{comments:?}");
    assert!(
        comments[0].contains("already shipped in `acme-v1.0.0` and every box"),
        "{}",
        comments[0]
    );
    quiet(&w);
}

#[test]
fn a_ticket_the_closure_sent_back_waits_in_post_deploy_for_its_own_boxes() {
    let w = World::new();
    let with_post_deploy = format!("{COMPLETE}\n## Post deploy\n- [ ] it holds in production\n");
    let pr = sent_back_by_the_closure(&w, &with_post_deploy);
    let checked = with_post_deploy.replace("- [ ] it is fast", "- [x] it is fast");
    crate::tracker::Tracker::update_description(&w.tracker, "ABC-9", &checked).unwrap();
    w.tick(false);
    assert_eq!(
        w.tracker.item("ABC-9").state.status,
        Some(Status::PostDeploy),
        "never Done before its post-deploy boxes are checked"
    );
    assert_eq!(
        said(&w, "ABC-9", &texts::settled_marker(&pr.url, "Post Deploy")).len(),
        1
    );
    quiet(&w);
}

#[test]
fn a_shipped_ticket_citing_an_unknown_rule_stays_in_progress_and_says_so_once() {
    let w = World::new();
    let cites = COMPLETE.replace("No new rule.", "BR-ACME-001 applies.");
    let pr = sent_back_by_the_closure(&w, &cites);
    rule_at(&w, "acme-v1.0.0", "BR-ACME-002");
    let checked = cites.replace("- [ ] it is fast", "- [x] it is fast");
    crate::tracker::Tracker::update_description(&w.tracker, "ABC-9", &checked).unwrap();
    w.tick(false);
    assert_eq!(
        w.tracker.item("ABC-9").state.status,
        Some(Status::InProgress)
    );
    let comments = said(&w, "ABC-9", &texts::settled_marker(&pr.url, "In Progress"));
    assert_eq!(comments.len(), 1, "{comments:?}");
    assert!(comments[0].contains("`BR-ACME-001`"), "{}", comments[0]);
    quiet(&w);
    // The rule's file at the tag (the id was a typo): Done.
    rule_at(&w, "acme-v1.0.0", "BR-ACME-001");
    w.tick(false);
    assert_eq!(w.tracker.item("ABC-9").state.status, Some(Status::Done));
}

fn rule_at(w: &World, rev: &str, id: &str) {
    w.github.state.borrow_mut().files.insert(
        (
            repo(),
            rev.into(),
            format!(".spl-asdlc/business-rules/{id}.md"),
        ),
        Ok(format!("# {id} — A rule.\n")),
    );
}

#[test]
fn a_merged_ticket_with_an_open_pr_is_left_to_the_pr_pass() {
    let w = World::new();
    merged_with_a_box_open(&w);
    // A second PR, sent back by the coordinator: the PR pass keeps the
    // ticket In Progress, and the settling does not move it past that PR.
    let mut second = green(pull_request(8));
    w.tracker.link("ABC-7", &second.url);
    second.reviews = vec![review(
        "coordinator-login",
        "CHANGES_REQUESTED",
        &second.head_sha,
        "redo",
    )];
    w.open(second);
    check_every_box(&w, "ABC-7");
    w.tick(false);
    assert_eq!(
        w.tracker.item("ABC-7").state.status,
        Some(Status::InProgress)
    );
    assert!(
        crate::tracker::Tracker::comments(&w.tracker, "ABC-7")
            .unwrap()
            .iter()
            .all(|c| !c.body.contains("spl-asdlc:settled")),
        "nothing settled while a PR of it is open"
    );
}

#[test]
fn a_complete_ticket_never_merged_is_left_in_progress() {
    let w = World::new();
    let all = COMPLETE.replace("- [ ] it is fast", "- [x] it is fast");
    w.tracker
        .seed("ABC-3", "A ticket", &all, Status::InProgress, &[]);
    quiet(&w);
    assert_eq!(
        w.tracker.item("ABC-3").state.status,
        Some(Status::InProgress)
    );
}

#[test]
fn a_cut_tick_retries_the_status_without_repeating_the_comment() {
    let w = World::new();
    let pr = merged_with_a_box_open(&w);
    check_every_box(&w, "ABC-7");
    w.tracker
        .fail_writes_starting_with("status ABC-7 To Deploy");
    let (report, _) = w.tick(false);
    assert!(!report.ok());
    assert_eq!(
        w.tracker.item("ABC-7").state.status,
        Some(Status::InProgress)
    );
    w.tracker.state.borrow_mut().failing_writes.clear();
    w.tick(false);
    assert_eq!(w.tracker.item("ABC-7").state.status, Some(Status::ToDeploy));
    assert_eq!(
        said(&w, "ABC-7", &texts::settled_marker(&pr.url, "To Deploy")).len(),
        1,
        "said once"
    );
    quiet(&w);
}
