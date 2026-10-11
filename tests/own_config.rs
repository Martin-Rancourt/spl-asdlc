//! This repository's own `spl-asdlc.toml` is neutral (BR-ASDLC-001): it is
//! incomplete alone, and valid once an instance overlay gives it identities.
//! The overlay is `tests/fixtures/own-config.instance.toml`, the one pr.yml's
//! job `config-latest-release` also gives the latest published release.

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn check(instance: Option<&Path>) -> bool {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_spl-asdlc"));
    cmd.args(["config", "check", "--file"])
        .arg(root().join("spl-asdlc.toml"));
    if let Some(p) = instance {
        cmd.arg("--instance").arg(p);
    }
    cmd.output().unwrap().status.success()
}

#[test]
fn the_repositorys_own_configuration_is_neutral_and_completed_by_an_overlay() {
    assert!(!check(None), "no identity in the repository");
    assert!(check(Some(
        &root().join("tests/fixtures/own-config.instance.toml")
    )));
}
