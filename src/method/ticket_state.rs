//! A ticket and its PRs (`docs/spec/04-pr-review.md` §10): one PR per ticket
//! by default, every PR independently deployable, and the state of the
//! ticket decided by two rules applied in order (BR-ASDLC-008) whenever one
//! of its PRs resolves (merged, or closed without merge) or a ticket past the
//! merge has an open PR:
//!
//! 1. **The review rule** has priority: while another PR of the ticket is in
//!    review, the ticket is In Review and that PR's review → deploy cycle
//!    runs in full; a PR the coordinator sent back puts it In Progress
//!    (spec 04 §5).
//! 2. **The completeness rule**: with no PR in review, a ticket whose boxes
//!    are not all checked (its `## Post deploy` section and its
//!    bridge-verified boxes set apart) goes back
//!    In Progress; an implementer resumes, and its next PR brings it to
//!    review again.
//!
//! Then REQ-MERGE-028, kept below them: a ticket every box of which is
//! checked, with an open PR not yet in review, is In Review; and only a
//! ticket with nothing open, every box checked and a PR merged goes To
//! Deploy.

use super::contract;
use super::vocabulary::Status;

/// An open PR of the ticket, as the bridge sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenPr {
    pub number: u64,
    pub draft: bool,
    /// A coordinator's request for changes stands at its head (spec 04 §5).
    pub sent_back: bool,
    /// It has a review round on the ticket, open or finished: an angle
    /// sub-item or the consolidation item scoped to it, not canceled. A PR
    /// whose round is done but which waits for its merge (an approval, the
    /// one-merge slot) is still in review: its cycle runs in full, and the
    /// pass, which opened the round, agrees.
    pub in_round: bool,
}

impl OpenPr {
    /// In review: ready, in its round, not sent back.
    pub fn in_review(&self) -> bool {
        !self.draft && !self.sent_back && self.in_round
    }
}

/// What the rules read.
#[derive(Debug, Clone)]
pub struct Facts<'a> {
    /// The ticket's open PRs, the one resolving now excluded.
    pub open: &'a [OpenPr],
    /// Every open PR of the repository had its ticket read: "no other PR"
    /// can be said. When it cannot, only "another PR is in review" can.
    pub open_known: bool,
    pub description: &'a str,
    /// A PR of the ticket was merged: the one resolving now, or one before.
    pub merged: bool,
}

/// Why the rules put the ticket where they do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// The review rule: another PR of the ticket is in review.
    InReview(u64),
    /// The review rule: the coordinator sent another PR of the ticket back.
    SentBack(u64),
    /// The completeness rule: these boxes are open; empty when the ticket
    /// declares no box at all (nothing declared is not everything checked).
    BoxesOpen(Vec<String>),
    /// REQ-MERGE-028: an open PR, not in review yet, on a complete ticket.
    OpenPr(u64),
    /// Every box checked but no PR of the ticket merged.
    NothingMerged,
    /// Every box checked, a PR merged, nothing open.
    Complete,
}

impl Why {
    /// The `next:` line of the comment that records the transition.
    pub fn next(&self) -> String {
        match self {
            Why::InReview(n) => format!(
                "PR #{n} of this ticket is in review: the ticket stays `In Review` until that PR resolves."
            ),
            Why::SentBack(n) => format!(
                "the coordinator requested changes on PR #{n} of this ticket: its implementer resumes there."
            ),
            Why::BoxesOpen(open) if open.is_empty() => {
                "this ticket declares no box, so nothing says it is complete: its implementer resumes."
                    .to_string()
            }
            Why::BoxesOpen(open) => format!(
                "{} box(es) of its contract remain open: its implementer resumes, and the next PR brings it back to review.",
                open.len()
            ),
            Why::OpenPr(n) => format!(
                "PR #{n} of this ticket is open: the ticket waits in `In Review` for that PR's review."
            ),
            Why::NothingMerged => {
                "no PR of this ticket is merged: its implementer resumes.".to_string()
            }
            Why::Complete => {
                "the release closure moves it to `Done` once every box of its contract is checked."
                    .to_string()
            }
        }
    }
}

