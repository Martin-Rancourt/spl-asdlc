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

// ---- rulesets (spl-asdlc doctor) ----

const RULESETS: &str = "/repos/example-org/acme/rulesets?includes_parents=true&per_page=100";

#[test]
fn each_ruleset_is_read_by_id_with_its_conditions_and_rules() {
    let (gh, t) = client();
    t.on(
        "GET",
        &url(RULESETS),
        200,
        r#"[{"id":11,"name":"release","target":"branch","enforcement":"active"},{"id":12,"name":"tags","target":"tag","enforcement":"evaluate"}]"#,
    );
    t.on(
        "GET",
        &url("/repos/example-org/acme/rulesets/11?"),
        200,
        r#"{"id":11,"name":"release","target":"branch","enforcement":"active","conditions":{"ref_name":{"include":["refs/heads/release/**"],"exclude":[]}},"rules":[{"type":"pull_request","parameters":{}},{"type":"required_status_checks","parameters":{"strict_required_status_checks_policy":true,"required_status_checks":[{"context":"required-check"}]}}],"bypass_actors":[]}"#,
    );
    t.on(
        "GET",
        &url("/repos/example-org/acme/rulesets/12?"),
        200,
        r#"{"id":12,"name":"tags","target":"tag","enforcement":"evaluate","conditions":{"ref_name":{"include":["refs/tags/*-rc.*"],"exclude":["refs/tags/v1"]}},"rules":[{"type":"update"},{"type":"deletion"}]}"#,
    );
    let r = gh.rulesets(&repo()).unwrap();
    assert_eq!(r.len(), 2);
    assert_eq!(r[0].include, vec!["refs/heads/release/**"]);
    assert!(r[0].is_active());
    assert_eq!(
        r[0].rules.iter().cloned().collect::<Vec<_>>(),
        vec!["pull_request", "required_status_checks"]
    );
    assert_eq!(
        r[0].required_checks.iter().cloned().collect::<Vec<_>>(),
        vec!["required-check"]
    );
    assert_eq!(r[1].target, "tag");
    assert!(!r[1].is_active());
    assert_eq!(r[1].exclude, vec!["refs/tags/v1"]);
    assert!(r[1].rules.contains("update") && r[1].rules.contains("deletion"));
}

