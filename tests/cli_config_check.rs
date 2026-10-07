//! `spl-asdlc config check` as a user runs it: exit codes and messages.

use std::path::PathBuf;
use std::process::Command;

fn write_temp(name: &str, text: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("spl-asdlc-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

fn check(path: &std::path::Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_spl-asdlc"))
        .args(["config", "check", "--file"])
        .arg(path)
        .output()
        .unwrap()
}

fn spec_example() -> String {
    let spec = include_str!("../docs/spec/08-configuration.md");
    let start = spec.find("```toml\n").unwrap() + "```toml\n".len();
    let end = start + spec[start..].find("```").unwrap();
    spec[start..end].to_string()
}

#[test]
fn a_valid_file_exits_zero_and_says_what_it_read() {
    let path = write_temp("valid.toml", &spec_example());
    let out = check(&path);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("valid — product acme (example-org/acme)"),
        "{stdout}"
    );
}

#[test]
fn an_invalid_file_exits_one_and_lists_every_problem() {
    let text = spec_example()
        .replace("kind = \"linear\"", "kind = \"jira\"")
        .replace("max_rc_passes = 2", "max_rc_passes = 0");
    let path = write_temp("invalid.toml", &text);
    let out = check(&path);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("will not serve this repository"),
        "{stderr}"
    );
    assert!(stderr.contains("tracker.kind"), "{stderr}");
    assert!(stderr.contains("release.max_rc_passes"), "{stderr}");
}

#[test]
fn a_missing_file_exits_two() {
    let out = check(std::path::Path::new("does-not-exist/spl-asdlc.toml"));
    assert_eq!(out.status.code(), Some(2), "{out:?}");
}

#[test]
fn an_unknown_verb_exits_two() {
    let out = Command::new(env!("CARGO_BIN_EXE_spl-asdlc"))
        .arg("frobnicate")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}
