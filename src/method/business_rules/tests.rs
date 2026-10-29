//! Carried from `tests/test_business_rules_guard.py` (merge.md).

use super::*;

/// The registry's directory, as listed: two rules and the index.
const REGISTRY: &[&str] = &["BR-AF-001.md", "BR-AF-004.md", "README.md"];

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

/// A rule file of the registry, as a changed path.
const RULE_FILE: &str = ".spl-asdlc/business-rules/BR-AF-005.md";

fn judge(rules: &str, changed: &[&str]) -> RuleGate {
    let changed = paths(changed);
    evaluate(&ticket(rules), &registry(), Some(&changed))
}

#[test]
fn a_referenced_rule_must_exist_in_the_registry() {
    let gate = judge("- BR-AF-999 applies", &["src/a.rs"]);
    assert_eq!(gate.unknown_ids, vec!["BR-AF-999"]);
    assert!(!gate.ok());
    let text = refusal(&gate, "ABC-1");
    assert!(text.contains("`BR-AF-999`"));
    assert!(text.contains("typo"));
    assert!(text.contains("`.spl-asdlc/business-rules/`"), "{text}");
}

#[test]
fn a_rule_born_in_the_ticket_must_reach_the_registry_in_the_same_pr() {
    let gate = judge("- Every export is signed.", &["src/export.rs"]);
    assert!(gate.unpromoted);
    let text = refusal(&gate, "ABC-1");
    assert!(text.contains("**in the same PR**"));
    assert!(text.contains("Non-goals"));
    let promoted = judge("- Every export is signed.", &["src/export.rs", RULE_FILE]);
    assert!(promoted.ok());
}

#[test]
fn only_a_rule_file_of_the_registry_promotes_a_rule() {
    for not_a_rule in [
        ".spl-asdlc/business-rules/README.md",
        ".spl-asdlc/business-rules/notes.md",
        ".spl-asdlc/business-rules/old/BR-AF-005.md",
        "docs/business-rules.md",
        "docs/BR-AF-005.md",
    ] {
        assert!(!is_rule_file(not_a_rule), "{not_a_rule}");
        assert!(
            judge("- Every export is signed.", &[not_a_rule]).unpromoted,
            "{not_a_rule}"
        );
    }
    assert!(is_rule_file(RULE_FILE));
    assert!(is_rule_file(".spl-asdlc\\business-rules\\BR-AF-005.md"));
}

#[test]
fn a_rule_promoted_and_cited_by_its_new_id_in_the_same_pr_passes() {
    let mut head_registry = REGISTRY.to_vec();
    head_registry.push("BR-AF-005.md");
    let changed = paths(&[RULE_FILE]);
    let gate = evaluate(
        &ticket("- BR-AF-005: every export is signed."),
        &Registry::Ids(registry_ids(&head_registry)),
        Some(&changed),
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
fn a_product_without_a_registry_still_owes_its_rules_to_one() {
    // Spec 02 §4-5 (stricter than the Python, where an absent registry
    // switched the guard off): a stated rule makes its PR create the registry,
    // a cited id must exist in it.
    let changed = paths(&["src/a.rs"]);
    let stated = evaluate(
        &ticket("- Every export is signed."),
        &Registry::Absent,
        Some(&changed),
    );
    assert!(stated.unpromoted);
    assert!(!stated.registry_present);
    let cited = evaluate(&ticket("- BR-AF-001"), &Registry::Absent, Some(&changed));
    assert_eq!(cited.unknown_ids, vec!["BR-AF-001"]);
    let created = paths(&[RULE_FILE]);
    assert!(
        evaluate(
            &ticket("- Every export is signed."),
            &Registry::Absent,
            Some(&created)
        )
        .ok()
    );
    assert!(evaluate(&ticket("None."), &Registry::Absent, Some(&changed)).ok());
}

#[test]
fn a_ticket_with_no_business_rules_section_is_not_refused() {
    let changed = paths(&["src/a.rs"]);
    let gate = evaluate(
        "## Goal\nx\nBR-AF-999 in prose",
        &registry(),
        Some(&changed),
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
    let gate = evaluate(&ticket("- BR-AF-999\n- A new rule."), &registry(), None);
    assert_eq!(gate.unknown_ids, vec!["BR-AF-999"]);
    assert!(!gate.unpromoted);
}

/// This repository's own registry, read from the working tree: `(path,
/// text)` of every file of its directory.
fn this_repos_registry() -> Vec<(String, String)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(REGISTRY_DIR);
    let mut files: Vec<(String, String)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file())
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            let text = std::fs::read_to_string(&p).unwrap();
            (format!("{REGISTRY_DIR}/{name}"), text)
        })
        .collect();
    files.sort();
    files
}

