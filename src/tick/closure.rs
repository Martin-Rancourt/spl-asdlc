//! The closure of a cycle (`docs/reference/05-release-cycle.md` §11-12),
//! triggered by facts, never assumed: the release published, the release
//! branch merged back. Then each member of the release is judged; the release note is sent;
//! the release item moves to Done only once every ticket that cites a rule
//! was judged and the note went out (a failed send is a failed closure step,
//! retried, never dropped). Once the release item is Done, the items the
//! cycle resolved are archived, the release item last (BR-ASDLC-019); an
//! archive that fails is retried at the next tick and never undoes the
//! closure.

use serde_json::json;

use crate::action_log::ActionLog;
use crate::github::{CompareStatus, Release};
use crate::method::business_rules::{self, Registry};
use crate::method::done_gate::{self, DuplicateNode};
use crate::method::markdown;
use crate::method::release::closure::{self, Action, CloseResult, Judgement, Member, Resolved};
use crate::method::release::findings;
use crate::method::release::note::{self, TicketEntry};
use crate::method::release::prs;
use crate::method::release::shape::Shape;
use crate::method::texts::{self, BridgeItem};
use crate::method::vocabulary::{self, ItemState, Status};
use crate::tracker::{Item, TResult};

use super::merge::{for_tracker, tracker_said};
use super::post_deploy;
use super::release::{Candidate, embarked_items};
use super::{Notice, Report, Tick};

/// Close the cycle of `c`. `newer` is the newest candidate when it belongs
/// to the next cycle, which overtook this one before its closure ended:
/// that cycle's release branch, cut after this one merged back, is not a
/// branch this closure waits for.
#[allow(clippy::too_many_arguments)]
pub fn close(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    shape: &Shape,
    c: &Candidate,
    item: &Item,
    published: &Release,
    newer: Option<&Candidate>,
) {
    let object = item.key.clone();
    match ready_to_close(t, shape, c, published, newer) {
        Ok(None) => {}
        Ok(Some(waiting)) => {
            // A fact the closure waits for: nothing written, nothing to say
            // each tick (the stagnation of a published release is a host's
            // job log matter until the owner settles REQ-REL-082). But a
            // publication that failed before deploying never merges back:
            // its release is retracted, and the cycle awaits its GO again.
            let _ = waiting;
            super::retract::when_undeployed(t, log, report, item, published);
            return;
        }
        Err(e) => {
            t.unreadable(report, &object, e);
            return;
        }
    }
    let label = vocabulary::release_label(&shape.product, &c.version.to_string());
    let members = match members(t, item, &label) {
        Ok(m) => m,
        Err(e) => {
            t.unreadable(report, &object, format!("the release's members: {e}"));
            return;
        }
    };
    // What this release shipped, ticket by ticket (its PRs in the range): a
    // ticket whose latest merged PR is not among them waits for the release
    // that ships it (several PRs per ticket, spec 04 §10).
    let range = match embarked_items(t, shape, c) {
        Ok(e) => e,
        Err(e) => {
            t.unreadable(report, &object, format!("the shipped tickets: {e}"));
            return;
        }
    };
    let delivered = range.delivered;
    let sub_items = range.sub_items;
    let shipped: std::collections::BTreeMap<String, Vec<u64>> = range.tickets.into_iter().collect();
    let registry = read_registry(t, &published.tag);
    let mut result = CloseResult::default();
    for m in &members {
        if !t.in_time() {
            report.out_of_time = true;
            return;
        }
        // The review round items of a PR are the bridge's, not tickets, even
        // when they carry the release label (an older bridge labelled them):
        // never judged, archived with the cycle once resolved.
        if round_item(m, item, &sub_items) {
            continue;
        }
        judge_member(
            t,
            log,
            report,
            m,
            item,
            registry.as_ref(),
            &shipped,
            &mut result,
        );
    }
    let marker = texts::marker("closure", &[("tag", &published.tag)]);
    match tracker_said(t, &item.key, &marker) {
        Ok(false) => {
            let go = prs::go_publisher_line(published, &t.github_login);
            let body = for_tracker(
                t,
                &format!("{}\n\n{go}\n\n{marker}", closure::comment(&result)),
            );
            t.act(log, report, &object, "comment closure", json!({}), || {
                t.tracker
                    .comment(&item.key, &body)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            });
        }
        Ok(true) => {}
        Err(e) => t.unreadable(report, &object, e),
    }
    let note_sent = release_note(t, log, report, shape, c, item, published);
    if result.complete() && note_sent {
        let closed = t
            .act(
                log,
                report,
                &object,
                "status Done (closure)",
                json!({"tag": published.tag}),
                || {
                    t.tracker
                        .set_status(&item.key, Status::Done, None)
                        .map_err(|e| e.to_string())
                },
            )
            .is_some();
        if closed {
            // The members as this tick left them: what it moved to Done is
            // Done, even in a dry run that wrote nothing.
            let done: Vec<&String> = result
                .closed
                .iter()
                .chain(&result.children_closed)
                .collect();
            let members: Vec<Item> = members
                .into_iter()
                .map(|mut m| {
                    if done.contains(&&m.key) {
                        m.state = ItemState::of(Status::Done);
                    }
                    m
                })
                .collect();
            if let Some(members) = with_delivered(t, report, members, &delivered) {
                archive_resolved(t, log, report, c, item, &members);
            }
        }
    }
}

