//! BR-ASDLC-001: `scripts/check-no-instance-names.sh` allows this product's
//! repository slug, and only it, when its owner part is a forbidden word.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo(name: &str, files: &[(&str, &str)], origin: Option<&str>) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("spl-asdlc-rule-zero-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q"]);
    for (path, text) in files {
        std::fs::write(dir.join(path), text).unwrap();
        git(&dir, &["add", path]);
    }
    if let Some(url) = origin {
        git(&dir, &["remote", "add", "origin", url]);
    }
    dir
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(ok.success(), "git {args:?}");
}

/// The script's exit code in `dir`, with `Acme` forbidden.
fn scan(dir: &Path, github_repository: Option<&str>) -> i32 {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/check-no-instance-names.sh");
    let mut cmd = Command::new("bash");
    cmd.arg(script)
        .current_dir(dir)
        .env("SPL_ASDLC_FORBIDDEN_WORDS", "Acme")
        .env_remove("GITHUB_REPOSITORY");
    if let Some(slug) = github_repository {
        cmd.env("GITHUB_REPOSITORY", slug);
    }
    cmd.output().unwrap().status.code().unwrap()
}

const SLUG_LINE: &str = "repository = \"https://github.com/Acme-Org/spl-asdlc\"\n";

#[test]
fn the_products_slug_is_allowed_when_read_from_the_ci() {
    let dir = repo("ci", &[("Cargo.toml", SLUG_LINE)], None);
    assert_eq!(scan(&dir, Some("Acme-Org/spl-asdlc")), 0);
}

#[test]
fn the_products_slug_is_allowed_when_read_from_the_origin_remote() {
    let dir = repo(
        "origin",
        &[("Cargo.toml", SLUG_LINE)],
        Some("https://github.com/Acme-Org/spl-asdlc.git"),
    );
    assert_eq!(scan(&dir, None), 0);
}

#[test]
fn the_owner_name_on_its_own_is_still_a_hit() {
    let text = format!("{SLUG_LINE}maintained by Acme\n");
    let dir = repo("alone", &[("Cargo.toml", &text)], None);
    assert_eq!(scan(&dir, Some("Acme-Org/spl-asdlc")), 1);
}

/// The script run as a pull request from a fork sees it under GitHub Actions:
/// no word list (no secret), a step summary file. Exit code, stdout, summary.
fn scan_without_words(dir: &Path) -> (i32, String, String) {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/check-no-instance-names.sh");
    let summary = dir.join("step-summary.md");
    let out = Command::new("bash")
        .arg(script)
        .current_dir(dir)
        .env_remove("SPL_ASDLC_FORBIDDEN_WORDS")
        .env_remove("GITHUB_REPOSITORY")
        .env("GITHUB_ACTIONS", "true")
        .env("GITHUB_STEP_SUMMARY", &summary)
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        std::fs::read_to_string(&summary).unwrap_or_default(),
    )
}

#[test]
fn without_the_word_list_the_forms_pass_and_the_skip_is_annotated() {
    let dir = repo("fork", &[("Cargo.toml", "maintained by Acme\n")], None);
    let (code, stdout, summary) = scan_without_words(&dir);
    assert_eq!(code, 0, "no word list: a word is not looked for");
    assert!(
        stdout.contains("::warning title=BR-ASDLC-001::"),
        "{stdout}"
    );
    assert!(
        summary.contains("only identifier forms were checked"),
        "{summary}"
    );
}

#[test]
fn without_the_word_list_an_identifier_form_is_still_a_hit() {
    // Built at run time: the literal home path would be a hit on this file itself.
    let text = format!("path: /{}/someone/work\n", "home");
    let dir = repo("fork-form", &[("notes.md", &text)], None);
    let (code, stdout, _) = scan_without_words(&dir);
    assert_eq!(code, 1, "{stdout}");
}

#[test]
fn without_a_slug_or_with_another_products_nothing_is_allowed() {
    let dir = repo("none", &[("Cargo.toml", SLUG_LINE)], None);
    assert_eq!(scan(&dir, None), 1);
    let dir = repo("other", &[("Cargo.toml", SLUG_LINE)], None);
    assert_eq!(scan(&dir, Some("Acme-Org/other")), 1);
}
