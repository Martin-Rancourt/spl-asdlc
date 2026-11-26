//! A published release whose merge back into the default branch is blocked
//! (ASDLC-PQ5J, `docs/reference/05-release-cycle.md` §11). The closure waits
//! for the release branch to be merged back and deleted; while it waits, and
//! once the publication's close concluded red, the bridge does what the
//! « Release close » recipe of `docs/setup/repository.md` asks of a person:
//!
//! 1. it reads why the merge-back PR did not merge (`DIRTY`, a required
//!    check red, or the App refused by the ruleset), says it once on the
//!    release item, and opens one fix ticket under it, assigned to
//!    `release.fix_assignee`, Ready, found again by its marker;
//! 2. it merges a `chore/merge-back-*` PR into the default branch as a merge
//!    commit, never a squash, once its checks are green, and only when its
//!    branch contains the release branch's tip (a refusal is said once per
//!    head on the PR);
//! 3. once the release tag is in the default branch, it dispatches the
//!    publication on the release branch with `replay: true`, once, which
//!    deletes the branch and publishes nothing.
//!
//! The closure then goes on as for any release, and closes the fix ticket
//! (`super::closure`). A person who does any of these steps by hand is taken
//! as having done it.

use serde_json::json;

use crate::action_log::ActionLog;
use crate::github::{CompareStatus, MergeMethod, PrState, PullRequest, Release};
use crate::method::checks::{self, CheckState};
use crate::method::release::publication;
use crate::method::release::shape::Shape;
use crate::method::review::{self, BranchState};
use crate::method::texts::{self, BlockedMergeBack, MergeBackBlock};
use crate::method::vocabulary::{self, Status};
use crate::tracker::{Item, ItemKind, NewItem, Priority};

use super::merge::{say_once, say_once_on_item, tracker_said};
use super::release::{Candidate, fixer, release_children, release_tag_commit};
use super::{Report, Tick, identity};

/// Why a merge-back PR did not merge, read off the PR and the required
/// checks of its base: `None` while nothing tells it is blocked (its checks
/// pending, or GitHub still computing its state).
pub fn block_of(
    pr: &PullRequest,
    required: &std::collections::BTreeSet<String>,
) -> Option<MergeBackBlock> {
    if review::branch_state(pr) == BranchState::Conflicting {
        return Some(MergeBackBlock::Dirty);
    }
    if pr.truncated.iter().any(|t| t == "checks") {
        return None;
    }
    let red: Vec<String> = checks::failures(&pr.checks)
        .into_iter()
        .map(|c| c.name)
        .filter(|n| required.is_empty() || required.contains(n))
        .collect();
    if !red.is_empty() {
        return Some(MergeBackBlock::RedCheck(red));
    }
    let state = pr.merge_state.to_ascii_uppercase();
    if checks::state(&pr.checks, required) == CheckState::Green
        && matches!(state.as_str(), "BLOCKED" | "BEHIND")
    {
        return Some(MergeBackBlock::AppRefused(state));
    }
    None
}

