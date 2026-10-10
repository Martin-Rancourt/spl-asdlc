//! Carried from `tests/test_release_advance.py` (release PR identity) and
//! `tests/test_release_advance_cycle.py` (components, ledger).

use super::*;
use crate::config::Config;
use crate::github::Actor;
use crate::github::fake::pull_request;

const BASE: &str = "release/2026-10-05-14-03-utc";
const LABEL: &str = "autorelease: pending";

fn authors() -> Vec<String> {
    vec!["app/release-app".to_string()]
}

fn bot(login: &str) -> Option<Actor> {
    Some(Actor {
        login: login.into(),
        is_bot: true,
    })
}

fn release_pr(number: u64) -> PullRequest {
    let mut pr = pull_request(number);
    pr.base_ref = BASE.into();
    pr.head_ref = format!("release-please--branches--{BASE}");
    pr.title = "chore(release): release acme 1.2.3".into();
    pr.labels = vec![LABEL.into()];
    pr.author = bot(&format!("{}[bot]", "release-app"));
    pr
}

fn shape() -> Shape {
    let text = "schema = 1\n[product]\nname = \"acme\"\nrepository = \"example-org/acme\"\n[owner]\ntracker_user = \"o\"\n\
        [tracker]\nkind = \"linear\"\nteam = \"ABC\"\nservice_identity = \"s\"\n\
        [release]\ncandidate_tag = \"v{version}-rc.{rc}\"\nrelease_tag = \"app-v{version}\"\ncomponents = [\"app\", \"website\", \"admin\"]\nversion_component = \"app\"\ngate_record = \"docs/releases/v{version}.md\"\nrelease_please_authors = [\"app/release-app\"]\ncoordinators = [\"c\"]\n[[release.angles]]\nkey = \"security\"\n\
        [review]\ncoordinators = [\"c\"]\ndiff_size_extra_angle = 0\n[[review.angles]]\nkey = \"m\"\n";
    let c = Config::parse(text).unwrap();
    Shape::of(&c.product.name, c.release.as_ref().unwrap())
}

#[test]
fn the_service_identity_is_merged_in_either_spelling() {
    let prs = [release_pr(1)];
    assert_eq!(
        release_please_pr(&prs, BASE, &authors(), LABEL)
            .unwrap()
            .pr
            .map(|p| p.number),
        Some(1)
    );
    let mut app_form = release_pr(2);
    app_form.author = bot("app/release-app");
    let prs = [app_form];
    assert!(
        release_please_pr(&prs, BASE, &authors(), LABEL)
            .unwrap()
            .pr
            .is_some()
    );
}

#[test]
fn a_release_pr_of_another_author_is_never_merged() {
    let mut other = release_pr(1);
    other.author = bot(&format!("{}[bot]", "former-release-app"));
    let prs = [other];
    let lookup = release_please_pr(&prs, BASE, &authors(), LABEL).unwrap();
    assert!(lookup.pr.is_none());
    assert!(lookup.shape_seen, "the shape is seen even when refused");
    assert_eq!(lookup.refused, vec![1]);
}

/// The login REST and the UI show for an App.
fn bot_login(slug: &str) -> String {
    format!("{slug}[bot]")
}

#[test]
fn a_configured_app_is_trusted_however_the_configuration_spells_it() {
    // The author as read: an App in the REST spelling. A list of Apps reads
    // a bare slug as the App too.
    let author = bot_login("release-app");
    for configured in [
        bot_login("release-app"),
        bot_login("RELEASE-APP"),
        "app/Release-App".to_string(),
        "release-app".to_string(),
    ] {
        let mut pr = release_pr(1);
        pr.author = bot(&author);
        assert!(
            trusted_author(&pr, std::slice::from_ref(&configured)),
            "configured {configured}, authored by {author}"
        );
        // `review.ticketless_authors` is matched the same way.
        pr.author = Some(Actor {
            login: "release-app".into(),
            is_bot: false,
        });
        assert!(
            !trusted_author(&pr, &[configured]),
            "a human account never qualifies"
        );
    }
}

#[test]
fn a_human_lookalike_is_refused() {
    let mut human = release_pr(1);
    human.author = Some(Actor {
        login: "release-app".into(),
        is_bot: false,
    });
    let prs = [human];
    assert!(
        release_please_pr(&prs, BASE, &authors(), LABEL)
            .unwrap()
            .pr
            .is_none()
    );
}

#[test]
fn a_stale_pr_against_main_is_never_merged() {
    let mut stale = release_pr(1);
    stale.base_ref = "main".into();
    let prs = [stale];
    let lookup = release_please_pr(&prs, BASE, &authors(), LABEL).unwrap();
    assert!(lookup.pr.is_none());
    assert!(!lookup.shape_seen);
}

