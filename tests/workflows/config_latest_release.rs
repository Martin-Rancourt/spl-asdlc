//! pr.yml's job `config-latest-release`: this repository's `spl-asdlc.toml`
//! is checked by the latest published release, the binary the host runs.
//! The step runs as shipped, with a `gh` stub that serves a fake release
//! whose binary records how it is called.

use std::path::Path;

use yaml_rust2::Yaml;

use crate::harness::{Ran, Scratch, calls, path_with, root, run, run_body, step, stub, workflow};

const JOB: &str = "config-latest-release";
const STEP: &str = "Check the file with the latest published release";
const REPO: &str = "example-org/spl-asdlc";
const TAG: &str = "v9.9.9";

fn pr() -> Yaml {
    workflow(".github/workflows/pr.yml")
}

fn env_of(key: &str) -> String {
    step(&pr(), JOB, STEP)["env"][key]
        .as_str()
        .unwrap_or_else(|| panic!("no env {key}"))
        .to_string()
}

/// The fake binary: `--version` prints `version`; anything else is
/// recorded in `bin.calls` and answered by `check` (bash).
struct Release {
    version: &'static str,
    check: &'static str,
    /// Spoil the archive's line in SHA256SUMS.
    bad_sum: bool,
}

fn sh(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new(args[0])
        .args(&args[1..])
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "{args:?}");
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// Publish `release` in `dir/release`, and a `gh` whose latest release is
/// `latest` (`None`: a 404).
fn serve(dir: &Path, release: &Release, latest: Option<&str>) {
    let target = env_of("TARGET");
    let name = format!("spl-asdlc-{TAG}-{target}");
    let pkg = dir.join("pkg").join(&name);
    std::fs::create_dir_all(&pkg).unwrap();
    stub(
        &pkg,
        "spl-asdlc",
        &format!(
            "if [ \"$1\" = --version ]; then echo 'spl-asdlc {}'; exit 0; fi\nprintf '%s\\n' \"$*\" >> '{}'\n{}",
            release.version,
            dir.join("bin.calls").display(),
            release.check
        ),
    );
    // The stub's own record of its calls stays out of the archive.
    let _ = std::fs::remove_file(pkg.join("spl-asdlc.calls"));
    let out = dir.join("release");
    std::fs::create_dir_all(&out).unwrap();
    let asset = format!("{name}.tar.gz");
    sh(
        &dir.join("pkg"),
        &["tar", "czf", out.join(&asset).to_str().unwrap(), &name],
    );
    let sum = sh(&out, &["sha256sum", &asset]);
    let hash = sum.split_whitespace().next().unwrap();
    let hash = if release.bad_sum {
        "0".repeat(hash.len())
    } else {
        hash.to_string()
    };
    std::fs::write(
        out.join("SHA256SUMS"),
        format!("{}  other.zip\n{hash}  {asset}\n", "f".repeat(64)),
    )
    .unwrap();
    let latest = match latest {
        Some(tag) => format!("echo {tag}"),
        None => "echo 'gh: Not Found (HTTP 404)' >&2; exit 1".to_string(),
    };
    stub(
        dir,
        "gh",
        &format!(
            r#"case "$*" in
"api repos/{REPO}/releases/latest --jq .tag_name") {latest} ;;
"release download {TAG} --repo {REPO} "*)
  while [ $# -gt 0 ]; do [ "$1" = --dir ] && d=$2; shift; done
  cp '{}'/* "$d"/ ;;
*) echo "unexpected gh $*" >&2; exit 97 ;;
esac"#,
            out.display()
        ),
    );
}

fn check(dir: &Path) -> Ran {
    let body = run_body(&step(&pr(), JOB, STEP));
    let tmp = dir.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    let path = path_with(dir);
    let (target, overlay) = (env_of("TARGET"), env_of("OVERLAY"));
    run(
        &body,
        &[
            ("PATH", &path),
            ("TMPDIR", tmp.to_str().unwrap()),
            ("REPO", REPO),
            ("TARGET", &target),
            ("OVERLAY", &overlay),
        ],
        &root(),
    )
}

const READS: Release = Release {
    version: "9.9.9",
    check: "echo 'spl-asdlc.toml: ok'",
    bad_sum: false,
};

#[test]
fn the_job_runs_on_every_pr_and_is_required() {
    let pr = pr();
    let job = &pr["jobs"][JOB];
    assert!(
        job["if"].is_badvalue() && job["needs"].is_badvalue(),
        "on every PR, whatever guards says"
    );
    let needs: Vec<&str> = pr["jobs"]["required-check"]["needs"]
        .as_vec()
        .unwrap()
        .iter()
        .filter_map(Yaml::as_str)
        .collect();
    assert!(needs.contains(&JOB), "a need of required-check");
    assert_eq!(
        job["permissions"]["contents"].as_str(),
        Some("read"),
        "it only reads"
    );
    assert_eq!(job["permissions"].as_hash().unwrap().len(), 1);
    // The archive is one binaries.yml publishes.
    let binaries = std::fs::read_to_string(root().join(".github/workflows/binaries.yml")).unwrap();
    assert!(
        binaries.contains(&format!("target: {}, archive: tar.gz", env_of("TARGET"))),
        "binaries.yml builds no tar.gz for {}",
        env_of("TARGET")
    );
    // The overlay is the one the crate's own test reads.
    let overlay = env_of("OVERLAY");
    assert!(root().join(&overlay).is_file(), "{overlay}");
    let own = std::fs::read_to_string(root().join("tests/own_config.rs")).unwrap();
    assert!(
        own.contains(&overlay),
        "tests/own_config.rs reads {overlay}"
    );
}

#[test]
fn a_release_that_reads_the_file_passes() {
    let dir = Scratch::new("cfg-release");
    serve(&dir.0, &READS, Some(TAG));
    let r = check(&dir.0);
    assert_eq!(r.code, 0, "{}", r.log);
    assert!(
        r.log
            .contains("spl-asdlc 9.9.9, the latest published release")
    );
    assert_eq!(
        calls(&dir.0, "bin"),
        [format!(
            "config check --file spl-asdlc.toml --instance {}",
            env_of("OVERLAY")
        )]
    );
}

#[test]
fn a_refusal_fails_and_names_the_key() {
    let dir = Scratch::new("cfg-release");
    serve(
        &dir.0,
        &Release {
            check: "echo 'unknown field `new_key`'; exit 1",
            ..READS
        },
        Some(TAG),
    );
    let r = check(&dir.0);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("unknown field `new_key`"), "{}", r.log);
    assert!(
        r.log
            .contains("::error file=spl-asdlc.toml::spl-asdlc v9.9.9, the release the host runs, refuses this file"),
        "{}",
        r.log
    );
}

#[test]
fn no_published_release_passes_and_says_so() {
    let dir = Scratch::new("cfg-release");
    serve(&dir.0, &READS, None);
    let r = check(&dir.0);
    assert_eq!(r.code, 0, "{}", r.log);
    assert!(
        r.log
            .contains(&format!("::notice::No published release of {REPO}")),
        "{}",
        r.log
    );
    assert_eq!(calls(&dir.0, "gh").len(), 1, "nothing downloaded");
    assert!(calls(&dir.0, "bin").is_empty());
}

#[test]
fn a_failed_lookup_is_not_taken_for_no_release() {
    let dir = Scratch::new("cfg-release");
    serve(&dir.0, &READS, Some(TAG));
    stub(
        &dir.0,
        "gh",
        "echo 'gh: Server Error (HTTP 502)' >&2; exit 1",
    );
    let r = check(&dir.0);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("HTTP 502"), "{}", r.log);
}

#[test]
fn an_archive_whose_checksum_differs_is_never_run() {
    let dir = Scratch::new("cfg-release");
    serve(
        &dir.0,
        &Release {
            bad_sum: true,
            ..READS
        },
        Some(TAG),
    );
    let r = check(&dir.0);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("its SHA-256 is not the one"), "{}", r.log);
    assert!(calls(&dir.0, "bin").is_empty());
}

#[test]
fn a_binary_of_another_version_is_refused() {
    let dir = Scratch::new("cfg-release");
    serve(
        &dir.0,
        &Release {
            version: "9.9.8",
            ..READS
        },
        Some(TAG),
    );
    let r = check(&dir.0);
    assert_eq!(r.code, 1, "{}", r.log);
    assert!(r.log.contains("not the version of v9.9.9"), "{}", r.log);
    assert!(calls(&dir.0, "bin").is_empty());
}
