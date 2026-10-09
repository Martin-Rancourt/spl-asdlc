//! Carried from `tests/test_pr_merge.py` (exclusions, triggers, approval,
//! up-to-date) and `tests/test_review_paths.py` (the list's format).

use super::*;
use crate::github::ReviewThread;
use crate::github::fake::pull_request;

fn review_config() -> config::Review {
    crate::config::spec_example::config().review
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn facts<'a>(
    paths: Option<&'a [String]>,
    sensitive: &'a SensitivePaths,
    labels: &'a [String],
) -> ReviewFacts<'a> {
    ReviewFacts {
        labels,
        changed_paths: paths,
        additions: 10,
        deletions: 2,
        sensitive,
        skipped_required: None,
    }
}

// ---- exclusions (REQ-MERGE-003..005) ----

#[test]
fn a_release_please_pr_is_never_touched() {
    let mut pr = pull_request(1);
    pr.head_ref = "release-please--branches--release/2026-10-05-14-03-utc".into();
    assert!(excluded_from_merge_pass(&pr, "release/", "autorelease: pending").is_some());
    let mut titled = pull_request(2);
    titled.title = "chore(main): release acme 1.2.3".into();
    assert!(excluded_from_merge_pass(&titled, "release/", "autorelease: pending").is_some());
    let mut labelled = pull_request(3);
    labelled.labels = s(&["autorelease: pending"]);
    assert!(excluded_from_merge_pass(&labelled, "release/", "autorelease: pending").is_some());
}

#[test]
fn a_gate_record_pr_is_never_merged_by_the_pass() {
    let mut pr = pull_request(1);
    pr.head_ref = "release-record/acme-v1.2.3-rc.1".into();
    assert_eq!(
        excluded_from_merge_pass(&pr, "release/", "autorelease: pending"),
        Some("gate-record PR — the release cycle merges it")
    );
}

#[test]
fn a_merge_back_is_recognised_by_its_head_never_its_base() {
    let mut back = pull_request(1);
    back.head_ref = "release/2026-10-05-14-03-utc".into();
    assert!(
        excluded_from_merge_pass(&back, "release/", "x")
            .unwrap()
            .contains("merge back")
    );
    let mut hotfix = pull_request(2);
    hotfix.base_ref = "release/2026-10-05-14-03-utc".into();
    assert_eq!(
        excluded_from_merge_pass(&hotfix, "release/", "x"),
        None,
        "a hotfix is ordinary"
    );
    let mut resolution = pull_request(3);
    resolution.head_ref = "chore/merge-back-2026-10-05".into();
    assert!(excluded_from_merge_pass(&resolution, "release/", "x").is_some());
    let mut backlog = pull_request(4);
    backlog.head_ref = "chore/merge-backlog".into();
    assert_eq!(excluded_from_merge_pass(&backlog, "release/", "x"), None);
}

#[test]
fn an_unreadable_head_is_treated_as_a_merge_back() {
    let mut pr = pull_request(1);
    pr.head_ref = String::new();
    assert!(
        excluded_from_merge_pass(&pr, "release/", "x")
            .unwrap()
            .contains("could not be read")
    );
}

#[test]
fn an_angle_pr_belongs_to_the_release_cycle() {
    let mut pr = pull_request(1);
    pr.head_ref = "qa/acme-v1.2.3-rc.1/security".into();
    pr.base_ref = "release/2026-10-05-14-03-utc".into();
    assert!(excluded_from_merge_pass(&pr, "release/", "x").is_some());
}

// ---- up to date (REQ-MERGE-016) ----

#[test]
fn conflicting_behind_and_everything_else() {
    let mut pr = pull_request(1);
    pr.merge_state = "DIRTY".into();
    assert_eq!(branch_state(&pr), BranchState::Conflicting);
    pr.merge_state = "CLEAN".into();
    pr.mergeable = Mergeable::Conflicting;
    assert_eq!(branch_state(&pr), BranchState::Conflicting);
    pr.mergeable = Mergeable::Mergeable;
    pr.merge_state = "BEHIND".into();
    assert_eq!(branch_state(&pr), BranchState::Behind);
    for state in ["CLEAN", "BLOCKED", "UNSTABLE", "HAS_HOOKS", "UNKNOWN", ""] {
        pr.merge_state = state.into();
        assert_eq!(branch_state(&pr), BranchState::Current, "{state}");
    }
}

// ---- the sensitive-path list (REQ-MERGE-045) ----

#[test]
fn parse_repo_paths_keeps_the_paths_and_ignores_the_rest() {
    let text = r#"{"_doc":"x","paths":["src/auth/**",".github/review-paths.json"],"exceptions":[{"path":"a","reason":"b"}]}"#;
    assert_eq!(
        parse_sensitive_paths(text).unwrap(),
        s(&["src/auth/**", ".github/review-paths.json"])
    );
}

