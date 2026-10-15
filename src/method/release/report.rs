//! The angle report (`docs/reference/06-qa-report.md`): the mandatory file every
//! angle commits, validated before its PR is merged and read after to file
//! the tickets. A report that fails validation is a failed review, never a GO.
//!
//! A finding is blocking or not (BR-ASDLC-022). A report merged before that
//! (schema `spl-asdlc/qa-report/1`, findings graded `blocker`, `should-fix`
//! or `nit`) stays readable from the merged history, never from an open
//! angle PR: a `blocker` reads as blocking, any other as non-blocking, and a
//! finding it marked `fixed_in_branch` is not read, as it was filed by none.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::method::vocabulary::Verdict;

pub const SCHEMA: &str = "spl-asdlc/qa-report/2";

/// The schema of a report merged before findings were blocking or not.
pub const LEGACY_SCHEMA: &str = "spl-asdlc/qa-report/1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub id: String,
    /// Must not ship: an urgent fix ticket that holds the GO. Otherwise a
    /// ticket logged for later.
    pub blocking: bool,
    pub summary: String,
    pub anchor: String,
    pub evidence: String,
    pub testable: bool,
    pub proposed_test: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub candidate: String,
    pub angle: String,
    /// The key of the angle sub-item the report answers (`item`). `None` for
    /// a report merged before the field existed, which named its `reviewer`
    /// instead: the tracker knows who holds the sub-item, so the bridge never
    /// reads a reviewer from a report.
    pub item: Option<String>,
    pub verdict: Verdict,
    pub checked: Vec<String>,
    pub findings: Vec<Finding>,
}

impl Report {
    /// The blocking findings: each becomes an urgent fix ticket that blocks
    /// the release issue (rule 3).
    pub fn blocking(&self) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(|f| f.blocking)
    }
}

