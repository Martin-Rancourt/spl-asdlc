//! The sweep (spec 04 §7, spec 07 step 4): a PR merged outside the bridge in
//! the last `bridge.sweep_hours` gets the same transition as one the bridge
//! merged, once. The bridge's own merge comment on the ticket
//! (`merged_pr: <url>`) is what "once" reads, so a ticket the closure sent
//! back to In Progress is not pushed forward again.

use serde_json::json;

use crate::action_log::ActionLog;
use crate::method::release::prs::trusted_author;
use crate::method::review;
use crate::method::texts;
use crate::method::vocabulary::Status;

use super::link::{self, Linked};
use super::merge;
use super::{Report, Tick};

pub fn pass(t: &Tick, log: &mut ActionLog, report: &mut Report) {
    merged(t, log, report);
    if t.in_time() {
        closed_ticketless(t, log, report);
    }
}

/// A PR of a ticketless App closed without merge in the window (spec 04 §9;
/// Dependabot supersedes its own PRs): the ticket the bridge opened for it,
/// and that ticket's open sub-items, are canceled. The sub-items go first,
/// so a failure leaves the ticket open and the next tick finishes.
fn closed_ticketless(t: &Tick, log: &mut ActionLog, report: &mut Report) {
    let authors = &t.config.review.ticketless_authors;
    if authors.is_empty() {
        return;
    }
    let hours = i64::from(t.config.bridge.sweep_hours);
    let since = crate::github::Timestamp(t.now.0 - time::Duration::hours(hours));
    let prs = match t.github.closed_pulls(&t.repo, since, 500) {
        Ok(p) => p,
        Err(e) => {
            t.unreadable(report, "closed PRs", e);
            return;
        }
    };
    for pr in prs.iter().filter(|p| trusted_author(p, authors)) {
        if !t.in_time() {
            report.out_of_time = true;
            return;
        }
        let object = format!("pr#{}", pr.number);
        let marker = texts::pr_ticket_marker(&pr.url);
        let keys = match t.tracker.items_linked_to_pr(&pr.url) {
            Ok(k) => k,
            Err(e) => {
                t.unreadable(report, &object, format!("the linked items: {e}"));
                continue;
            }
        };
        for key in keys {
            let item = match t.tracker.get(&key) {
                Ok(i) => i,
                Err(e) if e.is_not_found() => continue,
                Err(e) => {
                    t.unreadable(report, &key, e);
                    continue;
                }
            };
            // Only the ticket the bridge opened for this PR, still open.
            if item.archived
                || item.state.is_resolved()
                || !texts::has_marker(&item.description, &marker)
            {
                continue;
            }
            let children = match t.tracker.children(&item.key) {
                Ok(c) => c,
                Err(e) => {
                    t.unreadable(report, &item.key, format!("its sub-items: {e}"));
                    continue;
                }
            };
            let mut all = true;
            for child in children.iter().filter(|c| !c.state.is_resolved()) {
                all &= cancel(t, log, report, &child.key, &pr.url);
            }
            if all {
                cancel(t, log, report, &item.key, &pr.url);
            }
        }
    }
}

fn cancel(t: &Tick, log: &mut ActionLog, report: &mut Report, key: &str, pr_url: &str) -> bool {
    t.act(
        log,
        report,
        key,
        "status Canceled (PR closed without merge)",
        json!({"pr": pr_url}),
        || {
            t.tracker
                .set_status(key, Status::Canceled, None)
                .map_err(|e| e.to_string())
        },
    )
    .is_some()
}

/// PRs merged outside the bridge.
fn merged(t: &Tick, log: &mut ActionLog, report: &mut Report) {
    let hours = i64::from(t.config.bridge.sweep_hours);
    let since = crate::github::Timestamp(t.now.0 - time::Duration::hours(hours));
    let prs = match t.github.merged_pulls(&t.repo, None, Some(since), 500) {
        Ok(p) => p,
        Err(e) => {
            t.unreadable(report, "merged PRs", e);
            return;
        }
    };
    let release = &t.config.release;
    for pr in prs {
        if !t.in_time() {
            report.out_of_time = true;
            return;
        }
        if review::excluded_from_merge_pass(&pr, &release.branch_prefix, &release.release_pr_label)
            .is_some()
        {
            continue;
        }
        let object = format!("pr#{}", pr.number);
        let ticket = match link::ticket_item(&pr, t.tracker, t.config.tracker.link_fallback) {
            Ok(Linked::One(item)) => *item,
            Ok(_) => continue,
            Err(e) => {
                t.unreadable(report, &object, format!("the linked ticket: {e}"));
                continue;
            }
        };
        if ticket.archived
            || !matches!(
                ticket.state.status,
                Some(Status::Ready | Status::InProgress | Status::InReview)
            )
        {
            continue;
        }
        match merge::merge_recorded(t, &ticket.key, &pr.url) {
            Ok(true) => continue,
            Ok(false) => {}
            Err(e) => {
                t.unreadable(report, &object, format!("the ticket's comments: {e}"));
                continue;
            }
        }
        let by = pr.merged_by.as_ref().map(|a| a.login.clone());
        merge::to_deploy(
            t,
            log,
            report,
            &pr,
            &ticket,
            Some(by.as_deref().unwrap_or("someone")),
        );
    }
}
