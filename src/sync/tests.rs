use super::*;
use crate::github::fake::{FakeGitHub, pull_request};
use crate::github::{CheckEntry, CheckKind, Comparison, GitRef, Identity, ObjectType};
use crate::method::vocabulary::Status;
use crate::tracker::fake::FakeTracker;

fn repo() -> RepoSlug {
    RepoSlug::parse("example-org/acme").unwrap()
}

/// A GitHub that knows the App and has no PR.
fn github() -> FakeGitHub {
    let g = FakeGitHub::new();
    {
        let mut s = g.state.borrow_mut();
        s.identity = Some(Identity::App {
            slug: "spl-asdlc".into(),
            app_id: "1".into(),
        });
        s.default_branch.insert(repo(), "main".into());
        s.open_pulls.insert(repo(), Ok(Vec::new()));
        s.merged_pulls.insert(repo(), Ok(Vec::new()));
        s.closed_pulls.insert(repo(), Ok(Vec::new()));
        s.updated_pulls.insert(repo(), Ok(Vec::new()));
        s.tags.insert(repo(), Vec::new());
    }
    g
}

fn open(g: &FakeGitHub, prs: Vec<PullRequest>) {
    let mut s = g.state.borrow_mut();
    s.open_pulls.insert(repo(), Ok(prs.clone()));
    s.updated_pulls.insert(repo(), Ok(prs));
}

fn merged(g: &FakeGitHub, prs: Vec<PullRequest>) {
    let mut s = g.state.borrow_mut();
    s.merged_pulls.insert(repo(), Ok(prs.clone()));
    s.updated_pulls.insert(repo(), Ok(prs));
}