/// Where the ticket goes. `None` when it cannot be said (an open PR whose
/// ticket could not be read may be this ticket's): fail closed, nothing is
/// written, the next tick decides.
pub fn decide(f: &Facts) -> Option<(Status, Why)> {
    if let Some(pr) = f.open.iter().find(|p| p.in_review()) {
        return Some((Status::InReview, Why::InReview(pr.number)));
    }
    if !f.open_known {
        return None;
    }
    if let Some(pr) = f.open.iter().find(|p| p.sent_back) {
        return Some((Status::InProgress, Why::SentBack(pr.number)));
    }
    match contract::open_before_deploy(f.description) {
        None => return Some((Status::InProgress, Why::BoxesOpen(Vec::new()))),
        Some(open) if !open.is_empty() => return Some((Status::InProgress, Why::BoxesOpen(open))),
        Some(_) => {}
    }
    if let Some(pr) = f.open.first() {
        return Some((Status::InReview, Why::OpenPr(pr.number)));
    }
    if !f.merged {
        return Some((Status::InProgress, Why::NothingMerged));
    }
    Some((Status::ToDeploy, Why::Complete))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &str =
        "## Acceptance criteria\n- [x] a\n- [x] b\n\n## Post deploy\n- [ ] in production\n";
    const OPEN: &str = "## Acceptance criteria\n- [x] a\n- [ ] b\n";

    fn pr(number: u64) -> OpenPr {
        OpenPr {
            number,
            draft: false,
            sent_back: false,
            in_round: false,
        }
    }

    fn facts<'a>(open: &'a [OpenPr], description: &'a str) -> Facts<'a> {
        Facts {
            open,
            open_known: true,
            description,
            merged: true,
        }
    }

    // ---- BR-ASDLC-008 ----

    #[test]
    fn every_box_checked_and_nothing_open_goes_to_deploy() {
        assert_eq!(
            decide(&facts(&[], ALL)),
            Some((Status::ToDeploy, Why::Complete)),
            "the post-deploy section is set apart"
        );
    }

    #[test]
    fn an_open_box_sends_the_ticket_back_to_work() {
        assert_eq!(
            decide(&facts(&[], OPEN)),
            Some((Status::InProgress, Why::BoxesOpen(vec!["- [ ] b".into()])))
        );
        assert_eq!(
            decide(&facts(&[], "no box at all")),
            Some((Status::InProgress, Why::BoxesOpen(Vec::new()))),
            "nothing declared is not everything checked"
        );
        assert_eq!(
            decide(&facts(
                &[],
                "## Notes\n- [ ] a box under any heading\n## Acceptance criteria\n- [x] a\n"
            ))
            .map(|d| d.0),
            Some(Status::InProgress),
            "every box of the description counts (spec 01 §3)"
        );
    }

    #[test]
    fn the_review_rule_comes_before_the_completeness_rule() {
        let open = [OpenPr {
            in_round: true,
            ..pr(8)
        }];
        assert_eq!(
            decide(&facts(&open, OPEN)),
            Some((Status::InReview, Why::InReview(8)))
        );
        assert_eq!(
            decide(&Facts {
                open_known: false,
                ..facts(&open, OPEN)
            }),
            Some((Status::InReview, Why::InReview(8))),
            "a PR in review is enough, whatever else is unknown"
        );
    }

    #[test]
    fn a_pr_sent_back_or_a_draft_is_not_in_review() {
        let sent_back = [OpenPr {
            in_round: true,
            sent_back: true,
            ..pr(8)
        }];
        assert_eq!(
            decide(&facts(&sent_back, ALL)),
            Some((Status::InProgress, Why::SentBack(8)))
        );
        let draft = [OpenPr {
            draft: true,
            in_round: true,
            ..pr(8)
        }];
        assert_eq!(
            decide(&facts(&draft, OPEN)).map(|d| d.0),
            Some(Status::InProgress)
        );
    }

    #[test]
    fn an_open_pr_on_a_complete_ticket_keeps_it_in_review() {
        // REQ-MERGE-028, below the two rules.
        let draft = [OpenPr {
            draft: true,
            ..pr(8)
        }];
        assert_eq!(
            decide(&facts(&draft, ALL)),
            Some((Status::InReview, Why::OpenPr(8)))
        );
    }

    #[test]
    fn what_cannot_be_read_decides_nothing() {
        assert_eq!(
            decide(&Facts {
                open_known: false,
                ..facts(&[], ALL)
            }),
            None
        );
    }

    #[test]
    fn a_complete_ticket_with_nothing_merged_is_not_to_deploy() {
        assert_eq!(
            decide(&Facts {
                merged: false,
                ..facts(&[], ALL)
            }),
            Some((Status::InProgress, Why::NothingMerged))
        );
    }
}
