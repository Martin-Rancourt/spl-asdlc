//! The sweep (spec 04 §7, §10, spec 07 step 4): the PRs that resolved in the
//! last `bridge.sweep_hours`.
//!
//! - A PR merged outside the bridge gets the same transition as one the
//!   bridge merged, once. The bridge's own merge comment on the ticket
//!   (`merged_pr: <url>`) is what "once" reads, so a ticket the closure sent
//!   back to In Progress is not pushed forward again.
//! - A merged PR's review round closes, and a PR closed without merge has its
//!   round canceled, whatever becomes of its ticket (BR-ASDLC-007); a ticket
//!   left In Review with no PR in review goes where the state rules say.

use serde_json::json;

use crate::action_log::ActionLog;
use crate::method::release::prs::trusted_author;
use crate::method::review;
use crate::method::texts;
use crate::method::ticket_state::{self, Facts, OpenPr};
use crate::method::vocabulary::Status;
use crate::tracker::Item;

use super::link::{self, Linked};
use super::merge::{self, OpenPrs};
use super::{Report, Tick};

pub fn pass(t: &Tick, log: &mut ActionLog, report: &mut Report, open: &OpenPrs) {
    merged(t, log, report, open);
    if t.in_time() {
        closed(t, log, report, open);
    }
}

/// PRs closed without merge in the window. Every one has its review round
/// canceled; its ticket, if it was waiting In Review, goes where the state
/// rules say. A PR of a ticketless App (spec 04 §9; Dependabot supersedes its
/// own PRs) also has the ticket the bridge opened for it canceled, with that
/// ticket's open sub-items first, so a failure leaves the ticket open and the
/// next tick finishes.
fn closed(t: &Tick, log: &mut ActionLog, report: &mut Report, open: &OpenPrs) {
    let authors = &t.config.review.ticketless_authors;
    let hours = i64::from(t.config.bridge.sweep_hours);
    let since = crate::github::Timestamp(t.now.0 - time::Duration::hours(hours));
    let prs = match t.github.closed_pulls(&t.repo, since, 500) {
        Ok(p) => p,
        Err(e) => {
            t.unreadable(report, "closed PRs", e);
            return;
        }
    };
    let branch_prefix = t.config.release_machinery();
    for pr in &prs {
        if !t.in_time() {
            report.out_of_time = true;
            return;
        }
        if review::excluded_from_merge_pass(pr, branch_prefix).is_some() {
            continue;
        }
        let object = format!("pr#{}", pr.number);
        if trusted_author(pr, authors) {
            closed_ticketless(t, log, report, pr);
        }
        let ticket = match link::ticket_item(pr, t.tracker, t.config.tracker.link_fallback) {
            Ok(Linked::One(item)) => *item,
            Ok(_) => continue,
            Err(e) => {
                t.unreadable(report, &object, format!("the linked ticket: {e}"));
                continue;
            }
        };
        // The ticket the bridge opened for a ticketless PR is canceled above.
        if ticket.archived
            || texts::has_marker(&ticket.description, &texts::pr_ticket_marker(&pr.url))
        {
            continue;
        }
        if !merge::close_round(t, log, report, &ticket.key, &pr.url, false) {
            continue;
        }
        if ticket.state.status == Some(Status::InReview) {
            settle_after_close(t, log, report, &pr.url, pr.number, &ticket, open);
        }
    }
}

/// A ticket In Review whose PR closed without merge: In Review stays only
/// while another of its PRs is (or waits to be) reviewed; otherwise the
/// completeness rule says In Progress, and To Deploy only when every box is
/// checked and an earlier PR of it was merged (BR-ASDLC-008), or, with
/// nothing to deploy, Done (Post Deploy while its own `## Post deploy` boxes
/// wait). The comment
/// goes first and the status last, so a cut tick retries the status without
/// repeating the comment.
fn settle_after_close(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr_url: &str,
    number: u64,
    ticket: &Item,
    open: &OpenPrs,
) {
    let object = format!("pr#{number} {}", ticket.key);
    let prs: Vec<OpenPr> = match merge::open_prs_of(t, open, &ticket.key, Some(number)) {
        Ok(p) => p,
        Err(e) => {
            t.unreadable(report, &object, format!("the ticket's children: {e}"));
            return;
        }
    };
    let merged = match merge::any_merge_recorded(t, &ticket.key) {
        Ok(m) => m,
        Err(e) => {
            t.unreadable(report, &object, format!("the ticket's comments: {e}"));
            return;
        }
    };
    let facts = Facts {
        open: &prs,
        open_known: open.known,
        description: &ticket.description,
        merged,
    };
    let Some((to, why)) = ticket_state::decide(&facts) else {
        t.unreadable(
            report,
            &object,
            "an open PR's ticket could not be read, so the state of this one waits",
        );
        return;
    };
    if to == Status::InReview {
        return;
    }
    let (to, why) = match merge::delivered(t, ticket, to, why) {
        Ok(d) => d,
        Err(e) => {
            t.unreadable(report, &object, format!("its contract: {e}"));
            return;
        }
    };
    let body = texts::closed_unmerged_comment(pr_url, to.name(), &why.next());
    let marker = texts::marker("closed-unmerged", &[("pr", pr_url)]);
    match merge::tracker_said(t, &ticket.key, &marker) {
        Ok(true) => {}
        Ok(false) => {
            let body = merge::for_tracker(t, &body);
            let said = t.act(
                log,
                report,
                &object,
                "comment closed unmerged",
                json!({}),
                || {
                    t.tracker
                        .comment(&ticket.key, &body)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                },
            );
            if said.is_none() {
                return;
            }
        }
        Err(e) => {
            t.unreadable(report, &object, format!("the ticket's comments: {e}"));
            return;
        }
    }
    merge::set_state(
        t,
        log,
        report,
        &object,
        ticket,
        to,
        json!({"closed_pr": pr_url, "next": why.next()}),
    );
}

/// The ticket the bridge opened for a ticketless App's PR closed without
/// merge, and that ticket's open sub-items, are canceled (spec 04 §9).
fn closed_ticketless(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr: &crate::github::PullRequest,
) {
    let object = format!("pr#{}", pr.number);
    let marker = texts::pr_ticket_marker(&pr.url);
    let keys = match t.tracker.items_linked_to_pr(&pr.url) {
        Ok(k) => k,
        Err(e) => {
            t.unreadable(report, &object, format!("the linked items: {e}"));
            return;
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

/// PRs merged in the window: their review rounds close; those merged outside
/// the bridge get the transition of a merge.
fn merged(t: &Tick, log: &mut ActionLog, report: &mut Report, open: &OpenPrs) {
    let hours = i64::from(t.config.bridge.sweep_hours);
    let since = crate::github::Timestamp(t.now.0 - time::Duration::hours(hours));
    let prs = match t.github.merged_pulls(&t.repo, None, Some(since), 500) {
        Ok(p) => p,
        Err(e) => {
            t.unreadable(report, "merged PRs", e);
            return;
        }
    };
    let branch_prefix = t.config.release_machinery();
    for pr in prs {
        if !t.in_time() {
            report.out_of_time = true;
            return;
        }
        if review::excluded_from_merge_pass(&pr, branch_prefix).is_some() {
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
        if ticket.archived {
            continue;
        }
        merge::close_round(t, log, report, &ticket.key, &pr.url, true);
        if !matches!(
            ticket.state.status,
            Some(Status::Ready | Status::InProgress | Status::InReview)
        ) {
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
        merge::after_merge(
            t,
            log,
            report,
            &pr,
            &ticket,
            Some(by.as_deref().unwrap_or("someone")),
            open,
        );
    }
}
