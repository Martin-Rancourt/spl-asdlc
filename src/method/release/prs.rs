//! The PRs of the release cycle (REQ-REL-049, 077): angle PRs, the Apps
//! trusted by authorship, and who published a release.

use crate::github::{PullRequest, Release, same_app, same_login};
use crate::method::vocabulary::Status;

/// Authored by a configured App (a list of Apps: `<slug>[bot]`,
/// `app/<slug>` and `<slug>` name the same one, [`same_app`]), and not a
/// human account whose login merely resembles it.
pub fn trusted_author(pr: &PullRequest, authors: &[String]) -> bool {
    match &pr.author {
        Some(a) => a.is_bot && authors.iter().any(|x| same_app(x, &a.login)),
        None => false,
    }
}

/// Who published the release, as the closure records it (spec 09): the
/// bridge on its computed GO, or a person in GitHub (the release tag and
/// the release made by hand), which the publication accepts as well.
/// `bridge_login` is the bridge's own GitHub login.
pub fn go_publisher_line(release: &Release, bridge_login: &str) -> String {
    match &release.author {
        Some(a) if same_login(bridge_login, &a.login) => format!(
            "GO: the release {} was published by the bridge, on its computed GO.",
            release.url
        ),
        Some(a) if !a.is_bot => format!(
            "GO: the release {} was published by `{}`, a human, in GitHub (spec 09); the publication accepted it as the GO.",
            release.url, a.login
        ),
        Some(a) => format!(
            "GO: the release {} was published by `{}`, an App that is not the bridge.",
            release.url, a.login
        ),
        None => format!(
            "GO: the release {} was published; who published it could not be read.",
            release.url
        ),
    }
}

/// The `(candidate, angle)` of an angle branch `qa/<candidate>/<angle>`.
pub fn parse_angle_branch(head: &str) -> Option<(String, String)> {
    let rest = head.strip_prefix("qa/")?;
    let (candidate, angle) = rest.rsplit_once('/')?;
    (!candidate.is_empty() && !angle.is_empty() && !angle.contains('/'))
        .then(|| (candidate.to_string(), angle.to_string()))
}

/// The sub-item identifier an angle PR names, among the expected ones, read
/// from its title then its body; free text is matched only against the
/// identifiers the bridge itself created.
pub fn names_sub_item(pr: &PullRequest, sub_item: &str) -> bool {
    let token = |text: &str| {
        text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
            .any(|w| w.eq_ignore_ascii_case(sub_item))
    };
    token(&pr.title) || token(&pr.body)
}

/// What the bridge reads of an open angle PR to set its sub-item's status
/// (BR-ASDLC-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnglePrFacts {
    /// Its required checks are green at its head.
    pub green: bool,
    /// A configured coordinator's request for changes stands at its head.
    pub sent_back: bool,
}

/// The status a release angle sub-item goes to for its open angle PR
/// (spec 03, `05-release-cycle.md` §4): a bridge item, not a ticket, whose
/// work starts when the PR is opened (Ready → In Progress; a draft is out of
/// the flow and drives nothing); the PR with its required checks green and
/// not sent back is In Review; a coordinator's
/// request for changes at its head sends it back In Progress. `None` when it
/// stays: a status the flow does not drive (Triage, Backlog, a resolved
/// item) is never touched, and Done comes only from the merge.
pub fn angle_item_status(current: Option<Status>, pr: AnglePrFacts) -> Option<Status> {
    let ready = pr.green && !pr.sent_back;
    let to = match current? {
        Status::Ready if ready => Status::InReview,
        Status::Ready => Status::InProgress,
        Status::InProgress if ready => Status::InReview,
        Status::InReview if pr.sent_back => Status::InProgress,
        _ => return None,
    };
    Some(to)
}

#[cfg(test)]
mod tests;
