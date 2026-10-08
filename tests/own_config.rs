//! This repository's own `spl-asdlc.toml` is neutral (BR-ASDLC-001): it is
//! incomplete alone, and valid once an instance overlay gives it identities.

use std::path::Path;
use std::process::Command;

fn check(instance: Option<&Path>) -> bool {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("spl-asdlc.toml");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_spl-asdlc"));
    cmd.args(["config", "check", "--file"]).arg(file);
    if let Some(p) = instance {
        cmd.arg("--instance").arg(p);
    }
    cmd.output().unwrap().status.success()
}

#[test]
fn the_repositorys_own_configuration_is_neutral_and_completed_by_an_overlay() {
    assert!(!check(None), "no identity in the repository");
    let dir = std::env::temp_dir().join(format!("spl-asdlc-own-config-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let overlay = dir.join("spl-asdlc.instance.toml");
    std::fs::write(
        &overlay,
        r#"schema = 1

[product]
repository = "example-org/spl-asdlc"

[owner]
tracker_user = "owner-handle"

[tracker]
team = "ABC"
service_identity = "spl-asdlc"

[release]
release_please_authors = ["app/release-app"]
coordinators = ["coordinator-login"]

[review]
coordinators = ["coordinator-login"]
"#,
    )
    .unwrap();
    assert!(check(Some(&overlay)));
    let _ = std::fs::remove_dir_all(&dir);
}
