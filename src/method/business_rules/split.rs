//! `spl-asdlc rules split`: a project's own one-file register of business
//! rules, split into the method's registry, one file per rule (ASDLC-T68W).
//! Deterministic: the same register always gives the same files, and the
//! split invents nothing.
//!
//! - A rule starts at a level-2 heading `## <ID>: <title>` (or `—`, `–`,
//!   `-` after the id) and runs to the next level-1 or level-2 heading.
//!   Text before the first rule (the register's preamble) is not a rule.
//! - Its text, `Where:` lines included (the code that keeps it true), is
//!   its `## Rule` section, as written.
//! - The tests it names in a sentence `Tests: …` are copied into its
//!   `## Test` section; with none, the section says « None cited in the
//!   register. ».
//! - Its status is the one its own `Status:` line gives (`Enforced` or
//!   `Proposed`), and `Proposed` when it gives none or its title says it was
//!   withdrawn (the method has no withdrawn status: the rule's own text says
//!   so, and its id stays taken).
//! - Its `## Evidence` says it was migrated from the register, whose history
//!   holds the rule's.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;

use super::{STATUS, parse_rule_file};

static RULE_HEADING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^##[ \t]+(BR-[A-Z]{2,6}-\d{2,4})[ \t]*(?::|[ \t][—–-])[ \t]*(\S.*?)[ \t]*$")
        .unwrap()
});
static TESTS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\bTests?:\s*(\S.*?)(?:\.(?:\s|$)|\z)").unwrap());
static WITHDRAWN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\(\s*withdrawn\b[^)]*\)").unwrap());

/// One rule file of the split: its name (`<ID>.md`) and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleOut {
    pub id: String,
    pub name: String,
    pub text: String,
}

/// The rule files of the register `text`, read from `register` (the path
/// the Evidence names), in the register's order. Refused when the register
/// has no rule, names an id twice, or a rule has no text of its own.
pub fn split(register: &str, text: &str) -> Result<Vec<RuleOut>, String> {
    let mut rules: Vec<(String, String, Vec<String>)> = Vec::new();
    // Whether the lines read belong to the last rule: another heading
    // closes it.
    let mut open = false;
    let mut fence = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = !fence;
        }
        if !fence {
            if let Some(c) = RULE_HEADING.captures(line) {
                rules.push((c[1].to_string(), c[2].to_string(), Vec::new()));
                open = true;
                continue;
            }
            if line.starts_with("# ") || line.starts_with("## ") {
                open = false;
                continue;
            }
        }
        if open {
            if let Some(last) = rules.last_mut() {
                last.2.push(line.to_string());
            }
        }
    }
    if rules.is_empty() {
        return Err(format!(
            "{register}: no rule heading `## BR-<PRODUCT>-<NNN>: <title>`"
        ));
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (id, title, lines) in rules {
        if !seen.insert(id.clone()) {
            return Err(format!("{register}: {id} is named twice"));
        }
        let mut status = None;
        let mut rule = Vec::new();
        for p in paragraphs(&lines) {
            match STATUS.captures(&p) {
                Some(c) if p.lines().count() == 1 => status = Some(c[1].to_string()),
                _ => rule.push(p),
            }
        }
        if rule.is_empty() {
            return Err(format!("{register}: {id} has no text"));
        }
        let status = match status.as_deref() {
            _ if WITHDRAWN.is_match(&title) => "Proposed",
            Some("Enforced") => "Enforced",
            Some("Proposed") | None => "Proposed",
            Some(other) => {
                return Err(format!(
                    "{register}: {id}: the status is `{other}`, not `Proposed` nor `Enforced`"
                ));
            }
        };
        let tests: Vec<String> = rule
            .iter()
            .flat_map(|p| TESTS.captures_iter(p).map(|c| format!("Tests: {}.", &c[1])))
            .collect();
        let test = if tests.is_empty() {
            "None cited in the register.".to_string()
        } else {
            tests.join("\n\n")
        };
        let text = format!(
            "# {id} — {title}\n\nStatus: {status}\n\n## Rule\n\n{}\n\n## Evidence\n\n\
             Migrated from `{register}` by `spl-asdlc rules split`: the rule's history is that \
             file's history.\n\n## Test\n\n{test}\n",
            rule.join("\n\n")
        );
        let name = format!("{id}.md");
        parse_rule_file(&name, &text).map_err(|e| format!("{register}: {id}: {e}"))?;
        out.push(RuleOut { id, name, text });
    }
    Ok(out)
}

