//! The business-rule guard (`docs/reference/02-business-rules.md`,
//! REQ-MERGE-034..043, REQ-REL-088): the rules a ticket cites, the rule it
//! states, and the judgement against the registry read at a ref. The
//! registry is a directory, one file per rule (`<ID>.md`): the guard reads
//! its listing, `doctor` and the tests read and check every file.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;

use super::contract::is_business_rules_heading;
use super::markdown;
use crate::github::{GitHub, RepoSlug};

/// The sentence the refusal tells an author to write when a ticket has no
/// new rule; the reader accepts it.
pub const ABSENCE_DECLARATION: &str = "No new rule.";

/// The registry: one file per rule, `<ID>.md`, in this directory of the
/// repository. Fixed by the method, never configured (spec 02).
pub const REGISTRY_DIR: &str = ".spl-asdlc/business-rules";

/// The one file of the registry's directory that is not a rule: an
/// optional index, which the method never reads.
const INDEX: &str = "README.md";

static RULE_ID: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bBR-[A-Z]{2,6}-\d{2,4}\b").unwrap());
static RULE_FILE_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(BR-[A-Z]{2,6}-\d{2,4})\.md$").unwrap());
static TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^#[ \t]+(BR-[A-Z]{2,6}-\d{2,4})[ \t]+[—–-][ \t]+(\S.*?)[ \t]*$").unwrap()
});
static STATUS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^(?:\*\*)?Status(?:\*\*)?[ \t]*:[ \t]*(?:\*\*)?[ \t]*(\S+?)(?:\*\*)?[ \t]*$")
        .unwrap()
});
static THEMATIC_BREAK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:(?:-[ \t]*){3,}|(?:\*[ \t]*){3,}|(?:_[ \t]*){3,})$").unwrap()
});
static NEW_ITEM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:[-*+]\s+|\d+[.)]\s+|#+\s|>\s|\|)").unwrap());
static LIST_MARKER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:[-*+]|\d+[.)])\s*").unwrap());
static CHECKBOX_MARK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[[ xX]\]\s*").unwrap());
static NOISE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:[-*+]\s*|\d+[.)]\s*|\|[\s:|-]*\||#+\s|>\s*)?$").unwrap());
static DECORATION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[\W_]+|[\W_]+$").unwrap());
static ABSENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:aucune?(?:\s+(?:nouvel(?:les?)?|nouveaux?|r[eè]gles?|m[eé]tiers?))*|no(?:\s+(?:new|business|rules?))+|none|n/?a|n[eé]ant|sans\s+objet)$",
    )
    .unwrap()
});

/// Every rule id in a text (`BR-<2-6 capitals>-<2-4 digits>`).
pub fn rule_ids(text: &str) -> BTreeSet<String> {
    RULE_ID
        .find_iter(text)
        .map(|m| m.as_str().to_string())
        .collect()
}

/// `BR-ACME-151` -> `BR-ACME`.
pub fn rule_prefix(id: &str) -> &str {
    id.rsplit_once('-').map(|(p, _)| p).unwrap_or(id)
}

fn rules_bodies(description: &str) -> Vec<String> {
    markdown::sections(description, is_business_rules_heading)
        .iter()
        .map(|s| s.body())
        .collect()
}

/// The rule ids a ticket cites in its Business rules section(s).
pub fn cited_ids(description: &str) -> BTreeSet<String> {
    rules_bodies(description)
        .iter()
        .flat_map(|b| rule_ids(b))
        .collect()
}

/// The statements of a Business rules section: units of meaning, not lines
/// (REQ-MERGE-036). A blank line or a thematic break closes a statement; a
/// line that starts a list item, a heading, a quote or a table row starts a
/// new one; any other line continues the current one. A nested bullet is its
/// own statement, never its parent's continuation.
pub fn section_items(body: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current: Option<String> = None;
    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() || THEMATIC_BREAK.is_match(line) {
            items.extend(current.take());
            continue;
        }
        if NEW_ITEM.is_match(line) {
            items.extend(current.take());
            current = Some(line.to_string());
        } else {
            current = Some(match current.take() {
                Some(c) => format!("{c} {line}"),
                None => line.to_string(),
            });
        }
    }
    items.extend(current);
    items
}

