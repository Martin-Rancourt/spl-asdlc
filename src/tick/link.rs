//! PR ↔ ticket (`docs/reference/01-ticket-contract.md` §7): a ticket is linked
//! to exactly one PR at a time and a PR to exactly one ticket. The link is
//! the tracker's own (an attachment or web link carrying the PR URL). When
//! the tracker has none, the PR names its ticket in one of two set places,
//! whatever the tracker (`docs/agents/process.md`, steps 2 and 5): its
//! branch, `<type>/<ticket-id>-<description>`, and the end of its title,
//! `… (ABC-12)`; ids compare without case. With `link_fallback = "body-url"`,
//! the tracker item URL written in the PR body names it too. Free text is
//! never parsed for a bare identifier, and an id no item of the project has
//! names nothing: the bridge never guesses.

use crate::config::{LinkFallback, Tracker as TrackerConfig};
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

/// The ticket of a PR, and whether the tracker holds the link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolution {
    pub linked: Linked,
    /// The tracker holds no link to the PR: its ticket was found by its id
    /// in the branch, the title or the body. The PR pass then lays the
    /// tracker's link, which a release reads to embark the ticket.
    pub unlinked: bool,
}

/// Resolve the ticket of a PR. The items the bridge created for the PR's
/// review (its angle sub-items and consolidation item, linked to the same
/// PR) are not tickets. A tracker read that fails is an error, never "no
/// ticket".
pub fn ticket_item(
    pr: &PullRequest,
    tracker: &dyn Tracker,
    config: &TrackerConfig,
) -> TResult<Linked> {
    resolve(pr, tracker, config).map(|r| r.linked)
}

/// [`ticket_item`], saying whether the tracker holds the link. The
/// tracker's own link wins; without it, the ids the branch and the title
/// name ([`named_ids`]) and, when declared, the item URLs of the body, all
/// together: two of them naming different tickets are several tickets.
pub fn resolve(
    pr: &PullRequest,
    tracker: &dyn Tracker,
    config: &TrackerConfig,
) -> TResult<Resolution> {
    let tickets = real_items(tracker, tracker.items_linked_to_pr(&pr.url)?)?;
    if !tickets.is_empty() {
        return Ok(Resolution {
            linked: linked(tickets),
            unlinked: false,
        });
    }
    let mut keys = Vec::new();
    if config.link_fallback == LinkFallback::BodyUrl {
        keys = tracker.keys_in_text(&pr.body);
    }
    if !id_shapes(&pr.head_ref, &pr.title).is_empty() {
        let mut prefixes = config.team_keys.clone();
        if prefixes.is_empty() {
            prefixes.push(tracker.team_key()?);
        }
        for id in named_ids(&pr.head_ref, &pr.title, &prefixes) {
            // The item must carry the very code named: a tracker that
            // resolved a code by its start would name another item.
            match tracker.get(&id) {
                Ok(item) if code(&item.key).eq_ignore_ascii_case(code(&id)) => keys.push(item.key),
                Ok(_) => {}
                Err(e) if e.is_not_found() => {}
                Err(e) => return Err(e),
            }
        }
    }
    Ok(Resolution {
        linked: linked(real_items(tracker, keys)?),
        unlinked: true,
    })
}

fn linked(mut tickets: Vec<Item>) -> Linked {
    match tickets.len() {
        0 => Linked::None,
        1 => Linked::One(Box::new(tickets.remove(0))),
        _ => Linked::Several(tickets.into_iter().map(|i| i.key).collect()),
    }
}

/// The code of an id (`ABC-12` → `12`).
fn code(id: &str) -> &str {
    id.rsplit_once('-').map_or(id, |(_, c)| c)
}

/// The `<prefix>-<code>` an id-shaped word starts with: letters and digits,
/// a dash, letters and digits, then a dash or the end.
fn id_shape(word: &str) -> Option<(&str, &str)> {
    let mut parts = word.splitn(3, '-');
    let prefix = parts.next()?;
    let code = parts.next()?;
    let alnum = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric());
    (alnum(prefix) && alnum(code)).then_some((prefix, code))
}

/// The id-shaped words of the branch's segments (the start, and after each
/// `/`) and of the title's last word, its brackets set aside.
fn id_shapes<'a>(branch: &'a str, title: &'a str) -> Vec<(&'a str, &'a str)> {
    let last = title
        .split_whitespace()
        .last()
        .unwrap_or_default()
        .trim_matches(['(', ')', '[', ']']);
    let mut out: Vec<_> = branch.split('/').filter_map(id_shape).collect();
    out.extend(id_shape(last).filter(|(_, c)| !c.contains('-')));
    out
}

