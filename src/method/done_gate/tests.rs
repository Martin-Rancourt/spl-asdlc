//! Carried from `tests/test_linear_service.py` and `tests/test_release_service.py`
//! (the Done gate, duplicates and the post-deploy section).

use super::*;
use std::collections::HashMap;

fn member(id: &str, description: &str) -> ContractMember {
    ContractMember {
        id: id.into(),
        title: String::new(),
        description: description.into(),
    }
}

fn gate_of(members: &[(&str, &str)]) -> DoneGate {
    let m: Vec<ContractMember> = members.iter().map(|(i, d)| member(i, d)).collect();
    evaluate(&m, None)
}

fn node(description: &str, duplicates: &[&str]) -> DuplicateNode {
    DuplicateNode {
        title: String::new(),
        description: description.into(),
        duplicates: duplicates.iter().map(|s| s.to_string()).collect(),
        relations_cut: false,
    }
}

fn walk(
    start: &str,
    graph: &HashMap<&str, Result<DuplicateNode, String>>,
) -> (Vec<ContractMember>, Option<String>) {
    let first = graph[start].clone().unwrap();
    duplicate_closure(start, first, &mut |id| {
        graph
            .get(id)
            .cloned()
            .unwrap_or_else(|| Err("not found".into()))
    })
}

#[test]
fn evaluate_done_gate_is_ok_with_no_open_items_and_no_duplicates() {
    assert!(gate_of(&[("ABC-1", "- [x] a\n- [x] b")]).ok());
}

#[test]
fn evaluate_done_gate_refuses_on_a_box_outside_any_criteria_heading() {
    let gate = gate_of(&[(
        "ABC-1",
        "## Notes\n- [ ] stray box\n## Acceptance criteria\n- [x] a",
    )]);
    assert!(!gate.ok());
    assert_eq!(gate.open_items[0].line, "- [ ] stray box");
}

#[test]
fn evaluate_done_gate_refuses_on_the_tickets_own_open_box() {
    assert!(!gate_of(&[("ABC-1", "- [x] a\n- [ ] b")]).ok());
}

#[test]
fn evaluate_done_gate_refuses_a_ticket_with_no_criteria_section_and_says_so() {
    let gate = gate_of(&[("ABC-1", "## Goal\nno box at all")]);
    assert!(gate.no_criteria_declared);
    assert!(!gate.ok());
}

#[test]
fn evaluate_done_gate_a_duplicates_state_never_exempts_its_criteria() {
    let gate = gate_of(&[("ABC-1", "- [x] a"), ("ABC-2", "- [ ] the duplicate's box")]);
    assert!(!gate.ok());
    assert_eq!(gate.open_items[0].source, "ABC-2");
}

#[test]
fn a_box_declared_only_by_a_duplicate_is_a_declared_contract() {
    let gate = gate_of(&[("ABC-1", "nothing"), ("ABC-2", "- [x] done")]);
    assert!(!gate.no_criteria_declared);
    assert!(gate.ok());
}

#[test]
fn evaluate_done_gate_follows_a_duplicate_marked_against_this_ticket() {
    let graph = HashMap::from([
        ("ABC-1", Ok(node("- [x] a", &["ABC-2"]))),
        ("ABC-2", Ok(node("- [ ] b", &["ABC-1"]))),
    ]);
    let (members, unreadable) = walk("ABC-1", &graph);
    assert_eq!(unreadable, None);
    assert_eq!(members.len(), 2);
    assert!(!evaluate(&members, unreadable).ok());
}

#[test]
fn evaluate_done_gate_a_cycle_of_duplicates_terminates() {
    let graph = HashMap::from([
        ("ABC-1", Ok(node("- [x] a", &["ABC-2"]))),
        ("ABC-2", Ok(node("- [x] b", &["ABC-3"]))),
        ("ABC-3", Ok(node("- [x] c", &["ABC-1", "ABC-2"]))),
    ]);
    let (members, unreadable) = walk("ABC-1", &graph);
    assert_eq!(members.len(), 3);
    assert!(evaluate(&members, unreadable).ok());
}

#[test]
fn evaluate_done_gate_signals_a_duplicate_graph_deeper_than_the_bound() {
    let ids: Vec<String> = (0..=MAX_DUPLICATE_WALK + 1)
        .map(|i| format!("ABC-{i}"))
        .collect();
    let mut graph: HashMap<&str, Result<DuplicateNode, String>> = HashMap::new();
    for (i, id) in ids.iter().enumerate() {
        let next: Vec<&str> = ids.get(i + 1).map(|n| vec![n.as_str()]).unwrap_or_default();
        graph.insert(id, Ok(node("- [x] a", &next)));
    }
    let (members, unreadable) = walk("ABC-0", &graph);
    let reason = unreadable.expect("an exhausted walk is reported");
    assert!(reason.contains("more than 50 tickets"), "{reason}");
    assert!(!evaluate(&members, Some(reason)).ok());
}

