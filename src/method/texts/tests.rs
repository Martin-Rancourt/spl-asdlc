//! The texts the bridge writes: their markers, their contracts, and the
//! carried text tests (release.md, merge.md).

use super::*;

fn angle(key: &str) -> AngleLine {
    AngleLine {
        key: key.into(),
        title: format!("{} review", capitalised(key)),
        assignee: None,
    }
}

#[test]
fn a_release_item_is_known_by_its_marker_never_its_title() {
    let d = release_description("acme", "1.2.3", "release/acme-v1.2.3", &[angle("security")]);
    assert_eq!(
        bridge_item(&d),
        Some(BridgeItem::Release {
            product: "acme".into(),
            version: "v1.2.3".into()
        })
    );
    assert_eq!(release_title("acme", "1.2.3"), "Release acme v1.2.3");
    assert!(d.contains("## Release note"));
    assert!(d.contains(crate::method::release::note::PLACEHOLDER));
    assert!(d.contains("The GO belongs to nobody: it is computed."));
    assert!(
        !d.contains("- [ ]"),
        "the release item carries no verdict checkbox"
    );
}

#[test]
fn a_qa_sub_issue_carries_acceptance_criteria() {
    let (title, body) = release_angle_item(
        "acme",
        "1.2.3",
        "acme-v1.2.3-rc.2",
        2,
        &angle("security"),
        "release/2026-10-05-14-03-utc",
        ".spl-asdlc/qa/acme-v1.2.3-rc.2/security.json",
        "release/acme-v1.2.3",
        None,
    );
    assert_eq!(title, "Security review — acme v1.2.3 (rc.2)");
    assert!(
        crate::method::contract::missing_from_contract(&body).is_empty(),
        "{body}"
    );
    assert_eq!(
        crate::method::markdown::checklist(&body).1.len(),
        4,
        "real checkboxes, not bullets"
    );
    assert_eq!(
        bridge_item(&body),
        Some(BridgeItem::Angle {
            scope: "acme-v1.2.3-rc.2".into(),
            angle: "security".into()
        })
    );
}

#[test]
fn review_items_are_contracts_marked_as_the_bridges() {
    let (_, angle_body) = review_angle_item(
        "ABC-1",
        "https://github.com/example-org/acme/pull/9",
        "abcdef01abcdef01",
        "security",
        &["it touches reviewed paths: `a`".into()],
    );
    assert!(crate::method::contract::missing_from_contract(&angle_body).is_empty());
    assert!(matches!(
        bridge_item(&angle_body),
        Some(BridgeItem::Angle { .. })
    ));
    let (title, cons) = consolidation_item(
        "https://github.com/example-org/acme/pull/9",
        "ABC-1 (#9)",
        "docs/agents/consolidate.md",
    );
    assert_eq!(title, "Consolidation — ABC-1 (#9)");
    assert!(crate::method::contract::missing_from_contract(&cons).is_empty());
    assert!(matches!(
        bridge_item(&cons),
        Some(BridgeItem::Consolidation { .. })
    ));
}

#[test]
fn the_candidates_log_grows_once_per_candidate() {
    let d = release_description("acme", "1.2.3", "l", &[]);
    let line = candidate_log_line("acme-v1.2.3-rc.1", "routine", None);
    let once = append_candidate(&d, &line);
    assert_eq!(append_candidate(&once, &line), once);
    let twice = append_candidate(
        &once,
        &candidate_log_line("acme-v1.2.3-rc.2", "sensible", Some("acme-v1.2.3-rc.1")),
    );
    let section = twice
        .split("## Candidates")
        .nth(1)
        .unwrap()
        .split("## Scope")
        .next()
        .unwrap();
    assert!(section.contains("first candidate"));
    assert!(section.contains("after `acme-v1.2.3-rc.1`"));
}

#[test]
fn the_comment_says_so_when_nothing_is_reopened() {
    let c = candidate_comment(
        "acme-v1.2.3-rc.2",
        "routine",
        Some("acme-v1.2.3-rc.1"),
        None,
        &[],
        &[],
        true,
    );
    assert!(c.contains("No angle is offered on this candidate"));
    let paths: Vec<String> = (0..25).map(|i| format!("p{i}")).collect();
    let c = candidate_comment(
        "acme-v1.2.3-rc.1",
        "unclassified",
        None,
        Some("u"),
        &paths,
        &["security".into()],
        false,
    );
    assert!(c.contains("- First candidate."));
    assert!(c.contains("… and 5 more"));
    assert!(c.contains(&marker("candidate", &[("tag", "acme-v1.2.3-rc.1")])));
}

