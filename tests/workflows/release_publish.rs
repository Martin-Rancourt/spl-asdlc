//! `asdlc-release-publish` (REQ-CI-076..097): the two stages run as
//! shipped, with a stateful `gh` stub and real git repositories where the
//! decision is a git one.

use std::path::Path;

use yaml_rust2::Yaml;

use crate::harness::{Ran, Repo, Scratch, calls, path_with, run, run_body, step, stub, workflow};

const WF: &str = ".github/workflows/asdlc-release-publish.yml";

fn doc() -> Yaml {
    workflow(WF)
}

fn body(job: &str, id: &str) -> String {
    run_body(&step(&doc(), job, id))
}

/// A `gh` whose answers are `cases` (bash `case "$*"` arms), with `$S` the
/// state directory, plus a `sleep` that returns at once.
fn gh(dir: &Path, cases: &str) {
    let script = format!(
        "S='{}'\ncase \"$*\" in\n{cases}\n*) echo \"unexpected gh $*\" >&2; exit 97 ;;\nesac",
        dir.display()
    );
    stub(dir, "gh", &script);
    stub(dir, "sleep", "exit 0");
}

fn put(dir: &Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).unwrap();
}

fn read(dir: &Path, name: &str) -> String {
    std::fs::read_to_string(dir.join(name)).unwrap_or_default()
}

/// Run a step body with the stubs of `dir` first on PATH and a step summary.
fn run_with(script: &str, dir: &Path, cwd: &Path, env: &[(&str, &str)]) -> Ran {
    let path = path_with(dir);
    let summary = dir.join("summary");
    let summary = summary.to_str().unwrap().to_string();
    let mut all: Vec<(&str, &str)> = vec![("PATH", &path), ("GITHUB_STEP_SUMMARY", &summary)];
    all.extend_from_slice(env);
    run(script, &all, cwd)
}

