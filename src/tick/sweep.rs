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
//! - A ticket left In Progress past a merge, whose boxes were checked since,
//!   goes where the state rules say, whenever the PRs of the window are
//!   ([`settle_merged`]).

use serde_json::json;

use crate::action_log::ActionLog;
use crate::github::CompareStatus;
use crate::method::business_rules;
use crate::method::release::closure::{self as release_closure, Judgement};
use crate::method::release::prs::trusted_author;
use crate::method::release::shape::Shape;
use crate::method::review;
use crate::method::texts;
use crate::method::ticket_state::{self, Facts, OpenPr, Why};
use crate::method::vocabulary::Status;
use crate::tracker::Item;

use super::identity;
use super::link::{self, Linked};
use super::merge::{self, OpenPrs};
use super::{Report, Tick};

pub fn pass(t: &Tick, log: &mut ActionLog, report: &mut Report, open: &OpenPrs) {
    merged(t, log, report, open);
    if t.in_time() {
        closed(t, log, report, open);
    }
    if t.in_time() {
        settle_merged(t, log, report, open);
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
        let ticket = match link::ticket_item(pr, t.tracker, &t.config.tracker) {
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
/// while another of its PRs is opened and not sent back; otherwise the
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
    let prs: Vec<OpenPr> = merge::open_prs_of(open, &ticket.key, Some(number));
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
        // Only the ticket the bridge opened for this PR, still open: by its
        // marker, or by its identity comment when its description lost it.
        if item.archived || item.state.is_resolved() {
            continue;
        }
        let described = texts::has_marker(&item.description, &marker);
        match identity::is(t, log, report, &item, &marker, described) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(e) => {
                t.unreadable(report, &key, e);
                continue;
            }
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
        let ticket = match link::ticket_item(&pr, t.tracker, &t.config.tracker) {
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
            &texts::MergedBy::Outside(by.as_deref().unwrap_or("someone")),
            open,
        );
    }
}

/// The tickets left In Progress past a merge (spec 04 §10): merged while a
/// box was open, or sent back by the closure, their last box checked since.
/// Nothing else moves them, so each tick settles them by the state rules:
/// with no PR of theirs open, every box outside `## Post deploy` checked and
/// a merge recorded, To Deploy while their last merged PR waits for a
/// release; once a release tag holds it, judged as the closure judges, Done
/// or Post Deploy (never To Deploy: no later closure would judge them); with
/// nothing to deploy, as at a merge. A box still open leaves the ticket
/// where it is, and says nothing. The reads go from the cheapest: the
/// tickets In Progress, their PRs and boxes as the tick already has them,
/// then the comments of the complete ones, then GitHub only for one about
/// to move.
fn settle_merged(t: &Tick, log: &mut ActionLog, report: &mut Report, open: &OpenPrs) {
    // "No PR of it open" cannot be said: the PR pass reported why.
    if !open.known {
        return;
    }
    let items = match t.tracker.list_by_status(Status::InProgress) {
        Ok(i) => i,
        Err(e) => {
            t.unreadable(
                report,
                "merged tickets",
                format!("the items in {}: {e}", Status::InProgress),
            );
            return;
        }
    };
    for ticket in items {
        if !t.in_time() {
            report.out_of_time = true;
            return;
        }
        if ticket.archived
            || texts::bridge_item(&ticket.description, ticket.parent.as_deref()).is_some()
        {
            continue;
        }
        // A ticket with an open PR is the PR pass's.
        let prs = merge::open_prs_of(open, &ticket.key, None);
        if !prs.is_empty() {
            continue;
        }
        let facts = Facts {
            open: &prs,
            open_known: true,
            description: &ticket.description,
            merged: true,
        };
        if let Some((Status::ToDeploy, why)) = ticket_state::decide(&facts) {
            settle_merged_one(t, log, report, &ticket, why);
        }
    }
}

/// One complete ticket of [`settle_merged`]. The comment goes first and the
/// status last, as in [`settle_after_close`].
fn settle_merged_one(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    ticket: &Item,
    why: ticket_state::Why,
) {
    let object = ticket.key.clone();
    let comments = match t.tracker.comments(&ticket.key) {
        Ok(c) => c,
        Err(e) => {
            t.unreadable(report, &object, format!("the ticket's comments: {e}"));
            return;
        }
    };
    // Never merged: its PR brings it to review, and the merge moves it on.
    let Some(pr_url) = merge::latest_merge(&comments) else {
        return;
    };
    let (to, why, shipped) = if merge::nothing_to_deploy(t, ticket) {
        match merge::delivered(t, ticket, Status::ToDeploy, why) {
            Ok((to, why)) => (to, Judged::State(why), None),
            Err(e) => {
                t.unreadable(report, &object, format!("its contract: {e}"));
                return;
            }
        }
    } else {
        match shipped_in(t, &pr_url) {
            Ok(None) => (Status::ToDeploy, Judged::State(why), None),
            Ok(Some(tag)) => match judge_shipped(t, ticket, &tag) {
                Ok((to, why)) => (to, why, Some(tag)),
                Err(e) => {
                    t.unreadable(report, &object, e);
                    return;
                }
            },
            Err(e) => {
                t.unreadable(report, &object, format!("whether {pr_url} shipped: {e}"));
                return;
            }
        }
    };
    let next = match why {
        // A box still open (a duplicate's): it stays, and nothing is said.
        Judged::State(_) if to == Status::InProgress => return,
        Judged::State(why) => why.next(),
        Judged::UnknownRules(missing) => format!(
            "it cites business rules the registry at `{}` does not declare ({}): fix the id in `Business rules` if it is a typo, or open a PR adding the rule's file, which ships with the next release.",
            shipped.as_deref().unwrap_or_default(),
            missing
                .iter()
                .map(|i| format!("`{i}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    let marker = texts::settled_marker(&pr_url, to.name());
    let said = comments
        .iter()
        .any(|c| c.by_bridge && texts::has_marker(&c.body, &marker));
    if !said {
        let body = merge::for_tracker(
            t,
            &texts::settled_comment(&pr_url, shipped.as_deref(), to.name(), &next),
        );
        let written = t.act(log, report, &object, "comment settled", json!({}), || {
            t.tracker
                .comment(&ticket.key, &body)
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
        if written.is_none() {
            return;
        }
    }
    if to != Status::InProgress {
        merge::set_state(
            t,
            log,
            report,
            &object,
            ticket,
            to,
            json!({"merged_pr": pr_url, "shipped": shipped, "next": next}),
        );
    }
}

/// Why a settled ticket goes where it goes.
enum Judged {
    State(ticket_state::Why),
    /// Shipped and complete, but it cites rules the registry at the tag
    /// lacks: it stays In Progress, and says so once.
    UnknownRules(Vec<String>),
}

/// The release tag that already holds the merge of this PR: the newest
/// release tag, when it holds the PR's merge commit. `None` before the
/// first release, or while the merge waits for one.
fn shipped_in(t: &Tick, pr_url: &str) -> Result<Option<String>, String> {
    let shape = Shape::of(&t.config.product.name, &t.config.release);
    let tags = t
        .github
        .tags(&t.repo, shape.release_tag_prefix())
        .map_err(|e| e.to_string())?;
    let Some((tag, _)) = shape.previous_release(tags.iter().map(|r| r.name.as_str()), None) else {
        return Ok(None);
    };
    let number = pr_url
        .trim()
        .rsplit('/')
        .next()
        .and_then(|n| n.parse::<u64>().ok())
        .ok_or_else(|| format!("no PR number in {pr_url}"))?;
    let pr = t.github.pull(&t.repo, number).map_err(|e| e.to_string())?;
    let sha = pr
        .merge_commit_sha
        .ok_or_else(|| format!("{pr_url} has no merge commit"))?;
    let status = t
        .github
        .compare_status(&t.repo, &sha, &tag)
        .map_err(|e| e.to_string())?;
    Ok(matches!(status, CompareStatus::Ahead | CompareStatus::Identical).then_some(tag))
}

/// A shipped ticket judged as the closure judges it (spec 05 §11): the
/// rules it cites against the registry at `tag`, then the Done gate. `Err`
/// when something could not be read: nothing is written.
fn judge_shipped(t: &Tick, ticket: &Item, tag: &str) -> Result<(Status, Judged), String> {
    let gate =
        super::closure::contract_gate(t, ticket).map_err(|e| format!("its contract: {e}"))?;
    let registry = release_closure::needs_registry(&ticket.description)
        .then(|| business_rules::read_registry(t.github, &t.repo, tag));
    let judgement = release_closure::judge(
        &ticket.key,
        Some(Status::ToDeploy),
        &ticket.description,
        &gate,
        registry.as_ref(),
    );
    let tag = tag.to_string();
    Ok(match judgement {
        Judgement::Done => (Status::Done, Judged::State(Why::Shipped(tag))),
        Judgement::PostDeploy => (
            Status::PostDeploy,
            Judged::State(Why::ShippedAwaitsPostDeploy(tag)),
        ),
        Judgement::Reopen => (
            Status::InProgress,
            Judged::State(Why::BoxesOpen(
                gate.open_items.iter().map(|b| b.line.clone()).collect(),
            )),
        ),
        Judgement::UnknownRules(missing) => (Status::InProgress, Judged::UnknownRules(missing)),
        Judgement::RulesUnverified(why) => {
            return Err(format!("the business rules it cites at {tag}: {why}"));
        }
    })
}
