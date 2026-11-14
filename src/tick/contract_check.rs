//! The contract check (ASDLC-57KZ): on the head of every PR the bridge links
//! to a ticket, a check run `spl-asdlc / contract` that says what the
//! bridge's contract gate says (`docs/reference/04-pr-review.md` §2): red
//! with the missing parts while the ticket's contract is incomplete, green
//! once it is complete. The contract lives in the tracker, which a PR's CI
//! cannot read: only the bridge can say it on GitHub.
//!
//! - One verdict (rule C9VD): the check, the hold comment, the merge
//!   decision and `spl-asdlc ticket check` all read
//!   [`contract::Verdict`]; the check's summary is its list.
//! - It informs, it never gates alone (rule M6TQ): no ruleset requires it,
//!   and the bridge's reading of a PR's checks leaves it out
//!   ([`checks::is_contract_check`]); what holds a PR is the gate.
//! - A run is written only when what it says changes: created when the head
//!   has none, updated when its outcome or its list no longer matches (the
//!   verdict's key is its `external_id`). A new head gets a new run; a
//!   ticket completed turns the run green at the next tick, without a
//!   commit.
//! - Without the permission (GitHub answers 403: the App lacks Checks: write,
//!   or the bridge writes with a personal token, which cannot create check
//!   runs), no check is published: the hold comment stays as it is, the
//!   pass is not failed, and its report says so once.
//!
//! [`checks::is_contract_check`]: crate::method::checks::is_contract_check

use serde_json::json;

use crate::action_log::ActionLog;
use crate::github::{NewCheckRun, PullRequest, ReadError, WriteError};
use crate::method::checks;
use crate::method::contract::{CONTRACT_CHECK, Verdict};
use crate::tracker::Item;

use super::{Report, Tick};

/// Where the ticket contract is defined, in the method's repository at the
/// version of this bridge.
pub fn contract_doc_url() -> String {
    format!(
        "{}/blob/v{}/docs/reference/01-ticket-contract.md",
        env!("CARGO_PKG_REPOSITORY"),
        env!("CARGO_PKG_VERSION")
    )
}

/// The start of the report's note when GitHub refuses the check: said once
/// per pass, and no other check is tried in that pass.
const DENIED: &str = "the contract check `spl-asdlc / contract` is not published";

/// The key of what a run says: the ticket and its verdict. A run whose
/// `external_id` is this key says the right thing.
pub fn key(ticket: &str, verdict: &Verdict) -> String {
    match verdict {
        Verdict::Complete => format!("{ticket}:complete"),
        _ => format!("{ticket}:incomplete:{}", verdict.digest()),
    }
}

/// The run that says `verdict` on `head` for `ticket`; `None` when there is
/// no ticket (a PR with no linked ticket gets no contract check).
pub fn output(ticket: &Item, head: &str, verdict: &Verdict) -> Option<NewCheckRun> {
    let doc = contract_doc_url();
    let named = if ticket.url.is_empty() {
        ticket.key.clone()
    } else {
        format!("[{}]({})", ticket.key, ticket.url)
    };
    let (conclusion, title, summary) = match verdict {
        Verdict::NoTicket => return None,
        Verdict::Complete => (
            "success",
            format!("The contract of {} is complete", ticket.key),
            format!(
                "The contract of {named} is complete ([the ticket contract]({doc})). \
                 `spl-asdlc ticket check {}` says the same.",
                ticket.key
            ),
        ),
        Verdict::Incomplete(missing) => (
            "failure",
            format!(
                "The contract of {} is incomplete ({} missing)",
                ticket.key,
                missing.len()
            ),
            format!(
                "The contract of {named} is incomplete, so the bridge holds this PR \
                 ([the ticket contract]({doc}), `docs/reference/01-ticket-contract.md`):\n\n{}\n\n\
                 Complete the ticket: this check turns green at the bridge's next pass, without a new \
                 commit. `spl-asdlc ticket check {}` says the same. This check informs: no ruleset \
                 requires it, the bridge's merge rule is what holds the PR.",
                verdict.listed(),
                ticket.key
            ),
        ),
    };
    Some(NewCheckRun {
        name: CONTRACT_CHECK.to_string(),
        head_sha: head.to_string(),
        conclusion: conclusion.to_string(),
        external_id: key(&ticket.key, verdict),
        title,
        summary,
        details_url: (!ticket.url.is_empty()).then(|| ticket.url.clone()),
    })
}