/// The paragraphs of `lines`, each line trimmed of its trailing blanks.
fn paragraphs(lines: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            if !current.is_empty() {
                out.push(current.join("\n"));
                current.clear();
            }
        } else {
            // A `Where:` line starts its own paragraph, blank line or not.
            if line.starts_with("Where:") && !current.is_empty() {
                out.push(current.join("\n"));
                current.clear();
            }
            current.push(line.trim_end());
        }
    }
    if !current.is_empty() {
        out.push(current.join("\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGISTER: &str = "# Business rules

Rules that the product must keep true. A number is never reused.

## BR-AC-001: Public endpoint allowlist

The public endpoint rejects any source outside the allowlist; it never
fails open.

Status: Enforced

Where: `src/allowlist.rs`. Tests: `allowlist::tests::an_unknown_source_is_refused`. SID-1.

## BR-AC-002 — Signed releases

A published binary is always signed.
Where: `src/sign.rs`.

## BR-AC-003: The source marks what went through triage (withdrawn)

**Withdrawn on 2026-10-23, replaced by BR-AC-001.**

Where: `src/store.rs`.
";

    #[test]
    fn each_rule_becomes_a_valid_file_and_its_tests_are_copied() {
        let out = split("docs/business-rules.md", REGISTER).unwrap();
        assert_eq!(
            out.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["BR-AC-001.md", "BR-AC-002.md", "BR-AC-003.md"]
        );
        assert_eq!(
            out[0].text,
            "# BR-AC-001 — Public endpoint allowlist

Status: Enforced

## Rule

The public endpoint rejects any source outside the allowlist; it never
fails open.

Where: `src/allowlist.rs`. Tests: `allowlist::tests::an_unknown_source_is_refused`. SID-1.

## Evidence

Migrated from `docs/business-rules.md` by `spl-asdlc rules split`: the rule's history is that file's history.

## Test

Tests: `allowlist::tests::an_unknown_source_is_refused`.
"
        );
        for r in &out {
            assert!(parse_rule_file(&r.name, &r.text).is_ok(), "{}", r.text);
        }
    }

    #[test]
    fn the_status_is_the_registers_and_proposed_when_it_gives_none_or_withdrew_it() {
        let out = split("r.md", REGISTER).unwrap();
        assert!(out[1].text.contains("Status: Proposed"));
        assert!(
            out[1]
                .text
                .contains("## Test\n\nNone cited in the register.\n")
        );
        assert!(out[1].text.contains("Where: `src/sign.rs`."));
        assert!(out[2].text.contains("Status: Proposed"));
        assert!(out[2].text.contains("**Withdrawn on 2026-10-23"));
        let bad = "## BR-AC-001: A\n\nStatus: Retired\n\nText.\n";
        assert!(split("r.md", bad).is_err());
    }

    #[test]
    fn the_split_is_deterministic_and_refuses_what_it_cannot_read() {
        assert_eq!(split("r.md", REGISTER), split("r.md", REGISTER));
        assert!(split("r.md", "# Rules\n\nNone yet.\n").is_err());
        let twice = "## BR-AC-001: A\n\nText.\n\n## BR-AC-001: B\n\nText.\n";
        assert!(split("r.md", twice).unwrap_err().contains("twice"));
        assert!(split("r.md", "## BR-AC-001: A\n\nStatus: Enforced\n").is_err());
    }
}
