//! Findings turned into tickets (`docs/reference/06-qa-report.md` rules 3-5,
//! BR-ASDLC-022): their markers, identity keys and bodies (a blocking
//! finding an urgent fix ticket, a non-blocking one a ticket logged for
//! later, each a contract), the `blocks` cycle check, and the
//! requalification plan.

use std::collections::{BTreeSet, VecDeque};
use std::sync::LazyLock;

use regex::Regex;

use super::report::Finding;
use crate::method::business_rules::ABSENCE_DECLARATION;

static BLOCKER_MARKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"release_blocker::\s*\**\s*`?([A-Z][A-Z0-9]{1,9}-\d+)`?").unwrap()
});
static FINDING_MARKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"spl-asdlc:finding release=([^\s`>]+) candidate=([^\s`>]+) angle=([^\s`>]+) id=([^\s`>]+)",
    )
    .unwrap()
});

/// The marker a blocking finding's ticket carries in its own description,
/// written by the same create as the ticket (REQ-REL-056).
pub fn blocker_marker(release: &str) -> String {
    format!(
        "release_blocker:: {release}\n\nThis finding blocks the release above. The marker is written with the ticket; the \
         `blocks` relation is a second write that can fail, and while the marker is here without the relation the bridge \
         lays the relation again and keeps the GO false. Do not remove this line by hand: close the finding, or requalify \
         it with `spl-asdlc release requalify`."
    )
}

/// The release a description says it blocks, read tolerantly (bold,
/// backticks, spaces around the identifier).
pub fn parse_blocker_marker(description: &str) -> Option<String> {
    BLOCKER_MARKER
        .captures(description)
        .map(|c| c[1].to_string())
}

/// The description without its blocker marker paragraph (requalification
/// to non-blocking removes it).
pub fn remove_blocker_marker(description: &str) -> String {
    let marker_start = match BLOCKER_MARKER.find(description) {
        Some(m) => m.start(),
        None => return description.to_string(),
    };
    let line_start = description[..marker_start]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let tail = &description[marker_start..];
    let end = tail
        .find("`spl-asdlc release requalify`.")
        .map(|i| marker_start + i + "`spl-asdlc release requalify`.".len())
        .unwrap_or_else(|| marker_start + tail.find('\n').unwrap_or(tail.len()));
    let mut out = format!(
        "{}{}",
        &description[..line_start],
        description[end..].trim_start_matches('\n')
    );
    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }
    out
}

/// The identity of a finding ticket (spec 06 rule 4: candidate, angle,
/// finding id; report ids restart with every candidate): survives a retitle.
pub fn finding_marker(release: &str, candidate: &str, angle: &str, finding_id: &str) -> String {
    crate::method::texts::marker(
        "finding",
        &[
            ("release", release),
            ("candidate", candidate),
            ("angle", angle),
            ("id", finding_id),
        ],
    )
}

/// `(release, candidate, angle, finding id)` of a finding ticket.
pub fn parse_finding_marker(description: &str) -> Option<(String, String, String, String)> {
    FINDING_MARKER.captures(description).map(|c| {
        (
            c[1].to_string(),
            c[2].to_string(),
            c[3].to_string(),
            c[4].to_string(),
        )
    })
}

/// Titles compare whitespace-collapsed and case-folded (REQ-REL-053).
pub fn title_key(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// How a finding is named in titles and comments.
pub fn grade(blocking: bool) -> &'static str {
    if blocking { "blocking" } else { "non-blocking" }
}

/// The ticket a finding becomes (BR-ASDLC-012, BR-ASDLC-022): a complete
/// contract whose acceptance criteria carry the finding as the report gives
/// it (what was observed, where, in what context) and its proposed test. A
/// blocking finding's ticket carries the blocker marker and asks for the fix
/// on the release branch; a non-blocking one is logged for later.
pub fn finding_ticket(
    release: &str,
    candidate: &str,
    angle: &str,
    finding: &Finding,
    angle_pr_url: &str,
    report_path: &str,
) -> (String, String) {
    use crate::method::texts::inert;
    // The report is an agent's text: one line each, no marker, no backtick
    // that could close a code span.
    let code = |s: &str| inert(s).replace('`', "'");
    let grade = grade(finding.blocking);
    let title = format!("{} ({grade} — {angle})", inert(&finding.summary));
    let context = if finding.evidence.trim().is_empty() {
        "(none given)".to_string()
    } else {
        inert(&finding.evidence)
    };
    let mut criteria = vec![format!(
        "- [ ] The finding no longer reproduces: {what} — where: `{anchor}` — context: {context}",
        what = inert(&finding.summary),
        anchor = code(&finding.anchor),
    )];
    if let Some(test) = &finding.proposed_test {
        criteria.push(format!(
            "- [ ] The test `{}` exists and passes in CI.",
            code(test)
        ));
    }
    let mut body = String::new();
    let goal = if finding.blocking {
        body.push_str(&blocker_marker(release));
        body.push_str("\n\n");
        "It holds the release's GO: fix it on the release branch (a PR whose base is that branch); the fix merged there \
         makes the next candidate."
    } else {
        "It does not hold the release: it is logged for later, to refine and schedule like any ticket."
    };
    body.push_str(&format!(
        "## Goal\nFix the {grade} finding `{id}` of the {angle} review of `{candidate}`. {goal}\n\n\
         ## Non-goals\n- Anything the finding does not name.\n\n\
         ## Acceptance criteria\n{criteria}\n\n\
         ## Business rules\n{ABSENCE_DECLARATION}\n\n\
         ## Context\n- Angle PR: {angle_pr_url}\n- Report: `{report_path}`, finding `{id}`\n- {keep}\n\n{marker}",
        keep = crate::method::texts::KEEP_MARKER,
        id = code(&finding.id),
        criteria = criteria.join("\n"),
        marker = finding_marker(release, candidate, angle, &finding.id),
    ));
    (title, body)
}

