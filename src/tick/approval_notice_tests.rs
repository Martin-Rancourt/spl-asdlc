use super::*;

fn assert_replay_writes_nothing(w: &World) {
    let before = (w.tracker.writes().len(), w.github_writes().len());
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert_eq!((w.tracker.writes().len(), w.github_writes().len()), before);
}

fn waiting(w: &World) -> PullRequest {
    let mut pr = in_review(w, 7);
    pr.reviews = angle_reviews(w, &pr);
    pr
}

fn notices(w: &World, kind: &str) -> usize {
    w.github.state.borrow().comments[&(repo(), 7)]
        .as_ref()
        .unwrap()
        .iter()
        .filter(|c| c.body.contains(&format!("spl-asdlc:{kind}")))
        .count()
}

#[test]
fn outsider_approvals_are_explained_once_per_pr_and_full_head() {
    let w = World::new();
    let mut pr = waiting(&w);
    pr.reviews
        .push(review("outsider", "APPROVED", &pr.head_sha, "ok"));
    w.open(pr.clone());
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert_eq!(notices(&w, "non-coordinator-approval"), 1);
    let comments = w.github.state.borrow();
    let body = &comments.comments[&(repo(), 7)]
        .as_ref()
        .unwrap()
        .last()
        .unwrap()
        .body;
    assert!(
        body.contains("outsider") && body.contains("coordinator-login"),
        "{body}"
    );
    drop(comments);
    assert_replay_writes_nothing(&w);
    pr.head_sha = "head-7-another-full-sha".into();
    pr.reviews
        .push(review("outsider", "APPROVED", &pr.head_sha, "ok"));
    w.open(pr);
    w.tick(false);
    assert_eq!(notices(&w, "non-coordinator-approval"), 2);
}

#[test]
fn an_author_who_is_every_app_coordinator_is_explained_on_pr_and_consolidation() {
    let mut w = World::new();
    w.config.review.coordinators = vec![bot("coord-app"), "person-coordinator".into()];
    let mut pr = waiting(&w);
    pr.author = Some(Actor {
        login: bot("COORD-APP"),
        is_bot: true,
    });
    w.open(pr);
    let (report, _) = w.tick(false);
    assert!(report.ok(), "{report:?}");
    assert_eq!(notices(&w, "author-coordinator"), 1);
    let consolidation = children_of(&w, "ABC-7")
        .into_iter()
        .find(|c| {
            matches!(
                texts::bridge_item(&c.description, c.parent.as_deref()),
                Some(BridgeItem::Consolidation { .. })
            )
        })
        .unwrap();
    let comments = crate::tracker::Tracker::comments(&w.tracker, &consolidation.key).unwrap();
    let body = &comments
        .iter()
        .find(|c| c.body.contains("author-coordinator"))
        .unwrap()
        .body;
    assert!(
        body.contains("person-coordinator") && body.contains("GitHub refuses"),
        "{body}"
    );
    assert_replay_writes_nothing(&w);
}

#[test]
fn ordinary_wait_qualifying_approval_and_another_app_coordinator_stay_quiet() {
    for mode in ["empty", "qualified", "other-app", "superseded", "old"] {
        let mut w = World::new();
        if mode == "other-app" {
            w.config.review.coordinators = vec![bot("coord-app"), bot("other-app")];
        }
        let mut pr = waiting(&w);
        pr.author = Some(Actor {
            login: bot("coord-app"),
            is_bot: true,
        });
        match mode {
            "qualified" => {
                pr.reviews
                    .push(review("coordinator-login", "APPROVED", &pr.head_sha, "ok"))
            }
            "superseded" => {
                pr.reviews
                    .push(review("outsider", "APPROVED", &pr.head_sha, "ok"));
                pr.reviews.push(review(
                    "outsider",
                    "CHANGES_REQUESTED",
                    &pr.head_sha,
                    "redo",
                ));
            }
            "old" => pr.reviews.push(review("outsider", "APPROVED", "old", "ok")),
            _ => {}
        }
        w.open(pr);
        w.tick(false);
        assert_eq!(notices(&w, "non-coordinator-approval"), 0, "{mode}");
        assert_eq!(notices(&w, "author-coordinator"), 0, "{mode}");
    }
}

#[test]
fn another_hold_suppresses_the_approval_notice() {
    for mode in ["angle", "thread", "truncated"] {
        let w = World::new();
        let mut pr = if mode == "angle" {
            in_review(&w, 7)
        } else {
            waiting(&w)
        };
        pr.reviews
            .push(review("outsider", "APPROVED", &pr.head_sha, "ok"));
        if mode == "thread" {
            pr.threads.push(ReviewThread {
                resolved: false,
                outdated: false,
                path: None,
            });
        }
        if mode == "truncated" {
            pr.truncated.push("reviews".into());
        }
        w.open(pr);
        w.tick(false);
        assert_eq!(notices(&w, "non-coordinator-approval"), 0, "{mode}");
    }
}

#[test]
fn author_trap_dry_run_and_a_qualifying_person_approval_write_no_notices() {
    let mut w = World::new();
    w.config.review.coordinators = vec!["app/coord-app".into(), "person-coordinator".into()];
    let mut pr = waiting(&w);
    pr.author = Some(Actor {
        login: bot("coord-app"),
        is_bot: true,
    });
    w.open(pr.clone());
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
    assert_eq!((w.tracker.writes().len(), w.github_writes().len()), before);
    pr.reviews
        .push(review("person-coordinator", "APPROVED", &pr.head_sha, "ok"));
    pr.reviews
        .push(review("outsider", "APPROVED", &pr.head_sha, "ok"));
    w.open(pr);
    w.tick(false);
    assert_eq!(notices(&w, "non-coordinator-approval"), 0);
    assert_eq!(notices(&w, "author-coordinator"), 0);
    assert!(w.github_writes().iter().any(|s| s.starts_with("merge ")));
}

#[test]
fn an_author_app_without_a_person_coordinator_says_none_is_configured() {
    let mut w = World::new();
    w.config.review.coordinators = vec!["app/coord-app".into()];
    let mut pr = waiting(&w);
    pr.author = Some(Actor {
        login: bot("coord-app"),
        is_bot: true,
    });
    w.open(pr);
    w.tick(false);
    let state = w.github.state.borrow();
    assert!(
        state.comments[&(repo(), 7)]
            .as_ref()
            .unwrap()
            .iter()
            .any(|c| c.body.contains("author-coordinator") && c.body.contains("none configured"))
    );
}
