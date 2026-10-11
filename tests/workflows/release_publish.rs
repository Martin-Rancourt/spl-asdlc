//! `asdlc-release-publish` (REQ-CI-076..097): the three stages run as
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
    assert!(if_of("record-deployed").contains("inputs.stage == 'record-deployed'"));
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
    for j in ["verify", "record-deployed"] {
        assert!(
            jobs[j]["runs-on"]
                .as_str()
                .unwrap()
                .contains("fromJSON(inputs.runs-on)"),
            "{j}"
        );
    }
    // The close job outlasts its poll.
    assert!(jobs["close"]["timeout-minutes"].as_i64().unwrap() >= 40);
    let names: Vec<&str> = jobs
        .as_hash()
        .unwrap()
        .values()
        .map(|j| j["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec!["verify", "record-deployed", "close", "closing-guard"]
    );
}

#[test]
fn the_app_token_is_minted_right_before_the_writes_and_only_there() {
    let d = doc();
    let secrets = &d["on"]["workflow_call"]["secrets"];
    assert!(!secrets["SPL_ASDLC_APP_ID"].is_badvalue());
    assert!(!secrets["SPL_ASDLC_APP_PRIVATE_KEY"].is_badvalue());
    for (job, consumer) in [("record-deployed", "record"), ("close", "refs")] {
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

/// main, an old cycle's candidate, then a release branch with rc.9, a fix,
/// rc.10 and release-please's bookkeeping under the release tag.
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

fn release(repo: &Repo, tag: &str, files: &[(&str, &str)]) -> String {
    for (p, t) in files {
        repo.write(p, t);
    }
    let sha = repo.commit("chore(release): release acme 1.2.0");
    repo.git(&["tag", tag]);
    sha
}

const BOOKKEEPING: &str = r#"[".release-please-manifest.json", "CHANGELOG.md"]"#;

fn candidate(repo: &Repo, env: &[(&str, &str)]) -> Ran {
    let mut all: Vec<(&str, &str)> = vec![
        ("RELEASE_TAG", "acme-v1.2.0"),
        ("RESUME_FROM", ""),
        ("CANDIDATE_PATTERN", "acme-v{version}-rc.{rc}"),
        ("RELEASE_PATTERN", "acme-v{version}"),
        ("COMPONENTS", "[]"),
        ("BOOKKEEPING", BOOKKEEPING),
        ("GATE_RECORD", ""),
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
    let sha = release(
        &repo,
        "acme-v1.2.0",
        &[
            ("CHANGELOG.md", "## 1.2.0\n"),
            (".release-please-manifest.json", "{\".\": \"1.2.0\"}\n"),
        ],
    );
    let r = candidate(&repo, &[]);
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("rc-tag").as_deref(), Some("acme-v1.2.0-rc.10"));
    assert_eq!(r.output("rc-sha").as_deref(), Some(rc10.as_str()));
    assert_eq!(r.output("version").as_deref(), Some("1.2.0"));
    assert_eq!(r.output("release-sha").as_deref(), Some(sha.as_str()));
    assert_eq!(r.output("deploy-sha").as_deref(), Some(sha.as_str()));
}

#[test]
fn a_release_with_no_candidate_of_its_version_is_refused() {
    let repo = Repo::new();
    repo.git(&["tag", "acme-v1.1.0-rc.3"]);
    release(&repo, "acme-v1.2.0", &[("CHANGELOG.md", "x\n")]);
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
fn only_bookkeeping_may_differ_from_the_candidate() {
    let repo = cycle();
    release(
        &repo,
        "acme-v1.2.0",
        &[("CHANGELOG.md", "## 1.2.0\n"), ("src/extra.rs", "x\n")],
    );
    let r = candidate(&repo, &[]);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("src/extra.rs"), "{}", r.log);
    assert!(!r.log.contains("  CHANGELOG.md"), "{}", r.log);
    assert!(r.log.contains("Cut a new RC"), "{}", r.log);
    assert_eq!(r.output("rc-tag"), None);
}

#[test]
fn an_empty_bookkeeping_list_demands_the_candidate_exactly() {
    let repo = cycle();
    release(&repo, "acme-v1.2.0", &[("CHANGELOG.md", "## 1.2.0\n")]);
    let r = candidate(&repo, &[("BOOKKEEPING", "[]")]);
    assert_eq!(r.code, 1, "{}", r.log);
    let r = candidate(&repo, &[("BOOKKEEPING", "not json")]);
    assert_eq!(r.code, 1, "{}", r.log);
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
    release(&repo, "acme-v1.2.0", &[("CHANGELOG.md", "## 1.2.0\n")]);
    let r = candidate(&repo, &[]);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(
        r.log.contains("main merged into the release branch"),
        "{}",
        r.log
    );
    assert!(r.log.contains("feat: main moved on"), "{}", r.log);
    assert!(r.log.contains("does not read a merge commit"), "{}", r.log);
}

/// A multi-component cycle: global candidates `v{version}-rc.{rc}`, the gate
/// record the bridge lands, then a component's release.
fn components_cycle(record: &str) -> Repo {
    let repo = Repo::new();
    repo.git(&["tag", "v1.1.0-rc.1"]);
    repo.git(&["checkout", "-q", "-b", "release/2026-10-01-10-00-utc"]);
    repo.write("apps/app/src/a.ts", "a\n");
    repo.commit("fix: app");
    repo.git(&["tag", "v1.2.0-rc.2"]);
    repo.write(record, "gate record\n");
    repo.commit("docs(release): gate record 1.2.0");
    repo.write("apps/website/CHANGELOG.md", "## 0.3.1\n");
    repo.write(".release-please-manifest.json", "{}\n");
    repo.commit("chore(release): release website 0.3.1");
    repo.git(&["tag", "website-v0.3.1"]);
    repo
}

fn component_candidate(repo: &Repo, release: &str, components: &str) -> Ran {
    candidate(
        repo,
        &[
            ("RELEASE_TAG", release),
            ("CANDIDATE_PATTERN", "v{version}-rc.{rc}"),
            ("RELEASE_PATTERN", "app-v{version}"),
            ("COMPONENTS", components),
            (
                "BOOKKEEPING",
                r#"[".release-please-manifest.json", "apps/website/CHANGELOG.md", "apps/app/CHANGELOG.md"]"#,
            ),
            ("GATE_RECORD", "docs/releases/v{version}.md"),
        ],
    )
}

#[test]
fn the_gate_record_is_an_allowed_delta_and_another_components_tag_is_certified() {
    let repo = components_cycle("docs/releases/v1.2.0.md");
    let r = component_candidate(&repo, "website-v0.3.1", r#"["app", "website"]"#);
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("rc-tag").as_deref(), Some("v1.2.0-rc.2"));
    // A single-component product never accepts a tag it cannot read.
    let r = component_candidate(&repo, "website-v0.3.1", "[]");
    assert_eq!(r.code, 1, "{}", r.log);
}

#[test]
fn a_gate_record_of_another_version_is_not_bookkeeping() {
    let repo = components_cycle("docs/releases/v1.1.0.md");
    let r = component_candidate(&repo, "website-v0.3.1", r#"["app", "website"]"#);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("docs/releases/v1.1.0.md"), "{}", r.log);
}

#[test]
fn a_resume_deploys_the_newest_candidate_on_its_exact_commit() {
    let repo = cycle();
    let rc10 = repo.git(&["rev-parse", "acme-v1.2.0-rc.10"]);
    release(&repo, "acme-v1.2.0", &[("CHANGELOG.md", "## 1.2.0\n")]);
    // HEAD is the release: not the commit behind the RC.
    let r = candidate(&repo, &[("RESUME_FROM", "acme-v1.2.0-rc.10")]);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("is not the commit behind"), "{}", r.log);
    repo.git(&["checkout", "-q", "--detach", "acme-v1.2.0-rc.10"]);
    let r = candidate(&repo, &[("RESUME_FROM", "acme-v1.2.0-rc.10")]);
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

fn go(merger_input: &str, pulls: Option<&str>, pull: Option<&str>) -> (Ran, Scratch) {
    let dir = Scratch::new("go");
    if let Some(p) = pulls {
        put(&dir.0, "pulls", p);
    }
    if let Some(p) = pull {
        put(&dir.0, "pull", p);
    }
    gh(
        &dir.0,
        r#"'api repos/o/r/commits/abc/pulls') [ -f "$S/pulls" ] || exit 1; cat "$S/pulls" ;;
'api repos/o/r/pulls/42') [ -f "$S/pull" ] || exit 1; cat "$S/pull" ;;"#,
    );
    let r = run_with(
        &body("verify", "go"),
        &dir.0,
        &dir.0,
        &[
            ("REPO", "o/r"),
            ("RELEASE_TAG", "acme-v1.2.0"),
            ("RELEASE_SHA", "abc"),
            ("GO_MERGER", merger_input),
            ("BRANCH_PREFIX", "release/"),
        ],
    );
    (r, dir)
}

fn associated(number: u64, base: &str, head: &str, merge_sha: &str) -> String {
    format!(
        r#"{{"number": {number}, "merged_at": "2026-10-07T10:00:00Z", "merge_commit_sha": "{merge_sha}", "base": {{"ref": "{base}"}}, "head": {{"ref": "{head}"}}}}"#
    )
}

/// The PR as REST reads it, merged by `login`, an account of `kind`
/// (`Bot`, `User`, `Organization`, or empty when GitHub gave none).
fn merged_as(login: &str, kind: &str) -> String {
    format!(
        r#"{{"number": 42, "merged": true, "merged_by": {{"login": "{login}", "type": "{kind}"}}, "base": {{"ref": "release/2026-10-01-10-00-utc"}}}}"#
    )
}

fn merged_by(login: &str) -> String {
    merged_as(login, "Bot")
}

const SLUG: &str = "the-bridge";

#[test]
fn the_go_is_the_release_please_pr_merged_by_the_app_in_either_login_form() {
    let pulls = format!(
        "[{}, {}]",
        associated(
            42,
            "release/2026-10-01-10-00-utc",
            "release-please--branches--release/2026-10-01-10-00-utc",
            "abc"
        ),
        // A PR that merely contains the commit is not the one that made it.
        associated(7, "main", "feat/x", "other")
    );
    let bot = format!("{SLUG}[bot]");
    for input in [SLUG.to_string(), format!("app/{SLUG}"), bot.clone()] {
        let (r, dir) = go(&input, Some(&pulls), Some(&merged_by(&bot)));
        assert_eq!(r.code, 0, "{input}: {}", r.log);
        assert_eq!(r.output("release-pr-number").as_deref(), Some("42"));
        assert_eq!(r.output("go-merged-by").as_deref(), Some(bot.as_str()));
        assert_eq!(r.output("go-merged-by-kind").as_deref(), Some("app"));
        assert_eq!(
            r.output("release-branch").as_deref(),
            Some("release/2026-10-01-10-00-utc")
        );
        assert_eq!(r.output("verified").as_deref(), Some("true"));
        let summary = read(&dir.0, "summary");
        assert!(summary.contains("GO: #42"), "{summary}");
        assert!(summary.contains("(the bridge's App)"), "{summary}");
    }
    let (r, _d) = go(SLUG, Some(&pulls), Some(&merged_by(&format!("app/{SLUG}"))));
    assert_eq!(r.code, 0, "{}", r.log);
}

/// Spec 09 (BR-ASDLC-021): a human with write access may merge the
/// release-please PR in GitHub instead of the bridge; the rulesets decide
/// who may, the workflow does not. The merge is the GO either way, and the
/// outputs and the summary say it was a human.
#[test]
fn a_release_pr_merged_by_a_human_is_the_go_and_says_so() {
    let pulls = format!(
        "[{}]",
        associated(
            42,
            "release/2026-10-01-10-00-utc",
            "release-please--branches--release/2026-10-01-10-00-utc",
            "abc"
        )
    );
    let (r, dir) = go(SLUG, Some(&pulls), Some(&merged_as("owner-login", "User")));
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("release-pr-number").as_deref(), Some("42"));
    assert_eq!(r.output("go-merged-by").as_deref(), Some("owner-login"));
    assert_eq!(r.output("go-merged-by-kind").as_deref(), Some("human"));
    assert_eq!(
        r.output("release-branch").as_deref(),
        Some("release/2026-10-01-10-00-utc")
    );
    assert_eq!(r.output("verified").as_deref(), Some("true"));
    let summary = read(&dir.0, "summary");
    assert!(
        summary.contains(
            "GO: #42 merged into `release/2026-10-01-10-00-utc` by `owner-login` (a human)."
        ),
        "{summary}"
    );
    // The job and the workflow both expose who and which kind.
    let d = doc();
    assert_eq!(
        d["jobs"]["verify"]["outputs"]["go-merged-by-kind"].as_str(),
        Some("${{ steps.go.outputs.go-merged-by-kind }}")
    );
    assert_eq!(
        d["on"]["workflow_call"]["outputs"]["go-merged-by-kind"]["value"].as_str(),
        Some("${{ jobs.verify.outputs.go-merged-by-kind }}")
    );
}

#[test]
fn a_release_pr_merged_by_another_bot_or_nobody_readable_is_no_go() {
    let pulls = format!(
        "[{}]",
        associated(
            42,
            "release/r",
            "release-please--branches--release/r",
            "abc"
        )
    );
    for (login, kind) in [
        // Another App, one whose name merely starts like the bridge's, and
        // the Actions token: a token a CI job holds is never the GO.
        (format!("{SLUG}-{}[bot]", "impostor"), "Bot"),
        (format!("{}[bot]", "other-app"), "Bot"),
        (format!("{}[bot]", "github-actions"), "Bot"),
        // A login with no account type, or one that is not a person.
        ("owner-login".to_string(), ""),
        ("example-org".to_string(), "Organization"),
        (String::new(), "User"),
        (String::new(), ""),
    ] {
        let (r, _d) = go(SLUG, Some(&pulls), Some(&merged_as(&login, kind)));
        assert_eq!(r.code, 1, "{login} {kind}: {}", r.log);
        assert!(
            r.log
                .contains("neither the bridge's App (the-bridge) nor a human account: no GO"),
            "{}",
            r.log
        );
        assert_eq!(r.output("verified"), None);
        assert_eq!(r.output("go-merged-by-kind"), None);
    }
    let not_merged = r#"{"number": 42, "merged": false, "merged_by": null}"#;
    let (r, _d) = go(SLUG, Some(&pulls), Some(not_merged));
    assert_eq!(r.code, 1, "{}", r.log);
}

#[test]
fn no_release_please_pr_two_of_them_or_an_unreadable_one_is_no_go() {
    let bot = merged_by(&format!("{SLUG}[bot]"));
    let on_main = format!(
        "[{}]",
        associated(42, "main", "release-please--branches--main", "abc")
    );
    let not_release_please = format!("[{}]", associated(42, "release/r", "fix/x", "abc"));
    let two = format!(
        "[{}, {}]",
        associated(42, "release/r", "release-please--a", "abc"),
        associated(43, "release/r", "release-please--b", "abc")
    );
    for (pulls, count) in [
        (on_main.as_str(), 0),
        (&not_release_please, 0),
        (&two, 2),
        ("[]", 0),
    ] {
        let (r, _d) = go(SLUG, Some(pulls), Some(&bot));
        assert_eq!(r.code, 1, "{pulls}: {}", r.log);
        assert!(
            r.log.contains(&format!("{count} release-please PR(s)")),
            "{}",
            r.log
        );
    }
    let good = format!(
        "[{}]",
        associated(42, "release/r", "release-please--a", "abc")
    );
    let (r, _d) = go(SLUG, None, Some(&bot));
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("could not be read"), "{}", r.log);
    let (r, _d) = go(SLUG, Some(&good), None);
    assert_eq!(r.code, 1, "{}", r.log);
    let (r, _d) = go("", Some(&good), Some(&bot));
    assert_eq!(r.code, 1, "{}", r.log);
}

