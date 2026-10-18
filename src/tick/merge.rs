//! The PR-review pass (`docs/reference/04-pr-review.md`, spec 07 step 3): per
//! open PR, in order, stopping at the first thing that holds it: exclusions
//! (a draft PR is out of the flow), the linked ticket, the review
//! reconciliation, the PR opened (its ticket In Review), CI, the ticket's
//! contract, the
//! business-rule guard, the branch, then the review round, changes
//! requested, and the merge, at most one per repository per tick.
//!
//! A ticket may have several PRs (spec 04 §10): the pass reads the ticket of
//! every open PR first, each PR has its own review round, and the state of
//! the ticket follows the state rules of `method::ticket_state` whenever one
//! of its PRs resolves.
//!
//! Idempotence reads the observable state first (statuses, children, the
//! PR); a hidden marker is used only for what nobody can observe, a message
//! said once, and it is read only in comments the bridge itself wrote.

use std::collections::BTreeMap;

use serde_json::json;

use crate::action_log::ActionLog;
use crate::github::{PrState, PullRequest, RequiredChecks};
use crate::method::business_rules;
use crate::method::checks::{self, CheckState};
use crate::method::contract;
use crate::method::release::findings;
use crate::method::release::prs::trusted_author;
use crate::method::review::{self, Approval, BranchState, ReviewFacts};
use crate::method::texts::{self, BridgeItem, protocol};
use crate::method::ticket_state::{self, Facts, OpenPr};
use crate::method::vocabulary::{Status, TICKETLESS_LABEL};
use crate::tracker::{Item, NewItem, RelationKind};

use super::identity;
use super::link::{self, Linked};
use super::method_texts;
use super::{Report, Tick};

/// What the base of a PR says, read once per base and tick.
struct Base {
    required: Result<RequiredChecks, String>,
}

/// A PR the pass found ready to merge.
pub struct Candidate {
    pub pr: PullRequest,
    pub ticket: Item,
    /// The coordinator whose approval at the head lets it merge; `None` for a
    /// PR with no angle, which merges with no round and no approval (spec 04
    /// §11).
    pub approved_by: Option<String>,
    /// The Apps' requests for changes that approval supersedes, dismissed
    /// right before the merge (spec 04 §6).
    pub dismiss: Vec<Dismissal>,
}

/// A review the bridge dismisses right before a merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dismissal {
    pub id: u64,
    pub login: String,
}

/// Why a person's standing request for changes holds the PR, if one
/// stands besides the coordinators' own: the bridge never dismisses it.
pub fn held_by_a_person(pr: &PullRequest, coordinators: &[String]) -> Option<String> {
    let people: Vec<&str> = review::standing_changes_requested(&pr.reviews, coordinators)
        .into_iter()
        .filter(|s| !s.app)
        .map(|s| s.login)
        .collect();
    match people.as_slice() {
        [] => None,
        [one] => Some(format!(
            "changes requested by {one} (a person; the bridge never dismisses it)"
        )),
        many => Some(format!(
            "changes requested by {} (people; the bridge never dismisses them)",
            many.join(", ")
        )),
    }
}

/// The requests for changes standing besides the coordinators' on a PR a
/// coordinator approved at its head (spec 04 §6): an App's is superseded by
/// that approval and returned, to be dismissed right before the merge; a
/// person's holds the PR, and `Err` says why it waits.
pub fn superseded(pr: &PullRequest, coordinators: &[String]) -> Result<Vec<Dismissal>, String> {
    if let Some(why) = held_by_a_person(pr, coordinators) {
        return Err(why);
    }
    let mut out = Vec::new();
    for s in review::standing_changes_requested(&pr.reviews, coordinators) {
        for r in s.reviews {
            let Some(id) = r.id else {
                return Err(format!(
                    "changes requested by {} (an App, but GitHub gave no id to dismiss its review)",
                    s.login
                ));
            };
            out.push(Dismissal {
                id,
                login: s.login.to_string(),
            });
        }
    }
    Ok(out)
}

/// Dismiss the reviews a coordinator's approval at the head supersedes
/// (spec 04 §6), each logged, its message naming the approval and the head.
/// Whether every one was dismissed: the merge waits otherwise, and the next
/// tick dismisses what still stands.
pub fn dismiss(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    object: &str,
    pr: &PullRequest,
    approved_by: &str,
    dismissals: &[Dismissal],
) -> bool {
    let message = format!(
        "Dismissed by the bridge before the merge: {approved_by} approved this PR at its head \
         `{}`, which supersedes this request for changes from an App (`docs/reference/04-pr-review.md` §6).",
        pr.head_sha
    );
    dismissals.iter().all(|d| {
        t.act(
            log,
            report,
            object,
            &format!("dismiss review of {}", d.login),
            json!({"review": d.id, "author": d.login, "approved_by": approved_by, "head": pr.head_sha}),
            || {
                t.github
                    .dismiss_review(&t.repo, pr.number, d.id, &message)
                    .map_err(|e| e.to_string())
            },
        )
        .is_some()
    })
}

/// What the pass found: the PRs ready to merge, and the open PRs of each
/// ticket, which the state rules read when another PR of the ticket
/// resolves (spec 04 §10).
#[derive(Default)]
pub struct Pass {
    /// In the order they would be merged (oldest PR first). The caller
    /// merges at most one per tick.
    pub candidates: Vec<Candidate>,
    pub open: OpenPrs,
}

/// The open PRs of the repository, by the ticket they are linked to.
#[derive(Debug, Default)]
pub struct OpenPrs {
    by_ticket: BTreeMap<String, Vec<Open>>,
    /// Every open PR had its ticket read: "no other PR" can be said.
    pub known: bool,
}

#[derive(Debug, Clone)]
struct Open {
    number: u64,
    sent_back: bool,
}

impl OpenPrs {
    fn add(&mut self, ticket: &str, pr: &PullRequest, coordinators: &[String]) {
        self.by_ticket
            .entry(ticket.to_string())
            .or_default()
            .push(Open {
                number: pr.number,
                sent_back: review::changes_requested_at_head(pr, coordinators).is_some(),
            });
    }

    fn of(&self, ticket: &str, except: Option<u64>) -> Vec<&Open> {
        self.by_ticket
            .get(ticket)
            .map(|l| l.iter().filter(|o| Some(o.number) != except).collect())
            .unwrap_or_default()
    }
}

/// The open PRs of a ticket but `except`, as the state rules read them: a
/// PR not sent back is in review, round or not (spec 03). A draft PR is not
/// among them: it is out of the flow and counts for nothing.
pub fn open_prs_of(open: &OpenPrs, ticket: &str, except: Option<u64>) -> Vec<OpenPr> {
    open.of(ticket, except)
        .into_iter()
        .map(|o| OpenPr {
            number: o.number,
            sent_back: o.sent_back,
        })
        .collect()
}

