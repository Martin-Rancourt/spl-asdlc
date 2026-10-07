//! The Linear adapter against scripted GraphQL answers.

use super::*;
use crate::http::ScriptedTransport;
use std::sync::Arc;

const URL: &str = "https://linear.test/graphql";

struct Shared(Arc<ScriptedTransport>);
impl Transport for Shared {
    fn send(&self, r: HttpRequest) -> Result<crate::http::HttpResponse, String> {
        self.0.send(r)
    }
}

fn linear() -> (Linear, Arc<ScriptedTransport>) {
    let t = Arc::new(ScriptedTransport::new());
    let mut map = BTreeMap::new();
    map.insert("To Deploy".to_string(), "To deploy".to_string());
    let l = Linear::with_transport(
        "lin-test-key",
        "ABC",
        StatusTable::from_config(&map),
        Box::new(Shared(t.clone())),
        URL,
    );
    (l, t)
}

fn issue(key: &str, state: &str, kind: &str) -> String {
    format!(
        r#"{{"id":"id-{key}","identifier":"{key}","title":"T","description":"- [ ] a","url":"https://linear.test/{key}","archivedAt":null,
            "state":{{"name":"{state}","type":"{kind}"}},"labels":{{"nodes":[{{"name":"qa-finding"}}],"pageInfo":{{"hasNextPage":false}}}},
            "assignee":{{"id":"u1","name":"n","displayName":"Dev"}},"parent":{{"identifier":"ABC-1"}}}}"#
    )
}

fn team(t: &ScriptedTransport) {
    t.on_body("POST", URL, "teams(filter", 200, r#"{"data":{"teams":{"nodes":[{"id":"team-1","key":"ABC","states":{"nodes":[
        {"id":"s-ready","name":"Ready","type":"unstarted"},{"id":"s-review","name":"In Review","type":"started"},
        {"id":"s-deploy","name":"To deploy","type":"completed"},{"id":"s-done","name":"Done","type":"completed"}]}}]}}}"#);
}

#[test]
fn an_item_is_read_with_its_canonical_status_and_kind() {
    let (l, t) = linear();
    t.on_body(
        "POST",
        URL,
        "issue(id: $id) { id identifier",
        200,
        &format!(
            r#"{{"data":{{"issue":{}}}}}"#,
            issue("ABC-2", "To deploy", "completed")
        ),
    );
    let item = l.get("ABC-2").unwrap();
    assert_eq!(item.state.status, Some(Status::ToDeploy));
    assert_eq!(item.state.kind, StateKind::Completed);
    assert_eq!(item.parent.as_deref(), Some("ABC-1"));
    assert_eq!(item.assignee.unwrap().name, "Dev");
    let auth = t.log.lock().unwrap()[0]
        .headers
        .iter()
        .find(|(k, _)| k == "Authorization")
        .cloned();
    assert_eq!(auth.unwrap().1, "lin-test-key");
}

#[test]
fn a_state_the_table_does_not_name_is_kept_with_its_kind() {
    let (l, t) = linear();
    t.on_body(
        "POST",
        URL,
        "issue(id: $id) { id identifier",
        200,
        &format!(
            r#"{{"data":{{"issue":{}}}}}"#,
            issue("ABC-2", "Parked", "weird")
        ),
    );
    let item = l.get("ABC-2").unwrap();
    assert_eq!(item.state.status, None);
    assert_eq!(item.state.kind, StateKind::Unknown);
    assert!(!item.state.is_resolved(), "an unknown state counts as open");
}

#[test]
fn a_missing_item_is_not_found_and_an_outage_is_unreadable() {
    let (l, t) = linear();
    t.on_body(
        "POST",
        URL,
        "issue(id: $id) { id identifier",
        200,
        r#"{"data":null,"errors":[{"message":"Entity not found: Issue"}]}"#,
    );
    assert!(l.get("ABC-404").unwrap_err().is_not_found());
    let (l, t) = linear();
    t.on_body(
        "POST",
        URL,
        "issue(id: $id) { id identifier",
        400,
        r#"{"errors":[{"message":"Rate limit exceeded","extensions":{"code":"RATELIMITED"}}]}"#,
    );
    assert!(matches!(
        l.get("ABC-1").unwrap_err(),
        TrackerError::Unreadable { .. }
    ));
    let (l, t) = linear();
    t.on_body(
        "POST",
        URL,
        "issue(id: $id) { id identifier",
        502,
        "<html>bad gateway</html>",
    );
    assert!(matches!(
        l.get("ABC-1").unwrap_err(),
        TrackerError::Unreadable { .. }
    ));
}