fn git_at(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

// ---------------------------------------------------------------- shape

#[test]
fn each_job_runs_for_its_own_stage_and_the_guard_on_every_call() {
    let d = doc();
    let jobs = &d["jobs"];
    let if_of = |j: &str| jobs[j]["if"].as_str().unwrap_or("").to_string();
    assert!(if_of("verify").contains("inputs.stage == 'verify'"));
    let close = if_of("close");
    assert!(close.contains("inputs.stage == 'close'"), "{close}");
    // Closing proceeds after a successful publication, or on a replay read
    // from the event, never from a value a caller could pass alone.
    assert!(
        close.contains("inputs.publish-result == 'success'"),
        "{close}"
    );
    assert!(
        close.contains("github.event_name == 'workflow_dispatch' && inputs.replay"),
        "{close}"
    );
    assert_eq!(if_of("closing-guard"), "always()");
    assert_eq!(jobs["closing-guard"]["needs"].as_str(), Some("close"));
    for j in ["close", "closing-guard"] {
        assert_eq!(jobs[j]["runs-on"].as_str(), Some("ubuntu-latest"), "{j}");
    }
    assert!(
        jobs["verify"]["runs-on"]
            .as_str()
            .unwrap()
            .contains("fromJSON(inputs.runs-on)")
    );
    // The close job outlasts its poll.
    assert!(jobs["close"]["timeout-minutes"].as_i64().unwrap() >= 40);
    let names: Vec<&str> = jobs
        .as_hash()
        .unwrap()
        .values()
        .map(|j| j["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["verify", "close", "closing-guard"]);
}

#[test]
fn the_app_token_is_minted_right_before_the_writes_and_only_there() {
    let d = doc();
    let secrets = &d["on"]["workflow_call"]["secrets"];
    assert!(!secrets["SPL_ASDLC_APP_ID"].is_badvalue());
    assert!(!secrets["SPL_ASDLC_APP_PRIVATE_KEY"].is_badvalue());
    for (job, consumer) in [("close", "refs")] {
        let steps = d["jobs"][job]["steps"].as_vec().unwrap();
        let token = steps
            .iter()
            .position(|s| s["id"].as_str() == Some("app-token"))
            .unwrap();
        let first = steps
            .iter()
            .position(|s| s["id"].as_str() == Some(consumer))
            .unwrap();
        assert_eq!(token + 1, first, "{job}");
        assert!(
            steps[token]["uses"]
                .as_str()
                .unwrap()
                .starts_with("actions/create-github-app-token@")
        );
        for s in &steps[first..] {
            if let Some(t) = s["env"]["GH_TOKEN"].as_str() {
                assert!(t.contains("steps.app-token.outputs.token"), "{job}: {t}");
            }
        }
    }
    // Verify reads with the job's own token and holds no App secret.
    let verify = format!("{:?}", d["jobs"]["verify"]);
    assert!(!verify.contains("secrets."), "verify reads a secret");
    let perms = &d["jobs"]["verify"]["permissions"];
    assert_eq!(perms["actions"].as_str(), Some("read"));
    assert_eq!(perms["contents"].as_str(), Some("read"));
    assert!(
        perms["pull-requests"].is_badvalue(),
        "verify reads the release, never a PR"
    );
}

#[test]
fn the_reference_guard_is_the_first_step_of_verify() {
    let d = doc();
    let steps = d["jobs"]["verify"]["steps"].as_vec().unwrap();
    assert_eq!(steps[0]["id"].as_str(), Some("reference"));
    assert!(
        steps[1]["uses"]
            .as_str()
            .unwrap()
            .starts_with("actions/checkout@")
    );
}

// ---------------------------------------------------------------- verify

fn reference(release: &str, resume: &str, replay: &str) -> Ran {
    let dir = Scratch::new("ref");
    run(
        &body("verify", "reference"),
        &[
            ("RELEASE_TAG", release),
            ("RESUME_FROM", resume),
            ("CANDIDATE_PATTERN", "acme-v{version}-rc.{rc}"),
            ("REPLAY", replay),
        ],
        &dir.0,
    )
}

#[test]
fn verify_refuses_a_replay_a_candidate_as_release_and_a_resume_from_anything_but_an_rc() {
    assert_eq!(reference("acme-v1.2.0", "", "false").code, 0);
    assert_eq!(
        reference("acme-v1.2.0", "acme-v1.2.0-rc.3", "false").code,
        0
    );
    let r = reference("acme-v1.2.0", "", "true");
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("publishes nothing"), "{}", r.log);
    assert_eq!(reference("acme-v1.2.0-rc.3", "", "false").code, 1);
    assert_eq!(reference("", "", "false").code, 1);
    for bad in [
        "main",
        "release/2026-10-01-10-00-utc",
        "website-v0.3.1",
        "acme-v1.2.0-rc.3x",
    ] {
        let r = reference("acme-v1.2.0", bad, "false");
        assert_eq!(r.code, 1, "{bad}: {}", r.log);
    }
}

/// main, an old cycle's candidate, then a release branch with rc.9, a fix
/// and rc.10.
fn cycle() -> Repo {
    let repo = Repo::new();
    repo.git(&["tag", "acme-v1.1.0-rc.3"]);
    repo.write("src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    repo.commit("feat: work");
    repo.git(&["checkout", "-q", "-b", "release/2026-10-01-10-00-utc"]);
    repo.git(&["tag", "acme-v1.2.0-rc.9"]);
    repo.write("src/lib.rs", "pub fn f() -> u8 { 2 }\n");
    repo.commit("fix: during the cycle");
    repo.git(&["tag", "acme-v1.2.0-rc.10"]);
    repo
}

/// The release tag: on the candidate itself (the bridge's GO), or, with
/// `files`, on a commit of the release branch after it (a person who tagged
/// the branch's tip).
fn release(repo: &Repo, tag: &str, files: &[(&str, &str)]) -> String {
    if !files.is_empty() {
        for (p, t) in files {
            repo.write(p, t);
        }
        repo.commit("docs(qa): after the candidate");
    }
    repo.git(&["tag", tag]);
    repo.git(&["rev-parse", "HEAD"])
}

fn candidate(repo: &Repo, env: &[(&str, &str)]) -> Ran {
    let mut all: Vec<(&str, &str)> = vec![
        ("RELEASE_TAG", "acme-v1.2.0"),
        ("RESUME_FROM", ""),
        ("CANDIDATE_PATTERN", "acme-v{version}-rc.{rc}"),
        ("RELEASE_PATTERN", "acme-v{version}"),
        ("QA_RECORD", ".spl-asdlc/qa/{candidate}/"),
        ("QA_ANGLES", "[]"),
        ("DEFAULT_BRANCH", "main"),
    ];
    for (k, v) in env {
        all.retain(|(x, _)| x != k);
        all.push((k, v));
    }
    run(&body("verify", "candidate"), &all, &repo.dir.0)
}

#[test]
fn the_candidate_is_the_newest_rc_of_exactly_this_release() {
    let repo = cycle();
    let rc10 = repo.git(&["rev-parse", "acme-v1.2.0-rc.10"]);
    let sha = release(&repo, "acme-v1.2.0", &[]);
    assert_eq!(sha, rc10, "the bridge tags the candidate's own commit");
    let r = candidate(&repo, &[]);
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("rc-tag").as_deref(), Some("acme-v1.2.0-rc.10"));
    assert_eq!(r.output("rc-sha").as_deref(), Some(rc10.as_str()));
    assert_eq!(r.output("version").as_deref(), Some("1.2.0"));
    assert_eq!(r.output("release-sha").as_deref(), Some(sha.as_str()));
    assert_eq!(r.output("deploy-sha").as_deref(), Some(sha.as_str()));
    assert!(
        r.log.contains("acme-v1.2.0 is acme-v1.2.0-rc.10."),
        "{}",
        r.log
    );
}

#[test]
fn a_release_with_no_candidate_of_its_version_is_refused() {
    let repo = Repo::new();
    repo.git(&["tag", "acme-v1.1.0-rc.3"]);
    release(&repo, "acme-v1.2.0", &[("src/x.rs", "x\n")]);
    let r = candidate(&repo, &[]);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(
        r.log.contains("no release-candidate tag of 1.2.0"),
        "{}",
        r.log
    );
    // A tag that is not the product's release cannot name its version.
    let r = candidate(&repo, &[("RELEASE_PATTERN", "other-v{version}")]);
    assert_eq!(r.code, 1, "{}", r.log);
}

#[test]
fn nothing_but_the_candidates_own_reports_may_follow_it() {
    // The version and the notes are the candidate's: a CHANGELOG or a
    // version file changed after it is a change no gate saw.
    for files in [
        vec![("CHANGELOG.md", "## 1.2.0\n")],
        vec![("Cargo.toml", "[package]\n")],
        vec![("src/extra.rs", "x\n")],
    ] {
        let repo = cycle();
        release(&repo, "acme-v1.2.0", &files);
        let r = candidate(&repo, &[]);
        assert_eq!(r.code, 1, "{files:?}: {}", r.log);
        assert!(r.log.contains(&format!("  {}", files[0].0)), "{}", r.log);
        assert!(r.log.contains("Cut a new RC"), "{}", r.log);
        assert_eq!(r.output("rc-tag"), None);
    }
}

const ANGLES: &str = r#"["security", "product"]"#;

#[test]
fn the_qa_inputs_reach_the_script_through_its_environment() {
    let step = step(&doc(), "verify", "candidate");
    assert_eq!(
        step["env"]["QA_RECORD"].as_str(),
        Some("${{ inputs.qa-record }}")
    );
    assert_eq!(
        step["env"]["QA_ANGLES"].as_str(),
        Some("${{ inputs.qa-angles }}")
    );
    assert!(!run_body(&step).contains("inputs.qa-"));
    let inputs = &doc()["on"]["workflow_call"]["inputs"];
    // As spec 08's `release.record`; no angle, no exemption.
    assert_eq!(
        inputs["qa-record"]["default"].as_str(),
        Some(".spl-asdlc/qa/{candidate}/")
    );
    assert_eq!(inputs["qa-angles"]["default"].as_str(), Some("[]"));
}

#[test]
fn the_latest_candidates_own_qa_reports_may_follow_it() {
    let repo = cycle();
    release(
        &repo,
        "acme-v1.2.0",
        &[
            (".spl-asdlc/qa/acme-v1.2.0-rc.10/security.json", "{}\n"),
            (".spl-asdlc/qa/acme-v1.2.0-rc.10/product.json", "{}\n"),
        ],
    );
    let r = candidate(&repo, &[("QA_ANGLES", ANGLES)]);
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("rc-tag").as_deref(), Some("acme-v1.2.0-rc.10"));
    assert!(
        r.log
            .contains("acme-v1.2.0 is acme-v1.2.0-rc.10 with its own QA reports."),
        "{}",
        r.log
    );
    // The record directory as spec 08 renders it, with or without its slash.
    let r = candidate(
        &repo,
        &[
            ("QA_ANGLES", ANGLES),
            ("QA_RECORD", ".spl-asdlc/qa/{candidate}"),
        ],
    );
    assert_eq!(r.code, 0, "{}", r.log);
    // Without the angles, the reports are a delta like any other.
    let r = candidate(&repo, &[]);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(
        r.log
            .contains("  .spl-asdlc/qa/acme-v1.2.0-rc.10/security.json"),
        "{}",
        r.log
    );
    // An angle that is not configured is not exempted.
    let r = candidate(&repo, &[("QA_ANGLES", r#"["security"]"#)]);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(
        r.log
            .contains("  .spl-asdlc/qa/acme-v1.2.0-rc.10/product.json"),
        "{}",
        r.log
    );
    assert!(
        !r.log
            .contains("  .spl-asdlc/qa/acme-v1.2.0-rc.10/security.json"),
        "{}",
        r.log
    );
}

#[test]
fn an_earlier_candidates_qa_report_may_not_follow_the_latest() {
    let repo = cycle();
    release(
        &repo,
        "acme-v1.2.0",
        &[(".spl-asdlc/qa/acme-v1.2.0-rc.9/security.json", "{}\n")],
    );
    let r = candidate(&repo, &[("QA_ANGLES", ANGLES)]);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(
        r.log
            .contains("  .spl-asdlc/qa/acme-v1.2.0-rc.9/security.json"),
        "{}",
        r.log
    );
    assert!(r.log.contains("Cut a new RC"), "{}", r.log);
    assert_eq!(r.output("rc-tag"), None);
}

#[test]
fn a_code_change_beside_the_qa_reports_is_refused() {
    let repo = cycle();
    release(
        &repo,
        "acme-v1.2.0",
        &[
            (".spl-asdlc/qa/acme-v1.2.0-rc.10/security.json", "{}\n"),
            ("src/lib.rs", "pub fn f() -> u8 { 3 }\n"),
        ],
    );
    let r = candidate(&repo, &[("QA_ANGLES", ANGLES)]);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("  src/lib.rs"), "{}", r.log);
    assert!(
        !r.log
            .contains("  .spl-asdlc/qa/acme-v1.2.0-rc.10/security.json"),
        "{}",
        r.log
    );
    assert_eq!(r.output("rc-tag"), None);
}

#[test]
fn malformed_qa_inputs_are_refused() {
    let repo = cycle();
    release(&repo, "acme-v1.2.0", &[]);
    for angles in [
        "not json",
        r#""security""#,
        r#"["Security"]"#,
        r#"["../security"]"#,
        "[\"security\\nsrc/lib.rs\"]",
        "[1]",
    ] {
        let r = candidate(&repo, &[("QA_ANGLES", angles)]);
        assert_eq!(r.code, 1, "{angles}: {}", r.log);
        assert!(r.log.contains("qa-angles"), "{angles}: {}", r.log);
    }
    for record in [
        "",
        ".spl-asdlc/qa/",
        ".spl-asdlc/{candidate}/{candidate}/",
        ".spl-asdlc/qa/{candidate}/\nsrc",
    ] {
        let r = candidate(&repo, &[("QA_ANGLES", ANGLES), ("QA_RECORD", record)]);
        assert_eq!(r.code, 1, "{record:?}: {}", r.log);
        assert!(r.log.contains("qa-record"), "{record:?}: {}", r.log);
    }
}

#[test]
fn main_merged_into_the_release_branch_after_the_rc_is_refused_by_name() {
    let repo = cycle();
    repo.git(&["checkout", "-q", "main"]);
    repo.write("src/other.rs", "pub fn o() {}\n");
    repo.commit("feat: main moved on");
    repo.git(&["update-ref", "refs/remotes/origin/main", "main"]);
    repo.git(&["checkout", "-q", "release/2026-10-01-10-00-utc"]);
    repo.git(&["merge", "-q", "--no-ff", "main", "-m", "Merge main"]);
    release(&repo, "acme-v1.2.0", &[]);
    let r = candidate(&repo, &[]);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(
        r.log.contains("main merged into the release branch"),
        "{}",
        r.log
    );
    assert!(r.log.contains("feat: main moved on"), "{}", r.log);
    assert!(
        r.log.contains("No candidate covered these changes"),
        "{}",
        r.log
    );
}

#[test]
fn a_resume_deploys_the_newest_candidate_on_its_exact_commit() {
    let repo = cycle();
    let rc10 = repo.git(&["rev-parse", "acme-v1.2.0-rc.10"]);
    release(
        &repo,
        "acme-v1.2.0",
        &[(".spl-asdlc/qa/acme-v1.2.0-rc.10/security.json", "{}\n")],
    );
    // HEAD is the release: not the commit behind the RC.
    let r = candidate(&repo, &[("RESUME_FROM", "acme-v1.2.0-rc.10")]);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("is not the commit behind"), "{}", r.log);
    repo.git(&["checkout", "-q", "--detach", "acme-v1.2.0-rc.10"]);
    let r = candidate(
        &repo,
        &[("RESUME_FROM", "acme-v1.2.0-rc.10"), ("QA_ANGLES", ANGLES)],
    );
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("deploy-sha").as_deref(), Some(rc10.as_str()));
    // A candidate a newer one superseded is refused.
    repo.git(&["checkout", "-q", "--detach", "acme-v1.2.0-rc.9"]);
    let r = candidate(&repo, &[("RESUME_FROM", "acme-v1.2.0-rc.9")]);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("resume from acme-v1.2.0-rc.10"), "{}", r.log);
}

