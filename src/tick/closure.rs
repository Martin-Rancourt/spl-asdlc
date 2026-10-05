//! The closure of a cycle (`docs/spec/05-release-cycle.md` §11-12),
//! triggered by facts, never assumed: the release published, every
//! embarked component deployed (multi-component), the release branch merged
//! back. Then each member of the release is judged; the release note is sent;
//! the release item moves to Done only once every ticket that cites a rule
//! was judged and the note went out (a failed send is a failed closure step,
//! retried, never dropped).

use serde_json::json;

use crate::action_log::ActionLog;
use crate::github::{CompareStatus, Release};
use crate::method::business_rules::{self, Registry};
use crate::method::done_gate::{self, DuplicateNode};
use crate::method::release::closure::{self, Action, CloseResult, Judgement, Member};
use crate::method::release::note::{self, TicketEntry};
use crate::method::release::prs;
use crate::method::release::shape::Shape;
use crate::method::texts;
use crate::method::vocabulary::{self, Status};
use crate::tracker::{Item, TResult};

use super::merge::{for_tracker, tracker_said};
use super::release::{Candidate, embarked};
use super::{Report, Tick};

pub fn close(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    shape: &Shape,
    c: &Candidate,
    item: &Item,
    published: &Release,
) {
    let object = item.key.clone();
    match ready_to_close(t, shape, c, published) {
        Ok(None) => {}
        Ok(Some(waiting)) => {
            // A fact the closure waits for: nothing written, nothing to say
            // each tick (the stagnation of a published release is a host's
            // job log matter until the owner settles REQ-REL-082).
            let _ = waiting;
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
    let registry = read_registry(t, &published.tag);
    let mut result = CloseResult::default();
    for m in &members {
        judge_member(t, log, report, m, item, registry.as_ref(), &mut result);
    }
    let marker = texts::marker("closure", &[("tag", &published.tag)]);
    match tracker_said(t, &item.key, &marker) {
        Ok(false) => {
            let body = for_tracker(t, &format!("{}\n\n{marker}", closure::comment(&result)));
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
        t.act(
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
) -> Result<Option<String>, String> {
    // No release branch still holds the candidate: the branch was merged back
    // and deleted.
    let branches = t
        .github
        .branches(&t.repo, &t.config.release.branch_prefix)
        .map_err(|e| e.to_string())?;
    for b in branches.iter().filter(|b| shape.is_release_branch(&b.name)) {
        let cmp = t
            .github
            .compare(&t.repo, &c.sha, &b.name)
            .map_err(|e| e.to_string())?;
        if matches!(cmp.status, CompareStatus::Ahead | CompareStatus::Identical) {
            return Ok(Some(format!("{} still holds the candidate", b.name)));
        }
    }
    let default = t
        .github
        .default_branch(&t.repo)
        .map_err(|e| e.to_string())?;
    let cmp = t
        .github
        .compare(&t.repo, &published.tag, &default)
        .map_err(|e| e.to_string())?;
    if !matches!(cmp.status, CompareStatus::Ahead | CompareStatus::Identical) {
        return Ok(Some(format!(
            "{} is not merged back into {default}",
            published.tag
        )));
    }
    if shape.multi_component() {
        let merged = t
            .github
            .merged_pulls(&t.repo, Some(&default), None, 500)
            .map_err(|e| e.to_string())?;
        let ledger =
            prs::ledger(&merged, shape).ok_or("the deployment ledger could not be read")?;
        let undeployed: Vec<String> = ledger
            .iter()
            .filter(|e| e.version == c.version.to_string() && !e.deployed)
            .map(|e| e.component.clone())
            .collect();
        if !undeployed.is_empty() {
            return Ok(Some(format!("not deployed yet: {}", undeployed.join(", "))));
        }
    }
    Ok(None)
}

/// The release item's children and every item carrying the release label.
fn members(t: &Tick, item: &Item, label: &str) -> TResult<Vec<Item>> {
    let mut out = t.tracker.children(&item.key)?;
    for i in t.tracker.list_by_label(label)? {
        if i.key != item.key && !out.iter().any(|o| o.key == i.key) {
            out.push(i);
        }
    }
    Ok(out)
}

fn read_registry(t: &Tick, tag: &str) -> Option<Registry> {
    let path = &t.config.release.registry_path;
    Some(match t.github.file(&t.repo, path, tag) {
        Ok(text) => Registry::Ids(business_rules::registry_ids(&text)),
        Err(e) if e.is_not_found() => match t.github.commit(&t.repo, tag) {
            Ok(_) => Registry::Absent,
            Err(e) => Registry::Unreadable(e.to_string()),
        },
        Err(e) => Registry::Unreadable(e.to_string()),
    })
}

#[allow(clippy::too_many_arguments)]
fn judge_member(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    m: &Item,
    release: &Item,
    registry: Option<&Registry>,
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
            &format!("status {status} (closure)"),
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
                    if m.state.status == Some(Status::PostDeploy)
                        || set(Status::PostDeploy, None, log, report)
                    {
                        result.awaiting_post_release.push(m.key.clone());
                    }
                }
                Judgement::Reopen => {
                    // The implementer to hand it back to is the owner's to
                    // settle (SID-868); the current assignee keeps it.
                    let who = m.assignee.as_ref().map(|u| u.name.clone());
                    let body = for_tracker(
                        t,
                        &done_gate::reopen_comment(
                            &m.key,
                            &gate,
                            who.as_deref()
                                .ok_or("nobody is assigned: assign the implementer by hand"),
                        ),
                    );
                    if set(Status::InProgress, None, log, report) {
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
                        &closure::unknown_rules_comment(
                            &missing,
                            &t.config.release.registry_path,
                            &t.config.product.name,
                        ),
                    );
                    if set(Status::InProgress, None, log, report) {
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
fn contract_gate(t: &Tick, m: &Item) -> Result<done_gate::DoneGate, String> {
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
    let shipped = match embarked(t, shape, c) {
        Ok(k) => k,
        Err(e) => {
            t.unreadable(report, &item.key, format!("the shipped tickets: {e}"));
            return false;
        }
    };
    let mut tickets = Vec::new();
    for (key, prs) in shipped {
        let ticket = match t.tracker.get(&key) {
            Ok(i) => i,
            Err(e) => {
                t.unreadable(report, &key, e);
                return false;
            }
        };
        let snippet = crate::method::contract::release_note_snippet(&ticket.description);
        tickets.push(TicketEntry {
            id: key,
            title: ticket.title,
            snippet: (!snippet.is_empty()).then_some(snippet),
            goal: crate::method::contract::goal_summary(&ticket.description, 280),
            prs,
        });
    }
    let block = note::block(&c.tag, &tickets, &[], Some(&published.body));
    let description = note::splice(&item.description, &block);
    if description != item.description {
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
        );
    }
    let head = note::published_head(
        &shape.product,
        &c.version.to_string(),
        Some(&published.url),
        &item.url,
        None,
    );
    let text = format!("{head}\n\n{block}");
    let sent = t.act(
        log,
        report,
        &item.key,
        "notify release-note",
        json!({"tag": published.tag}),
        || t.signal.signal("release-note", &text).map(|_| ()),
    );
    if sent.is_none() {
        return false;
    }
    let body = for_tracker(
        t,
        &format!(
            "The release note of {} was sent.\n\n{marker}",
            published.tag
        ),
    );
    t.act(
        log,
        report,
        &item.key,
        "comment note sent",
        json!({}),
        || {
            t.tracker
                .comment(&item.key, &body)
                .map(|_| ())
                .map_err(|e| e.to_string())
        },
    )
    .is_some()
}
