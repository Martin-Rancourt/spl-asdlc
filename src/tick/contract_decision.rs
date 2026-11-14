//! The contract gate of a PR (`docs/reference/04-pr-review.md` §2,
//! `docs/reference/01-ticket-contract.md`): a PR whose ticket's contract is
//! incomplete is held, and its owner is asked (ASDLC-G8M2).
//!
//! On a tracker with decision requests ([`Tracker::holds_decisions`]), the
//! bridge raises one on the ticket, once per PR and per set of missing
//! elements (its marker carries both), so that the owner hears about it in
//! the decisions queue. It reads the answer itself, because the queue only
//! records it:
//! - « No new rule. » (or « None ») while `## Business rules` is missing or
//!   empty: the bridge writes that declaration into the description, and the
//!   PR goes on at the same tick when nothing else is missing;
//! - any other answer is left to people: the bridge says once that the PR
//!   waits for the contract to be completed.
//!
//! It withdraws its open request of a PR once the contract is complete, or
//! once the missing elements change (a new request then asks for the new
//! set). It never writes a goal or acceptance criteria. On a tracker without
//! decision requests, it says it in a comment on the ticket, as before
//! (SID-1222).
//!
//! [`Tracker::holds_decisions`]: crate::tracker::Tracker::holds_decisions

use serde_json::json;

use crate::action_log::ActionLog;
use crate::github::PullRequest;
use crate::method::contract;
use crate::method::texts;
use crate::tracker::{Decision, Item};

use super::merge::{for_tracker, say_once, say_once_on_item};
use super::{Report, Tick};

/// The marker kind of the bridge's decision request at the contract gate.
pub const KIND: &str = "contract-decision";

/// The marker of the request for this PR and this set of missing elements.
pub fn marker(pr: u64, digest: &str) -> String {
    texts::marker(KIND, &[("pr", &pr.to_string()), ("missing", digest)])
}

/// Whether a request is the bridge's contract request for this PR, whatever
/// set of missing elements it names.
fn of_pr(question: &str, pr: u64) -> bool {
    question.contains(&format!("spl-asdlc:{KIND} pr={pr} "))
}

