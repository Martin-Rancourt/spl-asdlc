//! A patch cycle beside an open cycle (ASDLC-MG26): the cut workflow tagged
//! the patch's first candidate from the release in production, and the
//! release pass carries it with its own release item while the open cycle
//! goes on, untouched.

use super::*;
use crate::github::AnnotatedTag;

const PATCH_BRANCH: &str = "release/2026-10-05-16-00-utc";
const PATCH_CAND: &str = "acme-v1.1.1-rc.1";
const PATCH_LABEL: &str = "release/acme-v1.1.1";

/// `acme-v1.1.0` in production, the open cycle of `World::new` on `BRANCH`,
/// and the patch `acme-v1.1.1-rc.1` cut from `acme-v1.1.0` on its own branch,
/// carrying PR 7 cherry-picked. `annotated` says whether its tag carries
/// the cut workflow's `Release-Patch-From:` line.
fn with_patch(annotated: bool) -> World {
    let w = World::new();
    w.in_production(Vec::new());
    w.tracker.seed(
        "ABC-7",
        "Fixed",
        "## Goal\nFix it.\n\n## Acceptance criteria\n- [x] fixed\n\n## Business rules\nNo new rule.\n\n## Release note\nIt works again.\n",
        Status::ToDeploy,
        &[],
    );
    w.tracker
        .link("ABC-7", "https://github.com/example-org/acme/pull/7");
    {
        let mut s = w.github.state.borrow_mut();
        let message = if annotated {
            "Release-Class: routine\nRelease-Patch-From: acme-v1.1.0\nRelease-Patch-Pulls: 7\n"
        } else {
            "Release-Class: routine\n"
        };
        s.tag_objects.insert(
            (repo(), "patch-tag-object".into()),
            AnnotatedTag {
                name: PATCH_CAND.into(),
                message: message.into(),
                tagger_date: Timestamp::parse("2026-10-05T16:00:00Z"),
                target_sha: "p1".into(),
                target_type: ObjectType::Commit,
            },
        );
        s.tags.get_mut(&repo()).unwrap().push(GitRef {
            name: PATCH_CAND.into(),
            sha: "patch-tag-object".into(),
            object_type: ObjectType::Tag,
        });
        s.branches
            .get_mut(&repo())
            .unwrap()
            .push(tag(PATCH_BRANCH, "p1"));
        // Each branch holds its own candidate only.
        s.compares.insert(
            (repo(), "p1".into(), PATCH_BRANCH.into()),
            cmp(CompareStatus::Identical, &[], &[]),
        );
        s.compares.insert(
            (repo(), "p1".into(), BRANCH.into()),
            cmp(CompareStatus::Diverged, &[], &[]),
        );
        s.compares.insert(
            (repo(), "c1".into(), PATCH_BRANCH.into()),
            cmp(CompareStatus::Diverged, &[], &[]),
        );
        // The diff since production: the cherry-pick of PR 7's merge commit.
        let mut range = cmp(CompareStatus::Ahead, &["p1"], &["src/fix.rs"]).unwrap();
        range.commits[0].picked_from = Some("m7".into());
        s.compares
            .insert((repo(), "acme-v1.1.0".into(), "p1".into()), Ok(range));
        s.commit_pulls.insert(
            (repo(), "m7".into()),
            Ok(vec![CommitPull {
                number: 7,
                url: "https://github.com/example-org/acme/pull/7".into(),
                title: "fix: seven".into(),
                merge_commit_sha: Some("m7".into()),
                base_ref: "main".into(),
                head_ref: "fix/seven".into(),
                ..CommitPull::default()
            }]),
        );
        s.runs.insert(
            (repo(), "release-gate.yml".into()),
            Ok(vec![
                gate_run(1, CAND, "c1", Some("success")),
                gate_run(2, PATCH_CAND, "p1", Some("success")),
            ]),
        );
        s.artifacts.insert(
            (repo(), 2),
            Ok(vec![evidence(&gate::evidence_artifact("p1"))]),
        );
        s.required_checks.insert(
            (repo(), PATCH_BRANCH.into()),
            Ok(RequiredChecks {
                contexts: ["required-check".to_string()].into(),
                strict: false,
            }),
        );
        s.files.insert(
            (repo(), "p1".into(), "CHANGELOG.md".into()),
            Ok(changelog()),
        );
    }
    w
}

fn release_item_of(w: &World, label: &str) -> Option<crate::tracker::Item> {
    w.tracker
        .list_by_label(label)
        .unwrap()
        .into_iter()
        .find(|i| {
            matches!(
                texts::bridge_item(&i.description, None),
                Some(BridgeItem::Release { .. })
            )
        })
}

