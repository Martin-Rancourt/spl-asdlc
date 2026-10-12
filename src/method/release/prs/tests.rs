//! Carried from `tests/test_release_advance.py` (identity by authorship).

use super::*;
use crate::github::Actor;
use crate::github::fake::pull_request;

fn bot(login: &str) -> Option<Actor> {
    Some(Actor {
        login: login.into(),
        is_bot: true,
    })
}

/// The login REST and the UI show for an App.
fn bot_login(slug: &str) -> String {
    format!("{slug}[bot]")
}

#[test]
fn a_configured_app_is_trusted_however_the_configuration_spells_it() {
    // The author as read: an App in the REST spelling. A list of Apps reads
    // a bare slug as the App too.
    let author = bot_login("ticketless-app");
    for configured in [
        bot_login("ticketless-app"),
        bot_login("TICKETLESS-APP"),
        "app/Ticketless-App".to_string(),
        "ticketless-app".to_string(),
    ] {
        let mut pr = pull_request(1);
        pr.author = bot(&author);
        assert!(
            trusted_author(&pr, std::slice::from_ref(&configured)),
            "configured {configured}, authored by {author}"
        );
        // `review.ticketless_authors` is matched the same way.
        pr.author = Some(Actor {
            login: "ticketless-app".into(),
            is_bot: false,
        });
        assert!(
            !trusted_author(&pr, &[configured]),
            "a human account never qualifies"
        );
    }
}

#[test]
fn angle_branches_parse_into_candidate_and_angle() {
    assert_eq!(
        parse_angle_branch("qa/acme-v1.2.3-rc.1/security"),
        Some(("acme-v1.2.3-rc.1".into(), "security".into()))
    );
    assert_eq!(parse_angle_branch("qa/security"), None);
    assert_eq!(parse_angle_branch("feat/qa/x/y"), None);
}

#[test]
fn an_angle_pr_names_its_sub_item_by_identifier() {
    let mut pr = pull_request(1);
    pr.body = "Closes ABC-12.".into();
    assert!(names_sub_item(&pr, "ABC-12"));
    assert!(!names_sub_item(&pr, "ABC-1"));
    pr.body = "ABC-123".into();
    assert!(
        !names_sub_item(&pr, "ABC-12"),
        "a longer identifier is not this one"
    );
}

// ---- BR-ASDLC-020 ----

fn facts(draft: bool, green: bool, sent_back: bool) -> AnglePrFacts {
    AnglePrFacts {
        draft,
        green,
        sent_back,
    }
}

#[test]
fn an_angle_item_follows_its_pr_through_the_normal_flow() {
    let draft = facts(true, false, false);
    let ready = facts(false, true, false);
    let red = facts(false, false, false);
    let sent_back = facts(false, true, true);
    assert_eq!(
        angle_item_status(Some(Status::Ready), draft),
        Some(Status::InProgress),
        "the PR appearing starts the work"
    );
    assert_eq!(
        angle_item_status(Some(Status::Ready), ready),
        Some(Status::InReview)
    );
    assert_eq!(
        angle_item_status(Some(Status::InProgress), ready),
        Some(Status::InReview)
    );
    assert_eq!(angle_item_status(Some(Status::InProgress), draft), None);
    assert_eq!(angle_item_status(Some(Status::InProgress), red), None);
    assert_eq!(
        angle_item_status(Some(Status::InReview), sent_back),
        Some(Status::InProgress)
    );
    assert_eq!(
        angle_item_status(Some(Status::InProgress), sent_back),
        None,
        "sent back, it stays with its angle until the head moves"
    );
    assert_eq!(
        angle_item_status(Some(Status::InReview), red),
        None,
        "red checks alone do not send it back"
    );
    for s in [
        Status::Triage,
        Status::Backlog,
        Status::ToDeploy,
        Status::Done,
        Status::Canceled,
    ] {
        assert_eq!(angle_item_status(Some(s), ready), None, "{s}");
    }
    assert_eq!(angle_item_status(None, ready), None);
}

/// Spec 09 (BR-ASDLC-021): the closure records who published the release,
/// the bridge on its GO or a person who tagged and released by hand.
#[test]
fn the_go_publisher_line_says_the_bridge_a_human_or_another_app() {
    let mut release = Release {
        tag: "app-v1.2.3".into(),
        url: "https://example.test/releases/app-v1.2.3".into(),
        body: String::new(),
        draft: false,
        prerelease: false,
        published_at: None,
        author: bot(&bot_login("bridge-app")),
    };
    let line = go_publisher_line(&release, &bot_login("bridge-app"));
    assert!(
        line.contains("published by the bridge, on its computed GO"),
        "{line}"
    );
    release.author = Some(Actor {
        login: "owner-login".into(),
        is_bot: false,
    });
    let line = go_publisher_line(&release, &bot_login("bridge-app"));
    assert!(
        line.contains("by `owner-login`, a human, in GitHub"),
        "{line}"
    );
    release.author = bot(&bot_login("other-app"));
    let line = go_publisher_line(&release, &bot_login("bridge-app"));
    assert!(line.contains("an App that is not the bridge"), "{line}");
    release.author = None;
    let line = go_publisher_line(&release, &bot_login("bridge-app"));
    assert!(line.contains("could not be read"), "{line}");
}
