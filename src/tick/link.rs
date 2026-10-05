//! PR ↔ ticket (`docs/spec/01-ticket-contract.md` §7): a ticket is linked
//! to exactly one PR at a time and a PR to exactly one ticket. The link is
//! the tracker's own (an attachment or web link carrying the PR URL), or,
//! when the configuration declares `link_fallback = "body-url"`, the
//! tracker item URL written in the PR body. Free text is never parsed for a
//! bare identifier.

use crate::config::LinkFallback;
use crate::github::PullRequest;
use crate::method::texts;
use crate::tracker::{Item, TResult, Tracker};

/// The ticket of a PR, as far as the facts say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Linked {
    /// No ticket names this PR.
    None,
    One(Box<Item>),
    /// Several tickets claim the PR: the link is ambiguous, nothing is
    /// decided on it.
    Several(Vec<String>),
}

/// Resolve the ticket of a PR. The items the bridge created for the PR's
/// review (its angle sub-items and consolidation item, linked to the same
/// PR) are not tickets. A tracker read that fails is an error, never "no
/// ticket".
pub fn ticket_item(
    pr: &PullRequest,
    tracker: &dyn Tracker,
    fallback: LinkFallback,
) -> TResult<Linked> {
    let mut tickets = real_items(tracker, tracker.items_linked_to_pr(&pr.url)?)?;
    if tickets.is_empty() && fallback == LinkFallback::BodyUrl {
        tickets = real_items(tracker, tracker.keys_in_text(&pr.body))?;
    }
    Ok(match tickets.len() {
        0 => Linked::None,
        1 => Linked::One(Box::new(tickets.remove(0))),
        _ => Linked::Several(tickets.into_iter().map(|i| i.key).collect()),
    })
}

fn real_items(tracker: &dyn Tracker, mut keys: Vec<String>) -> TResult<Vec<Item>> {
    keys.sort();
    keys.dedup();
    let mut out = Vec::new();
    for key in keys {
        let item = match tracker.get(&key) {
            Ok(i) => i,
            // A URL in a body that names no item is no link.
            Err(e) if e.is_not_found() => continue,
            Err(e) => return Err(e),
        };
        if texts::bridge_item(&item.description, item.parent.as_deref()).is_none() {
            out.push(item);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::fake::pull_request;
    use crate::method::vocabulary::Status;
    use crate::tracker::fake::FakeTracker;

    fn key(l: Linked) -> String {
        match l {
            Linked::One(i) => i.key,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_tracker_link_wins_and_the_body_is_read_only_when_declared() {
        let t = FakeTracker::default();
        t.seed("ABC-1", "a", "", Status::InProgress, &[]);
        t.seed("ABC-2", "b", "", Status::InProgress, &[]);
        let mut pr = pull_request(7);
        pr.body = "Implements https://tracker.test/ABC-2 and https://tracker.test/ABC-404".into();
        assert_eq!(
            ticket_item(&pr, &t, LinkFallback::None).unwrap(),
            Linked::None,
            "the body is not a link unless the configuration says so"
        );
        assert_eq!(
            key(ticket_item(&pr, &t, LinkFallback::BodyUrl).unwrap()),
            "ABC-2"
        );
        t.link("ABC-1", &pr.url);
        assert_eq!(
            key(ticket_item(&pr, &t, LinkFallback::BodyUrl).unwrap()),
            "ABC-1",
            "the tracker's own link wins"
        );
        t.link("ABC-2", &pr.url);
        assert_eq!(
            ticket_item(&pr, &t, LinkFallback::None).unwrap(),
            Linked::Several(vec!["ABC-1".into(), "ABC-2".into()])
        );
    }

    #[test]
    fn the_bridges_own_items_linked_to_the_pr_are_not_tickets() {
        let t = FakeTracker::default();
        let pr = pull_request(7);
        t.seed("ABC-1", "a", "", Status::InReview, &[]);
        let (_, body) = texts::review_angle_item("ABC-1", &pr.url, &pr.head_sha, "security", &[]);
        t.seed("ABC-2", "Security review", &body, Status::Ready, &[]);
        t.state.borrow_mut().set_parent("ABC-2", "ABC-1");
        t.link("ABC-1", &pr.url);
        t.link("ABC-2", &pr.url);
        assert_eq!(
            key(ticket_item(&pr, &t, LinkFallback::None).unwrap()),
            "ABC-1"
        );
    }
}