fn listed(missing: &[String]) -> String {
    missing
        .iter()
        .map(|m| format!("- {m}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The question the bridge asks the owner: the PR, each missing element, and
/// how to answer.
pub fn question(pr: &PullRequest, key: &str, missing: &[String], rules_missing: bool) -> String {
    let only_rules = rules_missing && missing.len() == 1;
    let mut text = format!(
        "PR #{} ({}) is held: the contract of this ticket is incomplete \
         (`docs/reference/01-ticket-contract.md`):\n\n{}\n\n",
        pr.number,
        pr.url,
        listed(missing)
    );
    if rules_missing {
        text.push_str(&format!(
            "If this ticket introduces no new rule, answer « No new rule. »{}: the bridge then writes \
             `## Business rules` with `No new rule.` into the description, and the PR goes on. ",
            if only_rules { " (the default)" } else { "" }
        ));
    }
    if !only_rules {
        text.push_str(
            "The bridge never writes a goal or acceptance criteria: complete them in the description. ",
        );
    }
    text.push_str(&format!(
        "Any other answer is left to people; once the contract is complete, the PR goes on at the next \
         tick and this request is withdrawn. `spl-asdlc ticket check {key}` says the same."
    ));
    text
}

/// The PR's latest request, withdrawn ones included, when it still stands
/// and names these gaps: a request withdrawn or for other gaps is never
/// read again, so a gap that comes back is asked anew.
fn standing<'a>(ours: &'a [Decision], asked: &str) -> Option<&'a Decision> {
    ours.last()
        .filter(|d| !d.withdrawn && texts::has_marker(&d.question, asked))
}

/// Gate the PR on its ticket's contract: `Some(ticket)` to go on (its
/// description completed when the owner answered « No new rule. »), `None`
/// to hold it.
pub fn gate(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr: &PullRequest,
    mut ticket: Item,
) -> Option<Item> {
    let decides = t.tracker.holds_decisions();
    let mut missing = contract::missing_from_contract(&ticket.description);
    if missing.is_empty() && !decides {
        return Some(ticket);
    }
    let object = format!("pr#{}", pr.number);
    let ours: Vec<Decision> = if decides {
        match t.tracker.decisions(&ticket.key) {
            Ok(all) => all
                .into_iter()
                .filter(|d| d.by_bridge && of_pr(&d.question, pr.number))
                .collect(),
            Err(e) => {
                t.unreadable(
                    report,
                    &object,
                    format!("the decision requests of {}: {e}", ticket.key),
                );
                // A complete contract does not need them: only a stale
                // request stays open until a later tick withdraws it.
                return missing.is_empty().then_some(ticket);
            }
        }
    } else {
        Vec::new()
    };

    // The owner answered « No new rule. » to the PR's latest request, and it
    // names the current gaps: the bridge writes the declaration, and nothing
    // else. Once a later request is raised, that answer is never applied
    // again.
    let asked = marker(pr.number, &texts::digest(&missing.join("\n")));
    let answer = standing(&ours, &asked).and_then(|d| d.answer.as_deref());
    if !missing.is_empty() && answer.is_some_and(contract::says_no_new_rule) {
        let text = contract::with_no_new_rule(&ticket.description);
        if text != ticket.description {
            t.act(
                log,
                report,
                &ticket.key,
                "declare no new rule",
                json!({"pr": pr.number}),
                || {
                    t.tracker
                        .update_description(&ticket.key, &text)
                        .map_err(|e| e.to_string())
                },
            )?;
            ticket.description = text;
            missing = contract::missing_from_contract(&ticket.description);
        }
    }

    // A request that no longer matches the contract's gaps is withdrawn.
    let digest = texts::digest(&missing.join("\n"));
    let current = marker(pr.number, &digest);
    for d in ours.iter().filter(|d| {
        d.is_open() && (missing.is_empty() || !texts::has_marker(&d.question, &current))
    }) {
        t.act(
            log,
            report,
            &ticket.key,
            "withdraw contract decision",
            json!({"pr": pr.number, "request": d.id}),
            || {
                t.tracker
                    .withdraw_decision(&ticket.key, &d.id)
                    .map_err(|e| e.to_string())
            },
        );
    }
    if missing.is_empty() {
        return Some(ticket);
    }

    t.wait(
        report,
        pr,
        format!("the contract of {} is incomplete", ticket.key),
    );
    say_once(
        t,
        log,
        report,
        pr,
        "contract-incomplete",
        &[("ticket", &ticket.key), ("missing", &digest)],
        &format!(
            "The contract of {} is incomplete, so this PR is not reviewed yet \
             (`docs/reference/01-ticket-contract.md`):\n\n{}\n\n`spl-asdlc ticket check {}` says the same.",
            ticket.key,
            listed(&missing),
            ticket.key
        ),
    );
    if decides {
        let rules_missing = contract::lacks_business_rules(&ticket.description);
        match standing(&ours, &current) {
            None => {
                let q = for_tracker(
                    t,
                    &format!(
                        "{}\n\n{current}",
                        question(pr, &ticket.key, &missing, rules_missing)
                    ),
                );
                let raised = t.act(
                    log,
                    report,
                    &ticket.key,
                    "request contract decision",
                    json!({"pr": pr.number}),
                    || {
                        t.tracker
                            .request_decision(&ticket.key, &q)
                            .map_err(|e| e.to_string())
                    },
                );
                if raised.is_some() {
                    return None;
                }
                // The request failed: the ticket still hears of it below.
            }
            Some(d) if d.is_open() => return None,
            Some(d) => {
                say_once_on_item(
                    t,
                    log,
                    report,
                    &ticket.key,
                    "contract-answered",
                    &[("request", &d.id)],
                    &format!(
                        "PR #{} still waits for the contract of this ticket to be completed:\n\n{}\n\n\
                         The bridge acts on one answer only, « No new rule. » for a missing `## Business rules` \
                         section; it leaves this one to people. Complete the description: the PR goes on at the \
                         next tick.",
                        pr.number,
                        listed(&missing)
                    ),
                );
                return None;
            }
        }
    }
    // Said on the ticket too, where people and agents look: a PR held for
    // its contract never waits unseen (SID-1222).
    say_once_on_item(
        t,
        log,
        report,
        &ticket.key,
        "contract-incomplete",
        &[("pr", &pr.number.to_string()), ("missing", &digest)],
        &format!(
            "PR #{} is held: the contract of this ticket is incomplete \
             (`docs/reference/01-ticket-contract.md`):\n\n{}\n\nComplete it here; the PR goes on \
             at the next tick. `spl-asdlc ticket check {}` says the same.",
            pr.number,
            listed(&missing),
            ticket.key
        ),
    );
    None
}
