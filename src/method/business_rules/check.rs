//! `spl-asdlc rules check`: the registry's errors, as the CI judges them
//! (ASDLC-YX9K). Each file of the registry is read and checked
//! ([`parse_registry`]), and each `Enforced` rule's Test section has the
//! tests it cites looked for ([`crate::method::rule_tests`]).
//!
//! A PR is judged on the errors it brings: [`new_errors`] keeps those its
//! head has and its base has not, so an error already on the base never
//! blocks it. A release candidate is judged on every error: the registry of
//! a release is clean as a whole.

use super::{RuleStatus, parse_registry, parse_rule_file, test_section};
use crate::method::rule_tests::{Judge, Tally};

/// The registry as checked at one commit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// The valid rule files.
    pub rules: usize,
    /// Of them, the `Enforced` ones, whose cited tests are looked for.
    pub enforced: usize,
    /// The cited names found, and the files they were found in.
    pub found: usize,
    pub files: usize,
    /// The cited names whose file cannot be located: counted, never an error.
    pub not_located: usize,
    /// Each error, once, in order: `<path>: <why>` for a file whose format
    /// is wrong, `<ID>: <what is absent>` for a cited test.
    pub errors: Vec<String>,
}

/// Check the registry's `files` (`(path, text)`, as [`parse_registry`]
/// reads them), each cited file read through `read` (`None` when absent).
/// `at` names the commit in the errors: pass the same label for the two
/// sides of a comparison, so an error reads the same on both. An error of
/// `read` is a file that could not be read: nothing is concluded.
pub fn check(
    files: &[(String, String)],
    at: &str,
    read: impl FnMut(&str) -> Result<Option<String>, String>,
) -> Result<Report, String> {
    let registry = parse_registry(files);
    let mut errors: Vec<String> = registry
        .invalid
        .iter()
        .map(|(path, why)| format!("{path}: {why}"))
        .collect();
    let mut judge = Judge::new(read);
    let mut tally = Tally::default();
    let mut enforced = 0;
    for (path, text) in files {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let Ok(rule) = parse_rule_file(name, text) else {
            continue;
        };
        if rule.status != RuleStatus::Enforced {
            continue;
        }
        enforced += 1;
        judge.rule(&rule.id, &test_section(text), at, &mut tally)?;
    }
    for p in tally.problems {
        if !errors.contains(&p) {
            errors.push(p);
        }
    }
    Ok(Report {
        rules: registry.rules.len(),
        enforced,
        found: tally.found,
        files: tally.files.len(),
        not_located: tally.not_located,
        errors,
    })
}

