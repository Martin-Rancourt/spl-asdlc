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

/// `n` minutes into the day: PRs past the 60th still sort by number.
fn minute(n: u64) -> Option<Timestamp> {
    Timestamp::parse("2026-10-25T00:00:00Z")
        .map(|t| Timestamp(t.0 + time::Duration::minutes(n as i64)))
}

/// PRs `1..=n` naming no ticket, updated one minute apart.
fn unrelated(n: u64) -> Vec<PullRequest> {
    (1..=n)
        .map(|i| PullRequest {
            updated_at: minute(i),
            ..named(i, "chore: unrelated")
        })
        .collect()
}

fn checkpoint(log: &Path) -> (Option<String>, BTreeSet<u64>) {
    let m = load(&memory_path(log)).unwrap();
    (m.since, m.handled_at_since)
}

/// ASDLC-47PN: a backlog after the checkpoint is read 25 PRs a request and
/// at most 50 a pass; the next pass goes on after the last one.
#[test]
fn a_catch_up_reads_batches_of_25_and_spreads_over_passes() {
    let t = FakeTracker::default();
    let g = github();
    open(&g, unrelated(60));
    let log = log("catch-up");
    let out = sync(&t, &g, &log);
    assert!(out.ok, "{out:?}");
    assert!(out.message.contains("50 PR(s) read"), "{}", out.message);
    assert!(
        out.message
            .contains("the next pass goes on after the checkpoint"),
        "{}",
        out.message
    );
    let reads = g.state.borrow().sync_reads.clone();
    assert_eq!(
        reads,
        [(1..=25).collect::<Vec<u64>>(), (26..=50).collect()],
        "two batches of 25, the oldest first"
    );
    assert_eq!(
        checkpoint(&log),
        (minute(50).map(|t| t.to_string()), BTreeSet::from([50]))
    );
    let out = sync(&t, &g, &log);
    assert!(out.ok, "{out:?}");
    assert!(out.message.contains("10 PR(s) read"), "{}", out.message);
    assert_eq!(
        g.state.borrow().sync_reads.last().unwrap(),
        &(51..=60).collect::<Vec<u64>>()
    );
}

/// ASDLC-47PN: a pass over its budget keeps the checkpoint of every PR it
/// handled, says where the next one resumes, and the next one does.
#[test]
fn an_over_budget_pass_keeps_its_progress_and_the_next_resumes_after_it() {
    let t = FakeTracker::default();
    let g = github();
    open(&g, unrelated(40));
    // GitHub is slow on the second batch: the budget is spent when it answers.
    g.state
        .borrow_mut()
        .slow_sync_reads
        .insert(30, Duration::from_millis(300));
    let log = log("over-budget");
    let out = sync_within(&t, &g, &log, Some(Duration::from_millis(200)));
    assert!(out.ok, "a step of a catch-up, not a failure: {out:?}");
    assert!(out.message.contains("25 PR(s) read"), "{}", out.message);
    let at = minute(25).unwrap().to_string();
    assert!(
        out.message.contains(&format!(
            "the time budget is spent: the next pass resumes after the checkpoint {at}"
        )),
        "{}",
        out.message
    );
    assert_eq!(checkpoint(&log), (Some(at), BTreeSet::from([25])));

    g.state.borrow_mut().slow_sync_reads.clear();
    let out = sync(&t, &g, &log);
    assert!(out.ok, "{out:?}");
    assert!(out.message.contains("15 PR(s) read"), "{}", out.message);
    assert_eq!(
        g.state.borrow().sync_reads.last().unwrap(),
        &(26..=40).collect::<Vec<u64>>()
    );
    assert_eq!(checkpoint(&log).0, minute(40).map(|t| t.to_string()));
}

/// ASDLC-47PN: a 504 on one batch keeps the batches before it; the pass
/// says the error on its line.
#[test]
fn a_504_on_one_batch_keeps_the_progress_of_the_batches_before_it() {
    let t = FakeTracker::default();
    let g = github();
    open(&g, unrelated(40));
    g.state.borrow_mut().failing_sync_reads.insert(
        30,
        crate::github::ReadError::Unreadable {
            what: "PRs #26… of example-org/acme".into(),
            status: Some(504),
            detail: "HTTP 504: <!DOCTYPE html>".into(),
        },
    );
    let log = log("504");
    let out = sync(&t, &g, &log);
    assert!(!out.ok, "{out:?}");
    assert!(!out.message.contains('\n'), "one line: {}", out.message);
    assert!(out.message.contains("25 PR(s) read"), "{}", out.message);
    assert!(
        out.message
            .contains("1 failure(s): the PRs after the checkpoint:")
            && out.message.contains("504"),
        "{}",
        out.message
    );
    assert_eq!(checkpoint(&log).0, minute(25).map(|t| t.to_string()));

    g.state.borrow_mut().failing_sync_reads.clear();
    let out = sync(&t, &g, &log);
    assert!(out.ok, "{out:?}");
    assert!(out.message.contains("15 PR(s) read"), "{}", out.message);
}