#[test]
fn rulesets_that_cannot_be_read_whole_are_unreadable() {
    // The list itself.
    for status in [404, 403, 500] {
        let (gh, t) = client();
        t.on("GET", &url(RULESETS), status, "{}");
        let err = gh.rulesets(&repo()).unwrap_err();
        assert!(matches!(err, ReadError::Unreadable { .. }), "{err:?}");
    }
    // One ruleset of the list.
    let listed = r#"[{"id":11,"name":"main","target":"branch","enforcement":"active"}]"#;
    let one = url("/repos/example-org/acme/rulesets/11?");
    for (status, body) in [
        (404, "{}"),
        (500, "{}"),
        (200, r#"{"id":11,"name":"main","target":"branch"}"#),
        (
            200,
            r#"{"id":11,"name":"main","target":"branch","enforcement":"active","rules":[{"parameters":{}}]}"#,
        ),
        (
            200,
            r#"{"id":11,"name":"main","target":"branch","enforcement":"active","conditions":{"ref_name":{"include":[7]}}}"#,
        ),
        (
            200,
            r#"{"id":11,"name":"main","target":"branch","enforcement":"active","conditions":{"ref_name":{"include":["~DEFAULT_BRANCH"],"exclude":null}},"rules":[]}"#,
        ),
        (
            200,
            r#"{"id":11,"name":"main","target":"branch","enforcement":"active","conditions":{"ref_name":{"include":["~DEFAULT_BRANCH"],"exclude":[]}},"rules":null}"#,
        ),
    ] {
        let (gh, t) = client();
        t.on("GET", &url(RULESETS), 200, listed);
        t.on("GET", &one, status, body);
        let err = gh.rulesets(&repo()).unwrap_err();
        assert!(
            matches!(err, ReadError::Unreadable { .. }),
            "{body}: {err:?}"
        );
    }
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
            "base_commit":{"sha":"r0","commit":{"committer":{"date":"2026-10-11T13:46:10Z"}}},
            "commits":[{"sha":"a1","commit":{"message":"feat: one\n\nbody",
                "committer":{"date":"2026-10-11T12:19:55Z"}}}],
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
    // The committer dates; one GitHub did not give
    // is unknown, never a guess.
    assert_eq!(
        c.base_committed_at,
        Timestamp::parse("2026-10-11T13:46:10Z")
    );
    assert_eq!(
        c.commits[0].committed_at,
        Timestamp::parse("2026-10-11T12:19:55Z")
    );
    assert_eq!(c.commits[1].committed_at, None);

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

#[test]
fn the_history_from_the_root_reads_every_commit_and_every_path_or_refuses() {
    let commits = "/repos/example-org/acme/commits?sha=c2&per_page=100";
    let tree = "/repos/example-org/acme/git/trees/c2?recursive=1";
    let (gh, t) = client();
    t.on(
        "GET",
        &url(commits),
        200,
        r#"[{"sha":"c2","commit":{"message":"feat: two\n\nbody","committer":{"date":"2026-10-12T10:00:00Z"}}},
            {"sha":"c1","commit":{"message":"initial"}}]"#,
    );
    t.on(
        "GET",
        &url(tree),
        200,
        r#"{"truncated":false,"tree":[{"path":"src","type":"tree"},{"path":"src/x.rs","type":"blob"},
            {"path":"README.md","type":"blob"},{"path":"vendor/lib","type":"commit"}]}"#,
    );
    let c = gh.since_root(&repo(), "c2").unwrap();
    // Oldest first, as a comparison lists them, with no base to bound them.
    assert_eq!(
        c.commits.iter().map(|c| c.sha.as_str()).collect::<Vec<_>>(),
        vec!["c1", "c2"]
    );
    assert_eq!(c.commits[1].subject, "feat: two");
    assert_eq!(
        c.commits[1].committed_at,
        Timestamp::parse("2026-10-12T10:00:00Z")
    );
    assert_eq!(c.ahead_by, 2);
    assert_eq!(c.base_committed_at, None);
    assert_eq!(c.files, vec!["src/x.rs", "README.md", "vendor/lib"]);

    // A tree GitHub cut, or one that does not say, is not the tree.
    for answer in [
        r#"{"truncated":true,"tree":[]}"#,
        r#"{"tree":[{"path":"a","type":"blob"}]}"#,
    ] {
        let (gh, t) = client();
        t.on(
            "GET",
            &url(commits),
            200,
            r#"[{"sha":"c2","commit":{"message":"x"}}]"#,
        );
        t.on("GET", &url(tree), 200, answer);
        assert!(gh.since_root(&repo(), "c2").is_err(), "{answer}");
    }
    // A commit list that cannot be read is no history.
    let (gh, t) = client();
    t.on("GET", &url(commits), 500, "{}");
    t.on("GET", &url(tree), 200, r#"{"truncated":false,"tree":[]}"#);
    assert!(gh.since_root(&repo(), "c2").is_err());
}

// ---- pull requests over GraphQL ----

