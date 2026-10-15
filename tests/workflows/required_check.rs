//! The `required-check` aggregator (REQ-CI-024, and the owner's decision of
//! 2026-10-07: only a job declared in `may-skip` may be skipped).

use crate::harness::{Ran, Scratch, run, run_body, step, workflow};

fn aggregate(needs: &str, may_skip: &str) -> Ran {
    aggregate_on(needs, may_skip, "false")
}

/// `draft` is what `github.event.pull_request.draft` gives the step.
fn aggregate_on(needs: &str, may_skip: &str, draft: &str) -> Ran {
    let doc = workflow("actions/required-check/action.yml");
    let body = run_body(&step(&doc, "runs", "Aggregate the gated jobs"));
    let dir = Scratch::new("required");
    run(
        &body,
        &[("NEEDS", needs), ("MAY_SKIP", may_skip), ("DRAFT", draft)],
        &dir.0,
    )
}

#[test]
fn a_draft_pr_never_passes_even_with_every_job_green() {
    let n = needs(&[("guards", "success"), ("test", "skipped")]);
    let r = aggregate_on(&n, "test", "true");
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("A draft PR is out of the flow"), "{}", r.log);
    // Marked ready, the same head passes on its ready_for_review run.
    let n = needs(&[("guards", "success"), ("test", "success")]);
    assert_eq!(aggregate_on(&n, "test", "false").code, 0);
    // Outside a pull request event the draft state is empty: not a draft.
    assert_eq!(aggregate_on(&n, "test", "").code, 0);
}

#[test]
fn the_action_reads_the_draft_state_of_the_pull_request_event() {
    let doc = workflow("actions/required-check/action.yml");
    let s = step(&doc, "runs", "Aggregate the gated jobs");
    assert_eq!(
        s["env"]["DRAFT"].as_str(),
        Some("${{ github.event.pull_request.draft }}")
    );
}

fn needs(pairs: &[(&str, &str)]) -> String {
    let inner: Vec<String> = pairs
        .iter()
        .map(|(n, r)| format!("\"{n}\": {{\"result\": \"{r}\", \"outputs\": {{}}}}"))
        .collect();
    format!("{{{}}}", inner.join(", "))
}

#[test]
fn every_job_green_passes() {
    let r = aggregate(&needs(&[("guards", "success"), ("test", "success")]), "");
    assert_eq!(r.code, 0, "{}", r.log);
}

#[test]
fn a_skipped_job_passes_only_when_declared() {
    let n = needs(&[("guards", "success"), ("test", "skipped")]);
    assert_eq!(aggregate(&n, "test").code, 0);
    let r = aggregate(&n, "");
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("not declared in may-skip"), "{}", r.log);
    // The guards themselves may never be skipped unless the caller says so.
    let r = aggregate(
        &needs(&[("guards", "skipped"), ("test", "skipped")]),
        "test",
    );
    assert_eq!(r.code, 1, "{}", r.log);
}

#[test]
fn a_failed_cancelled_empty_or_unknown_result_fails() {
    for result in ["failure", "cancelled", "", "neutral"] {
        let r = aggregate(&needs(&[("guards", "success"), ("test", result)]), "test");
        assert_eq!(r.code, 1, "{result:?}: {}", r.log);
    }
}

#[test]
fn no_results_or_a_may_skip_typo_never_passes() {
    for n in ["{}", "", "not json", "[]"] {
        let r = aggregate(n, "");
        assert_eq!(r.code, 1, "{n:?}: {}", r.log);
        assert!(r.log.contains("No job results supplied"), "{}", r.log);
    }
    let r = aggregate(&needs(&[("guards", "success")]), "tset");
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("not a job of needs"), "{}", r.log);
}
