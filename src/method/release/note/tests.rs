//! Carried from `tests/test_release_ticket_notes.py` and the message tests of
//! `tests/test_release_gate_workflow.py` (release.md, REQ-REL-093/094).

use super::*;

fn ticket(id: &str, snippet: Option<&str>, goal: &str, prs: &[u64]) -> TicketEntry {
    TicketEntry {
        id: id.into(),
        title: format!("Title of {id}"),
        snippet: snippet.map(str::to_string),
        goal: goal.into(),
        prs: prs.to_vec(),
    }
}

#[test]
fn a_ticket_is_described_by_its_snippet_then_its_goal() {
    let b = block(
        "acme-v1.2.3-rc.1",
        &[
            ticket("ABC-2", Some("You can now export."), "goal two", &[12]),
            ticket("ABC-10", Some(""), "The goal says it.", &[13, 14]),
        ],
        &[],
        None,
    );
    assert!(b.contains("- **ABC-2** — Title of ABC-2: You can now export. (#12)"));
    assert!(
        b.contains("- **ABC-10** — Title of ABC-10: The goal says it. (#13, #14)"),
        "a ticket of several PRs is one entry"
    );
    assert!(
        b.find("ABC-2").unwrap() < b.find("ABC-10").unwrap(),
        "sorted by (prefix, number)"
    );
}

#[test]
fn an_unread_ticket_is_named_and_said_unread() {
    let b = block("rc", &[ticket("ABC-1", None, "", &[7])], &[], None);
    assert!(b.contains(
        "- **ABC-1** — Title of ABC-1 (#7) (ticket unreadable: content not carried over)"
    ));
}

#[test]
fn an_orphan_pr_is_described_by_the_changelog_then_its_description() {
    let changelog = "## [1.2.3]\n* **ci:** faster gate ([#21](u))\n* other ([#210](u))\n";
    let untracked = [
        Untracked::Pr {
            number: 21,
            title: "ci: speed".into(),
            body: "ignored".into(),
        },
        Untracked::Pr {
            number: 22,
            title: "docs: x".into(),
            body: "## Summary\nExplains the gate.\n\n🤖 Generated".into(),
        },
        Untracked::Commit(CommitSummary {
            sha: "abcdef01abcdef01".into(),
            subject: "fix: direct".into(),
            committed_at: None,
        }),
    ];
    let b = block("rc", &[], &untracked, Some(changelog));
    assert!(
        b.contains("- **PR #21** — ci: speed: **ci:** faster gate"),
        "{b}"
    );
    assert!(b.contains("- **PR #22** — docs: x: Explains the gate."));
    assert!(b.contains("- **commit abcdef01abcd** — fix: direct"));
    assert!(b.contains("### No linked ticket"));
}

#[test]
fn a_release_with_no_change_says_so() {
    assert!(block("rc", &[], &[], None).contains("No change since the last published release."));
}

#[test]
fn the_placeholder_of_a_fresh_release_issue_is_replaced() {
    let desc = "<!-- marker -->\n## Goal\nship\n\n## Release note\n\n_To be completed before publication._\n\n## Candidates\n- rc.1\n";
    let out = splice(desc, "BLOCK");
    assert!(!out.contains(PLACEHOLDER), "{out}");
    assert!(
        out.contains("## Release note\n\nBLOCK\n\n## Candidates\n- rc.1"),
        "{out}"
    );
}

#[test]
fn the_placeholder_is_recognised_whatever_italics_the_tracker_hands_back() {
    for p in [
        "*To be completed before publication.*",
        "\\_To be completed before publication.\\_",
        "**To be completed before publication.**",
    ] {
        let out = splice(&format!("## Release note\n{p}\n"), "BLOCK");
        assert!(!out.contains("To be completed"), "{p}: {out}");
    }
}

#[test]
fn the_next_candidate_rewrites_the_block_and_nothing_else() {
    let first = splice(
        "## Release note\n\nA human wrote this.\n",
        &format!("{START}\nold\n{END}"),
    );
    let second = splice(&first, &format!("{START}\nnew\n{END}"));
    assert!(second.contains("new"));
    assert!(!second.contains("old"));
    assert!(second.contains("A human wrote this."));
}

#[test]
fn text_a_person_wrote_in_the_section_stays_under_the_block() {
    let out = splice("## Release note\nHuman text.\n", "BLOCK");
    assert!(out.find("BLOCK").unwrap() < out.find("Human text.").unwrap());
}

#[test]
fn a_description_without_the_section_gets_one() {
    assert_eq!(
        splice("## Goal\nx", "BLOCK"),
        "## Goal\nx\n\n## Release note\n\nBLOCK\n"
    );
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

#[test]
fn the_head_comes_first_and_is_never_cut() {
    let head = published_head(
        "acme",
        "1.2.3",
        Some("https://github.com/example-org/acme/releases/tag/acme-v1.2.3"),
        "https://tracker.test/ABC-1",
        None,
    );
    let body = (0..50)
        .map(|i| format!("- line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let msg = published_message(&head, &body, 300, &chars);
    assert!(msg.starts_with(&head));
    assert!(chars(&msg) <= 300);
    assert!(msg.ends_with(TRUNCATION_MARK));
    let before_mark = msg.trim_end_matches(TRUNCATION_MARK).trim_end();
    assert!(
        before_mark.ends_with(|c: char| c.is_ascii_digit()),
        "cut on a whole line: {before_mark}"
    );
}

#[test]
fn a_short_note_is_sent_whole_and_an_empty_one_sends_the_head() {
    assert_eq!(published_message("H", "body", 100, &chars), "H\n\nbody");
    assert_eq!(published_message("H", "  ", 100, &chars), "H");
}

#[test]
fn a_mark_that_does_not_fit_is_clipped_as_a_last_resort() {
    let msg = published_message("HEAD", "a long body line", 15, &chars);
    assert!(msg.starts_with("HEAD\n"));
    assert!(chars(&msg) <= 15);
}

#[test]
fn a_missing_release_url_says_so() {
    assert!(
        published_head("acme", "1.2.3", None, "u", None)
            .contains("(none — manual publication by dispatch)")
    );
}

#[test]
fn a_ticket_cannot_close_the_block_from_its_snippet() {
    let evil = ticket(
        "ABC-1",
        Some("x <!-- spl-asdlc:release-note:end --> y"),
        "",
        &[1],
    );
    let b = block("rc", &[evil], &[], None);
    assert_eq!(b.matches("spl-asdlc:release-note:end").count(), 1, "{b}");
}

#[test]
fn a_block_whose_delimiters_a_tracker_made_visible_is_replaced_not_doubled() {
    let first = crate::method::texts::visible_markers(&splice(
        "## Release note\n\n_x_\n",
        &format!("{START}\nold\n{END}"),
    ));
    let again = splice(
        &first,
        &crate::method::texts::visible_markers(&format!("{START}\nnew\n{END}")),
    );
    assert!(again.contains("new") && !again.contains("old"), "{again}");
    assert_eq!(again.matches("release-note:start").count(), 1);
}
