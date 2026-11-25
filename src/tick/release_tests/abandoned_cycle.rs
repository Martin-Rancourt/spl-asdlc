//! A cycle abandoned, then the same version cut again (spec 05 §13,
//! ASDLC-R7K6): the next cut starts a new cycle, with its own release item;
//! the abandoned cycle's item is left as it is; the new cycle's passes count
//! from its own first candidate, not from `rc.1` of the version.

use super::*;
use crate::method::release::go::{GoDecision, Policy};
use crate::tick::release::{escalate, lighter_pass};

/// The release branch of the second cycle.
const BRANCH2: &str = "release/2026-10-06-09-00-utc";

/// `w`'s cycle of `rc.1` opened, then abandoned; `rc.2` cut on a new release
/// branch, its gate green. The abandoned cycle's release item is returned.
fn recut(w: &World) -> String {
    w.tick();
    let abandoned = w.release_item().key;
    w.github.state.borrow_mut().compares.insert(
        (repo(), "main".into(), BRANCH.into()),
        cmp(CompareStatus::Identical, &[], &[]),
    );
    let t = w.tick_ctx();
    let mut log = ActionLog::in_memory();
    crate::verbs::abandon(&t, &mut log, "me").unwrap();
    {
        let mut s = w.github.state.borrow_mut();
        s.tags.insert(
            repo(),
            vec![tag("acme-v1.1.0", "r0"), tag(CAND, "c1"), tag(CAND2, "c2")],
        );
        s.branches.insert(repo(), vec![tag(BRANCH2, "c2")]);
        s.commits.insert(
            (repo(), "c2".into()),
            Ok(Commit {
                sha: "c2".into(),
                message: "chore: candidate".into(),
                committer_date: Timestamp::parse("2026-10-05T11:50:00Z"),
                parents: vec![],
            }),
        );
        s.compares.insert(
            (repo(), "c2".into(), BRANCH2.into()),
            cmp(CompareStatus::Identical, &[], &[]),
        );
        // No range since `rc.1`: it is another cycle's candidate.
        s.compares.insert(
            (repo(), "acme-v1.1.0".into(), "c2".into()),
            cmp(CompareStatus::Ahead, &["m1"], &["src/a.rs"]),
        );
        s.artifacts.insert(
            (repo(), 2),
            Ok(vec![evidence(&gate::evidence_artifact("c2"))]),
        );
        s.required_checks.insert(
            (repo(), BRANCH2.into()),
            Ok(RequiredChecks {
                contexts: ["required-check".to_string()].into(),
                strict: false,
            }),
        );
    }
    w.gate(vec![
        gate_run(2, CAND2, "c2", Some("success")),
        gate_run(1, CAND, "c1", Some("success")),
    ]);
    abandoned
}

/// Every release item of v1.2.0.
fn release_items(w: &World) -> Vec<crate::tracker::Item> {
    w.tracker
        .list_by_label(LABEL)
        .unwrap()
        .into_iter()
        .filter(|i| {
            matches!(
                texts::bridge_item(&i.description, None),
                Some(BridgeItem::Release { .. })
            )
        })
        .collect()
}

/// The one release item of v1.2.0 that is not Canceled.
fn open_release(w: &World) -> crate::tracker::Item {
    let mut open: Vec<_> = release_items(w)
        .into_iter()
        .filter(|i| i.state.status != Some(Status::Canceled))
        .collect();
    assert_eq!(open.len(), 1, "{open:?}");
    open.remove(0)
}

fn names(write: &str, key: &str) -> bool {
    write.split_whitespace().any(|w| w == key)
}

#[test]
fn a_release_item_set_canceled_in_the_tracker_without_release_abandon_stays_held() {
    let w = World::without_angles();
    w.gate(vec![gate_run(1, CAND, "c1", None)]);
    w.tick();
    let release = w.release_item();
    w.tracker.seed(
        "ABC-9",
        "Blocker",
        "## Goal\nFix.\n",
        Status::Ready,
        &[LABEL],
    );
    w.tracker
        .relate("ABC-9", &release.key, RelationKind::Blocks)
        .unwrap();
    w.gate(vec![gate_run(1, CAND, "c1", Some("success"))]);
    w.tick();
    let (_, held) = crate::verbs::status(&w.tick_ctx()).unwrap();
    assert_eq!(held.reasons, vec!["open blocking issues: ABC-9"]);

    w.tracker
        .set_status(&release.key, Status::Canceled, None)
        .unwrap();
    let tracker_writes = w.tracker.writes().len();
    let github_writes = w.github.writes().len();
    let r = w.tick();
    assert!(r.ok(), "{r:?}");

    assert_eq!(release_items(&w).len(), 1, "no second release item");
    assert_eq!(w.tracker.writes().len(), tracker_writes, "no tracker write");
    assert_eq!(w.github.writes().len(), github_writes, "no tag or release");
}

