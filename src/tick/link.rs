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

/// The linked items that can be the PR's ticket. Two rules drop the
/// bridge's own items: its marker (with a parent), and, since an agent
/// editing an item may mangle a marker the tracker shows as visible text,
/// its structure: an item whose parent is another of the linked items is a
/// sub-item of that item (an angle or consolidation item of its round), not
/// a second ticket.
fn real_items(tracker: &dyn Tracker, mut keys: Vec<String>) -> TResult<Vec<Item>> {
    keys.sort();
    keys.dedup();
    let mut items = Vec::new();
    for key in keys {
        match tracker.get(&key) {
            Ok(i) => items.push(i),
            // A URL in a body that names no item is no link.
            Err(e) if e.is_not_found() => continue,
            Err(e) => return Err(e),
        }
    }
    let linked: Vec<String> = items.iter().map(|i| i.key.clone()).collect();
    Ok(items
        .into_iter()
        .filter(|item| {
            texts::bridge_item(&item.description, item.parent.as_deref()).is_none()
                && !item.parent.as_ref().is_some_and(|p| linked.contains(p))
        })
        .collect())
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

    /// A ticket and one of its sub-items, both linked to the PR: the ticket
    /// alone is found, whatever the sub-item's description says.
    fn ticket_beside(title: &str, body: &str) {
        let t = FakeTracker::default();
        let pr = pull_request(7);
        t.seed("ABC-1", "a", "", Status::InReview, &[]);
        t.seed("ABC-2", title, body, Status::Ready, &[]);
        t.state.borrow_mut().set_parent("ABC-2", "ABC-1");
        t.link("ABC-1", &pr.url);
        t.link("ABC-2", &pr.url);
        assert_eq!(
            key(ticket_item(&pr, &t, LinkFallback::None).unwrap()),
            "ABC-1",
            "{title}"
        );
    }

    fn consolidation_body() -> String {
        let pr = pull_request(7);
        let scope = texts::round_scope(&pr.url, &pr.head_sha);
        texts::consolidation_item(&scope, "ABC-1", texts::Consolidating::PullRequest).1
    }

    #[test]
    fn the_bridges_own_items_linked_to_the_pr_are_not_tickets() {
        let pr = pull_request(7);
        let (_, angle) =
            texts::review_angle_item("ABC-1", &pr.url, &pr.head_sha, "security", None, &[]);
        ticket_beside("Security review", &angle);
        ticket_beside("Consolidation", &consolidation_body());
    }

    #[test]
    fn a_consolidation_item_whose_marker_an_agent_mangled_is_still_not_a_ticket() {
        let body = consolidation_body();
        // An agent ticking a box rewrote the marker the tracker shows as
        // visible text, or dropped it.
        let escaped = body.replace("spl-asdlc:", "spl-asdlc\\:");
        let dropped = body.lines().skip(1).collect::<Vec<_>>().join("\n");
        for mangled in [escaped, dropped] {
            assert_eq!(texts::bridge_item(&mangled, Some("ABC-1")), None);
            ticket_beside("Consolidation", &mangled);
        }
    }

    #[test]
    fn a_sub_item_of_a_ticket_not_linked_to_the_pr_is_the_ticket() {
        let t = FakeTracker::default();
        let pr = pull_request(7);
        t.seed("ABC-1", "a", "", Status::InProgress, &[]);
        t.seed("ABC-2", "b", "", Status::InProgress, &[]);
        t.state.borrow_mut().set_parent("ABC-2", "ABC-1");
        t.link("ABC-2", &pr.url);
        assert_eq!(
            key(ticket_item(&pr, &t, LinkFallback::None).unwrap()),
            "ABC-2"
        );
    }
}