/// A statement whose entire content says there is no rule (REQ-MERGE-037).
pub fn declares_absence(statement: &str) -> bool {
    let core = DECORATION.replace_all(statement.trim(), "");
    let core = core.split_whitespace().collect::<Vec<_>>().join(" ");
    ABSENCE.is_match(&core)
}

/// The first statement of the Business rules section(s) that states a new
/// rule: not noise, not an absence declaration, citing no rule id.
pub fn new_rule_statement(description: &str) -> Option<String> {
    for body in rules_bodies(description) {
        for item in section_items(&body) {
            let stripped = LIST_MARKER.replace(&item, "");
            let stripped = CHECKBOX_MARK.replace(&stripped, "").trim().to_string();
            if stripped.is_empty() || NOISE.is_match(&stripped) || NOISE.is_match(&item) {
                continue;
            }
            if declares_absence(&stripped) || RULE_ID.is_match(&stripped) {
                continue;
            }
            return Some(stripped);
        }
    }
    None
}

/// The id a rule file's name declares: `BR-ACME-001.md` (or a path ending
/// in it) -> `BR-ACME-001`; any other name declares none.
pub fn rule_file_id(name: &str) -> Option<&str> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    RULE_FILE_NAME
        .captures(base)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
}

/// Whether a repository path is a rule file of the registry: directly in
/// `REGISTRY_DIR`, named `<ID>.md`. The index is not one.
pub fn is_rule_file(path: &str) -> bool {
    let path = path.replace('\\', "/");
    path.strip_prefix(REGISTRY_DIR)
        .and_then(|rest| rest.strip_prefix('/'))
        .is_some_and(|name| !name.contains('/') && rule_file_id(name).is_some())
}

/// The ids a registry declares: the names of its rule files, as the
/// directory lists them. Any other name declares nothing.
pub fn registry_ids<S: AsRef<str>>(names: &[S]) -> BTreeSet<String> {
    names
        .iter()
        .filter_map(|n| rule_file_id(n.as_ref()))
        .map(str::to_string)
        .collect()
}

/// A rule's status (spec 02 §3): only an `Enforced` rule blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleStatus {
    Proposed,
    Enforced,
}

/// One rule file, read and checked (spec 02, "The rule file").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleFile {
    pub id: String,
    /// The rule in one sentence: the title line after its id.
    pub title: String,
    pub status: RuleStatus,
}

/// The sections every rule file carries, beside its title and status. The
/// test section may be written `## Tests`.
const FIELDS: [&str; 3] = ["Rule", "Evidence", "Test"];

fn field_heading(field: &str, heading: &str) -> bool {
    let heading = heading.trim();
    heading.eq_ignore_ascii_case(field)
        || (field == "Test" && heading.eq_ignore_ascii_case("Tests"))
}

/// Read one rule file: `name` is its file name (or path), `text` its
/// content. The error says what is wrong, for a person to fix.
pub fn parse_rule_file(name: &str, text: &str) -> Result<RuleFile, String> {
    let id = rule_file_id(name)
        .ok_or_else(|| "the file name is not `<ID>.md` (`BR-<PRODUCT>-<NNN>.md`)".to_string())?;
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let title = TITLE
        .captures(first.trim_end())
        .ok_or_else(|| format!("the first line is not the title `# {id} — <the rule>`"))?;
    if &title[1] != id {
        return Err(format!(
            "the title names `{}`, the file name `{id}`",
            &title[1]
        ));
    }
    let lines = markdown::without_fences(text);
    let status = match lines
        .iter()
        .find_map(|l| STATUS.captures(l).map(|c| c[1].to_string()))
        .as_deref()
    {
        Some("Enforced") => RuleStatus::Enforced,
        Some("Proposed") => RuleStatus::Proposed,
        Some(other) => {
            return Err(format!(
                "the status is `{other}`, not `Proposed` nor `Enforced`"
            ));
        }
        None => return Err("no `Status: Proposed` or `Status: Enforced` line".to_string()),
    };
    for field in FIELDS {
        let sections = markdown::sections_of(&lines, |h| field_heading(field, h));
        match sections.first() {
            None => return Err(format!("no `## {field}` section")),
            Some(s) if s.body().trim().is_empty() => {
                return Err(format!("the `## {field}` section is empty"));
            }
            Some(_) => {}
        }
    }
    Ok(RuleFile {
        id: id.to_string(),
        title: title[2].to_string(),
        status,
    })
}

