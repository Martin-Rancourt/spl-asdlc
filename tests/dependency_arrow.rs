//! The dependency arrow points toward this crate, never outward
//! (BR-ASDLC-010, `docs/livela-contract.md` §3): the public crate depends on
//! no consumer. Each runtime or tracker (the Livela runner's `BridgeJob`, for
//! one) writes its own wrapper on its side, depending on `spl-asdlc` by
//! version. A Git dependency would also name an account in `Cargo.lock`
//! (BR-ASDLC-001).

const MANIFEST: &str = include_str!("../Cargo.toml");
const LOCKFILE: &str = include_str!("../Cargo.lock");

/// A consumer of this crate: a name of the Livela family (`livela-core`,
/// `livela-runner`, `livela_core`, …), however it is spelled.
fn is_consumer(name: &str) -> bool {
    name.to_ascii_lowercase()
        .replace('_', "-")
        .starts_with("livela")
}

/// Every dependency table of the manifest, with the path that names it:
/// `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]`, their
/// `[target.<cfg>.…]` forms, and the `[patch.<source>]` and `[replace]`
/// tables that could swap a dependency for another.
fn dependency_tables(manifest: &toml::Table) -> Vec<(String, &toml::Table)> {
    const KINDS: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];
    let mut tables = Vec::new();
    for kind in KINDS {
        if let Some(t) = manifest.get(kind).and_then(|v| v.as_table()) {
            tables.push((kind.to_string(), t));
        }
    }
    if let Some(targets) = manifest.get("target").and_then(|v| v.as_table()) {
        for (cfg, target) in targets {
            for kind in KINDS {
                if let Some(t) = target.get(kind).and_then(|v| v.as_table()) {
                    tables.push((format!("target.{cfg}.{kind}"), t));
                }
            }
        }
    }
    if let Some(patches) = manifest.get("patch").and_then(|v| v.as_table()) {
        for (source, t) in patches {
            if let Some(t) = t.as_table() {
                tables.push((format!("patch.{source}"), t));
            }
        }
    }
    if let Some(t) = manifest.get("replace").and_then(|v| v.as_table()) {
        tables.push(("replace".to_string(), t));
    }
    tables
}

#[test]
fn the_manifest_depends_on_no_consumer_and_on_no_git_source() {
    let manifest: toml::Table = toml::from_str(MANIFEST).expect("Cargo.toml parses");
    let tables = dependency_tables(&manifest);
    assert!(
        tables.iter().any(|(path, _)| path == "dependencies"),
        "the manifest's [dependencies] were not found: the check would see nothing"
    );
    for (path, table) in tables {
        for (name, spec) in table {
            assert!(!is_consumer(name), "[{path}] depends on `{name}`");
            let Some(spec) = spec.as_table() else {
                continue; // `name = "1.2"`: a version from the registry.
            };
            if let Some(package) = spec.get("package").and_then(|p| p.as_str()) {
                assert!(
                    !is_consumer(package),
                    "[{path}] `{name}` is the package `{package}`"
                );
            }
            assert!(
                !spec.contains_key("git"),
                "[{path}] `{name}` is a Git dependency"
            );
        }
    }
}

#[test]
fn the_lockfile_holds_no_consumer_and_no_git_source() {
    let lock: toml::Table = toml::from_str(LOCKFILE).expect("Cargo.lock parses");
    let packages = lock
        .get("package")
        .and_then(|p| p.as_array())
        .expect("Cargo.lock lists its packages");
    assert!(!packages.is_empty());
    for package in packages {
        let name = package.get("name").and_then(|n| n.as_str()).unwrap_or("");
        assert!(!is_consumer(name), "Cargo.lock holds `{name}`");
        if let Some(source) = package.get("source").and_then(|s| s.as_str()) {
            assert!(
                !source.starts_with("git+"),
                "Cargo.lock takes `{name}` from a Git source"
            );
        }
    }
}

#[test]
fn a_consumer_is_recognised_however_it_is_spelled() {
    for name in ["livela-core", "livela_core", "Livela-Runner", "livela"] {
        assert!(is_consumer(name), "{name}");
    }
    for name in ["spl-asdlc", "serde", "toml"] {
        assert!(!is_consumer(name), "{name}");
    }
}
