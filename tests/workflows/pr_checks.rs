//! `pr-checks`: the title check and the guards in one job. Its steps are
//! those of `asdlc-pr-title.yml` and `asdlc-pr-guards.yml` as they ship,
//! compared here, so the tests of those two (`pr_title.rs`, `pr_guards.rs`)
//! cover them; what is its own is run here: the order of the checks, the
//! title's verdict, and the callers it names to refresh the process page.

use std::collections::BTreeSet;

use yaml_rust2::Yaml;

use crate::examples::keys;
use crate::harness::{Repo, Scratch, run, run_body, step, steps, workflow};

const CHECKS: &str = ".github/workflows/asdlc-pr-checks.yml";
const TITLE: &str = ".github/workflows/asdlc-pr-title.yml";
const GUARDS: &str = ".github/workflows/asdlc-pr-guards.yml";

/// The steps of the one job, in order.
fn job_steps() -> Vec<Yaml> {
    steps(&workflow(CHECKS))
        .into_iter()
        .map(|(job, s)| {
            assert_eq!(job, "checks");
            s
        })
        .collect()
}

/// A step without the keys `skip` names.
fn without(s: &Yaml, skip: &[&str]) -> Yaml {
    let mut h = s.as_hash().unwrap().clone();
    for k in skip {
        h.remove(&Yaml::String((*k).to_string()));
    }
    Yaml::Hash(h)
}

#[test]
fn one_job_with_the_union_of_the_inputs_and_the_outputs_of_both() {
    let doc = workflow(CHECKS);
    assert_eq!(keys(&doc["jobs"]), BTreeSet::from(["checks".to_string()]));
    let job = &doc["jobs"]["checks"];
    // The title check's own name: called from a job named `title`, the
    // context the rulesets require, `title / conventional-title`.
    assert_eq!(job["name"].as_str(), Some("conventional-title"));
    assert_eq!(
        job["if"].as_str(),
        Some("github.event.pull_request.draft != true"),
        "no runner on a draft, and no other condition"
    );
    assert_eq!(
        job["timeout-minutes"].as_str(),
        Some("${{ inputs.timeout-minutes }}")
    );
    assert_eq!(
        keys(&job["permissions"]),
        BTreeSet::from(["contents".into()])
    );
    assert_eq!(job["permissions"]["contents"].as_str(), Some("read"));

    let inputs = &doc["on"]["workflow_call"]["inputs"];
    let mut want = BTreeSet::new();
    for other in [TITLE, GUARDS] {
        let theirs = &workflow(other)["on"]["workflow_call"]["inputs"];
        for name in keys(theirs) {
            for k in ["type", "required", "default"] {
                assert_eq!(
                    inputs[name.as_str()][k],
                    theirs[name.as_str()][k],
                    "input {name}: {k} differs from {other}"
                );
            }
            want.insert(name);
        }
    }
    assert_eq!(keys(inputs), want, "the union of the inputs, nothing else");

    // The outputs of both, `needs.<job>.outputs.code` unchanged for callers.
    let outputs = &doc["on"]["workflow_call"]["outputs"];
    assert_eq!(
        keys(outputs),
        ["code", "detections", "skipped", "exempt"]
            .into_iter()
            .map(str::to_string)
            .collect::<BTreeSet<_>>()
    );
    for name in keys(outputs) {
        assert_eq!(
            outputs[name.as_str()]["value"].as_str(),
            Some(format!("${{{{ jobs.checks.outputs.{name} }}}}").as_str())
        );
    }
    let guards = &workflow(GUARDS)["jobs"]["guards"]["outputs"];
    for name in ["code", "detections", "skipped"] {
        assert_eq!(job["outputs"][name], guards[name], "{name}");
    }
    assert_eq!(
        job["outputs"]["exempt"],
        workflow(TITLE)["jobs"]["conventional-title"]["outputs"]["exempt"]
    );
}

#[test]
fn the_steps_are_the_title_check_then_the_guards_as_they_ship() {
    let all = job_steps();
    // First, the title check: the same script and environment, never
    // skipped, its failure held until the last step.
    let title = step(&workflow(TITLE), "conventional-title", "title");
    assert_eq!(without(&all[0], &["continue-on-error"]), title);
    assert_eq!(all[0]["continue-on-error"].as_bool(), Some(true));
    assert!(all[0]["if"].is_badvalue());

    // Then every step of the guards, in their order; the process check also
    // reads a caller of this workflow to say how to refresh the page.
    let guards: Vec<Yaml> = steps(&workflow(GUARDS))
        .into_iter()
        .map(|(_, s)| s)
        .collect();
    assert_eq!(all.len(), 1 + guards.len() + 1);
    for (mine, theirs) in all[1..=guards.len()].iter().zip(&guards) {
        let what = theirs["name"].as_str().unwrap();
        if theirs["id"].as_str() == Some("process") {
            assert_eq!(without(mine, &["run"]), without(theirs, &["run"]), "{what}");
            assert_eq!(
                run_body(mine),
                run_body(theirs)
                    .replace(r"asdlc-pr-guards\.yml@", r"asdlc-pr-(checks|guards)\.yml@"),
                "{what}"
            );
        } else {
            assert_eq!(mine, theirs, "{what}");
        }
    }

    // Last, the title's verdict, which nothing skips.
    let verdict = all.last().unwrap();
    assert!(verdict["if"].is_badvalue());
    assert!(verdict["continue-on-error"].is_badvalue());
    assert_eq!(
        verdict["env"]["OUTCOME"].as_str(),
        Some("${{ steps.title.outcome }}")
    );
}

#[test]
fn the_job_fails_unless_the_title_check_succeeded() {
    let body = run_body(job_steps().last().unwrap());
    let dir = Scratch::new("verdict");
    let ok = run(&body, &[("OUTCOME", "success")], &dir.0);
    assert_eq!(ok.code, 0, "{}", ok.log);
    for outcome in ["failure", "cancelled", "skipped", ""] {
        let r = run(&body, &[("OUTCOME", outcome)], &dir.0);
        assert_eq!(r.code, 1, "{outcome:?}: {}", r.log);
        assert!(
            r.log
                .contains("::error title=PR title is not a Conventional Commit::"),
            "{}",
            r.log
        );
    }
}

#[test]
fn a_refused_page_names_the_ref_of_a_caller_of_this_workflow() {
    let s = step(&workflow(CHECKS), "checks", "process");
    let blob = s["env"]["PINNED_BLOB"].as_str().unwrap().to_string();
    let repo = Repo::new();
    repo.write(
        ".github/workflows/pr.yml",
        "jobs:\n  guards:\n    name: title\n    uses: example-org/spl-asdlc/.github/workflows/asdlc-pr-checks.yml@v1.5\n",
    );
    repo.write(".spl-asdlc/process.md", "edited\n");
    let head = repo.commit("a caller and an edited page");
    let r = run(
        &run_body(&s),
        &[
            ("HEAD_SHA", &head),
            ("PAGE", ".spl-asdlc/process.md"),
            ("PINNED_BLOB", &blob),
        ],
        &repo.dir.0,
    );
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(
        r.log.contains(
            r#"gh api "repos/example-org/spl-asdlc/contents/docs/agents/process.md?ref=v1.5""#
        ),
        "{}",
        r.log
    );
}
