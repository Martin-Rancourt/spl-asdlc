//! The colour of a PR's checks (REQ-MERGE-010, 011, 050): only the latest
//! run of each check counts; a red wins over a pending; no check at all is
//! not green; a required check that never reported is not green either.

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
    if required
        .iter()
        .any(|r| !latest.iter().any(|c| &c.name == r))
    {
        return CheckState::Pending;
    }
    CheckState::Green
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

/// The first required check that was skipped or neutral: green does not
/// mean tested (REQ-MERGE-050).
pub fn skipped_required(checks: &[CheckEntry], required: &BTreeSet<String>) -> Option<String> {
    latest_runs(checks)
        .into_iter()
        .find(|c| required.contains(&c.name) && SKIPPED.contains(&upper(&c.conclusion).as_str()))
        .map(|c| c.name)
}

/// The first failing check, for the red-CI report (REQ-MERGE-013).
pub fn first_failure(checks: &[CheckEntry]) -> Option<CheckEntry> {
    latest_runs(checks).into_iter().find(is_red)
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
    fn a_skipped_required_check_buys_a_reviewer() {
        let checks = [run("CI", "required-check", "COMPLETED", "SKIPPED", T1)];
        let required: BTreeSet<String> = ["required-check".to_string()].into();
        assert_eq!(
            skipped_required(&checks, &required).as_deref(),
            Some("required-check")
        );
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
