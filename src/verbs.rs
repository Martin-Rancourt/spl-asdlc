//! The human gestures of a cycle (`docs/spec/09-human-gestures.md`) and the
//! GO status, written against the traits so that the command line and the
//! tests drive the same code. Each writes through [`Tick::act`], so each is
//! in the action log.

use serde_json::json;

use crate::action_log::ActionLog;
use crate::github::{CompareStatus, MergeMethod};
use crate::method::release::findings::{self, RequalifyStep};
use crate::method::release::go::{AngleState, GoDecision};
use crate::method::release::prs;
use crate::method::release::report;
use crate::method::release::shape::Shape;
use crate::method::texts;
use crate::method::vocabulary::{self, Severity, Status};
use crate::tick::merge::{for_tracker, tracker_said};
use crate::tick::release::{self, Candidate};
use crate::tick::{Report, Tick};
use crate::tracker::{Item, RelationKind};

/// The cycle in flight: its candidate, release item and release branch.
pub struct Cycle {
    pub shape: Shape,
    pub candidate: Candidate,
    pub item: Item,
    pub branch: Option<String>,
}

/// Read the cycle in flight; nothing is created.
pub fn cycle(t: &Tick) -> Result<Cycle, String> {
    let shape = Shape::of(&t.config.product.name, &t.config.release);
    let (candidate, _) = release::newest(t, &shape)?.ok_or("no release candidate tag")?;
    let item = release::find_release_item(t, &shape, candidate.version)?
        .ok_or("no release item yet: the next tick creates it")?;
    let branch = release::release_branch(t, &shape, &candidate).ok();
    Ok(Cycle {
        shape,
        candidate,
        item,
        branch,
    })
}

/// `release status`: the GO and its inputs, read-only.
pub fn status(t: &Tick) -> Result<(Cycle, GoDecision), String> {
    let cy = cycle(t)?;
    let branch = cy
        .branch
        .clone()
        .ok_or_else(|| format!("no release branch holds {}", cy.candidate.tag))?;
    let children = t
        .tracker
        .children(&cy.item.key)
        .map_err(|e| e.to_string())?;
    let open = t
        .github
        .open_pulls(&t.repo, Some(&branch))
        .map_err(|e| e.to_string())?;
    let merged = t
        .github
        .merged_pulls(&t.repo, Some(&branch), None, 500)
        .map_err(|e| e.to_string())?;
    let c = &cy.candidate;
    let without_angles = release::without_angles(t, &cy.item, c)?;
    let offered = if without_angles {
        Vec::new()
    } else {
        release::offered(t, c)
    };
    let states: Vec<(String, AngleState)> = offered
        .into_iter()
        .map(|angle| {
            let offered = children
                .iter()
                .any(|ch| release::angle_of(ch, &c.tag).as_deref() == Some(angle.as_str()));
            let mine =
                |head: &str| prs::parse_angle_branch(head) == Some((c.tag.clone(), angle.clone()));
            let state = if merged.iter().any(|p| mine(&p.head_ref)) {
                AngleState::Merged
            } else if let Some(pr) = open.iter().find(|p| mine(&p.head_ref)) {
                let path = report::report_path(&t.config.release.record, &c.tag, &angle);
                match t.github.file(&t.repo, &path, &pr.head_sha) {
                    Ok(text) if report::validate(&text, &c.tag, &angle).is_ok() => {
                        AngleState::InReview
                    }
                    _ => AngleState::FailedReview,
                }
            } else if offered {
                AngleState::Offered
            } else {
                AngleState::NotOffered
            };
            (angle, state)
        })
        .collect();
    let mut report = Report::default();
    let (decision, _) = release::decide(
        t,
        &mut report,
        &cy.shape,
        &cy.item,
        c,
        &branch,
        &states,
        &open,
        without_angles,
    )
    .ok_or_else(|| report.failures.join("; "))?;
    Ok((cy, decision))
}

