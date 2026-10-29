//! Post-deploy validation (`docs/reference/05-release-cycle.md` §11,
//! BR-ASDLC-002/003/017). The closure (or, for a ticket with nothing to
//! deploy, the merge) moves a ticket whose only open boxes are its `## Post
//! deploy` ones to Post Deploy, and that ticket is the validation: an agent
//! checks the boxes it verifies in production and hands the same ticket to
//! the owner for the rest. At each tick the bridge reads every item in Post
//! Deploy and moves to Done each ticket whose boxes are all checked, whatever
//! its labels. No verb, no sub-task: the boxes are the gesture.
//!
//! Earlier versions created a validation sub-task under the ticket (marker
//! `spl-asdlc:post-deploy-validation parent=<key>`) and labelled the ticket
//! `post-deploy-validation/<product>` as an index. The pass cancels such a
//! sub-task once, with a comment that sends its reader to the ticket, and
//! takes the old label off a ticket it moves to Done.

use serde_json::json;

use crate::action_log::ActionLog;
use crate::method::texts;
use crate::method::vocabulary::{Status, post_deploy_label};
use crate::tracker::Item;

use super::closure::contract_gate;
use super::merge::for_tracker;
use super::{Report, Tick};

/// The tick's step after the sweep: every ticket in Post Deploy whose
/// contract is complete moves to Done; a validation sub-task of an earlier
/// version is canceled.
pub fn pass(t: &Tick, log: &mut ActionLog, report: &mut Report) {
    let items = match t.tracker.list_by_status(Status::PostDeploy) {
        Ok(i) => i,
        Err(e) => {
            t.unreadable(
                report,
                "post-deploy validation",
                format!("the items in {}: {e}", Status::PostDeploy),
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
        if texts::is_post_deploy_validation(&item.description) {
            retire_sub_task(t, log, report, &item);
            continue;
        }
        // The same Done gate as every door to Done: the ticket's boxes and
        // its duplicates', a read that fails never promotes. A ticket with
        // a box still open waits, and nothing is written for it.
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
                unlabel(t, log, report, &item);
            }
        }
    }
}

/// Take the index label of earlier versions off a ticket the pass resolved.
fn unlabel(t: &Tick, log: &mut ActionLog, report: &mut Report, item: &Item) {
    let label = post_deploy_label(&t.config.product.name);
    if !item.labels.iter().any(|l| l.eq_ignore_ascii_case(&label)) {
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

/// Cancel, once, an open validation sub-task an earlier version created
/// (BR-ASDLC-017): its comment first, found again by its marker, so a lost
/// cancel is retried at the next tick without a second comment. A canceled
/// sub-task is no longer in Post Deploy, so the next passes never see it.
fn retire_sub_task(t: &Tick, log: &mut ActionLog, report: &mut Report, item: &Item) {
    let marker = texts::post_deploy_validation_retired_marker();
    let commented = match t.tracker.comments(&item.key) {
        Ok(c) => c
            .iter()
            .any(|c| c.by_bridge && texts::has_marker(&c.body, &marker)),
        Err(e) => {
            t.unreadable(report, &item.key, format!("its comments: {e}"));
            return;
        }
    };
    if !commented {
        let body = for_tracker(
            t,
            &texts::post_deploy_validation_retired_comment(item.parent.as_deref()),
        );
        let posted = t
            .act(
                log,
                report,
                &item.key,
                "comment the validation moved to its ticket",
                json!({}),
                || {
                    t.tracker
                        .comment(&item.key, &body)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                },
            )
            .is_some();
        if !posted {
            return;
        }
    }
    t.act(
        log,
        report,
        &item.key,
        "status Canceled (the validation happens on its ticket)",
        json!({}),
        || {
            t.tracker
                .set_status(&item.key, Status::Canceled, None)
                .map_err(|e| e.to_string())
        },
    );
}
