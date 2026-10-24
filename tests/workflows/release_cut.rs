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
        ("SPL_REPOSITORY", "example-org/spl-asdlc".into()),
        ("SPL_VERSION", "latest".into()),
        ("CONFIG_FILE", ".spl-asdlc/config.toml".into()),
        ("BRANCH_PREFIX", "release/".into()),
        ("DEFAULT_BRANCH", "main".into()),
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
    // A minor line `v<X.Y>` is a channel too: its newest published patch.
    for channel in [
        "latest",
        "staging",
        "v0.4.0",
        "v10.20.30",
        "v1.2",
        "v0.4",
        "v10.20",
    ] {
        ok(&preflight(&[("SPL_VERSION", channel)]));
    }
    ok(&preflight(&[("FORCE_VERSION", "2.0.0")]));
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
        (&[("SPL_REPOSITORY", "spl-asdlc")], "is not <owner>/<repo>"),
        (
            &[("SPL_VERSION", "main")],
            "spl-asdlc-version 'main' is not a channel",
        ),
        (&[("SPL_VERSION", "0.4.0")], "is not a channel"),
        (&[("SPL_VERSION", "v1")], "is not a channel"),
        (&[("SPL_VERSION", "1.2")], "is not a channel"),
        (&[("SPL_VERSION", "v1.2.x")], "is not a channel"),
        (&[("SPL_VERSION", "v1.2.")], "is not a channel"),
        (&[("SPL_VERSION", "v1.x")], "is not a channel"),
        (&[("SPL_VERSION", "")], "is not a channel"),
        (&[("CONFIG_FILE", "../x.toml")], "inside the repository"),
        (&[("CONFIG_FILE", "/etc/x")], "inside the repository"),
        (&[("CONFIG_FILE", "")], "inside the repository"),
        (&[("FORCE_VERSION", "1.2")], "version '1.2' is not X.Y.Z"),
        (&[("FORCE_VERSION", "v1.2.0")], "is not X.Y.Z"),
        (
            &[("ACTION", "candidate"), ("FORCE_VERSION", "1.2.0")],
            "version only applies to 'open'",
        ),
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

// ------------------------------------- open: the published binary, prepared

/// This host's target, as the binary step maps `uname`.
fn target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        _ => None,
    }
}

/// The SHA-256 of a file, as `sha256sum` (or `shasum -a 256`) prints it.
fn sha256(path: &Path) -> String {
    let try_tool = |cmd: &str, args: &[&str]| {
        std::process::Command::new(cmd)
            .args(args)
            .arg(path)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
    };
    let line = try_tool("sha256sum", &[])
        .or_else(|| try_tool("shasum", &["-a", "256"]))
        .expect("a sha256 tool");
    line.split_whitespace().next().unwrap().to_string()
}

/// The release `tag` of the method as `gh release download` serves it from
/// `serve/<tag>/`: the archive of this host's target, holding this crate's
/// own binary, and its SHA256SUMS. `tamper` changes the archive after its
/// sum was taken.
fn published(serve: &Path, tag: &str, tamper: bool) -> String {
    let t = target().expect("a published target");
    let name = if tag == "staging" {
        format!("spl-asdlc-staging-{t}")
    } else {
        format!("spl-asdlc-{tag}-{t}")
    };
    let build = serve.join("build").join(&name);
    std::fs::create_dir_all(&build).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_spl-asdlc"), build.join("spl-asdlc")).unwrap();
    let out = serve.join(tag);
    std::fs::create_dir_all(&out).unwrap();
    let archive = out.join(format!("{name}.tar.gz"));
    let tar = std::process::Command::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(serve.join("build"))
        .arg(&name)
        .output()
        .unwrap();
    assert!(tar.status.success(), "tar");
    std::fs::write(
        out.join("SHA256SUMS"),
        format!(
            "{}  {name}.tar.gz\n{}  spl-asdlc-{tag}-other-target.zip\n",
            sha256(&archive),
            "0".repeat(64)
        ),
    )
    .unwrap();
    if tamper {
        std::fs::write(&archive, "not the archive that was summed").unwrap();
    }
    name
}

