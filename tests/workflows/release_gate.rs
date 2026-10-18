//! `release-gate` and its auto-rerun (REQ-CI-055..075): the steps run as
//! shipped, in the order and under the conditions the runner applies them,
//! against a real repository; `gh` is stubbed where GitHub answers.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::harness::{
    Ran, Repo, Scratch, calls, path_with, run_body, run_owned, step, stub, workflow,
};

const GATE: &str = ".github/workflows/asdlc-release-gate.yml";
const RERUN: &str = ".github/workflows/asdlc-release-gate-auto-rerun.yml";

fn body(file: &str, job: &str, id: &str) -> String {
    run_body(&step(&workflow(file), job, id))
}

// ------------------------------------------------------------ the candidate

fn resolve(
    repo: &Repo,
    event: &str,
    dispatch: &str,
    ref_type: &str,
    name: &str,
) -> (String, String) {
    let r = crate::harness::run(
        &body(GATE, "candidate", "class"),
        &[
            ("EVENT_NAME", event),
            ("DISPATCH_CLASS", dispatch),
            ("REF_TYPE", ref_type),
            ("REF_NAME", name),
        ],
        &repo.dir.0,
    );
    assert_eq!(r.code, 0, "the resolver always answers: {}", r.log);
    (r.output("class").unwrap(), r.output("source").unwrap())
}

#[test]
fn the_class_is_read_from_the_annotation_and_never_defaults_to_routine() {
    let repo = Repo::new();
    let cases: &[(&str, &str, &str)] = &[
        (
            "Release-Class: sensible\nRelease-Class-Reason: x",
            "sensible",
            "tag annotation",
        ),
        (
            "Release-Class: routine\nRelease-Class-Surfaces: none",
            "routine",
            "tag annotation",
        ),
        (
            "Release-Class:   routine  \nRelease-Class-Surfaces: none",
            "routine",
            "tag annotation",
        ),
        // Missing evidence is not "no risk".
        (
            "Release-Class: routine",
            "sensible",
            "without recorded reviewed paths",
        ),
        (
            "Release-Class: routine\nRelease-Class-Surfaces: src/auth/**\nRelease-Class-Justification: renames only",
            "routine",
            "justified: renames only",
        ),
        (
            "Release-Class: routine\nRelease-Class-Surfaces: src/auth/**",
            "sensible",
            "no recorded justification",
        ),
        ("no class at all", "sensible", "undetermined"),
        ("Release-Class: sensible!", "sensible", "undetermined"),
        // Anything appended to the class line makes it unreadable.
        (
            "Release-Class: routine because docs\nRelease-Class-Surfaces: none",
            "sensible",
            "undetermined",
        ),
        // The first Release-Class line decides, as the bridge reads it.
        (
            "Release-Class: bogus\nRelease-Class: routine\nRelease-Class-Surfaces: none",
            "sensible",
            "undetermined",
        ),
        // Incidental text and the evidence keys are never the class.
        (
            "see the Release-Class: routine line",
            "sensible",
            "undetermined",
        ),
        (
            "Release-Class-Reason: routine\nRelease-Class-Surfaces: none",
            "sensible",
            "undetermined",
        ),
    ];
    for (i, (annotation, class, source)) in cases.iter().enumerate() {
        let tag = format!("acme-v1.0.{i}-rc.1");
        repo.annotated(&tag, &format!("Release candidate {tag}\n\n{annotation}"));
        let (got, why) = resolve(&repo, "push", "", "tag", &tag);
        assert_eq!(&got, class, "{annotation:?}: {why}");
        assert!(why.contains(source), "{annotation:?}: {why}");
    }
    // A lightweight tag's contents are the commit message.
    repo.write("x.txt", "x\n");
    repo.commit("Release-Class: routine\nRelease-Class-Surfaces: none");
    repo.git(&["tag", "acme-v2.0.0-rc.1"]);
    let (got, why) = resolve(&repo, "push", "", "tag", "acme-v2.0.0-rc.1");
    assert_eq!(got, "sensible");
    assert!(why.contains("not an annotated tag"), "{why}");
    // A branch carries no class.
    assert_eq!(resolve(&repo, "push", "", "branch", "main").0, "sensible");
}

#[test]
fn a_dispatch_names_the_class_only_on_a_dispatch() {
    let repo = Repo::new();
    repo.annotated(
        "acme-v1.0.0-rc.1",
        "Release candidate\n\nRelease-Class: sensible",
    );
    for class in ["routine", "sensible"] {
        let (got, why) = resolve(&repo, "workflow_dispatch", class, "branch", "main");
        assert_eq!(got, class);
        assert_eq!(why, "workflow_dispatch input");
    }
    let (got, why) = resolve(&repo, "workflow_dispatch", "Routine", "branch", "main");
    assert_eq!(got, "sensible");
    assert!(why.contains("invalid workflow_dispatch input"), "{why}");
    // A value forwarded on a tag push does not outrank the tag.
    let (got, _) = resolve(&repo, "push", "routine", "tag", "acme-v1.0.0-rc.1");
    assert_eq!(got, "sensible");
    // An empty dispatch reads the ref.
    let (got, why) = resolve(&repo, "workflow_dispatch", "", "tag", "acme-v1.0.0-rc.1");
    assert_eq!((got.as_str(), why.as_str()), ("sensible", "tag annotation"));
}

fn last(repo: &Repo, ref_type: &str, name: &str) -> Ran {
    crate::harness::run(
        &body(GATE, "candidate", "last"),
        &[
            ("CANDIDATE_TAG", "acme-v{version}-rc.{rc}"),
            ("RELEASE_TAG", "acme-v{version}"),
            ("REF_TYPE", ref_type),
            ("REF_NAME", name),
        ],
        &repo.dir.0,
    )
}

