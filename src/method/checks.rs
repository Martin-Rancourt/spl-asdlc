//! The colour of a PR's checks (REQ-MERGE-010, 011, 050): only the latest
//! run of each check counts; a red wins over a pending; no check at all is
//! not green; a required check is green only by a success: one that never
//! reported, or was skipped or neutral, is not green either; a rollup in
//! which nothing succeeded is not green. A check run's conclusion is its
//! outcome: GitHub can report `SUCCESS` next to a status still
//! `IN_PROGRESS` (post-job steps that hang), and a run that concluded is
//! never waited for (ASDLC-CPSK).

use std::collections::BTreeSet;

use crate::github::{CheckEntry, CheckKind};

const RED: [&str; 7] = [
    "FAILURE",
    "ERROR",
    "TIMED_OUT",
    "CANCELLED",
    "ACTION_REQUIRED",
    "STARTUP_FAILURE",
    "STALE",
];
/// `EXPECTED` is a commit status GitHub awaits: not reported yet.
const PENDING: [&str; 6] = [
    "QUEUED",
    "IN_PROGRESS",
    "PENDING",
    "WAITING",
    "REQUESTED",
    "EXPECTED",
];
const SKIPPED: [&str; 2] = ["SKIPPED", "NEUTRAL"];
/// The only outcomes that are not pending or red: an allowlist, so that a
/// state nobody listed never reads as green.
const PASSING: [&str; 3] = ["SUCCESS", "SKIPPED", "NEUTRAL"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckState {
    Green,
    Red,
    Pending,
    /// No check reported at all: nothing verified anything.
    None,
    /// GitHub cut the list, or the rollup is of another commit: it says
    /// nothing about the head.
    Unreadable,
}

/// The latest run of each check, identified by (workflow, name); a run not
/// started yet is newer than any started run; runs the timestamps cannot
/// order are all kept; entries without a name are kept as they are.
pub fn latest_runs(checks: &[CheckEntry]) -> Vec<CheckEntry> {
    let key = |c: &CheckEntry| match c.kind {
        CheckKind::Run => (Some(c.workflow.clone().unwrap_or_default()), c.name.clone()),
        CheckKind::Status => (None, c.name.clone()),
    };
    let mut out = Vec::new();
    for (i, c) in checks.iter().enumerate() {
        if c.name.is_empty() {
            out.push(c.clone());
            continue;
        }
        let newer_exists = checks
            .iter()
            .enumerate()
            .any(|(j, o)| j != i && !o.name.is_empty() && key(o) == key(c) && newer(o, c));
        if !newer_exists {
            out.push(c.clone());
        }
    }
    out
}

/// `a` is strictly newer than `b`.
fn newer(a: &CheckEntry, b: &CheckEntry) -> bool {
    match (a.started_at, b.started_at) {
        (None, Some(_)) => true,
        (Some(x), Some(y)) => x > y,
        _ => false,
    }
}

fn upper(s: &Option<String>) -> String {
    s.as_deref().unwrap_or("").to_ascii_uppercase()
}

fn is_red(c: &CheckEntry) -> bool {
    RED.contains(&upper(&c.conclusion).as_str())
}

fn is_pending(c: &CheckEntry) -> bool {
    let status = upper(&c.status);
    let conclusion = upper(&c.conclusion);
    if c.kind == CheckKind::Run && PASSING.contains(&conclusion.as_str()) {
        return false;
    }
    PENDING.contains(&status.as_str())
        || PENDING.contains(&conclusion.as_str())
        || (c.kind == CheckKind::Run && status != "COMPLETED" && conclusion.is_empty())
}

/// The colour of a rollup. `required` names the checks the base requires: a
/// required check absent from the rollup is pending (it has not reported),
/// never green.
pub fn state(checks: &[CheckEntry], required: &BTreeSet<String>) -> CheckState {
    let latest = latest_runs(checks);
    if latest.is_empty() {
        return CheckState::None;
    }
    if latest.iter().any(is_red) {
        return CheckState::Red;
    }
    if latest.iter().any(is_pending) {
        return CheckState::Pending;
    }
    // Completed without a conclusion, or with one nobody listed: not green.
    if latest
        .iter()
        .any(|c| !PASSING.contains(&upper(&c.conclusion).as_str()))
    {
        return CheckState::Pending;
    }
    // A required check is green only by a success (the owner's decision,
    // 2026-10-15): GitHub reports a skipped job as passing a required check,
    // and the `required-check` of a draft PR's run is skipped. Missing,
    // skipped or neutral, it waits for a run that succeeds.
    if required.iter().any(|r| !required_succeeded(&latest, r)) {
        return CheckState::Pending;
    }
    // Every check skipped or neutral: nothing verified anything.
    if !latest.iter().any(|c| upper(&c.conclusion) == "SUCCESS") {
        return CheckState::Pending;
    }
    CheckState::Green
}

/// The required check `name` reported, and each of its latest runs succeeded.
fn required_succeeded(latest: &[CheckEntry], name: &str) -> bool {
    let mut runs = latest.iter().filter(|c| c.name == name).peekable();
    runs.peek().is_some() && runs.all(|c| upper(&c.conclusion) == "SUCCESS")
}

/// The checks of the rollup still running (their latest run), by name.
pub fn running(checks: &[CheckEntry]) -> Vec<String> {
    latest_runs(checks)
        .into_iter()
        .filter(is_pending)
        .map(|c| c.name)
        .collect()
}

/// The first check that is neither running, red nor passing: completed
/// without a conclusion, or with one nobody listed.
pub fn first_unsettled(checks: &[CheckEntry]) -> Option<String> {
    latest_runs(checks)
        .into_iter()
        .find(|c| !is_pending(c) && !is_red(c) && !PASSING.contains(&upper(&c.conclusion).as_str()))
        .map(|c| c.name)
}

/// No check of the rollup succeeded (its latest runs).
pub fn none_succeeded(checks: &[CheckEntry]) -> bool {
    !latest_runs(checks)
        .iter()
        .any(|c| upper(&c.conclusion) == "SUCCESS")
}

/// The colour of a PR's head: unreadable when GitHub cut its check list.
pub fn pr_state(pr: &crate::github::PullRequest, required: &BTreeSet<String>) -> CheckState {
    if pr.truncated.iter().any(|t| t == "checks") {
        return CheckState::Unreadable;
    }
    state(&pr.checks, required)
}

/// Required checks that have not reported on the head.
pub fn missing_required(checks: &[CheckEntry], required: &BTreeSet<String>) -> Vec<String> {
    required
        .iter()
        .filter(|r| !checks.iter().any(|c| &c.name == *r))
        .cloned()
        .collect()
}

/// The first required check that was skipped or neutral: it is not green
/// (REQ-MERGE-050, as corrected), and the wait says so.
pub fn skipped_required(checks: &[CheckEntry], required: &BTreeSet<String>) -> Option<String> {
    latest_runs(checks)
        .into_iter()
        .find(|c| required.contains(&c.name) && SKIPPED.contains(&upper(&c.conclusion).as_str()))
        .map(|c| c.name)
}

/// The first failing check, for the red-CI report (REQ-MERGE-013).
pub fn first_failure(checks: &[CheckEntry]) -> Option<CheckEntry> {
    failures(checks).into_iter().next()
}

/// Latest failing runs only, for implementation handbacks.
pub fn failures(checks: &[CheckEntry]) -> Vec<CheckEntry> {
    latest_runs(checks).into_iter().filter(is_red).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::Timestamp;

    fn run(
        workflow: &str,
        name: &str,
        status: &str,
        conclusion: &str,
        started: Option<&str>,
    ) -> CheckEntry {
        CheckEntry {
            kind: CheckKind::Run,
            name: name.into(),
            workflow: Some(workflow.into()),
            status: Some(status.into()),
            conclusion: (!conclusion.is_empty()).then(|| conclusion.into()),
            started_at: started.and_then(Timestamp::parse),
            details_url: None,
        }
    }

    fn status(context: &str, state: &str) -> CheckEntry {
        CheckEntry {
            kind: CheckKind::Status,
            name: context.into(),
            workflow: None,
            status: None,
            conclusion: Some(state.into()),
            started_at: None,
            details_url: None,
        }
    }

    const T1: Option<&str> = Some("2026-10-05T10:00:00Z");
    const T2: Option<&str> = Some("2026-10-05T11:00:00Z");

    fn none() -> BTreeSet<String> {
        BTreeSet::new()
    }

    #[test]
    fn no_check_is_none_never_green() {
        assert_eq!(state(&[], &none()), CheckState::None);
    }

    #[test]
    fn red_wins_over_pending() {
        let checks = [
            run("CI", "test", "COMPLETED", "FAILURE", T1),
            run("CI", "lint", "IN_PROGRESS", "", T1),
        ];
        assert_eq!(state(&checks, &none()), CheckState::Red);
    }

    #[test]
    fn skipped_neutral_and_success_are_green() {
        let checks = [
            run("CI", "a", "COMPLETED", "SUCCESS", T1),
            run("CI", "b", "COMPLETED", "SKIPPED", T1),
            run("CI", "c", "COMPLETED", "NEUTRAL", T1),
            status("legacy", "SUCCESS"),
        ];
        assert_eq!(state(&checks, &none()), CheckState::Green);
    }

    #[test]
    fn a_status_context_is_read_by_its_state_case_insensitively() {
        assert_eq!(
            state(&[status("legacy", "error")], &none()),
            CheckState::Red
        );
        assert_eq!(
            state(&[status("legacy", "pending")], &none()),
            CheckState::Pending
        );
        assert_eq!(
            state(&[status("legacy", "EXPECTED")], &none()),
            CheckState::Pending
        );
    }

    #[test]
    fn a_red_run_superseded_by_a_green_rerun_on_the_same_commit_is_green() {
        let checks = [
            run("Title", "pr-title", "COMPLETED", "FAILURE", T1),
            run("Title", "pr-title", "COMPLETED", "SUCCESS", T2),
        ];
        assert_eq!(state(&checks, &none()), CheckState::Green);
    }

    #[test]
    fn a_green_run_superseded_by_a_red_rerun_is_red() {
        let checks = [
            run("CI", "test", "COMPLETED", "SUCCESS", T1),
            run("CI", "test", "COMPLETED", "FAILURE", T2),
        ];
        assert_eq!(state(&checks, &none()), CheckState::Red);
    }

    #[test]
    fn a_rerun_not_started_yet_is_pending_not_the_red_it_replaces() {
        let checks = [
            run("CI", "test", "COMPLETED", "FAILURE", T1),
            run("CI", "test", "QUEUED", "", None),
        ];
        assert_eq!(state(&checks, &none()), CheckState::Pending);
    }

    #[test]
    fn one_job_name_in_two_workflows_is_two_checks() {
        let checks = [
            run("CI", "build", "COMPLETED", "FAILURE", T1),
            run("Release", "build", "COMPLETED", "SUCCESS", T2),
        ];
        assert_eq!(state(&checks, &none()), CheckState::Red);
    }

    #[test]
    fn runs_the_timestamps_cannot_order_are_all_kept() {
        let checks = [
            run("CI", "test", "COMPLETED", "SUCCESS", T1),
            run("CI", "test", "COMPLETED", "FAILURE", T1),
        ];
        assert_eq!(latest_runs(&checks).len(), 2);
        assert_eq!(state(&checks, &none()), CheckState::Red);
    }

    #[test]
    fn a_required_check_that_never_reported_is_not_green() {
        let checks = [run("CI", "test", "COMPLETED", "SUCCESS", T1)];
        let required: BTreeSet<String> = ["required-check".to_string()].into();
        assert_eq!(state(&checks, &required), CheckState::Pending);
        assert_eq!(missing_required(&checks, &required), vec!["required-check"]);
    }

    #[test]
    fn a_skipped_required_check_is_named() {
        let checks = [run("CI", "required-check", "COMPLETED", "SKIPPED", T1)];
        let required: BTreeSet<String> = ["required-check".to_string()].into();
        assert_eq!(
            skipped_required(&checks, &required).as_deref(),
            Some("required-check")
        );
    }

    #[test]
    fn a_required_check_is_green_only_by_a_success() {
        let required: BTreeSet<String> = ["required-check".to_string()].into();
        for conclusion in ["SKIPPED", "NEUTRAL"] {
            let checks = [
                run("CI", "guards", "COMPLETED", "SUCCESS", T1),
                run("CI", "required-check", "COMPLETED", conclusion, T1),
            ];
            assert_eq!(
                state(&checks, &required),
                CheckState::Pending,
                "{conclusion}"
            );
        }
        // A commit status of the required name is read the same way.
        let checks = [
            run("CI", "guards", "COMPLETED", "SUCCESS", T1),
            status("required-check", "SUCCESS"),
        ];
        assert_eq!(state(&checks, &required), CheckState::Green);
    }

    #[test]
    fn the_skipped_required_check_of_a_draft_run_waits_for_the_ready_run() {
        // The draft run skipped every job; the ready run, on the same head,
        // is the one that reports the check.
        let required: BTreeSet<String> = ["required-check".to_string()].into();
        let draft = [
            run("pr", "guards", "COMPLETED", "SKIPPED", T1),
            run("pr", "required-check", "COMPLETED", "SKIPPED", T1),
        ];
        assert_eq!(state(&draft, &required), CheckState::Pending);
        assert!(running(&draft).is_empty());
        let mut ready = draft.to_vec();
        ready.push(run("pr", "guards", "COMPLETED", "SUCCESS", T2));
        assert_eq!(state(&ready, &required), CheckState::Pending);
        ready.push(run("pr", "required-check", "QUEUED", "", None));
        assert_eq!(state(&ready, &required), CheckState::Pending);
        assert!(!running(&ready).is_empty());
        ready.pop();
        ready.push(run("pr", "required-check", "COMPLETED", "SUCCESS", T2));
        assert_eq!(state(&ready, &required), CheckState::Green);
    }

    #[test]
    fn a_rollup_in_which_nothing_succeeded_is_not_green() {
        let checks = [
            run("pr", "guards", "COMPLETED", "SKIPPED", T1),
            run("pr", "test", "COMPLETED", "NEUTRAL", T1),
        ];
        assert_eq!(state(&checks, &none()), CheckState::Pending);
        assert!(none_succeeded(&checks));
        assert!(!none_succeeded(&[run(
            "pr",
            "a",
            "COMPLETED",
            "SUCCESS",
            T1
        )]));
    }

    /// GitHub can report a run's `SUCCESS` while its status still says
    /// `IN_PROGRESS`: the conclusion wins, the run is not waited for.
    #[test]
    fn a_run_that_concluded_is_not_pending_whatever_its_status() {
        let required: BTreeSet<String> = ["test".to_string()].into();
        let checks = [
            run("CI", "test", "IN_PROGRESS", "SUCCESS", T1),
            run("CI", "lint", "IN_PROGRESS", "NEUTRAL", T1),
        ];
        assert_eq!(state(&checks, &required), CheckState::Green);
        assert!(running(&checks).is_empty());
        let checks = [
            run("CI", "test", "COMPLETED", "SUCCESS", T1),
            run("CI", "lint", "IN_PROGRESS", "", T1),
        ];
        assert_eq!(state(&checks, &required), CheckState::Pending);
        assert_eq!(running(&checks), vec!["lint".to_string()]);
    }

    #[test]
    fn a_skipped_check_that_is_not_required_does_not() {
        let checks = [run("CI", "optional", "COMPLETED", "SKIPPED", T1)];
        let required: BTreeSet<String> = ["required-check".to_string()].into();
        assert_eq!(skipped_required(&checks, &required), None);
        assert_eq!(skipped_required(&checks, &none()), None);
    }

    #[test]
    fn a_state_nobody_listed_is_never_green() {
        let none = BTreeSet::new();
        assert_eq!(state(&[status("legacy", "")], &none), CheckState::Pending);
        assert_eq!(
            state(&[status("legacy", "BRAND_NEW")], &none),
            CheckState::Pending
        );
        assert_eq!(
            state(&[run("CI", "a", "COMPLETED", "", T1)], &none),
            CheckState::Pending
        );
        assert_eq!(
            state(&[run("CI", "a", "COMPLETED", "NEW_OUTCOME", T1)], &none),
            CheckState::Pending
        );
    }

    #[test]
    fn a_cut_check_list_is_unreadable_never_green() {
        let mut pr = crate::github::fake::pull_request(1);
        pr.checks = vec![run("CI", "a", "COMPLETED", "SUCCESS", T1)];
        pr.truncated = vec!["checks".into()];
        assert_eq!(pr_state(&pr, &BTreeSet::new()), CheckState::Unreadable);
        pr.truncated.clear();
        assert_eq!(pr_state(&pr, &BTreeSet::new()), CheckState::Green);
    }

    #[test]
    fn the_first_failure_is_named() {
        let checks = [
            run("CI", "lint", "COMPLETED", "SUCCESS", T1),
            run("CI", "test", "COMPLETED", "TIMED_OUT", T1),
        ];
        assert_eq!(first_failure(&checks).unwrap().name, "test");
    }
}
