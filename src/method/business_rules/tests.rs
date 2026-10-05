//! Carried from `tests/test_business_rules_guard.py` (merge.md).

use super::*;

const REGISTRY: &str = "| ID | Rule | Evidence | Test | Status |\n|---|---|---|---|---|\n| BR-AF-001 | **one** | e | t | Enforced |\n| BR-AF-004 | **four** see BR-HC-151 | e | t | Enforced |\n";

fn ids(list: &[&str]) -> Registry {
    Registry::Ids(list.iter().map(|s| s.to_string()).collect())
}

fn registry() -> Registry {
    Registry::Ids(registry_ids(REGISTRY))
}

fn ticket(rules: &str) -> String {
    format!("## Goal\nx\n\n## Acceptance criteria\n- [ ] a\n\n## Business rules\n{rules}\n")
}

fn paths(p: &[&str]) -> Vec<String> {
    p.iter().map(|s| s.to_string()).collect()
}

const REG: &str = "docs/business-rules.md";

fn judge(rules: &str, changed: &[&str]) -> RuleGate {
    let changed = paths(changed);
    evaluate(&ticket(rules), &registry(), Some(&changed), REG)
}

#[test]
fn a_referenced_rule_must_exist_in_the_registry() {
    let gate = judge("- BR-AF-999 applies", &["src/a.rs"]);
    assert_eq!(gate.unknown_ids, vec!["BR-AF-999"]);
    assert!(!gate.ok());
    let text = refusal(&gate, "ABC-1", REG);
    assert!(text.contains("`BR-AF-999`"));
    assert!(text.contains("typo"));
}

#[test]
fn a_rule_born_in_the_ticket_must_reach_the_registry_in_the_same_pr() {
    let gate = judge("- Every export is signed.", &["src/export.rs"]);
    assert!(gate.unpromoted);
    let text = refusal(&gate, "ABC-1", REG);
    assert!(text.contains("**in the same PR**"));
    assert!(text.contains("Non-goals"));
    let promoted = judge(
        "- Every export is signed.",
        &["src/export.rs", "docs/business-rules.md"],
    );
    assert!(promoted.ok());
}

#[test]
fn a_rule_promoted_and_cited_by_its_new_id_in_the_same_pr_passes() {
    let head_registry = format!("{REGISTRY}| BR-AF-005 | **new** | e | t | Proposed |\n");
    let changed = paths(&["docs/business-rules.md"]);
    let gate = evaluate(
        &ticket("- BR-AF-005: every export is signed."),
        &Registry::Ids(registry_ids(&head_registry)),
        Some(&changed),
        REG,
    );
    assert!(gate.ok(), "{gate:?}");
}

#[test]
fn citing_only_existing_rules_passes() {
    let gate = judge("- BR-AF-001\n- BR-AF-004", &["src/a.rs"]);
    assert!(gate.ok());
    assert!(gate.registry_present);
}

#[test]
fn a_product_without_a_registry_is_not_refused() {
    let changed = paths(&["src/a.rs"]);
    let gate = evaluate(
        &ticket("- Every export is signed."),
        &Registry::Absent,
        Some(&changed),
        REG,
    );
    assert!(gate.ok());
    assert!(!gate.registry_present);
}

#[test]
fn a_ticket_with_no_business_rules_section_is_not_refused() {
    let changed = paths(&["src/a.rs"]);
    let gate = evaluate(
        "## Goal\nx\nBR-AF-999 in prose",
        &registry(),
        Some(&changed),
        REG,
    );
    assert!(gate.ok());
}

#[test]
fn the_section_is_read_exactly_as_the_gates_read_a_section() {
    let text = "## Goal\nBR-AF-901 outside\n## Business rules\n```\nBR-AF-902 fenced\n```\n### Detail\n- BR-AF-903 under a sub-heading\n## Next\nBR-AF-904 after\n";
    assert_eq!(
        cited_ids(text),
        ["BR-AF-903".to_string()].into_iter().collect()
    );
}

#[test]
fn a_heading_or_a_table_row_is_not_a_new_rule() {
    assert_eq!(new_rule_statement(&ticket("|---|---|")), None);
    assert_eq!(
        new_rule_statement(&ticket("- Every export is signed.")),
        Some("Every export is signed.".into())
    );
}