#[test]
fn the_base_is_the_last_published_release_never_a_candidate() {
    let repo = Repo::new();
    for t in [
        "acme-v1.2.0",
        "acme-v1.10.0",
        "acme-v1.9.0-rc.1",
        "acme-v2.0.0-rc.1",
        "acme-vnext",
        "other-v9.0.0",
    ] {
        repo.git(&["tag", t]);
    }
    let r = last(&repo, "tag", "acme-v2.0.0-rc.1");
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("last-tag").as_deref(), Some("acme-v1.10.0"));
    assert_eq!(r.output("candidate").as_deref(), Some("acme-v2.0.0-rc.1"));
    // A candidate is measured from the highest release below its version.
    let r = last(&repo, "tag", "acme-v1.9.0-rc.1");
    assert_eq!(r.output("last-tag").as_deref(), Some("acme-v1.2.0"));
    // A dispatch from a branch is no candidate, measured from the newest.
    let r = last(&repo, "branch", "main");
    assert_eq!(r.output("last-tag").as_deref(), Some("acme-v1.10.0"));
    assert_eq!(r.output("candidate").as_deref(), Some(""));
    // Nothing published below it: the product's first release, measured from
    // the repository's root, with no tag set by hand. Green, and said.
    let r = last(&repo, "tag", "acme-v1.0.0-rc.1");
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("last-tag").as_deref(), Some(""));
    assert_eq!(r.output("candidate").as_deref(), Some("acme-v1.0.0-rc.1"));
    assert!(
        r.log.contains("::notice title=First release::No published release (acme-v*) below acme-v1.0.0-rc.1: this is the product's first release, measured from the repository's root"),
        "{}",
        r.log
    );
}

#[test]
fn a_first_candidate_with_no_earlier_release_is_measured_from_the_root() {
    // A brand-new repository: candidates only, never a release.
    let repo = Repo::new();
    repo.git(&["tag", "acme-v0.2.0-rc.1"]);
    let r = last(&repo, "tag", "acme-v0.2.0-rc.1");
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("last-tag").as_deref(), Some(""));
    assert!(
        r.log
            .contains("last published release: none (first release"),
        "{}",
        r.log
    );

    // The suites then run on the whole history, and the pack says so.
    let mut g = Gate::new(r#"[{"id": "unit", "run": "echo 1 passed"}]"#);
    g.repo.git(&["tag", "-d", "acme-v1.0.0"]);
    g.set("LAST_TAG", "");
    g.set(
        "GUARDS",
        r#"[{"name": "range", "run": "test \"$BASE_SHA\" = \"$(git hash-object -t tree /dev/null)\" && test -z \"$BASE_REF\" && git diff --name-only \"$BASE_SHA\" \"$HEAD_SHA\" | grep -qx README.md"}]"#,
    );
    let d = g.job("");
    assert_eq!(d.prepare.code, 0, "{}", d.prepare.log);
    assert!(
        d.prepare
            .log
            .contains("Evidence prepared since the repository's root (first release)."),
        "{}",
        d.prepare.log
    );
    assert_eq!(d.enforce.code, 0, "{}", d.enforce.log);
    let s = d.suites();
    assert_eq!(s["from_tag"], Value::Null);
    assert_eq!(d.failed(), Vec::<String>::new(), "{s}");
    assert_eq!(s["suites"][0]["name"], "pr_guards");
    let run: Value =
        serde_json::from_str(&std::fs::read_to_string(d.pack.join("run.json")).unwrap()).unwrap();
    assert_eq!(run["lastReleaseTag"], Value::Null);
    // Every commit, the first included, and every file of the first commit.
    let commits = std::fs::read_to_string(d.pack.join("commits.tsv")).unwrap();
    assert!(commits.contains("\tfeat: g"), "{commits}");
    assert!(commits.contains("\tinitial"), "{commits}");
    let diff = std::fs::read_to_string(d.pack.join("changes.diff")).unwrap();
    assert!(diff.contains("+++ b/README.md"), "{diff}");
    let report = std::fs::read_to_string(d.pack.join("qa-report.md")).unwrap();
    assert!(
        report.contains(
            "Previous release: none (first release, measured from the repository's root)"
        ),
        "{report}"
    );
    assert!(
        std::fs::read_to_string(g.state.0.join("summary.md"))
            .unwrap()
            .contains("Previous release: none (first release"),
    );
}

// ---------------------------------------------------------------- the suites

struct Gate {
    repo: Repo,
    state: Scratch,
    _stubs: Scratch,
    env: Vec<(&'static str, String)>,
}

/// What the suites job left behind.
struct Done {
    prepare: Ran,
    run: Option<Ran>,
    summary: Ran,
    enforce: Ran,
    pack: PathBuf,
}

impl Done {
    fn suites(&self) -> Value {
        let text = std::fs::read_to_string(self.pack.join("results/suites.json"))
            .unwrap_or_else(|e| panic!("no suites.json ({e}): {}", self.summary.log));
        serde_json::from_str(&text).unwrap()
    }

