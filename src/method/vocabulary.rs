//! Canonical names (`docs/spec/03-vocabulary.md`). A tracker names statuses
//! exactly so; an adapter may map backend-specific IDs internally. The method
//! speaks only these.

use std::fmt;

/// Canonical status of a work item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    Triage,
    Backlog,
    Ready,
    InProgress,
    InReview,
    ToDeploy,
    PostDeploy,
    Done,
    Canceled,
}

impl Status {
    pub const ALL: [Status; 9] = [
        Status::Triage,
        Status::Backlog,
        Status::Ready,
        Status::InProgress,
        Status::InReview,
        Status::ToDeploy,
        Status::PostDeploy,
        Status::Done,
        Status::Canceled,
    ];

    /// The canonical name, as the tracker and the bridge write it.
    pub fn name(self) -> &'static str {
        match self {
            Status::Triage => "Triage",
            Status::Backlog => "Backlog",
            Status::Ready => "Ready",
            Status::InProgress => "In Progress",
            Status::InReview => "In Review",
            Status::ToDeploy => "To Deploy",
            Status::PostDeploy => "Post Deploy",
            Status::Done => "Done",
            Status::Canceled => "Canceled",
        }
    }

    pub fn from_name(name: &str) -> Option<Status> {
        Status::ALL.into_iter().find(|s| s.name() == name)
    }

    /// Statuses past the merge: a PR still open on such a ticket means the
    /// board is wrong (REQ-MERGE-028). Post Deploy is not reopened.
    pub fn is_terminal_for_open_pr(self) -> bool {
        matches!(self, Status::ToDeploy | Status::Done)
    }

    /// Resolved: nothing will move it again on its own.
    pub fn is_resolved(self) -> bool {
        matches!(self, Status::Done | Status::Canceled)
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The state of an item as a tracker reports it: a canonical status when the
/// mapping knows it, and the tracker's own kind of state. A state nobody can
/// place counts as open everywhere (waiting costs a delay; shipping costs an
/// incident).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemState {
    /// The tracker's own name.
    pub name: String,
    pub status: Option<Status>,
    pub kind: StateKind,
}

impl ItemState {
    pub fn of(status: Status) -> ItemState {
        ItemState {
            name: status.name().to_string(),
            status: Some(status),
            kind: StateKind::of(status),
        }
    }

    /// Done or Canceled. The validation statuses (In Review, To Deploy,
    /// Post Deploy) are open: they still block their dependents, whatever
    /// category the tracker files them under. A state outside the status
    /// table is read by its kind; an unknown kind is open.
    pub fn is_resolved(&self) -> bool {
        match self.status {
            Some(status) => status.is_resolved(),
            None => matches!(self.kind, StateKind::Completed | StateKind::Canceled),
        }
    }
}

/// The kind of a state, as trackers group them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateKind {
    Triage,
    Backlog,
    Unstarted,
    Started,
    Completed,
    Canceled,
    Unknown,
}

impl StateKind {
    pub fn of(status: Status) -> StateKind {
        match status {
            Status::Triage => StateKind::Triage,
            Status::Backlog => StateKind::Backlog,
            Status::Ready => StateKind::Unstarted,
            Status::InProgress | Status::InReview => StateKind::Started,
            Status::ToDeploy | Status::PostDeploy | Status::Done => StateKind::Completed,
            Status::Canceled => StateKind::Canceled,
        }
    }
}

/// The verdict of a release angle report (`docs/spec/06-qa-report.md`):
/// `hold` when at least one finding is blocking, `go` otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Go,
    Hold,
}

impl Verdict {
    pub fn name(self) -> &'static str {
        match self {
            Verdict::Go => "go",
            Verdict::Hold => "hold",
        }
    }

    pub fn from_name(name: &str) -> Option<Verdict> {
        match name {
            "go" => Some(Verdict::Go),
            "hold" => Some(Verdict::Hold),
            _ => None,
        }
    }
}

/// Label on finding tickets.
pub const QA_FINDING_LABEL: &str = "qa-finding";
/// Label a PR author adds to ask for a review whatever the paths say.
pub const NEEDS_REVIEW_LABEL: &str = "needs-review";
/// The tickets the bridge opens for PRs of `review.ticketless_authors`.
pub const TICKETLESS_LABEL: &str = "ticketless-pr";
/// The label on a ticket waiting in Post Deploy for its validation, per
/// product: two bridges sharing a tracker team never take each other's.
pub fn post_deploy_label(product: &str) -> String {
    format!("post-deploy-validation/{product}")
}

/// `release/<product>-v<version>`, the version normalised to `vX.Y.Z`.
pub fn release_label(product: &str, version: &str) -> String {
    format!("release/{product}-{}", normalise_version(version))
}

/// `0.4.90` and `v0.4.90` are the same version: `v0.4.90`.
pub fn normalise_version(version: &str) -> String {
    format!("v{}", version.trim().trim_start_matches(['v', 'V']))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_names_round_trip_and_match_the_configuration() {
        for s in Status::ALL {
            assert_eq!(Status::from_name(s.name()), Some(s));
            assert!(crate::config::CANONICAL_STATUSES.contains(&s.name()));
        }
        assert_eq!(Status::from_name("To deploy"), None, "names are exact");
    }

    #[test]
    fn the_label_is_the_same_whichever_way_the_version_is_spelled() {
        assert_eq!(release_label("acme", "0.4.90"), "release/acme-v0.4.90");
        assert_eq!(release_label("acme", "v0.4.90"), "release/acme-v0.4.90");
    }

    #[test]
    fn an_unknown_state_kind_counts_as_open() {
        let unknown = ItemState {
            name: "Parked".into(),
            status: None,
            kind: StateKind::Unknown,
        };
        assert!(!unknown.is_resolved());
        assert!(!ItemState::of(Status::InReview).is_resolved());
        assert!(ItemState::of(Status::Done).is_resolved());
        assert!(ItemState::of(Status::Canceled).is_resolved());
    }

    #[test]
    fn the_validation_statuses_are_open() {
        // Livela's `validation` group (2026-10-05): open, still blocking,
        // even where the tracker files To Deploy as a completed state.
        for s in [Status::InReview, Status::ToDeploy, Status::PostDeploy] {
            assert!(!ItemState::of(s).is_resolved(), "{s}");
        }
        let shipped = ItemState {
            name: "Shipped".into(),
            status: None,
            kind: StateKind::Completed,
        };
        assert!(shipped.is_resolved(), "outside the table, the kind decides");
    }

    #[test]
    fn post_deploy_stays_outside_the_in_progress_group() {
        // BR-ASDLC-003: an implementer is served the unstarted and started
        // kinds (Ready, In Progress); Post Deploy is neither, its
        // verification is served through the validation sub-task, in Ready.
        let kind = StateKind::of(Status::PostDeploy);
        assert!(!matches!(kind, StateKind::Unstarted | StateKind::Started));
        assert_eq!(StateKind::of(Status::Ready), StateKind::Unstarted);
    }

    #[test]
    fn post_deploy_is_not_reopened_by_an_open_pr() {
        assert!(Status::Done.is_terminal_for_open_pr());
        assert!(Status::ToDeploy.is_terminal_for_open_pr());
        assert!(!Status::PostDeploy.is_terminal_for_open_pr());
    }
}