#[test]
fn evaluate_done_gate_fails_closed_when_a_duplicate_cannot_be_read() {
    let graph = HashMap::from([
        ("ABC-1", Ok(node("- [x] a", &["ABC-2"]))),
        ("ABC-2", Err("timeout".to_string())),
    ]);
    let (members, unreadable) = walk("ABC-1", &graph);
    let gate = evaluate(&members, unreadable);
    assert!(!gate.ok());
    assert!(
        gate.unreadable_reason
            .unwrap()
            .contains("ABC-2 could not be read")
    );
}

#[test]
fn evaluate_done_gate_fails_closed_when_relations_page_is_truncated() {
    let mut cut = node("- [x] a", &[]);
    cut.relations_cut = true;
    let graph = HashMap::from([("ABC-1", Ok(cut))]);
    let (members, unreadable) = walk("ABC-1", &graph);
    assert!(!evaluate(&members, unreadable).ok());
}

#[test]
fn evaluate_done_gate_fails_closed_when_a_duplicates_relations_are_truncated() {
    let mut cut = node("- [x] b", &[]);
    cut.relations_cut = true;
    let graph = HashMap::from([
        ("ABC-1", Ok(node("- [x] a", &["ABC-2"]))),
        ("ABC-2", Ok(cut)),
    ]);
    let (_, unreadable) = walk("ABC-1", &graph);
    assert!(
        unreadable
            .unwrap()
            .contains("ABC-2's duplicate relations were cut short")
    );
}

// ---- post deploy (REQ-MERGE-067) ----

const POST: &str =
    "## Acceptance criteria\n- [x] built\n## Post deploy\n- [ ] verified in production\n";

#[test]
fn evaluate_done_gate_counts_a_post_release_box_but_sets_it_apart() {
    let gate = gate_of(&[("ABC-1", POST)]);
    assert!(!gate.ok(), "a post-deploy box still keeps Done closed");
    assert_eq!(gate.post_release_items.len(), 1);
    assert!(gate.release_blocking_items().is_empty());
    assert!(gate.awaits_post_release());
}

#[test]
fn a_post_release_box_does_not_hide_a_box_that_blocks_the_release() {
    let gate = gate_of(&[("ABC-1", &POST.replace("- [x] built", "- [ ] built"))]);
    assert_eq!(gate.release_blocking_items().len(), 1);
    assert!(!gate.awaits_post_release());
}

#[test]
fn the_same_line_inside_and_outside_the_section_is_told_apart() {
    let text = "- [ ] verified in production\n## Post deploy\n- [ ] verified in production\n";
    let gate = gate_of(&[("ABC-1", text)]);
    assert_eq!(gate.open_items.len(), 2);
    assert_eq!(gate.release_blocking_items().len(), 1);
}

#[test]
fn a_gate_that_cannot_read_or_finds_no_contract_never_awaits_post_release() {
    let m = vec![member("ABC-1", POST)];
    assert!(!evaluate(&m, Some("cut".into())).awaits_post_release());
}

#[test]
fn a_duplicates_post_release_box_is_set_apart_too() {
    let gate = gate_of(&[("ABC-1", "- [x] a"), ("ABC-2", POST)]);
    assert!(gate.awaits_post_release());
    assert!(
        !gate.awaits_post_release_of("ABC-1"),
        "a duplicate's box cannot be ticked here"
    );
}

// ---- closure verdicts (REQ-REL-087) ----

#[test]
fn moves_a_ticket_with_only_post_release_boxes_to_post_deploy() {
    let gate = gate_of(&[("ABC-1", POST)]);
    assert_eq!(
        closure_verdict("ABC-1", Some(Status::ToDeploy), &gate),
        ClosureVerdict::PostDeploy
    );
}

#[test]
fn a_replayed_closure_leaves_a_post_deploy_ticket_where_it_is() {
    let gate = gate_of(&[("ABC-1", POST)]);
    assert_eq!(
        closure_verdict("ABC-1", Some(Status::PostDeploy), &gate),
        ClosureVerdict::PostDeploy
    );
}

#[test]
fn reopens_a_ticket_that_is_not_to_deploy_even_with_only_post_release_boxes() {
    let gate = gate_of(&[("ABC-1", POST)]);
    assert_eq!(
        closure_verdict("ABC-1", Some(Status::InProgress), &gate),
        ClosureVerdict::Reopen
    );
}

#[test]
fn still_reopens_a_ticket_with_a_blocking_box_beside_the_post_release_one() {
    let gate = gate_of(&[("ABC-1", &POST.replace("- [x] built", "- [ ] built"))]);
    assert_eq!(
        closure_verdict("ABC-1", Some(Status::ToDeploy), &gate),
        ClosureVerdict::Reopen
    );
}