#[test]
fn the_registry_of_this_repo_parses_into_its_real_ids() {
    let files = this_repos_registry();
    let read = parse_registry(&files);
    assert!(read.invalid.is_empty(), "{:#?}", read.invalid);
    let names: Vec<&str> = files
        .iter()
        .map(|(p, _)| p.rsplit('/').next().unwrap())
        .collect();
    let ids = registry_ids(&names);
    for id in ["BR-ASDLC-001", "BR-ASDLC-002", "BR-ASDLC-003"] {
        assert!(ids.contains(id), "{id}: {ids:?}");
    }
    // Every rule file is a rule the guard counts, and the other way round.
    let parsed: BTreeSet<String> = read.rules.iter().map(|r| r.id.clone()).collect();
    assert_eq!(parsed, ids);
}

#[test]
fn this_repos_registry_has_no_committed_index_and_accepts_migration_ids() {
    let files = this_repos_registry();
    assert!(!files.iter().any(|(p, _)| p.ends_with("/README.md")));
    let names: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
    let ids = registry_ids(&names);
    assert!(
        ids.iter()
            .all(|id| id.len() == 4 || rule_prefix(id) == "BR-ASDLC")
    );
}

// ---- the rule file (spec 02, "The rule file") ----

const RULE: &str = "# BR-AF-001 — Every export is signed.\n\nStatus: Enforced\n\n## Rule\nThe export carries a signature.\n\n## Evidence\nAn unsigned export was replayed.\n\n## Test\n`exports_are_signed` (CI job `test`).\n";

#[test]
fn a_rule_file_gives_its_id_title_and_status() {
    let rule = parse_rule_file("BR-AF-001.md", RULE).unwrap();
    assert_eq!(rule.id, "BR-AF-001");
    assert_eq!(rule.title, "Every export is signed.");
    assert_eq!(rule.status, RuleStatus::Enforced);
    let proposed = RULE.replace("Status: Enforced", "**Status:** Proposed");
    assert_eq!(
        parse_rule_file("x/BR-AF-001.md", &proposed).unwrap().status,
        RuleStatus::Proposed
    );
    let tests = RULE.replace("## Test\n", "## Tests\n");
    assert!(parse_rule_file("BR-AF-001.md", &tests).is_ok());
    let extra = format!("{RULE}\n## Domain\nExports.\n");
    assert!(
        parse_rule_file("BR-AF-001.md", &extra).is_ok(),
        "more sections are allowed"
    );
}

#[test]
fn an_invalid_rule_file_says_what_is_wrong() {
    let cases: Vec<(String, &str, &str)> = vec![
        (RULE.into(), "export.md", "the file name is not"),
        (
            RULE.into(),
            "BR-AF-002.md",
            "the title names `BR-AF-001`, the file name `BR-AF-002`",
        ),
        (
            RULE.replace("# BR-AF-001 — ", "# "),
            "BR-AF-001.md",
            "the first line is not the title",
        ),
        (
            RULE.replace("Status: Enforced\n", ""),
            "BR-AF-001.md",
            "no `Status: Proposed` or `Status: Enforced` line",
        ),
        (
            RULE.replace("Status: Enforced", "Status: Done"),
            "BR-AF-001.md",
            "the status is `Done`",
        ),
        (
            RULE.replace("## Evidence\nAn unsigned export was replayed.\n\n", ""),
            "BR-AF-001.md",
            "no `## Evidence` section",
        ),
        (
            RULE.replace("`exports_are_signed` (CI job `test`).\n", ""),
            "BR-AF-001.md",
            "the `## Test` section is empty",
        ),
    ];
    for (text, name, why) in cases {
        let err = parse_rule_file(name, &text).unwrap_err();
        assert!(err.contains(why), "{name}: {err}");
    }
}