#[test]
fn the_brief_names_tickets_criteria_rules_and_distinguishes_first_from_unresolvable() {
    let mut b = Brief {
        candidate: "acme-v1.2.3-rc.2".into(),
        sha: "abc".into(),
        class: "sensible".into(),
        last_release: Some("acme-v1.2.2".into()),
        previous_candidate: Some("acme-v1.2.3-rc.1".into()),
        paths_since_release: vec!["docs/business-rules.md".into()],
        paths_since_candidate: None,
        gate_url: Some("https://github.com/example-org/acme/actions/runs/1".into()),
        evidence_url: Some("https://github.com/example-org/acme/actions/runs/1#artifacts".into()),
        tickets: vec![
            BriefTicket { id: "ABC-1".into(), title: "One".into(), state: "To Deploy".into(), prs: vec![9], description: Some("## Acceptance criteria\n- [x] a\n## Post deploy\n- [ ] verified\n## Business rules\n- BR-ACME-001\n".into()) },
            BriefTicket { id: "ABC-2".into(), title: "Two".into(), state: "?".into(), prs: vec![], description: None },
        ],
        untracked: vec!["PR #12".into(), "commit abcdef01abcd".into()],
        rule_files_touched: vec!["docs/business-rules.md".into()],
    };
    let text = brief(&b);
    assert!(text.contains("`acme-v1.2.3-rc.1` not found — diff cannot be computed"));
    assert!(text.contains("- [ ] verified (post deploy)"));
    assert!(text.contains("Business rules cited: BR-ACME-001"));
    assert!(text.contains("(ticket unreadable: its criteria are not shown)"));
    assert!(text.contains("Each rule touched must have a named test that is green."));
    b.previous_candidate = None;
    assert!(brief(&b).contains("first candidate of this version"));
    b.last_release = None;
    assert!(brief(&b).contains("(first release)"));
}

#[test]
fn a_hold_whose_reasons_change_is_said_again_and_a_flap_is_not() {
    let a = hold_key(&["ABC-2".into(), "ABC-1".into()], &[]);
    assert_eq!(
        a,
        hold_key(&["ABC-1".into(), "ABC-2".into()], &[]),
        "order does not matter"
    );
    assert_ne!(a, hold_key(&["ABC-1".into()], &[]));
    assert_eq!(digest("x"), digest("x"));
    assert_eq!(
        digest(""),
        format!("{:016x}", 0xcbf2_9ce4_8422_2325_u64),
        "the digest is stable across builds (FNV-1a offset basis)"
    );
    let c = hold_comment(
        "acme-v1.2.3-rc.1",
        &["open blocking issues: ABC-1".into()],
        "ABC-1",
        &a,
    );
    assert!(c.starts_with("**HOLD computed on `acme-v1.2.3-rc.1`**"));
    assert!(c.contains("Blockers-Carried: ABC-1"));
    assert!(c.contains("nothing to tick by hand"));
}

#[test]
fn an_escalation_explains_and_carries_its_marker() {
    let c = escalation_comment(
        "acme-v1.2.3-rc.2",
        2,
        2,
        &["ABC-7 — token leak".into()],
        false,
    );
    assert!(c.contains("Candidate 2 of 2 allowed, and 1 blocker(s) remain."));
    assert!(c.contains(&marker(
        "qa-escalation",
        &[("candidate", "acme-v1.2.3-rc.2")]
    )));
    assert!(
        escalation_comment("c", 2, 2, &[], true).contains("fixes landed after the last candidate")
    );
}

#[test]
fn a_red_gate_is_named_before_the_qa_and_not_repeated() {
    let m = stagnation_message(
        "c",
        2,
        Some("`release-gate.yml` concluded `failure`"),
        &[("Angles not merged", vec!["security (in review)".into()])],
        &["the CI gate is not green".into()],
        "ABC-100",
        "https://tracker.test/ABC-100",
    );
    assert!(m.find("Candidate gate red").unwrap() < m.find("Angles not merged").unwrap());
    let fallback = stagnation_message(
        "c",
        2,
        None,
        &[("x", vec![])],
        &[
            "the CI gate is not green".into(),
            "angles missing: a".into(),
        ],
        "ABC-100",
        "u",
    );
    assert!(fallback.contains("- angles missing: a"));
    assert!(!fallback.contains("gate is not green"));
}

#[test]
fn a_record_names_its_candidate_and_carries_one_blockers_line() {
    let text = gate_record(
        "1.2.3",
        "v1.2.3-rc.2",
        "abcdef01abcdef01",
        "release/x",
        "ABC-100",
        "u",
        "2026-10-05 14:03",
        "none",
        &[("security".into(), "merged".into())],
        &[("app".into(), "1.2.3".into(), 10)],
    );
    assert!(record_names(&text, "v1.2.3-rc.2"));
    assert!(
        !record_names(&text, "v1.2.3-rc.1"),
        "a record of an earlier candidate does not count"
    );
    assert!(!record_names(
        &text.replace("Blockers-Carried: none", "Blockers-Carried: ABC-7"),
        "v1.2.3-rc.2"
    ));
    let twice = format!("{text}\nBlockers-Carried: none\n");
    assert!(!record_names(&twice, "v1.2.3-rc.2"));
}

#[test]
fn a_merge_is_said_once_with_its_machine_fields() {
    let c = merged_comment(
        "https://github.com/example-org/acme/pull/9",
        "main",
        None,
        "To Deploy",
    );
    assert!(c.contains("merged_pr: https://github.com/example-org/acme/pull/9"));
    assert!(c.contains("state: To Deploy"));
    assert!(
        merged_comment("u", "main", Some("someone"), "To Deploy").contains("outside the bridge")
    );
}