/// The tick's part while the closure waits for the merge back of a
/// published release (`super::closure::close`). `branch` is the release
/// branch that still holds the candidate.
#[allow(clippy::too_many_arguments)]
pub fn when_blocked(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    shape: &Shape,
    c: &Candidate,
    item: &Item,
    published: &Release,
    branch: &str,
) {
    let tag = &published.tag;
    let object = item.key.clone();
    // The publication's close must have concluded red: while it polls, the
    // merge back is its own, and a close that succeeded deleted the branch.
    let sha = match release_tag_commit(t, tag) {
        Ok(Some(s)) => s,
        Ok(None) => return,
        Err(e) => {
            t.unreadable(report, &object, format!("the tag {tag}: {e}"));
            return;
        }
    };
    let Some((run_url, _job)) = (match failed_close(t, &sha) {
        Ok(f) => f,
        Err(e) => {
            t.unreadable(
                report,
                &object,
                format!("the publication run of {tag}: {e}"),
            );
            return;
        }
    }) else {
        return;
    };
    let default = match t.github.default_branch(&t.repo) {
        Ok(d) => d,
        Err(e) => {
            t.unreadable(report, &object, format!("the default branch: {e}"));
            return;
        }
    };
    let tip = match t.github.branches(&t.repo, branch) {
        Ok(refs) => match refs.into_iter().find(|r| r.name == branch) {
            Some(r) => r.sha,
            None => return,
        },
        Err(e) => {
            t.unreadable(report, &object, format!("the branch {branch}: {e}"));
            return;
        }
    };
    // The release tag already in the default branch: only the replay is left.
    match t.github.compare_status(&t.repo, tag, &default) {
        Ok(CompareStatus::Ahead | CompareStatus::Identical) => {
            replay(t, log, report, item, tag, branch);
            return;
        }
        Ok(_) => {}
        Err(e) => {
            t.unreadable(report, &object, format!("{tag}...{default}: {e}"));
            return;
        }
    }
    let open = match t.github.open_pulls(&t.repo, Some(&default)) {
        Ok(p) => p,
        Err(e) => {
            t.unreadable(report, &object, format!("the open PRs into {default}: {e}"));
            return;
        }
    };
    let label = vocabulary::release_label(&shape.product, &c.version.to_string());
    let fix = match fix_ticket(t, log, report, item, tag) {
        Ok(f) => f,
        Err(e) => {
            t.unreadable(report, &object, e);
            return;
        }
    };
    // A fix ticket exists: the block was seen. Its `chore/merge-back-*` PR
    // is merged once it may be; then the next tick replays.
    let required = || match t.github.required_checks(&t.repo, &default) {
        Ok(r) => Ok(r.contexts),
        Err(e) => Err(format!("the required checks of {default}: {e}")),
    };
    if fix.is_some() {
        let resolutions: Vec<&PullRequest> =
            open.iter().filter(|p| resolution_pr(p, &default)).collect();
        if resolutions.is_empty() {
            return;
        }
        let required = match required() {
            Ok(r) => r,
            Err(e) => {
                t.unreadable(report, &object, e);
                return;
            }
        };
        for pr in resolutions {
            if land(t, log, report, pr, &tip, branch, &required) {
                return;
            }
        }
        return;
    }
    let Some(pr) = open
        .iter()
        .find(|p| p.head_ref == branch && !p.cross_repository && p.state == PrState::Open)
    else {
        return;
    };
    let required = match required() {
        Ok(r) => r,
        Err(e) => {
            t.unreadable(report, &object, e);
            return;
        }
    };
    let Some(block) = block_of(pr, &required) else {
        return;
    };
    let facts = BlockedMergeBack {
        release: &item.key,
        tag,
        branch,
        default_branch: &default,
        pr_url: &pr.url,
        run_url: &run_url,
        block: &block,
    };
    say_once_on_item(
        t,
        log,
        report,
        &item.key,
        "merge-back-blocked",
        &[("tag", tag)],
        &texts::merge_back_blocked_comment(&facts),
    );
    let (title, body) = texts::merge_back_fix_ticket(&facts);
    let body = super::merge::for_tracker(t, &body);
    let assignee = fixer(t);
    identity::create(
        t,
        log,
        report,
        &item.key,
        "create merge-back fix ticket",
        json!({"tag": tag, "cause": block.key(), "pr": pr.url, "run": run_url, "assignee": assignee.as_ref().map(|u| u.name.as_str())}),
        &NewItem {
            title,
            description: body,
            labels: vec![label],
            status: Status::Ready,
            priority: Some(Priority::Urgent),
            assignee,
            parent: Some(item.key.clone()),
            kind: ItemKind::Ticket,
        },
    );
}

