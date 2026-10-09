//! The PRs of the release cycle (REQ-REL-030, 049, 076, 077, 081): the
//! release-please PR(s), recognised by shape and trusted by authorship;
//! angle PRs; the deployed ledger of a multi-component cycle.

use std::sync::LazyLock;

use regex::Regex;

use super::shape::{Shape, Version};
use crate::github::{PullRequest, same_login};
use crate::method::review::is_release_please_shaped;
use crate::method::vocabulary::DEPLOYED_LABEL;

/// Authored by a configured App (`<slug>[bot]`, `app/<slug>` and `<slug>`
/// name the same one, [`same_login`]), and not a human account whose login
/// merely resembles it.
pub fn trusted_author(pr: &PullRequest, authors: &[String]) -> bool {
    match &pr.author {
        Some(a) => a.is_bot && authors.iter().any(|x| same_login(x, &a.login)),
        None => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasePrLookup<'a> {
    pub pr: Option<&'a PullRequest>,
    /// A PR against the base had the shape, even one refused on authorship.
    pub shape_seen: bool,
    /// Shape-matching PRs refused on authorship, by number (to log).
    pub refused: Vec<u64>,
}

/// The release PR of a cycle: shaped like release-please's, open against
/// the cycle's release branch (never a repository-wide search) and authored
/// by a configured identity. `base` must not be empty.
pub fn release_please_pr<'a>(
    open: &'a [PullRequest],
    base: &str,
    authors: &[String],
    label: &str,
) -> Result<ReleasePrLookup<'a>, &'static str> {
    if base.trim().is_empty() {
        return Err("no base: the release PR is never looked for repository-wide");
    }
    let mut out = ReleasePrLookup {
        pr: None,
        shape_seen: false,
        refused: Vec::new(),
    };
    let mut trusted = Vec::new();
    for pr in open
        .iter()
        .filter(|p| p.base_ref == base && is_release_please_shaped(p, label))
    {
        out.shape_seen = true;
        if trusted_author(pr, authors) {
            trusted.push(pr);
        } else {
            out.refused.push(pr.number);
        }
    }
    if trusted.len() > 1 {
        // Which one would be merged is not a fact: refuse.
        return Err("two release-please PRs are open against the release branch");
    }
    out.pr = trusted.first().copied();
    Ok(out)
}

/// The `(candidate, angle)` of an angle branch `qa/<candidate>/<angle>`.
pub fn parse_angle_branch(head: &str) -> Option<(String, String)> {
    let rest = head.strip_prefix("qa/")?;
    let (candidate, angle) = rest.rsplit_once('/')?;
    (!candidate.is_empty() && !angle.is_empty() && !angle.contains('/'))
        .then(|| (candidate.to_string(), angle.to_string()))
}

/// The sub-item identifier an angle PR names, among the expected ones, read
/// from its title then its body; free text is matched only against the
/// identifiers the bridge itself created.
pub fn names_sub_item(pr: &PullRequest, sub_item: &str) -> bool {
    let token = |text: &str| {
        text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
            .any(|w| w.eq_ignore_ascii_case(sub_item))
    };
    token(&pr.title) || token(&pr.body)
}

static COMPONENT_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^chore(\([^)]*\))?:\s*release\s+([a-z0-9][a-z0-9-]*)\s+v?(\d+\.\d+\.\d+)\s*$")
        .unwrap()
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentPr<'a> {
    pub component: String,
    pub version: Version,
    pub pr: &'a PullRequest,
}

/// The release PRs of a multi-component cycle (REQ-REL-076), in merge order:
/// the other components in declared order, the version component last. Any
/// anomaly refuses the whole cycle: nothing merges while one PR is unexplained.
pub fn component_prs<'a>(
    open: &'a [PullRequest],
    shape: &Shape,
    base: &str,
    authors: &[String],
    label: &str,
) -> Result<Vec<ComponentPr<'a>>, String> {
    let mut found: Vec<ComponentPr<'a>> = Vec::new();
    for pr in open.iter().filter(|p| p.base_ref == base) {
        let titled = COMPONENT_TITLE.is_match(pr.title.trim());
        if !(titled || is_release_please_shaped(pr, label)) {
            continue;
        }
        if !trusted_author(pr, authors) {
            continue;
        }
        let Some(c) = COMPONENT_TITLE.captures(pr.title.trim()) else {
            return Err(format!(
                "#{} has no readable component and version in its title",
                pr.number
            ));
        };
        let component = c[2].to_ascii_lowercase();
        let version = Version::parse(&c[3]).ok_or_else(|| {
            format!(
                "#{} has no readable component and version in its title",
                pr.number
            )
        })?;
        if let Some((_, branch_component)) = pr.head_ref.rsplit_once("--components--") {
            if !branch_component.eq_ignore_ascii_case(&component) {
                return Err(format!(
                    "#{} is titled for {component} but its branch is {branch_component}'s",
                    pr.number
                ));
            }
        }
        if !shape.components.contains(&component) {
            return Err(format!(
                "#{} releases {component}, which {} does not declare",
                pr.number, shape.product
            ));
        }
        if found.iter().any(|f| f.component == component) {
            return Err(format!("two release PRs are open for {component}"));
        }
        found.push(ComponentPr {
            component,
            version,
            pr,
        });
    }
    let index = |c: &str| {
        if Some(c) == shape.version_component.as_deref() {
            usize::MAX
        } else {
            shape
                .components
                .iter()
                .position(|x| x == c)
                .unwrap_or(usize::MAX - 1)
        }
    };
    found.sort_by_key(|f| index(&f.component));
    Ok(found)
}

/// One embarked component of a multi-component cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerEntry {
    pub component: String,
    pub version: String,
    pub pr: u64,
    pub deployed: bool,
}

static LEDGER_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\brelease (?P<component>[a-z0-9][a-z0-9-]*) v?(?P<version>\d+\.\d+\.\d+\S*)")
        .unwrap()
});

/// The deploy ledger from the PRs merged on the cycle's branch (REQ-REL-081):
/// an embarked component is a merged release PR (`autorelease:` label) of a
/// declared component; it is in production iff it carries `deployed:
/// production`. No embarked PR at all is "I do not know", never an empty ledger.
pub fn ledger(merged: &[PullRequest], shape: &Shape) -> Option<Vec<LedgerEntry>> {
    let mut out: Vec<LedgerEntry> = merged
        .iter()
        .filter(|p| p.labels.iter().any(|l| l.starts_with("autorelease:")))
        .filter_map(|p| {
            let c = LEDGER_TITLE.captures(&p.title)?;
            let component = c["component"].to_string();
            shape.components.contains(&component).then(|| LedgerEntry {
                component,
                version: c["version"].to_string(),
                pr: p.number,
                deployed: p.labels.iter().any(|l| l == DEPLOYED_LABEL),
            })
        })
        .collect();
    if out.is_empty() {
        return None;
    }
    out.sort_by_key(|e| e.pr);
    Some(out)
}

#[cfg(test)]
mod tests;
