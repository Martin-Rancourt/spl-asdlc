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

#[test]
fn a_patch_cycle_is_read_off_its_first_candidate() {
    let w = with_patch(true);
    let t = w.tick_ctx();
    let shape = crate::method::release::shape::Shape::of(&t.config.product.name, &t.config.release);
    let (newest, tags) = crate::tick::release::newest(&t, &shape).unwrap().unwrap();
    let mut patches = crate::tick::release::patches_in_flight(&t, &shape, &tags, &newest).unwrap();
    let mut patch = patches.remove(0);
    patch.first_rc = 1;
    assert_eq!(
        crate::tick::release::cycle_patches(&t, &shape, &patch)
            .unwrap()
            .as_deref(),
        Some("acme-v1.1.0"),
        "the GO copies this line into the patch's release tag"
    );
    let mut open = newest;
    open.first_rc = 1;
    assert_eq!(
        crate::tick::release::cycle_patches(&t, &shape, &open).unwrap(),
        None
    );
}

/// The patch `acme-v1.1.1` published: its release tag carries the GO's
/// `Release-Patch-From:` line, and its branch still holds its candidate.
fn patch_published(w: &World) {
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
    s.tag_objects.insert(
        (repo(), "patch-release-object".into()),
        AnnotatedTag {
            name: "acme-v1.1.1".into(),
            message: format!(
                "Release acme-v1.1.1\n\nCandidate: {PATCH_CAND}\nRelease-Patch-From: acme-v1.1.0"
            ),
            tagger_date: Timestamp::parse("2026-10-05T18:00:00Z"),
            target_sha: "p1".into(),
            target_type: ObjectType::Commit,
        },
    );
    s.tags.get_mut(&repo()).unwrap().push(GitRef {
        name: "acme-v1.1.1".into(),
        sha: "patch-release-object".into(),
        object_type: ObjectType::Tag,
    });
}

fn closure_said(w: &World, key: &str) -> bool {
    w.tracker.comments(key).unwrap().iter().any(|c| {
        texts::has_marker(
            &c.body,
            &texts::marker("closure", &[("tag", "acme-v1.1.1")]),
        )
    })
}

#[test]
fn a_published_patch_closes_once_merged_into_the_open_cycle_never_main() {
    let w = with_patch(true);
    let r = w.tick();
    assert!(r.failures.is_empty(), "{:?}", r.failures);
    let patch = release_item_of(&w, PATCH_LABEL).expect("the patch's release item");
    patch_published(&w);
    // Its branch still holds the candidate: the closure waits.
    w.tick();
    assert!(
        !closure_said(&w, &patch.key),
        "the patch branch is still there"
    );
    // The publication merged the patch tag into the open cycle and deleted
    // the patch branch; `main` was never asked about.
    {
        let mut s = w.github.state.borrow_mut();
        s.branches
            .get_mut(&repo())
            .unwrap()
            .retain(|b| b.name != PATCH_BRANCH);
        s.compares.insert(
            (repo(), "p1".into(), BRANCH.into()),
            cmp(CompareStatus::Ahead, &[], &[]),
        );
        s.compares.insert(
            (repo(), "acme-v1.1.1".into(), BRANCH.into()),
            cmp(CompareStatus::Ahead, &[], &[]),
        );
    }
    let r = w.tick();
    assert!(
        !r.failures.iter().any(|f| f.contains("main")),
        "{:?}",
        r.failures
    );
    assert!(closure_said(&w, &patch.key), "{:?}", r.failures);
}

#[test]
fn a_published_patch_not_yet_in_the_open_cycle_waits() {
    let w = with_patch(true);
    w.tick();
    let patch = release_item_of(&w, PATCH_LABEL).unwrap();
    patch_published(&w);
    {
        let mut s = w.github.state.borrow_mut();
        s.branches
            .get_mut(&repo())
            .unwrap()
            .retain(|b| b.name != PATCH_BRANCH);
        s.compares.insert(
            (repo(), "acme-v1.1.1".into(), BRANCH.into()),
            cmp(CompareStatus::Diverged, &[], &[]),
        );
    }
    w.tick();
    assert!(!closure_said(&w, &patch.key));
}