/// `release cut [--open]`: dispatch the cut workflow, for a new cycle from
/// the default branch or for the next candidate on the release branch.
pub fn cut(t: &Tick, log: &mut ActionLog, open: bool) -> Result<String, String> {
    let shape = Shape::of(&t.config.product.name, &t.config.release);
    let branches: Vec<String> = t
        .github
        .branches(&t.repo, &t.config.release.branch_prefix)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|b| shape.is_release_branch(&b.name))
        .map(|b| b.name)
        .collect();
    let (git_ref, action) = if open {
        if let Some(b) = branches.first() {
            return Err(format!(
                "a cycle is in flight on {b}: one release branch at a time"
            ));
        }
        (
            t.github
                .default_branch(&t.repo)
                .map_err(|e| e.to_string())?,
            "open",
        )
    } else {
        match branches.as_slice() {
            [b] => (b.clone(), "candidate"),
            [] => return Err("no release branch: open a cycle with `release cut --open`".into()),
            _ => return Err(format!("several release branches: {}", branches.join(", "))),
        }
    };
    let workflow = &t.config.release.cut_workflow;
    let mut report = Report::default();
    t.act(
        log,
        &mut report,
        "release cut",
        &format!("dispatch {workflow} ({action})"),
        json!({"ref": git_ref}),
        || {
            t.github
                .dispatch_workflow(
                    &t.repo,
                    workflow,
                    &git_ref,
                    &[("action".to_string(), action.to_string())],
                )
                .map_err(|e| e.to_string())
        },
    )
    .ok_or_else(|| report.failures.join("; "))?;
    Ok(format!(
        "{workflow} dispatched on {git_ref} (action {action}); the bridge takes it from the next tick"
    ))
}

/// `release close --version <v> --reason <r>`: the waiver of a component
/// deliberately not deployed, recorded on the release item; the next tick
/// closes the cycle on the other facts.
pub fn close(
    t: &Tick,
    log: &mut ActionLog,
    version: &str,
    reason: &str,
    who: &str,
) -> Result<String, String> {
    if reason.trim().is_empty() {
        return Err("a waiver needs a reason".into());
    }
    let shape = Shape::of(&t.config.product.name, &t.config.release);
    let v = crate::method::release::shape::Version::parse(version).ok_or("not a version")?;
    let item =
        release::find_release_item(t, &shape, v)?.ok_or("no release item for this version")?;
    let tag = shape.release_tag(v);
    let marker = texts::marker("waiver", &[("tag", &tag)]);
    if tracker_said(t, &item.key, &marker)? {
        return Ok(format!("{tag}: the waiver is already recorded"));
    }
    let body = for_tracker(
        t,
        &format!(
            "Closed by waiver ({who}): the undeployed components of `{tag}` are deliberately not deployed.\n\nReason: {}\n\n{marker}",
            texts::inert(reason)
        ),
    );
    let mut report = Report::default();
    t.act(
        log,
        &mut report,
        &item.key,
        "comment waiver",
        json!({"reason": reason}),
        || {
            t.tracker
                .comment(&item.key, &body)
                .map(|_| ())
                .map_err(|e| e.to_string())
        },
    )
    .ok_or_else(|| report.failures.join("; "))?;
    Ok(format!(
        "{tag}: waiver recorded on {}; the next tick closes the cycle once the release is published and merged back",
        item.key
    ))
}

/// The severity a finding ticket carries.
fn severity_of(item: &Item) -> Option<Severity> {
    findings::parse_finding_marker(&item.description)?;
    if findings::parse_blocker_marker(&item.description).is_some() {
        Some(Severity::Blocker)
    } else if item
        .labels
        .iter()
        .any(|l| l == vocabulary::QA_FINDING_LABEL)
    {
        Some(Severity::ShouldFix)
    } else {
        None
    }
}