/// The members of a closed cycle with, added, the tickets with nothing to
/// deploy whose merge the release's range holds (BR-ASDLC-019): delivered at
/// their merge, never embarked nor labelled, they are archived with the
/// cycle that ships their PRs. A ticket whose latest merged PR is not in the
/// range is left to the release that ships it. `None` when one could not be
/// read: nothing is archived this tick, the next one retries.
fn with_delivered(
    t: &Tick,
    report: &mut Report,
    mut members: Vec<Item>,
    delivered: &[(String, Vec<u64>)],
) -> Option<Vec<Item>> {
    for (key, prs) in delivered {
        if members.iter().any(|m| &m.key == key) {
            continue;
        }
        match latest_merged_pr(t, key) {
            Ok(Some(n)) if !prs.contains(&n) => continue,
            Ok(_) => {}
            Err(e) => {
                t.unreadable(report, key, format!("its merged PRs: {e}"));
                return None;
            }
        }
        match t.tracker.get(key) {
            Ok(i) => members.push(i),
            Err(e) => {
                t.unreadable(report, key, e);
                return None;
            }
        }
    }
    Some(members)
}

/// The archiving of a cycle already closed, at a later tick: what a failed
/// archive or the time budget left (BR-ASDLC-019). Only a release item the
/// closure itself moved to Done (its closure comment is there) is archived
/// from; one closed by hand, or canceled, is not this step's.
pub fn archive_closed(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    shape: &Shape,
    c: &Candidate,
    item: &Item,
    published: &Release,
) {
    if !t.tracker.archives() || item.archived || item.state.status != Some(Status::Done) {
        return;
    }
    let marker = texts::marker("closure", &[("tag", &published.tag)]);
    match tracker_said(t, &item.key, &marker) {
        Ok(true) => {}
        Ok(false) => return,
        Err(e) => {
            t.unreadable(report, &item.key, e);
            return;
        }
    }
    let label = vocabulary::release_label(&shape.product, &c.version.to_string());
    let members = match members(t, item, &label) {
        Ok(m) => m,
        Err(e) => {
            t.unreadable(report, &item.key, format!("the release's members: {e}"));
            return;
        }
    };
    let delivered = match embarked_items(t, shape, c) {
        Ok(e) => e.delivered,
        Err(e) => {
            t.unreadable(report, &item.key, format!("the shipped tickets: {e}"));
            return;
        }
    };
    if let Some(members) = with_delivered(t, report, members, &delivered) {
        archive_resolved(t, log, report, c, item, &members);
    }
}

