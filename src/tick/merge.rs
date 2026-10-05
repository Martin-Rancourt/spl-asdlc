//! The PR-review pass (`docs/spec/04-pr-review.md`, spec 07 step 3): per
//! open PR, in order, stopping at the first thing that holds it: exclusions,
//! the linked ticket, the review reconciliation, CI, the ticket's contract,
//! the business-rule guard, the branch, then entering In Review, changes
//! requested, and the merge, at most one per repository per tick.
//!
//! Idempotence reads the observable state first (statuses, children, the
//! PR); a hidden marker is used only for what nobody can observe, a message
//! said once, and it is read only in comments the bridge itself wrote.

use std::collections::BTreeMap;

use serde_json::json;

use crate::action_log::ActionLog;
use crate::github::{PrState, PullRequest, RequiredChecks};
use crate::method::business_rules::{self, Registry};
use crate::method::checks::{self, CheckState};
use crate::method::contract;
use crate::method::review::{self, Approval, BranchState, ReviewFacts, SensitivePaths};
use crate::method::texts::{self, BridgeItem};
use crate::method::vocabulary::Status;
use crate::tracker::{Item, NewItem};

use super::link::{self, Linked};
use super::{Report, Tick};

/// What the base of a PR says, read once per base and tick.
struct Base {
    required: Result<RequiredChecks, String>,
    sensitive: SensitivePaths,
}

/// A PR the pass found ready to merge.
pub struct Candidate {
    pub pr: PullRequest,
    pub ticket: Item,
    pub approved_by: String,
}

/// Run the pass; returns the PRs ready to merge, in the order they would be
/// merged (oldest PR first). The caller merges at most one per tick.
pub fn pass(t: &Tick, log: &mut ActionLog, report: &mut Report) -> Vec<Candidate> {
    let mut prs = match t.github.open_pulls(&t.repo, None) {
        Ok(p) => p,
        Err(e) => {
            t.unreadable(report, "open PRs", e);
            return Vec::new();
        }
    };
    prs.sort_by_key(|p| p.number);
    let mut bases: BTreeMap<String, Base> = BTreeMap::new();
    let mut candidates = Vec::new();
    for pr in prs {
        if !t.in_time() {
            report.out_of_time = true;
            break;
        }
        let base = bases
            .entry(pr.base_ref.clone())
            .or_insert_with(|| read_base(t, &pr.base_ref));
        if let Some(c) = one(t, log, report, pr, base) {
            candidates.push(c);
        }
    }
    candidates
}

fn read_base(t: &Tick, base: &str) -> Base {
    let required = t
        .github
        .required_checks(&t.repo, base)
        .map_err(|e| e.to_string());
    let sensitive = match &t.config.review.sensitive_paths {
        None => SensitivePaths::NotDeclared,
        // Read at the base: a PR cannot weaken the list that judges it.
        Some(path) => match t.github.file(&t.repo, path, base) {
            Ok(text) => match review::parse_sensitive_paths(&text) {
                Ok(paths) => SensitivePaths::Paths(paths),
                Err(e) => SensitivePaths::Invalid(format!("{path} is invalid: {e}")),
            },
            Err(e) => SensitivePaths::Invalid(format!("{path} could not be read at {base}: {e}")),
        },
    };
    Base {
        required,
        sensitive,
    }
}

