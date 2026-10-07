//! `release-cut` (REQ-CI-035..054): its steps run as shipped against a real
//! repository and a bare `origin`, with `gh` stubbed where GitHub answers.

use std::path::Path;

use regex::Regex;

use crate::harness::{
    Ran, Repo, Scratch, calls, path_with, run_body, run_owned, step, stub, workflow,
};

const CUT: &str = ".github/workflows/asdlc-release-cut.yml";

fn body(job: &str, id: &str) -> String {
    run_body(&step(&workflow(CUT), job, id))
}

/// The cut job's environment, as the job-level `env:` sets it, plus `extra`.
fn env(stubs: &Path, extra: &[(&'static str, &str)]) -> Vec<(&'static str, String)> {
    let mut e: Vec<(&'static str, String)> = vec![
        ("PATH", path_with(stubs)),
        ("ACTION", "candidate".into()),
        ("DRY_RUN", "false".into()),
        ("CANDIDATE_TAG", "acme-v{version}-rc.{rc}".into()),
        ("RELEASE_TAG", "acme-v{version}".into()),
        ("BRANCH_PREFIX", "release/".into()),
        ("DEFAULT_BRANCH", "main".into()),
        ("GH_REPO", "example-org/acme".into()),
        ("POLL_INTERVAL", "0".into()),
    ];
    for (k, v) in extra {
        e.retain(|(key, _)| key != k);
        e.push((k, v.to_string()));
    }
    e
}

fn exec(job: &str, id: &str, repo: &Repo, stubs: &Path, extra: &[(&'static str, &str)]) -> Ran {
    run_owned(&body(job, id), &env(stubs, extra), &repo.dir.0)
}

fn refused(r: &Ran, words: &str) {
    assert_eq!(r.code, 1, "expected a refusal: {}", r.log);
    assert!(r.log.contains("::error"), "{}", r.log);
    assert!(r.log.contains(words), "expected {words:?} in: {}", r.log);
}

fn ok(r: &Ran) {
    assert_eq!(r.code, 0, "{}", r.log);
}

const BRANCH: &str = "release/2026-10-07-12-00-utc";

/// A repository whose origin holds `main` and, when `branch`, the release
/// branch `BRANCH` checked out locally.
fn cycle(branch: bool) -> (Repo, Scratch) {
    let (repo, origin) = Repo::with_origin();
    if branch {
        repo.git(&["checkout", "-q", "-b", BRANCH]);
        repo.git(&["push", "-q", "origin", BRANCH]);
    }
    (repo, origin)
}

// ---------------------------------------------------------------- preflight

fn preflight(extra: &[(&'static str, &str)]) -> Ran {
    let dir = Scratch::new("preflight");
    let mut e = vec![
        ("ACTION", "open".to_string()),
        ("CLASS", String::new()),
        ("ROUTINE_BECAUSE", String::new()),
        ("CANDIDATE_TAG", "acme-v{version}-rc.{rc}".into()),
        ("RELEASE_TAG", "acme-v{version}".into()),
        ("COMPONENT", "acme".into()),
        ("BRANCH_PREFIX", "release/".into()),
        ("DEFAULT_BRANCH", "main".into()),
        ("CLASS_PATHS_FILE", ".github/review-paths.json".into()),
        ("MANIFEST_FILE", ".release-please-manifest.json".into()),
    ];
    for (k, v) in extra {
        e.retain(|(key, _)| key != k);
        e.push((k, v.to_string()));
    }
    run_owned(&body("preflight", "validate"), &e, &dir.0)
}

#[test]
fn preflight_accepts_both_gestures_and_an_empty_or_named_class() {
    for action in ["open", "candidate"] {
        for class in ["", "routine", "sensible"] {
            ok(&preflight(&[("ACTION", action), ("CLASS", class)]));
        }
    }
    ok(&preflight(&[
        ("CLASS", "routine"),
        ("ROUTINE_BECAUSE", "docs only"),
    ]));
    ok(&preflight(&[
        ("CANDIDATE_TAG", "v{version}-rc.{rc}"),
        ("RELEASE_TAG", "v{version}"),
        ("BRANCH_PREFIX", "rel/cycles/"),
    ]));
}

#[test]
fn preflight_refuses_what_it_cannot_cut_before_touching_anything() {
    let cases: &[(&[(&'static str, &str)], &str)] = &[
        (&[("ACTION", "close")], "Unknown action 'close'"),
        (&[("ACTION", "")], "Unknown action ''"),
        (
            &[("CLASS", "Routine")],
            "Expected 'routine', 'sensible', or empty",
        ),
        (&[("CLASS", "unclassified")], "Unknown release class"),
        (
            &[("CLASS", "sensible"), ("ROUTINE_BECAUSE", "why")],
            "only applies to the class 'routine'",
        ),
        (
            &[("ROUTINE_BECAUSE", "why")],
            "only applies to the class 'routine' (the class is 'computed')",
        ),
        (
            &[("CANDIDATE_TAG", "acme-v{version}")],
            "must hold {rc} exactly once",
        ),
        (
            &[("CANDIDATE_TAG", "acme-rc.{rc}-v{version}")],
            "must name {version} before {rc}",
        ),
        (
            &[("RELEASE_TAG", "acme-v{version}-{x}")],
            "holds a placeholder other than",
        ),
        (
            &[("RELEASE_TAG", "acme v{version}")],
            "may hold letters, digits",
        ),
        (
            &[("RELEASE_TAG", "acme-v{version}{version}")],
            "{version} exactly once",
        ),
        (
            &[("BRANCH_PREFIX", "release")],
            "must be a path ending in '/'",
        ),
        (
            &[("COMPONENT", "")],
            "is not a release-please component name",
        ),
        (
            &[("CLASS_PATHS_FILE", "../x.json")],
            "inside the repository",
        ),
        (&[("MANIFEST_FILE", "/etc/x")], "inside the repository"),
    ];
    for (extra, words) in cases {
        let r = preflight(extra);
        refused(&r, words);
    }
}

// ------------------------------------------------------------ find the cycle

fn find(repo: &Repo, action: &str, dispatched: &str) -> Ran {
    let stubs = Scratch::new("stubs");
    exec(
        "cut",
        "cycle",
        repo,
        &stubs.0,
        &[("ACTION", action), ("DISPATCHED_REF", dispatched)],
    )
}

#[test]
fn open_names_a_utc_minute_branch_from_the_default_branch_tip() {
    let (repo, _origin) = cycle(false);
    let before = utc_minute();
    let r = find(&repo, "open", "main");
    ok(&r);
    let branch = r.output("branch").unwrap();
    let shape = Regex::new(r"^release/[0-9]{4}-[0-9]{2}-[0-9]{2}-[0-9]{2}-[0-9]{2}-utc$").unwrap();
    assert!(shape.is_match(&branch), "{branch}");
    let after = utc_minute();
    assert!(
        branch == format!("release/{before}-utc") || branch == format!("release/{after}-utc"),
        "{branch} is not the UTC minute ({before}..{after})"
    );
}

fn utc_minute() -> String {
    let out = std::process::Command::new("date")
        .args(["-u", "+%Y-%m-%d-%H-%M"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn open_refuses_while_a_cycle_is_in_flight_and_names_it() {
    let (repo, _origin) = cycle(false);
    repo.git(&[
        "push",
        "-q",
        "origin",
        "main:refs/heads/release/2026-10-01-08-30-utc",
    ]);
    let r = find(&repo, "open", "main");
    refused(&r, "already in flight: release/2026-10-01-08-30-utc");
    // Any branch under the prefix counts, whatever its shape.
    let (repo, _origin) = cycle(false);
    repo.git(&["push", "-q", "origin", "main:refs/heads/release/2026-09-05"]);
    refused(&find(&repo, "open", "main"), "release/2026-09-05");
}

#[test]
fn open_refuses_when_origin_cannot_be_read() {
    let (repo, origin) = cycle(false);
    repo.git(&[
        "remote",
        "set-url",
        "origin",
        &origin.0.join("gone").display().to_string(),
    ]);
    refused(
        &find(&repo, "open", "main"),
        "Could not list the release branches",
    );
}

#[test]
fn open_refuses_a_default_branch_that_moved() {
    let (repo, _origin) = cycle(false);
    repo.write("src/new.rs", "pub fn n() {}\n");
    repo.commit("newer");
    repo.git(&["push", "-q", "origin", "main"]);
    repo.git(&["reset", "-q", "--hard", "HEAD~1"]);
    refused(
        &find(&repo, "open", "main"),
        "main moved while this run started",
    );
}

#[test]
fn a_candidate_is_cut_on_the_one_release_branch_from_origin() {
    // Nothing in flight.
    let (repo, _origin) = cycle(false);
    refused(
        &find(&repo, "candidate", "main"),
        "No release branch is in flight",
    );
    // Dispatched anywhere else than the branch in flight.
    let (repo, _origin) = cycle(true);
    repo.git(&["checkout", "-q", "main"]);
    refused(
        &find(&repo, "candidate", "main"),
        &format!("dispatch the cut on '{BRANCH}', not on 'main'"),
    );
    // On the branch: found on origin, checked out, equal to its remote.
    let r = find(&repo, "candidate", BRANCH);
    ok(&r);
    assert_eq!(r.output("branch").as_deref(), Some(BRANCH));
    assert_eq!(repo.git(&["rev-parse", "--abbrev-ref", "HEAD"]), BRANCH);
    // Two branches break the one-cycle invariant.
    repo.git(&[
        "push",
        "-q",
        "origin",
        "main:refs/heads/release/2026-10-08-09-00-utc",
    ]);
    refused(
        &find(&repo, "candidate", BRANCH),
        "Several release branches",
    );
    // The retired date-only shape is not a release branch.
    let (repo, _origin) = cycle(false);
    repo.git(&["push", "-q", "origin", "main:refs/heads/release/2026-09-05"]);
    refused(
        &find(&repo, "candidate", "release/2026-09-05"),
        "'release/2026-09-05' is not a release branch",
    );
}

#[test]
fn modified_tracked_files_refuse_the_cut() {
    let (repo, _origin) = cycle(true);
    repo.write("README.md", "edited\n");
    refused(
        &find(&repo, "candidate", BRANCH),
        "Tracked files are modified",
    );
}

// -------------------------------------------- open: push, wait for the run

/// A `gh` that answers `run list` from `rows` (one JSON per call, the last
/// one repeated), through the caller's own `--jq` filter.
fn gh_runs(stubs: &Path, rows: &[&str], fail: bool) {
    let mut cases = String::new();
    for (i, row) in rows.iter().enumerate() {
        cases.push_str(&format!("    {}) json='{}' ;;\n", i + 1, row));
    }
    let last = rows.last().copied().unwrap_or("[]");
    stub(
        stubs,
        "gh",
        &format!(
            r#"if [ "{fail}" = true ]; then echo "HTTP 401: Bad credentials" >&2; exit 1; fi
counter="$(dirname "$0")/runs.n"
n=$(( $(cat "$counter" 2>/dev/null || echo 0) + 1 ))
echo "$n" > "$counter"
filter=''
while [ $# -gt 0 ]; do
  if [ "$1" = --jq ]; then filter="$2"; fi
  shift
done
case "$n" in
{cases}    *) json='{last}' ;;
esac
printf '%s' "$json" | jq -r "$filter""#
        ),
    );
}

fn open_step(repo: &Repo, stubs: &Path, branch: &str) -> Ran {
    exec(
        "cut",
        "open",
        repo,
        stubs,
        &[
            ("ACTION", "open"),
            ("BRANCH", branch),
            ("RP_WORKFLOW", "release-please.yml"),
        ],
    )
}

#[test]
fn open_pushes_the_branch_and_waits_on_the_run_however_long() {
    let (repo, origin) = cycle(false);
    let stubs = Scratch::new("stubs");
    gh_runs(
        &stubs.0,
        &[
            "[]",
            r#"[{"status":"queued","conclusion":null,"url":"https://example.test/runs/7"}]"#,
            r#"[{"status":"in_progress","conclusion":"","url":"https://example.test/runs/7"}]"#,
            r#"[{"status":"completed","conclusion":"success","url":"https://example.test/runs/7"}]"#,
        ],
        false,
    );
    let r = open_step(&repo, &stubs.0, BRANCH);
    ok(&r);
    assert_eq!(
        r.output("release-run-url").as_deref(),
        Some("https://example.test/runs/7")
    );
    // The branch is on origin, at the default branch's tip.
    let remote = repo.git(&["ls-remote", "--heads", "origin", BRANCH]);
    assert!(
        remote.starts_with(&repo.git(&["rev-parse", "main"])),
        "{remote}"
    );
    let _ = origin;
    let gh = calls(&stubs.0, "gh");
    assert_eq!(gh.len(), 4, "{gh:?}");
    for c in &gh {
        assert!(
            c.starts_with(&format!(
                "run list --workflow release-please.yml --branch {BRANCH} --limit 1 "
            )),
            "{c}"
        );
    }
    assert!(
        !gh.iter().any(|c| c.starts_with("workflow run")),
        "the cut never dispatches release-please"
    );
}

#[test]
fn a_leftover_local_branch_of_the_same_minute_does_not_break_open() {
    let (repo, _origin) = cycle(false);
    repo.git(&["branch", BRANCH]);
    repo.write("src/more.rs", "pub fn m() {}\n");
    repo.commit("more");
    repo.git(&["push", "-q", "origin", "main"]);
    let stubs = Scratch::new("stubs");
    gh_runs(
        &stubs.0,
        &[r#"[{"status":"completed","conclusion":"success","url":"u"}]"#],
        false,
    );
    ok(&open_step(&repo, &stubs.0, BRANCH));
    assert_eq!(
        repo.git(&["rev-parse", BRANCH]),
        repo.git(&["rev-parse", "main"])
    );
}

#[test]
fn a_failed_release_please_run_refuses_now_and_leaves_the_branch() {
    let (repo, _origin) = cycle(false);
    let stubs = Scratch::new("stubs");
    gh_runs(
        &stubs.0,
        &[r#"[{"status":"completed","conclusion":"failure","url":"https://example.test/runs/9"}]"#],
        false,
    );
    let r = open_step(&repo, &stubs.0, BRANCH);
    refused(&r, "did not succeed on");
    assert!(r.log.contains("https://example.test/runs/9"), "{}", r.log);
    assert!(r.log.contains("left in place"), "{}", r.log);
    assert!(
        r.log
            .contains(&format!("git push origin --delete {BRANCH}"))
    );
    assert!(
        !repo
            .git(&["ls-remote", "--heads", "origin", BRANCH])
            .is_empty()
    );
}

#[test]
fn a_broken_run_listing_refuses_now_instead_of_waiting() {
    let (repo, _origin) = cycle(false);
    let stubs = Scratch::new("stubs");
    gh_runs(&stubs.0, &["[]"], true);
    let r = open_step(&repo, &stubs.0, BRANCH);
    refused(&r, "gh run list failed");
    assert!(r.log.contains("Bad credentials"), "{}", r.log);
    assert!(r.log.contains("gh auth status"), "{}", r.log);
    assert_eq!(calls(&stubs.0, "gh").len(), 1);
}

// ------------------------------------------------------- the staged version

/// A `gh` whose `pr list --label` answers `labelled` and plain `pr list`
/// answers `all`; `fail` makes every call fail.
fn gh_prs(stubs: &Path, labelled: &str, all: &str, fail: bool) {
    std::fs::write(stubs.join("labelled.json"), labelled).unwrap();
    std::fs::write(stubs.join("all.json"), all).unwrap();
    stub(
        stubs,
        "gh",
        r#"if [ -e "$(dirname "$0")/fail" ]; then echo "HTTP 502" >&2; exit 1; fi
case " $* " in
  *" --label "*) cat "$(dirname "$0")/labelled.json" ;;
  *) cat "$(dirname "$0")/all.json" ;;
esac"#,
    );
    if fail {
        std::fs::write(stubs.join("fail"), "").unwrap();
    }
}

fn version(repo: &Repo, stubs: &Path, action: &str, branch: &str) -> Ran {
    exec(
        "cut",
        "version",
        repo,
        stubs,
        &[
            ("ACTION", action),
            ("BRANCH", branch),
            ("COMPONENT", "acme"),
            ("PR_LABEL", "autorelease: pending"),
            ("RUN_URL", "https://example.test/runs/7"),
            ("MANIFEST_FILE", ".release-please-manifest.json"),
            ("MANIFEST_KEY", "."),
        ],
    )
}

fn pr(title: &str, body: &str, head: &str) -> String {
    serde_json::json!({"number": 1, "title": title, "body": body, "headRefName": head}).to_string()
}

#[test]
fn the_version_is_the_components_in_the_pr_against_this_base() {
    let (repo, _origin) = cycle(true);
    let cases = [
        // The title first.
        (
            pr(
                "chore(release/x): release acme 1.4.0",
                "<summary>acme: 9.9.9</summary>",
                "release-please--branches--release/x",
            ),
            "1.4.0",
        ),
        // Then the body, whatever the head is called.
        (
            pr(
                "chore: release release/x",
                "<details><summary>acme: 1.5.0</summary>",
                "anything",
            ),
            "1.5.0",
        ),
        // `acme` inside another name is another component.
        (
            pr(
                "chore: release webacme 7.0.0",
                "<summary>acme : 1.6.0</summary>",
                "x",
            ),
            "1.6.0",
        ),
    ];
    for (one, want) in cases {
        let stubs = Scratch::new("stubs");
        gh_prs(&stubs.0, &format!("[{one}]"), "[]", false);
        let r = version(&repo, &stubs.0, "open", BRANCH);
        ok(&r);
        assert_eq!(r.output("version").as_deref(), Some(want), "{}", r.log);
        for c in calls(&stubs.0, "gh") {
            assert!(c.contains(&format!("--base {BRANCH} ")), "{c}");
            assert!(!c.contains("--head"), "{c}");
        }
    }
}

#[test]
fn another_components_release_pr_never_names_the_candidate() {
    let (repo, _origin) = cycle(true);
    let stubs = Scratch::new("stubs");
    let other = pr(
        "chore(release/x): release website 3.0.0",
        "<summary>website: 3.0.0</summary>",
        "release-please--branches--release/x--components--website",
    );
    let mine = pr(
        "chore(release/x): release acme 1.7.0",
        "",
        "release-please--branches--release/x--components--acme",
    );
    gh_prs(&stubs.0, &format!("[{other},{mine}]"), "[]", false);
    let r = version(&repo, &stubs.0, "open", BRANCH);
    assert_eq!(r.output("version").as_deref(), Some("1.7.0"), "{}", r.log);
    let stubs = Scratch::new("stubs");
    gh_prs(&stubs.0, &format!("[{other}]"), "[]", false);
    refused(
        &version(&repo, &stubs.0, "open", BRANCH),
        "no open release PR names 'acme'",
    );
}

#[test]
fn an_unlabelled_release_pr_is_found_by_its_head_prefix() {
    let (repo, _origin) = cycle(true);
    let stubs = Scratch::new("stubs");
    let unrelated = pr("feat: release acme 6.6.6", "", "feature/x");
    let rp = pr(
        "chore: release",
        "<summary>acme: 2.0.0</summary>",
        "release-please--branches--release/x",
    );
    gh_prs(&stubs.0, "[]", &format!("[{unrelated},{rp}]"), false);
    let r = version(&repo, &stubs.0, "open", BRANCH);
    assert_eq!(r.output("version").as_deref(), Some("2.0.0"), "{}", r.log);
    let gh = calls(&stubs.0, "gh");
    assert_eq!(gh.len(), 2, "{gh:?}");
    assert!(gh[0].contains("--label autorelease: pending"), "{gh:?}");
}

#[test]
fn open_without_a_staged_version_refuses_and_names_the_run() {
    let (repo, _origin) = cycle(true);
    let stubs = Scratch::new("stubs");
    gh_prs(&stubs.0, "[]", "[]", false);
    let r = version(&repo, &stubs.0, "open", BRANCH);
    refused(&r, "https://example.test/runs/7");
    assert!(r.log.contains("Most likely nothing is staged"), "{}", r.log);
    assert!(r.log.contains("left in place"), "{}", r.log);
}

#[test]
fn a_failed_pr_read_refuses_instead_of_falling_back() {
    let (repo, _origin) = cycle(true);
    repo.git(&["tag", "acme-v1.1.0-rc.1"]);
    for action in ["open", "candidate"] {
        let stubs = Scratch::new("stubs");
        gh_prs(&stubs.0, "[]", "[]", true);
        refused(
            &version(&repo, &stubs.0, action, BRANCH),
            "Could not read the open release PRs",
        );
    }
}

#[test]
fn a_candidate_without_a_pr_reuses_the_cycles_newest_unpublished_version() {
    let (repo, _origin) = cycle(true);
    repo.git(&["tag", "acme-v1.0.0"]);
    repo.git(&["tag", "acme-v1.0.0-rc.3"]);
    repo.write("src/fix.rs", "pub fn f() {}\n");
    repo.commit("fix");
    repo.git(&["tag", "acme-v1.1.0-rc.1"]);
    repo.git(&["tag", "acme-v1.1.0-rc.2"]);
    repo.git(&["tag", "acme-v1.10.0-rc.1", "HEAD~1"]);
    // Not reachable from the branch: another line's candidate.
    repo.git(&[
        "tag",
        "acme-v9.0.0-rc.1",
        &repo.git(&["commit-tree", "-m", "x", "HEAD^{tree}"]),
    ]);
    let stubs = Scratch::new("stubs");
    gh_prs(&stubs.0, "[]", "[]", false);
    let r = version(&repo, &stubs.0, "candidate", BRANCH);
    ok(&r);
    // 1.10.0 > 1.1.0: numeric, not textual.
    assert_eq!(r.output("version").as_deref(), Some("1.10.0"), "{}", r.log);
    // Once that version is published, nothing is staged.
    repo.git(&["tag", "acme-v1.10.0"]);
    let stubs = Scratch::new("stubs");
    gh_prs(&stubs.0, "[]", "[]", false);
    refused(
        &version(&repo, &stubs.0, "candidate", BRANCH),
        "is already published (acme-v1.10.0)",
    );
}

#[test]
fn a_dry_run_of_open_estimates_the_version_from_the_manifest_and_calls_nothing() {
    let (repo, _origin) = cycle(false);
    repo.write(".release-please-manifest.json", "{\".\": \"1.2.3\"}\n");
    repo.commit("manifest");
    let stubs = Scratch::new("stubs");
    gh_prs(&stubs.0, "[]", "[]", true);
    let r = exec(
        "cut",
        "version",
        &repo,
        &stubs.0,
        &[
            ("ACTION", "open"),
            ("DRY_RUN", "true"),
            ("BRANCH", BRANCH),
            ("COMPONENT", "acme"),
            ("PR_LABEL", "autorelease: pending"),
            ("MANIFEST_FILE", ".release-please-manifest.json"),
            ("MANIFEST_KEY", "."),
        ],
    );
    ok(&r);
    assert_eq!(r.output("version").as_deref(), Some("1.2.3"));
    assert!(r.log.contains("estimate"), "{}", r.log);
    assert!(calls(&stubs.0, "gh").is_empty());
}

// ------------------------------------------------------ class, round, delta

const TABLE: &str = r#"{"paths": ["src/auth/**", ".github/workflows/**"], "exceptions": {"src/x.rs": "documented"}}"#;

/// A release branch whose published base is `acme-v1.0.0`, carrying the
/// review table, with one more commit touching `path`.
fn delta(path: &str) -> (Repo, Scratch) {
    let (repo, origin) = Repo::with_origin();
    repo.write(".github/review-paths.json", TABLE);
    repo.commit("table");
    repo.git(&["tag", "acme-v1.0.0"]);
    repo.git(&["push", "-q", "origin", "main", "--tags"]);
    repo.git(&["checkout", "-q", "-b", BRANCH]);
    repo.write(path, "changed\n");
    repo.commit("change");
    repo.git(&["push", "-q", "origin", BRANCH, "--tags"]);
    (repo, origin)
}

fn compute(repo: &Repo, extra: &[(&'static str, &str)]) -> Ran {
    let stubs = Scratch::new("stubs");
    let mut e: Vec<(&'static str, &str)> = vec![
        ("ACTION", "candidate"),
        ("BRANCH", BRANCH),
        ("VERSION", "1.1.0"),
        ("CLASS", ""),
        ("ROUTINE_BECAUSE", ""),
        ("MAIN_MERGE_BECAUSE", ""),
        ("CLASS_PATHS_FILE", ".github/review-paths.json"),
    ];
    for (k, v) in extra {
        e.retain(|(key, _)| key != k);
        e.push((k, v));
    }
    exec("cut", "candidate", repo, &stubs.0, &e)
}

fn trailers(r: &Ran) -> String {
    r.block("trailers").unwrap_or_default()
}

#[test]
fn the_class_is_computed_from_the_reviewed_paths_the_candidate_carries() {
    let (repo, _o) = delta("docs/guide.md");
    let r = compute(&repo, &[]);
    ok(&r);
    assert_eq!(r.output("class").as_deref(), Some("routine"));
    assert_eq!(
        r.output("class-reason").as_deref(),
        Some("no reviewed path touched")
    );
    assert_eq!(r.output("tag").as_deref(), Some("acme-v1.1.0-rc.1"));
    assert_eq!(r.output("round").as_deref(), Some("1"));
    assert_eq!(r.output("previous-tag").as_deref(), Some("acme-v1.0.0"));
    let t = trailers(&r);
    assert!(t.contains("Release-Class: routine\n"), "{t}");
    assert!(t.contains("Release-Class-Base: acme-v1.0.0\n"), "{t}");
    assert!(t.ends_with("Release-Class-Surfaces: none"), "{t}");

    // `*` crosses `/`, and a directory glob matches what lies under it.
    for path in ["src/auth/deep/token.rs", ".github/workflows/ci.yml"] {
        let (repo, _o) = delta(path);
        let r = compute(&repo, &[]);
        ok(&r);
        assert_eq!(r.output("class").as_deref(), Some("sensible"), "{path}");
        assert_eq!(
            r.output("class-reason"),
            Some(format!("touches the reviewed path {path}"))
        );
    }
}

#[test]
fn every_way_of_not_knowing_the_class_is_sensible() {
    // No published release to measure from.
    let (repo, _o) = delta("docs/a.md");
    repo.git(&["tag", "-d", "acme-v1.0.0"]);
    repo.git(&["push", "-q", "origin", ":refs/tags/acme-v1.0.0"]);
    let r = compute(&repo, &[]);
    ok(&r);
    assert_eq!(r.output("class").as_deref(), Some("sensible"));
    assert!(
        r.output("class-reason")
            .unwrap()
            .starts_with("the candidate class could not be computed: no published release"),
        "{}",
        r.log
    );
    assert!(trailers(&r).contains("Release-Class-Base: none"));
    assert!(trailers(&r).contains("Release-Class-Surfaces: unknown"));
    // An RC tag is never the published base.
    repo.git(&["tag", "acme-v0.9.0-rc.1", "HEAD~1"]);
    let r = compute(&repo, &[]);
    assert_eq!(r.output("class").as_deref(), Some("sensible"));
    // The candidate carries no table, or an unusable one.
    for table in [
        None,
        Some("{\"paths\": []}"),
        Some("not json"),
        Some("{\"paths\": [\"\"]}"),
    ] {
        let (repo, _o) = delta("docs/a.md");
        match table {
            None => {
                repo.git(&["rm", "-q", ".github/review-paths.json"]);
            }
            Some(t) => repo.write(".github/review-paths.json", t),
        }
        repo.commit("table");
        let r = compute(&repo, &[]);
        ok(&r);
        assert_eq!(r.output("class").as_deref(), Some("sensible"), "{table:?}");
        assert!(
            r.output("class-reason")
                .unwrap()
                .contains("could not be computed"),
            "{table:?}"
        );
    }
    // An empty delta is nobody telling.
    let (repo, _o) = delta("docs/a.md");
    repo.git(&["tag", "-f", "acme-v1.0.0"]);
    repo.git(&["push", "-q", "-f", "origin", "acme-v1.0.0"]);
    let r = compute(&repo, &[]);
    assert_eq!(r.output("class").as_deref(), Some("sensible"), "{}", r.log);
    assert!(
        r.output("class-reason")
            .unwrap()
            .contains("no path changed")
    );
}

#[test]
fn routine_on_a_reviewed_path_needs_a_reason_and_the_tag_records_it() {
    let (repo, _o) = delta("src/auth/session.rs");
    let r = compute(&repo, &[("CLASS", "routine")]);
    refused(&r, "Refusing the class 'routine'");
    assert!(
        r.log.contains("src/auth/session.rs  (src/auth/**)"),
        "{}",
        r.log
    );
    assert!(r.log.contains("class 'sensible'"), "{}", r.log);
    let r = compute(
        &repo,
        &[
            ("CLASS", "routine"),
            ("ROUTINE_BECAUSE", "a comment\n\tin the session module"),
        ],
    );
    ok(&r);
    assert_eq!(
        r.output("class-reason").as_deref(),
        Some("forced with the class input")
    );
    let t = trailers(&r);
    assert!(t.contains("Release-Class: routine\n"), "{t}");
    assert!(t.contains("Release-Class-Surfaces: src/auth/**\n"), "{t}");
    assert!(
        t.contains("Release-Class-Justification: a comment in the session module"),
        "{t}"
    );
    // Sensible never needs a reason.
    let r = compute(&repo, &[("CLASS", "sensible")]);
    ok(&r);
    assert!(!trailers(&r).contains("Justification"));
    // A clean delta records no justification: it would read as a waiver.
    let (repo, _o) = delta("docs/a.md");
    let r = compute(&repo, &[("CLASS", "routine"), ("ROUTINE_BECAUSE", "why")]);
    ok(&r);
    assert!(!trailers(&r).contains("Justification"), "{}", trailers(&r));
    // Unknown reviewed paths are treated like a hit.
    repo.write(".github/review-paths.json", "garbage");
    repo.commit("break the table");
    refused(
        &compute(&repo, &[("CLASS", "routine")]),
        "reviewed paths are 'unknown'",
    );
}

#[test]
fn the_next_round_is_the_lowest_unused_and_reopens_from_the_previous_candidate() {
    let (repo, _o) = delta("docs/a.md");
    repo.git(&["tag", "acme-v1.1.0-rc.1", "HEAD~1"]);
    repo.git(&["push", "-q", "origin", "--tags"]);
    let r = compute(&repo, &[]);
    ok(&r);
    assert_eq!(r.output("tag").as_deref(), Some("acme-v1.1.0-rc.2"));
    assert_eq!(
        r.output("previous-tag").as_deref(),
        Some("acme-v1.1.0-rc.1")
    );
    assert!(
        r.log
            .contains("Changed since acme-v1.1.0-rc.1 (1 file(s)):"),
        "{}",
        r.log
    );
    // The class is still measured from the published release.
    assert!(trailers(&r).contains("Release-Class-Base: acme-v1.0.0"));
    assert!(!r.log.contains("::warning"), "{}", r.log);
    // A third round is a warning, not a refusal.
    repo.git(&["tag", "acme-v1.1.0-rc.2"]);
    let r = compute(&repo, &[]);
    ok(&r);
    assert_eq!(r.output("round").as_deref(), Some("3"));
    assert!(r.log.contains("Two rounds is the cap"), "{}", r.log);
}

#[test]
fn renames_and_non_ascii_paths_are_listed_whole() {
    let (repo, _o) = delta("docs/a.md");
    repo.git(&["tag", "acme-v1.1.0-rc.1"]);
    repo.git(&["mv", "docs/a.md", "docs/été.md"]);
    repo.commit("rename");
    let r = compute(&repo, &[]);
    ok(&r);
    assert!(r.log.contains("  docs/a.md\n"), "{}", r.log);
    assert!(r.log.contains("  docs/été.md\n"), "{}", r.log);
}

#[test]
fn an_unreadable_delta_since_the_previous_candidate_refuses() {
    let (repo, _o) = delta("docs/a.md");
    let blob = repo.git(&["hash-object", "-w", "README.md"]);
    repo.git(&["tag", "acme-v1.1.0-rc.1", &blob]);
    refused(
        &compute(&repo, &[]),
        "Could not list the files changed since acme-v1.1.0-rc.1",
    );
}

#[test]
fn main_merged_into_the_cycle_is_refused_unless_justified() {
    let (repo, _o) = delta("docs/a.md");
    repo.git(&["tag", "acme-v1.1.0-rc.1"]);
    repo.git(&["push", "-q", "origin", "--tags"]);
    // A fix on the release branch itself is not a merge of main.
    repo.write("src/fix.rs", "pub fn f() {}\n");
    repo.commit("fix(x): on the branch");
    let r = compute(&repo, &[]);
    ok(&r);
    assert!(!trailers(&r).contains("Main-Merge"));
    // Main moves on, and is merged into the branch.
    repo.git(&["checkout", "-q", "main"]);
    repo.write("src/feature.rs", "pub fn g() {}\n");
    repo.commit("feat: on main");
    repo.git(&["push", "-q", "origin", "main"]);
    repo.git(&["checkout", "-q", BRANCH]);
    repo.git(&["merge", "-q", "--no-edit", "main"]);
    let r = compute(&repo, &[]);
    refused(&r, "main was merged into");
    assert!(r.log.contains("feat: on main"), "{}", r.log);
    assert!(r.log.contains("never main merged into it"), "{}", r.log);
    let r = compute(
        &repo,
        &[("MAIN_MERGE_BECAUSE", "the merge-back\nconflicted")],
    );
    ok(&r);
    let t = trailers(&r);
    assert!(t.contains("Release-Main-Merge-Commits: 1\n"), "{t}");
    assert!(
        t.ends_with("Release-Main-Merge-Justification: the merge-back conflicted"),
        "{t}"
    );
    // Once a candidate covers it, it is not asked about again.
    repo.git(&["tag", "acme-v1.1.0-rc.2"]);
    repo.write("src/fix2.rs", "pub fn h() {}\n");
    repo.commit("fix: again");
    let r = compute(&repo, &[]);
    ok(&r);
    assert!(!trailers(&r).contains("Main-Merge"));
}

#[test]
fn the_first_candidate_of_a_cycle_is_never_asked_about_main() {
    let (repo, _o) = delta("docs/a.md");
    repo.git(&["checkout", "-q", "main"]);
    repo.write("src/feature.rs", "pub fn g() {}\n");
    repo.commit("feat: on main");
    repo.git(&["push", "-q", "origin", "main"]);
    repo.git(&["checkout", "-q", BRANCH]);
    repo.git(&["merge", "-q", "--no-edit", "main"]);
    ok(&compute(&repo, &[]));
}

#[test]
fn an_unreadable_default_branch_refuses_a_later_round() {
    let (repo, origin) = delta("docs/a.md");
    repo.git(&["tag", "acme-v1.1.0-rc.1"]);
    repo.git(&["push", "-q", "origin", "--tags"]);
    // Tags still fetch from the copy; main cannot.
    let copy = Scratch::new("copy");
    let out = std::process::Command::new("cp")
        .args([
            "-R",
            &format!("{}/.", origin.0.display()),
            &copy.0.display().to_string(),
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let out = std::process::Command::new("git")
        .args(["update-ref", "-d", "refs/heads/main"])
        .current_dir(&copy.0)
        .output()
        .unwrap();
    assert!(out.status.success());
    repo.git(&["remote", "set-url", "origin", &copy.0.display().to_string()]);
    refused(&compute(&repo, &[]), "Could not read origin/main");
}

// ------------------------------------------------------------- tag and push

fn tag_step(repo: &Repo, computed: &Ran) -> Ran {
    let stubs = Scratch::new("stubs");
    let get = |k: &str| computed.output(k).unwrap_or_default();
    run_owned(
        &body("cut", "tag"),
        &[
            ("PATH", path_with(&stubs.0)),
            ("BRANCH", BRANCH.to_string()),
            ("VERSION", "1.1.0".to_string()),
            ("TAG", get("tag")),
            ("ROUND", get("round")),
            ("CLASS_OUT", get("class")),
            ("CLASS_REASON", get("class-reason")),
            ("PREVIOUS", get("previous-tag")),
            ("COMMIT", get("commit")),
            ("TRAILERS", trailers(computed)),
            ("APP_SLUG", "example-app".to_string()),
        ],
        &repo.dir.0,
    )
}

#[test]
fn the_candidate_is_an_annotated_tag_pushed_without_force_that_the_gate_reads() {
    let (repo, origin) = delta("src/auth/x.rs");
    let computed = compute(
        &repo,
        &[("CLASS", "routine"), ("ROUTINE_BECAUSE", "renames only")],
    );
    ok(&computed);
    let r = tag_step(&repo, &computed);
    ok(&r);
    assert_eq!(r.output("tag").as_deref(), Some("acme-v1.1.0-rc.1"));
    assert_eq!(r.output("class").as_deref(), Some("routine"));
    let git_origin = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(&origin.0)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    assert_eq!(git_origin(&["cat-file", "-t", "acme-v1.1.0-rc.1"]), "tag");
    assert_eq!(
        git_origin(&["rev-parse", "acme-v1.1.0-rc.1^{commit}"]),
        repo.git(&["rev-parse", "HEAD"])
    );
    let message = git_origin(&[
        "for-each-ref",
        "--format=%(contents)",
        "refs/tags/acme-v1.1.0-rc.1",
    ]);
    assert!(
        message.starts_with("Release candidate acme-v1.1.0-rc.1\n\nRelease-Class: routine\n"),
        "{message}"
    );
    assert!(message.contains("Release-Class-Justification: renames only"));
    let tagger = git_origin(&[
        "for-each-ref",
        "--format=%(taggername)",
        "refs/tags/acme-v1.1.0-rc.1",
    ]);
    assert_eq!(tagger, format!("{}[bot]", "example-app"));

    // The gate's resolver reads back the class the cut wrote.
    let gate = workflow(".github/workflows/asdlc-release-gate.yml");
    let resolved = crate::harness::run(
        &run_body(&step(&gate, "candidate", "class")),
        &[
            ("EVENT_NAME", "push"),
            ("DISPATCH_CLASS", ""),
            ("REF_TYPE", "tag"),
            ("REF_NAME", "acme-v1.1.0-rc.1"),
        ],
        &repo.dir.0,
    );
    assert_eq!(resolved.output("class").as_deref(), Some("routine"));

    // A tag that exists is never moved: a second push fails, leaving nothing.
    repo.git(&["tag", "-d", "acme-v1.1.0-rc.1"]);
    repo.write("docs/b.md", "b\n");
    repo.commit("b");
    let again = compute(&repo, &[("CLASS", "sensible")]);
    ok(&again);
    let stale = Ran {
        code: 0,
        log: String::new(),
        outputs: again
            .outputs
            .replace("acme-v1.1.0-rc.2", "acme-v1.1.0-rc.1"),
    };
    // Only origin has it now (a local one would already refuse the creation).
    repo.git(&["tag", "-d", "acme-v1.1.0-rc.1"]);
    let r = tag_step(&repo, &stale);
    refused(&r, "Could not push acme-v1.1.0-rc.1");
    assert!(
        repo.git(&["tag", "--list", "acme-v1.1.0-rc.1"]).is_empty(),
        "the local tag is removed"
    );
}

#[test]
fn a_moved_head_is_never_tagged() {
    let (repo, _o) = delta("docs/a.md");
    let computed = compute(&repo, &[]);
    repo.write("docs/c.md", "c\n");
    repo.commit("c");
    refused(&tag_step(&repo, &computed), "HEAD moved");
}

// -------------------------------------------------------------------- shape

#[test]
fn the_cut_is_two_called_jobs_with_the_bridges_one_input() {
    let doc = workflow(CUT);
    let jobs: Vec<&str> = doc["jobs"]
        .as_hash()
        .unwrap()
        .keys()
        .filter_map(|k| k.as_str())
        .collect();
    assert_eq!(jobs, ["preflight", "cut"]);
    assert_eq!(
        doc["jobs"]["preflight"]["runs-on"].as_str(),
        Some("ubuntu-latest")
    );
    assert_eq!(doc["jobs"]["cut"]["needs"].as_str(), Some("preflight"));
    assert_eq!(
        doc["jobs"]["cut"]["if"].as_str(),
        Some("needs.preflight.result == 'success'")
    );
    let inputs = doc["on"]["workflow_call"]["inputs"].as_hash().unwrap();
    // `action` is the one input the bridge passes; the others the caller
    // fixes or its dispatch defaults.
    for (k, v) in inputs {
        let k = k.as_str().unwrap();
        let required = v["required"].as_bool().unwrap_or(false);
        let fixed = ["action", "candidate-tag", "release-tag", "component"].contains(&k);
        assert_eq!(required, fixed, "{k}");
        if !required {
            assert!(!v["default"].is_badvalue(), "{k} has no default");
        }
    }
    let outputs: Vec<&str> = doc["on"]["workflow_call"]["outputs"]
        .as_hash()
        .unwrap()
        .keys()
        .filter_map(|k| k.as_str())
        .collect();
    for want in [
        "branch",
        "tag",
        "version",
        "round",
        "class",
        "class-reason",
        "previous-tag",
        "release-run-url",
    ] {
        assert!(outputs.contains(&want), "{want}");
    }
}

#[test]
fn every_push_is_made_with_the_app_token_and_nothing_dispatches_release_please() {
    let doc = workflow(CUT);
    let steps = doc["jobs"]["cut"]["steps"].as_vec().unwrap();
    assert_eq!(steps[0]["id"].as_str(), Some("app-token"));
    assert!(
        steps[0]["uses"]
            .as_str()
            .unwrap()
            .starts_with("actions/create-github-app-token@")
    );
    assert_eq!(
        steps[0]["with"]["app-id"].as_str(),
        Some("${{ secrets.SPL_ASDLC_APP_ID }}")
    );
    assert!(
        steps[1]["uses"]
            .as_str()
            .unwrap()
            .starts_with("actions/checkout@")
    );
    assert_eq!(
        steps[1]["with"]["token"].as_str(),
        Some("${{ steps.app-token.outputs.token }}")
    );
    assert_eq!(
        steps[1]["with"]["ref"].as_str(),
        Some("${{ inputs.default-branch }}")
    );
    let text = std::fs::read_to_string(crate::harness::root().join(CUT)).unwrap();
    for forbidden in [
        "workflow run",
        "dispatches",
        "GITHUB_TOKEN }}",
        "github.token",
    ] {
        let hits: Vec<&str> = text
            .lines()
            .filter(|l| !l.trim_start().starts_with('#') && l.contains(forbidden))
            .collect();
        assert!(hits.is_empty(), "{forbidden}: {hits:?}");
    }
}

#[test]
fn a_dry_run_creates_and_pushes_nothing() {
    let doc = workflow(CUT);
    assert_eq!(
        step(&doc, "cut", "open")["if"].as_str(),
        Some("inputs.action == 'open' && !inputs.dry-run")
    );
    assert_eq!(
        step(&doc, "cut", "tag")["if"].as_str(),
        Some("!inputs.dry-run")
    );
    // The job outputs come from the tag step only: empty on a dry run.
    for (k, v) in doc["jobs"]["cut"]["outputs"].as_hash().unwrap() {
        let v = v.as_str().unwrap();
        let k = k.as_str().unwrap();
        assert!(
            v.contains("steps.tag.") || k == "release-run-url" || k == "dry-run",
            "{k}: {v}"
        );
    }
    let (repo, _o) = delta("docs/a.md");
    let r = compute(&repo, &[("DRY_RUN", "true")]);
    ok(&r);
    for line in [
        "Candidate: acme-v1.1.0-rc.1",
        "Branch:    ",
        "Commit:    ",
        "Class:     routine",
        "Surfaces:  none",
    ] {
        assert!(r.log.contains(line), "{line}: {}", r.log);
    }
    assert!(r.log.contains("(dry run: nothing tagged, nothing pushed)"));
    assert!(repo.git(&["tag", "--list", "acme-v1.1.0-rc.1"]).is_empty());
}