    fn outcomes(&self) -> Vec<(String, String)> {
        self.suites()["suites"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| {
                (
                    s["name"].as_str().unwrap().to_string(),
                    s["outcome"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    fn failed(&self) -> Vec<String> {
        self.suites()["failed"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect()
    }
}

fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
    v.iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

impl Gate {
    /// A candidate one commit past the release `acme-v1.0.0`.
    fn new(suites: &str) -> Gate {
        let repo = Repo::new();
        repo.git(&["tag", "acme-v1.0.0"]);
        repo.write("src/lib.rs", "pub fn g() {}\n");
        let sha = repo.commit("feat: g");
        let state = Scratch::new("runner-temp");
        let stubs = Scratch::new("stubs");
        std::fs::write(state.0.join("github-env"), "").unwrap();
        let env = vec![
            ("PATH", path_with(&stubs.0)),
            ("EVIDENCE_DIR", "release-gate-evidence".to_string()),
            ("SUITES", suites.to_string()),
            ("RERUN_GUARDS", "true".to_string()),
            ("GUARDS", "[]".to_string()),
            ("UPSTREAM_RESULT", "success".to_string()),
            ("LAST_TAG", "acme-v1.0.0".to_string()),
            ("CANDIDATE", "acme-v1.1.0-rc.1".to_string()),
            ("RELEASE_CLASS", "sensible".to_string()),
            ("GITHUB_SHA", sha),
            ("GITHUB_REF_NAME", "acme-v1.1.0-rc.1".to_string()),
            ("GITHUB_RUN_ID", "41".to_string()),
            ("GITHUB_REPOSITORY", "example-org/acme".to_string()),
            ("GITHUB_SERVER_URL", "https://github.com".to_string()),
            ("RUNNER_TEMP", state.0.display().to_string()),
            (
                "GITHUB_ENV",
                state.0.join("github-env").display().to_string(),
            ),
            (
                "GITHUB_STEP_SUMMARY",
                state.0.join("summary.md").display().to_string(),
            ),
            (
                "EXCEPTIONS_FILE",
                ".github/release-gate/pr-guard-exceptions.json".to_string(),
            ),
            ("CHANGELOG_PATH", String::new()),
            ("STATIC_ENV", "{}".to_string()),
            ("INCLUDE", "[]".to_string()),
        ];
        Gate {
            repo,
            state,
            _stubs: stubs,
            env,
        }
    }

    fn set(&mut self, key: &'static str, value: &str) -> &mut Gate {
        self.env.retain(|(k, _)| *k != key);
        self.env.push((key, value.to_string()));
        self
    }

    fn get(&self, key: &str) -> String {
        self.env
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }

    /// The variables earlier steps wrote to `$GITHUB_ENV`.
    fn exported(&self) -> Vec<(String, String)> {
        let text = std::fs::read_to_string(self.state.0.join("github-env")).unwrap();
        let mut out = Vec::new();
        let mut lines = text.lines();
        while let Some(line) = lines.next() {
            if let Some((k, delimiter)) = line.split_once("<<") {
                let value: Vec<&str> = lines.by_ref().take_while(|l| *l != delimiter).collect();
                out.push((k.to_string(), value.join("\n")));
            } else if let Some((k, v)) = line.split_once('=') {
                out.push((k.to_string(), v.to_string()));
            }
        }
        out
    }

    fn step(&self, id: &str, extra: &[(String, String)]) -> Ran {
        let mut env: Vec<(String, String)> = self
            .env
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect();
        for (k, v) in self.exported().into_iter().chain(extra.iter().cloned()) {
            env.retain(|(key, _)| *key != k);
            env.push((k, v));
        }
        let leaked: Vec<(&str, String)> =
            env.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
        run_owned(&body(GATE, "suites", id), &leaked, &self.repo.dir.0)
    }

    /// The suites job, step by step, under the `if:`s the YAML declares.
    fn job(&self, setup: &str) -> Done {
        let prepare = self.step("prepare", &[]);
        let prepared = prepare.code == 0;
        let setup_outcome =
            if prepared && self.get("UPSTREAM_RESULT") == "success" && !setup.is_empty() {
                let r = self.step("setup", &pairs(&[("SETUP_COMMAND", setup)]));
                if r.code == 0 { "success" } else { "failure" }
            } else {
                "skipped"
            };
        let run = prepared.then(|| {
            self.step(
                "run",
                &pairs(&[
                    ("SETUP_OUTCOME", setup_outcome),
                    ("HAS_SETUP", if setup.is_empty() { "false" } else { "true" }),
                ]),
            )
        });
        let _collect = self.step("Collect the extra evidence", &[]);
        let summary = self.step("summary", &[]);
        let enforce = self.step("Enforce the release gate", &[]);
        Done {
            prepare,
            run,
            summary,
            enforce,
            pack: self.repo.dir.0.join("release-gate-evidence"),
        }
    }
}

#[test]
fn a_green_gate_writes_the_whole_pack_and_reports_once() {
    let g = Gate::new(
        r#"[{"id": "unit", "run": "echo 3 passed", "artifacts": ["junit.xml"]},
            {"id": "build", "run": "echo built > \"$EVIDENCE_DIR/build.txt\""}]"#,
    );
    let d = g.job("");
    assert_eq!(d.prepare.code, 0, "{}", d.prepare.log);
    assert_eq!(d.enforce.code, 0, "{}", d.enforce.log);
    assert!(d.enforce.log.contains("All suites green."));
    let s = d.suites();
    assert_eq!(s["schema_version"], 1);
    assert_eq!(s["sha"], g.get("GITHUB_SHA").as_str());
    assert_eq!(s["ref"], "acme-v1.1.0-rc.1");
    assert_eq!(s["candidate"], "acme-v1.1.0-rc.1");
    assert_eq!(s["from_tag"], "acme-v1.0.0");
    assert_eq!(s["release_class"], "sensible");
    assert_eq!(
        s["run_url"],
        "https://github.com/example-org/acme/actions/runs/41"
    );
    assert_eq!(s["failed"], serde_json::json!([]));
    assert_eq!(
        s["suites"],
        serde_json::json!([
            {"name": "unit", "outcome": "success", "artifacts": ["junit.xml", "unit.log"]},
            {"name": "build", "outcome": "success", "artifacts": ["build.log"]}
        ])
    );
    assert_eq!(d.summary.output("failed").as_deref(), Some(""));
    // The attestation is the last thing the report step writes.
    assert_eq!(d.summary.outputs.lines().last(), Some("written=true"));
    for f in [
        "qa-report.md",
        "results/qa-report.md",
        "run.json",
        "commits.tsv",
        "changes-stat.txt",
        "changes.diff",
        "unit.log",
        "build.txt",
    ] {
        assert!(d.pack.join(f).is_file(), "{f}");
    }
    let run: Value =
        serde_json::from_str(&std::fs::read_to_string(d.pack.join("run.json")).unwrap()).unwrap();
    assert_eq!(run["sha"], g.get("GITHUB_SHA").as_str());
    assert_eq!(run["lastReleaseTag"], "acme-v1.0.0");
    let commits = std::fs::read_to_string(d.pack.join("commits.tsv")).unwrap();
    assert!(commits.ends_with("\tfeat: g"), "{commits}");
    assert!(
        std::fs::read_to_string(g.state.0.join("summary.md"))
            .unwrap()
            .contains("| unit | success | yes |")
    );
}

#[test]
fn a_broken_suite_is_named_once_and_its_dependents_are_skipped_upstream() {
    let g = Gate::new(
        r#"[{"id": "build", "run": "echo broken; exit 3"},
            {"id": "smoke", "run": "echo never", "requires": ["build"]},
            {"id": "lint", "run": "true"},
            {"id": "audit", "run": "echo never", "requires": ["lint", "smoke"]},
            {"id": "piped", "run": "false | tee /dev/null"}]"#,
    );
    let d = g.job("");
    assert_eq!(
        d.outcomes(),
        [
            ("build", "failure"),
            ("smoke", "skipped-upstream"),
            ("lint", "success"),
            ("audit", "skipped-upstream"),
            // pipefail: the suite's status, never tee's.
            ("piped", "failure"),
        ]
        .map(|(a, b)| (a.to_string(), b.to_string()))
    );
    assert_eq!(d.failed(), ["build", "smoke", "audit", "piped"]);
    let s = d.suites();
    assert_eq!(s["suites"][0]["detail"], "exit 3");
    assert_eq!(s["suites"][1]["detail"], "build is failure");
    assert_eq!(s["suites"][3]["detail"], "smoke is skipped-upstream");
    assert_eq!(s["suites"][1]["artifacts"], serde_json::json!([]));
    assert_eq!(
        d.summary.output("failed").as_deref(),
        Some("build,smoke,audit,piped")
    );
    assert_eq!(d.enforce.code, 1);
    assert!(
        d.enforce
            .log
            .contains("Release gate is RED. Failing suites: build, smoke, audit, piped"),
        "{}",
        d.enforce.log
    );
    assert!(d.enforce.log.contains("skipped-upstream\tsmoke"));
}

#[test]
fn a_failed_preflight_or_setup_runs_no_suite_and_is_red() {
    let suites =
        r#"[{"id": "unit", "run": "touch \"$EVIDENCE_DIR/ran\""}, {"id": "e2e", "run": "true"}]"#;
    for upstream in ["failure", "skipped", "cancelled"] {
        let mut g = Gate::new(suites);
        g.set("UPSTREAM_RESULT", upstream);
        let d = g.job("");
        assert!(d.run.as_ref().unwrap().log.contains("no suite runs"));
        assert_eq!(
            d.outcomes(),
            [("unit", "not-run"), ("e2e", "not-run")].map(|(a, b)| (a.to_string(), b.to_string()))
        );
        assert!(!d.pack.join("ran").exists());
        assert_eq!(d.enforce.code, 1);
    }
    let g = Gate::new(suites);
    let d = g.job("exit 1");
    assert_eq!(d.failed(), ["unit", "e2e"]);
    assert!(!d.pack.join("ran").exists());
}

#[test]
fn the_setup_and_the_static_env_reach_the_suites() {
    let mut g = Gate::new(
        r#"[{"id": "env", "run": "test \"$GREETING\" = 'hello world' && test \"$FROM_SETUP\" = yes && test -n \"$HEAD_SHA\" && test \"$LAST_TAG\" = acme-v1.0.0"}]"#,
    );
    g.set("STATIC_ENV", r#"{"GREETING": "hello world"}"#);
    let d = g.job("echo FROM_SETUP=yes >> \"$GITHUB_ENV\"");
    assert_eq!(
        d.failed(),
        Vec::<String>::new(),
        "{}",
        d.run.as_ref().unwrap().log
    );
    for bad in [
        r#"{"GITHUB_TOKEN": "x"}"#,
        r#"{"A B": "x"}"#,
        r#"{"A": {"nested": 1}}"#,
        "[]",
    ] {
        let mut g = Gate::new(r#"[{"id": "unit", "run": "true"}]"#);
        g.set("STATIC_ENV", bad);
        let d = g.job("");
        assert_eq!(d.prepare.code, 1, "{bad}");
        assert_eq!(d.failed(), ["unit"], "{bad}");
    }
}

#[test]
fn zero_tests_is_never_a_pass() {
    let g = Gate::new(
        r#"[{"id": "missing", "run": "true", "expect-nonempty": "report.json"},
            {"id": "empty", "run": ": > \"$EVIDENCE_DIR/empty.log\"", "expect-nonempty": "empty.log"},
            {"id": "zero", "run": "echo '{\"tests\": []}' > \"$EVIDENCE_DIR/zero.json\"", "expect-nonempty": {"file": "zero.json", "count": ".tests | length"}},
            {"id": "garbled", "run": "echo '{' > \"$EVIDENCE_DIR/g.json\"", "expect-nonempty": {"file": "g.json", "count": ".tests | length"}},
            {"id": "three", "run": "echo '{\"tests\": [1, 2, 3]}' > \"$EVIDENCE_DIR/three.json\"", "expect-nonempty": {"file": "three.json", "count": ".tests | length"}}]"#,
    );
    let d = g.job("");
    assert_eq!(d.failed(), ["missing", "empty", "zero", "garbled"]);
    let s = d.suites();
    assert_eq!(
        s["suites"][0]["detail"],
        "no test executed: report.json is missing or empty"
    );
    assert_eq!(
        s["suites"][2]["detail"],
        "no test executed: .tests | length of zero.json is 0"
    );
    assert_eq!(
        s["suites"][3]["detail"],
        "no test executed: .tests | length of g.json is ?"
    );
}

#[test]
fn a_suite_green_only_after_a_retry_is_partial_and_blocks() {
    let g = Gate::new(
        r#"[{"id": "quality", "run": "echo '{\"flakyCount\": 2}' > \"$EVIDENCE_DIR/q.json\"", "flaky": {"file": "q.json", "count": ".flakyCount"}},
            {"id": "steady", "run": "echo '{\"flakyCount\": 0}' > \"$EVIDENCE_DIR/s.json\"", "flaky": {"file": "s.json", "count": ".flakyCount"}},
            {"id": "unreadable", "run": "true", "flaky": {"file": "none.json", "count": ".flakyCount"}}]"#,
    );
    let d = g.job("");
    let s = d.suites();
    assert_eq!(s["suites"][0]["outcome"], "partial");
    assert_eq!(s["suites"][0]["flaky"], 2);
    assert_eq!(s["suites"][1]["outcome"], "success");
    assert_eq!(s["suites"][2]["outcome"], "partial");
    assert_eq!(d.failed(), ["quality", "unreadable"]);
    let report = std::fs::read_to_string(d.pack.join("qa-report.md")).unwrap();
    assert!(
        report.contains("| quality | PARTIAL (2 flaky): 2 flaky test(s) | yes |"),
        "{report}"
    );
}

#[test]
fn a_suite_that_never_started_or_never_finished_is_not_a_pass() {
    let g = Gate::new(
        r#"[{"id": "a", "run": "true"}, {"id": "b", "run": "true"}, {"id": "c", "run": "true"}, {"id": "d", "run": "true"}]"#,
    );
    // What a killed job leaves: a finished suite, one cut mid-run, the rest
    // never reached; the report step still runs.
    let results = g.repo.dir.0.join("release-gate-evidence/results");
    std::fs::create_dir_all(&results).unwrap();
    std::fs::write(
        results.join("outcomes.tsv"),
        "a\tstarted\t\t\na\tsuccess\t\t\nb\tstarted\t\t\nd\tskipped\t\t\n",
    )
    .unwrap();
    let summary = g.step("summary", &[]);
    assert_eq!(summary.code, 0, "{}", summary.log);
    let d = Done {
        prepare: g.step("Enforce the release gate", &[]),
        run: None,
        summary,
        enforce: g.step("Enforce the release gate", &[]),
        pack: g.repo.dir.0.join("release-gate-evidence"),
    };
    assert_eq!(
        d.outcomes(),
        [
            ("a", "success"),
            ("b", "cancelled"),
            ("c", "not-run"),
            ("d", "skipped-upstream")
        ]
        .map(|(a, b)| (a.to_string(), b.to_string()))
    );
    assert_eq!(d.enforce.code, 1);
}

#[test]
fn suites_that_cannot_be_read_never_make_a_green_gate() {
    let cases: &[(&str, &str)] = &[
        ("not json", "not JSON"),
        ("[]", "declares no suite"),
        (r#"{"id": "a"}"#, "not a JSON array"),
        (r#"[{"id": "Bad", "run": "true"}]"#, "id must match"),
        (
            r#"[{"id": "a", "run": "true"}, {"id": "a", "run": "true"}]"#,
            "duplicate id",
        ),
        (r#"[{"id": "a", "run": ""}]"#, "run must be a command"),
        (
            r#"[{"id": "a", "run": "true", "shell": "sh"}]"#,
            "shell must be bash",
        ),
        (
            r#"[{"id": "a", "run": "true", "requires": ["b"]}, {"id": "b", "run": "true"}]"#,
            "requires must name suites declared before it",
        ),
        (
            r#"[{"id": "a", "run": "true", "timeout-minutes": 0}]"#,
            "timeout-minutes",
        ),
        (
            r#"[{"id": "a", "run": "true", "expect-nonempty": {"file": "x"}}]"#,
            "expect-nonempty",
        ),
        (
            r#"[{"id": "a", "run": "true", "flaky": {"count": ".n"}}]"#,
            "flaky must be",
        ),
        (
            r#"[{"id": "a", "run": "true", "artifacts": [1]}]"#,
            "artifacts must be",
        ),
        (r#"[{"id": "pr_guards", "run": "true"}]"#, "reserved"),
    ];
    for (suites, words) in cases {
        let g = Gate::new(suites);
        let d = g.job("");
        assert_eq!(d.prepare.code, 1, "{suites}");
        assert!(d.prepare.log.contains(words), "{suites}: {}", d.prepare.log);
        assert!(!d.failed().is_empty(), "{suites}");
        assert_eq!(d.enforce.code, 1, "{suites}");
    }
    let g = Gate::new("not json");
    assert_eq!(g.job("").failed(), ["suites_input"]);
}

#[test]
fn the_pr_guards_rerun_against_the_whole_release_range() {
    let mut g = Gate::new(r#"[{"id": "unit", "run": "true"}]"#);
    g.set(
        "GUARDS",
        r#"[{"name": "range", "run": "test \"$BASE_SHA\" = \"$(git rev-parse acme-v1.0.0^{commit})\" && test \"$HEAD_SHA\" = \"$GITHUB_SHA\" && test \"$BASE_REF\" = acme-v1.0.0"},
            {"name": "broken", "run": "echo no; exit 1"},
            {"name": "pr-only", "run": "exit 1"}]"#,
    );
    g.repo.write(
        ".github/release-gate/pr-guard-exceptions.json",
        r#"{"pr-only": "reads the PR's own diff, which the gate has none of"}"#,
    );
    let d = g.job("");
    assert_eq!(d.prepare.code, 0, "{}", d.prepare.log);
    let s = d.suites();
    assert_eq!(s["suites"][0]["name"], "pr_guards");
    assert_eq!(s["suites"][0]["outcome"], "failure");
    assert_eq!(s["suites"][0]["detail"], "failed: broken");
    assert_eq!(d.failed(), ["pr_guards"]);
    let log = std::fs::read_to_string(d.pack.join("pr_guards.log")).unwrap();
    assert!(
        log.contains("Guard 'pr-only' is not run at the gate"),
        "{log}"
    );

    // Without the re-run, the guards are not part of the gate.
    g.set("RERUN_GUARDS", "false");
    let d = g.job("");
    assert_eq!(d.outcomes(), [("unit".to_string(), "success".to_string())]);

    // An exception needs a reason, and must excuse a guard that exists.
    for exceptions in [
        r#"{"pr-only": "short"}"#,
        r#"{"gone": "a guard removed long ago"}"#,
        "[]",
    ] {
        let mut g = Gate::new(r#"[{"id": "unit", "run": "true"}]"#);
        g.set("GUARDS", r#"[{"name": "pr-only", "run": "true"}]"#);
        g.repo
            .write(".github/release-gate/pr-guard-exceptions.json", exceptions);
        let d = g.job("");
        assert_eq!(d.prepare.code, 1, "{exceptions}");
        assert!(
            d.prepare.log.contains("written reason"),
            "{}",
            d.prepare.log
        );
        assert_eq!(d.enforce.code, 1);
    }
}

#[test]
fn the_extra_evidence_is_copied_and_stays_inside_the_workspace() {
    let mut g = Gate::new(
        r#"[{"id": "e2e", "run": "mkdir -p app/report && echo x > app/report/index.html"}]"#,
    );
    g.set("INCLUDE", r#"["app/report*", "nothing-here/*"]"#);
    let d = g.job("");
    assert!(d.pack.join("app/report/index.html").is_file());
    let mut g = Gate::new(r#"[{"id": "e2e", "run": "true"}]"#);
    g.set("INCLUDE", r#"["../outside"]"#);
    let r = g.step("Collect the extra evidence", &[]);
    assert_eq!(r.code, 1);
    assert!(r.log.contains("inside the workspace"), "{}", r.log);
}

#[test]
fn the_enforce_step_reads_only_the_file_and_fails_without_it() {
    let g = Gate::new(r#"[{"id": "unit", "run": "true"}]"#);
    let r = g.step("Enforce the release gate", &[]);
    assert_eq!(r.code, 1);
    assert!(r.log.contains("No suite summary was written"), "{}", r.log);
}

// ----------------------------------------------------------------- the verdict

fn verdict(
    stubs: &Path,
    candidate: &str,
    suites: &str,
    upstream: &str,
    failed: &str,
    written: &str,
) -> Ran {
    let dir = Scratch::new("verdict");
    crate::harness::run(
        &body(GATE, "gate-status", "verdict"),
        &[
            ("PATH", &path_with(stubs)),
            ("CANDIDATE_RESULT", candidate),
            ("SUITES_RESULT", suites),
            ("UPSTREAM_RESULT", upstream),
            ("FAILED_SUITES", failed),
            ("SUMMARY_WRITTEN", written),
            ("REF_TYPE", "tag"),
            ("SHA", "abc123"),
            (
                "RUN_URL",
                "https://github.com/example-org/acme/actions/runs/41",
            ),
            ("GH_TOKEN", "token"),
            ("GH_REPO", "example-org/acme"),
        ],
        &dir.0,
    )
}

#[test]
fn gate_status_is_green_only_when_the_suites_job_succeeded() {
    let stubs = Scratch::new("stubs");
    stub(&stubs.0, "gh", "true");
    let r = verdict(&stubs.0, "success", "success", "success", "", "true");
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("conclusion").as_deref(), Some("success"));
    let gh = calls(&stubs.0, "gh");
    assert_eq!(
        gh,
        [
            "api --method POST repos/example-org/acme/statuses/abc123 -f state=success -f context=release-gate -f target_url=https://github.com/example-org/acme/actions/runs/41 -f description=green"
        ]
    );
    let cases: &[(&str, &str, &str, &str, &str, &str)] = &[
        (
            "success",
            "failure",
            "failure",
            "",
            "",
            "preflight (ended 'failure'",
        ),
        (
            "failure",
            "skipped",
            "success",
            "",
            "",
            "candidate (the RC could not be read",
        ),
        (
            "success",
            "skipped",
            "success",
            "",
            "",
            "suites never started",
        ),
        (
            "success",
            "failure",
            "success",
            "",
            "",
            "suites never reported",
        ),
        // Only the literal `true` is a written report.
        (
            "success",
            "failure",
            "success",
            "unit",
            "TRUE",
            "suites never reported",
        ),
        (
            "success",
            "cancelled",
            "success",
            "",
            "",
            "suites never reported",
        ),
        (
            "success",
            "failure",
            "success",
            "",
            "true",
            "outside the QA suites",
        ),
        (
            "success", "failure", "success", "unit,e2e", "true", "suites",
        ),
    ];
    for (candidate, suites, upstream, failed, written, stage) in cases {
        let stubs = Scratch::new("stubs");
        stub(&stubs.0, "gh", "true");
        let r = verdict(&stubs.0, candidate, suites, upstream, failed, written);
        assert_eq!(r.code, 1, "{suites}: {}", r.log);
        assert_eq!(r.output("conclusion").as_deref(), Some("failure"));
        assert!(
            r.output("stage").unwrap().contains(stage),
            "{stage}: {:?}",
            r.output("stage")
        );
        let gh = calls(&stubs.0, "gh");
        assert!(gh[0].contains("-f state=failure"), "{gh:?}");
    }
    let stubs = Scratch::new("stubs");
    stub(&stubs.0, "gh", "true");
    let r = verdict(&stubs.0, "success", "failure", "success", "", "");
    assert!(r.log.contains("gh run rerun"), "the abort hint: {}", r.log);
    let r = verdict(
        &stubs.0, "success", "failure", "success", "unit,e2e", "true",
    );
    assert!(
        calls(&stubs.0, "gh")
            .last()
            .unwrap()
            .ends_with("-f description=unit,e2e")
    );
    assert!(!r.log.contains("Hint"), "{}", r.log);
}

#[test]
fn a_failed_status_write_never_turns_the_verdict() {
    let stubs = Scratch::new("stubs");
    stub(&stubs.0, "gh", "exit 1");
    let r = verdict(&stubs.0, "success", "success", "success", "", "true");
    assert_eq!(r.code, 0, "{}", r.log);
    assert!(
        r.log
            .contains("::warning::Could not publish the commit status")
    );
    let r = verdict(&stubs.0, "success", "failure", "success", "unit", "true");
    assert_eq!(r.code, 1);
}

// ---------------------------------------------------------- the rerun decision

fn decide(file: &str, job: &str, failed: &str, written: &str, flakes: Option<&str>) -> Ran {
    let dir = Scratch::new("decide");
    if let Some(f) = flakes {
        std::fs::write(dir.0.join("known-flakes.json"), f).unwrap();
    }
    crate::harness::run(
        &body(file, job, "decide"),
        &[
            ("FAILED_SUITES", failed),
            ("SUMMARY_WRITTEN", written),
            ("KNOWN_FLAKES", "known-flakes.json"),
            ("TODAY", "2026-10-07"),
        ],
        &dir.0,
    )
}

#[test]
fn both_halves_decide_with_the_same_script() {
    assert_eq!(
        body(GATE, "auto-rerun-decision", "decide"),
        body(RERUN, "auto-rerun", "decide")
    );
}

#[test]
fn a_rerun_is_decided_only_for_an_abort_or_known_unexpired_flakes() {
    let flakes = r#"[
        "not an object",
        {"suite": "e2e", "ticket": "ABC-1", "expires": "2026-12-31", "note": "timing"},
        {"suite": "audit", "ticket": "ABC-2", "expires": "2026-10-07"},
        {"suite": "old", "ticket": "ABC-3", "expires": "2026-10-06"},
        {"suite": "bad-date", "ticket": "ABC-4", "expires": "2026-02-30"},
        {"suite": "word", "ticket": "ABC-5", "expires": "soon"},
        {"suite": "number", "ticket": "ABC-6", "expires": 20261231},
        {"suite": "none", "ticket": "ABC-7"}
    ]"#;
    let cases: &[(&str, &str, bool, &str)] = &[
        (
            "",
            "",
            true,
            "no suites summary produced (probable runner abort)",
        ),
        // The abort is checked first, whatever else is claimed.
        ("unit", "false", true, "probable runner abort"),
        ("", "true", false, "named no failed suite"),
        (
            " e2e ",
            " TRUE ",
            true,
            "every failed suite matches an unexpired known flake (ABC-1)",
        ),
        ("e2e,audit", "true", true, "(ABC-1, ABC-2)"),
        (
            "e2e,unit",
            "true",
            false,
            "covered by an unexpired known flake: unit",
        ),
        ("old", "true", false, ": old"),
        ("bad-date", "true", false, ": bad-date"),
        ("word,number,none", "true", false, ": word, number, none"),
    ];
    for file in [(GATE, "auto-rerun-decision"), (RERUN, "auto-rerun")] {
        for (failed, written, rerun, reason) in cases {
            let r = decide(file.0, file.1, failed, written, Some(flakes));
            assert_eq!(r.code, 0, "{}", r.log);
            assert_eq!(
                r.output("rerun").as_deref(),
                Some(if *rerun { "true" } else { "false" }),
                "{failed:?} {written:?}"
            );
            let got = r.output("reason").unwrap();
            assert!(got.contains(reason), "{failed:?}: {got}");
        }
    }
}

#[test]
fn a_missing_or_garbage_allow_list_excuses_nothing() {
    for flakes in [
        None,
        Some("{not json"),
        Some(r#"{"suite": "e2e"}"#),
        Some(""),
    ] {
        let r = decide(GATE, "auto-rerun-decision", "e2e", "true", flakes);
        assert_eq!(r.code, 0, "{}", r.log);
        assert_eq!(r.output("rerun").as_deref(), Some("false"), "{flakes:?}");
        assert!(r.log.contains("::warning::"), "{flakes:?}: {}", r.log);
        // An abort is still an abort.
        let r = decide(GATE, "auto-rerun-decision", "", "", flakes);
        assert_eq!(r.output("rerun").as_deref(), Some("true"));
    }
    let r = decide(GATE, "auto-rerun-decision", "e2e", "true", Some("[]"));
    assert_eq!(r.output("rerun").as_deref(), Some("false"));
    assert!(
        !r.log.contains("::warning::"),
        "the shipped empty list loads: {}",
        r.log
    );
}

#[test]
fn a_hand_edited_ticket_cannot_inject_an_output() {
    let r = decide(
        GATE,
        "auto-rerun-decision",
        "e2e",
        "true",
        Some(
            r#"[{"suite": "e2e", "ticket": "ABC-1\nrerun=true\n\tmore", "expires": "2026-12-31"}]"#,
        ),
    );
    assert_eq!(r.outputs.lines().count(), 2, "{}", r.outputs);
    assert_eq!(
        r.output("reason").as_deref(),
        Some("every failed suite matches an unexpired known flake (ABC-1 rerun=true more)")
    );
}

// ------------------------------------------------------ the executing half

fn rerun_step(id: &str, stubs: &Path, cwd: &Path, extra: &[(&str, &str)]) -> Ran {
    let mut env = vec![
        ("PATH", path_with(stubs)),
        ("GH_TOKEN", "token".to_string()),
        ("GH_REPO", "example-org/acme".to_string()),
        ("SOURCE_RUN_ID", "41".to_string()),
        ("SOURCE_SHA", "abc123".to_string()),
        ("SUITES_JOB", "suites".to_string()),
    ];
    for (k, v) in extra {
        env.push((k, v.to_string()));
    }
    run_owned(&body(RERUN, "auto-rerun", id), &env, cwd)
}

/// A `gh api` answering the run's jobs through the caller's `--jq`.
fn gh_jobs(stubs: &Path, jobs: &str, fail: bool) {
    std::fs::write(stubs.join("jobs.json"), jobs).unwrap();
    stub(
        stubs,
        "gh",
        &format!(
            r#"if [ "{fail}" = true ]; then exit 1; fi
filter=''
while [ $# -gt 0 ]; do
  if [ "$1" = --jq ]; then filter="$2"; fi
  shift
done
jq -r "$filter" "$(dirname "$0")/jobs.json""#
        ),
    );
}

#[test]
fn only_a_failed_suites_job_is_a_candidate_for_a_rerun() {
    let dir = Scratch::new("cwd");
    let cases = [
        (
            r#"{"jobs": [{"name": "candidate", "conclusion": "success"}, {"name": "gate / suites", "conclusion": "failure"}, {"name": "gate / gate-status", "conclusion": "failure"}]}"#,
            "true",
        ),
        (
            r#"{"jobs": [{"name": "suites", "conclusion": "failure"}]}"#,
            "true",
        ),
        // The prerequisite failed: the suites were skipped, nothing to rerun.
        (
            r#"{"jobs": [{"name": "gate / candidate", "conclusion": "failure"}, {"name": "gate / suites", "conclusion": "skipped"}]}"#,
            "false",
        ),
        (
            r#"{"jobs": [{"name": "gate / my-suites", "conclusion": "failure"}]}"#,
            "false",
        ),
        (r#"{"jobs": []}"#, "false"),
    ];
    for (jobs, failed) in cases {
        let stubs = Scratch::new("stubs");
        gh_jobs(&stubs.0, jobs, false);
        let r = rerun_step("source-job", &stubs.0, &dir.0, &[]);
        assert_eq!(r.code, 0, "{}", r.log);
        assert_eq!(r.output("failed").as_deref(), Some(failed), "{jobs}");
        assert!(
            calls(&stubs.0, "gh")[0]
                .starts_with("api --paginate repos/example-org/acme/actions/runs/41/jobs")
        );
    }
    let stubs = Scratch::new("stubs");
    gh_jobs(&stubs.0, "{}", true);
    let r = rerun_step("source-job", &stubs.0, &dir.0, &[]);
    assert_eq!(
        r.code, 1,
        "an unreadable job list decides nothing: {}",
        r.log
    );
    assert_eq!(r.output("failed"), None);
}

#[test]
fn the_facts_come_from_the_runs_evidence_and_a_missing_pack_is_an_abort() {
    let dir = Scratch::new("cwd");
    let download = |suites: Option<&str>| {
        let stubs = Scratch::new("stubs");
        // The listing answers first (absent and unreadable differ), then
        // the download.
        let body = match suites {
            None => r#"if [ "$1" = api ]; then exit 0; fi
echo 'no artifact matches' >&2; exit 1"#
                .to_string(),
            Some(json) => {
                std::fs::write(stubs.0.join("suites.json"), json).unwrap();
                r#"if [ "$1" = api ]; then echo release-gate-evidence-abc123; exit 0; fi
while [ $# -gt 0 ]; do if [ "$1" = --dir ]; then d="$2"; fi; shift; done
mkdir -p "$d/results" && cp "$(dirname "$0")/suites.json" "$d/results/""#
                    .to_string()
            }
        };
        stub(&stubs.0, "gh", &body);
        let r = rerun_step("evidence", &stubs.0, &dir.0, &[]);
        assert_eq!(r.code, 0, "{}", r.log);
        let c = calls(&stubs.0, "gh");
        assert!(
            c[0].starts_with("api repos/example-org/acme/actions/runs/41/artifacts"),
            "{c:?}"
        );
        if suites.is_some() {
            assert_eq!(
                c[1].split(" --dir ").next().unwrap(),
                "run download 41 --repo example-org/acme --name release-gate-evidence-abc123"
            );
        } else {
            assert_eq!(c.len(), 1, "an absent pack is not downloaded: {c:?}");
        }
        (
            r.output("summary_written").unwrap(),
            r.output("failed_suites").unwrap(),
        )
    };
    assert_eq!(download(None), ("false".into(), "".into()));
    assert_eq!(
        download(Some(r#"{"failed": ["e2e", "unit"]}"#)),
        ("true".into(), "e2e,unit".into())
    );
    // A summary that exists but is unreadable names nothing: never an abort.
    assert_eq!(download(Some("{garbage")), ("true".into(), "".into()));
    assert!(
        body(RERUN, "auto-rerun", "evidence").starts_with("set -uo pipefail\n"),
        "no set -e: a missing artifact is data"
    );
}

#[test]
fn the_rerun_output_is_the_fact_and_a_failed_call_is_red() {
    let dir = Scratch::new("cwd");
    let stubs = Scratch::new("stubs");
    stub(&stubs.0, "gh", "true");
    let r = rerun_step(
        "rerun",
        &stubs.0,
        &dir.0,
        &[("REASON", "probable runner abort")],
    );
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(
        calls(&stubs.0, "gh"),
        ["run rerun 41 --repo example-org/acme --failed"]
    );
    assert_eq!(r.output("reran").as_deref(), Some("true"));
    let stubs = Scratch::new("stubs");
    stub(&stubs.0, "gh", "echo 'HTTP 403' >&2; exit 1");
    let r = rerun_step("rerun", &stubs.0, &dir.0, &[("REASON", "x")]);
    assert_eq!(r.code, 1);
    assert_eq!(r.output("reran"), None);
}

// -------------------------------------------------------------------- shape

fn job_names(file: &str) -> Vec<String> {
    workflow(file)["jobs"]
        .as_hash()
        .unwrap()
        .keys()
        .map(|k| k.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn the_gate_jobs_are_the_interface_and_cannot_read_green_by_skipping() {
    let doc = workflow(GATE);
    assert_eq!(
        job_names(GATE),
        ["candidate", "suites", "gate-status", "auto-rerun-decision"]
    );
    for (id, job) in doc["jobs"].as_hash().unwrap() {
        assert_eq!(
            job["name"].as_str(),
            id.as_str(),
            "the called job name is its id"
        );
    }
    let status = &doc["jobs"]["gate-status"];
    assert_eq!(status["if"].as_str(), Some("always()"));
    assert_eq!(status["runs-on"].as_str(), Some("ubuntu-latest"));
    assert_eq!(status["timeout-minutes"].as_i64(), Some(2));
    assert_eq!(status["permissions"]["statuses"].as_str(), Some("write"));
    let decision = &doc["jobs"]["auto-rerun-decision"];
    assert_eq!(
        decision["if"].as_str(),
        Some("always() && needs.suites.result == 'failure' && github.run_attempt == 1")
    );
    assert_eq!(decision["runs-on"].as_str(), Some("ubuntu-latest"));
    let perms = decision["permissions"].as_hash().unwrap();
    assert_eq!(perms.len(), 1, "contents: read only");
    assert!(
        !body(GATE, "auto-rerun-decision", "decide").contains("gh "),
        "no gh call in the decision"
    );
    assert_eq!(
        decision["outputs"]["eligible"].as_str(),
        Some("${{ steps.decide.outputs.rerun }}")
    );
    let suites = &doc["jobs"]["suites"];
    assert_eq!(
        suites["outputs"]["summary-written"].as_str(),
        Some("${{ steps.summary.outputs.written }}")
    );
    assert_eq!(
        suites["outputs"]["failed-suites"].as_str(),
        Some("${{ steps.summary.outputs.failed }}")
    );
    for job in ["candidate", "suites"] {
        let checkout = &doc["jobs"][job]["steps"][0];
        assert_eq!(
            checkout["with"]["ref"].as_str(),
            Some("${{ github.sha }}"),
            "{job}"
        );
        assert_eq!(
            checkout["with"]["fetch-tags"].as_bool(),
            Some(true),
            "{job}"
        );
        assert_eq!(checkout["with"]["fetch-depth"].as_i64(), Some(0), "{job}");
    }
    let upload = step(&doc, "suites", "Upload the evidence pack");
    assert_eq!(upload["if"].as_str(), Some("always()"));
    assert_eq!(
        upload["with"]["name"].as_str(),
        Some("release-gate-evidence-${{ github.sha }}")
    );
    assert_eq!(upload["with"]["if-no-files-found"].as_str(), Some("error"));
    assert_eq!(
        upload["with"]["path"].as_str(),
        Some("release-gate-evidence")
    );
    let steps = suites["steps"].as_vec().unwrap();
    assert_eq!(
        steps.last().unwrap()["name"].as_str(),
        Some("Enforce the release gate")
    );
    for id in [
        "summary",
        "Collect the extra evidence",
        "Enforce the release gate",
    ] {
        assert_eq!(
            step(&doc, "suites", id)["if"].as_str(),
            Some("always()"),
            "{id}"
        );
    }
    assert!(body(GATE, "suites", "Enforce the release gate").contains("results/suites.json"));
    assert!(!body(GATE, "suites", "Enforce the release gate").contains("steps."));
}

#[test]
fn the_class_never_comes_from_a_dispatch_only_value_or_a_routine_default() {
    let text = std::fs::read_to_string(crate::harness::root().join(GATE)).unwrap();
    assert!(!text.contains("github.event.inputs"));
    assert!(!text.contains("|| 'routine'"));
    assert!(!text.contains("|| \"routine\""));
}

#[test]
fn the_rerun_runs_trusted_code_once_on_a_failed_first_attempt() {
    let doc = workflow(RERUN);
    assert_eq!(job_names(RERUN), ["auto-rerun"]);
    let job = &doc["jobs"]["auto-rerun"];
    assert_eq!(
        job["if"].as_str(),
        Some(
            "github.event.workflow_run.conclusion == 'failure' && github.event.workflow_run.run_attempt == 1"
        )
    );
    assert_eq!(job["runs-on"].as_str(), Some("ubuntu-latest"));
    assert_eq!(job["permissions"]["contents"].as_str(), Some("read"));
    assert_eq!(job["permissions"]["actions"].as_str(), Some("write"));
    assert_eq!(
        job["steps"][0]["with"]["ref"].as_str(),
        Some("${{ github.event.repository.default_branch }}")
    );
    assert_eq!(
        step(&doc, "auto-rerun", "rerun")["if"].as_str(),
        Some("steps.decide.outputs.rerun == 'true'")
    );
    assert_eq!(
        job["outputs"]["reran"].as_str(),
        Some("${{ steps.rerun.outputs.reran }}")
    );
    let evidence = &step(&doc, "auto-rerun", "evidence")["env"];
    assert_eq!(
        evidence["SOURCE_SHA"].as_str(),
        Some("${{ github.event.workflow_run.head_sha }}")
    );
    let text = std::fs::read_to_string(crate::harness::root().join(RERUN)).unwrap();
    assert!(
        !text.contains("needs."),
        "another run's outputs are out of reach"
    );
}

#[test]
fn an_unreadable_artifact_list_decides_no_rerun() {
    let dir = Scratch::new("cwd");
    let stubs = Scratch::new("stubs");
    stub(&stubs.0, "gh", "exit 1");
    let r = rerun_step("evidence", &stubs.0, &dir.0, &[]);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("could not be listed"), "{}", r.log);
}