/// Whether a ticket has nothing to deploy, such as documentation: it
/// carries the configured `tracker.no_deploy_label` (labels compare without
/// regard to case, as the trackers show them). Its merge delivers it (Done
/// rather than To Deploy, `docs/method.md`, "Merge") and no release embarks
/// it (spec 04 §10). Every project has a release cycle: this is the one way
/// past it.
pub fn nothing_to_deploy(t: &Tick, ticket: &Item) -> bool {
    let label = &t.config.tracker.no_deploy_label;
    ticket.labels.iter().any(|l| l.eq_ignore_ascii_case(label))
}

/// Whether the PR has a review round on the ticket that was not canceled:
/// angle sub-items or a consolidation item scoped to it, open or done.
fn live_round(children: &[Item], pr_url: &str) -> bool {
    children
        .iter()
        .any(|c| c.state.status != Some(Status::Canceled) && round_of(c, pr_url))
}

/// Whether an item is an angle sub-item or the consolidation item of a
/// review round of this PR (any head).
pub fn round_of(item: &Item, pr_url: &str) -> bool {
    let prefix = format!("{pr_url}@");
    match texts::bridge_item(&item.description, item.parent.as_deref()) {
        Some(BridgeItem::Angle { scope, .. } | BridgeItem::Consolidation { scope }) => {
            scope.starts_with(&prefix)
        }
        _ => false,
    }
}

/// Run the pass. The tickets of every open PR are read first: the state of a
/// ticket depends on all its PRs.
pub fn pass(t: &Tick, log: &mut ActionLog, report: &mut Report) -> Pass {
    let mut prs = match t.github.open_pulls(&t.repo, None) {
        Ok(p) => p,
        Err(e) => {
            t.unreadable(report, "open PRs", e);
            return Pass::default();
        }
    };
    prs.sort_by_key(|p| p.number);
    let branch_prefix = t.config.release_machinery();
    let coordinators = &t.config.review.coordinators;
    let mut open = OpenPrs {
        known: true,
        ..OpenPrs::default()
    };
    let mut linked = Vec::new();
    for pr in prs {
        if !t.in_time() {
            report.out_of_time = true;
            return Pass::default();
        }
        if review::excluded_from_merge_pass(&pr, branch_prefix).is_some() {
            continue;
        }
        // A draft PR is out of the flow (`docs/method.md`): nothing is read
        // of it nor written for it, and it counts for nothing among its
        // ticket's PRs; a round it had stays as it is and goes on once the PR
        // is marked ready.
        if pr.draft {
            t.wait(
                report,
                &pr,
                "it is a draft, out of the flow until it is marked ready",
            );
            continue;
        }
        let link = link::ticket_item(&pr, t.tracker, t.config.tracker.link_fallback);
        match &link {
            Ok(Linked::One(item)) => open.add(&item.key, &pr, coordinators),
            // A PR that several tickets claim is live work on each of them.
            Ok(Linked::Several(keys)) => {
                for key in keys {
                    open.add(key, &pr, coordinators);
                }
            }
            Ok(Linked::None) => {}
            Err(_) => open.known = false,
        }
        linked.push((pr, link));
    }
    let mut bases: BTreeMap<String, Base> = BTreeMap::new();
    let mut candidates = Vec::new();
    for (pr, link) in linked {
        if !t.in_time() {
            report.out_of_time = true;
            break;
        }
        let base = bases
            .entry(pr.base_ref.clone())
            .or_insert_with(|| read_base(t, &pr.base_ref));
        if let Some(c) = one(t, log, report, pr, link, base, &open) {
            candidates.push(c);
        }
    }
    Pass { candidates, open }
}

fn read_base(t: &Tick, base: &str) -> Base {
    let required = t
        .github
        .required_checks(&t.repo, base)
        .map_err(|e| e.to_string());
    Base { required }
}

