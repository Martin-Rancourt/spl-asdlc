//! The coordinator's verdict (`docs/reference/06-qa-report.md`, "The
//! verdict"): the file the coordinator of a release candidate commits once
//! every angle report of the candidate is merged. It judges each finding of
//! those reports once, accepted or rejected (a rejection always with its
//! reason), says what was checked between the angles, and gives `go` or
//! `hold` for the candidate. The bridge files tickets from the accepted
//! findings only, and the GO of a candidate with QA angles needs its merged
//! verdict `go`.
//!
//! Like a report, the verdict names no person and no agent: its `item` key
//! and its commit's author say who (BR-ASDLC-001).

use std::collections::BTreeSet;

use serde_json::Value;

use super::report::{self, Finding};

pub const SCHEMA: &str = "spl-asdlc/qa-verdict/1";

/// The reserved key of the verdict among the files of `release.record`: no
/// release angle may take it (its branch would be the verdict's).
pub const KEY: &str = "consolidation";

/// The path of a candidate's verdict: `release.record` rendered for the
/// candidate, with the reserved key.
pub fn path(record_template: &str, candidate: &str) -> String {
    report::report_path(record_template, candidate, KEY)
}

/// The coordinator's `go` or `hold` for the candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Call {
    Go,
    Hold,
}

impl Call {
    pub fn name(self) -> &'static str {
        match self {
            Call::Go => "go",
            Call::Hold => "hold",
        }
    }
}

/// One finding as the coordinator judged it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Judgment {
    pub angle: String,
    pub id: String,
    pub accepted: bool,
    /// For an accepted finding: whether it must be fixed before the release
    /// ships. A rejected one is never blocking.
    pub blocking: bool,
    /// Why: required for a rejected finding, optional for an accepted one.
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub candidate: String,
    /// The key of the coordination item the verdict answers.
    pub item: String,
    pub call: Call,
    pub between_angles: String,
    pub judgments: Vec<Judgment>,
}

