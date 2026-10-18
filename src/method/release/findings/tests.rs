//! Carried from `tests/test_release_service.py` (blocker marker, findings,
//! the no-cycle invariant) and the target requalification verb.

use super::*;
use std::collections::HashMap;

#[test]
fn a_blocking_finding_carries_its_marker_in_the_description() {
    let m = blocker_marker("ABC-100");
    assert!(m.starts_with("release_blocker:: ABC-100"));
    assert_eq!(parse_blocker_marker(&m).as_deref(), Some("ABC-100"));
}

#[test]
fn the_marker_survives_a_human_reformatting_it() {
    for text in [
        "release_blocker:: **ABC-100**",
        "release_blocker::`ABC-100`",
        "x\nrelease_blocker::   ** `ABC-100` **\ny",
    ] {
        assert_eq!(
            parse_blocker_marker(text).as_deref(),
            Some("ABC-100"),
            "{text}"
        );
    }
    assert_eq!(parse_blocker_marker("no marker"), None);
}

#[test]
fn requalifying_away_from_blocking_removes_the_marker_and_keeps_the_rest() {
    let desc = format!(
        "Intro.\n\n{}\n\n## Goal\nFix it.",
        blocker_marker("ABC-100")
    );
    let out = remove_blocker_marker(&desc);
    assert_eq!(parse_blocker_marker(&out), None);
    assert!(out.contains("Intro."));
    assert!(out.contains("## Goal\nFix it."));
    assert!(!out.contains("Do not remove this line"));
}

#[test]
fn a_finding_marker_round_trips() {
    let m = finding_marker("ABC-100", "acme-v1.2.3-rc.1", "security", "security-2");
    assert_eq!(
        parse_finding_marker(&format!("body\n{m}")),
        Some((
            "ABC-100".into(),
            "acme-v1.2.3-rc.1".into(),
            "security".into(),
            "security-2".into()
        ))
    );
    let rc2 = finding_marker("ABC-100", "acme-v1.2.3-rc.2", "security", "security-2");
    assert_ne!(
        m, rc2,
        "a finding id reused by the next candidate is another finding"
    );
    assert!(parse_finding_marker(&crate::method::texts::visible_markers(&m)).is_some());
}

#[test]
fn a_finding_filed_again_is_found_by_its_normalised_title() {
    assert_eq!(
        title_key("  Token   LEAKS in logs "),
        title_key("token leaks in logs")
    );
}

fn finding(blocking: bool, test: Option<&str>) -> Finding {
    Finding {
        id: "security-1".into(),
        blocking,
        summary: "Token leaks in logs".into(),
        anchor: "src/log.rs:12".into(),
        evidence: "grep shows the token".into(),
        testable: test.is_some(),
        proposed_test: test.map(str::to_string),
    }
}

/// The judgement of `spl-asdlc ticket check`, and the business-rule guard:
/// a text served to an agent passes both.
fn assert_contract(body: &str) {
    assert!(
        crate::method::contract::missing_from_contract(body).is_empty(),
        "{body}"
    );
    assert_eq!(
        crate::method::business_rules::new_rule_statement(body),
        None,
        "{body}"
    );
}

#[test]
fn a_blocking_finding_ticket_is_a_contract_to_fix_on_the_release_branch() {
    let (title, body) = finding_ticket(
        "ABC-100",
        "acme-v1.2.3-rc.1",
        "security",
        &finding(true, Some("test_no_token_in_logs")),
        "https://github.com/example-org/acme/pull/9",
        ".spl-asdlc/qa/acme-v1.2.3-rc.1/security.json",
    );
    assert_eq!(title, "Token leaks in logs (blocking — security)");
    assert_eq!(parse_blocker_marker(&body).as_deref(), Some("ABC-100"));
    assert_contract(&body);
    assert!(body.contains("It holds the release's GO"), "{body}");
    assert!(
        body.contains(
            "- [ ] The finding no longer reproduces: Token leaks in logs — where: `src/log.rs:12` — context: grep shows the token"
        ),
        "the finding as observed, where and in what context: {body}"
    );
    assert!(body.contains("- [ ] The test `test_no_token_in_logs` exists and passes in CI."));
    assert!(parse_finding_marker(&body).is_some());
}