/// One PR, up to the point where something holds it. The exclusions were
/// applied and its ticket resolved by the pass.
fn one(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr: PullRequest,
    link: crate::tracker::TResult<Linked>,
    base: &Base,
    open: &OpenPrs,
) -> Option<Candidate> {
    let object = format!("pr#{}", pr.number);

    // ---- the ticket ----
    let ticket = match link {
        Err(e) => {
            t.unreadable(report, &object, format!("the linked ticket: {e}"));
            return None;
        }
        Ok(Linked::None) if trusted_author(&pr, &t.config.review.ticketless_authors) => {
            open_pr_ticket(t, log, report, &pr);
            return None;
        }
        Ok(Linked::None) => {
            t.wait(report, &pr, "no ticket is linked to it");
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
            t.wait(
                report,
                &pr,
                format!("several tickets are linked to it ({})", keys.join(", ")),
            );
            say_once(
                t,
                log,
                report,
                &pr,
                "several-tickets",
                &[("pr", &pr.url)],
                &format!(
                    "Several tickets are linked to this PR ({}); a PR implements exactly one ticket \
                     (`docs/reference/01-ticket-contract.md` §7). The bridge waits until one link remains.",
                    keys.join(", ")
                ),
            );
            return None;
        }
        Ok(Linked::One(item)) => *item,
    };
    if ticket.archived {
        t.wait(
            report,
            &pr,
            format!("its ticket {} is archived", ticket.key),
        );
        return None;
    }
    let object = format!("pr#{} {}", pr.number, ticket.key);

    // ---- an open PR on a ticket past the merge (REQ-MERGE-028) ----
    if ticket
        .state
        .status
        .is_some_and(Status::is_terminal_for_open_pr)
    {
        reopen_for_open_pr(t, log, report, &pr, &ticket, open);
        return None;
    }

    // ---- reviews submitted close their angle sub-items ----
    let mut has_round = true;
    let mut open_angles = Vec::new();
    let mut read_children = None;
    if ticket.state.status == Some(Status::InReview) {
        match t.tracker.children(&ticket.key) {
            Ok(children) => {
                has_round = children.iter().any(|c| round_of(c, &pr.url));
                let closed = close_reviewed_sub_items(t, log, report, &pr, &children);
                open_angles = angles_open(&children, &pr, &closed);
                repair_rounds(t, log, report, &pr, &children);
                read_children = Some(children);
            }
            Err(e) => {
                t.unreadable(report, &ticket.key, format!("the ticket's children: {e}"));
                return None;
            }
        }
    }

    // ---- a PR appears (spec 03) ----
    // In Progress is the implementer's, agent or person, who starts the
    // ticket: the bridge never writes it as a backup. A PR opened on a
    // ticket not marked started (Backlog, Ready) goes straight to In Review
    // below.
    let mut status = ticket.state.status;
    if !matches!(
        status,
        Some(Status::Backlog | Status::Ready | Status::InProgress | Status::InReview)
    ) {
        // Triage, a ticket past the merge, or a state outside the table:
        // the bridge never drags a ticket forward nor back here.
        let state = status
            .map(Status::name)
            .unwrap_or("in a state outside the table");
        t.wait(
            report,
            &pr,
            format!(
                "its ticket {} is {state}, not Backlog, Ready, In Progress or In Review",
                ticket.key
            ),
        );
        return None;
    }
    let head = texts::short(&pr.head_sha);
    let changes_at_head = review::changes_requested_at_head(&pr, &t.config.review.coordinators);

    // ---- the PR is opened: In Review at once, before CI (spec 03) ----
    // A PR with a round of its own that is In Progress was sent back (spec
    // 04 §5): its next round, on a new head, brings the ticket back.
    if status != Some(Status::InReview) && changes_at_head.is_none() {
        let children = match read_children.take() {
            Some(c) => c,
            None => match t.tracker.children(&ticket.key) {
                Ok(c) => c,
                Err(e) => {
                    t.unreadable(report, &object, format!("the ticket's children: {e}"));
                    return None;
                }
            },
        };
        // A write that fails does not hold the PR: its round, once CI is
        // green, writes In Review again.
        if !live_round(&children, &pr.url)
            && t.act(
                log,
                report,
                &object,
                "status In Review",
                json!({"pr": pr.url, "head": pr.head_sha, "opened": true}),
                || {
                    t.tracker
                        .set_status(&ticket.key, Status::InReview, None)
                        .map_err(|e| e.to_string())
                },
            )
            .is_some()
        {
            status = Some(Status::InReview);
            has_round = false;
        }
        read_children = Some(children);
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
        let head = texts::short(&pr.head_sha);
        let missing = checks::missing_required(&pr.checks, &required.contexts);
        let why = match ci {
            CheckState::Red => match checks::first_failure(&pr.checks) {
                Some(c) => format!("checks are red at {head} (`{}`)", c.name),
                None => format!("checks are red at {head}"),
            },
            CheckState::Pending if !missing.is_empty() => format!(
                "required checks have not reported at {head}: {}",
                missing.join(", ")
            ),
            CheckState::Pending if checks::running(&pr.checks) => {
                format!("checks are still running at {head}")
            }
            // Only a success counts: a skipped required check, a draft
            // run's for one, waits for a run that succeeds.
            CheckState::Pending => match checks::skipped_required(&pr.checks, &required.contexts) {
                Some(name) => format!(
                    "the required check `{name}` was skipped at {head}: only a success counts"
                ),
                None if checks::none_succeeded(&pr.checks) => {
                    format!("no check succeeded at {head}")
                }
                None => format!("checks are still running at {head}"),
            },
            CheckState::None => format!("no check reported at {head}"),
            _ => format!("the checks of {head} could not be read whole"),
        };
        t.wait(report, &pr, why);
        if ci == CheckState::None && branch == BranchState::Conflicting {
            conflict(t, log, report, &pr);
        }
        return None;
    }

    // ---- the ticket's contract gates the review (spec 04 §2) ----
    let missing = contract::missing_from_contract(&ticket.description);
    if !missing.is_empty() {
        t.wait(
            report,
            &pr,
            format!("the contract of {} is incomplete", ticket.key),
        );
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
                 (`docs/reference/01-ticket-contract.md`):\n\n{}\n\n`spl-asdlc ticket check {}` says the same.",
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
        t.wait(
            report,
            &pr,
            format!("{} checks none of its boxes", ticket.key),
        );
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
    let files = match t.github.pull_files(&t.repo, pr.number) {
        Ok(f) => Some(f),
        Err(e) => {
            // The plan holds the merge on an unread diff; the host is told.
            t.unreadable(report, &object, format!("the PR's files: {e}"));
            None
        }
    };
    if business_rules::needs_registry(&ticket.description, files.as_deref()) {
        let registry = business_rules::read_registry(t.github, &t.repo, &pr.head_sha);
        let gate = business_rules::evaluate(&ticket.description, &registry, files.as_deref());
        if !gate.ok() {
            t.wait(report, &pr, "the business-rule guard refuses it");
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
                &business_rules::refusal(&gate, &ticket.key),
            );
            return None;
        }
    }

    // ---- the branch ----
    let must_be_current = t.config.review.require_up_to_date || required.strict;
    match branch {
        BranchState::Conflicting => {
            t.wait(report, &pr, format!("it conflicts with {}", pr.base_ref));
            conflict(t, log, report, &pr);
            return None;
        }
        BranchState::Behind if must_be_current => {
            t.wait(
                report,
                &pr,
                format!(
                    "it is behind {}, which it must be up to date with: its branch is updated",
                    pr.base_ref
                ),
            );
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
            skipped_required: skipped.as_deref(),
        },
        &t.config.review,
    );
    let changes_wait =
        |by: &str| format!("{by} requested changes at {head}: the implementer's turn");
    // No angle under the current configuration (spec 04 §11): no round is
    // opened, and a round opened under an older one no longer applies. Its
    // open items are canceled first, and the PR merges on its CI.
    if plan.unreviewed() {
        if let Some(by) = &changes_at_head {
            t.wait(report, &pr, changes_wait(by));
            send_back(t, log, report, &pr, &ticket, status, open, by);
            return None;
        }
        let children = match read_children {
            Some(c) => c,
            None => match t.tracker.children(&ticket.key) {
                Ok(c) => c,
                Err(e) => {
                    t.unreadable(report, &object, format!("the ticket's children: {e}"));
                    return None;
                }
            },
        };
        // An item not canceled would be closed Done by the merge: the merge
        // waits for the next tick instead.
        if !drop_unasked(t, log, report, &pr, &children, &plan).1 {
            return None;
        }
        return unreviewed(t, log, report, pr, ticket, &plan, branch);
    }
    // A ticket In Review for another of its PRs: this PR gets its own round
    // (a round is scoped to its PR, BR-ASDLC-007).
    if status != Some(Status::InReview) || !has_round {
        if let Some(by) = &changes_at_head {
            t.wait(report, &pr, changes_wait(by));
            return None;
        }
        enter_review(t, log, report, &pr, &ticket, status, &plan);
        return None;
    }
    // Fewer angles than the round has: the angle sub-items the current
    // configuration no longer asks for are canceled, the rest of the round
    // stands. Only on a diff read in full: an unread diff ran no path
    // trigger, so a missing angle says nothing.
    if plan.diff_read {
        if let Some(children) = &read_children {
            let (canceled, _) = drop_unasked(t, log, report, &pr, children, &plan);
            open_angles.retain(|k| !canceled.contains(k));
        }
    }

    // ---- In Review: changes requested, or the merge ----
    let approval = review::pr_approval(&pr, &t.config.review.coordinators);
    if let Approval::ChangesRequested { by } = &approval {
        if changes_at_head.is_none() {
            t.wait(
                report,
                &pr,
                format!(
                    "{by} requested changes on an older head and has not reviewed {head} since"
                ),
            );
            return None;
        }
        t.wait(report, &pr, changes_wait(by));
        send_back(t, log, report, &pr, &ticket, status, open, by);
        return None;
    }
    let by = match approval {
        Approval::Qualified { by } => by,
        Approval::Unreadable => {
            t.unreadable(report, &object, "GitHub cut the review list");
            return None;
        }
        other => {
            t.wait(report, &pr, approval_wait(&pr, &other));
            return None;
        }
    };
    // Another reviewer's standing request for changes: a person's holds the
    // PR; an App's is superseded by the approval at the head and dismissed
    // at the merge (spec 04 §6).
    let dismiss = match superseded(&pr, &t.config.review.coordinators) {
        Ok(d) => d,
        Err(why) => {
            t.wait(report, &pr, why);
            return None;
        }
    };
    // The consolidation starts only after its angles (BR-ASDLC-018): an
    // approval given while an angle sub-item of the round is open does not
    // count, even when a dispatcher ignored the blocking relation.
    if !open_angles.is_empty() {
        say_once(
            t,
            log,
            report,
            &pr,
            "angles-open",
            &[("pr", &pr.url), ("head", texts::short(&pr.head_sha))],
            &format!(
                "Waiting for the angle reviews: {} still open, so the approval at `{}` does not \
                 count yet and the PR is not merged (BR-ASDLC-018).",
                open_angles.join(", "),
                texts::short(&pr.head_sha)
            ),
        );
        t.wait(
            report,
            &pr,
            format!(
                "its angle reviews are still open ({}): the approval at {head} does not count yet",
                open_angles.join(", ")
            ),
        );
        return None;
    }
    if !plan.holds.is_empty() {
        t.wait(report, &pr, format!("held: {}", plan.holds.join("; ")));
        return None;
    }
    match review::unresolved_threads(&pr) {
        Some(0) => {}
        Some(n) => {
            t.wait(report, &pr, format!("{n} unresolved review thread(s)"));
            return None;
        }
        None => {
            t.unreadable(report, &object, "GitHub cut the review threads");
            return None;
        }
    }
    if branch != BranchState::Current {
        t.wait(report, &pr, format!("it is behind {}", pr.base_ref));
        return None;
    }
    if review::needs_human_approval(&t.config.review) {
        t.wait(
            report,
            &pr,
            "it needs a human approval besides the coordinator's",
        );
        // Whose approval counts as the human one is the owner's to say
        // (`.spl-asdlc/open-points.md` #4); until then the PR waits and a human is asked once.
        ask_human(t, log, report, &pr, true);
        return None;
    }
    Some(Candidate {
        pr,
        ticket,
        approved_by: Some(by),
        dismiss,
    })
}