fn pull_node(number: u64) -> String {
    format!(
        r#"{{"number":{number},"url":"https://github.com/example-org/acme/pull/{number}","title":"feat: x","body":"b","state":"OPEN","isDraft":false,
        "author":{{"__typename":"Bot","login":"release-app"}},"baseRefName":"main","headRefName":"feat/x","headRefOid":"h{number}",
        "labels":{{"pageInfo":{{"hasNextPage":false}},"nodes":[{{"name":"needs-review"}}]}},
        "mergeable":"CONFLICTING","mergeStateStatus":"DIRTY","additions":10,"deletions":2,
        "createdAt":"2026-10-05T10:00:00Z","updatedAt":"2026-10-05T11:00:00Z","mergedAt":null,"mergedBy":null,"mergeCommit":null,
        "reviews":{{"pageInfo":{{"hasNextPage":false}},"nodes":[{{"databaseId":7,"author":{{"login":"coordinator-login"}},"state":"APPROVED","commit":{{"oid":"h{number}"}},"submittedAt":"2026-10-05T11:00:00Z"}}]}},
        "reviewThreads":{{"pageInfo":{{"hasNextPage":true}},"nodes":[{{"isResolved":false,"isOutdated":false,"path":"src/x.rs","comments":{{"nodes":[{{"author":{{"login":"reviewer"}},"body":"blocker: x"}}]}}}}]}},
        "commits":{{"nodes":[{{"commit":{{"oid":"h{number}","statusCheckRollup":{{"contexts":{{"pageInfo":{{"hasNextPage":false}},"nodes":[
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
            r#"{{"data":{{"repository":{{"pullRequests":{{"totalCount":1,"pageInfo":{{"hasNextPage":false,"endCursor":null}},"nodes":[{}]}}}}}}}}"#,
            pull_node(12)
        ),
    );
    let pulls = gh.open_pulls(&repo(), Some("main")).unwrap();
    let p = &pulls[0];
    assert_eq!(p.number, 12);
    assert_eq!(
        p.author,
        Some(Actor {
            login: format!("{}[bot]", "release-app"),
            is_bot: true
        }),
        "a bot is named in the spelling REST and the UI use"
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
    assert_eq!(p.reviews[0].id, Some(7), "a dismissal names it");
    assert!(!p.threads[0].resolved);
    assert_eq!(p.truncated, vec!["reviewThreads"]);
    let body = &t.bodies()[0];
    assert!(
        body.contains("baseRefName: $base"),
        "the base filter is sent"
    );
    assert!(body.contains(r#""base":"main""#));
}

/// The login REST and the UI show for an App.
fn bot(slug: &str) -> String {
    format!("{slug}[bot]")
}

#[test]
fn an_app_reviewing_is_named_in_the_rest_spelling() {
    // GraphQL names an App by its bare slug; the bridge reads it as REST
    // and the UI show it, and a human as they are.
    let node = pull_node(12)
        .replace(
            r#""author":{"login":"coordinator-login"}"#,
            r#""author":{"__typename":"Bot","login":"coord-app"}"#,
        )
        .replace(
            r#""author":{"login":"reviewer"}"#,
            r#""author":{"__typename":"User","login":"reviewer"}"#,
        );
    assert!(node.contains(r#""__typename":"User""#), "the fixture moved");
    let (gh, t) = client();
    t.on(
        "POST",
        &url("/graphql"),
        200,
        &format!(
            r#"{{"data":{{"repository":{{"pullRequests":{{"totalCount":1,"pageInfo":{{"hasNextPage":false,"endCursor":null}},"nodes":[{node}]}}}}}}}}"#
        ),
    );
    let pulls = gh.open_pulls(&repo(), None).unwrap();
    let p = &pulls[0];
    assert_eq!(p.reviews[0].author, Some(bot("coord-app")));
    assert_eq!(p.threads[0].first_author.as_deref(), Some("reviewer"));
    assert!(
        t.bodies()[0].contains("nodes { databaseId author { __typename login } state"),
        "the query asks what the review's author is"
    );
}

#[test]
fn open_pulls_follow_the_cursor() {
    let (gh, t) = client();
    t.on(
        "POST",
        &url("/graphql"),
        200,
        &format!(
            r#"{{"data":{{"repository":{{"pullRequests":{{"totalCount":2,"pageInfo":{{"hasNextPage":true,"endCursor":"C1"}},"nodes":[{}]}}}}}}}}"#,
            pull_node(1)
        ),
    );
    t.on(
        "POST",
        &url("/graphql"),
        200,
        &format!(
            r#"{{"data":{{"repository":{{"pullRequests":{{"totalCount":2,"pageInfo":{{"hasNextPage":false,"endCursor":null}},"nodes":[{}]}}}}}}}}"#,
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
        r#"{{"data":{{"repository":{{"pullRequests":{{"totalCount":3,"pageInfo":{{"hasNextPage":true,"endCursor":"C"}},"nodes":[{},{},{}]}}}}}}}}"#,
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
fn a_dismissal_names_its_review_and_says_why() {
    let (gh, t) = client();
    t.on(
        "PUT",
        &url("/repos/example-org/acme/pulls/12/reviews/41/dismissals"),
        200,
        r#"{"id":41,"state":"DISMISSED"}"#,
    );
    gh.dismiss_review(&repo(), 12, 41, "superseded").unwrap();
    let body: Value = serde_json::from_str(&t.bodies()[0]).unwrap();
    assert_eq!(body, json!({"message": "superseded", "event": "DISMISS"}));
    let (gh, t) = client();
    t.on(
        "PUT",
        &url("/repos/example-org/acme/pulls/12/reviews/41/dismissals"),
        403,
        r#"{"message":"Resource not accessible by integration"}"#,
    );
    let err = gh
        .dismiss_review(&repo(), 12, 41, "superseded")
        .unwrap_err();
    assert_eq!(err.status, Some(403));
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
fn removing_a_label_on_a_404_is_a_refusal_not_a_success() {
    // A 404 is also an unknown PR or a repository out of reach: callers
    // remove only a label the PR is read to carry.
    let (gh, t) = client();
    t.on(
        "DELETE",
        &url("/repos/example-org/acme/issues/5/labels/needs%20review"),
        404,
        "{}",
    );
    assert_eq!(
        gh.remove_label(&repo(), 5, "needs review")
            .unwrap_err()
            .status,
        Some(404)
    );
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

// ---- the review of lot 2: fail-open residues closed ----

fn one_page(nodes: &str, total: u64) -> String {
    format!(
        r#"{{"data":{{"repository":{{"pullRequests":{{"totalCount":{total},"pageInfo":{{"hasNextPage":false,"endCursor":null}},"nodes":[{nodes}]}}}}}}}}"#
    )
}

#[test]
fn a_rollup_of_another_commit_than_the_head_says_nothing_about_the_head() {
    let (gh, t) = client();
    let node = pull_node(12).replace(r#""oid":"h12""#, r#""oid":"an-older-commit""#);
    t.on("POST", &url("/graphql"), 200, &one_page(&node, 1));
    let p = &gh.open_pulls(&repo(), None).unwrap()[0];
    assert!(p.checks.is_empty());
    assert!(p.truncated.contains(&"checks".to_string()));
}

#[test]
fn a_connection_that_does_not_say_whether_it_was_cut_is_unreadable() {
    let (gh, t) = client();
    let node = pull_node(12).replace(
        r#""labels":{"pageInfo":{"hasNextPage":false},"nodes""#,
        r#""labels":{"nodes""#,
    );
    assert_ne!(node, pull_node(12), "the fixture carries the field");
    t.on("POST", &url("/graphql"), 200, &one_page(&node, 1));
    assert!(gh.open_pulls(&repo(), None).is_err());

    let (gh, t) = client();
    t.on(
        "POST",
        &url("/graphql"),
        200,
        &format!(
            r#"{{"data":{{"repository":{{"pullRequests":{{"totalCount":1,"nodes":[{}]}}}}}}}}"#,
            pull_node(1)
        ),
    );
    assert!(
        gh.open_pulls(&repo(), None).is_err(),
        "a page without pageInfo is not the last page"
    );
}

#[test]
fn a_pr_without_its_size_is_unreadable_not_empty() {
    let (gh, t) = client();
    let node = pull_node(12).replace(r#""additions":10,"#, "");
    assert_ne!(node, pull_node(12));
    t.on("POST", &url("/graphql"), 200, &one_page(&node, 1));
    assert!(gh.open_pulls(&repo(), None).is_err());
}

#[test]
fn a_list_that_moved_while_it_was_read_is_unreadable() {
    let (gh, t) = client();
    t.on("POST", &url("/graphql"), 200, &one_page(&pull_node(1), 2));
    let err = gh.open_pulls(&repo(), None).unwrap_err();
    assert!(err.to_string().contains("moved while it was read"), "{err}");
}

#[test]
fn a_pr_read_twice_across_pages_counts_once() {
    let (gh, t) = client();
    t.on(
        "POST",
        &url("/graphql"),
        200,
        &format!(
            r#"{{"data":{{"repository":{{"pullRequests":{{"totalCount":2,"pageInfo":{{"hasNextPage":true,"endCursor":"C"}},"nodes":[{},{}]}}}}}}}}"#,
            pull_node(1),
            pull_node(2)
        ),
    );
    t.on("POST", &url("/graphql"), 200, &one_page(&pull_node(2), 2));
    assert_eq!(gh.open_pulls(&repo(), None).unwrap().len(), 2);
}

#[test]
fn a_merged_pr_without_a_merge_time_is_unreadable() {
    let (gh, t) = client();
    let node = pull_node(3).replace(r#""state":"OPEN""#, r#""state":"MERGED""#);
    t.on("POST", &url("/graphql"), 200, &one_page(&node, 1));
    assert!(gh.merged_pulls(&repo(), None, None, 30).is_err());
}

#[test]
fn a_comparison_without_its_counts_or_files_is_unreadable() {
    let path = "/repos/example-org/acme/compare/a...b?per_page=100";
    let (gh, t) = client();
    t.on(
        "GET",
        &url(path),
        200,
        r#"{"status":"ahead","commits":[],"files":[]}"#,
    );
    assert!(gh.compare(&repo(), "a", "b").is_err());
    let (gh, t) = client();
    t.on(
        "GET",
        &url(path),
        200,
        r#"{"status":"ahead","total_commits":0,"commits":[]}"#,
    );
    assert!(gh.compare(&repo(), "a", "b").is_err());
}

#[test]
fn a_malformed_ref_is_never_dropped_from_the_list() {
    let (gh, t) = client();
    t.on(
        "GET",
        &url("/repos/example-org/acme/git/matching-refs/heads/release/?per_page=100"),
        200,
        r#"[{"ref":"refs/heads/release/2026-10-05-14-03-utc","object":{"sha":"a","type":"commit"}},{"ref":"refs/heads/release/x"}]"#,
    );
    assert!(gh.branches(&repo(), "release/").is_err());
}

#[test]
fn branches_and_tags_are_read_by_prefix() {
    let (gh, t) = client();
    t.on(
        "GET",
        &url("/repos/example-org/acme/git/matching-refs/tags?per_page=100"),
        200,
        r#"[{"ref":"refs/tags/acme-v1.0.0","object":{"sha":"t1","type":"tag"}}]"#,
    );
    let tags = gh.tags(&repo(), "").unwrap();
    assert_eq!(tags[0].name, "acme-v1.0.0");
    assert_eq!(tags[0].object_type, ObjectType::Tag);
}

#[test]
fn a_rename_names_both_paths() {
    let (gh, t) = client();
    t.on(
        "GET",
        &url("/repos/example-org/acme/pulls/7/files"),
        200,
        r#"[{"filename":"src/public/x.rs","previous_filename":"src/auth/x.rs","status":"renamed"}]"#,
    );
    assert_eq!(
        gh.pull_files(&repo(), 7).unwrap(),
        vec!["src/auth/x.rs", "src/public/x.rs"]
    );
}

#[test]
fn a_merge_github_does_not_confirm_is_a_refusal() {
    let (gh, t) = client();
    t.on(
        "PUT",
        &url("/repos/example-org/acme/pulls/12/merge"),
        200,
        r#"{"merged":false,"message":"?"}"#,
    );
    assert!(
        gh.merge_pull(&repo(), 12, "h12", MergeMethod::Squash)
            .is_err()
    );
}

#[test]
fn the_token_never_leaves_the_api_host() {
    let (gh, t) = client();
    t.on_with_headers(
        "GET",
        &url("/repos/example-org/acme/pulls/7/files"),
        200,
        r#"[{"filename":"a"}]"#,
        &[(
            "Link",
            "<https://elsewhere.test/steal?page=2>; rel=\"next\"",
        )],
    );
    assert!(gh.pull_files(&repo(), 7).is_err());
    assert_eq!(t.calls().len(), 1);
}

// ---- the App path ----

static TEST_KEY: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    use rsa::pkcs1::EncodeRsaPrivateKey;
    let key = rsa::RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap();
    key.to_pkcs1_pem(rsa::pkcs8::LineEnding::LF)
        .unwrap()
        .to_string()
});

fn app_client() -> (RestGitHub, Arc<ScriptedTransport>) {
    let t = Arc::new(ScriptedTransport::new());
    let gh = RestGitHub::with_transport(
        Credentials::App {
            app_id: "42".into(),
            private_key_pem: TEST_KEY.as_bytes().to_vec(),
        },
        Box::new(Shared(t.clone())),
        BASE,
    );
    (gh, t)
}

fn script_installation(t: &ScriptedTransport, token: &str) {
    t.on(
        "GET",
        &url("/repos/example-org/acme/installation"),
        200,
        r#"{"id":7}"#,
    );
    t.on(
        "POST",
        &url("/app/installations/7/access_tokens"),
        201,
        &format!(r#"{{"token":"{token}","expires_at":"2999-01-01T00:00:00Z"}}"#),
    );
}

fn bearer_of(req: &HttpRequest) -> String {
    req.headers
        .iter()
        .find(|(k, _)| k == "Authorization")
        .map(|(_, v)| v.clone())
        .unwrap_or_default()
}

#[test]
fn an_app_mints_an_installation_token_once_and_reuses_it() {
    let (gh, t) = app_client();
    script_installation(&t, "inst-1");
    t.on(
        "GET",
        &url("/repos/example-org/acme"),
        200,
        r#"{"default_branch":"main"}"#,
    );
    assert_eq!(gh.default_branch(&repo()).unwrap(), "main");
    assert_eq!(gh.default_branch(&repo()).unwrap(), "main");
    let calls = t.calls();
    assert_eq!(
        calls.iter().filter(|c| c.contains("access_tokens")).count(),
        1,
        "{calls:?}"
    );
    let log = t.log.lock().unwrap();
    assert!(
        bearer_of(&log[0]).starts_with("Bearer ey"),
        "the installation lookup uses the App JWT"
    );
    assert_eq!(bearer_of(log.last().unwrap()), "Bearer inst-1");
}

#[test]
fn the_app_jwt_is_backdated_and_short_lived() {
    let now = time::OffsetDateTime::now_utc();
    let jwt = crate::github::auth::app_jwt("42", TEST_KEY.as_bytes(), now).unwrap();
    let payload = jwt.split('.').nth(1).unwrap();
    let claims: Value = serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(claims["iss"], "42");
    assert_eq!(now.unix_timestamp() - claims["iat"].as_i64().unwrap(), 60);
    assert_eq!(claims["exp"].as_i64().unwrap() - now.unix_timestamp(), 540);
}

#[test]
fn an_app_not_installed_on_the_repository_is_unreadable_not_absent() {
    let (gh, t) = app_client();
    t.on(
        "GET",
        &url("/repos/example-org/acme/installation"),
        404,
        r#"{"message":"Not Found"}"#,
    );
    let err = gh.default_branch(&repo()).unwrap_err();
    assert!(matches!(err, ReadError::Unreadable { .. }), "{err:?}");
    assert!(err.to_string().contains("not installed"), "{err}");
}

#[test]
fn a_revoked_installation_token_is_minted_again_once() {
    let (gh, t) = app_client();
    script_installation(&t, "inst-1");
    t.on(
        "POST",
        &url("/app/installations/7/access_tokens"),
        201,
        r#"{"token":"inst-2","expires_at":"2999-01-01T00:00:00Z"}"#,
    );
    t.on(
        "GET",
        &url("/repos/example-org/acme"),
        401,
        r#"{"message":"Bad credentials"}"#,
    );
    t.on(
        "GET",
        &url("/repos/example-org/acme"),
        200,
        r#"{"default_branch":"main"}"#,
    );
    assert_eq!(gh.default_branch(&repo()).unwrap(), "main");
    let log = t.log.lock().unwrap();
    assert_eq!(bearer_of(log.last().unwrap()), "Bearer inst-2");
}

#[test]
fn the_app_identity_is_read_with_the_jwt() {
    let (gh, t) = app_client();
    t.on("GET", &url("/app"), 200, r#"{"slug":"spl-asdlc","id":42}"#);
    assert_eq!(
        gh.identity().unwrap(),
        Identity::App {
            slug: "spl-asdlc".into(),
            app_id: "42".into()
        }
    );
}

// ---- workflow runs and the evidence pack (spec 05 §3, BR-ASDLC-016) ----

#[test]
fn a_run_says_its_attempt_and_an_absent_attempt_reads_as_the_first() {
    let (gh, t) = client();
    t.on(
        "GET",
        &url("/repos/example-org/acme/actions/workflows/release-gate.yml/runs"),
        200,
        r#"{"workflow_runs":[
            {"id":2,"head_sha":"s","head_branch":"t","event":"push","status":"completed","conclusion":"failure","run_attempt":2,"html_url":"u2","created_at":"2026-10-05T11:00:00Z","updated_at":"2026-10-05T11:40:00Z"},
            {"id":1,"head_sha":"s","head_branch":"t","event":"push","status":"completed","conclusion":"failure","html_url":"u1"}]}"#,
    );
    let runs = gh
        .workflow_runs(&repo(), "release-gate.yml", None, Some("s"), None)
        .unwrap();
    assert_eq!(runs.iter().map(|r| r.attempt).collect::<Vec<_>>(), [2, 1]);
    assert_eq!(
        runs[0].updated_at,
        crate::github::Timestamp::parse("2026-10-05T11:40:00Z"),
        "when it completed, for the bounded wait of its rerun"
    );
    assert_eq!(runs[1].updated_at, None);
}

#[test]
fn a_file_is_read_out_of_an_artifact_zip_without_sending_the_token_to_its_host() {
    let (gh, t) = client();
    let zip = crate::github::zip::tests::archive(
        &[("results/suites.json", br#"{"failed":["tests"]}"#)],
        true,
    );
    t.on_with_headers(
        "GET",
        &url("/repos/example-org/acme/actions/artifacts/7/zip"),
        302,
        "",
        &[("Location", "https://blob.test/pack.zip?sig=x")],
    );
    t.on_bytes("GET", "https://blob.test/pack.zip", 200, &zip);
    assert_eq!(
        gh.artifact_file(&repo(), 7, "results/suites.json").unwrap(),
        r#"{"failed":["tests"]}"#
    );
    let log = t.log.lock().unwrap();
    let blob = log
        .iter()
        .find(|r| r.url.starts_with("https://blob.test/"))
        .expect("the redirect was followed");
    assert!(
        !blob
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("authorization")),
        "the token goes to the API host only"
    );
    drop(log);
    assert!(
        gh.artifact_file(&repo(), 7, "results/other.json")
            .unwrap_err()
            .is_not_found()
    );
}

#[test]
fn an_artifact_that_cannot_be_downloaded_or_read_is_unreadable() {
    let (gh, t) = client();
    t.on(
        "GET",
        &url("/repos/example-org/acme/actions/artifacts/7/zip"),
        410,
        r#"{"message":"Artifact has expired"}"#,
    );
    let e = gh
        .artifact_file(&repo(), 7, "results/suites.json")
        .unwrap_err();
    assert!(!e.is_not_found(), "{e}");
    let (gh, t) = client();
    t.on_bytes(
        "GET",
        &url("/repos/example-org/acme/actions/artifacts/8/zip"),
        200,
        b"not a zip",
    );
    let e = gh
        .artifact_file(&repo(), 8, "results/suites.json")
        .unwrap_err();
    assert!(!e.is_not_found(), "{e}");
}
