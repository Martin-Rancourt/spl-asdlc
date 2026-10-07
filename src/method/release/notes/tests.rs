//! Carried from `tests/test_release_notes.py` and the coverage half of
//! `tests/test_verify_release_notes.py` (release.md).

use super::*;

fn c(sha: &str, subject: &str) -> CommitSummary {
    CommitSummary {
        sha: sha.into(),
        subject: subject.into(),
    }
}

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

/// The shape of the 0.4.118 range: five commits the first note omitted.
fn range_0_4_118() -> Vec<CommitSummary> {
    vec![
        c("a1", "feat(release): read the range from git (#760)"),
        c("a2", "fix(merge): pin the merge to the head (#761)"),
        c("a3", "docs: explain the gate (#762)"),
        c("a4", "chore(ci): bump the runner (#763)"),
        c("a5", "refactor: split the pass (#764)"),
        c("a6", "fix: a direct commit on the branch"),
        c("a7", "Merge branch 'main' into release"),
    ]
}

const OMITTING: &str = "## [0.4.118](link) (2026-09-29)\n\n### Features\n\n* **release:** read the range from git ([#760](u))\n";

fn complete() -> String {
    format!(
        "{OMITTING}* fix ([#761](u))\n* docs ([#762](u))\n* chore ([#763](u))\n* refactor ([#764](u))\n* a direct commit on the branch\n\n## [0.4.117](link)\n\n* older (#759)\n"
    )
}

#[test]
fn a_complete_note_covers_the_range() {
    let cov = coverage(v("0.4.118"), Some(&complete()), None, Ok(&range_0_4_118()));
    assert!(cov.covered(), "{:?}", cov.gaps());
}

#[test]
fn the_0_4_118_note_omitting_five_commits_names_all_five() {
    let cov = coverage(v("0.4.118"), Some(OMITTING), None, Ok(&range_0_4_118()));
    assert_eq!(cov.missing.len(), 5);
    assert_eq!(
        cov.gaps()[0],
        "a2 fix(merge): pin the merge to the head (#761)"
    );
}

#[test]
fn one_omitted_commit_is_enough() {
    let note = complete().replace("* refactor ([#764](u))\n", "");
    let cov = coverage(v("0.4.118"), Some(&note), None, Ok(&range_0_4_118()));
    assert!(!cov.covered());
    assert_eq!(cov.missing.len(), 1);
}

#[test]
fn a_longer_pr_number_is_not_coverage() {
    let range = [c("b1", "fix: a thing (#783)")];
    let note = "## [1.0.0]\n* other ([#7830](u)) and (#1783)\n";
    assert_eq!(
        coverage(v("1.0.0"), Some(note), None, Ok(&range))
            .missing
            .len(),
        1
    );
    let ok = "## [1.0.0]\n* a thing ([#783](u))\n";
    assert!(coverage(v("1.0.0"), Some(ok), None, Ok(&range)).covered());
}

#[test]
fn a_direct_commit_without_a_pr_is_covered_by_its_summary() {
    let range = [c("b1", "fix(x): a direct commit")];
    assert!(
        coverage(
            v("1.0.0"),
            Some("## [1.0.0]\n* **x:** a direct commit\n"),
            None,
            Ok(&range)
        )
        .covered()
    );
}

#[test]
fn a_commit_that_is_not_conventional_is_not_owed_a_line() {
    let range = [c("b1", "Merge pull request #12 from x"), c("b2", "wip")];
    assert!(coverage(v("1.0.0"), Some("## [1.0.0]\n"), None, Ok(&range)).covered());
}

#[test]
fn a_note_without_a_section_for_the_version_is_unreadable_not_covered() {
    let cov = coverage(v("1.0.1"), Some("## [1.0.0]\n* x\n"), None, Ok(&[]));
    assert!(!cov.covered());
    assert_eq!(cov.gaps().len(), 1);
    assert!(cov.gaps()[0].starts_with("note unreadable:"));
}

#[test]
fn an_empty_changelog_is_unreadable() {
    assert!(
        coverage(v("1.0.0"), Some("  \n"), None, Ok(&[]))
            .unreadable
            .is_some()
    );
    assert!(
        coverage(v("1.0.0"), None, None, Ok(&[]))
            .unreadable
            .is_some()
    );
}