#[test]
fn parse_repo_paths_refuses_what_it_cannot_trust() {
    for bad in [
        "not json",
        "[]",
        r#"{"other":1}"#,
        r#"{"paths":[]}"#,
        r#"{"paths":"src/**"}"#,
        r#"{"paths":[""]}"#,
        r#"{"paths":["  "]}"#,
        r#"{"paths":[1]}"#,
    ] {
        assert!(parse_sensitive_paths(bad).is_err(), "{bad}");
    }
}

#[test]
fn the_repositorys_own_list_buys_a_reviewer() {
    let config = review_config();
    let paths = s(&["src/auth/login.rs"]);
    let sensitive = SensitivePaths::Paths(s(&["src/auth/**"]));
    let plan = plan(&facts(Some(&paths), &sensitive, &[]), &config);
    assert!(plan.angles.contains(&"security".to_string()));
    assert!(plan.reasons[0].contains("`src/auth/login.rs`"));
    assert!(plan.holds.is_empty());
}

#[test]
fn an_unreadable_list_fails_closed_and_never_falls_back() {
    let config = review_config();
    let paths = s(&["docs/readme.md"]);
    let sensitive = SensitivePaths::Invalid(".github/review-paths.json is absent at main".into());
    let plan = plan(&facts(Some(&paths), &sensitive, &[]), &config);
    assert!(plan.holds[0].starts_with("configuration invalid for this repository"));
}

#[test]
fn no_declared_list_means_no_sensitive_path() {
    let config = review_config();
    let paths = s(&["src/auth/login.rs"]);
    let plan = plan(
        &facts(Some(&paths), &SensitivePaths::NotDeclared, &[]),
        &config,
    );
    assert!(plan.holds.is_empty());
    assert!(plan.sensitive_hits.is_empty());
}

// ---- triggers (REQ-MERGE-044..050) ----

#[test]
fn every_round_carries_the_base_angles() {
    let config = review_config();
    let paths = s(&["src/x.rs"]);
    let plan = plan(
        &facts(Some(&paths), &SensitivePaths::NotDeclared, &[]),
        &config,
    );
    assert_eq!(plan.angles, s(&["maintainability"]));
    assert!(plan.reasons.is_empty());
}

#[test]
fn the_authors_flag_is_a_reason() {
    let config = review_config();
    let paths = s(&["src/x.rs"]);
    let labels = s(&[" Needs-Review "]);
    let plan = plan(
        &facts(Some(&paths), &SensitivePaths::NotDeclared, &labels),
        &config,
    );
    assert_eq!(plan.reasons[0], "the author flagged it (`needs-review`)");
}

#[test]
fn an_unreadable_diff_buys_a_reviewer_instead_of_a_merge() {
    let config = review_config();
    let plan = plan(&facts(None, &SensitivePaths::NotDeclared, &[]), &config);
    assert!(plan.holds[0].contains("the diff could not be read"));
    assert!(plan.angles.contains(&"security".to_string()));
}

#[test]
fn an_empty_file_list_that_contradicts_the_pr_is_not_an_answer() {
    let config = review_config();
    let empty: Vec<String> = Vec::new();
    let plan = plan(
        &facts(Some(&empty), &SensitivePaths::NotDeclared, &[]),
        &config,
    );
    assert!(!plan.holds.is_empty());
}

#[test]
fn a_diff_read_as_genuinely_empty_still_merges() {
    let config = review_config();
    let empty: Vec<String> = Vec::new();
    let mut f = facts(Some(&empty), &SensitivePaths::NotDeclared, &[]);
    f.additions = 0;
    f.deletions = 0;
    assert!(plan(&f, &config).holds.is_empty());
}

#[test]
fn a_skipped_required_check_holds_the_merge() {
    let config = review_config();
    let paths = s(&["src/x.rs"]);
    let mut f = facts(Some(&paths), &SensitivePaths::NotDeclared, &[]);
    f.skipped_required = Some("required-check");
    let plan = plan(&f, &config);
    assert_eq!(
        plan.holds,
        vec!["the required check `required-check` was skipped, so green does not mean tested"]
    );
}

#[test]
fn the_default_threshold_was_raised_for_agent_produced_diffs() {
    let config = review_config();
    let paths = s(&["src/x.rs"]);
    let mut f = facts(Some(&paths), &SensitivePaths::NotDeclared, &[]);
    f.additions = 1400;
    f.deletions = 0;
    assert!(
        !plan(&f, &config)
            .reasons
            .iter()
            .any(|r| r.contains("threshold"))
    );
    f.additions = 1600;
    let p = plan(&f, &config);
    assert!(
        p.reasons
            .iter()
            .any(|r| r == "it changes 1600 lines, over the 1500-line review threshold")
    );
}

