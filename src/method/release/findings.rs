//! Findings turned into tickets (`docs/spec/06-qa-report.md` rules 4-6,
//! REQ-REL-052..061, 096): their markers, identity keys, bodies, the nits
//! tickets (shared or one per nit, each a contract), the `blocks` cycle
//! check, and the requalification plan.

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
    Regex::new(
        r"spl-asdlc:finding release=([^\s`>]+) candidate=([^\s`>]+) angle=([^\s`>]+) id=([^\s`>]+)",
    )
    .unwrap()
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

/// The identity of a finding ticket (spec 06 rule 5: candidate, angle,
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
    use crate::method::texts::inert;
    // The report is an agent's text: one line each, no marker, no backtick
    // that could close a code span.
    let code = |s: &str| inert(s).replace('`', "'");
    let title = format!(
        "{} ({} — {angle})",
        inert(&finding.summary),
        finding.severity
    );
    let mut criteria = vec![format!(
        "- [ ] The finding no longer reproduces at `{}`.",
        code(&finding.anchor)
    )];
    if let Some(test) = &finding.proposed_test {
        criteria.push(format!(
            "- [ ] The test `{}` exists and passes in CI.",
            code(test)
        ));
    }
    let mut body = String::new();
    if finding.severity == Severity::Blocker {
        body.push_str(&blocker_marker(release));
        body.push_str("\n\n");
    }
    let evidence = if finding.evidence.trim().is_empty() {
        "(none given)".to_string()
    } else {
        inert(&finding.evidence)
    };
    body.push_str(&format!(
        "## Goal\nFix the {severity} finding `{id}` of the {angle} review of `{candidate}`: {summary}\n\n\
         ## Non-goals\n- Anything the finding does not name.\n\n\
         ## Acceptance criteria\n{criteria}\n\n\
         ## Business rules\n{ABSENCE_DECLARATION}\n\n\
         ## Context\n- Anchor: `{anchor}`\n- Evidence: {evidence}\n- Angle PR: {angle_pr_url}\n- Report: `{report_path}`, finding `{id}`\n\n{marker}",
        severity = finding.severity,
        id = code(&finding.id),
        summary = inert(&finding.summary),
        criteria = criteria.join("\n"),
        anchor = code(&finding.anchor),
        marker = finding_marker(release, candidate, angle, &finding.id),
    ));
    (title, body)
}

/// The title of a release's shared nits ticket (REQ-REL-061).
pub fn nits_title(product: &str, version: &str) -> String {
    format!(
        "Nits — {product} {}",
        crate::method::vocabulary::normalise_version(version)
    )
}

/// Who judges a nit (spec 05 §6): the QA documented it, it did not decide it.
const NIT_JUDGEMENT: &str = "Kept or dropped is the judgement of the agent who takes this ticket up at \
     refinement: the QA that reported the nit documented it, it did not decide it.";

/// The key a nit's box starts with: candidate, angle and finding id (spec 06
/// rule 5; report ids restart with every candidate). It is the box's
/// identity, so that a reason written after it does not make it a new nit.
pub fn nit_key(candidate: &str, angle: &str, finding_id: &str) -> String {
    let v = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if c.is_whitespace() || c == '`' {
                    '_'
                } else {
                    c
                }
            })
            .collect()
    };
    format!("`{}/{}/{}`", v(candidate), v(angle), v(finding_id))
}

/// One nit as one acceptance box, carrying what the report says of it: what
/// was observed, where, and in what context. Each part is the report's text
/// made inert and collapsed to one line.
pub fn nit_line(candidate: &str, angle: &str, finding: &Finding) -> String {
    use crate::method::texts::inert;
    let context = if finding.evidence.trim().is_empty() {
        "(none given)".to_string()
    } else {
        inert(&finding.evidence)
    };
    format!(
        "- [ ] {key} {what} — where: `{anchor}` — context: {context}",
        key = nit_key(candidate, angle, &finding.id),
        what = inert(&finding.summary),
        anchor = inert(&finding.anchor).replace('`', "'"),
    )
}

