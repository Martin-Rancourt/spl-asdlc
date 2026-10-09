//! The ticket contract (`docs/spec/01-ticket-contract.md`): the headings
//! that make it, the refinement gate (`ticket check`), the zero-boxes guard
//! and the "requires deployment" reading.

use std::sync::LazyLock;

use regex::Regex;

use super::markdown::{self, Checkbox};

static GOAL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:goal|objectif|but)$").unwrap());
static ACCEPTANCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)acceptance\s+criteria\b|crit[eè]res?\s+d[e'’]\s*acceptation\b").unwrap()
});
static BUSINESS_RULES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^[*_\s]*(?:business\s+rules?|r[eè]gles?\s+m[eé]tiers?)").unwrap()
});
static POST_DEPLOY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:(?:QA[ \t]+)?post[- ]?release|post[- ]?deploy)[ \t]*:?[ \t]*$").unwrap()
});
static NON_GOAL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^non[- ]?goals?$").unwrap());
static RELEASE_NOTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^release[ \t]+note$").unwrap());

/// `## Goal` (also `Objectif`, `But`): the whole heading text.
pub fn is_goal_heading(text: &str) -> bool {
    GOAL.is_match(text.trim())
}

/// `## Acceptance criteria` (also `Critères d'acceptation`).
pub fn is_acceptance_heading(text: &str) -> bool {
    ACCEPTANCE.is_match(text.trim())
}

