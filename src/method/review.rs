//! PR review judgements (`docs/spec/04-pr-review.md`, REQ-MERGE-003..005,
//! 016, 044..055): which PRs the merge pass leaves alone, which angles a
//! review round gets, what holds a PR whatever its approval, whether the
//! coordinator's approval qualifies, and whether the branch is up to date.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::glob;
use super::vocabulary::NEEDS_REVIEW_LABEL;
use crate::config;
use crate::github::{Mergeable, PullRequest, Review};

static RELEASE_PLEASE_TITLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^chore(\(.*\))?:\s*release\b").unwrap());

/// Head prefixes of the release machinery.
pub const RELEASE_PLEASE_PREFIX: &str = "release-please--";
pub const RELEASE_RECORD_PREFIX: &str = "release-record/";
pub const MERGE_BACK_RESOLUTION_PREFIX: &str = "chore/merge-back-";
pub const ANGLE_BRANCH_PREFIX: &str = "qa/";

/// A PR shaped like release-please's (label, head prefix or title).
pub fn is_release_please_shaped(pr: &PullRequest, release_pr_label: &str) -> bool {
    pr.head_ref.starts_with(RELEASE_PLEASE_PREFIX)
        || RELEASE_PLEASE_TITLE.is_match(pr.title.trim())
        || pr
            .labels
            .iter()
            .any(|l| l.trim().eq_ignore_ascii_case(release_pr_label))
}

/// Why the merge pass never touches this PR, if it does not (spec 04 §8):
/// the release machinery is governed by the release cycle. Decided on the
/// head branch, never the base: a PR based on a release branch is an
/// ordinary PR (a hotfix). An unreadable head is treated as a merge-back.
pub fn excluded_from_merge_pass(
    pr: &PullRequest,
    branch_prefix: &str,
    release_pr_label: &str,
) -> Option<&'static str> {
    let head = pr.head_ref.trim();
    if head.is_empty() {
        return Some(
            "the head branch could not be read — refusing to squash what may be a merge back",
        );
    }
    if head.starts_with(MERGE_BACK_RESOLUTION_PREFIX) {
        return Some(
            "resolution of a merge-back conflict — merged by hand, as a merge commit, never squashed",
        );
    }
    if head.starts_with(branch_prefix) {
        return Some(
            "merge back of a release branch — the release close merges it, as a merge commit",
        );
    }
    if is_release_please_shaped(pr, release_pr_label) {
        return Some("release-please PR — the release cycle merges it on a computed GO");
    }
    if head.starts_with(RELEASE_RECORD_PREFIX) {
        return Some("gate-record PR — the release cycle merges it");
    }
    if head.starts_with(ANGLE_BRANCH_PREFIX) && pr.base_ref.starts_with(branch_prefix) {
        return Some("angle PR of a release QA — the release cycle merges it");
    }
    None
}

/// Where a PR stands against its base (REQ-MERGE-016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchState {
    Current,
    Behind,
    Conflicting,
}

/// Conflicting when GitHub says so either way; behind when `BEHIND`;
/// anything else (`CLEAN`, `BLOCKED`, `UNSTABLE`, `UNKNOWN`, …) is current:
/// the merge is pinned to the head and GitHub enforces the rest.
pub fn branch_state(pr: &PullRequest) -> BranchState {
    let state = pr.merge_state.to_ascii_uppercase();
    if pr.mergeable == Mergeable::Conflicting || state == "DIRTY" {
        BranchState::Conflicting
    } else if state == "BEHIND" {
        BranchState::Behind
    } else {
        BranchState::Current
    }
}

/// The sensitive-path list of a base (REQ-MERGE-045, spec 08 rule 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SensitivePaths {
    /// `[review] sensitive_paths` is not declared: no path is sensitive.
    NotDeclared,
    Paths(Vec<String>),
    /// Declared, but absent or invalid at the base: the PR waits.
    Invalid(String),
}

