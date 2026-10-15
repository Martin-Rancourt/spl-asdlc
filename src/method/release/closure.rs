//! The closure of a cycle (`docs/spec/05-release-cycle.md` §11,
//! REQ-REL-084..091): what happens to each member of the release, judged by
//! code, and the comment that says so.

use crate::method::business_rules::{Registry, cited_ids, rule_prefix};
use crate::method::done_gate::{self, ClosureVerdict, DoneGate};
use crate::method::vocabulary::{ItemState, StateKind, Status};

/// A member of the release: an item carrying its label or a child of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub id: String,
    pub state: ItemState,
    /// An angle sub-item or the consolidation item (a child the bridge made).
    pub bridge_child: bool,
}

/// What the closure does to one member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// An open sub-item closes with its release.
    CloseChild,
    /// A finding left in Triage/Backlog on purpose.
    KeepFinding,
    /// Already Done or canceled: no write.
    Untouched,
    /// An embarked ticket to judge.
    Judge,
}

/// REQ-REL-086: sub-items close, findings stay, resolved items are left,
/// everything else is judged.
pub fn classify(member: &Member) -> Action {
    if member.bridge_child {
        return if member.state.is_resolved() {
            Action::Untouched
        } else {
            Action::CloseChild
        };
    }
    match member.state.kind {
        StateKind::Triage | StateKind::Backlog => Action::KeepFinding,
        StateKind::Canceled => Action::Untouched,
        StateKind::Completed if member.state.status == Some(Status::Done) => Action::Untouched,
        _ => Action::Judge,
    }
}

/// What an item is to a closed cycle, for its archiving (BR-ASDLC-019).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolved {
    /// A child of the release item: an angle sub-item, the consolidation
    /// item, a gate-fix ticket.
    ReleaseChild,
    /// The shared nits ticket of the release.
    Nits,
    /// A ticket carrying the release label: an embarked ticket, a finding.
    Embarked,
    /// An angle sub-item or the consolidation item of a PR review round of
    /// an embarked ticket (recognised by its bridge marker).
    ReviewRound,
}

/// Whether the closure archives this item (BR-ASDLC-019): a release child,
/// the nits ticket or a review round item once resolved (Done or Canceled);
/// an embarked ticket only in Done, as the closure moves it. An open item
/// never: Post Deploy, In Progress, a finding in Triage, an open nits
/// ticket; nor a state the status table does not place (fail closed). An
/// item already archived is not archived again.
pub fn archivable(kind: Resolved, state: &ItemState, archived: bool) -> bool {
    if archived {
        return false;
    }
    match kind {
        Resolved::ReleaseChild | Resolved::Nits | Resolved::ReviewRound => {
            state.status.is_some_and(Status::is_resolved)
        }
        Resolved::Embarked => state.status == Some(Status::Done),
    }
}

/// Whether judging this ticket needs the registry at the release tag.
pub fn needs_registry(description: &str) -> bool {
    !cited_ids(description).is_empty()
}

/// The judgement of one embarked ticket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Judgement {
    /// Every box ticked.
    Done,
    /// Only its own post-deploy boxes remain.
    PostDeploy,
    /// Open boxes, unreadable or absent contract: back to In Progress,
    /// unassigned (BR-ASDLC-009).
    Reopen,
    /// It cites rules the registry at the tag does not declare: back to In
    /// Progress, unassigned too, with the missing ids.
    UnknownRules(Vec<String>),
    /// It cites rules and the registry could not be read: left in its state.
    RulesUnverified(String),
}