fn gate(runs: Option<&str>) -> (Ran, Scratch) {
    let dir = Scratch::new("gate");
    if let Some(runs) = runs {
        put(&dir.0, "runs", runs);
    }
    gh(
        &dir.0,
        r#"api\ repos/o/r/actions/workflows/release-gate.yml/runs*) [ -f "$S/runs" ] || exit 1; cat "$S/runs" ;;"#,
    );
    let r = run_with(
        &body("verify", "gate"),
        &dir.0,
        &dir.0,
        &[
            ("REPO", "o/r"),
            ("GATE_WORKFLOW", "release-gate.yml"),
            ("RC_TAG", "acme-v1.2.0-rc.10"),
            ("RC_SHA", "abc"),
        ],
    );
    (r, dir)
}

fn gate_run(sha: &str, branch: &str, event: &str, conclusion: &str) -> String {
    format!(
        r#"{{"head_sha": "{sha}", "head_branch": "{branch}", "event": "{event}", "conclusion": {conclusion}}}"#
    )
}

#[test]
fn the_newest_push_run_of_the_gate_on_the_candidate_decides() {
    let green = gate_run("abc", "acme-v1.2.0-rc.10", "push", "\"success\"");
    let red = gate_run("abc", "acme-v1.2.0-rc.10", "push", "\"failure\"");
    let dispatch = gate_run(
        "abc",
        "acme-v1.2.0-rc.10",
        "workflow_dispatch",
        "\"success\"",
    );
    let other = gate_run("abc", "release/x", "push", "\"success\"");
    let pending = gate_run("abc", "acme-v1.2.0-rc.10", "push", "null");
    let page = |runs: &[&String]| {
        format!(
            r#"{{"workflow_runs": [{}]}}"#,
            runs.iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(",")
        )
    };
    let (r, dir) = gate(Some(&page(&[&dispatch, &other, &green, &red])));
    assert_eq!(r.code, 0, "{}", r.log);
    let call = &calls(&dir.0, "gh")[0];
    assert!(call.contains("head_sha=abc&event=push"), "{call}");
    // A newer red run hides an older green one.
    let (r, _d) = gate(Some(&page(&[&red, &green])));
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("is 'failure'"), "{}", r.log);
    let (r, _d) = gate(Some(&page(&[&pending, &green])));
    assert_eq!(r.code, 1, "{}", r.log);
    // Runs on another ref or by another event are not the gate's verdict.
    let (r, _d) = gate(Some(&page(&[&dispatch, &other])));
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("is 'none'"), "{}", r.log);
}

