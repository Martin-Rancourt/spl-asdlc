//! The release note (`docs/reference/05-release-cycle.md` §12, spec 01 §6,
//! REQ-REL-093/094): built from every PR of the release's range — each
//! shipped ticket's `## Release note` snippet, and for a PR no ticket
//! describes its own `## Release note` section or its title — written into
//! the release item before the GO publishes the release, and sent through
//! the notifier when the release is published.

use crate::method::markdown::clip_words;

pub const START: &str = "<!-- spl-asdlc:release-note:start -->";
pub const END: &str = "<!-- spl-asdlc:release-note:end -->";
pub const PLACEHOLDER: &str = "To be completed before publication.";
pub const TRUNCATION_MARK: &str = "(note truncated — full text via the release link above)";
const SUMMARY_MAX: usize = 280;

/// A shipped ticket as the note names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketEntry {
    pub id: String,
    pub title: String,
    /// The ticket's `## Release note` snippet, or `None` when the ticket could
    /// not be read.
    pub snippet: Option<String>,
    /// The first paragraph of its Goal, used when it carries no snippet.
    pub goal: String,
    pub prs: Vec<u64>,
}

/// A PR of the range that no ticket describes: the note announces it by its
/// own `## Release note` section, or its title when it has none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Untracked {
    pub number: u64,
    pub title: String,
    pub body: String,
}

/// One line, and nothing in it can forge the block's markers.
fn one_line(s: &str) -> String {
    crate::method::texts::inert(s)
}

/// The generated block of the release item's `## Release note` section.
pub fn block(candidate: &str, tickets: &[TicketEntry], untracked: &[Untracked]) -> String {
    let mut out = vec![
        START.to_string(),
        format!(
            "_Generated from `{candidate}` and rewritten at every candidate; text outside this block is never touched._"
        ),
    ];
    let mut tickets = tickets.to_vec();
    tickets.sort_by_key(|t| ticket_order(&t.id));
    if !tickets.is_empty() {
        out.push(String::new());
        out.push("### Tickets".to_string());
        out.push(String::new());
        for t in &tickets {
            let prs = t
                .prs
                .iter()
                .map(|n| format!("#{n}"))
                .collect::<Vec<_>>()
                .join(", ");
            let prs = if prs.is_empty() {
                String::new()
            } else {
                format!(" ({prs})")
            };
            let title = if t.title.trim().is_empty() {
                "(untitled)".to_string()
            } else {
                one_line(&t.title)
            };
            match &t.snippet {
                None => out.push(format!(
                    "- **{}** — {title}{prs} (ticket unreadable: content not carried over)",
                    t.id
                )),
                Some(snippet) => {
                    let summary = if snippet.trim().is_empty() {
                        one_line(&t.goal)
                    } else {
                        clip_words(&one_line(snippet), SUMMARY_MAX)
                    };
                    if summary.is_empty() {
                        out.push(format!("- **{}** — {title}{prs}", t.id));
                    } else {
                        out.push(format!("- **{}** — {title}: {summary}{prs}", t.id));
                    }
                }
            }
        }
    }
    if !untracked.is_empty() {
        out.push(String::new());
        out.push("### No linked ticket".to_string());
        out.push(String::new());
        for u in untracked {
            let title = one_line(&u.title);
            let snippet = crate::method::contract::release_note_snippet(&u.body);
            if snippet.is_empty() {
                out.push(format!("- **PR #{}** — {title}", u.number));
            } else {
                out.push(format!(
                    "- **PR #{}** — {title}: {}",
                    u.number,
                    clip_words(&one_line(&snippet), SUMMARY_MAX)
                ));
            }
        }
    }
    if tickets.is_empty() && untracked.is_empty() {
        out.push(String::new());
        out.push("No change since the last published release.".to_string());
    }
    out.push(END.to_string());
    out.join("\n")
}

/// `(prefix, number)` order of identifiers like `ABC-12`.
pub fn ticket_order(id: &str) -> (String, u64) {
    match id.rsplit_once('-') {
        Some((p, n)) => (p.to_string(), n.parse().unwrap_or(u64::MAX)),
        None => (id.to_string(), u64::MAX),
    }
}

