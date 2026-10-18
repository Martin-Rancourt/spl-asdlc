//! `spl-asdlc tick`, `release` and `doctor` as a user runs them, without
//! credentials: they refuse to start and name what is missing (exit 2).

use std::process::Command;

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

/// The example of spec 08 on disk, in `dir`: the repository's file and,
/// beside it, its instance overlay.
fn spec_config(dir: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("spl-asdlc-cli-{dir}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, spec_block(0)).unwrap();
    std::fs::write(dir.join("config.instance.toml"), spec_block(1)).unwrap();
    path
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_spl-asdlc"))
        .args(args)
        .env_remove("SPL_ASDLC_APP_ID")
        .env_remove("SPL_ASDLC_APP_PRIVATE_KEY")
        .env_remove("SPL_ASDLC_APP_PRIVATE_KEY_PATH")
        .env_remove("SPL_ASDLC_GITHUB_TOKEN")
        .env_remove("SPL_ASDLC_TRACKER_TOKEN")
        .output()
        .unwrap()
}

/// The configuration is read first, merged with the overlay beside it or
/// the one named: only then is the missing credential found.
#[test]
fn without_credentials_the_bridge_refuses_to_start_and_names_them() {
    let file = spec_config("tick");
    let overlay = file.with_file_name("config.instance.toml");
    let file = file.to_str().unwrap();
    let overlay = overlay.to_str().unwrap();
    for args in [
        vec!["tick", "--file", file],
        vec!["tick", "--dry-run", "--file", file],
        vec!["tick", "--dry-run", "--file", file, "--instance", overlay],
        vec!["release", "status", "--file", file],
        vec!["release", "status", "--file", file, "--instance", overlay],
        vec!["doctor", "--file", file],
        vec!["doctor", "--file", file, "--instance", overlay],
    ] {
        let out = run(&args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let text = String::from_utf8_lossy(&out.stderr);
        assert!(text.contains("SPL_ASDLC_APP"), "{args:?}: {text}");
    }
}

#[test]
fn an_invalid_configuration_stops_the_tick() {
    let dir = std::env::temp_dir().join(format!("spl-asdlc-cli-bad-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "schema = 1\n").unwrap();
    let out = run(&["tick", "--file", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid configuration"));
}

#[test]
fn a_neutral_file_without_its_overlay_stops_the_dry_run_and_names_the_instance_keys() {
    let dir = std::env::temp_dir().join(format!("spl-asdlc-cli-neutral-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, spec_block(0)).unwrap();
    let out = run(&["tick", "--dry-run", "--file", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(text.contains("product.repository: is required"), "{text}");
    assert!(text.contains("instance overlay"), "{text}");
}

#[test]
fn an_instance_overlay_is_named_for_a_single_file_only() {
    let one = spec_config("tick-one");
    let two = spec_config("tick-two");
    let overlay = one.with_file_name("config.instance.toml");
    let out = run(&[
        "tick",
        "--file",
        one.to_str().unwrap(),
        "--file",
        two.to_str().unwrap(),
        "--instance",
        overlay.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(
        text.contains("--instance names the overlay of one configuration"),
        "{text}"
    );
    assert!(
        !text.contains("SPL_ASDLC_APP"),
        "refused before anything is read: {text}"
    );
}

#[test]
fn a_named_overlay_that_does_not_exist_stops_the_tick() {
    let file = spec_config("tick-named");
    let missing = file.with_file_name("not-there.toml");
    let out = run(&[
        "tick",
        "--dry-run",
        "--file",
        file.to_str().unwrap(),
        "--instance",
        missing.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(2));
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(
        text.contains("the instance overlay named does not exist"),
        "{text}"
    );
}