#[test]
fn no_diff_to_read_skips_the_promotion_half_but_still_checks_ids() {
    let gate = evaluate(
        &ticket("- BR-AF-999\n- A new rule."),
        &registry(),
        None,
        REG,
    );
    assert_eq!(gate.unknown_ids, vec!["BR-AF-999"]);
    assert!(!gate.unpromoted);
}

#[test]
fn the_registry_of_this_repo_parses_into_its_real_ids() {
    let real = include_str!("../../../docs/business-rules.md");
    let parsed = registry_ids(real);
    assert!(parsed.contains("BR-ASDLC-001"), "{parsed:?}");
    assert!(duplicate_registry_ids(real).is_empty());
}

#[test]
fn this_repos_registry_owns_only_its_own_prefix() {
    let real = include_str!("../../../docs/business-rules.md");
    let ids = registry_ids(real);
    let prefixes: BTreeSet<&str> = ids.iter().map(|i| rule_prefix(i)).collect();
    assert_eq!(prefixes.into_iter().collect::<Vec<_>>(), vec!["BR-ASDLC"]);
}

#[test]
fn a_cited_rule_wrapped_over_two_lines_is_not_a_new_rule() {
    let rules = "- BR-AF-001: the first rule, whose sentence\n  wraps onto a second line.\n- BR-AF-004: another one\n  also wrapped.";
    let gate = judge(rules, &["src/a.rs"]);
    assert!(gate.ok(), "{gate:?}");
}

#[test]
fn a_continuation_left_at_column_zero_still_belongs_to_its_rule() {
    let rules = "- BR-AF-001: a rule whose sentence\ncontinues at column zero.";
    assert_eq!(new_rule_statement(&ticket(rules)), None);
}

#[test]
fn a_thematic_break_is_punctuation_not_a_rule() {
    for brk in ["---", "***", "___", "-----", "- - -", "* * *", "_ _ _"] {
        let rules = format!("- BR-AF-001\n{brk}\n- BR-AF-004");
        assert_eq!(new_rule_statement(&ticket(&rules)), None, "{brk}");
    }
}

#[test]
fn a_wrapped_rule_without_an_id_is_still_refused() {
    let rules = "- BR-AF-001: cited.\n- Exports are signed\n  before they leave the box.";
    let gate = judge(rules, &["src/a.rs"]);
    assert!(gate.unpromoted);
    assert_eq!(
        gate.statement.as_deref(),
        Some("Exports are signed before they leave the box.")
    );
}

#[test]
fn the_refusal_quotes_the_sentence_it_read_as_a_new_rule() {
    let gate = judge("- Exports are signed.", &["src/a.rs"]);
    let text = refusal(&gate, "ABC-1", REG);
    assert!(text.contains("> Exports are signed."));
    assert!(text.contains("the sentence above"));
}

#[test]
fn a_passing_section_carries_no_quoted_statement() {
    let gate = judge("- Exports are signed.", &["docs/business-rules.md"]);
    assert!(gate.ok());
    assert_eq!(gate.statement, None);
}

#[test]
fn section_items_regroups_lines_into_statements() {
    let body = "- first\n  wrapped\n- second\n\nA paragraph\nthat continues.\n---\nAfter a break.";
    assert_eq!(
        section_items(body),
        vec![
            "- first wrapped",
            "- second",
            "A paragraph that continues.",
            "After a break."
        ]
    );
}

#[test]
fn a_nested_bullet_is_its_own_statement_not_its_parents_continuation() {
    for indent in ["  ", "    ", "\t", "   "] {
        let rules =
            format!("- BR-AF-001: the parent cites a rule\n{indent}- a new rule hides here");
        assert_eq!(
            new_rule_statement(&ticket(&rules)).as_deref(),
            Some("a new rule hides here"),
            "indent {indent:?}"
        );
    }
}

#[test]
fn an_unknown_id_alone_quotes_nothing() {
    let gate = judge("- BR-AF-999", &["src/a.rs"]);
    assert_eq!(gate.statement, None);
    assert!(!refusal(&gate, "ABC-1", REG).contains("\n> "));
}

