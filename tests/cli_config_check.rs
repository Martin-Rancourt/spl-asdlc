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
    check_with(path, &[])
}

fn check_with(path: &std::path::Path, extra: &[&std::ffi::OsStr]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_spl-asdlc"))
        .args(["config", "check", "--file"])
        .arg(path)
        .args(extra)
        .output()
        .unwrap()
}

/// The `n`-th ```toml block of spec 08: 0 is the repository's file of the
/// example, 1 its instance overlay.
fn spec_block(n: usize) -> &'static str {
    let spec = include_str!("../docs/reference/08-configuration.md");
    let mut start = 0;
    for _ in 0..=n {
        start += spec[start..].find("```toml\n").unwrap() + "```toml\n".len();
    }
    let end = start + spec[start..].find("```").unwrap();
    &spec[start..end]
}

/// The example as one file: the overlay's keys written in the repository's
/// file, as a private project may keep them.
fn spec_example() -> String {
    spec_block(0)
        .replacen(
            "name = \"acme\"",
            "name = \"acme\"\nrepository = \"example-org/acme\"\n\n[owner]\ntracker_user = \"owner-handle\"",
            1,
        )
        .replacen(
            "kind = \"linear\"",
            "kind = \"linear\"\nteam = \"ABC\"\nservice_identity = \"spl-asdlc\"",
            1,
        )
        .replacen(
            "[release]\n",
            "[release]\ncoordinators = [\"coordinator-login\"]\n",
            1,
        )
        .replacen(
            "[review]\n",
            "[review]\ncoordinators = [\"coordinator-login\"]\n",
            1,
        )
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
fn a_file_without_a_release_section_is_refused_every_project_has_the_cycle() {
    let text = spec_example();
    let start = text.find("[release]\n").unwrap();
    let end = text.find("[review]\n").unwrap();
    let path = write_temp(
        "no-release.toml",
        &format!("{}{}", &text[..start], &text[end..]),
    );
    let out = check(&path);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("release: the [release] section is required"),
        "{stderr}"
    );
}

#[test]
fn the_default_file_is_the_one_in_the_methods_folder() {
    let dir = std::env::temp_dir().join(format!("spl-asdlc-cli-default-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".spl-asdlc")).unwrap();
    std::fs::write(dir.join(".spl-asdlc/config.toml"), spec_block(0)).unwrap();
    std::fs::write(dir.join(".spl-asdlc/config.instance.toml"), spec_block(1)).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_spl-asdlc"))
        .args(["config", "check"])
        .current_dir(&dir)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    // The overlay's path is printed with the platform's separator.
    let stdout = String::from_utf8_lossy(&out.stdout).replace('\\', "/");
    assert!(
        stdout.starts_with(".spl-asdlc/config.toml with .spl-asdlc/config.instance.toml: valid"),
        "{stdout}"
    );
    let _ = std::fs::remove_dir_all(&dir);
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
    let out = check(std::path::Path::new("does-not-exist/config.toml"));
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

/// The neutral file of spec 08 and its overlay beside it, in a directory of
/// its own.
fn neutral_pair(dir: &str, with_overlay: bool) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("spl-asdlc-cli-{dir}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, spec_block(0)).unwrap();
    if with_overlay {
        std::fs::write(dir.join("config.instance.toml"), spec_block(1)).unwrap();
    }
    path
}

#[test]
fn the_overlay_beside_the_file_is_merged_and_named() {
    let path = neutral_pair("sibling", true);
    let out = check(&path);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("config.instance.toml: valid — product acme (example-org/acme)"),
        "{stdout}"
    );
}

#[test]
fn without_an_overlay_a_neutral_file_is_incomplete() {
    let path = neutral_pair("no-sibling", false);
    let out = check(&path);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("product.repository: is required"),
        "{stderr}"
    );
}

#[test]
fn a_named_overlay_is_read_and_one_that_does_not_exist_exits_two() {
    let path = neutral_pair("named", false);
    let overlay = write_temp("named-overlay.toml", spec_block(1));
    let out = check_with(&path, &["--instance".as_ref(), overlay.as_os_str()]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let missing = path.with_file_name("absent-overlay.toml");
    let out = check_with(&path, &["--instance".as_ref(), missing.as_os_str()]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("the instance overlay named does not exist")
    );
}

#[test]
fn a_key_in_both_files_exits_one_and_names_both() {
    let path = neutral_pair("both", true);
    std::fs::write(
        &path,
        spec_block(0).replacen(
            "name = \"acme\"",
            "name = \"acme\"\nrepository = \"example-org/acme\"",
            1,
        ),
    )
    .unwrap();
    let out = check(&path);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("product.repository: is set in both"),
        "{stderr}"
    );
}