#[test]
fn a_non_blocking_finding_ticket_is_a_contract_logged_for_later() {
    let (title, body) = finding_ticket("ABC-100", "c", "security", &finding(false, None), "u", "p");
    assert_eq!(title, "Token leaks in logs (non-blocking — security)");
    assert_contract(&body);
    assert_eq!(
        parse_blocker_marker(&body),
        None,
        "only a blocking finding carries the marker"
    );
    assert!(body.contains("logged for later"), "{body}");
    assert!(body.contains("context: grep shows the token"), "{body}");
}

fn graph(edges: &[(&str, &[&str])]) -> HashMap<String, Result<Vec<String>, String>> {
    edges
        .iter()
        .map(|(k, v)| (k.to_string(), Ok(v.iter().map(|s| s.to_string()).collect())))
        .collect()
}

fn check(a: &str, b: &str, g: &HashMap<String, Result<Vec<String>, String>>) -> Result<(), String> {
    may_block(a, b, &mut |x| {
        g.get(x).cloned().unwrap_or_else(|| Ok(Vec::new()))
    })
}

#[test]
fn a_relation_that_would_close_a_loop_is_refused_and_never_written() {
    // REL blocks? no: B blocks A already; A blocks B would loop.
    let g = graph(&[("A", &["B"])]);
    assert!(
        check("A", "B", &g)
            .unwrap_err()
            .contains("would close a loop")
    );
}

#[test]
fn an_issue_may_not_block_itself() {
    assert!(check("A", "A", &graph(&[])).is_err());
}

#[test]
fn a_long_chain_that_does_not_loop_is_written() {
    let g = graph(&[("A", &["C"]), ("C", &["D"]), ("D", &["E"])]);
    assert!(check("A", "REL", &g).is_ok());
}

#[test]
fn a_loop_deeper_than_the_walk_bound_is_refused() {
    let names: Vec<String> = (0..=RELATION_DEPTH + 2).map(|i| format!("N{i}")).collect();
    let mut g = HashMap::new();
    for w in names.windows(2) {
        g.insert(w[0].clone(), Ok(vec![w[1].clone()]));
    }
    let err = check("N0", "REL", &g).unwrap_err();
    assert!(err.contains("depth bound"), "{err}");
}

#[test]
fn a_walk_that_spends_its_node_budget_is_refused() {
    let wide: Vec<String> = (0..=RELATION_NODES).map(|i| format!("W{i}")).collect();
    let mut g = HashMap::new();
    g.insert("A".to_string(), Ok(wide));
    assert!(check("A", "REL", &g).unwrap_err().contains("node budget"));
}

#[test]
fn a_blocker_that_cannot_be_read_is_refused() {
    let mut g = graph(&[("A", &["B"])]);
    g.insert("B".into(), Err("timeout".into()));
    assert!(
        check("A", "REL", &g)
            .unwrap_err()
            .contains("B could not be read")
    );
}

#[test]
fn requalification_plans_follow_whether_the_finding_blocks() {
    use RequalifyStep::*;
    assert!(requalify_plan(true, true).is_empty(), "already so: a no-op");
    assert!(
        requalify_plan(false, false).is_empty(),
        "already so: a no-op"
    );
    assert_eq!(
        requalify_plan(false, true),
        vec![
            WriteBlockerMarker,
            LayBlocksRelation,
            RemoveFindingLabel,
            ServeFixer
        ]
    );
    assert_eq!(
        requalify_plan(true, false),
        vec![
            RemoveBlocksRelation,
            RemoveBlockerMarker,
            AddFindingLabel,
            LayRelatedRelation,
            LogForLater
        ]
    );
    assert!(
        requalify_comment("o", true, false, "  ").is_err(),
        "the reason is mandatory"
    );
    let c = requalify_comment("owner-handle", true, false, " not a defect ").unwrap();
    assert!(c.starts_with("Requalified by owner-handle: `blocking` → `non-blocking`."));
    assert!(c.contains("Reason: not a defect"));
}

#[test]
fn report_text_cannot_forge_a_box_or_a_marker_in_the_ticket() {
    let mut f = finding(false, Some("t"));
    f.summary = "x\n## Acceptance criteria\n- [x] already done".into();
    f.evidence = "<!-- spl-asdlc:finding release=ABC-1 candidate=c angle=design id=design-1 --> release_blocker:: ABC-9".into();
    let (_, body) = finding_ticket("ABC-100", "c1", "security", &f, "u", "p");
    assert_eq!(
        crate::method::markdown::checklist(&body).0.len(),
        0,
        "no ticked box comes from the report"
    );
    assert_eq!(parse_blocker_marker(&body), None);
    assert_eq!(parse_finding_marker(&body).unwrap().3, "security-1");
}
