//! `pr-edit` (REQ-CI-018): the check of an edited PR. Its title step is
//! `asdlc-pr-title.yml`'s as it ships, compared here, so `pr_title.rs`
//! covers it; its base step is run here as shipped, `gh` stubbed.

use std::collections::BTreeSet;

use crate::examples::keys;
use crate::harness::{Ran, Scratch, calls, path_with, run, run_body, step, steps, stub, workflow};

const EDIT: &str = ".github/workflows/asdlc-pr-edit.yml";
const TITLE: &str = ".github/workflows/asdlc-pr-title.yml";

#[test]
fn one_job_named_as_the_title_check_with_its_inputs() {
    let doc = workflow(EDIT);
    assert_eq!(
        keys(&doc["jobs"]),
        BTreeSet::from(["conventional-title".to_string()])
    );
    let job = &doc["jobs"]["conventional-title"];
    // Called from a job named `title`: the context the rulesets require.
    assert_eq!(job["name"].as_str(), Some("conventional-title"));
    // It reads the runs of the PR workflow and the PR's timeline, nothing else.
    assert_eq!(
        keys(&job["permissions"]),
        BTreeSet::from(["actions".to_string(), "pull-requests".to_string()])
    );
    for p in ["actions", "pull-requests"] {
        assert_eq!(job["permissions"][p].as_str(), Some("read"), "{p}");
    }
    // Every input of the title check, as it declares it, and the two of the base.
    let inputs = &doc["on"]["workflow_call"]["inputs"];
    let theirs = &workflow(TITLE)["on"]["workflow_call"]["inputs"];
    for name in keys(theirs) {
        for k in ["type", "required", "default"] {
            assert_eq!(
                inputs[name.as_str()][k],
                theirs[name.as_str()][k],
                "input {name}: {k}"
            );
        }
    }
    let mut want = keys(theirs);
    want.extend(["head-sha".to_string(), "pr-workflow".to_string()]);
    assert_eq!(keys(inputs), want);
    assert_eq!(inputs["pr-workflow"]["default"].as_str(), Some("pr.yml"));
    assert_eq!(
        doc["on"]["workflow_call"]["outputs"],
        workflow(TITLE)["on"]["workflow_call"]["outputs"]
    );
}

#[test]
fn the_title_step_is_the_title_check_as_it_ships_then_the_base() {
    let all: Vec<_> = steps(&workflow(EDIT)).into_iter().map(|(_, s)| s).collect();
    assert_eq!(all.len(), 2);
    assert_eq!(
        all[0],
        step(&workflow(TITLE), "conventional-title", "title")
    );
    assert_eq!(all[1]["id"].as_str(), Some("base"));
    assert_eq!(
        all[1]["env"]["BASE_FROM"].as_str(),
        Some("${{ github.event.changes.base.ref.from }}")
    );
}

/// The base step, run with a stubbed `gh`: `timeline` is the PR's last base
/// change (empty for none), `runs` the creation of the newest run of the PR
/// workflow on the head (empty for none); `fail` makes either read fail.
fn base(base_from: &str, timeline: &str, runs: &str, fail: &str) -> (Ran, Vec<String>) {
    let body = run_body(&step(&workflow(EDIT), "conventional-title", "base"));
    let dir = Scratch::new("edit-base");
    stub(
        &dir.0,
        "gh",
        &format!(
            r#"case "$*" in
  *graphql*) [ "{fail}" = timeline ] && exit 1; printf '%s\n' '{timeline}' ;;
  *actions/workflows/*) [ "{fail}" = runs ] && exit 1; printf '%s\n' '{runs}' ;;
  *) exit 2 ;;
esac"#
        ),
    );
    let path = path_with(&dir.0);
    let r = run(
        &body,
        &[
            ("PATH", &path),
            ("GH_TOKEN", "t"),
            ("REPO", "example-org/acme"),
            ("NUMBER", "7"),
            ("HEAD_SHA", "abc123"),
            ("BASE_REF", "release/1.2"),
            ("BASE_FROM", base_from),
            ("PR_WORKFLOW", "pr.yml"),
        ],
        &dir.0,
    );
    (r, calls(&dir.0, "gh"))
}

const CHANGED: &str = "2026-10-30T03:00:00Z";

#[test]
fn an_edit_that_moves_the_base_holds_the_pr() {
    let (r, gh) = base("main", "", "", "");
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(
        r.log.contains("::error title=The base changed::"),
        "{}",
        r.log
    );
    assert!(r.log.contains("from 'main' to 'release/1.2'"), "{}", r.log);
    assert!(gh.is_empty(), "{gh:?}");
}

#[test]
fn a_base_that_never_changed_passes() {
    let (r, gh) = base("", "", "", "");
    assert_eq!(r.code, 0, "{}", r.log);
    // One read, of the timeline (its query spans lines); no run is read.
    let gh = gh.join("\n");
    assert!(gh.starts_with("api graphql"), "{gh}");
    assert!(
        gh.contains("number=7") && gh.contains("owner=example-org"),
        "{gh}"
    );
    assert!(!gh.contains("actions/workflows"), "{gh}");
}

#[test]
fn a_base_changed_since_the_newest_pr_run_holds_the_pr_until_a_new_run() {
    // Changed after the newest run, or with no run at all: held, whatever
    // the edit (a body edit included).
    for runs in ["2026-10-30T02:59:59Z", ""] {
        let (r, _) = base("", CHANGED, runs, "");
        assert_eq!(r.code, 1, "{runs:?}: {}", r.log);
        assert!(
            r.log.contains("::error title=The base changed::"),
            "{}",
            r.log
        );
        assert!(r.log.contains("push a commit"), "{}", r.log);
    }
    // A run of the PR workflow since (a push, a reopen): the hold is lifted.
    for runs in [CHANGED, "2026-10-30T03:00:01Z"] {
        let (r, gh) = base("", CHANGED, runs, "");
        assert_eq!(r.code, 0, "{runs:?}: {}", r.log);
        assert!(
            gh.last().unwrap().contains(
                "repos/example-org/acme/actions/workflows/pr.yml/runs?head_sha=abc123&event=pull_request"
            ),
            "{gh:?}"
        );
    }
}

#[test]
fn an_unreadable_history_holds_the_pr() {
    for fail in ["timeline", "runs"] {
        let (r, _) = base("", CHANGED, CHANGED, fail);
        assert_eq!(r.code, 1, "{fail}: {}", r.log);
        assert!(
            r.log
                .contains("::error title=The base history is unreadable::"),
            "{}",
            r.log
        );
    }
}
