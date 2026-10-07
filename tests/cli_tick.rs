//! `spl-asdlc tick`, `release` and `doctor` as a user runs them, without
//! credentials: they refuse to start and name what is missing (exit 2).

use std::process::Command;

fn spec_config() -> std::path::PathBuf {
    let spec = include_str!("../docs/spec/08-configuration.md");
    let start = spec.find("```toml\n").unwrap() + "```toml\n".len();
    let end = start + spec[start..].find("```").unwrap();
    let dir = std::env::temp_dir().join(format!("spl-asdlc-cli-tick-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("spl-asdlc.toml");
    std::fs::write(&path, &spec[start..end]).unwrap();
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

#[test]
fn without_credentials_the_bridge_refuses_to_start_and_names_them() {
    let file = spec_config();
    let file = file.to_str().unwrap();
    for args in [
        vec!["tick", "--file", file],
        vec!["tick", "--dry-run", "--file", file],
        vec!["release", "status", "--file", file],
        vec!["doctor", "--file", file],
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
    let path = dir.join("spl-asdlc.toml");
    std::fs::write(&path, "schema = 1\n").unwrap();
    let out = run(&["tick", "--file", path.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid configuration"));
}