/// Archive what the closed cycle resolved (spec 05 §11, BR-ASDLC-019): the
/// review round items of its tickets first, then its resolved members, the
/// release item last and only once every other archive went through, so
/// that a failure keeps the cycle in view and the next tick retries it. An
/// open item is never archived, nor an item with an open child (Linear
/// itself archives no issue whose sub-issues are not all closed). An item
/// already archived is skipped: a second tick writes nothing.
fn archive_resolved(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    c: &Candidate,
    release: &Item,
    members: &[Item],
) {
    if !t.tracker.archives() || release.archived {
        return;
    }
    let nits = findings::nits_title(&t.config.product.name, &c.version.to_string());
    let open = |i: &Item| !i.archived && !i.state.status.is_some_and(Status::is_resolved);
    let mut rounds: Vec<String> = Vec::new();
    let mut resolved: Vec<String> = Vec::new();
    // Whether every item of the cycle could be read and none holds the
    // release item open.
    let mut whole = true;
    for m in members.iter().filter(|m| m.key != release.key) {
        let kind = if m.parent.as_deref() == Some(release.key.as_str()) {
            Resolved::ReleaseChild
        } else if m.title == nits {
            Resolved::Nits
        } else if round_item(m, release, &[]) {
            // A review round item an older bridge labelled with the release.
            Resolved::ReviewRound
        } else {
            Resolved::Embarked
        };
        if kind == Resolved::ReleaseChild && open(m) {
            whole = false;
        }
        let children = match t.tracker.children(&m.key) {
            Ok(ch) => ch,
            Err(e) => {
                t.unreadable(report, &m.key, format!("its sub-items: {e}"));
                whole = false;
                continue;
            }
        };
        if kind != Resolved::ReleaseChild {
            for ch in &children {
                let round = matches!(
                    texts::bridge_item(&ch.description, ch.parent.as_deref()),
                    Some(BridgeItem::Angle { .. } | BridgeItem::Consolidation { .. })
                );
                if round
                    && closure::archivable(Resolved::ReviewRound, &ch.state, ch.archived)
                    && !rounds.contains(&ch.key)
                {
                    rounds.push(ch.key.clone());
                }
            }
        }
        // The resolved rounds of an open ticket are archived; a resolved
        // ticket is not while a sub-item of its own is open.
        if closure::archivable(kind, &m.state, m.archived)
            && !children.iter().any(open)
            && !resolved.contains(&m.key)
        {
            resolved.push(m.key.clone());
        }
    }
    // A round item met both as a member and under its ticket is archived
    // once.
    resolved.retain(|k| !rounds.contains(k));
    for key in rounds.iter().chain(&resolved) {
        if !t.in_time() {
            report.out_of_time = true;
            return;
        }
        let done = t.act(
            log,
            report,
            key,
            "archive (closure)",
            json!({"release": release.key}),
            || t.tracker.archive(key).map_err(|e| e.to_string()),
        );
        if done.is_none() {
            whole = false;
        }
    }
    if whole {
        t.act(
            log,
            report,
            &release.key,
            "archive (closure)",
            json!({"release": release.key}),
            || t.tracker.archive(&release.key).map_err(|e| e.to_string()),
        );
    }
}