/// `## Business rules`, read by what the heading starts with: `Business
/// rules (ABC-617)`, `**Business rules**`, `Business rules:` and the French
/// `Règles métier` are the same section; `Business rulesets` is not.
pub fn is_business_rules_heading(text: &str) -> bool {
    let text = text.trim();
    match BUSINESS_RULES.find(text) {
        Some(m) => !text[m.end()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-'),
        None => false,
    }
}

/// `## Post deploy` (and the older `post-release`, `QA post-release`, …).
pub fn is_post_deploy_heading(text: &str) -> bool {
    POST_DEPLOY.is_match(text.trim())
}

pub fn is_non_goal_heading(text: &str) -> bool {
    NON_GOAL.is_match(text.trim())
}

pub fn is_release_note_heading(text: &str) -> bool {
    RELEASE_NOTE.is_match(text.trim())
}

/// A ticket "has a contract" iff `ticket check` finds nothing missing: one
/// judgement, never two readings (spec 01, REQ-MERGE-032).
pub fn has_contract(description: &str) -> bool {
    missing_from_contract(description).is_empty()
}

/// The judgement of `spl-asdlc ticket check` (spec 01, the refinement gate):
/// what is missing for the contract to be complete. Empty means complete.
/// It reads headings and checkboxes, never the quality of the prose.
pub fn missing_from_contract(description: &str) -> Vec<String> {
    let lines = markdown::without_fences(description);
    let mut missing = Vec::new();
    let goals = markdown::sections_of(&lines, is_goal_heading);
    if goals.is_empty() {
        missing.push("no `## Goal` section".to_string());
    } else if goals.iter().all(|s| s.body().trim().is_empty()) {
        missing.push("the `## Goal` section is empty".to_string());
    }
    let acs = markdown::sections_of(&lines, is_acceptance_heading);
    if acs.is_empty() {
        missing.push("no `## Acceptance criteria` section".to_string());
    } else if acs
        .iter()
        .all(|s| markdown::checkboxes(&s.body()).is_empty())
    {
        missing.push(
            "no acceptance-criterion checkbox (`- [ ] …`) under `## Acceptance criteria`"
                .to_string(),
        );
    }
    let rules = markdown::sections_of(&lines, is_business_rules_heading);
    if rules.is_empty() {
        missing.push("no `## Business rules` section".to_string());
    } else if rules.iter().all(|s| s.body().trim().is_empty()) {
        missing.push(format!(
            "the `## Business rules` section is empty: an empty section is not a declaration (write `{}` when there is none)",
            super::business_rules::ABSENCE_DECLARATION
        ));
    }
    missing
}

/// The zero-boxes guard (REQ-MERGE-031): the ticket declares boxes and
/// checks none of them. A ticket with no box at all does not trigger it.
pub fn declares_nothing(description: &str) -> bool {
    let (checked, open) = markdown::checklist(description);
    !open.is_empty() && checked.is_empty()
}

/// Boxes of the `## Post deploy` section(s).
pub fn post_deploy_boxes(description: &str) -> Vec<Checkbox> {
    markdown::checkboxes_in_sections(description, is_post_deploy_heading)
}

/// The completeness rule (spec 04 §10, BR-ASDLC-008): the open boxes of
/// the ticket's own description outside its `## Post deploy` section(s),
/// under any heading (spec 01 §3). `None` when it declares no box there:
/// nothing declared is not everything checked.
pub fn open_before_deploy(description: &str) -> Option<Vec<String>> {
    let after: std::collections::BTreeSet<usize> = post_deploy_boxes(description)
        .iter()
        .map(|b| b.line_no)
        .collect();
    let boxes: Vec<Checkbox> = markdown::checkboxes(description)
        .into_iter()
        .filter(|b| !after.contains(&b.line_no))
        .collect();
    if boxes.is_empty() {
        return None;
    }
    Some(
        boxes
            .into_iter()
            .filter(|b| !b.checked)
            .map(|b| b.line)
            .collect(),
    )
}

/// The text of the `## Release note` section (spec 01 §6), trimmed; empty
/// when the section is absent or empty.
pub fn release_note_snippet(description: &str) -> String {
    markdown::sections(description, is_release_note_heading)
        .iter()
        .map(|s| s.body())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// The first paragraph of the Goal section (REQ-REL-093).
pub fn goal_summary(description: &str, max: usize) -> String {
    let goal = markdown::sections(description, is_goal_heading)
        .first()
        .map(|s| s.body())
        .unwrap_or_default();
    markdown::first_paragraph(&goal, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMPLETE: &str = "## Goal\nThe thing works.\n\n## Non-goals\n- not that\n\n## Acceptance criteria\n- [ ] it works\n\n## Business rules\nNone.\n\n## Release note\nYou can now do the thing.\n";

    // ---- REQ-MERGE-032 ----

    #[test]
    fn a_feature_without_a_readable_contract_is_never_served() {
        for text in [
            "## Goal\nx\n",                                           // goal without criteria
            "## Acceptance criteria\n- [ ] a\n",                      // criteria without a goal
            "## Goal\nx\n## Acceptance criteria\n- a plain bullet\n", // criteria as bullets
            "no contract at all",
        ] {
            assert!(!has_contract(text), "{text:?}");
        }
    }

    #[test]
    fn a_feature_with_a_goal_and_a_checkbox_criterion_is_served() {
        assert!(has_contract(COMPLETE));
        assert!(has_contract(
            "# Objectif\nx\n### Critères d'acceptation\n- [x] fait\n## Règles métier\nAucune.\n"
        ));
        assert!(has_contract(
            "## But\nx\n## Critères d’acceptation\n- [ ] a\n## Business rules\nNone.\n"
        ));
        assert!(
            !has_contract("## Goal\nx\n## Acceptance criteria\n- [ ] a\n"),
            "the business-rules section is part of the contract (spec 01)"
        );
    }

    #[test]
    fn a_bold_pseudo_heading_is_not_a_heading() {
        assert!(!has_contract(
            "## Goal\nx\n**Acceptance criteria**\n- [ ] a\n## Business rules\nNone.\n"
        ));
    }

    #[test]
    fn criteria_of_another_kind_are_not_acceptance_criteria() {
        assert!(!is_acceptance_heading("Critères de performance"));
        assert!(is_acceptance_heading("Acceptance criteria (draft)"));
    }

    // ---- ticket check (spec 01 refinement gate) ----

    #[test]
    fn ticket_check_says_complete_or_lists_what_is_missing() {
        assert!(missing_from_contract(COMPLETE).is_empty());
        let missing = missing_from_contract("## Goal\nx\n## Acceptance criteria\n- a\n");
        assert_eq!(missing.len(), 2, "{missing:?}");
        assert!(missing[0].contains("no acceptance-criterion checkbox"));
        assert!(missing[1].contains("no `## Business rules` section"));
        let all = missing_from_contract("");
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn an_empty_business_rules_section_is_not_a_declaration() {
        let text = COMPLETE.replace("## Business rules\nNone.\n", "## Business rules\n\n");
        let missing = missing_from_contract(&text);
        assert_eq!(missing.len(), 1);
        assert!(missing[0].contains("an empty section is not a declaration"));
    }

    // ---- REQ-MERGE-031 ----

    #[test]
    fn a_ticket_that_checks_no_box_declares_nothing() {
        assert!(declares_nothing("- [ ] a\n- [ ] b\n"));
        assert!(
            !declares_nothing("- [x] a\n- [ ] b\n"),
            "one checked box is enough"
        );
        assert!(
            !declares_nothing("no box at all"),
            "the contract judgement's business"
        );
    }

    // ---- REQ-MERGE-034 headings ----

    #[test]
    fn the_section_is_found_by_what_its_title_starts_with() {
        for t in [
            "Business rules",
            "Business rules (ABC-617)",
            "**Business rules**",
            "Business rules:",
            "Business Rule",
            "Règles métier",
            "Regles metier",
        ] {
            assert!(is_business_rules_heading(t), "{t}");
        }
    }

    #[test]
    fn a_title_that_only_shares_a_prefix_is_not_the_section() {
        assert!(!is_business_rules_heading("Business rulesets"));
        assert!(!is_business_rules_heading("Business rules-engine"));
        assert!(!is_business_rules_heading("Our business rules"));
    }

    // ---- REQ-MERGE-067 ----

    #[test]
    fn the_post_release_section_is_found_at_any_depth_and_spelling() {
        for t in [
            "Post deploy",
            "post-deploy",
            "postdeploy",
            "Post Deploy :",
            "post-release",
            "Post-Release",
            "post release",
            "postrelease",
            "QA post-release",
            "QA post release",
        ] {
            assert!(is_post_deploy_heading(t), "{t}");
        }
    }

    #[test]
    fn a_heading_that_is_not_the_section_is_not_the_section() {
        for t in [
            "Vérification post-release",
            "QA",
            "QA post-release notes",
            "Post deploy notes",
            "Post deployment",
            "QA post deploy",
        ] {
            assert!(!is_post_deploy_heading(t), "{t}");
        }
    }

    #[test]
    fn post_release_checklist_items_reads_only_the_section() {
        let text = "## Acceptance criteria\n- [ ] before\n## Post deploy\n- [ ] after\n- [x] done after\n### detail\n- [ ] nested\n## Other\n- [ ] other\n";
        let boxes = post_deploy_boxes(text);
        assert_eq!(
            boxes.iter().map(|b| b.line.as_str()).collect::<Vec<_>>(),
            vec!["- [ ] after", "- [x] done after", "- [ ] nested"]
        );
    }

    // ---- spec 04 §10: the completeness rule ----

    #[test]
    fn the_boxes_before_deploy_set_the_post_deploy_section_apart() {
        let text =
            "## Acceptance criteria\n- [x] a\n- [ ] b\n## Post deploy\n- [ ] in production\n";
        assert_eq!(open_before_deploy(text), Some(vec!["- [ ] b".to_string()]));
        let done = text.replace("- [ ] b", "- [x] b");
        assert_eq!(open_before_deploy(&done), Some(Vec::new()));
        assert_eq!(
            open_before_deploy("## Post deploy\n- [ ] in production\n"),
            None,
            "no box before deploy: nothing declared"
        );
        assert_eq!(
            open_before_deploy("```\n- [ ] an example\n```\n- [x] a\n"),
            Some(Vec::new()),
            "a fenced box is an example"
        );
    }

    // ---- release note snippet ----

    #[test]
    fn the_release_note_snippet_and_goal_summary_are_read_from_their_sections() {
        assert_eq!(release_note_snippet(COMPLETE), "You can now do the thing.");
        assert_eq!(release_note_snippet("## Goal\nx"), "");
        assert_eq!(goal_summary(COMPLETE, 280), "The thing works.");
    }
}