impl Verdict {
    /// The accepted findings, in the verdict's order.
    pub fn accepted(&self) -> impl Iterator<Item = &Judgment> {
        self.judgments.iter().filter(|j| j.accepted)
    }
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Parse and validate a verdict for `candidate` against the findings of the
/// candidate's merged reports, `(angle, finding)`: every one of them judged
/// exactly once, none unknown. Every violation is listed.
pub fn validate(
    text_of_file: &str,
    candidate: &str,
    findings: &[(String, Finding)],
) -> Result<Verdict, Vec<String>> {
    let (v, judged, mut errors) = read(text_of_file, candidate)?;
    let known: BTreeSet<(&str, &str)> = findings
        .iter()
        .map(|(a, f)| (a.as_str(), f.id.as_str()))
        .collect();
    let judged: BTreeSet<(&str, &str)> = judged
        .iter()
        .map(|(a, id)| (a.as_str(), id.as_str()))
        .collect();
    for (a, id) in judged.difference(&known) {
        errors.push(format!(
            "the finding `{id}` of the angle `{a}` is in no merged report of {candidate}"
        ));
    }
    for (a, id) in known.difference(&judged) {
        errors.push(format!(
            "the finding `{id}` of the angle `{a}` is not judged: accept or reject it"
        ));
    }
    match (v, errors.is_empty()) {
        (Some(v), true) => Ok(v),
        _ => Err(errors),
    }
}

/// Parse a verdict on its own: the schema and every rule that needs no
/// report. A merged verdict is read with it.
pub fn parse(text_of_file: &str, candidate: &str) -> Result<Verdict, Vec<String>> {
    match read(text_of_file, candidate)? {
        (Some(v), _, errors) if errors.is_empty() => Ok(v),
        (_, _, errors) => Err(errors),
    }
}

/// The verdict when it is whole, the `(angle, id)` of every finding it
/// judges, and every violation found without the reports.
#[allow(clippy::type_complexity)]
fn read(
    text_of_file: &str,
    candidate: &str,
) -> Result<(Option<Verdict>, Vec<(String, String)>, Vec<String>), Vec<String>> {
    let v: Value = serde_json::from_str(text_of_file)
        .map_err(|e| vec![format!("the verdict is not JSON: {e}")])?;
    if !v.is_object() {
        return Err(vec!["the verdict is not a JSON object".into()]);
    }
    let mut errors = Vec::new();
    if v.get("schema").and_then(Value::as_str) != Some(SCHEMA) {
        errors.push(format!(
            "`schema` must be \"{SCHEMA}\", got {:?}",
            v.get("schema").and_then(Value::as_str)
        ));
    }
    match text(&v, "candidate") {
        Some(c) if c == candidate => {}
        other => errors.push(format!(
            "`candidate` must be \"{candidate}\", got {other:?}"
        )),
    }
    let item = text(&v, "item");
    if item.is_none() {
        errors.push("`item` is missing: name the key of the coordination item".into());
    }
    let call = match v.get("verdict").and_then(Value::as_str) {
        Some("go") => Some(Call::Go),
        Some("hold") => Some(Call::Hold),
        _ => {
            errors.push("`verdict` must be \"go\" or \"hold\"".into());
            None
        }
    };
    let between_angles = text(&v, "between_angles");
    if between_angles.is_none() {
        errors.push(
            "`between_angles` is empty: say what was checked across the angles' reports".into(),
        );
    }
    let mut judgments = Vec::new();
    let mut seen = BTreeSet::new();
    match v.get("findings").and_then(Value::as_array) {
        None => {
            errors.push("`findings` must be a list (empty when no angle found anything)".into())
        }
        Some(list) => {
            for (i, f) in list.iter().enumerate() {
                let at = format!("findings[{i}]");
                let angle = text(f, "angle");
                let id = text(f, "id");
                if angle.is_none() {
                    errors.push(format!("{at}.angle is missing"));
                }
                if id.is_none() {
                    errors.push(format!("{at}.id is missing"));
                }
                let accepted = match f.get("decision").and_then(Value::as_str) {
                    Some("accepted") => Some(true),
                    Some("rejected") => Some(false),
                    _ => {
                        errors.push(format!(
                            "{at}.decision must be \"accepted\" or \"rejected\""
                        ));
                        None
                    }
                };
                let reason = text(f, "reason");
                let blocking = f.get("blocking").and_then(Value::as_bool);
                match accepted {
                    Some(true) if blocking.is_none() => {
                        errors.push(format!(
                            "{at}.blocking must be true or false for an accepted finding"
                        ));
                    }
                    Some(false) if reason.is_none() => {
                        errors.push(format!("{at}: a rejected finding says its `reason`"));
                    }
                    Some(false) if blocking == Some(true) => {
                        errors.push(format!("{at}: a rejected finding is not blocking"));
                    }
                    _ => {}
                }
                if let (Some(angle), Some(id), Some(accepted)) = (angle, id, accepted) {
                    if !seen.insert((angle.clone(), id.clone())) {
                        errors.push(format!("{at}: `{id}` of `{angle}` is judged twice"));
                        continue;
                    }
                    judgments.push(Judgment {
                        angle,
                        id,
                        accepted,
                        blocking: accepted && blocking == Some(true),
                        reason,
                    });
                }
            }
        }
    }
    if call == Some(Call::Go) {
        let blocking: Vec<&str> = judgments
            .iter()
            .filter(|j| j.blocking)
            .map(|j| j.id.as_str())
            .collect();
        if !blocking.is_empty() {
            errors.push(format!(
                "a `go` verdict carries no accepted blocking finding: {} (the verdict is `hold`)",
                blocking.join(", ")
            ));
        }
    }
    let judged = judgments
        .iter()
        .map(|j| (j.angle.clone(), j.id.clone()))
        .collect();
    let verdict = match (item, call, between_angles) {
        (Some(item), Some(call), Some(between_angles)) => Some(Verdict {
            candidate: candidate.to_string(),
            item,
            call,
            between_angles,
            judgments,
        }),
        _ => None,
    };
    Ok((verdict, judged, errors))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn finding(id: &str, blocking: bool) -> Finding {
        Finding {
            id: id.into(),
            blocking,
            summary: "s".into(),
            anchor: "a".into(),
            evidence: "e".into(),
            testable: false,
            proposed_test: None,
        }
    }

    fn reports() -> Vec<(String, Finding)> {
        vec![
            ("security".into(), finding("security-1", true)),
            ("product".into(), finding("product-1", false)),
        ]
    }

    fn good() -> Value {
        json!({
            "schema": SCHEMA,
            "candidate": "acme-v1.2.0-rc.1",
            "item": "ABC-9",
            "verdict": "hold",
            "between_angles": "The login change against both reports.",
            "findings": [
                {"angle": "security", "id": "security-1", "decision": "accepted", "blocking": true},
                {"angle": "product", "id": "product-1", "decision": "rejected", "reason": "Works as designed."}
            ]
        })
    }

    fn check(v: &Value) -> Result<Verdict, Vec<String>> {
        validate(&v.to_string(), "acme-v1.2.0-rc.1", &reports())
    }

    #[test]
    fn a_complete_verdict_is_valid_and_files_only_the_accepted_findings() {
        let v = check(&good()).unwrap();
        assert_eq!(v.call, Call::Hold);
        assert_eq!(v.item, "ABC-9");
        let accepted: Vec<&str> = v.accepted().map(|j| j.id.as_str()).collect();
        assert_eq!(accepted, vec!["security-1"]);
        assert!(v.judgments[0].blocking);
        assert!(!v.judgments[1].blocking);
    }

    #[test]
    fn a_rejection_needs_its_reason() {
        let mut v = good();
        v["findings"][1]["reason"] = json!("  ");
        let e = check(&v).unwrap_err();
        assert!(e.iter().any(|e| e.contains("says its `reason`")), "{e:?}");
    }

    #[test]
    fn every_finding_is_judged_once_and_none_is_unknown() {
        let mut v = good();
        v["findings"] = json!([
            {"angle": "security", "id": "security-1", "decision": "accepted", "blocking": true},
            {"angle": "security", "id": "security-1", "decision": "accepted", "blocking": true},
            {"angle": "ux", "id": "ux-1", "decision": "rejected", "reason": "r"}
        ]);
        let e = check(&v).unwrap_err();
        assert!(e.iter().any(|e| e.contains("judged twice")), "{e:?}");
        assert!(
            e.iter()
                .any(|e| e.contains("`ux-1`") && e.contains("no merged report")),
            "{e:?}"
        );
        assert!(
            e.iter()
                .any(|e| e.contains("`product-1`") && e.contains("not judged")),
            "{e:?}"
        );
    }

    #[test]
    fn a_go_carries_no_accepted_blocking_finding() {
        let mut v = good();
        v["verdict"] = json!("go");
        let e = check(&v).unwrap_err();
        assert!(
            e.iter()
                .any(|e| e.contains("security-1") && e.contains("`go`")),
            "{e:?}"
        );
        v["findings"][0]["blocking"] = json!(false);
        assert_eq!(check(&v).unwrap().call, Call::Go);
    }

    #[test]
    fn an_accepted_finding_says_whether_it_blocks() {
        let mut v = good();
        v["findings"][0].as_object_mut().unwrap().remove("blocking");
        let e = check(&v).unwrap_err();
        assert!(
            e.iter()
                .any(|e| e.contains("blocking must be true or false")),
            "{e:?}"
        );
    }

    #[test]
    fn the_schema_candidate_item_and_between_angles_are_required() {
        let v = json!({"schema": "x", "candidate": "other", "verdict": "maybe", "findings": []});
        let e = check(&v).unwrap_err();
        for want in [
            "`schema`",
            "`candidate`",
            "`item`",
            "`verdict`",
            "`between_angles`",
        ] {
            assert!(e.iter().any(|e| e.contains(want)), "{want}: {e:?}");
        }
    }

    #[test]
    fn a_candidate_with_no_finding_takes_an_empty_list() {
        let v = json!({
            "schema": SCHEMA, "candidate": "acme-v1.2.0-rc.1", "item": "ABC-9",
            "verdict": "go", "between_angles": "Nothing to cross.", "findings": []
        });
        let ok = validate(&v.to_string(), "acme-v1.2.0-rc.1", &[]).unwrap();
        assert_eq!(ok.call, Call::Go);
    }

    #[test]
    fn the_verdict_lives_beside_the_reports() {
        assert_eq!(
            path(".spl-asdlc/qa/{candidate}", "acme-v1.2.0-rc.1"),
            ".spl-asdlc/qa/acme-v1.2.0-rc.1/consolidation.json"
        );
    }
}