/// `Ok(None)` when every fact of the closure is there; `Ok(Some(why))` when
/// one is still awaited.
fn ready_to_close(
    t: &Tick,
    shape: &Shape,
    c: &Candidate,
    published: &Release,
    newer: Option<&Candidate>,
) -> Result<Option<String>, String> {
    // No release branch still holds the candidate: the branch was merged back
    // and deleted. The next cycle's branch holds it too, and is not this
    // cycle's.
    let branches = t
        .github
        .branches(&t.repo, &shape.branch_prefix)
        .map_err(|e| e.to_string())?;
    let holds = |sha: &str, branch: &str| -> Result<bool, String> {
        let status = t
            .github
            .compare_status(&t.repo, sha, branch)
            .map_err(|e| e.to_string())?;
        Ok(matches!(
            status,
            CompareStatus::Ahead | CompareStatus::Identical
        ))
    };
    for b in branches.iter().filter(|b| shape.is_release_branch(&b.name)) {
        if !holds(&c.sha, &b.name)? {
            continue;
        }
        match newer {
            Some(n) if holds(&n.sha, &b.name)? => continue,
            _ => return Ok(Some(format!("{} still holds the candidate", b.name))),
        }
    }
    let default = t
        .github
        .default_branch(&t.repo)
        .map_err(|e| e.to_string())?;
    let status = t
        .github
        .compare_status(&t.repo, &published.tag, &default)
        .map_err(|e| e.to_string())?;
    if !matches!(status, CompareStatus::Ahead | CompareStatus::Identical) {
        return Ok(Some(format!(
            "{} is not merged back into {default}",
            published.tag
        )));
    }
    Ok(None)
}

/// Whether a member of the release is a review round item of a PR (an angle
/// or consolidation item under a ticket): by its marker, or, as the PR pass
/// recognises it, linked to an embarked PR beside its parent (`sub_items`).
/// The release item's own children are not: the closure closes them.
fn round_item(m: &Item, release: &Item, sub_items: &[String]) -> bool {
    m.parent.as_deref() != Some(release.key.as_str())
        && (sub_items.contains(&m.key)
            || matches!(
                texts::bridge_item(&m.description, m.parent.as_deref()),
                Some(BridgeItem::Angle { .. } | BridgeItem::Consolidation { .. })
            ))
}

/// The release item's children and every item carrying the release label.
fn members(t: &Tick, item: &Item, label: &str) -> TResult<Vec<Item>> {
    let mut out = super::release::release_children(t, item)?;
    for i in t.tracker.list_by_label(label)? {
        if i.key != item.key && !out.iter().any(|o| o.key == i.key) {
            out.push(i);
        }
    }
    Ok(out)
}

fn read_registry(t: &Tick, tag: &str) -> Option<Registry> {
    Some(business_rules::read_registry(t.github, &t.repo, tag))
}