/// A coordinator requested changes at the head (spec 04 §5): the ticket In
/// Review goes back In Progress, its implementer's turn. The review rule
/// first: while another PR of the ticket is in review, the ticket stays In
/// Review (BR-ASDLC-008); when an open PR's ticket could not be read, it
/// may be one: the state waits.
#[allow(clippy::too_many_arguments)]
fn send_back(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr: &PullRequest,
    ticket: &Item,
    status: Option<Status>,
    open: &OpenPrs,
    by: &str,
) {
    let another = open_prs_of(open, &ticket.key, Some(pr.number))
        .iter()
        .any(OpenPr::in_review);
    if status != Some(Status::InReview) || another || !open.known {
        return;
    }
    t.act(
        log,
        report,
        &format!("pr#{} {}", pr.number, ticket.key),
        "status In Progress",
        json!({"changes_requested_by": by, "head": pr.head_sha}),
        || {
            t.tracker
                .set_status(&ticket.key, Status::InProgress, None)
                .map_err(|e| e.to_string())
        },
    );
}

/// A PR with no angle (spec 04 §11): no round, no sub-item, no approval (a
/// round opened under an earlier configuration was canceled first). It
/// merges on what the pass already checked (CI green, the contract complete
/// with a box ticked, the business-rule guard), once nothing holds it, no
/// thread is open and its branch is current; its ticket went In Review when
/// the PR was opened, as any. A coordinator's standing request for changes
/// still holds it, and a human approval, when the project asks for one, is
/// then the only approval required.
fn unreviewed(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr: PullRequest,
    ticket: Item,
    plan: &review::ReviewPlan,
    branch: BranchState,
) -> Option<Candidate> {
    let object = format!("pr#{} {}", pr.number, ticket.key);
    let coordinators = &t.config.review.coordinators;
    if !coordinators.is_empty() {
        match review::pr_approval(&pr, coordinators) {
            Approval::ChangesRequested { .. } => return None,
            Approval::Unreadable => {
                t.unreadable(report, &object, "GitHub cut the review list");
                return None;
            }
            _ => {}
        }
    }
    // No approval supersedes anything here: a person's request for changes
    // holds the PR, an App's is left to GitHub.
    if let Some(why) = held_by_a_person(&pr, coordinators) {
        t.wait(report, &pr, why);
        return None;
    }
    if !plan.holds.is_empty() {
        return None;
    }
    match review::unresolved_threads(&pr) {
        Some(0) => {}
        Some(_) => return None,
        None => {
            t.unreadable(report, &object, "GitHub cut the review threads");
            return None;
        }
    }
    if branch != BranchState::Current {
        return None;
    }
    if review::needs_human_approval(&t.config.review) {
        ask_human(t, log, report, &pr, false);
        return None;
    }
    Some(Candidate {
        pr,
        ticket,
        approved_by: None,
        dismiss: Vec::new(),
    })
}