#[test]
fn a_cut_label_list_is_unreadable() {
    let (l, t) = linear();
    let cut = issue("ABC-2", "Done", "completed").replace(
        r#""pageInfo":{"hasNextPage":false}"#,
        r#""pageInfo":{"hasNextPage":true}"#,
    );
    t.on_body(
        "POST",
        URL,
        "issue(id: $id) { id identifier",
        200,
        &format!(r#"{{"data":{{"issue":{cut}}}}}"#),
    );
    assert!(l.get("ABC-2").is_err());
}

#[test]
fn issue_fields_carry_the_duplicate_relation_both_directions() {
    let (l, t) = linear();
    t.on_body("POST", URL, "inverseRelations", 200, r#"{"data":{"issue":{
        "relations":{"nodes":[{"id":"r1","type":"blocks","relatedIssue":{"identifier":"ABC-100"}},{"id":"r2","type":"duplicate","relatedIssue":{"identifier":"ABC-3"}}],"pageInfo":{"hasNextPage":false}},
        "inverseRelations":{"nodes":[{"id":"r3","type":"blocks","issue":{"identifier":"ABC-9"}},{"id":"r4","type":"duplicate","issue":{"identifier":"ABC-4"}}],"pageInfo":{"hasNextPage":false}}}}}"#);
    let r = l.relations("ABC-2").unwrap();
    assert_eq!(r.blocks[0].other, "ABC-100");
    assert_eq!(r.blocked_by[0].other, "ABC-9");
    assert_eq!(
        r.duplicates
            .iter()
            .map(|x| x.other.as_str())
            .collect::<Vec<_>>(),
        vec!["ABC-3", "ABC-4"]
    );
    assert!(!r.cut);
}

#[test]
fn a_cut_relation_page_is_flagged() {
    let (l, t) = linear();
    t.on_body(
        "POST",
        URL,
        "inverseRelations",
        200,
        r#"{"data":{"issue":{
        "relations":{"nodes":[],"pageInfo":{"hasNextPage":false}},
        "inverseRelations":{"nodes":[],"pageInfo":{"hasNextPage":true}}}}}"#,
    );
    assert!(l.relations("ABC-2").unwrap().cut);
}

#[test]
fn comments_are_the_whole_thread_oldest_first_and_say_which_are_the_bridges() {
    let (l, t) = linear();
    t.on_body(
        "POST",
        URL,
        "viewer",
        200,
        r#"{"data":{"viewer":{"id":"u-bridge","name":"spl-asdlc","displayName":""}}}"#,
    );
    t.on_body("POST", URL, "comments(first: 100", 200, r#"{"data":{"issue":{"comments":{"nodes":[
        {"id":"c2","body":"later","createdAt":"2026-10-05T11:00:00Z","user":{"id":"u-human","name":"h","displayName":"Human"}},
        {"id":"c1","body":"merged_pr: x","createdAt":"2026-10-05T10:00:00Z","user":{"id":"u-bridge","name":"spl-asdlc","displayName":""}}],
        "pageInfo":{"hasNextPage":false,"endCursor":null}}}}}"#);
    let c = l.comments("ABC-2").unwrap();
    assert_eq!(c[0].id, "c1");
    assert!(c[0].by_bridge);
    assert!(!c[1].by_bridge);
    assert_eq!(c[0].author.as_ref().unwrap().name, "spl-asdlc");
}

#[test]
fn a_status_the_team_lacks_is_refused_before_any_write() {
    let (l, t) = linear();
    team(&t);
    let err = l.set_status("ABC-2", Status::PostDeploy, None).unwrap_err();
    assert!(
        err.to_string()
            .contains("has no state named \"Post Deploy\""),
        "{err}"
    );
    assert!(!t.bodies().iter().any(|b| b.contains("issueUpdate")));
}

