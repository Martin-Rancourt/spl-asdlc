//! Post-deploy validation (`docs/reference/05-release-cycle.md` §11,
//! BR-ASDLC-002/003). When the closure (or, for a ticket with nothing to
//! deploy, the merge) moves a ticket to Post Deploy it labels it `post-deploy-validation` and creates one validation sub-task,
//! assigned to the operator in Post Deploy and urgent (BR-ASDLC-017:
//! production is waiting on it). The operator verifies the ticket's `## Post
//! deploy` boxes in production and checks them. At each tick the bridge reads the labelled tickets and moves
//! to Done each one whose boxes are all checked. No verb: the boxes are the
//! gesture. The ticket itself is never served: Post Deploy stays a
//! validation state.
//!
//! The label is only an index (a tracker cannot be listed by status): the
//! status and the boxes are the facts, and the label is removed once the
//! ticket is resolved, so the list stays as short as the tickets still
//! waiting. The sub-task is found again among the ticket's children by its
//! marker: a lost answer is found, never created twice.

use serde_json::json;

use crate::action_log::ActionLog;
use crate::method::texts;
use crate::method::vocabulary::{post_deploy_label, Status};
use crate::tracker::{Item, NewItem, Priority};

use super::closure::contract_gate;
use super::identity;
use super::merge::for_tracker;
use super::{Report, Tick};

/// The tick's step after the sweep: every labelled ticket still in Post
/// Deploy whose contract is complete moves to Done; one that still waits
/// gets its validation sub-task if it lost it.
pub fn pass(t: &Tick, log: &mut ActionLog, report: &mut Report) {
    let label = label(t);
    let items = match t.tracker.list_by_label(&label) {
        Ok(i) => i,
        Err(e) => {
            t.unreadable(
                report,
                "post-deploy validation",
                format!("the items labelled {label}: {e}"),
            );
            return;
        }
    };
    for item in items {
        if !t.in_time() {
            report.out_of_time = true;
            return;
        }
        if item.archived {
            continue;
        }
        if item.state.is_resolved() {
            unwatch(t, log, report, &item);
            continue;
        }
        // Moved elsewhere by someone: not the bridge's to judge here. The
        // label stays until the ticket is resolved.
        if item.state.status != Some(Status::PostDeploy) {
            continue;
        }
        // The same Done gate as every door to Done: the ticket's boxes and
        // its duplicates', a read that fails never promotes.
        let gate = match contract_gate(t, &item) {
            Ok(g) => g,
            Err(e) => {
                t.unreadable(report, &item.key, format!("its contract: {e}"));
                continue;
            }
        };
        if let Some(why) = &gate.unreadable_reason {
            t.unreadable(report, &item.key, format!("its contract: {why}"));
        } else if gate.ok() {
            let done = t
                .act(
                    log,
                    report,
                    &item.key,
                    "status Done (post-deploy boxes checked)",
                    json!({}),
                    || {
                        t.tracker
                            .set_status(&item.key, Status::Done, None)
                            .map_err(|e| e.to_string())
                    },
                )
                .is_some();
            if done {
                unwatch(t, log, report, &item);
            }
        } else {
            ensure_sub_task(t, log, report, &item);
        }
    }
}

fn label(t: &Tick) -> String {
    post_deploy_label(&t.config.product.name)
}

fn labelled(t: &Tick, item: &Item) -> bool {
    let label = label(t);
    item.labels.iter().any(|l| l.eq_ignore_ascii_case(&label))
}

/// Put the index label on a ticket going to Post Deploy; whether it carries
/// it now. Nothing is written when it already does.
pub fn watch(t: &Tick, log: &mut ActionLog, report: &mut Report, item: &Item) -> bool {
    let label = label(t);
    if labelled(t, item) {
        return true;
    }
    t.act(
        log,
        report,
        &item.key,
        &format!("ensure label {label}"),
        json!({}),
        || t.tracker.ensure_label(&label).map_err(|e| e.to_string()),
    )
    .is_some()
        && t.act(
            log,
            report,
            &item.key,
            &format!("label {label}"),
            json!({}),
            || {
                t.tracker
                    .add_label(&item.key, &label)
                    .map_err(|e| e.to_string())
            },
        )
        .is_some()
}

