//! The GO (`docs/spec/05-release-cycle.md` §8, REQ-REL-066/067): a pure
//! reading of facts others established. Nobody gives it; under the `human`
//! policy the owner's approval is one more condition, never a substitute. A
//! candidate with no QA angle (`release.angles = []`) is judged without the
//! angle condition: its GO rests on the gate and the conditions that involve
//! no angle.

use serde_json::{Value, json};

/// Where one configured angle stands for the latest candidate (REQ-REL-055).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AngleState {
    /// No sub-item: nobody was ever asked, so nothing will close it.
    NotOffered,
    /// Sub-item exists, no PR yet.
    Offered,
    /// Angle PR open.
    InReview,
    Merged,
    /// Report invalid, or the PR closed without merging.
    FailedReview,
}

impl AngleState {
    pub fn name(self) -> &'static str {
        match self {
            AngleState::NotOffered => "not offered",
            AngleState::Offered => "offered",
            AngleState::InReview => "in review",
            AngleState::Merged => "merged",
            AngleState::FailedReview => "failed review",
        }
    }
}

/// The facts the GO reads, established by the caller.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GoFacts {
    pub release: String,
    /// `Some(true)` only for a gate run that concluded `success` on the
    /// latest RC; unknown is not green.
    pub gate_green: Option<bool>,
    /// The angles the GO expects, in configuration order, with their state.
    pub angles: Vec<(String, AngleState)>,
    /// The candidate has no QA angle (`release.angles = []`, or the
    /// candidate was recorded while none was configured): an empty `angles`
    /// is then expected, not a round that never ran.
    pub no_qa_angle: bool,
    pub blockers: Vec<String>,
    pub unlinked_blockers: Vec<String>,
    pub fix_gaps: Vec<String>,
    pub tip_gaps: Vec<String>,
    pub policy: Policy,
}