// ---------------------------------------------------------------- record-deployed

const BRANCH: &str = "release/2026-10-01-10-00-utc";

fn pr_json(number: u64, title: &str, labels: &[&str]) -> String {
    let labels: Vec<String> = labels
        .iter()
        .map(|l| format!(r#"{{"name": "{l}"}}"#))
        .collect();
    format!(
        r#"{{"number": {number}, "title": "{title}", "labels": [{}]}}"#,
        labels.join(", ")
    )
}

/// `gh` for the ledger: merged PR pages per label, label writes appended to
/// `labels-<n>` (unless `drop` exists), refused when `refuse` exists.
fn ledger_gh(dir: &Path, tagged: Option<&str>, pending: &str) {
    if let Some(t) = tagged {
        put(dir, "tagged", t);
    }
    put(dir, "pending", pending);
    gh(
        dir,
        r#"'pr list --state merged --base release/2026-10-01-10-00-utc --label autorelease: tagged --limit 100 --json number,title,labels') [ -f "$S/tagged" ] || exit 1; cat "$S/tagged" ;;
'pr list --state merged --base release/2026-10-01-10-00-utc --label autorelease: pending --limit 100 --json number,title,labels') cat "$S/pending" ;;
'api -X POST repos/o/r/issues/'*'/labels -f labels[]=deployed: production')
  [ -f "$S/refuse" ] && exit 1
  n="${4#repos/o/r/issues/}"; n="${n%/labels}"
  [ -f "$S/drop" ] || echo 'deployed: production' >> "$S/labels-$n" ;;
'api repos/o/r/issues/'*'/labels --jq .[].name')
  n="${2#repos/o/r/issues/}"; n="${n%/labels}"
  echo 'autorelease: tagged'; cat "$S/labels-$n" 2>/dev/null || true ;;"#,
    );
}