#[allow(clippy::too_many_arguments)]
fn judge_member(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    m: &Item,
    release: &Item,
    registry: Option<&Registry>,
    shipped: &std::collections::BTreeMap<String, Vec<u64>>,
    result: &mut CloseResult,
) {
    let bridge_child = m.parent.as_deref() == Some(release.key.as_str())
        && texts::bridge_item(&m.description, m.parent.as_deref()).is_some();
    let member = Member {
        id: m.key.clone(),
        state: m.state.clone(),
        bridge_child,
    };
    let set = |status: Status,
               assignee: Option<Option<&crate::tracker::User>>,
               log: &mut ActionLog,
               report: &mut Report| {
        t.act(
            log,
            report,
            &m.key,
            &format!(
                "status {status}{} (closure)",
                if matches!(assignee, Some(None)) {
                    ", unassigned"
                } else {
                    ""
                }
            ),
            json!({}),
            || {
                t.tracker
                    .set_status(&m.key, status, assignee)
                    .map_err(|e| e.to_string())
            },
        )
        .is_some()
    };
    match closure::classify(&member) {
        Action::Untouched => {}
        Action::KeepFinding => result.kept_findings.push(m.key.clone()),
        Action::CloseChild => {
            if set(Status::Done, None, log, report) {
                result.children_closed.push(m.key.clone());
            }
        }
        Action::Judge => {
            // Only what still waits for the closure is judged: a ticket the
            // closure already sent back (In Progress, then maybe In Review
            // again) belongs to its implementer, not to this cycle's replays.
            if !matches!(m.state.status, Some(Status::ToDeploy | Status::PostDeploy)) {
                return;
            }
            match latest_merged_pr(t, &m.key) {
                Ok(Some(n)) if !shipped.get(&m.key).is_some_and(|prs| prs.contains(&n)) => {
                    // Its latest PR merged after this release's range: the
                    // release that ships it judges it.
                    return;
                }
                Ok(_) => {}
                Err(e) => {
                    t.unreadable(report, &m.key, format!("its merged PRs: {e}"));
                    return;
                }
            }
            let gate = match contract_gate(t, m) {
                Ok(g) => g,
                Err(e) => {
                    t.unreadable(report, &m.key, e);
                    return;
                }
            };
            let needs = closure::needs_registry(&m.description);
            let judgement = closure::judge(
                &m.key,
                m.state.status,
                &m.description,
                &gate,
                if needs { registry } else { None },
            );
            match judgement {
                Judgement::Done => {
                    if set(Status::Done, None, log, report) {
                        result.closed.push(m.key.clone());
                    }
                }
                Judgement::PostDeploy => {
                    // The index label first, so the post-deploy pass finds
                    // every ticket this moves; then its validation sub-task
                    // (BR-ASDLC-003). A replay writes none of them again.
                    if post_deploy::watch(t, log, report, m)
                        && (m.state.status == Some(Status::PostDeploy)
                            || set(Status::PostDeploy, None, log, report))
                    {
                        result.awaiting_post_release.push(m.key.clone());
                        post_deploy::ensure_sub_task(t, log, report, m);
                    }
                }
                Judgement::Reopen => {
                    // Back In Progress and unassigned, in one write: rework is
                    // distributed by being pickable, never to someone named
                    // (BR-ASDLC-009).
                    let body = for_tracker(t, &done_gate::reopen_comment(&m.key, &gate));
                    if set(Status::InProgress, Some(None), log, report) {
                        result.incomplete.push(m.key.clone());
                        t.act(log, report, &m.key, "comment reopened", json!({}), || {
                            t.tracker
                                .comment(&m.key, &body)
                                .map(|_| ())
                                .map_err(|e| e.to_string())
                        });
                    }
                }
                Judgement::UnknownRules(missing) => {
                    let body = for_tracker(
                        t,
                        &closure::unknown_rules_comment(&missing, &t.config.product.name),
                    );
                    // Sent back like any other reopening: unassigned (BR-ASDLC-009).
                    if set(Status::InProgress, Some(None), log, report) {
                        result.incomplete.push(m.key.clone());
                        t.act(
                            log,
                            report,
                            &m.key,
                            "comment unknown rules",
                            json!({}),
                            || {
                                t.tracker
                                    .comment(&m.key, &body)
                                    .map(|_| ())
                                    .map_err(|e| e.to_string())
                            },
                        );
                    }
                }
                Judgement::RulesUnverified(why) => {
                    result.rules_unverified.push((m.key.clone(), why))
                }
            }
        }
    }
}

/// The Done gate of a ticket: its boxes and its duplicates' (REQ-MERGE-064).
pub(super) fn contract_gate(t: &Tick, m: &Item) -> Result<done_gate::DoneGate, String> {
    let node = |key: &str, description: String| -> Result<DuplicateNode, String> {
        let r = t.tracker.relations(key).map_err(|e| e.to_string())?;
        Ok(DuplicateNode {
            description,
            duplicates: r.duplicates.into_iter().map(|d| d.other).collect(),
            relations_cut: r.cut,
        })
    };
    let first = node(&m.key, m.description.clone())?;
    let mut read = |key: &str| -> Result<DuplicateNode, String> {
        let item = t.tracker.get(key).map_err(|e| e.to_string())?;
        node(key, item.description)
    };
    let (members, unreadable) = done_gate::duplicate_closure(&m.key, first, &mut read);
    Ok(done_gate::evaluate(&members, unreadable))
}