/// One PR, up to the point where something holds it.
fn one(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr: PullRequest,
    base: &Base,
) -> Option<Candidate> {
    let object = format!("pr#{}", pr.number);
    let release = &t.config.release;
    if review::excluded_from_merge_pass(&pr, &release.branch_prefix, &release.release_pr_label)
        .is_some()
    {
        return None;
    }

    // ---- the ticket ----
    let ticket = match link::ticket_item(&pr, t.tracker, t.config.tracker.link_fallback) {
        Err(e) => {
            t.unreadable(report, &object, format!("the linked ticket: {e}"));
            return None;
        }
        Ok(Linked::None) => {
            say_once(
                t,
                log,
                report,
                &pr,
                "no-ticket",
                &[("pr", &pr.url)],
                &format!(
                    "No ticket is linked to this PR, so the bridge does not review or merge it. \
                     Link the ticket in the tracker (its PR link{}).",
                    match t.config.tracker.link_fallback {
                        crate::config::LinkFallback::BodyUrl =>
                            ", or the ticket's URL in this PR's description",
                        crate::config::LinkFallback::None => "",
                    }
                ),
            );
            return None;
        }
        Ok(Linked::Several(keys)) => {
            say_once(
                t,
                log,
                report,
                &pr,
                "several-tickets",
                &[("pr", &pr.url)],
                &format!(
                    "Several tickets are linked to this PR ({}); a PR implements exactly one ticket \
                     (`docs/spec/01-ticket-contract.md` §7). The bridge waits until one link remains.",
                    keys.join(", ")
                ),
            );
            return None;
        }
        Ok(Linked::One(item)) => *item,
    };
    if ticket.archived {
        return None;
    }
    let object = format!("pr#{} {}", pr.number, ticket.key);

    // ---- reviews submitted close their angle sub-items ----
    if ticket.state.status == Some(Status::InReview) {
        close_reviewed_sub_items(t, log, report, &pr, &ticket);
    }

    // ---- a PR appears: Ready → In Progress (spec 03) ----
    let mut status = ticket.state.status;
    if status == Some(Status::Ready) {
        let done = t.act(
            log,
            report,
            &object,
            "status In Progress",
            json!({"pr": pr.url, "from": "Ready"}),
            || {
                t.tracker
                    .set_status(&ticket.key, Status::InProgress, None)
                    .map_err(|e| e.to_string())
            },
        );
        if done.is_none() {
            return None;
        }
        status = Some(Status::InProgress);
    }
    if !matches!(status, Some(Status::InProgress | Status::InReview)) {
        // Triage, Backlog, a ticket past the merge, or a state outside the
        // table: the bridge never drags a ticket forward nor back here.
        return None;
    }
    if pr.draft {
        return None;
    }

    // ---- CI ----
    let required = match &base.required {
        Ok(r) => r,
        Err(e) => {
            t.unreadable(
                report,
                &object,
                format!("the required checks of {}: {e}", pr.base_ref),
            );
            return None;
        }
    };
    let ci = checks::pr_state(&pr, &required.contexts);
    let branch = review::branch_state(&pr);
    if ci != CheckState::Green {
        if ci == CheckState::None && branch == BranchState::Conflicting {
            conflict(t, log, report, &pr);
        }
        return None;
    }

    // ---- the ticket's contract gates the review (spec 04 §2) ----
    let missing = contract::missing_from_contract(&ticket.description);
    if !missing.is_empty() {
        let digest = texts::digest(&missing.join("\n"));
        say_once(
            t,
            log,
            report,
            &pr,
            "contract-incomplete",
            &[("ticket", &ticket.key), ("missing", &digest)],
            &format!(
                "The contract of {} is incomplete, so this PR is not reviewed yet \
                 (`docs/spec/01-ticket-contract.md`):\n\n{}\n\n`spl-asdlc ticket check {}` says the same.",
                ticket.key,
                missing
                    .iter()
                    .map(|m| format!("- {m}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
                ticket.key
            ),
        );
        return None;
    }
    if contract::declares_nothing(&ticket.description) {
        say_once(
            t,
            log,
            report,
            &pr,
            "nothing-declared",
            &[
                ("ticket", &ticket.key),
                ("head", texts::short(&pr.head_sha)),
            ],
            &format!(
                "{} checks none of its boxes at this head (`{}`): tick the acceptance criteria this PR \
                 meets before it is reviewed.",
                ticket.key,
                texts::short(&pr.head_sha)
            ),
        );
        return None;
    }

    // ---- the diff and the business-rule guard ----
    let files = t.github.pull_files(&t.repo, pr.number).ok();
    if business_rules::needs_registry(&ticket.description, files.as_deref()) {
        let registry = read_registry(t, &pr.head_sha);
        let gate = business_rules::evaluate(
            &ticket.description,
            &registry,
            files.as_deref(),
            &release.registry_path,
        );
        if !gate.ok() {
            say_once(
                t,
                log,
                report,
                &pr,
                "business-rule",
                &[
                    ("ticket", &ticket.key),
                    ("head", texts::short(&pr.head_sha)),
                ],
                &business_rules::refusal(&gate, &ticket.key, &release.registry_path),
            );
            return None;
        }
    }

    // ---- the branch ----
    let must_be_current = t.config.review.require_up_to_date || required.strict;
    match branch {
        BranchState::Conflicting => {
            conflict(t, log, report, &pr);
            return None;
        }
        BranchState::Behind if must_be_current => {
            t.act(
                log,
                report,
                &object,
                "update branch",
                json!({"head": pr.head_sha, "base": pr.base_ref}),
                || {
                    t.github
                        .update_branch(&t.repo, pr.number, &pr.head_sha)
                        .map_err(|e| e.to_string())
                },
            );
            return None;
        }
        _ => {}
    }

    // ---- the review round ----
    let skipped = checks::skipped_required(&pr.checks, &required.contexts);
    let plan = review::plan(
        &ReviewFacts {
            labels: &pr.labels,
            changed_paths: files.as_deref(),
            additions: pr.additions,
            deletions: pr.deletions,
            sensitive: &base.sensitive,
            skipped_required: skipped.as_deref(),
        },
        &t.config.review,
    );
    if let SensitivePaths::Invalid(why) = &base.sensitive {
        notify_once(
            t,
            log,
            report,
            &pr,
            "stuck-pr",
            "config-invalid",
            &format!(
                "{}: configuration invalid for this repository — {why}. The PR waits.",
                pr.url
            ),
        );
    }
    let changes_at_head = changes_requested_at_head(&pr, &t.config.review.coordinators);
    if status == Some(Status::InProgress) {
        if changes_at_head {
            return None;
        }
        enter_review(t, log, report, &pr, &ticket, &plan);
        return None;
    }

    // ---- In Review: changes requested, or the merge ----
    let approval = review::pr_approval(&pr, &t.config.review.coordinators);
    if let Approval::ChangesRequested { by } = &approval {
        if changes_at_head {
            t.act(
                log,
                report,
                &object,
                "status In Progress",
                json!({"changes_requested_by": by, "head": pr.head_sha}),
                || {
                    t.tracker
                        .set_status(&ticket.key, Status::InProgress, None)
                        .map_err(|e| e.to_string())
                },
            );
        }
        return None;
    }
    let Approval::Qualified { by } = approval else {
        return None;
    };
    if !plan.holds.is_empty() {
        return None;
    }
    if review::unresolved_threads(&pr) != Some(0) {
        return None;
    }
    if branch != BranchState::Current {
        return None;
    }
    if review::needs_human_approval(&t.config.review, &plan) {
        // Whose approval counts as the human one is the owner's to say
        // (SID-868); until then the PR waits and a human is asked once.
        ask_human(t, log, report, &pr);
        return None;
    }
    Some(Candidate {
        pr,
        ticket,
        approved_by: by,
    })
}

/// Merge the first candidate: pinned to its head, then the branch deleted
/// and the ticket To Deploy (spec 04 §6). Returns whether a merge happened.
pub fn merge(t: &Tick, log: &mut ActionLog, report: &mut Report, c: &Candidate) -> bool {
    let pr = &c.pr;
    let object = format!("pr#{} {}", pr.number, c.ticket.key);
    let method: crate::github::MergeMethod = t.config.review.merge_method.into();
    let merged = t.act(
        log,
        report,
        &object,
        &format!("merge ({})", method.as_str()),
        json!({"head": pr.head_sha, "approved_by": c.approved_by, "base": pr.base_ref}),
        || match t
            .github
            .merge_pull(&t.repo, pr.number, &pr.head_sha, method)
        {
            Ok(sha) => Ok(sha),
            // The answer may be lost, not the merge: GitHub says what happened.
            Err(e) => match t.github.pull(&t.repo, pr.number) {
                Ok(p) if p.state == PrState::Merged => Ok(p.merge_commit_sha.unwrap_or_default()),
                _ => Err(e.to_string()),
            },
        },
    );
    if merged.is_none() {
        return false;
    }
    if !pr.head_ref.is_empty() {
        t.act(
            log,
            report,
            &object,
            &format!("delete branch {}", pr.head_ref),
            json!({}),
            || {
                t.github
                    .delete_branch(&t.repo, &pr.head_ref)
                    .map_err(|e| e.to_string())
            },
        );
    }
    to_deploy(t, log, report, pr, &c.ticket, None);
    true
}

/// A merged PR's ticket goes To Deploy, with the merge comment, once
/// (REQ-MERGE-062 as spec 04 §6 corrects it: never Done, never backwards).
pub fn to_deploy(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr: &PullRequest,
    ticket: &Item,
    merged_by: Option<&str>,
) {
    let object = format!("pr#{} {}", pr.number, ticket.key);
    if matches!(
        ticket.state.status,
        Some(Status::InProgress | Status::InReview | Status::Ready)
    ) {
        t.act(
            log,
            report,
            &object,
            "status To Deploy",
            json!({"merged_pr": pr.url}),
            || {
                t.tracker
                    .set_status(&ticket.key, Status::ToDeploy, None)
                    .map_err(|e| e.to_string())
            },
        );
    }
    match merge_recorded(t, &ticket.key, &pr.url) {
        Ok(true) => {}
        Ok(false) => {
            let body = texts::merged_comment(&pr.url, &pr.base_ref, merged_by, "To Deploy");
            t.act(log, report, &object, "comment merged", json!({}), || {
                t.tracker
                    .comment(&ticket.key, &body)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            });
        }
        Err(e) => t.unreadable(report, &object, format!("the ticket's comments: {e}")),
    }
}

/// Whether the bridge already recorded this merge on the ticket.
pub fn merge_recorded(t: &Tick, key: &str, pr_url: &str) -> Result<bool, String> {
    let line = format!("merged_pr: {pr_url}");
    Ok(t.tracker
        .comments(key)
        .map_err(|e| e.to_string())?
        .iter()
        .any(|c| c.by_bridge && c.body.lines().any(|l| l.trim() == line)))
}

fn read_registry(t: &Tick, head: &str) -> Registry {
    let path = &t.config.release.registry_path;
    match t.github.file(&t.repo, path, head) {
        Ok(text) => Registry::Ids(business_rules::registry_ids(&text)),
        Err(e) if e.is_not_found() => match t.github.commit(&t.repo, head) {
            // The file is absent at a revision that exists.
            Ok(_) => Registry::Absent,
            Err(e) => Registry::Unreadable(e.to_string()),
        },
        Err(e) => Registry::Unreadable(e.to_string()),
    }
}

/// A standing request for changes from a coordinator on the current head:
/// the ticket stays with its implementer until the head moves.
fn changes_requested_at_head(pr: &PullRequest, coordinators: &[String]) -> bool {
    let mut latest: BTreeMap<String, &crate::github::Review> = BTreeMap::new();
    for r in &pr.reviews {
        let Some(login) = r.author.as_deref() else {
            continue;
        };
        let state = r.state.to_ascii_uppercase();
        if state == "APPROVED" || state == "CHANGES_REQUESTED" {
            latest.insert(login.to_ascii_lowercase(), r);
        }
    }
    latest.iter().any(|(login, r)| {
        coordinators.iter().any(|c| c.eq_ignore_ascii_case(login))
            && r.state.eq_ignore_ascii_case("CHANGES_REQUESTED")
            && r.commit_sha.as_deref() == Some(pr.head_sha.as_str())
    })
}

/// Open the review round: the angle sub-items and the consolidation item
/// (each linked to the PR), the round comment, then In Review last, so that
/// a tick cut short re-creates nothing (spec 04 §3).
fn enter_review(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr: &PullRequest,
    ticket: &Item,
    plan: &review::ReviewPlan,
) {
    let object = format!("pr#{} {}", pr.number, ticket.key);
    let scope = texts::round_scope(&pr.url, &pr.head_sha);
    let children = match t.tracker.children(&ticket.key) {
        Ok(c) => c,
        Err(e) => {
            t.unreadable(report, &object, format!("the ticket's children: {e}"));
            return;
        }
    };
    let existing = |want: &BridgeItem| {
        children
            .iter()
            .find(|c| {
                texts::bridge_item(&c.description, c.parent.as_deref()).as_ref() == Some(want)
            })
            .map(|c| c.key.clone())
    };
    let mut items: Vec<(String, String)> = Vec::new();
    for angle in &plan.angles {
        let want = BridgeItem::Angle {
            scope: scope.clone(),
            angle: angle.clone(),
        };
        let key = match existing(&want) {
            Some(k) => k,
            None => {
                let (title, body) = texts::review_angle_item(
                    &ticket.key,
                    &pr.url,
                    &pr.head_sha,
                    angle,
                    &plan.reasons,
                );
                let assignee = hint(t, &t.config.review.angles, angle);
                match create(
                    t,
                    log,
                    report,
                    &object,
                    &title,
                    &body,
                    &ticket.key,
                    assignee,
                    &pr.url,
                ) {
                    Some(k) => k,
                    None => return,
                }
            }
        };
        items.push((angle.clone(), key));
    }
    let want = BridgeItem::Consolidation {
        scope: scope.clone(),
    };
    if existing(&want).is_none() {
        let (title, body) = texts::consolidation_item(
            &scope,
            &format!("{} ({})", ticket.key, texts::short(&pr.head_sha)),
            "docs/agents/consolidate.md",
        );
        if create(
            t,
            log,
            report,
            &object,
            &title,
            &body,
            &ticket.key,
            None,
            &pr.url,
        )
        .is_none()
        {
            return;
        }
    }
    let round = texts::review_round_comment(
        &pr.url,
        &pr.head_sha,
        &items,
        &plan.reasons,
        texts::short(&pr.head_sha),
    );
    let marker = texts::marker(
        "review-round",
        &[("pr", &pr.url), ("head", texts::short(&pr.head_sha))],
    );
    match tracker_said(t, &ticket.key, &marker) {
        Ok(true) => {}
        Ok(false) => {
            let body = for_tracker(t, &round);
            if t.act(
                log,
                report,
                &object,
                "comment review round",
                json!({"angles": plan.angles}),
                || {
                    t.tracker
                        .comment(&ticket.key, &body)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                },
            )
            .is_none()
            {
                return;
            }
        }
        Err(e) => {
            t.unreadable(report, &object, format!("the ticket's comments: {e}"));
            return;
        }
    }
    t.act(
        log,
        report,
        &object,
        "status In Review",
        json!({"head": pr.head_sha, "angles": plan.angles, "reasons": plan.reasons}),
        || {
            t.tracker
                .set_status(&ticket.key, Status::InReview, None)
                .map_err(|e| e.to_string())
        },
    );
}

#[allow(clippy::too_many_arguments)]
fn create(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    object: &str,
    title: &str,
    body: &str,
    parent: &str,
    assignee: Option<crate::tracker::User>,
    pr_url: &str,
) -> Option<String> {
    let body = for_tracker(t, body);
    let item = t.act(
        log,
        report,
        object,
        &format!("create \"{title}\""),
        json!({"parent": parent}),
        || {
            t.tracker
                .create_item(&NewItem {
                    title: title.to_string(),
                    description: body.clone(),
                    labels: Vec::new(),
                    status: Status::Ready,
                    priority: None,
                    assignee: assignee.clone(),
                    parent: Some(parent.to_string()),
                })
                .map_err(|e| e.to_string())
        },
    )?;
    t.act(
        log,
        report,
        &item.key,
        &format!("link {pr_url}"),
        json!({}),
        || {
            t.tracker
                .link_pr(&item.key, pr_url, title)
                .map_err(|e| e.to_string())
        },
    );
    Some(item.key)
}

/// The assignee hint of an angle, when the tracker knows the person.
fn hint(t: &Tick, angles: &[crate::config::Angle], angle: &str) -> Option<crate::tracker::User> {
    let name = angles.iter().find(|a| a.key == angle)?.assignee.as_ref()?;
    t.tracker.user(name).ok()
}

/// A submitted review closes the angle sub-item it names (spec 04 §4).
fn close_reviewed_sub_items(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr: &PullRequest,
    ticket: &Item,
) {
    let children = match t.tracker.children(&ticket.key) {
        Ok(c) => c,
        Err(e) => {
            t.unreadable(report, &ticket.key, format!("the ticket's children: {e}"));
            return;
        }
    };
    let prefix = format!("{}@", pr.url);
    for child in children {
        let Some(BridgeItem::Angle { scope, .. }) =
            texts::bridge_item(&child.description, child.parent.as_deref())
        else {
            continue;
        };
        if !scope.starts_with(&prefix) || child.state.is_resolved() {
            continue;
        }
        let reviewed = pr
            .reviews
            .iter()
            .any(|r| !r.state.eq_ignore_ascii_case("PENDING") && names(&r.body, &child.key));
        if reviewed {
            t.act(
                log,
                report,
                &child.key,
                "status Done (review submitted)",
                json!({"pr": pr.url}),
                || {
                    t.tracker
                        .set_status(&child.key, Status::Done, None)
                        .map_err(|e| e.to_string())
                },
            );
        }
    }
}

/// Whether a text names an item key as a whole word.
pub fn names(text: &str, key: &str) -> bool {
    text.match_indices(key).any(|(i, _)| {
        let before = text[..i].chars().next_back();
        let after = text[i + key.len()..].chars().next();
        let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_');
        !word(before) && !word(after)
    })
}

fn conflict(t: &Tick, log: &mut ActionLog, report: &mut Report, pr: &PullRequest) {
    say_once(
        t,
        log,
        report,
        pr,
        "conflict",
        &[("head", texts::short(&pr.head_sha))],
        &format!(
            "This PR conflicts with `{}` at `{}`: the implementer resolves the conflict on the branch; \
             the bridge cannot.",
            pr.base_ref,
            texts::short(&pr.head_sha)
        ),
    );
}

fn ask_human(t: &Tick, log: &mut ActionLog, report: &mut Report, pr: &PullRequest) {
    let who = t.config.review.human_reviewer.clone();
    let text = match &who {
        Some(login) => format!(
            "This PR needs a human approval besides the coordinator's (`human_approval = {:?}`); @{login} is asked to look.",
            t.config.review.human_approval
        ),
        None => "This PR needs a human approval besides the coordinator's; no `human_reviewer` is configured, so a reviewer must be assigned by hand.".to_string(),
    };
    let asked = say_once(
        t,
        log,
        report,
        pr,
        "human-approval",
        &[("head", texts::short(&pr.head_sha))],
        &text,
    );
    if let (true, Some(login)) = (asked, who) {
        t.act(
            log,
            report,
            &format!("pr#{}", pr.number),
            &format!("request review from {login}"),
            json!({}),
            || {
                t.github
                    .request_reviewers(&t.repo, pr.number, std::slice::from_ref(&login))
                    .map_err(|e| e.to_string())
            },
        );
    }
}

/// Post `text` on the PR once per marker; returns whether it was posted now.
pub fn say_once(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr: &PullRequest,
    kind: &str,
    fields: &[(&str, &str)],
    text: &str,
) -> bool {
    let object = format!("pr#{}", pr.number);
    let marker = texts::marker(kind, fields);
    match pr_said(t, pr.number, &marker) {
        Ok(true) => false,
        Ok(false) => {
            let body = format!("{text}\n\n{marker}");
            t.act(
                log,
                report,
                &object,
                &format!("comment {kind}"),
                json!({}),
                || {
                    t.github
                        .comment(&t.repo, pr.number, &body)
                        .map(|_| ())
                        .map_err(|e| e.to_string())
                },
            )
            .is_some()
        }
        Err(e) => {
            t.unreadable(report, &object, format!("the PR's comments: {e}"));
            false
        }
    }
}

/// Send a notice once per PR and cause; the marker is written on the PR
/// after the notice went out (a failed send is retried).
fn notify_once(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr: &PullRequest,
    event: &str,
    cause: &str,
    text: &str,
) {
    let object = format!("pr#{}", pr.number);
    let marker = texts::marker("notice", &[("event", event), ("cause", cause)]);
    match pr_said(t, pr.number, &marker) {
        Ok(true) => {}
        Ok(false) => {
            if t.act(
                log,
                report,
                &object,
                &format!("notify {event}"),
                json!({"cause": cause}),
                || t.signal.signal(event, text).map(|_| ()),
            )
            .is_some()
            {
                let body = format!("{text}\n\n{marker}");
                t.act(
                    log,
                    report,
                    &object,
                    &format!("comment {event}"),
                    json!({}),
                    || {
                        t.github
                            .comment(&t.repo, pr.number, &body)
                            .map(|_| ())
                            .map_err(|e| e.to_string())
                    },
                );
            }
        }
        Err(e) => t.unreadable(report, &object, format!("the PR's comments: {e}")),
    }
}

/// Whether one of the bridge's own comments on the PR carries the marker.
fn pr_said(t: &Tick, number: u64, marker: &str) -> Result<bool, String> {
    let comments = t
        .github
        .issue_comments(&t.repo, number)
        .map_err(|e| e.to_string())?;
    Ok(comments.iter().any(|c| {
        c.author
            .as_ref()
            .is_some_and(|a| a.login.eq_ignore_ascii_case(&t.github_login))
            && texts::has_marker(&c.body, marker)
    }))
}

/// Whether one of the bridge's own comments on the item carries the marker.
pub fn tracker_said(t: &Tick, key: &str, marker: &str) -> Result<bool, String> {
    Ok(t.tracker
        .comments(key)
        .map_err(|e| e.to_string())?
        .iter()
        .any(|c| c.by_bridge && texts::has_marker(&c.body, marker)))
}

/// A text as the tracker keeps it: markers visible where HTML comments
/// would be dropped.
pub fn for_tracker(t: &Tick, text: &str) -> String {
    if t.tracker.keeps_html_comments() {
        text.to_string()
    } else {
        texts::visible_markers(text)
    }
}
