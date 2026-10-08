//! Carried from `tests/test_release_service.py` (blocker marker, findings,
//! nits, the no-cycle invariant) and the target requalification verb.

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
fn requalifying_away_from_blocker_removes_the_marker_and_keeps_the_rest() {
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

fn finding(severity: Severity, test: Option<&str>) -> Finding {
    Finding {
        id: "security-1".into(),
        severity,
        summary: "Token leaks in logs".into(),
        anchor: "src/log.rs:12".into(),
        evidence: "grep shows the token".into(),
        fixed_in_branch: false,
        testable: test.is_some(),
        proposed_test: test.map(str::to_string),
        ticket: None,
    }
}

#[test]
fn a_finding_ticket_is_a_contract_carrying_its_test() {
    let (title, body) = finding_ticket(
        "ABC-100",
        "acme-v1.2.3-rc.1",
        "security",
        &finding(Severity::Blocker, Some("test_no_token_in_logs")),
        "https://github.com/example-org/acme/pull/9",
        ".spl-asdlc/qa/acme-v1.2.3-rc.1/security.json",
    );
    assert_eq!(title, "Token leaks in logs (blocker — security)");
    assert_eq!(parse_blocker_marker(&body).as_deref(), Some("ABC-100"));
    assert!(
        crate::method::contract::missing_from_contract(&body).is_empty(),
        "{body}"
    );
    assert!(body.contains("- [ ] The test `test_no_token_in_logs` exists and passes in CI."));
    assert!(parse_finding_marker(&body).is_some());
    let (_, should_fix) = finding_ticket(
        "ABC-100",
        "c",
        "security",
        &finding(Severity::ShouldFix, None),
        "u",
        "p",
    );
    assert_eq!(
        parse_blocker_marker(&should_fix),
        None,
        "only a blocker carries the marker"
    );
}

fn nit(id: &str, summary: &str, anchor: &str, evidence: &str) -> Finding {
    Finding {
        id: id.into(),
        severity: Severity::Nit,
        summary: summary.into(),
        anchor: anchor.into(),
        evidence: evidence.into(),
        fixed_in_branch: false,
        testable: false,
        proposed_test: None,
        ticket: None,
    }
}

const RC: &str = "acme-v1.2.3-rc.1";

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
fn a_nit_line_carries_what_where_and_context_on_one_line() {
    let f = nit(
        "design-2",
        "typo in   a\n label",
        "screen `Settings`",
        "seen on the\nsave button <!-- spl-asdlc:x -->",
    );
    let line = nit_line(RC, "design", &f);
    assert_eq!(
        line,
        "- [ ] `acme-v1.2.3-rc.1/design/design-2` typo in a label — where: `screen 'Settings'` — context: \
         seen on the save button &lt;!-- spl-asdlc :x -->"
    );
    assert!(line.starts_with(&format!("- [ ] {}", nit_key(RC, "design", "design-2"))));
    let bare = nit_line(RC, "design", &nit("design-3", "s", "a", " "));
    assert!(bare.ends_with("context: (none given)"), "{bare}");
}

#[test]
fn the_shared_nits_ticket_is_a_contract_whose_goal_is_to_triage() {
    let f = nit("design-2", "typo", "src/a.rs:1", "e");
    let body = nits_ticket("ABC-100", "acme", "1.2.3", &[nit_line(RC, "design", &f)]);
    assert_contract(&body);
    for part in [
        "## Goal\nTriage the nits",
        "**kept** or **dropped**; never presume a nit fixed",
        "A dropped nit is closed with its reason",
        "## Non-goals\n",
        "## Business rules\nNo new rule.",
    ] {
        assert!(body.contains(part), "{part}: {body}");
    }
    let acs = crate::method::markdown::checkboxes_in_sections(
        &body,
        crate::method::contract::is_acceptance_heading,
    );
    assert_eq!(acs.len(), 1);
    assert!(
        acs[0]
            .line
            .contains("typo — where: `src/a.rs:1` — context: e")
    );
    assert_eq!(nits_title("acme", "1.2.3"), "Nits — acme v1.2.3");
}

#[test]
fn a_nit_ticket_of_its_own_is_a_contract_keyed_like_a_finding() {
    let f = nit("design-2", "typo", "src/a.rs:1", "e");
    let (title, body) = nit_ticket("ABC-100", RC, "design", &f, "u", "p");
    assert_eq!(title, "typo (nit — design)");
    assert_contract(&body);
    assert!(
        body.contains("## Goal\nTriage the nit `design-2`"),
        "{body}"
    );
    assert!(body.contains("never presume it fixed"), "{body}");
    assert_eq!(
        parse_finding_marker(&body),
        Some((
            "ABC-100".into(),
            RC.into(),
            "design".into(),
            "design-2".into()
        ))
    );
    assert_eq!(parse_blocker_marker(&body), None, "a nit never blocks");
}

#[test]
fn a_nit_goes_under_the_acceptance_criteria_once_and_the_ticket_stays_a_contract() {
    let first = nit_line(RC, "design", &nit("design-1", "one", "a", "e"));
    let body = nits_ticket("ABC-100", "acme", "1.2.3", std::slice::from_ref(&first));
    let k2 = nit_key(RC, "design", "design-2");
    let second = nit_line(RC, "design", &nit("design-2", "two", "b", "e"));
    let once = append_nit(&body, &k2, &second).unwrap().unwrap();
    assert!(
        once.contains(&format!("{first}\n{second}\n\n## Business rules")),
        "{once}"
    );
    assert_contract(&once);
    assert_eq!(open_nits(&once), 2);
    assert_eq!(append_nit(&once, &k2, &second), Ok(None));
    // Ticked and given a reason: still that nit, never re-added.
    let judged = once.replace(
        &second,
        &format!(
            "{} — dropped: not worth it",
            second.replace("- [ ]", "- [x]")
        ),
    );
    assert_eq!(append_nit(&judged, &k2, &second), Ok(None));
    // `design-1` is not a prefix of `design-10`.
    let k10 = nit_key(RC, "design", "design-10");
    assert!(append_nit(&once, &k10, "- [ ] x").unwrap().is_some());
}

#[test]
fn appending_never_reformats_content_it_did_not_write() {
    let line = nit_line(RC, "design", &nit("design-1", "one", "a", "e"));
    let human = format!(
        "{}\nA person's note.\n",
        nits_ticket("ABC-100", "acme", "1.2.3", &[line])
    );
    let human = human.replace("## Non-goals\n", "## Non-goals\n- Their own   spacing.\n");
    let k = nit_key(RC, "design", "design-2");
    let out = append_nit(&human, &k, "- [ ] new").unwrap().unwrap();
    assert_eq!(out.replace("\n- [ ] new", ""), human);
}

#[test]
fn a_nits_ticket_without_a_contract_is_left_alone() {
    let older = "- [ ] src/b.rs:2: other\n";
    let err = append_nit(older, "`k`", "- [ ] `k` new").unwrap_err();
    assert!(err.contains("carries no contract"), "{err}");
    assert!(err.contains("no `## Goal` section"), "{err}");
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
fn requalification_plans_follow_the_severity_change() {
    use RequalifyStep::*;
    assert!(
        requalify_plan(Severity::Nit, Severity::Nit).is_empty(),
        "already that severity: a no-op"
    );
    assert_eq!(
        requalify_plan(Severity::ShouldFix, Severity::Blocker),
        vec![WriteBlockerMarker, LayBlocksRelation]
    );
    assert_eq!(
        requalify_plan(Severity::Blocker, Severity::ShouldFix),
        vec![
            RemoveBlocksRelation,
            RemoveBlockerMarker,
            AddFindingLabel,
            SetTriage
        ]
    );
    assert_eq!(
        requalify_plan(Severity::Blocker, Severity::Nit),
        vec![
            RemoveBlocksRelation,
            RemoveBlockerMarker,
            MoveToNitsTicket,
            CancelTicket
        ]
    );
    assert!(
        requalify_comment("o", Severity::Blocker, Severity::Nit, "  ").is_err(),
        "the reason is mandatory"
    );
    let c = requalify_comment(
        "owner-handle",
        Severity::Blocker,
        Severity::ShouldFix,
        " not a defect ",
    )
    .unwrap();
    assert!(c.starts_with("Requalified by owner-handle: `blocker` → `should-fix`."));
    assert!(c.contains("Reason: not a defect"));
}

#[test]
fn report_text_cannot_forge_a_box_or_a_marker_in_the_ticket() {
    let mut f = finding(Severity::ShouldFix, Some("t"));
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
