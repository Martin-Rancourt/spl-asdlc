//! The texts the bridge writes: their markers, their contracts, and the
//! carried text tests (release.md, merge.md).

use super::*;
use crate::github::fake::METHOD_AT as AT;
use crate::method::texts::protocol;

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
        &protocol::Texts::of_this_build(
            AT,
            protocol::Job::ReleaseAngle,
            Some(&angle("security").key),
        ),
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
        None,
        &["it touches reviewed paths: `a`".into()],
        &protocol::Texts::of_this_build(AT, protocol::Job::PrAngle, Some("security")),
    );
    assert!(crate::method::contract::missing_from_contract(&angle_body).is_empty());
    assert!(matches!(
        bridge_item(&angle_body, Some("ABC-1")),
        Some(BridgeItem::Angle { .. })
    ));
    let (title, cons) = consolidation_item(
        "https://github.com/example-org/acme/pull/9",
        "ABC-1 (#9)",
        Consolidating::PullRequest,
        &protocol::Texts::of_this_build(AT, Consolidating::PullRequest.job(), None),
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
        rerun_unseen: None,
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
            !body.contains("The gate of the next candidate cut from"),
            "the next gate is the verification, not a box: {body}"
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
fn the_gate_fix_criterion_is_an_ordinary_box_the_implementer_ticks() {
    for (suites, criterion) in [
        (
            Ok(vec!["tests".to_string()]),
            "- [ ] The cause of each failing suite is fixed in a PR whose base is `release/x`: `tests`.",
        ),
        (
            Err("HTTP 502".to_string()),
            "- [ ] The cause of the suites that failed in the gate run https://github.com/example-org/acme/actions/runs/1 (unknown here: the evidence pack could not be read) is fixed in a PR whose base is `release/x`.",
        ),
    ] {
        let (_, body) = red_gate(suites);
        // SID-1203: the box is met by the fix's PR, so it is ticked before
        // the merge; the next gate checks the suites.
        assert!(
            body.contains("Tick the box once the fix's PR is ready, before its merge"),
            "{body}"
        );
        // One box, the failing suites on the release branch, and no marker
        // that would set it apart: the completeness rule reads it as any box.
        assert_eq!(
            crate::method::contract::open_before_deploy(&body),
            Some(vec![criterion.to_string()]),
            "{body}"
        );
        assert!(!body.contains("bridge-verified"), "{body}");
        let ticked = body.replacen("- [ ] ", "- [x] ", 1);
        assert_eq!(
            crate::method::contract::open_before_deploy(&ticked),
            Some(Vec::new()),
            "ticked by the implementer, the ticket is complete"
        );
    }
}

#[test]
fn a_gate_fix_ticket_opened_past_the_bounded_wait_says_no_rerun_was_seen() {
    let (_, body) = gate_fix_ticket(&RedGateFacts {
        release: "ABC-1",
        candidate: "acme-v1.2.0-rc.1",
        sha: "c1c1c1c1c1c1c1c1",
        branch: "release/x",
        conclusion: "failure",
        run_url: "https://github.com/example-org/acme/actions/runs/1",
        suites: Ok(vec!["tests".to_string()]),
        rerun_unseen: Some("no run of the automatic rerun workflow showed up within 30 minutes"),
    });
    assert!(
        body.contains("- Automatic rerun: no run of the automatic rerun workflow showed up within 30 minutes; check the rerun workflow.\n"),
        "{body}"
    );
    assert!(crate::method::contract::has_contract(&body));
    let (_, settled) = red_gate(Ok(vec!["tests".to_string()]));
    assert!(!settled.contains("Automatic rerun:"));
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
fn the_release_consolidation_item_is_the_overall_review_after_every_angle() {
    // BR-ASDLC-020.
    let (title, body) = consolidation_item(
        "acme-v1.2.0-rc.1",
        "acme v1.2.0 (rc.1)",
        Consolidating::Candidate,
        &protocol::Texts::of_this_build(AT, Consolidating::Candidate.job(), None),
    );
    assert_eq!(title, "Consolidation — acme v1.2.0 (rc.1)");
    assert!(crate::method::contract::missing_from_contract(&body).is_empty());
    assert_eq!(
        bridge_item(&body, Some("ABC-1")),
        Some(BridgeItem::Consolidation {
            scope: "acme-v1.2.0-rc.1".into()
        })
    );
    assert!(body.contains("once every angle PR of the candidate is merged"));
    assert!(body.contains("the coordinator reviews each one as it comes"));
    assert!(body.contains("tag the next candidate"));
}

#[test]
fn a_validation_sub_task_is_recognised_by_its_marker_whatever_key_it_names() {
    for key in ["ABC-5", "SID-1185"] {
        let m = post_deploy_validation_marker(key);
        assert!(is_post_deploy_validation(&format!("{m}\n\n## Goal\nx")));
        assert!(is_post_deploy_validation(&visible_markers(&m)));
    }
    assert!(!is_post_deploy_validation("## Goal\nShip it."));
    let comment = post_deploy_validation_retired_comment(Some("ABC-5"));
    assert!(
        !is_post_deploy_validation(&comment),
        "the comment is not a sub-task"
    );
    assert!(has_marker(
        &comment,
        &post_deploy_validation_retired_marker()
    ));
    assert!(comment.contains("`## Post deploy` boxes of ABC-5"));
    assert!(post_deploy_validation_retired_comment(None).contains("of its parent ticket"));
}

#[test]
fn the_candidates_log_grows_once_per_candidate() {
    let d = release_description("acme", "1.2.3", "l", &[]);
    let line = candidate_log_line("acme-v1.2.3-rc.1", "routine", None);
    let once = append_candidate(&d, "acme-v1.2.3-rc.1", &line);
    assert_eq!(append_candidate(&once, "acme-v1.2.3-rc.1", &line), once);
    let twice = append_candidate(
        &once,
        "acme-v1.2.3-rc.2",
        &candidate_log_line("acme-v1.2.3-rc.2", "sensible", Some("acme-v1.2.3-rc.1")),
    );
    let section = candidates_section(&twice);
    assert!(section.contains("first candidate"));
    assert!(section.contains("after `acme-v1.2.3-rc.1`"));
}

/// The `## Candidates` section of a description.
fn candidates_section(description: &str) -> &str {
    description
        .split("## Candidates")
        .nth(1)
        .unwrap()
        .split("## Scope")
        .next()
        .unwrap()
}

#[test]
fn a_listed_candidate_is_found_by_its_tag_whatever_the_tracker_did_to_the_markdown() {
    let rc1 = candidate_log_line("acme-v1.2.3-rc.1", "routine", None);
    let rc2 = candidate_log_line("acme-v1.2.3-rc.2", "sensible", Some("acme-v1.2.3-rc.1"));
    let rc3 = candidate_log_line("acme-v1.2.3-rc.3", "routine", Some("acme-v1.2.3-rc.2"));
    // As Linear stores it: `*` bullets, blank lines between the items and
    // after the headings, trailing spaces; the other bullets too.
    for bullet in ["*", "-", "+"] {
        let stored = format!(
            "`spl-asdlc:release product=acme version=v1.2.3`\n\n## Goal\n\nDeliver it.\n\n\
             ## Candidates\n\n{bullet} `acme-v1.2.3-rc.1` — class `routine` — first candidate  \n\n\n\
             {bullet} `acme-v1.2.3-rc.2` — class `sensible` — after `acme-v1.2.3-rc.1`\t\n\n\
             ## Scope\n\nThe label `l` groups the release's issues.\n"
        );
        assert_eq!(
            append_candidate(&stored, "acme-v1.2.3-rc.1", &rc1),
            stored,
            "{bullet}"
        );
        assert_eq!(
            append_candidate(&stored, "acme-v1.2.3-rc.2", &rc2),
            stored,
            "{bullet}"
        );
        // A new candidate is still appended, once.
        let next = append_candidate(&stored, "acme-v1.2.3-rc.3", &rc3);
        assert_eq!(
            candidates_section(&next)
                .matches("`acme-v1.2.3-rc.3`")
                .count(),
            1,
            "{next}"
        );
        assert_eq!(append_candidate(&next, "acme-v1.2.3-rc.3", &rc3), next);
        assert!(next.contains("\n\n## Scope\n"), "{next}");
    }
}

#[test]
fn a_candidate_is_listed_by_its_own_tag_only() {
    // rc.2's line names rc.1 as its previous candidate: that does not list rc.1.
    let d = release_description("acme", "1.2.3", "l", &[]);
    let rc2 = candidate_log_line("acme-v1.2.3-rc.2", "sensible", Some("acme-v1.2.3-rc.1"));
    let d = append_candidate(&d, "acme-v1.2.3-rc.2", &rc2);
    let rc1 = candidate_log_line("acme-v1.2.3-rc.1", "routine", None);
    let d = append_candidate(&d, "acme-v1.2.3-rc.1", &rc1);
    assert!(candidates_section(&d).contains("first candidate"), "{d}");
    // A tag is matched whole: rc.1 does not list rc.10.
    let rc10 = candidate_log_line("acme-v1.2.3-rc.10", "routine", Some("acme-v1.2.3-rc.2"));
    let d10 = append_candidate(&d, "acme-v1.2.3-rc.10", &rc10);
    assert!(
        candidates_section(&d10).contains("`acme-v1.2.3-rc.10` —"),
        "{d10}"
    );
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
        AngleScope::LighterPass,
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
        AngleScope::Configured,
    );
    assert!(c.contains("- First candidate."));
    assert!(!has_marker(&c, &no_qa_angle_marker("acme-v1.2.3-rc.1")));
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
        paths_since_release: vec![".spl-asdlc/business-rules/BR-ACME-001.md".into()],
        paths_since_candidate: None,
        gate_url: Some("https://github.com/example-org/acme/actions/runs/1".into()),
        evidence_url: Some("https://github.com/example-org/acme/actions/runs/1#artifacts".into()),
        tickets: vec![
            BriefTicket { id: "ABC-1".into(), title: "One".into(), state: "To Deploy".into(), prs: vec![9], description: Some("## Acceptance criteria\n- [x] a\n## Post deploy\n- [ ] verified\n## Business rules\n- BR-ACME-001\n".into()) },
            BriefTicket { id: "ABC-2".into(), title: "Two".into(), state: "?".into(), prs: vec![], description: None },
        ],
        untracked: vec!["PR #12".into(), "commit abcdef01abcd".into()],
        rule_files_touched: vec![".spl-asdlc/business-rules/BR-ACME-001.md".into()],
        no_qa_angle: false,
    };
    let text = brief(&b);
    assert!(text.contains("`acme-v1.2.3-rc.1` not found — diff cannot be computed"));
    assert!(text.contains("- [ ] verified (post deploy)"));
    assert!(text.contains("Business rules cited: BR-ACME-001"));
    assert!(text.contains("(ticket unreadable: its criteria are not shown)"));
    assert!(text.contains("Each rule touched must have a named test that is green."));
    assert!(
        !text.contains("Not in the release note"),
        "the notes follow the graph: no commit of the range is left out of them"
    );
    b.previous_candidate = None;
    assert!(brief(&b).contains("first candidate of this version"));
    // A first release is measured from the repository's root, and says so.
    b.last_release = None;
    let text = brief(&b);
    assert!(text.contains(
        "- Diff since the repository's root: first release, no earlier release to measure from — 1 file(s)\n"
    ));
    assert!(text.contains("<summary>Paths since the repository's root (1)</summary>"));
    assert!(!text.contains("last published release"), "{text}");
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
fn a_merge_is_said_once_with_its_machine_fields() {
    let bridge = MergedBy::Bridge {
        approved_by: Some("coordinator"),
        up_to_date: true,
    };
    let c = merged_comment(
        "https://github.com/example-org/acme/pull/9",
        "main",
        &bridge,
        "In Progress",
        "its implementer resumes.",
    );
    assert!(c.contains("merged_pr: https://github.com/example-org/acme/pull/9"));
    assert!(c.contains("merged_by: bridge\n"));
    assert!(c.contains("state: In Progress\nnext: its implementer resumes."));
    let outside = merged_comment("u", "main", &MergedBy::Outside("someone"), "To Deploy", "x");
    assert!(outside.contains("outside the bridge"));
    assert!(outside.contains("merged_by: someone\n"));
}

#[test]
fn the_merge_comment_states_only_the_conditions_that_applied() {
    let say = |approved_by, up_to_date| {
        merged_comment(
            "u",
            "main",
            &MergedBy::Bridge {
                approved_by,
                up_to_date,
            },
            "To Deploy",
            "x",
        )
    };
    assert!(say(Some("coordinator"), true)
        .starts_with("u was green, approved by `coordinator` and up to date with `main`, so the bridge merged it."));
    let not_current = say(Some("coordinator"), false);
    assert!(not_current.starts_with("u was green and approved by `coordinator`, so"));
    assert!(!not_current.contains("up to date"));
    let unreviewed = say(None, false);
    assert!(unreviewed.starts_with("u was green; no review angle applies to it"));
    assert!(!unreviewed.contains("approved"), "{unreviewed}");
    assert!(!unreviewed.contains("up to date"), "{unreviewed}");
    assert!(say(None, true).starts_with("u was green and up to date with `main`; no review angle"));
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
        &protocol::Texts::of_this_build(
            AT,
            protocol::Job::ReleaseAngle,
            Some(&angle("security").key),
        ),
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
        None,
        &[],
        &protocol::Texts::of_this_build(AT, protocol::Job::PrAngle, Some("security")),
    );
    let (_, b) = review_angle_item(
        "ABC-1",
        "https://github.com/example-org/acme/pull/9",
        "bbbbbbbbbbbbbbbb",
        "security",
        None,
        &[],
        &protocol::Texts::of_this_build(AT, protocol::Job::PrAngle, Some("security")),
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

// ---- BR-ASDLC-015: an item the bridge serves carries its protocol ----

const PR: &str = "https://github.com/example-org/acme/pull/9";

/// The one section of each contract heading an item has, and its protocol
/// under `## Protocol`, with nothing of the contract below it.
fn assert_one_contract(body: &str) {
    use crate::method::contract::*;
    use crate::method::markdown::sections;
    assert!(missing_from_contract(body).is_empty(), "{body}");
    assert_eq!(sections(body, is_goal_heading).len(), 1, "{body}");
    assert_eq!(sections(body, is_acceptance_heading).len(), 1, "{body}");
    assert_eq!(sections(body, is_business_rules_heading).len(), 1, "{body}");
    let protocol = body.split("\n## Protocol\n").nth(1).expect("a protocol");
    assert!(
        crate::method::markdown::checkboxes(protocol).is_empty(),
        "the protocol adds no box: {protocol}"
    );
    assert!(
        !protocol
            .lines()
            .any(|l| l.starts_with("# ") || l.starts_with("## ")),
        "every heading of the protocol is demoted below `## Protocol`: {protocol}"
    );
    assert!(
        body.len() <= protocol::ITEM_LIMIT,
        "{} characters",
        body.len()
    );
}

/// A protocol's text, its headings demoted as the item embeds them.
fn embedded(text: &str) -> String {
    protocol::demote(text, 2).trim().to_string()
}

#[test]
fn a_pr_review_angle_sub_item_embeds_the_pr_review_protocol_and_its_angles_notes() {
    let (_, body) = review_angle_item(
        "ABC-1",
        PR,
        "abcdef01abcdef01",
        "security",
        Some(".spl-asdlc/angles/security.md"),
        &[],
        &protocol::Texts::of_this_build(AT, protocol::Job::PrAngle, Some("security")),
    );
    assert_one_contract(&body);
    assert!(body.contains(&embedded(protocol::REVIEW_PR)), "{body}");
    assert!(
        body.contains(&embedded(protocol::angle_notes("security").unwrap())),
        "{body}"
    );
    assert!(body.contains(&format!("spl-asdlc {}", protocol::VERSION)));
    assert!(
        body.contains(
            "- Project notes: `.spl-asdlc/angles/security.md`, in the project's repository"
        ),
        "the project's notes are linked, never embedded: {body}"
    );
    assert!(
        !body.contains("as `docs/agents/review-pr.md` describes it"),
        "no path the project's repository lacks is the instruction: {body}"
    );
    // What closes it, in the bridge's terms (`close_reviewed_sub_items`).
    assert!(
        body.contains(
            "a submitted review (any state but pending) whose body names this item's key"
        ),
        "{body}"
    );
    assert!(
        body.contains("- [ ] The review is submitted on GitHub, its body naming this item's key.")
    );
}

#[test]
fn a_release_angle_sub_item_embeds_the_release_protocol_its_angles_notes_and_the_report_schema() {
    let (_, body) = release_angle_item(
        "acme",
        "1.2.3",
        "acme-v1.2.3-rc.2",
        2,
        &angle("product"),
        "release/2026-10-05-14-03-utc",
        ".spl-asdlc/qa/acme-v1.2.3-rc.2/product.json",
        "release/acme-v1.2.3",
        None,
        &protocol::Texts::of_this_build(
            AT,
            protocol::Job::ReleaseAngle,
            Some(&angle("product").key),
        ),
    );
    assert_one_contract(&body);
    assert!(body.contains(&embedded(protocol::REVIEW_RELEASE)), "{body}");
    assert!(
        body.contains(&embedded(protocol::angle_notes("product").unwrap())),
        "{body}"
    );
    assert!(
        body.contains("\"schema\": \"spl-asdlc/qa-report/2\""),
        "{body}"
    );
    assert!(body.contains("- Project notes: none"), "{body}");
    assert!(
        body.contains("The bridge sets this sub-item Done when it merges your angle PR into `release/2026-10-05-14-03-utc`"),
        "{body}"
    );
    assert!(
        body.contains("names this item's key in its title or body"),
        "{body}"
    );
}

#[test]
fn a_consolidation_item_embeds_the_consolidation_protocol_and_says_what_closes_it() {
    let (_, pr) = consolidation_item(
        &round_scope(PR, "abcdef01abcdef01"),
        "ABC-1 (abcdef01abcd)",
        Consolidating::PullRequest,
        &protocol::Texts::of_this_build(AT, Consolidating::PullRequest.job(), None),
    );
    assert_one_contract(&pr);
    assert!(pr.contains(&embedded(protocol::CONSOLIDATE)), "{pr}");
    assert!(pr.contains("its part \"For a PR review\""), "{pr}");
    assert!(
        pr.contains("The bridge sets it Done when it merges the PR, which it does once a coordinator's approval stands at the PR's head commit"),
        "{pr}"
    );
    assert!(
        !pr.contains("qa-report/"),
        "a PR's consolidation has no report"
    );

    let (_, rc) = consolidation_item(
        "acme-v1.2.3-rc.1",
        "acme v1.2.3 (rc.1)",
        Consolidating::Candidate,
        &protocol::Texts::of_this_build(AT, Consolidating::Candidate.job(), None),
    );
    assert_one_contract(&rc);
    assert!(rc.contains(&embedded(protocol::CONSOLIDATE)), "{rc}");
    assert!(rc.contains("its part \"For a release candidate\""), "{rc}");
    assert!(rc.contains("\"schema\": \"spl-asdlc/qa-report/2\""), "{rc}");
    assert!(
        rc.contains("The bridge sets it Done when a newer candidate is tagged"),
        "{rc}"
    );
}

#[test]
fn a_project_only_angle_gets_the_generic_protocol_a_line_and_its_linked_notes() {
    let (_, body) = review_angle_item(
        "ABC-1",
        PR,
        "abcdef01abcdef01",
        "accessibility",
        Some(".spl-asdlc/angles/accessibility.md"),
        &[],
        &protocol::Texts::of_this_build(AT, protocol::Job::PrAngle, Some("accessibility")),
    );
    assert_one_contract(&body);
    assert!(body.contains(&embedded(protocol::REVIEW_PR)), "{body}");
    assert!(
        body.contains("The method has no notes for the `accessibility` angle"),
        "{body}"
    );
    assert!(body.contains("`.spl-asdlc/angles/accessibility.md`, in the project's repository"));
    let (_, release) = release_angle_item(
        "acme",
        "1.2.3",
        "c",
        1,
        &angle("accessibility"),
        "b",
        "p",
        "l",
        None,
        &protocol::Texts::of_this_build(
            AT,
            protocol::Job::ReleaseAngle,
            Some(&angle("accessibility").key),
        ),
    );
    assert_one_contract(&release);
    assert!(release.contains("The method has no notes for the `accessibility` angle"));
}

#[test]
fn the_longest_item_stays_within_the_trackers_size() {
    let reasons: Vec<String> = (0..10)
        .map(|i| {
            format!(
                "it touches reviewed paths: `{}` (+99 more)",
                "p/".repeat(40) + &i.to_string()
            )
        })
        .collect();
    let mut longest = 0;
    for (key, _) in protocol::ANGLES {
        let (_, pr) = review_angle_item(
            "ABC-1",
            PR,
            "abcdef01abcdef01",
            key,
            Some(&"n/".repeat(100)),
            &reasons,
            &protocol::Texts::of_this_build(AT, protocol::Job::PrAngle, Some(key)),
        );
        let (_, rc) = release_angle_item(
            "acme",
            "1.2.3",
            "acme-v1.2.3-rc.12",
            12,
            &angle(key),
            "release/2026-10-05-14-03-utc",
            ".spl-asdlc/qa/acme-v1.2.3-rc.12/security.json",
            "release/acme-v1.2.3",
            Some(&"n/".repeat(100)),
            &protocol::Texts::of_this_build(AT, protocol::Job::ReleaseAngle, Some(&angle(key).key)),
        );
        longest = longest.max(pr.len()).max(rc.len());
    }
    for of in [Consolidating::PullRequest, Consolidating::Candidate] {
        let (_, c) = consolidation_item(
            &round_scope(PR, "abcdef01abcdef01"),
            "ABC-1",
            of,
            &protocol::Texts::of_this_build(AT, of.job(), None),
        );
        longest = longest.max(c.len());
    }
    assert!(
        longest <= protocol::ITEM_LIMIT,
        "{longest} characters: trim the protocols, never cut them in the item"
    );
}

#[test]
fn the_markers_carry_no_protocol_text() {
    let (_, body) = review_angle_item(
        "ABC-1",
        PR,
        "abcdef01abcdef01",
        "security",
        None,
        &[],
        &protocol::Texts::of_this_build(AT, protocol::Job::PrAngle, Some("security")),
    );
    let m = marker(
        "angle",
        &[
            ("scope", &round_scope(PR, "abcdef01abcdef01")),
            ("angle", "security"),
        ],
    );
    assert!(body.starts_with(&format!("{m}\n")), "{body}");
    // The same item with another protocol is the same item.
    let older = body.split("\n## Protocol\n").next().unwrap().to_string()
        + "\n## Protocol\n\nAn older protocol.\n";
    assert_eq!(
        bridge_item(&older, Some("ABC-1")),
        bridge_item(&body, Some("ABC-1"))
    );
}

#[test]
fn a_release_with_no_qa_angle_says_so_in_its_texts() {
    let d = release_description("acme", "1.2.3", "l", &[]);
    assert!(d.contains("No QA angle is configured"), "{d}");
    assert!(!d.contains("every angle's PR merged"), "{d}");
    assert!(!d.contains("| Angle | Owner |"), "{d}");

    let c = candidate_comment(
        "acme-v1.2.3-rc.1",
        "routine",
        None,
        None,
        &[],
        &[],
        AngleScope::NoQaAngle,
    );
    assert!(c.contains("No QA angle is configured"), "{c}");
    assert!(c.contains("Angles configured later apply from the next candidate."));
    assert!(has_marker(
        &c,
        &marker("candidate", &[("tag", "acme-v1.2.3-rc.1")])
    ));
    assert!(has_marker(&c, &no_qa_angle_marker("acme-v1.2.3-rc.1")));
    assert!(!has_marker(&c, &no_qa_angle_marker("acme-v1.2.3-rc.10")));
    assert_eq!(
        bridge_item(&c, Some("ABC-1")),
        None,
        "the marker names no item of the bridge's"
    );

    let b = Brief {
        candidate: "acme-v1.2.3-rc.1".into(),
        sha: "abc".into(),
        class: "routine".into(),
        last_release: None,
        previous_candidate: None,
        paths_since_release: vec![],
        paths_since_candidate: None,
        gate_url: Some("https://github.com/example-org/acme/actions/runs/1".into()),
        evidence_url: None,
        tickets: vec![],
        untracked: vec![],
        rule_files_touched: vec![],
        no_qa_angle: true,
    };
    assert!(brief(&b).contains("#### QA angles\n\nNo QA angle is configured"));
    assert!(
        !brief(&Brief {
            no_qa_angle: false,
            ..b
        })
        .contains("No QA angle")
    );
}

#[test]
fn an_identity_marker_is_read_hidden_or_visible_and_never_a_fact_marker() {
    let gate = gate_fix_marker("acme-v1.2.0-rc.1");
    let body = format!("{gate}\n\n## Goal\nMake it green.");
    assert_eq!(identity_marker(&body).as_deref(), Some(gate.as_str()));
    assert_eq!(
        identity_marker(&visible_markers(&body)).as_deref(),
        Some(gate.as_str())
    );
    let pr = pr_ticket_marker("https://github.com/acme/app/pull/7");
    assert_eq!(
        identity_marker(&format!("text\n{pr}")).as_deref(),
        Some(pr.as_str())
    );
    // Facts about an item, never its identity.
    for fact in [
        marker("gate-fix-closed", &[("candidate", "c1"), ("green", "c2")]),
        marker("candidate", &[("tag", "c1")]),
        crate::method::release::note::START.to_string(),
        "release_blocker:: ABC-1".to_string(),
    ] {
        assert_eq!(identity_marker(&fact), None, "{fact}");
    }
    // The identity comment carries it, whole; the restored comment says so
    // without it.
    assert!(has_marker(&identity_comment(&gate), &gate));
    assert!(!has_marker(&marker_restored_comment(), &gate));
    assert!(identity_comment(&gate).contains(KEEP_MARKER));
}