#[test]
fn an_unreadable_gate_refuses() {
    let (r, _d) = gate(None);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("could not be read"), "{}", r.log);
    let (r, _d) = gate(Some("not json"));
    assert_eq!(r.code, 1, "{}", r.log);
}

fn go(publisher_input: &str, release: Option<&str>) -> (Ran, Scratch) {
    let dir = Scratch::new("go");
    if let Some(r) = release {
        put(&dir.0, "release", r);
    }
    gh(
        &dir.0,
        r#"'api repos/o/r/releases/tags/acme-v1.2.0') [ -f "$S/release" ] || exit 1; cat "$S/release" ;;"#,
    );
    let r = run_with(
        &body("verify", "go"),
        &dir.0,
        &dir.0,
        &[
            ("REPO", "o/r"),
            ("RELEASE_TAG", "acme-v1.2.0"),
            ("GO_PUBLISHER", publisher_input),
        ],
    );
    (r, dir)
}

/// The release as REST reads it, published by `login`, an account of
/// `kind` (`Bot`, `User`, `Organization`, or empty when GitHub gave none).
fn published_as(login: &str, kind: &str) -> String {
    format!(
        r#"{{"tag_name": "acme-v1.2.0", "draft": false, "author": {{"login": "{login}", "type": "{kind}"}}}}"#
    )
}

const SLUG: &str = "the-bridge";

#[test]
fn the_go_is_the_release_published_by_the_app_in_either_login_form() {
    let bot = format!("{SLUG}[bot]");
    for input in [SLUG.to_string(), format!("app/{SLUG}"), bot.clone()] {
        let (r, dir) = go(&input, Some(&published_as(&bot, "Bot")));
        assert_eq!(r.code, 0, "{input}: {}", r.log);
        assert_eq!(r.output("go-published-by").as_deref(), Some(bot.as_str()));
        assert_eq!(r.output("go-published-by-kind").as_deref(), Some("app"));
        assert_eq!(r.output("verified").as_deref(), Some("true"));
        assert!(read(&dir.0, "summary").contains("the bridge's App"));
    }
}

/// Spec 09 (BR-ASDLC-021): a person tags the candidate and publishes the
/// release in GitHub instead of the bridge; the rulesets decide who may.
#[test]
fn a_release_published_by_a_human_is_the_go_and_says_so() {
    let (r, dir) = go(SLUG, Some(&published_as("owner-login", "User")));
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("go-published-by").as_deref(), Some("owner-login"));
    assert_eq!(r.output("go-published-by-kind").as_deref(), Some("human"));
    assert!(r.log.contains("(a human)"), "{}", r.log);
    assert!(read(&dir.0, "summary").contains("`owner-login` (a human)"));
}

#[test]
fn a_release_published_by_another_bot_or_nobody_readable_is_no_go() {
    let bot = |slug: &str| format!("{slug}[bot]");
    for (login, kind) in [
        (bot("other-app"), "Bot"),
        (bot("github-actions"), "Bot"),
        ("some-org".to_string(), "Organization"),
        ("owner-login".to_string(), ""),
        (String::new(), ""),
    ] {
        let (r, _d) = go(SLUG, Some(&published_as(&login, kind)));
        assert_eq!(r.code, 1, "{login} {kind}: {}", r.log);
        assert!(r.log.contains("no GO"), "{}", r.log);
        assert_eq!(r.output("verified"), None);
    }
    // A draft is no publication, an unreadable release no GO, and a slug
    // that is not one refuses before anything is read.
    let draft = published_as(&bot(SLUG), "Bot").replace(r#""draft": false"#, r#""draft": true"#);
    let (r, _d) = go(SLUG, Some(&draft));
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("is a draft"), "{}", r.log);
    let (r, _d) = go(SLUG, None);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("could not be read"), "{}", r.log);
    let (r, d) = go("Not A Slug", Some(&published_as("x", "User")));
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(calls(&d.0, "gh").is_empty());
}

// ---------------------------------------------------------------- close: refs

const BRANCH: &str = "release/2026-10-01-10-00-utc";

/// A repository with an `origin` holding main, the release branch and tags.
struct Remote {
    repo: Repo,
    origin: Scratch,
}

fn remote_cycle() -> Remote {
    let repo = cycle();
    release(&repo, "acme-v1.2.0", &[]);
    let origin = Scratch::new("origin");
    git_at(&origin.0, &["init", "-q", "--bare"]);
    repo.git(&["remote", "add", "origin", origin.0.to_str().unwrap()]);
    repo.git(&["push", "-q", "origin", "main", BRANCH, "--tags"]);
    repo.git(&["checkout", "-q", "--detach", "acme-v1.2.0"]);
    Remote { repo, origin }
}

/// A `gh` the refs step never calls: it reads the refs alone.
fn refs_gh(dir: &Path) {
    gh(dir, "");
}

fn refs(dir: &Path, cwd: &Path, env: &[(&str, &str)]) -> Ran {
    let mut all: Vec<(&str, &str)> = vec![
        ("RELEASE_TAG", "acme-v1.2.0"),
        ("RESUME_FROM", ""),
        ("REPLAY", "false"),
        ("REF_NAME", "acme-v1.2.0"),
        ("REF_TYPE", "tag"),
        ("BRANCH_PREFIX", "release/"),
        ("DEFAULT_BRANCH", "main"),
    ];
    for (k, v) in env {
        all.retain(|(x, _)| x != k);
        all.push((k, v));
    }
    run_with(&body("close", "refs"), dir, cwd, &all)
}