/// Take the index label off a resolved ticket.
fn unwatch(t: &Tick, log: &mut ActionLog, report: &mut Report, item: &Item) {
    let label = label(t);
    if !labelled(t, item) {
        return;
    }
    t.act(
        log,
        report,
        &item.key,
        &format!("unlabel {label}"),
        json!({}),
        || {
            t.tracker
                .remove_label(&item.key, &label)
                .map_err(|e| e.to_string())
        },
    );
}

/// The validation sub-task of a ticket in Post Deploy, created once
/// (BR-ASDLC-003): assigned to the operator, in Post Deploy, urgent
/// (BR-ASDLC-017), under the
/// ticket. One the ticket
/// already has, in any state, is never created again: a sub-task closed while
/// boxes remain leaves the ticket waiting in Post Deploy for a human.
pub fn ensure_sub_task(t: &Tick, log: &mut ActionLog, report: &mut Report, parent: &Item) {
    let marker = texts::post_deploy_validation_marker(&parent.key);
    let assignee = match t.tracker.user(&t.config.owner.tracker_user) {
        Ok(user) => user,
        Err(e) => {
            t.unreadable(
                report,
                &parent.key,
                format!(
                    "the post-deploy operator {}: {e}",
                    t.config.owner.tracker_user
                ),
            );
            return;
        }
    };
    let children = match t.tracker.children(&parent.key) {
        Ok(c) => c,
        Err(e) => {
            t.unreadable(report, &parent.key, format!("its sub-items: {e}"));
            return;
        }
    };
    match identity::find(t, log, report, &children, &marker, |c| {
        texts::has_marker(&c.description, &marker)
    }) {
        Ok(found) if !found.is_empty() => {
            for child in found {
                reconcile_sub_task(t, log, report, &child, &assignee);
            }
            return;
        }
        Ok(_) => {}
        Err(e) => {
            t.unreadable(report, &parent.key, e);
            return;
        }
    }
    let (title, body) = texts::post_deploy_validation_item(
        &parent.key,
        &parent.title,
        &parent.url,
        &t.config.owner.tracker_user,
    );
    identity::create(
        t,
        log,
        report,
        &parent.key,
        "create post-deploy validation",
        json!({"parent": parent.key}),
        &NewItem {
            title,
            description: for_tracker(t, &body),
            labels: Vec::new(),
            status: Status::PostDeploy,
            // Production is waiting on it (BR-ASDLC-017).
            priority: Some(Priority::Urgent),
            assignee: Some(assignee),
            parent: Some(parent.key.clone()),
        },
    );
}

/// Bring validation sub-tasks created before SR-015 to their current shape.
/// The migration is deliberately part of the normal pass: it reaches every
/// open indexed parent on the next tick and remains idempotent afterwards.
fn reconcile_sub_task(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    child: &Item,
    operator: &crate::tracker::User,
) {
    let reconciled = if child.state.status != Some(Status::PostDeploy) {
        t.act(
            log,
            report,
            &child.key,
            "move legacy post-deploy validation to Post Deploy and assign operator",
            json!({}),
            || {
                t.tracker
                    .set_status(&child.key, Status::PostDeploy, Some(Some(operator)))
                    .map_err(|e| e.to_string())
            },
        )
        .is_some()
    } else if child.assignee.as_ref() != Some(operator) {
        t.act(
            log,
            report,
            &child.key,
            "assign post-deploy validation to operator",
            json!({}),
            || {
                t.tracker
                    .assign(&child.key, Some(operator))
                    .map_err(|e| e.to_string())
            },
        )
        .is_some()
    } else {
        true
    };
    if reconciled
        && child.labels
        .iter()
        .any(|label| label.eq_ignore_ascii_case("blocked-by-PM-decision"))
    {
        t.act(
            log,
            report,
            &child.key,
            "remove PM-decision label from post-deploy validation",
            json!({}),
            || {
                t.tracker
                    .remove_label(&child.key, "blocked-by-PM-decision")
                    .map_err(|e| e.to_string())
            },
        );
    }
}
