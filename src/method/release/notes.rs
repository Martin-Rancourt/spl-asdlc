//! The release-please note and the range it must cover (REQ-REL-020..024,
//! REQ-REL-073): every Conventional commit of the candidate's range is named
//! in the note; unreadable is a gap, never coverage.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::shape::Version;
use crate::github::CommitSummary;

const ALL_TYPES: [&str; 11] = [
    "feat", "fix", "perf", "revert", "docs", "style", "chore", "refactor", "test", "build", "ci",
];
const ALWAYS_WRITTEN: [&str; 4] = ["feat", "fix", "perf", "revert"];

static CONVENTIONAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(feat|fix|perf|revert|docs|style|chore|refactor|test|build|ci)(\(([^)]+)\))?!?:\s+(.+)$",
    )
    .unwrap()
});
static PR_SUFFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*\(#(\d+)\)\s*$").unwrap());

/// The commit types the note writes, read from `release-please-config.json`
/// at the judged commit. `None` (cannot tell) relaxes nothing: every
/// Conventional type is owed.
pub fn written_types(config_text: Option<&str>) -> Option<BTreeSet<String>> {
    let v: Value = serde_json::from_str(config_text?).ok()?;
    let root = v.get("packages")?.get(".")?;
    if !root.is_object() {
        return None;
    }
    let sections = root
        .get("changelog-sections")
        .or_else(|| v.get("changelog-sections"));
    match sections {
        None => Some(ALWAYS_WRITTEN.iter().map(|s| s.to_string()).collect()),
        Some(Value::Array(list)) => Some(
            list.iter()
                .filter(|e| !e.get("hidden").and_then(Value::as_bool).unwrap_or(false))
                .filter_map(|e| e.get("type").and_then(Value::as_str))
                .map(str::to_string)
                .collect(),
        ),
        Some(_) => None,
    }
}

/// The commits of a range the note owes a line, in range order.
pub fn missing_entries(
    commits: &[CommitSummary],
    note: &str,
    types: Option<&BTreeSet<String>>,
) -> Vec<CommitSummary> {
    commits
        .iter()
        .filter(|c| owed(c, types) && !named(c, note))
        .cloned()
        .collect()
}

fn owed(c: &CommitSummary, types: Option<&BTreeSet<String>>) -> bool {
    let Some(caps) = CONVENTIONAL.captures(c.subject.trim()) else {
        return false;
    };
    match types {
        Some(t) => t.contains(&caps[1]),
        None => ALL_TYPES.contains(&&caps[1]),
    }
}

fn named(c: &CommitSummary, note: &str) -> bool {
    let subject = c.subject.trim();
    if let Some(m) = PR_SUFFIX.captures(subject) {
        return contains_pr_number(note, &m[1]);
    }
    let summary = CONVENTIONAL
        .captures(subject)
        .map(|c| c[4].trim().to_string())
        .unwrap_or_default();
    !summary.is_empty() && note.contains(&summary)
}

/// `#N` as a complete numeric token: `#7830` is not `#783`.
fn contains_pr_number(note: &str, n: &str) -> bool {
    let needle = format!("#{n}");
    let bytes = note.as_bytes();
    let mut start = 0;
    while let Some(i) = note[start..].find(&needle) {
        let at = start + i;
        let before_ok = at == 0 || !bytes[at - 1].is_ascii_digit();
        let end = at + needle.len();
        let after_ok = end >= bytes.len() || !bytes[end].is_ascii_digit();
        if before_ok && after_ok {
            return true;
        }
        start = at + 1;
    }
    false
}

/// The `## [<version>]` section of a changelog, up to the next `## [`.
pub fn release_section(version: Version, changelog: &str) -> Option<String> {
    let heading = format!("## [{version}]");
    let mut out: Option<Vec<&str>> = None;
    for line in changelog.lines() {
        match &mut out {
            None => {
                if line.trim_start().starts_with(&heading) {
                    out = Some(vec![line]);
                }
            }
            Some(lines) => {
                if line.trim_start().starts_with("## [") {
                    break;
                }
                lines.push(line);
            }
        }
    }
    out.map(|l| l.join("\n"))
}

/// The coverage of a candidate's range by its note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteCoverage {
    pub missing: Vec<CommitSummary>,
    /// Why coverage could not be judged; a gap, never coverage.
    pub unreadable: Option<String>,
}