#[test]
fn the_release_branch_is_read_off_the_refs() {
    let rem = remote_cycle();
    let dir = Scratch::new("refs");
    refs_gh(&dir.0);
    let r = refs(&dir.0, &rem.repo.dir.0, &[]);
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("branch").as_deref(), Some(BRANCH));
    assert!(r.output("held").is_none(), "nothing holds a single release");
    assert!(calls(&dir.0, "gh").is_empty());
}

#[test]
fn no_release_branch_is_fine_only_when_the_tag_is_already_on_main() {
    let rem = remote_cycle();
    let dir = Scratch::new("refs");
    refs_gh(&dir.0);
    // Gone on the remote; the stale tracking ref is pruned by the fetch.
    git_at(rem.origin.0.as_path(), &["branch", "-q", "-D", BRANCH]);
    let r = refs(&dir.0, &rem.repo.dir.0, &[]);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("not an ancestor of main"), "{}", r.log);
    // Once main holds the tag (an earlier run merged and deleted), it is done.
    rem.repo
        .git(&["push", "-q", "origin", "acme-v1.2.0:refs/heads/main"]);
    let r = refs(&dir.0, &rem.repo.dir.0, &[]);
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("branch").as_deref(), Some(""));
    assert!(calls(&dir.0, "gh").is_empty());
}

#[test]
fn a_replay_names_its_branch_and_off_any_release_branch_asks_nothing() {
    let rem = remote_cycle();
    let dir = Scratch::new("refs");
    refs_gh(&dir.0);
    let r = refs(
        &dir.0,
        &rem.repo.dir.0,
        &[
            ("RELEASE_TAG", ""),
            ("REPLAY", "true"),
            ("REF_NAME", BRANCH),
            ("REF_TYPE", "branch"),
        ],
    );
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("branch").as_deref(), Some(BRANCH));

    let dir = Scratch::new("refs");
    refs_gh(&dir.0);
    let r = refs(
        &dir.0,
        &rem.repo.dir.0,
        &[
            ("RELEASE_TAG", ""),
            ("REPLAY", "true"),
            ("REF_NAME", "main"),
            ("REF_TYPE", "branch"),
        ],
    );
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("branch").as_deref(), Some(""));
    assert!(calls(&dir.0, "gh").is_empty());
    // Without a replay, no tag is a misconfiguration, not a no-op.
    let r = refs(
        &dir.0,
        &rem.repo.dir.0,
        &[
            ("RELEASE_TAG", ""),
            ("REF_NAME", "main"),
            ("REF_TYPE", "branch"),
        ],
    );
    assert_eq!(r.code, 1, "{}", r.log);
}

#[test]
fn a_resume_merges_nothing_back_but_says_which_branch_still_holds_its_rc() {
    let rem = remote_cycle();
    let dir = Scratch::new("refs");
    refs_gh(&dir.0);
    let r = refs(
        &dir.0,
        &rem.repo.dir.0,
        &[("RESUME_FROM", "acme-v1.2.0-rc.10")],
    );
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("branch").as_deref(), Some(""));
    assert!(
        r.log.contains(&format!(
            "::warning::{BRANCH} still exists and holds acme-v1.2.0-rc.10"
        )),
        "{}",
        r.log
    );
    assert!(calls(&dir.0, "gh").is_empty());
}

// ---------------------------------------------------------------- close: merge back

/// The merge-back `gh`: `ahead` (or `fail`), `open`/`merged-pr` PR numbers
/// (absent = the list fails), `head`, `states` popped one per poll (the last
/// one sticks), `checks` popped per read (`fail` = the read fails), `merge-ok`
/// (the REST merge succeeds), `delete-ok`, `ref` (the branch exists).
fn mergeback_gh(dir: &Path) {
    gh(
        dir,
        r#"'api repos/o/r/compare/main...release/2026-10-01-10-00-utc --jq .ahead_by')
  a="$(cat "$S/ahead")"; [ "$a" = fail ] && exit 1; echo "$a" ;;
'pr list --state open --base main --head release/2026-10-01-10-00-utc --json number --jq .[0].number // ""')
  [ -f "$S/open" ] || exit 1; cat "$S/open" ;;
'pr list --state merged --base main --head release/2026-10-01-10-00-utc --json number --jq .[0].number // ""')
  [ -f "$S/merged-pr" ] || exit 1; cat "$S/merged-pr" ;;
'pr create --base main --head release/2026-10-01-10-00-utc '*) echo 7 > "$S/open"; echo "https://example.invalid/pull/7" ;;
'pr view 7 --json headRefName --jq .headRefName // ""') [ -f "$S/head" ] || exit 1; cat "$S/head" ;;
'pr merge 7 --merge --auto') [ -f "$S/auto-refused" ] && exit 1; exit 0 ;;
'pr view 7 --json state,mergeStateStatus,headRefOid '*)
  if [ -f "$S/merged" ]; then echo 'MERGED|MERGED|h1'; exit 0; fi
  head -n 1 "$S/states"
  if [ "$(wc -l < "$S/states")" -gt 1 ]; then tail -n +2 "$S/states" > "$S/s.tmp"; mv "$S/s.tmp" "$S/states"; fi ;;
'api repos/o/r/commits/'*'/check-runs?check_name=required-check --jq '*)
  c="$(head -n 1 "$S/checks")"
  if [ "$(wc -l < "$S/checks")" -gt 1 ]; then tail -n +2 "$S/checks" > "$S/c.tmp"; mv "$S/c.tmp" "$S/checks"; fi
  [ "$c" = fail ] && exit 1; echo "$c" ;;
'api -X PUT repos/o/r/pulls/7/merge -f merge_method=merge -f sha='*)
  [ -f "$S/merge-ok" ] || exit 1; touch "$S/merged" ;;
'api -X DELETE repos/o/r/git/refs/heads/release/2026-10-01-10-00-utc')
  [ -f "$S/delete-ok" ] || exit 1; rm -f "$S/ref" ;;
'api repos/o/r/git/refs/heads/release/2026-10-01-10-00-utc') [ -f "$S/ref" ] ;;"#,
    );
}

/// A fresh merge back: ahead, no PR yet, head readable, the branch exists.
fn fresh(states: &str, checks: &str) -> Scratch {
    let dir = Scratch::new("mergeback");
    mergeback_gh(&dir.0);
    put(&dir.0, "ahead", "3\n");
    put(&dir.0, "open", "");
    put(&dir.0, "merged-pr", "");
    put(&dir.0, "head", &format!("{BRANCH}\n"));
    put(&dir.0, "states", states);
    put(&dir.0, "checks", checks);
    put(&dir.0, "delete-ok", "");
    put(&dir.0, "ref", "");
    dir
}

