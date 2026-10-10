//! The setup files a project copies (`docs/setup/templates/`) and this
//! repository's own (`.github/review-paths.json`, the release-please
//! configuration and manifest): each is read by the parser that reads it in
//! production, never by a copy of its rules.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use spl_asdlc::config::Config;
use spl_asdlc::method::{business_rules, contract, glob, review};
use yaml_rust2::YamlLoader;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn json(rel: &str) -> Value {
    serde_json::from_str(&read(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_spl-asdlc"))
        .args(args)
        .current_dir(root())
        .output()
        .unwrap()
}

/// The first fenced block of `lang` in a document.
fn fenced(text: &str, lang: &str) -> String {
    let open = format!("```{lang}\n");
    let start = text.find(&open).expect("a fenced block") + open.len();
    let end = start + text[start..].find("```").expect("the block is closed");
    text[start..end].to_string()
}

const TOML: &str = "docs/setup/templates/spl-asdlc.toml";
const PATHS: &str = "docs/setup/templates/review-paths.json";
const REGISTRY: &str = "docs/setup/templates/business-rules.md";
const TICKET: &str = "docs/setup/templates/ticket.md";

// ---- the templates ----

#[test]
fn the_configuration_template_is_valid_as_written() {
    let c = Config::parse(&read(TOML)).unwrap_or_else(|e| panic!("{TOML}: {e}"));
    assert_eq!(c.product.name, "acme");
    assert_eq!(c.product.repository, "example-org/acme");
    // Declared, so the review-paths template is part of the setup, and an
    // absent file holds every PR (spec 08 rule 5).
    assert_eq!(
        c.review.sensitive_paths.as_deref(),
        Some(".github/review-paths.json")
    );
    assert!(c.notify.is_empty(), "no channel is configured by default");
}

#[test]
fn config_check_accepts_the_configuration_template() {
    let out = cli(&["config", "check", "--file", TOML]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
}

#[test]
fn the_configuration_template_names_the_example_callers() {
    let c = Config::parse(&read(TOML)).unwrap();
    for wf in [
        &c.release.cut_workflow,
        &c.release.gate_workflow,
        &c.release.gate_rerun_workflow,
        &c.release.release_please_workflow,
    ] {
        assert!(
            root().join("docs/setup/examples").join(wf).is_file(),
            "no example caller {wf}"
        );
    }
}

#[test]
fn the_configuration_template_names_its_identities_by_placeholder() {
    let text = read(TOML);
    for placeholder in [
        "owner-handle",
        "coordinator-login",
        "app/example-app",
        "example-org/acme",
    ] {
        assert!(text.contains(placeholder), "{placeholder}");
    }
}

#[test]
fn the_sensitive_path_template_parses_and_lists_itself() {
    let paths = review::parse_sensitive_paths(&read(PATHS)).unwrap();
    assert!(
        !glob::hits(&[".github/review-paths.json".to_string()], &paths).is_empty(),
        "the list must name itself: {paths:?}"
    );
    let v = json(PATHS);
    assert!(
        v["exceptions"].is_array(),
        "the exceptions list is documented"
    );
}

#[test]
fn the_registry_template_is_an_empty_registry_with_the_five_columns() {
    let text = read(REGISTRY);
    assert!(business_rules::registry_ids(&text).is_empty(), "{text}");
    assert!(business_rules::duplicate_registry_ids(&text).is_empty());
    assert!(
        text.contains("| ID | Rule | Evidence | Test | Status |"),
        "the header row of spec 02"
    );
    // A first row added under the header is read as a registered rule.
    let one = format!("{text}| BR-ACME-001 | **A rule.** | Why. | `a_test` | Proposed |\n");
    assert_eq!(
        business_rules::registry_ids(&one),
        BTreeSet::from(["BR-ACME-001".to_string()])
    );
}

#[test]
fn the_ticket_template_is_the_template_of_spec_01() {
    let spec = read("docs/spec/01-ticket-contract.md");
    assert_eq!(read(TICKET), fenced(&spec, "markdown"));
}

#[test]
fn the_ticket_template_passes_the_refinement_gate_as_written() {
    let text = read(TICKET);
    assert!(
        contract::missing_from_contract(&text).is_empty(),
        "{:?}",
        contract::missing_from_contract(&text)
    );
    let out = cli(&["ticket", "check", "--description", TICKET]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
}

#[test]
fn an_unedited_business_rules_placeholder_is_read_as_a_rule_never_as_none() {
    // The gate reads structure, not prose: the skeleton passes it. Its
    // Business rules placeholder is not a declaration of absence, so a
    // ticket whose author forgot to replace it is held for a rule the
    // registry lacks, never waved through (spec 02 invariant 7).
    let text = read(TICKET);
    assert!(business_rules::new_rule_statement(&text).is_some());
}

// ---- this repository's own files ----

const OWN_PATHS: &str = ".github/review-paths.json";

#[test]
fn this_repositorys_sensitive_paths_parse_and_cover_what_releases_and_authenticates() {
    let paths = review::parse_sensitive_paths(&read(OWN_PATHS)).unwrap();
    for sensitive in [
        ".github/review-paths.json",
        ".github/workflows/asdlc-release-cut.yml",
        ".github/workflows/pr.yml",
        ".github/workflows/binaries.yml",
        "actions/required-check/action.yml",
        "scripts/check-no-instance-names.sh",
        "src/github/auth.rs",
        "Cargo.toml",
        "Cargo.lock",
    ] {
        assert!(
            !glob::hits(&[sensitive.to_string()], &paths).is_empty(),
            "{sensitive} is not covered by {paths:?}"
        );
    }
    for ordinary in [
        "src/tick/mod.rs",
        "docs/spec/08-configuration.md",
        "README.md",
    ] {
        assert!(
            glob::hits(&[ordinary.to_string()], &paths).is_empty(),
            "{ordinary} is not meant to be sensitive"
        );
    }
    // Every glob names something that exists: a typo would protect nothing.
    let tracked = tracked_files();
    for p in &paths {
        assert!(
            !glob::hits(&tracked, std::slice::from_ref(p)).is_empty(),
            "{p} matches no file of the repository"
        );
    }
}

fn tracked_files() -> Vec<String> {
    let mut out = Vec::new();
    collect(&root(), &root(), &mut out);
    out
}

fn collect(base: &Path, dir: &Path, out: &mut Vec<String>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if p.is_dir() {
            if name == "target" || name == ".git" || name == ".claude" {
                continue;
            }
            collect(base, &p, out);
        } else {
            out.push(p.strip_prefix(base).unwrap().display().to_string());
        }
    }
}

#[test]
fn this_repositorys_release_please_files_make_the_existing_tags() {
    let config = json("release-please-config.json");
    let package = &config["packages"]["."];
    assert_eq!(package["release-type"], "rust");
    assert_eq!(package["component"], "spl-asdlc");
    // Tags are `v<X.Y.Z>` (the existing v0.1.0, the release tag of this
    // repository's release callers), so the component stays out of the tag.
    assert_eq!(package["include-component-in-tag"], false);
    let publish = read(".github/workflows/release-publish.yml");
    assert!(
        publish.contains("release-tag-pattern: v{version}\n"),
        "release-publish.yml publishes v<X.Y.Z> tags"
    );
    // Without the component in the tag release-please leaves it out of its
    // PR title too, and release-cut finds the staged version after the
    // component's name: the title pattern names it.
    let title = package["pull-request-title-pattern"].as_str().unwrap();
    assert!(title.contains("${scope}"), "{title}");
    assert!(title.ends_with("release spl-asdlc ${version}"), "{title}");

    let manifest = json(".release-please-manifest.json");
    let cargo: toml::Value = toml::from_str(&read("Cargo.toml")).unwrap();
    let version = cargo["package"]["version"].as_str().unwrap();
    assert_eq!(manifest, serde_json::json!({ ".": version }));
}

#[test]
fn this_repositorys_changelog_sections_are_the_allowed_title_types() {
    // REQ-CI-016: a type the changelog has no visible section for would ship
    // in silence.
    let text = read(".github/workflows/asdlc-pr-title.yml");
    let doc = YamlLoader::load_from_str(&text).unwrap().remove(0);
    let allowed: BTreeSet<String> =
        doc["on"]["workflow_call"]["inputs"]["allowed-types"]["default"]
            .as_str()
            .unwrap()
            .split(',')
            .map(|t| t.trim().to_string())
            .collect();
    let config = json("release-please-config.json");
    let sections = config["packages"]["."]["changelog-sections"]
        .as_array()
        .unwrap();
    let written: BTreeSet<String> = sections
        .iter()
        .filter(|s| !s["hidden"].as_bool().unwrap_or(false))
        .map(|s| s["type"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(written, allowed);
    assert_eq!(sections.len(), allowed.len(), "no type twice, none hidden");
    // The bridge reads the same set from the same file.
    assert_eq!(
        spl_asdlc::method::release::notes::written_types(Some(&read("release-please-config.json"))),
        Some(allowed)
    );
}