#[test]
fn a_path_angle_is_added_by_its_globs() {
    let config = review_config();
    let paths = s(&["db/migrations/001.sql"]);
    let plan = plan(
        &facts(Some(&paths), &SensitivePaths::NotDeclared, &[]),
        &config,
    );
    assert_eq!(plan.angles, s(&["maintainability", "security"]));
}

#[test]
fn reviewed_paths_are_named_five_then_counted() {
    let config = review_config();
    let paths: Vec<String> = (0..7).map(|i| format!("src/auth/{i}.rs")).collect();
    let sensitive = SensitivePaths::Paths(s(&["src/auth/**"]));
    let plan = plan(&facts(Some(&paths), &sensitive, &[]), &config);
    assert!(
        plan.reasons[0].ends_with("(+2 more)"),
        "{}",
        plan.reasons[0]
    );
}

// ---- approval (REQ-MERGE-055, the owner's decision) ----

fn review(login: &str, state: &str, sha: &str) -> Review {
    Review {
        body: String::new(),
        author: Some(login.into()),
        state: state.into(),
        commit_sha: Some(sha.into()),
        submitted_at: None,
    }
}

#[test]
fn a_current_approval_at_the_head_qualifies() {
    let reviews = [review("Coordinator-Login", "APPROVED", "h1")];
    assert_eq!(
        approval(&reviews, &s(&["coordinator-login"]), "h1"),
        Approval::Qualified {
            by: "Coordinator-Login".into()
        }
    );
}

#[test]
fn an_approval_at_an_older_head_sha_does_not_qualify() {
    let reviews = [review("coord", "APPROVED", "h0")];
    assert_eq!(
        approval(&reviews, &s(&["coord"]), "h1"),
        Approval::Stale {
            by: "coord".into(),
            sha: "h0".into()
        }
    );
}

#[test]
fn a_later_changes_requested_holds_even_next_to_an_earlier_approval() {
    let reviews = [
        review("coord", "APPROVED", "h1"),
        review("coord", "CHANGES_REQUESTED", "h1"),
    ];
    assert_eq!(
        approval(&reviews, &s(&["coord"]), "h1"),
        Approval::ChangesRequested { by: "coord".into() }
    );
}

#[test]
fn a_comment_never_cancels_an_approval() {
    let reviews = [
        review("coord", "APPROVED", "h1"),
        review("coord", "COMMENTED", "h1"),
    ];
    assert!(matches!(
        approval(&reviews, &s(&["coord"]), "h1"),
        Approval::Qualified { .. }
    ));
}

#[test]
fn a_new_approval_lifts_an_earlier_request_for_changes() {
    let reviews = [
        review("coord", "CHANGES_REQUESTED", "h0"),
        review("coord", "APPROVED", "h1"),
    ];
    assert!(matches!(
        approval(&reviews, &s(&["coord"]), "h1"),
        Approval::Qualified { .. }
    ));
}

#[test]
fn an_unrelated_approval_does_not_qualify() {
    let reviews = [review("someone-else", "APPROVED", "h1")];
    assert_eq!(approval(&reviews, &s(&["coord"]), "h1"), Approval::Missing);
}

#[test]
fn a_dismissed_or_pending_review_changes_nothing() {
    let reviews = [
        review("coord", "APPROVED", "h1"),
        review("coord", "DISMISSED", "h1"),
        review("coord", "PENDING", "h1"),
    ];
    assert!(matches!(
        approval(&reviews, &s(&["coord"]), "h1"),
        Approval::Qualified { .. }
    ));
}

#[test]
fn a_review_without_a_login_is_ignored() {
    let reviews = [Review {
        body: String::new(),
        author: None,
        state: "CHANGES_REQUESTED".into(),
        commit_sha: None,
        submitted_at: None,
    }];
    assert_eq!(approval(&reviews, &s(&["coord"]), "h1"), Approval::Missing);
}

#[test]
fn unresolved_threads_are_counted_outdated_or_not() {
    let mut pr = pull_request(1);
    let thread = |resolved, outdated| ReviewThread {
        resolved,
        outdated,
        path: None,
        first_author: None,
        first_body: String::new(),
    };
    pr.threads = vec![
        thread(true, false),
        thread(false, true),
        thread(false, false),
    ];
    assert_eq!(unresolved_threads(&pr), Some(2));
    pr.truncated = vec!["reviewThreads".into()];
    assert_eq!(
        unresolved_threads(&pr),
        None,
        "a cut thread list is not read as resolved"
    );
}