/// Parse `.github/review-paths.json`: an object whose `paths` is a
/// non-empty list of non-empty strings; every other key is ignored.
pub fn parse_sensitive_paths(text: &str) -> Result<Vec<String>, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    let obj = v.as_object().ok_or("not a JSON object")?;
    let paths = obj.get("paths").ok_or("no `paths` list")?;
    let list = paths.as_array().ok_or("`paths` is not a list")?;
    if list.is_empty() {
        return Err("`paths` is empty: an empty list would make nothing sensitive".into());
    }
    list.iter()
        .map(|p| match p.as_str() {
            Some(s) if !s.trim().is_empty() => Ok(s.to_string()),
            _ => Err("`paths` holds an entry that is not a non-empty string".to_string()),
        })
        .collect()
}

/// The facts a review round is planned from.
#[derive(Debug, Clone)]
pub struct ReviewFacts<'a> {
    pub labels: &'a [String],
    /// `None` when the file list could not be read.
    pub changed_paths: Option<&'a [String]>,
    pub additions: u64,
    pub deletions: u64,
    pub sensitive: &'a SensitivePaths,
    /// A required check that was skipped or neutral.
    pub skipped_required: Option<&'a str>,
}

/// What a review round is: its angles, why they were chosen, and what
/// holds the merge whatever the approval says.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReviewPlan {
    /// Base angles first, then the ones the triggers add, each once.
    pub angles: Vec<String>,
    /// Every trigger that matched, in order of certainty.
    pub reasons: Vec<String>,
    /// Holds no approval lifts: the diff could not be read, the configuration
    /// is invalid, a required check was skipped.
    pub holds: Vec<String>,
    /// Sensitive paths touched (also used by `human_approval`).
    pub sensitive_hits: Vec<String>,
}

fn push_unique(list: &mut Vec<String>, item: &str) {
    if !list.iter().any(|a| a == item) {
        list.push(item.to_string());
    }
}

/// Plan a review round (REQ-MERGE-044..052 as the target defines them).
pub fn plan(facts: &ReviewFacts<'_>, review: &config::Review) -> ReviewPlan {
    let mut plan = ReviewPlan::default();
    for a in &review.angles {
        push_unique(&mut plan.angles, &a.key);
    }
    if facts
        .labels
        .iter()
        .any(|l| l.trim().eq_ignore_ascii_case(NEEDS_REVIEW_LABEL))
    {
        plan.reasons
            .push(format!("the author flagged it (`{NEEDS_REVIEW_LABEL}`)"));
    }
    // A file list that names nothing while the PR changes lines is two reads
    // that contradict each other: not an answer.
    let paths = match facts.changed_paths {
        Some(p) if p.is_empty() && facts.additions + facts.deletions > 0 => None,
        other => other,
    };
    match paths {
        None => {
            let reason = "the diff could not be read, so the reviewed-path rule could not run";
            plan.reasons.push(reason.to_string());
            plan.holds.push(reason.to_string());
            push_unique(&mut plan.angles, &review.sensitive_angle);
        }
        Some(paths) => {
            match facts.sensitive {
                SensitivePaths::NotDeclared => {}
                SensitivePaths::Invalid(why) => {
                    let reason = format!(
                        "configuration invalid for this repository: the sensitive-path list could not be read ({why})"
                    );
                    plan.reasons.push(reason.clone());
                    plan.holds.push(reason);
                    push_unique(&mut plan.angles, &review.sensitive_angle);
                }
                SensitivePaths::Paths(patterns) => {
                    plan.sensitive_hits = glob::hits(paths, patterns);
                    if !plan.sensitive_hits.is_empty() {
                        let shown = plan
                            .sensitive_hits
                            .iter()
                            .take(5)
                            .map(|p| format!("`{p}`"))
                            .collect::<Vec<_>>()
                            .join(", ");
                        let more = plan.sensitive_hits.len().saturating_sub(5);
                        plan.reasons.push(if more > 0 {
                            format!("it touches reviewed paths: {shown} (+{more} more)")
                        } else {
                            format!("it touches reviewed paths: {shown}")
                        });
                        push_unique(&mut plan.angles, &review.sensitive_angle);
                    }
                }
            }
            for pa in &review.path_angles {
                if !glob::hits(paths, &pa.paths).is_empty() {
                    push_unique(&mut plan.angles, &pa.angle);
                }
            }
        }
    }
    if let Some(check) = facts.skipped_required {
        let reason =
            format!("the required check `{check}` was skipped, so green does not mean tested");
        plan.reasons.push(reason.clone());
        plan.holds.push(reason);
    }
    let size = facts.additions + facts.deletions;
    let threshold = u64::from(review.diff_size_extra_angle);
    if threshold > 0 && size > threshold {
        plan.reasons.push(format!(
            "it changes {size} lines, over the {threshold}-line review threshold"
        ));
        if let Some(angle) = &review.diff_size_angle {
            push_unique(&mut plan.angles, angle);
        }
    }
    plan
}

