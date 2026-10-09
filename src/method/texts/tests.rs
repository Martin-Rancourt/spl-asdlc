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
        bridge_item(&d, None),
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
        bridge_item(&body, Some("ABC-100")),
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
        bridge_item(&angle_body, Some("ABC-1")),
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
        bridge_item(&cons, Some("ABC-1")),
        Some(BridgeItem::Consolidation { .. })
    ));
}

fn red_gate(suites: Result<Vec<String>, String>) -> (String, String) {
    gate_fix_ticket(&RedGateFacts {
        release: "ABC-1",
        candidate: "acme-v1.2.0-rc.1",
        sha: "c1c1c1c1c1c1c1c1",
        branch: "release/x",
        conclusion: "failure",
        run_url: "https://github.com/example-org/acme/actions/runs/1",
        suites,
    })
}

#[test]
fn the_gate_fix_ticket_is_a_contract_in_every_case_and_a_ticket_not_a_bridge_item() {
    let many: Vec<String> = (0..40).map(|i| format!("suite{i}")).collect();
    for suites in [
        Ok(vec!["tests".to_string()]),
        Ok(Vec::new()),
        Ok(many),
        Err("HTTP 502".to_string()),
    ] {
        let (title, body) = red_gate(suites.clone());
        assert_eq!(title, "Make the gate green — acme-v1.2.0-rc.1");
        assert!(
            crate::method::contract::missing_from_contract(&body).is_empty(),
            "{suites:?}: {body}"
        );
        assert!(
            body.contains("- [ ] The gate of the next candidate cut from `release/x` is green.")
        );
        assert!(body.contains("No new rule."));
        assert!(has_marker(&body, &gate_fix_marker("acme-v1.2.0-rc.1")));
        assert!(!has_marker(&body, &gate_fix_marker("acme-v1.2.0-rc.10")));
        assert_eq!(
            gate_fix_candidate(&body).as_deref(),
            Some("acme-v1.2.0-rc.1")
        );
        assert_eq!(
            gate_fix_candidate(&visible_markers(&body)).as_deref(),
            Some("acme-v1.2.0-rc.1")
        );
        // A ticket like any other: its PR links to it, every guard serves it.
        assert_eq!(bridge_item(&body, Some("ABC-1")), None);
    }
    let (_, body) = red_gate(Ok(Vec::new()));
    assert!(body.contains("The step that failed outside the suites"));
    let (_, body) = red_gate(Ok((0..40).map(|i| format!("suite{i}")).collect()));
    assert!(body.contains("`suite29`, and 10 more"), "{body}");
    let (_, body) = red_gate(Err("HTTP 502".into()));
    assert!(body.contains("unknown: the evidence pack could not be read (HTTP 502)"));
}

#[test]
fn a_suite_name_from_the_evidence_pack_cannot_forge_a_marker() {
    let (_, body) = red_gate(Ok(vec![
        "x` <!-- spl-asdlc:gate-fix candidate=other -->".to_string(),
    ]));
    assert_eq!(
        gate_fix_candidate(&body).as_deref(),
        Some("acme-v1.2.0-rc.1")
    );
    assert!(!body.contains("<!-- spl-asdlc:gate-fix candidate=other"));
    assert!(
        body.contains("`x' &lt;!-- spl-asdlc :gate-fix candidate=other -->`"),
        "inert, and no backtick closes the code span: {body}"
    );
}