/// A directory of its own per test: the memory is a file.
fn log(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("spl-asdlc-sync-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("acme.jsonl")
}

fn sync(tracker: &FakeTracker, github: &FakeGitHub, log: &Path) -> PassOutcome {
    sync_within(tracker, github, log, None)
}

fn sync_within(
    tracker: &FakeTracker,
    github: &FakeGitHub,
    log: &Path,
    budget: Option<Duration>,
) -> PassOutcome {
    let config = crate::config::spec_example::config();
    pass_with(
        &Host {
            config_path: Path::new(crate::config::DEFAULT_PATH),
            tracker,
            budget,
            log_path: Some(log.to_path_buf()),
            env: &|_| None,
        },
        &config,
        github,
    )
}

fn named(number: u64, title: &str) -> PullRequest {
    PullRequest {
        title: title.into(),
        updated_at: Timestamp::parse(&format!("2026-10-25T00:{number:02}:00Z")),
        ..pull_request(number)
    }
}

fn check(conclusion: &str) -> CheckEntry {
    CheckEntry {
        kind: CheckKind::Run,
        name: "test".into(),
        workflow: Some("CI".into()),
        status: Some("COMPLETED".into()),
        conclusion: Some(conclusion.into()),
        started_at: None,
        details_url: None,
    }
}

#[test]
fn a_pr_names_its_ticket_by_its_branch_its_title_or_a_line_of_its_body() {
    let k = "ABC-12";
    assert!(names_item(k, "main", "feat: x (ABC-12)", ""));
    assert!(names_item(k, "claude/abc-12-fix", "feat: x", ""));
    assert!(names_item(k, "main", "feat: x", "Refs: ABC-12"));
    assert!(names_item(k, "main", "feat: x", "- Closes ABC-12"));
    assert!(names_item(
        "TD-7K3F",
        "main",
        "feat: x",
        "See livela://item/TD-7K3F"
    ));
    // In passing, or another ticket that shares a prefix of the code.
    assert!(!names_item(
        k,
        "main",
        "feat: x",
        "Unlike ABC-12, this one…"
    ));
    assert!(!names_item(k, "main", "feat: x (ABC-123)", ""));
    assert!(!names_item(k, "feat/abc-123", "feat: x", ""));
    assert!(!names_item(k, "feat/x-abc-12x", "feat: x", ""));
    assert!(!names_item(k, "main", "feat: x", "Referenced ABC-12"));
    assert_eq!(
        candidates(&["ABC".into()], &["feat: abc-7 and ABC-12, xABC-9"]),
        ["abc-7", "ABC-12"]
    );
}

#[test]
fn a_pr_naming_a_ticket_is_linked_its_status_recorded_and_a_second_pass_writes_nothing() {
    let t = FakeTracker::default();
    t.seed("ABC-12", "T", "", Status::InProgress, &[]);
    let g = github();
    let mut pr = named(7, "feat: thing (ABC-12)");
    pr.checks = vec![check("SUCCESS")];
    open(&g, vec![pr.clone(), named(8, "chore: unrelated")]);
    let log = log("link");
    let out = sync(&t, &g, &log);
    assert!(out.ok, "{out:?}");
    assert!(
        out.message
            .contains("2 PR(s) read, 1 link(s) laid, 1 status(es) recorded"),
        "{}",
        out.message
    );
    assert_eq!(
        t.writes(),
        [
            format!("link ABC-12 {}", pr.url),
            format!("pr status ABC-12 {} open:passing", pr.url),
        ]
    );
    let again = sync(&t, &g, &log);
    assert!(again.ok, "{again:?}");
    assert_eq!(t.writes().len(), 2, "no heartbeat: {:?}", t.writes());
    assert!(g.writes().is_empty(), "the sync never writes on GitHub");
    // Its checks turn red: one write, the new status.
    pr.checks = vec![check("FAILURE")];
    pr.updated_at = Timestamp::parse("2026-10-25T01:00:00Z");
    open(&g, vec![pr.clone()]);
    sync(&t, &g, &log);
    assert_eq!(
        t.writes().last().unwrap(),
        &format!("pr status ABC-12 {} open:failing", pr.url)
    );
    // Never a status, a label, an assignee or a description.
    assert_eq!(t.item("ABC-12").state.status, Some(Status::InProgress));
    assert!(
        t.writes()
            .iter()
            .all(|w| w.starts_with("link ") || w.starts_with("pr status "))
    );
}

/// Little left of the GitHub quota: the sync reads nothing, keeps its
/// checkpoint, and the next pass with quota links the PR (ASDLC-5DDN).
#[test]
fn a_sync_low_on_quota_waits_and_the_next_pass_resumes() {
    let t = FakeTracker::default();
    t.seed("ABC-12", "T", "", Status::InProgress, &[]);
    let g = github();
    let pr = named(7, "feat: thing (ABC-12)");
    open(&g, vec![pr.clone()]);
    let quota = |remaining| {
        Some(Ok(vec![crate::github::Quota {
            resource: "graphql".into(),
            limit: 5000,
            remaining,
            reset: Timestamp::parse("2026-10-26T13:22:00Z").unwrap(),
        }]))
    };
    g.state.borrow_mut().quota = quota(999);
    let log = log("low-quota");
    let out = sync(&t, &g, &log);
    assert!(out.ok, "{out:?}");
    assert!(
        out.message.contains("GitHub sync skipped"),
        "{}",
        out.message
    );
    assert!(
        out.message.contains("graphql 999/5000 until 13:22Z"),
        "{}",
        out.message
    );
    assert!(t.writes().is_empty());
    assert!(
        !memory_path(&log).exists(),
        "the checkpoint is kept as it was"
    );
    g.state.borrow_mut().quota = quota(1000);
    let out = sync(&t, &g, &log);
    assert!(out.ok, "{out:?}");
    assert!(out.message.contains("1 link(s) laid"), "{}", out.message);
    assert!(out.message.contains("graphql 1000/5000"), "{}", out.message);
}

#[test]
fn a_draft_is_linked_and_a_link_a_person_removed_is_not_laid_again() {
    let t = FakeTracker::default();
    t.seed("ABC-12", "T", "", Status::InProgress, &[]);
    let g = github();
    let pr = PullRequest {
        draft: true,
        ..named(7, "feat: thing (ABC-12)")
    };
    open(&g, vec![pr.clone()]);
    let log = log("removed");
    sync(&t, &g, &log);
    assert_eq!(
        t.writes(),
        [
            format!("link ABC-12 {}", pr.url),
            format!("pr status ABC-12 {} draft", pr.url),
        ]
    );
    t.unlink("ABC-12", &pr.url);
    let out = sync(&t, &g, &log);
    assert!(out.ok, "{out:?}");
    assert_eq!(t.writes().len(), 2, "{:?}", t.writes());
    assert!(t.items_linked_to_pr(&pr.url).unwrap().is_empty());
}

#[test]
fn a_link_laid_by_another_is_used_and_an_unknown_ticket_is_skipped() {
    let t = FakeTracker::default();
    t.seed("ABC-12", "T", "", Status::InReview, &[]);
    let g = github();
    let pr = named(7, "fix: thing (ABC-99)");
    t.link("ABC-12", &pr.url);
    open(&g, vec![pr.clone()]);
    let out = sync(&t, &g, &log("other"));
    assert!(out.ok, "{out:?}");
    assert_eq!(
        t.writes(),
        [format!("pr status ABC-12 {} open", pr.url)],
        "no link to an unknown ticket, the status on the linked one"
    );
}

#[test]
fn a_merged_pr_is_released_in_the_oldest_tag_that_holds_its_commit() {
    let t = FakeTracker::default();
    t.seed("ABC-1", "T", "", Status::PostDeploy, &[]);
    t.seed("ABC-2", "T", "", Status::ToDeploy, &[]);
    t.seed("ABC-3", "T", "", Status::InReview, &[]);
    let g = github();
    let now = Timestamp::now();
    let merged_pr = |n: u64, key: &str, sha: &str| PullRequest {
        state: PrState::Merged,
        merged_at: Some(now),
        merge_commit_sha: Some(sha.into()),
        ..named(n, &format!("feat: x ({key})"))
    };
    let released = merged_pr(1, "ABC-1", "sha-1");
    let waiting = merged_pr(2, "ABC-2", "sha-2");
    merged(&g, vec![released.clone(), waiting.clone()]);
    let closed = PullRequest {
        state: PrState::Closed,
        updated_at: Some(now),
        ..named(3, "feat: x (ABC-3)")
    };
    g.state
        .borrow_mut()
        .closed_pulls
        .insert(repo(), Ok(vec![closed.clone()]));
    g.state
        .borrow_mut()
        .updated_pulls
        .get_mut(&repo())
        .unwrap()
        .as_mut()
        .unwrap()
        .push(closed.clone());
    {
        let mut s = g.state.borrow_mut();
        let tag = |name: &str| GitRef {
            name: name.into(),
            sha: format!("c-{name}"),
            object_type: ObjectType::Commit,
        };
        s.tags.insert(
            repo(),
            vec![tag("acme-v1.10.0"), tag("acme-v1.2.0"), tag("acme-v1.9.0")],
        );
        let cmp = |status| {
            Ok(Comparison {
                status,
                ahead_by: 0,
                behind_by: 0,
                commits: Vec::new(),
                files: Vec::new(),
                base_committed_at: None,
            })
        };
        for (sha, tag, status) in [
            ("sha-1", "acme-v1.10.0", CompareStatus::Ahead),
            ("sha-1", "acme-v1.2.0", CompareStatus::Diverged),
            ("sha-1", "acme-v1.9.0", CompareStatus::Ahead),
            ("sha-2", "acme-v1.10.0", CompareStatus::Behind),
        ] {
            s.compares
                .insert((repo(), sha.into(), tag.into()), cmp(status));
        }
    }
    let out = sync(&t, &g, &log("released"));
    assert!(out.ok, "{out:?}");
    let writes = t.writes();
    assert!(
        writes.contains(&format!(
            "pr status ABC-1 {} released:acme-v1.9.0",
            released.url
        )),
        "{writes:?}"
    );
    assert!(
        writes.contains(&format!("pr status ABC-2 {} merged", waiting.url)),
        "{writes:?}"
    );
    assert!(
        writes.contains(&format!("pr status ABC-3 {} closed", closed.url)),
        "{writes:?}"
    );
}

/// A PR linked to an open ticket is read by its number once it leaves the
/// window, until the ticket is resolved.
#[test]
fn a_followed_pr_is_read_until_its_tickets_are_resolved() {
    let t = FakeTracker::default();
    t.seed("ABC-12", "T", "", Status::InReview, &[]);
    let g = github();
    let pr = named(7, "feat: thing (ABC-12)");
    open(&g, vec![pr.clone()]);
    let log = log("followed");
    sync(&t, &g, &log);
    let memory = load(&memory_path(&log)).unwrap();
    assert_eq!(memory.followed.get(&pr.url), Some(&7));
    assert!(memory.since.is_some());
    // Merged long ago: out of the window, read by its number.
    let old = PullRequest {
        state: PrState::Merged,
        merged_at: Timestamp::parse("2020-01-01T00:00:00Z"),
        ..pr.clone()
    };
    open(&g, Vec::new());
    merged(&g, vec![old.clone()]);
    let out = sync(&t, &g, &log);
    assert!(out.ok, "{out:?}");
    assert_eq!(
        t.writes().last().unwrap(),
        &format!("pr status ABC-12 {} merged", pr.url)
    );
    t.set_status("ABC-12", Status::Done, None).unwrap();
    let before = t.writes().len();
    sync(&t, &g, &log);
    assert_eq!(t.writes().len(), before, "a resolved ticket gets nothing");
    assert!(load(&memory_path(&log)).unwrap().followed.is_empty());
}

#[test]
fn a_spent_budget_stops_the_pass_says_so_and_the_next_resumes() {
    let t = FakeTracker::default();
    t.seed("ABC-12", "T", "", Status::InProgress, &[]);
    let g = github();
    open(&g, vec![named(7, "feat: thing (ABC-12)")]);
    let log = log("budget");
    let out = sync_within(&t, &g, &log, Some(Duration::ZERO));
    assert!(!out.ok);
    assert!(out.message.contains("time budget"), "{}", out.message);
    assert!(t.writes().is_empty());
    assert_eq!(
        load(&memory_path(&log)).unwrap().since,
        None,
        "not advanced"
    );
    let out = sync(&t, &g, &log);
    assert!(out.ok, "{out:?}");
    assert_eq!(t.writes().len(), 2);
}

#[test]
fn a_failure_checkpoints_each_prior_pr_and_the_next_pass_resumes_after_it() {
    let t = FakeTracker::default();
    t.seed("ABC-12", "T", "", Status::InProgress, &[]);
    t.fail_writes_starting_with("link ABC-12");
    let g = github();
    let first = named(1, "chore: unrelated");
    let second = named(2, "feat: thing (ABC-12)");
    open(&g, vec![second.clone(), first.clone()]);
    let log = log("checkpoint-each");
    let out = sync(&t, &g, &log);
    assert!(!out.ok, "{out:?}");
    let memory = load(&memory_path(&log)).unwrap();
    assert_eq!(memory.since, first.updated_at.map(|t| t.to_string()));
    assert_eq!(memory.handled_at_since, BTreeSet::from([1]));

    t.clear_failing_writes();
    let out = sync(&t, &g, &log);
    assert!(out.ok, "{out:?}");
    assert_eq!(out.message.matches("PR(s) read").count(), 1);
    assert_eq!(
        load(&memory_path(&log)).unwrap().handled_at_since,
        BTreeSet::from([2])
    );
}

#[test]
fn an_old_pr_on_a_closed_ticket_only_lays_a_missing_link() {
    let t = FakeTracker::default();
    t.seed("ABC-12", "T", "", Status::Done, &[]);
    let g = github();
    let pr = named(7, "fix: history (ABC-12)");
    open(&g, vec![pr.clone()]);
    let out = sync(&t, &g, &log("closed-ticket"));
    assert!(out.ok, "{out:?}");
    assert_eq!(t.writes(), [format!("link ABC-12 {}", pr.url)]);
}

#[test]
fn without_github_or_with_an_unreadable_tracker_the_pass_fails_and_writes_nothing() {
    let t = FakeTracker::default();
    t.seed("ABC-12", "T", "", Status::InProgress, &[]);
    let g = github();
    open(&g, vec![named(7, "feat: thing (ABC-12)")]);
    g.state.borrow_mut().identity = None;
    let log = log("fails");
    let out = sync(&t, &g, &log);
    assert!(!out.ok);
    assert!(out.message.contains("GitHub identity"), "{}", out.message);
    let g = github();
    open(&g, vec![named(7, "feat: thing (ABC-12)")]);
    t.make_unreadable("identity");
    let out = sync(&t, &g, &log);
    assert!(!out.ok);
    assert!(out.message.contains("the tracker"), "{}", out.message);
    assert!(t.writes().is_empty());
    assert!(!memory_path(&log).exists(), "no memory written");
}

/// The host's entry: the configuration read at the default branch, the
/// overlay as a value.
#[test]
fn a_host_runs_the_sync_from_the_repositorys_configuration() {
    let t = FakeTracker::default();
    t.seed("ABC-12", "T", "", Status::InProgress, &[]);
    let g = github();
    g.state.borrow_mut().files.insert(
        (repo(), "main".into(), crate::config::DEFAULT_PATH.into()),
        Ok(crate::config::spec_example::repository().into()),
    );
    open(&g, vec![named(7, "feat: thing (ABC-12)")]);
    let host = RepositoryHost {
        repository: repo(),
        instance: Some(crate::config::Source {
            label: "the host's overlay".into(),
            text: crate::config::spec_example::instance().into(),
        }),
        tracker: &t,
        budget: Some(Duration::from_secs(60)),
        log_path: Some(log("host")),
        env: &|_| None,
    };
    let out = pass_from_repository(&host, &g);
    assert!(out.ok, "{out:?}");
    assert_eq!(t.writes().len(), 2);
}