fn mergeback(dir: &Path) -> Ran {
    run_with(
        &body("close", "mergeback"),
        dir,
        dir,
        &[
            ("REPO", "o/r"),
            ("BRANCH", BRANCH),
            ("RELEASE_TAG", "acme-v1.2.0"),
            ("DEFAULT_BRANCH", "main"),
            ("REQUIRED_CONTEXT", "required-check"),
            ("POLL_INTERVAL", "15"),
            // Four polls.
            ("POLL_TIMEOUT", "1"),
            ("RUN_URL", "https://example.invalid/run"),
        ],
    )
}

fn gh_calls(dir: &Path) -> Vec<String> {
    calls(dir, "gh")
}

fn position(calls: &[String], needle: &str) -> Option<usize> {
    calls.iter().position(|c| c.contains(needle))
}

const PUT: &str = "api -X PUT repos/o/r/pulls/7/merge -f merge_method=merge -f sha=";
const DELETE: &str = "api -X DELETE repos/o/r/git/refs/heads/";

#[test]
fn the_happy_path_merges_a_merge_commit_pinned_to_the_head_then_deletes_the_branch() {
    let dir = fresh("OPEN|CLEAN|h1\n", "success\n");
    put(&dir.0, "merge-ok", "");
    let r = mergeback(&dir.0);
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("merged").as_deref(), Some("true"));
    assert_eq!(r.output("number").as_deref(), Some("7"));
    let c = gh_calls(&dir.0);
    let create = position(&c, "pr create").unwrap();
    assert!(
        c[create]
            .contains("--title chore(release): merge release/2026-10-01-10-00-utc back into main"),
        "{}",
        c[create]
    );
    assert!(position(&c, "pr merge 7 --merge --auto").unwrap() > create);
    let put_at = position(&c, &format!("{PUT}h1")).expect("a REST merge pinned to h1");
    let delete_at = position(&c, DELETE).unwrap();
    assert!(delete_at > put_at, "{c:?}");
    for call in &c {
        assert!(
            !call.contains("--squash") && !call.contains("--rebase") && !call.contains("--admin")
        );
        assert!(!call.contains("update-branch"), "{call}");
    }
}

#[test]
fn the_branch_is_never_deleted_before_merged_and_a_slow_close_says_so() {
    let dir = fresh("OPEN|UNKNOWN|h1\n", "success\n");
    let r = mergeback(&dir.0);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(position(&gh_calls(&dir.0), DELETE).is_none());
    assert_eq!(r.output("last-state").as_deref(), Some("UNKNOWN"));
    assert_eq!(r.output("merged").as_deref(), Some("false"));
    assert!(r.log.contains("only slow"), "{}", r.log);
    // A merge the poll sees only later: deleted then, not before.
    let dir = fresh(
        "OPEN|BLOCKED|h1\nOPEN|BLOCKED|h1\nMERGED|MERGED|h1\n",
        "pending\n",
    );
    let r = mergeback(&dir.0);
    assert_eq!(r.code, 0, "{}", r.log);
    let c = gh_calls(&dir.0);
    assert!(position(&c, PUT).is_none(), "{c:?}");
    assert!(position(&c, DELETE).is_some());
}

#[test]
fn blocked_or_behind_merges_over_rest_only_once_the_required_check_is_green() {
    for state in ["BLOCKED", "BEHIND"] {
        let dir = fresh(&format!("OPEN|{state}|h1\n"), "pending\nfail\nsuccess\n");
        put(&dir.0, "merge-ok", "");
        let r = mergeback(&dir.0);
        assert_eq!(r.code, 0, "{state}: {}", r.log);
        let c = gh_calls(&dir.0);
        let checks: Vec<&String> = c
            .iter()
            .filter(|x| x.contains("check-runs?check_name=required-check"))
            .collect();
        // Read from the REST check runs of the head, never statusCheckRollup.
        assert_eq!(checks.len(), 3, "{c:?}");
        assert!(checks[0].contains("repos/o/r/commits/h1/check-runs"));
        assert!(!c.iter().any(|x| x.contains("statusCheckRollup")));
        // One merge, after the third read, pinned to the head that was read.
        let puts: Vec<&String> = c.iter().filter(|x| x.contains("-X PUT")).collect();
        assert_eq!(puts.len(), 1, "{c:?}");
        assert_eq!(puts[0].as_str(), format!("{PUT}h1"));
        assert!(
            r.log.contains("'pending'") && r.log.contains("'pending or unreadable'"),
            "{}",
            r.log
        );
        assert!(!c.iter().any(|x| x.contains("update-branch")));
    }
}

#[test]
fn a_refused_rest_merge_warns_keeps_polling_and_ends_red_naming_the_bypass_list() {
    let dir = fresh("OPEN|BLOCKED|h1\n", "success\n");
    let r = mergeback(&dir.0);
    assert_eq!(r.code, 1, "{}", r.log);
    let c = gh_calls(&dir.0);
    assert_eq!(c.iter().filter(|x| x.contains("-X PUT")).count(), 4);
    assert!(
        r.log
            .contains("::warning::#7 is BLOCKED and GitHub refused the merge at h1"),
        "{}",
        r.log
    );
    assert!(r.log.contains("bypass list"), "{}", r.log);
    assert!(r.log.contains("NOT a conflict"), "{}", r.log);
    assert!(position(&c, DELETE).is_none());
}

#[test]
fn an_unreadable_head_sha_merges_nothing() {
    let dir = fresh("OPEN|BLOCKED|\n", "success\n");
    put(&dir.0, "merge-ok", "");
    let r = mergeback(&dir.0);
    assert_eq!(r.code, 1, "{}", r.log);
    let c = gh_calls(&dir.0);
    assert!(
        !c.iter()
            .any(|x| x.contains("check-runs") || x.contains("-X PUT")),
        "{c:?}"
    );
    let dir = fresh("OPEN|CLEAN|\n", "success\n");
    put(&dir.0, "merge-ok", "");
    let r = mergeback(&dir.0);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(!gh_calls(&dir.0).iter().any(|x| x.contains("-X PUT")));
}

