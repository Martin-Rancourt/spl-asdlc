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
    assert!(all.len() >= 7, "{all:?}");
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

// ---- per caller ----

#[test]
fn the_title_check_is_its_own_workflow_and_reruns_on_edits() {
    let title = example("pr-title.yml");
    let types: Vec<&str> = title["on"]["pull_request"]["types"]
        .as_vec()
        .unwrap()
        .iter()
        .filter_map(Yaml::as_str)
        .collect();
    assert!(types.contains(&"edited"), "REQ-CI-018");
    for (id, job) in jobs(&title) {
        assert!(job["if"].is_badvalue(), "job {id} has an if: (REQ-CI-018)");
    }
    let pr = example("pr.yml");
    for (_, job) in jobs(&pr) {
        assert_ne!(
            job["uses"].as_str().map(|u| u.contains("asdlc-pr-title")),
            Some(true),
            "the title check is not a job of pr.yml (REQ-CI-018, REQ-CI-021)"
        );
    }
    assert_ne!(
        title["concurrency"]["group"].as_str(),
        pr["concurrency"]["group"].as_str()
    );
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
    assert!(
        pr["concurrency"]["group"]
            .as_str()
            .unwrap()
            .contains("github.event.pull_request.draft"),
        "REQ-CI-023"
    );

    let aggregator = &pr["jobs"]["required-check"];
    assert_eq!(aggregator["name"].as_str(), Some("required-check"));
    assert_eq!(aggregator["if"].as_str(), Some("always()"));
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

fn template() -> spl_asdlc::config::Config {
    let text = std::fs::read_to_string(root().join("docs/setup/templates/spl-asdlc.toml")).unwrap();
    spl_asdlc::config::Config::parse(&text).unwrap()
}

#[test]
fn the_callers_pass_the_tag_patterns_of_the_configuration_template() {
    let c = template();
    let mut seen = 0;
    for f in examples() {
        for (id, job) in jobs(&load(&f)) {
            let with = &job["with"];
            for (input, want) in [
                ("candidate-tag", &c.release.as_ref().unwrap().candidate_tag),
                (
                    "release-tag-pattern",
                    &c.release.as_ref().unwrap().release_tag,
                ),
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
                    assert_eq!(
                        v,
                        c.release.as_ref().unwrap().release_tag,
                        "{} job {id}",
                        relative(&f)
                    );
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
        .as_ref()
        .unwrap()
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
    assert_eq!(cut["on"].as_hash().unwrap().len(), 1, "dispatch only");
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
fn the_publication_calls_the_three_stages_and_always_closes() {
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
    assert_eq!(
        ids,
        [
            ("verify", "verify"),
            ("record-deployed", "record-deployed"),
            ("close", "close")
        ]
    );
    let close = &publish["jobs"]["close"];
    assert_eq!(close["if"].as_str(), Some("${{ always() }}"));
    assert_eq!(
        close["with"]["publish-result"].as_str(),
        Some("${{ needs.publish.result }}")
    );
    // The App writes the ledger and the merge back.
    for id in ["record-deployed", "close"] {
        assert_eq!(
            keys(&publish["jobs"][id]["secrets"]),
            BTreeSet::from([
                "SPL_ASDLC_APP_ID".to_string(),
                "SPL_ASDLC_APP_PRIVATE_KEY".to_string()
            ]),
            "{id}"
        );
    }
    // The project's job sits between verify and the ledger.
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

#[test]
fn release_please_runs_on_release_branches_only_as_the_app() {
    let rp = example("release-please.yml");
    assert_eq!(
        keys(&rp["on"]),
        BTreeSet::from(["push".to_string(), "workflow_dispatch".to_string()])
    );
    let branches: Vec<&str> = rp["on"]["push"]["branches"]
        .as_vec()
        .unwrap()
        .iter()
        .filter_map(Yaml::as_str)
        .collect();
    assert_eq!(branches, ["release/**"], "REQ-CI-098");
    assert!(rp["on"]["push"]["tags"].is_badvalue());

    let job = &rp["jobs"]["release-please"];
    let all = job["steps"].as_vec().unwrap();
    assert_eq!(
        all[0]["env"]["REF"].as_str(),
        Some("${{ github.ref }}"),
        "the refusal reads the full ref, not the ref name"
    );
    let action = all
        .iter()
        .find(|s| {
            s["uses"]
                .as_str()
                .is_some_and(|u| u.starts_with("googleapis/release-please-action@"))
        })
        .expect("the release-please step");
    assert_eq!(
        action["with"]["token"].as_str(),
        Some("${{ steps.app-token.outputs.token }}"),
        "REQ-CI-099: the App token, never GITHUB_TOKEN"
    );
    assert_eq!(
        action["with"]["target-branch"].as_str(),
        Some("${{ github.ref_name }}"),
        "REQ-CI-099"
    );
    let c = template();
    assert_eq!(
        action["with"]["config-file"].as_str(),
        Some(c.release.as_ref().unwrap().release_please_config.as_str())
    );
    assert_eq!(
        action["with"]["manifest-file"].as_str(),
        Some(c.release.as_ref().unwrap().release_please_manifest.as_str())
    );
    let mint = all
        .iter()
        .find(|s| {
            s["uses"]
                .as_str()
                .is_some_and(|u| u.starts_with("actions/create-github-app-token@"))
        })
        .expect("the App token step");
    assert_eq!(mint["id"].as_str(), Some("app-token"));
}

#[cfg(unix)]
#[test]
fn release_please_refuses_any_ref_but_a_release_branch() {
    let rp = example("release-please.yml");
    let body = run_body(&rp["jobs"]["release-please"]["steps"][0]);
    let dir = crate::harness::Scratch::new("rp-ref");
    for (reference, ok) in [
        ("refs/heads/release/2026-10-08-12-00-utc", true),
        ("refs/heads/main", false),
        ("refs/heads/feat/release/thing", false),
        ("refs/tags/release/thing", false),
        ("refs/tags/acme-v1.2.3-rc.1", false),
        ("refs/heads/release/", false),
        ("", false),
    ] {
        let ran = crate::harness::run(&body, &[("REF", reference)], &dir.0);
        assert_eq!(ran.code == 0, ok, "{reference}: {}", ran.log);
    }
}
