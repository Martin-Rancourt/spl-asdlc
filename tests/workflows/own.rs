//! This repository's own CI: every file of `.github/workflows/` that is not
//! one of the product's `asdlc-*` workflows. Its callers call the product's
//! workflows from the same commit (`./`), and are checked as the examples
//! are (`examples.rs`): only declared inputs and secrets, every required one,
//! the permissions the called jobs ask for, pins. Then what holds for this
//! repository only: its tag patterns, its suites, its publication.

use std::collections::BTreeSet;
use std::path::Path;

use regex::Regex;
use yaml_rust2::Yaml;

use crate::examples::{
    check_called_actions, check_called_workflows, check_permissions, check_pins, jobs, keys,
};
use crate::harness::{own_workflows, relative, root, workflow};

/// The runner of a job of this repository: the repository variable, a JSON
/// string or array.
const RUNS_ON: &str = r#"${{ fromJSON(vars.SPL_ASDLC_RUNS_ON || '"ubuntu-latest"') }}"#;

/// The same variable passed to a reusable workflow, which takes a label or a
/// JSON array: a JSON string is decoded to its label.
const RUNS_ON_INPUT: &str = r#"${{ startsWith(vars.SPL_ASDLC_RUNS_ON, '"') && fromJSON(vars.SPL_ASDLC_RUNS_ON) || vars.SPL_ASDLC_RUNS_ON || 'ubuntu-latest' }}"#;

const CANDIDATE_TAG: &str = "v{version}-rc.{rc}";
const RELEASE_TAG: &str = "v{version}";

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
        "release-cut.yml",
        "release-gate-auto-rerun.yml",
        "release-gate.yml",
        "release-please.yml",
        "release-publish.yml",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    assert_eq!(names, want, "checks.yml is replaced by pr.yml");
    for f in own_workflows() {
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(
            !text.contains("secrets: inherit"),
            "{}: secrets: inherit (REQ-CI-002)",
            relative(&f)
        );
    }
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
    for (file, job) in [
        ("pr.yml", "guards"),
        ("pr-title.yml", "title"),
        ("release-cut.yml", "release"),
        ("release-gate.yml", "gate"),
        ("release-publish.yml", "verify"),
    ] {
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
        ["main", "release/**"],
        "REQ-CI-022, REQ-CI-034"
    );
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
    // Exactly the jobs an `if:` may skip.
    let may_skip: BTreeSet<String> = step["with"]["may-skip"]
        .as_str()
        .unwrap()
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();
    let gated: BTreeSet<String> = jobs(&pr)
        .into_iter()
        .filter(|(id, job)| id != "required-check" && !job["if"].is_badvalue())
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
    // Rule zero runs on every PR, whatever guards says.
    let rule_zero = &pr["jobs"]["rule-zero"];
    assert!(rule_zero["if"].is_badvalue() && rule_zero["needs"].is_badvalue());
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
            out.push(p.strip_prefix(base).unwrap().display().to_string());
        }
    }
}

#[test]
fn no_file_a_test_reads_is_prose() {
    // The tests read docs/spec/, docs/business-rules.md, docs/setup/templates/
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
        "docs/spec/08-configuration.md",
        "docs/business-rules.md",
        "docs/setup/templates/spl-asdlc.toml",
        "docs/setup/examples/pr.yml",
        "README.md",
    ] {
        assert!(!re.is_match(read), "{read} is read by a test");
    }
}

#[test]
fn the_gate_reruns_the_guards_the_pr_runs() {
    // REQ-CI-033: a guard that runs on PRs also runs on the candidate.
    let pr = own("pr.yml");
    let gate = own("release-gate.yml");
    let given = pr["jobs"]["guards"]["with"]["guard-commands"].as_str();
    assert!(given.is_some());
    assert_eq!(
        given,
        gate["jobs"]["gate"]["with"]["guard-commands"].as_str()
    );
}

// ---- pr-title.yml ----