/// The errors of `head` that `base` does not have, in `head`'s order: what
/// a change brings to the registry.
pub fn new_errors(head: &Report, base: &Report) -> Vec<String> {
    head.errors
        .iter()
        .filter(|e| !base.errors.contains(e))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::method::business_rules::REGISTRY_DIR;

    /// A commit: its files, and those that cannot be read.
    #[derive(Default)]
    struct Tree {
        files: HashMap<String, Result<String, String>>,
    }

    impl Tree {
        fn put(&mut self, path: &str, text: &str) {
            self.files.insert(path.into(), Ok(text.into()));
        }
        fn rule(&mut self, (path, text): (String, String)) {
            self.files.insert(path, Ok(text));
        }
        fn registry(&self) -> Vec<(String, String)> {
            let mut out: Vec<_> = self
                .files
                .iter()
                .filter(|(p, _)| p.starts_with(&format!("{REGISTRY_DIR}/")))
                .map(|(p, t)| (p.clone(), t.clone().unwrap()))
                .collect();
            out.sort();
            out
        }
        fn check(&self) -> Result<Report, String> {
            check(&self.registry(), "main", |path: &str| {
                self.files.get(path).cloned().transpose()
            })
        }
    }

    /// A rule file, its status and Test section given.
    fn rule_file(id: &str, status: &str, test: &str) -> (String, String) {
        (
            format!("{REGISTRY_DIR}/{id}.md"),
            format!(
                "# {id} — A rule.\n\nStatus: {status}\n\n## Rule\nIt holds.\n\n## Evidence\nWhy.\n\n## Test\n\n{test}\n"
            ),
        )
    }

    #[test]
    fn the_check_reads_every_rule_file_and_names_each_invalid_one() {
        let mut t = Tree::default();
        assert_eq!(t.check().unwrap(), Report::default());
        t.rule(rule_file("BR-ACME-001", "Proposed", "None yet."));
        t.put(
            &format!("{REGISTRY_DIR}/README.md"),
            "# Index\n\nNot a rule.\n",
        );
        let ok = t.check().unwrap();
        assert_eq!((ok.rules, ok.errors.len()), (1, 0), "{ok:?}");
        let (_, wrong_title) = rule_file("BR-ACME-003", "Proposed", "None yet.");
        t.put(&format!("{REGISTRY_DIR}/BR-ACME-002.md"), &wrong_title);
        t.put(&format!("{REGISTRY_DIR}/notes.md"), "Anything.");
        let (_, no_evidence) = rule_file("BR-ACME-004", "Proposed", "None yet.");
        t.put(
            &format!("{REGISTRY_DIR}/BR-ACME-004.md"),
            &no_evidence.replace("## Evidence\nWhy.\n\n", ""),
        );
        let (_, no_status) = rule_file("BR-ACME-005", "Proposed", "None yet.");
        t.put(
            &format!("{REGISTRY_DIR}/BR-ACME-005.md"),
            &no_status.replace("Status: Proposed\n\n", ""),
        );
        let bad = t.check().unwrap();
        assert_eq!(bad.rules, 1);
        assert_eq!(
            bad.errors[..3],
            [
                format!(
                    "{REGISTRY_DIR}/BR-ACME-002.md: the title names `BR-ACME-003`, the file name `BR-ACME-002`"
                ),
                format!("{REGISTRY_DIR}/BR-ACME-004.md: no `## Evidence` section"),
                format!(
                    "{REGISTRY_DIR}/BR-ACME-005.md: no `Status: Proposed` or `Status: Enforced` line"
                ),
            ]
        );
        assert!(
            bad.errors[3].starts_with(&format!("{REGISTRY_DIR}/notes.md: the file name")),
            "{bad:?}"
        );
    }

    #[test]
    fn the_tests_a_rule_cites_are_found_in_the_files_it_names() {
        let mut t = Tree::default();
        // A JS project: the file, then the nested names in a list.
        t.rule(rule_file(
            "BR-ACME-001",
            "Enforced",
            "`apps/webapp/src/app/api/process/__tests__/pdfImportWorker.test.ts`, in the\n\
             `webapp#test` task of the `build-test` job:\n\n\
             - `runPdfImportJob > duplicate delivery while a first attempt is still running > refuses the second delivery without calling the provider`",
        ));
        t.put(
            "apps/webapp/src/app/api/process/__tests__/pdfImportWorker.test.ts",
            "describe('runPdfImportJob', () => {\n  it('refuses the second delivery without calling the provider', async () => {});\n});\n",
        );
        // Rust: a module path, a wildcard, a file then its names, in prose.
        t.rule(rule_file(
            "BR-ACME-002",
            "Enforced",
            "The flow holds it: `tick::release_tests::a_case`, `method::ticket_state::tests::*`;\n\
             `tests/workflows/own.rs`: `name_a`, `name_b`, run by the `ci` job.",
        ));
        t.put("src/tick/release_tests.rs", "#[test]\nfn a_case() {}\n");
        t.put("src/method/ticket_state.rs", "mod tests {}\n");
        t.put("tests/workflows/own.rs", "fn name_a() {}\nfn name_b() {}\n");
        // Enforced by review, its names nowhere to be located: counted.
        t.rule(rule_file(
            "BR-ACME-003",
            "Enforced",
            "Enforced by review, not by code. The bridge's side is `tick::tests::a_case_elsewhere`, and `a_bare_name`.",
        ));
        // A Proposed rule is not judged.
        t.rule(rule_file("BR-ACME-004", "Proposed", "`tests/not_yet.rs`"));
        let ok = t.check().unwrap();
        assert!(ok.errors.is_empty(), "{ok:?}");
        assert_eq!(
            (ok.rules, ok.enforced, ok.found, ok.files, ok.not_located),
            (4, 3, 5, 4, 2)
        );
    }

    #[test]
    fn a_cited_test_that_is_absent_is_an_error_and_an_unreadable_file_concludes_nothing() {
        let mut t = Tree::default();
        t.rule(rule_file(
            "BR-ACME-001",
            "Enforced",
            "`tests/workflows/own.rs`: `name_a`, `name_c`; `tick::release_tests::gone_case`",
        ));
        t.rule(rule_file(
            "BR-ACME-002",
            "Enforced",
            "`gone_case_too` (`tests/gone.rs`)",
        ));
        t.put("tests/workflows/own.rs", "fn name_a() {}\n");
        t.put("src/tick/release_tests.rs", "fn a_case() {}\n");
        assert_eq!(
            t.check().unwrap().errors,
            [
                "BR-ACME-001: `name_c` is not in `tests/workflows/own.rs`",
                "BR-ACME-001: `tick::release_tests::gone_case` is not in `src/tick/release_tests.rs`",
                "BR-ACME-002: `tests/gone.rs` is absent at main",
            ]
        );
        t.files.insert(
            "tests/gone.rs".into(),
            Err("tests/gone.rs: HTTP 502".into()),
        );
        assert_eq!(t.check().unwrap_err(), "tests/gone.rs: HTTP 502");
    }

    #[test]
    fn a_change_is_judged_only_on_the_errors_it_brings() {
        let mut base = Tree::default();
        let (path, text) = rule_file("BR-ACME-001", "Enforced", "`tests/gone.rs`");
        base.put(&path, &text.replace("## Evidence\nWhy.\n\n", ""));
        base.rule(rule_file("BR-ACME-002", "Enforced", "`tests/old.rs`"));
        let before = base.check().unwrap();
        assert_eq!(before.errors.len(), 2, "{before:?}");
        let mut head = Tree {
            files: base.files.clone(),
        };
        // The head keeps both old errors and brings one.
        head.rule(rule_file("BR-ACME-003", "Enforced", "`tests/new.rs`"));
        let after = head.check().unwrap();
        assert_eq!(
            new_errors(&after, &before),
            ["BR-ACME-003: `tests/new.rs` is absent at main"]
        );
        assert!(new_errors(&before, &before).is_empty());
        // Fixing an old error is never required, nor refused.
        head.put("tests/old.rs", "");
        assert_eq!(new_errors(&head.check().unwrap(), &before).len(), 1);
    }

    /// This repository runs no bridge (BR-ASDLC-023): its own registry is
    /// held to the check here, read from the working tree.
    #[test]
    fn this_repositorys_registry_is_clean() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files: Vec<(String, String)> = std::fs::read_dir(root.join(REGISTRY_DIR))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.is_file())
            .map(|p| {
                let name = p.file_name().unwrap().to_string_lossy().to_string();
                (
                    format!("{REGISTRY_DIR}/{name}"),
                    std::fs::read_to_string(&p).unwrap(),
                )
            })
            .collect();
        files.sort();
        let report = check(
            &files,
            "the working tree",
            |path: &str| match std::fs::read_to_string(root.join(path)) {
                Ok(text) => Ok(Some(text)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(format!("{path}: {e}")),
            },
        )
        .unwrap();
        assert!(report.errors.is_empty(), "{:#?}", report.errors);
        assert_eq!(report.not_located, 0, "{report:?}");
    }
}