#[test]
fn state_and_assignee_change_in_one_write() {
    let (l, t) = linear();
    team(&t);
    t.on_body(
        "POST",
        URL,
        "issueUpdate",
        200,
        r#"{"data":{"issueUpdate":{"success":true}}}"#,
    );
    let dev = User {
        id: "u-dev".into(),
        name: "Dev".into(),
    };
    l.set_status("ABC-2", Status::ToDeploy, Some(Some(&dev)))
        .unwrap();
    let body = t
        .bodies()
        .into_iter()
        .find(|b| b.contains("issueUpdate"))
        .unwrap();
    assert!(body.contains(r#""stateId":"s-deploy""#), "{body}");
    assert!(body.contains(r#""assigneeId":"u-dev""#));
}

#[test]
fn a_write_linear_does_not_confirm_is_refused() {
    let (l, t) = linear();
    t.on_body(
        "POST",
        URL,
        "commentUpdate",
        200,
        r#"{"data":{"commentUpdate":{"success":false}}}"#,
    );
    assert!(matches!(
        l.update_comment("c1", "x").unwrap_err(),
        TrackerError::Refused { .. }
    ));
}

#[test]
fn items_are_found_by_the_url_of_their_pr() {
    let (l, t) = linear();
    t.on_body("POST", URL, "attachmentsForURL", 200, r#"{"data":{"attachmentsForURL":{"nodes":[{"issue":{"identifier":"ABC-7"}},{"issue":{"identifier":"ABC-7"}}],"pageInfo":{"hasNextPage":false}}}}"#);
    assert_eq!(
        l.items_linked_to_pr("https://github.com/example-org/acme/pull/9")
            .unwrap(),
        vec!["ABC-7"]
    );
    let (l, t) = linear();
    t.on_body(
        "POST",
        URL,
        "attachmentsForURL",
        200,
        r#"{"data":{"attachmentsForURL":{"nodes":[]}}}"#,
    );
    assert!(
        l.items_linked_to_pr("u").is_err(),
        "an answer that does not say it is whole is not 'no ticket'"
    );
}

#[test]
fn an_item_is_created_with_its_state_labels_and_parent() {
    let (l, t) = linear();
    team(&t);
    t.on_body("POST", URL, "issueLabels(filter", 200, r#"{"data":{"issueLabels":{"nodes":[{"id":"l-1","name":"release/acme-v1.2.3","team":{"id":"team-1"}}],"pageInfo":{"hasNextPage":false}}}}"#);
    t.on_body(
        "POST",
        URL,
        "issue(id: $id) { id identifier",
        200,
        &format!(
            r#"{{"data":{{"issue":{}}}}}"#,
            issue("ABC-1", "Ready", "unstarted")
        ),
    );
    t.on_body(
        "POST",
        URL,
        "issueCreate",
        200,
        &format!(
            r#"{{"data":{{"issueCreate":{{"success":true,"issue":{}}}}}}}"#,
            issue("ABC-5", "Ready", "unstarted")
        ),
    );
    let created = l
        .create_item(&NewItem {
            title: "Security review — acme v1.2.3 (rc.1)".into(),
            description: "d".into(),
            labels: vec!["release/acme-v1.2.3".into()],
            status: Status::Ready,
            priority: Some(1),
            assignee: None,
            parent: Some("ABC-1".into()),
        })
        .unwrap();
    assert_eq!(created.key, "ABC-5");
    let body = t
        .bodies()
        .into_iter()
        .find(|b| b.contains("issueCreate"))
        .unwrap();
    for needle in [
        r#""stateId":"s-ready""#,
        r#""labelIds":["l-1"]"#,
        r#""parentId":"id-ABC-1""#,
        r#""priority":1"#,
    ] {
        assert!(body.contains(needle), "{needle} in {body}");
    }
}

#[test]
fn a_label_the_team_lacks_is_created_once() {
    let (l, t) = linear();
    team(&t);
    t.on_body(
        "POST",
        URL,
        "issueLabels(filter",
        200,
        r#"{"data":{"issueLabels":{"nodes":[],"pageInfo":{"hasNextPage":false}}}}"#,
    );
    t.on_body(
        "POST",
        URL,
        "issueLabelCreate",
        200,
        r#"{"data":{"issueLabelCreate":{"success":true,"issueLabel":{"id":"l-new"}}}}"#,
    );
    l.ensure_label("qa-finding").unwrap();
    l.ensure_label("qa-finding").unwrap();
    assert_eq!(
        t.bodies()
            .iter()
            .filter(|b| b.contains("issueLabelCreate"))
            .count(),
        1
    );
}

#[test]
fn a_label_list_is_read_page_by_page_and_refused_without_its_page_info() {
    let (l, t) = linear();
    team(&t);
    t.on_body("POST", URL, "issues(filter", 200, &format!(r#"{{"data":{{"issues":{{"nodes":[{}],"pageInfo":{{"hasNextPage":true,"endCursor":"c1"}}}}}}}}"#, issue("ABC-2", "Done", "completed")));
    t.on_body("POST", URL, "issues(filter", 200, &format!(r#"{{"data":{{"issues":{{"nodes":[{}],"pageInfo":{{"hasNextPage":false,"endCursor":null}}}}}}}}"#, issue("ABC-3", "Done", "completed")));
    assert_eq!(l.list_by_label("release/acme-v1.2.3").unwrap().len(), 2);
    let (l, t) = linear();
    team(&t);
    t.on_body(
        "POST",
        URL,
        "issues(filter",
        200,
        r#"{"data":{"issues":{"nodes":[]}}}"#,
    );
    assert!(l.list_by_label("x").is_err());
}

#[test]
fn a_relation_without_an_id_is_never_deleted() {
    let (l, _) = linear();
    assert!(l.unrelate("").is_err());
}

#[test]
fn a_creation_whose_answer_is_lost_is_looked_up_by_the_id_it_chose() {
    let (l, t) = linear();
    team(&t);
    t.on_body("POST", URL, "issueCreate", 502, "upstream timeout");
    t.on_body(
        "POST",
        URL,
        "issue(id: $id) { id identifier",
        200,
        &format!(
            r#"{{"data":{{"issue":{}}}}}"#,
            issue("ABC-7", "Ready", "unstarted")
        ),
    );
    let created = l
        .create_item(&NewItem {
            title: "t".into(),
            description: "d".into(),
            labels: vec![],
            status: Status::Ready,
            priority: None,
            assignee: None,
            parent: None,
        })
        .unwrap();
    assert_eq!(created.key, "ABC-7", "found, not created twice");
    let bodies = t.bodies();
    let create = bodies.iter().find(|b| b.contains("issueCreate")).unwrap();
    let lookup = bodies
        .iter()
        .find(|b| b.contains("issue(id: $id)"))
        .unwrap();
    let chosen = serde_json::from_str::<Value>(create).unwrap()["variables"]["input"]["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(looks_like_uuid(&chosen));
    assert!(lookup.contains(&chosen));
}

#[test]
fn an_existing_relation_is_not_created_again() {
    let (l, t) = linear();
    for key in ["ABC-1", "ABC-2"] {
        t.on_body(
            "POST",
            URL,
            "issue(id: $id) { id identifier",
            200,
            &format!(
                r#"{{"data":{{"issue":{}}}}}"#,
                issue(key, "Ready", "unstarted")
            ),
        );
    }
    t.on_body("POST", URL, "relations(first", 200, r#"{"data":{"issue":{
        "relations":{"nodes":[{"id":"r1","type":"blocks","relatedIssue":{"identifier":"ABC-2"}}],"pageInfo":{"hasNextPage":false}},
        "inverseRelations":{"nodes":[],"pageInfo":{"hasNextPage":false}}}}}"#);
    l.relate("ABC-1", "ABC-2", RelationKind::Blocks).unwrap();
    assert!(!t.bodies().iter().any(|b| b.contains("issueRelationCreate")));
}

#[test]
fn a_pr_already_linked_is_not_linked_again() {
    let (l, t) = linear();
    t.on_body("POST", URL, "attachmentsForURL", 200, r#"{"data":{"attachmentsForURL":{"nodes":[{"issue":{"identifier":"ABC-7"}}],"pageInfo":{"hasNextPage":false}}}}"#);
    l.link_pr("ABC-7", "https://github.com/example-org/acme/pull/9", "PR")
        .unwrap();
    assert!(!t.bodies().iter().any(|b| b.contains("attachmentLinkURL")));

    // An unreadable attachment list is not "not linked": nothing is written.
    let (l, t) = linear();
    t.on_body(
        "POST",
        URL,
        "attachmentsForURL",
        200,
        r#"{"data":{"attachmentsForURL":{"nodes":[]}}}"#,
    );
    assert!(l.link_pr("ABC-7", "u", "PR").is_err());
    assert!(!t.bodies().iter().any(|b| b.contains("attachmentLinkURL")));
}

#[test]
fn a_pr_linked_to_another_item_or_to_none_is_linked() {
    for nodes in [r#"[{"issue":{"identifier":"ABC-8"}}]"#, "[]"] {
        let (l, t) = linear();
        t.on_body(
            "POST",
            URL,
            "attachmentsForURL",
            200,
            &format!(
                r#"{{"data":{{"attachmentsForURL":{{"nodes":{nodes},"pageInfo":{{"hasNextPage":false}}}}}}}}"#
            ),
        );
        t.on_body(
            "POST",
            URL,
            "issue(id: $id) { id identifier",
            200,
            &format!(
                r#"{{"data":{{"issue":{}}}}}"#,
                issue("ABC-7", "Ready", "unstarted")
            ),
        );
        t.on_body(
            "POST",
            URL,
            "attachmentLinkURL",
            200,
            r#"{"data":{"attachmentLinkURL":{"success":true}}}"#,
        );
        l.link_pr("ABC-7", "https://github.com/example-org/acme/pull/9", "PR")
            .unwrap();
        let link = t
            .bodies()
            .into_iter()
            .find(|b| b.contains("attachmentLinkURL"))
            .expect("the link is written");
        assert!(link.contains("pull/9"), "{link}");
    }
}

#[test]
fn a_relation_that_names_no_item_is_unreadable() {
    let (l, t) = linear();
    t.on_body("POST", URL, "relations(first", 200, r#"{"data":{"issue":{
        "relations":{"nodes":[{"id":"r1","type":"blocks","relatedIssue":null}],"pageInfo":{"hasNextPage":false}},
        "inverseRelations":{"nodes":[],"pageInfo":{"hasNextPage":false}}}}}"#);
    assert!(matches!(
        l.relations("ABC-1").unwrap_err(),
        TrackerError::Unreadable { .. }
    ));
}

#[test]
fn lists_include_archived_items_and_ask_few_labels_per_item() {
    let (l, t) = linear();
    t.on_body("POST", URL, "children(first", 200, &format!(r#"{{"data":{{"issue":{{"children":{{"nodes":[{}],"pageInfo":{{"hasNextPage":false}}}}}}}}}}"#, issue("ABC-2", "Done", "completed")));
    assert_eq!(l.children("ABC-1").unwrap().len(), 1);
    let body = &t.bodies()[0];
    assert!(body.contains("includeArchived: true"), "{body}");
    assert!(body.contains("labels(first: 20)"), "{body}");
}

#[test]
fn labels_are_looked_up_without_case_within_the_team() {
    let (l, t) = linear();
    team(&t);
    t.on_body("POST", URL, "issueLabels(filter", 200, r#"{"data":{"issueLabels":{"nodes":[{"id":"l-1","name":"Release","team":{"id":"team-1"}}],"pageInfo":{"hasNextPage":false}}}}"#);
    l.ensure_label("release").unwrap();
    let bodies = t.bodies();
    assert!(!bodies.iter().any(|b| b.contains("issueLabelCreate")));
    let lookup = bodies.iter().find(|b| b.contains("issueLabels")).unwrap();
    assert!(
        lookup.contains("eqIgnoreCase") && lookup.contains("team-1"),
        "{lookup}"
    );
}

#[test]
fn a_user_is_found_by_id_as_well_as_by_name() {
    let (l, t) = linear();
    let id = new_uuid();
    t.on_body(
        "POST",
        URL,
        "user(id: $id)",
        200,
        &format!(r#"{{"data":{{"user":{{"id":"{id}","name":"n","displayName":"Dev"}}}}}}"#),
    );
    assert_eq!(l.user(&id).unwrap().name, "Dev");
    assert_ne!(new_uuid(), new_uuid());
}

#[test]
fn the_body_url_fallback_reads_only_linear_urls() {
    let (l, _) = linear();
    let body = "Closes https://linear.app/acme-ws/issue/ABC-12/some-title and mentions ABC-13 and BR-AF-001.";
    assert_eq!(l.keys_in_text(body), vec!["ABC-12"]);
}