/// Business rule: once the patch is published and its tag merged into the
/// open cycle's branch, the open cycle's candidate no longer goes (the branch
/// moved past it) and, with `auto_candidate`, the bridge cuts the next one on
/// that branch. The open cycle takes the new production version by the
/// merge, never by a cherry-pick.
#[test]
fn the_patch_merged_into_the_open_cycle_cancels_its_candidate_and_cuts_the_next() {
    let mut w = with_patch(true);
    let text = crate::config::spec_example::repository()
        .replace("auto_candidate = false", "auto_candidate = true");
    w.config = crate::config::spec_example::parse(&text);
    w.tick();
    patch_published(&w);
    // The publication merged the patch tag into the open branch (its fix is
    // past the open candidate) and deleted the patch branch.
    w.moved_past("c1", &["src/fix.rs"]);
    {
        let mut s = w.github.state.borrow_mut();
        let branches = s.branches.get_mut(&repo()).unwrap();
        branches.retain(|b| b.name != PATCH_BRANCH && b.name != BRANCH);
        branches.push(tag(BRANCH, "merged-patch"));
        s.compares.insert(
            (repo(), "acme-v1.1.1".into(), BRANCH.into()),
            cmp(CompareStatus::Ahead, &[], &[]),
        );
    }
    let r = w.tick();
    assert!(r.failures.is_empty(), "{:?}", r.failures);
    let gaps = w.tip_gaps();
    assert!(
        gaps.iter()
            .any(|g| g.contains(&format!("moved past {CAND}"))),
        "the open candidate gets no GO: {gaps:?}"
    );
    let dispatched: Vec<String> = w
        .github
        .writes()
        .into_iter()
        .filter(|l| l.contains("dispatch "))
        .collect();
    assert_eq!(
        dispatched,
        vec![format!(
            "dispatch example-org/acme release-cut.yml on {BRANCH} action=candidate"
        )],
        "the next candidate is cut on the open branch only"
    );
}

#[test]
fn release_status_reads_the_patch_cycle_on_its_branch() {
    let w = with_patch(true);
    w.tick();
    let t = w.tick_ctx();
    let (open, _) = crate::verbs::status_on(&t, None).unwrap();
    assert_eq!(open.candidate.tag, CAND, "the newest cycle by default");
    let (patch, _) = crate::verbs::status_on(&t, Some(PATCH_BRANCH)).unwrap();
    assert_eq!(patch.candidate.tag, PATCH_CAND);
    assert_eq!(patch.branch.as_deref(), Some(PATCH_BRANCH));
    let refused = crate::verbs::status_on(&t, Some("release/elsewhere"))
        .err()
        .unwrap();
    assert!(refused.contains("holds no cycle in flight"), "{refused}");
}

#[test]
fn abandoning_a_patch_names_its_branch_and_never_merges_it_into_main() {
    let w = with_patch(true);
    w.tick();
    let patch = release_item_of(&w, PATCH_LABEL).unwrap();
    let open = release_item_of(&w, LABEL).unwrap();
    let t = w.tick_ctx();
    let mut log = ActionLog::in_memory();
    let refused = crate::verbs::abandon_on(&t, &mut log, None, "me").unwrap_err();
    assert!(refused.contains("name one with `--branch`"), "{refused}");
    let before = w.github.writes().len();
    let done = crate::verbs::abandon_on(&t, &mut log, Some(PATCH_BRANCH), "me").unwrap();
    assert!(done.contains(PATCH_CAND), "{done}");
    let writes: Vec<String> = w.github.writes().into_iter().skip(before).collect();
    assert!(
        !writes
            .iter()
            .any(|l| l.contains("create_pull") || l.contains("merge")),
        "no merge back of a patch: {writes:?}"
    );
    assert!(
        writes
            .iter()
            .any(|l| l.contains("delete") && l.contains(PATCH_BRANCH)),
        "{writes:?}"
    );
    assert!(w.tracker.get(&patch.key).unwrap().state.is_resolved());
    assert!(
        !w.tracker.get(&open.key).unwrap().state.is_resolved(),
        "the open cycle is untouched"
    );
}

/// A patch that carries its caller's release-notes commit (ASDLC-7SWR):
/// the copy maps back to a commit no PR merged, so it embarks no ticket and
/// no untracked PR; the patch still embarks the ticket of its PR.
#[test]
fn a_carried_commit_embarks_no_ticket() {
    let w = with_patch(true);
    {
        let mut s = w.github.state.borrow_mut();
        let mut range = cmp(CompareStatus::Ahead, &["p0", "p1"], &["src/fix.rs"]).unwrap();
        range.commits[0].picked_from = Some("m7".into());
        range.commits[1].picked_from = Some("n1".into());
        range.commits[1].subject = "chore(changelog): the release notes of v1.1.1".into();
        s.compares
            .insert((repo(), "acme-v1.1.0".into(), "p1".into()), Ok(range));
        // GitHub lists no PR for a commit pushed straight to the branch.
        s.commit_pulls.insert((repo(), "n1".into()), Ok(Vec::new()));
    }
    let t = w.tick_ctx();
    let shape = crate::method::release::shape::Shape::of(&t.config.product.name, &t.config.release);
    let (newest, tags) = crate::tick::release::newest(&t, &shape).unwrap().unwrap();
    let patches = crate::tick::release::patches_in_flight(&t, &shape, &tags, &newest).unwrap();
    let patch = patches
        .iter()
        .find(|c| c.tag == PATCH_CAND)
        .expect("the patch in flight");
    let embarked = crate::tick::release::embarked_items(&t, &shape, patch).unwrap();
    assert_eq!(
        embarked
            .tickets
            .iter()
            .map(|(k, _)| k.clone())
            .collect::<Vec<_>>(),
        vec!["ABC-7".to_string()],
        "only the ticket of the PR it cherry-picked"
    );
    assert!(
        embarked.untracked.is_empty(),
        "no untracked PR from the carried commit"
    );
    assert!(embarked.delivered.is_empty());
}
