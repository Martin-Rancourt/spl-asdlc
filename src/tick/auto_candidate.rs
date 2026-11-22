//! The next candidate cut by the bridge (`release.auto_candidate = true`,
//! `docs/reference/05-release-cycle.md` §7, rule C8WN): once the release
//! branch moved past the latest candidate with more than bookkeeping and
//! every fix of the release is merged, the bridge dispatches the cut
//! workflow (`candidate`) on the release branch, once per branch head.
//!
//! Every fix of the release is merged when no gate-fix ticket of the
//! release (a child of the release item carrying the gate-fix marker) and
//! no blocking finding's ticket of the release (the release label and the
//! blocker marker naming the release item) is still before To Deploy. The
//! bridge never cuts a candidate already reaching its GO (the GO computed,
//! or the release tag already there), nor past the last allowed pass
//! (`release.max_rc_passes`): there the escalation hands the cycle to the
//! owner (§10).
//!
//! The dispatch is written first, then the comment that carries its marker
//! on the release item: a dispatch that failed writes no marker and is
//! retried at the next tick; a head whose marker is there is never
//! dispatched again. Without `auto_candidate` nothing is dispatched, and
//! `release status` says the branch moved past the candidate and that a cut
//! is due.

use serde_json::json;

use crate::action_log::ActionLog;
use crate::method::release::findings;
use crate::method::release::go::GoDecision;
use crate::method::release::shape::Shape;
use crate::method::texts;
use crate::method::vocabulary::Status;
use crate::tracker::Item;

use super::merge::{for_tracker, tracker_said};
use super::release::{self, Candidate};
use super::{Report, Tick};

/// The marker of the cut the bridge dispatched for `branch` at `head`.
pub fn marker(branch: &str, head: &str) -> String {
    texts::marker("auto-candidate", &[("branch", branch), ("head", head)])
}

/// Cut the next candidate when the policy lets the bridge and the facts
/// call for it. Reads fail closed: nothing is dispatched, the failure is
/// reported and the next tick retries.
#[allow(clippy::too_many_arguments)]
pub fn cut(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    c: &Candidate,
    branch: &str,
    label: &str,
    decision: &GoDecision,
) {
    let cfg = release::section(t);
    if !cfg.auto_candidate || decision.go || c.rc >= u64::from(cfg.max_rc_passes) {
        return;
    }
    let moved = match t.github.compare(&t.repo, &c.sha, branch) {
        Ok(cmp) => cmp.files.iter().any(|f| !release::bookkeeping(cfg, c, f)),
        Err(e) => {
            t.unreadable(
                report,
                &item.key,
                format!("{branch} could not be compared with {}: {e}", c.tag),
            );
            return;
        }
    };
    if !moved {
        return;
    }
    let head = match branch_head(t, branch) {
        Ok(h) => h,
        Err(e) => {
            t.unreadable(report, &item.key, e);
            return;
        }
    };
    let marker = marker(branch, &head);
    match tracker_said(t, &item.key, &marker) {
        Ok(false) => {}
        Ok(true) => return,
        Err(e) => {
            t.unreadable(report, &item.key, format!("comments: {e}"));
            return;
        }
    }
    // A release tag already there: the GO is under way (or a person's),
    // never a new candidate.
    let release_tag = Shape::of(&t.config.product.name, cfg).release_tag(c.version);
    match release::release_tag_commit(t, &release_tag) {
        Ok(None) => {}
        Ok(Some(_)) => return,
        Err(e) => {
            t.unreadable(report, &item.key, format!("the tag {release_tag}: {e}"));
            return;
        }
    }
    let pending = match open_fixes(t, item, label) {
        Ok(p) => p,
        Err(e) => {
            t.unreadable(report, &item.key, e);
            return;
        }
    };
    if !pending.is_empty() || !t.in_time() {
        return;
    }
    let workflow = &cfg.cut_workflow;
    let dispatched = t.act(
        log,
        report,
        &item.key,
        &format!("dispatch {workflow} (candidate)"),
        json!({"ref": branch, "head": head, "after": c.tag}),
        || {
            t.github
                .dispatch_workflow(
                    &t.repo,
                    workflow,
                    branch,
                    &[("action".to_string(), "candidate".to_string())],
                )
                .map_err(|e| e.to_string())
        },
    );
    if dispatched.is_none() {
        return;
    }
    let body = for_tracker(t, &comment(branch, &head, &c.tag, workflow, &marker));
    t.act(
        log,
        report,
        &item.key,
        "comment next candidate cut",
        json!({"ref": branch, "head": head}),
        || {
            t.tracker
                .comment(&item.key, &body)
                .map(|_| ())
                .map_err(|e| e.to_string())
        },
    );
}

/// The release's fix tickets still before To Deploy: its gate-fix tickets
/// and its blocking findings' tickets. A state outside the status table
/// counts as open unless the tracker resolved it (fail-closed).
pub fn open_fixes(t: &Tick, item: &Item, label: &str) -> Result<Vec<String>, String> {
    let children = release::release_children(t, item).map_err(|e| format!("children: {e}"))?;
    let gate_fixes = children
        .into_iter()
        .filter(|ch| texts::gate_fix_candidate(&ch.description).is_some());
    let findings = t
        .tracker
        .list_by_label(label)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|i| {
            findings::parse_blocker_marker(&i.description).as_deref() == Some(item.key.as_str())
        });
    let mut open: Vec<String> = Vec::new();
    for i in gate_fixes.chain(findings) {
        if !i.archived && before_to_deploy(&i) && !open.contains(&i.key) {
            open.push(i.key);
        }
    }
    Ok(open)
}

/// Whether a ticket's fix is not merged yet: Triage to In Review, or an
/// open state the status table does not place.
fn before_to_deploy(i: &Item) -> bool {
    match i.state.status {
        Some(s) => matches!(
            s,
            Status::Triage
                | Status::Backlog
                | Status::Ready
                | Status::InProgress
                | Status::InReview
        ),
        None => !i.state.is_resolved(),
    }
}

/// The commit the release branch names now.
fn branch_head(t: &Tick, branch: &str) -> Result<String, String> {
    let prefix = &release::section(t).branch_prefix;
    t.github
        .branches(&t.repo, prefix)
        .map_err(|e| format!("the branches {prefix}*: {e}"))?
        .into_iter()
        .find(|b| b.name == branch)
        .map(|b| b.sha)
        .ok_or_else(|| format!("the branch {branch} is not listed"))
}

/// The release item's record of the dispatch.
fn comment(branch: &str, head: &str, after: &str, workflow: &str, marker: &str) -> String {
    format!(
        "### Next candidate cut\n\n`{branch}` moved past `{after}` (now at `{head}`) and every fix of the release is merged: \
         the bridge dispatched `{workflow}` (`candidate`) on `{branch}` (`release.auto_candidate`), once for this head. \
         The next candidate is taken over at the next tick once its tag is there.\n\n{marker}"
    )
}
