//! The angle report (`docs/spec/06-qa-report.md`): the mandatory file every
//! angle commits, validated before its PR is merged and read after to file
//! the tickets. A report that fails validation is a failed review, never a GO.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::method::vocabulary::{Severity, Verdict};

pub const SCHEMA: &str = "spl-asdlc/qa-report/1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub id: String,
    pub severity: Severity,
    pub summary: String,
    pub anchor: String,
    pub evidence: String,
    pub fixed_in_branch: bool,
    pub testable: bool,
    pub proposed_test: Option<String>,
    pub ticket: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub candidate: String,
    pub angle: String,
    pub reviewer: String,
    pub verdict: Verdict,
    pub checked: Vec<String>,
    pub findings: Vec<Finding>,
}

impl Report {
    /// Blockers the angle could not fix in its branch: each becomes a
    /// blocker ticket that blocks the release issue (rule 4).
    pub fn open_blockers(&self) -> impl Iterator<Item = &Finding> {
        self.findings
            .iter()
            .filter(|f| f.severity == Severity::Blocker && !f.fixed_in_branch)
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

/// Parse and validate a report for `(candidate, angle)`. Every violation is
/// listed; the first is the one the bridge names on the sub-item.
pub fn validate(text_of_file: &str, candidate: &str, angle: &str) -> Result<Report, Vec<String>> {
    let v: Value = serde_json::from_str(text_of_file)
        .map_err(|e| vec![format!("the report is not JSON: {e}")])?;
    if !v.is_object() {
        return Err(vec!["the report is not a JSON object".into()]);
    }
    let mut errors = Vec::new();
    match v.get("schema").and_then(Value::as_str) {
        Some(SCHEMA) => {}
        other => errors.push(format!("`schema` must be \"{SCHEMA}\", got {other:?}")),
    }
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
    let reviewer = text(&v, "reviewer").unwrap_or_else(|| {
        errors.push("`reviewer` is missing".into());
        String::new()
    });
    let verdict = match v
        .get("verdict")
        .and_then(Value::as_str)
        .and_then(Verdict::from_name)
    {
        Some(verdict) => Some(verdict),
        None => {
            errors.push("`verdict` must be one of \"go\", \"go-with-note\", \"hold\"".into());
            None
        }
    };
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
                let severity = f
                    .get("severity")
                    .and_then(Value::as_str)
                    .and_then(Severity::from_name);
                if severity.is_none() {
                    errors.push(format!(
                        "{at}.severity must be one of \"blocker\", \"should-fix\", \"nit\""
                    ));
                }
                let summary = text(f, "summary");
                if summary.is_none() {
                    errors.push(format!("{at}.summary is missing"));
                }
                let anchor = text(f, "anchor");
                if anchor.is_none() {
                    errors.push(format!("{at}.anchor is missing"));
                }
                let evidence = text(f, "evidence").unwrap_or_default();
                if evidence.is_empty() && severity != Some(Severity::Nit) {
                    errors.push(format!("{at}.evidence is missing"));
                }
                let fixed_in_branch = f.get("fixed_in_branch").and_then(Value::as_bool);
                let testable = f.get("testable").and_then(Value::as_bool);
                if fixed_in_branch.is_none() {
                    errors.push(format!("{at}.fixed_in_branch must be true or false"));
                }
                if testable.is_none() {
                    errors.push(format!("{at}.testable must be true or false"));
                }
                let proposed_test = text(f, "proposed_test");
                if testable == Some(true) && proposed_test.is_none() {
                    errors.push(format!(
                        "{at}: a testable finding names its `proposed_test`"
                    ));
                }
                if severity == Some(Severity::Blocker)
                    && fixed_in_branch == Some(true)
                    && proposed_test.is_none()
                {
                    errors.push(format!(
                        "{at}: a blocker fixed in the branch names its test in `proposed_test`"
                    ));
                }
                if let (Some(severity), Some(summary), Some(anchor)) = (severity, summary, anchor) {
                    findings.push(Finding {
                        id,
                        severity,
                        summary,
                        anchor,
                        evidence,
                        fixed_in_branch: fixed_in_branch.unwrap_or(false),
                        testable: testable.unwrap_or(false),
                        proposed_test,
                        ticket: text(f, "ticket"),
                    });
                }
            }
        }
    }
    if let Some(verdict) = verdict {
        let unfixed = findings
            .iter()
            .any(|f| f.severity == Severity::Blocker && !f.fixed_in_branch);
        let any_blocker = findings.iter().any(|f| f.severity == Severity::Blocker);
        match verdict {
            Verdict::Hold if !unfixed => errors.push(
                "a `hold` verdict requires at least one blocker that is not fixed in the branch"
                    .into(),
            ),
            Verdict::Go if any_blocker => {
                errors.push("a `go` verdict requires no blocker at all (use `go-with-note`)".into())
            }
            Verdict::Go | Verdict::GoWithNote if unfixed => {
                errors.push("a blocker not fixed in the branch makes the verdict `hold`".into())
            }
            _ => {}
        }
    }
    match (verdict, errors.is_empty()) {
        (Some(verdict), true) => Ok(Report {
            candidate: candidate.to_string(),
            angle: angle.to_string(),
            reviewer,
            verdict,
            checked,
            findings,
        }),
        _ => Err(errors),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(verdict: &str, findings: &str) -> String {
        format!(
            r#"{{"schema":"spl-asdlc/qa-report/1","candidate":"acme-v1.2.3-rc.1","angle":"security","reviewer":"reviewer-handle",
               "verdict":"{verdict}","checked":["auth routes: grep require_auth, 14/14"],"findings":[{findings}]}}"#
        )
    }

    fn finding(id: &str, severity: &str, fixed: bool, testable: bool, test: &str) -> String {
        format!(
            r#"{{"id":"{id}","severity":"{severity}","summary":"s","anchor":"src/a.rs:1","evidence":"e","fixed_in_branch":{fixed},"testable":{testable},"proposed_test":"{test}","ticket":null}}"#
        )
    }

    fn check(text: &str) -> Result<Report, Vec<String>> {
        validate(text, "acme-v1.2.3-rc.1", "security")
    }

    #[test]
    fn a_report_that_found_nothing_is_a_go_with_what_it_checked() {
        let r = check(&report("go", "")).unwrap();
        assert_eq!(r.verdict, Verdict::Go);
        assert!(r.findings.is_empty());
    }

    #[test]
    fn a_report_with_an_empty_checked_is_refused() {
        let text = report("go", "").replace(r#"["auth routes: grep require_auth, 14/14"]"#, "[]");
        let errs = check(&text).unwrap_err();
        assert!(errs[0].contains("\"found nothing\" is not \"did nothing\""));
    }

    #[test]
    fn a_hold_names_a_blocker_not_fixed_in_the_branch() {
        assert!(
            check(&report(
                "hold",
                &finding("security-1", "blocker", false, false, "")
            ))
            .is_ok()
        );
        let errs = check(&report(
            "hold",
            &finding("security-1", "should-fix", false, false, ""),
        ))
        .unwrap_err();
        assert!(
            errs.iter()
                .any(|e| e.contains("requires at least one blocker"))
        );
    }

    #[test]
    fn a_go_carries_no_blocker_at_all() {
        let fixed = finding("security-1", "blocker", true, true, "test_x");
        assert!(check(&report("go", &fixed)).is_err());
        assert!(check(&report("go-with-note", &fixed)).is_ok());
    }

    #[test]
    fn an_unfixed_blocker_cannot_ride_a_go_with_note() {
        let errs = check(&report(
            "go-with-note",
            &finding("security-1", "blocker", false, false, ""),
        ))
        .unwrap_err();
        assert!(errs.iter().any(|e| e.contains("makes the verdict `hold`")));
    }

    #[test]
    fn a_blocker_fixed_in_the_branch_names_its_test() {
        let errs = check(&report(
            "go-with-note",
            &finding("security-1", "blocker", true, false, ""),
        ))
        .unwrap_err();
        assert!(errs.iter().any(|e| e.contains("names its test")));
    }

    #[test]
    fn a_testable_finding_names_its_proposed_test() {
        let errs = check(&report(
            "go-with-note",
            &finding("security-1", "should-fix", false, true, ""),
        ))
        .unwrap_err();
        assert!(errs.iter().any(|e| e.contains("proposed_test")));
    }

    #[test]
    fn finding_ids_are_the_angle_and_a_number_used_once() {
        let two = format!(
            "{},{}",
            finding("security-1", "nit", false, false, ""),
            finding("security-1", "nit", false, false, "")
        );
        assert!(
            check(&report("go-with-note", &two))
                .unwrap_err()
                .iter()
                .any(|e| e.contains("used twice"))
        );
        let wrong = finding("design-1", "nit", false, false, "");
        assert!(check(&report("go-with-note", &wrong)).is_err());
    }

    #[test]
    fn a_report_for_another_candidate_or_angle_is_refused() {
        assert!(validate(&report("go", ""), "acme-v1.2.3-rc.2", "security").is_err());
        assert!(validate(&report("go", ""), "acme-v1.2.3-rc.1", "design").is_err());
    }

    #[test]
    fn an_unknown_verdict_severity_or_schema_is_refused() {
        assert!(check(&report("lgtm", "")).is_err());
        assert!(
            check(&report(
                "go-with-note",
                &finding("security-1", "major", false, false, "")
            ))
            .is_err()
        );
        assert!(check(&report("go", "").replace("qa-report/1", "qa-report/2")).is_err());
        assert!(check("not json").is_err());
    }

    #[test]
    fn open_blockers_are_the_unfixed_ones() {
        let both = format!(
            "{},{}",
            finding("security-1", "blocker", false, false, ""),
            finding("security-2", "blocker", true, true, "t")
        );
        let r = check(&report("hold", &both)).unwrap();
        assert_eq!(
            r.open_blockers().map(|f| f.id.as_str()).collect::<Vec<_>>(),
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
