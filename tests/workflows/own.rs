//! This repository's own CI: every file of `.github/workflows/` that is not
//! one of the product's `asdlc-*` workflows. Its callers call the product's
//! workflows from the same commit (`./`), and are checked as the examples
//! are (`examples.rs`): only declared inputs and secrets, every required one,
//! the permissions the called jobs ask for, pins. Then what holds for this
//! repository only: GitHub Flow on `main`, release-please on `main` as the
//! App, and a publication that builds the binaries. This repository does not
//! run the SPL-ASDLC release cycle on itself: it has no release-cut, gate or
//! release-publish caller.

use std::collections::BTreeSet;
use std::path::Path;

use regex::Regex;
use yaml_rust2::Yaml;

use crate::examples::{
    NOT_A_DRAFT, check_called_actions, check_called_workflows, check_no_job_runs_on_a_draft,
    check_permissions, check_pins, jobs, keys,
};
use crate::harness::{own_workflows, relative, root, workflow};

/// The runner of a job of this repository: the repository variable, a JSON
/// string or array.
const RUNS_ON: &str = r#"${{ fromJSON(vars.SPL_ASDLC_RUNS_ON || '"ubuntu-latest"') }}"#;

/// The same variable passed to a reusable workflow, which takes a label or a
/// JSON array: a JSON string is decoded to its label.
const RUNS_ON_INPUT: &str = r#"${{ startsWith(vars.SPL_ASDLC_RUNS_ON, '"') && fromJSON(vars.SPL_ASDLC_RUNS_ON) || vars.SPL_ASDLC_RUNS_ON || 'ubuntu-latest' }}"#;

fn own(name: &str) -> Yaml {
    workflow(&format!(".github/workflows/{name}"))
}

/// `./<path>` of this commit -> `<path>`, refusing any other reference: this
/// repository's PRs and releases exercise the workflows they carry.
fn local(uses: &str, what: &str) -> String {
    let shape = Regex::new(r"^\./(\.github/workflows/[a-z-]+\.yml|actions/[a-z-]+)$").unwrap();
    let caps = shape
        .captures(uses)
        .unwrap_or_else(|| panic!("{what}: `{uses}` is not a unit of this commit (./<path>)"));
    let path = caps[1].to_string();
    assert!(root().join(&path).exists(), "{what}: {path} does not exist");
    path
}

fn ours(uses: &str) -> bool {
    uses.starts_with("./")
}

fn strings(y: &Yaml) -> Vec<&str> {
    y.as_vec()
        .unwrap_or_else(|| panic!("not a list: {y:?}"))
        .iter()
        .filter_map(Yaml::as_str)
        .collect()
}

fn needs(job: &Yaml) -> BTreeSet<String> {
    match job["needs"].as_str() {
        Some(one) => BTreeSet::from([one.to_string()]),
        None => job["needs"]
            .as_vec()
            .into_iter()
            .flatten()
            .filter_map(Yaml::as_str)
            .map(str::to_string)
            .collect(),
    }
}

fn json(text: &str, what: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("{what}: {e}: {text}"))
}

fn file_json(rel: &str) -> serde_json::Value {
    json(&std::fs::read_to_string(root().join(rel)).unwrap(), rel)
}

