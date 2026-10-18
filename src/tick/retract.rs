//! The retraction of a release whose publication failed before anything was
//! deployed (`docs/reference/05-release-cycle.md` §9): its GitHub release and
//! its tag deleted, as the App (which the tag rulesets let bypass them), and
//! said once on the release issue, so that the cycle awaits its GO again and
//! the next candidate takes the version. The tick does it while the closure
//! waits for the merge back; `release retract <tag>` does it by hand, with
//! the same refusals.
//!
//! The comment, carrying the marker `spl-asdlc:retracted tag=<tag>
//! sha=<commit>`, comes first: it is what keeps the retracted candidate from
//! a second GO, so no release is deleted before it is written. Each step is
//! read before it is written: once retracted, there is nothing to retract.

use serde_json::json;

use crate::action_log::ActionLog;
use crate::github::{Release, WorkflowRun};
use crate::method::release::publication::{self, Publication};
use crate::method::texts;
use crate::tracker::Item;

use super::merge::{for_tracker, tracker_said};
use super::release::release_tag_commit;
use super::{Report, Tick};

/// The marker of the retraction of `tag` from the commit `sha`.
pub fn marker(tag: &str, sha: &str) -> String {
    texts::marker("retracted", &[("tag", tag), ("sha", sha)])
}

/// Whether the release issue says that `tag` was retracted from `sha`.
pub fn retracted(t: &Tick, item: &str, tag: &str, sha: &str) -> Result<bool, String> {
    tracker_said(t, item, &marker(tag, sha))
}

/// The publication run of a release and what it says.
pub struct Assessed {
    pub run: WorkflowRun,
    pub publication: Publication,
}

/// The newest run of `release.publish_workflow` on the `release` event at
/// the release tag's commit, judged from its jobs. `None` when there is
/// none (the workflow absent from GitHub, or not run yet). A dispatch of the
/// workflow created after that run (a resume or a replay a person started)
/// keeps the release: the person took the publication over.
pub fn assess(t: &Tick, sha: &str) -> Result<Option<Assessed>, String> {
    let workflow = &t.config.release.publish_workflow;
    let runs = match t.github.workflow_runs(&t.repo, workflow, None, None, None) {
        Ok(r) => r,
        Err(e) if e.is_not_found() => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let Some(run) = runs
        .iter()
        .find(|r| r.event == "release" && r.head_sha == sha)
        .cloned()
    else {
        return Ok(None);
    };
    let publication = if run.status != "completed" || run.conclusion.as_deref() == Some("success") {
        publication::judge(&run, &[])
    } else {
        let jobs = t
            .github
            .run_jobs(&t.repo, run.id)
            .map_err(|e| format!("the jobs of {}: {e}", run.url))?;
        publication::judge(&run, &jobs)
    };
    let dispatched = runs.iter().find(|r| {
        r.event == "workflow_dispatch"
            && match (r.created_at, run.created_at) {
                (Some(d), Some(p)) => d >= p,
                // Unknown order: taken as later, fail-safe.
                _ => true,
            }
    });
    let publication = match (publication, dispatched) {
        (Publication::Undeployed { .. }, Some(d)) => Publication::Kept {
            why: format!(
                "{workflow} was dispatched after it ({}): a resume or a replay took the publication over",
                d.url
            ),
        },
        (p, _) => p,
    };
    Ok(Some(Assessed { run, publication }))
}

/// Retract `tag` (on `sha`): the comment with its marker once, then the
/// GitHub release when it is there, then the tag when it still names `sha`.
/// `by` says who retracts, as the comment's subject. Whether every step is
/// done.
#[allow(clippy::too_many_arguments)]
pub fn retract(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    tag: &str,
    sha: &str,
    release: Option<&Release>,
    run_url: &str,
    why: &str,
    by: &str,
) -> bool {
    let marker = marker(tag, sha);
    match tracker_said(t, &item.key, &marker) {
        Ok(true) => {}
        Ok(false) => {
            let body = for_tracker(
                t,
                &format!(
                    "### Release `{tag}` retracted\n\nThe publication of `{tag}` failed before anything was deployed: {why} ({run_url}). {by} retracts it: its GitHub release and its tag `{tag}` (on `{}`) are deleted, so the version is not delivered. The fix is a PR to the release branch, then a new candidate: the next candidate takes `{tag}` at its GO.\n\n{marker}",
                    texts::short(sha)
                ),
            );
            let said = t.act(
                log,
                report,
                &item.key,
                "comment retraction",
                json!({"tag": tag, "run": run_url}),
                || {
                    t.tracker
                        .comment(&item.key, &body)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                },
            );
            if said.is_none() {
                return false;
            }
        }
        Err(e) => {
            t.unreadable(report, &item.key, e);
            return false;
        }
    }
    if let Some(r) = release {
        let deleted = t.act(
            log,
            report,
            &item.key,
            &format!("delete release {tag} (retract)"),
            json!({"id": r.id}),
            || {
                t.github
                    .delete_release(&t.repo, r.id)
                    .map_err(|e| e.to_string())
            },
        );
        if deleted.is_none() {
            return false;
        }
    }
    delete_tag(t, log, report, item, tag, sha)
}

/// The tag of a retraction, deleted when it still names the retracted
/// commit; a tag on another commit is a person's, left alone and said.
pub fn delete_tag(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    tag: &str,
    sha: &str,
) -> bool {
    match release_tag_commit(t, tag) {
        Ok(None) => true,
        Ok(Some(on)) if on == sha => t
            .act(
                log,
                report,
                &item.key,
                &format!("delete tag {tag} (retract)"),
                json!({"sha": sha}),
                || t.github.delete_tag(&t.repo, tag).map_err(|e| e.to_string()),
            )
            .is_some(),
        Ok(Some(on)) => {
            t.unreadable(
                report,
                &item.key,
                format!(
                    "{tag} now names {}, not the retracted {}: left alone",
                    texts::short(&on),
                    texts::short(sha)
                ),
            );
            false
        }
        Err(e) => {
            t.unreadable(report, &item.key, format!("the tag {tag}: {e}"));
            false
        }
    }
}

/// The tick's part, while the closure waits for the merge back of a
/// published release: its publication run read, and the release retracted
/// when that run deployed nothing. A run that cannot be read retracts
/// nothing and is said.
pub fn when_undeployed(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    published: &Release,
) {
    let tag = &published.tag;
    let sha = match release_tag_commit(t, tag) {
        Ok(Some(s)) => s,
        // A release without its tag: no publication run to read.
        Ok(None) => return,
        Err(e) => {
            t.unreadable(report, &item.key, format!("the tag {tag}: {e}"));
            return;
        }
    };
    match assess(t, &sha) {
        Ok(Some(Assessed {
            run,
            publication: Publication::Undeployed { why },
        })) => {
            retract(
                t,
                log,
                report,
                item,
                tag,
                &sha,
                Some(published),
                &run.url,
                &why,
                "The bridge",
            );
        }
        Ok(_) => {}
        Err(e) => t.unreadable(
            report,
            &item.key,
            format!("the publication run of {tag}: {e}"),
        ),
    }
}