#[test]
fn the_title_check_allows_the_changelog_types_and_reruns_on_edits() {
    let title = own("pr-title.yml");
    assert!(strings(&title["on"]["pull_request"]["types"]).contains(&"edited"));
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

// ---- the release chain ----

#[test]
fn the_release_callers_share_this_repositorys_tag_patterns() {
    let cut = &own("release-cut.yml")["jobs"]["release"]["with"];
    let gate = own("release-gate.yml");
    let gate_with = &gate["jobs"]["gate"]["with"];
    let verify = &own("release-publish.yml")["jobs"]["verify"]["with"];
    for (what, with) in [("cut", cut), ("gate", gate_with), ("verify", verify)] {
        assert_eq!(
            with["candidate-tag"].as_str(),
            Some(CANDIDATE_TAG),
            "{what}"
        );
    }
    assert_eq!(cut["release-tag"].as_str(), Some(RELEASE_TAG));
    assert_eq!(gate_with["release-tag"].as_str(), Some(RELEASE_TAG));
    assert_eq!(verify["release-tag-pattern"].as_str(), Some(RELEASE_TAG));
    // The gate starts on the push of a candidate tag, and on nothing else.
    assert_eq!(
        strings(&gate["on"]["push"]["tags"]),
        [CANDIDATE_TAG.replace("{version}", "*").replace("{rc}", "*")]
    );
    assert!(gate["on"]["push"]["branches"].is_badvalue());
    // The release tags release-please makes: no component in the tag.
    let config = file_json("release-please-config.json");
    assert_eq!(config["packages"]["."]["include-component-in-tag"], false);
}

#[test]
fn the_cut_is_dispatched_with_one_input_for_the_crate() {
    let cut = own("release-cut.yml");
    crate::examples::check_cut_follows_release_please(&cut, &own("release-please.yml"), "release/");
    for (k, v) in cut["on"]["workflow_dispatch"]["inputs"].as_hash().unwrap() {
        let k = k.as_str().unwrap();
        if k == "action" {
            assert_eq!(strings(&v["options"]), ["open", "candidate"]);
        } else {
            assert!(!v["default"].is_badvalue(), "input {k} has no default");
            assert_ne!(v["required"].as_bool(), Some(true), "input {k} is required");
        }
    }
    // OQ-17: the caller holds the concurrency group, never cancelled
    // (checked with the triggers above).
    let with = &cut["jobs"]["release"]["with"];
    let config = file_json("release-please-config.json");
    let manifest = file_json(".release-please-manifest.json");
    let key = with["manifest-key"].as_str().unwrap();
    assert!(manifest.get(key).is_some(), "manifest key {key}");
    assert_eq!(
        with["component"].as_str(),
        config["packages"][key]["component"].as_str()
    );
}

#[test]
fn the_gate_runs_the_pr_checks_after_rule_zero() {
    let gate = own("release-gate.yml");
    assert_eq!(gate["concurrency"]["group"].as_str(), Some("release-gate"));
    assert_eq!(
        gate["concurrency"]["cancel-in-progress"].as_bool(),
        Some(false)
    );
    let call = &gate["jobs"]["gate"];
    assert_eq!(needs(call), BTreeSet::from(["preflight".to_string()]));
    assert_eq!(call["if"].as_str(), Some("always()"));
    assert_eq!(
        call["with"]["upstream-result"].as_str(),
        Some("${{ needs.preflight.result }}")
    );

    // The suites run what pr.yml's jobs run.
    let suites = json(call["with"]["suites"].as_str().unwrap(), "suites");
    let ids: Vec<&str> = suites
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["fmt", "clippy", "test"]);
    let pr = own("pr.yml");
    for suite in suites.as_array().unwrap() {
        let id = suite["id"].as_str().unwrap();
        let command = pr["jobs"][id]["steps"]
            .as_vec()
            .unwrap()
            .iter()
            .filter_map(|s| s["run"].as_str())
            .next_back()
            .unwrap_or_else(|| panic!("pr.yml job {id} runs nothing"));
        assert!(
            suite["run"].as_str().unwrap().starts_with(command),
            "suite {id} does not run `{command}`"
        );
    }
    // Zero tests is never a pass.
    assert!(!suites[2]["expect-nonempty"].is_null());

    // Rule zero, with the forbidden words, before any suite: as on PRs.
    let scan = |doc: &Yaml, job: &str| {
        doc["jobs"][job]["steps"]
            .as_vec()
            .unwrap()
            .iter()
            .find(|s| s["run"].as_str() == Some("bash scripts/check-no-instance-names.sh"))
            .unwrap_or_else(|| panic!("{job}: no rule-zero scan"))
            .clone()
    };
    let at_gate = scan(&gate, "preflight");
    let on_pr = scan(&pr, "rule-zero");
    assert_eq!(
        at_gate["env"]["SPL_ASDLC_FORBIDDEN_WORDS"].as_str(),
        on_pr["env"]["SPL_ASDLC_FORBIDDEN_WORDS"].as_str()
    );
    assert!(
        at_gate["env"]["SPL_ASDLC_FORBIDDEN_WORDS"]
            .as_str()
            .unwrap()
            .contains("secrets.SPL_ASDLC_FORBIDDEN_WORDS")
    );
}