#[test]
fn human_approval_follows_the_policy() {
    let mut config = review_config();
    let plan_hit = ReviewPlan {
        sensitive_hits: s(&["src/auth/a.rs"]),
        ..ReviewPlan::default()
    };
    let plan_clean = ReviewPlan::default();
    config.human_approval = config::HumanApproval::Never;
    assert!(!needs_human_approval(&config, &plan_hit));
    config.human_approval = config::HumanApproval::Always;
    assert!(needs_human_approval(&config, &plan_clean));
    config.human_approval = config::HumanApproval::SensitivePaths;
    assert!(needs_human_approval(&config, &plan_hit));
    assert!(!needs_human_approval(&config, &plan_clean));
}

// ---- an App's login, however it is spelt (SID-1008) ----

/// The login REST and the UI show for an App.
fn bot(slug: &str) -> String {
    format!("{slug}[bot]")
}

#[test]
fn an_app_approval_qualifies_however_either_side_spells_it() {
    // (configured, as GitHub reported the review's author)
    let spellings = [
        (bot("coord-app"), "coord-app".to_string()),
        ("coord-app".to_string(), bot("coord-app")),
        (bot("Coord-App"), "coord-app".to_string()),
        ("coord-app".to_string(), bot("COORD-APP")),
        (bot("coord-app"), bot("coord-app")),
    ];
    for (configured, author) in spellings {
        let reviews = [review(&author, "APPROVED", "h1")];
        assert_eq!(
            approval(&reviews, std::slice::from_ref(&configured), "h1"),
            Approval::Qualified { by: author.clone() },
            "configured {configured}, reviewed by {author}"
        );
    }
    let other = [review(&bot("coord-app-2"), "APPROVED", "h1")];
    assert_eq!(
        approval(&other, &[bot("coord-app")], "h1"),
        Approval::Missing,
        "a longer slug is another App"
    );
}

#[test]
fn an_app_spelt_two_ways_is_one_reviewer() {
    // The approval under one spelling, the request for changes under the
    // other: the later one stands.
    let reviews = [
        review(&bot("coord-app"), "APPROVED", "h1"),
        review("coord-app", "CHANGES_REQUESTED", "h1"),
    ];
    assert_eq!(
        approval(&reviews, &[bot("coord-app")], "h1"),
        Approval::ChangesRequested {
            by: "coord-app".into()
        }
    );
}

#[test]
fn a_standing_request_for_changes_is_found_however_the_app_is_spelt() {
    let mut pr = pull_request(1);
    let head = pr.head_sha.clone();
    for (configured, author) in [
        (bot("coord-app"), "Coord-App".to_string()),
        ("coord-app".to_string(), bot("coord-app")),
    ] {
        pr.reviews = vec![review(&author, "CHANGES_REQUESTED", &head)];
        assert_eq!(
            changes_requested_at_head(&pr, std::slice::from_ref(&configured)),
            Some(author.clone()),
            "configured {configured}, reviewed by {author}"
        );
    }
    pr.reviews = vec![review("coord-app", "CHANGES_REQUESTED", "older")];
    assert_eq!(
        changes_requested_at_head(&pr, &[bot("coord-app")]),
        None,
        "a request on an older head is not standing at this one"
    );
    pr.reviews = vec![
        review("coord-app", "CHANGES_REQUESTED", &head),
        review(&bot("coord-app"), "APPROVED", &head),
    ];
    assert_eq!(changes_requested_at_head(&pr, &[bot("coord-app")]), None);
}

#[test]
fn the_approvals_at_the_head_are_named_as_github_spelt_them() {
    let reviews = [
        review("dev", "APPROVED", "h1"),
        review("coord-app", "APPROVED", "h0"),
        review("other", "APPROVED", "h1"),
        review("other", "CHANGES_REQUESTED", "h1"),
        review(&bot("qa-app"), "APPROVED", "h1"),
    ];
    assert_eq!(
        approvals_at_head(&reviews, "h1"),
        vec!["dev".to_string(), bot("qa-app")]
    );
}

#[test]
fn a_cut_review_list_never_qualifies() {
    let mut pr = pull_request(1);
    let head = pr.head_sha.clone();
    pr.reviews = vec![review("coord", "APPROVED", &head)];
    assert!(matches!(
        pr_approval(&pr, &s(&["coord"])),
        Approval::Qualified { .. }
    ));
    pr.truncated = vec!["reviews".into()];
    assert_eq!(pr_approval(&pr, &s(&["coord"])), Approval::Unreadable);
}