/// The body of a rule file's `## Test` (or `## Tests`) section, fences
/// blanked; empty when it has none.
pub fn test_section(text: &str) -> String {
    let lines = markdown::without_fences(text);
    markdown::sections_of(&lines, |h| field_heading("Test", h))
        .first()
        .map(|s| s.body())
        .unwrap_or_default()
}

/// The registry's files, read and checked: the rules, and every file that
/// is not a valid rule with its path and why. The index (`README.md`) and
/// dot files are skipped.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RegistryFiles {
    pub rules: Vec<RuleFile>,
    pub invalid: Vec<(String, String)>,
}

/// Check every file of the registry's directory: `files` are `(path,
/// text)`, each path ending in the file's name.
pub fn parse_registry(files: &[(String, String)]) -> RegistryFiles {
    let mut out = RegistryFiles::default();
    for (path, text) in files {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        if name == INDEX || name.starts_with('.') {
            continue;
        }
        match parse_rule_file(name, text) {
            Ok(rule) => out.rules.push(rule),
            Err(why) => out.invalid.push((path.clone(), why)),
        }
    }
    out
}

/// The registry as read at the judged ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Registry {
    /// The directory is absent at a ref that exists. The guard still holds
    /// (`docs/reference/02-business-rules.md` §4-5): a cited id must exist, so
    /// none does; a stated rule must reach the registry, so its PR creates it.
    Absent,
    Ids(BTreeSet<String>),
    /// The read failed; nothing is judged (fail closed).
    Unreadable(String),
}

/// Read the registry at `rev` through the Git host, never from a working
/// tree (spec 02 §5): one listing of `REGISTRY_DIR`. A 404 is `Absent` only
/// at a revision that exists; any other failure is `Unreadable`.
pub fn read_registry(github: &dyn GitHub, repo: &RepoSlug, rev: &str) -> Registry {
    match github.dir(repo, REGISTRY_DIR, rev) {
        Ok(names) => Registry::Ids(registry_ids(&names)),
        Err(e) if e.is_not_found() => match github.commit(repo, rev) {
            Ok(_) => Registry::Absent,
            Err(e) => Registry::Unreadable(e.to_string()),
        },
        Err(e) => Registry::Unreadable(e.to_string()),
    }
}

/// The guard's judgement of one ticket.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuleGate {
    /// Cited ids missing from the registry (of the prefixes it owns), sorted.
    pub unknown_ids: Vec<String>,
    /// The ticket states a rule the PR does not add to the registry.
    pub unpromoted: bool,
    /// The statement read as a new rule, quoted only when unpromoted.
    pub statement: Option<String>,
    /// Why the registry could not be read, when it could not.
    pub unreadable: Option<String>,
    pub registry_present: bool,
}

impl RuleGate {
    pub fn ok(&self) -> bool {
        self.unknown_ids.is_empty() && !self.unpromoted && self.unreadable.is_none()
    }
}

/// Whether judging this ticket needs the registry at all: it cites an id, or
/// it states a rule and the changed paths are known (REQ-MERGE-041 (2)).
pub fn needs_registry(description: &str, changed_paths: Option<&[String]>) -> bool {
    !cited_ids(description).is_empty()
        || (changed_paths.is_some() && new_rule_statement(description).is_some())
}