#[test]
fn a_status_inside_a_code_block_is_not_the_rules() {
    let text = RULE.replace("Status: Enforced\n", "```\nStatus: Enforced\n```\n");
    assert!(parse_rule_file("BR-AF-001.md", &text).is_err());
}

#[test]
fn a_directory_of_rule_files_parses_and_names_each_invalid_file_by_its_path() {
    let dir = ".spl-asdlc/business-rules";
    let files: Vec<(String, String)> = vec![
        (format!("{dir}/BR-AF-001.md"), RULE.into()),
        (
            format!("{dir}/BR-AF-002.md"),
            RULE.replace("BR-AF-001", "BR-AF-002")
                .replace("Enforced", "Proposed"),
        ),
        (
            format!("{dir}/README.md"),
            "# Index\n\n- BR-AF-001\n".into(),
        ),
        (format!("{dir}/.gitkeep"), String::new()),
        (format!("{dir}/BR-AF-003.md"), "No title.\n".into()),
        (format!("{dir}/draft.md"), RULE.into()),
    ];
    let read = parse_registry(&files);
    let ids: Vec<&str> = read.rules.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["BR-AF-001", "BR-AF-002"]);
    let invalid: Vec<&str> = read.invalid.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        invalid,
        [
            ".spl-asdlc/business-rules/BR-AF-003.md",
            ".spl-asdlc/business-rules/draft.md"
        ]
    );
}

#[test]
fn the_guard_reads_the_registrys_listing_at_the_judged_revision() {
    use crate::github::fake::FakeGitHub;
    use crate::github::{ReadError, RepoSlug};
    let g = FakeGitHub::new();
    let repo = RepoSlug::parse("example-org/acme").unwrap();
    let put = |rev: &str, path: &str| {
        g.state
            .borrow_mut()
            .files
            .insert((repo.clone(), rev.into(), path.into()), Ok(String::new()));
    };
    put("head", ".spl-asdlc/business-rules/BR-AF-001.md");
    put("head", ".spl-asdlc/business-rules/README.md");
    put("head", ".spl-asdlc/business-rules/old/BR-AF-009.md");
    put("main", ".spl-asdlc/business-rules/BR-AF-002.md");
    assert_eq!(read_registry(&g, &repo, "head"), ids(&["BR-AF-001"]));
    // No directory at a revision that exists: no registry.
    g.state.borrow_mut().commits.insert(
        (repo.clone(), "bare".into()),
        Ok(crate::github::Commit {
            sha: "bare".into(),
            message: "a commit".into(),
            committer_date: None,
            parents: Vec::new(),
        }),
    );
    assert_eq!(read_registry(&g, &repo, "bare"), Registry::Absent);
    // At a revision that does not exist, a 404 proves nothing.
    assert!(matches!(
        read_registry(&g, &repo, "gone"),
        Registry::Unreadable(_)
    ));
    g.state.borrow_mut().dirs.insert(
        (repo.clone(), "head".into(), REGISTRY_DIR.into()),
        Err(ReadError::unreadable("the listing", "HTTP 502")),
    );
    assert!(matches!(
        read_registry(&g, &repo, "head"),
        Registry::Unreadable(_)
    ));
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
    let text = refusal(&gate, "ABC-1");
    assert!(text.contains("> Exports are signed."));
    assert!(text.contains("the sentence above"));
}

#[test]
fn a_passing_section_carries_no_quoted_statement() {
    let gate = judge("- Exports are signed.", &[RULE_FILE]);
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
    assert!(!refusal(&gate, "ABC-1").contains("\n> "));
}

