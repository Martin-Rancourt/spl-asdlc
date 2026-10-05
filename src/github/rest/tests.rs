//! The REST/GraphQL client against scripted answers.

use super::*;
use crate::github::http::ScriptedTransport;
use std::sync::Arc;

const BASE: &str = "https://api.test";

struct Shared(Arc<ScriptedTransport>);

impl Transport for Shared {
    fn send(&self, request: HttpRequest) -> Result<HttpResponse, String> {
        self.0.send(request)
    }
}

fn client() -> (RestGitHub, Arc<ScriptedTransport>) {
    let t = Arc::new(ScriptedTransport::new());
    let gh = RestGitHub::with_transport(
        Credentials::Token("test-token".into()),
        Box::new(Shared(t.clone())),
        BASE,
    );
    (gh, t)
}

fn repo() -> RepoSlug {
    RepoSlug::parse("example-org/acme").unwrap()
}

fn url(path: &str) -> String {
    format!("{BASE}{path}")
}

const PROTECTION: &str = "/repos/example-org/acme/branches/main/protection/required_status_checks";
const RULES: &str = "/repos/example-org/acme/rules/branches/main";

// ---- required checks (REQ-MERGE-014, REQ-MERGE-015, REQ-MERGE-021) ----

#[test]
fn required_contexts_separates_no_protection_from_no_answer() {
    let (gh, t) = client();
    t.on(
        "GET",
        &url(PROTECTION),
        404,
        r#"{"message":"Branch not protected"}"#,
    );
    t.on("GET", &url(RULES), 200, "[]");
    assert_eq!(
        gh.required_checks(&repo(), "main").unwrap(),
        RequiredChecks::default()
    );

    let (gh, t) = client();
    t.on("GET", &url(PROTECTION), 502, r#"{"message":"Bad gateway"}"#);
    t.on("GET", &url(RULES), 200, "[]");
    let err = gh.required_checks(&repo(), "main").unwrap_err();
    assert!(
        matches!(
            err,
            ReadError::Unreadable {
                status: Some(502),
                ..
            }
        ),
        "{err:?}"
    );
}

#[test]
fn a_404_on_the_protection_endpoint_is_an_answer_whatever_its_message() {
    let (gh, t) = client();
    t.on(
        "GET",
        &url(PROTECTION),
        404,
        r#"{"message":"something else entirely"}"#,
    );
    t.on("GET", &url(RULES), 200, "[]");
    assert!(gh.required_checks(&repo(), "main").is_ok());
}

#[test]
fn words_without_a_404_are_not_an_answer_about_protection() {
    let (gh, t) = client();
    t.on(
        "GET",
        &url(PROTECTION),
        403,
        r#"{"message":"Branch not protected"}"#,
    );
    t.on("GET", &url(RULES), 200, "[]");
    assert!(gh.required_checks(&repo(), "main").is_err());
}

#[test]
fn a_ruleset_is_combined_with_a_real_classic_answer() {
    let (gh, t) = client();
    t.on(
        "GET",
        &url(PROTECTION),
        200,
        r#"{"strict":false,"contexts":["legacy"],"checks":[{"context":"build"}]}"#,
    );
    t.on(
        "GET",
        &url(RULES),
        200,
        r#"[{"type":"required_status_checks","parameters":{"strict_required_status_checks_policy":true,"required_status_checks":[{"context":"required-check"}]}},{"type":"pull_request","parameters":{}}]"#,
    );
    let r = gh.required_checks(&repo(), "main").unwrap();
    assert_eq!(
        r.contexts.iter().cloned().collect::<Vec<_>>(),
        vec!["build", "legacy", "required-check"]
    );
    assert!(r.strict, "the ruleset's up-to-date policy counts");
}

#[test]
fn an_unreadable_ruleset_list_stops_the_read() {
    let (gh, t) = client();
    t.on("GET", &url(PROTECTION), 404, "{}");
    t.on("GET", &url(RULES), 500, "{}");
    assert!(gh.required_checks(&repo(), "main").is_err());
}

#[test]
fn the_required_checks_are_read_for_the_branch_asked() {
    let (gh, t) = client();
    let protection = "/repos/example-org/acme/branches/release%2F2026-10-05-14-03-utc/protection/required_status_checks";
    t.on("GET", &url(protection), 404, "{}");
    t.on(
        "GET",
        &url("/repos/example-org/acme/rules/branches/release%2F2026-10-05-14-03-utc"),
        200,
        "[]",
    );
    assert!(
        gh.required_checks(&repo(), "release/2026-10-05-14-03-utc")
            .is_ok()
    );
}

// ---- files at a ref (REQ-MERGE-040) ----

#[test]
fn a_file_answers_its_text_not_found_or_unreadable() {
    let (gh, t) = client();
    let path = "/repos/example-org/acme/contents/docs/business-rules.md?ref=";
    t.on(
        "GET",
        &url(&format!("{path}abc")),
        200,
        "| BR-ACME-001 | rule |",
    );
    t.on(
        "GET",
        &url(&format!("{path}missing")),
        404,
        r#"{"message":"Not Found"}"#,
    );
    t.on(
        "GET",
        &url(&format!("{path}flaky")),
        503,
        r#"{"message":"unavailable"}"#,
    );
    assert_eq!(
        gh.file(&repo(), "docs/business-rules.md", "abc").unwrap(),
        "| BR-ACME-001 | rule |"
    );
    assert!(
        gh.file(&repo(), "docs/business-rules.md", "missing")
            .unwrap_err()
            .is_not_found()
    );
    assert!(matches!(
        gh.file(&repo(), "docs/business-rules.md", "flaky")
            .unwrap_err(),
        ReadError::Unreadable {
            status: Some(503),
            ..
        }
    ));
    let calls = t.log.lock().unwrap();
    assert!(
        calls[0]
            .headers
            .iter()
            .any(|(k, v)| k == "Accept" && v == "application/vnd.github.raw+json")
    );
}

// ---- lists ----

#[test]
fn a_paged_list_follows_the_next_link() {
    let (gh, t) = client();
    let first = url("/repos/example-org/acme/pulls/7/files?per_page=100");
    let second = url("/repos/example-org/acme/pulls/7/files?per_page=100&page=2");
    t.on_with_headers(
        "GET",
        &first,
        200,
        r#"[{"filename":"a.rs"}]"#,
        &[(
            "Link",
            &format!("<{second}>; rel=\"next\", <{second}>; rel=\"last\""),
        )],
    );
    t.on("GET", &second, 200, r#"[{"filename":"b.rs"}]"#);
    assert_eq!(gh.pull_files(&repo(), 7).unwrap(), vec!["a.rs", "b.rs"]);
}

#[test]
fn changed_files_returns_an_error_rather_than_empty_when_github_fails() {
    let (gh, t) = client();
    t.on(
        "GET",
        &url("/repos/example-org/acme/pulls/7/files"),
        500,
        "{}",
    );
    assert!(gh.pull_files(&repo(), 7).is_err());
}

#[test]
fn a_file_list_github_would_cut_is_unreadable() {
    let (gh, t) = client();
    let page: Vec<String> = (0..3000)
        .map(|i| format!(r#"{{"filename":"f{i}"}}"#))
        .collect();
    t.on(
        "GET",
        &url("/repos/example-org/acme/pulls/7/files"),
        200,
        &format!("[{}]", page.join(",")),
    );
    let err = gh.pull_files(&repo(), 7).unwrap_err();
    assert!(err.to_string().contains("would be cut"), "{err}");
}

#[test]
fn a_list_that_is_not_a_list_is_unreadable() {
    let (gh, t) = client();
    t.on(
        "GET",
        &url("/repos/example-org/acme/pulls/7/files"),
        200,
        r#"{"files":[]}"#,
    );
    assert!(gh.pull_files(&repo(), 7).is_err());
}

// ---- compare ----

#[test]
fn a_comparison_reads_every_commit_and_refuses_a_cut_file_list() {
    let (gh, t) = client();
    let path = "/repos/example-org/acme/compare/acme-v1.0.0...acme-v1.1.0-rc.1?per_page=100";
    t.on(
        "GET",
        &url(path),
        200,
        r#"{"status":"ahead","ahead_by":2,"behind_by":0,"total_commits":2,
            "commits":[{"sha":"a1","commit":{"message":"feat: one\n\nbody"}}],
            "files":[{"filename":"src/x.rs"}]}"#,
    );
    t.on(
        "GET",
        &url(&format!("{path}&page=2")),
        200,
        r#"{"status":"ahead","total_commits":2,"commits":[{"sha":"b2","commit":{"message":"fix: two"}}],"files":[]}"#,
    );
    let c = gh
        .compare(&repo(), "acme-v1.0.0", "acme-v1.1.0-rc.1")
        .unwrap();
    assert_eq!(c.status, CompareStatus::Ahead);
    assert_eq!(
        c.commits
            .iter()
            .map(|c| c.subject.as_str())
            .collect::<Vec<_>>(),
        vec!["feat: one", "fix: two"]
    );
    assert_eq!(c.files, vec!["src/x.rs"]);

    let (gh, t) = client();
    let files: Vec<String> = (0..300)
        .map(|i| format!(r#"{{"filename":"f{i}"}}"#))
        .collect();
    t.on(
        "GET",
        &url(path),
        200,
        &format!(
            r#"{{"status":"ahead","total_commits":0,"commits":[],"files":[{}]}}"#,
            files.join(",")
        ),
    );
    assert!(
        gh.compare(&repo(), "acme-v1.0.0", "acme-v1.1.0-rc.1")
            .is_err()
    );
}

#[test]
fn a_comparison_github_lists_partially_is_unreadable() {
    let (gh, t) = client();
    let path = "/repos/example-org/acme/compare/a...b?per_page=100";
    t.on("GET", &url(path), 200, r#"{"status":"ahead","total_commits":5,"commits":[{"sha":"a","commit":{"message":"x"}}],"files":[]}"#);
    t.on(
        "GET",
        &url(&format!("{path}&page=2")),
        200,
        r#"{"status":"ahead","total_commits":5,"commits":[],"files":[]}"#,
    );
    assert!(gh.compare(&repo(), "a", "b").is_err());
}

// ---- pull requests over GraphQL ----

fn pull_node(number: u64) -> String {
    format!(
        r#"{{"number":{number},"url":"https://github.com/example-org/acme/pull/{number}","title":"feat: x","body":"b","state":"OPEN","isDraft":false,
        "author":{{"__typename":"Bot","login":"release-app"}},"baseRefName":"main","headRefName":"feat/x","headRefOid":"h{number}",
        "labels":{{"pageInfo":{{"hasNextPage":false}},"nodes":[{{"name":"needs-review"}}]}},
        "mergeable":"CONFLICTING","mergeStateStatus":"DIRTY","additions":10,"deletions":2,
        "createdAt":"2026-10-05T10:00:00Z","updatedAt":"2026-10-05T11:00:00Z","mergedAt":null,"mergedBy":null,"mergeCommit":null,
        "reviews":{{"pageInfo":{{"hasNextPage":false}},"nodes":[{{"author":{{"login":"coordinator-login"}},"state":"APPROVED","commit":{{"oid":"h{number}"}},"submittedAt":"2026-10-05T11:00:00Z"}}]}},
        "reviewThreads":{{"pageInfo":{{"hasNextPage":true}},"nodes":[{{"isResolved":false,"isOutdated":false,"path":"src/x.rs","comments":{{"nodes":[{{"author":{{"login":"reviewer"}},"body":"blocker: x"}}]}}}}]}},
        "commits":{{"nodes":[{{"commit":{{"statusCheckRollup":{{"contexts":{{"pageInfo":{{"hasNextPage":false}},"nodes":[
            {{"__typename":"CheckRun","name":"test","status":"COMPLETED","conclusion":"SUCCESS","startedAt":"2026-10-05T10:01:00Z","detailsUrl":"https://github.com/example-org/acme/actions/runs/1/job/2","checkSuite":{{"workflowRun":{{"workflow":{{"name":"Checks"}}}}}}}},
            {{"__typename":"CheckRun","name":"lint","status":"QUEUED","conclusion":null,"startedAt":"0001-01-01T00:00:00Z","detailsUrl":null,"checkSuite":{{"workflowRun":null}}}},
            {{"__typename":"StatusContext","context":"legacy/ci","state":"FAILURE","targetUrl":"https://ci.test/1","createdAt":"2026-10-05T10:02:00Z"}}
        ]}}}}}}}}]}}}}"#
    )
}

#[test]
fn open_pulls_parse_checks_reviews_threads_and_say_what_was_cut() {
    let (gh, t) = client();
    t.on(
        "POST",
        &url("/graphql"),
        200,
        &format!(
            r#"{{"data":{{"repository":{{"pullRequests":{{"pageInfo":{{"hasNextPage":false,"endCursor":null}},"nodes":[{}]}}}}}}}}"#,
            pull_node(12)
        ),
    );
    let pulls = gh.open_pulls(&repo(), Some("main")).unwrap();
    let p = &pulls[0];
    assert_eq!(p.number, 12);
    assert_eq!(
        p.author,
        Some(Actor {
            login: "release-app".into(),
            is_bot: true
        })
    );
    assert_eq!(p.labels, vec!["needs-review"]);
    assert_eq!(p.mergeable, Mergeable::Conflicting);
    assert_eq!(p.merge_state, "DIRTY");
    assert_eq!(p.checks.len(), 3);
    assert_eq!(p.checks[0].workflow.as_deref(), Some("Checks"));
    assert_eq!(
        p.checks[1].started_at, None,
        "the zero time is 'not started'"
    );
    assert_eq!(p.checks[2].kind, CheckKind::Status);
    assert_eq!(p.checks[2].conclusion.as_deref(), Some("FAILURE"));
    assert_eq!(p.reviews[0].commit_sha.as_deref(), Some("h12"));
    assert!(!p.threads[0].resolved);
    assert_eq!(p.truncated, vec!["reviewThreads"]);
    let body = &t.bodies()[0];
    assert!(
        body.contains("baseRefName: $base"),
        "the base filter is sent"
    );
    assert!(body.contains(r#""base":"main""#));
}

#[test]
fn open_pulls_follow_the_cursor() {
    let (gh, t) = client();
    t.on(
        "POST",
        &url("/graphql"),
        200,
        &format!(
            r#"{{"data":{{"repository":{{"pullRequests":{{"pageInfo":{{"hasNextPage":true,"endCursor":"C1"}},"nodes":[{}]}}}}}}}}"#,
            pull_node(1)
        ),
    );
    t.on(
        "POST",
        &url("/graphql"),
        200,
        &format!(
            r#"{{"data":{{"repository":{{"pullRequests":{{"pageInfo":{{"hasNextPage":false,"endCursor":null}},"nodes":[{}]}}}}}}}}"#,
            pull_node(2)
        ),
    );
    let pulls = gh.open_pulls(&repo(), None).unwrap();
    assert_eq!(
        pulls.iter().map(|p| p.number).collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(t.bodies()[1].contains(r#""cursor":"C1""#));
}

#[test]
fn a_graphql_error_is_unreadable_and_a_missing_object_not_found() {
    let (gh, t) = client();
    t.on(
        "POST",
        &url("/graphql"),
        200,
        r#"{"data":null,"errors":[{"type":"RATE_LIMITED","message":"slow down"}]}"#,
    );
    let err = gh.open_pulls(&repo(), None).unwrap_err();
    assert!(err.to_string().contains("slow down"), "{err}");

    let (gh, t) = client();
    t.on(
        "POST",
        &url("/graphql"),
        200,
        r#"{"data":{"repository":null},"errors":[{"type":"NOT_FOUND","message":"no repo"}]}"#,
    );
    assert!(gh.pull(&repo(), 3).unwrap_err().is_not_found());

    let (gh, t) = client();
    t.on(
        "POST",
        &url("/graphql"),
        401,
        r#"{"message":"Bad credentials"}"#,
    );
    assert!(matches!(
        gh.open_pulls(&repo(), None).unwrap_err(),
        ReadError::Unreadable {
            status: Some(401),
            ..
        }
    ));
}

#[test]
fn merged_pulls_stop_at_the_window_and_refuse_a_list_beyond_the_limit() {
    let merged = |n: u64, merged: &str, updated: &str| {
        pull_node(n)
            .replace(r#""state":"OPEN""#, r#""state":"MERGED""#)
            .replace(r#""mergedAt":null"#, &format!(r#""mergedAt":"{merged}""#))
            .replace(
                r#""updatedAt":"2026-10-05T11:00:00Z""#,
                &format!(r#""updatedAt":"{updated}""#),
            )
    };
    let page = format!(
        r#"{{"data":{{"repository":{{"pullRequests":{{"pageInfo":{{"hasNextPage":true,"endCursor":"C"}},"nodes":[{},{},{}]}}}}}}}}"#,
        merged(3, "2026-10-05T09:00:00Z", "2026-10-05T09:00:00Z"),
        merged(2, "2026-10-03T09:00:00Z", "2026-10-05T08:00:00Z"),
        merged(1, "2026-10-01T09:00:00Z", "2026-10-01T09:00:00Z"),
    );
    let (gh, t) = client();
    t.on("POST", &url("/graphql"), 200, &page);
    let since = Timestamp::parse("2026-10-04T09:00:00Z");
    let list = gh.merged_pulls(&repo(), None, since, 30).unwrap();
    assert_eq!(list.iter().map(|p| p.number).collect::<Vec<_>>(), vec![3]);
    assert_eq!(t.calls().len(), 1, "no page is read past the window");

    let (gh, t) = client();
    t.on("POST", &url("/graphql"), 200, &page);
    assert!(gh.merged_pulls(&repo(), None, None, 1).is_err());
}

// ---- writes ----

#[test]
fn a_merge_is_pinned_to_the_head_it_judged() {
    let (gh, t) = client();
    t.on(
        "PUT",
        &url("/repos/example-org/acme/pulls/12/merge"),
        200,
        r#"{"sha":"m12","merged":true}"#,
    );
    let sha = gh
        .merge_pull(&repo(), 12, "h12", MergeMethod::Squash)
        .unwrap();
    assert_eq!(sha, "m12");
    let body: Value = serde_json::from_str(&t.bodies()[0]).unwrap();
    assert_eq!(body, json!({"sha": "h12", "merge_method": "squash"}));
}

#[test]
fn a_refused_merge_carries_its_status() {
    let (gh, t) = client();
    t.on(
        "PUT",
        &url("/repos/example-org/acme/pulls/12/merge"),
        409,
        r#"{"message":"Head branch was modified"}"#,
    );
    let err = gh
        .merge_pull(&repo(), 12, "h12", MergeMethod::Merge)
        .unwrap_err();
    assert_eq!(err.status, Some(409));
    assert!(err.detail.contains("Head branch was modified"));
}

#[test]
fn an_update_branch_is_accepted_only_on_202() {
    let (gh, t) = client();
    t.on(
        "PUT",
        &url("/repos/example-org/acme/pulls/5/update-branch"),
        422,
        r#"{"message":"merge conflict"}"#,
    );
    assert_eq!(
        gh.update_branch(&repo(), 5, "h5").unwrap_err().status,
        Some(422)
    );
    let (gh, t) = client();
    t.on(
        "PUT",
        &url("/repos/example-org/acme/pulls/5/update-branch"),
        202,
        r#"{"message":"Updating"}"#,
    );
    assert!(gh.update_branch(&repo(), 5, "h5").is_ok());
    assert!(t.bodies()[0].contains(r#""expected_head_sha":"h5""#));
}

#[test]
fn removing_a_label_that_is_not_there_is_a_success() {
    let (gh, t) = client();
    t.on(
        "DELETE",
        &url("/repos/example-org/acme/issues/5/labels/needs%20review"),
        404,
        "{}",
    );
    assert!(gh.remove_label(&repo(), 5, "needs review").is_ok());
}

#[test]
fn put_file_replaces_an_existing_blob() {
    let (gh, t) = client();
    let path = "/repos/example-org/acme/contents/docs/releases/v1.2.3.md";
    t.on(
        "GET",
        &url(&format!("{path}?ref=release-record%2Facme-v1.2.3-rc.1")),
        200,
        r#"{"sha":"old-blob"}"#,
    );
    t.on("PUT", &url(path), 200, r#"{"commit":{"sha":"c1"}}"#);
    let sha = gh
        .put_file(
            &repo(),
            "release-record/acme-v1.2.3-rc.1",
            "docs/releases/v1.2.3.md",
            "text",
            "docs: record",
        )
        .unwrap();
    assert_eq!(sha, "c1");
    let body: Value = serde_json::from_str(&t.bodies()[1]).unwrap();
    assert_eq!(body["sha"], "old-blob");
    assert_eq!(body["content"], "dGV4dA==");
    assert_eq!(body["branch"], "release-record/acme-v1.2.3-rc.1");
}

#[test]
fn a_dispatch_names_the_ref_and_inputs() {
    let (gh, t) = client();
    t.on(
        "POST",
        &url("/repos/example-org/acme/actions/workflows/release-cut.yml/dispatches"),
        204,
        "",
    );
    gh.dispatch_workflow(
        &repo(),
        "release-cut.yml",
        "main",
        &[("action".into(), "open".into())],
    )
    .unwrap();
    let body: Value = serde_json::from_str(&t.bodies()[0]).unwrap();
    assert_eq!(body, json!({"ref": "main", "inputs": {"action": "open"}}));
}

// ---- identity and auth ----

#[test]
fn a_personal_token_names_its_user() {
    let (gh, t) = client();
    t.on("GET", &url("/user"), 200, r#"{"login":"someone"}"#);
    assert_eq!(
        gh.identity().unwrap(),
        Identity::User {
            login: "someone".into()
        }
    );
    let req = &t.log.lock().unwrap()[0];
    assert!(
        req.headers
            .iter()
            .any(|(k, v)| k == "Authorization" && v == "Bearer test-token")
    );
    assert!(
        req.headers
            .iter()
            .any(|(k, v)| k == "X-GitHub-Api-Version" && v == API_VERSION)
    );
}

#[test]
fn an_app_with_an_unusable_key_is_refused_before_any_call() {
    let env = |k: &str| match k {
        "SPL_ASDLC_APP_ID" => Some("1".to_string()),
        "SPL_ASDLC_APP_PRIVATE_KEY" => Some("not a key".to_string()),
        _ => None,
    };
    let err = Credentials::app_from_env(&env).unwrap_err();
    assert!(err.contains("not an RSA PEM key"), "{err}");
    let missing = Credentials::app_from_env(&|_| None).unwrap_err();
    assert!(missing.contains("SPL_ASDLC_APP_ID is not set"), "{missing}");
    let token = Credentials::token_from_env(&|_| None).unwrap_err();
    assert!(token.contains("SPL_ASDLC_GITHUB_TOKEN"), "{token}");
}

#[test]
fn the_app_identity_writes_as_its_bot_login() {
    let id = Identity::App {
        slug: "spl-asdlc".into(),
        app_id: "7".into(),
    };
    assert_eq!(id.login(), format!("{}[bot]", "spl-asdlc"));
    assert!(id.to_string().contains("writing as"));
}

#[test]
fn path_segments_are_encoded() {
    assert_eq!(encode("release/2026 x", false), "release%2F2026%20x");
    assert_eq!(encode("docs/a b.md", true), "docs/a%20b.md");
}

#[test]
fn next_link_reads_only_the_next_relation() {
    assert_eq!(
        next_link(r#"<https://a/1?page=2>; rel="next", <https://a/1?page=9>; rel="last""#),
        Some("https://a/1?page=2".to_string())
    );
    assert_eq!(next_link(r#"<https://a/1?page=1>; rel="prev""#), None);
}