/// The publication run of the release at `sha`, when it concluded with its
/// merge back red: its URL and the red job.
fn failed_close(t: &Tick, sha: &str) -> Result<Option<(String, String)>, String> {
    let workflow = &t.config.release.publish_workflow;
    let runs = match t.github.workflow_runs(&t.repo, workflow, None, None, None) {
        Ok(r) => r,
        Err(e) if e.is_not_found() => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let Some(run) = runs
        .iter()
        .find(|r| r.event == "release" && r.head_sha == sha)
    else {
        return Ok(None);
    };
    if run.status != "completed" || run.conclusion.as_deref() == Some("success") {
        return Ok(None);
    }
    let jobs = t
        .github
        .run_jobs(&t.repo, run.id)
        .map_err(|e| format!("the jobs of {}: {e}", run.url))?;
    Ok(publication::merge_back_failed(run, &jobs).map(|j| (run.url.clone(), j)))
}

/// The release item's merge-back fix ticket of `tag`, found by its marker.
pub fn fix_ticket(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    tag: &str,
) -> Result<Option<Item>, String> {
    let children = release_children(t, item).map_err(|e| format!("children: {e}"))?;
    let marker = texts::merge_back_fix_marker(tag);
    let found = identity::find(t, log, report, &children, &marker, |ch| {
        texts::has_marker(&ch.description, &marker)
    })?;
    Ok(found.into_iter().next())
}

/// A PR resolving a merge back: a `chore/merge-back-*` head of this
/// repository into the default branch, ready.
fn resolution_pr(pr: &PullRequest, default: &str) -> bool {
    pr.state == PrState::Open
        && !pr.draft
        && !pr.cross_repository
        && pr.base_ref == default
        && pr
            .head_ref
            .starts_with(review::MERGE_BACK_RESOLUTION_PREFIX)
}

/// Merge a resolution PR as a merge commit once it may be: its branch holds
/// the release branch's tip and its checks are green. Whether it merged.
fn land(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr: &PullRequest,
    tip: &str,
    branch: &str,
    required: &std::collections::BTreeSet<String>,
) -> bool {
    let holds = match t.github.compare_status(&t.repo, tip, &pr.head_sha) {
        Ok(s) => matches!(s, CompareStatus::Ahead | CompareStatus::Identical),
        Err(e) => {
            t.unreadable(
                report,
                &format!("pr#{}", pr.number),
                format!("{tip}...{}: {e}", pr.head_sha),
            );
            return false;
        }
    };
    if !holds {
        t.wait(
            report,
            pr,
            format!("its branch does not contain the tip of {branch}"),
        );
        say_once(
            t,
            log,
            report,
            pr,
            "merge-back-refused",
            &[("head", &pr.head_sha), ("tip", tip)],
            &format!(
                "The bridge does not merge this PR: its branch does not contain the tip of `{branch}` (`{}`). \
                 Cut `chore/merge-back-<stamp>` from the tip of `{branch}`, merge the default branch into it and \
                 fix there; never bring the default branch into `{branch}`.",
                texts::short(tip)
            ),
        );
        return false;
    }
    if review::branch_state(pr) == BranchState::Conflicting {
        t.wait(report, pr, "it conflicts with its base");
        return false;
    }
    match checks::pr_state(pr, required) {
        CheckState::Green => {}
        other => {
            t.wait(report, pr, format!("its checks are not green ({other:?})"));
            return false;
        }
    }
    t.act(
        log,
        report,
        &format!("pr#{}", pr.number),
        "merge (merge commit, merge back)",
        json!({"head": pr.head_sha, "tip": tip, "branch": branch}),
        || {
            t.github
                .merge_pull(&t.repo, pr.number, &pr.head_sha, MergeMethod::Merge)
                .map(|_| ())
                .map_err(|e| e.to_string())
        },
    )
    .is_some()
}

/// Dispatch the publication on the release branch with `replay: true`,
/// once per release: the merge back is in the default branch, only the
/// branch's deletion is left. A replay already running is waited for.
fn replay(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    tag: &str,
    branch: &str,
) {
    let marker = texts::merge_back_replay_marker(tag);
    match tracker_said(t, &item.key, &marker) {
        Ok(true) => return,
        Ok(false) => {}
        Err(e) => {
            t.unreadable(report, &item.key, e);
            return;
        }
    }
    let workflow = &t.config.release.publish_workflow;
    match t.github.workflow_runs(
        &t.repo,
        workflow,
        Some(branch),
        None,
        Some("workflow_dispatch"),
    ) {
        Ok(runs) if runs.iter().any(|r| r.status != "completed") => return,
        Ok(_) => {}
        Err(e) if e.is_not_found() => {}
        Err(e) => {
            t.unreadable(report, &item.key, format!("the replays of {branch}: {e}"));
            return;
        }
    }
    let dispatched = t.act(
        log,
        report,
        &item.key,
        "dispatch replay (merge back)",
        json!({"workflow": workflow, "branch": branch, "tag": tag}),
        || {
            t.github
                .dispatch_workflow(
                    &t.repo,
                    workflow,
                    branch,
                    &[("replay".to_string(), "true".to_string())],
                )
                .map_err(|e| e.to_string())
        },
    );
    if dispatched.is_some() {
        say_once_on_item(
            t,
            log,
            report,
            &item.key,
            "merge-back-replay",
            &[("tag", tag)],
            &format!(
                "`{}` is in the default branch: the bridge dispatched `{workflow}` on `{branch}` with `replay: true`, \
                 which deletes the release branch and publishes nothing. The closure follows.",
                texts::inert(tag)
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::fake::pull_request;
    use crate::github::{CheckEntry, CheckKind, Mergeable};

    fn pr(merge_state: &str, conclusion: &str) -> PullRequest {
        let mut pr = pull_request(40);
        pr.merge_state = merge_state.into();
        pr.checks = vec![CheckEntry {
            kind: CheckKind::Run,
            name: "required-check".into(),
            workflow: Some("checks".into()),
            status: Some("COMPLETED".into()),
            conclusion: Some(conclusion.into()),
            started_at: None,
            details_url: None,
        }];
        pr
    }

    fn required() -> std::collections::BTreeSet<String> {
        ["required-check".to_string()].into()
    }

    #[test]
    fn the_block_is_read_as_a_conflict_a_red_check_or_an_app_refusal() {
        assert_eq!(
            block_of(&pr("DIRTY", "SUCCESS"), &required()),
            Some(MergeBackBlock::Dirty)
        );
        let mut conflicting = pr("UNKNOWN", "SUCCESS");
        conflicting.mergeable = Mergeable::Conflicting;
        assert_eq!(
            block_of(&conflicting, &required()),
            Some(MergeBackBlock::Dirty)
        );
        assert_eq!(
            block_of(&pr("BLOCKED", "FAILURE"), &required()),
            Some(MergeBackBlock::RedCheck(vec!["required-check".into()]))
        );
        for state in ["BLOCKED", "BEHIND"] {
            assert_eq!(
                block_of(&pr(state, "SUCCESS"), &required()),
                Some(MergeBackBlock::AppRefused(state.into()))
            );
        }
    }

    #[test]
    fn nothing_blocks_while_checks_run_or_github_still_computes() {
        assert_eq!(block_of(&pr("BLOCKED", "IN_PROGRESS"), &required()), None);
        assert_eq!(block_of(&pr("UNKNOWN", "SUCCESS"), &required()), None);
        assert_eq!(block_of(&pr("CLEAN", "SUCCESS"), &required()), None);
        let mut cut = pr("BLOCKED", "FAILURE");
        cut.truncated = vec!["checks".into()];
        assert_eq!(
            block_of(&cut, &required()),
            None,
            "a cut check list is unreadable"
        );
    }

    #[test]
    fn only_a_ready_chore_merge_back_pr_of_this_repository_into_main_resolves() {
        let mut p = pull_request(41);
        p.head_ref = "chore/merge-back-2026-11-25-18-30-utc".into();
        assert!(resolution_pr(&p, "main"));
        let mut other = p.clone();
        other.base_ref = "release/2026-11-25-18-30-utc".into();
        assert!(!resolution_pr(&other, "main"));
        let mut fork = p.clone();
        fork.cross_repository = true;
        assert!(!resolution_pr(&fork, "main"));
        let mut draft = p.clone();
        draft.draft = true;
        assert!(!resolution_pr(&draft, "main"));
        let mut feat = p;
        feat.head_ref = "feat/abc-1-x".into();
        assert!(!resolution_pr(&feat, "main"));
    }
}