/// A `gh` that names `latest` the latest release, lists `serve/releases.json`
/// for the releases API (failing when there is none), and serves
/// `serve/<tag>/` for `release download`.
fn gh_releases(stubs: &Path, serve: &Path, latest: &str) {
    stub(
        stubs,
        "gh",
        &format!(
            r#"case "$1 $2" in
  "release view") echo "{latest}" ;;
  "api --paginate") cat "{serve}/releases.json" ;;
  "release download")
    tag="$3"; dir=''; patterns=()
    shift 3
    while [ $# -gt 0 ]; do
      case "$1" in
        --dir) dir="$2"; shift 2 ;;
        --pattern) patterns+=("$2"); shift 2 ;;
        *) shift ;;
      esac
    done
    for p in "${{patterns[@]}}"; do cp "{serve}/$tag/$p" "$dir/" || exit 1; done
    ;;
  *) echo "unexpected gh $*" >&2; exit 1 ;;
esac"#,
            serve = serve.display()
        ),
    );
}

fn binary(stubs: &Path, channel: &str) -> Ran {
    let (repo, _origin) = cycle(false);
    exec(
        "cut",
        "binary",
        &repo,
        stubs,
        &[
            ("ACTION", "open"),
            ("SPL_REPOSITORY", "example-org/spl-asdlc"),
            ("SPL_VERSION", channel),
            ("GH_TOKEN", "a-token"),
        ],
    )
}

#[test]
fn open_runs_the_published_binary_of_its_channel_checked_against_its_sums() {
    if target().is_none() {
        return;
    }
    let serve = Scratch::new("serve");
    let stubs = Scratch::new("stubs");
    let name = published(&serve.0, "v9.9.9", false);
    gh_releases(&stubs.0, &serve.0, "v9.9.9");
    let r = binary(&stubs.0, "latest");
    ok(&r);
    let bin = r.output("bin").expect("the binary's path");
    assert!(bin.ends_with(&format!("{name}/spl-asdlc")), "{bin}");
    assert!(r.log.contains("checksum verified"), "{}", r.log);
    let seen = calls(&stubs.0, "gh");
    assert_eq!(
        seen[0],
        "release view --repo example-org/spl-asdlc --json tagName --jq .tagName"
    );
    assert!(
        seen[1].starts_with(&format!(
            "release download v9.9.9 --repo example-org/spl-asdlc --pattern {name}.tar.gz --pattern SHA256SUMS --dir "
        )),
        "{seen:?}"
    );
    // A release tag is that release: nothing asks which is the latest.
    let stubs = Scratch::new("stubs");
    let name = published(&serve.0, "v1.2.3", false);
    gh_releases(&stubs.0, &serve.0, "v9.9.9");
    let r = binary(&stubs.0, "v1.2.3");
    ok(&r);
    assert!(r.output("bin").unwrap().contains(&name));
    assert_eq!(calls(&stubs.0, "gh").len(), 1, "one download");
    // The staging channel is the rolling prerelease, built for Linux only.
    let stubs = Scratch::new("stubs");
    if target() == Some("x86_64-unknown-linux-gnu") {
        let name = published(&serve.0, "staging", false);
        gh_releases(&stubs.0, &serve.0, "v9.9.9");
        let r = binary(&stubs.0, "staging");
        ok(&r);
        assert!(r.output("bin").unwrap().contains(&name));
    } else {
        gh_releases(&stubs.0, &serve.0, "v9.9.9");
        refused(&binary(&stubs.0, "staging"), "The staging channel builds");
    }
}