/// The path of a report in an angle branch.
pub fn report_path(record_template: &str, candidate: &str, angle: &str) -> String {
    let dir = record_template.replace("{candidate}", candidate);
    format!("{}/{angle}.json", dir.trim_end_matches('/'))
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Parse and validate the report of an open angle PR for `(candidate,
/// angle)`: the current schema only. Every violation is listed; the first is
/// the one the bridge names on the sub-item.
pub fn validate(text_of_file: &str, candidate: &str, angle: &str) -> Result<Report, Vec<String>> {
    parse(text_of_file, candidate, angle, false)
}

/// Read a report from the merged history: the current schema, or the one
/// of a report merged before findings were blocking or not.
pub fn read_merged(
    text_of_file: &str,
    candidate: &str,
    angle: &str,
) -> Result<Report, Vec<String>> {
    parse(text_of_file, candidate, angle, true)
}

fn parse(
    text_of_file: &str,
    candidate: &str,
    angle: &str,
    accept_legacy: bool,
) -> Result<Report, Vec<String>> {
    let v: Value = serde_json::from_str(text_of_file)
        .map_err(|e| vec![format!("the report is not JSON: {e}")])?;
    if !v.is_object() {
        return Err(vec!["the report is not a JSON object".into()]);
    }
    let mut errors = Vec::new();
    let legacy = match v.get("schema").and_then(Value::as_str) {
        Some(SCHEMA) => false,
        Some(LEGACY_SCHEMA) if accept_legacy => true,
        other => {
            errors.push(format!(
                "`schema` must be \"{SCHEMA}\", got {other:?} (a finding is `blocking` true or false)"
            ));
            false
        }
    };
    match text(&v, "candidate") {
        Some(c) if c == candidate => {}
        other => errors.push(format!(
            "`candidate` must be \"{candidate}\", got {other:?}"
        )),
    }
    match text(&v, "angle") {
        Some(a) if a == angle => {}
        other => errors.push(format!("`angle` must be \"{angle}\", got {other:?}")),
    }
    // `item` names the angle sub-item, never a person or an agent: a report
    // is committed to the repository (BR-ASDLC-001). A report merged before
    // `item` existed carries `reviewer` instead and stays readable.
    let item = text(&v, "item");
    if item.is_none() && text(&v, "reviewer").is_none() {
        errors.push("`item` is missing: name the key of your angle sub-item".into());
    }
    let verdict = match v.get("verdict").and_then(Value::as_str) {
        Some("go-with-note") if legacy => Some(Verdict::Go),
        Some(name) => Verdict::from_name(name),
        None => None,
    };
    if verdict.is_none() {
        errors.push("`verdict` must be one of \"go\", \"hold\"".into());
    }
    let checked: Vec<String> = match v.get("checked").and_then(Value::as_array) {
        Some(list) => list
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        None => Vec::new(),
    };
    if checked.is_empty() {
        errors.push(
            "`checked` is empty: \"found nothing\" is not \"did nothing\" — name what was examined"
                .into(),
        );
    }
    let mut findings = Vec::new();
    let mut ids = BTreeSet::new();
    match v.get("findings").and_then(Value::as_array) {
        None => errors.push("`findings` must be a list (empty when nothing was found)".into()),
        Some(list) => {
            for (i, f) in list.iter().enumerate() {
                let at = format!("findings[{i}]");
                let id = text(f, "id").unwrap_or_default();
                let well_formed = id
                    .strip_prefix(&format!("{angle}-"))
                    .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
                if !well_formed {
                    errors.push(format!("{at}.id \"{id}\" must be `{angle}-<n>`"));
                } else if !ids.insert(id.clone()) {
                    errors.push(format!("{at}.id \"{id}\" is used twice"));
                }
                let (blocking, fixed) = if legacy {
                    legacy_grade(f, &at, &mut errors)
                } else {
                    (grade(f, &at, &mut errors), false)
                };
                let summary = text(f, "summary");
                if summary.is_none() {
                    errors.push(format!("{at}.summary is missing"));
                }
                let anchor = text(f, "anchor");
                if anchor.is_none() {
                    errors.push(format!("{at}.anchor is missing"));
                }
                // What was observed, where and in what context: the
                // finding's ticket carries these words alone (BR-ASDLC-013).
                // A merged nit of the older schema could leave it empty.
                let evidence = text(f, "evidence").unwrap_or_default();
                if evidence.is_empty() && !(legacy && blocking == Some(false)) {
                    errors.push(format!("{at}.evidence is missing"));
                }
                let testable = f.get("testable").and_then(Value::as_bool);
                if testable.is_none() {
                    errors.push(format!("{at}.testable must be true or false"));
                }
                let proposed_test = text(f, "proposed_test");
                if testable == Some(true) && proposed_test.is_none() {
                    errors.push(format!(
                        "{at}: a testable finding names its `proposed_test`"
                    ));
                }
                if let (Some(blocking), Some(summary), Some(anchor), false) =
                    (blocking, summary, anchor, fixed)
                {
                    findings.push(Finding {
                        id,
                        blocking,
                        summary,
                        anchor,
                        evidence,
                        testable: testable.unwrap_or(false),
                        proposed_test,
                    });
                }
            }
        }
    }
    // The verdict follows the findings; a merged report of the older schema
    // was judged by the bridge that merged it.
    if let (Some(verdict), false) = (verdict, legacy) {
        let any_blocking = findings.iter().any(|f| f.blocking);
        match verdict {
            Verdict::Hold if !any_blocking => {
                errors.push("a `hold` verdict requires at least one blocking finding".into())
            }
            Verdict::Go if any_blocking => {
                errors.push("a blocking finding makes the verdict `hold`".into())
            }
            _ => {}
        }
    }
    match (verdict, errors.is_empty()) {
        (Some(verdict), true) => Ok(Report {
            candidate: candidate.to_string(),
            angle: angle.to_string(),
            item,
            verdict,
            checked,
            findings,
        }),
        _ => Err(errors),
    }
}

/// `blocking` of a finding of the current schema, which knows no severity
/// and no fix in the angle branch (angles fix nothing).
fn grade(f: &Value, at: &str, errors: &mut Vec<String>) -> Option<bool> {
    for gone in ["severity", "fixed_in_branch"] {
        if f.get(gone).is_some() {
            errors.push(format!(
                "{at}.{gone} is not part of the report: a finding is `blocking` true or false, and an angle fixes nothing"
            ));
        }
    }
    let blocking = f.get("blocking").and_then(Value::as_bool);
    if blocking.is_none() {
        errors.push(format!("{at}.blocking must be true or false"));
    }
    blocking
}

/// `(blocking, fixed in its branch)` of a finding of the older schema: a
/// `blocker` is blocking, a `should-fix` or a `nit` is not.
fn legacy_grade(f: &Value, at: &str, errors: &mut Vec<String>) -> (Option<bool>, bool) {
    let blocking = match f.get("severity").and_then(Value::as_str) {
        Some("blocker") => Some(true),
        Some("should-fix" | "nit") => Some(false),
        _ => {
            errors.push(format!(
                "{at}.severity must be one of \"blocker\", \"should-fix\", \"nit\""
            ));
            None
        }
    };
    let fixed = f
        .get("fixed_in_branch")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    (blocking, fixed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(verdict: &str, findings: &str) -> String {
        format!(
            r#"{{"schema":"spl-asdlc/qa-report/2","candidate":"acme-v1.2.3-rc.1","angle":"security","item":"ABC-123",
               "verdict":"{verdict}","checked":["auth routes: grep require_auth, 14/14"],"findings":[{findings}]}}"#
        )
    }

    fn finding(id: &str, blocking: bool, testable: bool, test: &str) -> String {
        format!(
            r#"{{"id":"{id}","blocking":{blocking},"summary":"s","anchor":"src/a.rs:1","evidence":"e","testable":{testable},"proposed_test":"{test}"}}"#
        )
    }

    fn check(text: &str) -> Result<Report, Vec<String>> {
        validate(text, "acme-v1.2.3-rc.1", "security")
    }

    /// A report merged before findings were blocking or not.
    fn legacy(verdict: &str, findings: &str) -> String {
        format!(
            r#"{{"schema":"spl-asdlc/qa-report/1","candidate":"acme-v1.2.3-rc.1","angle":"security","item":"ABC-123",
               "verdict":"{verdict}","checked":["the whole project"],"findings":[{findings}]}}"#
        )
    }

    fn legacy_finding(id: &str, severity: &str, fixed: bool, evidence: &str) -> String {
        format!(
            r#"{{"id":"{id}","severity":"{severity}","summary":"s","anchor":"src/a.rs:1","evidence":"{evidence}","fixed_in_branch":{fixed},"testable":false,"proposed_test":"","ticket":null}}"#
        )
    }

    #[test]
    fn a_report_that_found_nothing_is_a_go_with_what_it_checked() {
        let r = check(&report("go", "")).unwrap();
        assert_eq!(r.verdict, Verdict::Go);
        assert!(r.findings.is_empty());
    }

    #[test]
    fn a_report_names_its_angle_sub_item() {
        let r = check(&report("go", "")).unwrap();
        assert_eq!(r.item.as_deref(), Some("ABC-123"));
    }

    #[test]
    fn a_report_merged_with_the_old_reviewer_field_still_parses() {
        let old =
            report("go", "").replace(r#""item":"ABC-123""#, r#""reviewer":"reviewer-handle""#);
        let r = check(&old).unwrap();
        assert_eq!(r.item, None);
        assert_eq!(r.verdict, Verdict::Go);
    }

    #[test]
    fn a_report_that_names_neither_its_item_nor_a_reviewer_is_refused() {
        let none = report("go", "").replace(r#""item":"ABC-123","#, "");
        let errs = check(&none).unwrap_err();
        assert!(
            errs.iter().any(|e| e.contains("`item` is missing")),
            "{errs:?}"
        );
    }

    #[test]
    fn a_report_with_an_empty_checked_is_refused() {
        let text = report("go", "").replace(r#"["auth routes: grep require_auth, 14/14"]"#, "[]");
        let errs = check(&text).unwrap_err();
        assert!(errs[0].contains("\"found nothing\" is not \"did nothing\""));
    }

    #[test]
    fn a_finding_is_blocking_or_not_and_the_verdict_follows() {
        let r = check(&report("hold", &finding("security-1", true, false, ""))).unwrap();
        assert!(r.findings[0].blocking);
        let r = check(&report("go", &finding("security-1", false, false, ""))).unwrap();
        assert!(!r.findings[0].blocking);
        let errs = check(&report("hold", &finding("security-1", false, false, ""))).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| e.contains("requires at least one blocking finding")),
            "{errs:?}"
        );
        let errs = check(&report("go", &finding("security-1", true, false, ""))).unwrap_err();
        assert!(
            errs.iter().any(|e| e.contains("makes the verdict `hold`")),
            "{errs:?}"
        );
        let unset = finding("security-1", false, false, "").replace(r#""blocking":false,"#, "");
        let errs = check(&report("go", &unset)).unwrap_err();
        assert!(
            errs.iter()
                .any(|e| e.contains("blocking must be true or false")),
            "{errs:?}"
        );
    }

    #[test]
    fn go_with_note_is_no_verdict_of_the_current_schema() {
        let errs = check(&report(
            "go-with-note",
            &finding("security-1", false, false, ""),
        ))
        .unwrap_err();
        assert!(errs.iter().any(|e| e.contains("`verdict` must be one of")));
    }

    #[test]
    fn a_severity_or_a_fix_in_the_branch_is_refused_in_an_open_angle_pr() {
        for gone in [r#""severity":"blocker","#, r#""fixed_in_branch":false,"#] {
            let f = finding("security-1", true, false, "")
                .replace(r#""blocking":true,"#, &format!(r#""blocking":true,{gone}"#));
            let errs = check(&report("hold", &f)).unwrap_err();
            assert!(
                errs.iter().any(|e| e.contains("an angle fixes nothing")),
                "{errs:?}"
            );
        }
        // The older schema is history: never accepted from an open angle PR.
        let old = legacy("hold", &legacy_finding("security-1", "blocker", false, "e"));
        let errs = check(&old).unwrap_err();
        assert!(errs[0].contains("qa-report/2"), "{errs:?}");
    }

    #[test]
    fn a_merged_report_of_the_older_schema_is_still_read() {
        let findings = [
            legacy_finding("security-1", "blocker", false, "e"),
            legacy_finding("security-2", "should-fix", false, "e"),
            legacy_finding("security-3", "nit", false, ""),
            legacy_finding("security-4", "blocker", true, "e"),
        ]
        .join(",");
        let r = read_merged(&legacy("hold", &findings), "acme-v1.2.3-rc.1", "security").unwrap();
        let read: Vec<(&str, bool)> = r
            .findings
            .iter()
            .map(|f| (f.id.as_str(), f.blocking))
            .collect();
        assert_eq!(
            read,
            vec![
                ("security-1", true),
                ("security-2", false),
                ("security-3", false)
            ],
            "a blocker is blocking, the others not; one fixed in its branch was filed by none"
        );
        let note = legacy(
            "go-with-note",
            &legacy_finding("security-1", "nit", false, "e"),
        );
        assert_eq!(
            read_merged(&note, "acme-v1.2.3-rc.1", "security")
                .unwrap()
                .verdict,
            Verdict::Go
        );
        let unknown = legacy("go", &legacy_finding("security-1", "major", false, "e"));
        assert!(read_merged(&unknown, "acme-v1.2.3-rc.1", "security").is_err());
        // The current schema reads the same from the merged history.
        assert!(read_merged(&report("go", ""), "acme-v1.2.3-rc.1", "security").is_ok());
    }

    #[test]
    fn every_finding_says_what_was_observed_where_and_in_what_context() {
        let f = finding("security-1", false, false, "").replace(r#""evidence":"e","#, "");
        let errs = check(&report("go", &f)).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("evidence is missing")));
    }

    #[test]
    fn a_testable_finding_names_its_proposed_test() {
        let errs = check(&report("go", &finding("security-1", false, true, ""))).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("proposed_test")));
    }

    #[test]
    fn finding_ids_are_the_angle_and_a_number_used_once() {
        let two = format!(
            "{},{}",
            finding("security-1", false, false, ""),
            finding("security-1", false, false, "")
        );
        assert!(
            check(&report("go", &two))
                .unwrap_err()
                .iter()
                .any(|e| e.contains("used twice"))
        );
        let wrong = finding("design-1", false, false, "");
        assert!(check(&report("go", &wrong)).is_err());
    }

    #[test]
    fn a_report_for_another_candidate_or_angle_is_refused() {
        assert!(validate(&report("go", ""), "acme-v1.2.3-rc.2", "security").is_err());
        assert!(validate(&report("go", ""), "acme-v1.2.3-rc.1", "design").is_err());
    }

    #[test]
    fn an_unknown_verdict_or_schema_is_refused() {
        assert!(check(&report("lgtm", "")).is_err());
        assert!(check(&report("go", "").replace("qa-report/2", "qa-report/3")).is_err());
        assert!(check("not json").is_err());
    }

    #[test]
    fn the_blocking_findings_are_the_ones_marked_so() {
        let both = format!(
            "{},{}",
            finding("security-1", true, false, ""),
            finding("security-2", false, true, "t")
        );
        let r = check(&report("hold", &both)).unwrap();
        assert_eq!(
            r.blocking().map(|f| f.id.as_str()).collect::<Vec<_>>(),
            vec!["security-1"]
        );
    }

    #[test]
    fn the_report_path_follows_the_record_template() {
        assert_eq!(
            report_path(".spl-asdlc/qa/{candidate}/", "acme-v1.2.3-rc.1", "security"),
            ".spl-asdlc/qa/acme-v1.2.3-rc.1/security.json"
        );
    }
}