/// Whether a human must approve besides the coordinator (spec 04).
pub fn needs_human_approval(review: &config::Review, plan: &ReviewPlan) -> bool {
    match review.human_approval {
        config::HumanApproval::Never => false,
        config::HumanApproval::Always => true,
        config::HumanApproval::SensitivePaths => {
            !plan.sensitive_hits.is_empty() || !plan.holds.is_empty()
        }
    }
}

/// Where the approvals of a PR stand at its head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Approval {
    /// An approver approved the current head.
    Qualified {
        by: String,
    },
    /// A configured login's latest stateful review asks for changes.
    ChangesRequested {
        by: String,
    },
    /// The latest approval is on an older head.
    Stale {
        by: String,
        sha: String,
    },
    Missing,
    /// GitHub cut the review list: a later request for changes may hide.
    Unreadable,
}

/// The approval of a PR at its head; unreadable when its reviews were cut.
pub fn pr_approval(pr: &PullRequest, approvers: &[String]) -> Approval {
    if pr.truncated.iter().any(|t| t == "reviews") {
        return Approval::Unreadable;
    }
    approval(&pr.reviews, approvers, &pr.head_sha)
}

/// The approval state among `approvers` (REQ-MERGE-055, as the owner
/// decided it): per login, only the latest `APPROVED` or `CHANGES_REQUESTED`
/// counts — a `COMMENTED` review never cancels an approval; a standing
/// `CHANGES_REQUESTED` of any approver blocks; an approval qualifies only on
/// the current head. Logins compare case-insensitively; reviews are oldest
/// first.
pub fn approval(reviews: &[Review], approvers: &[String], head_sha: &str) -> Approval {
    let approver = |login: &str| approvers.iter().any(|a| a.eq_ignore_ascii_case(login));
    let mut latest: Vec<(String, &Review)> = Vec::new();
    for r in reviews {
        let Some(login) = r.author.as_deref().filter(|l| !l.is_empty()) else {
            continue;
        };
        let state = r.state.to_ascii_uppercase();
        if state != "APPROVED" && state != "CHANGES_REQUESTED" {
            continue;
        }
        let key = login.to_ascii_lowercase();
        match latest.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = r,
            None => latest.push((key, r)),
        }
    }
    let mut stale = None;
    let mut qualified = None;
    for (_, r) in &latest {
        let login = r.author.clone().unwrap_or_default();
        if !approver(&login) {
            continue;
        }
        if r.state.eq_ignore_ascii_case("CHANGES_REQUESTED") {
            return Approval::ChangesRequested { by: login };
        }
        if r.commit_sha.as_deref() == Some(head_sha) {
            qualified = Some(login);
        } else {
            stale = Some((login, r.commit_sha.clone().unwrap_or_default()));
        }
    }
    match (qualified, stale) {
        (Some(by), _) => Approval::Qualified { by },
        (None, Some((by, sha))) => Approval::Stale { by, sha },
        (None, None) => Approval::Missing,
    }
}

/// Review threads still open on the PR; `None` when GitHub cut the list
/// (an unresolved thread may hide in what was not read).
pub fn unresolved_threads(pr: &PullRequest) -> Option<usize> {
    if pr.truncated.iter().any(|t| t == "reviewThreads") {
        return None;
    }
    Some(pr.threads.iter().filter(|t| !t.resolved).count())
}

#[cfg(test)]
mod tests;
