//! Repository lints over every workflow and composite action
//! (`docs/requirements/ci.md`, REQ-CI-001..010, 018).

use std::collections::BTreeSet;

use regex::Regex;
use yaml_rust2::Yaml;

use crate::harness::{files, is_product_workflow, is_reusable, load, relative, run_body, steps};

/// Every `run:` body of a file, with the step's name.
fn runs(doc: &Yaml) -> Vec<(String, String)> {
    steps(doc)
        .into_iter()
        .filter(|(_, s)| !s["run"].is_badvalue())
        .map(|(job, s)| {
            let name = s["name"]
                .as_str()
                .or(s["id"].as_str())
                .unwrap_or("?")
                .to_string();
            (format!("{job} / {name}"), run_body(&s))
        })
        .collect()
}

/// The lines of a script that are not comments.
fn code_lines(body: &str) -> impl Iterator<Item = &str> {
    body.lines().filter(|l| !l.trim_start().starts_with('#'))
}

#[test]
fn every_file_parses_as_one_yaml_document() {
    let all = files();
    assert!(all.len() >= 3, "{all:?}");
    for f in all {
        load(&f);
    }
}

#[test]
fn a_reusable_workflow_has_no_trigger_of_its_own() {
    for f in files() {
        let doc = load(&f);
        if !is_reusable(&doc) {
            continue;
        }
        if !is_product_workflow(&f) {
            // This repository's binaries.yml: called by its publish.yml,
            // and dispatched by hand to rebuild a tag. Never a push trigger.
            assert_eq!(relative(&f), ".github/workflows/binaries.yml");
            assert!(doc["on"]["push"].is_badvalue(), "{}", relative(&f));
            continue;
        }
        let triggers: Vec<String> = doc["on"]
            .as_hash()
            .unwrap()
            .keys()
            .map(|k| k.as_str().unwrap_or("?").to_string())
            .collect();
        assert_eq!(triggers, vec!["workflow_call"], "{}", relative(&f));
    }
}

#[test]
fn untrusted_values_reach_scripts_only_through_env() {
    for f in files() {
        for (step, body) in runs(&load(&f)) {
            assert!(
                !body.contains("${{"),
                "{} ({step}): an expression inside run: (REQ-CI-003); pass it through env:",
                relative(&f)
            );
        }
    }
}

#[test]
fn no_decision_is_taken_on_a_pipe_into_a_quiet_grep() {
    let piped =
        Regex::new(r"\|\s*grep\s+(-[A-Za-z]+\s+)*(-[A-Za-z]*q[A-Za-z]*|--quiet)\b").unwrap();
    let mut bodies: Vec<(String, String)> = Vec::new();
    for f in files() {
        for (step, body) in runs(&load(&f)) {
            bodies.push((format!("{} ({step})", relative(&f)), body));
        }
    }
    for entry in std::fs::read_dir(crate::harness::root().join("scripts"))
        .unwrap()
        .flatten()
    {
        let p = entry.path();
        bodies.push((relative(&p), std::fs::read_to_string(&p).unwrap()));
    }
    for (what, body) in bodies {
        for line in code_lines(&body) {
            assert!(
                !piped.is_match(line),
                "{what}: `| grep -q` under pipefail (REQ-CI-009): {line}"
            );
        }
    }
}

#[test]
fn no_forced_push_no_branch_update_no_squash_of_a_release_and_no_unbased_pr_list() {
    let forbidden = [
        (
            Regex::new(r"git\s+push\b.*(\s-f\b|--force)").unwrap(),
            "a forced push",
        ),
        (Regex::new(r"update-branch").unwrap(), "an update-branch"),
        (
            Regex::new(r"gh\s+pr\s+merge\b.*--(squash|rebase|admin)").unwrap(),
            "a squash, rebase or admin merge",
        ),
    ];
    let pr_list = Regex::new(r"gh\s+pr\s+list\b").unwrap();
    for f in files() {
        for (step, body) in runs(&load(&f)) {
            for line in code_lines(&body) {
                for (re, what) in &forbidden {
                    assert!(
                        !re.is_match(line),
                        "{} ({step}): {what} (REQ-CI-010): {line}",
                        relative(&f)
                    );
                }
                if pr_list.is_match(line) {
                    assert!(
                        line.contains("--base "),
                        "{} ({step}): `gh pr list` without --base (REQ-CI-010): {line}",
                        relative(&f)
                    );
                }
            }
        }
    }
}

