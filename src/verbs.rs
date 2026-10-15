//! The human gestures of a cycle (`docs/reference/09-human-gestures.md`) and the
//! GO status, written against the traits so that the command line and the
//! tests drive the same code. Each writes through [`Tick::act`], so each is
//! in the action log.

use serde_json::json;

use crate::action_log::ActionLog;
use crate::config::Release;
use crate::github::{CompareStatus, MergeMethod};
use crate::method::release::findings::{self, RequalifyStep};
use crate::method::release::go::{AngleState, GoDecision};
use crate::method::release::prs;
use crate::method::release::report;
use crate::method::release::shape::Shape;
use crate::method::vocabulary::{self, Status};
use crate::tick::merge::for_tracker;
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

/// The `[release]` section: every configuration has one.
pub fn release_cycle<'a>(t: &Tick<'a>) -> &'a Release {
    &t.config.release
}

/// Read the cycle in flight; nothing is created.
pub fn cycle(t: &Tick) -> Result<Cycle, String> {
    let shape = Shape::of(&t.config.product.name, release_cycle(t));
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

/// What `release status` says: the cycle in flight and its GO, or that no
/// cycle is in flight because the newest candidate's cycle closed.
pub enum CycleStatus {
    InFlight(Box<Cycle>, GoDecision),
    /// The newest candidate's release is published and its release item
    /// closed: the cycle is over.
    Over {
        release_tag: String,
        item: String,
    },
}

/// `release status`: the cycle in flight and its GO, read-only; or, once the
/// newest candidate's release is published and its cycle closed, that no
/// cycle is in flight and which release closed the last one.
pub fn release_status(t: &Tick) -> Result<CycleStatus, String> {
    if let Some((release_tag, item)) = closed_cycle(t)? {
        return Ok(CycleStatus::Over { release_tag, item });
    }
    let (cycle, decision) = status(t)?;
    Ok(CycleStatus::InFlight(Box::new(cycle), decision))
}

/// The release tag and release item of the newest candidate's cycle when it
/// is over: its release published (not a draft) and its release item
/// resolved by the closure. `None` while a cycle is in flight.
fn closed_cycle(t: &Tick) -> Result<Option<(String, String)>, String> {
    let shape = Shape::of(&t.config.product.name, release_cycle(t));
    let Some((candidate, _)) = release::newest(t, &shape)? else {
        return Ok(None);
    };
    let Some(item) = release::find_release_item(t, &shape, candidate.version)? else {
        return Ok(None);
    };
    if !item.state.is_resolved() {
        return Ok(None);
    }
    let tag = shape.release_tag(candidate.version);
    match t.github.release(&t.repo, &tag) {
        Ok(r) if !r.draft => Ok(Some((tag, item.key))),
        Ok(_) => Ok(None),
        Err(e) if e.is_not_found() => Ok(None),
        Err(e) => Err(format!("the release {tag}: {e}")),
    }
}

/// The GO of the cycle in flight and its inputs, read-only.
pub fn status(t: &Tick) -> Result<(Cycle, GoDecision), String> {
    let release_cfg = release_cycle(t);
    let record = &release_cfg.record;
    let cy = cycle(t)?;
    let branch = cy
        .branch
        .clone()
        .ok_or_else(|| format!("no release branch holds {}", cy.candidate.tag))?;
    let children = t
        .tracker
        .children(&cy.item.key)
        .map_err(|e| e.to_string())?;
    // A draft angle PR is out of the flow and counts for nothing.
    let open: Vec<_> = t
        .github
        .open_pulls(&t.repo, Some(&branch))
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|p| !p.draft)
        .collect();
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
                let path = report::report_path(record, &c.tag, &angle);
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
    // A later pass under the lighter policy: the angles of the first pass.
    let (states, without_angles) = if !without_angles && release::lighter_pass(release_cfg, c) {
        release::first_pass_angles(t, &cy.shape, &cy.item, c, &branch, &open)?
    } else {
        (states, without_angles)
    };
    let mut report = Report::default();
    let decision = release::decide(
        t,
        &mut report,
        &cy.item,
        c,
        &branch,
        &states,
        without_angles,
    )
    .ok_or_else(|| report.failures.join("; "))?;
    Ok((cy, decision))
}