#[test]
fn a_patch_cut_beside_an_open_cycle_gets_its_own_release_item_and_round() {
    let w = with_patch(true);
    let r = w.tick();
    assert!(r.failures.is_empty(), "{:?}", r.failures);
    let patch = release_item_of(&w, PATCH_LABEL).expect("the patch's release item");
    let open = release_item_of(&w, LABEL).expect("the open cycle's release item");
    assert_ne!(patch.key, open.key, "each cycle has its own release item");
    // The patch embarks the ticket of the PR it cherry-picked, not the
    // open cycle's.
    let fixed = w.tracker.get("ABC-7").unwrap();
    assert!(
        fixed.labels.iter().any(|l| l == PATCH_LABEL),
        "{:?}",
        fixed.labels
    );
    assert!(
        !fixed.labels.iter().any(|l| l == LABEL),
        "{:?}",
        fixed.labels
    );
    let shipped = w.tracker.get("ABC-5").unwrap();
    assert!(
        shipped.labels.iter().any(|l| l == LABEL),
        "{:?}",
        shipped.labels
    );
    assert!(
        !shipped.labels.iter().any(|l| l == PATCH_LABEL),
        "{:?}",
        shipped.labels
    );
    // Its round is opened on its own candidate, under its own item.
    let children = w.tracker.children(&patch.key).unwrap();
    assert!(
        children.iter().any(|c| matches!(
            texts::bridge_item(&c.description, c.parent.as_deref()),
            Some(BridgeItem::Consolidation { scope }) if scope == PATCH_CAND
        )),
        "the patch's consolidation item"
    );
    let open_children = w.tracker.children(&open.key).unwrap();
    assert!(
        !open_children
            .iter()
            .any(|c| c.description.contains(PATCH_CAND)),
        "the open cycle is untouched by the patch"
    );
}

#[test]
fn a_lower_cycle_not_cut_as_a_patch_is_not_carried() {
    let w = with_patch(false);
    let r = w.tick();
    assert!(r.failures.is_empty(), "{:?}", r.failures);
    assert!(release_item_of(&w, PATCH_LABEL).is_none());
    assert!(release_item_of(&w, LABEL).is_some());
}

#[test]
fn a_patch_at_or_below_the_release_in_production_is_over() {
    let w = with_patch(true);
    // acme-v1.1.1 is published: the patch is no longer in flight here (its
    // closure is `overtaken`'s), and a stale tag below production never is.
    let mut s = w.github.state.borrow_mut();
    let published = s
        .releases
        .get(&(repo(), "acme-v1.1.0".into()))
        .unwrap()
        .clone()
        .unwrap();
    s.releases.insert(
        (repo(), "acme-v1.1.1".into()),
        Ok(Release {
            tag: "acme-v1.1.1".into(),
            ..published
        }),
    );
    s.tags
        .get_mut(&repo())
        .unwrap()
        .push(tag("acme-v1.1.1", "p1"));
    drop(s);
    let t = w.tick_ctx();
    let shape = crate::method::release::shape::Shape::of(&t.config.product.name, &t.config.release);
    let (newest, tags) = crate::tick::release::newest(&t, &shape).unwrap().unwrap();
    let patches = crate::tick::release::patches_in_flight(&t, &shape, &tags, &newest).unwrap();
    assert!(patches.is_empty());
}

#[test]
fn the_patch_in_flight_is_found_with_its_newest_candidate() {
    let w = with_patch(true);
    w.github
        .state
        .borrow_mut()
        .tags
        .get_mut(&repo())
        .unwrap()
        .push(tag("acme-v1.1.1-rc.2", "p2"));
    let t = w.tick_ctx();
    let shape = crate::method::release::shape::Shape::of(&t.config.product.name, &t.config.release);
    let (newest, tags) = crate::tick::release::newest(&t, &shape).unwrap().unwrap();
    // The commit of the plain rc.2 tag.
    w.github.state.borrow_mut().commits.insert(
        (repo(), "p2".into()),
        Ok(Commit {
            sha: "p2".into(),
            message: "chore: candidate".into(),
            committer_date: Timestamp::parse("2026-10-05T17:00:00Z"),
            parents: vec![],
        }),
    );
    let patches = crate::tick::release::patches_in_flight(&t, &shape, &tags, &newest).unwrap();
    assert_eq!(newest.tag, CAND);
    assert_eq!(
        patches.iter().map(|c| c.tag.as_str()).collect::<Vec<_>>(),
        vec!["acme-v1.1.1-rc.2"],
        "the rc.1 annotation marks the cycle; its newest candidate is carried"
    );
}

#[test]
fn release_cut_names_the_branch_while_a_patch_is_in_flight() {
    let w = with_patch(true);
    let t = w.tick_ctx();
    let mut log = ActionLog::in_memory();
    let refused = crate::verbs::cut(&t, &mut log, false, "me").unwrap_err();
    assert!(refused.contains("name one with `--branch`"), "{refused}");
    let refused =
        crate::verbs::cut_on(&t, &mut log, false, Some("release/elsewhere"), "me").unwrap_err();
    assert!(
        refused.contains("not a release branch in flight"),
        "{refused}"
    );
    assert!(w.github.writes().is_empty(), "nothing dispatched");
    let line = crate::verbs::cut_on(&t, &mut log, false, Some(PATCH_BRANCH), "me").unwrap();
    assert!(line.contains("action candidate"), "{line}");
    let writes = w.github.writes();
    assert!(
        writes
            .iter()
            .any(|l| l.contains("release-cut.yml") && l.contains(PATCH_BRANCH)),
        "{writes:?}"
    );
}