/// The title of the shared nits ticket an older bridge opened per release:
/// the closure still recognises it, to archive it once resolved
/// (BR-ASDLC-019). No nits ticket is opened any more (BR-ASDLC-022).
pub fn nits_title(product: &str, version: &str) -> String {
    format!(
        "Nits — {product} {}",
        crate::method::vocabulary::normalise_version(version)
    )
}

/// Bounds of the walk that proves a `blocks` relation closes no loop.
pub const RELATION_DEPTH: usize = 25;
pub const RELATION_NODES: usize = 200;

/// Whether `a blocks b` may be written (REQ-REL-057): never onto itself,
/// never closing a loop, and refused when the walk could not prove the
/// absence of a loop (a blocker unreadable, the bounds spent).
/// `blockers_of(x)` answers the items that block `x`.
pub fn may_block(
    a: &str,
    b: &str,
    blockers_of: &mut dyn FnMut(&str) -> Result<Vec<String>, String>,
) -> Result<(), String> {
    if a == b {
        return Err(format!("{a} may not block itself"));
    }
    // `a blocks b` closes a loop iff `b` already blocks `a`, directly or
    // through others: walk the blockers of `a` looking for `b`.
    let mut seen = BTreeSet::from([a.to_string()]);
    let mut queue = VecDeque::from([(a.to_string(), 0usize)]);
    while let Some((item, depth)) = queue.pop_front() {
        if depth >= RELATION_DEPTH {
            return Err(format!(
                "the walk of the blockers of {a} reached its depth bound: a loop could not be ruled out"
            ));
        }
        let blockers = blockers_of(&item).map_err(|e| {
            format!("{item} could not be read ({e}): a loop could not be ruled out")
        })?;
        for x in blockers {
            if x == b {
                return Err(format!(
                    "{a} blocks {b} would close a loop ({b} already blocks {item})"
                ));
            }
            if seen.insert(x.clone()) {
                if seen.len() > RELATION_NODES {
                    return Err(format!(
                        "the walk of the blockers of {a} spent its node budget: a loop could not be ruled out"
                    ));
                }
                queue.push_back((x, depth + 1));
            }
        }
    }
    Ok(())
}

/// The steps of a human requalification (REQ-REL-096, spec 09).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequalifyStep {
    WriteBlockerMarker,
    LayBlocksRelation,
    RemoveFindingLabel,
    /// Ready, assigned to `release.fix_assignee` (unassigned when unset or
    /// unknown), as a blocking finding's ticket is created.
    ServeFixer,
    RemoveBlocksRelation,
    RemoveBlockerMarker,
    AddFindingLabel,
    LayRelatedRelation,
    /// Triage, unassigned, as a non-blocking finding's ticket is created.
    LogForLater,
}

/// What requalifying a finding ticket takes; empty when it already is
/// what is asked. The priority is left as it is (the trackers' interface
/// writes it at creation only): the person changes it in the tracker.
pub fn requalify_plan(from_blocking: bool, to_blocking: bool) -> Vec<RequalifyStep> {
    use RequalifyStep::*;
    match (from_blocking, to_blocking) {
        (false, true) => vec![
            WriteBlockerMarker,
            LayBlocksRelation,
            RemoveFindingLabel,
            ServeFixer,
        ],
        (true, false) => vec![
            RemoveBlocksRelation,
            RemoveBlockerMarker,
            AddFindingLabel,
            LayRelatedRelation,
            LogForLater,
        ],
        _ => Vec::new(),
    }
}

/// The comment a requalification leaves on the finding: who, from, to, why.
/// The reason is mandatory (spec 09): an empty one is refused.
pub fn requalify_comment(
    who: &str,
    from_blocking: bool,
    to_blocking: bool,
    reason: &str,
) -> Result<String, String> {
    if reason.trim().is_empty() {
        return Err("a requalification needs a reason".into());
    }
    Ok(format!(
        "Requalified by {who}: `{}` → `{}`.\n\nReason: {}\n\nThe GO is recomputed at the next tick.",
        grade(from_blocking),
        grade(to_blocking),
        crate::method::texts::inert(reason)
    ))
}

#[cfg(test)]
mod tests;