#[test]
fn this_repositorys_workflows_are_its_callers_and_binaries() {
    let names: BTreeSet<String> = own_workflows()
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    let want: BTreeSet<String> = [
        "binaries.yml",
        "pr-title.yml",
        "pr.yml",
        "publish.yml",
        "release-please.yml",
        "staging.yml",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    assert_eq!(names, want);
    for f in own_workflows() {
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(
            !text.contains("secrets: inherit"),
            "{}: secrets: inherit (REQ-CI-002)",
            relative(&f)
        );
        // The release cycle is the product's, exercised on a separate
        // repository: no caller of the release workflows here.
        assert!(
            !text.contains("asdlc-release-"),
            "{}: this repository does not run its own release cycle",
            relative(&f)
        );
    }
}

/// BR-ASDLC-023: this repository is the one project outside its own method.
/// It has no method configuration, so no bridge can take it on, and its
/// releases are release-please's (`release_please_runs_on_main_as_the_app_with_this_repositorys_files`).
#[test]
fn this_repository_has_no_method_configuration() {
    assert!(
        !root().join(".spl-asdlc/config.toml").exists(),
        ".spl-asdlc/config.toml: this repository does not run the method on itself (BR-ASDLC-023)"
    );
}

#[test]
fn every_call_is_to_this_commit_with_declared_inputs_and_secrets() {
    let files = own_workflows();
    check_called_workflows(&files, local);
    check_called_actions(&files, ours, local);
}

#[test]
fn every_caller_grants_what_its_called_jobs_ask_for() {
    check_permissions(&own_workflows(), local);
}

#[test]
fn every_other_action_is_pinned_as_the_product_pins_it() {
    check_pins(&own_workflows(), ours);
}

#[test]
fn every_job_runs_on_the_repositorys_runner() {
    for f in own_workflows() {
        if relative(&f).ends_with("/binaries.yml") {
            // One runner per target, by design.
            continue;
        }
        for (id, job) in jobs(&crate::harness::load(&f)) {
            let what = format!("{} job {id}", relative(&f));
            if job["uses"].is_badvalue() {
                assert_eq!(job["runs-on"].as_str(), Some(RUNS_ON), "{what}");
            } else if let Some(given) = job["with"]["runs-on"].as_str() {
                assert_eq!(given, RUNS_ON_INPUT, "{what}");
            }
        }
    }
    // The calls whose jobs run on the project's runner take it.
    for (file, job) in [("pr.yml", "guards"), ("pr-title.yml", "title")] {
        assert_eq!(
            own(file)["jobs"][job]["with"]["runs-on"].as_str(),
            Some(RUNS_ON_INPUT),
            "{file} job {job}"
        );
    }
}

// ---- pr.yml ----

#[test]
fn the_pr_workflow_is_unfiltered_runs_on_prs_only_and_aggregates_every_job() {
    let pr = own("pr.yml");
    assert_eq!(
        keys(&pr["on"]),
        BTreeSet::from(["pull_request".to_string()]),
        "no push run on main: the PR ran on its merge commit"
    );
    let trigger = &pr["on"]["pull_request"];
    assert!(trigger["paths"].is_badvalue() && trigger["paths-ignore"].is_badvalue());
    assert_eq!(
        strings(&trigger["branches"]),
        ["main"],
        "GitHub Flow: every PR targets main (REQ-CI-022)"
    );
    assert!(
        strings(&trigger["types"]).contains(&"ready_for_review"),
        "a PR marked ready runs the jobs a draft does not run, required-check included"
    );
    assert!(
        pr["concurrency"]["group"]
            .as_str()
            .unwrap()
            .contains("github.event.pull_request.draft"),
        "REQ-CI-023"
    );

    check_no_job_runs_on_a_draft(&pr, "pr.yml");

    let aggregator = &pr["jobs"]["required-check"];
    assert_eq!(aggregator["name"].as_str(), Some("required-check"));
    let others: BTreeSet<String> = jobs(&pr)
        .into_iter()
        .map(|(id, _)| id)
        .filter(|id| id != "required-check")
        .collect();
    assert_eq!(
        needs(aggregator),
        others,
        "every job is a need of required-check (REQ-CI-025)"
    );
    let steps = aggregator["steps"].as_vec().unwrap();
    assert!(
        steps[0]["uses"]
            .as_str()
            .is_some_and(|u| u.starts_with("actions/checkout@")),
        "the action of this commit is checked out first"
    );
    let step = steps.last().unwrap();
    assert_eq!(step["uses"].as_str(), Some("./actions/required-check"));
    assert_eq!(step["with"]["needs"].as_str(), Some("${{ toJSON(needs) }}"));
    // Exactly the jobs an `if:` may skip on a PR that is not a draft.
    let may_skip: BTreeSet<String> = step["with"]["may-skip"]
        .as_str()
        .unwrap()
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();
    let gated: BTreeSet<String> = jobs(&pr)
        .into_iter()
        .filter(|(id, job)| id != "required-check" && job["if"].as_str() != Some(NOT_A_DRAFT))
        .map(|(id, _)| id)
        .collect();
    assert_eq!(may_skip, gated);
    assert_eq!(
        may_skip,
        BTreeSet::from(["fmt".into(), "clippy".into(), "test".into()])
    );
    for id in ["fmt", "clippy", "test"] {
        assert_eq!(
            needs(&pr["jobs"][id]),
            BTreeSet::from(["guards".into()]),
            "{id}"
        );
    }
    // Rule zero runs on every PR but a draft, whatever guards says.
    let rule_zero = &pr["jobs"]["rule-zero"];
    assert_eq!(rule_zero["if"].as_str(), Some(NOT_A_DRAFT));
    assert!(rule_zero["needs"].is_badvalue());
}

/// Every file of the working tree but the build output and the VCS.
fn tree(base: &Path, dir: &Path, out: &mut Vec<String>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if matches!(name.as_str(), "target" | ".git" | ".claude") {
                continue;
            }
            tree(base, &p, out);
        } else {
            out.push(spl_asdlc::method::glob::repo_path(
                p.strip_prefix(base).unwrap(),
            ));
        }
    }
}