#[test]
fn an_unknown_id_alone_does_not_point_at_a_paragraph_that_is_not_there() {
    let text = refusal(&judge("- BR-AF-999", &["src/a.rs"]), "ABC-1", REG);
    assert!(!text.contains("below"));
    assert!(!text.contains("the sentence above"));
    assert!(text.contains("in this PR"));
}

#[test]
fn section_items_splits_on_every_marker_it_names() {
    let body = "intro\n### heading\n> quote\n| a | b |\n1. ordered\n2) other\n+ plus\n* star";
    assert_eq!(section_items(body).len(), 8);
}

const ABSENCES: [&str; 28] = [
    "Aucune.",
    "Aucun.",
    "Aucune nouvelle règle métier.",
    "AUCUNE NOUVELLE RÈGLE.",
    "Aucune règle.",
    "Aucune nouvelle règle",
    "Aucun nouveau",
    "Aucune nouvelle règle métier",
    "None",
    "None.",
    "No new rule.",
    "No new rules",
    "No new business rules",
    "No business rule.",
    "No rules",
    "N/A",
    "n/a.",
    "NA",
    "Néant",
    "neant",
    "Sans objet.",
    "**Aucune.**",
    "_Aucune nouvelle règle._",
    "`N/A`",
    "*None*",
    "\"None.\"",
    "(none)",
    "None\u{a0}!",
];

#[test]
fn a_declaration_of_absence_is_not_a_new_rule() {
    for phrase in ABSENCES {
        for shape in [
            "{}", "- {}", "* {}", "+ {}", "1. {}", "- [ ] {}", "- [x] {}",
        ] {
            let rules = shape.replace("{}", phrase);
            assert_eq!(new_rule_statement(&ticket(&rules)), None, "{rules:?}");
        }
    }
}

#[test]
fn a_declaration_of_absence_lets_the_pr_merge_without_touching_the_registry() {
    assert!(judge("Aucune nouvelle règle métier.", &["src/a.rs"]).ok());
}

#[test]
fn a_declaration_of_absence_that_wraps_over_two_lines_is_still_one_declaration() {
    assert_eq!(
        new_rule_statement(&ticket("- Aucune nouvelle\n  règle métier.")),
        None
    );
}

#[test]
fn a_real_rule_that_merely_starts_like_an_absence_is_still_a_rule() {
    for rule in [
        "Aucune redirection ne quitte l'origine publique.",
        "None of the redirects may leave the public origin.",
        "No redirect leaves the public origin.",
        "No new rule without a test.",
        "N/A pour l'admin, mais jamais pour un invité.",
        "Néant ne vaut pas accord.",
        "Aucune. Et pourtant une PR par ticket reste la règle.",
        "None but the owner may publish.",
        "No business rule may be skipped.",
        "Sans objet pour l'invité, obligatoire pour l'admin.",
    ] {
        let gate = judge(&format!("- {rule}"), &["src/a.rs"]);
        assert!(gate.unpromoted, "{rule}");
    }
}

#[test]
fn a_declaration_of_absence_does_not_hide_a_real_rule_next_to_it() {
    assert!(judge("- Aucune.\n- Exports are signed.", &["src/a.rs"]).unpromoted);
    assert!(judge("- Exports are signed.\n- None.", &["src/a.rs"]).unpromoted);
}

#[test]
fn a_rule_of_process_cited_without_an_id_is_still_a_new_rule() {
    assert!(judge("- The usual one-PR-per-ticket rule applies.", &["src/a.rs"]).unpromoted);
}

#[test]
fn an_empty_section_declares_no_rule_either() {
    // A heading with nothing under it, or followed at once by the next one.
    for rules in ["", "\n\n", "## Next section\n"] {
        assert!(judge(rules, &["src/a.rs"]).ok(), "{rules:?}");
    }
}

#[test]
fn the_refusal_says_how_to_declare_that_there_is_no_rule() {
    let text = refusal(&judge("- Exports are signed.", &["src/a.rs"]), "ABC-1", REG);
    assert!(text.contains(ABSENCE_DECLARATION));
    assert!(text.contains("If the ticket really has no new rule"));
}