#[test]
fn a_reusable_workflow_declares_exactly_the_secrets_it_reads() {
    let read = Regex::new(r"secrets\.([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    for f in files() {
        let doc = load(&f);
        if !is_reusable(&doc) {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap();
        let used: BTreeSet<String> = read
            .captures_iter(&text)
            .map(|c| c[1].to_string())
            .filter(|s| s != "GITHUB_TOKEN")
            .collect();
        let declared: BTreeSet<String> = doc["on"]["workflow_call"]["secrets"]
            .as_hash()
            .map(|h| {
                h.keys()
                    .filter_map(|k| k.as_str())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(
            used,
            declared,
            "{}: secrets read vs declared (REQ-CI-002)",
            relative(&f)
        );
        assert!(
            !text.contains("secrets: inherit"),
            "{}: secrets: inherit",
            relative(&f)
        );
    }
}

#[test]
fn permissions_are_set_at_the_top_and_restated_per_job_of_a_reusable_workflow() {
    for f in files() {
        let doc = load(&f);
        if doc["jobs"].is_badvalue() {
            continue;
        }
        assert!(
            !doc["permissions"].is_badvalue(),
            "{}: no top-level permissions (REQ-CI-005)",
            relative(&f)
        );
        if !is_reusable(&doc) {
            continue;
        }
        for (id, job) in doc["jobs"].as_hash().unwrap() {
            assert!(
                !job["permissions"].is_badvalue(),
                "{} job {:?}: permissions not restated (REQ-CI-005)",
                relative(&f),
                id.as_str()
            );
        }
    }
}

#[test]
fn every_action_is_pinned_by_commit() {
    let pinned = Regex::new(r"^[^@\s]+@[0-9a-f]{40}$").unwrap();
    for f in files() {
        let doc = load(&f);
        for (job, s) in steps(&doc) {
            let Some(uses) = s["uses"].as_str() else {
                continue;
            };
            if uses.starts_with("./") {
                continue;
            }
            assert!(
                pinned.is_match(uses),
                "{} ({job}): `{uses}` is not pinned to a commit",
                relative(&f)
            );
        }
    }
}

#[test]
fn a_reusable_workflow_takes_its_runner_as_an_input() {
    for f in files() {
        let doc = load(&f);
        // binaries.yml names its runners: one per target.
        if !is_reusable(&doc) || !is_product_workflow(&f) {
            continue;
        }
        let input = &doc["on"]["workflow_call"]["inputs"]["runs-on"];
        assert_eq!(
            input["default"].as_str(),
            Some("ubuntu-latest"),
            "{}: runs-on input (REQ-CI-004)",
            relative(&f)
        );
        for (id, job) in doc["jobs"].as_hash().unwrap() {
            let runs_on = job["runs-on"].as_str().unwrap_or("");
            assert!(
                runs_on.contains("inputs.runs-on") || runs_on == "ubuntu-latest",
                "{} job {:?}: runs-on is neither the input nor hosted",
                relative(&f),
                id.as_str()
            );
        }
    }
}

#[test]
fn the_title_check_has_no_if_anywhere() {
    let doc = crate::harness::workflow(".github/workflows/asdlc-pr-title.yml");
    for (id, job) in doc["jobs"].as_hash().unwrap() {
        assert!(
            job["if"].is_badvalue(),
            "job {:?} has an if: (REQ-CI-018)",
            id.as_str()
        );
    }
    for (_, s) in steps(&doc) {
        assert!(s["if"].is_badvalue(), "a step has an if: (REQ-CI-018)");
    }
}

#[test]
fn the_rust_cache_never_cleans_the_cargo_bin_directory() {
    // Its post step deletes what `cargo install` did not put in $CARGO_HOME/bin,
    // rustup included: fatal on a self-hosted runner (SID-987).
    for f in files() {
        let doc = load(&f);
        for (job, s) in steps(&doc) {
            let Some(uses) = s["uses"].as_str() else {
                continue;
            };
            if !uses.starts_with("Swatinem/rust-cache@") {
                continue;
            }
            assert_eq!(
                s["with"]["cache-bin"].as_str(),
                Some("false"),
                "{} ({job}): Swatinem/rust-cache without `cache-bin: \"false\"`",
                relative(&f)
            );
        }
    }
}

/// Why a script polls another workflow's run, if it does: a read of a run
/// (`gh run list|view`, the runs API) inside a loop or next to a `sleep`, or
/// a `gh run watch`. On one runner, a job that waits for another run holds
/// the runner that run needs, and the wait ends at the job's timeout.
fn polls(body: &str) -> Option<String> {
    let read = Regex::new(r"gh\s+run\s+(list|view)\b|gh\s+api\b.*actions/runs\b").unwrap();
    let watch = Regex::new(r"gh\s+run\s+watch\b").unwrap();
    let opens = Regex::new(r"(^|[|;&]\s*|\bdo\s+|\bthen\s+)(while|until|for)\s").unwrap();
    let closes = Regex::new(r"(^|[;&]\s*)done\b").unwrap();
    let sleep = Regex::new(r"(^|[;&]\s*|\bdo\s+|\bthen\s+)sleep\s").unwrap();
    let sleeps = code_lines(body).any(|l| sleep.is_match(l.trim()));
    let mut depth = 0usize;
    for line in code_lines(body) {
        let l = line.trim();
        if watch.is_match(l) {
            return Some(format!("watches a run: {l}"));
        }
        let open = opens.find_iter(l).count();
        if read.is_match(l) {
            if depth + open > 0 {
                return Some(format!("reads a run in a loop: {l}"));
            }
            if sleeps {
                return Some(format!("reads a run and sleeps: {l}"));
            }
        }
        depth = (depth + open).saturating_sub(closes.find_iter(l).count());
    }
    None
}

#[test]
fn the_poll_detector_sees_the_shapes_of_a_wait() {
    let waits = [
        "while :; do\n  gh run list --workflow x --limit 1 > row\n  sleep 15\ndone",
        "until gh run view 7 --json status > s; do sleep 5; done",
        "for i in 1 2 3; do gh api \"repos/$R/actions/runs/$ID\" > r; done",
        "gh run watch 7",
        "gh run list --limit 1 > row\nsleep 30\ngh run list --limit 1 > row",
    ];
    for w in waits {
        assert!(polls(w).is_some(), "{w}");
    }
    let reads = [
        "if ! gh run list --workflow x --commit \"$head\" --limit 1 > row; then exit 1; fi",
        "while read -r t; do echo \"$t\"; done < tags\ngh run list --limit 1",
        "# while :; do gh run list; sleep 1; done",
    ];
    for r in reads {
        assert_eq!(polls(r), None, "{r}");
    }
}

#[test]
fn no_step_polls_another_workflows_run() {
    let mut all = files();
    let examples = crate::harness::root().join("docs/setup/examples");
    all.extend(
        std::fs::read_dir(examples)
            .unwrap()
            .flatten()
            .map(|e| e.path()),
    );
    for f in all {
        for (step, body) in runs(&load(&f)) {
            if let Some(why) = polls(&body) {
                panic!("{} ({step}) {why}", relative(&f));
            }
        }
    }
}