/// Livela's sync failed every pass on « 1 PR(s) read » and no reason: the
/// failure was on a second line the host's job line drops, and the failing
/// PR held the checkpoint back for good. Now the reason is on the line, the
/// PRs after it go on, and it is read again until it goes through.
#[test]
fn a_pr_that_fails_says_why_on_the_line_and_no_longer_holds_back_the_others() {
    let t = FakeTracker::default();
    t.seed("ABC-12", "T", "", Status::InProgress, &[]);
    t.fail_writes_starting_with("link ABC-12");
    let g = github();
    let first = named(1, "chore: unrelated");
    let second = named(2, "feat: thing (ABC-12)");
    let third = named(3, "chore: unrelated");
    open(&g, vec![second.clone(), first.clone(), third.clone()]);
    let log = log("checkpoint-each");
    let out = sync(&t, &g, &log);
    assert!(!out.ok, "{out:?}");
    assert!(!out.message.contains('\n'), "one line: {}", out.message);
    assert!(out.message.contains("3 PR(s) read"), "{}", out.message);
    assert!(
        out.message.contains("; 1 failure(s): PR #2: ") && out.message.contains("link ABC-12"),
        "the reason: {}",
        out.message
    );
    assert_eq!(
        checkpoint(&log),
        (third.updated_at.map(|t| t.to_string()), BTreeSet::from([3]))
    );
    assert_eq!(
        load(&memory_path(&log)).unwrap().followed.get(&second.url),
        Some(&2),
        "read again until it goes through"
    );

    t.clear_failing_writes();
    let out = sync(&t, &g, &log);
    assert!(out.ok, "{out:?}");
    assert!(
        out.message.contains("1 PR(s) read, 1 link(s) laid"),
        "{}",
        out.message
    );
    assert_eq!(
        t.writes(),
        [
            format!("REFUSED link ABC-12 {}", second.url),
            format!("link ABC-12 {}", second.url),
            format!("pr status ABC-12 {} open", second.url),
        ]
    );
}

/// The checkpoint takes the update time of the list, not of the PR read
/// after it: a PR updated in between is listed again by the next pass, and
/// the checkpoint never jumps past a row the pass did not handle.
#[test]
fn the_checkpoint_takes_the_lists_update_time() {
    let t = FakeTracker::default();
    let g = github();
    let mut prs = unrelated(3);
    // #2 was updated after the list was read: read with a later time.
    prs[1].updated_at = minute(30);
    open(&g, prs);
    {
        let mut s = g.state.borrow_mut();
        s.listed_at.insert((repo(), 2), minute(2).unwrap());
        s.listed_at.insert((repo(), 3), minute(3).unwrap());
    }
    let log = log("list-time");
    let out = sync(&t, &g, &log);
    assert!(out.ok, "{out:?}");
    assert_eq!(
        checkpoint(&log),
        (minute(3).map(|t| t.to_string()), BTreeSet::from([3]))
    );
}

/// More PRs followed than a pass reads: each pass goes on from where the
/// last one stopped, so every one is read in turn.
#[test]
fn followed_prs_are_read_in_turn_a_bounded_number_per_pass() {
    let t = FakeTracker::default();
    t.seed("ABC-12", "T", "", Status::InReview, &[]);
    let g = github();
    let prs = unrelated(120);
    for p in &prs {
        t.link("ABC-12", &p.url);
    }
    {
        let mut s = g.state.borrow_mut();
        s.open_pulls.insert(repo(), Ok(prs.clone()));
    }
    let log = log("followed-turn");
    let memory = Memory {
        since: minute(1000).map(|t| t.to_string()),
        followed: prs.iter().map(|p| (p.url.clone(), p.number)).collect(),
        ..Memory::default()
    };
    save(&memory_path(&log), &memory).unwrap();
    let read = |g: &FakeGitHub| -> BTreeSet<u64> {
        g.state
            .borrow_mut()
            .sync_reads
            .drain(..)
            .flatten()
            .collect()
    };
    let out = sync(&t, &g, &log);
    assert!(out.ok, "{out:?}");
    assert!(
        out.message.contains("100 of 120 PR(s) followed read"),
        "{}",
        out.message
    );
    let first = read(&g);
    assert_eq!(first.len(), 100);
    let out = sync(&t, &g, &log);
    assert!(out.ok, "{out:?}");
    let second = read(&g);
    let all: BTreeSet<u64> = (1..=120).collect();
    assert!(
        all.difference(&first).all(|n| second.contains(n)),
        "the second pass reads the ones the first did not"
    );
}

/// Hundreds of release tags: the oldest that holds a commit is found in a
/// few compares, not one per tag (one PR took most of a pass's budget).
#[test]
fn a_release_is_found_in_a_few_compares_among_hundreds_of_tags() {
    let t = FakeTracker::default();
    t.seed("ABC-1", "T", "", Status::PostDeploy, &[]);
    let g = github();
    let pr = PullRequest {
        state: PrState::Merged,
        merged_at: Some(Timestamp::now()),
        merge_commit_sha: Some("sha-1".into()),
        ..named(1, "feat: x (ABC-1)")
    };
    merged(&g, vec![pr.clone()]);
    {
        let mut s = g.state.borrow_mut();
        let names: Vec<String> = (0..263).map(|i| format!("acme-v1.{i}.0")).collect();
        s.tags.insert(
            repo(),
            names
                .iter()
                .map(|n| GitRef {
                    name: n.clone(),
                    sha: format!("c-{n}"),
                    object_type: ObjectType::Commit,
                })
                .collect(),
        );
        for (i, n) in names.iter().enumerate() {
            let status = if i >= 200 {
                CompareStatus::Ahead
            } else {
                CompareStatus::Behind
            };
            s.compares.insert(
                (repo(), "sha-1".into(), n.clone()),
                Ok(Comparison {
                    status,
                    ahead_by: 0,
                    behind_by: 0,
                    commits: Vec::new(),
                    files: Vec::new(),
                    base_committed_at: None,
                }),
            );
        }
    }
    let out = sync(&t, &g, &log("many-tags"));
    assert!(out.ok, "{out:?}");
    assert!(
        t.writes().contains(&format!(
            "pr status ABC-1 {} released:acme-v1.200.0",
            pr.url
        )),
        "{:?}",
        t.writes()
    );
    let compares = g.state.borrow().compares_read;
    assert!(compares <= 10, "{compares} compares");
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