/// `release requalify <ticket> --severity <s> --reason <r>`.
pub fn requalify(
    t: &Tick,
    log: &mut ActionLog,
    key: &str,
    to: Severity,
    reason: &str,
    who: &str,
) -> Result<String, String> {
    let item = t.tracker.get(key).map_err(|e| e.to_string())?;
    let from =
        severity_of(&item).ok_or_else(|| format!("{key} is not a finding ticket of a release"))?;
    let comment = findings::requalify_comment(who, from, to, reason)?;
    if from == to {
        return Ok(format!("{key} is already {}: nothing to do", to.name()));
    }
    let (release_key, ..) =
        findings::parse_finding_marker(&item.description).ok_or("no finding marker")?;
    let mut report = Report::default();
    let mut ok = true;
    for step in findings::requalify_plan(from, to) {
        let done = match step {
            RequalifyStep::WriteBlockerMarker => {
                let d = format!(
                    "{}\n\n{}",
                    findings::blocker_marker(&release_key),
                    item.description
                );
                let d = for_tracker(t, &d);
                t.act(
                    log,
                    &mut report,
                    key,
                    "describe: blocker marker",
                    json!({}),
                    || {
                        t.tracker
                            .update_description(key, &d)
                            .map_err(|e| e.to_string())
                    },
                )
            }
            RequalifyStep::RemoveBlockerMarker => {
                let d = findings::remove_blocker_marker(&item.description);
                t.act(
                    log,
                    &mut report,
                    key,
                    "describe: no blocker marker",
                    json!({}),
                    || {
                        t.tracker
                            .update_description(key, &d)
                            .map_err(|e| e.to_string())
                    },
                )
            }
            RequalifyStep::LayBlocksRelation => t.act(
                log,
                &mut report,
                key,
                &format!("relate Blocks {release_key}"),
                json!({}),
                || {
                    t.tracker
                        .relate(key, &release_key, RelationKind::Blocks)
                        .map_err(|e| e.to_string())
                },
            ),
            RequalifyStep::RemoveBlocksRelation => {
                let arcs = t.tracker.relations(key).map_err(|e| e.to_string())?;
                let mut all = Some(());
                for arc in arcs.blocks.iter().filter(|a| a.other == release_key) {
                    all = all.and(t.act(
                        log,
                        &mut report,
                        key,
                        &format!("unrelate {}", arc.id),
                        json!({}),
                        || t.tracker.unrelate(&arc.id).map_err(|e| e.to_string()),
                    ));
                }
                all
            }
            RequalifyStep::AddFindingLabel => {
                t.act(log, &mut report, key, "label qa-finding", json!({}), || {
                    t.tracker
                        .add_label(key, vocabulary::QA_FINDING_LABEL)
                        .map_err(|e| e.to_string())
                })
            }
            RequalifyStep::SetTriage => {
                t.act(log, &mut report, key, "status Triage", json!({}), || {
                    t.tracker
                        .set_status(key, Status::Triage, None)
                        .map_err(|e| e.to_string())
                })
            }
            RequalifyStep::MoveToNitsTicket | RequalifyStep::CancelTicket => {
                // A nit is a line of the shared nits ticket: the ticket is
                // canceled and its finding becomes that line.
                t.act(
                    log,
                    &mut report,
                    key,
                    "status Canceled (now a nit)",
                    json!({}),
                    || {
                        t.tracker
                            .set_status(key, Status::Canceled, None)
                            .map_err(|e| e.to_string())
                    },
                )
            }
        };
        ok &= done.is_some();
    }
    let body = for_tracker(t, &comment);
    for target in [key, release_key.as_str()] {
        ok &= t
            .act(
                log,
                &mut report,
                target,
                "comment requalification",
                json!({"from": from.name(), "to": to.name()}),
                || {
                    t.tracker
                        .comment(target, &body)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                },
            )
            .is_some();
    }
    if ok {
        Ok(format!(
            "{key}: {} → {}; the GO is recomputed at the next tick",
            from.name(),
            to.name()
        ))
    } else {
        Err(report.failures.join("; "))
    }
}