/// Who gives the GO besides the facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Policy {
    /// The facts alone.
    #[default]
    Computed,
    /// The facts and the owner's approval on the release issue; `None` when
    /// it could not be read, which never passes.
    Human { approved: Option<bool> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoDecision {
    pub go: bool,
    pub release: String,
    pub gate_green: bool,
    pub open_angles: Vec<String>,
    pub missing_angles: Vec<String>,
    pub blockers: Vec<String>,
    pub unlinked_blockers: Vec<String>,
    pub fix_gaps: Vec<String>,
    pub tip_gaps: Vec<String>,
    pub policy: Policy,
    /// One English sentence per failing condition; never empty when `go` is false.
    pub reasons: Vec<String>,
}

fn dedup(list: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in list {
        if !out.contains(item) {
            out.push(item.clone());
        }
    }
    out
}

/// Compute the GO.
pub fn evaluate(facts: &GoFacts) -> GoDecision {
    let gate_green = facts.gate_green == Some(true);
    let missing_angles: Vec<String> = facts
        .angles
        .iter()
        .filter(|(_, s)| *s == AngleState::NotOffered)
        .map(|(a, _)| a.clone())
        .collect();
    let open_angles: Vec<String> = facts
        .angles
        .iter()
        .filter(|(_, s)| !matches!(s, AngleState::NotOffered | AngleState::Merged))
        .map(|(a, s)| format!("{a} ({})", s.name()))
        .collect();
    let unlinked = dedup(&facts.unlinked_blockers);
    let mut blockers = dedup(&facts.blockers);
    for u in &unlinked {
        if !blockers.contains(u) {
            blockers.push(u.clone());
        }
    }
    let fix_gaps = dedup(&facts.fix_gaps);
    let tip_gaps = dedup(&facts.tip_gaps);

    let mut reasons = Vec::new();
    if !gate_green {
        reasons.push("the CI gate is not green".to_string());
    }
    if facts.angles.is_empty() && !facts.no_qa_angle {
        reasons.push("the release carries no angle sub-item and no verdict".to_string());
    }
    if !missing_angles.is_empty() {
        reasons.push(format!("angles missing: {}", missing_angles.join(", ")));
    }
    if !open_angles.is_empty() {
        reasons.push(format!("angles not merged: {}", open_angles.join(", ")));
    }
    if !blockers.is_empty() {
        reasons.push(format!("open blocking issues: {}", blockers.join(", ")));
    }
    if !unlinked.is_empty() {
        reasons.push(format!(
            "findings marked as blocking whose `blocks` relation is missing and could not be re-laid: {}",
            unlinked.join(", ")
        ));
    }
    if !fix_gaps.is_empty() {
        reasons.push(format!(
            "resolved blockers whose fix is not in the candidate (a new candidate that contains it frees them): {}",
            fix_gaps.join("; ")
        ));
    }
    if !tip_gaps.is_empty() {
        reasons.push(format!(
            "the candidate is not the release branch tip: {}",
            tip_gaps.join("; ")
        ));
    }
    match facts.policy {
        Policy::Computed
        | Policy::Human {
            approved: Some(true),
        } => {}
        Policy::Human {
            approved: Some(false),
        } => reasons.push("the owner has not approved the release (GO policy `human`)".to_string()),
        Policy::Human { approved: None } => {
            reasons.push("the owner's approval could not be read (GO policy `human`)".to_string())
        }
    }
    GoDecision {
        go: reasons.is_empty(),
        release: facts.release.clone(),
        gate_green,
        open_angles,
        missing_angles,
        blockers,
        unlinked_blockers: unlinked,
        fix_gaps,
        tip_gaps,
        policy: facts.policy,
        reasons,
    }
}

impl GoDecision {
    /// The only value a gate record's `Blockers-Carried:` line may hold.
    pub fn blockers_carried(&self) -> String {
        if self.blockers.is_empty() {
            "none".to_string()
        } else {
            self.blockers.join(", ")
        }
    }

    /// Every angle has answered: none missing, none still open.
    pub fn every_angle_answered(&self) -> bool {
        self.open_angles.is_empty() && self.missing_angles.is_empty()
    }

    pub fn as_json(&self) -> Value {
        json!({
            "go": self.go,
            "release": self.release,
            "gate_green": self.gate_green,
            "open_angles": self.open_angles,
            "missing_angles": self.missing_angles,
            "blockers": self.blockers,
            "unlinked_blockers": self.unlinked_blockers,
            "fix_gaps": self.fix_gaps,
            "tip_gaps": self.tip_gaps,
            "owner_approved": match self.policy {
                Policy::Computed => Value::Null,
                Policy::Human { approved } => json!(approved),
            },
            "blockers_carried": self.blockers_carried(),
            "reasons": self.reasons,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_good() -> GoFacts {
        GoFacts {
            release: "ABC-100".into(),
            gate_green: Some(true),
            angles: vec![
                ("security".into(), AngleState::Merged),
                ("product".into(), AngleState::Merged),
            ],
            ..GoFacts::default()
        }
    }

    #[test]
    fn the_go_is_computed_when_all_conditions_hold() {
        let d = evaluate(&all_good());
        assert!(d.go, "{:?}", d.reasons);
        assert_eq!(d.blockers_carried(), "none");
    }

    #[test]
    fn a_red_or_unknown_gate_is_not_a_go() {
        for gate in [Some(false), None] {
            let d = evaluate(&GoFacts {
                gate_green: gate,
                ..all_good()
            });
            assert!(!d.go);
            assert_eq!(d.reasons, vec!["the CI gate is not green"]);
        }
    }

    #[test]
    fn an_open_angle_is_not_a_go() {
        let mut f = all_good();
        f.angles[1].1 = AngleState::InReview;
        let d = evaluate(&f);
        assert_eq!(d.reasons, vec!["angles not merged: product (in review)"]);
    }

    #[test]
    fn a_release_missing_a_review_is_not_a_go() {
        let mut f = all_good();
        f.angles[0].1 = AngleState::NotOffered;
        let d = evaluate(&f);
        assert_eq!(d.missing_angles, vec!["security"]);
        assert_eq!(d.reasons, vec!["angles missing: security"]);
    }

    #[test]
    fn a_release_with_no_qa_sub_issue_is_not_a_go() {
        let d = evaluate(&GoFacts {
            angles: vec![],
            ..all_good()
        });
        assert!(
            d.reasons
                .contains(&"the release carries no angle sub-item and no verdict".to_string())
        );
    }

    fn no_qa_angle() -> GoFacts {
        GoFacts {
            angles: vec![],
            no_qa_angle: true,
            ..all_good()
        }
    }

    #[test]
    fn with_no_qa_angle_a_green_gate_is_a_go() {
        let d = evaluate(&no_qa_angle());
        assert!(d.go, "{:?}", d.reasons);
        assert!(d.every_angle_answered());
        assert_eq!(d.blockers_carried(), "none");
    }

    #[test]
    fn with_no_qa_angle_a_red_or_unknown_gate_is_not_a_go() {
        for gate in [Some(false), None] {
            let d = evaluate(&GoFacts {
                gate_green: gate,
                ..no_qa_angle()
            });
            assert!(!d.go);
            assert_eq!(d.reasons, vec!["the CI gate is not green"]);
        }
    }

    #[test]
    fn with_no_qa_angle_the_other_conditions_still_hold_the_go() {
        for facts in [
            GoFacts {
                blockers: vec!["ABC-7".into()],
                ..no_qa_angle()
            },
            GoFacts {
                tip_gaps: vec!["src/a.rs".into()],
                ..no_qa_angle()
            },
            GoFacts {
                policy: Policy::Human {
                    approved: Some(false),
                },
                ..no_qa_angle()
            },
        ] {
            let d = evaluate(&facts);
            assert!(!d.go, "{facts:?}");
            assert_eq!(d.reasons.len(), 1, "{:?}", d.reasons);
        }
    }

    #[test]
    fn a_failed_review_is_not_a_verdict() {
        let mut f = all_good();
        f.angles[0].1 = AngleState::FailedReview;
        assert!(!evaluate(&f).go);
    }

    #[test]
    fn a_blocking_issue_is_not_a_go_and_feeds_blockers_carried() {
        let d = evaluate(&GoFacts {
            blockers: vec!["ABC-7".into(), "ABC-7".into(), "ABC-9".into()],
            ..all_good()
        });
        assert_eq!(d.blockers_carried(), "ABC-7, ABC-9");
        assert_eq!(d.reasons, vec!["open blocking issues: ABC-7, ABC-9"]);
    }

    #[test]
    fn a_finding_marked_but_unlinked_keeps_the_go_false() {
        let d = evaluate(&GoFacts {
            unlinked_blockers: vec!["ABC-8".into()],
            ..all_good()
        });
        assert!(!d.go);
        assert_eq!(d.blockers, vec!["ABC-8"]);
        assert_eq!(d.blockers_carried(), "ABC-8");
    }

    #[test]
    fn a_fix_not_in_the_candidate_or_a_moved_tip_is_not_a_go() {
        assert!(
            !evaluate(&GoFacts {
                fix_gaps: vec!["ABC-3: not in rc".into()],
                ..all_good()
            })
            .go
        );
        let d = evaluate(&GoFacts {
            tip_gaps: vec!["src/a.rs".into()],
            ..all_good()
        });
        assert_eq!(
            d.reasons,
            vec!["the candidate is not the release branch tip: src/a.rs"]
        );
    }

    #[test]
    fn a_human_policy_adds_the_owners_approval_and_never_replaces_the_rest() {
        let human = |approved| Policy::Human { approved };
        assert!(
            !evaluate(&GoFacts {
                policy: human(Some(false)),
                ..all_good()
            })
            .go
        );
        assert!(
            evaluate(&GoFacts {
                policy: human(Some(true)),
                ..all_good()
            })
            .go
        );
        assert!(
            !evaluate(&GoFacts {
                policy: human(Some(true)),
                gate_green: None,
                ..all_good()
            })
            .go
        );
        let unread = evaluate(&GoFacts {
            policy: human(None),
            ..all_good()
        });
        assert!(
            !unread.go,
            "an approval that could not be read never passes"
        );
        assert_eq!(
            unread.reasons,
            vec!["the owner's approval could not be read (GO policy `human`)"]
        );
    }

    #[test]
    fn the_go_decision_serialises_for_the_deployment_guard() {
        let v = evaluate(&GoFacts {
            blockers: vec!["ABC-7".into()],
            ..all_good()
        })
        .as_json();
        assert_eq!(v["go"], false);
        assert_eq!(v["blockers_carried"], "ABC-7");
        assert_eq!(v["release"], "ABC-100");
    }
}