impl NoteCoverage {
    pub fn covered(&self) -> bool {
        self.missing.is_empty() && self.unreadable.is_none()
    }

    /// One line per missing commit (`<sha12> <subject>`), or the reason.
    pub fn gaps(&self) -> Vec<String> {
        if let Some(why) = &self.unreadable {
            return vec![format!("note unreadable: {why}")];
        }
        self.missing
            .iter()
            .map(|c| format!("{} {}", short(&c.sha), c.subject))
            .collect()
    }
}

pub fn short(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}

/// Judge the note of `version` in `changelog` over `commits` (REQ-REL-022).
/// `range` is the commits of the range or why they could not be read.
pub fn coverage(
    version: Version,
    changelog: Option<&str>,
    config: Option<&str>,
    range: Result<&[CommitSummary], &str>,
) -> NoteCoverage {
    let unreadable = |why: String| NoteCoverage {
        missing: Vec::new(),
        unreadable: Some(why),
    };
    let commits = match range {
        Ok(c) => c,
        Err(why) => return unreadable(format!("the range could not be read: {why}")),
    };
    let Some(changelog) = changelog.filter(|c| !c.trim().is_empty()) else {
        return unreadable("the changelog is empty or could not be read".into());
    };
    let Some(section) = release_section(version, changelog) else {
        return unreadable(format!("the changelog has no `## [{version}]` section"));
    };
    let types = written_types(config);
    NoteCoverage {
        missing: missing_entries(commits, &section, types.as_ref()),
        unreadable: None,
    }
}

/// Lines a person may paste into the release-please PR (never written by
/// the bridge, REQ-REL-024).
pub fn suggested_entries(missing: &[CommitSummary], repo_url: Option<&str>) -> Vec<String> {
    missing
        .iter()
        .map(|c| {
            let subject = c.subject.trim();
            let pr = PR_SUFFIX.captures(subject).map(|m| m[1].to_string());
            let without_pr = PR_SUFFIX.replace(subject, "").to_string();
            let (scope, text) = match CONVENTIONAL.captures(&without_pr) {
                Some(caps) => (
                    caps.get(3).map(|m| m.as_str().to_string()),
                    caps[4].to_string(),
                ),
                None => (None, without_pr.clone()),
            };
            let text = match scope {
                Some(s) => format!("**{s}:** {text}"),
                None => text,
            };
            match (pr, repo_url) {
                (Some(n), Some(url)) => format!("* {text} ([#{n}]({url}/issues/{n}))"),
                (Some(n), None) => format!("* {text} (#{n})"),
                (None, _) => format!("* {text}"),
            }
        })
        .collect()
}

/// The version release-please stages for `product` in one release PR, or
/// `None` (a mismatch by construction). The cut workflow reads it the same
/// way (`asdlc-release-cut.yml`, "Read the staged version"), so the version
/// the cut names and the version the GO checks come from one reading:
///
/// 1. the first `X.Y.Z` right after the product's name, matched as a whole
///    name and case-insensitively, then blanks or a colon: in the title
///    (`chore(<branch>): release <product> X.Y.Z`), else in the body (the
///    multi-component `<summary><product>: X.Y.Z</summary>`);
/// 2. else, in a body with no `<summary>` (one component's note), the first
///    `## [X.Y.Z]` heading, unless the head is another component's
///    (`release-please--branches--<target>--components--<other>`).
pub fn staged_version(title: &str, body: &str, head: &str, product: &str) -> Option<Version> {
    static HEADING: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?:^|\n)## \[([0-9]+\.[0-9]+\.[0-9]+)\]").unwrap());
    let named = Regex::new(&format!(
        r"(?:^|[^A-Za-z0-9_.@/-])(?i:{})(?:\s+|\s*:\s*)([0-9]+\.[0-9]+\.[0-9]+)",
        regex::escape(product)
    ))
    .ok()?;
    let by_name = |s: &str| named.captures(s).and_then(|c| Version::parse(&c[1]));
    if let Some(v) = by_name(title).or_else(|| by_name(body)) {
        return Some(v);
    }
    let another_components = head
        .split_once("--components--")
        .is_some_and(|(_, c)| !c.eq_ignore_ascii_case(product));
    if body.contains("<summary>") || another_components {
        return None;
    }
    HEADING.captures(body).and_then(|c| Version::parse(&c[1]))
}

#[cfg(test)]
mod tests;