#[test]
fn dirty_is_never_merged_never_deleted_and_is_resolved_off_the_release_branch() {
    let dir = fresh("OPEN|DIRTY|h1\n", "success\n");
    put(&dir.0, "merge-ok", "");
    let r = mergeback(&dir.0);
    assert_eq!(r.code, 1, "{}", r.log);
    let c = gh_calls(&dir.0);
    assert!(
        !c.iter()
            .any(|x| x.contains("-X PUT") || x.contains("check-runs"))
    );
    assert!(position(&c, DELETE).is_none());
    assert_eq!(r.output("last-state").as_deref(), Some("DIRTY"));
    let summary = read(&dir.0, "summary");
    assert!(summary.contains("chore/merge-back-<stamp>"), "{summary}");
    assert!(summary.contains(&format!("cut chore/merge-back-<stamp> from {BRANCH}")));
    // DIRTY on the first poll does not end the loop: GitHub reports it
    // transiently while it recomputes.
    assert_eq!(
        c.iter()
            .filter(|x| x.starts_with("pr view 7 --json state"))
            .count(),
        4
    );
}

#[test]
fn no_recovery_line_brings_the_default_branch_into_the_release_branch() {
    for state in ["DIRTY", "BLOCKED", "BEHIND", "CLEAN"] {
        let dir = fresh(&format!("OPEN|{state}|h1\n"), "pending\n");
        let r = mergeback(&dir.0);
        assert_eq!(r.code, 1, "{}", r.log);
        let summary = read(&dir.0, "summary");
        assert!(summary.contains("FORBIDDEN"), "{state}: {summary}");
        for line in summary.lines().filter(|l| !l.contains("FORBIDDEN")) {
            assert!(
                !line.contains(&format!("into {BRANCH}")) && !line.contains("into release/"),
                "{state}: {line}"
            );
        }
    }
}

#[test]
fn the_head_is_read_back_and_anything_but_the_branch_is_refused() {
    let dir = fresh("OPEN|CLEAN|h1\n", "success\n");
    put(&dir.0, "merge-ok", "");
    put(&dir.0, "head", "release/another\n");
    let r = mergeback(&dir.0);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("not release/2026"), "{}", r.log);
    assert!(
        !gh_calls(&dir.0)
            .iter()
            .any(|x| x.contains("pr merge") || x.contains("-X PUT"))
    );
    std::fs::remove_file(dir.0.join("head")).unwrap();
    let r = mergeback(&dir.0);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("<unreadable>"), "{}", r.log);
}

#[test]
fn a_refused_auto_merge_falls_back_to_the_poll() {
    let dir = fresh("OPEN|UNSTABLE|h1\n", "failure\n");
    put(&dir.0, "merge-ok", "");
    put(&dir.0, "auto-refused", "");
    let r = mergeback(&dir.0);
    assert_eq!(r.code, 0, "{}", r.log);
    assert!(
        r.log.contains("::warning::auto-merge was refused"),
        "{}",
        r.log
    );
    // UNSTABLE: GitHub says the required checks passed; no check read.
    let c = gh_calls(&dir.0);
    assert!(!c.iter().any(|x| x.contains("check-runs")));
    assert!(position(&c, &format!("{PUT}h1")).is_some());
}

#[test]
fn a_branch_main_already_holds_is_only_deleted_and_nothing_is_opened_or_merged() {
    let dir = fresh("OPEN|CLEAN|h1\n", "success\n");
    put(&dir.0, "ahead", "0\n");
    let r = mergeback(&dir.0);
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("merged").as_deref(), Some("true"));
    let c = gh_calls(&dir.0);
    assert!(position(&c, DELETE).is_some());
    assert!(!c.iter().any(|x| x.starts_with("pr ")), "{c:?}");
    // A delete that fails while the branch is still there is an error.
    let dir = fresh("OPEN|CLEAN|h1\n", "success\n");
    put(&dir.0, "ahead", "0\n");
    std::fs::remove_file(dir.0.join("delete-ok")).unwrap();
    let r = mergeback(&dir.0);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("Delete it by hand"), "{}", r.log);
    // Already gone: the same end state.
    std::fs::remove_file(dir.0.join("ref")).unwrap();
    let r = mergeback(&dir.0);
    assert_eq!(r.code, 0, "{}", r.log);
}

/// Spec 09 (BR-ASDLC-021): the merge back is the close stage's automation,
/// never the only way. A human who merges it in GitHub, while the poll
/// waits or before the close runs at all, leaves a state the close takes as
/// done: nothing is merged twice, no second PR is opened, and the closing
/// guard reads the run green.
#[test]
fn a_merge_back_a_human_did_in_github_is_taken_as_done() {
    // The merge-back PR merged by a human while the poll waited on its
    // required check: the close merges nothing itself and deletes the branch.
    let dir = fresh("OPEN|BLOCKED|h1\nMERGED|MERGED|h1\n", "pending\n");
    let r = mergeback(&dir.0);
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("merged").as_deref(), Some("true"));
    let c = gh_calls(&dir.0);
    assert!(position(&c, PUT).is_none(), "{c:?}");
    assert!(position(&c, DELETE).is_some(), "{c:?}");

    // The release branch merged back by hand (a merge commit) and deleted
    // before the close ran: nothing is left to merge back, and GitHub is not
    // asked for a PR.
    let rem = remote_cycle();
    rem.repo.git(&["checkout", "-q", "main"]);
    rem.repo.git(&[
        "merge",
        "-q",
        "--no-ff",
        "-m",
        "Merge the release back",
        BRANCH,
    ]);
    rem.repo.git(&["push", "-q", "origin", "main"]);
    git_at(rem.origin.0.as_path(), &["branch", "-q", "-D", BRANCH]);
    rem.repo.git(&["checkout", "-q", "--detach", "acme-v1.2.0"]);
    let dir = Scratch::new("refs");
    refs_gh(&dir.0);
    let r = refs(&dir.0, &rem.repo.dir.0, &[]);
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("branch").as_deref(), Some(""));
    assert!(
        r.log.contains("already on main: nothing to merge back"),
        "{}",
        r.log
    );
    assert!(calls(&dir.0, "gh").is_empty());
    // The close succeeded with nothing to do: the guard is green.
    assert_eq!(guard("close", "success", "false", "success").code, 0);
}

#[test]
fn a_compare_that_cannot_be_read_changes_nothing() {
    let dir = fresh("OPEN|CLEAN|h1\n", "success\n");
    put(&dir.0, "ahead", "fail\n");
    put(&dir.0, "merge-ok", "");
    let r = mergeback(&dir.0);
    assert_eq!(r.code, 0, "{}", r.log);
    let c = gh_calls(&dir.0);
    assert!(position(&c, "pr create").is_some());
    assert!(position(&c, PUT).unwrap() < position(&c, DELETE).unwrap());
}