#[test]
fn no_file_a_test_reads_is_prose() {
    // The tests read docs/reference/, .spl-asdlc/business-rules/, docs/setup/templates/
    // and docs/setup/examples/, and rule zero reads every file: a docs-only PR
    // runs the suite. Only the licence text skips it.
    let pr = own("pr.yml");
    let prose = pr["jobs"]["guards"]["with"]["prose-regex"]
        .as_str()
        .unwrap();
    let re = Regex::new(prose).unwrap();
    let mut files = Vec::new();
    tree(&root(), &root(), &mut files);
    let matched: Vec<&String> = files.iter().filter(|f| re.is_match(f)).collect();
    assert_eq!(matched, ["LICENSE"], "{prose}");
    for read in [
        "docs/reference/08-configuration.md",
        ".spl-asdlc/business-rules/BR-ASDLC-001.md",
        "docs/setup/templates/config.toml",
        "docs/setup/examples/pr.yml",
        "README.md",
    ] {
        assert!(!re.is_match(read), "{read} is read by a test");
    }
}

// ---- pr-title.yml ----

#[test]
fn the_title_check_allows_the_changelog_types_and_reruns_on_edits() {
    let title = own("pr-title.yml");
    assert!(strings(&title["on"]["pull_request"]["types"]).contains(&"edited"));
    assert_eq!(strings(&title["on"]["pull_request"]["branches"]), ["main"]);
    for (id, job) in jobs(&title) {
        assert!(job["if"].is_badvalue(), "job {id} has an if: (REQ-CI-018)");
    }
    assert_ne!(
        title["concurrency"]["group"].as_str(),
        own("pr.yml")["concurrency"]["group"].as_str()
    );
    let allowed: BTreeSet<String> = title["jobs"]["title"]["with"]["allowed-types"]
        .as_str()
        .unwrap()
        .split(',')
        .map(|t| t.trim().to_string())
        .collect();
    let config = file_json("release-please-config.json");
    let sections: BTreeSet<String> = config["packages"]["."]["changelog-sections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["type"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(allowed, sections, "REQ-CI-016");
}

// ---- the releases of spl-asdlc itself ----

#[test]
fn release_please_runs_on_main_as_the_app_with_this_repositorys_files() {
    let rp = own("release-please.yml");
    assert_eq!(keys(&rp["on"]), BTreeSet::from(["push".to_string()]));
    assert_eq!(strings(&rp["on"]["push"]["branches"]), ["main"]);
    let steps = rp["jobs"]["release-please"]["steps"].as_vec().unwrap();
    let uses = |prefix: &str| {
        steps
            .iter()
            .find(|s| s["uses"].as_str().is_some_and(|u| u.starts_with(prefix)))
            .unwrap_or_else(|| panic!("no step uses {prefix}"))
            .clone()
    };
    // The App token: a release PR opened with GITHUB_TOKEN would get no
    // checks, and its release would start no publication.
    let mint = uses("actions/create-github-app-token@");
    assert_eq!(mint["id"].as_str(), Some("app-token"));
    assert_eq!(
        keys(&mint["with"]),
        BTreeSet::from(["app-id".to_string(), "private-key".to_string()])
    );
    let step = uses("googleapis/release-please-action@");
    assert_eq!(
        step["with"]["token"].as_str(),
        Some("${{ steps.app-token.outputs.token }}")
    );
    assert_eq!(step["with"]["target-branch"].as_str(), Some("main"));
    for (input, file) in [
        ("config-file", "release-please-config.json"),
        ("manifest-file", ".release-please-manifest.json"),
    ] {
        assert_eq!(step["with"][input].as_str(), Some(file));
        assert!(root().join(file).is_file(), "{file}");
    }
}

#[test]
fn the_publication_builds_the_binaries_of_the_published_release_and_nothing_else() {
    let publish = own("publish.yml");
    assert_eq!(
        keys(&publish["on"]),
        BTreeSet::from(["release".to_string()])
    );
    assert_eq!(strings(&publish["on"]["release"]["types"]), ["published"]);
    let all = jobs(&publish);
    assert_eq!(
        all.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(),
        ["binaries"],
        "no verify, no merge-back, no closing-guard"
    );
    let call = &publish["jobs"]["binaries"];
    assert_eq!(
        call["uses"].as_str(),
        Some("./.github/workflows/binaries.yml")
    );
    assert_eq!(
        call["with"]["tag"].as_str(),
        Some("${{ github.event.release.tag_name }}")
    );
    assert_eq!(
        keys(&call["permissions"]),
        BTreeSet::from(["contents".to_string()])
    );
    assert_eq!(call["permissions"]["contents"].as_str(), Some("write"));
    // The release tags release-please makes: `v<X.Y.Z>`, no component.
    let config = file_json("release-please-config.json");
    assert_eq!(config["packages"]["."]["include-component-in-tag"], false);
}

// ---- binaries.yml ----

#[test]
fn the_binaries_are_built_once_per_release_by_the_publication() {
    let binaries = own("binaries.yml");
    // No tag push: the release-please tag would start a second, duplicate
    // build of every target, macOS included.
    assert!(
        binaries["on"]["push"].is_badvalue(),
        "binaries.yml has a push trigger"
    );
    assert_eq!(
        keys(&binaries["on"]),
        BTreeSet::from(["workflow_call".to_string(), "workflow_dispatch".to_string()])
    );
    for trigger in ["workflow_call", "workflow_dispatch"] {
        assert_eq!(
            binaries["on"][trigger]["inputs"]["tag"]["required"].as_bool(),
            Some(true),
            "{trigger}"
        );
    }
    // The one write is the release job's.
    assert_eq!(binaries["permissions"]["contents"].as_str(), Some("read"));
    for (id, job) in jobs(&binaries) {
        let want = if id == "release" { "write" } else { "read" };
        assert_eq!(
            keys(&job["permissions"]),
            BTreeSet::from(["contents".to_string()]),
            "{id}"
        );
        assert_eq!(job["permissions"]["contents"].as_str(), Some(want), "{id}");
        assert_eq!(
            job["env"]["TAG"].as_str(),
            Some("${{ inputs.tag }}"),
            "{id}"
        );
    }
}

#[test]
fn every_job_of_the_release_workflows_has_a_bound() {
    // A hung runner runs to the job's timeout, six hours by default; a macOS
    // minute counts ten. A job that calls a workflow takes its jobs' bounds.
    for name in [
        "binaries.yml",
        "publish.yml",
        "release-please.yml",
        "staging.yml",
    ] {
        for (id, job) in jobs(&own(name)) {
            if job["uses"].is_badvalue() {
                assert!(
                    job["timeout-minutes"].as_i64().is_some(),
                    "{name}: job {id} has no timeout-minutes"
                );
            }
        }
    }
    assert_eq!(
        own("binaries.yml")["jobs"]["build"]["timeout-minutes"].as_i64(),
        Some(15),
        "a build takes a few minutes"
    );
}

#[test]
fn staging_rebuilds_the_rolling_prerelease_on_every_push_to_main() {
    let staging = own("staging.yml");
    assert_eq!(keys(&staging["on"]), BTreeSet::from(["push".to_string()]));
    assert_eq!(
        staging["on"]["push"]["branches"][0].as_str(),
        Some("main"),
        "staging follows main only"
    );
    let job = &staging["jobs"]["staging"];
    assert_eq!(
        keys(&job["permissions"]),
        BTreeSet::from(["contents".to_string()])
    );
    let text = std::fs::read_to_string(root().join(".github/workflows/staging.yml")).unwrap();
    // A prerelease, never the latest, on its own tag: the release tags stay
    // the published releases'.
    assert!(text.contains("--prerelease"));
    assert!(text.contains("--latest=false"));
    assert!(text.contains("refs/tags/staging"));
    assert!(!text.contains("refs/tags/v"));
    // The archive says which commit it was built from, and the sums cover it.
    assert!(text.contains("COMMIT"));
    assert!(text.contains("sha256sum"));
}