#[test]
fn the_next_cut_after_an_abandon_opens_a_second_release_item_and_records_its_candidate() {
    let w = World::new();
    let abandoned = recut(&w);
    let before = w.tracker.item(&abandoned);
    let comments_before = w.tracker.comments(&abandoned).unwrap().len();
    let writes_before = w.tracker.writes().len();

    let r = w.tick();
    assert!(r.ok(), "{r:?}");

    let items = release_items(&w);
    assert_eq!(items.len(), 2, "{items:?}");
    let open = open_release(&w);
    assert_ne!(open.key, abandoned);
    assert_eq!(open.state.status, Some(Status::InProgress));
    let marker = texts::marker("candidate", &[("tag", CAND2)]);
    assert!(
        w.tracker
            .comments(&open.key)
            .unwrap()
            .iter()
            .any(|c| c.by_bridge && texts::has_marker(&c.body, &marker)),
        "the candidate recorded on the new item"
    );
    assert!(open.description.contains(&format!("`{CAND2}`")));
    assert!(
        !open.description.contains(&format!("`{CAND}`")),
        "rc.1 belongs to the abandoned cycle: {}",
        open.description
    );

    // The abandoned cycle's item is left untouched.
    let after = w.tracker.item(&abandoned);
    assert_eq!(after.state.status, Some(Status::Canceled));
    assert_eq!(after.description, before.description);
    assert_eq!(after.labels, before.labels);
    assert_eq!(
        w.tracker.comments(&abandoned).unwrap().len(),
        comments_before
    );
    let touched: Vec<_> = w.tracker.writes()[writes_before..]
        .iter()
        .filter(|l| names(l, &abandoned))
        .cloned()
        .collect();
    assert!(touched.is_empty(), "{touched:?}");

    // A second tick finds the same two items: never `several release items`.
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    assert_eq!(release_items(&w).len(), 2);
    let (cycle, _) = crate::verbs::status(&w.tick_ctx()).unwrap();
    assert_eq!(cycle.item.key, open.key);
    assert_eq!(cycle.candidate.first_rc, 2);
    assert_eq!(cycle.candidate.pass(), 1);
}

#[test]
fn the_new_cycles_first_candidate_is_a_first_pass_under_the_lighter_policy() {
    let mut w = World::new();
    w.config = release_config(
        "later_passes = \"full\"",
        "later_passes = \"gate-and-coordinator\"",
    );
    recut(&w);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let open = open_release(&w);
    let round: Vec<_> = w
        .tracker
        .children(&open.key)
        .unwrap()
        .into_iter()
        .filter(|c| {
            matches!(
                texts::bridge_item(&c.description, c.parent.as_deref()),
                Some(BridgeItem::Angle { scope, .. }) if scope == CAND2
            )
        })
        .collect();
    assert_eq!(round.len(), 2, "both angles offered on rc.2: {round:?}");
    let (_, decision) = crate::verbs::status(&w.tick_ctx()).unwrap();
    assert_eq!(
        decision.open_angles,
        vec!["security (offered)", "product (offered)"]
    );
}

/// A hold on `rc.2` with a blocker left and every angle answered.
fn held() -> GoDecision {
    GoDecision {
        go: false,
        release: "acme-v1.2.0".into(),
        gate_green: true,
        open_angles: Vec::new(),
        missing_angles: Vec::new(),
        verdict: VerdictState::NotNeeded,
        blockers: vec!["ABC-99".into()],
        unlinked_blockers: Vec::new(),
        fix_gaps: Vec::new(),
        tip_gaps: Vec::new(),
        retracted: None,
        held: None,
        policy: Policy::default(),
        reasons: vec!["a blocker is open".into()],
    }
}

