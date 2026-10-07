//! Post-deploy validation (`docs/spec/05-release-cycle.md` §11,
//! BR-ASDLC-002/003). When the closure moves a ticket to Post Deploy it
//! labels it `post-deploy-validation` and creates one validation sub-task,
//! unassigned in Ready, that an agent takes: verify the ticket's
//! `## Post deploy` boxes in production and check them, or hand the sub-task
//! to the owner. At each tick the bridge reads the labelled tickets and moves
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
use crate::method::vocabulary::{POST_DEPLOY_LABEL, Status};
use crate::tracker::{Item, NewItem};

use super::closure::contract_gate;
use super::merge::for_tracker;
use super::{Report, Tick};

/// The tick's step after the sweep: every labelled ticket still in Post
/// Deploy whose contract is complete moves to Done; one that still waits
/// gets its validation sub-task if it lost it.
pub fn pass(t: &Tick, log: &mut ActionLog, report: &mut Report) {
    let items = match t.tracker.list_by_label(POST_DEPLOY_LABEL) {
        Ok(i) => i,
        Err(e) => {
            t.unreadable(
                report,
                "post-deploy validation",
                format!("the items labelled {POST_DEPLOY_LABEL}: {e}"),
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

fn labelled(item: &Item) -> bool {
    item.labels
        .iter()
        .any(|l| l.eq_ignore_ascii_case(POST_DEPLOY_LABEL))
}

/// Put the index label on a ticket going to Post Deploy; whether it carries
/// it now. Nothing is written when it already does.
pub fn watch(t: &Tick, log: &mut ActionLog, report: &mut Report, item: &Item) -> bool {
    if labelled(item) {
        return true;
    }
    t.act(
        log,
        report,
        &item.key,
        &format!("ensure label {POST_DEPLOY_LABEL}"),
        json!({}),
        || {
            t.tracker
                .ensure_label(POST_DEPLOY_LABEL)
                .map_err(|e| e.to_string())
        },
    )
    .is_some()
        && t.act(
            log,
            report,
            &item.key,
            &format!("label {POST_DEPLOY_LABEL}"),
            json!({}),
            || {
                t.tracker
                    .add_label(&item.key, POST_DEPLOY_LABEL)
                    .map_err(|e| e.to_string())
            },
        )
        .is_some()
}

/// Take the index label off a resolved ticket.
fn unwatch(t: &Tick, log: &mut ActionLog, report: &mut Report, item: &Item) {
    if !labelled(item) {
        return;
    }
    t.act(
        log,
        report,
        &item.key,
        &format!("unlabel {POST_DEPLOY_LABEL}"),
        json!({}),
        || {
            t.tracker
                .remove_label(&item.key, POST_DEPLOY_LABEL)
                .map_err(|e| e.to_string())
        },
    );
}

/// The validation sub-task of a ticket in Post Deploy, created once
/// (BR-ASDLC-003): unassigned, in Ready, under the ticket. One the ticket
/// already has, in any state, is never created again: a sub-task closed while
/// boxes remain leaves the ticket waiting in Post Deploy for a human.
pub fn ensure_sub_task(t: &Tick, log: &mut ActionLog, report: &mut Report, parent: &Item) {
    let marker = texts::post_deploy_validation_marker(&parent.key);
    let children = match t.tracker.children(&parent.key) {
        Ok(c) => c,
        Err(e) => {
            t.unreadable(report, &parent.key, format!("its sub-items: {e}"));
            return;
        }
    };
    if children
        .iter()
        .any(|c| texts::has_marker(&c.description, &marker))
    {
        return;
    }
    let (title, body) = texts::post_deploy_validation_item(
        &parent.key,
        &parent.title,
        &parent.url,
        &t.config.owner.tracker_user,
    );
    let description = for_tracker(t, &body);
    t.act(
        log,
        report,
        &parent.key,
        "create post-deploy validation",
        json!({"parent": parent.key}),
        || {
            t.tracker
                .create_item(&NewItem {
                    title: title.clone(),
                    description: description.clone(),
                    labels: Vec::new(),
                    status: Status::Ready,
                    priority: None,
                    assignee: None,
                    parent: Some(parent.key.clone()),
                })
                .map(|_| ())
                .map_err(|e| e.to_string())
        },
    );
}
