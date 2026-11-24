//! A candidate whose comparison GitHub cuts at 300 files (rules G7CK,
//! X4PV): the range is still read, its commits complete, so its tickets are
//! labelled; its brief is written and says the list was cut; the cut list
//! counts as touching every reviewed path, so the candidate is `sensible`
//! and every configured angle is offered.

use super::*;
use crate::github::AnnotatedTag;

/// `n` paths, none of them reviewed apart nor a business rule.
fn many(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("docs/page-{i:03}.md")).collect()
}

/// `CAND` annotated `routine`, its range since `acme-v1.1.0` changing
/// `files` paths, PR #5's commit among its commits.
fn world_with(files: usize) -> World {
    let w = World::new();
    let paths = many(files);
    let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
    let mut s = w.github.state.borrow_mut();
    s.compares.insert(
        (repo(), "acme-v1.1.0".into(), "c1".into()),
        cmp(CompareStatus::Ahead, &["m1"], &paths),
    );
    s.tag_objects.insert(
        (repo(), "tag-object-rc1".into()),
        AnnotatedTag {
            name: CAND.into(),
            message: "Release candidate\n\nRelease-Class: routine\n".into(),
            tagger_date: None,
            target_sha: "c1".into(),
            target_type: ObjectType::Commit,
        },
    );
    s.tags.insert(
        repo(),
        vec![
            tag("acme-v1.1.0", "r0"),
            GitRef {
                name: CAND.into(),
                sha: "tag-object-rc1".into(),
                object_type: ObjectType::Tag,
            },
        ],
    );
    drop(s);
    w
}

#[test]
fn the_fake_github_cuts_a_comparison_at_300_files_and_keeps_its_commits() {
    let w = world_with(350);
    let cmp = w
        .github
        .compare(&repo(), "acme-v1.1.0", "c1")
        .expect("a cut comparison is read");
    assert!(cmp.files_cut);
    assert_eq!(cmp.files.len(), 300);
    assert_eq!(cmp.commits.len(), 1);

    let w = world_with(299);
    let cmp = w.github.compare(&repo(), "acme-v1.1.0", "c1").unwrap();
    assert!(!cmp.files_cut);
    assert_eq!(cmp.files.len(), 299);
}

#[test]
fn a_candidate_whose_range_github_cuts_at_300_files_is_labelled_briefed_sensible_with_every_angle()
{
    let w = world_with(300);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");

    // The embarked tickets: read from the range's commits, labelled.
    assert!(
        w.tracker.item("ABC-5").labels.contains(&LABEL.to_string()),
        "embarked"
    );

    // The brief: written, it says GitHub cut the list, and the class.
    let briefs = w.briefs(CAND);
    assert_eq!(briefs.len(), 1, "the brief is written");
    let brief = &briefs[0];
    assert!(
        brief.contains(
            "- Diff since the last published release: `acme-v1.1.0` — 300 file(s) or more: GitHub cut the list at 300\n"
        ),
        "{brief}"
    );
    assert!(brief.contains("#### File list cut by GitHub"), "{brief}");
    assert!(
        brief.contains(
            "- Class: sensible (GitHub cut the file list at 300: it counts as touching every reviewed path; the tag says routine)\n"
        ),
        "{brief}"
    );
    assert!(brief.contains("**ABC-5** — Shipped"), "{brief}");

    // Every configured angle is offered, with its consolidation item.
    assert_eq!(w.angle_keys(CAND).len(), w.config.release.angles.len());
    for angle in ["security", "product"] {
        assert_eq!(w.sub_item(angle).state.status, Some(Status::Ready));
    }
    assert_eq!(w.round(CAND).len(), 3);

    // A second tick writes nothing more.
    let (r, writes) = w.writes_of_a_tick();
    assert!(r.ok(), "{r:?}");
    assert!(writes.is_empty(), "{writes:?}");
}

#[test]
fn a_range_github_lists_whole_keeps_its_tags_class() {
    let w = world_with(299);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let brief = &w.briefs(CAND)[0];
    assert!(brief.contains("- Class: routine\n"), "{brief}");
    assert!(brief.contains("— 299 file(s)\n"), "{brief}");
    assert!(!brief.contains("cut"), "{brief}");
}

#[test]
fn a_later_candidate_whose_diff_since_the_previous_one_is_cut_says_so_and_is_sensible() {
    let w = world_with(10);
    w.tick();
    w.cut_rc2();
    let paths = many(320);
    let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
    w.github.state.borrow_mut().compares.insert(
        (repo(), CAND.into(), "c2".into()),
        cmp(CompareStatus::Ahead, &["f1"], &paths),
    );
    w.gate(vec![
        gate_run(2, CAND2, "c2", Some("success")),
        gate_run(1, CAND, "c1", Some("success")),
    ]);
    let r = w.tick();
    assert!(r.ok(), "{r:?}");
    let comments = w.tracker.comments(&w.release_item().key).unwrap();
    let candidate = comments
        .iter()
        .find(|c| c.body.starts_with(&format!("### Candidate `{CAND2}`")))
        .expect("rc.2's candidate comment");
    assert!(
        candidate.body.contains(
            "- Paths touched since the previous candidate: 300 file(s) or more: GitHub cut the list at 300\n"
        ),
        "{}",
        candidate.body
    );
    assert!(
        candidate.body.contains(
            "Angles offered on this candidate once its gate is green: security, product."
        ),
        "{}",
        candidate.body
    );
    let brief = &w.briefs(CAND2)[0];
    assert!(
        brief.contains("- Diff since the previous candidate: `acme-v1.2.0-rc.1` — 300 file(s) or more: GitHub cut the list at 300\n"),
        "{brief}"
    );
    assert!(brief.contains("- Class: sensible"), "{brief}");
    assert_eq!(w.round(CAND2).len(), 3, "every angle and the consolidation");
}

#[test]
fn a_release_branch_that_moved_past_its_candidate_by_a_cut_list_is_a_tip_gap() {
    let w = World::new();
    w.tick();
    let paths = many(300);
    let paths: Vec<&str> = paths.iter().map(String::as_str).collect();
    w.moved_past("c1", &paths);
    assert_eq!(
        w.tip_gaps(),
        vec![format!(
            "{BRANCH} moved past {CAND} (300 file(s) or more: GitHub cut the list at 300): tag the next candidate"
        )]
    );
}