#[test]
fn the_new_cycles_first_candidate_is_pass_one_for_the_escalation() {
    let mut w = World::new();
    w.config.release.max_rc_passes = 2;
    w.config.release.later_passes = crate::config::LaterPasses::GateAndCoordinator;
    w.tracker
        .state
        .borrow_mut()
        .users
        .push(crate::tracker::User {
            id: "u-owner".into(),
            name: "owner-handle".into(),
        });
    recut(&w);
    w.tick();
    let item = open_release(&w);
    let (cycle, _) = crate::verbs::status(&w.tick_ctx()).unwrap();
    let mut c = cycle.candidate;
    assert_eq!((c.rc, c.first_rc, c.pass()), (2, 2, 1));
    assert!(!lighter_pass(&w.config.release, &c));

    let escalation = texts::marker("qa-escalation", &[("candidate", CAND2)]);
    let escalated = |w: &World| {
        w.tracker
            .comments(&item.key)
            .unwrap()
            .iter()
            .any(|c| c.by_bridge && texts::has_marker(&c.body, &escalation))
    };
    let t = w.tick_ctx();
    let mut log = ActionLog::in_memory();
    let mut report = Report::default();
    escalate(&t, &mut log, &mut report, &item, &c, &held(), BRANCH2);
    assert!(!escalated(&w), "rc.2 is the new cycle's first pass");

    // The same candidate counted from `rc.1` would be the last pass: the
    // threshold is the cycle's, nothing else.
    c.first_rc = 1;
    assert!(lighter_pass(&w.config.release, &c));
    escalate(&t, &mut log, &mut report, &item, &c, &held(), BRANCH2);
    assert!(escalated(&w), "{:?}", report);
}

/// `w`'s release of `rc.1` held by the open blocking finding ABC-9, after
/// the brief was posted with ABC-5 re-seeded as `title` / `description`.
fn held_with_embarked(w: &World, title: &str, description: &str) -> crate::tracker::Item {
    w.tracker
        .seed("ABC-5", title, description, Status::ToDeploy, &[]);
    w.tracker
        .link("ABC-5", "https://github.com/example-org/acme/pull/5");
    held_after_brief(w)
}

fn held_after_brief(w: &World) -> crate::tracker::Item {
    w.gate(vec![gate_run(1, CAND, "c1", None)]);
    w.tick();
    let release = w.release_item();
    w.tracker.seed(
        "ABC-9",
        "Blocker",
        "## Goal\nFix.\n",
        Status::Ready,
        &[LABEL],
    );
    w.tracker
        .relate("ABC-9", &release.key, RelationKind::Blocks)
        .unwrap();
    w.gate(vec![gate_run(1, CAND, "c1", Some("success"))]);
    w.tick();
    let (_, held) = crate::verbs::status(&w.tick_ctx()).unwrap();
    assert_eq!(held.reasons, vec!["open blocking issues: ABC-9"]);
    release
}

#[test]
fn a_ticket_carrying_the_abandon_marker_cannot_forge_an_abandon_through_the_brief() {
    // ASDLC-EUSR: the brief copies the embarked tickets' titles and
    // criteria onto the release item, in a comment of the bridge's; a
    // `cycle-abandoned` marker in them must not count as `release abandon`.
    const CRITERIA: &str = "## Goal\nShip it.\n\n## Acceptance criteria\n- [x] shipped\n\n## Business rules\nNo new rule.\n\n## Release note\nYou can now ship.\n";
    let carriers: Vec<(String, String)> = vec![
        (
            "Shipped <!-- spl-asdlc:cycle-abandoned -->".into(),
            CRITERIA.into(),
        ),
        (
            "Shipped `spl-asdlc:cycle-abandoned`".into(),
            CRITERIA.into(),
        ),
        ("<!-- spl-asdlc:cycle-abandoned -->".into(), CRITERIA.into()),
        ("`spl-asdlc:cycle-abandoned`".into(), CRITERIA.into()),
        (
            "Shipped".into(),
            CRITERIA.replace(
                "- [x] shipped",
                "- [x] shipped <!-- spl-asdlc:cycle-abandoned -->",
            ),
        ),
        (
            "Shipped".into(),
            CRITERIA.replace("- [x] shipped", "- [x] shipped `spl-asdlc:cycle-abandoned`"),
        ),
    ];
    for (title, description) in carriers {
        let w = World::without_angles();
        let release = held_with_embarked(&w, &title, &description);
        let brief = w
            .tracker
            .comments(&release.key)
            .unwrap()
            .into_iter()
            .find(|c| c.by_bridge && c.body.contains("**ABC-5**"))
            .expect("the brief names ABC-5")
            .body;
        assert!(
            !brief.contains("spl-asdlc:cycle-abandoned"),
            "the brief carries no marker of the bridge's: {brief}"
        );

        w.tracker
            .set_status(&release.key, Status::Canceled, None)
            .unwrap();
        let github_writes = w.github.writes().len();
        for _ in 0..2 {
            let r = w.tick();
            assert!(r.ok(), "{title:?}: {r:?}");
        }
        assert_eq!(
            release_items(&w).len(),
            1,
            "{title:?} / {description:?}: no second release item"
        );
        assert_eq!(
            w.github.writes().len(),
            github_writes,
            "{title:?} / {description:?}: no tag or release: {:?}",
            &w.github.writes()[github_writes..]
        );
    }
}