/// Build, splice and send the release note once (spec 05 §12). Returns
/// whether it has been sent (now or before).
fn release_note(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    shape: &Shape,
    c: &Candidate,
    item: &Item,
    published: &Release,
) -> bool {
    let marker = texts::marker("note-sent", &[("tag", &published.tag)]);
    match tracker_said(t, &item.key, &marker) {
        Ok(true) => return true,
        Ok(false) => {}
        Err(e) => {
            t.unreadable(report, &item.key, e);
            return false;
        }
    }
    let block = match note_block(t, shape, c) {
        Ok(b) => b,
        Err(e) => {
            t.unreadable(report, &item.key, format!("the release note: {e}"));
            return false;
        }
    };
    describe_note(t, log, report, item, &block);
    let head = note::published_head(
        &shape.product,
        &c.version.to_string(),
        Some(&published.url),
        &item.url,
        None,
    );
    let text = format!("{head}\n\n{block}");
    let (said, action) = match t.notify(
        log,
        report,
        &item.key,
        "release-note",
        json!({"tag": published.tag}),
        &text,
    ) {
        None => return false,
        Some(Notice::Sent) => (
            format!("The release note of {} was sent.", published.tag),
            "comment note sent",
        ),
        // Nothing to send is done, said once here, never retried.
        Some(Notice::NoChannel) => (
            format!(
                "The release note of {} has nothing to send: no notification channel takes `release-note`. This item carries it.",
                published.tag
            ),
            "comment note sent: no channel takes release-note",
        ),
    };
    let body = for_tracker(t, &format!("{said}\n\n{marker}"));
    t.act(log, report, &item.key, action, json!({}), || {
        t.tracker
            .comment(&item.key, &body)
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .is_some()
}

/// The release note of a candidate (spec 05 §12): every PR of its range, the
/// release's own bookkeeping apart. A ticket is named once with its PRs, by
/// its `## Release note` snippet (its Goal when it has none), a ticket with
/// nothing to deploy included; a PR no ticket describes is announced by its
/// own `## Release note` section, or its title. `Err` when the range or a
/// ticket could not be read: the note is never guessed.
pub(super) fn note_block(t: &Tick, shape: &Shape, c: &Candidate) -> Result<String, String> {
    let range = embarked_items(t, shape, c)?;
    let mut tickets = Vec::new();
    for (key, prs) in range.tickets.into_iter().chain(range.delivered) {
        let ticket = t.tracker.get(&key).map_err(|e| format!("{key}: {e}"))?;
        let snippet = crate::method::contract::release_note_snippet(&ticket.description);
        tickets.push(TicketEntry {
            id: key,
            title: ticket.title,
            snippet: Some(snippet),
            goal: crate::method::contract::goal_summary(&ticket.description, 280),
            prs,
        });
    }
    Ok(note::block(&c.tag, &tickets, &range.untracked))
}

/// Splice the note's block into the release item's `## Release note`
/// section, in place of the placeholder or an earlier block; whether the
/// item now carries it. Compared as the tracker stores it: a description it
/// restyled (its own bullets, blank lines) is not rewritten at every tick.
pub(super) fn describe_note(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    item: &Item,
    block: &str,
) -> bool {
    let description = for_tracker(t, &note::splice(&item.description, block));
    if markdown::same_stored(&description, &item.description) {
        return true;
    }
    t.act(
        log,
        report,
        &item.key,
        "describe release note",
        json!({}),
        || {
            t.tracker
                .update_description(&item.key, &description)
                .map_err(|e| e.to_string())
        },
    )
    .is_some()
}

/// The number of the ticket's latest PR the bridge recorded as merged (its
/// `merged_pr: <url>` line), if any.
fn latest_merged_pr(t: &Tick, key: &str) -> TResult<Option<u64>> {
    let comments = t.tracker.comments(key)?;
    Ok(comments
        .iter()
        .filter(|c| c.by_bridge)
        .flat_map(|c| c.body.lines())
        .filter_map(|l| l.trim().strip_prefix("merged_pr: "))
        .filter_map(|url| url.trim().rsplit('/').next()?.parse::<u64>().ok())
        .next_back())
}