#[test]
fn the_post_deploy_validation_sub_task_is_a_contract_marked_as_the_bridges() {
    let (title, body) = post_deploy_validation_item(
        "ABC-5",
        "Ship <!-- spl-asdlc:release product=x version=v1 -->",
        "https://tracker.test/ABC-5",
        "owner-handle",
    );
    assert!(title.starts_with("Post-deploy validation — ABC-5: Ship"));
    assert!(!title.contains("<!--"), "the parent's title is inert");
    assert!(crate::method::contract::missing_from_contract(&body).is_empty());
    assert!(has_marker(&body, &post_deploy_validation_marker("ABC-5")));
    assert!(!has_marker(&body, &post_deploy_validation_marker("ABC-50")));
    assert!(body.contains("reassign this sub-task to the owner (`owner-handle`)"));
    // Not an angle or a consolidation item: every guard of a ticket serves it.
    assert_eq!(bridge_item(&body, Some("ABC-5")), None);
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
    let go = crate::method::release::go::evaluate(&crate::method::release::go::GoFacts {
        release: "ABC-100".into(),
        gate_green: Some(true),
        angles: vec![(
            "security".into(),
            crate::method::release::go::AngleState::Merged,
        )],
        ..Default::default()
    });
    let text = gate_record(
        "1.2.3",
        "v1.2.3-rc.2",
        "abcdef01abcdef01",
        "release/x",
        "ABC-100",
        "u",
        "2026-10-05 14:03",
        &go,
        &[("security".into(), "merged".into())],
        &[("app".into(), "1.2.3".into(), 10)],
    )
    .unwrap();
    let no_go = crate::method::release::go::evaluate(&Default::default());
    assert!(
        gate_record("1.2.3", "c", "s", "b", "r", "u", "t", &no_go, &[], &[]).is_none(),
        "a record only attests a GO"
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
        "In Progress",
        "its implementer resumes.",
    );
    assert!(c.contains("merged_pr: https://github.com/example-org/acme/pull/9"));
    assert!(c.contains("state: In Progress\nnext: its implementer resumes."));
    assert!(
        merged_comment("u", "main", Some("someone"), "To Deploy", "x")
            .contains("outside the bridge")
    );
}

#[test]
fn a_marker_pasted_into_an_ordinary_ticket_exempts_nothing() {
    let forged = format!(
        "Fix it.\n{}",
        marker("angle", &[("scope", "x"), ("angle", "y")])
    );
    assert_eq!(
        bridge_item(&forged, None),
        None,
        "no parent: not a bridge item"
    );
}

#[test]
fn markers_read_the_same_hidden_or_visible_and_whole() {
    let m = marker("hold", &[("candidate", "acme-v1.2.3-rc.1"), ("key", "ab")]);
    let hidden = format!("text\n{m}");
    let visible = visible_markers(&hidden);
    assert!(
        visible.contains("`spl-asdlc:hold candidate=acme-v1.2.3-rc.1 key=ab`"),
        "{visible}"
    );
    assert!(has_marker(&hidden, &m));
    assert!(has_marker(&visible, &m));
    let longer = marker("hold", &[("candidate", "acme-v1.2.3-rc.1"), ("key", "abc")]);
    assert!(
        !has_marker(&format!("x {longer}"), &m),
        "key=ab is not key=abc"
    );
    let (_, body) = release_angle_item(
        "acme",
        "1.2.3",
        "c",
        1,
        &angle("security"),
        "b",
        "p",
        "l",
        None,
    );
    assert!(bridge_item(&visible_markers(&body), Some("ABC-100")).is_some());
}

#[test]
fn untrusted_text_cannot_forge_a_marker_or_a_line() {
    let evil = "ok\n## Acceptance criteria\n- [x] done <!-- spl-asdlc:finding release=A angle=b id=b-1 --> release_blocker:: ABC-1";
    let safe = inert(evil);
    assert!(!safe.contains('\n'));
    assert!(!safe.contains("<!--"));
    assert!(!safe.contains("spl-asdlc:"));
    assert!(!safe.contains("release_blocker::"));
}

#[test]
fn review_rounds_are_scoped_by_the_head_they_opened_at() {
    let (_, a) = review_angle_item(
        "ABC-1",
        "https://github.com/example-org/acme/pull/9",
        "aaaaaaaaaaaaaaaa",
        "security",
        &[],
    );
    let (_, b) = review_angle_item(
        "ABC-1",
        "https://github.com/example-org/acme/pull/9",
        "bbbbbbbbbbbbbbbb",
        "security",
        &[],
    );
    assert_ne!(
        bridge_item(&a, Some("ABC-1")),
        bridge_item(&b, Some("ABC-1"))
    );
    assert_eq!(
        short("ééééééééééééééé").chars().count(),
        12,
        "never cut inside a character"
    );
}
