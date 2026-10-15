//! Carried from `tests/test_pr_merge.py` (exclusions, triggers, approval,
//! up-to-date); the angles' paths replace its sensitive-path list.

use super::*;
use crate::github::ReviewThread;
use crate::github::fake::pull_request;

fn review_config() -> config::Review {
    crate::config::spec_example::config().review
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn facts<'a>(paths: Option<&'a [String]>, labels: &'a [String]) -> ReviewFacts<'a> {
    ReviewFacts {
        labels,
        changed_paths: paths,
        additions: 10,
        deletions: 2,
        skipped_required: None,
    }
}

// ---- exclusions (REQ-MERGE-003..005) ----

#[test]
fn a_release_shaped_pr_is_an_ordinary_pr_now_that_the_cycle_has_no_release_pr() {
    // The cycle tags its release itself (spec 05 §9): a PR from a
    // release-please head, a release title or label, or a former gate-record
    // head is judged like any other PR, never merged on a GO.
    for (head, title, labels) in [
        (
            "release-please--branches--main",
            "chore(main): release acme 1.2.3",
            vec![],
        ),
        ("feat/x", "feat: x", s(&["autorelease: pending"])),
        ("release-record/acme-v1.2.3-rc.1", "docs: record", vec![]),
    ] {
        let mut pr = pull_request(1);
        pr.head_ref = head.into();
        pr.title = title.into();
        pr.labels = labels;
        assert_eq!(excluded_from_merge_pass(&pr, "release/"), None, "{head}");
    }
}

#[test]
fn a_merge_back_is_recognised_by_its_head_never_its_base() {
    let mut back = pull_request(1);
    back.head_ref = "release/2026-10-05-14-03-utc".into();
    assert!(
        excluded_from_merge_pass(&back, "release/")
            .unwrap()
            .contains("merge back")
    );
    let mut hotfix = pull_request(2);
    hotfix.base_ref = "release/2026-10-05-14-03-utc".into();
    assert_eq!(
        excluded_from_merge_pass(&hotfix, "release/"),
        None,
        "a hotfix is ordinary"
    );
    let mut resolution = pull_request(3);
    resolution.head_ref = "chore/merge-back-2026-10-05".into();
    assert!(excluded_from_merge_pass(&resolution, "release/").is_some());
    let mut backlog = pull_request(4);
    backlog.head_ref = "chore/merge-backlog".into();
    assert_eq!(excluded_from_merge_pass(&backlog, "release/"), None);
}

#[test]
fn an_unreadable_head_is_treated_as_a_merge_back() {
    let mut pr = pull_request(1);
    pr.head_ref = String::new();
    assert!(
        excluded_from_merge_pass(&pr, "release/")
            .unwrap()
            .contains("could not be read")
    );
}

#[test]
fn an_angle_pr_belongs_to_the_release_cycle() {
    let mut pr = pull_request(1);
    pr.head_ref = "qa/acme-v1.2.3-rc.1/security".into();
    pr.base_ref = "release/2026-10-05-14-03-utc".into();
    assert!(excluded_from_merge_pass(&pr, "release/").is_some());
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

// ---- an angle's paths (spec 08 rule 5) ----

#[test]
fn an_angle_on_paths_comes_only_with_a_pr_that_touches_one() {
    let config = review_config();
    let paths = s(&["src/auth/login.rs"]);
    let plan = plan(&facts(Some(&paths), &[]), &config);
    assert_eq!(plan.angles, s(&["maintainability", "security"]));
    assert_eq!(
        plan.reasons,
        vec!["it touches the paths of the `security` angle: `src/auth/login.rs`"]
    );
    assert!(plan.holds.is_empty());
    let docs = s(&["docs/readme.md"]);
    let plan = super::plan(&facts(Some(&docs), &[]), &config);
    assert_eq!(plan.angles, s(&["maintainability"]), "on every PR only");
    assert!(plan.reasons.is_empty());
}

#[test]
fn angles_on_paths_alone_review_only_the_prs_that_touch_them() {
    let mut config = review_config();
    config.angles.retain(|a| !a.paths.is_empty());
    let docs = s(&["docs/readme.md"]);
    let plan = super::plan(&facts(Some(&docs), &[]), &config);
    assert!(plan.unreviewed(), "{plan:?}");
    assert!(plan.diff_read);
    let auth = s(&[".github/workflows/ci.yml"]);
    let plan = super::plan(&facts(Some(&auth), &[]), &config);
    assert_eq!(plan.angles, s(&["security"]));
}

// ---- triggers (REQ-MERGE-044..050) ----

#[test]
fn every_round_carries_the_base_angles() {
    let config = review_config();
    let paths = s(&["src/x.rs"]);
    let plan = plan(&facts(Some(&paths), &[]), &config);
    assert_eq!(plan.angles, s(&["maintainability"]));
    assert!(plan.reasons.is_empty());
}

#[test]
fn the_authors_flag_is_a_reason() {
    let config = review_config();
    let paths = s(&["src/x.rs"]);
    let labels = s(&[" Needs-Review "]);
    let plan = plan(&facts(Some(&paths), &labels), &config);
    assert_eq!(plan.reasons[0], "the author flagged it (`needs-review`)");
}

#[test]
fn an_unreadable_diff_buys_every_angle_on_paths_instead_of_a_merge() {
    let config = review_config();
    let plan = plan(&facts(None, &[]), &config);
    assert!(plan.holds[0].contains("the diff could not be read"));
    assert_eq!(plan.angles, s(&["maintainability", "security"]));
    // No path trigger ran: an angle missing from the plan says nothing.
    assert!(!plan.diff_read);
}

/// A project with no PR review (spec 04 §11): a PR gets no angle, and an
/// unread diff holds it without opening a round nobody would approve.
#[test]
fn with_no_possible_angle_a_pr_is_unreviewed_and_an_unread_diff_only_holds_it() {
    let mut config = review_config();
    config.angles.clear();
    config.diff_size_extra_angle = 0;
    let paths = s(&["src/auth/login.rs"]);
    let read = plan(&facts(Some(&paths), &[]), &config);
    assert!(read.unreviewed() && read.holds.is_empty(), "{read:?}");
    assert!(read.diff_read);
    let unread = plan(&facts(None, &[]), &config);
    assert!(unread.unreviewed(), "{unread:?}");
    assert!(unread.holds[0].contains("the diff could not be read"));
}

#[test]
fn an_empty_file_list_that_contradicts_the_pr_is_not_an_answer() {
    let config = review_config();
    let empty: Vec<String> = Vec::new();
    let plan = plan(&facts(Some(&empty), &[]), &config);
    assert!(!plan.holds.is_empty());
    assert!(!plan.diff_read);
}

#[test]
fn a_diff_read_as_genuinely_empty_still_merges() {
    let config = review_config();
    let empty: Vec<String> = Vec::new();
    let mut f = facts(Some(&empty), &[]);
    f.additions = 0;
    f.deletions = 0;
    assert!(plan(&f, &config).holds.is_empty());
}

#[test]
fn a_skipped_required_check_holds_the_merge() {
    let config = review_config();
    let paths = s(&["src/x.rs"]);
    let mut f = facts(Some(&paths), &[]);
    f.skipped_required = Some("required-check");
    let plan = plan(&f, &config);
    assert_eq!(
        plan.holds,
        vec!["the required check `required-check` was skipped, so green does not mean tested"]
    );
}

#[test]
fn a_configured_size_threshold_adds_its_angle_only_above_it() {
    let config = review_config();
    let paths = s(&["src/x.rs"]);
    let mut f = facts(Some(&paths), &[]);
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
fn an_angle_on_paths_is_added_by_its_globs() {
    let config = review_config();
    let paths = s(&["db/migrations/001.sql"]);
    let plan = plan(&facts(Some(&paths), &[]), &config);
    assert_eq!(plan.angles, s(&["maintainability", "security"]));
}

#[test]
fn an_angles_paths_touched_are_named_five_then_counted() {
    let config = review_config();
    let paths: Vec<String> = (0..7).map(|i| format!("src/auth/{i}.rs")).collect();
    let plan = plan(&facts(Some(&paths), &[]), &config);
    assert!(
        plan.reasons[0].ends_with("(+2 more)"),
        "{}",
        plan.reasons[0]
    );
}

// ---- approval (REQ-MERGE-055, the owner's decision) ----

fn review(login: &str, state: &str, sha: &str) -> Review {
    Review {
        id: None,
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
        id: None,
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
    config.human_approval = config::HumanApproval::Never;
    assert!(!needs_human_approval(&config));
    config.human_approval = config::HumanApproval::Always;
    assert!(needs_human_approval(&config));
}

// ---- an App's login against a person's (SID-1008) ----
//
// Authors are compared as read: an App in the REST spelling (`bot()`,
// GraphQL's bare slug normalised at read time), a person bare.

/// The login REST and the UI show for an App.
fn bot(slug: &str) -> String {
    format!("{slug}[bot]")
}

fn approved_by(configured: &str, author: &str) -> bool {
    let reviews = [review(author, "APPROVED", "h1")];
    matches!(
        approval(&reviews, &[configured.to_string()], "h1"),
        Approval::Qualified { .. }
    )
}

#[test]
fn a_coordinator_app_approves_and_a_person_of_its_name_does_not() {
    for configured in [
        bot("coord-app"),
        bot("Coord-App"),
        "app/coord-app".to_string(),
        "APP/Coord-App".to_string(),
    ] {
        assert!(approved_by(&configured, &bot("coord-app")), "{configured}");
        assert!(approved_by(&configured, &bot("COORD-APP")), "{configured}");
        assert!(
            !approved_by(&configured, "coord-app"),
            "{configured}: a person who took the App's slug is not the App"
        );
    }
    assert!(!approved_by(&bot("coord-app"), &bot("coord-app-2")));
}

#[test]
fn a_bare_coordinator_is_a_person_and_never_an_app() {
    assert!(approved_by("coord", "coord"));
    assert!(approved_by("Coord", "COORD"));
    assert!(
        !approved_by("coord-app", &bot("coord-app")),
        "a bare login never matches an App"
    );
}

#[test]
fn an_app_and_a_person_of_its_name_are_two_reviewers() {
    // The person's request for changes is not the App's: the App's
    // approval stands.
    let reviews = [
        review(&bot("coord-app"), "APPROVED", "h1"),
        review("coord-app", "CHANGES_REQUESTED", "h1"),
    ];
    assert_eq!(
        approval(&reviews, &[bot("coord-app")], "h1"),
        Approval::Qualified {
            by: bot("coord-app")
        }
    );
}

#[test]
fn a_standing_request_for_changes_is_the_configured_accounts_only() {
    let mut pr = pull_request(1);
    let head = pr.head_sha.clone();
    for (configured, author, standing) in [
        (bot("coord-app"), bot("Coord-App"), true),
        ("app/coord-app".to_string(), bot("coord-app"), true),
        (bot("coord-app"), "coord-app".to_string(), false),
        ("coord".to_string(), "Coord".to_string(), true),
        ("coord".to_string(), bot("coord"), false),
    ] {
        pr.reviews = vec![review(&author, "CHANGES_REQUESTED", &head)];
        assert_eq!(
            changes_requested_at_head(&pr, std::slice::from_ref(&configured)),
            standing.then(|| author.clone()),
            "configured {configured}, reviewed by {author}"
        );
    }
    pr.reviews = vec![review(&bot("coord-app"), "CHANGES_REQUESTED", "older")];
    assert_eq!(
        changes_requested_at_head(&pr, &[bot("coord-app")]),
        None,
        "a request on an older head is not standing at this one"
    );
    pr.reviews = vec![
        review(&bot("coord-app"), "CHANGES_REQUESTED", &head),
        review(&bot("Coord-App"), "APPROVED", &head),
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

// ---- the requests for changes besides the approvers' (spec 04 §6) ----

#[test]
fn the_requests_for_changes_standing_besides_the_approvers_are_told_app_or_person() {
    let app = bot("Angle-App");
    let mut first = review(&app, "CHANGES_REQUESTED", "h0");
    first.id = Some(1);
    let mut second = review(&bot("angle-app"), "CHANGES_REQUESTED", "h1");
    second.id = Some(2);
    let reviews = [
        first,
        review("coord", "CHANGES_REQUESTED", "h1"),
        review("passer-by", "CHANGES_REQUESTED", "h0"),
        second,
        review("lifted", "CHANGES_REQUESTED", "h0"),
        review("lifted", "APPROVED", "h1"),
        review("gone", "DISMISSED", "h0"),
    ];
    let standing = standing_changes_requested(&reviews, &s(&["coord"]));
    let told: Vec<(&str, bool, Vec<Option<u64>>)> = standing
        .iter()
        .map(|c| (c.login, c.app, c.reviews.iter().map(|r| r.id).collect()))
        .collect();
    assert_eq!(
        told,
        vec![
            // Every standing review of the App, whatever its spelling.
            (bot("angle-app").as_str(), true, vec![Some(1), Some(2)]),
            ("passer-by", false, vec![None]),
        ],
        "an approver's own is the approval's to weigh; a later approval lifts a request"
    );
    assert!(
        standing_changes_requested(
            &[review(&bot("coord"), "CHANGES_REQUESTED", "h1")],
            &s(&["coord"])
        )[0]
        .app,
        "an App of a person approver's name is not that approver"
    );
}