#[test]
fn the_auto_rerun_follows_the_gate_by_name() {
    let rerun = own("release-gate-auto-rerun.yml");
    let gate = own("release-gate.yml");
    assert_eq!(
        rerun["on"]["workflow_run"]["workflows"][0].as_str(),
        gate["name"].as_str()
    );
    assert_eq!(
        strings(&rerun["on"]["workflow_run"]["types"]),
        ["completed"]
    );
}

#[test]
fn release_please_runs_on_release_branches_with_this_repositorys_files() {
    let rp = own("release-please.yml");
    assert_eq!(
        keys(&rp["on"]),
        BTreeSet::from(["push".to_string(), "workflow_dispatch".to_string()])
    );
    assert_eq!(strings(&rp["on"]["push"]["branches"]), ["release/**"]);
    let step = rp["jobs"]["release-please"]["steps"]
        .as_vec()
        .unwrap()
        .iter()
        .find(|s| {
            s["uses"]
                .as_str()
                .is_some_and(|u| u.starts_with("googleapis/release-please-action@"))
        })
        .unwrap()
        .clone();
    assert_eq!(
        step["with"]["token"].as_str(),
        Some("${{ steps.app-token.outputs.token }}"),
        "REQ-CI-099"
    );
    for (input, file) in [
        ("config-file", "release-please-config.json"),
        ("manifest-file", ".release-please-manifest.json"),
    ] {
        assert_eq!(step["with"][input].as_str(), Some(file));
        assert!(root().join(file).is_file(), "{file}");
    }
}

#[test]
fn the_publication_verifies_builds_the_binaries_and_always_closes() {
    let publish = own("release-publish.yml");
    assert_eq!(
        keys(&publish["on"]),
        BTreeSet::from(["release".to_string(), "workflow_dispatch".to_string()])
    );
    assert_eq!(strings(&publish["on"]["release"]["types"]), ["published"]);
    assert_eq!(
        publish["concurrency"]["cancel-in-progress"].as_bool(),
        Some(false)
    );

    // A single component: verify and close, no ledger.
    let stages: Vec<(String, String)> = jobs(&publish)
        .into_iter()
        .filter_map(|(id, job)| job["with"]["stage"].as_str().map(|s| (id, s.to_string())))
        .collect();
    assert_eq!(
        stages,
        [
            ("verify".to_string(), "verify".to_string()),
            ("close".to_string(), "close".to_string())
        ]
    );
    let close = &publish["jobs"]["close"];
    assert_eq!(close["if"].as_str(), Some("${{ always() }}"));
    assert_eq!(
        needs(close),
        BTreeSet::from(["verify".to_string(), "publish".to_string()])
    );
    assert_eq!(
        close["with"]["publish-result"].as_str(),
        Some("${{ needs.publish.result }}")
    );
    assert!(close["with"]["components"].is_badvalue());
    assert_eq!(
        keys(&close["secrets"]),
        BTreeSet::from([
            "SPL_ASDLC_APP_ID".to_string(),
            "SPL_ASDLC_APP_PRIVATE_KEY".to_string()
        ])
    );
    assert!(
        publish["jobs"]["verify"]["if"]
            .as_str()
            .unwrap()
            .contains("inputs.replay")
    );

    // What release-please changes after the RC, and nothing else: the rust
    // release type bumps the crate's version in Cargo.toml and Cargo.lock.
    let verify = &publish["jobs"]["verify"]["with"];
    let config = file_json("release-please-config.json");
    assert_eq!(config["packages"]["."]["release-type"], "rust");
    let changelog = config["packages"]["."]["changelog-path"].as_str().unwrap();
    let bookkeeping: BTreeSet<String> = json(
        verify["bookkeeping-paths"].as_str().unwrap(),
        "bookkeeping-paths",
    )
    .as_array()
    .unwrap()
    .iter()
    .map(|p| p.as_str().unwrap().to_string())
    .collect();
    assert_eq!(
        bookkeeping,
        BTreeSet::from([
            ".release-please-manifest.json".to_string(),
            changelog.to_string(),
            "Cargo.toml".to_string(),
            "Cargo.lock".to_string(),
        ])
    );
    // The bridge's App, by its slug.
    assert_eq!(verify["go-merger-login"].as_str(), Some("spl-asdlc"));

    // The publication is binaries.yml, on the verified release's tag.
    let own_job = &publish["jobs"]["publish"];
    assert_eq!(
        own_job["uses"].as_str(),
        Some("./.github/workflows/binaries.yml")
    );
    assert_eq!(needs(own_job), BTreeSet::from(["verify".to_string()]));
    assert_eq!(
        own_job["with"]["tag"].as_str(),
        verify["release-tag"].as_str(),
        "the tag verify verified"
    );
    assert_eq!(
        keys(&own_job["permissions"]),
        BTreeSet::from(["contents".to_string()])
    );
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