/// What the PR's rollup says of the bridge's run at its head.
enum Seen {
    /// No run of the check at the head.
    None,
    /// A run whose outcome is this verdict's, and that says nothing else.
    Same,
    /// A run to read before writing: another outcome, or a list that may
    /// have changed.
    Read,
}

fn seen(pr: &PullRequest, run: &NewCheckRun) -> Seen {
    if pr.truncated.iter().any(|t| t == "checks") {
        return Seen::Read;
    }
    let ours: Vec<_> = pr
        .checks
        .iter()
        .filter(|c| checks::is_contract_check(c))
        .collect();
    if ours.is_empty() {
        return Seen::None;
    }
    let same = ours.iter().all(|c| {
        c.conclusion
            .as_deref()
            .is_some_and(|x| x.eq_ignore_ascii_case(&run.conclusion))
    });
    // A green run says only that the contract is complete; a red one lists
    // what is missing, which the rollup does not show.
    if same && run.conclusion == "success" {
        Seen::Same
    } else {
        Seen::Read
    }
}

/// Publish the contract check on the PR's head, when what it says changed.
/// `fresh` reads the head's runs whatever the PR's rollup says: the
/// verdict changed during this tick (the gate completed the ticket).
pub fn publish(
    t: &Tick,
    log: &mut ActionLog,
    report: &mut Report,
    pr: &PullRequest,
    ticket: &Item,
    verdict: &Verdict,
    fresh: bool,
) {
    let Some(run) = output(ticket, &pr.head_sha, verdict) else {
        return;
    };
    if report.notes.iter().any(|n| n.starts_with(DENIED)) {
        return;
    }
    let seen = if fresh { Seen::Read } else { seen(pr, &run) };
    let existing = match seen {
        Seen::Same => return,
        Seen::None => None,
        Seen::Read => match t.github.check_runs(&t.repo, &pr.head_sha, CONTRACT_CHECK) {
            Ok(runs) => {
                let app = t.github_login.strip_suffix("[bot]");
                runs.into_iter()
                    .filter(|r| r.app.as_deref() == app)
                    .max_by_key(|r| r.id)
            }
            Err(ReadError::Unreadable {
                status: Some(403),
                detail,
                ..
            }) => {
                denied(report, Some(403), &detail);
                return;
            }
            Err(e) => {
                t.unreadable(report, &format!("pr#{}", pr.number), e);
                return;
            }
        },
    };
    if let Some(r) = &existing {
        let same_outcome = r
            .conclusion
            .as_deref()
            .is_some_and(|c| c.eq_ignore_ascii_case(&run.conclusion));
        if same_outcome && r.external_id.as_deref() == Some(run.external_id.as_str()) {
            return;
        }
    }
    let (action, result): (&str, Result<(), WriteError>) = match &existing {
        Some(r) => (
            "update contract check",
            t.github.update_check_run(&t.repo, r.id, &run),
        ),
        None => (
            "create contract check",
            t.github.create_check_run(&t.repo, &run).map(|_| ()),
        ),
    };
    if let Err(e) = &result {
        if e.status == Some(403) {
            denied(report, e.status, &e.detail);
            return;
        }
    }
    t.act(
        log,
        report,
        &format!("pr#{} {}", pr.number, ticket.key),
        action,
        json!({"head": pr.head_sha, "conclusion": run.conclusion, "verdict": run.external_id}),
        || result.map_err(|e| e.to_string()),
    );
}

/// GitHub refused the check: said once in the pass's report, never a
/// failure; the hold comment carries the verdict alone.
fn denied(report: &mut Report, status: Option<u16>, detail: &str) {
    if report.notes.iter().any(|n| n.starts_with(DENIED)) {
        return;
    }
    report.notes.push(format!(
        "{DENIED}: GitHub refused it (HTTP {}: {detail}); the GitHub App needs Checks: read and write. \
         The bridge's comments still say why a PR is held",
        status.map(|s| s.to_string()).unwrap_or_default()
    ));
}
