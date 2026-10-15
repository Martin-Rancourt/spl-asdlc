//! The setup files a project copies (`docs/setup/templates/`) and this
//! repository's own (the release-please configuration and manifest): each is
//! read by the parser that reads it in production, never by a copy of its
//! rules.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;
use spl_asdlc::config::Config;
use spl_asdlc::method::{business_rules, contract};
use yaml_rust2::YamlLoader;

#[path = "support/template.rs"]
mod template;

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

const TOML: &str = template::PATH;
const RULE: &str = "docs/setup/templates/business-rule.md";
const TICKET: &str = "docs/setup/templates/ticket.md";

// ---- the templates ----

/// The template as written is the light configuration: no PR review, the
/// release cycle with no QA angle, no channel.
#[test]
fn the_configuration_template_is_valid_as_written_and_light() {
    let c = Config::parse(&read(TOML)).unwrap_or_else(|e| panic!("{TOML}: {e}"));
    assert_eq!(c.product.name, "acme");
    assert_eq!(c.product.repository, "example-org/acme");
    assert!(!c.review.angle_possible(), "no PR review (spec 04 §11)");
    assert!(c.review.coordinators.is_empty());
    assert!(
        c.release.angles.is_empty(),
        "the GO rests on the release CI"
    );
    assert!(c.release.coordinators.is_empty());
    assert!(c.notify.is_empty(), "no channel is configured by default");
    // No PR review is said, never assumed: without `angles = []` the file is
    // refused.
    let absent = read(TOML).replacen("\nangles = []", "\n# angles = []", 1);
    let errs = Config::parse(&absent).expect_err("absent angles are refused");
    assert!(errs.to_string().contains("review.angles"), "{errs}");
    // Every project has the release cycle: without its section the file is
    // refused.
    let start = read(TOML).find("\n[release]").unwrap();
    let end = start + read(TOML)[start..].find("\n# [bridge]").unwrap();
    let without = format!("{}{}", &read(TOML)[..start], &read(TOML)[end..]);
    let errs = Config::parse(&without).expect_err("no [release] is refused");
    assert!(
        errs.to_string()
            .contains("the [release] section is required"),
        "{errs}"
    );
}

fn parse(text: &str) -> Config {
    Config::parse(text).unwrap_or_else(|e| panic!("{e}\n{text}"))
}

/// Each layer the template's comments describe, added as they say, gives a
/// valid file with that layer on: alone, then on top of the others.
#[test]
fn each_layer_of_the_configuration_template_is_valid() {
    let light = template::light();

    let c = parse(&template::pr_review(&light));
    assert!(c.review.angle_possible());
    assert_eq!(c.review.coordinators, ["coordinator-login"]);
    let angles: Vec<(&str, usize)> = c
        .review
        .angles
        .iter()
        .map(|a| (a.key.as_str(), a.paths.len()))
        .collect();
    assert_eq!(
        angles,
        [("maintainability", 0), ("security", 2)],
        "one angle on every PR, one on its paths"
    );
    assert!(c.release.angles.is_empty(), "the layers are independent");

    let c = parse(&template::release_angles(&light));
    assert!(!c.review.angle_possible(), "the layers are independent");
    let keys: Vec<&str> = c.release.angles.iter().map(|a| a.key.as_str()).collect();
    assert_eq!(keys, ["security", "product"]);
    assert_eq!(c.release.coordinators, ["coordinator-login"]);

    let c = parse(&template::full());
    assert!(c.review.angle_possible());
    assert_eq!(c.review.coordinators, ["coordinator-login"]);
    assert_eq!(c.release.angles.len(), 2);
    assert_eq!(c.release.coordinators, ["coordinator-login"]);
}

#[test]
fn config_check_accepts_the_configuration_template() {
    let out = cli(&["config", "check", "--file", TOML]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
}

#[test]
fn the_configuration_template_names_the_example_callers() {
    let c = parse(&template::full());
    for wf in [
        &c.release.cut_workflow,
        &c.release.gate_workflow,
        &c.release.gate_rerun_workflow,
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
    for placeholder in ["owner-handle", "coordinator-login", "example-org/acme"] {
        assert!(text.contains(placeholder), "{placeholder}");
    }
}

#[test]
fn the_rule_template_is_a_valid_rule_file_under_its_ids_name() {
    let text = read(RULE);
    // Copied to `.spl-asdlc/business-rules/BR-ACME-001.md`, it is a rule.
    let path = format!("{}/BR-ACME-001.md", business_rules::REGISTRY_DIR);
    let rule =
        business_rules::parse_rule_file(&path, &text).unwrap_or_else(|e| panic!("{RULE}: {e}"));
    assert_eq!(rule.id, "BR-ACME-001");
    assert_eq!(rule.status, business_rules::RuleStatus::Proposed);
    assert!(business_rules::is_rule_file(&path));
    assert_eq!(
        business_rules::registry_ids(&[path]),
        BTreeSet::from(["BR-ACME-001".to_string()])
    );
    // Under its template name it is not one: the name is the id.
    assert!(business_rules::parse_rule_file(RULE, &text).is_err());
    // The spec documents the template.
    let spec = read("docs/spec/02-business-rules.md");
    assert!(
        spec.contains("templates/business-rule.md"),
        "spec 02 names the template"
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

#[test]
fn this_repositorys_release_please_files_make_the_existing_tags() {
    let config = json("release-please-config.json");
    let package = &config["packages"]["."];
    assert_eq!(package["release-type"], "rust");
    assert_eq!(package["component"], "spl-asdlc");
    // Tags are `v<X.Y.Z>` (the existing ones, and the tag binaries.yml
    // builds), so the component stays out of the tag.
    assert_eq!(package["include-component-in-tag"], false);
    let binaries = read(".github/workflows/binaries.yml");
    assert!(
        binaries.contains(r#"test "v$version" = "$TAG""#),
        "binaries.yml builds v<X.Y.Z> tags"
    );
    // Without the component in the tag release-please leaves it out of its
    // PR title too: the title pattern names the product. release-please runs
    // on main, where the scope is empty.
    let title = package["pull-request-title-pattern"].as_str().unwrap();
    assert_eq!(title, "chore: release spl-asdlc ${version}");

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
}

#[test]
fn the_default_changelog_sections_are_the_allowed_title_types() {
    // REQ-CI-016, for a project of the method: `release prepare` lists
    // every type a PR title may carry, so none ships in silence.
    let text = read(".github/workflows/asdlc-pr-title.yml");
    let doc = YamlLoader::load_from_str(&text).unwrap().remove(0);
    let allowed: Vec<String> = doc["on"]["workflow_call"]["inputs"]["allowed-types"]["default"]
        .as_str()
        .unwrap()
        .split(',')
        .map(|t| t.trim().to_string())
        .collect();
    let mut sections: Vec<String> = spl_asdlc::config::default_changelog_sections()
        .into_iter()
        .map(|s| s.kind)
        .collect();
    sections.sort();
    let mut allowed_sorted = allowed.clone();
    allowed_sorted.sort();
    assert_eq!(sections, allowed_sorted);
}
