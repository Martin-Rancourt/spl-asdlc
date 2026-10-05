//! Carried from `tests/test_linear_service.py` and `tests/test_release_service.py`
//! (the Done gate, duplicates and the post-deploy section).

use super::*;
use std::collections::HashMap;

fn member(id: &str, description: &str) -> ContractMember {
    ContractMember {
        id: id.into(),
        description: description.into(),
    }
}

fn gate_of(members: &[(&str, &str)]) -> DoneGate {
    let m: Vec<ContractMember> = members.iter().map(|(i, d)| member(i, d)).collect();
    evaluate(&m, None)
}

fn node(description: &str, duplicates: &[&str]) -> DuplicateNode {
    DuplicateNode {
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
    assert!(text.contains("[ABC-1] - [ ] a"));
    assert!(text.contains("[ABC-2] - [ ] b"));
}

#[test]
fn done_gate_refusal_quotes_five_boxes_then_counts_the_rest() {
    let many = (0..8)
        .map(|i| format!("- [ ] box {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let text = refusal("ABC-1", &gate_of(&[("ABC-1", &many)]), false);
    assert!(text.contains("box 4"));
    assert!(!text.contains("box 5"));
    assert!(text.ends_with("+3 more"));
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
fn reassigns_to_its_author_or_says_why_it_is_left_unassigned() {
    let gate = gate_of(&[("ABC-1", "- [ ] a")]);
    let assigned = reopen_comment("ABC-1", &gate, Ok("implementer-handle"));
    assert!(assigned.contains("reassigned to its implementer (implementer-handle)"));
    let alone = reopen_comment("ABC-1", &gate, Err("no implementer is known"));
    assert!(alone.contains("left unassigned — no implementer is known"));
    assert!(alone.contains("The default implementer picks it up"));
}