fn is_release_note_heading(line: &str) -> bool {
    let t = line.trim_end();
    t.strip_prefix("##").is_some_and(|rest| {
        rest.starts_with([' ', '\t']) && rest.trim().eq_ignore_ascii_case("release note")
    })
}

fn is_placeholder(line: &str) -> bool {
    let core = line.trim().trim_matches(|c| matches!(c, '\\' | '_' | '*'));
    core.trim() == PLACEHOLDER
}

/// Put `block` into a description (REQ-REL-093): replace a previous block;
/// else place it first in the `## Release note` section, above any human
/// text and without the fresh-issue placeholder; else append the section.
pub fn splice(description: &str, block: &str) -> String {
    // The delimiters as written, or made visible by a tracker that drops HTML
    // comments.
    let visible = |m: &str| crate::method::texts::visible_markers(m);
    for (start, end) in [
        (START.to_string(), END.to_string()),
        (visible(START), visible(END)),
    ] {
        if let (Some(s), Some(e)) = (description.find(&start), description.find(&end)) {
            if e > s {
                return format!(
                    "{}{}{}",
                    &description[..s],
                    block,
                    &description[e + end.len()..]
                );
            }
        }
    }
    let lines: Vec<&str> = description.lines().collect();
    if let Some(h) = lines.iter().position(|l| is_release_note_heading(l)) {
        let end = lines[h + 1..]
            .iter()
            .position(|l| l.starts_with('#'))
            .map(|i| h + 1 + i)
            .unwrap_or(lines.len());
        let mut body: Vec<&str> = lines[h + 1..end]
            .iter()
            .copied()
            .filter(|l| !is_placeholder(l))
            .collect();
        while body.first().is_some_and(|l| l.trim().is_empty()) {
            body.remove(0);
        }
        while body.last().is_some_and(|l| l.trim().is_empty()) {
            body.pop();
        }
        let mut out: Vec<String> = lines[..=h].iter().map(|l| l.to_string()).collect();
        out.push(String::new());
        out.push(block.to_string());
        if !body.is_empty() {
            out.push(String::new());
            out.extend(body.iter().map(|l| l.to_string()));
        }
        if end < lines.len() {
            out.push(String::new());
            out.extend(lines[end..].iter().map(|l| l.to_string()));
        }
        return out.join("\n");
    }
    format!("{}\n\n## Release note\n\n{block}\n", description.trim_end())
}

/// The message sent when a release is published (REQ-REL-094): the head
/// first and whole, then the note cut on whole lines so a channel's limit
/// only ever loses the tail; `measure` counts in the channel's unit.
pub fn published_message(
    head: &str,
    body: &str,
    max: usize,
    measure: &dyn Fn(&str) -> usize,
) -> String {
    if body.trim().is_empty() {
        return head.to_string();
    }
    let full = format!("{head}\n\n{body}");
    if measure(&full) <= max {
        return full;
    }
    let mut kept = format!("{head}\n\n");
    let mark = format!("\n{TRUNCATION_MARK}");
    for line in body.lines() {
        let candidate = format!("{kept}{line}\n");
        if measure(&format!("{}{mark}", candidate.trim_end_matches('\n'))) > max {
            break;
        }
        kept = candidate;
    }
    let with_mark = format!("{}{mark}", kept.trim_end_matches('\n'));
    if measure(&with_mark) <= max {
        return with_mark;
    }
    // Last resort: the mark itself does not fit beside the head; clip it.
    let mut out = format!("{head}\n");
    for c in TRUNCATION_MARK.chars() {
        let next = format!("{out}{c}");
        if measure(&next) > max {
            break;
        }
        out = next;
    }
    out
}

/// The head of the published message.
pub fn published_head(
    product: &str,
    version: &str,
    release_url: Option<&str>,
    item_url: &str,
    run_url: Option<&str>,
) -> String {
    format!(
        "{product} {version} is published.\nRelease: {}\nRelease item: {item_url}\nRun: {}",
        release_url.unwrap_or("(none — manual publication by dispatch)"),
        run_url.unwrap_or("(none)")
    )
}

#[cfg(test)]
mod tests;