/// `release abandon` (spec 05 §13): keep the tags, merge the release branch
/// back into the default branch, cancel the release item, and turn the
/// remaining blockers into ordinary tickets.
pub fn abandon(t: &Tick, log: &mut ActionLog, who: &str) -> Result<String, String> {
    let cy = cycle(t)?;
    let mut report = Report::default();
    let default = t
        .github
        .default_branch(&t.repo)
        .map_err(|e| e.to_string())?;
    if let Some(branch) = &cy.branch {
        let cmp = t
            .github
            .compare(&t.repo, &default, branch)
            .map_err(|e| e.to_string())?;
        if matches!(cmp.status, CompareStatus::Ahead | CompareStatus::Diverged) {
            // The merge is pinned to the branch as read now.
            let head = t
                .github
                .commit(&t.repo, branch)
                .map_err(|e| e.to_string())?
                .sha;
            let title = format!("chore: merge back {branch} (cycle abandoned)");
            let number = t
                .act(log, &mut report, &cy.item.key, &format!("open merge-back of {branch}"), json!({}), || {
                    t.github
                        .create_pull(&t.repo, branch, &default, &title, "The cycle was abandoned; its merged fixes return to the default branch.")
                        .map_err(|e| e.to_string())
                })
                .ok_or_else(|| report.failures.join("; "))?;
            t.act(
                log,
                &mut report,
                &cy.item.key,
                &format!("merge #{number} (merge commit)"),
                json!({"head": head}),
                || {
                    t.github
                        .merge_pull(&t.repo, number, &head, MergeMethod::Merge)
                        .map_err(|e| e.to_string())
                },
            )
            .ok_or_else(|| {
                format!(
                    "{} — resolve the merge-back by hand, then run abandon again",
                    report.failures.join("; ")
                )
            })?;
        }
        // The next cut opens a new cycle only once no release branch is left.
        t.act(
            log,
            &mut report,
            &cy.item.key,
            &format!("delete branch {branch}"),
            json!({}),
            || {
                t.github
                    .delete_branch(&t.repo, branch)
                    .map_err(|e| e.to_string())
            },
        )
        .ok_or_else(|| report.failures.join("; "))?;
    }
    let relations = t
        .tracker
        .relations(&cy.item.key)
        .map_err(|e| e.to_string())?;
    for arc in &relations.blocked_by {
        let blocker = t.tracker.get(&arc.other).map_err(|e| e.to_string())?;
        if blocker.state.is_resolved() {
            continue;
        }
        t.act(
            log,
            &mut report,
            &arc.other,
            &format!("unrelate {}", arc.id),
            json!({}),
            || t.tracker.unrelate(&arc.id).map_err(|e| e.to_string()),
        );
        let d = findings::remove_blocker_marker(&blocker.description);
        if d != blocker.description {
            t.act(
                log,
                &mut report,
                &arc.other,
                "describe: no blocker marker",
                json!({}),
                || {
                    t.tracker
                        .update_description(&arc.other, &d)
                        .map_err(|e| e.to_string())
                },
            );
        }
    }
    let body = for_tracker(
        t,
        &format!(
            "Cycle abandoned by {who}: the tags are kept, the release branch is merged back, the remaining blockers are ordinary tickets. The next cut starts a new cycle."
        ),
    );
    t.act(
        log,
        &mut report,
        &cy.item.key,
        "comment abandoned",
        json!({}),
        || {
            t.tracker
                .comment(&cy.item.key, &body)
                .map(|_| ())
                .map_err(|e| e.to_string())
        },
    );
    t.act(
        log,
        &mut report,
        &cy.item.key,
        "status Canceled",
        json!({}),
        || {
            t.tracker
                .set_status(&cy.item.key, Status::Canceled, None)
                .map_err(|e| e.to_string())
        },
    );
    if report.failures.is_empty() {
        Ok(format!(
            "{}: cycle of {} abandoned",
            cy.item.key, cy.candidate.tag
        ))
    } else {
        Err(report.failures.join("; "))
    }
}