/// Judge one ticket (REQ-REL-087/088): the registry guard first, then the
/// Done gate. `registry` is `None` only when the ticket cites no rule.
pub fn judge(
    id: &str,
    status: Option<Status>,
    description: &str,
    gate: &DoneGate,
    registry: Option<&Registry>,
) -> Judgement {
    let cited = cited_ids(description);
    if !cited.is_empty() {
        let empty = std::collections::BTreeSet::new();
        let ids = match registry {
            None => return Judgement::RulesUnverified("the registry was not read".into()),
            Some(Registry::Unreadable(why)) => {
                return Judgement::RulesUnverified(why.chars().take(200).collect());
            }
            // Absent at the tag: a cited id must exist, so none does (spec 02 §5).
            Some(Registry::Absent) => &empty,
            Some(Registry::Ids(ids)) => ids,
        };
        // An id of another product's prefix is that product's to judge.
        let owned: std::collections::BTreeSet<&str> = ids.iter().map(|i| rule_prefix(i)).collect();
        let missing: Vec<String> = cited
            .into_iter()
            .filter(|c| !ids.contains(c))
            .filter(|c| owned.is_empty() || owned.contains(rule_prefix(c)))
            .collect();
        if !missing.is_empty() {
            return Judgement::UnknownRules(missing);
        }
    }
    match done_gate::closure_verdict(id, status, gate) {
        ClosureVerdict::Done => Judgement::Done,
        ClosureVerdict::PostDeploy => Judgement::PostDeploy,
        ClosureVerdict::Reopen => Judgement::Reopen,
    }
}