#[test]
fn still_closes_a_ticket_whose_post_release_boxes_are_all_checked() {
    let gate = gate_of(&[("ABC-1", &POST.replace("- [ ] verified", "- [x] verified"))]);
    assert_eq!(
        closure_verdict("ABC-1", Some(Status::ToDeploy), &gate),
        ClosureVerdict::Done
    );
}

#[test]
fn reopens_a_ticket_whose_post_release_box_belongs_to_its_duplicate() {
    let gate = gate_of(&[("ABC-1", "- [x] a"), ("ABC-2", POST)]);
    assert_eq!(
        closure_verdict("ABC-1", Some(Status::ToDeploy), &gate),
        ClosureVerdict::Reopen
    );
}

// ---- refusal texts (REQ-MERGE-066, REQ-REL-089) ----

#[test]
fn done_gate_refusal_names_the_count_and_the_source_ticket() {
    let gate = gate_of(&[("ABC-1", "- [ ] a"), ("ABC-2", "- [ ] b\n- [ ] c")]);
    let text = refusal("ABC-1", &gate, true);
    assert!(text.starts_with("Cannot move to `Done`: 3 criteria/boxes unchecked, of which 2 taken from ABC-2. State unchanged."), "{text}");
    assert!(
        text.contains("Unchecked on ABC-1, this ticket:\n- [ ] a"),
        "{text}"
    );
    assert!(
        text.contains("Unchecked on ABC-2, a duplicate merged into this ticket:\n- [ ] b\n- [ ] c"),
        "{text}"
    );
}

/// A ticket and two merged duplicates, each with a title, as the walk reads
/// them.
fn titled(members: &[(&str, &str, &str)]) -> DoneGate {
    let m: Vec<ContractMember> = members
        .iter()
        .map(|(id, title, description)| ContractMember {
            id: id.to_string(),
            title: title.to_string(),
            description: description.to_string(),
        })
        .collect();
    evaluate(&m, None)
}