/// Cancel the open items of this PR's review rounds (any head) that the
/// current configuration no longer asks for (spec 04 §11): an angle sub-item
/// whose angle the plan does not give the PR, and, when the plan gives it no
/// angle at all, the consolidation item too. Each gets a comment saying so,
/// once (a marker the bridge reads in its own comments), then Canceled; a
/// replay finds the item resolved and writes nothing. Returns the items
/// canceled now and whether every one was.
fn drop_unasked(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr: &PullRequest,
    children: &[Item],
    plan: &review::ReviewPlan,
) -> (Vec<String>, bool) {
    let prefix = format!("{}@", pr.url);
    let asked = if plan.angles.is_empty() {
        "no angle".to_string()
    } else {
        format!(
            "the angles {}",
            plan.angles
                .iter()
                .map(|a| format!("`{a}`"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let mut canceled = Vec::new();
    let mut all = true;
    for child in children.iter().filter(|c| !c.state.is_resolved()) {
        let what = match texts::bridge_item(&child.description, child.parent.as_deref()) {
            Some(BridgeItem::Angle { scope, angle })
                if scope.starts_with(&prefix) && !plan.angles.contains(&angle) =>
            {
                format!("the `{angle}` review")
            }
            Some(BridgeItem::Consolidation { scope })
                if scope.starts_with(&prefix) && plan.angles.is_empty() =>
            {
                "the consolidation".to_string()
            }
            _ => continue,
        };
        let object = child.key.as_str();
        let marker = texts::marker("not-asked", &[("pr", &pr.url)]);
        match tracker_said(t, object, &marker) {
            Ok(true) => {}
            Ok(false) => {
                let body = for_tracker(
                    t,
                    &format!(
                        "The configuration no longer asks for {what} of {}: it now gives this PR {asked}. \
                         The bridge cancels this item (`docs/reference/04-pr-review.md` §11).\n\n{marker}",
                        pr.url
                    ),
                );
                let said = t.act(
                    log,
                    report,
                    object,
                    "comment not asked",
                    json!({"pr": pr.url, "angles": plan.angles}),
                    || {
                        t.tracker
                            .comment(object, &body)
                            .map(|_| ())
                            .map_err(|e| e.to_string())
                    },
                );
                if said.is_none() {
                    all = false;
                    continue;
                }
            }
            Err(e) => {
                t.unreadable(report, object, format!("its comments: {e}"));
                all = false;
                continue;
            }
        }
        let done = t.act(
            log,
            report,
            object,
            "status Canceled (no longer asked)",
            json!({"pr": pr.url, "angles": plan.angles}),
            || {
                t.tracker
                    .set_status(object, Status::Canceled, None)
                    .map_err(|e| e.to_string())
            },
        );
        if done.is_some() {
            canceled.push(child.key.clone());
        } else {
            all = false;
        }
    }
    (canceled, all)
}

/// Merge the first candidate: the Apps' requests for changes its approval
/// supersedes dismissed first, pinned to its head, then the branch deleted,
/// the PR's review round closed and the ticket where the state rules put it
/// (spec 04 §6, §10). Returns whether a merge happened.
pub fn merge(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    c: &Candidate,
    open: &OpenPrs,
) -> bool {
    let pr = &c.pr;
    let object = format!("pr#{} {}", pr.number, c.ticket.key);
    let method: crate::github::MergeMethod = t.config.review.merge_method.into();
    // A PR merged with no review says so in the action log and the dry run.
    let (action, inputs) = match &c.approved_by {
        Some(by) => (
            format!("merge ({})", method.as_str()),
            json!({"head": pr.head_sha, "approved_by": by, "base": pr.base_ref}),
        ),
        None => (
            format!("merge ({}, no PR review)", method.as_str()),
            json!({"head": pr.head_sha, "approved_by": null, "review": "none: no angle for this PR", "base": pr.base_ref}),
        ),
    };
    if let Some(by) = &c.approved_by {
        if !dismiss(t, log, report, &object, pr, by, &c.dismiss) {
            return false;
        }
    }
    let merged = t.act(log, report, &object, &action, inputs, || {
        match t
            .github
            .merge_pull(&t.repo, pr.number, &pr.head_sha, method)
        {
            Ok(sha) => Ok(sha),
            // The answer may be lost, not the merge: GitHub says what happened.
            Err(e) => match t.github.pull(&t.repo, pr.number) {
                Ok(p) if p.state == PrState::Merged => Ok(p.merge_commit_sha.unwrap_or_default()),
                _ => Err(e.to_string()),
            },
        }
    });
    if merged.is_none() {
        return false;
    }
    // A fork's branch is not the base repository's to delete.
    if !pr.head_ref.is_empty() && !pr.cross_repository {
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
    close_round(t, log, report, &c.ticket.key, &pr.url, true);
    // Read again: the pass may have moved the ticket for another of its PRs.
    match t.tracker.get(&c.ticket.key) {
        Ok(ticket) => {
            let by = texts::MergedBy::Bridge {
                approved_by: c.approved_by.as_deref(),
                up_to_date: t.config.review.require_up_to_date,
            };
            after_merge(t, log, report, pr, &ticket, &by, open)
        }
        // The sweep catches the ticket up: its merge is not recorded yet.
        Err(e) => t.unreadable(report, &object, format!("the ticket: {e}")),
    }
    true
}

/// A PR resolves, so its review round is over (BR-ASDLC-007): the angle
/// sub-items and the consolidation item of every round of this PR still
/// open go Done at a merge, Canceled at a close without merge, whatever
/// becomes of the ticket.
pub fn close_round(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    ticket: &str,
    pr_url: &str,
    merged: bool,
) -> bool {
    let children = match t.tracker.children(ticket) {
        Ok(c) => c,
        Err(e) => {
            t.unreadable(report, ticket, format!("the ticket's children: {e}"));
            return false;
        }
    };
    let (status, why) = if merged {
        (Status::Done, "PR merged")
    } else {
        (Status::Canceled, "PR closed without merge")
    };
    let mut all = true;
    for child in children
        .iter()
        .filter(|c| !c.state.is_resolved() && round_of(c, pr_url))
    {
        all &= t
            .act(
                log,
                report,
                &child.key,
                &format!("status {status} ({why})"),
                json!({"pr": pr_url}),
                || {
                    t.tracker
                        .set_status(&child.key, status, None)
                        .map_err(|e| e.to_string())
                },
            )
            .is_some();
    }
    all
}

/// An open PR on a ticket past the merge (To Deploy, Done) puts the ticket
/// back where the state rules say (REQ-MERGE-028 under BR-ASDLC-008): In
/// Review, unless a request for changes or an open box sends it In
/// Progress. The PR is not operated on this tick. Post Deploy is not
/// reopened; the bridge's own items are not tickets.
fn reopen_for_open_pr(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr: &PullRequest,
    ticket: &Item,
    open: &OpenPrs,
) {
    let object = format!("pr#{} {}", pr.number, ticket.key);
    let prs = open_prs_of(open, &ticket.key, None);
    let facts = Facts {
        open: &prs,
        open_known: open.known,
        description: &ticket.description,
        merged: true,
    };
    let Some((to, why)) = ticket_state::decide(&facts) else {
        t.unreadable(
            report,
            &object,
            "an open PR's ticket could not be read, so the state of this one waits",
        );
        return;
    };
    if ticket.state.status != Some(to) {
        t.act(
            log,
            report,
            &object,
            &format!("status {to}"),
            json!({"pr": pr.url, "from": ticket.state.status.map(|s| s.name()), "next": why.next()}),
            || {
                t.tracker
                    .set_status(&ticket.key, to, None)
                    .map_err(|e| e.to_string())
            },
        );
    }
}

/// A merged PR's ticket goes where the state rules say (spec 04 §10,
/// BR-ASDLC-008): In Review while another of its PRs is in review, In
/// Progress while a box of its contract is open, otherwise To Deploy, or, with nothing to deploy
/// ([`nothing_to_deploy`]), Done (Post Deploy while its own `## Post deploy`
/// boxes wait); never backwards past the merge (REQ-MERGE-062 as the spec
/// corrects it). The merge comment records it, once.
pub fn after_merge(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr: &PullRequest,
    ticket: &Item,
    merged_by: &texts::MergedBy,
    open: &OpenPrs,
) {
    let object = format!("pr#{} {}", pr.number, ticket.key);
    let mut state = ticket.state.status;
    let mut next = "none — its state is left alone.".to_string();
    if matches!(
        state,
        Some(Status::InProgress | Status::InReview | Status::Ready)
    ) {
        let prs = open_prs_of(open, &ticket.key, Some(pr.number));
        let facts = Facts {
            open: &prs,
            open_known: open.known,
            description: &ticket.description,
            merged: true,
        };
        // Unknown: the sweep retries at the next tick, the merge not being
        // recorded yet.
        let Some((to, why)) = ticket_state::decide(&facts) else {
            t.unreadable(
                report,
                &object,
                "an open PR's ticket could not be read, so the state after the merge waits",
            );
            return;
        };
        let (to, why) = match delivered(t, ticket, to, why) {
            Ok(d) => d,
            Err(e) => {
                t.unreadable(report, &object, format!("its contract: {e}"));
                return;
            }
        };
        // The comment records the transition: without it, the sweep
        // retries the status at the next tick.
        if state != Some(to)
            && !set_state(
                t,
                log,
                report,
                &object,
                ticket,
                to,
                json!({"merged_pr": pr.url, "next": why.next()}),
            )
        {
            return;
        }
        state = Some(to);
        next = why.next();
    }
    let state = state.map(|s| s.name()).unwrap_or("unknown");
    match merge_recorded(t, &ticket.key, &pr.url) {
        Ok(true) => {}
        Ok(false) => {
            let body = texts::merged_comment(&pr.url, &pr.base_ref, merged_by, state, &next);
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

/// For a ticket with nothing to deploy ([`nothing_to_deploy`]) merging is
/// delivering: a ticket the state rules put To Deploy goes where the Done
/// gate says ([`ticket_state::delivered`]). For any other ticket, or any
/// other state, the rules' answer stands: To Deploy waits for the release.
/// `Err` when the contract could not be read in full: nothing is written,
/// the next tick retries.
pub fn delivered(
    t: &Tick,
    ticket: &Item,
    to: Status,
    why: ticket_state::Why,
) -> Result<(Status, ticket_state::Why), String> {
    if to != Status::ToDeploy || !nothing_to_deploy(t, ticket) {
        return Ok((to, why));
    }
    let gate = super::closure::contract_gate(t, ticket)?;
    ticket_state::delivered(&ticket.key, &gate).ok_or_else(|| {
        gate.unreadable_reason
            .unwrap_or_else(|| "it could not be read in full".to_string())
    })
}

/// Write the status the state rules gave a ticket whose PR resolved; whether
/// it is in place. Post Deploy (only with nothing to deploy, at a merge)
/// gets its index label first and its validation sub-task after, as at the
/// closure (BR-ASDLC-003): a replay writes none of them again.
pub fn set_state(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    object: &str,
    ticket: &Item,
    to: Status,
    inputs: serde_json::Value,
) -> bool {
    if to == Status::PostDeploy && !super::post_deploy::watch(t, log, report, ticket) {
        return false;
    }
    let moved = t
        .act(log, report, object, &format!("status {to}"), inputs, || {
            t.tracker
                .set_status(&ticket.key, to, None)
                .map_err(|e| e.to_string())
        })
        .is_some();
    if moved && to == Status::PostDeploy {
        super::post_deploy::ensure_sub_task(t, log, report, ticket);
    }
    moved
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

/// Whether the bridge recorded the merge of any PR on the ticket.
pub fn any_merge_recorded(t: &Tick, key: &str) -> Result<bool, String> {
    Ok(t.tracker
        .comments(key)
        .map_err(|e| e.to_string())?
        .iter()
        .any(|c| c.by_bridge && c.body.lines().any(|l| l.trim().starts_with("merged_pr: "))))
}

/// Why a PR In Review waits on its approval (Missing or Stale).
fn approval_wait(pr: &PullRequest, approval: &Approval) -> String {
    let head = texts::short(&pr.head_sha);
    let from = review::approvals_at_head(&pr.reviews, &pr.head_sha);
    let from = if from.is_empty() {
        "no approval at this head".to_string()
    } else {
        format!("approvals from: {}", from.join(", "))
    };
    match approval {
        Approval::Stale { by, sha } => format!(
            "no coordinator approval at {head}: {by} approved {}, an older head ({from})",
            texts::short(sha)
        ),
        _ => format!("no coordinator approval at {head} ({from})"),
    }
}

/// Open the review round once CI is green: the angle sub-items and the
/// consolidation item (each linked to the PR), the consolidation item
/// blocked by every angle sub-item (BR-ASDLC-018), the round comment, then
/// In Review last for a ticket not In Review yet (one sent back), so that a
/// tick cut short re-creates nothing (spec 04 §3). `status` is the ticket's
/// as this tick left it.
#[allow(clippy::too_many_arguments)]
fn enter_review(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr: &PullRequest,
    ticket: &Item,
    status: Option<Status>,
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
    // A child is found by its marker or by its exact title (REQ-REL-048), or
    // by its identity comment: a marker an agent edited does not make the
    // item invisible.
    let existing =
        |want: &BridgeItem, marker: &str, title: &str, log: &mut ActionLog, report: &mut Report| {
            identity::find(t, log, report, &children, marker, |c| {
                texts::bridge_item(&c.description, c.parent.as_deref()).as_ref() == Some(want)
                    || c.title == title
            })
            .map(|found| found.into_iter().next().map(|c| c.key))
        };
    // An item found by its marker whose link to the PR failed when it was
    // created is linked again.
    let linked = match t.tracker.items_linked_to_pr(&pr.url) {
        Ok(l) => l,
        Err(e) => {
            t.unreadable(report, &object, format!("the PR's linked items: {e}"));
            return;
        }
    };
    let relink = |key: &str, title: &str, log: &mut ActionLog, report: &mut Report| {
        if !linked.iter().any(|k| k == key) {
            t.act(
                log,
                report,
                key,
                &format!("link {}", pr.url),
                json!({}),
                || {
                    t.tracker
                        .link_pr(key, &pr.url, title)
                        .map_err(|e| e.to_string())
                },
            );
        }
    };
    let mut items: Vec<(String, String)> = Vec::new();
    for angle in &plan.angles {
        let want = BridgeItem::Angle {
            scope: scope.clone(),
            angle: angle.clone(),
        };
        let notes = t
            .config
            .review
            .angles
            .iter()
            .find(|a| a.key == *angle)
            .and_then(|a| a.notes.as_deref());
        let title = texts::review_angle_title(&ticket.key, &pr.head_sha, angle);
        let marker = texts::marker("angle", &[("scope", &scope), ("angle", angle)]);
        let found = match existing(&want, &marker, &title, log, report) {
            Ok(f) => f,
            Err(e) => {
                t.unreadable(report, &object, e);
                return;
            }
        };
        let key = match found {
            Some(k) => {
                relink(&k, &format!("{angle} review"), log, report);
                k
            }
            None => {
                // The texts of the version the project is pinned on, read
                // only for an item to create (BR-ASDLC-032).
                let pinned =
                    match method_texts::texts(t, &pr.base_ref, protocol::Job::PrAngle, Some(angle))
                    {
                        Ok(p) => p,
                        Err(e) => {
                            let what = format!("the review round of {}", pr.url);
                            method_texts::unreadable(
                                t,
                                log,
                                report,
                                &ticket.key,
                                &scope,
                                &what,
                                &e,
                            );
                            return;
                        }
                    };
                let (_, body) = texts::review_angle_item(
                    &ticket.key,
                    &pr.url,
                    &pr.head_sha,
                    angle,
                    notes,
                    &plan.reasons,
                    &pinned,
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
    let title_of = format!("{} ({})", ticket.key, texts::short(&pr.head_sha));
    let title = texts::consolidation_title(&title_of);
    let marker = texts::marker("consolidation", &[("scope", &scope)]);
    let found = match existing(&want, &marker, &title, log, report) {
        Ok(f) => f,
        Err(e) => {
            t.unreadable(report, &object, e);
            return;
        }
    };
    let consolidation = match found {
        Some(k) => {
            relink(&k, "consolidation", log, report);
            k
        }
        None => {
            let of = texts::Consolidating::PullRequest;
            let pinned = match method_texts::texts(t, &pr.base_ref, of.job(), None) {
                Ok(p) => p,
                Err(e) => {
                    let what = format!("the review round of {}", pr.url);
                    method_texts::unreadable(t, log, report, &ticket.key, &scope, &what, &e);
                    return;
                }
            };
            let (_, body) = texts::consolidation_item(&scope, &title_of, of, &pinned);
            // Its coordinator, at creation only: an existing consolidation
            // item is never reassigned (someone may have taken it).
            let assignee = t
                .config
                .review
                .coordinator_assignee
                .as_ref()
                .and_then(|name| t.tracker.user(name).ok());
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
    // The consolidation item waits on every angle of its round
    // (BR-ASDLC-018); a relation not written holds the round's opening, so
    // the next tick writes it again.
    let angle_keys: Vec<String> = items.iter().map(|(_, k)| k.clone()).collect();
    if !block_consolidation(t, log, report, &consolidation, &angle_keys) {
        return;
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
    // A ticket In Review (its PR opened, or another of its PRs in review)
    // stays so.
    if status == Some(Status::InReview) {
        return;
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
    let item = identity::create(
        t,
        log,
        report,
        object,
        &format!("create \"{title}\""),
        json!({"parent": parent}),
        &NewItem {
            title: title.to_string(),
            description: for_tracker(t, body),
            labels: Vec::new(),
            status: Status::Ready,
            priority: None,
            assignee,
            parent: Some(parent.to_string()),
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

/// A PR of an App whose PRs need no ticket (`review.ticketless_authors`,
/// spec 04 §9): the bridge opens its ticket and links it; the next tick
/// takes the PR like any other. A ticket opened before whose link was lost
/// is linked again, never opened twice.
fn open_pr_ticket(t: &Tick, log: &mut ActionLog, report: &mut Report, pr: &PullRequest) {
    let object = format!("pr#{}", pr.number);
    let marker = texts::pr_ticket_marker(&pr.url);
    // Only an open ticket is the PR's: a resolved one keeps its link, so a
    // PR found here without a link has none open.
    let opened: Vec<Item> = match t
        .tracker
        .find_labelled_containing(TICKETLESS_LABEL, texts::token(&marker))
    {
        Ok(items) => items
            .into_iter()
            .filter(|i| {
                !i.archived && !i.state.is_resolved() && texts::has_marker(&i.description, &marker)
            })
            .collect(),
        Err(e) => {
            t.unreadable(
                report,
                &object,
                format!("the items labelled {TICKETLESS_LABEL}: {e}"),
            );
            return;
        }
    };
    // None found by its description: one whose description lost the marker
    // is found by its identity comment, among the open items of the label
    // whose description carries no marker of the bridge's.
    let opened = if opened.is_empty() {
        let open: Vec<Item> = match t.tracker.list_by_label(TICKETLESS_LABEL) {
            Ok(items) => items
                .into_iter()
                .filter(|i| !i.archived && !i.state.is_resolved())
                .collect(),
            Err(e) => {
                t.unreadable(
                    report,
                    &object,
                    format!("the items labelled {TICKETLESS_LABEL}: {e}"),
                );
                return;
            }
        };
        match identity::find(t, log, report, &open, &marker, |_| false) {
            Ok(found) => found,
            Err(e) => {
                t.unreadable(report, &object, e);
                return;
            }
        }
    } else {
        opened
    };
    if opened.len() > 1 {
        let keys: Vec<&str> = opened.iter().map(|i| i.key.as_str()).collect();
        t.unreadable(
            report,
            &object,
            format!(
                "several open tickets were opened for it: {}",
                keys.join(", ")
            ),
        );
        return;
    }
    let key = match opened.into_iter().next() {
        Some(item) => item.key,
        None => {
            let ensured = t.act(
                log,
                report,
                &object,
                &format!("ensure label {TICKETLESS_LABEL}"),
                json!({}),
                || {
                    t.tracker
                        .ensure_label(TICKETLESS_LABEL)
                        .map_err(|e| e.to_string())
                },
            );
            if ensured.is_none() {
                return;
            }
            let author = pr.author.as_ref().map(|a| a.login.as_str()).unwrap_or("");
            let (title, description) = texts::pr_ticket(pr.number, &pr.title, author, &pr.url);
            let created = identity::create(
                t,
                log,
                report,
                &object,
                "open ticket",
                json!({"pr": pr.url, "author": author}),
                &NewItem {
                    title,
                    description: for_tracker(t, &description),
                    labels: vec![TICKETLESS_LABEL.to_string()],
                    status: Status::Ready,
                    priority: None,
                    assignee: None,
                    parent: None,
                },
            );
            match created {
                Some(item) => item.key,
                None => return,
            }
        }
    };
    t.act(
        log,
        report,
        &key,
        &format!("link {}", pr.url),
        json!({}),
        || {
            t.tracker
                .link_pr(&key, &pr.url, &pr.title)
                .map_err(|e| e.to_string())
        },
    );
}

/// The assignee hint of an angle, when the tracker knows the person.
fn hint(t: &Tick, angles: &[crate::config::Angle], angle: &str) -> Option<crate::tracker::User> {
    let name = angles.iter().find(|a| a.key == angle)?.assignee.as_ref()?;
    t.tracker.user(name).ok()
}

/// The consolidation item of a round is blocked by every angle sub-item of
/// the round (BR-ASDLC-018; a release candidate's too, BR-ASDLC-020),
/// through the tracker's own `blocks` relation, which dispatchers respect;
/// it stops blocking when the angle item is resolved. A relation already there is not written again. `false` when a
/// relation could not be read or written: reported, retried at the next tick.
pub fn block_consolidation(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    consolidation: &str,
    angles: &[String],
) -> bool {
    if angles.is_empty() {
        return true;
    }
    let present: Vec<String> = match t.tracker.relations(consolidation) {
        Ok(r) if r.cut => {
            t.unreadable(
                report,
                consolidation,
                "the relation list is cut: whether its angles block it cannot be told",
            );
            return false;
        }
        Ok(r) => r.blocked_by.into_iter().map(|r| r.other).collect(),
        Err(e) => {
            t.unreadable(report, consolidation, format!("relations: {e}"));
            return false;
        }
    };
    let mut all = true;
    for angle in angles {
        if present.iter().any(|p| p.eq_ignore_ascii_case(angle)) {
            continue;
        }
        let mut blockers_of = |x: &str| -> Result<Vec<String>, String> {
            t.tracker
                .relations(x)
                .map(|r| r.blocked_by.into_iter().map(|r| r.other).collect())
                .map_err(|e| e.to_string())
        };
        if let Err(e) = findings::may_block(angle, consolidation, &mut blockers_of) {
            t.unreadable(
                report,
                angle,
                format!("{angle} cannot block {consolidation}: {e}"),
            );
            all = false;
            continue;
        }
        all &= t
            .act(
                log,
                report,
                angle,
                &format!("relate Blocks {consolidation}"),
                json!({"consolidation": consolidation}),
                || {
                    t.tracker
                        .relate(angle, consolidation, RelationKind::Blocks)
                        .map_err(|e| e.to_string())
                },
            )
            .is_some();
    }
    all
}

/// Every open round of this PR (its consolidation item not resolved) gets
/// the relations it lacks: a round opened before them, or one whose
/// relation failed, is repaired.
fn repair_rounds(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr: &PullRequest,
    children: &[Item],
) {
    let prefix = format!("{}@", pr.url);
    for child in children {
        let Some(BridgeItem::Consolidation { scope }) =
            texts::bridge_item(&child.description, child.parent.as_deref())
        else {
            continue;
        };
        if !scope.starts_with(&prefix) || child.state.is_resolved() {
            continue;
        }
        let angles: Vec<String> = children
            .iter()
            .filter(|c| {
                matches!(
                    texts::bridge_item(&c.description, c.parent.as_deref()),
                    Some(BridgeItem::Angle { scope: s, .. }) if s == scope
                )
            })
            .map(|c| c.key.clone())
            .collect();
        block_consolidation(t, log, report, &child.key, &angles);
    }
}

/// The angle sub-items of this PR's round still open (BR-ASDLC-018), but
/// those closed this tick: the round at the PR's head when there is one,
/// otherwise every round of the PR (fail closed).
fn angles_open(children: &[Item], pr: &PullRequest, closed: &[String]) -> Vec<String> {
    let prefix = format!("{}@", pr.url);
    let head = texts::round_scope(&pr.url, &pr.head_sha);
    let angles: Vec<(&Item, String)> = children
        .iter()
        .filter_map(
            |c| match texts::bridge_item(&c.description, c.parent.as_deref()) {
                Some(BridgeItem::Angle { scope, .. }) if scope.starts_with(&prefix) => {
                    Some((c, scope))
                }
                _ => None,
            },
        )
        .collect();
    let at_head = angles.iter().any(|(_, s)| *s == head);
    angles
        .into_iter()
        .filter(|(_, s)| !at_head || *s == head)
        .filter(|(c, _)| !c.state.is_resolved() && !closed.contains(&c.key))
        .map(|(c, _)| c.key.clone())
        .collect()
}

/// A submitted review closes the angle sub-item it names (spec 04 §4).
/// Returns the sub-items closed now.
fn close_reviewed_sub_items(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr: &PullRequest,
    children: &[Item],
) -> Vec<String> {
    let mut closed = Vec::new();
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
        if reviewed
            && t.act(
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
            )
            .is_some()
        {
            closed.push(child.key.clone());
        }
    }
    closed
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

/// Ask a human to approve, once per head. `reviewed`: the PR has a round,
/// so the human approves besides the coordinator; otherwise the human's is
/// the only approval it needs.
fn ask_human(t: &Tick, log: &mut ActionLog, report: &mut Report, pr: &PullRequest, reviewed: bool) {
    let who = t.config.review.human_reviewer.clone();
    let needs = if reviewed {
        "a human approval besides the coordinator's"
    } else {
        "a human approval, the only one it needs: it has no review angle"
    };
    let text = match &who {
        Some(login) => format!(
            "This PR needs {needs} (`human_approval = {:?}`); @{login} is asked to look.",
            t.config.review.human_approval
        ),
        None => format!(
            "This PR needs {needs}; no `human_reviewer` is configured, so a reviewer must be assigned by hand."
        ),
    };
    let fields = [("head", texts::short(&pr.head_sha))];
    match pr_said(t, pr.number, &texts::marker("human-approval", &fields)) {
        Ok(false) => {}
        Ok(true) => return,
        Err(e) => {
            t.unreadable(
                report,
                &format!("pr#{}", pr.number),
                format!("the PR's comments: {e}"),
            );
            return;
        }
    }
    // The request first, the comment (and its marker) after: a failed
    // request is retried at the next tick.
    if let Some(login) = &who {
        let asked = t.act(
            log,
            report,
            &format!("pr#{}", pr.number),
            &format!("request review from {login}"),
            json!({}),
            || {
                t.github
                    .request_reviewers(&t.repo, pr.number, std::slice::from_ref(login))
                    .map_err(|e| e.to_string())
            },
        );
        if asked.is_none() {
            return;
        }
    }
    say_once(t, log, report, pr, "human-approval", &fields, &text);
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
