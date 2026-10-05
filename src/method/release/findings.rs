//! Findings turned into tickets (`docs/spec/06-qa-report.md` rules 4-6,
//! REQ-REL-052..061, 096): their markers, identity keys, bodies, the shared
//! nits ticket, the `blocks` cycle check, and the requalification plan.

use std::collections::{BTreeSet, VecDeque};
use std::sync::LazyLock;

use regex::Regex;

use super::report::Finding;
use crate::method::business_rules::ABSENCE_DECLARATION;
use crate::method::vocabulary::Severity;

static BLOCKER_MARKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"release_blocker::\s*\**\s*`?([A-Z][A-Z0-9]{1,9}-\d+)`?").unwrap()
});
static FINDING_MARKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<!-- spl-asdlc:finding release=(\S+) angle=(\S+) id=(\S+) -->").unwrap()
});
static NIT_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*[-*+]\s+\[[ xX]\]\s+(.*)$").unwrap());

/// The marker a blocker carries in its own description, written by the same
/// create as the ticket (REQ-REL-056).
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
/// to a non-blocking severity removes it).
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

/// The hidden identity of a finding ticket: survives a retitle.
pub fn finding_marker(release: &str, angle: &str, finding_id: &str) -> String {
    format!("<!-- spl-asdlc:finding release={release} angle={angle} id={finding_id} -->")
}

pub fn parse_finding_marker(description: &str) -> Option<(String, String, String)> {
    FINDING_MARKER
        .captures(description)
        .map(|c| (c[1].to_string(), c[2].to_string(), c[3].to_string()))
}

/// Titles compare whitespace-collapsed and case-folded (REQ-REL-053).
pub fn title_key(title: &str) -> String {
    title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The ticket a blocker or should-fix finding becomes: a complete contract,
/// whose acceptance criteria carry the proposed test (rule 6).
pub fn finding_ticket(
    release: &str,
    candidate: &str,
    angle: &str,
    finding: &Finding,
    angle_pr_url: &str,
    report_path: &str,
) -> (String, String) {
    let title = format!(
        "{} ({} — {angle})",
        finding
            .summary
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
        finding.severity
    );
    let mut criteria = vec![format!(
        "- [ ] The finding no longer reproduces at `{}`.",
        finding.anchor
    )];
    if let Some(test) = &finding.proposed_test {
        criteria.push(format!("- [ ] The test `{test}` exists and passes in CI."));
    }
    let mut body = String::new();
    if finding.severity == Severity::Blocker {
        body.push_str(&blocker_marker(release));
        body.push_str("\n\n");
    }
    body.push_str(&format!(
        "## Goal\nFix the {severity} finding `{id}` of the {angle} review of `{candidate}`: {summary}\n\n\
         ## Non-goals\n- Anything the finding does not name.\n\n\
         ## Acceptance criteria\n{criteria}\n\n\
         ## Business rules\n{ABSENCE_DECLARATION}\n\n\
         ## Context\n- Anchor: `{anchor}`\n- Evidence: {evidence}\n- Angle PR: {angle_pr_url}\n- Report: `{report_path}`, finding `{id}`\n\n{marker}",
        severity = finding.severity,
        id = finding.id,
        summary = finding.summary,
        criteria = criteria.join("\n"),
        anchor = finding.anchor,
        evidence = if finding.evidence.is_empty() { "(none given)" } else { &finding.evidence },
        marker = finding_marker(release, angle, &finding.id),
    ));
    (title, body)
}

/// The title of a release's single shared nits ticket (REQ-REL-061).
pub fn nits_title(product: &str, version: &str) -> String {
    format!(
        "Nits — {product} {}",
        crate::method::vocabulary::normalise_version(version)
    )
}

/// One nit as one checkbox line: anchor and text each collapsed to a line.
pub fn nit_line(anchor: &str, text: &str) -> String {
    let one = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    format!("- [ ] {}: {}", one(anchor), one(text))
}

fn nit_text(line: &str) -> Option<String> {
    NIT_LINE.captures(line).map(|c| c[1].trim().to_string())
}

/// Append a nit to the shared ticket's description, unless a line with the
/// same text is there, ticked or not. Content this did not write survives.
pub fn append_nit(description: &str, line: &str) -> Option<String> {
    let text = nit_text(line)?;
    if description
        .lines()
        .any(|l| nit_text(l).as_deref() == Some(text.as_str()))
    {
        return None;
    }
    let base = description.trim_end_matches('\n');
    Some(if base.is_empty() {
        format!("{line}\n")
    } else {
        format!("{base}\n{line}\n")
    })
}

/// Unchecked nit lines of the shared ticket (the status count).
pub fn open_nits(description: &str) -> usize {
    crate::method::markdown::checklist(description).1.len()
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
    RemoveBlocksRelation,
    RemoveBlockerMarker,
    AddFindingLabel,
    SetTriage,
    MoveToNitsTicket,
    CancelTicket,
}

/// What requalifying a finding ticket from `from` to `to` takes; empty
/// when it already has that severity.
pub fn requalify_plan(from: Severity, to: Severity) -> Vec<RequalifyStep> {
    use RequalifyStep::*;
    match (from, to) {
        (f, t) if f == t => Vec::new(),
        (_, Severity::Blocker) => vec![WriteBlockerMarker, LayBlocksRelation],
        (Severity::Blocker, Severity::ShouldFix) => {
            vec![
                RemoveBlocksRelation,
                RemoveBlockerMarker,
                AddFindingLabel,
                SetTriage,
            ]
        }
        (Severity::Blocker, Severity::Nit) => {
            vec![
                RemoveBlocksRelation,
                RemoveBlockerMarker,
                MoveToNitsTicket,
                CancelTicket,
            ]
        }
        (Severity::ShouldFix, Severity::Nit) => vec![MoveToNitsTicket, CancelTicket],
        (Severity::Nit, Severity::ShouldFix) => vec![AddFindingLabel, SetTriage],
        _ => Vec::new(),
    }
}

/// The comment a requalification leaves on the finding: who, from, to, why.
pub fn requalify_comment(who: &str, from: Severity, to: Severity, reason: &str) -> String {
    format!(
        "Requalified by {who}: `{from}` → `{to}`.\n\nReason: {}\n\nThe GO is recomputed at the next tick.",
        reason.trim()
    )
}

#[cfg(test)]
mod tests;