#[test]
fn the_release_pr_is_never_looked_for_without_a_base() {
    assert!(release_please_pr(&[release_pr(1)], "", &authors(), LABEL).is_err());
}

#[test]
fn the_label_alone_identifies_it() {
    let mut labelled = release_pr(1);
    labelled.head_ref = "some-branch".into();
    labelled.title = "whatever".into();
    let prs = [labelled];
    assert!(
        release_please_pr(&prs, BASE, &authors(), LABEL)
            .unwrap()
            .pr
            .is_some()
    );
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

fn component(number: u64, title: &str, head: &str) -> PullRequest {
    let mut pr = release_pr(number);
    pr.title = title.into();
    pr.head_ref = head.into();
    pr
}

#[test]
fn the_other_components_merge_before_the_version_component() {
    let prs = [
        component(
            10,
            "chore: release app 1.36.3",
            "release-please--branches--x--components--app",
        ),
        component(
            11,
            "chore: release admin 1.4.0",
            "release-please--branches--x--components--admin",
        ),
        component(
            12,
            "chore: release website 2.0.1",
            "release-please--branches--x--components--website",
        ),
    ];
    let order = component_prs(&prs, &shape(), BASE, &authors(), LABEL).unwrap();
    assert_eq!(
        order
            .iter()
            .map(|c| c.component.as_str())
            .collect::<Vec<_>>(),
        vec!["website", "admin", "app"]
    );
    assert_eq!(order[2].version, Version::parse("1.36.3").unwrap());
}

#[test]
fn an_undeclared_component_refuses_the_whole_cycle() {
    let prs = [component(10, "chore: release mobile 1.0.0", "x")];
    let err = component_prs(&prs, &shape(), BASE, &authors(), LABEL).unwrap_err();
    assert_eq!(err, "#10 releases mobile, which acme does not declare");
}

#[test]
fn a_title_and_branch_of_two_components_refuse_the_cycle() {
    let prs = [component(
        10,
        "chore: release app 1.0.0",
        "release-please--branches--x--components--website",
    )];
    assert!(
        component_prs(&prs, &shape(), BASE, &authors(), LABEL)
            .unwrap_err()
            .contains("titled for app")
    );
}

#[test]
fn two_release_prs_for_one_component_refuse_the_cycle() {
    let prs = [
        component(10, "chore: release app 1.0.0", "a"),
        component(11, "chore: release app 1.0.1", "b"),
    ];
    assert_eq!(
        component_prs(&prs, &shape(), BASE, &authors(), LABEL).unwrap_err(),
        "two release PRs are open for app"
    );
}

#[test]
fn a_shaped_pr_without_a_readable_title_refuses_the_cycle() {
    let prs = [component(
        10,
        "chore(release): release main",
        "release-please--x",
    )];
    assert!(
        component_prs(&prs, &shape(), BASE, &authors(), LABEL)
            .unwrap_err()
            .contains("no readable component")
    );
}

#[test]
fn a_release_pr_from_another_author_is_not_a_release_pr() {
    let mut pr = component(10, "chore: release app 1.0.0", "a");
    pr.author = bot("someone-else");
    let prs = [pr];
    assert!(
        component_prs(&prs, &shape(), BASE, &authors(), LABEL)
            .unwrap()
            .is_empty()
    );
}

fn merged(number: u64, title: &str, labels: &[&str]) -> PullRequest {
    let mut pr = pull_request(number);
    pr.title = title.into();
    pr.labels = labels.iter().map(|s| s.to_string()).collect();
    pr
}

#[test]
fn the_ledger_reads_embarked_components_and_their_deployment() {
    let prs = [
        merged(
            12,
            "chore: release website 2.0.1",
            &["autorelease: tagged", "deployed: production"],
        ),
        merged(10, "chore: release app 1.36.3", &["autorelease: tagged"]),
        merged(9, "fix: something", &[]),
    ];
    let ledger = ledger(&prs, &shape()).unwrap();
    assert_eq!(ledger.len(), 2);
    assert_eq!(
        ledger[0],
        LedgerEntry {
            component: "app".into(),
            version: "1.36.3".into(),
            pr: 10,
            deployed: false
        }
    );
    assert!(ledger[1].deployed);
}

#[test]
fn a_component_not_embarked_does_not_count() {
    let prs = [merged(
        10,
        "chore: release mobile 1.0.0",
        &["autorelease: tagged"],
    )];
    assert_eq!(
        ledger(&prs, &shape()),
        None,
        "nothing embarked is 'I do not know', never an empty ledger"
    );
}

#[test]
fn two_trusted_release_prs_refuse_the_merge() {
    let prs = [release_pr(1), release_pr(2)];
    assert!(release_please_pr(&prs, BASE, &authors(), LABEL).is_err());
}