fn record(dir: &Path, in_production: &str, resume: &str) -> Ran {
    run_with(
        &body("record-deployed", "record"),
        dir,
        dir,
        &[
            ("REPO", "o/r"),
            ("IN_PRODUCTION", in_production),
            ("BRANCH", BRANCH),
            ("RESUME_FROM", resume),
            ("LABEL", "deployed: production"),
            ("TAGGED_LABEL", "autorelease: tagged"),
            ("PENDING_LABEL", "autorelease: pending"),
            ("READBACK_SECONDS", "0"),
        ],
    )
}

fn three_components() -> String {
    format!(
        "[{}, {}, {}]",
        pr_json(
            11,
            "chore(release/2026-10-01-10-00-utc): release app 1.2.0",
            &["autorelease: tagged"]
        ),
        pr_json(
            12,
            "chore(release/2026-10-01-10-00-utc): release website 0.3.1",
            &["autorelease: tagged"]
        ),
        pr_json(
            13,
            "chore(release/2026-10-01-10-00-utc): release app-admin 0.2.0",
            &["autorelease: tagged"]
        )
    )
}

#[test]
fn the_ledger_labels_the_release_pr_of_each_component_in_production_and_only_those() {
    let dir = Scratch::new("record");
    ledger_gh(&dir.0, Some(&three_components()), "[]");
    let r = record(&dir.0, r#"["app", "website"]"#, "");
    assert_eq!(r.code, 0, "{}", r.log);
    let writes: Vec<String> = calls(&dir.0, "gh")
        .into_iter()
        .filter(|c| c.contains("-X POST"))
        .collect();
    assert_eq!(
        writes,
        vec![
            "api -X POST repos/o/r/issues/11/labels -f labels[]=deployed: production",
            "api -X POST repos/o/r/issues/12/labels -f labels[]=deployed: production",
        ]
    );
    // Each write is read back.
    let reads = calls(&dir.0, "gh")
        .into_iter()
        .filter(|c| c.starts_with("api repos/o/r/issues/"))
        .count();
    assert_eq!(reads, 2);
    let recorded = r.output("recorded").unwrap();
    assert!(
        recorded.contains(r#"{"component":"app","branch":"release/2026-10-01-10-00-utc","pr":11}"#),
        "{recorded}"
    );
    assert_eq!(r.output("skipped").as_deref(), Some("[]"));
    // Never repository-wide.
    for c in calls(&dir.0, "gh") {
        if c.starts_with("pr list") {
            assert!(
                c.contains("--base release/") && c.contains("--state merged"),
                "{c}"
            );
        }
    }
}

#[test]
fn nothing_in_production_writes_nothing_and_asks_nothing_and_unknown_is_red() {
    let dir = Scratch::new("record");
    ledger_gh(&dir.0, Some(&three_components()), "[]");
    let r = record(&dir.0, "[]", "");
    assert_eq!(r.code, 0, "{}", r.log);
    assert!(calls(&dir.0, "gh").is_empty());
    let r = record(&dir.0, "", "");
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("unknown"), "{}", r.log);
    assert!(calls(&dir.0, "gh").is_empty());
    let r = record(&dir.0, "app", "");
    assert_eq!(r.code, 1, "{}", r.log);
}

#[test]
fn a_component_without_a_merged_release_pr_is_red_automatically_and_a_notice_on_a_resume() {
    let dir = Scratch::new("record");
    ledger_gh(&dir.0, Some(&three_components()), "[]");
    let r = record(&dir.0, r#"["docs"]"#, "");
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("no merged release PR"), "{}", r.log);
    let r = record(&dir.0, r#"["docs", "website"]"#, "v1.2.0-rc.2");
    assert_eq!(r.code, 0, "{}", r.log);
    assert!(r.log.contains("::notice::docs"), "{}", r.log);
    assert!(r.output("recorded").unwrap().contains(r#""pr":12"#));
}

#[test]
fn a_release_pr_merged_but_never_tagged_is_found_under_its_pending_label() {
    let dir = Scratch::new("record");
    let pending = format!(
        "[{}]",
        pr_json(
            14,
            "chore(release/2026-10-01-10-00-utc): release website 0.3.1",
            &["autorelease: pending"]
        )
    );
    ledger_gh(&dir.0, Some("[]"), &pending);
    let r = record(&dir.0, r#"["website"]"#, "");
    assert_eq!(r.code, 0, "{}", r.log);
    assert!(r.output("recorded").unwrap().contains(r#""pr":14"#));
}

#[test]
fn a_refused_write_an_unread_label_or_an_unreadable_list_is_red() {
    let dir = Scratch::new("record");
    ledger_gh(&dir.0, Some(&three_components()), "[]");
    put(&dir.0, "refuse", "");
    let r = record(&dir.0, r#"["app", "website"]"#, "");
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("Pull requests permission"), "{}", r.log);
    // The loop goes on: both components were tried.
    assert_eq!(
        calls(&dir.0, "gh")
            .iter()
            .filter(|c| c.contains("-X POST"))
            .count(),
        2
    );
    assert!(r.output("skipped").unwrap().contains("label refused"));

    let dir = Scratch::new("record");
    ledger_gh(&dir.0, Some(&three_components()), "[]");
    put(&dir.0, "drop", "");
    let r = record(&dir.0, r#"["app"]"#, "");
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("does not carry"), "{}", r.log);
    // Three reads, then the verdict.
    assert_eq!(
        calls(&dir.0, "gh")
            .iter()
            .filter(|c| c.starts_with("api repos/o/r/issues/11/labels"))
            .count(),
        3
    );
    assert_eq!(r.output("recorded").as_deref(), Some("[]"));

    let dir = Scratch::new("record");
    ledger_gh(&dir.0, None, "[]");
    let r = record(&dir.0, r#"["app"]"#, "");
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(
        !calls(&dir.0, "gh").iter().any(|c| c.contains("-X POST")),
        "a write after an unreadable list"
    );
}

// ---------------------------------------------------------------- close: refs

/// A repository with an `origin` holding main, the release branch and tags.
struct Remote {
    repo: Repo,
    origin: Scratch,
}

fn remote_cycle() -> Remote {
    let repo = cycle();
    release(&repo, "acme-v1.2.0", &[("CHANGELOG.md", "## 1.2.0\n")]);
    let origin = Scratch::new("origin");
    git_at(&origin.0, &["init", "-q", "--bare"]);
    repo.git(&["remote", "add", "origin", origin.0.to_str().unwrap()]);
    repo.git(&["push", "-q", "origin", "main", BRANCH, "--tags"]);
    repo.git(&["checkout", "-q", "--detach", "acme-v1.2.0"]);
    Remote { repo, origin }
}

fn refs_gh(dir: &Path, open: Option<&str>) {
    if let Some(o) = open {
        put(dir, "open", o);
    }
    gh(
        dir,
        r#"'pr list --state open --base release/2026-10-01-10-00-utc --label autorelease: pending --json number,title') [ -f "$S/open" ] || exit 1; cat "$S/open" ;;"#,
    );
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
        ("PENDING_LABEL", "autorelease: pending"),
    ];
    for (k, v) in env {
        all.retain(|(x, _)| x != k);
        all.push((k, v));
    }
    run_with(&body("close", "refs"), dir, cwd, &all)
}

#[test]
fn the_release_branch_is_read_off_the_refs_and_an_open_release_pr_holds_it_green() {
    let rem = remote_cycle();
    let dir = Scratch::new("refs");
    refs_gh(&dir.0, Some("[]"));
    let r = refs(&dir.0, &rem.repo.dir.0, &[]);
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("branch").as_deref(), Some(BRANCH));
    assert_eq!(r.output("held").as_deref(), Some(""));

    let dir = Scratch::new("refs");
    refs_gh(
        &dir.0,
        Some(r#"[{"number": 15, "title": "chore: release app 1.2.0"}]"#),
    );
    let r = refs(&dir.0, &rem.repo.dir.0, &[]);
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("held").as_deref(), Some("open-release-pr"));
    assert!(r.log.contains("#15"), "{}", r.log);

    let dir = Scratch::new("refs");
    refs_gh(&dir.0, None);
    let r = refs(&dir.0, &rem.repo.dir.0, &[]);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("could not be listed"), "{}", r.log);
}

#[test]
fn no_release_branch_is_fine_only_when_the_tag_is_already_on_main() {
    let rem = remote_cycle();
    let dir = Scratch::new("refs");
    refs_gh(&dir.0, Some("[]"));
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
    refs_gh(&dir.0, Some("[]"));
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
    refs_gh(&dir.0, Some("[]"));
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
    refs_gh(&dir.0, Some("[]"));
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

// ---------------------------------------------------------------- close: ledger

fn close_ledger(dir: &Path, replay: &str) -> Ran {
    run_with(
        &body("close", "ledger"),
        dir,
        dir,
        &[
            ("BRANCH", BRANCH),
            ("COMPONENTS", r#"["app", "website", "app-admin"]"#),
            ("LABEL", "deployed: production"),
            ("TAGGED_LABEL", "autorelease: tagged"),
            ("PENDING_LABEL", "autorelease: pending"),
            ("REPLAY", replay),
        ],
    )
}

fn deployed_title(n: u64, component: &str, deployed: bool) -> String {
    let mut labels = vec!["autorelease: tagged"];
    if deployed {
        labels.push("deployed: production");
    }
    pr_json(
        n,
        &format!("chore(release/2026-10-01-10-00-utc): release {component} 1.0.0"),
        &labels,
    )
}

#[test]
fn every_embarked_component_in_production_lets_the_merge_back_run() {
    let dir = Scratch::new("ledger");
    let tagged = format!(
        "[{}, {}]",
        deployed_title(11, "app", true),
        deployed_title(12, "website", true)
    );
    ledger_gh(&dir.0, Some(&tagged), "[]");
    let r = close_ledger(&dir.0, "false");
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("held").as_deref(), Some(""));
    assert!(
        r.log.contains("app (#11)") && r.log.contains("website (#12)"),
        "{}",
        r.log
    );
}

#[test]
fn an_embarked_component_not_in_production_holds_the_cycle_open_green_and_loud() {
    let dir = Scratch::new("ledger");
    let tagged = format!(
        "[{}, {}]",
        deployed_title(11, "app", false),
        deployed_title(12, "website", true)
    );
    // Merged and never tagged: embarked all the same.
    let pending = format!(
        "[{}]",
        pr_json(
            13,
            "chore(release/2026-10-01-10-00-utc): release app-admin 0.2.0",
            &["autorelease: pending"]
        )
    );
    ledger_gh(&dir.0, Some(&tagged), &pending);
    let r = close_ledger(&dir.0, "false");
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("held").as_deref(), Some("undeployed"));
    assert!(r.log.contains("::warning::The cycle on"), "{}", r.log);
    assert!(
        r.log.contains("app (#11) is merged but not in production"),
        "{}",
        r.log
    );
    assert!(r.log.contains("app-admin (#13)"), "{}", r.log);
    assert!(read(&dir.0, "summary").contains("replay"), "the recipe");
    // The replay is the human decision, and it says what it closes over.
    let r = close_ledger(&dir.0, "true");
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("held").as_deref(), Some(""));
    assert!(
        r.log.contains("Closing over an explicit replay: app (#11)"),
        "{}",
        r.log
    );
}

#[test]
fn the_ledger_fails_closed_on_nothing_embarked_an_unknown_component_or_an_unreadable_list() {
    let dir = Scratch::new("ledger");
    ledger_gh(&dir.0, Some("[]"), "[]");
    let r = close_ledger(&dir.0, "false");
    assert_eq!(r.code, 0, "{}", r.log);
    assert_eq!(r.output("held").as_deref(), Some("undeployed"));
    assert!(r.log.contains("not established"), "{}", r.log);

    let dir = Scratch::new("ledger");
    let tagged = format!(
        "[{}]",
        pr_json(11, "chore: release docs 1.0.0", &["autorelease: tagged"])
    );
    ledger_gh(&dir.0, Some(&tagged), "[]");
    let r = close_ledger(&dir.0, "false");
    assert_eq!(r.output("held").as_deref(), Some("undeployed"), "{}", r.log);

    let dir = Scratch::new("ledger");
    ledger_gh(&dir.0, None, "[]");
    let r = close_ledger(&dir.0, "false");
    assert_eq!(r.code, 1, "{}", r.log);
    assert_eq!(r.output("held").as_deref(), Some("unknown"));
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
    refs_gh(&dir.0, Some("[]"));
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
            ("HELD", ""),
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
    assert_eq!(guard("record-deployed", "", "false", "skipped").code, 0);
    assert_eq!(guard("Close", "success", "false", "skipped").code, 1);
}

#[test]
fn the_setup_docs_resolve_a_conflict_off_the_release_branch() {
    let text =
        std::fs::read_to_string(crate::harness::root().join("docs/setup/repository.md")).unwrap();
    assert!(text.contains("chore/merge-back-"), "{text}");
    assert!(text.contains("never by bringing `main` into"), "{text}");
}