#[test]
fn a_range_that_cannot_be_read_is_unreadable_not_covered() {
    let cov = coverage(
        v("1.0.0"),
        Some("## [1.0.0]\n"),
        None,
        Err("compare failed"),
    );
    assert!(cov.unreadable.unwrap().contains("compare failed"));
}

#[test]
fn the_section_stops_at_the_next_version() {
    let range = [c("b1", "fix: older thing (#759)")];
    let cov = coverage(v("0.4.118"), Some(&complete()), None, Ok(&range));
    assert_eq!(
        cov.missing.len(),
        1,
        "a PR named only in the previous section is not covered"
    );
}

#[test]
fn the_types_a_config_hides_are_not_owed_a_line() {
    let config = r#"{"packages":{".":{"changelog-sections":[{"type":"feat","section":"Features"},{"type":"chore","section":"Misc","hidden":true}]}}}"#;
    let types = written_types(Some(config)).unwrap();
    assert!(types.contains("feat"));
    assert!(!types.contains("chore"));
}

#[test]
fn a_config_without_sections_owes_the_types_every_strategy_writes() {
    let types = written_types(Some(r#"{"packages":{".":{}}}"#)).unwrap();
    assert_eq!(
        types.into_iter().collect::<Vec<_>>(),
        vec!["feat", "fix", "perf", "revert"]
    );
}

#[test]
fn top_level_sections_apply_when_the_root_package_has_none() {
    let config = r#"{"changelog-sections":[{"type":"docs"}],"packages":{".":{}}}"#;
    assert_eq!(
        written_types(Some(config))
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>(),
        vec!["docs"]
    );
}

#[test]
fn a_config_that_cannot_be_read_relaxes_nothing() {
    for bad in [
        None,
        Some("not json"),
        Some(r#"{"packages":{}}"#),
        Some(r#"{"packages":{".":[]}}"#),
        Some(r#"{"packages":{".":{"changelog-sections":"x"}}}"#),
    ] {
        assert_eq!(written_types(bad), None, "{bad:?}");
    }
    let range = [c("b1", "chore: owed when nothing says otherwise (#9)")];
    assert_eq!(
        coverage(v("1.0.0"), Some("## [1.0.0]\n"), Some("broken"), Ok(&range))
            .missing
            .len(),
        1
    );
}

#[test]
fn a_hidden_type_is_not_missing_but_a_visible_one_is() {
    let config = r#"{"packages":{".":{"changelog-sections":[{"type":"feat"},{"type":"chore","hidden":true}]}}}"#;
    let range = [c("b1", "chore: hidden (#1)"), c("b2", "feat: visible (#2)")];
    let cov = coverage(v("1.0.0"), Some("## [1.0.0]\n"), Some(config), Ok(&range));
    assert_eq!(cov.missing, vec![c("b2", "feat: visible (#2)")]);
}

#[test]
fn the_missing_entries_come_back_as_lines_to_paste() {
    let missing = [c("a2", "fix(merge): pin the merge to the head (#761)")];
    assert_eq!(
        suggested_entries(&missing, Some("https://github.com/example-org/acme")),
        vec![
            "* **merge:** pin the merge to the head ([#761](https://github.com/example-org/acme/issues/761))"
        ]
    );
    assert_eq!(
        suggested_entries(&missing, None),
        vec!["* **merge:** pin the merge to the head (#761)"]
    );
}

#[test]
fn a_commit_without_a_scope_or_a_pr_is_suggested_by_its_summary() {
    assert_eq!(
        suggested_entries(&[c("x", "fix: plain")], None),
        vec!["* plain"]
    );
}

#[test]
fn a_candidate_of_another_version_is_not_judged_against_this_release() {
    let cov = coverage(v("0.4.119"), Some(&complete()), None, Ok(&range_0_4_118()));
    assert!(
        cov.unreadable.is_some(),
        "no section for 0.4.119: unreadable, never covered"
    );
}

#[test]
fn the_staged_version_is_read_off_the_body() {
    let body = "<details><summary>other: 9.9.9</summary></details>\n<details><summary>acme: 0.4.92</summary>";
    assert_eq!(staged_version(body, "acme"), Some(v("0.4.92")));
    assert_eq!(staged_version(body, "ACME"), Some(v("0.4.92")));
    assert_eq!(
        staged_version("chore: release main", "acme"),
        None,
        "an unreadable version is a mismatch"
    );
    assert_eq!(
        staged_version("<summary>  acme :  1.2.3</summary>", "acme"),
        Some(v("1.2.3"))
    );
}