#[test]
fn the_sentence_the_refusal_recommends_is_accepted_by_the_reader() {
    for shape in ["{}", "- {}", "- [ ] {}"] {
        let rules = shape.replace("{}", ABSENCE_DECLARATION);
        assert_eq!(new_rule_statement(&ticket(&rules)), None, "{rules}");
    }
}

#[test]
fn a_registry_that_could_not_be_read_is_its_own_refusal() {
    let changed = paths(&["src/a.rs"]);
    let gate = evaluate(
        &ticket("- BR-AF-999\n- A new rule."),
        &Registry::Unreadable("HTTP 502: `bad gateway`\n  retry".into()),
        Some(&changed),
        REG,
    );
    assert!(!gate.ok());
    assert!(gate.unknown_ids.is_empty());
    assert!(!gate.unpromoted);
    let text = refusal(&gate, "ABC-1", REG);
    assert!(text.contains("Registry unreadable"));
    assert!(text.contains("HTTP 502: 'bad gateway' retry"));
    for gesture in ["typo", "promote", "does not exist", "Non-goal", "same PR"] {
        assert!(!text.contains(gesture), "{gesture}");
    }
    assert!(text.contains("nothing needs fixing in the ticket or the PR"));
}

#[test]
fn a_registry_that_was_read_is_not_unreadable() {
    assert_eq!(judge("- BR-AF-001", &[]).unreadable, None);
}

#[test]
fn refinement_reads_the_section_title_the_guard_reads() {
    let text =
        "## Goal\nx\n## Acceptance criteria\n- [ ] a\n## **Business rules** (ABC-617)\nNone.\n";
    assert!(crate::method::contract::missing_from_contract(text).is_empty());
    assert_eq!(new_rule_statement(text), None);
}

#[test]
fn an_id_of_another_product_is_left_to_that_products_registry() {
    assert!(judge("- BR-HC-151", &["src/a.rs"]).ok());
}

#[test]
fn an_unknown_id_of_this_registrys_own_product_is_still_refused() {
    assert_eq!(
        judge("- BR-HC-151\n- BR-AF-777", &["src/a.rs"]).unknown_ids,
        vec!["BR-AF-777"]
    );
}

#[test]
fn the_same_ticket_is_judged_against_the_other_products_registry_too() {
    let changed = paths(&["src/a.rs"]);
    let gate = evaluate(
        &ticket("- BR-HC-151\n- BR-AF-777"),
        &ids(&["BR-HC-001"]),
        Some(&changed),
        REG,
    );
    assert_eq!(gate.unknown_ids, vec!["BR-HC-151"]);
}

#[test]
fn a_registry_that_declares_nothing_still_judges_every_id() {
    let changed = paths(&["src/a.rs"]);
    let gate = evaluate(&ticket("- BR-HC-151"), &ids(&[]), Some(&changed), REG);
    assert_eq!(gate.unknown_ids, vec!["BR-HC-151"]);
}

#[test]
fn a_mention_in_another_rows_cell_is_not_a_declaration() {
    let parsed = registry_ids(REGISTRY);
    assert!(!parsed.contains("BR-HC-151"));
    assert!(registry_ids("|BR-AF-088|x|\n").contains("BR-AF-088"));
    let prose = registry_ids("We keep BR-AF-010 and BR-AF-011 in prose.");
    assert_eq!(
        prose.len(),
        2,
        "a registry with no table row falls back to every id"
    );
}

#[test]
fn a_ticket_that_cites_and_states_no_rule_needs_no_registry() {
    let changed = paths(&["src/a.rs"]);
    assert!(!needs_registry(&ticket("None."), Some(&changed)));
    assert!(needs_registry(&ticket("- BR-AF-001"), None));
    assert!(needs_registry(&ticket("- A new rule."), Some(&changed)));
    assert!(
        !needs_registry(&ticket("- A new rule."), None),
        "no diff: the promotion half is skipped"
    );
}

#[test]
fn skip_reasons_name_what_holds_the_pr() {
    assert_eq!(
        skip_reason(&judge("- BR-AF-999", &["src/a.rs"])),
        "cites a business rule that is not in the registry: BR-AF-999"
    );
    assert_eq!(
        skip_reason(&judge("- New.", &["src/a.rs"])),
        "declares a business rule the PR does not add to the registry"
    );
}