fn many(prefix: &str, n: usize) -> String {
    (0..n)
        .map(|i| format!("- [ ] {prefix} {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_reopen_lists_every_open_box_in_full_grouped_by_origin() {
    let gate = titled(&[
        ("ABC-1", "The ticket", &many("own", 3)),
        ("ABC-2", "A first duplicate", &many("first", 12)),
        (
            "ABC-3",
            "A second duplicate",
            &format!("- [x] done\n{}", many("second", 8)),
        ),
    ]);
    let text = reopen_comment("ABC-1", &gate);
    assert!(
        text.starts_with("Cannot move to `Done`: 23 criteria/boxes unchecked, of which 12 taken from ABC-2, 8 taken from ABC-3."),
        "{text}"
    );
    assert!(!text.contains("more"), "no box is left out: {text}");
    for (prefix, n) in [("own", 3), ("first", 12), ("second", 8)] {
        for i in 0..n {
            assert!(
                text.contains(&format!("- [ ] {prefix} {i}\n")),
                "{prefix} {i}: {text}"
            );
        }
    }
    assert!(!text.contains("- [x] done"), "a checked box is not listed");
    let own = text.find("Unchecked on ABC-1, this ticket:").unwrap();
    let first = text
        .find("Unchecked on ABC-2 — A first duplicate, a duplicate merged into this ticket:")
        .unwrap();
    let second = text
        .find("Unchecked on ABC-3 — A second duplicate, a duplicate merged into this ticket:")
        .unwrap();
    assert!(
        own < first && first < second,
        "the ticket first, then each duplicate: {text}"
    );
    // Each box under its own origin.
    assert!(text[own..first].contains("- [ ] own 2") && !text[own..first].contains("first 0"));
    assert!(
        text[first..second].contains("- [ ] first 11") && !text[first..second].contains("second 0")
    );
    assert!(text[second..].contains("- [ ] second 7"));
}

#[test]
fn a_reopen_skips_an_origin_with_nothing_open_and_names_a_post_deploy_box() {
    let gate = titled(&[
        (
            "ABC-1",
            "The ticket",
            "- [x] a\n## Post deploy\n- [ ] it holds in production",
        ),
        ("ABC-2", "Nothing left", "- [x] b"),
        (
            "ABC-3",
            "Its own check",
            "## Post deploy\n- [ ] it is signed in production",
        ),
    ]);
    let text = reopen_comment("ABC-1", &gate);
    assert!(!text.contains("ABC-2"), "{text}");
    assert!(
        text.contains("- [ ] it holds in production (Post deploy)"),
        "{text}"
    );
    assert!(
        text.contains("- [ ] it is signed in production (Post deploy)"),
        "{text}"
    );
}

#[test]
fn a_reopen_with_duplicate_boxes_says_how_to_reconcile_them_and_what_comes_next() {
    let gate = titled(&[
        ("ABC-1", "The ticket", "- [x] a"),
        ("ABC-2", "A duplicate", "- [ ] b"),
    ]);
    let text = reopen_comment("ABC-1", &gate);
    assert!(text.contains(RECONCILE_DUPLICATES), "{text}");
    for words in [
        "check it on the duplicate and comment there with the evidence",
        "(the comment, PR or test that proves it)",
        "a gesture in production, hand it to a human through a decision request",
        "nothing is lost in a merge",
    ] {
        assert!(RECONCILE_DUPLICATES.contains(words), "{words}");
    }
    assert!(text.contains(WHAT_COMES_NEXT), "{text}");
    assert!(WHAT_COMES_NEXT.contains("goes to the `Post Deploy` column as usual"));
    // In that order, the unassignment last.
    let listed = text.find("- [ ] b").unwrap();
    let reconcile = text.find(RECONCILE_DUPLICATES).unwrap();
    let next = text.find(WHAT_COMES_NEXT).unwrap();
    assert!(listed < reconcile && reconcile < next, "{text}");
    assert!(text.ends_with(RETURNED_UNASSIGNED), "{text}");
}

#[test]
fn a_reopen_with_only_its_own_boxes_has_nothing_to_reconcile() {
    let gate = titled(&[
        ("ABC-1", "The ticket", "- [ ] a"),
        ("ABC-2", "A duplicate", "- [x] b"),
    ]);
    let text = reopen_comment("ABC-1", &gate);
    assert!(!text.contains(RECONCILE_DUPLICATES), "{text}");
    assert!(text.contains(WHAT_COMES_NEXT), "{text}");
}

#[test]
fn a_reopen_for_an_unreadable_or_absent_contract_lists_no_boxes() {
    for gate in [
        evaluate(&[member("ABC-1", "- [ ] a")], Some("relations cut".into())),
        gate_of(&[("ABC-1", "nothing")]),
    ] {
        let text = reopen_comment("ABC-1", &gate);
        assert!(
            !text.contains(RECONCILE_DUPLICATES) && !text.contains(WHAT_COMES_NEXT),
            "{text}"
        );
        assert!(text.ends_with(RETURNED_UNASSIGNED));
    }
}

#[test]
fn the_duplicates_boxes_hold_the_ticket_until_checked_on_the_duplicate() {
    let open = gate_of(&[
        ("ABC-1", "- [x] a\n## Post deploy\n- [ ] it holds"),
        ("ABC-2", "- [ ] b"),
    ]);
    assert_eq!(
        closure_verdict("ABC-1", Some(Status::ToDeploy), &open),
        ClosureVerdict::Reopen,
        "nothing is lost in a merge"
    );
    let checked = gate_of(&[
        ("ABC-1", "- [x] a\n## Post deploy\n- [ ] it holds"),
        ("ABC-2", "- [x] b"),
    ]);
    assert_eq!(
        closure_verdict("ABC-1", Some(Status::ToDeploy), &checked),
        ClosureVerdict::PostDeploy,
        "only its own post-deploy boxes left: Post Deploy as usual"
    );
}

#[test]
fn done_gate_refusal_names_the_absent_contract_not_a_zero_count() {
    let text = refusal("ABC-1", &gate_of(&[("ABC-1", "nothing")]), true);
    assert!(text.contains("no criterion declared"));
    assert!(!text.contains("0 criteria"));
    assert!(text.ends_with("State unchanged."));
}

#[test]
fn done_gate_refusal_names_the_absent_contract_without_reopen_line_when_reopening() {
    let text = refusal("ABC-1", &gate_of(&[("ABC-1", "nothing")]), false);
    assert!(!text.contains("State unchanged"));
}

#[test]
fn done_gate_refusal_names_the_unreadable_reason_instead() {
    let m = vec![member("ABC-1", "- [x] a")];
    let text = refusal(
        "ABC-1",
        &evaluate(&m, Some("ABC-9 could not be read: timeout".into())),
        true,
    );
    assert!(text.contains("could not be read in full (ABC-9 could not be read: timeout)"));
    assert!(text.contains("a read that fails never promotes"));
}

#[test]
fn a_reopened_ticket_is_said_unassigned_with_nobody_named() {
    let gate = gate_of(&[("ABC-1", "- [ ] a")]);
    let text = reopen_comment("ABC-1", &gate);
    assert!(text.starts_with(&refusal("ABC-1", &gate, false)));
    assert!(text.ends_with(RETURNED_UNASSIGNED), "{text}");
    assert!(!text.contains("reassigned"));
}