/// Judge a ticket against the registry (REQ-MERGE-039). `changed_paths`
/// `None` skips the promotion half (the diff could not be read); unknown
/// ids are still caught.
pub fn evaluate(
    description: &str,
    registry: &Registry,
    changed_paths: Option<&[String]>,
) -> RuleGate {
    let empty = BTreeSet::new();
    let ids = match registry {
        Registry::Absent => &empty,
        Registry::Unreadable(why) => {
            return RuleGate {
                unreadable: Some(why.clone()),
                registry_present: true,
                ..RuleGate::default()
            };
        }
        Registry::Ids(ids) => ids,
    };
    let owned: BTreeSet<&str> = ids.iter().map(|i| rule_prefix(i)).collect();
    let unknown_ids: Vec<String> = cited_ids(description)
        .into_iter()
        .filter(|id| !ids.contains(id))
        .filter(|id| owned.is_empty() || owned.contains(rule_prefix(id)))
        .collect();
    let mut gate = RuleGate {
        unknown_ids,
        registry_present: !matches!(registry, Registry::Absent),
        ..RuleGate::default()
    };
    if let Some(paths) = changed_paths {
        if let Some(statement) = new_rule_statement(description) {
            // A rule file added or changed; the index alone promotes nothing.
            if !paths.iter().any(|p| is_rule_file(p)) {
                gate.unpromoted = true;
                gate.statement = Some(statement);
            }
        }
    }
    gate
}

fn clean_cause(cause: &str) -> String {
    cause
        .replace('`', "'")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The refusal written to the ticket, once per head SHA (REQ-MERGE-042).
pub fn refusal(gate: &RuleGate, ticket: &str) -> String {
    if let Some(cause) = &gate.unreadable {
        return format!(
            "Business rules: the registry could not be read, this PR waits ({ticket}).\n\n\
             **Registry unreadable**: reading `{REGISTRY_DIR}/` at the PR's revision failed (`{}`). \
             No rule was judged, and nothing needs fixing in the ticket or the PR: the next tick reads \
             the registry again and the PR resumes as soon as the read succeeds.\n\n\
             The registry is reviewed by a human: this message states a fact, it writes nothing.",
            clean_cause(cause)
        );
    }
    let mut out =
        format!("Business rules: this change cannot be merged as it stands ({ticket}).\n");
    if !gate.unknown_ids.is_empty() {
        let ids = gate
            .unknown_ids
            .iter()
            .map(|i| format!("`{i}`"))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "\n**Cited rule not in the registry**: {ids}. A referenced rule has its file in `{REGISTRY_DIR}/` (`<ID>.md`) — \
             either the id is a typo, or the rule is actually new and its file is added in this PR, \
             with that id, its evidence and its named tests.\n"
        ));
    }
    if gate.unpromoted {
        if let Some(s) = &gate.statement {
            out.push_str(&format!("\n> {s}\n"));
        }
        out.push_str(&format!(
            "\n**New rule not promoted**: the sentence above, in the ticket's `Business rules` section, \
             states a rule that cites no `BR-XX-NNN`, and this PR adds no rule file to `{REGISTRY_DIR}/`. \
             A rule born in a ticket outlives the ticket: its file `<ID>.md` is added **in the same PR** \
             as the change that introduces it, with a stable id, its evidence and its named tests — like \
             a red/green test. If the line is not a rule but a scope boundary, it belongs in `Non-goals`.\n\n\
             **If the ticket really has no new rule**, say so: write only « {ABSENCE_DECLARATION} » — the \
             whole sentence, nothing else. A sentence that starts with “No” or “None” but states something \
             else remains a rule.\n"
        ));
    }
    out.push_str(
        "\nThe registry is reviewed by a human: this message states a fact, it writes nothing.",
    );
    out
}

/// The skip reason of a PR the guard holds (REQ-MERGE-041).
pub fn skip_reason(gate: &RuleGate) -> String {
    if let Some(cause) = &gate.unreadable {
        return format!(
            "the business-rule registry could not be read ({cause}) — nothing is judged, the next tick reads it again"
        );
    }
    let mut reasons = Vec::new();
    if !gate.unknown_ids.is_empty() {
        reasons.push(format!(
            "cites a business rule that is not in the registry: {}",
            gate.unknown_ids.join(", ")
        ));
    }
    if gate.unpromoted {
        reasons.push("declares a business rule the PR does not add to the registry".to_string());
    }
    reasons.join("; ")
}

#[cfg(test)]
mod tests;