/// The release's shared nits ticket (`release.nits = "shared"`): a complete
/// contract whose acceptance boxes are the nits, one per line.
pub fn nits_ticket(release: &str, product: &str, version: &str, lines: &[String]) -> String {
    format!(
        "## Goal\nTriage the nits the QA of {product} {version} reported: classify each box below as **kept** or \
         **dropped**; never presume a nit fixed. A dropped nit is closed with its reason: write the reason after its \
         line and tick its box. A kept nit becomes real work: its box stays open as a criterion of this ticket until \
         the nit no longer reproduces, or its line names the ticket the work moved to and its box is ticked here. \
         {NIT_JUDGEMENT}\n\n\
         ## Non-goals\n- Fixing a nit before judging whether to keep it.\n- Anything the boxes below do not name.\n\n\
         ## Acceptance criteria\n{lines}\n\n\
         ## Business rules\n{ABSENCE_DECLARATION}\n\n\
         ## Context\n- Release: {release}\n- Each box starts with its key, `<candidate>/<angle>/<finding id>`, then \
         says what was observed, where, and in what context, as the angle report gives it. The bridge adds the box of \
         each nit of a later merged report at the end of `## Acceptance criteria`, finds a box again by its key, and \
         never edits one.\n",
        version = crate::method::vocabulary::normalise_version(version),
        lines = lines.join("\n"),
    )
}

/// The ticket of one nit (`release.nits = "per-nit"`): a complete contract
/// whose one box is the nit, found again by its finding marker like any
/// finding ticket.
pub fn nit_ticket(
    release: &str,
    candidate: &str,
    angle: &str,
    finding: &Finding,
    angle_pr_url: &str,
    report_path: &str,
) -> (String, String) {
    use crate::method::texts::inert;
    let code = |s: &str| inert(s).replace('`', "'");
    let title = format!("{} (nit — {angle})", inert(&finding.summary));
    let body = format!(
        "## Goal\nTriage the nit `{id}` of the {angle} review of `{candidate}`: classify it as **kept** or \
         **dropped**; never presume it fixed. Dropped, it is closed with its reason: give the reason in a comment and \
         cancel this ticket. Kept, it becomes real work: refine this ticket like any other, and tick its box once the \
         nit no longer reproduces. {NIT_JUDGEMENT}\n\n\
         ## Non-goals\n- Fixing the nit before judging whether to keep it.\n- Anything the nit does not name.\n\n\
         ## Acceptance criteria\n{line}\n\n\
         ## Business rules\n{ABSENCE_DECLARATION}\n\n\
         ## Context\n- Release: {release}\n- Angle PR: {angle_pr_url}\n- Report: `{report_path}`, finding `{id}`\n\n{marker}",
        id = code(&finding.id),
        line = nit_line(candidate, angle, finding),
        marker = finding_marker(release, candidate, angle, &finding.id),
    );
    (title, body)
}

fn nit_text(line: &str) -> Option<String> {
    NIT_LINE.captures(line).map(|c| c[1].trim().to_string())
}

/// Add a nit's box at the end of the shared ticket's `## Acceptance
/// criteria`, unless a box starting with its key is there, ticked or not
/// (`Ok(None)`: nothing to write). Content this did not write survives.
/// A ticket that carries no contract (the older format of checkbox lines
/// only, or a contract broken by hand) is refused, fail-closed: the bridge
/// never writes a nit into it, so that what it serves keeps a contract.
pub fn append_nit(description: &str, key: &str, line: &str) -> Result<Option<String>, String> {
    use crate::method::{contract, markdown};
    let missing = contract::missing_from_contract(description);
    if !missing.is_empty() {
        return Err(format!(
            "carries no contract ({}): no nit is written into it until it has one",
            missing.join("; ")
        ));
    }
    let fenced = markdown::without_fences(description);
    if fenced
        .iter()
        .any(|l| nit_text(l).is_some_and(|t| t.starts_with(key)))
    {
        return Ok(None);
    }
    let Some(section) = markdown::sections_of(&fenced, contract::is_acceptance_heading)
        .into_iter()
        .next()
    else {
        return Err("has no `## Acceptance criteria` section".into());
    };
    let lines: Vec<&str> = description.lines().collect();
    let at = (section.start..section.end)
        .rev()
        .find(|&i| !lines[i].trim().is_empty())
        .map_or(section.start, |i| i + 1);
    let mut out = lines[..at].to_vec();
    out.push(line);
    out.extend_from_slice(&lines[at..]);
    let mut text = out.join("\n");
    if description.ends_with('\n') {
        text.push('\n');
    }
    Ok(Some(text))
}

/// Unchecked boxes of the nits ticket (the status count).
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
/// The reason is mandatory (spec 09): an empty one is refused.
pub fn requalify_comment(
    who: &str,
    from: Severity,
    to: Severity,
    reason: &str,
) -> Result<String, String> {
    if reason.trim().is_empty() {
        return Err("a requalification needs a reason".into());
    }
    Ok(format!(
        "Requalified by {who}: `{from}` → `{to}`.\n\nReason: {}\n\nThe GO is recomputed at the next tick.",
        crate::method::texts::inert(reason)
    ))
}

#[cfg(test)]
mod tests;