/// The comment on a ticket reopened for rules the registry lacks.
pub fn unknown_rules_comment(missing: &[String], product: &str) -> String {
    let ids = missing
        .iter()
        .map(|i| format!("`{i}`"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Business rules: {ids} cited in `Business rules` (no file in `{}/` of `{product}` at this release's tag; \
         if the id is a typo, fix it in `Business rules`; if the rule is new, open a PR adding its file — it ships \
         with the next release). {}",
        crate::method::business_rules::REGISTRY_DIR,
        crate::method::done_gate::RETURNED_UNASSIGNED
    )
}

/// What the closure did, for its comment and the verb's output.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CloseResult {
    pub closed: Vec<String>,
    pub incomplete: Vec<String>,
    pub awaiting_post_release: Vec<String>,
    pub rules_unverified: Vec<(String, String)>,
    pub kept_findings: Vec<String>,
    pub children_closed: Vec<String>,
    pub repairs: Vec<String>,
}

impl CloseResult {
    /// Whether the release item itself may move to Done: not while a ticket
    /// that cites a rule could not be judged (REQ-REL-091, editor's decision).
    pub fn complete(&self) -> bool {
        self.rules_unverified.is_empty()
    }
}

/// The closure's comment on the release item (REQ-REL-091).
pub fn comment(r: &CloseResult) -> String {
    let n = r.closed.len();
    let mut out = format!(
        "Deployed. This item and the release's issues move to `Done` in code — {n} issue(s)."
    );
    let list = |ids: &[String]| {
        ids.iter()
            .map(|i| format!("`{i}`"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    if !r.kept_findings.is_empty() {
        out.push_str(&format!(
            "\n\n{} non-blocking finding(s) remain in the backlog on purpose: {}.",
            r.kept_findings.len(),
            list(&r.kept_findings)
        ));
    }
    if !r.incomplete.is_empty() {
        out.push_str(&format!(
            "\n\nBack to `In Progress` (open boxes in their contract): {}.",
            list(&r.incomplete)
        ));
    }
    if !r.rules_unverified.is_empty() {
        let ids: Vec<String> = r.rules_unverified.iter().map(|(i, _)| i.clone()).collect();
        out.push_str(&format!(
            "\n\nLeft in their state because they cite a business rule and the registry could not be read at the tag — nothing was judged; the next tick judges them again, and this item stays open until then: {}.",
            list(&ids)
        ));
    }
    if !r.awaiting_post_release.is_empty() {
        out.push_str(&format!(
            "\n\nMoved to `Post Deploy`: {} — their `## Post deploy` boxes are verified in production and checked on each ticket, as its validation sub-task asks; the bridge moves each ticket to `Done` at the tick that finds all its `## Post deploy` boxes checked.",
            list(&r.awaiting_post_release)
        ));
    }
    if !r.repairs.is_empty() {
        out.push_str(&format!(
            "\n\nRepaired before closing: {}.",
            r.repairs.join("; ")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::method::done_gate::{ContractMember, evaluate};

    fn member(state: ItemState, bridge_child: bool) -> Member {
        Member {
            id: "ABC-1".into(),
            state,
            bridge_child,
        }
    }

    #[test]
    fn the_closure_archives_resolved_items_and_never_an_open_one() {
        use super::Resolved::*;
        let of = ItemState::of;
        for kind in [ReleaseChild, Nits, ReviewRound] {
            assert!(archivable(kind, &of(Status::Done), false), "{kind:?}");
            assert!(archivable(kind, &of(Status::Canceled), false), "{kind:?}");
            assert!(!archivable(kind, &of(Status::Ready), false), "{kind:?}");
            assert!(!archivable(kind, &of(Status::Triage), false), "{kind:?}");
        }
        assert!(archivable(Embarked, &of(Status::Done), false));
        for open in [
            Status::PostDeploy,
            Status::InProgress,
            Status::InReview,
            Status::ToDeploy,
            Status::Triage,
            Status::Backlog,
            Status::Canceled,
        ] {
            assert!(!archivable(Embarked, &of(open), false), "{open}");
        }
        assert!(
            !archivable(ReleaseChild, &of(Status::Done), true),
            "already archived"
        );
        let unplaced = ItemState {
            name: "Shipped".into(),
            status: None,
            kind: StateKind::Completed,
        };
        assert!(
            !archivable(ReleaseChild, &unplaced, false),
            "a state the table does not place is never archived"
        );
    }

    #[test]
    fn closure_distinguishes_children_findings_and_shipped_tickets() {
        assert_eq!(
            classify(&member(ItemState::of(Status::Ready), true)),
            Action::CloseChild
        );
        assert_eq!(
            classify(&member(ItemState::of(Status::Done), true)),
            Action::Untouched
        );
        assert_eq!(
            classify(&member(ItemState::of(Status::Triage), false)),
            Action::KeepFinding
        );
        assert_eq!(
            classify(&member(ItemState::of(Status::Backlog), false)),
            Action::KeepFinding
        );
        assert_eq!(
            classify(&member(ItemState::of(Status::Done), false)),
            Action::Untouched
        );
        assert_eq!(
            classify(&member(ItemState::of(Status::Canceled), false)),
            Action::Untouched
        );
        assert_eq!(
            classify(&member(ItemState::of(Status::ToDeploy), false)),
            Action::Judge,
            "to-deploy is judged though completed"
        );
        assert_eq!(
            classify(&member(ItemState::of(Status::PostDeploy), false)),
            Action::Judge
        );
        assert_eq!(
            classify(&member(ItemState::of(Status::InProgress), false)),
            Action::Judge
        );
    }

    fn gate(desc: &str) -> DoneGate {
        evaluate(
            &[ContractMember {
                id: "ABC-1".into(),
                description: desc.into(),
            }],
            None,
        )
    }

    const CITES: &str = "## Acceptance criteria\n- [x] a\n## Business rules\n- BR-ACME-001\n";

    #[test]
    fn a_ticket_citing_a_rule_that_exists_is_closed() {
        let reg = Registry::Ids(["BR-ACME-001".to_string()].into());
        assert_eq!(
            judge(
                "ABC-1",
                Some(Status::ToDeploy),
                CITES,
                &gate(CITES),
                Some(&reg)
            ),
            Judgement::Done
        );
    }

    #[test]
    fn a_ticket_citing_a_rule_not_in_the_registry_is_reopened() {
        let reg = Registry::Ids(["BR-ACME-002".to_string()].into());
        assert_eq!(
            judge(
                "ABC-1",
                Some(Status::ToDeploy),
                CITES,
                &gate(CITES),
                Some(&reg)
            ),
            Judgement::UnknownRules(vec!["BR-ACME-001".into()])
        );
        let text = unknown_rules_comment(&["BR-ACME-001".into()], "acme");
        assert!(text.contains("at this release's tag"));
        assert!(text.ends_with(crate::method::done_gate::RETURNED_UNASSIGNED));
    }

    #[test]
    fn an_unreadable_registry_fails_closed_on_a_ticket_that_cites_a_rule() {
        let reg = Registry::Unreadable("HTTP 502".into());
        assert_eq!(
            judge(
                "ABC-1",
                Some(Status::ToDeploy),
                CITES,
                &gate(CITES),
                Some(&reg)
            ),
            Judgement::RulesUnverified("HTTP 502".into())
        );
        assert!(matches!(
            judge("ABC-1", Some(Status::ToDeploy), CITES, &gate(CITES), None),
            Judgement::RulesUnverified(_)
        ));
    }

    #[test]
    fn a_ticket_citing_a_rule_of_a_product_with_no_registry_at_its_tag_is_reopened() {
        assert_eq!(
            judge(
                "ABC-1",
                Some(Status::ToDeploy),
                CITES,
                &gate(CITES),
                Some(&Registry::Absent)
            ),
            Judgement::UnknownRules(vec!["BR-ACME-001".into()])
        );
    }

    #[test]
    fn an_id_of_another_product_is_left_to_that_products_registry_at_closure() {
        let reg = Registry::Ids(["BR-ACME-002".to_string()].into());
        let foreign = "## Acceptance criteria\n- [x] a\n## Business rules\n- BR-OTHER-001\n";
        assert_eq!(
            judge(
                "ABC-1",
                Some(Status::ToDeploy),
                foreign,
                &gate(foreign),
                Some(&reg)
            ),
            Judgement::Done
        );
    }

    #[test]
    fn a_closure_whose_tickets_cite_no_rule_never_reads_the_registry() {
        assert!(!needs_registry("## Business rules\nNone.\n"));
        let desc = "- [x] a\n";
        assert_eq!(
            judge("ABC-1", Some(Status::ToDeploy), desc, &gate(desc), None),
            Judgement::Done
        );
    }

    #[test]
    fn open_boxes_and_post_deploy_boxes_take_their_paths() {
        let open = "- [x] a\n- [ ] b\n";
        assert_eq!(
            judge("ABC-1", Some(Status::ToDeploy), open, &gate(open), None),
            Judgement::Reopen
        );
        let post = "- [x] a\n## Post deploy\n- [ ] verified\n";
        assert_eq!(
            judge("ABC-1", Some(Status::ToDeploy), post, &gate(post), None),
            Judgement::PostDeploy
        );
    }

    #[test]
    fn the_closure_comment_names_what_stays_open_and_why() {
        let r = CloseResult {
            closed: vec!["ABC-1".into(), "ABC-2".into()],
            incomplete: vec!["ABC-3".into()],
            awaiting_post_release: vec!["ABC-4".into()],
            rules_unverified: vec![("ABC-5".into(), "HTTP 502".into())],
            kept_findings: vec!["ABC-6".into()],
            ..CloseResult::default()
        };
        let text = comment(&r);
        assert!(text.starts_with(
            "Deployed. This item and the release's issues move to `Done` in code — 2 issue(s)."
        ));
        assert!(
            text.contains("1 non-blocking finding(s) remain in the backlog on purpose: `ABC-6`")
        );
        assert!(text.contains("Back to `In Progress` (open boxes in their contract): `ABC-3`"));
        assert!(text.contains("the next tick judges them again"));
        assert!(text.contains("Moved to `Post Deploy`: `ABC-4`"));
        // BR-ASDLC-002: the boxes are the gesture, no verb.
        assert!(text.contains("at the tick that finds all its `## Post deploy` boxes checked"));
        assert!(!text.contains("post-release-check"));
        assert!(!r.complete());
        assert_eq!(
            comment(&CloseResult::default()),
            "Deployed. This item and the release's issues move to `Done` in code — 0 issue(s)."
        );
    }
}