/// The ticket ids a PR names in its branch and its title, upper case as
/// trackers write them: the branch `<type>/<ticket-id>-<description>`
/// (`fix/abc-12-short-description`: the id right after the start or a `/`,
/// then a dash or the end), and the title's last word (`fix: … (ABC-12)`).
/// Only ids whose prefix is one of the project's (`prefixes`), compared
/// without case: `feat/update-deps` names nothing.
pub fn named_ids(branch: &str, title: &str, prefixes: &[String]) -> Vec<String> {
    let mut ids: Vec<String> = id_shapes(branch, title)
        .into_iter()
        .filter(|(p, _)| prefixes.iter().any(|k| k.eq_ignore_ascii_case(p)))
        .map(|(p, c)| format!("{p}-{c}").to_ascii_uppercase())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// The linked items that can be the PR's ticket. Two rules drop the
/// bridge's own items: its marker (with a parent), and, since an agent
/// editing an item may mangle a marker the tracker shows as visible text,
/// its structure: an item whose parent is another of the linked items is a
/// sub-item of that item (an angle or consolidation item of its round), not
/// a second ticket.
fn real_items(tracker: &dyn Tracker, keys: Vec<String>) -> TResult<Vec<Item>> {
    Ok(split_linked(tracker, keys)?.tickets)
}

/// The items linked to one PR, split into tickets and the bridge's own
/// sub-items.
#[derive(Debug, Default)]
pub struct LinkedItems {
    pub tickets: Vec<Item>,
    /// The angle and consolidation items of the PR's review rounds.
    pub sub_items: Vec<Item>,
}

/// Read the items linked to one PR (`keys`) and split them by the rules of
/// [`real_items`]: an item with a bridge marker, or whose parent is another
/// of the linked items, is a sub-item of the bridge, never a ticket. A key
/// that names no item is dropped; any other failed read is an error.
pub fn split_linked(tracker: &dyn Tracker, mut keys: Vec<String>) -> TResult<LinkedItems> {
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
    let (sub_items, tickets) = items.into_iter().partition(|item| {
        texts::bridge_item(&item.description, item.parent.as_deref()).is_some()
            || item.parent.as_ref().is_some_and(|p| linked.contains(p))
    });
    Ok(LinkedItems { tickets, sub_items })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::fake::{METHOD_AT as AT, pull_request};
    use crate::method::texts::protocol;
    use crate::method::vocabulary::Status;
    use crate::tracker::fake::FakeTracker;

    /// The example's tracker table with this fallback and the team's own
    /// prefix (`team_keys` empty: the tracker's team key, `ABC`).
    fn cfg(fallback: LinkFallback) -> TrackerConfig {
        let mut c = crate::config::spec_example::config().tracker;
        c.link_fallback = fallback;
        c.team_keys.clear();
        c
    }

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
            ticket_item(&pr, &t, &cfg(LinkFallback::None)).unwrap(),
            Linked::None,
            "the body is not a link unless the configuration says so"
        );
        assert_eq!(
            key(ticket_item(&pr, &t, &cfg(LinkFallback::BodyUrl)).unwrap()),
            "ABC-2"
        );
        t.link("ABC-1", &pr.url);
        assert_eq!(
            key(ticket_item(&pr, &t, &cfg(LinkFallback::BodyUrl)).unwrap()),
            "ABC-1",
            "the tracker's own link wins"
        );
        t.link("ABC-2", &pr.url);
        assert_eq!(
            ticket_item(&pr, &t, &cfg(LinkFallback::None)).unwrap(),
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
            key(ticket_item(&pr, &t, &cfg(LinkFallback::None)).unwrap()),
            "ABC-1",
            "{title}"
        );
    }

    fn consolidation_body() -> String {
        let pr = pull_request(7);
        let scope = texts::round_scope(&pr.url, &pr.head_sha);
        texts::consolidation_item(
            &scope,
            "ABC-1",
            texts::Consolidating::PullRequest,
            &protocol::Texts::of_this_build(AT, texts::Consolidating::PullRequest.job(), None),
        )
        .1
    }

    #[test]
    fn the_bridges_own_items_linked_to_the_pr_are_not_tickets() {
        let pr = pull_request(7);
        let (_, angle) = texts::review_angle_item(
            "ABC-1",
            &pr.url,
            &pr.head_sha,
            "security",
            None,
            &[],
            &protocol::Texts::of_this_build(AT, protocol::Job::PrAngle, Some("security")),
        );
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
            key(ticket_item(&pr, &t, &cfg(LinkFallback::None)).unwrap()),
            "ABC-2"
        );
    }

    fn named(branch: &str, title: &str) -> Linked {
        let t = FakeTracker::new("LVL");
        t.seed("LVL-7GTC", "the ticket", "", Status::InProgress, &[]);
        t.seed("LVL-7GTD", "another", "", Status::InProgress, &[]);
        let mut pr = pull_request(160);
        pr.head_ref = branch.into();
        pr.title = title.into();
        ticket_item(&pr, &t, &cfg(LinkFallback::None)).unwrap()
    }

    #[test]
    fn the_branch_and_the_end_of_the_title_name_the_ticket_without_regard_to_case() {
        assert_eq!(
            key(named(
                "fix/lvl-7gtc-mcp-asdlc-settings",
                "fix(mcp): ASDLC settings (LVL-7GTC)"
            )),
            "LVL-7GTC"
        );
        assert_eq!(
            key(named(
                "fix/lvl-7gtc-mcp-asdlc-settings",
                "fix(mcp): ASDLC settings"
            )),
            "LVL-7GTC",
            "the branch alone"
        );
        assert_eq!(
            key(named("ben/LVL-7gtc", "fix(mcp): ASDLC settings")),
            "LVL-7GTC",
            "the id ends the branch, any case"
        );
        assert_eq!(
            key(named("patch-1", "fix(mcp): ASDLC settings (lvl-7gtc)")),
            "LVL-7GTC",
            "the title alone"
        );
    }

    #[test]
    fn an_unknown_or_foreign_id_names_nothing_and_nothing_is_guessed() {
        for (branch, title) in [
            // No item has this code: never the nearest one.
            ("fix/lvl-7gtx-mcp", "fix(mcp): settings (LVL-7GTX)"),
            // A code that only starts an item's code.
            ("fix/lvl-7gt-mcp", "fix(mcp): settings"),
            // Another project's prefix, and words shaped like an id.
            ("fix/abc-7gtc-mcp", "fix(mcp): settings (ABC-7GTC)"),
            ("feat/update-deps", "chore(deps): bump serde-json"),
            // An id mentioned in passing is not the end of the title.
            ("feat/new-screen", "feat: LVL-7GTC follow-up screen"),
        ] {
            assert_eq!(named(branch, title), Linked::None, "{branch} / {title}");
        }
    }

    #[test]
    fn a_branch_and_a_title_naming_two_tickets_are_several() {
        assert_eq!(
            named("fix/lvl-7gtc-mcp", "fix(mcp): settings (LVL-7GTD)"),
            Linked::Several(vec!["LVL-7GTC".into(), "LVL-7GTD".into()])
        );
    }

    #[test]
    fn the_tracker_link_wins_over_the_branch_and_says_it_is_linked() {
        let t = FakeTracker::new("LVL");
        t.seed("LVL-7GTC", "a", "", Status::InProgress, &[]);
        t.seed("LVL-7GTD", "b", "", Status::InProgress, &[]);
        let mut pr = pull_request(160);
        pr.head_ref = "fix/lvl-7gtc-mcp".into();
        let named = resolve(&pr, &t, &cfg(LinkFallback::None)).unwrap();
        assert!(named.unlinked, "found by the branch, not linked yet");
        t.link("LVL-7GTD", &pr.url);
        let r = resolve(&pr, &t, &cfg(LinkFallback::None)).unwrap();
        assert_eq!(key(r.linked), "LVL-7GTD");
        assert!(!r.unlinked);
    }

    #[test]
    fn named_ids_keep_the_projects_prefixes_and_write_ids_upper_case() {
        let prefixes = ["ABC".to_string(), "OLD".to_string()];
        assert_eq!(
            named_ids("feat/abc-12-x", "feat: x (old-7)", &prefixes),
            vec!["ABC-12".to_string(), "OLD-7".to_string()]
        );
        assert_eq!(named_ids("abc-12", "[ABC-12]", &prefixes), vec!["ABC-12"]);
        assert!(named_ids("release/2026-10-25", "chore: release 1.2.3", &prefixes).is_empty());
    }
}