/// `release cut [--open]`: dispatch the cut workflow, for a new cycle from
/// the default branch or for the next candidate on the release branch.
pub fn cut(t: &Tick, log: &mut ActionLog, open: bool) -> Result<String, String> {
    let cfg = release_cycle(t);
    let shape = Shape::of(&t.config.product.name, cfg);
    let branches: Vec<String> = t
        .github
        .branches(&t.repo, &cfg.branch_prefix)
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
    let workflow = &cfg.cut_workflow;
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

/// Whether a finding ticket blocks its release (its blocker marker), or
/// `None` when the item is no finding ticket of a release.
fn blocking_of(item: &Item) -> Option<bool> {
    findings::parse_finding_marker(&item.description)?;
    Some(findings::parse_blocker_marker(&item.description).is_some())
}

/// `release requalify <ticket> --blocking <true|false> --reason <r>`.
pub fn requalify(
    t: &Tick,
    log: &mut ActionLog,
    key: &str,
    to: bool,
    reason: &str,
    who: &str,
) -> Result<String, String> {
    let item = t.tracker.get(key).map_err(|e| e.to_string())?;
    let from =
        blocking_of(&item).ok_or_else(|| format!("{key} is not a finding ticket of a release"))?;
    let comment = findings::requalify_comment(who, from, to, reason)?;
    if from == to {
        return Ok(format!(
            "{key} is already {}: nothing to do",
            findings::grade(to)
        ));
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
            RequalifyStep::LayBlocksRelation | RequalifyStep::LayRelatedRelation => {
                let kind = if step == RequalifyStep::LayBlocksRelation {
                    RelationKind::Blocks
                } else {
                    RelationKind::Related
                };
                t.act(
                    log,
                    &mut report,
                    key,
                    &format!("relate {kind:?} {release_key}"),
                    json!({}),
                    || {
                        t.tracker
                            .relate(key, &release_key, kind)
                            .map_err(|e| e.to_string())
                    },
                )
            }
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
            RequalifyStep::RemoveFindingLabel => {
                if item
                    .labels
                    .iter()
                    .any(|l| l == vocabulary::QA_FINDING_LABEL)
                {
                    t.act(
                        log,
                        &mut report,
                        key,
                        "unlabel qa-finding",
                        json!({}),
                        || {
                            t.tracker
                                .remove_label(key, vocabulary::QA_FINDING_LABEL)
                                .map_err(|e| e.to_string())
                        },
                    )
                } else {
                    Some(())
                }
            }
            RequalifyStep::ServeFixer => {
                // Ready for the release fixer, in the status's own write;
                // unassigned (taken by role) when it is unset or unknown.
                let fixer = release::fixer(t);
                let who = fixer.as_ref().map_or("unassigned".to_string(), |u| {
                    format!("assigned to {}", u.name)
                });
                t.act(
                    log,
                    &mut report,
                    key,
                    &format!("status Ready, {who}"),
                    json!({}),
                    || {
                        t.tracker
                            .set_status(key, Status::Ready, Some(fixer.as_ref()))
                            .map_err(|e| e.to_string())
                    },
                )
            }
            RequalifyStep::LogForLater => t.act(
                log,
                &mut report,
                key,
                "status Triage, unassigned",
                json!({}),
                || {
                    t.tracker
                        .set_status(key, Status::Triage, Some(None))
                        .map_err(|e| e.to_string())
                },
            ),
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
                json!({"from": findings::grade(from), "to": findings::grade(to)}),
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
            findings::grade(from),
            findings::grade(to)
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