#[test]
fn the_replay_reuses_the_open_pr_or_only_deletes_after_a_merged_one() {
    // An open PR is reused, never a second one.
    let dir = fresh("OPEN|CLEAN|h1\n", "success\n");
    put(&dir.0, "open", "7\n");
    put(&dir.0, "merge-ok", "");
    let r = mergeback(&dir.0);
    assert_eq!(r.code, 0, "{}", r.log);
    assert!(position(&gh_calls(&dir.0), "pr create").is_none());
    // Merged already and nothing came after: only the deletion.
    let dir = fresh("OPEN|CLEAN|h1\n", "success\n");
    put(&dir.0, "merged-pr", "7\n");
    put(&dir.0, "ahead", "0\n");
    let r = mergeback(&dir.0);
    assert_eq!(r.code, 0, "{}", r.log);
    let c = gh_calls(&dir.0);
    assert!(position(&c, "pr create").is_none() && position(&c, DELETE).is_some());
    // Merged once, but commits came after: never deleted, a new PR instead.
    let dir = fresh("OPEN|CLEAN|h1\n", "success\n");
    put(&dir.0, "merged-pr", "7\n");
    put(&dir.0, "merge-ok", "");
    let r = mergeback(&dir.0);
    assert_eq!(r.code, 0, "{}", r.log);
    let c = gh_calls(&dir.0);
    assert!(position(&c, "pr create").is_some(), "{c:?}");
    assert!(position(&c, "pr create") < position(&c, DELETE), "{c:?}");
    // Merged once, and the compare is unreadable: nothing is deleted.
    let dir = fresh("OPEN|CLEAN|h1\n", "success\n");
    put(&dir.0, "merged-pr", "7\n");
    put(&dir.0, "ahead", "fail\n");
    let r = mergeback(&dir.0);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(position(&gh_calls(&dir.0), DELETE).is_none());
    // Merged and the branch already gone: a no-op.
    let dir = fresh("OPEN|CLEAN|h1\n", "success\n");
    put(&dir.0, "merged-pr", "7\n");
    put(&dir.0, "ahead", "0\n");
    std::fs::remove_file(dir.0.join("delete-ok")).unwrap();
    std::fs::remove_file(dir.0.join("ref")).unwrap();
    assert_eq!(mergeback(&dir.0).code, 0);
    // Merged, and the surviving branch cannot be deleted: red, by name.
    let dir = fresh("OPEN|CLEAN|h1\n", "success\n");
    put(&dir.0, "merged-pr", "7\n");
    put(&dir.0, "ahead", "0\n");
    std::fs::remove_file(dir.0.join("delete-ok")).unwrap();
    let r = mergeback(&dir.0);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("Delete it by hand"), "{}", r.log);
}

#[test]
fn a_failed_pr_lookup_never_becomes_a_second_pr() {
    let dir = fresh("OPEN|CLEAN|h1\n", "success\n");
    std::fs::remove_file(dir.0.join("merged-pr")).unwrap();
    let r = mergeback(&dir.0);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("could not read"), "{}", r.log);
    assert!(position(&gh_calls(&dir.0), "pr create").is_none());
    let dir = fresh("OPEN|CLEAN|h1\n", "success\n");
    std::fs::remove_file(dir.0.join("open")).unwrap();
    let r = mergeback(&dir.0);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(position(&gh_calls(&dir.0), "pr create").is_none());
}

#[test]
fn a_delete_that_fails_after_the_merge_is_a_warning_the_merge_is_the_fact() {
    let dir = fresh("OPEN|CLEAN|h1\n", "success\n");
    put(&dir.0, "merge-ok", "");
    std::fs::remove_file(dir.0.join("delete-ok")).unwrap();
    let r = mergeback(&dir.0);
    assert_eq!(r.code, 0, "{}", r.log);
    assert!(
        r.log.contains(
            "::warning::release/2026-10-01-10-00-utc was merged but could not be deleted"
        )
    );
    assert_eq!(r.output("merged").as_deref(), Some("true"));
}

#[test]
fn the_poll_budget_is_bounded_and_outlasted_by_the_job() {
    let dir = fresh("OPEN|CLEAN|h1\n", "success\n");
    let r = run_with(
        &body("close", "mergeback"),
        &dir.0,
        &dir.0,
        &[
            ("REPO", "o/r"),
            ("BRANCH", BRANCH),
            ("DEFAULT_BRANCH", "main"),
            ("POLL_INTERVAL", "15"),
            ("POLL_TIMEOUT", "50"),
        ],
    );
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(gh_calls(&dir.0).is_empty());
    let d = doc();
    let inputs = &d["on"]["workflow_call"]["inputs"];
    assert_eq!(inputs["poll-timeout-minutes"]["default"].as_i64(), Some(30));
    assert_eq!(
        inputs["poll-interval-seconds"]["default"].as_i64(),
        Some(15)
    );
}

// ---------------------------------------------------------------- closing-guard

fn guard(stage: &str, publish: &str, replay: &str, close: &str) -> Ran {
    let dir = Scratch::new("guard");
    run(
        &body(
            "closing-guard",
            "Refuse a publication whose close did not run",
        ),
        &[
            ("STAGE", stage),
            ("PUBLISH_RESULT", publish),
            ("REPLAY", replay),
            ("CLOSE_RESULT", close),
            ("RELEASE_TAG", "acme-v1.2.0"),
        ],
        &dir.0,
    )
}

#[test]
fn the_closing_guard_is_red_when_the_close_was_skipped_or_failed() {
    assert_eq!(guard("close", "success", "false", "success").code, 0);
    let r = guard("close", "success", "false", "skipped");
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("SKIPPED"), "{}", r.log);
    for close in ["failure", "cancelled", ""] {
        assert_eq!(guard("close", "success", "false", close).code, 1, "{close}");
    }
    // A publication that did not succeed never reads green.
    for publish in ["failure", "skipped", "cancelled", ""] {
        assert_eq!(
            guard("close", publish, "false", "skipped").code,
            1,
            "{publish}"
        );
    }
    // A replay publishes nothing; its close must still succeed.
    assert_eq!(guard("close", "skipped", "true", "success").code, 0);
    assert_eq!(guard("close", "skipped", "true", "failure").code, 1);
    // The other stages have nothing to guard; an unknown stage is red.
    assert_eq!(guard("verify", "", "false", "skipped").code, 0);
    assert_eq!(guard("record-deployed", "", "false", "skipped").code, 1);
    assert_eq!(guard("Close", "success", "false", "skipped").code, 1);
}

#[test]
fn the_setup_docs_resolve_a_conflict_off_the_release_branch() {
    let text =
        std::fs::read_to_string(crate::harness::root().join("docs/setup/repository.md")).unwrap();
    assert!(text.contains("chore/merge-back-"), "{text}");
    assert!(text.contains("never by bringing `main` into"), "{text}");
}