/// One release as the releases API lists it.
fn release(tag: &str, draft: bool, prerelease: bool) -> String {
    format!(r#"{{"tag_name":"{tag}","draft":{draft},"prerelease":{prerelease}}}"#)
}

#[test]
fn a_minor_line_runs_its_newest_published_patch() {
    if target().is_none() {
        return;
    }
    let serve = Scratch::new("serve");
    // Two pages, newest first by date: the line's newest patch by version
    // is v1.2.10, published before v1.2.3 and ranked above v1.2.9; the
    // draft, the prerelease, the candidate and the other lines are ignored.
    let first = [
        release("v1.2.3", false, false),
        release("v1.2.12", true, false),
        release("v1.2.11", false, true),
        release("v1.3.0", false, false),
        release("v1.20.1", false, false),
    ];
    let second = [
        release("v1.2.10", false, false),
        release("v1.2.9-rc.1", false, false),
        release("v1.2.9", false, false),
        release("v11.2.99", false, false),
        release("staging", false, true),
    ];
    std::fs::write(
        serve.0.join("releases.json"),
        format!("[{}]\n[{}]\n", first.join(","), second.join(",")),
    )
    .unwrap();
    let name = published(&serve.0, "v1.2.10", false);
    let stubs = Scratch::new("stubs");
    gh_releases(&stubs.0, &serve.0, "v1.3.0");
    let r = binary(&stubs.0, "v1.2");
    ok(&r);
    assert!(r.output("bin").unwrap().contains(&name), "{}", r.log);
    assert!(r.log.contains("from the release v1.2.10 of"), "{}", r.log);
    let seen = calls(&stubs.0, "gh");
    assert_eq!(
        seen[0],
        "api --paginate repos/example-org/spl-asdlc/releases?per_page=100"
    );
    assert!(
        seen[1].starts_with(&format!(
            "release download v1.2.10 --repo example-org/spl-asdlc --pattern {name}.tar.gz "
        )),
        "{seen:?}"
    );
    assert_eq!(seen.len(), 2, "one list, one download: {seen:?}");
    // A line with no published release: only a draft and a prerelease.
    std::fs::write(
        serve.0.join("releases.json"),
        format!(
            "[{},{},{}]",
            release("v1.4.0", true, false),
            release("v1.4.0-rc.1", false, true),
            release("v1.3.0", false, false)
        ),
    )
    .unwrap();
    let stubs = Scratch::new("stubs");
    gh_releases(&stubs.0, &serve.0, "v1.3.0");
    let r = binary(&stubs.0, "v1.4");
    refused(&r, "has no published release of the line v1.4");
    assert!(r.output("bin").is_none());
    assert_eq!(calls(&stubs.0, "gh").len(), 1, "nothing is downloaded");
    // Releases that cannot be listed.
    let stubs = Scratch::new("stubs");
    gh_releases(&stubs.0, &Scratch::new("empty").0, "v1.3.0");
    refused(&binary(&stubs.0, "v1.2"), "Could not list the releases");
}

#[test]
fn a_binary_its_sums_do_not_vouch_for_never_runs() {
    if target().is_none() {
        return;
    }
    let serve = Scratch::new("serve");
    let stubs = Scratch::new("stubs");
    published(&serve.0, "v9.9.9", true);
    gh_releases(&stubs.0, &serve.0, "v9.9.9");
    let r = binary(&stubs.0, "latest");
    refused(&r, "does not match its SHA256SUMS line");
    assert!(r.output("bin").is_none());
    // A release whose sums list another archive only.
    let serve = Scratch::new("serve");
    let name = published(&serve.0, "v9.9.9", false);
    let sums = serve.0.join("v9.9.9/SHA256SUMS");
    let text = std::fs::read_to_string(&sums).unwrap();
    std::fs::write(
        &sums,
        text.lines()
            .filter(|l| !l.contains(&name))
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let stubs = Scratch::new("stubs");
    gh_releases(&stubs.0, &serve.0, "v9.9.9");
    refused(&binary(&stubs.0, "latest"), "lists no");
    // A release that cannot be downloaded.
    let stubs = Scratch::new("stubs");
    gh_releases(&stubs.0, &Scratch::new("empty").0, "v9.9.9");
    refused(&binary(&stubs.0, "latest"), "Could not download");
}

/// A product with a release cycle and one feature to release: its
/// configuration, a version file, the feature, all on `main` and its origin.
fn product() -> (Repo, Scratch) {
    let (repo, origin) = Repo::with_origin();
    repo.write(
        ".spl-asdlc/config.toml",
        "schema = 1\n[product]\nname = \"acme\"\n\n[release]\nangles = []\n\n[[release.version_files]]\npath = \"VERSION\"\npattern = \"acme {version}\"\n",
    );
    repo.write("VERSION", "acme 0.0.0\n");
    repo.commit("chore: the release cycle");
    repo.write("src/feature.rs", "pub fn g() {}\n");
    repo.commit("feat: the first feature (#1)");
    repo.git(&["push", "-q", "origin", "main"]);
    (repo, origin)
}

fn open_step(repo: &Repo, extra: &[(&'static str, &str)]) -> Ran {
    let stubs = Scratch::new("stubs");
    let mut e: Vec<(&'static str, &str)> = vec![
        ("ACTION", "open"),
        ("BRANCH", BRANCH),
        ("BIN", env!("CARGO_BIN_EXE_spl-asdlc")),
        ("CONFIG_FILE", ".spl-asdlc/config.toml"),
        ("APP_SLUG", "example-app"),
    ];
    for (k, v) in extra {
        e.retain(|(key, _)| key != k);
        e.push((k, v));
    }
    exec("cut", "open", repo, &stubs.0, &e)
}

fn git_in(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn open_prepares_the_release_and_pushes_the_branch_with_its_first_candidate() {
    let (repo, origin) = product();
    let main = repo.git(&["rev-parse", "main"]);
    let r = open_step(&repo, &[]);
    ok(&r);
    assert_eq!(r.output("version").as_deref(), Some("0.1.0"), "{}", r.log);
    assert_eq!(r.output("branch").as_deref(), Some(BRANCH));
    assert!(r.log.contains("Release notes:"), "{}", r.log);
    // One commit on the new branch: the version and the notes.
    assert_eq!(repo.git(&["rev-parse", "--abbrev-ref", "HEAD"]), BRANCH);
    assert_eq!(repo.git(&["rev-parse", "HEAD~1"]), main);
    assert_eq!(
        repo.git(&["log", "-1", "--format=%s|%an"]),
        format!("chore(release): v0.1.0|{}[bot]", "example-app")
    );
    let files = repo.git(&["show", "--name-only", "--format=", "HEAD"]);
    assert_eq!(files, "CHANGELOG.md\nVERSION");
    assert_eq!(repo.git(&["show", "HEAD:VERSION"]), "acme 0.1.0");
    let changelog = repo.git(&["show", "HEAD:CHANGELOG.md"]);
    assert!(
        changelog.starts_with("# Changelog\n\n## 0.1.0 ("),
        "a first release, measured from the root: {changelog}"
    );
    assert!(
        changelog
            .contains("* the first feature ([#1](https://github.com/example-org/acme/issues/1))"),
        "{changelog}"
    );
    // Nothing on origin yet: the branch goes with its first candidate.
    assert!(git_in(&origin.0, &["branch", "--list", BRANCH]).is_empty());

    let computed = compute(&repo, &[("ACTION", "open"), ("VERSION", "0.1.0")]);
    ok(&computed);
    assert_eq!(computed.output("tag").as_deref(), Some("acme-v0.1.0-rc.1"));
    assert_eq!(computed.output("base").as_deref(), Some("root"));
    let r = tag_step(&repo, &computed, "open");
    ok(&r);
    assert_eq!(r.output("tag").as_deref(), Some("acme-v0.1.0-rc.1"));
    assert_eq!(r.output("version").as_deref(), Some("0.1.0"));
    let head = repo.git(&["rev-parse", "HEAD"]);
    assert_eq!(git_in(&origin.0, &["rev-parse", BRANCH]), head);
    assert_eq!(
        git_in(&origin.0, &["rev-parse", "acme-v0.1.0-rc.1^{commit}"]),
        head
    );
    assert!(
        r.log
            .contains(&format!("--ref {BRANCH} -f action=candidate"))
    );
}

#[test]
fn open_forces_the_version_it_is_given() {
    let (repo, _origin) = product();
    let r = open_step(&repo, &[("FORCE_VERSION", "2.0.0")]);
    ok(&r);
    assert_eq!(r.output("version").as_deref(), Some("2.0.0"), "{}", r.log);
    assert_eq!(
        repo.git(&["log", "-1", "--format=%s"]),
        "chore(release): v2.0.0"
    );
    assert_eq!(repo.git(&["show", "HEAD:VERSION"]), "acme 2.0.0");
}

#[test]
fn open_with_nothing_to_release_creates_nothing() {
    let (repo, origin) = product();
    repo.git(&["tag", "acme-v0.1.0"]);
    let r = open_step(&repo, &[]);
    refused(&r, "release prepare refused");
    assert!(
        r.log
            .contains("nothing to release: no commit since acme-v0.1.0"),
        "{}",
        r.log
    );
    assert!(r.output("version").is_none());
    assert!(git_in(&origin.0, &["branch", "--list", BRANCH]).is_empty());
}

#[test]
fn open_reads_the_configuration_of_the_default_branch() {
    let (repo, _origin) = cycle(false);
    refused(
        &open_step(&repo, &[]),
        "main carries no .spl-asdlc/config.toml",
    );
}

#[test]
fn a_dry_run_of_open_prints_the_version_and_commits_nothing() {
    let (repo, _origin) = product();
    let head = repo.git(&["rev-parse", "HEAD"]);
    let r = open_step(&repo, &[("DRY_RUN", "true")]);
    ok(&r);
    assert_eq!(r.output("version").as_deref(), Some("0.1.0"));
    assert_eq!(repo.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repo.git(&["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    assert_eq!(repo.git(&["status", "--porcelain"]), "");
}

// ------------------------------------------------ candidate: the cycle's version

fn version(repo: &Repo) -> Ran {
    let stubs = Scratch::new("stubs");
    exec("cut", "version", repo, &stubs.0, &[("BRANCH", BRANCH)])
}

#[test]
fn a_candidate_takes_the_version_of_its_cycles_newest_candidate() {
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
    let r = version(&repo);
    ok(&r);
    // 1.10.0 > 1.1.0: numeric, not textual.
    assert_eq!(r.output("version").as_deref(), Some("1.10.0"), "{}", r.log);
    // Once that version is published, nothing is left to cut.
    repo.git(&["tag", "acme-v1.10.0"]);
    refused(&version(&repo), "is already published (acme-v1.10.0)");
}

#[test]
fn a_candidate_on_a_branch_with_no_first_candidate_is_refused() {
    let (repo, _origin) = cycle(true);
    refused(
        &version(&repo),
        "No candidate of this cycle is on release/2026-10-07-12-00-utc",
    );
}

// ------------------------------------------------------ class, round, delta

/// The configuration's path, as the cut's `config-file` input defaults it.
const CONFIG: &str = ".spl-asdlc/config.toml";

/// A configuration whose review angles carry the reviewed paths: one angle
/// on every PR (no paths), two on paths, one glob shared by both.
const TABLE: &str = r#"schema = 1

[review]
coordinators = ["coordinator-login"]

[[review.angles]]
key = "maintainability"

[[review.angles]]
key = "security"
paths = ["src/auth/**", ".github/workflows/**"]

[[review.angles]]
key = "design"
paths = ["src/auth/**"]
"#;

/// A release branch whose published base is `acme-v1.0.0`, carrying the
/// configuration, with one more commit touching `path`.
fn delta(path: &str) -> (Repo, Scratch) {
    let (repo, origin) = Repo::with_origin();
    repo.write(CONFIG, TABLE);
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
        ("CONFIG_FILE", CONFIG),
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
fn review_angles_without_paths_review_no_path_apart_so_the_class_is_routine() {
    let (repo, _o) = delta("src/auth/token.rs");
    repo.write(
        CONFIG,
        "schema = 1\n[review]\nangles = []\n\n[release]\nangles = []\n",
    );
    repo.commit("no review angle");
    let r = compute(&repo, &[]);
    ok(&r);
    assert!(
        r.log
            .contains("No review angle of .spl-asdlc/config.toml carries paths"),
        "{}",
        r.log
    );
    assert_eq!(r.output("class").as_deref(), Some("routine"), "{}", r.log);
    assert!(trailers(&r).contains("Release-Class-Surfaces: none"));
}

#[test]
fn every_way_of_not_knowing_the_class_is_sensible() {
    // A published release exists, but none the candidate's history reaches.
    let (repo, _o) = delta("docs/a.md");
    repo.git(&["tag", "-d", "acme-v1.0.0"]);
    repo.git(&["push", "-q", "origin", ":refs/tags/acme-v1.0.0"]);
    let elsewhere = repo.git(&["commit-tree", "HEAD^{tree}", "-m", "elsewhere"]);
    repo.git(&["tag", "acme-v0.5.0", &elsewhere]);
    repo.git(&["push", "-q", "origin", "acme-v0.5.0"]);
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
    // The candidate carries no configuration, or one whose angles' paths
    // cannot be read.
    for table in [
        None,
        Some("not toml ="),
        Some("[[review.angles]]\nkey = \"security\"\npaths = [\"\"]\n"),
        Some("[[review.angles]]\nkey = \"security\"\npaths = \"src/**\"\n"),
        Some("[review]\nangles = \"security\"\n"),
    ] {
        let (repo, _o) = delta("docs/a.md");
        match table {
            None => {
                repo.git(&["rm", "-q", CONFIG]);
            }
            Some(t) => repo.write(CONFIG, t),
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
fn a_first_release_is_classed_on_its_whole_history_with_no_tag_set_by_hand() {
    // A brand-new product: no release was ever published, only candidates.
    let (repo, _o) = delta("docs/a.md");
    repo.git(&["tag", "-d", "acme-v1.0.0"]);
    repo.git(&["push", "-q", "origin", ":refs/tags/acme-v1.0.0"]);
    repo.git(&["tag", "acme-v0.9.0-rc.1", "HEAD~1"]);
    repo.git(&["push", "-q", "origin", "acme-v0.9.0-rc.1"]);
    let r = compute(&repo, &[]);
    ok(&r);
    assert!(
        r.log.contains("No published release (acme-v*) exists: this is the product's first release, measured from the repository's root (its whole history)."),
        "{}",
        r.log
    );
    // No reviewed path anywhere in the history: routine, and said why.
    assert_eq!(r.output("class").as_deref(), Some("routine"), "{}", r.log);
    assert_eq!(
        r.output("class-reason").as_deref(),
        Some("no reviewed path touched (first release: measured from the repository's root)")
    );
    assert_eq!(r.output("base").as_deref(), Some("root"));
    assert_eq!(r.output("previous-tag").as_deref(), Some(""));
    let t = trailers(&r);
    assert!(t.contains("Release-Class-Base: root\n"), "{t}");
    assert!(t.contains("Release-Class-Surfaces: none"), "{t}");
    assert!(r.log.contains("(since the repository's root)"), "{}", r.log);

    // A reviewed path anywhere in the history, here on `main` before the
    // cycle, makes the first release sensible.
    let (repo, _o) = Repo::with_origin();
    repo.write(CONFIG, TABLE);
    repo.write("src/auth/token.rs", "first\n");
    repo.commit("table and auth");
    repo.git(&["checkout", "-q", "-b", BRANCH]);
    repo.write("docs/a.md", "changed\n");
    repo.commit("change");
    repo.git(&["push", "-q", "origin", "main", BRANCH]);
    let r = compute(&repo, &[]);
    ok(&r);
    assert_eq!(r.output("class").as_deref(), Some("sensible"), "{}", r.log);
    assert_eq!(
        r.output("class-reason").as_deref(),
        Some(
            "touches the reviewed path src/auth/token.rs (first release: measured from the repository's root)"
        )
    );
    assert!(trailers(&r).contains("Release-Class-Surfaces: src/auth/**"));
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
    repo.write(CONFIG, "garbage");
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

fn tag_step(repo: &Repo, computed: &Ran, action: &str) -> Ran {
    let stubs = Scratch::new("stubs");
    let get = |k: &str| computed.output(k).unwrap_or_default();
    let version = get("tag")
        .trim_start_matches("acme-v")
        .split("-rc.")
        .next()
        .unwrap_or_default()
        .to_string();
    run_owned(
        &body("cut", "tag"),
        &[
            ("PATH", path_with(&stubs.0)),
            ("ACTION", action.to_string()),
            ("DEFAULT_BRANCH", "main".to_string()),
            ("BRANCH", BRANCH.to_string()),
            ("VERSION", version),
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
    let r = tag_step(&repo, &computed, "candidate");
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
    let r = tag_step(&repo, &stale, "candidate");
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
    refused(&tag_step(&repo, &computed, "candidate"), "HEAD moved");
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
        let fixed = [
            "action",
            "candidate-tag",
            "release-tag",
            "spl-asdlc-repository",
        ]
        .contains(&k);
        assert_eq!(required, fixed, "{k}");
        if !required {
            assert!(!v["default"].is_badvalue(), "{k} has no default");
        }
    }
    assert_eq!(
        doc["on"]["workflow_call"]["inputs"]["spl-asdlc-version"]["default"].as_str(),
        Some("latest")
    );
    let outputs: Vec<&str> = doc["on"]["workflow_call"]["outputs"]
        .as_hash()
        .unwrap()
        .keys()
        .filter_map(|k| k.as_str())
        .collect();
    assert_eq!(
        outputs,
        [
            "branch",
            "tag",
            "version",
            "round",
            "class",
            "class-reason",
            "previous-tag",
            "dry-run"
        ]
    );
}

#[test]
fn every_push_is_made_with_the_app_token_and_nothing_dispatches_a_workflow() {
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
    // The binary is downloaded with the App's token too: one scoped to the
    // method's repository, which may be private, else this repository's.
    assert_eq!(
        step(&doc, "cut", "binary")["env"]["GH_TOKEN"].as_str(),
        Some("${{ steps.method-token.outputs.token || steps.app-token.outputs.token }}")
    );
    let method = step(&doc, "cut", "method-token");
    assert_eq!(
        method["with"]["repositories"].as_str(),
        Some("${{ steps.method-repo.outputs.name }}")
    );
    assert_eq!(method["continue-on-error"].as_bool(), Some(true));
    let text = std::fs::read_to_string(crate::harness::root().join(CUT)).unwrap();
    let code = || text.lines().filter(|l| !l.trim_start().starts_with('#'));
    for forbidden in ["dispatches", "GITHUB_TOKEN }}", "github.token"] {
        let hits: Vec<&str> = code().filter(|l| l.contains(forbidden)).collect();
        assert!(hits.is_empty(), "{forbidden}: {hits:?}");
    }
    // `gh workflow run` only inside a message (a quoted string) that says
    // how to dispatch the cut, never called.
    let called: Vec<&str> = code()
        .filter(|l| {
            l.match_indices("workflow run")
                .any(|(i, _)| l[..i].matches('"').count() % 2 == 0)
        })
        .collect();
    assert!(called.is_empty(), "{called:?}");
}

#[test]
fn the_cut_waits_for_no_other_workflow() {
    let doc = workflow(CUT);
    let ids: Vec<String> = doc["jobs"]["cut"]["steps"]
        .as_vec()
        .unwrap()
        .iter()
        .filter_map(|s| s["id"].as_str().map(str::to_string))
        .collect();
    assert_eq!(
        ids,
        [
            "app-token",
            "cycle",
            "method-repo",
            "method-token",
            "binary",
            "open",
            "version",
            "candidate",
            "tag"
        ]
    );
    // No step reads another workflow's runs, and none waits: `gh run` only
    // in a message that says how to follow the gate.
    for (_, s) in crate::harness::steps(&doc) {
        if let Some(body) = s["run"].as_str() {
            for line in body.lines().map(str::trim) {
                assert!(
                    !line.contains("gh run ") || line.starts_with("echo "),
                    "{line}"
                );
            }
            assert!(!body.contains("sleep"), "{body}");
        }
    }
    assert!(doc["jobs"]["cut"]["env"]["AUTOMATIC"].is_badvalue());
    assert_eq!(
        step(&doc, "cut", "cycle")["env"]["DISPATCHED_REF"].as_str(),
        Some("${{ github.ref_name }}")
    );
    // Each step goes on only on its predecessor's word: `open` prepares
    // with the binary it fetched, `candidate` reads the committed version.
    assert_eq!(
        step(&doc, "cut", "binary")["if"].as_str(),
        Some("inputs.action == 'open' && steps.cycle.outputs.proceed == 'true'")
    );
    assert_eq!(
        step(&doc, "cut", "open")["if"].as_str(),
        Some("steps.binary.outputs.bin != ''")
    );
    assert_eq!(
        step(&doc, "cut", "version")["if"].as_str(),
        Some("inputs.action == 'candidate' && steps.cycle.outputs.proceed == 'true'")
    );
    assert_eq!(
        step(&doc, "cut", "candidate")["if"].as_str(),
        Some("steps.open.outputs.proceed == 'true' || steps.version.outputs.proceed == 'true'")
    );
}

#[test]
fn a_dry_run_creates_and_pushes_nothing() {
    let doc = workflow(CUT);
    assert_eq!(
        step(&doc, "cut", "tag")["if"].as_str(),
        Some("!inputs.dry-run && steps.candidate.outputs.proceed == 'true'")
    );
    // The job outputs come from the tag step, skipped on a dry run: empty
    // then.
    for (k, v) in doc["jobs"]["cut"]["outputs"].as_hash().unwrap() {
        let v = v.as_str().unwrap();
        let k = k.as_str().unwrap();
        assert!(v.contains("steps.tag.") || k == "dry-run", "{k}: {v}");
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

#[test]
fn the_summary_names_the_configured_gate_unless_the_caller_names_one() {
    let doc = workflow(CUT);
    let inputs = &doc["on"]["workflow_call"]["inputs"];
    assert_eq!(inputs["gate-workflow"]["default"].as_str(), Some(""));
    assert_eq!(
        step(&doc, "cut", "Summary")["env"]["CONFIG_FILE"].as_str(),
        Some("${{ inputs.config-file }}")
    );
    let repo = Repo::new();
    repo.write(
        ".spl-asdlc/config.toml",
        "[release]\ngate_workflow = \"pre-release-qa.yml\"\n",
    );
    repo.commit("chore: configure the gate");
    let stubs = Scratch::new("cut-summary");
    let out = stubs.0.join("summary");
    let out = out.to_str().unwrap().to_string();
    let line = |gate: &str, config: &str| {
        let r = exec(
            "cut",
            "Summary",
            &repo,
            &stubs.0,
            &[
                ("GITHUB_STEP_SUMMARY", &out),
                ("PUSHED", "acme-v1.2.0-rc.1"),
                ("TAG", "acme-v1.2.0-rc.1"),
                ("BRANCH", "release/2026-10-01-10-00-utc"),
                ("VERSION", "1.2.0"),
                ("ROUND", "1"),
                ("CLASS_OUT", "routine"),
                ("CLASS_REASON", "no reviewed path"),
                ("SURFACES", "none"),
                ("BASE", "acme-v1.1.0"),
                ("PREVIOUS", ""),
                ("GATE_WORKFLOW", gate),
                ("CONFIG_FILE", config),
            ],
        );
        assert_eq!(r.code, 0, "{}", r.log);
        let text = std::fs::read_to_string(&out).unwrap();
        std::fs::remove_file(&out).unwrap();
        text.lines()
            .find(|l| l.starts_with("The gate: "))
            .unwrap_or_default()
            .to_string()
    };
    // Configured, explicit, default.
    let configured = line("", ".spl-asdlc/config.toml");
    assert!(
        configured.contains("--workflow=pre-release-qa.yml "),
        "{configured}"
    );
    let explicit = line("gate.yml", ".spl-asdlc/config.toml");
    assert!(explicit.contains("--workflow=gate.yml "), "{explicit}");
    let default = line("", "ci/absent.toml");
    assert!(
        default.contains("--workflow=release-gate.yml "),
        "{default}"
    );
}