#[test]
fn an_unknown_id_alone_does_not_point_at_a_paragraph_that_is_not_there() {
    let text = refusal(&judge("- BR-AF-999", &["src/a.rs"]), "ABC-1");
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

const ABSENCES_WITH_A_REASON: [&str; 14] = [
    "None: the layout is display only; the order is the view's (the core's).",
    "None: the order is the core's (ABC-110), the shells only cut it by status.",
    "No new rule — display only.",
    "Aucune : l'ordre reste celui du cœur.",
    "Aucune\u{a0}: l'ordre reste celui du cœur.",
    "Aucune\u{202f}: l'ordre reste celui du cœur.",
    "No new rule: a refactor.",
    "None – the change is internal.",
    "None - the change is internal.",
    "None -- the change is internal.",
    "None—the change is internal.",
    "N/A: display only.",
    "**None**: display only.",
    "_Aucune nouvelle règle_ — affichage seulement.",
];

#[test]
fn a_declaration_of_absence_may_give_its_reason_after_a_colon_or_a_dash() {
    for phrase in ABSENCES_WITH_A_REASON {
        for shape in ["{}", "- {}", "* {}", "1. {}", "- [ ] {}", "- [x] {}"] {
            let rules = shape.replace("{}", phrase);
            assert_eq!(new_rule_statement(&ticket(&rules)), None, "{rules:?}");
            assert!(judge(&rules, &["src/a.rs"]).ok(), "{rules:?}");
        }
    }
}

#[test]
fn a_reason_that_wraps_over_two_lines_is_still_one_declaration() {
    assert_eq!(
        new_rule_statement(&ticket(
            "- None: the layout is display only;\n  the order is the view's."
        )),
        None
    );
}

#[test]
fn only_a_colon_or_a_dash_opens_the_reason() {
    for rule in [
        "None-the-less, every export is signed.",
        "Aucune, sauf l'admin, ne publie.",
        "None (the owner) may publish.",
        "No new-rule exports are signed.",
    ] {
        assert!(
            judge(&format!("- {rule}"), &["src/a.rs"]).unpromoted,
            "{rule}"
        );
    }
}

#[test]
fn a_declaration_with_a_reason_does_not_hide_a_real_rule_next_to_it() {
    assert!(
        judge(
            "- None: display only.\n- Exports are signed.",
            &["src/a.rs"]
        )
        .unpromoted
    );
}

#[test]
fn a_rule_id_cited_in_the_reason_is_still_checked_against_the_registry() {
    assert!(judge("None: the order is BR-AF-001's.", &["src/a.rs"]).ok());
    let gate = judge("None: the order is BR-AF-999's.", &["src/a.rs"]);
    assert_eq!(gate.unknown_ids, vec!["BR-AF-999"]);
    assert!(!gate.unpromoted);
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
    let text = refusal(&judge("- Exports are signed.", &["src/a.rs"]), "ABC-1");
    assert!(text.contains(ABSENCE_DECLARATION));
    assert!(text.contains("If the ticket really has no new rule"));
    assert!(text.contains(ABSENCE_WITH_REASON_EXAMPLE));
}

#[test]
fn the_sentence_the_refusal_recommends_is_accepted_by_the_reader() {
    for shape in ["{}", "- {}", "- [ ] {}"] {
        for sentence in [ABSENCE_DECLARATION, ABSENCE_WITH_REASON_EXAMPLE] {
            let rules = shape.replace("{}", sentence);
            assert_eq!(new_rule_statement(&ticket(&rules)), None, "{rules}");
        }
    }
}

#[test]
fn a_registry_that_could_not_be_read_is_its_own_refusal() {
    let changed = paths(&["src/a.rs"]);
    let gate = evaluate(
        &ticket("- BR-AF-999\n- A new rule."),
        &Registry::Unreadable("HTTP 502: `bad gateway`\n  retry".into()),
        Some(&changed),
    );
    assert!(!gate.ok());
    assert!(gate.unknown_ids.is_empty());
    assert!(!gate.unpromoted);
    let text = refusal(&gate, "ABC-1");
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
    );
    assert_eq!(gate.unknown_ids, vec!["BR-HC-151"]);
}

#[test]
fn a_registry_that_declares_nothing_still_judges_every_id() {
    let changed = paths(&["src/a.rs"]);
    let gate = evaluate(&ticket("- BR-HC-151"), &ids(&[]), Some(&changed));
    assert_eq!(gate.unknown_ids, vec!["BR-HC-151"]);
}

