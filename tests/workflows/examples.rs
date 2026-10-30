//! The example callers of `docs/setup/examples/` (REQ-CI-001, 002, 005):
//! each one parses, calls a reusable workflow or action of this repository
//! with only the inputs and secrets it declares, every required one given,
//! grants the permissions its called jobs ask for, pins every other action
//! by commit, and keeps expressions out of its scripts. The checks of a call
//! are shared with this repository's own callers (`own.rs`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use regex::Regex;
use yaml_rust2::Yaml;

use crate::harness::{load, relative, root, run_body, steps, workflow};

const PREFIX: &str = "<owner>/spl-asdlc/";

/// The workflows `asdlc-pr-checks.yml` runs in one job, kept unchanged for
/// the projects whose callers still name them.
const SUPERSEDED: [&str; 2] = [
    ".github/workflows/asdlc-pr-guards.yml",
    ".github/workflows/asdlc-pr-title.yml",
];

fn examples() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(root().join("docs/setup/examples"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| matches!(p.extension().and_then(|x| x.to_str()), Some("yml" | "yaml")))
        .collect();
    out.sort();
    out
}

fn example(name: &str) -> Yaml {
    load(&root().join("docs/setup/examples").join(name))
}

pub(crate) fn keys(y: &Yaml) -> BTreeSet<String> {
    y.as_hash()
        .map(|h| {
            h.keys()
                .filter_map(|k| k.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn jobs(doc: &Yaml) -> Vec<(String, Yaml)> {
    doc["jobs"]
        .as_hash()
        .expect("jobs")
        .iter()
        .map(|(k, v)| (k.as_str().unwrap().to_string(), v.clone()))
        .collect()
}

/// `<owner>/spl-asdlc/<path>@v1` -> `<path>`, refusing any other shape.
fn local(uses: &str, what: &str) -> String {
    let shape = Regex::new(
        r"^<owner>/spl-asdlc/(\.github/workflows/asdlc-[a-z-]+\.yml|actions/[a-z-]+)@v1$",
    )
    .unwrap();
    let caps = shape
        .captures(uses)
        .unwrap_or_else(|| panic!("{what}: `{uses}` is not <owner>/spl-asdlc/<file>@v1"));
    caps[1].to_string()
}

/// The inputs a called unit declares: name -> required.
fn declared(inputs: &Yaml) -> BTreeMap<String, bool> {
    inputs
        .as_hash()
        .map(|h| {
            h.iter()
                .map(|(k, v)| {
                    (
                        k.as_str().unwrap().to_string(),
                        v["required"].as_bool().unwrap_or(false),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn check_with(given: &BTreeSet<String>, declared: &BTreeMap<String, bool>, what: &str) {
    for k in given {
        assert!(
            declared.contains_key(k),
            "{what}: input `{k}` is not declared by the called unit"
        );
    }
    for (k, required) in declared {
        if *required {
            assert!(
                given.contains(k),
                "{what}: required input `{k}` is not given"
            );
        }
    }
}

#[test]
fn every_example_parses_and_is_a_caller() {
    let all = examples();
    assert!(all.len() >= 5, "{all:?}");
    for f in &all {
        let doc = load(f);
        assert!(
            !crate::harness::is_reusable(&doc),
            "{}: an example is a caller, never a reusable workflow",
            relative(f)
        );
        assert!(!doc["on"].is_badvalue(), "{}: no trigger", relative(f));
        assert!(
            !std::fs::read_to_string(f)
                .unwrap()
                .contains("secrets: inherit"),
            "{}: secrets: inherit (REQ-CI-002)",
            relative(f)
        );
    }
}

#[test]
fn every_reusable_workflow_and_action_has_an_example_caller() {
    let mut called = BTreeSet::new();
    for f in examples() {
        let doc = load(&f);
        for (_, job) in jobs(&doc) {
            if let Some(u) = job["uses"].as_str() {
                called.insert(local(u, &relative(&f)));
            }
        }
        for (_, s) in steps(&doc) {
            if let Some(u) = s["uses"].as_str().filter(|u| u.starts_with(PREFIX)) {
                called.insert(local(u, &relative(&f)));
            }
        }
    }
    for f in crate::harness::files() {
        let rel = relative(&f);
        // The two workflows asdlc-pr-checks.yml replaced stay for the
        // callers that name them; no example calls them any more.
        if SUPERSEDED.contains(&rel.as_str()) {
            continue;
        }
        let unit = if rel.starts_with(".github/workflows/asdlc-") {
            rel
        } else if let Some(dir) = rel.strip_prefix("actions/") {
            format!("actions/{}", dir.split('/').next().unwrap())
        } else {
            continue;
        };
        assert!(called.contains(&unit), "no example calls {unit}");
    }
}

/// How a caller names a unit of this repository: the `uses:` value and a
/// label for messages -> the unit's path (a workflow file or an action
/// directory). It panics on any other shape.
pub(crate) type Resolve = fn(&str, &str) -> String;

/// Each job of `files` that calls a workflow gives it only the inputs and
/// secrets it declares, every required one, and each secret by name.
pub(crate) fn check_called_workflows(files: &[PathBuf], resolve: Resolve) {
    for f in files {
        let doc = load(f);
        for (id, job) in jobs(&doc) {
            let Some(uses) = job["uses"].as_str() else {
                continue;
            };
            let what = format!("{} job {id}", relative(f));
            let called = workflow(&resolve(uses, &what));
            let call = &called["on"]["workflow_call"];
            assert!(!call.is_badvalue(), "{what}: {uses} is not callable");
            check_with(&keys(&job["with"]), &declared(&call["inputs"]), &what);

            let secrets = &call["secrets"];
            let given = keys(&job["secrets"]);
            let declared = declared(secrets);
            for s in &given {
                assert!(
                    declared.contains_key(s),
                    "{what}: secret `{s}` is not declared"
                );
            }
            for (s, required) in &declared {
                if *required {
                    assert!(
                        given.contains(s),
                        "{what}: required secret `{s}` is not passed"
                    );
                }
            }
            // By name, from the caller's own secret of the same name.
            if let Some(h) = job["secrets"].as_hash() {
                for (k, v) in h {
                    let k = k.as_str().unwrap();
                    assert_eq!(
                        v.as_str(),
                        Some(format!("${{{{ secrets.{k} }}}}").as_str()),
                        "{what}: secret {k}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_called_workflow_gets_only_declared_inputs_and_every_required_one() {
    check_called_workflows(&examples(), local);
}

/// Each step of `files` that uses an action of this repository (`ours`)
/// gives it only the inputs it declares, every required one.
pub(crate) fn check_called_actions(files: &[PathBuf], ours: fn(&str) -> bool, resolve: Resolve) {
    for f in files {
        for (job, s) in steps(&load(f)) {
            let Some(uses) = s["uses"].as_str().filter(|u| ours(u)) else {
                continue;
            };
            let what = format!("{} job {job}", relative(f));
            let dir = resolve(uses, &what);
            assert!(dir.starts_with("actions/"), "{what}: a step uses {dir}");
            let action = workflow(&format!("{dir}/action.yml"));
            check_with(&keys(&s["with"]), &declared(&action["inputs"]), &what);
        }
    }
}

#[test]
fn a_called_action_gets_only_declared_inputs_and_every_required_one() {
    check_called_actions(&examples(), |u| u.starts_with(PREFIX), local);
}

/// `none` < `read` < `write`.
fn level(v: &str) -> u8 {
    match v {
        "read" => 1,
        "write" => 2,
        _ => 0,
    }
}

fn grants(p: &Yaml, what: &str) -> BTreeMap<String, u8> {
    let h = p
        .as_hash()
        .unwrap_or_else(|| panic!("{what}: permissions must be a mapping"));
    h.iter()
        .map(|(k, v)| (k.as_str().unwrap().to_string(), level(v.as_str().unwrap())))
        .collect()
}

/// Each file of `files` sets top-level permissions, each job states its
/// own, and a job that calls a workflow grants every permission the called
/// jobs ask for.
pub(crate) fn check_permissions(files: &[PathBuf], resolve: Resolve) {
    for f in files {
        let doc = load(f);
        assert!(
            !doc["permissions"].is_badvalue(),
            "{}: no top-level permissions (REQ-CI-005)",
            relative(f)
        );
        for (id, job) in jobs(&doc) {
            let what = format!("{} job {id}", relative(f));
            assert!(
                !job["permissions"].is_badvalue(),
                "{what}: permissions not stated"
            );
            let Some(uses) = job["uses"].as_str() else {
                continue;
            };
            let have = grants(&job["permissions"], &what);
            let called = workflow(&resolve(uses, &what));
            // GitHub refuses at startup a called job asking more than its
            // caller job holds, whatever that job's `if:` says.
            let mut asked = vec![("top level".to_string(), called["permissions"].clone())];
            for (cid, cjob) in jobs(&called) {
                asked.push((cid, cjob["permissions"].clone()));
            }
            for (cid, p) in asked {
                for (scope, need) in grants(&p, &format!("{uses} {cid}")) {
                    let got = have.get(&scope).copied().unwrap_or(0);
                    assert!(
                        got >= need,
                        "{what}: the called job {cid} asks `{scope}` at level {need}, the caller grants {got}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_caller_grants_every_permission_its_called_jobs_ask_for() {
    check_permissions(&examples(), local);
}

/// Every action of `files` but this repository's own (`ours`) is pinned by
/// commit, with its version in a comment, at the commit this repository's
/// workflows and actions pin it to.
pub(crate) fn check_pins(files: &[PathBuf], ours: fn(&str) -> bool) {
    let pinned = Regex::new(r"^([^@\s]+)@([0-9a-f]{40})$").unwrap();
    // The pins of this repository's own workflows.
    let mut pins: BTreeMap<String, String> = BTreeMap::new();
    for f in crate::harness::files() {
        for (_, s) in steps(&load(&f)) {
            if let Some(c) = s["uses"].as_str().and_then(|u| pinned.captures(u)) {
                if let Some(sha) = pins.get(&c[1]) {
                    assert_eq!(
                        sha.as_str(),
                        &c[2],
                        "{}: {} is pinned elsewhere here",
                        relative(&f),
                        &c[1]
                    );
                }
                pins.insert(c[1].to_string(), c[2].to_string());
            }
        }
    }
    let commented = Regex::new(r"uses:\s*\S+@[0-9a-f]{40} # v?\S+").unwrap();
    for f in files {
        let text = std::fs::read_to_string(f).unwrap();
        for (job, s) in steps(&load(f)) {
            let Some(uses) = s["uses"].as_str() else {
                continue;
            };
            if ours(uses) {
                continue;
            }
            let what = format!("{} ({job})", relative(f));
            let c = pinned
                .captures(uses)
                .unwrap_or_else(|| panic!("{what}: `{uses}` is not pinned to a commit"));
            if let Some(sha) = pins.get(&c[1]) {
                assert_eq!(
                    &c[2],
                    sha.as_str(),
                    "{what}: {} is pinned elsewhere here",
                    &c[1]
                );
            }
            let line = text
                .lines()
                .find(|l| l.contains(uses))
                .expect("the pin is on one line");
            assert!(
                commented.is_match(line),
                "{what}: no version comment: {line}"
            );
        }
    }
}

#[test]
fn every_other_action_is_pinned_by_commit_as_this_repository_pins_it() {
    check_pins(&examples(), |u| u.starts_with(PREFIX));
}

#[test]
fn no_example_expands_an_expression_inside_a_script() {
    for f in examples() {
        for (job, s) in steps(&load(&f)) {
            if s["run"].is_badvalue() {
                continue;
            }
            assert!(
                !run_body(&s).contains("${{"),
                "{} ({job}): an expression inside run: (REQ-CI-003); pass it through env:",
                relative(&f)
            );
        }
    }
}

/// The callers of a project's setup PR (`spl_asdlc::setup::files`),
/// written to a directory of their own: each passes the checks of an
/// example, its method's repository and the embedded minor line in place
/// of `<owner>/spl-asdlc` and `@v1`.
fn generated() -> Vec<PathBuf> {
    use spl_asdlc::config::TrackerKind;
    use spl_asdlc::setup::{Answers, ReviewAngle, files};
    let pr = files(&Answers {
        product: "acme".into(),
        repository: "example-org/acme".into(),
        tracker: TrackerKind::Livela,
        review_angles: vec![ReviewAngle {
            key: "security".into(),
            paths: vec![],
        }],
        release_angles: vec!["security".into()],
        test_command: "cargo test --locked".into(),
        app_slug: "example-app".into(),
        method_repository: Some("example-org/spl-asdlc".into()),
    })
    .unwrap();
    let dir = std::env::temp_dir().join(format!("spl-asdlc-setup-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let line = format!(
        "@{}",
        spl_asdlc::embed::minor_line(spl_asdlc::embed::VERSION)
    );
    let mut out = Vec::new();
    for f in pr
        .files
        .iter()
        .filter(|f| f.path.starts_with(".github/workflows/"))
    {
        let mut text = String::new();
        for l in f.content.lines() {
            let l = if l.contains("example-org/spl-asdlc/") {
                assert!(l.ends_with(&line), "{}: {l}", f.path);
                format!("{}@v1", &l[..l.len() - line.len()])
            } else {
                l.to_string()
            };
            text.push_str(&l.replace("example-org/spl-asdlc", "<owner>/spl-asdlc"));
            text.push('\n');
        }
        let path = dir.join(f.path.trim_start_matches(".github/workflows/"));
        std::fs::write(&path, text).unwrap();
        out.push(path);
    }
    assert_eq!(out.len(), 6);
    out
}

#[test]
fn the_generated_setup_callers_pass_the_checks_of_an_example() {
    let all = generated();
    for f in &all {
        let doc = load(f);
        assert!(!crate::harness::is_reusable(&doc), "{}", relative(f));
        for (job, s) in steps(&doc) {
            if !s["run"].is_badvalue() {
                assert!(!run_body(&s).contains("${{"), "{} ({job})", relative(f));
            }
        }
    }
    check_called_workflows(&all, local);
    check_called_actions(&all, |u| u.starts_with(PREFIX), local);
    check_permissions(&all, local);
    check_pins(&all, |u| u.starts_with(PREFIX));
}

// ---- per caller ----

/// The condition of a job that never runs on a draft PR.
pub(crate) const NOT_A_DRAFT: &str = "github.event.pull_request.draft == false";

/// The events of the PR workflow: never `edited`. GitHub reads a required
/// context in the newest check suite of each workflow on the head, so a run
/// on an edit of the body, whose jobs would be skipped, would hide the last
/// full run and block the PR (ASDLC-76ZK): the edit workflow checks an edit.
pub(crate) const PR_EVENTS: [&str; 4] = ["opened", "synchronize", "reopened", "ready_for_review"];

/// The events of the edit workflow: those that keep the head. An edit is
/// checked there; a reopen and a PR marked ready refresh its verdict on the
/// head, where the PR workflow ran again.
pub(crate) const EDIT_EVENTS: [&str; 3] = ["edited", "reopened", "ready_for_review"];

fn types(doc: &Yaml) -> Vec<&str> {
    doc["on"]["pull_request"]["types"]
        .as_vec()
        .unwrap()
        .iter()
        .filter_map(Yaml::as_str)
        .collect()
}

/// One job of a PR workflow runs every method check (asdlc-pr-checks.yml):
/// its id is `guards`, its name `title`, so that it reports the title
/// context the rulesets require, `title / conventional-title`. The workflow
/// never runs on an edit, and its concurrency group is the ref's and the
/// draft state's, nothing else: no run of its own for an edit.
pub(crate) fn check_the_method_checks(doc: &Yaml, uses: &str, what: &str) {
    assert_eq!(types(doc), PR_EVENTS, "{what}: no run on an edit");
    let guards = &doc["jobs"]["guards"];
    assert_eq!(guards["uses"].as_str(), Some(uses), "{what}");
    assert_eq!(guards["name"].as_str(), Some("title"), "{what}");
    assert_eq!(
        guards["with"]["title"].as_str(),
        Some("${{ github.event.pull_request.title }}"),
        "{what}"
    );
    assert_eq!(guards["if"].as_str(), Some(NOT_A_DRAFT), "{what}");
    for (id, job) in jobs(doc) {
        let cond = job["if"].as_str().unwrap_or_default();
        assert!(
            !cond.contains("github.event.action") && !cond.contains("github.event.changes"),
            "{what}: job {id} reads the event's action: if: {cond:?}"
        );
    }
    let aggregator = &doc["jobs"]["required-check"];
    assert!(
        aggregator["name"].is_badvalue() || aggregator["name"].as_str() == Some("required-check"),
        "{what}: required-check is reported under its own name"
    );
    assert_eq!(
        doc["concurrency"]["group"].as_str(),
        Some("pr-${{ github.ref }}-${{ github.event.pull_request.draft }}"),
        "{what}: REQ-CI-023"
    );
    assert_eq!(
        doc["concurrency"]["cancel-in-progress"].as_bool(),
        Some(true)
    );
}

/// The edit workflow beside it: one job, `title`, calling asdlc-pr-edit.yml
/// with the title the PR has now, on the bases of the PR workflow, on the
/// events that keep the head, never on a draft and on no other condition.
/// It reports no `required-check`: that one is the PR workflow's alone.
pub(crate) fn check_the_edit_workflow(edit: &Yaml, pr: &Yaml, uses: &str, what: &str) {
    assert_eq!(
        keys(&edit["on"]),
        BTreeSet::from(["pull_request".to_string()]),
        "{what}"
    );
    let trigger = &edit["on"]["pull_request"];
    assert!(
        trigger["paths"].is_badvalue() && trigger["paths-ignore"].is_badvalue(),
        "{what}: REQ-CI-022"
    );
    assert_eq!(
        trigger["branches"], pr["on"]["pull_request"]["branches"],
        "{what}"
    );
    assert_eq!(types(edit), EDIT_EVENTS, "{what}");
    assert_eq!(
        keys(&edit["jobs"]),
        BTreeSet::from(["title".to_string()]),
        "{what}"
    );
    let job = &edit["jobs"]["title"];
    assert_eq!(job["name"].as_str(), Some("title"), "{what}");
    assert_eq!(job["uses"].as_str(), Some(uses), "{what}");
    assert_eq!(job["if"].as_str(), Some(NOT_A_DRAFT), "{what}");
    for (input, value) in [
        ("title", "${{ github.event.pull_request.title }}"),
        ("head-ref", "${{ github.event.pull_request.head.ref }}"),
        ("base-ref", "${{ github.event.pull_request.base.ref }}"),
        ("head-sha", "${{ github.event.pull_request.head.sha }}"),
    ] {
        assert_eq!(job["with"][input].as_str(), Some(value), "{what}: {input}");
    }
    // Its title check takes the PR workflow's types.
    assert_eq!(
        job["with"]["allowed-types"], pr["jobs"]["guards"]["with"]["allowed-types"],
        "{what}"
    );
    assert!(
        job["with"]["pr-workflow"].is_badvalue(),
        "{what}: pr.yml, the default"
    );
    assert_eq!(
        edit["concurrency"]["group"].as_str(),
        Some("pr-edit-${{ github.ref }}"),
        "{what}"
    );
    assert_eq!(
        edit["concurrency"]["cancel-in-progress"].as_bool(),
        Some(true)
    );
}

#[test]
fn one_job_runs_the_method_checks_and_an_edit_runs_the_edit_workflow() {
    let pr = example("pr.yml");
    check_the_method_checks(
        &pr,
        "<owner>/spl-asdlc/.github/workflows/asdlc-pr-checks.yml@v1",
        "the example pr.yml",
    );
    check_the_edit_workflow(
        &example("pr-edit.yml"),
        &pr,
        "<owner>/spl-asdlc/.github/workflows/asdlc-pr-edit.yml@v1",
        "the example pr-edit.yml",
    );
    // No example calls the two workflows it replaced, nor has a title
    // workflow of its own.
    assert!(!root().join("docs/setup/examples/pr-title.yml").exists());
    for f in examples() {
        let text = std::fs::read_to_string(&f).unwrap();
        for old in SUPERSEDED {
            let name = old.rsplit('/').next().unwrap();
            assert!(!text.contains(name), "{}: {name}", relative(&f));
        }
    }
}

/// No job of a PR workflow runs on a draft PR, `required-check` included
/// (the owner's decision, 2026-10-15): a draft costs no runner minute. Each
/// job's `if:` is the draft condition, alone or first of an `&&`, and nothing
/// can `||` it away; `required-check`'s is `always() &&` it.
pub(crate) fn check_no_job_runs_on_a_draft(doc: &Yaml, what: &str) {
    for (id, job) in jobs(doc) {
        let cond = job["if"].as_str().unwrap_or_default();
        if id == "required-check" {
            assert_eq!(cond, format!("always() && {NOT_A_DRAFT}"), "{what}: {id}");
            continue;
        }
        assert!(
            cond == NOT_A_DRAFT || cond.starts_with(&format!("{NOT_A_DRAFT} && ")),
            "{what}: job {id} runs on a draft: if: {cond:?}"
        );
        assert!(!cond.contains("||"), "{what}: job {id}: if: {cond:?}");
    }
}

#[test]
fn the_pr_workflow_is_unfiltered_and_aggregates_every_job() {
    let pr = example("pr.yml");
    let trigger = &pr["on"]["pull_request"];
    assert!(trigger["paths"].is_badvalue() && trigger["paths-ignore"].is_badvalue());
    let bases: Vec<&str> = trigger["branches"]
        .as_vec()
        .unwrap()
        .iter()
        .filter_map(Yaml::as_str)
        .collect();
    assert_eq!(bases, ["main", "release/**"], "REQ-CI-022, REQ-CI-034");
    let types: Vec<&str> = trigger["types"]
        .as_vec()
        .unwrap()
        .iter()
        .filter_map(Yaml::as_str)
        .collect();
    assert!(
        types.contains(&"ready_for_review"),
        "a PR marked ready runs the jobs a draft does not run, required-check included: {types:?}"
    );
    assert!(
        pr["concurrency"]["group"]
            .as_str()
            .unwrap()
            .contains("github.event.pull_request.draft"),
        "REQ-CI-023"
    );

    check_no_job_runs_on_a_draft(&pr, "the example pr.yml");

    let aggregator = &pr["jobs"]["required-check"];
    assert_eq!(aggregator["runs-on"].as_str(), Some("ubuntu-latest"));
    let needs: BTreeSet<String> = aggregator["needs"]
        .as_vec()
        .unwrap()
        .iter()
        .map(|n| n.as_str().unwrap().to_string())
        .collect();
    let others: BTreeSet<String> = jobs(&pr)
        .into_iter()
        .map(|(id, _)| id)
        .filter(|id| id != "required-check")
        .collect();
    assert_eq!(
        needs, others,
        "every job is a need of required-check (REQ-CI-025)"
    );
    let step = &aggregator["steps"][0];
    assert_eq!(step["with"]["needs"].as_str(), Some("${{ toJSON(needs) }}"));
    for skip in step["with"]["may-skip"].as_str().unwrap().split(',') {
        assert!(needs.contains(skip.trim()), "may-skip names {skip}");
    }
}

#[test]
fn the_gate_reruns_the_guards_the_pr_runs() {
    // REQ-CI-033: a guard that runs on PRs also runs on the candidate.
    let pr = example("pr.yml");
    let gate = example("release-gate.yml");
    assert_eq!(
        pr["jobs"]["guards"]["with"]["guard-commands"].as_str(),
        gate["jobs"]["gate"]["with"]["guard-commands"].as_str()
    );
}

/// The configuration template with every layer added as its comments say
/// (`tests/support/template.rs`): the template is light as written, and the
/// release examples are the callers of the full configuration.
fn template() -> spl_asdlc::config::Config {
    spl_asdlc::config::Config::parse(&crate::template::full()).unwrap()
}

#[test]
fn the_callers_pass_the_tag_patterns_of_the_configuration_template() {
    let c = template();
    let mut seen = 0;
    for f in examples() {
        for (id, job) in jobs(&load(&f)) {
            let with = &job["with"];
            for (input, want) in [
                ("candidate-tag", &c.release.candidate_tag),
                ("release-tag-pattern", &c.release.release_tag),
            ] {
                if let Some(v) = with[input].as_str() {
                    assert_eq!(v, want, "{} job {id}: {input}", relative(&f));
                    seen += 1;
                }
            }
            // `release-tag` is the pattern for cut and gate, the tag itself
            // for publish.
            if !job["uses"]
                .as_str()
                .unwrap_or("")
                .contains("release-publish")
            {
                if let Some(v) = with["release-tag"].as_str() {
                    assert_eq!(v, c.release.release_tag, "{} job {id}", relative(&f));
                }
            }
        }
    }
    assert!(seen >= 4, "{seen}");
    // The gate starts on the push of a candidate tag of that pattern.
    let gate = example("release-gate.yml");
    let filter = gate["on"]["push"]["tags"][0].as_str().unwrap();
    let glob = c
        .release
        .candidate_tag
        .replace("{version}", "*")
        .replace("{rc}", "*");
    assert_eq!(filter, glob);
    assert!(gate["on"]["push"]["branches"].is_badvalue());
}

#[test]
fn the_cut_is_dispatched_with_one_input() {
    // The bridge dispatches it with `action` only: every other input has a default.
    let cut = example("release-cut.yml");
    let inputs = cut["on"]["workflow_dispatch"]["inputs"].as_hash().unwrap();
    for (k, v) in inputs {
        let k = k.as_str().unwrap();
        if k == "action" {
            let options: Vec<&str> = v["options"]
                .as_vec()
                .unwrap()
                .iter()
                .filter_map(Yaml::as_str)
                .collect();
            assert_eq!(options, ["open", "candidate"]);
        } else {
            assert!(!v["default"].is_badvalue(), "input {k} has no default");
            assert_ne!(v["required"].as_bool(), Some(true), "input {k} is required");
        }
    }
    check_cut_waits_for_no_workflow(&cut);
}

/// The cut caller's one trigger, the dispatch: `open` prepares the release
/// and tags its first candidate in the same run, so nothing follows another
/// workflow's run (REQ-CI-042), and the binary it runs is a published one.
pub(crate) fn check_cut_waits_for_no_workflow(cut: &Yaml) {
    assert_eq!(
        keys(&cut["on"]),
        BTreeSet::from(["workflow_dispatch".to_string()])
    );
    assert_eq!(cut["concurrency"]["group"].as_str(), Some("release-cut"));
    assert_eq!(
        cut["concurrency"]["cancel-in-progress"].as_bool(),
        Some(false)
    );
    let job = &cut["jobs"]["release"];
    assert!(job["if"].is_badvalue(), "the dispatch alone runs it");
    assert_eq!(job["with"]["action"].as_str(), Some("${{ inputs.action }}"));
    assert_eq!(
        job["with"]["dry-run"].as_str(),
        Some("${{ inputs.dry_run == true }}")
    );
    assert_eq!(
        job["with"]["spl-asdlc-repository"].as_str(),
        Some("<owner>/spl-asdlc")
    );
    assert_eq!(job["with"]["spl-asdlc-version"].as_str(), Some("latest"));
}

#[test]
fn the_auto_rerun_follows_the_gate_by_name() {
    let rerun = example("release-gate-auto-rerun.yml");
    let gate = example("release-gate.yml");
    let followed = rerun["on"]["workflow_run"]["workflows"][0].as_str();
    assert_eq!(followed, gate["name"].as_str());
    let types: Vec<&str> = rerun["on"]["workflow_run"]["types"]
        .as_vec()
        .unwrap()
        .iter()
        .filter_map(Yaml::as_str)
        .collect();
    assert_eq!(types, ["completed"]);
    // The gate's suites job, as the rerun looks it up: `<caller job> / suites`.
    assert!(!gate["jobs"]["gate"]["uses"].is_badvalue());
}

#[test]
fn the_publication_calls_the_two_stages_and_always_closes() {
    let publish = example("release-publish.yml");
    let mut stages = Vec::new();
    for (id, job) in jobs(&publish) {
        if let Some(stage) = job["with"]["stage"].as_str() {
            stages.push((id.clone(), stage.to_string()));
        }
    }
    let ids: Vec<(&str, &str)> = stages
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    assert_eq!(ids, [("verify", "verify"), ("close", "close")]);
    let close = &publish["jobs"]["close"];
    assert_eq!(close["if"].as_str(), Some("${{ always() }}"));
    assert_eq!(
        close["with"]["publish-result"].as_str(),
        Some("${{ needs.publish.result }}")
    );
    // The App writes the merge back.
    assert_eq!(
        keys(&publish["jobs"]["close"]["secrets"]),
        BTreeSet::from([
            "SPL_ASDLC_APP_ID".to_string(),
            "SPL_ASDLC_APP_PRIVATE_KEY".to_string()
        ])
    );
    assert_eq!(
        strings(&publish["jobs"]["close"]["needs"]),
        ["verify", "publish"]
    );
    // The latest candidate's own QA reports, as the configuration renders them.
    let release = template().release;
    let verify = &publish["jobs"]["verify"]["with"];
    assert_eq!(verify["qa-record"].as_str(), Some(release.record.as_str()));
    let angles: Vec<String> =
        serde_json::from_str(verify["qa-angles"].as_str().expect("qa-angles")).unwrap();
    let keys_of: Vec<String> = release.angles.iter().map(|a| a.key.clone()).collect();
    assert_eq!(angles, keys_of);
    // The project's job sits between verify and the close.
    let own = &publish["jobs"]["publish"];
    assert_eq!(own["needs"].as_str(), Some("verify"));
    assert!(own["uses"].is_badvalue());
    // The replay: a dispatch input, honoured only on a dispatch, never verified.
    assert!(!publish["on"]["workflow_dispatch"]["inputs"]["replay"].is_badvalue());
    assert!(
        publish["jobs"]["verify"]["if"]
            .as_str()
            .unwrap()
            .contains("inputs.replay")
    );
    assert_eq!(
        keys(&publish["on"]),
        BTreeSet::from(["release".to_string(), "workflow_dispatch".to_string()])
    );
}

fn strings(y: &Yaml) -> Vec<String> {
    y.as_vec()
        .unwrap()
        .iter()
        .filter_map(Yaml::as_str)
        .map(str::to_string)
        .collect()
}

#[test]
fn the_method_has_no_release_please_example() {
    // The cycle prepares its version and notes itself (spec 05 §1): no
    // example caller runs release-please, and none follows its runs.
    for f in examples() {
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(!text.contains("release-please"), "{}", relative(&f));
        assert!(
            load(&f)["on"]["workflow_run"]["workflows"]
                .as_vec()
                .is_none_or(|w| w.iter().all(|x| x.as_str() != Some("release-please"))),
            "{}",
            relative(&f)
        );
    }
}