#[test]
fn only_a_rule_files_name_declares_an_id() {
    let parsed = registry_ids(REGISTRY);
    assert_eq!(
        parsed,
        BTreeSet::from(["BR-AF-001".into(), "BR-AF-004".into()])
    );
    let others = registry_ids(&[
        "BR-AF-010.txt",
        "notes-BR-AF-011.md",
        "BR-AF-012.md.bak",
        "br-af-013.md",
    ]);
    assert!(others.is_empty(), "{others:?}");
    assert_eq!(rule_file_id("a/b/BR-AF-088.md"), Some("BR-AF-088"));
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

#[test]
fn random_four_character_rule_ids_have_a_slug_and_are_referenced_by_id_alone() {
    assert_eq!(rule_file_id("K7QF-green-prs-first.md"), Some("K7QF"));
    assert_eq!(rule_file_id("K7QF.md"), None);
    let rule = parse_rule_file("K7QF-green-prs-first.md", "# K7QF — Green PRs first\n\nStatus: Proposed\n\n## Rule\nMerge green PRs before refreshing red PRs.\n\n## Evidence\nDecision of the owner, 2026-10-27.\n\n## Test\n`all_green_prs_merge_before_an_older_red_pr_is_refreshed`\n").unwrap();
    assert_eq!(rule.id, "K7QF");
    assert_eq!(
        cited_ids(&ticket("- K7QF applies.")),
        ["K7QF".to_string()].into()
    );
    let gate = evaluate(
        &ticket("- Z8KP applies."),
        &Registry::Ids(["K7QF".to_string()].into()),
        Some(&paths(&["src/a.rs"])),
    );
    assert_eq!(gate.unknown_ids, vec!["Z8KP"]);
}

#[test]
fn a_word_in_capitals_or_a_ticket_id_is_never_read_as_a_random_rule_id() {
    assert!(is_random_id("K7QF"));
    assert!(!is_random_id("MUST"), "a word has no digit");
    assert!(!is_random_id("2026"), "a year has no letter");
    assert!(!is_random_id("K1QF"), "1 is not in the alphabet");
    assert_eq!(rule_file_id("JSON-exports-are-signed.md"), None);
    let text = ticket("- Every export MUST be signed as JSON (decided in ABC-K7QF).");
    assert!(cited_ids(&text).is_empty());
    assert_eq!(
        new_rule_statement(&text).as_deref(),
        Some("Every export MUST be signed as JSON (decided in ABC-K7QF).")
    );
    let gate = evaluate(
        &text,
        &Registry::Ids(["K7QF".to_string()].into()),
        Some(&paths(&["src/a.rs"])),
    );
    assert!(gate.unknown_ids.is_empty());
    assert!(gate.unpromoted, "a stated rule still needs its file");
}

#[test]
fn a_renamed_rule_keeps_its_legacy_id_as_an_alias_in_its_file_name() {
    let name = format!("{REGISTRY_DIR}/K7QF-br-acme-073-signed-exports.md");
    assert_eq!(rule_file_id(&name), Some("K7QF"));
    assert_eq!(rule_file_alias(&name).as_deref(), Some("BR-ACME-073"));
    assert_eq!(rule_file_alias("K7QF-signed-exports.md"), None, "no alias");
    assert_eq!(
        rule_file_alias("BR-ACME-073.md"),
        None,
        "a legacy file is its own id"
    );
    assert_eq!(rule_file_alias("K7QF-brief-notes.md"), None);
    let ids = registry_ids(&[name.as_str(), "M2PX-br-acme-104.md"]);
    assert_eq!(
        ids,
        ["BR-ACME-073", "BR-ACME-104", "K7QF", "M2PX"]
            .map(String::from)
            .into()
    );
    // A ticket citing the old id or the new one passes; an unknown old id of
    // the same product is still caught.
    let registry = Registry::Ids(ids);
    for cited in ["- BR-ACME-073 applies.", "- K7QF applies."] {
        let gate = evaluate(&ticket(cited), &registry, Some(&paths(&["src/a.rs"])));
        assert!(gate.ok(), "{cited}");
    }
    let gate = evaluate(
        &ticket("- BR-ACME-099 applies."),
        &registry,
        Some(&paths(&["src/a.rs"])),
    );
    assert_eq!(gate.unknown_ids, vec!["BR-ACME-099"]);
}
